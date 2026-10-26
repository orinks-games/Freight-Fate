//! The editable world source and the indexed tree the game loads.
//!
//! The source is a tree of per-state shards, so a one-leg edit is a small
//! reviewable diff instead of a 60 MB blob:
//!
//! ```text
//! data/world_source/
//!   meta.json         # every top-level key that is not cities or legs
//!   cities.json       # {"cities": {...}}
//!   legs/TX.json      # {"legs": [...]} for legs starting in Texas
//! ```
//!
//! [`load_world`] returns the merged world as one object. A leg is filed under the state of its `from` city; shard
//! files and the leg order inside them are sorted, so the same data always
//! produces the same bytes.

pub mod index;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::pyjson::{self, Object, Options, Value};
use crate::util::{self, Result};

/// Legs whose `from` city has no state (or an unknown city) land here
/// rather than failing the save.
const UNSHARDED: &str = "_unsharded";

pub fn source_root() -> PathBuf {
    util::repo_root().join("data").join("world_source")
}

/// The repository's JSON text: indent 2, escaped non-ASCII, a final newline.
pub fn dumps(value: &Value) -> String {
    pyjson::dumps(value, &Options::indent(2)) + "\n"
}

/// Which shard a leg belongs in: the state of the city it starts from.
pub fn shard_key(leg: &Value, cities: Option<&Object>) -> String {
    let state = leg
        .get("from")
        .and_then(Value::as_str)
        .and_then(|from| cities.and_then(|c| c.get(from)))
        .and_then(|city| city.get("state"))
        .map(Value::py_str)
        .unwrap_or_default();
    let state = util::py_strip(&state);
    if state.is_empty() {
        UNSHARDED.to_string()
    } else {
        state.to_string()
    }
}

/// Legs grouped by shard, each group sorted by (from, to); shard order is
/// first appearance.
pub fn shard_legs<'a>(
    legs: impl Iterator<Item = &'a Value>,
    cities: Option<&Object>,
) -> Vec<(String, Vec<Value>)> {
    let mut order: Vec<String> = Vec::new();
    let mut groups: BTreeMap<String, Vec<Value>> = BTreeMap::new();
    for leg in legs {
        let key = shard_key(leg, cities);
        if !groups.contains_key(&key) {
            order.push(key.clone());
        }
        groups.entry(key).or_default().push(leg.clone());
    }
    let end = |leg: &Value, k: &str| leg.get(k).map(Value::py_str).unwrap_or_default();
    order
        .into_iter()
        .map(|key| {
            let mut legs = groups.remove(&key).unwrap_or_default();
            legs.sort_by(|a, b| {
                (end(a, "from"), end(a, "to")).cmp(&(end(b, "from"), end(b, "to")))
            });
            (key, legs)
        })
        .collect()
}

/// The merged world source, shaped like the old single world.json.
pub fn load_world(root: &Path) -> Result<Object> {
    let Value::Object(mut data) = pyjson::load_path(&root.join("meta.json"))? else {
        return Err("world_source/meta.json is not an object".into());
    };
    let cities = pyjson::load_path(&root.join("cities.json"))?
        .get("cities")
        .cloned()
        .unwrap_or(Value::Object(Object::new()));
    data.insert("cities", cities);
    let mut legs = Vec::new();
    for shard in util::glob_suffix(&root.join("legs"), ".json") {
        if let Some(Value::Array(rows)) = pyjson::load_path(&shard)?.get("legs").cloned() {
            legs.extend(rows);
        }
    }
    data.insert("legs", Value::Array(legs));
    Ok(data)
}

/// The exact text every source file should hold, keyed by path. The test
/// writes a world with it; nothing else writes the source now.
#[cfg(test)]
fn build_source_files(data: &Object, root: &Path) -> Vec<(PathBuf, String)> {
    let cities = data.get("cities").and_then(Value::as_object);
    let mut meta = Object::new();
    for (k, v) in data.iter() {
        if k != "cities" && k != "legs" {
            meta.insert(k.clone(), v.clone());
        }
    }
    let mut files = vec![
        (root.join("meta.json"), dumps(&Value::Object(meta))),
        (
            root.join("cities.json"),
            dumps(&Value::Object(crate::pyobj! {
                "cities" => data.get("cities").cloned().unwrap_or(Value::Object(Object::new())),
            })),
        ),
    ];
    let legs = data
        .get("legs")
        .and_then(Value::as_array)
        .into_iter()
        .flatten();
    for (state, legs) in shard_legs(legs, cities) {
        files.push((
            root.join("legs").join(format!("{state}.json")),
            dumps(&Value::Object(crate::pyobj! { "legs" => legs })),
        ));
    }
    files
}

/// Write the world source back out as shards; return how many files
/// changed. Only changed files are touched, and a shard with no legs left
/// is removed so a renamed state cannot linger.
#[cfg(test)]
fn save_world(data: &Object, root: &Path) -> Result<usize> {
    let files = build_source_files(data, root);
    std::fs::create_dir_all(root.join("legs")).map_err(|e| e.to_string())?;
    let mut written = 0;
    for (path, text) in &files {
        if util::read_text_lf(path).as_ref() != Some(text) {
            util::write_text(path, text)?;
            written += 1;
        }
    }
    for stale in util::glob_suffix(&root.join("legs"), ".json") {
        if !files.iter().any(|(p, _)| *p == stale) {
            std::fs::remove_file(&stale).map_err(|e| e.to_string())?;
            written += 1;
        }
    }
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legs_shard_by_starting_state_and_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let data = pyjson::loads(
            r#"{"version": 1, "cities": {"A": {"state": "TX"}, "B": {"state": "OK"}},
                "legs": [{"from": "A", "to": "B"}, {"from": "B", "to": "A"}, {"from": "Z", "to": "A"}]}"#,
        )
        .unwrap();
        let data = data.as_object().unwrap();
        assert_eq!(save_world(data, dir.path()).unwrap(), 5);
        assert!(dir.path().join("legs").join("_unsharded.json").is_file());
        let back = load_world(dir.path()).unwrap();
        assert_eq!(
            back.get("legs").and_then(Value::as_array).map(Vec::len),
            Some(3)
        );
        assert_eq!(save_world(&back, dir.path()).unwrap(), 0);
    }
}
