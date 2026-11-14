//! Response parsers for the state 511 traffic APIs.
//!
//! Split out of `real_traffic` to keep both halves at a reviewable size:
//! this module owns the per-platform response formats and the
//! [`TrafficEvent`] model they produce; `real_traffic` owns the endpoint
//! registry, caching, and background fetching.
//!
//! Parsers:
//!   `ohgo`   — Ohio OHGO native JSON format (reference implementation).
//!   `iteris` — Shared Iteris/INRIX-platform `/Events` endpoint format.
//!              No state currently rides it (their REST APIs are gone) but
//!              the CARS parser reuses its closure/location helpers.
//!   `wzdx`   — Work Zone Data Exchange standard (GeoJSON FeatureCollection).
//!              Handles both the older camelCase property layout and the
//!              v4.x snake_case `core_details` layout the live state feeds
//!              publish today (checked 2026-08-09).
//!   `cars`   — Castle Rock CARS GraphQL platform (`POST /api/graphql`
//!              MapFeatures query).  Used by Indiana 511IN, Minnesota 511MN,
//!              and Colorado COtrip.
//!   `list511` — The 511 sites' own list-page JSON rows joined with the
//!              map-pin locations (Florida FL511 and New York 511NY
//!              incidents).  Lives in `real_traffic_list511`.
//!   `caltrans_lcs` — Caltrans's Lane Closure System, one CSV per
//!              district (California lane closures).
//!
//! Port of `freight_fate/sim/real_traffic_parsers.py`. The Python parsers
//! are a mixin on the provider; here they are free functions over
//! `serde_json::Value`, with the `state` argument kept for parity.

use once_cell::sync::Lazy;
use regex::Regex;
use serde_json::{Map, Value};

/// Python semantics over JSON values (`dict.get`, `str()`, truthiness,
/// `float()`), shared with the other feed parsers.
pub(crate) mod pyval;

use pyval::{as_map, chain, chain_str, py_str, str_or_empty, to_f64, to_i64, truthy};

/// A traffic incident or construction event.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TrafficEvent {
    pub id: String,
    /// "incident", "construction", "weather"
    pub event_type: String,
    /// "low", "medium", "high"
    pub severity: String,
    pub description: String,
    pub county: String,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    pub start_time: Option<String>,
    pub estimated_end: Option<String>,
    pub lanes_affected: Option<String>,
    /// highway/road name for construction events
    pub road_name: String,
    /// "near milepost 45" or "between exits 43 and 47"
    pub location_text: String,
    /// "construction", "maintenance", "utility", "bridge", "paving"
    pub work_type: String,
    /// "alternating", "single lane", "shoulder", "full closure"
    pub closure: String,
    /// The one direction of travel the event holds, as the road is signed:
    /// "North", "South", "East" or "West". Empty when it holds both ways or
    /// the feed does not say.
    pub direction: String,
}

impl TrafficEvent {
    /// The five required dataclass fields; the rest keep their defaults.
    pub fn new(
        id: &str,
        event_type: &str,
        severity: &str,
        description: &str,
        county: &str,
    ) -> Self {
        Self {
            id: id.to_string(),
            event_type: event_type.to_string(),
            severity: severity.to_string(),
            description: description.to_string(),
            county: county.to_string(),
            ..Self::default()
        }
    }

