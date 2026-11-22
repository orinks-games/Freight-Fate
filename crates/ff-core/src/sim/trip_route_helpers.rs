//! Small route-geometry helpers shared by the trip and its former mixins
//! (port of `freight_fate/sim/trip_route_helpers.py`).

use std::f64::consts::PI;

use regex::Regex;

use crate::data::world::World;
use crate::data::world_models::{Interchange, Leg, RoutePoint, Stop};
use crate::pyfmt::fmt_f;
use crate::sim::trip_models::Zone;

/// Miles per degree of latitude on the 3,956-mile sphere
/// [`haversine_distance_mi`] measures.
const MILES_PER_DEGREE: f64 = 3956.0 * PI / 180.0;

/// A leg-native (A-to-B) milepost as an offset in the direction of travel.
pub fn stop_offset_for_direction(at_mi: f64, leg_miles: f64, forward: bool) -> f64 {
    if forward {
        at_mi
    } else {
        leg_miles - at_mi
    }
}

/// Signed heading for onramp/merge framing ("Merge onto I-95 South...").
///
/// Uses the US route-numbering convention -- odd routes are signed
/// north/south, even routes east/west -- so the spoken direction matches
/// real signage even where a leg runs diagonally. The sign comes from the
/// endpoints' coordinates on the route's primary axis. A leg wholly in
/// Canada is signed by the axis it runs along instead. Empty when the
/// highway has no number or a city lacks coordinates.
pub fn leg_heading(world: &World, highway: &str, from_city: &str, to_city: &str) -> &'static str {
    let digits = Regex::new(r"\d+").expect("static regex");
    let Some(m) = digits.find(highway) else {
        return "";
    };
    let (Some(a), Some(b)) = (world.cities.get(from_city), world.cities.get(to_city)) else {
        return "";
    };
    if a.lat == 0.0 && a.lon == 0.0 {
        return "";
    }
    // Canada signs the way the road runs, not by the number's parity:
    // Highway 400 is north and south, the 401 east and west, and the
    // Trans-Canada's Highway 1 east and west through BC. A leg wholly in
    // Canada takes the axis its ends lie along. (Quebec's autoroutes keep
    // parity, A-20 east and A-55 north, and the axis agrees with them.)
    if a.country == "CA" && b.country == "CA" {
        let north = b.lat - a.lat;
        let east = (b.lon - a.lon) * ((a.lat + b.lat) / 2.0).to_radians().cos();
        return if north.abs() >= east.abs() {
            if north >= 0.0 {
                "North"
            } else {
                "South"
            }
        } else if east >= 0.0 {
            "East"
        } else {
            "West"
        };
    }
    // Python `int(match.group()) % 2`: a very long digit run still parses
    // there; here a parse failure reads as even, which no real shield hits.
    let number: u128 = m.as_str().parse().unwrap_or(0);
    if number % 2 == 1 {
        // odd -> north/south route
        return if b.lat >= a.lat { "North" } else { "South" };
    }
    if b.lon >= a.lon {
        "East"
    } else {
        "West"
    } // even -> east/west route
}

/// Signed exit label of the interchange nearest a stop on the same leg, in
/// the leg's native (a->b) frame. Empty when none is within `tol_mi` or the
/// nearest junction carries no exit number.
pub fn nearest_exit_label(leg: &Leg, at_mi: f64, tol_mi: f64) -> String {
    let mut best_label = String::new();
    let mut best_dist = tol_mi;
    for ix in leg.interchanges() {
        let dist = (ix.at_mi - at_mi).abs();
        let label = ix.exit_label();
        if dist <= best_dist && !label.is_empty() {
            best_dist = dist;
            best_label = label;
        }
    }
    best_label
}

/// How close two mile markers must be to be the same record. A stop's
/// `interchange_mi` is a copy of its interchange's `at_mi`, so the two differ
/// by float arithmetic on the way to a route mile and by nothing else.
pub const INTERCHANGE_IDENTITY_MI: f64 = 1e-6;

/// The interchange record a stop was matched to at bake time, in the leg's
/// native frame. None when the stop carries no match or the record is gone.
pub fn served_interchange<'a>(leg: &'a Leg, stop: &Stop) -> Option<&'a Interchange> {
    let mi = stop.interchange_mi?;
    leg.interchanges()
        .iter()
        .find(|ix| (ix.at_mi - mi).abs() <= INTERCHANGE_IDENTITY_MI)
}

