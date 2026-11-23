//! The Appalachia and Mid-South roads (missing roads pass, 2026-11-23): three
//! new towns, the legs that join neighbours the map routed far around, and
//! the long legs split at a city they pass. The game's own route search has
//! to take each new leg for the pair it was built for, and a trip saved on a
//! split leg has to resume on its chain.

use crate::data_support::world;
use ff_core::data::retired_legs::replacement_chain;
use ff_core::data::world_constants::is_curated_complete_market;
use ff_core::data::world_models::Route;

/// Every new leg and split piece, stored one way as `from:to`.
const NEW_LEGS: &str = include_str!("data_appalachia_mid_south_legs.txt");

fn route(a: &str, b: &str) -> Route {
    world()
        .supported_route(a, b, None)
        .unwrap()
        .unwrap_or_else(|| panic!("no supported route from {a} to {b}"))
}

fn has_leg(a: &str, b: &str) -> bool {
    world()
        .legs
        .iter()
        .any(|leg| (leg.a == a && leg.b == b) || (leg.a == b && leg.b == a))
}

fn new_legs() -> Vec<(&'static str, &'static str)> {
    NEW_LEGS
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|l| l.split_once(':').expect("from:to"))
        .collect()
}

fn drive(cities: &[&str]) -> Route {
    world()
        .route_from_cities(cities)
        .unwrap_or_else(|| panic!("no route through {cities:?}"))
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

#[test]
fn every_new_leg_is_the_route_for_its_own_pair() {
    let mut wrong = Vec::new();
    for (a, b) in new_legs() {
        assert!(has_leg(a, b), "no {a} -> {b} leg");
        for (from, to) in [(a, b), (b, a)] {
            let found = route(from, to);
            if found.cities != [from, to] {
                wrong.push(format!("{from} -> {to}: {:?}", found.cities));
            }
        }
    }
    assert!(wrong.is_empty(), "{wrong:#?}");
}

#[test]
fn split_legs_are_gone_and_resume_on_their_chain() {
    let splits = [
        ("johnson_city_tn_us", "pikeville_ky_us"),
        ("asheville_nc_us", "winston_salem_nc_us"),
        ("charleston_wv_us", "richmond_va_us"),
    ];
    for (a, b) in splits {
        assert!(!has_leg(a, b), "{a} -> {b} still a leg");
        let chain = replacement_chain(a, b).unwrap_or_else(|| panic!("{a} -> {b} not retired"));
        let resumed = world()
            .route_from_cities(&[a, b])
            .unwrap_or_else(|| panic!("chain {chain:?} has a gap"));
        assert_eq!(resumed.cities, chain);
    }
}

#[test]
fn the_new_towns_are_complete_markets() {
    for key in ["morehead_city_nc_us", "pinehurst_nc_us", "reedville_va_us"] {
        assert!(is_curated_complete_market(key), "{key}");
        assert!(world().cities.contains_key(key), "{key}");
    }
}

#[test]
fn the_west_virginia_turnpike_charges_ghent_on_the_way_to_wytheville() {
    // West Virginia Parkways Authority, 5 axle, Ghent plaza, 13.00 each way.
    for cities in [
        ["beckley_wv_us", "wytheville_va_us"],
        ["wytheville_va_us", "beckley_wv_us"],
    ] {
        let trip = drive(&cities);
        assert_eq!(trip.toll_events().len(), 1, "{cities:?}");
        assert!(
            close(trip.estimated_tolls(), 13.0),
            "{cities:?}: {}",
            trip.estimated_tolls()
        );
    }
}

#[test]
fn the_charleston_to_richmond_chain_pays_the_old_through_fare() {
    // The retired leg paid Chelyan and Pax, 13.00 each; both sit on
    // Charleston to Beckley, and the rest of the chain is toll free.
    let trip = drive(&[
        "charleston_wv_us",
        "beckley_wv_us",
        "staunton_va_us",
        "charlottesville_va_us",
        "richmond_va_us",
    ]);
    assert!(
        close(trip.estimated_tolls(), 26.0),
        "{}",
        trip.estimated_tolls()
    );
}
