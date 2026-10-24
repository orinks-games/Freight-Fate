//! Parsing helpers for checked-in world data (port of
//! `freight_fate/data/world_parsing.py`).
//!
//! The world JSON is validated record by record with the same checks and the
//! same error messages as the Python loader (tests match on those messages).
//! The per-mile corridor record parsers live in the `records` submodule and
//! are re-exported; the Python-value shims at the top (`py_str`, `py_float`,
//! ...) reproduce `str()`, `float()`, `int()`, `bool()` and `repr()` on a
//! JSON value so a parse reads the same way it did.

use serde_json::{Map, Value};

use super::legacy_aliases::legacy_city_slug;
use super::world_constants::{
    facility_cargo_roles, is_stand_in_market, lookup, set_contains, template_facility_city_gate,
    BASE_MARKET_FACILITY_TYPES, CITY_MARKET_TAGS, DEFAULT_POI_ACTIONS, DEFAULT_VEHICLE_ACCESS,
    FACILITY_LEVEL_UNLOCKS, FACILITY_NAME_TEMPLATES, FACILITY_SOURCE_NOTES, FREIGHT_LOCATION_TYPES,
    MARKET_TAG_FACILITY_TYPES, PARKING_CERTAINTY_LABELS, POI_ACTIONS, POI_DENSITY_MEDIUM_LEG_MILES,
    POI_DENSITY_SHORT_LEG_MILES, RAW_FACILITY_TEXT_MARKERS, RAW_POI_TEXT_MARKERS,
    REGION_MARKET_TAGS, SOURCE_BACKED_POI_ACTIONS, STAND_IN_MARKET_FACILITY_TYPE,
    STATE_MARKET_TAGS, STOP_CURATION_LEVELS, STOP_DIRECTIONS, STOP_TYPE_LABELS,
    VEHICLE_ACCESS_LEVELS,
};
use super::world_loader::{RawCity, RawLeg, WorldData};
use super::world_models::{DataError, Location, Stop};
use crate::pyfmt::{py_str_float, round_py_n};

mod advisory;
mod limits;
mod pyval;
mod records;
pub use limits::*;
pub use pyval::*;
pub use records::*;

fn contains_marker(lowered: &str, markers: &[&str]) -> bool {
    markers.iter().any(|marker| lowered.contains(marker))
}

// ---------------------------------------------------------------- overlays

/// The key an overlay city name lands on: itself, or the slug it aliases.
///
/// Overlays written before the slug migration name cities by display name;
/// treating those as the base city they alias keeps the merge additive
/// instead of duplicating the city under its old name.
fn overlay_city_key(name: &str, cities: &indexmap::IndexMap<String, RawCity>) -> String {
    if cities.contains_key(name) {
        return name.to_string();
    }
    match legacy_city_slug(name) {
        Some(slug) if cities.contains_key(slug) => slug.to_string(),
        _ => name.to_string(),
    }
}

fn leg_pair_key(leg: &RawLeg, cities: &indexmap::IndexMap<String, RawCity>) -> (String, String) {
    let a = overlay_city_key(&leg.from, cities);
    let b = overlay_city_key(&leg.to, cities);
    if a <= b {
        (a, b)
    } else {
        (b, a)
    }
}

/// Return `base` with overlay cities and legs added, never overridden.
///
/// The merge is purely additive so the checked-in base stays authoritative:
/// a city already present (by key, or by a pre-slug legacy name aliasing one)
/// keeps its base definition, and a leg already present (by unordered
/// endpoint pair) keeps its base definition. Only genuinely new cities and
/// legs from the overlay are appended.
pub fn merge_overlay(base: WorldData, overlay: WorldData) -> WorldData {
    let mut cities = base.cities;
    for (name, city) in overlay.cities {
        if !cities.contains_key(&overlay_city_key(&name, &cities)) {
            cities.insert(name, city);
        }
    }
    let mut legs = base.legs;
    let mut seen: std::collections::HashSet<(String, String)> =
        legs.iter().map(|leg| leg_pair_key(leg, &cities)).collect();
    for leg in overlay.legs {
        let key = leg_pair_key(&leg, &cities);
        if seen.insert(key) {
            legs.push(leg);
        }
    }
    WorldData {
        geo: base.geo,
        cities,
        legs,
    }
}

