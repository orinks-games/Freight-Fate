//! `index-world`: rebuild the indexed `data/world_data/` tree the game loads
//! from the editable `data/world_source/` tree the build tools edit.
//!
//! ```text
//! cargo tools index-world            # rewrite world_data/ from the source
//! cargo tools index-world --check    # verify in sync (exit 1 if not)
//! ```
//!
//! The split follows `world_data/index.json`: cities go to each country's
//! `cities.json`, legs to per-state shards under the country's `legs_dir`
//! (`legs/TX.json`), the source's top-level `geo` lookup to
//! `world_data/geo.json`, and the top-level `route_stop_data` is merged into
//! each country's `metadata.json`. A city is assigned to a country by its
//! `country` field; when the index lists exactly one country every city and
//! leg belongs to it.

use std::collections::HashMap;
use std::path::PathBuf;

use super::{dumps, load_world, shard_legs, source_root};
use crate::cli::{Args, Spec};
use crate::pyjson::{self, Object, Value};
use crate::util::{self, Result};

fn world_data() -> PathBuf {
    util::repo_root().join("data").join("world_data")
}

fn country_of(city: Option<&Value>, single: Option<&str>) -> Result<String> {
    let code = city
        .and_then(|c| c.get("country"))
        .map(Value::py_str)
        .unwrap_or_default();
    let code = util::py_strip(&code);
    if !code.is_empty() {
        return Ok(code.to_string());
    }
    single.map(str::to_string).ok_or_else(|| {
        "world.json cities need a 'country' field when world_data/index.json lists more than \
         one country."
            .to_string()
    })
}

fn field<'a>(value: &'a Value, key: &str, default: &'a str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or(default)
}

/// The exact text every world_data file should contain, keyed by path.
fn build_country_files(data: &Object, index: &Value) -> Result<Vec<(PathBuf, String)>> {
    let countries = index
        .get("countries")
        .and_then(Value::as_array)
        .filter(|c| !c.is_empty())
        .ok_or("world_data/index.json has no 'countries'")?;
    let single = (countries.len() == 1).then(|| field(&countries[0], "code", ""));
    let cities = data.get("cities").and_then(Value::as_object);
    let mut by_country: HashMap<String, (Object, Vec<Value>)> = countries
        .iter()
        .map(|c| {
            (
                field(c, "code", "").to_string(),
                (Object::new(), Vec::new()),
            )
        })
        .collect();
    let unknown =
        |code: &str| format!("a city names country {code:?}, which index.json does not list");
    for (name, city) in cities.into_iter().flat_map(Object::iter) {
        let code = country_of(Some(city), single)?;
        by_country
            .get_mut(&code)
            .ok_or_else(|| unknown(&code))?
            .0
            .insert(name.clone(), city.clone());
    }
    for leg in data
        .get("legs")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let from = field(leg, "from", "");
        let to = field(leg, "to", "");
        let code = country_of(cities.and_then(|c| c.get(from)), single)?;
        if country_of(cities.and_then(|c| c.get(to)), single)? != code {
            return Err(format!("Cross-country leg {from}->{to} is not supported."));
        }
        // Complete leg records pass through, optional corridor fields too.
        by_country
            .get_mut(&code)
            .ok_or_else(|| unknown(&code))?
            .1
            .push(leg.clone());
    }

    let root = world_data();
    let mut files = Vec::new();
    if let Some(geo) = data.get("geo") {
        files.push((root.join("geo.json"), dumps(geo)));
    }
    for country in countries {
        let code = field(country, "code", "");
        let dir = root.join(field(country, "path", ""));
        let (country_cities, legs) = by_country.remove(code).unwrap_or_default();
        files.push((
            dir.join(field(country, "cities", "cities.json")),
            dumps(&Value::Object(crate::pyobj! { "cities" => country_cities })),
        ));
        match country
            .get("legs_dir")
            .and_then(Value::as_str)
            .filter(|d| !d.is_empty())
        {
            Some(legs_dir) => {
                for (state, legs) in shard_legs(legs.iter(), cities) {
                    files.push((
                        dir.join(legs_dir).join(format!("{state}.json")),
                        dumps(&Value::Object(crate::pyobj! { "legs" => legs })),
                    ));
                }
            }
            None => files.push((
                dir.join(field(country, "legs", "legs.json")),
                dumps(&Value::Object(crate::pyobj! { "legs" => legs })),
            )),
        }
        // Merge route_stop_data into the country's metadata, keeping any
        // other keys already on disk.
        let metadata_path = dir.join("metadata.json");
        let mut metadata = if metadata_path.exists() {
            pyjson::load_path(&metadata_path)?
        } else {
            Value::Object(Object::new())
        };
        if let (Some(stops), Some(meta)) = (data.get("route_stop_data"), metadata.as_object_mut()) {
            meta.insert("route_stop_data", stops.clone());
        }
        files.push((metadata_path, dumps(&metadata)));
    }
    Ok(files)
}

/// Leg shards on disk the source no longer produces. The loader globs the
/// shard folder, so a shard left behind after its last leg moved away would
/// keep feeding the game a leg that no longer exists.
fn stale_shards(index: &Value, files: &[(PathBuf, String)]) -> Vec<PathBuf> {
    let mut stale = Vec::new();
    for country in index
        .get("countries")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let Some(legs_dir) = country
            .get("legs_dir")
            .and_then(Value::as_str)
            .filter(|d| !d.is_empty())
        else {
            continue;
        };
        let dir = world_data().join(field(country, "path", "")).join(legs_dir);
        for path in util::glob_suffix(&dir, ".json") {
            if !files.iter().any(|(p, _)| *p == path) {
                stale.push(path);
            }
        }
    }
    util::sort_paths(&mut stale);
    stale
}

/// Each file's path and the exact text it should hold.
type Files = Vec<(PathBuf, String)>;

/// The files `index-world` would write, and the shards it would delete.
pub fn plan() -> Result<(Files, Vec<PathBuf>)> {
    let data = load_world(&source_root())?;
    let index = pyjson::load_path(&world_data().join("index.json"))?;
    let files = build_country_files(&data, &index)?;
    let stale = stale_shards(&index, &files);
    Ok((files, stale))
}

pub fn main(argv: &[String]) -> Result<i32> {
    let args = Args::parse(
        argv,
        &Spec {
            values: &[],
            flags: &["check"],
        },
    )
    .map_err(|e| format!("{e}\nusage: index-world [--check]"))?;
    let root = util::repo_root();
    let (files, stale) = plan()?;
    let changed = |path: &PathBuf, text: &String| util::read_text_lf(path).as_ref() != Some(text);
    if args.flag("check") {
        let drifted: Vec<&PathBuf> = files
            .iter()
            .filter(|(p, t)| changed(p, t))
            .map(|(p, _)| p)
            .collect();
        if drifted.is_empty() && stale.is_empty() {
            println!("world_data/ is in sync with world_source/.");
            return Ok(0);
        }
        println!("world_data/ is out of sync with world_source/; run `cargo tools index-world`:");
        for path in drifted.into_iter().chain(stale.iter()) {
            println!("  {}", util::rel(path, &root));
        }
        return Ok(1);
    }
    let mut written = 0;
    for (path, text) in &files {
        if changed(path, text) {
            util::write_text(path, text)?;
            written += 1;
        }
    }
    for path in &stale {
        std::fs::remove_file(path).map_err(|e| e.to_string())?;
        written += 1;
    }
    println!("Wrote {written} world_data file(s) from world_source/.");
    Ok(0)
}