/// A stop's signed exit label. The interchange decided at bake time answers
/// first, then the stop's own recorded exit number; only a stop with neither
/// falls back to the nearest numbered interchange within `tol_mi`. Where
/// the exit is known that search named another one 464 times in 1,086
/// (measured by `tools/snap_stops_to_interchanges.py`, 2026-09-17).
pub fn stop_exit_label(leg: &Leg, stop: &Stop, tol_mi: f64) -> String {
    let served = served_interchange(leg, stop)
        .map(Interchange::exit_label)
        .unwrap_or_default();
    if !served.is_empty() {
        return served;
    }
    if !stop.exit_ref.is_empty() {
        return format!("exit {}", stop.exit_ref);
    }
    nearest_exit_label(leg, stop.at_mi, tol_mi)
}

/// Keyed by place and reason only: a congestion zone's limit_mph is the live
/// traffic speed and changes with the clock, and a re-keyed zone would
/// re-announce itself every time the jam deepened a notch.
pub fn zone_key(zone: &Zone) -> String {
    format!(
        "{}:{}:{}",
        zone.reason,
        fmt_f(zone.start_mi, 3),
        fmt_f(zone.end_mi, 3)
    )
}

/// Auditable fallback for legs without elevation samples: flat roads stay
/// level, hills and mountains get a small deterministic profile from the
/// curated terrain label.
pub fn fallback_grade(terrain: &str, mile: f64, highway: &str) -> f64 {
    let amplitude = match terrain {
        "flat" => 0.0,
        "hills" => 0.012,
        "mountain" => 0.035,
        _ => 0.0,
    };
    if amplitude == 0.0 {
        return 0.0;
    }
    let wavelength = match terrain {
        "hills" => 14.0,
        "mountain" => 8.0,
        _ => 16.0,
    };
    let code_sum: u64 = highway.chars().map(|ch| ch as u64).sum();
    let phase = (code_sum % 628) as f64 / 100.0;
    amplitude * (2.0 * PI * mile / wavelength + phase).sin()
}

/// Every 511 state key a leg is looked up under, in the order the truck
/// drives through them: a leg that crosses a state line runs in both
/// states, and each state's feed carries only its own roadwork. Lower-cased
/// the way the provider keys its cache; empty where the bake is silent.
pub fn leg_states(leg: &Leg, forward: bool) -> Vec<String> {
    let crossings = leg.state_crossings();
    let mut states: Vec<&str> = match crossings.first() {
        Some(first) => std::iter::once(first.from_state.as_str())
            .chain(crossings.iter().map(|c| c.state.as_str()))
            .collect(),
        None => leg.state_miles().iter().map(|m| m.state.as_str()).collect(),
    };
    if !forward {
        states.reverse();
    }
    let mut keys: Vec<String> = Vec::new();
    for state in states {
        let key = state.trim().to_lowercase();
        if !key.is_empty() && !keys.contains(&key) {
            keys.push(key);
        }
    }
    keys
}

/// A leg's road as (lat, lon) points no more than a mile apart: its route
/// points with each chord between two of them filled in, the straight line
/// `Trip::leg_latlon_at` puts the truck on. The baked points run about 25
/// miles apart (the median gap in California), so a search around them
/// alone misses most of the road between.
pub fn leg_track(leg: &Leg) -> Vec<(f64, f64)> {
    let points = leg.route_points();
    let mut track = Vec::new();
    for pair in points.windows(2) {
        let (a, b) = (&pair[0], &pair[1]);
        let steps = haversine_distance_mi(a.lat, a.lon, b.lat, b.lon)
            .ceil()
            .max(1.0) as usize;
        for k in 0..steps {
            let t = k as f64 / steps as f64;
            track.push((a.lat + t * (b.lat - a.lat), a.lon + t * (b.lon - a.lon)));
        }
    }
    track.extend(points.last().map(|p| (p.lat, p.lon)));
    track
}

/// Where a coordinate meets a leg's road, from [`snap_to_leg`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LegSnap {
    /// The trip-absolute milepost.
    pub mile: f64,
    /// How far off the road the coordinate lies, in miles.
    pub off_road_mi: f64,
    /// The road's compass bearing there in the direction of travel, degrees
    /// clockwise from north; NaN on a leg of one point, which has none.
    pub bearing_deg: f64,
}