// ---------------------------------------------------------------- facilities

pub fn parse_location(
    raw: &Value,
    city_key: &str,
    spoken_city: &str,
    city_lat: f64,
    city_lon: f64,
) -> Result<Location, DataError> {
    let Some(raw) = raw.as_object() else {
        return Err(DataError::value(format!(
            "{spoken_city} facility must be an object"
        )));
    };
    let name = clean_facility_name(spoken_city, &get_str(raw, "name"))?;
    let facility_type = get_str(raw, "type");
    if !set_contains(FREIGHT_LOCATION_TYPES, &facility_type) {
        return Err(DataError::value(format!(
            "{spoken_city} facility {} has unknown type {}",
            py_repr_str(&name),
            py_repr_str(&facility_type)
        )));
    }
    let (default_ships, default_receives) =
        facility_cargo_roles(&facility_type).unwrap_or((&[], &[]));
    let raw_cargo = get_str_list(raw, "cargo");
    let default_cargo = dedupe(
        default_ships
            .iter()
            .chain(default_receives.iter())
            .map(|s| s.to_string()),
    );
    let cargo = if raw_cargo.is_empty() {
        default_cargo
    } else {
        raw_cargo
    };
    let ships = role_cargo(raw, "ships", &cargo, default_ships);
    let receives = role_cargo(raw, "receives", &cargo, default_receives);
    let mut roles = Vec::new();
    if !ships.is_empty() {
        roles.push("shipper".to_string());
    }
    if !receives.is_empty() {
        roles.push("receiver".to_string());
    }
    let source_note = [raw.get("source_note"), raw.get("source")]
        .into_iter()
        .flatten()
        .find(|v| py_truthy(v))
        .map(py_str)
        .unwrap_or_else(|| {
            lookup(FACILITY_SOURCE_NOTES, &facility_type)
                .unwrap_or("Curated representative facility.")
                .to_string()
        })
        .trim()
        .to_string();
    let spoken = [raw.get("spoken_name"), raw.get("spoken")]
        .into_iter()
        .flatten()
        .find(|v| py_truthy(v))
        .map(py_str)
        .unwrap_or_default()
        .trim()
        .to_string();
    let locality = get_str(raw, "locality");
    let traits = get_str_list(raw, "traits");
    let id = match raw.get("id").filter(|v| py_truthy(v)) {
        Some(v) => py_str(v),
        None => stable_facility_id(city_key, &facility_type, &name),
    }
    .trim()
    .to_string();
    let min_level_default = lookup(FACILITY_LEVEL_UNLOCKS, &facility_type).unwrap_or(1);
    Ok(Location {
        name,
        facility_type,
        cargo,
        id,
        city: city_key.to_string(),
        locality,
        roles,
        ships,
        receives,
        lat: get_float(raw, "lat", city_lat)?,
        lon: get_float(raw, "lon", city_lon)?,
        traits,
        source_note,
        spoken,
        template: get_bool(raw, "template", false),
        min_level: get_int(raw, "min_level", min_level_default)?,
    })
}

