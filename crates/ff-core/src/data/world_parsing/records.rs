//! Per-mile corridor record parsers -- the `_parse_*` half of
//! `freight_fate/data/world_parsing.py` that `world_corridor` builds a leg
//! from (route points, elevation, grades, lanes, limits, tolls, exits, ...).

use serde_json::{Map, Value};

use super::{
    get_bool, get_float, get_int, get_str, get_str_list, get_str_raw, py_int_of, py_repr_list,
    py_repr_str, py_repr_value, py_str, req_float, sorted_unique,
};
use crate::data::world_constants::{
    lookup, set_contains, RAW_POI_TEXT_MARKERS, STOP_DIRECTIONS, TOLL_METHOD_LABELS,
};
use crate::data::world_models::{
    BillboardBan, DataError, ElevationSample, GradeSegment, HpmsTerrain, Interchange, Landmark,
    LaneSegment, RouteCheckpoint, RoutePoint, RouteRestriction, StateCrossing, StateMileage,
    TollEvent, TrafficVolumeSample,
};
use crate::pyfmt::{fmt_f, py_str_float};

pub(super) fn object<'a>(
    raw: &'a Value,
    from_city: &str,
    to_city: &str,
    what: &str,
) -> Result<&'a Map<String, Value>, DataError> {
    raw.as_object().ok_or_else(|| {
        DataError::value(format!("{from_city} to {to_city} {what} must be an object"))
    })
}

fn exposes_raw(text: &str) -> bool {
    let lowered = text.to_lowercase();
    RAW_POI_TEXT_MARKERS.iter().any(|m| lowered.contains(m))
}

pub fn parse_at_mi(
    raw: &Map<String, Value>,
    leg_miles: f64,
    from_city: &str,
    to_city: &str,
    label: &str,
    allow_endpoints: bool,
) -> Result<f64, DataError> {
    if !raw.contains_key("at_mi") {
        return Err(DataError::value(format!(
            "{from_city} to {to_city} {label} is missing explicit at_mi"
        )));
    }
    let at_mi = req_float(raw, "at_mi")?;
    let in_range = if allow_endpoints {
        0.0 <= at_mi && at_mi <= leg_miles
    } else {
        0.0 < at_mi && at_mi < leg_miles
    };
    if !in_range {
        return Err(DataError::value(format!(
            "{from_city} to {to_city} {label} has at_mi {}, outside leg mileage 0-{}",
            py_str_float(at_mi),
            py_str_float(leg_miles)
        )));
    }
    Ok(at_mi)
}

pub fn parse_route_point(
    raw: &Value,
    leg_miles: f64,
    from_city: &str,
    to_city: &str,
) -> Result<RoutePoint, DataError> {
    let raw = object(raw, from_city, to_city, "route point")?;
    let at_mi = parse_at_mi(raw, leg_miles, from_city, to_city, "route point", true)?;
    let lat = req_float(raw, "lat")?;
    let lon = req_float(raw, "lon")?;
    if !(-90.0..=90.0).contains(&lat) || !(-180.0..=180.0).contains(&lon) {
        return Err(DataError::value(format!(
            "{from_city} to {to_city} route point has invalid coordinates"
        )));
    }
    Ok(RoutePoint { at_mi, lat, lon })
}

pub fn parse_elevation_sample(
    raw: &Value,
    leg_miles: f64,
    from_city: &str,
    to_city: &str,
) -> Result<ElevationSample, DataError> {
    let raw = object(raw, from_city, to_city, "elevation sample")?;
    let at_mi = parse_at_mi(raw, leg_miles, from_city, to_city, "elevation sample", true)?;
    let elevation_ft = req_float(raw, "elevation_ft")?;
    if !(-300.0..=20_500.0).contains(&elevation_ft) {
        return Err(DataError::value(format!(
            "{from_city} to {to_city} elevation sample has invalid elevation"
        )));
    }
    Ok(ElevationSample {
        at_mi,
        elevation_ft,
        source: get_str(raw, "source"),
    })
}