/// Snap a (lat, lon) coordinate onto the nearest point of the line through
/// a leg's route points. None when the leg has no route points or the
/// coordinate is more than 2 miles off that line (the construction event is
/// on a cross street, not the highway itself).
pub fn snap_to_leg(
    lat: f64,
    lon: f64,
    leg: &Leg,
    forward: bool,
    leg_start_mi: f64,
) -> Option<LegSnap> {
    let points = leg.route_points();
    // Flat-earth about the coordinate, the way `caltrans` measures a county
    // edge: a few feet out at the 2-mile reach that counts.
    let kx = lat.to_radians().cos() * MILES_PER_DEGREE;
    let local = |p: &RoutePoint| ((p.lon - lon) * kx, (p.lat - lat) * MILES_PER_DEGREE);
    let lone = points.first().filter(|_| points.len() == 1).map(|p| (p, p));
    // (leg-native mile, miles off the road, native bearing)
    let mut best: Option<(f64, f64, f64)> = None;
    for (a, b) in points.windows(2).map(|w| (&w[0], &w[1])).chain(lone) {
        let ((ax, ay), (bx, by)) = (local(a), local(b));
        let (dx, dy) = (bx - ax, by - ay);
        let length = dx * dx + dy * dy;
        let (t, bearing) = if length > 0.0 {
            (
                (-(ax * dx + ay * dy) / length).clamp(0.0, 1.0),
                dx.atan2(dy).to_degrees(),
            )
        } else {
            (0.0, f64::NAN)
        };
        let off = (ax + t * dx).hypot(ay + t * dy);
        if off < best.map_or(f64::INFINITY, |(_, d, _)| d) {
            best = Some((a.at_mi + t * (b.at_mi - a.at_mi), off, bearing));
        }
    }
    let (native_mi, off_road_mi, bearing) = best?;
    (off_road_mi <= 2.0).then(|| LegSnap {
        mile: leg_start_mi + stop_offset_for_direction(native_mi, leg.miles, forward),
        off_road_mi,
        bearing_deg: (bearing + if forward { 0.0 } else { 180.0 }).rem_euclid(360.0),
    })
}

/// The trip-absolute milepost where a coordinate meets a leg's road (see
/// [`snap_to_leg`]).
pub fn nearest_mile_on_leg(
    lat: f64,
    lon: f64,
    leg: &Leg,
    forward: bool,
    leg_start_mi: f64,
) -> Option<f64> {
    snap_to_leg(lat, lon, leg, forward, leg_start_mi).map(|snap| snap.mile)
}

/// Whether a closure signed for one direction of travel ("North") lies on
/// the other carriageway from a truck heading `bearing_deg`. A signed
/// direction is the route's, not the compass's (US-101 "North" runs due
/// west through Santa Barbara), so only a heading more than 120 degrees from
/// it, 30 past that worst case, reads as the other side. A closure signed
/// both ways, or not at all, is on every carriageway.
pub fn runs_against_travel(direction: &str, bearing_deg: f64) -> bool {
    let signed = match direction {
        "North" => 0.0,
        "East" => 90.0,
        "South" => 180.0,
        "West" => 270.0,
        _ => return false,
    };
    let apart = (bearing_deg - signed).rem_euclid(360.0);
    apart.min(360.0 - apart) > 120.0
}

/// Great-circle distance in miles between two coordinates.
pub fn haversine_distance_mi(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    let (lat1, lon1, lat2, lon2) = (
        lat1.to_radians(),
        lon1.to_radians(),
        lat2.to_radians(),
        lon2.to_radians(),
    );
    let dlat = lat2 - lat1;
    let dlon = lon2 - lon1;
    let a = (dlat / 2.0).sin().powi(2) + lat1.cos() * lat2.cos() * (dlon / 2.0).sin().powi(2);
    2.0 * a.sqrt().asin() * 3956.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fallback_grade_is_level_on_flat_ground() {
        assert_eq!(fallback_grade("flat", 10.0, "I-80"), 0.0);
        assert!(fallback_grade("mountain", 3.0, "I-70").abs() <= 0.035);
    }

    #[test]
    fn haversine_known_distance() {
        // Chicago to Indianapolis is about 165 miles.
        let d = haversine_distance_mi(41.8781, -87.6298, 39.7684, -86.1581);
        assert!((160.0..170.0).contains(&d), "{d}");
    }
}
