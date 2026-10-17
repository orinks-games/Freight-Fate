//! Lazy per-leg corridor parsing for the world data layer (port of
//! `freight_fate/data/world_corridor.py`).
//!
//! `World` used to build every leg's heavy per-mile detail (grades,
//! interchanges, landmarks, posted limits, state crossings, ...) for all fifty
//! states at startup, even though the route graph and dispatch never read any
//! of it -- roughly a second of pure latency before the menu. `Leg` defers
//! that work to the first time a leg is driven; this module holds the two
//! pieces of that deferral that lean on the parse helpers:
//!
//! * [`raw_metadata_complete`] -- `Leg::metadata_complete` computed from raw
//!   corridor counts, so the route graph can gate dispatch without a parse.
//! * [`build_leg_corridor`] -- the full parse of a single leg's corridor,
//!   byte-identical to the old eager construction.
//!
//! Kept separate from `world_parsing` only to keep that module under the
//! 1000-line ceiling; it depends on the `parse_*` helpers that still live
//! there and that tests import from there.

use serde_json::{Map, Value};

use super::grades::screen_grade_segments;
use super::world_constants::{LIMIT_ARTIFACT_PLACE_CHECKPOINTS, LIMIT_EXPLAINING_CATEGORIES};
use super::world_models::{CorridorDetail, DataError};
use super::world_parsing::{
    fold_limit_artifacts, list_field, parse_billboard_bans, parse_checkpoint,
    parse_elevation_sample, parse_grade_segment, parse_hpms_terrain, parse_interchange,
    parse_landmarks, parse_lane_segments, parse_restrictions, parse_route_point,
    parse_speed_limits, parse_state_crossing, parse_state_mileage, parse_toll_event,
    parse_traffic_volumes, py_truthy, FlaggedLimit,
};

static EMPTY: once_cell::sync::Lazy<Map<String, Value>> = once_cell::sync::Lazy::new(Map::new);

/// The object behind a corridor value; anything else reads as empty, as
/// Python's `leg.get("corridor", {})` followed by `.get(...)` did.
pub fn corridor_map(corridor: &Value) -> &Map<String, Value> {
    corridor.as_object().unwrap_or(&EMPTY)
}

fn list_len(corridor: &Map<String, Value>, key: &str) -> usize {
    match corridor.get(key) {
        Some(Value::Array(items)) => items.len(),
        Some(Value::Object(map)) => map.len(),
        Some(Value::String(s)) => s.chars().count(),
        _ => 0,
    }
}

/// `Leg::metadata_complete` computed from raw corridor counts.
///
/// The five fields dispatch gates on (route points, elevation samples, grade
/// segments, state miles, state crossings) all parse one-for-one with no
/// filtering, so counting the raw lists is byte-identical to constructing the
/// leg and asking. That lets startup bake this flag without parsing the heavy
/// per-mile detail, which the lazy leg then defers until a leg is driven.
pub fn raw_metadata_complete(corridor: &Value, from_state: &str, to_state: &str) -> bool {
    let corridor = corridor_map(corridor);
    if list_len(corridor, "route_points") < 2 {
        return false;
    }
    if !corridor.get("state_miles").is_some_and(py_truthy) {
        return false;
    }
    if list_len(corridor, "elevation_samples") < 2
        || !corridor.get("grade_segments").is_some_and(py_truthy)
    {
        return false;
    }
    from_state == to_state || corridor.get("state_crossings").is_some_and(py_truthy)
}

