//! Texas and Louisiana (missing roads brief 3, 2026-11-23): 61 new legs, six
//! of them reaching the new cities of Denton, Galveston, Freeport, Point
//! Comfort, Port Fourchon and Venice, and four legs retired for chains
//! through Denton and Dallas. The game's own route search has to take each
//! new leg for the pair it was built for, and a split leg is gone with its
//! chain in place.

use crate::data_support::world;
use ff_core::data::retired_legs::RETIRED_LEGS;

/// Every leg this region built, as stored.
const BUILT: &[(&str, &str)] = &[
    ("paris_tx_us", "mcalester_ok_us"),
    ("lufkin_tx_us", "shreveport_la_us"),
    ("dumas_tx_us", "raton_nm_us"),
    ("killeen_tx_us", "lampasas_tx_us"),
    ("austin_tx_us", "marble_falls_tx_us"),
    ("fort_worth_tx_us", "mineral_wells_tx_us"),
    ("brownwood_tx_us", "stephenville_tx_us"),
    ("hereford_tx_us", "plainview_tx_us"),
    ("dumas_tx_us", "pampa_tx_us"),
    ("fort_worth_tx_us", "stephenville_tx_us"),
    ("alexandria_la_us", "natchez_ms_us"),
    ("kerrville_tx_us", "marble_falls_tx_us"),
    ("natchitoches_la_us", "ruston_la_us"),
    ("ozona_tx_us", "san_angelo_tx_us"),
    ("hammond_la_us", "houma_la_us"),
    ("big_spring_tx_us", "san_angelo_tx_us"),
    ("stephenville_tx_us", "waco_tx_us"),
    ("longview_tx_us", "lufkin_tx_us"),
    ("alexandria_la_us", "ruston_la_us"),
    ("kerrville_tx_us", "uvalde_tx_us"),
    ("monroe_la_us", "natchez_ms_us"),
    ("alexandria_la_us", "lake_charles_la_us"),
    ("palestine_tx_us", "waco_tx_us"),
    ("longview_tx_us", "paris_tx_us"),
    ("houma_la_us", "lafayette_la_us"),
    ("junction_tx_us", "uvalde_tx_us"),
    ("dallas_tx_us", "paris_tx_us"),
    ("big_spring_tx_us", "lubbock_tx_us"),
    ("abilene_tx_us", "stephenville_tx_us"),
    ("hereford_tx_us", "tucumcari_nm_us"),
    ("lufkin_tx_us", "natchitoches_la_us"),
    ("brownwood_tx_us", "junction_tx_us"),
    ("abilene_tx_us", "mineral_wells_tx_us"),
    ("big_spring_tx_us", "hobbs_nm_us"),
    ("sherman_tx_us", "wichita_falls_tx_us"),
    ("houston_tx_us", "lufkin_tx_us"),
    ("childress_tx_us", "plainview_tx_us"),
    ("eagle_pass_tx_us", "laredo_tx_us"),
    ("midland_tx_us", "ozona_tx_us"),
    ("hammond_la_us", "hattiesburg_ms_us"),
    ("stratford_tx_us", "tucumcari_nm_us"),
    ("college_station_tx_us", "lufkin_tx_us"),
    ("del_rio_tx_us", "ozona_tx_us"),
    ("austin_tx_us", "victoria_tx_us"),
    ("houston_tx_us", "victoria_tx_us"),
    ("junction_tx_us", "lampasas_tx_us"),
    ("laredo_tx_us", "uvalde_tx_us"),
    ("fort_stockton_tx_us", "hobbs_nm_us"),
    ("abilene_tx_us", "vernon_tx_us"),
    ("abilene_tx_us", "childress_tx_us"),
    ("college_station_tx_us", "victoria_tx_us"),
    ("denton_tx_us", "jacksboro_tx_us"),
    ("freeport_tx_us", "houston_tx_us"),
    ("galveston_tx_us", "houston_tx_us"),
    ("galveston_tx_us", "beaumont_tx_us"),
    ("point_comfort_tx_us", "victoria_tx_us"),
    ("point_comfort_tx_us", "corpus_christi_tx_us"),
    ("point_comfort_tx_us", "freeport_tx_us"),
    ("port_fourchon_la_us", "houma_la_us"),
    ("port_fourchon_la_us", "new_orleans_la_us"),
    ("venice_la_us", "new_orleans_la_us"),
    ("mckinney_tx_us", "dallas_tx_us"),
    ("dallas_tx_us", "denton_tx_us"),
    ("denton_tx_us", "ardmore_ok_us"),
    ("fort_worth_tx_us", "denton_tx_us"),
    ("wichita_falls_tx_us", "denton_tx_us"),
    ("denton_tx_us", "mckinney_tx_us"),
];

