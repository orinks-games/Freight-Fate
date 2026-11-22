//! The `--find bridge` and `--find jointed` finders, from the pavement layer
//! (`ff_core::data::pavement`): a bridge with joints at its ends, and a run
//! of road with seams.
//!
//! The miles are the drive's own (`Trip::bridge_joint_spans`,
//! `Trip::pavement_reading_at`), so the mile the tool promises is where the
//! first joint or seam is. Longest first, and only the few longest a route
//! has: one bridge sounds like the next.

use ff_core::sim::road_texture::joints::SpanOrigin;
use ff_core::sim::road_texture::RoadSurface;
use ff_core::sim::trip::Trip;

use super::{Hit, DEFAULT_LEAD_MI, SCAN_STEP_MI};

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
            SpanOrigin::NamedBridge => Some((
                s.from_mi,
                s.to_mi - s.from_mi,
                format!("named bridge {:.2} mi", s.to_mi - s.from_mi),
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