    pub fn to_dict(&self) -> Value {
        let opt_f = |v: Option<f64>| v.map(Value::from).unwrap_or(Value::Null);
        let opt_s = |v: &Option<String>| v.clone().map(Value::from).unwrap_or(Value::Null);
        let mut map = Map::new();
        map.insert("id".into(), Value::from(self.id.clone()));
        map.insert("event_type".into(), Value::from(self.event_type.clone()));
        map.insert("severity".into(), Value::from(self.severity.clone()));
        map.insert("description".into(), Value::from(self.description.clone()));
        map.insert("county".into(), Value::from(self.county.clone()));
        map.insert("latitude".into(), opt_f(self.latitude));
        map.insert("longitude".into(), opt_f(self.longitude));
        map.insert("start_time".into(), opt_s(&self.start_time));
        map.insert("estimated_end".into(), opt_s(&self.estimated_end));
        map.insert("lanes_affected".into(), opt_s(&self.lanes_affected));
        map.insert("road_name".into(), Value::from(self.road_name.clone()));
        map.insert(
            "location_text".into(),
            Value::from(self.location_text.clone()),
        );
        map.insert("work_type".into(), Value::from(self.work_type.clone()));
        map.insert("closure".into(), Value::from(self.closure.clone()));
        Value::Object(map)
    }

    pub fn from_dict(data: &Value) -> Option<TrafficEvent> {
        // Require at least basic identification
        let data = data.as_object()?;
        if data.is_empty() {
            return None;
        }
        let event_id = chain_str(data, &["id"], "");
        if event_id.is_empty() {
            return None;
        }
        // `float(data["latitude"]) if data.get("latitude") else None`: a
        // truthy value that will not convert fails the whole record.
        let coordinate = |key: &str| -> Result<Option<f64>, ()> {
            match data.get(key) {
                Some(v) if truthy(v) => to_f64(v).map(Some).ok_or(()),
                _ => Ok(None),
            }
        };
        let latitude = coordinate("latitude").ok()?;
        let longitude = coordinate("longitude").ok()?;
        let raw_opt = |key: &str| -> Option<String> {
            match data.get(key) {
                None | Some(Value::Null) => None,
                Some(v) => Some(py_str(v)),
            }
        };
        Some(TrafficEvent {
            id: event_id,
            event_type: chain_str(data, &["event_type"], "incident"),
            severity: chain_str(data, &["severity"], "low"),
            description: chain_str(data, &["description"], ""),
            county: chain_str(data, &["county"], ""),
            latitude,
            longitude,
            start_time: raw_opt("start_time"),
            estimated_end: raw_opt("estimated_end"),
            lanes_affected: raw_opt("lanes_affected"),
            road_name: chain_str(data, &["road_name"], ""),
            location_text: chain_str(data, &["location_text"], ""),
            work_type: chain_str(data, &["work_type"], ""),
            closure: chain_str(data, &["closure"], ""),
            direction: String::new(),
        })
    }
}

/// `data.get(a, data.get(b, data.get(c, [])))` normalised to a list: a
/// lone dict becomes a one-item list; anything else is no events at all.
pub(crate) fn event_list(data: &Value, keys: &[&str]) -> Option<Vec<Value>> {
    let raw: Option<&Value> = if data.is_array() {
        Some(data)
    } else {
        as_map(Some(data)).and_then(|map| chain(map, keys))
    };
    match raw {
        None => Some(Vec::new()),
        Some(Value::Array(items)) => Some(items.clone()),
        Some(Value::Object(_)) => Some(vec![raw.unwrap().clone()]),
        Some(_) => None,
    }
}

/// Parse construction work zone events from API response.
///
/// This is the reference parser for Ohio OHGO. Iteris-platform states
/// use [`parse_iteris_construction_events`] instead.
pub fn parse_construction_events(data: &Value, _state: &str) -> Vec<TrafficEvent> {
    let mut events = Vec::new();
    let Some(raw_events) = (if data.is_object() {
        event_list(data, &["construction", "events", "results"])
    } else {
        None
    }) else {
        return events;
    };
    for construction in &raw_events {
        let Some(construction) = construction.as_object() else {
            continue;
        };
        let event_id = str_or_empty(construction.get("id"));
        if event_id.is_empty() {
            continue;
        }
        let (lat, lon) = extract_construction_coordinates(construction);
        let location_text = build_construction_location_text(construction);
        let closure = determine_closure_type(construction);
        let lanes = describe_lanes_affected(construction);
        let work_type = classify_work_type(construction);
        let severity = construction_severity(&closure);

        let road_name = chain_str(construction, &["road", "route"], "");
        let description = chain_str(construction, &["description", "details"], "");
        let county = chain_str(construction, &["county"], "");
        let start_time = chain_str(construction, &["start_date", "start_time"], "");
        let estimated_end = chain_str(construction, &["end_date", "end_time"], "");

        events.push(TrafficEvent {
            id: event_id,
            event_type: "construction".into(),
            severity: severity.into(),
            description,
            county,
            latitude: lat,
            longitude: lon,
            start_time: Some(start_time),
            estimated_end: Some(estimated_end),
            lanes_affected: Some(lanes),
            road_name,
            location_text,
            work_type,
            closure,
            direction: String::new(),
        });
    }
    events
}

