//! Which country's folder under `world_data/` a leg's road and gameplay
//! layers live in.
//!
//! `world_data/index.json` lists the countries and the folder each one is
//! indexed into (`us`, `ca`). A leg belongs to the country of the city it
//! starts from, the same rule that files it in its country's `legs/` shard,
//! so its archived road goes in `<country>/geometry/<state>.jsonl` and its
//! curves, ramps and postings in `<country>/gameplay/`. Readers take every
//! country's folder: a leg id is unique across the map.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::osm::geo::{self, LegLine};
use crate::pyjson::{self, Object, Value};
use crate::util::{self, Result};

/// The folder a country with no `index.json` row is filed in.
const DEFAULT_PATH: &str = "us";

pub fn world_data_dir() -> PathBuf {
    util::repo_root().join("data/world_data")
}

/// `(code, folder)` for each country in `index.json`, in its order. A tree
/// with no index (a test fixture) is the United States alone.
pub fn countries_in(world_data: &Path) -> Vec<(String, String)> {
    let parsed = pyjson::load_path(&world_data.join("index.json")).ok();
    let rows: Vec<(String, String)> = parsed
        .as_ref()
        .and_then(|i| i.get("countries"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|c| {
            Some((
                c.get("code")?.as_str()?.to_string(),
                c.get("path")?.as_str()?.to_string(),
            ))
        })
        .collect();
    if rows.is_empty() {
        vec![("US".into(), DEFAULT_PATH.into())]
    } else {
        rows
    }
}

pub fn countries() -> Vec<(String, String)> {
    countries_in(&world_data_dir())
}

/// The folder for a country code, case-insensitively; the US folder when the
/// code is unknown or empty.
pub fn folder_for(code: &str, countries: &[(String, String)]) -> String {
    countries
        .iter()
        .find(|(c, _)| c.eq_ignore_ascii_case(code.trim()))
        .map(|(_, p)| p.clone())
        .unwrap_or_else(|| DEFAULT_PATH.to_string())
}

/// The country code of a city slug, from the world's `cities`.
pub fn city_country(world: &Object, slug: &str) -> String {
    world
        .get("cities")
        .and_then(|c| c.get(slug))
        .and_then(|c| c.get("country"))
        .map(Value::py_str)
        .unwrap_or_default()
}

/// Whether either end of a leg is a city in country `code`. A leg that
/// starts or ends in Canada is read by the Canadian rules whole: the US
/// federal layers (HPMS, the Census county lines) hold nothing north of
/// the border, and a cross-border leg's box would catch only its US end.
pub fn leg_touches(world: &Object, leg: &Value, code: &str) -> bool {
    ["from", "to"].iter().any(|end| {
        let slug = leg.get(end).and_then(Value::as_str).unwrap_or("");
        city_country(world, slug).eq_ignore_ascii_case(code)
    })
}

/// The country folder of the city a leg starts from.
pub fn leg_folder(world: &Object, leg: &Value) -> String {
    let from = leg.get("from").and_then(Value::as_str).unwrap_or_default();
    folder_for(&city_country(world, from), &countries())
}

/// The folder of the leg whose id is `from:to`.
pub fn id_folder(world: &Object, id: &str) -> String {
    let from = id.split(':').next().unwrap_or_default();
    folder_for(&city_country(world, from), &countries())
}

pub fn geometry_dir_of(folder: &str) -> PathBuf {
    world_data_dir().join(folder).join("geometry")
}

pub fn gameplay_dir_of(folder: &str) -> PathBuf {
    world_data_dir().join(folder).join("gameplay")
}

/// Every country's gameplay folder, in index order.
pub fn gameplay_dirs() -> Vec<(String, PathBuf)> {
    countries()
        .into_iter()
        .map(|(_, p)| (p.clone(), gameplay_dir_of(&p)))
        .collect()
}

/// Every country's archived roads, keyed by leg id.
pub fn load_archives() -> Result<HashMap<String, LegLine>> {
    let mut out = HashMap::new();
    for (_, folder) in countries() {
        let dir = geometry_dir_of(&folder);
        if dir.is_dir() {
            out.extend(geo::load_archive(&dir)?);
        }
    }
    Ok(out)
}

/// Rows carrying a `leg` key, grouped by the folder of that leg's country.
/// Every folder in the index gets an entry, empty or not, so a writer that
/// rewrites each country's file also clears a country that lost its rows.
pub fn split_rows(world: &Object, rows: Vec<Value>) -> Vec<(String, Vec<Value>)> {
    let countries = countries();
    let mut out: Vec<(String, Vec<Value>)> = countries
        .iter()
        .map(|(_, p)| (p.clone(), Vec::new()))
        .collect();
    for row in rows {
        let id = row.get("leg").map(Value::py_str).unwrap_or_default();
        let from = id.split(':').next().unwrap_or_default();
        let folder = folder_for(&city_country(world, from), &countries);
        match out.iter_mut().find(|(p, _)| *p == folder) {
            Some((_, list)) => list.push(row),
            None => out.push((folder, vec![row])),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn index_dir() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("index.json"),
            r#"{"countries": [{"code": "US", "path": "us"}, {"code": "CA", "path": "ca"}]}"#,
        )
        .unwrap();
        dir
    }

    #[test]
    fn a_country_code_names_its_folder_and_an_unknown_one_is_the_us() {
        let dir = index_dir();
        let countries = countries_in(dir.path());
        assert_eq!(folder_for("CA", &countries), "ca");
        assert_eq!(folder_for("ca", &countries), "ca");
        assert_eq!(folder_for("US", &countries), "us");
        assert_eq!(folder_for("", &countries), "us");
        assert_eq!(folder_for("MX", &countries), "us");
    }

    #[test]
    fn a_tree_with_no_index_is_the_us_alone() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(countries_in(dir.path()), vec![("US".into(), "us".into())]);
    }

    #[test]
    fn the_checked_in_index_files_canada_under_ca() {
        assert_eq!(folder_for("CA", &countries()), "ca");
    }

    #[test]
    fn rows_split_by_the_country_the_leg_starts_in() {
        let world = pyjson::loads(
            r#"{"cities": {"surrey_bc_ca": {"country": "CA"}, "blaine_wa_us": {"country": "US"}}}"#,
        )
        .unwrap();
        let world = world.as_object().unwrap();
        let rows = vec![
            pyjson::loads(r#"{"leg": "surrey_bc_ca:blaine_wa_us"}"#).unwrap(),
            pyjson::loads(r#"{"leg": "blaine_wa_us:surrey_bc_ca"}"#).unwrap(),
        ];
        let split = split_rows(world, rows);
        let of = |f: &str| split.iter().find(|(p, _)| p == f).map(|(_, r)| r.len());
        assert_eq!(of("us"), Some(1));
        assert_eq!(of("ca"), Some(1));
    }
}
