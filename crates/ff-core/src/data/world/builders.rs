//! The load-time builders behind `World::from_data_at`: city identity from
//! the geo lookup, facility validation, and the alias tables that keep
//! pre-slug saves resolving (the module-level helpers of `world.py`).

use std::collections::{HashMap, HashSet};

use indexmap::IndexMap;
use serde_json::Value;

use crate::data::legacy_aliases::LEGACY_CITY_SLUGS;
use crate::data::world_constants::{set_contains, FREIGHT_LOCATION_TYPES};
use crate::data::world_loader::{GeoCountry, RawCity};
use crate::data::world_models::{City, DataError, Location};
use crate::data::world_parsing::{
    py_float, py_repr_str, py_str, service_city_slug, stable_facility_id,
};

pub(super) fn py_float_or(value: &Value, default: f64) -> Result<f64, DataError> {
    match value {
        Value::Null => Ok(default),
        v => py_float(v),
    }
}

pub(super) struct CityIdentity {
    pub(super) spoken_city: String,
    pub(super) state_name: String,
    pub(super) state_code: String,
    pub(super) country_code: String,
    pub(super) country_name: String,
}

fn value_text(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        v => py_str(v).trim().to_string(),
    }
}

/// (spoken city, spoken state, state code, country code, spoken country).
///
/// Migrated cities carry `spoken_city` plus 2-letter `state`/`country` codes
/// resolved through the geo lookup. Pre-slug shapes (bare-name key, full
/// state name, no country) still compose sensibly so overlays and small test
/// fixtures keep loading.
pub(super) fn city_identity(
    key: &str,
    raw: &RawCity,
    countries: &IndexMap<String, GeoCountry>,
) -> CityIdentity {
    let mut spoken_city = value_text(&raw.spoken_city);
    if spoken_city.is_empty() {
        spoken_city = key.to_string();
    }
    let raw_state = value_text(&raw.state);
    let mut country_code = value_text(&raw.country);
    if country_code.is_empty() {
        country_code = "US".to_string();
    }
    let country_info = countries.get(&country_code);
    let states = country_info.map(|c| &c.states);
    let mut country_name = country_info
        .map(|c| c.name.trim().to_string())
        .unwrap_or_default();
    if country_name.is_empty() {
        country_name = country_code.clone();
    }
    if let Some(name) = states.and_then(|s| s.get(&raw_state)) {
        return CityIdentity {
            spoken_city,
            state_name: name.clone(),
            state_code: raw_state,
            country_code,
            country_name,
        };
    }
    let code = states
        .and_then(|s| s.iter().find(|(_, name)| **name == raw_state))
        .map(|(c, _)| c.clone())
        .unwrap_or_default();
    CityIdentity {
        spoken_city,
        state_name: raw_state,
        state_code: code,
        country_code,
        country_name,
    }
}

pub(super) fn validate_city_locations(
    city: &str,
    locations: &[Location],
    facilities_by_id: &mut HashMap<String, Location>,
) -> Result<(), DataError> {
    if locations.is_empty() {
        return Err(DataError::value(format!(
            "{city} has no freight facilities"
        )));
    }
    for location in locations {
        let rname = py_repr_str(&location.name);
        if !set_contains(FREIGHT_LOCATION_TYPES, &location.facility_type) {
            return Err(DataError::value(format!(
                "{city} facility {rname} has unknown type {}",
                py_repr_str(&location.facility_type)
            )));
        }
        if location.id.is_empty() {
            return Err(DataError::value(format!(
                "{city} facility {rname} has no stable id"
            )));
        }
        if facilities_by_id.contains_key(&location.id) {
            return Err(DataError::value(format!(
                "Duplicate facility id {}",
                py_repr_str(&location.id)
            )));
        }
        if location.spoken_name().is_empty() {
            return Err(DataError::value(format!(
                "{city} facility {rname} has no spoken name"
            )));
        }
        if location.source_note.is_empty() {
            return Err(DataError::value(format!(
                "{city} facility {rname} has no source note"
            )));
        }
        if location.ships.is_empty() && location.receives.is_empty() {
            return Err(DataError::value(format!(
                "{city} facility {rname} has no cargo roles"
            )));
        }
        facilities_by_id.insert(location.id.clone(), location.clone());
    }
    Ok(())
}

