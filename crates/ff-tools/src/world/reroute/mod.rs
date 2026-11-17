//! `reroute-leg` and `reroute-enrich`: put a leg back on the road it is named
//! for, then rebuild what rides on that road.
//!
//! ```text
//! cargo tools reroute-leg --leg kenosha_wi_us:chicago_il_us           # dry run
//! cargo tools reroute-leg --leg kenosha_wi_us:chicago_il_us --write
//! cargo tools reroute-enrich --leg kenosha_wi_us:chicago_il_us \
//!     --pbf us.osm.pbf --extract-date 2026-11-03 --snapshot NAME --write
//! cargo tools reroute-leg --check        # legs rerouted but not finished
//! ```
//!
//! A port of the deleted `tools/reroute_leg.py` and the core of
//! `tools/reroute_enrich.py`. The router is Valhalla's truck costing with the
//! leg's curated `route_via` pins, set so tolls cost a route nothing (see
//! [`valhalla`]). See [`leg`] for what the first step writes and carries
//! across, and [`enrich`] for the layers the second rebuilds and the rule
//! that refuses to call a leg finished when a layer came back empty or thin.

pub mod carry;
pub mod enrich;
pub mod grades;
pub mod leg;
pub mod roads;
pub mod shape;
pub mod states;
pub mod valhalla;

use std::path::{Path, PathBuf};

use crate::pyjson::{Object, Value};
use crate::util::{self, Result};

pub use enrich::main as enrich_main;
pub use leg::main as leg_main;

pub fn leg_id(leg: &Value) -> String {
    let end = |k| leg.get(k).map(Value::py_str).unwrap_or_default();
    format!("{}:{}", end("from"), end("to"))
}

pub fn geometry_dir() -> PathBuf {
    util::repo_root().join("data/world_data/us/geometry")
}

pub fn gameplay_dir() -> PathBuf {
    util::repo_root().join("data/world_data/us/gameplay")
}

/// The archive shard a leg lives in: the state of its `from` city.
pub fn shard_path(world: &Object, leg: &Value) -> Option<PathBuf> {
    let from = leg.get("from")?.as_str()?;
    let state = world.get("cities")?.get(from)?.get("state")?.as_str()?;
    Some(geometry_dir().join(format!("{}.jsonl", state.to_lowercase())))
}

/// One leg's archive record, as written.
pub fn archive_record(path: &Path, id: &str) -> Result<Option<serde_json::Value>> {
    let Some(text) = util::read_text_lf(path) else {
        return Ok(None);
    };
    for line in text.lines() {
        if line.trim().is_empty() || line.starts_with("{\"meta\"") {
            continue;
        }
        let rec: serde_json::Value =
            serde_json::from_str(line).map_err(|e| format!("{}: {e}", path.display()))?;
        if rec.get("leg").and_then(|l| l.as_str()) == Some(id) {
            return Ok(Some(rec));
        }
    }
    Ok(None)
}

/// Replace (or add) one leg's record in its archive shard. Python wrote
/// these lines with `json.dumps(sort_keys=True)`, which this matches.
pub fn write_archive_record(path: &Path, id: &str, record: &serde_json::Value) -> Result<()> {
    let text = util::read_text_lf(path).unwrap_or_default();
    let line = python_line(record);
    let mut out = Vec::new();
    let mut replaced = false;
    for existing in text.lines() {
        if existing.trim().is_empty() {
            continue;
        }
        if !existing.starts_with("{\"meta\"") {
            let rec: serde_json::Value =
                serde_json::from_str(existing).map_err(|e| format!("{}: {e}", path.display()))?;
            if rec.get("leg").and_then(|l| l.as_str()) == Some(id) {
                out.push(line.clone());
                replaced = true;
                continue;
            }
        }
        out.push(existing.to_string());
    }
    if !replaced {
        out.push(line);
    }
    util::write_text(path, &(out.join("\n") + "\n"))
}

