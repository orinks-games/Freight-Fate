//! On a leg into another country, the destination exit is past the border:
//! an exit on the side being left would end the trip short of the bridge
//! and the customs booth.

use ff_core::sim::trip::destination_exit::scan_destination_exit;
use ff_core::sim::trip::{Trip, TripOptions};
use ff_core::sim::vehicle::TruckState;

use crate::sim_support::{weather, world};

fn trip_on(start: &str, end: &str) -> Trip {
    let route = world()
        .supported_route(start, end, None)
        .unwrap()
        .expect("route");
    Trip::new(
        route,
        TruckState::default(),
        weather("great_lakes", 7),
        TripOptions {
            seed: Some(7),
            world: Some(world()),
            ..Default::default()
        },
    )
}

/// The mile of the last line on the trip's one leg (its own record, driven
/// forward).
fn border_mile(trip: &Trip) -> f64 {
    let leg = &trip.route.legs[0];
    leg.state_crossings()
        .iter()
        .map(|c| c.at_mi)
        .fold(0.0, f64::max)
}

#[test]
fn detroit_to_windsor_never_ends_at_a_detroit_exit() {
    let trip = trip_on("detroit_mi_us", "windsor_on_ca");
    let line = border_mile(&trip);
    assert!(line > 5.0, "the Gordie Howe line, mile {line}");
    if let Some((at_mi, label, _)) = scan_destination_exit(world(), &trip, true) {
        assert!(
            at_mi > line,
            "exit {label} at mile {at_mi}, before the line at {line}"
        );
    }
}

#[test]
fn windsor_to_detroit_still_takes_a_detroit_exit() {
    let trip = trip_on("windsor_on_ca", "detroit_mi_us");
    let (at_mi, label, _) = scan_destination_exit(world(), &trip, true).expect("a Detroit exit");
    let line = border_mile(&trip);
    assert!(at_mi > line, "exit {label} at mile {at_mi}, line at {line}");
}
