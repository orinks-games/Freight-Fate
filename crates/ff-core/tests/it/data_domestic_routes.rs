//! A route between two cities of one country stays in that country when a
//! road that does exists; only a trip that starts or ends abroad, or has no
//! domestic road at all, crosses the border.

use crate::data_support::world;
use ff_core::data::world_models::Route;

/// The countries a route passes through, in order, each run named once.
fn countries(route: &Route) -> Vec<String> {
    let world = world();
    let mut runs: Vec<String> = Vec::new();
    for key in &route.cities {
        let country = &world.cities[key.as_str()].country;
        if runs.last() != Some(country) {
            runs.push(country.clone());
        }
    }
    runs
}

fn route(a: &str, b: &str) -> Route {
    world()
        .supported_route(a, b, None)
        .unwrap()
        .unwrap_or_else(|| panic!("no supported route from {a} to {b}"))
}

#[test]
fn us_to_us_loads_stay_out_of_ontario_where_a_us_road_exists() {
    for (a, b) in [
        ("detroit_mi_us", "buffalo_ny_us"),
        ("chicago_il_us", "buffalo_ny_us"),
        ("detroit_mi_us", "syracuse_ny_us"),
        ("cleveland_oh_us", "burlington_vt_us"),
    ] {
        let found = route(a, b);
        assert_eq!(countries(&found), ["US"], "{a} to {b}: {:?}", found.cities);
    }
}

#[test]
fn a_border_city_pair_still_crosses_once() {
    let found = route("detroit_mi_us", "windsor_on_ca");
    assert_eq!(found.cities, ["detroit_mi_us", "windsor_on_ca"]);
}

#[test]
fn alaska_still_runs_through_canada_when_no_us_road_exists() {
    let found = route("bellingham_wa_us", "fairbanks_ak_us");
    assert_eq!(countries(&found), ["US", "CA", "US"], "{:?}", found.cities);
}

#[test]
fn the_alternatives_keep_the_rule() {
    let options = world()
        .supported_route_options("detroit_mi_us", "buffalo_ny_us", 3)
        .unwrap();
    assert!(!options.is_empty());
    for option in &options {
        assert_eq!(countries(option), ["US"], "{:?}", option.cities);
    }
}

#[test]
fn canada_to_canada_stays_in_canada() {
    for (a, b) in [
        ("windsor_on_ca", "niagara_falls_on_ca"),
        ("sarnia_on_ca", "montreal_qc_ca"),
        ("saint_jean_sur_richelieu_qc_ca", "sherbrooke_qc_ca"),
    ] {
        let found = route(a, b);
        assert_eq!(countries(&found), ["CA"], "{a} to {b}: {:?}", found.cities);
    }
}
