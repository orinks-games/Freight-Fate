//! The `--find river` finder: a bridge over a river, where the water under
//! the truck plays (`ff_core::sim::outside::Water`).
//!
//! The miles are the drive's own (`Trip::river_crossing_miles`), so the mile
//! the tool promises is the mile the water starts at. Rivers are not ranked:
//! one bridge sounds like the next, so the first on the route comes first.

use ff_core::sim::trip::Trip;
use ff_core::sim::trip_route_helpers::stop_offset_for_direction;

use super::{Hit, DEFAULT_LEAD_MI};

/// Every river the route crosses, named from its landmark.
pub fn river_hits(trip: &mut Trip, origin: &str, destination: &str) -> Vec<Hit> {
    let total = trip.total_miles();
    let miles = trip.river_crossing_miles();
    let mut named: Vec<(f64, String)> = Vec::new();
    let mut offset = 0.0;
    for (i, leg) in trip.route.legs.iter().enumerate() {
        let forward = trip.route.cities[i] == leg.a;
        for landmark in leg.landmarks() {
            if landmark.category != "river" {
                continue;
            }
            let at = offset + stop_offset_for_direction(landmark.at_mi, leg.miles, forward);
            if miles.iter().any(|&m| (m - at).abs() < 1e-6) && (0.0..total).contains(&at) {
                named.push((at, landmark.name.clone()));
            }
        }
        offset += leg.miles;
    }
    named
        .into_iter()
        .map(|(at, name)| {
            let (limit, _) = trip.speed_limit_at((at - DEFAULT_LEAD_MI).max(0.0));
            Hit {
                origin: origin.to_string(),
                destination: destination.to_string(),
                at_mi: at,
                total_mi: total,
                magnitude: 0.0,
                run_mi: 0.0,
                limit_mph: limit,
                label: format!("river: {name}"),
                trip_seed: None,
                origin_location: None,
            }
        })
        .collect()
}