// ---- Shared Iteris-platform parser ------------------------------------

/// Parse general traffic incidents from an Iteris-platform API response.
///
/// The Iteris platform returns an array of event objects with `id`,
/// `event_type`, `severity`, `headline`, `location`, `road_name`, and
/// date fields.  No state currently serves this format (their `/Events`
/// REST APIs are gone) but the closure and location helpers below are
/// shared with the CARS parser.
pub fn parse_iteris_events(data: &Value, _state: &str) -> Vec<TrafficEvent> {
    let mut events = Vec::new();
    let Some(raw) = event_list(data, &["events", "results"]) else {
        return events;
    };
    for item in &raw {
        let Some(item) = item.as_object() else {
            continue;
        };
        let event_id = chain_str(item, &["id", "event_id"], "");
        if event_id.is_empty() {
            continue;
        }
        // Determine event type (only incidents here)
        let api_type = chain_str(item, &["event_type", "type"], "incident").to_lowercase();
        let event_type = if ["construction", "roadwork", "work_zone"].contains(&api_type.as_str()) {
            "construction"
        } else {
            "incident"
        };
        // Coordinates: Iteris puts lat/lon in a sub-object or top-level fields
        let (lat, lon) = parse_iteris_coordinates(item);
        // Severity
        let severity = map_severity(&chain_str(item, &["severity"], "low"));
        // Road name
        let road_name = chain_str(item, &["road_name", "road", "route"], "");
        let description = chain_str(item, &["headline", "description", "event_text"], "");
        let county = chain_str(item, &["county", "region"], "");
        let start_time = chain_str(item, &["start_date", "start_time"], "");
        let estimated_end = chain_str(item, &["end_date", "end_time"], "");
        let lanes = chain_str(item, &["lanes_affected", "lanes"], "");

        events.push(TrafficEvent {
            id: event_id,
            event_type: event_type.into(),
            severity: severity.into(),
            description,
            county,
            latitude: lat,
            longitude: lon,
            start_time: Some(start_time),
            estimated_end: Some(estimated_end),
            lanes_affected: Some(lanes),
            road_name,
            ..TrafficEvent::default()
        });
    }
    events
}