pub fn expand_market_locations(
    city_key: &str,
    spoken_city: &str,
    lat: f64,
    lon: f64,
    explicit_locations: &[Location],
    market_tags: &[String],
) -> Vec<Location> {
    let mut locations: Vec<Location> = explicit_locations.to_vec();
    let mut existing_types: Vec<String> =
        locations.iter().map(|l| l.facility_type.clone()).collect();
    let mut existing_names: Vec<String> = locations.iter().map(|l| l.name.to_lowercase()).collect();
    // A STAND-IN MARKET gets one yard, not a skyline.
    //
    // Every other city is stamped with the four base types plus whatever its
    // market tags add, and the endpoint sweep then binds each to a real OSM
    // freight site nearby. In these cities it binds nothing: not one facility
    // has an endpoint the freight-site screen accepts, so all of them are
    // invented, and four invented warehouses in one town is four stories the
    // map cannot back (owner ruling, 2026-09-20). One company yard keeps the
    // town a place freight moves through -- it ships general, retail and
    // parcel and takes bulk fuel -- without the rest of the pretence.
    //
    // A city that curates its own freight is never a stand-in, whatever the
    // endpoint sweep found: those locations were authored by hand, and the
    // market types that go with them are owed too.
    let stand_in = is_stand_in_market(city_key) && explicit_locations.is_empty();
    let mut desired_types: Vec<String> = if stand_in {
        vec![STAND_IN_MARKET_FACILITY_TYPE.to_string()]
    } else {
        BASE_MARKET_FACILITY_TYPES
            .iter()
            .map(|s| s.to_string())
            .collect()
    };
    if !stand_in {
        for tag in market_tags {
            if let Some(types) = lookup(MARKET_TAG_FACILITY_TYPES, tag) {
                desired_types.extend(types.iter().map(|s| s.to_string()));
            }
        }
    }
    for facility_type in dedupe(desired_types) {
        if existing_types.contains(&facility_type) {
            continue;
        }
        // Geography gate: region/state market tags over-stamp water- and
        // rail-dependent types; skip a template the city plainly can't host
        // (curated facilities above are never gated).
        if let Some(gate) = template_facility_city_gate(&facility_type) {
            if gate
                .allowlist
                .is_some_and(|allow| !allow.contains(city_key))
            {
                continue;
            }
            if gate.denylist.is_some_and(|deny| deny.contains(city_key)) {
                continue;
            }
        }
        let mut location = template_location(
            city_key,
            spoken_city,
            lat,
            lon,
            &facility_type,
            market_tags,
            "",
        );
        if existing_names.contains(&location.name.to_lowercase()) {
            location = template_location(
                city_key,
                spoken_city,
                lat,
                lon,
                &facility_type,
                market_tags,
                " Facility",
            );
        }
        existing_types.push(location.facility_type.clone());
        existing_names.push(location.name.to_lowercase());
        locations.push(location);
    }
    locations
}

fn template_location(
    city_key: &str,
    spoken_city: &str,
    lat: f64,
    lon: f64,
    facility_type: &str,
    market_tags: &[String],
    name_suffix: &str,
) -> Location {
    let template = lookup(FACILITY_NAME_TEMPLATES, facility_type)
        .unwrap_or_else(|| panic!("no facility name template for {facility_type:?}"));
    let name = format!("{}{}", template.replace("{city}", spoken_city), name_suffix);
    let (ships, receives) = facility_cargo_roles(facility_type)
        .unwrap_or_else(|| panic!("no cargo roles for {facility_type:?}"));
    let cargo = dedupe(ships.iter().chain(receives.iter()).map(|s| s.to_string()));
    let source_note = format!(
        "{} Generated offline as a representative {spoken_city} metro-market facility; \
         not a claim about a specific real-world shipper.",
        lookup(FACILITY_SOURCE_NOTES, facility_type).unwrap_or_default()
    );
    let (jitter_lat, jitter_lon) = jittered_coordinates(city_key, facility_type, lat, lon);
    let mut traits = vec!["representative".to_string(), "template".to_string()];
    traits.extend(market_tags.iter().cloned());
    Location {
        name: name.clone(),
        facility_type: facility_type.to_string(),
        cargo,
        id: stable_facility_id(city_key, facility_type, &name),
        city: city_key.to_string(),
        locality: String::new(),
        roles: vec!["shipper".to_string(), "receiver".to_string()],
        ships: ships.iter().map(|s| s.to_string()).collect(),
        receives: receives.iter().map(|s| s.to_string()).collect(),
        lat: jitter_lat,
        lon: jitter_lon,
        traits,
        source_note,
        spoken: String::new(),
        template: true,
        min_level: lookup(FACILITY_LEVEL_UNLOCKS, facility_type).unwrap_or(1),
    }
}

pub fn market_tags_for_city(
    city_key: &str,
    state_code: &str,
    raw_city: &RawCity,
    locations: &[Location],
) -> Vec<String> {
    let mut tags: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for tag in lookup(REGION_MARKET_TAGS, &raw_city.region).unwrap_or(&[]) {
        tags.insert(tag.to_string());
    }
    for tag in lookup(STATE_MARKET_TAGS, state_code).unwrap_or(&[]) {
        tags.insert(tag.to_string());
    }
    for tag in lookup(CITY_MARKET_TAGS, city_key).unwrap_or(&[]) {
        tags.insert(tag.to_string());
    }
    for location in locations {
        for tag in tags_for_facility_type(&location.facility_type) {
            tags.insert(tag.to_string());
        }
    }
    // Python returned `tuple(sorted(tags))`; a BTreeSet iterates sorted.
    tags.into_iter().collect()
}