/// `json.dumps(value, sort_keys=True)`: `", "` and `": "` separators.
pub fn python_line(value: &serde_json::Value) -> String {
    let parsed = crate::pyjson::loads(&value.to_string()).unwrap_or(Value::Null);
    crate::pyjson::dumps(&parsed, &crate::pyjson::Options::compact().sorted())
}

/// The rows of a `leg`-keyed gameplay shard, its meta line kept verbatim.
pub fn read_shard(path: &Path) -> Result<(Option<String>, Vec<Value>)> {
    let Some(text) = util::read_text_lf(path) else {
        return Ok((None, Vec::new()));
    };
    let mut meta = None;
    let mut rows = Vec::new();
    for line in text.lines() {
        if line.trim().is_empty() {
            continue;
        }
        if line.starts_with("{\"meta\"") {
            meta = Some(line.to_string());
            continue;
        }
        rows.push(crate::pyjson::loads(line).map_err(|e| format!("{}: {e}", path.display()))?);
    }
    Ok((meta, rows))
}

/// Write a gameplay shard: its meta (with `data_version` re-hashed over the
/// rows, as every builder writes it) and the rows grouped by leg in sorted
/// order.
pub fn write_shard(path: &Path, meta_line: Option<&str>, rows: &[Value]) -> Result<()> {
    use sha2::{Digest, Sha256};
    let mut rows: Vec<&Value> = rows.iter().collect();
    let leg_of = |r: &Value| r.get("leg").map(Value::py_str).unwrap_or_default();
    rows.sort_by_key(|r| leg_of(r));
    let opts = crate::pyjson::Options::compact().sorted();
    let payload: Vec<String> = rows
        .iter()
        .map(|r| crate::pyjson::dumps(r, &opts))
        .collect();
    let payload = payload.join("\n");
    let version = format!(
        "sha256:{}",
        &hex::encode(Sha256::digest(payload.as_bytes()))[..12]
    );
    let mut meta = match meta_line.map(crate::pyjson::loads) {
        Some(Ok(v)) => v,
        _ => Value::Object(crate::pyobj! { "meta" => Object::new() }),
    };
    if let Some(m) = meta
        .as_object_mut()
        .and_then(|o| o.get_mut("meta"))
        .and_then(Value::as_object_mut)
    {
        m.insert("data_version", Value::Str(version));
    }
    let head = crate::pyjson::dumps(&meta, &opts);
    let text = if payload.is_empty() {
        head + "\n"
    } else {
        format!("{head}\n{payload}\n")
    };
    util::write_text(path, &text)
}

/// Replace one leg's rows in a gameplay shard (an empty `rows` drops them).
/// A leg's rows keep the order given; the other legs keep theirs.
pub fn replace_leg_rows(path: &Path, id: &str, new_rows: Vec<Value>) -> Result<usize> {
    let (meta, rows) = read_shard(path)?;
    let leg_of = |r: &Value| r.get("leg").map(Value::py_str).unwrap_or_default();
    let before = rows.iter().filter(|r| leg_of(r) == id).count();
    let mut kept: Vec<Value> = rows.into_iter().filter(|r| leg_of(r) != id).collect();
    kept.extend(new_rows);
    write_shard(path, meta.as_deref(), &kept)?;
    Ok(before)
}

/// A city's `(lat, lon)`.
pub fn city_point(world: &Object, slug: &str) -> Option<(f64, f64)> {
    let city = world.get("cities")?.get(slug)?;
    Some((city.get("lat")?.as_f64()?, city.get("lon")?.as_f64()?))
}

/// The curated `route_via` pins, in source order, as `(lat, lon)`.
pub fn route_via(leg: &Value) -> Vec<(f64, f64)> {
    leg.get("route_via")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|p| Some((p.get("lat")?.as_f64()?, p.get("lon")?.as_f64()?)))
        .collect()
}

/// The legs named on the command line, `;`-separated or repeated.
pub fn wanted_legs(values: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for v in values {
        for part in v.split(';').map(str::trim).filter(|p| !p.is_empty()) {
            if !out.iter().any(|o| o == part) {
                out.push(part.to_string());
            }
        }
    }
    out
}