/// Parse construction work-zone events from an Iteris-platform API.
///
/// The Iteris-platform `/Events` endpoint mixes incidents and
/// construction events.  This parser filters to construction-type events
/// only, then applies the same enrichment helpers
/// ([`determine_closure_type`], [`classify_work_type`], …) used by the
/// Ohio parser so downstream zone conversion behaves identically.
pub fn parse_iteris_construction_events(data: &Value, state: &str) -> Vec<TrafficEvent> {
    let all_events = parse_iteris_events(data, state);
    let mut construction_events = Vec::new();
    for event in all_events {
        if event.event_type != "construction" {
            continue;
        }
        // Re-parse with construction-specific enrichment
        // We need the raw dict item again for richer field access.
        let Some(raw) = event_list(data, &["events", "results"]) else {
            continue;
        };
        let matching = raw.iter().find(|r| {
            r.as_object()
                .is_some_and(|r| chain_str(r, &["id", "event_id"], "") == event.id)
        });
        let Some(item) = matching.and_then(Value::as_object) else {
            log::debug!(
                "No raw Iteris item for event {}, appending unenriched",
                event.id
            );
            construction_events.push(event);
            continue;
        };
        // Enrich with construction-specific fields using the shared helpers
        let location_text = build_iteris_location_text(item);
        let closure = determine_iteris_closure(item, &event.description);
        let lanes = describe_lanes_affected(item); // Uses the same logic
        let work_type = classify_work_type(item);
        let severity = construction_severity(&closure);
        let lanes_affected = if !lanes.is_empty() {
            lanes
        } else {
            event.lanes_affected.clone().unwrap_or_default()
        };
        construction_events.push(TrafficEvent {
            id: event.id,
            event_type: "construction".into(),
            severity: severity.into(),
            description: event.description,
            county: event.county,
            latitude: event.latitude,
            longitude: event.longitude,
            start_time: event.start_time,
            estimated_end: event.estimated_end,
            lanes_affected: Some(lanes_affected),
            road_name: event.road_name,
            location_text,
            work_type,
            closure,
            direction: String::new(),
        });
    }
    construction_events
}

/// `float(lat), float(lon)` for a present, non-null pair; `None` when
/// either is missing, null, or will not convert.
pub(crate) fn float_pair(lat: Option<&Value>, lon: Option<&Value>) -> Option<(f64, f64)> {
    match (lat, lon) {
        (Some(lat), Some(lon)) if !lat.is_null() && !lon.is_null() => {
            Some((to_f64(lat)?, to_f64(lon)?))
        }
        _ => None,
    }
}

/// Extract coordinates from an Iteris-platform event.
///
/// Iteris puts `lat`/`lon` directly on the object, or inside a
/// `location` sub-object.
pub fn parse_iteris_coordinates(item: &Map<String, Value>) -> (Option<f64>, Option<f64>) {
    // Direct top-level fields
    let lat = chain(item, &["lat", "latitude"]);
    let lon = chain(item, &["lon", "lng", "longitude"]);
    if let Some((lat, lon)) = float_pair(lat, lon) {
        return (Some(lat), Some(lon));
    }
    // Sub-object (location: {lat: ..., lon: ...})
    if let Some(loc) = as_map(item.get("location")) {
        let lat = chain(loc, &["lat", "latitude"]);
        let lon = chain(loc, &["lon", "lng", "longitude"]);
        if let Some((lat, lon)) = float_pair(lat, lon) {
            return (Some(lat), Some(lon));
        }
    }
    (None, None)
}

/// Build a location description from Iteris fields.
pub fn build_iteris_location_text(item: &Map<String, Value>) -> String {
    // Direct location text
    let text = chain_str(item, &["location_text", "location"], "");
    if !text.is_empty() {
        return text;
    }
    // Cross streets / intersection
    if let Some(cross) = item.get("cross_street").filter(|v| truthy(v)) {
        return format!("At {}", py_str(cross));
    }
    // Milepost / mile range
    let start = chain(item, &["start_milepost", "milepost"]).filter(|v| truthy(v));
    let end = item.get("end_milepost").filter(|v| truthy(v));
    if let (Some(start), Some(end)) = (start, end) {
        return format!("Between milepost {} and {}", py_str(start), py_str(end));
    }
    if let Some(start) = start {
        return format!("Near milepost {}", py_str(start));
    }
    String::new()
}

/// Determine closure type from Iteris fields.
pub fn determine_iteris_closure(item: &Map<String, Value>, description: &str) -> String {
    // Direct field
    let closure = chain_str(item, &["closure", "closure_type"], "").to_lowercase();
    if !closure.is_empty() {
        return closure;
    }
    // Check the description for closure keywords
    let desc = description.to_lowercase();
    let any = |words: &[&str]| words.iter().any(|w| desc.contains(w));
    if any(&["full closure", "road closed", "detour"]) {
        return "full closure".into();
    }
    if any(&["alternating", "flag", "one-way"]) {
        return "alternating".into();
    }
    if desc.contains("shoulder") {
        return "shoulder".into();
    }
    if any(&["lane closure", "right lane", "left lane"]) {
        return "single lane".into();
    }
    "single lane".into()
}