fn tags_for_facility_type(facility_type: &str) -> &'static [&'static str] {
    match facility_type {
        "air_cargo" => &["air"],
        "distribution" => &["retail"],
        "food_terminal" => &["food", "cold_chain"],
        "industrial_park" => &["industrial"],
        "intermodal" => &["intermodal"],
        "manufacturing" => &["manufacturing"],
        "port" => &["port"],
        "rail" => &["intermodal"],
        "retail_distribution" => &["retail"],
        "terminal" => &["cross_dock"],
        "warehouse" => &["retail"],
        _ => &[],
    }
}

fn role_cargo(
    raw: &Map<String, Value>,
    key: &str,
    cargo: &[String],
    defaults: &[&str],
) -> Vec<String> {
    let values = get_str_list(raw, key);
    if !values.is_empty() {
        return values;
    }
    let plausible: Vec<String> = cargo
        .iter()
        .filter(|value| defaults.contains(&value.as_str()))
        .cloned()
        .collect();
    if !plausible.is_empty() {
        return plausible;
    }
    cargo.iter().filter(|v| !v.is_empty()).cloned().collect()
}

fn clean_facility_name(city: &str, name: &str) -> Result<String, DataError> {
    if name.is_empty() {
        return Err(DataError::value(format!(
            "{city} has a facility without a name"
        )));
    }
    if contains_marker(&name.to_lowercase(), RAW_FACILITY_TEXT_MARKERS) {
        return Err(DataError::value(format!(
            "{city} facility {} exposes raw source text",
            py_repr_str(name)
        )));
    }
    Ok(name.to_string())
}

pub fn stable_facility_id(city: &str, facility_type: &str, name: &str) -> String {
    format!("{}:{}:{}", slug(city), facility_type, slug(name))
}

/// The city fragment of a local city-service id ("Sault Ste. Marie" ->
/// "sault-ste-marie"), matching how the local-data sweep slugged names.
pub fn service_city_slug(text: &str) -> String {
    let mut out = String::new();
    let mut pending_dash = false;
    for ch in text.to_lowercase().chars() {
        if ch.is_ascii_lowercase() || ch.is_ascii_digit() {
            if pending_dash {
                out.push('-');
            }
            out.push(ch);
            pending_dash = false;
        } else {
            pending_dash = true;
        }
    }
    if pending_dash && !out.is_empty() {
        out.push('-');
    }
    out.trim_matches('-').to_string()
}

/// The facility-id slug: lowercase, runs of non-alphanumerics to one dash.
pub fn slug(text: &str) -> String {
    let mut out = String::new();
    let mut pending_dash = false;
    for ch in text.to_lowercase().chars() {
        if ch.is_alphanumeric() {
            if pending_dash && !out.is_empty() {
                out.push('-');
            }
            out.push(ch);
            pending_dash = false;
        } else {
            pending_dash = true;
        }
    }
    let trimmed = out.trim_matches('-');
    if trimmed.is_empty() {
        "facility".to_string()
    } else {
        trimmed.to_string()
    }
}

pub fn dedupe<I: IntoIterator<Item = String>>(values: I) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for value in values {
        if !value.is_empty() && !out.contains(&value) {
            out.push(value);
        }
    }
    out
}

