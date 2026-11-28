//! `Trip::river_crossing_miles`: the route miles where the water plays.

use ff_core::data::world::get_world;
use ff_core::sim::trip::{Trip, TripOptions};
use ff_core::sim::trip_models::TripEventKind;
use ff_core::sim::vehicle::TruckState;
use ff_core::sim::weather::{WeatherKind, WeatherSystem};

fn trip(origin: &str, destination: &str) -> Trip {
    let world = get_world();
    let route = world
        .supported_route(origin, destination, None)
        .expect("the world routes")
        .expect("the corridor is supported");
    let mut weather = WeatherSystem::new("great_lakes", Some(1), None, None, true);
    weather.current = WeatherKind::Clear;
    Trip::new(
        route,
        TruckState::default(),
        weather,
        TripOptions {
            seed: Some(1),
            world: Some(world),
            ..Default::default()
        },
    )
}

#[test]
fn the_genesee_is_crossed_at_its_mile_in_both_directions() {
    // Buffalo to Rochester on I-90: the Genesee River sits 73.9 miles from
    // Buffalo on a 74-mile leg.
    let east = trip("Buffalo", "Rochester");
    assert_eq!(
        east.route.legs.len(),
        1,
        "one leg, so the miles are the leg's"
    );
    let leg_miles = east.route.legs[0].miles;
    let rivers = east.river_crossing_miles();
    assert!(
        rivers.iter().any(|&at| (at - 73.9).abs() < 1e-6),
        "{rivers:?}"
    );

    let west = trip("Rochester", "Buffalo");
    let rivers = west.river_crossing_miles();
    assert!(
        rivers
            .iter()
            .any(|&at| (at - (leg_miles - 73.9)).abs() < 1e-6),
        "{rivers:?}"
    );
    assert!(rivers.windows(2).all(|w| w[0] <= w[1]), "sorted");
}

/// Drive `origin` to `destination` at 65 mph and `time_scale`, and return
/// the mile of every water onset with the river or bridge lines spoken, by
/// mile.
fn water_and_names(
    origin: &str,
    destination: &str,
    time_scale: f64,
) -> (Vec<f64>, Vec<(f64, String)>) {
    let world = get_world();
    let route = world
        .supported_route(origin, destination, None)
        .expect("the world routes")
        .expect("the corridor is supported");
    let mut weather = WeatherSystem::new("great_lakes", Some(1), None, None, true);
    weather.current = WeatherKind::Clear;
    let mut truck = TruckState::default();
    truck.transmission.automatic = true;
    truck.start_engine();
    truck.parking_brake = false;
    let mut trip = Trip::new(
        route,
        truck,
        weather,
        TripOptions {
            seed: Some(1),
            time_scale,
            start_hour: 12.0,
            hazard_scale: 0.0,
            world: Some(world),
            ..Default::default()
        },
    );
    trip.traffic_manager.rolling_bubble = false;
    let mut onsets = Vec::new();
    let mut names = Vec::new();
    let end = trip.total_miles() - 1.0;
    let mut guard = 0;
    while trip.position_mi < end && !trip.finished && guard < 400_000 {
        trip.truck.velocity_mps = 65.0 / 2.23694;
        let before = trip.river.water.is_over();
        for event in trip.update(0.25) {
            if event.kind == TripEventKind::Landmark
                && matches!(event.data.category.as_deref(), Some("river" | "bridge"))
            {
                names.push((trip.position_mi, event.message.normal.clone()));
            }
        }
        if !before && trip.river.water.is_over() {
            onsets.push(trip.position_mi);
        }
        guard += 1;
    }
    (onsets, names)
}

/// Every water onset on the drive has a river or bridge name, and the
/// rivers in `must` are among them.
fn assert_every_water_named(origin: &str, destination: &str, must: &[&str]) {
    let (onsets, names) = water_and_names(origin, destination, 20.0);
    eprintln!(
        "{origin} to {destination}: {} water sounds, named {names:?}",
        onsets.len()
    );
    assert!(!onsets.is_empty(), "{origin} to {destination}");
    for at in &onsets {
        assert!(
            names.iter().any(|(m, _)| (m - at).abs() < 0.5),
            "{origin} to {destination}: water at mile {at:.2} with no name; named {names:?}"
        );
    }
    for river in must {
        assert!(
            names.iter().any(|(_, line)| line.contains(river)),
            "{origin} to {destination}: {river} not named; named {names:?}"
        );
    }
    // One water, one name: never two names for one sound.
    assert!(
        names.len() <= onsets.len(),
        "{origin} to {destination}: {} names for {} water sounds: {names:?}",
        names.len(),
        onsets.len()
    );
}

// Orinks's drive, 2026-11-27: the water played at 18 crossings and only 7
// were named. A river near a town name or a billboard, or within 25 miles
// of other scenery at 20x, crossed in silence.
#[test]
fn every_water_onset_pembroke_to_north_bay_is_named_at_20x() {
    // Petawawa sits by a town name never spoken at sparse; Kaibuskong 12
    // miles after Amable du Fond.
    assert_every_water_named(
        "Pembroke",
        "North Bay",
        &["Petawawa River", "Chalk River", "Kaibuskong River"],
    );
}

#[test]
#[cfg_attr(
    not(feature = "slow-tests"),
    ignore = "drives 217 miles of Highway 11; run with cargo test-all"
)]
fn every_water_onset_thunder_bay_to_fort_frances_is_named_at_20x() {
    // Kashabowie followed a billboard; French followed "Arctic watershed".
    assert_every_water_named(
        "Thunder Bay",
        "Fort Frances",
        &["Kashabowie River", "French River", "Seine River"],
    );
}