/// Parse a leg's heavy per-mile corridor detail into model records.
///
/// Split out of the world constructor so a lazy leg can defer this work
/// until a leg is actually driven: the route graph and dispatch never read
/// these fields, so parsing all fifty states' worth at startup was pure
/// latency. The result is byte-identical to the old eager construction --
/// same inputs, same parse, same order.
///
/// `flagged_limits` are this leg's rows of `limit_artifacts.jsonl`: postings
/// the offline screen found off the leg's own highway, folded here unless a
/// place explains them (see [`fold_limit_artifacts`]).
pub fn build_leg_corridor(
    corridor: &Value,
    miles: f64,
    leg_from: &str,
    leg_to: &str,
    from_state: &str,
    highway: &str,
    flagged_limits: &[FlaggedLimit],
) -> Result<CorridorDetail, DataError> {
    let corridor = corridor_map(corridor);
    let route_points = list_field(corridor, "route_points")
        .iter()
        .map(|p| parse_route_point(p, miles, leg_from, leg_to))
        .collect::<Result<Vec<_>, _>>()?;
    let elevation_samples = list_field(corridor, "elevation_samples")
        .iter()
        .map(|s| parse_elevation_sample(s, miles, leg_from, leg_to))
        .collect::<Result<Vec<_>, _>>()?;
    // Screened on the way in: the elevation profile the grades were baked from
    // reads bridge decks as road, which put slopes on the map that no highway
    // of their class can hold. See `grades` -- the bake itself is left
    // untouched and a capped segment says so in its own source string.
    let hpms_terrain = parse_hpms_terrain(corridor.get("hpms_terrain"), leg_from, leg_to)?;
    let grade_segments = screen_grade_segments(
        &list_field(corridor, "grade_segments")
            .iter()
            .map(|s| parse_grade_segment(s, miles, leg_from, leg_to))
            .collect::<Result<Vec<_>, _>>()?,
        highway,
        hpms_terrain.as_ref().map(|t| t.terrain_type),
    );
    let lane_segments = parse_lane_segments(
        list_field(corridor, "lane_segments"),
        miles,
        leg_from,
        leg_to,
    )?;
    let state_crossings = list_field(corridor, "state_crossings")
        .iter()
        .map(|c| parse_state_crossing(c, miles, leg_from, leg_to, from_state))
        .collect::<Result<Vec<_>, _>>()?;
    let checkpoints = list_field(corridor, "checkpoints")
        .iter()
        .map(|c| parse_checkpoint(c, miles, leg_from, leg_to))
        .collect::<Result<Vec<_>, _>>()?;
    let state_miles = list_field(corridor, "state_miles")
        .iter()
        .map(|m| parse_state_mileage(m, leg_from, leg_to))
        .collect::<Result<Vec<_>, _>>()?;
    let toll_events = list_field(corridor, "toll_events")
        .iter()
        .map(|e| parse_toll_event(e, miles, leg_from, leg_to, highway))
        .collect::<Result<Vec<_>, _>>()?;
    let interchanges = list_field(corridor, "interchanges")
        .iter()
        .map(|x| parse_interchange(x, miles, leg_from, leg_to, highway))
        .collect::<Result<Vec<_>, _>>()?;
    let traffic_volumes = parse_traffic_volumes(
        list_field(corridor, "traffic_aadt"),
        miles,
        leg_from,
        leg_to,
    )?;
    let landmarks = parse_landmarks(list_field(corridor, "landmarks"), miles, leg_from, leg_to)?;
    // Landmarks first: the dwell filter keeps a short posting that a place on
    // the road explains, and drops the ones nothing does.
    let places: Vec<f64> = landmarks
        .iter()
        .filter(|lm| LIMIT_EXPLAINING_CATEGORIES.contains(&lm.category.as_str()))
        .map(|lm| lm.at_mi)
        .collect();
    let speed_limits = parse_speed_limits(
        list_field(corridor, "speed_limits"),
        miles,
        leg_from,
        leg_to,
        &places,
    )?;
    // The fold's guard is wider than the dwell filter's: a town checkpoint is
    // as good a reason for a short posting as a village landmark.
    let guard_places: Vec<f64> = places
        .iter()
        .copied()
        .chain(
            checkpoints
                .iter()
                .filter(|cp| {
                    LIMIT_ARTIFACT_PLACE_CHECKPOINTS.contains(&cp.checkpoint_type.as_str())
                })
                .map(|cp| cp.at_mi),
        )
        .collect();
    let (speed_limits, speed_limits_folded) =
        fold_limit_artifacts(speed_limits, flagged_limits, &guard_places);
    let restrictions = parse_restrictions(
        list_field(corridor, "restrictions"),
        miles,
        leg_from,
        leg_to,
    )?;
    let billboard_bans = parse_billboard_bans(
        list_field(corridor, "billboard_bans"),
        miles,
        leg_from,
        leg_to,
    )?;
    Ok(CorridorDetail {
        route_points,
        elevation_samples,
        grade_segments,
        state_crossings,
        checkpoints,
        state_miles,
        toll_events,
        interchanges,
        speed_limits,
        speed_limits_folded,
        traffic_volumes,
        hpms_terrain,
        landmarks,
        restrictions,
        lane_segments,
        billboard_bans,
    })
}
