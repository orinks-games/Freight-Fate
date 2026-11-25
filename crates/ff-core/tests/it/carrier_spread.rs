//! The carrier spread for Career 2.0 (brief 12): every lower-48 city has a
//! choice of regionals, every national has a terminal within reach, and the
//! big port and metro markets each have a local.

use ff_core::data::world::World;
use ff_core::models::carriers::carrier_catalog;
use ff_core::models::start_options::start_options_for_home_city;

use crate::data_support::world;

/// The farthest any lower-48 city sits from a national's nearest terminal.
/// About 400 air miles was the target. Terminals sit at real freight hubs
/// rather than wherever closes the last gap, so remote towns (Shelby and
/// Havre, Montana, the Upper Peninsula, northern Minnesota, Bangor) sit past
/// it; the worst is about 512 miles.
const NATIONAL_REACH_MI: f64 = 520.0;

fn lower_48_freight_cities(world: &World) -> Vec<String> {
    let mut keys: Vec<String> = world
        .cities
        .iter()
        .filter(|(_, c)| {
            c.country.eq_ignore_ascii_case("us")
                && !matches!(c.state_code.to_ascii_uppercase().as_str(), "AK" | "HI")
                && !c.no_freight
        })
        .map(|(k, _)| k.clone())
        .collect();
    keys.sort();
    keys
}

#[test]
fn test_every_lower_48_city_has_two_regionals_hiring() {
    let world = world();
    let mut thin = Vec::new();
    for city in lower_48_freight_cities(world) {
        let n = carrier_catalog()
            .values()
            .filter(|c| c.tier == "regional" && c.hires_in(world, &city))
            .count();
        if n < 2 {
            thin.push((city, n));
        }
    }
    assert!(thin.is_empty(), "fewer than two regionals: {thin:?}");
}

#[test]
fn test_every_national_has_a_terminal_within_reach_of_every_lower_48_city() {
    let world = world();
    let nationals: Vec<_> = carrier_catalog()
        .values()
        .filter(|c| c.is_national() && c.home_country() == "US")
        .collect();
    assert_eq!(nationals.len(), 6);
    for c in &nationals {
        assert!(
            (8..=12).contains(&c.terminal_city_keys.len()),
            "{} has {} terminals",
            c.key,
            c.terminal_city_keys.len()
        );
    }
    let mut far = Vec::new();
    for city in lower_48_freight_cities(world) {
        for c in &nationals {
            assert!(
                c.hires_in(world, &city),
                "{} does not hire in {city}",
                c.key
            );
            let mi = c.hiring_distance_mi(world, &city).unwrap();
            if mi > NATIONAL_REACH_MI {
                far.push((c.key.clone(), city.clone(), mi.round()));
            }
        }
    }
    assert!(far.is_empty(), "national terminal out of reach: {far:?}");
}

#[test]
fn test_each_big_market_has_a_local() {
    let world = world();
    for city in [
        "new_york_ny_us",
        "newark_nj_us",
        "savannah_ga_us",
        "houston_tx_us",
        "dallas_tx_us",
        "fort_worth_tx_us",
        "atlanta_ga_us",
        "seattle_wa_us",
        "tacoma_wa_us",
        "san_francisco_ca_us",
        "laredo_tx_us",
        "memphis_tn_us",
        "kansas_city_mo_us",
        "miami_fl_us",
        "norfolk_va_us",
        "detroit_mi_us",
        "chicago_il_us",
        "los_angeles_ca_us",
        "anchorage_ak_us",
    ] {
        assert!(
            carrier_catalog()
                .values()
                .any(|c| c.is_local() && c.hires_in(world, city)),
            "no local hires in {city}"
        );
    }
}

#[test]
fn test_every_terminal_city_has_a_garage() {
    // A home terminal sends the driver to its city's garage; Alaska cities
    // are served by the world's own garages there.
    let world = world();
    let mut missing = Vec::new();
    for c in carrier_catalog().values() {
        for t in &c.terminal_city_keys {
            let city = &world.cities[t];
            if city.state_code.eq_ignore_ascii_case("AK") {
                continue;
            }
            if world.city_service(t, "garage").is_err() {
                missing.push((c.key.clone(), t.clone()));
            }
        }
    }
    assert!(
        missing.is_empty(),
        "terminal city without a garage: {missing:?}"
    );
}

#[test]
fn test_picker_lengths_at_the_listening_check_cities() {
    // The brief's listening check: rows a driver hears (carriers; the
    // owner-operator row comes on top).
    let world = world();
    let carriers_at = |city: &str| {
        start_options_for_home_city(world, city)
            .iter()
            .filter(|o| o.is_company_driver())
            .count()
    };
    assert_eq!(carriers_at("new_york_ny_us"), 10);
    assert_eq!(carriers_at("chicago_il_us"), 12);
    assert_eq!(carriers_at("dallas_tx_us"), 11);
    assert_eq!(carriers_at("billings_mt_us"), 8);
    assert_eq!(carriers_at("anchorage_ak_us"), 2);
    for city in lower_48_freight_cities(world) {
        assert!(
            carriers_at(&city) <= 12,
            "{city} lists {}",
            carriers_at(&city)
        );
    }
}