/// (alias text -> key) plus the keys whose bare spoken name is shared.
///
/// Bare spoken names alias only while globally unique; qualified forms
/// ("Jackson, Michigan" / "Jackson, MI") always alias. The frozen
/// `LEGACY_CITY_SLUGS` map wins every conflict: an old save's name must
/// keep meaning the city it meant when the save was written, even after a
/// later map expansion reuses the name.
pub(super) fn build_city_aliases(
    cities: &IndexMap<String, City>,
) -> (HashMap<String, String>, HashSet<String>) {
    let mut spoken_count: HashMap<String, usize> = HashMap::new();
    for city in cities.values() {
        *spoken_count.entry(city.name.to_lowercase()).or_insert(0) += 1;
    }
    let ambiguous: HashSet<String> = cities
        .values()
        .filter(|city| spoken_count[&city.name.to_lowercase()] > 1)
        .map(|city| city.key.clone())
        .collect();
    let mut aliases: HashMap<String, String> = HashMap::new();
    for (key, city) in cities {
        if spoken_count[&city.name.to_lowercase()] == 1 {
            aliases
                .entry(city.name.clone())
                .or_insert_with(|| key.clone());
        }
        if !city.state.is_empty() {
            aliases
                .entry(format!("{}, {}", city.name, city.state))
                .or_insert_with(|| key.clone());
        }
        if !city.state_code.is_empty() {
            aliases
                .entry(format!("{}, {}", city.name, city.state_code))
                .or_insert_with(|| key.clone());
        }
    }
    for (old_name, slug) in LEGACY_CITY_SLUGS {
        if cities.contains_key(*slug) {
            aliases.insert(old_name.to_string(), slug.to_string());
        }
    }
    (aliases, ambiguous)
}

/// Map pre-slug facility ids to current ones.
///
/// Old ids embedded a slug of the display name (`jackson-michigan:...`);
/// template facility names embedded the display name too, so both parts can
/// differ. Rebuilt per legacy name so every persisted job facility id keeps
/// resolving.
pub(super) fn build_legacy_facility_ids(
    cities: &IndexMap<String, City>,
    legacy_names_by_key: &IndexMap<String, Vec<String>>,
) -> HashMap<String, String> {
    let mut legacy_ids: HashMap<String, String> = HashMap::new();
    for (key, old_names) in legacy_names_by_key {
        let city = &cities[key];
        for old_name in old_names {
            for location in &city.locations {
                let old_facility_name = if location.template {
                    location.name.replace(&city.name, old_name)
                } else {
                    location.name.clone()
                };
                let old_id =
                    stable_facility_id(old_name, &location.facility_type, &old_facility_name);
                legacy_ids
                    .entry(old_id)
                    .or_insert_with(|| location.id.clone());
            }
        }
    }
    legacy_ids
}

/// Map the city slug embedded in local city-service ids to the city key.
///
/// The checked-in local approach and geometry ids were generated from
/// pre-slug display names (`city_service:sault-ste-marie:garage`); current
/// spoken names map too so future data keyed either way keeps resolving.
pub(super) fn build_service_city_keys(
    cities: &IndexMap<String, City>,
    legacy_names_by_key: &IndexMap<String, Vec<String>>,
) -> HashMap<String, String> {
    let mut service_keys: HashMap<String, String> = HashMap::new();
    for (key, city) in cities {
        service_keys
            .entry(service_city_slug(&city.name))
            .or_insert_with(|| key.clone());
    }
    for (key, old_names) in legacy_names_by_key {
        for old_name in old_names {
            service_keys.insert(service_city_slug(old_name), key.clone());
        }
    }
    service_keys
}
