//! The Great Lakes roads (missing roads pass, 2026-11-22): Joliet, Alpena,
//! Ashtabula and Sandusky, the legs that join neighbours the map routed far
//! around, and the long legs split at a city they pass. The game's own route
//! search has to take each new leg for the pair it was built for, and a trip
//! saved on a split leg has to resume on its chain.

use crate::data_support::world;
use ff_core::data::retired_legs::replacement_chain;
use ff_core::data::world_constants::is_curated_complete_market;
use ff_core::data::world_models::Route;

/// Every new leg and split piece, stored one way as `from:to`.
const NEW_LEGS: &str = include_str!("data_great_lakes_legs.txt");

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
        ("bloomington_il_us", "chicago_il_us"),
        ("chicago_il_us", "st_louis_mo_us"),
        ("eau_claire_wi_us", "wausau_wi_us"),
        ("indianapolis_in_us", "south_bend_in_us"),
        ("ann_arbor_mi_us", "fort_wayne_in_us"),
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
fn the_new_cities_keep_only_their_surveyed_sites() {
    for key in [
        "joliet_il_us",
        "alpena_mi_us",
        "ashtabula_oh_us",
        "sandusky_oh_us",
    ] {
        assert!(is_curated_complete_market(key), "{key}");
        assert!(world().cities.contains_key(key), "{key}");
    }
}

#[test]
fn chicago_to_st_louis_runs_through_joliet_and_bloomington() {
    let found = route("chicago_il_us", "st_louis_mo_us");
    assert_eq!(
        found.cities,
        [
            "chicago_il_us",
            "joliet_il_us",
            "bloomington_il_us",
            "springfield_il_us",
            "st_louis_mo_us",
        ]
    );
}