/// zlib.crc32 (IEEE 802.3 CRC-32), for the deterministic template jitter.
pub fn crc32(bytes: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    for &byte in bytes {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

fn jittered_coordinates(city: &str, facility_type: &str, lat: f64, lon: f64) -> (f64, f64) {
    if lat == 0.0 && lon == 0.0 {
        return (lat, lon);
    }
    let seed = crc32(format!("{city}:{facility_type}").as_bytes());
    let lat_offset = f64::from((seed & 0xFF) as i32 - 128) / 5000.0;
    let lon_offset = f64::from(((seed >> 8) & 0xFF) as i32 - 128) / 5000.0;
    (
        round_py_n(lat + lat_offset, 5),
        round_py_n(lon + lon_offset, 5),
    )
}

/// True when `name` is one of the old whole-city market placeholders.
///
/// Checked against every name the city has answered to (current spoken plus
/// frozen legacy display names) so pre-slug saves keep resolving.
pub fn is_legacy_market_name(city_names: &[&str], name: &str) -> bool {
    let normalized = name.trim().to_lowercase();
    if normalized.is_empty() {
        return true;
    }
    city_names.iter().any(|city_name| {
        let city_lower = city_name.to_lowercase();
        normalized == city_lower
            || normalized == format!("{city_lower} freight market")
            || normalized == format!("{city_lower} metro freight market")
    })
}

// ---------------------------------------------------------------- stops

pub fn parse_stop(
    raw: &Value,
    leg_miles: f64,
    from_city: &str,
    to_city: &str,
) -> Result<Stop, DataError> {
    let err = |msg: String| DataError::value(format!("{from_city} to {to_city} {msg}"));
    let Some(raw) = raw.as_object() else {
        return Err(err(format!(
            "stop {} is missing explicit at_mi",
            py_repr_value(raw)
        )));
    };
    let name = get_str(raw, "name");
    if name.is_empty() {
        return Err(err("has a stop without a name".to_string()));
    }
    let rname = py_repr_str(&name);
    if contains_marker(&name.to_lowercase(), RAW_POI_TEXT_MARKERS) {
        return Err(err(format!("stop {rname} exposes raw OSM/source text")));
    }
    if !raw.contains_key("at_mi") {
        return Err(err(format!("stop {rname} is missing explicit at_mi")));
    }
    let at_mi = req_float(raw, "at_mi")?;
    if !(0.0 < at_mi && at_mi < leg_miles) {
        return Err(err(format!(
            "stop {rname} has at_mi {}, outside leg mileage 0-{}",
            py_str_float(at_mi),
            py_str_float(leg_miles)
        )));
    }
    let mut stop_type = get_str(raw, "type");
    if stop_type.is_empty() {
        stop_type = classify_stop(&name).to_string();
    }
    if lookup(STOP_TYPE_LABELS, &stop_type).is_none() {
        return Err(err(format!(
            "stop {rname} has unknown type {}",
            py_repr_str(&stop_type)
        )));
    }
    let source = get_str(raw, "source");
    let default_actions: &[&str] = lookup(DEFAULT_POI_ACTIONS, &stop_type).unwrap_or(&[]);
    let actions: Vec<String> = if raw.contains_key("actions") {
        get_str_list_unfiltered(raw, "actions")
    } else {
        default_actions.iter().map(|s| s.to_string()).collect()
    };
    if actions.is_empty() {
        return Err(err(format!("stop {rname} has no actions")));
    }
    let unknown = sorted_unique(actions.iter().filter(|a| !set_contains(POI_ACTIONS, a)));
    if !unknown.is_empty() {
        return Err(err(format!(
            "stop {rname} has unknown actions {}",
            py_repr_list(&unknown)
        )));
    }
    let disallowed = sorted_unique(
        actions
            .iter()
            .filter(|a| !default_actions.contains(&a.as_str())),
    );
    if !disallowed.is_empty() {
        let source_backed = disallowed
            .iter()
            .all(|a| set_contains(SOURCE_BACKED_POI_ACTIONS, a));
        if !source_backed {
            return Err(err(format!(
                "stop {rname} actions {} do not match type {}",
                py_repr_list(&disallowed),
                py_repr_str(&stop_type)
            )));
        }
    }
    let services = get_str_list(raw, "services");
    let mut parking = get_str(raw, "parking");
    if parking.is_empty() {
        parking = default_parking_certainty(&stop_type, &services, &actions).to_string();
    }
    if lookup(PARKING_CERTAINTY_LABELS, &parking).is_none() {
        return Err(err(format!(
            "stop {rname} has unknown parking certainty {}",
            py_repr_str(&parking)
        )));
    }
    let directions: Vec<String> = if raw.contains_key("directions") {
        get_str_list(raw, "directions")
    } else {
        vec!["both".to_string()]
    };
    if directions.is_empty() {
        return Err(err(format!("stop {rname} has no directions")));
    }
    let unknown_directions = sorted_unique(
        directions
            .iter()
            .filter(|d| !set_contains(STOP_DIRECTIONS, d)),
    );
    if !unknown_directions.is_empty() {
        return Err(err(format!(
            "stop {rname} has unknown directions {}",
            py_repr_list(&unknown_directions)
        )));
    }
    if directions.iter().any(|d| d == "both") && directions.len() > 1 {
        return Err(err(format!(
            "stop {rname} mixes 'both' with direction-specific applicability"
        )));
    }
    let parking_spaces = get_int(raw, "parking_spaces", 0)?;
    if !(0..=1000).contains(&parking_spaces) {
        return Err(err(format!(
            "stop {rname} has implausible parking_spaces {parking_spaces}"
        )));
    }
    let mut vehicle_access = get_str(raw, "vehicle_access");
    if vehicle_access.is_empty() {
        vehicle_access = DEFAULT_VEHICLE_ACCESS.to_string();
    }
    if !set_contains(VEHICLE_ACCESS_LEVELS, &vehicle_access) {
        return Err(err(format!(
            "stop {rname} has unknown vehicle_access {}",
            py_repr_str(&vehicle_access)
        )));
    }
    let mut curation = get_str(raw, "curation");
    if curation.is_empty() {
        curation = infer_stop_curation(&name, &source).to_string();
    }
    if !set_contains(STOP_CURATION_LEVELS, &curation) {
        return Err(err(format!(
            "stop {rname} has unknown curation {}",
            py_repr_str(&curation)
        )));
    }
    if curation == "curated" && infer_stop_curation(&name, &source) == "placeholder" {
        return Err(err(format!(
            "stop {rname} looks synthetic but is marked curated"
        )));
    }
    // Python iterated `SOURCE_BACKED_POI_ACTIONS & set(actions)` -- set order
    // is arbitrary there, so the first error reported could vary; the checks
    // themselves are the same.
    for action in SOURCE_BACKED_POI_ACTIONS {
        if !actions.iter().any(|a| a == action) {
            continue;
        }
        if !services.iter().any(|s| s == action) {
            return Err(err(format!(
                "stop {rname} action {} requires matching source-backed service metadata",
                py_repr_str(action)
            )));
        }
        if source.is_empty() {
            return Err(err(format!(
                "stop {rname} action {} requires a source note",
                py_repr_str(action)
            )));
        }
    }
    // The interchange that serves the stop, when the bake decided one. The
    // corridor is the lazy half of a leg and is not parsed yet, so whether a
    // record sits at that mile is proved by `data_stop_exits`, not here.
    let interchange_mi = if raw.contains_key("interchange_mi") {
        let mi = req_float(raw, "interchange_mi")?;
        if !(0.0..=leg_miles).contains(&mi) {
            return Err(err(format!(
                "stop {rname} has interchange_mi {}, outside leg mileage 0-{}",
                py_str_float(mi),
                py_str_float(leg_miles)
            )));
        }
        Some(mi)
    } else {
        None
    };
    // The streets from each of that exit's ramp terminals to the stop's
    // driveway (`tools/build_stop_approaches.py`), validated like a
    // facility's exit chains; none for a stop on the mainline.
    let approach_chains = match raw.get("approach_chains") {
        None => Vec::new(),
        Some(value) => {
            let chains =
                super::world_local_data::parse_exit_chains(&format!("stop {rname}"), value)?;
            if !chains.is_empty() && get_str(raw, "approach_source").is_empty() {
                return Err(err(format!(
                    "stop {rname} has approach chains without a source"
                )));
            }
            chains
        }
    };
    Ok(Stop {
        name,
        at_mi,
        stop_type,
        source,
        actions,
        services,
        parking,
        directions,
        curation,
        parking_spaces,
        vehicle_access,
        exit_ref: get_str(raw, "exit_ref"),
        interchange_mi,
        approach_chains,
    })
}

/// `sorted(set(items))` for a handful of strings.
pub(crate) fn sorted_unique<'a, I: IntoIterator<Item = &'a String>>(items: I) -> Vec<String> {
    let set: std::collections::BTreeSet<String> = items.into_iter().cloned().collect();
    set.into_iter().collect()
}

fn classify_stop(name: &str) -> &'static str {
    let lower = name.to_lowercase();
    if lower.contains("weigh") {
        return "weigh_station";
    }
    if lower.contains("parking") {
        return "truck_parking";
    }
    if lower.contains("rest area") {
        return "public_rest_area";
    }
    if lower.contains("service plaza") {
        return "service_plaza";
    }
    if lower.contains("truck") {
        return "truck_stop";
    }
    // Python fell through to "travel_center" either way.
    "travel_center"
}

fn default_parking_certainty(
    stop_type: &str,
    services: &[String],
    actions: &[String],
) -> &'static str {
    if !services.iter().any(|s| s == "parking") && !actions.iter().any(|a| a == "park") {
        return "none";
    }
    if matches!(stop_type, "truck_stop" | "travel_center" | "service_plaza") {
        return "likely";
    }
    if matches!(stop_type, "public_rest_area" | "truck_parking") {
        return "limited";
    }
    "unknown"
}

