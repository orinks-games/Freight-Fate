//! A rest area on the far side of a divided highway.
//!
//! The truck-parking inventory (FHWA Jason's Law, via BTS NTAD) lists each
//! carriageway's facility on its own, with the signed bound in the record:
//! "Rest Area #52 on I-43N", "Rest Area #51 on I-43S". The import read the
//! bound into the source note and stored `directions: both` on each, so a
//! northbound drive from Sheboygan to Green Bay offered both: "Rest Area #52
//! in 5 miles", then three seconds later "Rest Area #51 in 4 miles", the
//! second one across the median (Orinks's drive, 2026-11-04).
//!
//! This screens the record at load and never edits the bake: the stored
//! `both` stays in the data, so the rule can be re-judged.
//!
//! # The rule
//!
//! A stop whose `directions` is `both` and whose source note reads a signed
//! bound ("on I-43N", "on I-74WB") serves one direction of the leg: forward
//! when the leg runs that way, reverse when it runs the other. Which way the
//! leg runs is DERIVED from its route points: the component of the leg's
//! heading along the bound's axis (north for N, east for E, and their
//! negatives), read twice -- across [`NEAR_STOP_MILES`] either side of the
//! stop, and from the leg's first point to its last. The two must agree in
//! sign, or the stop keeps `both`: a stop where the road near it and the leg
//! as a whole disagree sits at a junction or on a road the leg only touches,
//! and offering a stop the driver cannot reach is a smaller loss than hiding
//! one they can.
//!
//! # Where ten miles comes from
//!
//! The route points are a simplified line: across the map they are a median
//! of 25 miles apart (tenth percentile 11). A window much under that reads a
//! single straight segment that may not be the road beside the stop; much
//! over it reads the leg as a whole, which the second reading already does.
//!
//! Calibrated against the map, 2026-11-04: 92 stops carry a signed bound in
//! their source. With the window at ten miles, 88 resolve and 4 keep `both`
//! (Big Cypress on Alligator Alley, signed south while it runs east; Thompson
//! Hill and Culkin on I-35 at the start of the Duluth to Fargo leg; the Nash
//! County rest area where Norfolk to Raleigh leaves I-95). The four records
//! the data already marks one way (the Darien and Richard Stockton service
//! plazas) read the same way under the rule.

use once_cell::sync::Lazy;
use regex::Regex;
use serde_json::Value;

use crate::data::world_corridor::corridor_map;
use crate::data::world_models::Stop;
use crate::data::world_parsing::list_field;

/// Half-width of the window the heading near a stop is read over.
pub const NEAR_STOP_MILES: f64 = 10.0;

/// "on I-43N", "on I-74 WB", "on US-12E,": the signed bound of a carriageway.
static SIGNED_BOUND: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"\bon (?:I|US)-\d+ ?([NSEW])B?\b").expect("signed-bound pattern compiles")
});

/// The bound a stop's source note reads, if it reads one.
pub fn signed_bound(source: &str) -> Option<char> {
    SIGNED_BOUND
        .captures(source)
        .and_then(|caps| caps.get(1))
        .and_then(|m| m.as_str().chars().next())
}

#[derive(Debug, Clone, Copy)]
struct Point {
    at_mi: f64,
    lat: f64,
    lon: f64,
}

fn route_points(corridor: &Value) -> Vec<Point> {
    let field = |raw: &Value, key: &str| raw.get(key).and_then(Value::as_f64);
    let mut points: Vec<Point> = list_field(corridor_map(corridor), "route_points")
        .iter()
        .filter_map(|raw| {
            Some(Point {
                at_mi: field(raw, "at_mi")?,
                lat: field(raw, "lat")?,
                lon: field(raw, "lon")?,
            })
        })
        .collect();
    points.sort_by(|a, b| a.at_mi.total_cmp(&b.at_mi));
    points
}

/// Where the route line is at `at_mi`, clamped to its ends.
fn position(points: &[Point], at_mi: f64) -> (f64, f64) {
    let (first, last) = (points[0], points[points.len() - 1]);
    if at_mi <= first.at_mi {
        return (first.lat, first.lon);
    }
    for pair in points.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        if at_mi <= b.at_mi {
            let span = b.at_mi - a.at_mi;
            let t = if span > 0.0 {
                (at_mi - a.at_mi) / span
            } else {
                0.0
            };
            return (a.lat + t * (b.lat - a.lat), a.lon + t * (b.lon - a.lon));
        }
    }
    (last.lat, last.lon)
}

/// How far the line runs in the bound's direction between two miles, in
/// degrees of latitude (east-west scaled by the cosine of the latitude).
/// Positive when the leg runs the signed way.
fn along_bound(points: &[Point], from_mi: f64, to_mi: f64, bound: char) -> f64 {
    let (lat_a, lon_a) = position(points, from_mi);
    let (lat_b, lon_b) = position(points, to_mi);
    let north = lat_b - lat_a;
    let east = (lon_b - lon_a) * lat_a.to_radians().cos();
    match bound {
        'N' => north,
        'S' => -north,
        'E' => east,
        _ => -east,
    }
}

/// Whether a stop signed `bound` at `at_mi` serves the leg's forward
/// direction; None when the two readings disagree or there is no line.
fn serves_forward(points: &[Point], at_mi: f64, bound: char) -> Option<bool> {
    if points.len() < 2 {
        return None;
    }
    let near = along_bound(
        points,
        at_mi - NEAR_STOP_MILES,
        at_mi + NEAR_STOP_MILES,
        bound,
    );
    let whole = along_bound(
        points,
        points[0].at_mi,
        points[points.len() - 1].at_mi,
        bound,
    );
    if near == 0.0 || whole == 0.0 || (near > 0.0) != (whole > 0.0) {
        return None;
    }
    Some(near > 0.0)
}