// ---- Castle Rock CARS GraphQL parser ---------------------------------

/// Parse traffic events from a CARS MapFeatures GraphQL response.
///
/// The Castle Rock CARS platform (Indiana 511IN, Minnesota 511MN,
/// Colorado COtrip) answers `POST /api/graphql` with one map feature
/// per event inside `data.mapFeaturesQuery.mapFeatures`: a `uri`
/// ("event/CARSy-30"), a `title` that packs road, mile range, and
/// text ("US 20 (Mile Point 42.5 - 42.61): Lane closed."), a bbox, a
/// Point feature with the marker position, and a `priority` where 1
/// is most urgent.  The layer slug requested in the query decides
/// whether the whole batch is construction or incidents.
pub fn parse_cars_events(data: &Value, _state: &str, construction: bool) -> Vec<TrafficEvent> {
    let mut events = Vec::new();
    let Some(query) = as_map(Some(data)).and_then(|d| as_map(d.get("data"))) else {
        return events;
    };
    let Some(features_query) = as_map(query.get("mapFeaturesQuery")) else {
        return events;
    };
    let Some(raw) = features_query
        .get("mapFeatures")
        .filter(|v| truthy(v))
        .and_then(Value::as_array)
    else {
        return events;
    };
    for item in raw {
        let Some(item) = item.as_object() else {
            continue;
        };
        // Zoom 15 keeps the server from clustering, but skip any
        // non-event feature type (Cluster, Sign, ...) defensively.
        match item.get("__typename") {
            None | Some(Value::Null) => {}
            Some(Value::String(name)) if name == "Event" => {}
            Some(_) => continue,
        }
        let uri = chain_str(item, &["uri"], "");
        let event_id = uri.rsplit('/').next().unwrap_or("").to_string();
        if event_id.is_empty() {
            continue;
        }
        let title = chain_str(item, &["title"], "").trim().to_string();
        // Scheduled-but-inactive events ride the same layer with a
        // "STARTS FRIDAY." style prefix; they are not on the road yet.
        if title.to_uppercase().starts_with("STARTS ") {
            continue;
        }
        let (road_name, location_text, remainder) = split_cars_title(&title);
        let direction = cars_title_direction(&title).to_string();
        let description = if title.is_empty() {
            chain_str(item, &["tooltip"], "")
        } else {
            title.clone()
        };
        let (lat, lon) = extract_cars_coordinates(item);
        let text = if remainder.is_empty() {
            title.as_str()
        } else {
            remainder.as_str()
        };
        let (closure, severity, lanes, work_type) = if construction {
            let closure = determine_iteris_closure(&Map::new(), text);
            let severity = construction_severity(&closure);
            let mut closure_map = Map::new();
            closure_map.insert("closure".into(), Value::from(closure.clone()));
            let lanes = describe_lanes_affected(&closure_map);
            let mut desc_map = Map::new();
            desc_map.insert("description".into(), Value::from(text));
            let work_type = classify_work_type(&desc_map);
            (closure, severity.to_string(), lanes, work_type)
        } else {
            let severity = cars_priority_severity(item.get("priority").unwrap_or(&Value::Null));
            (
                String::new(),
                severity.to_string(),
                String::new(),
                String::new(),
            )
        };
        events.push(TrafficEvent {
            id: event_id,
            event_type: if construction {
                "construction".into()
            } else {
                "incident".into()
            },
            severity,
            description,
            county: String::new(),
            latitude: lat,
            longitude: lon,
            start_time: None,
            estimated_end: None,
            lanes_affected: Some(lanes),
            road_name,
            location_text,
            work_type,
            closure,
            direction,
        });
    }
    events
}