fn infer_stop_curation(name: &str, source: &str) -> &'static str {
    let text = format!("{name} {source}").to_lowercase();
    const SYNTHETIC_MARKERS: &[&str] = &[
        "corridor rest area",
        "corridor truck parking",
        "corridor fuel stop",
        "descriptive gameplay stop seeded",
        "seeded for offline route coverage",
        "no actionable overpass poi candidate",
    ];
    if contains_marker(&text, SYNTHETIC_MARKERS) {
        "placeholder"
    } else {
        "curated"
    }
}

pub fn minimum_curated_pois(miles: f64) -> i64 {
    if miles < POI_DENSITY_SHORT_LEG_MILES {
        1
    } else if miles <= POI_DENSITY_MEDIUM_LEG_MILES {
        2
    } else {
        3
    }
}

pub fn minimum_fuel_capable_pois(miles: f64) -> i64 {
    if miles < POI_DENSITY_SHORT_LEG_MILES {
        0
    } else {
        1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn python_shims_match_cpython_spellings() {
        assert_eq!(py_repr_str("Jackson"), "'Jackson'");
        assert_eq!(py_repr_str("Love's"), "\"Love's\"");
        assert_eq!(py_repr_list(&["b".into(), "a".into()]), "['b', 'a']");
        assert_eq!(py_repr_value(&serde_json::json!(null)), "None");
        assert_eq!(py_repr_value(&serde_json::json!(2)), "2");
        assert_eq!(py_str(&serde_json::json!(2.5)), "2.5");
        assert!(py_truthy(&serde_json::json!("x")) && !py_truthy(&serde_json::json!("")));
        assert_eq!(py_int_of(&serde_json::json!(3.7)).unwrap(), 3);
    }

    #[test]
    fn slugs_and_crc_match_the_python_originals() {
        assert_eq!(slug("Sault Ste. Marie"), "sault-ste-marie");
        assert_eq!(slug("!!!"), "facility");
        assert_eq!(service_city_slug("Sault Ste. Marie"), "sault-ste-marie");
        assert_eq!(
            stable_facility_id("jackson_ms_us", "cross_dock", "Jackson Cross-Dock"),
            "jackson-ms-us:cross_dock:jackson-cross-dock"
        );
        // zlib.crc32(b"hello") == 907060870
        assert_eq!(crc32(b"hello"), 907_060_870);
        assert_eq!(crc32(b""), 0);
    }

    #[test]
    fn legacy_market_names_cover_the_old_placeholders() {
        assert!(is_legacy_market_name(
            &["Jackson"],
            "Jackson freight market"
        ));
        assert!(is_legacy_market_name(
            &["Jackson"],
            "  jackson METRO freight market "
        ));
        assert!(is_legacy_market_name(&["Jackson"], ""));
        assert!(!is_legacy_market_name(&["Jackson"], "Jackson Cross-Dock"));
    }

    #[test]
    fn poi_density_floors() {
        assert_eq!(minimum_curated_pois(100.0), 1);
        assert_eq!(minimum_curated_pois(200.0), 2);
        assert_eq!(minimum_curated_pois(400.0), 3);
        assert_eq!(minimum_fuel_capable_pois(100.0), 0);
        assert_eq!(minimum_fuel_capable_pois(160.0), 1);
    }
}
