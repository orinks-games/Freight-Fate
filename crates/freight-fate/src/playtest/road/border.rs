//! The `--find border` finder: a customs booth at a land border crossing,
//! where every truck stops (`DrivingState::check_border_booth_crossing`).
//!
//! The miles are the drive's own (`Trip::border_booths`), so the mile the
//! tool promises is the mile the booth stands at, in the direction driven.

use ff_core::sim::trip::Trip;

use super::{Hit, DEFAULT_LEAD_MI};

/// Every customs booth on the route, named with the country it admits to.
pub fn border_hits(trip: &mut Trip, origin: &str, destination: &str) -> Vec<Hit> {
    let total = trip.total_miles();
    let booths = trip.border_booths.clone();
    booths
        .into_iter()
        .filter(|booth| (0.0..total).contains(&booth.at_mi))
        .map(|booth| {
            let (limit, _) = trip.speed_limit_at((booth.at_mi - DEFAULT_LEAD_MI).max(0.0));
            Hit {
                origin: origin.to_string(),
                destination: destination.to_string(),
                at_mi: booth.at_mi,
                total_mi: total,
                magnitude: 0.0,
                run_mi: 0.0,
                limit_mph: limit,
                label: format!("{} into {}", booth.name, booth.entering_country),
                trip_seed: None,
                origin_location: None,
            }
        })
        .collect()
}
