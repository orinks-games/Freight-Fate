//! South Jersey's roads (missing roads brief 1, 2026-11-22): the New Jersey
//! Turnpike south of Interchange 7A, I-295 and the Delaware Memorial Bridge
//! to Carneys Point, I-195 from Toms River, and the two ways down to Cape
//! May. The game's own route search has to find them, and a toll that is
//! collected one way only is quoted that way only.

use crate::data_support::world;
use ff_core::data::world_constants::is_curated_complete_market;
use ff_core::data::world_models::{Leg, Route};

fn leg(a: &str, b: &str) -> &'static Leg {
    world()
        .legs
        .iter()
        .find(|leg| leg.a == a && leg.b == b)
        .unwrap_or_else(|| panic!("no {a} -> {b} leg"))
}

fn route(a: &str, b: &str) -> Route {
    world()
        .supported_route(a, b, None)
        .unwrap()
        .unwrap_or_else(|| panic!("no supported route from {a} to {b}"))
}

fn drive(cities: &[&str]) -> Route {
    world()
        .route_from_cities(cities)
        .unwrap_or_else(|| panic!("no route through {cities:?}"))
}

#[test]
fn baltimore_from_newark_crosses_the_delaware_memorial_bridge() {
    // By the Turnpike and the bridge 185 miles, through Philadelphia 187.
    let found = route("newark_nj_us", "baltimore_md_us");
    assert_eq!(
        found.cities,
        ["newark_nj_us", "carneys_point_nj_us", "baltimore_md_us"]
    );
    let through_philadelphia = world()
        .supported_route_options("newark_nj_us", "baltimore_md_us", 3)
        .unwrap()
        .into_iter()
        .any(|r| r.cities.iter().any(|c| c == "philadelphia_pa_us"));
    assert!(through_philadelphia, "Philadelphia is still offered");
}

#[test]
fn the_new_legs_answer_the_trips_they_were_built_for() {
    let vineland = route("vineland_nj_us", "wilmington_de_us");
    assert_eq!(
        vineland.cities,
        ["vineland_nj_us", "carneys_point_nj_us", "wilmington_de_us"]
    );
    let toms_river = route("toms_river_nj_us", "trenton_nj_us");
    assert_eq!(toms_river.cities, ["toms_river_nj_us", "trenton_nj_us"]);
    assert_eq!(toms_river.highways(), ["I-195"]);
    for from in ["atlantic_city_nj_us", "vineland_nj_us"] {
        let found = route(from, "cape_may_nj_us");
        assert_eq!(found.cities, [from, "cape_may_nj_us"]);
    }
    // Trenton to Wilmington stays on I-95 through Philadelphia (64 miles);
    // I-295 and the bridge (74) is the alternative.
    let options = world()
        .supported_route_options("trenton_nj_us", "wilmington_de_us", 3)
        .unwrap();
    assert!(options[0].cities.iter().any(|c| c == "philadelphia_pa_us"));
    assert!(options
        .iter()
        .any(|r| r.cities.iter().any(|c| c == "carneys_point_nj_us")));
}

#[test]
fn the_delaware_memorial_bridge_is_paid_southbound_only() {
    let south = drive(&["carneys_point_nj_us", "wilmington_de_us"]);
    let names: Vec<&str> = south
        .toll_events()
        .iter()
        .map(|t| t.name.as_str())
        .collect();
    assert_eq!(names, ["Delaware Memorial Bridge toll"], "{names:?}");
    assert!((south.estimated_tolls() - 35.0).abs() < 1e-9);
    let north = drive(&["wilmington_de_us", "carneys_point_nj_us"]);
    assert!(north.toll_events().is_empty());
    assert!(north.estimated_tolls().abs() < 1e-9);
}

#[test]
fn a_turnpike_trip_is_quoted_one_ticket_each_way() {
    // The ticket is stored once for each direction, at the exit booth that
    // collects it, so a quote that ignored direction would bill it twice.
    let fare = 51.21;
    for cities in [
        ["newark_nj_us", "carneys_point_nj_us"],
        ["carneys_point_nj_us", "newark_nj_us"],
    ] {
        let trip = drive(&cities);
        assert_eq!(trip.toll_events().len(), 1, "{cities:?}");
        assert!(
            (trip.estimated_tolls() - fare).abs() < 1e-9,
            "{cities:?}: {}",
            trip.estimated_tolls()
        );
    }
}

#[test]
fn turnpike_plazas_serve_the_side_of_the_road_they_stand_on() {
    let turnpike = leg("newark_nj_us", "carneys_point_nj_us");
    let offered = |forward: bool| -> Vec<&str> {
        turnpike
            .stops
            .iter()
            .filter(|stop| stop.applies_to_direction(forward))
            .map(|stop| stop.name.as_str())
            .collect()
    };
    let south = offered(true);
    let north = offered(false);
    for plaza in ["Walt Whitman Service Area", "Clara Barton Service Area"] {
        assert!(south.contains(&plaza), "{plaza}: {south:?}");
        assert!(!north.contains(&plaza), "{plaza}: {north:?}");
    }
    for plaza in [
        "James Fenimore Cooper Service Area",
        "John Fenwick Service Area",
    ] {
        assert!(north.contains(&plaza), "{plaza}: {north:?}");
        assert!(!south.contains(&plaza), "{plaza}: {south:?}");
    }
}

#[test]
fn carneys_point_and_cape_may_keep_only_their_real_sites() {
    for key in ["carneys_point_nj_us", "cape_may_nj_us"] {
        assert!(is_curated_complete_market(key), "{key}");
        assert!(world().cities.contains_key(key), "{key}");
    }
}