/// The legs this region split, as they were stored.
const SPLIT: &[(&str, &str)] = &[
    ("dallas_tx_us", "st_louis_mo_us"),
    ("fort_worth_tx_us", "oklahoma_city_ok_us"),
    ("wichita_falls_tx_us", "mckinney_tx_us"),
    ("mckinney_tx_us", "waco_tx_us"),
];

fn has_leg(a: &str, b: &str) -> bool {
    world()
        .legs
        .iter()
        .any(|leg| (leg.a == a && leg.b == b) || (leg.a == b && leg.b == a))
}

#[test]
fn every_new_leg_is_the_route_for_its_own_pair() {
    let mut wrong = Vec::new();
    for &(a, b) in BUILT {
        assert!(has_leg(a, b), "{a} -> {b} is not on the map");
        let found = world()
            .supported_route(a, b, None)
            .unwrap()
            .unwrap_or_else(|| panic!("no route from {a} to {b}"));
        if found.cities != [a, b] {
            wrong.push(format!("{a} -> {b}: {:?}", found.cities));
        }
    }
    assert!(
        wrong.is_empty(),
        "pairs that do not take their leg: {wrong:#?}"
    );
}

#[test]
fn a_split_leg_is_gone_and_its_chain_resumes_it() {
    for &(a, b) in SPLIT {
        assert!(!has_leg(a, b), "{a} -> {b} was split and is still a leg");
        let (_, _, chain) = RETIRED_LEGS
            .iter()
            .find(|(from, to, _)| (*from == a && *to == b) || (*from == b && *to == a))
            .unwrap_or_else(|| panic!("{a} -> {b} has no RETIRED_LEGS entry"));
        for hop in chain.windows(2) {
            assert!(
                has_leg(hop[0], hop[1]),
                "{a} -> {b}: no leg {} -> {}",
                hop[0],
                hop[1]
            );
        }
    }
}

/// A new leg that leaves its city on a freeway never ends at that city's
/// exit (Flagstaff to Sedona once crowned an exit 4.7 miles out of
/// Flagstaff as the Sedona exit).
#[test]
fn a_new_leg_ends_past_its_halfway_mark() {
    use ff_core::sim::trip::destination_exit::scan_destination_exit;
    use ff_core::sim::trip::{Trip, TripOptions};
    use ff_core::sim::vehicle::TruckState;
    let world = world();
    for (a, b) in BUILT {
        for (from, to) in [(a, b), (b, a)] {
            let route = world
                .supported_route(from, to, None)
                .unwrap()
                .expect("route");
            let trip = Trip::new(
                route,
                TruckState::default(),
                crate::sim_support::weather("southern_plains", 7),
                TripOptions {
                    seed: Some(7),
                    world: Some(world),
                    ..Default::default()
                },
            );
            if let Some((at_mi, label, _)) = scan_destination_exit(world, &trip, true) {
                assert!(
                    at_mi >= trip.total_miles() / 2.0,
                    "{from} to {to}: exit {label} at mile {at_mi} of {}",
                    trip.total_miles()
                );
            }
        }
    }
}

/// The toll roads these legs ride are charged, at the authorities' five-axle
/// rates: College Station to Houston and McKinney to Fort Worth rode their
/// tollways for free before.
#[test]
fn the_region_toll_roads_are_charged() {
    let world = world();
    let cases: &[(&str, &str, f64)] = &[
        ("paris_tx_us", "mcalester_ok_us", 12.35),
        ("port_fourchon_la_us", "houma_la_us", 21.00),
        ("port_fourchon_la_us", "new_orleans_la_us", 21.00),
        ("fort_worth_tx_us", "stephenville_tx_us", 7.12),
        ("mckinney_tx_us", "fort_worth_tx_us", 21.16),
        ("college_station_tx_us", "houston_tx_us", 32.72),
        ("austin_tx_us", "victoria_tx_us", 5.36),
    ];
    for &(a, b, total) in cases {
        let leg = world
            .legs
            .iter()
            .find(|leg| leg.a == a && leg.b == b)
            .unwrap_or_else(|| panic!("{a} -> {b} is not on the map"));
        let charged: f64 = leg.toll_events().iter().map(|t| t.amount).sum();
        assert!(
            (charged - total).abs() < 0.005,
            "{a} -> {b} charges {charged}, not {total}"
        );
    }
}
