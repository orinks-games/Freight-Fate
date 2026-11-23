//! The South Central roads (missing roads pass, 2026-11-23): Sikeston,
//! Missouri, the legs that join Oklahoma, Kansas, Missouri and Arkansas
//! towns the map routed far around, and the long legs split at a city they
//! pass. The game's own route search has to take each new leg for the pair
//! it was built for, a trip saved on a split leg has to resume on its
//! chain, and each turnpike leg pays its five-axle fare.

use crate::data_support::world;
use ff_core::data::retired_legs::replacement_chain;
use ff_core::data::world_constants::is_curated_complete_market;
use ff_core::data::world_models::Route;

/// Every new leg and split piece, stored one way as `from:to`.
const NEW_LEGS: &str = include_str!("data_south_central_legs.txt");

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
        ("altus_ok_us", "wichita_falls_tx_us"),
        ("el_dorado_ar_us", "monroe_la_us"),
        ("garden_city_ks_us", "amarillo_tx_us"),
        ("dodge_city_ks_us", "salina_ks_us"),
        ("poplar_bluff_mo_us", "cape_girardeau_mo_us"),
        ("little_rock_ar_us", "oklahoma_city_ok_us"),
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
fn sikeston_keeps_only_its_surveyed_sites() {
    assert!(is_curated_complete_market("sikeston_mo_us"));
    assert!(world().cities.contains_key("sikeston_mo_us"));
}

#[test]
fn the_turnpike_legs_charge_their_five_axle_fares() {
    // Oklahoma Turnpike Authority 2024 rates (estimated: OTA has raised
    // them since) and the Kansas Turnpike's per-mile rate times the
    // turnpike miles on each leg.
    let fares = [
        (
            ["ada_ok_us", "tulsa_ok_us"],
            "Turner Turnpike settlement",
            8.50,
            35.90,
        ),
        (
            ["ada_ok_us", "lawton_ok_us"],
            "Chickasaw Turnpike settlement",
            2.20,
            5.45,
        ),
        (
            ["stillwater_ok_us", "tulsa_ok_us"],
            "Cimarron Turnpike settlement",
            7.90,
            14.05,
        ),
        (
            ["atoka_ok_us", "henryetta_ok_us"],
            "Indian Nation Turnpike settlement",
            8.45,
            17.55,
        ),
        (
            ["atoka_ok_us", "texarkana_ar_us"],
            "Indian Nation Turnpike settlement",
            2.90,
            5.70,
        ),
        (
            ["atoka_ok_us", "paris_tx_us"],
            "Indian Nation Turnpike settlement",
            2.90,
            5.70,
        ),
        (
            ["henryetta_ok_us", "mcalester_ok_us"],
            "Indian Nation Turnpike settlement",
            7.05,
            14.45,
        ),
        (
            ["joplin_mo_us", "muskogee_ok_us"],
            "Will Rogers Turnpike settlement",
            9.20,
            35.90,
        ),
        (
            ["tulsa_ok_us", "muskogee_ok_us"],
            "Muskogee Turnpike settlement",
            5.75,
            11.35,
        ),
        (
            ["muskogee_ok_us", "fort_smith_ar_us"],
            "Muskogee Turnpike settlement",
            4.50,
            8.75,
        ),
        (
            ["lawrence_ks_us", "saint_joseph_mo_us"],
            "Kansas Turnpike settlement",
            2.87,
            5.74,
        ),
        (
            ["lawrence_ks_us", "kansas_city_mo_us"],
            "Kansas Turnpike settlement",
            2.73,
            5.46,
        ),
        (
            ["enid_ok_us", "wichita_ks_us"],
            "Kansas Turnpike settlement",
            5.69,
            11.37,
        ),
        (
            ["wichita_ks_us", "tulsa_ok_us"],
            "Kansas Turnpike settlement",
            5.62,
            11.23,
        ),
    ];
    for (pair, name, amount, plate) in fares {
        for cities in [pair, [pair[1], pair[0]]] {
            let trip = drive(&cities);
            let events = trip.toll_events();
            let paid: Vec<_> = events.iter().filter(|t| t.name == name).collect();
            assert_eq!(paid.len(), 1, "{cities:?}: {events:?}");
            assert!((paid[0].amount - amount).abs() < 1e-6, "{cities:?}");
            assert!((paid[0].amount_plate - plate).abs() < 1e-6, "{cities:?}");
        }
    }
}