/// The one direction a CARS title holds ("I-80 eastbound: Exit ramp
/// closed." is "East"); empty for both directions or none named.
pub fn cars_title_direction(title: &str) -> &'static str {
    let road = title.split(": ").next().unwrap_or("").to_lowercase();
    if road.contains("both directions") {
        return "";
    }
    let found: Vec<&'static str> = [
        ("northbound", "North"),
        ("southbound", "South"),
        ("eastbound", "East"),
        ("westbound", "West"),
    ]
    .into_iter()
    .filter(|(word, _)| road.contains(word))
    .map(|(_, direction)| direction)
    .collect();
    match found[..] {
        [one] => one,
        _ => "",
    }
}

static CARS_PAREN: Lazy<Regex> = Lazy::new(|| Regex::new(r"\(([^)]*)\)\s*").unwrap());

/// Split a CARS event title into (road_name, location_text, text).
///
/// Titles look like "US 20 (Mile Point 42.5 - 42.61): Lane closed." or
/// "I-35W southbound: Crash."; the road part may carry a parenthesised
/// mile range and a direction suffix that would break road matching.
pub fn split_cars_title(title: &str) -> (String, String, String) {
    let mut text = title.trim().to_string();
    let mut location_text = String::new();
    if let Some(m) = CARS_PAREN.captures(&text) {
        let whole = m.get(0).unwrap();
        location_text = m.get(1).unwrap().as_str().trim().to_string();
        text = format!("{}{}", &text[..whole.start()], &text[whole.end()..])
            .trim()
            .to_string();
    }
    let Some((road_part, remainder)) = text.split_once(": ") else {
        return (String::new(), location_text, text);
    };
    // Drop any leading sentence ("ends Friday." style notes) and the
    // direction suffix so _road_name_matches sees a bare designation.
    let mut road = road_part
        .rsplit(". ")
        .next()
        .unwrap_or("")
        .trim()
        .to_string();
    let lowered = road.to_lowercase();
    for suffix in [
        " northbound",
        " southbound",
        " eastbound",
        " westbound",
        " in both directions",
    ] {
        if lowered.ends_with(suffix) {
            road = road[..road.len() - suffix.len()].trim().to_string();
            break;
        }
    }
    (road, location_text, remainder.trim().to_string())
}

/// Extract lat/lon from a CARS map feature.
///
/// Prefers the marker Point geometry; falls back to the bbox midpoint
/// (bbox is [west, south, east, north]).
pub fn extract_cars_coordinates(item: &Map<String, Value>) -> (Option<f64>, Option<f64>) {
    if let Some(features) = item.get("features").and_then(Value::as_array) {
        for feature in features {
            let Some(feature) = feature.as_object() else {
                continue;
            };
            let Some(geometry) = as_map(feature.get("geometry")) else {
                continue;
            };
            if geometry.get("type").and_then(Value::as_str) != Some("Point") {
                continue;
            }
            if let Some(coords) = geometry.get("coordinates").and_then(Value::as_array) {
                if coords.len() >= 2 {
                    if let (Some(lat), Some(lon)) = (to_f64(&coords[1]), to_f64(&coords[0])) {
                        return (Some(lat), Some(lon)); // [lon, lat]
                    }
                }
            }
        }
    }
    if let Some(bbox) = item.get("bbox").and_then(Value::as_array) {
        if bbox.len() >= 4 {
            let parsed: Vec<Option<f64>> = bbox[..4].iter().map(to_f64).collect();
            if let [Some(west), Some(south), Some(east), Some(north)] = parsed[..] {
                return (Some((south + north) / 2.0), Some((west + east) / 2.0));
            }
        }
    }
    (None, None)
}

/// Map a CARS event priority (1 = most urgent) to severity levels.
pub fn cars_priority_severity(priority: &Value) -> &'static str {
    let Some(value) = to_i64(priority) else {
        return "low";
    };
    if value <= 2 {
        "high"
    } else if value <= 5 {
        "medium"
    } else {
        "low"
    }
}

