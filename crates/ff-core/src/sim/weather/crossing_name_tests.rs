//! The state and province line names a place the way a sentence does:
//! "the Northwest Territories", but "Washington" and "Yukon" bare.

use super::test_support::new_system;
use crate::sim::trip::{Trip, TripOptions};
use crate::sim::trip_models::TripEventKind;
use crate::sim::vehicle::TruckState;

/// The line spoken where the route's first mapped state crossing is.
fn crossing_line(cities: &[&str]) -> Vec<String> {
    let route = crate::data::world::get_world()
        .route_from_cities(cities)
        .unwrap_or_else(|| panic!("no route through {cities:?}"));
    let weather = new_system("pacific_northwest", Some(1), None, None, true);
    let mut trip = Trip::new(
        route.clone(),
        TruckState::default(),
        weather,
        TripOptions::seeded(2),
    );
    trip.traffic_manager.rolling_bubble = false;
    trip.traffic_manager.vehicles = Vec::new();
    let line_mi = route.legs[0].state_crossings()[0].at_mi;
    trip.position_mi = (line_mi - 1.0).max(0.0);
    trip.update(0.0);
    trip.position_mi = line_mi;
    trip.update(0.0)
        .iter()
        .filter(|e| e.kind == TripEventKind::StateCrossing)
        .map(|e| e.text().to_string())
        .collect()
}

#[test]
fn the_alberta_line_says_the_northwest_territories() {
    assert_eq!(
        crossing_line(&["high_level_ab_ca", "hay_river_nt_ca"]),
        vec!["Crossing into the Northwest Territories."]
    );
}

#[test]
fn a_drive_into_washington_names_it_bare() {
    let lines = crossing_line(&["portland_or_us", "seattle_wa_us"]);
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert!(
        lines[0].starts_with("Crossing into Washington"),
        "{lines:?}"
    );
}