pub fn parse_grade_segment(
    raw: &Value,
    leg_miles: f64,
    from_city: &str,
    to_city: &str,
) -> Result<GradeSegment, DataError> {
    let raw = object(raw, from_city, to_city, "grade segment")?;
    if !raw.contains_key("start_mi") {
        return Err(DataError::value(format!(
            "{from_city} to {to_city} grade segment is missing explicit start_mi"
        )));
    }
    let start_mi = req_float(raw, "start_mi")?;
    if !(0.0..=leg_miles).contains(&start_mi) {
        return Err(DataError::value(format!(
            "{from_city} to {to_city} grade segment start has start_mi {}, outside leg mileage 0-{}",
            py_str_float(start_mi),
            py_str_float(leg_miles)
        )));
    }
    let end_mi = req_float(raw, "end_mi")?;
    if !(0.0..=leg_miles).contains(&end_mi) || end_mi <= start_mi {
        return Err(DataError::value(format!(
            "{from_city} to {to_city} grade segment has invalid range {}-{}",
            py_str_float(start_mi),
            py_str_float(end_mi)
        )));
    }
    let avg_grade_pct = req_float(raw, "avg_grade_pct")?;
    if !(-15.0..=15.0).contains(&avg_grade_pct) {
        return Err(DataError::value(format!(
            "{from_city} to {to_city} grade segment has unrealistic grade {}",
            py_str_float(avg_grade_pct)
        )));
    }
    let mut terrain = get_str(raw, "terrain");
    if terrain.is_empty() {
        terrain = "flat".to_string();
    }
    if !matches!(terrain.as_str(), "flat" | "hills" | "mountain") {
        return Err(DataError::value(format!(
            "{from_city} to {to_city} grade segment has unknown terrain {}",
            py_repr_str(&terrain)
        )));
    }
    Ok(GradeSegment {
        start_mi,
        end_mi,
        avg_grade_pct,
        terrain,
        source: get_str(raw, "source"),
    })
}

pub fn parse_lane_segment(
    raw: &Value,
    leg_miles: f64,
    from_city: &str,
    to_city: &str,
) -> Result<LaneSegment, DataError> {
    let raw = object(raw, from_city, to_city, "lane segment")?;
    let start_mi = req_float(raw, "start_mi")?;
    let end_mi = req_float(raw, "end_mi")?;
    if !(0.0..=leg_miles).contains(&start_mi)
        || !(0.0..=leg_miles).contains(&end_mi)
        || end_mi <= start_mi
    {
        return Err(DataError::value(format!(
            "{from_city} to {to_city} lane segment has invalid range {}-{}",
            py_str_float(start_mi),
            py_str_float(end_mi)
        )));
    }
    let lanes = raw
        .get("lanes")
        .ok_or_else(|| DataError::key("'lanes'"))
        .and_then(py_int_of)?;
    if !(1..=10).contains(&lanes) {
        return Err(DataError::value(format!(
            "{from_city} to {to_city} lane segment has out-of-range lanes {lanes}"
        )));
    }
    let opt_lanes = |key: &str| -> Result<i64, DataError> {
        match raw.get(key) {
            None => Ok(0),
            Some(v) => {
                let val = py_int_of(v)?;
                if !(1..=10).contains(&val) {
                    return Err(DataError::value(format!(
                        "{from_city} to {to_city} lane segment has out-of-range {key} {val}"
                    )));
                }
                Ok(val)
            }
        }
    };
    Ok(LaneSegment {
        start_mi,
        end_mi,
        lanes,
        lanes_forward: opt_lanes("lanes_forward")?,
        lanes_backward: opt_lanes("lanes_backward")?,
        oneway: get_bool(raw, "oneway", false),
        source: get_str(raw, "source"),
    })
}

pub fn parse_lane_segments(
    raw: &[Value],
    leg_miles: f64,
    from_city: &str,
    to_city: &str,
) -> Result<Vec<LaneSegment>, DataError> {
    raw.iter()
        .map(|s| parse_lane_segment(s, leg_miles, from_city, to_city))
        .collect()
}

pub fn parse_restriction(
    raw: &Value,
    leg_miles: f64,
    from_city: &str,
    to_city: &str,
) -> Result<RouteRestriction, DataError> {
    let raw = object(raw, from_city, to_city, "restriction")?;
    let at_mi = parse_at_mi(raw, leg_miles, from_city, to_city, "restriction", true)?;
    let kind = get_str(raw, "kind");
    if !matches!(kind.as_str(), "low_clearance" | "weight_limit") {
        return Err(DataError::value(format!(
            "{from_city} to {to_city} restriction has unknown kind {}",
            py_repr_str(&kind)
        )));
    }
    let feet = get_float(raw, "feet", 0.0)?;
    let tons = get_float(raw, "tons", 0.0)?;
    if kind == "low_clearance" && !(9.0..=16.5).contains(&feet) {
        return Err(DataError::value(format!(
            "{from_city} to {to_city} restriction has implausible clearance {} ft",
            py_str_float(feet)
        )));
    }
    if kind == "weight_limit" && !(3.0..=40.0).contains(&tons) {
        return Err(DataError::value(format!(
            "{from_city} to {to_city} restriction has implausible weight limit {} tons",
            py_str_float(tons)
        )));
    }
    Ok(RouteRestriction {
        at_mi,
        kind,
        feet,
        tons,
        source: get_str(raw, "source"),
    })
}

