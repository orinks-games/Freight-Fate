//! The `--find bridge`, `--find jointed`, `--find grooved` and `--find
//! grating` finders, from the pavement layer (`ff_core::data::pavement`): a
//! bridge with joints at its ends, a run of road with seams, grooved
//! concrete, and an open steel grating the tires hum on.
//!
//! The miles are the drive's own (`Trip::bridge_joint_spans`,
//! `Trip::pavement_reading_at`), so the mile the tool promises is where the
//! first joint or seam is. Longest first, and only the few longest a route
//! has: one bridge sounds like the next.

use ff_core::sim::road_texture::joints::SpanOrigin;
use ff_core::sim::road_texture::song::{grooves_origin, Underfoot};
use ff_core::sim::road_texture::RoadSurface;
use ff_core::sim::trip::Trip;

use super::{Hit, DEFAULT_LEAD_MI, SCAN_STEP_MI};

/// The surface finders' scan step: a movable span is a few hundred feet.
const SURFACE_STEP_MI: f64 = 0.01;
/// The most hits one route offers for either feature.
const PER_ROUTE: usize = 3;

fn hit(trip: &mut Trip, origin: &str, destination: &str, at: f64, run: f64, label: String) -> Hit {
    let (limit, _) = trip.speed_limit_at((at - DEFAULT_LEAD_MI).max(0.0));
    Hit {
        origin: origin.to_string(),
        destination: destination.to_string(),
        at_mi: at,
        total_mi: trip.total_miles(),
        magnitude: run,
        run_mi: run,
        limit_mph: limit,
        label,
        trip_seed: None,
        origin_location: None,
    }
}

fn longest(mut found: Vec<(f64, f64, String)>) -> Vec<(f64, f64, String)> {
    found.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.total_cmp(&b.0)));
    found.truncate(PER_ROUTE);
    found
}

/// The route's bridges and causeways that have joints.
pub fn bridge_hits(trip: &mut Trip, origin: &str, destination: &str) -> Vec<Hit> {
    let found = trip
        .bridge_joint_spans()
        .into_iter()
        .filter_map(|s| match s.origin {
            SpanOrigin::Structure(kind) => Some((
                s.from_mi,
                s.to_mi - s.from_mi,
                format!("{} {:.2} mi", kind.name(), s.to_mi - s.from_mi),
            )),
            SpanOrigin::RiverAssumed => None,
        })
        .collect();
    longest(found)
        .into_iter()
        .map(|(at, run, label)| hit(trip, origin, destination, at, run, label))
        .collect()
}

/// The route's runs of road the layer reads as having seams.
pub fn jointed_hits(trip: &mut Trip, origin: &str, destination: &str) -> Vec<Hit> {
    let total = trip.total_miles();
    let mut found: Vec<(f64, f64, String)> = Vec::new();
    let mut open: Option<(f64, &'static str)> = None;
    let mut at = 0.0;
    while at <= total {
        let reading = trip
            .pavement_reading_at(at)
            .filter(|r| RoadSurface::Read(*r).is_jointed());
        match (open, reading) {
            (None, Some(r)) => open = Some((at, r.surface.name())),
            (Some((from, name)), r) if r.map(|r| r.surface.name()) != Some(name) => {
                found.push((from, at - from, format!("{name} {:.1} mi", at - from)));
                open = r.map(|r| (at, r.surface.name()));
            }
            _ => {}
        }
        at += SCAN_STEP_MI;
    }
    if let Some((from, name)) = open {
        found.push((from, total - from, format!("{name} {:.1} mi", total - from)));
    }
    longest(found)
        .into_iter()
        .map(|(at, run, label)| hit(trip, origin, destination, at, run, label))
        .collect()
}

/// The route's runs of a road texture: open steel gratings or grooved concrete.
pub fn surface_hits(trip: &mut Trip, origin: &str, destination: &str, grating: bool) -> Vec<Hit> {
    let total = trip.total_miles();
    let wanted = |u: Underfoot| match u {
        Underfoot::Grating => grating,
        Underfoot::Grooved(_) => !grating,
        Underfoot::Plain => false,
    };
    let label = |u: Underfoot, run: f64| match u {
        Underfoot::Grooved(g) => format!("grooved, {} {run:.2} mi", grooves_origin(g)),
        _ => format!("grating {run:.2} mi"),
    };
    let mut found: Vec<(f64, f64, String)> = Vec::new();
    let mut open: Option<(f64, Underfoot)> = None;
    // A grating can be a movable span of a few hundred feet: scan finer.
    let step = SURFACE_STEP_MI;
    let mut at = 0.0;
    while at <= total {
        let under = Some(trip.underfoot_at(at)).filter(|u| wanted(*u));
        match (open, under) {
            (None, Some(u)) => open = Some((at, u)),
            (Some((from, was)), now) if now != Some(was) => {
                found.push((from, at - from, label(was, at - from)));
                open = now.map(|u| (at, u));
            }
            _ => {}
        }
        at += step;
    }
    if let Some((from, was)) = open {
        found.push((from, total - from, label(was, total - from)));
    }
    longest(found)
        .into_iter()
        .map(|(at, run, label)| hit(trip, origin, destination, at, run, label))
        .collect()
}