// ---- WZDx standard parser (GeoJSON FeatureCollection) ----------------
// Lives in `real_traffic_parsers/wzdx.rs`; re-exported here so the module's
// surface matches the Python file.
mod wzdx;
pub use wzdx::{
    build_wzdx_location_text, build_wzdx_v4_event, describe_wzdx_v4_lanes,
    extract_wzdx_coordinates, parse_wzdx_construction_events, parse_wzdx_events,
    wzdx_impact_to_closure, wzdx_prop,
};

// ---- Caltrans Lane Closure System (per-district CSV) -----------------
mod caltrans_lcs;
pub use caltrans_lcs::parse_lcs_csv;

// ---- Shared construction-field helpers -------------------------------

/// Extract lat/lon from a construction event, handling various API formats.
pub fn extract_construction_coordinates(
    construction: &Map<String, Value>,
) -> (Option<f64>, Option<f64>) {
    // Direct lat/lon fields (OHGO format)
    let lat = chain(construction, &["lat", "latitude"]);
    let lon = chain(construction, &["lon", "lng", "longitude"]);
    if let Some((lat, lon)) = float_pair(lat, lon) {
        return (Some(lat), Some(lon));
    }
    // Geometry object with coordinates array (GeoJSON format used by some 511 APIs)
    if let Some(geometry) = as_map(construction.get("geometry")) {
        if let Some(coords) = geometry.get("coordinates").and_then(Value::as_array) {
            if coords.len() >= 2 {
                if let (Some(lat), Some(lon)) = (to_f64(&coords[1]), to_f64(&coords[0])) {
                    return (Some(lat), Some(lon)); // [lon, lat] GeoJSON convention
                }
            }
        }
    }
    // Start/end point objects
    for key in ["start_point", "end_point"] {
        let Some(point) = as_map(construction.get(key)) else {
            continue;
        };
        let slat = chain(point, &["lat", "latitude"]);
        let slon = chain(point, &["lon", "lng", "longitude"]);
        if let Some((lat, lon)) = float_pair(slat, slon) {
            return (Some(lat), Some(lon));
        }
    }
    (None, None)
}

/// Build a human-readable location reference from construction data.
pub fn build_construction_location_text(construction: &Map<String, Value>) -> String {
    // Direct location text field
    let text = chain_str(construction, &["location", "location_text"], "");
    if !text.is_empty() {
        return text;
    }
    // Milepost range
    let start_mile = chain(construction, &["start_milepost", "beg_mm"]).filter(|v| truthy(v));
    let end_mile = chain(construction, &["end_milepost", "end_mm"]).filter(|v| truthy(v));
    if let (Some(start), Some(end)) = (start_mile, end_mile) {
        return format!("Between milepost {} and {}", py_str(start), py_str(end));
    }
    if let Some(start) = start_mile {
        return format!("Near milepost {}", py_str(start));
    }
    // Street/intersection reference
    if let Some(cross) =
        chain(construction, &["cross_street", "intersection"]).filter(|v| truthy(v))
    {
        return format!("At {}", py_str(cross));
    }
    String::new()
}

/// Determine the type of lane or road closure.
pub fn determine_closure_type(construction: &Map<String, Value>) -> String {
    // Direct closure field
    let closure = chain_str(construction, &["closure", "closure_type"], "").to_lowercase();
    if !closure.is_empty() {
        return closure;
    }
    // Look for closure keywords in description
    let desc = chain_str(construction, &["description"], "").to_lowercase();
    if desc.contains("full closure") || desc.contains("road closed") || desc.contains("detour") {
        return "full closure".into();
    }
    if desc.contains("alternating") || desc.contains("flag") || desc.contains("one-way") {
        return "alternating".into();
    }
    if desc.contains("shoulder") {
        return "shoulder".into();
    }
    if desc.contains("lane closure") {
        return "single lane".into();
    }
    // Default: implied lane restriction for construction
    "single lane".into()
}

