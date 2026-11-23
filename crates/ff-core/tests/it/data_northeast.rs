//! The Northeast roads (missing roads pass, 2026-11-23): thirteen new towns,
//! the legs that join neighbours the map routed far around, and the long
//! legs split at a city they pass. The game's own route search has to take
//! each new leg for the pair it was built for, a trip saved on a split leg
//! has to resume on its chain, and each toll road pays its 2026 five-axle
//! fare in the direction it is collected.

use crate::data_support::world;
use ff_core::data::retired_legs::replacement_chain;
use ff_core::data::world_constants::is_curated_complete_market;
use ff_core::data::world_models::Route;

/// Every new leg and split piece, stored one way as `from:to`.
const NEW_LEGS: &str = include_str!("data_northeast_legs.txt");

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

fn fare(cities: &[&str]) -> f64 {
    drive(cities).estimated_tolls()
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

/// Thruway pieces kept so a trip saved on the old Buffalo or Rochester to New
/// York leg resumes on its road. Between their own ends I-90, I-81 and NY 17
/// through Syracuse and Binghamton run about 30 miles shorter, so the route
/// search rightly takes that way instead.
const RESUME_ONLY: &[(&str, &str)] = &[
    ("buffalo_ny_us", "newburgh_ny_us"),
    ("rochester_ny_us", "newburgh_ny_us"),
];

#[test]
fn every_new_leg_is_the_route_for_its_own_pair() {
    let mut wrong = Vec::new();
    for (a, b) in new_legs() {
        assert!(has_leg(a, b), "no {a} -> {b} leg");
        if RESUME_ONLY.contains(&(a, b)) {
            continue;
        }
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
        ("newport_ri_us", "new_haven_ct_us"),
        ("allentown_pa_us", "baltimore_md_us"),
        ("albany_ny_us", "montpelier_vt_us"),
        ("new_york_ny_us", "albany_ny_us"),
        ("buffalo_ny_us", "new_york_ny_us"),
        ("rochester_ny_us", "new_york_ny_us"),
        ("meadville_pa_us", "pittsburgh_pa_us"),
        ("youngstown_oh_us", "pittsburgh_pa_us"),
        ("pittsburgh_pa_us", "cleveland_oh_us"),
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
fn the_new_towns_keep_only_their_surveyed_sites() {
    for key in [
        "newburgh_ny_us",
        "lancaster_pa_us",
        "reading_pa_us",
        "gloucester_ma_us",
        "dubois_pa_us",
        "elmira_ny_us",
        "hanover_pa_us",
        "ithaca_ny_us",
        "johnstown_pa_us",
        "lexington_park_md_us",
        "pittsfield_ma_us",
        "riverhead_ny_us",
        "cranberry_township_pa_us",
    ] {
        assert!(is_curated_complete_market(key), "{key}");
        assert!(world().cities.contains_key(key), "{key}");
    }
}

#[test]
fn the_pennsylvania_turnpike_charges_its_2026_five_axle_fare_each_way() {
    // PA Turnpike 2026 E-ZPass, 5 Axle High: Cranberry (28) to Carlisle
    // (226) 116.76, Bedford (146) to Carlisle 46.04, Bedford to Breezewood
    // (161) 8.92, Morgantown (298) to Valley Forge (326) 21.80, Lehigh
    // Valley (56) to Pocono (95) 25.00.
    for (a, b, amount) in [
        ("cranberry_township_pa_us", "carlisle_pa_us", 116.76),
        ("johnstown_pa_us", "carlisle_pa_us", 46.04),
        ("altoona_pa_us", "hagerstown_md_us", 8.92),
        ("reading_pa_us", "philadelphia_pa_us", 21.80),
        ("allentown_pa_us", "williamsport_pa_us", 25.00),
    ] {
        for cities in [[a, b], [b, a]] {
            let trip = drive(&cities);
            assert_eq!(trip.toll_events().len(), 1, "{cities:?}");
            assert!(
                close(trip.estimated_tolls(), amount),
                "{cities:?}: {}",
                trip.estimated_tolls()
            );
        }
    }
}

#[test]
fn the_ohio_line_pays_gateway_eastbound_and_eastgate_westbound() {
    // Gateway (PA, axle classes) collects eastbound only, 31.57 for five
    // axles; Eastgate (Ohio, class 5) collects westbound only, 9.25.
    assert!(close(
        fare(&["youngstown_oh_us", "cranberry_township_pa_us"]),
        31.57
    ));
    assert!(close(
        fare(&["cranberry_township_pa_us", "youngstown_oh_us"]),
        9.25
    ));
    // Cranberry to Cleveland adds the Newton Falls to exit 173 ticket, 10.75.
    assert!(close(
        fare(&["cranberry_township_pa_us", "cleveland_oh_us"]),
        9.25 + 10.75
    ));
    assert!(close(
        fare(&["cleveland_oh_us", "cranberry_township_pa_us"]),
        10.75 + 31.57
    ));
}

#[test]
fn one_way_bridges_charge_only_the_way_they_collect() {
    // Newburgh-Beacon Bridge (eastbound, 15.25), Chesapeake Bay Bridge
    // (eastbound, 24.00), Delaware Water Gap Toll Bridge (westbound, 32.50).
    for (toward, back, amount) in [
        (
            ["newburgh_ny_us", "hartford_ct_us"],
            ["hartford_ct_us", "newburgh_ny_us"],
            15.25,
        ),
        (
            ["washington_dc_us", "dover_de_us"],
            ["dover_de_us", "washington_dc_us"],
            24.0,
        ),
        (
            ["newark_nj_us", "scranton_pa_us"],
            ["scranton_pa_us", "newark_nj_us"],
            32.5,
        ),
    ] {
        assert!(
            close(fare(&toward), amount),
            "{toward:?}: {}",
            fare(&toward)
        );
        assert!(drive(&back).toll_events().is_empty(), "{back:?}");
    }
}

#[test]
fn hampton_is_paid_both_ways_on_the_roads_into_portsmouth() {
    for a in ["boston_ma_us", "worcester_ma_us", "gloucester_ma_us"] {
        for cities in [[a, "portsmouth_nh_us"], ["portsmouth_nh_us", a]] {
            let trip = drive(&cities);
            let names: Vec<&str> = trip.toll_events().iter().map(|t| t.name.as_str()).collect();
            assert_eq!(names, ["Hampton toll plaza"], "{cities:?}");
            assert!(close(trip.estimated_tolls(), 4.95));
        }
    }
}

#[test]
fn the_thruway_chains_still_add_up_to_the_old_through_fares() {
    // The pieces share the old through fares by Thruway miles, so a trip
    // over the chain pays what the retired leg did.
    for (cities, amount) in [
        (["new_york_ny_us", "newburgh_ny_us", "albany_ny_us"], 46.43),
        (
            ["buffalo_ny_us", "newburgh_ny_us", "new_york_ny_us"],
            167.53,
        ),
        (
            ["rochester_ny_us", "newburgh_ny_us", "new_york_ny_us"],
            151.0,
        ),
    ] {
        assert!(
            close(fare(&cities), amount),
            "{cities:?}: {}",
            fare(&cities)
        );
    }
}
