//! Reading the indexed world tree (port of `freight_fate/data/world_loader.py`).
//!
//! `index.json` names the countries; each country has a cities file and
//! either one `legs` file or a sharded `legs/` directory. The result is the
//! raw, typed-but-unvalidated world that `World::from_data` turns into the
//! model: city and leg shapes are fixed enough for serde, while stops,
//! locations and the corridor stay JSON values because the validating parsers
//! need to see a malformed record and report it the way Python did.

use std::path::Path;

use indexmap::IndexMap;
use serde::Deserialize;
use serde_json::Value;

use super::world_models::DataError;

/// `geo.json`: spoken names for state and country codes.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct GeoData {
    #[serde(default)]
    pub countries: IndexMap<String, GeoCountry>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct GeoCountry {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub states: IndexMap<String, String>,
}

/// One city as checked in (or supplied by an overlay / a test fixture).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct RawCity {
    #[serde(default)]
    pub spoken_city: Value,
    #[serde(default)]
    pub state: Value,
    #[serde(default)]
    pub country: Value,
    /// Required: Python read `c["region"]`.
    pub region: String,
    #[serde(default)]
    pub lat: Value,
    #[serde(default)]
    pub lon: Value,
    /// Required: Python read `c["locations"]`; each is validated later.
    pub locations: Vec<Value>,
}

/// One leg as checked in. `corridor` stays raw: it is the lazy half.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct RawLeg {
    pub from: String,
    pub to: String,
    pub miles: Value,
    pub highway: String,
    pub terrain: String,
    #[serde(default)]
    pub stops: Vec<Value>,
    #[serde(default)]
    pub corridor: Value,
    #[serde(default)]
    pub lanes: Value,
    #[serde(default)]
    pub divided: Value,
    #[serde(default)]
    pub truck_advisory: Value,
    #[serde(default)]
    pub route_via: Value,
}

/// The whole raw world: what `load_world_data` returns and `World` consumes.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct WorldData {
    #[serde(default)]
    pub geo: Option<GeoData>,
    #[serde(default)]
    pub cities: IndexMap<String, RawCity>,
    #[serde(default)]
    pub legs: Vec<RawLeg>,
}

impl WorldData {
    /// Parse a world (or overlay) from its JSON text: `{"cities": {...},
    /// "legs": [...]}` with an optional `"geo"`.
    pub fn from_json_str(text: &str) -> Result<WorldData, DataError> {
        serde_json::from_str(text).map_err(|e| DataError::io(e.to_string()))
    }

    /// Parse a world from an in-memory JSON value (test fixtures).
    pub fn from_value(value: Value) -> Result<WorldData, DataError> {
        serde_json::from_value(value).map_err(|e| DataError::io(e.to_string()))
    }
}

#[derive(Deserialize)]
struct Index {
    countries: Option<Vec<IndexCountry>>,
}

#[derive(Deserialize)]
struct IndexCountry {
    code: String,
    path: String,
    #[serde(default)]
    cities: Option<String>,
    #[serde(default)]
    legs: Option<String>,
    #[serde(default)]
    legs_dir: Option<String>,
}

#[derive(Deserialize)]
struct CitiesFile {
    cities: Option<IndexMap<String, RawCity>>,
}

#[derive(Deserialize)]
struct LegsFile {
    legs: Option<Vec<RawLeg>>,
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, DataError> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| DataError::io(format!("{}: {e}", path.display())))?;
    serde_json::from_str(&text).map_err(|e| DataError::io(format!("{}: {e}", path.display())))
}

/// Read a country's legs, from a sharded `legs/` directory or one file.
///
/// The US legs outgrew a single 60 MB file, so they ship as per-state shards
/// an index entry points at with `legs_dir`. Shards are read in sorted
/// filename order, which is the order the build tools write them in, so the
/// merged list is the same every load. Small trees (test fixtures, any future
/// country that stays small) can still name a single `legs` file.
fn load_legs(country_dir: &Path, country: &IndexCountry) -> Result<Vec<RawLeg>, DataError> {
    if let Some(legs_dir) = country.legs_dir.as_deref().filter(|d| !d.is_empty()) {
        let dir = country_dir.join(legs_dir);
        let mut shards: Vec<std::path::PathBuf> = std::fs::read_dir(&dir)
            .map_err(|e| DataError::io(format!("{}: {e}", dir.display())))?
            .filter_map(|entry| entry.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|ext| ext == "json"))
            .collect();
        shards.sort();
        let mut legs = Vec::new();
        for shard in shards {
            let data: LegsFile = read_json(&shard)?;
            let Some(shard_legs) = data.legs else {
                return Err(DataError::value(format!(
                    "{} does not contain a 'legs' list",
                    shard.display()
                )));
            };
            legs.extend(shard_legs);
        }
        return Ok(legs);
    }
    let legs_path = country_dir.join(country.legs.as_deref().unwrap_or("legs.json"));
    let data: LegsFile = read_json(&legs_path)?;
    data.legs.ok_or_else(|| {
        DataError::value(format!(
            "{} does not contain a 'legs' list",
            legs_path.display()
        ))
    })
}

/// Load indexed country data from `world_data/index.json`.
pub fn load_world_data(root: &Path) -> Result<WorldData, DataError> {
    let index_path = root.join("index.json");
    let index: Index = read_json(&index_path)?;
    let Some(countries) = index.countries else {
        return Err(DataError::value(format!(
            "{} does not contain a 'countries' list",
            index_path.display()
        )));
    };
    let mut data = WorldData::default();
    // Spoken-name lookup for state and country codes ("MS" -> "Mississippi").
    // Optional so pre-slug data trees and minimal test fixtures still load.
    let geo_path = root.join("geo.json");
    if geo_path.exists() {
        data.geo = Some(read_json(&geo_path)?);
    }
    for country in &countries {
        let country_dir = root.join(&country.path);
        let cities_path = country_dir.join(country.cities.as_deref().unwrap_or("cities.json"));
        let cities_file: CitiesFile = read_json(&cities_path)?;
        let Some(cities) = cities_file.cities else {
            return Err(DataError::value(format!(
                "{} does not contain a 'cities' object",
                cities_path.display()
            )));
        };
        let legs = load_legs(&country_dir, country)?;
        for (name, mut city) in cities {
            if data.cities.contains_key(&name) {
                return Err(DataError::value(format!(
                    "Duplicate city {} in {}",
                    super::world_parsing::py_repr_str(&name),
                    cities_path.display()
                )));
            }
            if city.country.is_null() {
                city.country = Value::String(country.code.clone());
            }
            data.cities.insert(name, city);
        }
        data.legs.extend(legs);
    }
    Ok(data)
}
