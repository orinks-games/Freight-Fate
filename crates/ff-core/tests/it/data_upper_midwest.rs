//! The Upper Midwest and Northern Plains roads (missing roads pass,
//! 2026-11-23): the legs that join neighbours the map routed far around in
//! Iowa, Minnesota, the Dakotas and Nebraska, and the long legs split at a
//! city they pass. The game's own route search has to take each new leg for
//! the pair it was built for, a trip saved on a split leg has to resume on
//! its chain, and a renamed leg carries the road it rides.

use crate::data_support::world;
use ff_core::data::retired_legs::replacement_chain;
use ff_core::data::world_models::Route;

/// Every new leg and split piece, stored one way as `from:to`.
const NEW_LEGS: &str = include_str!("data_upper_midwest_legs.txt");

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
        ("hibbing_mn_us", "minneapolis_mn_us"),
        ("mankato_mn_us", "rochester_mn_us"),
        ("des_moines_ia_us", "chicago_il_us"),
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
