//! A career that starts in Canada (brief 2, director rulings 2026-11-25):
//! Canadian carriers hire in Canada only, a Canadian city is a home only
//! when it has a first load, the North regional serves the territories, and
//! the owner-operator start has a Canadian twin leased to Longspur.

use ff_core::data::regions::canada_picker_group;
use ff_core::data::world::World;
use ff_core::models::carriers::{carrier, carrier_catalog, has_first_load, is_offerable_home_city};
use ff_core::models::start_options::{
    start_options_for_home_city, CANADIAN_OWNER_OPERATOR_START_KEY, OWNER_OPERATOR_START_KEY,
};

use crate::data_support::world;

fn is_ca(world: &World, key: &str) -> bool {
    world.cities[key].country.eq_ignore_ascii_case("ca")
}

fn state(world: &World, key: &str) -> String {
    world.cities[key].state_code.to_ascii_uppercase()
}

fn canadian_cities(world: &World) -> Vec<String> {
    let mut keys: Vec<String> = world
        .cities
        .keys()
        .filter(|k| is_ca(world, k))
        .cloned()
        .collect();
    keys.sort();
    keys
}

#[test]
fn every_carrier_hires_in_its_own_country_only() {
    let world = world();
    for c in carrier_catalog().values() {
        let canadian = c.home_country() == "CA";
        for key in world.cities.keys() {
            if c.hires_in(world, key) {
                assert_eq!(is_ca(world, key), canadian, "{} hires in {key}", c.key);
            }
        }
    }
}

#[test]
fn the_closed_canadian_homes_are_the_ones_without_a_first_load() {
    let world = world();
    let closed: Vec<String> = canadian_cities(world)
        .into_iter()
        .filter(|k| !is_offerable_home_city(world, k))
        .collect();
    // The ROADMAP's list: these wait for real freight sites.
    assert_eq!(
        closed,
        [
            "dawson_creek_bc_ca",
            "fort_nelson_bc_ca",
            "fort_st_john_bc_ca",
            "haines_junction_yt_ca",
            "kitimat_bc_ca",
            "sept_iles_qc_ca",
            "smithers_bc_ca",
            "terrace_bc_ca",
            "watson_lake_yt_ca",
        ]
    );
    for key in &closed {
        assert!(!has_first_load(&world.cities[key]), "{key}");
    }
    // The two cities given real freight sites are homes again, and Dawson
    // City keeps its North terminal because it passes the rule.
    for key in [
        "whitehorse_yt_ca",
        "prince_george_bc_ca",
        "dawson_city_yt_ca",
    ] {
        assert!(has_first_load(&world.cities[key]), "{key}");
        assert!(is_offerable_home_city(world, key), "{key}");
    }
}

#[test]
fn every_terminal_city_of_a_canadian_carrier_is_a_home() {
    let world = world();
    for c in carrier_catalog()
        .values()
        .filter(|c| c.home_country() == "CA")
    {
        for t in &c.terminal_city_keys {
            assert!(is_offerable_home_city(world, t), "{} terminal {t}", c.key);
        }
    }
}

#[test]
fn the_nationals_hire_in_the_ten_provinces_and_the_north_regional_in_the_territories() {
    let world = world();
    let longspur = carrier("longspur_transport").expect("Longspur");
    let liard = carrier("liard_river_freight").expect("Liard River");
    for key in canadian_cities(world) {
        let territory = matches!(state(world, &key).as_str(), "YT" | "NT" | "NU");
        assert_eq!(longspur.hires_in(world, &key), !territory, "{key}");
        if territory {
            assert!(liard.hires_in(world, &key), "{key}");
            for c in carrier_catalog().values() {
                if c.hires_in(world, &key) {
                    assert_eq!(c.key, liard.key, "{} hires in {key}", c.key);
                }
            }
        }
    }
}

#[test]
fn every_canadian_city_has_a_province_row_and_no_us_city_has_one() {
    let world = world();
    for (key, city) in &world.cities {
        let row = canada_picker_group(&city.state_code.to_ascii_uppercase());
        assert_eq!(row.is_some(), is_ca(world, key), "{key}");
    }
}

#[test]
fn the_owner_operator_start_has_a_canadian_twin_leased_to_longspur() {
    let world = world();
    let keys = |city: &str| -> Vec<&str> {
        start_options_for_home_city(world, city)
            .into_iter()
            .map(|o| o.key)
            .collect()
    };
    let toronto = keys("toronto_on_ca");
    assert!(toronto.contains(&CANADIAN_OWNER_OPERATOR_START_KEY));
    assert!(!toronto.contains(&OWNER_OPERATOR_START_KEY));
    assert_eq!(toronto.last(), Some(&CANADIAN_OWNER_OPERATOR_START_KEY));
    let chicago = keys("chicago_il_us");
    assert!(chicago.contains(&OWNER_OPERATOR_START_KEY));
    assert!(!chicago.contains(&CANADIAN_OWNER_OPERATOR_START_KEY));
    // Longspur does not hire in the territories, so neither does its lease.
    assert!(!keys("whitehorse_yt_ca").contains(&CANADIAN_OWNER_OPERATOR_START_KEY));

    let us = ff_core::models::start_options::start_option(Some(OWNER_OPERATOR_START_KEY));
    let ca = ff_core::models::start_options::start_option(Some(CANADIAN_OWNER_OPERATOR_START_KEY));
    assert_eq!(ca.label, us.label);
    assert_eq!(ca.menu_summary, us.menu_summary);
    assert_eq!(ca.help_text, us.help_text);
    assert_eq!(ca.carrier_name, "Longspur Transport");
}
