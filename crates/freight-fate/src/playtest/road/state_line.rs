//! The `--find state-line` finder: a state or province line, where the
//! crossing call speaks and the lines after it (time zone, units, hours
//! rules) follow in order.
//!
//! The miles are the drive's own navigation cues, so the mile the tool
//! promises is the mile the line is called at, in the direction driven.

use ff_core::sim::trip::Trip;

use super::{Hit, DEFAULT_LEAD_MI};

/// Every state or province line on the route, named by its crossing call.
pub fn state_line_hits(trip: &mut Trip, origin: &str, destination: &str) -> Vec<Hit> {
    let total = trip.total_miles();
    let lines: Vec<(f64, String)> = trip
        .navigation_cues
        .iter()
        .filter(|cue| cue.kind == "state_crossing" && (0.0..total).contains(&cue.at_mi))
        .map(|cue| (cue.at_mi, cue.text.clone()))
        .collect();
    lines
        .into_iter()
        .map(|(at_mi, label)| {
            let (limit, _) = trip.speed_limit_at((at_mi - DEFAULT_LEAD_MI).max(0.0));
            Hit {
                origin: origin.to_string(),
                destination: destination.to_string(),
                at_mi,
                total_mi: total,
                magnitude: 0.0,
                run_mi: 0.0,
                limit_mph: limit,
                label,
                trip_seed: None,
                origin_location: None,
            }
        })
        .collect()
}