/// A leg's stops with each signed-bound `both` record narrowed to the
/// direction it serves.
pub fn screen_stop_bounds(mut stops: Vec<Stop>, corridor: &Value) -> Vec<Stop> {
    let mut points: Option<Vec<Point>> = None;
    for stop in &mut stops {
        if stop.directions != ["both"] {
            continue;
        }
        let Some(bound) = signed_bound(&stop.source) else {
            continue;
        };
        let points = points.get_or_insert_with(|| route_points(corridor));
        let Some(forward) = serves_forward(points, stop.at_mi, bound) else {
            log::debug!(
                "stop bound unresolved: {} at mile {} is signed {bound}",
                stop.name,
                stop.at_mi
            );
            continue;
        };
        let direction = if forward { "forward" } else { "reverse" };
        stop.directions = vec![direction.to_string()];
    }
    stops
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn stop(name: &str, at_mi: f64, source: &str) -> Stop {
        Stop {
            name: name.to_string(),
            at_mi,
            source: source.to_string(),
            directions: vec!["both".to_string()],
            ..Stop::default()
        }
    }

    /// Sheboygan to Green Bay on I-43, as shipped: the leg runs north.
    fn sheboygan_to_green_bay() -> Value {
        json!({"route_points": [
            {"at_mi": 0.0, "lat": 43.75088, "lon": -87.7145},
            {"at_mi": 16.5, "lat": 43.96794, "lon": -87.73614},
            {"at_mi": 33.0, "lat": 44.18, "lon": -87.75},
            {"at_mi": 49.5, "lat": 44.33, "lon": -87.86},
            {"at_mi": 66.0, "lat": 44.51, "lon": -88.02},
        ]})
    }

    fn directions(stops: &[Stop]) -> Vec<(&str, &str)> {
        stops
            .iter()
            .map(|s| (s.name.as_str(), s.directions[0].as_str()))
            .collect()
    }

    #[test]
    fn test_the_northbound_drive_keeps_only_the_northbound_rest_area() {
        let stops = vec![
            stop(
                "Rest Area #52",
                44.0,
                "FHWA Jason's Law truck parking inventory (NTAD via BTS) lists \
                 Rest Area #52 on I-43N in Denmark, WI with 20 truck parking spaces",
            ),
            stop(
                "Rest Area #51",
                44.2,
                "FHWA Jason's Law truck parking inventory (NTAD via BTS) lists \
                 Rest Area #51 on I-43S in Maribel, WI with 20 truck parking spaces",
            ),
        ];
        let screened = screen_stop_bounds(stops, &sheboygan_to_green_bay());
        assert_eq!(
            directions(&screened),
            [("Rest Area #52", "forward"), ("Rest Area #51", "reverse")]
        );
        assert!(screened[0].applies_to_direction(true));
        assert!(!screened[1].applies_to_direction(true));
        assert!(screened[1].applies_to_direction(false));
    }

    #[test]
    fn test_a_bound_is_read_with_or_without_the_b_and_the_space() {
        assert_eq!(
            signed_bound("lists Spoon River EB on I-74EB in IL"),
            Some('E')
        );
        assert_eq!(
            signed_bound("lists Bliss W on I-84W in Bliss, ID"),
            Some('W')
        );
        assert_eq!(signed_bound("on I-95 N near the line"), Some('N'));
        assert_eq!(signed_bound("on I-43 in Denmark"), None);
        assert_eq!(signed_bound("OpenStreetMap amenity query"), None);
    }

    #[test]
    fn test_a_stop_where_the_road_and_the_leg_disagree_keeps_both() {
        // The leg runs north overall, but the ten miles round the stop run
        // south: a junction, not a reading.
        let corridor = json!({"route_points": [
            {"at_mi": 0.0, "lat": 44.0, "lon": -88.0},
            {"at_mi": 30.0, "lat": 44.4, "lon": -88.0},
            {"at_mi": 60.0, "lat": 44.1, "lon": -88.0},
            {"at_mi": 90.0, "lat": 45.0, "lon": -88.0},
        ]});
        let stops = vec![stop("Rest Area", 45.0, "lists Rest Area on I-39N")];
        let screened = screen_stop_bounds(stops, &corridor);
        assert_eq!(screened[0].directions, ["both"]);
    }

    #[test]
    fn test_recorded_directions_and_unsigned_stops_are_left_alone() {
        let mut recorded = stop("Darien Service Plaza", 39.7, "lists it on I-95N");
        recorded.directions = vec!["forward".to_string()];
        let unsigned = stop("Kwik Trip", 1.0, "OpenStreetMap amenity query");
        let screened = screen_stop_bounds(vec![recorded, unsigned], &sheboygan_to_green_bay());
        assert_eq!(
            directions(&screened),
            [("Darien Service Plaza", "forward"), ("Kwik Trip", "both")]
        );
    }

    #[test]
    fn test_a_leg_with_no_route_line_keeps_both() {
        let stops = vec![stop("Rest Area #52", 44.0, "lists it on I-43N")];
        let screened = screen_stop_bounds(stops, &Value::Null);
        assert_eq!(screened[0].directions, ["both"]);
    }
}
