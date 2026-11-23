//! Alabama, Mississippi and South Carolina's missing roads (missing roads,
//! 2026-11-23): the game's own route search has to take each new leg for
//! the pair it was built for, and the I-55 split at Hammond keeps its chain
//! near the old leg's miles.

use crate::data_support::world;
use ff_core::data::world_models::Route;

/// Every leg the region list built, as `(from, to)`.
const BUILT: &[(&str, &str)] = &[
    ("jackson_ms_us", "greenville_ms_us"),
    ("florence_al_us", "memphis_tn_us"),
    ("jackson_ms_us", "tupelo_ms_us"),
    ("grenada_ms_us", "oxford_ms_us"),
    ("cullman_al_us", "gadsden_al_us"),
    ("gadsden_al_us", "rome_ga_us"),
    ("cullman_al_us", "florence_al_us"),
    ("gadsden_al_us", "huntsville_al_us"),
    ("dothan_al_us", "panama_city_fl_us"),
    ("columbus_ms_us", "meridian_ms_us"),
    ("gulfport_ms_us", "hammond_la_us"),
    ("clarksdale_ms_us", "stuttgart_ar_us"),
    ("gadsden_al_us", "lagrange_ga_us"),
    ("charleston_sc_us", "sumter_sc_us"),
    ("columbus_ms_us", "grenada_ms_us"),
    ("greenville_ms_us", "monroe_la_us"),
    ("florence_al_us", "jackson_tn_us"),
    ("florence_al_us", "tuscaloosa_al_us"),
    ("florence_al_us", "nashville_tn_us"),
    ("grenada_ms_us", "meridian_ms_us"),
    ("hattiesburg_ms_us", "natchez_ms_us"),
    ("beaufort_sc_us", "savannah_ga_us"),
    ("beaufort_sc_us", "charleston_sc_us"),
    ("pascagoula_ms_us", "mobile_al_us"),
    ("pascagoula_ms_us", "gulfport_ms_us"),
    ("jackson_ms_us", "hammond_la_us"),
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
fn jackson_to_baton_rouge_rides_its_chain_through_hammond() {
    let found = route("jackson_ms_us", "baton_rouge_la_us");
    assert_eq!(
        found.cities,
        ["jackson_ms_us", "hammond_la_us", "baton_rouge_la_us"],
        "{:?}",
        found.cities
    );
    // The retired leg was 175 miles; the pieces are 133 and 50.
    let chain = drive(&["jackson_ms_us", "hammond_la_us", "baton_rouge_la_us"]);
    assert!((chain.miles() - 175.0).abs() <= 10.0, "{}", chain.miles());
}
