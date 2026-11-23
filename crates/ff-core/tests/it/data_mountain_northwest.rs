//! The Mountain West and Northwest (missing roads brief 3, 2026-11-23):
//! Aberdeen, Washington, 45 new legs, seven legs split at the cities on their
//! road, and Wenatchee to Everett put on US 2. The game's own route search
//! has to take each new leg for the pair it was built for, and a split leg is
//! gone with its chain in place.

use crate::data_support::world;
use ff_core::data::retired_legs::RETIRED_LEGS;

/// Every leg this region built, as stored.
const BUILT: &[(&str, &str)] = &[
    ("newberg_or_us", "woodburn_or_us"),
    ("mcminnville_or_us", "newport_or_us"),
    ("coos_bay_or_us", "newport_or_us"),
    ("the_dalles_or_us", "yakima_wa_us"),
    ("coos_bay_or_us", "eugene_or_us"),
    ("bend_or_us", "corvallis_or_us"),
    ("bend_or_us", "the_dalles_or_us"),
    ("coos_bay_or_us", "crescent_city_ca_us"),
    ("colorado_springs_co_us", "limon_co_us"),
    ("burlington_co_us", "lamar_co_us"),
    ("lamar_co_us", "pueblo_co_us"),
    ("lamar_co_us", "walsenburg_co_us"),
    ("limon_co_us", "sidney_ne_us"),
    ("burlington_co_us", "ogallala_ne_us"),
    ("pagosa_springs_co_us", "santa_fe_nm_us"),
    ("burlington_co_us", "sidney_ne_us"),
    ("alamosa_co_us", "silverthorne_co_us"),
    ("alamosa_co_us", "montrose_co_us"),
    ("laramie_wy_us", "wheatland_wy_us"),
    ("lusk_wy_us", "scottsbluff_ne_us"),
    ("casper_wy_us", "lusk_wy_us"),
    ("casper_wy_us", "rawlins_wy_us"),
    ("lusk_wy_us", "spearfish_sd_us"),
    ("gillette_wy_us", "lusk_wy_us"),
    ("clovis_nm_us", "tucumcari_nm_us"),
    ("clovis_nm_us", "plainview_tx_us"),
    ("carlsbad_nm_us", "van_horn_tx_us"),
    ("las_vegas_nm_us", "tucumcari_nm_us"),
    ("clovis_nm_us", "hobbs_nm_us"),
    ("alamogordo_nm_us", "socorro_nm_us"),
    ("carlsbad_nm_us", "fort_stockton_tx_us"),
    ("alamogordo_nm_us", "carlsbad_nm_us"),
    ("roswell_nm_us", "socorro_nm_us"),
    ("carlsbad_nm_us", "el_paso_tx_us"),
    ("lewiston_id_us", "walla_walla_wa_us"),
    ("lewiston_id_us", "moses_lake_wa_us"),
    ("lewiston_id_us", "missoula_mt_us"),
    ("kalispell_mt_us", "superior_mt_us"),
    ("libby_mt_us", "superior_mt_us"),
    ("cedar_city_ut_us", "page_az_us"),
    ("logan_ut_us", "twin_falls_id_us"),
    ("logan_ut_us", "rock_springs_wy_us"),
    ("longview_wa_us", "yakima_wa_us"),
    ("aberdeen_wa_us", "olympia_wa_us"),
    ("aberdeen_wa_us", "astoria_or_us"),
    ("astoria_or_us", "longview_wa_us"),
    ("clovis_nm_us", "lubbock_tx_us"),
    ("denver_co_us", "alamosa_co_us"),
    ("alamosa_co_us", "santa_fe_nm_us"),
    ("trinidad_co_us", "walsenburg_co_us"),
    ("burlington_co_us", "limon_co_us"),
    ("casper_wy_us", "wheatland_wy_us"),
    ("eugene_or_us", "the_dalles_or_us"),
    ("the_dalles_or_us", "tri_cities_wa_us"),
    ("wenatchee_wa_us", "everett_wa_us"),
];

/// The legs this region split, as they were stored.
const SPLIT: &[(&str, &str)] = &[
    ("astoria_or_us", "olympia_wa_us"),
    ("roswell_nm_us", "lubbock_tx_us"),
    ("denver_co_us", "albuquerque_nm_us"),
    ("trinidad_co_us", "pueblo_co_us"),
    ("burlington_co_us", "denver_co_us"),
    ("billings_mt_us", "cheyenne_wy_us"),
    ("eugene_or_us", "tri_cities_wa_us"),
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

/// A new leg never ends at the exit of the city it leaves.
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
                crate::sim_support::weather("rockies", 7),
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

#[test]
fn aberdeen_is_a_curated_market_with_a_store() {
    let city = world().cities.get("aberdeen_wa_us").expect("Aberdeen");
    assert_eq!(city.name, "Aberdeen");
    assert!(ff_core::data::world_constants::is_curated_complete_market(
        "aberdeen_wa_us"
    ));
    assert!(
        city.locations
            .iter()
            .any(|l| l.facility_type == "shopping_mall" && l.ships.is_empty()),
        "Aberdeen needs a site that receives dry freight"
    );
}
