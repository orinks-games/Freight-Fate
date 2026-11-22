//! Named bridges on a trip: where they sit, which names are spoken, the
//! joints and water at the bridge's real length.

use ff_core::data::world::get_world;
use ff_core::sim::bridges::BridgeCrossing;
use ff_core::sim::road_texture::joints::SpanOrigin;
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

/// Drive the bridge state over `from..to` in tenth-mile steps and return
/// every bridge line said.
fn lines_between(trip: &mut Trip, from: f64, to: f64) -> Vec<String> {
    let mut said = Vec::new();
    let mut at = from;
    while at < to {
        trip.position_mi = at;
        trip.check_bridges();
        for event in std::mem::take(&mut trip.events) {
            if event.kind == TripEventKind::Landmark
                && event.data.category.as_deref() == Some("bridge")
            {
                said.push(event.text().to_string());
            }
        }
        at += 0.1;
    }
    said
}

#[test]
fn the_confederation_bridge_is_named_once_driving_onto_the_island() {
    let mut east = trip("Moncton", "Charlottetown");
    let bridge = east
        .bridges
        .iter()
        .find(|b| b.name == "Confederation Bridge")
        .expect("the bridge is on the route")
        .clone();
    assert!(bridge.water);
    assert!((bridge.length_mi() - 8.0).abs() < 0.2, "{bridge:?}");
    let said = lines_between(&mut east, bridge.from_mi - 1.0, bridge.to_mi + 1.0);
    assert_eq!(said, vec!["Crossing the Confederation Bridge.".to_string()]);
    assert!(matches!(east.bridge_state(), BridgeCrossing::RoadAfter { .. }));

    let spans = east.bridge_joint_spans();
    assert!(
        spans.iter().any(|s| s.origin == SpanOrigin::NamedBridge
            && (s.from_mi - bridge.from_mi).abs() < 1e-6
            && (s.to_mi - bridge.to_mi).abs() < 1e-6),
        "{spans:?}"
    );
}

#[test]
fn a_toll_that_names_the_bridge_leaves_the_bridge_line_out() {
    // Leaving the island the Borden-Carleton plaza says the bridge's name.
    let west = trip("Charlottetown", "Moncton");
    let bridge = west
        .bridges
        .iter()
        .find(|b| b.name == "Confederation Bridge")
        .expect("the bridge is on the route");
    assert!(bridge.line.is_none(), "{bridge:?}");
}

#[test]
fn the_river_line_gives_way_to_the_bridge_over_it() {
    let east = trip("Port Huron", "Sarnia");
    let bridge = east
        .bridges
        .iter()
        .find(|b| b.name == "Blue Water Bridge")
        .expect("the bridge is on the route")
        .clone();
    assert!(
        !east
            .landmarks
            .iter()
            .any(|c| c.category == "river" && bridge.from_mi <= c.at_mi && c.at_mi < bridge.to_mi),
        "{:?}",
        east.landmarks
    );
    // The water still plays under it.
    assert!(east
        .river_crossing_miles()
        .iter()
        .any(|&at| bridge.from_mi - 0.5 <= at && at <= bridge.to_mi + 0.5));
}
