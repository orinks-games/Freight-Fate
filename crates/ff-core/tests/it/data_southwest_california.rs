//! California and the Southwest (missing roads brief 3, 2026-11-22): 35 new
//! legs, 16 legs split at the cities on their road, and Payson to Winslow put
//! on AZ 87. The game's own route search has to take each new leg for the
//! pair it was built for, and a split leg is gone with its chain in place.

use crate::data_support::world;
use ff_core::data::retired_legs::RETIRED_LEGS;

/// Every leg this region built, as stored.
const BUILT: &[(&str, &str)] = &[
    ("bakersfield_ca_us", "san_luis_obispo_ca_us"),
    ("flagstaff_az_us", "sedona_az_us"),
    ("riverside_ca_us", "santa_ana_ca_us"),
    ("carson_city_nv_us", "fallon_nv_us"),
    ("lancaster_ca_us", "los_angeles_ca_us"),
    ("barstow_ca_us", "ridgecrest_ca_us"),
    ("casa_grande_az_us", "globe_az_us"),
    ("ridgecrest_ca_us", "victorville_ca_us"),
    ("payson_az_us", "show_low_az_us"),
    ("blythe_ca_us", "lake_havasu_city_az_us"),
    ("bullhead_city_az_us", "las_vegas_nv_us"),
    ("chico_ca_us", "susanville_ca_us"),
    ("blythe_ca_us", "el_centro_ca_us"),
    ("globe_az_us", "tucson_az_us"),
    ("merced_ca_us", "salinas_ca_us"),
    ("modesto_ca_us", "salinas_ca_us"),
    ("bishop_ca_us", "tonopah_nv_us"),
    ("austin_nv_us", "tonopah_nv_us"),
    ("blythe_ca_us", "bullhead_city_az_us"),
    ("ukiah_ca_us", "yuba_city_ca_us"),
    ("reno_nv_us", "yuba_city_ca_us"),
    ("carson_city_nv_us", "sacramento_ca_us"),
    ("mount_shasta_ca_us", "susanville_ca_us"),
    ("fresno_ca_us", "san_luis_obispo_ca_us"),
    ("kingman_az_us", "prescott_az_us"),
    ("chico_ca_us", "willits_ca_us"),
    ("page_az_us", "saint_george_ut_us"),
    ("blythe_ca_us", "prescott_az_us"),
    ("barstow_ca_us", "bullhead_city_az_us"),
    ("buckeye_az_us", "lake_havasu_city_az_us"),
    ("fallon_nv_us", "tonopah_nv_us"),
    ("chico_ca_us", "fortuna_ca_us"),
    ("redding_ca_us", "ukiah_ca_us"),
    ("bishop_ca_us", "fallon_nv_us"),
    ("mammoth_lakes_ca_us", "modesto_ca_us"),
    ("los_angeles_ca_us", "santa_ana_ca_us"),
    ("santa_ana_ca_us", "oceanside_ca_us"),
    ("oceanside_ca_us", "san_diego_ca_us"),
    ("santa_rosa_ca_us", "fairfield_ca_us"),
    ("fairfield_ca_us", "stockton_ca_us"),
    ("mount_shasta_ca_us", "klamath_falls_or_us"),
    ("los_angeles_ca_us", "valencia_ca_us"),
    ("valencia_ca_us", "fresno_ca_us"),
    ("prescott_az_us", "camp_verde_az_us"),
    ("fairfield_ca_us", "sacramento_ca_us"),
    ("fairfield_ca_us", "san_francisco_ca_us"),
    ("valencia_ca_us", "lancaster_ca_us"),
    ("mojave_ca_us", "bakersfield_ca_us"),
    ("mojave_ca_us", "barstow_ca_us"),
    ("ridgecrest_ca_us", "lone_pine_ca_us"),
    ("visalia_ca_us", "san_luis_obispo_ca_us"),
    ("yuma_az_us", "buckeye_az_us"),
    ("payson_az_us", "winslow_az_us"),
];

/// The legs this region split, as they were stored.
const SPLIT: &[(&str, &str)] = &[
    ("los_angeles_ca_us", "san_diego_ca_us"),
    ("santa_rosa_ca_us", "stockton_ca_us"),
    ("redding_ca_us", "klamath_falls_or_us"),
    ("oxnard_ca_us", "santa_ana_ca_us"),
    ("los_angeles_ca_us", "fresno_ca_us"),
    ("prescott_az_us", "flagstaff_az_us"),
    ("san_jose_ca_us", "sacramento_ca_us"),
    ("sacramento_ca_us", "san_francisco_ca_us"),
    ("oceanside_ca_us", "los_angeles_ca_us"),
    ("santa_rosa_ca_us", "sacramento_ca_us"),
    ("oxnard_ca_us", "lancaster_ca_us"),
    ("lancaster_ca_us", "bakersfield_ca_us"),
    ("lancaster_ca_us", "barstow_ca_us"),
    ("ridgecrest_ca_us", "bishop_ca_us"),
    ("visalia_ca_us", "santa_maria_ca_us"),
    ("yuma_az_us", "phoenix_az_us"),
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

/// A short new leg that leaves its city on a freeway never ends at that
/// city's exit: Flagstaff to Sedona once crowned I-17 exit 337, 4.7 miles
/// out of Flagstaff, as the Sedona exit.
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
                crate::sim_support::weather("desert_southwest", 7),
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
