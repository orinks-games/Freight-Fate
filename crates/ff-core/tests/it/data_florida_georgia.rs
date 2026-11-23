//! Florida and Georgia's missing roads (missing roads, Florida and Georgia
//! region, 2026-11-22): the game's own route search has to take each new
//! leg for the pair it was built for, the Turnpike split keeps its chain,
//! and a toll collected one way only is quoted that way only.

use crate::data_support::world;
use ff_core::data::world_models::Route;

/// Every leg the region list built, as `(from, to)`.
const BUILT: &[(&str, &str)] = &[
    ("cordele_ga_us", "savannah_ga_us"),
    ("cape_coral_fl_us", "fort_myers_fl_us"),
    ("americus_ga_us", "cordele_ga_us"),
    ("lakeland_fl_us", "tampa_fl_us"),
    ("gainesville_fl_us", "ocala_fl_us"),
    ("dalton_ga_us", "rome_ga_us"),
    ("coral_springs_fl_us", "miami_fl_us"),
    ("coral_springs_fl_us", "west_palm_beach_fl_us"),
    ("gainesville_fl_us", "lake_city_fl_us"),
    ("hinesville_ga_us", "statesboro_ga_us"),
    ("lakeland_fl_us", "orlando_fl_us"),
    ("americus_ga_us", "macon_ga_us"),
    ("daytona_beach_fl_us", "ocala_fl_us"),
    ("dublin_ga_us", "statesboro_ga_us"),
    ("orlando_fl_us", "spring_hill_fl_us"),
    ("gainesville_fl_us", "palm_coast_fl_us"),
    ("ocala_fl_us", "palm_coast_fl_us"),
    ("ocala_fl_us", "orlando_fl_us"),
    ("cordele_ga_us", "dublin_ga_us"),
    ("lakeland_fl_us", "ocala_fl_us"),
    ("tallahassee_fl_us", "tifton_ga_us"),
    ("crestview_fl_us", "troy_al_us"),
    ("daytona_beach_fl_us", "palm_bay_fl_us"),
    ("augusta_ga_us", "dublin_ga_us"),
    ("lagrange_ga_us", "rome_ga_us"),
    ("dublin_ga_us", "tifton_ga_us"),
    ("panama_city_fl_us", "pensacola_fl_us"),
    ("coral_springs_fl_us", "naples_fl_us"),
    ("augusta_ga_us", "greenville_sc_us"),
    ("brunswick_ga_us", "valdosta_ga_us"),
    ("americus_ga_us", "troy_al_us"),
    ("brunswick_ga_us", "tifton_ga_us"),
    ("fort_myers_fl_us", "port_saint_lucie_fl_us"),
    ("hinesville_ga_us", "valdosta_ga_us"),
    ("hinesville_ga_us", "tifton_ga_us"),
    ("brunswick_ga_us", "dublin_ga_us"),
    ("pensacola_fl_us", "selma_al_us"),
    ("crystal_river_fl_us", "spring_hill_fl_us"),
    ("crystal_river_fl_us", "gainesville_fl_us"),
    ("crystal_river_fl_us", "orlando_fl_us"),
    ("port_canaveral_fl_us", "palm_bay_fl_us"),
    ("port_canaveral_fl_us", "orlando_fl_us"),
    ("port_canaveral_fl_us", "daytona_beach_fl_us"),
    ("sebring_fl_us", "lakeland_fl_us"),
    ("sebring_fl_us", "orlando_fl_us"),
    ("sebring_fl_us", "sarasota_fl_us"),
    ("sebring_fl_us", "fort_myers_fl_us"),
    ("sebring_fl_us", "port_saint_lucie_fl_us"),
    ("gainesville_ga_us", "athens_ga_us"),
    ("gainesville_ga_us", "atlanta_ga_us"),
];

/// Pairs whose best route is still another road, with why.
const ANSWERED_ELSEWHERE: &[(&str, &str)] = &[];

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
fn every_new_leg_answers_the_pair_it_was_built_for() {
    let mut wrong = Vec::new();
    for &(a, b) in BUILT {
        let found = route(a, b);
        println!("{a} -> {b}: {:?} {:.1} mi", found.cities, found.miles());
        if ANSWERED_ELSEWHERE.contains(&(a, b)) {
            continue;
        }
        if found.cities != [a, b] {
            wrong.push(format!("{a} -> {b} goes {:?}", found.cities));
        }
    }
    assert!(wrong.is_empty(), "{wrong:#?}");
}

#[test]
fn alligator_alley_charges_one_plaza_each_way() {
    let west = drive(&["coral_springs_fl_us", "naples_fl_us"]);
    let east = drive(&["naples_fl_us", "coral_springs_fl_us"]);
    for trip in [&west, &east] {
        let alley: Vec<f64> = trip
            .toll_events()
            .iter()
            .filter(|t| t.name == "Alligator Alley toll")
            .map(|t| t.amount)
            .collect();
        assert_eq!(alley, [12.72], "{alley:?}");
    }
}

#[test]
fn the_turnpike_split_keeps_its_chain_near_the_old_miles() {
    let chain = drive(&[
        "west_palm_beach_fl_us",
        "port_saint_lucie_fl_us",
        "palm_bay_fl_us",
    ]);
    // The retired leg was 110 miles; the two pieces are 51 and 65.
    assert!((chain.miles() - 110.0).abs() <= 10.0, "{}", chain.miles());
}

#[test]
fn naples_to_miami_pays_one_alligator_alley_plaza_each_way() {
    // Each plaza collects on one carriageway, so a trip pays one: 12.72 for
    // five axles with SunPass, 14.04 by plate (FDOT, August 2026). It was
    // 25.44, both plazas added.
    for cities in [
        ["naples_fl_us", "miami_fl_us"],
        ["miami_fl_us", "naples_fl_us"],
    ] {
        let trip = drive(&cities);
        let alley: Vec<(f64, f64)> = trip
            .toll_events()
            .iter()
            .filter(|t| t.name == "Alligator Alley commercial toll")
            .map(|t| (t.amount, t.plate_amount()))
            .collect();
        assert_eq!(alley, [(12.72, 14.04)], "{cities:?}");
    }
}