/// Parse the baked restriction advisories, ordered along the leg.
pub fn parse_restrictions(
    raw_samples: &[Value],
    leg_miles: f64,
    from_city: &str,
    to_city: &str,
) -> Result<Vec<RouteRestriction>, DataError> {
    let mut samples: Vec<RouteRestriction> = raw_samples
        .iter()
        .map(|s| parse_restriction(s, leg_miles, from_city, to_city))
        .collect::<Result<_, _>>()?;
    samples.sort_by(|a, b| a.at_mi.partial_cmp(&b.at_mi).expect("finite at_mi"));
    Ok(samples)
}

pub fn parse_billboard_bans(
    raw_samples: &[Value],
    leg_miles: f64,
    from_city: &str,
    to_city: &str,
) -> Result<Vec<BillboardBan>, DataError> {
    let mut bans = raw_samples
        .iter()
        .map(|raw| {
            let raw = object(raw, from_city, to_city, "billboard ban")?;
            let from_mi = req_float(raw, "from_mi")?;
            let to_mi = req_float(raw, "to_mi")?;
            if from_mi < 0.0 || to_mi <= from_mi || to_mi > leg_miles {
                return Err(DataError::value(format!(
                    "{from_city} to {to_city} billboard ban has invalid range {}-{}",
                    py_str_float(from_mi),
                    py_str_float(to_mi)
                )));
            }
            let name = get_str(raw, "name");
            let source = get_str(raw, "source");
            if name.trim().is_empty() || source.trim().is_empty() {
                return Err(DataError::value(format!(
                    "{from_city} to {to_city} billboard ban needs a name and source"
                )));
            }
            Ok(BillboardBan {
                from_mi,
                to_mi,
                name,
                source,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    bans.sort_by(|a, b| {
        a.from_mi
            .partial_cmp(&b.from_mi)
            .expect("finite billboard-ban mileposts")
            .then_with(|| {
                a.to_mi
                    .partial_cmp(&b.to_mi)
                    .expect("finite billboard-ban mileposts")
            })
    });
    Ok(bans)
}

pub fn parse_traffic_volume(
    raw: &Value,
    leg_miles: f64,
    from_city: &str,
    to_city: &str,
) -> Result<TrafficVolumeSample, DataError> {
    let raw = object(raw, from_city, to_city, "traffic volume")?;
    let at_mi = parse_at_mi(raw, leg_miles, from_city, to_city, "traffic volume", true)?;
    let aadt = get_float(raw, "aadt", 0.0)?;
    if aadt <= 0.0 {
        return Err(DataError::value(format!(
            "{from_city} to {to_city} traffic volume at {} has no AADT",
            py_str_float(at_mi)
        )));
    }
    let lanes = get_int(raw, "lanes", 2)?.max(1);
    Ok(TrafficVolumeSample {
        at_mi,
        aadt,
        lanes,
        source: get_str(raw, "source"),
    })
}

/// A single lane passes roughly 2,000 vehicles an hour, and the busiest hour
/// of a weekday carries about 8 percent of the day in the peak direction. So
/// one lane can plausibly stand behind an AADT of very roughly 45,000 -- call
/// it 40,000 to leave room for genuinely brutal roads. Above that, a record
/// claiming ONE lane per direction is not reporting a busy road: it is
/// disagreeing with itself, describing a divided freeway and a country lane in
/// the same breath.
///
/// This is the volume/lane version of the screens in `data/curves`, and
/// it exists for the same reason. Two records in the 2026-08-19 HPMS bake hit
/// it, both on CA-99 -- a divided freeway whose windowed lane median was
/// dragged to 1 by a frontage-road section snapping inside the corridor. Left
/// alone, congestion would have divided 69,000 vehicles a day by that single
/// lane and parked a permanent phantom jam on a road that flows.
///
/// Screened at LOAD, never edited out of the bake: the bake keeps saying what
/// HPMS said, so the rule can be re-judged if it turns out too broad. The
/// flagged sample keeps its volume -- the volume is the reading -- and only
/// its contradicted lane count is replaced, by the median of the lane counts
/// the rest of the leg agreed on.
pub const LANE_CONTRADICTION_AADT: f64 = 40000.0;

/// Repair lane counts that disagree with the volume beside them.
pub fn screen_lane_contradictions(
    samples: Vec<TrafficVolumeSample>,
    from_city: &str,
    to_city: &str,
) -> Vec<TrafficVolumeSample> {
    let suspect: Vec<usize> = samples
        .iter()
        .enumerate()
        .filter(|(_, s)| s.lanes <= 1 && s.aadt >= LANE_CONTRADICTION_AADT)
        .map(|(i, _)| i)
        .collect();
    if suspect.is_empty() {
        return samples;
    }
    let mut trusted: Vec<i64> = samples
        .iter()
        .enumerate()
        .filter(|(i, _)| !suspect.contains(i))
        .map(|(_, s)| s.lanes)
        .collect();
    trusted.sort_unstable();
    // Nothing on the leg to learn from: two lanes is the shipped default for
    // a divided highway (DEFAULT_LEG_LANES), and any answer beats one.
    let replacement = if trusted.is_empty() {
        2
    } else {
        trusted[trusted.len() / 2]
    };
    let replacement = replacement.max(2);
    let mut repaired = samples;
    for i in suspect {
        let bad = &repaired[i];
        log::warn!(
            "{} to {}: traffic sample at mile {} claims {} lane(s) for {} AADT; \
             reading {} lanes from the rest of the leg instead",
            from_city,
            to_city,
            fmt_f(bad.at_mi, 1),
            bad.lanes,
            fmt_f(bad.aadt, 0),
            replacement
        );
        repaired[i].lanes = replacement;
    }
    repaired
}

/// The baked HPMS terrain class, or None where the bake has nothing.
///
/// Absence stays absence: a leg HPMS never classified must not be read as
/// level, because the curve screen treats level ground as license to remove
/// geometry.
pub fn parse_hpms_terrain(
    raw: Option<&Value>,
    from_city: &str,
    to_city: &str,
) -> Result<Option<HpmsTerrain>, DataError> {
    let Some(raw) = raw.filter(|v| super::py_truthy(v)) else {
        return Ok(None);
    };
    let Some(raw) = raw.as_object() else {
        return Err(DataError::value(format!(
            "{from_city} to {to_city} hpms_terrain must be an object"
        )));
    };
    let kind = raw.get("type").cloned().unwrap_or(Value::Null);
    let code = match &kind {
        Value::Number(n) => n.as_f64().filter(|f| [1.0, 2.0, 3.0].contains(f)),
        _ => None,
    };
    let Some(code) = code else {
        return Err(DataError::value(format!(
            "{from_city} to {to_city} hpms_terrain type {} is not 1, 2 or 3",
            py_repr_value(&kind)
        )));
    };
    let sections = match raw.get("sections") {
        Some(v) if super::py_truthy(v) => py_int_of(v)?,
        _ => 0,
    };
    Ok(Some(HpmsTerrain {
        terrain_type: code as i64,
        name: get_str_raw(raw, "name"),
        sections,
        source: get_str_raw(raw, "source"),
    }))
}

/// Parse the baked HPMS AADT profile, ordered along the leg.
pub fn parse_traffic_volumes(
    raw_samples: &[Value],
    leg_miles: f64,
    from_city: &str,
    to_city: &str,
) -> Result<Vec<TrafficVolumeSample>, DataError> {
    let samples: Vec<TrafficVolumeSample> = raw_samples
        .iter()
        .map(|s| parse_traffic_volume(s, leg_miles, from_city, to_city))
        .collect::<Result<_, _>>()?;
    let mut samples = screen_lane_contradictions(samples, from_city, to_city);
    samples.sort_by(|a, b| a.at_mi.partial_cmp(&b.at_mi).expect("finite at_mi"));
    Ok(samples)
}

/// Mirrors the bake-side filter in tools/enrich_routes_landmarks.py, plus the
/// hand-curated highway heritage markers ("the Loneliest Road in America") and
/// placed roadside billboards ("billboard_sign", baked by the billboard spider
/// at an attraction's real milepost); anything outside this set is a bake bug
/// and should fail the load loudly.
pub const LANDMARK_CATEGORIES: &[&str] = &[
    "national_park",
    "wilderness",
    "national_forest",
    "mountain_pass",
    "river",
    "museum",
    "protected_area",
    "highway_marker",
    "billboard_sign",
    "village",
];

pub fn parse_landmark(
    raw: &Value,
    leg_miles: f64,
    from_city: &str,
    to_city: &str,
) -> Result<Landmark, DataError> {
    let raw = object(raw, from_city, to_city, "landmark")?;
    let name = get_str(raw, "name");
    if name.is_empty() {
        return Err(DataError::value(format!(
            "{from_city} to {to_city} has a landmark without a name"
        )));
    }
    let rname = py_repr_str(&name);
    let at_mi = parse_at_mi(
        raw,
        leg_miles,
        from_city,
        to_city,
        &format!("landmark {rname}"),
        true,
    )?;
    let category = get_str(raw, "category");
    if !set_contains(LANDMARK_CATEGORIES, &category) {
        return Err(DataError::value(format!(
            "{from_city} to {to_city} landmark {rname} has unknown category {}",
            py_repr_str(&category)
        )));
    }
    let kind = get_str(raw, "kind");
    if !matches!(kind.as_str(), "zone" | "point") {
        return Err(DataError::value(format!(
            "{from_city} to {to_city} landmark {rname} has unknown kind {}",
            py_repr_str(&kind)
        )));
    }
    let spoken = get_str(raw, "spoken");
    if spoken.is_empty() {
        return Err(DataError::value(format!(
            "{from_city} to {to_city} landmark {rname} has no spoken line"
        )));
    }
    if exposes_raw(&format!("{name} {spoken}")) {
        return Err(DataError::value(format!(
            "{from_city} to {to_city} landmark {rname} exposes raw OSM/source text"
        )));
    }
    let off_mi = get_float(raw, "off_mi", 0.0).map_err(|_| {
        DataError::value(format!(
            "{from_city} to {to_city} landmark {rname} has a non-numeric off_mi"
        ))
    })?;
    if off_mi < 0.0 {
        return Err(DataError::value(format!(
            "{from_city} to {to_city} landmark {rname} has a negative off_mi"
        )));
    }
    // A billboard faces the traffic it was written for; see `Landmark`.
    let directions: Vec<String> = if raw.contains_key("directions") {
        get_str_list(raw, "directions")
    } else if category == "billboard_sign" {
        vec!["forward".to_string()]
    } else {
        vec!["both".to_string()]
    };
    if directions.is_empty()
        || directions.iter().any(|d| !set_contains(STOP_DIRECTIONS, d))
        || (directions.iter().any(|d| d == "both") && directions.len() > 1)
    {
        return Err(DataError::value(format!(
            "{from_city} to {to_city} landmark {rname} has invalid directions {}",
            py_repr_list(&directions)
        )));
    }
    Ok(Landmark {
        name,
        at_mi,
        category,
        kind,
        spoken,
        off_mi,
        directions,
    })
}

/// Parse the baked landmark list, ordered along the leg.
pub fn parse_landmarks(
    raw_landmarks: &[Value],
    leg_miles: f64,
    from_city: &str,
    to_city: &str,
) -> Result<Vec<Landmark>, DataError> {
    let mut landmarks: Vec<Landmark> = raw_landmarks
        .iter()
        .map(|x| parse_landmark(x, leg_miles, from_city, to_city))
        .collect::<Result<_, _>>()?;
    landmarks.sort_by(|a, b| a.at_mi.partial_cmp(&b.at_mi).expect("finite at_mi"));
    Ok(landmarks)
}

pub fn parse_state_crossing(
    raw: &Value,
    leg_miles: f64,
    from_city: &str,
    to_city: &str,
    default_from_state: &str,
) -> Result<StateCrossing, DataError> {
    let raw = object(raw, from_city, to_city, "state crossing")?;
    let at_mi = parse_at_mi(raw, leg_miles, from_city, to_city, "state crossing", false)?;
    let state = get_str(raw, "state");
    if state.is_empty() {
        return Err(DataError::value(format!(
            "{from_city} to {to_city} has a state crossing without a state"
        )));
    }
    let mut from_state = get_str(raw, "from_state");
    if from_state.is_empty() {
        from_state = default_from_state.to_string();
    }
    let mut place = get_str(raw, "place");
    if place.is_empty() {
        place = "state line".to_string();
    }
    Ok(StateCrossing {
        at_mi,
        from_state,
        state,
        place,
        source: get_str(raw, "source"),
    })
}

pub fn parse_checkpoint(
    raw: &Value,
    leg_miles: f64,
    from_city: &str,
    to_city: &str,
) -> Result<RouteCheckpoint, DataError> {
    let raw = object(raw, from_city, to_city, "checkpoint")?;
    let name = get_str(raw, "name");
    if name.is_empty() {
        return Err(DataError::value(format!(
            "{from_city} to {to_city} has a checkpoint without a name"
        )));
    }
    let at_mi = parse_at_mi(
        raw,
        leg_miles,
        from_city,
        to_city,
        &format!("checkpoint {}", py_repr_str(&name)),
        false,
    )?;
    let mut checkpoint_type = get_str(raw, "type");
    if checkpoint_type.is_empty() {
        checkpoint_type = "place".to_string();
    }
    Ok(RouteCheckpoint {
        name,
        at_mi,
        checkpoint_type,
        state: get_str(raw, "state"),
        highway: get_str(raw, "highway"),
        source: get_str(raw, "source"),
    })
}

pub fn parse_state_mileage(
    raw: &Value,
    from_city: &str,
    to_city: &str,
) -> Result<StateMileage, DataError> {
    let raw = object(raw, from_city, to_city, "state mileage")?;
    let state = get_str(raw, "state");
    if state.is_empty() {
        return Err(DataError::value(format!(
            "{from_city} to {to_city} has state mileage without a state"
        )));
    }
    let miles = req_float(raw, "miles")?;
    if miles <= 0.0 {
        return Err(DataError::value(format!(
            "{from_city} to {to_city} state mileage must be positive"
        )));
    }
    Ok(StateMileage { state, miles })
}

pub fn parse_toll_event(
    raw: &Value,
    leg_miles: f64,
    from_city: &str,
    to_city: &str,
    default_road: &str,
) -> Result<TollEvent, DataError> {
    let raw = object(raw, from_city, to_city, "toll event")?;
    let name = get_str(raw, "name");
    if name.is_empty() {
        return Err(DataError::value(format!(
            "{from_city} to {to_city} toll event has no name"
        )));
    }
    let rname = py_repr_str(&name);
    if exposes_raw(&name) {
        return Err(DataError::value(format!(
            "{from_city} to {to_city} toll event {rname} exposes raw OSM/source text"
        )));
    }
    let at_mi = parse_at_mi(
        raw,
        leg_miles,
        from_city,
        to_city,
        &format!("toll event {rname}"),
        false,
    )?;
    let mut road = get_str(raw, "road");
    if road.is_empty() {
        road = default_road.to_string();
    }
    let authority = get_str(raw, "authority");
    let method = get_str(raw, "method");
    let source = get_str(raw, "source");
    if authority.is_empty() {
        return Err(DataError::value(format!(
            "{from_city} to {to_city} toll event {rname} has no authority"
        )));
    }
    if lookup(TOLL_METHOD_LABELS, &method).is_none() {
        return Err(DataError::value(format!(
            "{from_city} to {to_city} toll event {rname} has unknown method {}",
            py_repr_str(&method)
        )));
    }
    let amount = req_float(raw, "amount")?;
    if !(0.0..=500.0).contains(&amount) {
        return Err(DataError::value(format!(
            "{from_city} to {to_city} toll event {rname} has invalid amount"
        )));
    }
    let amount_plate = get_float(raw, "amount_plate", 0.0)?;
    if !(0.0..=500.0).contains(&amount_plate) {
        return Err(DataError::value(format!(
            "{from_city} to {to_city} toll event {rname} has invalid amount_plate"
        )));
    }
    // Paying by plate is never MEANINGFULLY cheaper than running a transponder,
    // so a real gap the wrong way is a transcription error. A few cents is not:
    // the Indiana Toll Road's published table rounds cash a hair under E-ZPass
    // (91.30 against 91.37 full length), which is genuine and must parse.
    if 0.0 < amount_plate && amount_plate < amount - 0.25 {
        return Err(DataError::value(format!(
            "{from_city} to {to_city} toll event {rname} has a plate rate ({}) well below its transponder rate ({})",
            py_str_float(amount_plate),
            py_str_float(amount)
        )));
    }
    let directions: Vec<String> = if raw.contains_key("directions") {
        get_str_list(raw, "directions")
    } else {
        vec!["both".to_string()]
    };
    if directions.is_empty() {
        return Err(DataError::value(format!(
            "{from_city} to {to_city} toll event {rname} has no directions"
        )));
    }
    let unknown = sorted_unique(
        directions
            .iter()
            .filter(|d| !set_contains(STOP_DIRECTIONS, d)),
    );
    if !unknown.is_empty() {
        return Err(DataError::value(format!(
            "{from_city} to {to_city} toll event {rname} has unknown directions {}",
            py_repr_list(&unknown)
        )));
    }
    if directions.iter().any(|d| d == "both") && directions.len() > 1 {
        return Err(DataError::value(format!(
            "{from_city} to {to_city} toll event {rname} mixes 'both' with direction-specific applicability"
        )));
    }
    if source.is_empty() {
        return Err(DataError::value(format!(
            "{from_city} to {to_city} toll event {rname} has no source"
        )));
    }
    Ok(TollEvent {
        name,
        at_mi,
        road,
        authority,
        method,
        amount,
        estimated: get_bool(raw, "estimated", true),
        source,
        amount_plate,
        directions,
    })
}

pub fn parse_interchange(
    raw: &Value,
    leg_miles: f64,
    from_city: &str,
    to_city: &str,
    default_highway: &str,
) -> Result<Interchange, DataError> {
    let raw = object(raw, from_city, to_city, "interchange")?;
    // OSM exit refs occasionally carry stray internal spaces ("103 B"); a real
    // exit number never does, so collapse them ("103 B" -> "103B").
    let exit_ref: String = get_str(raw, "exit_ref")
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    let name = get_str(raw, "name");
    let via = get_str(raw, "via");
    let destinations: Vec<String> = match raw.get("destinations") {
        Some(Value::String(s)) => vec![s.trim().to_string()]
            .into_iter()
            .filter(|d| !d.is_empty())
            .collect(),
        Some(Value::Array(items)) => items
            .iter()
            .map(|item| py_str(item).trim().to_string())
            .filter(|d| !d.is_empty())
            .collect(),
        _ => Vec::new(),
    };
    let label = format!(
        "interchange {}",
        py_repr_str(if !exit_ref.is_empty() {
            &exit_ref
        } else if !name.is_empty() {
            &name
        } else {
            "(unnamed)"
        })
    );
    let at_mi = parse_at_mi(raw, leg_miles, from_city, to_city, &label, false)?;
    // An interchange must carry *something* sayable beyond a milepost.
    if exit_ref.is_empty() && destinations.is_empty() && name.is_empty() {
        return Err(DataError::value(format!(
            "{from_city} to {to_city} interchange at {} has no exit ref, destinations, or name",
            py_str_float(at_mi)
        )));
    }
    let mut blob_parts = vec![name.clone(), via.clone()];
    blob_parts.extend(destinations.iter().cloned());
    if exposes_raw(&blob_parts.join(" ")) {
        return Err(DataError::value(format!(
            "{from_city} to {to_city} {label} exposes raw OSM/source text"
        )));
    }
    let mut highway = get_str(raw, "highway");
    if highway.is_empty() {
        highway = default_highway.to_string();
    }
    let source = get_str(raw, "source");
    if source.is_empty() {
        return Err(DataError::value(format!(
            "{from_city} to {to_city} {label} has no source"
        )));
    }
    let ramp_control = get_str(raw, "ramp_control").to_lowercase();
    if !matches!(
        ramp_control.as_str(),
        "" | "signal" | "stop" | "yield" | "roundabout" | "none"
    ) {
        return Err(DataError::value(format!(
            "{from_city} to {to_city} {label} has unknown ramp_control"
        )));
    }
    let ramp_far_end = get_str(raw, "ramp_far_end").to_lowercase();
    if !matches!(ramp_far_end.as_str(), "" | "motorway" | "surface") {
        return Err(DataError::value(format!(
            "{from_city} to {to_city} {label} has unknown ramp_far_end"
        )));
    }
    let ramp_advisory_mph_forward = parse_advisory_field(raw, "ramp_advisory_forward");
    let ramp_advisory_mph_backward = parse_advisory_field(raw, "ramp_advisory_backward");
    let ramp_advisory_source = get_str(raw, "ramp_advisory_source");
    if (ramp_advisory_mph_forward.is_some() || ramp_advisory_mph_backward.is_some())
        && ramp_advisory_source.is_empty()
    {
        return Err(DataError::value(format!(
            "{from_city} to {to_city} {label} has an observed ramp advisory without a source"
        )));
    }
    let ramp_length_ft = |key: &str| {
        raw.get(key)
            .and_then(Value::as_f64)
            .filter(|ft| ft.is_finite() && *ft > 0.0)
    };
    let ramp_length_ft_forward = ramp_length_ft("ramp_length_ft_forward");
    let ramp_length_ft_backward = ramp_length_ft("ramp_length_ft_backward");
    let ramp_length_source = get_str(raw, "ramp_length_source");
    if (ramp_length_ft_forward.is_some() || ramp_length_ft_backward.is_some())
        && ramp_length_source.is_empty()
    {
        return Err(DataError::value(format!(
            "{from_city} to {to_city} {label} has a ramp length without a source"
        )));
    }
    let ramp_terminal_node = |key: &str| {
        raw.get(key)
            .and_then(|terminal| terminal.get("node"))
            .and_then(Value::as_i64)
            .filter(|node| *node > 0)
    };
    let ramp_terminal_node_forward = ramp_terminal_node("ramp_terminal_forward");
    let ramp_terminal_node_backward = ramp_terminal_node("ramp_terminal_backward");
    let ramp_terminal_source = get_str(raw, "ramp_terminal_source");
    if (ramp_terminal_node_forward.is_some() || ramp_terminal_node_backward.is_some())
        && ramp_terminal_source.is_empty()
    {
        return Err(DataError::value(format!(
            "{from_city} to {to_city} {label} has a ramp terminal without a source"
        )));
    }
    Ok(Interchange {
        at_mi,
        exit_ref,
        name,
        destinations,
        via,
        highway,
        source,
        ramp_control,
        ramp_far_end,
        ramp_advisory_mph_forward,
        ramp_advisory_mph_backward,
        ramp_advisory_source,
        ramp_length_ft_forward,
        ramp_length_ft_backward,
        ramp_length_source,
        ramp_terminal_node_forward,
        ramp_terminal_node_backward,
        ramp_terminal_source,
    })
}

/// Normalize an OSM advisory speed. Bare values and metric suffixes are km/h,
/// as required by OSM's maxspeed value syntax; an explicit mph suffix is kept.
pub fn parse_osm_advisory_speed(value: &str) -> Option<f64> {
    let value = value.trim().to_lowercase();
    let (number, factor) = if let Some(v) = value.strip_suffix("mph") {
        (v.trim(), 1.0)
    } else if let Some(v) = value.strip_suffix("km/h") {
        (v.trim(), 0.621_371_192)
    } else if let Some(v) = value.strip_suffix("kmh") {
        (v.trim(), 0.621_371_192)
    } else if let Some(v) = value.strip_suffix("kph") {
        (v.trim(), 0.621_371_192)
    } else {
        (value.as_str(), 0.621_371_192)
    };
    let speed = number.parse::<f64>().ok()? * factor;
    (speed.is_finite() && (5.0..=100.0).contains(&speed)).then_some(speed)
}

/// Read the advisory applying OSM way direction before the generic value.
/// Callers must positively identify a motorway/trunk ramp; ordinary-road
/// advisory tags belong to the curve layer, not this first ramp slice.
pub fn ramp_advisory_from_osm_tags(
    tags: &Map<String, Value>,
    traversal_forward: bool,
    is_ramp: bool,
) -> Option<f64> {
    if !is_ramp {
        return None;
    }
    let directional = if traversal_forward {
        "maxspeed:advisory:forward"
    } else {
        "maxspeed:advisory:backward"
    };
    [directional, "maxspeed:advisory"]
        .into_iter()
        .find_map(|key| tags.get(key)?.as_str().and_then(parse_osm_advisory_speed))
}

fn parse_advisory_field(raw: &Map<String, Value>, key: &str) -> Option<f64> {
    raw.get(key).and_then(|value| match value {
        Value::String(text) => parse_osm_advisory_speed(text),
        Value::Number(number) => number
            .as_f64()
            .filter(|speed| speed.is_finite() && (5.0..=100.0).contains(speed))
            .map(super::advisory::screen_bare_kmh_advisory),
        _ => None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn lane_contradiction_is_repaired_from_the_rest_of_the_leg() {
        let samples = vec![
            TrafficVolumeSample {
                at_mi: 0.0,
                aadt: 69000.0,
                lanes: 1,
                source: String::new(),
            },
            TrafficVolumeSample {
                at_mi: 5.0,
                aadt: 60000.0,
                lanes: 3,
                source: String::new(),
            },
            TrafficVolumeSample {
                at_mi: 10.0,
                aadt: 55000.0,
                lanes: 3,
                source: String::new(),
            },
        ];
        let repaired = screen_lane_contradictions(samples, "a", "b");
        assert_eq!(repaired[0].lanes, 3);
        assert_eq!(repaired[0].aadt, 69000.0);
    }

    #[test]
    fn hpms_terrain_absence_stays_absence() {
        assert!(parse_hpms_terrain(None, "a", "b").unwrap().is_none());
        assert!(parse_hpms_terrain(Some(&json!({})), "a", "b")
            .unwrap()
            .is_none());
        let err = parse_hpms_terrain(Some(&json!({"type": 4})), "a", "b").unwrap_err();
        assert_eq!(
            err.to_string(),
            "a to b hpms_terrain type 4 is not 1, 2 or 3"
        );
        let level = parse_hpms_terrain(Some(&json!({"type": 1, "name": "level"})), "a", "b")
            .unwrap()
            .unwrap();
        assert_eq!(level.terrain_type, 1);
    }

    #[test]
    fn interchange_messages_match_python() {
        let err = parse_interchange(&json!({"at_mi": 3.0}), 10.0, "a", "b", "I-1").unwrap_err();
        assert_eq!(
            err.to_string(),
            "a to b interchange at 3.0 has no exit ref, destinations, or name"
        );
        let exit = parse_interchange(
            &json!({"at_mi": 3.0, "exit_ref": "103 B", "source": "x", "destinations": "Trenton"}),
            10.0,
            "a",
            "b",
            "I-1",
        )
        .unwrap();
        assert_eq!(exit.exit_ref, "103B");
        assert_eq!(exit.destinations, vec!["Trenton".to_string()]);
        assert_eq!(exit.highway, "I-1");
    }
}