/// Build a description of which lanes are affected.
pub fn describe_lanes_affected(construction: &Map<String, Value>) -> String {
    // Direct lanes affected field
    if let Some(lanes) = chain(construction, &["lanes_affected", "lanes"]).filter(|v| truthy(v)) {
        return py_str(lanes);
    }
    // Infer from closure type
    match determine_closure_type(construction).as_str() {
        "full closure" => "all lanes closed".into(),
        "alternating" => "alternating single lane".into(),
        "shoulder" => "right shoulder closed".into(),
        _ => "left lane closed".into(),
    }
}

/// Classify the type of work being performed.
pub fn classify_work_type(construction: &Map<String, Value>) -> String {
    let work_type = chain_str(construction, &["work_type", "type"], "").to_lowercase();
    if !work_type.is_empty() {
        return work_type;
    }
    // Infer from description keywords
    let desc = chain_str(construction, &["description"], "").to_lowercase();
    let any = |words: &[&str]| words.iter().any(|w| desc.contains(w));
    if any(&["bridge", "overpass", "structure"]) {
        return "bridge".into();
    }
    if any(&["pave", "paving", "resurface", "mill"]) {
        return "paving".into();
    }
    if any(&["utility", "pipe", "gas"]) {
        return "utility".into();
    }
    if any(&["inspect", "repair", "maintain"]) {
        return "maintenance".into();
    }
    "construction".into()
}

/// Map construction closure type to severity.
pub fn construction_severity(closure: &str) -> &'static str {
    match closure {
        "full closure" => "high",
        "alternating" | "single lane" => "medium",
        _ => "low",
    }
}

// ---- Ohio OHGO incident parser ---------------------------------------

/// Parse traffic events from API response.
///
/// This is a reference implementation for Ohio OHGO. Other states will
/// need their own parsers as API formats vary.
pub fn parse_events(data: &Value, _state: &str) -> Vec<TrafficEvent> {
    let mut events = Vec::new();
    // Ohio OHGO format parsing
    let Some(incidents) = data.get("incidents").and_then(Value::as_array) else {
        return events;
    };
    for incident in incidents {
        let Some(incident) = incident.as_object() else {
            continue;
        };
        let coordinate = |key: &str| -> Result<Option<f64>, ()> {
            match incident.get(key) {
                Some(v) if truthy(v) => to_f64(v).map(Some).ok_or(()),
                _ => Ok(None),
            }
        };
        let (Ok(latitude), Ok(longitude)) = (coordinate("lat"), coordinate("lon")) else {
            log::debug!("Failed to parse incident: bad coordinates");
            continue;
        };
        let raw_opt = |key: &str| -> Option<String> {
            match incident.get(key) {
                None | Some(Value::Null) => None,
                Some(v) => Some(py_str(v)),
            }
        };
        events.push(TrafficEvent {
            id: chain_str(incident, &["id"], ""),
            event_type: "incident".into(),
            severity: map_severity(&chain_str(incident, &["severity"], "low")).into(),
            description: chain_str(incident, &["description"], ""),
            county: chain_str(incident, &["county"], ""),
            latitude,
            longitude,
            start_time: raw_opt("start_time"),
            estimated_end: raw_opt("estimated_end"),
            lanes_affected: raw_opt("lanes_affected"),
            ..TrafficEvent::default()
        });
    }
    events
}

/// Map API severity to our standard severity levels.
pub fn map_severity(api_severity: &str) -> &'static str {
    match api_severity.to_lowercase().as_str() {
        "low" => "low",
        "minor" => "low",
        "medium" => "medium",
        "moderate" => "medium",
        "intermediate" => "medium", // FL511's middle tier
        "high" => "high",
        "major" => "high",
        "severe" => "high",
        "critical" => "high",
        _ => "low",
    }
}

#[cfg(test)]
mod tests;
