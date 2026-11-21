//! `Trip::river_crossing_miles`: the route miles where the water plays.

use ff_core::data::world::get_world;
use ff_core::sim::trip::{Trip, TripOptions};
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
