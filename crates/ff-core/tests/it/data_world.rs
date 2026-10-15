//! World data and route graph tests (port of `tests/test_world.py`).

use std::collections::{BTreeSet, HashSet};

use crate::data_support::{data_dir, shortest, supported, world};
use ff_core::data::regions::REGIONS;
use ff_core::data::world::{max_alternate_miles, World};
use ff_core::data::world_constants::{
    is_stand_in_market, lookup, set_contains, DEFAULT_POI_ACTIONS, FREIGHT_LOCATION_TYPES,
    PARKING_CERTAINTY_LABELS, POI_ACTIONS, STOP_DIRECTIONS, STOP_TYPE_LABELS,
};
use ff_core::data::world_models::{City, Route};
use serde_json::json;

// Every direct connection that existed in the 21-city 1.2.x map. Old
// mid-trip snapshots store these as consecutive route_cities pairs, so each
// one must remain a direct leg forever (or ship with a save migration).
const ORIGINAL_ADJACENT_PAIRS: &[(&str, &str)] = &[
    ("New York", "Boston"),
    ("New York", "Philadelphia"),
    ("Philadelphia", "Pittsburgh"),
    ("Pittsburgh", "Cleveland"),
    ("Cleveland", "Chicago"),
    ("Chicago", "Indianapolis"),
    ("Indianapolis", "Nashville"),
    ("Nashville", "Atlanta"),
    ("Indianapolis", "St. Louis"),
    ("Chicago", "St. Louis"),
    ("St. Louis", "Nashville"),
    ("St. Louis", "Kansas City"),
    ("Kansas City", "Denver"),
    ("Denver", "Salt Lake City"),
    ("Denver", "Albuquerque"),
    ("Albuquerque", "Phoenix"),
    ("Phoenix", "Los Angeles"),
    ("Salt Lake City", "Las Vegas"),
    ("Las Vegas", "Los Angeles"),
    ("Dallas", "Albuquerque"),
    ("Dallas", "St. Louis"),
    ("Atlanta", "Dallas"),
    ("Los Angeles", "San Francisco"),
    ("San Francisco", "Salt Lake City"),
    ("San Francisco", "Portland"),
    ("Portland", "Seattle"),
    ("Portland", "Salt Lake City"),
];

fn assert_route_sequence_and_paid_miles(
    world: &World,
    start: &str,
    end: &str,
    expected_cities: &[&str],
    expected_paid_miles: f64,
) {
    let route = world
        .shortest_route(start, end, None, false)
        .unwrap_or_else(|err| panic!("{start}→{end} route: {err}"))
        .unwrap_or_else(|| panic!("no route from {start} to {end}"));
    assert_eq!(
        route.cities,
        expected_cities
            .iter()
            .map(|city| (*city).to_string())
            .collect::<Vec<_>>()
    );
    assert!(
        (route.miles() - expected_paid_miles).abs() <= 0.5,
        "{start}→{end} paid miles: expected {expected_paid_miles}, got {}",
        route.miles()
    );
}

fn raw_world_leg<'a>(
    shards: &'a [serde_json::Value],
    from: &str,
    to: &str,
) -> &'a serde_json::Value {
    shards
        .iter()
        .flat_map(|shard| shard["legs"].as_array().expect("world data legs"))
        .find(|leg| leg["from"].as_str() == Some(from) && leg["to"].as_str() == Some(to))
        .unwrap_or_else(|| panic!("missing world data leg {from}->{to}"))
}

fn has_only_public_lots(city: &City) -> bool {
    !city.locations.is_empty()
        && city.locations.iter().all(|location| {
            matches!(
                location.facility_type.as_str(),
                "travel_center" | "truck_parking"
            ) && location.ships.is_empty()
                && location.receives.is_empty()
        })
}

#[test]
fn test_world_loads() {
    let world = world();
    assert!(world.cities.len() >= 45);
    assert!(world.legs.len() >= 80);
}

#[test]
fn test_every_city_reachable_from_everywhere() {
    let world = world();
    let names = world.city_names();
    let start = &names[0];
    for city in &names[1..] {
        let route = world
            .shortest_route(start, city, None, false)
            .unwrap()
            .unwrap_or_else(|| panic!("{city} unreachable from {start}"));
        assert_eq!(&route.cities[0], start);
        assert_eq!(route.cities.last().unwrap(), city);
    }
}

#[test]
fn test_route_legs_chain_correctly() {
    let world = world();
    let route = shortest(world, "New York", "Los Angeles");
    for (i, leg) in route.legs.iter().enumerate() {
        let pair: BTreeSet<&str> = [route.cities[i].as_str(), route.cities[i + 1].as_str()]
            .into_iter()
            .collect();
        let ends: BTreeSet<&str> = [leg.a.as_str(), leg.b.as_str()].into_iter().collect();
        assert_eq!(pair, ends);
    }
}

#[test]
fn test_route_options_are_distinct_and_sorted() {
    let world = world();
    let options = world
        .route_options("New York", "Los Angeles", 3, false)
        .unwrap();
    assert!(options.len() >= 2);
    let paths: HashSet<Vec<String>> = options.iter().map(|r| r.cities.clone()).collect();
    assert_eq!(paths.len(), options.len());
    let miles: Vec<f64> = options.iter().map(Route::miles).collect();
    let mut sorted = miles.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    assert_eq!(miles, sorted);
}

#[test]
fn test_route_options_reject_out_of_direction_detours() {
    let world = world();
    for (start, end) in [
        ("Philadelphia", "New York"), // Northeast Corridor freight
        ("Philadelphia", "Boston"),   // I-95 with plausible I-84 option
        ("Atlanta", "Dallas"),        // I-20, not a St. Louis loop
        ("Dallas", "Los Angeles"),    // Southwest corridors
        ("Denver", "Seattle"),        // I-80/I-84 or US-95/I-90
        ("New York", "Los Angeles"),  // long-haul alternatives still allowed
    ] {
        let best = shortest(world, start, end);
        let options = world.route_options(start, end, 5, false).unwrap();
        assert!(!options.is_empty());
        assert_eq!(options[0].cities, best.cities);
        assert!(options
            .iter()
            .all(|route| route.miles() <= max_alternate_miles(best.miles())));
    }
}

#[test]
fn test_northeast_corridors_prefer_i95_not_inland_loops() {
    let world = world();
    let philly_ny = world
        .route_options("Philadelphia", "New York", 5, false)
        .unwrap();
    // Best option is the direct I-95 hop. The NJ Turnpike alternate via
    // Trenton/Newark is the real I-95 corridor, so it is allowed; loops inland
    // through the Alleghenies (Pittsburgh/Harrisburg) or Great Lakes are not.
    assert_eq!(
        philly_ny[0].cities,
        vec!["philadelphia_pa_us", "new_york_ny_us"]
    );
    assert_eq!(philly_ny[0].highways(), vec!["I-95"]);
    let inland = [
        "pittsburgh_pa_us",
        "buffalo_ny_us",
        "harrisburg_pa_us",
        "scranton_pa_us",
        "binghamton_ny_us",
    ];
    assert!(philly_ny
        .iter()
        .all(|route| !route.cities.iter().any(|c| inland.contains(&c.as_str()))));

    let philly_boston = world
        .route_options("Philadelphia", "Boston", 5, false)
        .unwrap();
    assert_eq!(
        philly_boston[0].cities,
        vec!["philadelphia_pa_us", "new_york_ny_us", "boston_ma_us"]
    );
    assert_eq!(philly_boston[0].highways(), vec!["I-95"]);
    assert!(philly_boston
        .iter()
        .all(|route| !route.cities.iter().any(|c| c == "pittsburgh_pa_us")));
    assert!(philly_boston
        .iter()
        .all(|route| !route.cities.iter().any(|c| c == "buffalo_ny_us")));
}

#[test]
fn test_shortest_route_is_actually_shortest() {
    let world = world();
    let direct = shortest(world, "New York", "Boston");
    // New York's node is the Hunts Point (Bronx) freight hub, so I-95 legs land
    // there rather than at lower Manhattan; the direct hop is 205 mi.
    assert_eq!(direct.miles(), 205.0);
    assert_eq!(direct.legs.len(), 1);
}

#[test]
fn test_unknown_city_raises() {
    let world = world();
    assert!(world
        .shortest_route("New York", "Atlantis", None, false)
        .is_err());
}

#[test]
fn test_every_city_has_locations_with_known_cargo() {
    // The `CARGO_CATALOG` membership checks wait for `models::jobs`; every
    // other assertion of the Python test is live here.
    let world = world();
    for city in world.cities.values() {
        if city.no_freight {
            assert!(
                city.locations.is_empty(),
                "{} must not invent freight locations",
                city.name
            );
            continue;
        }
        assert!(
            !city.locations.is_empty(),
            "{} has no freight locations",
            city.name
        );
        for loc in &city.locations {
            assert!(!loc.id.is_empty(), "{} has no stable id", loc.name);
            assert_eq!(loc.city, city.key);
            assert!(
                set_contains(FREIGHT_LOCATION_TYPES, &loc.facility_type),
                "unknown location type {}",
                loc.facility_type
            );
            assert!(!loc.spoken_name().is_empty());
            assert!(!loc.source_note.is_empty());
            let service_only = matches!(
                loc.facility_type.as_str(),
                "travel_center" | "truck_parking"
            );
            if service_only {
                // Fuel / parking / rest pins: no freight roles by design.
                assert!(loc.ships.is_empty() && loc.receives.is_empty());
                assert!(loc.roles.is_empty());
            } else {
                assert!(!loc.roles.is_empty());
                assert!(!loc.ships.is_empty() || !loc.receives.is_empty());
            }
        }
    }
}

#[test]
fn test_world_yard_pins_are_freight_endpoints_not_homes() {
    // World terminal/company_yard pins only name freight endpoints (and the
    // fallback garage); homes are the hiring carrier's own terminals.
    let world = world();
    assert_eq!(world.yard_pin_name("Nashville"), Some("Music City Freight"));
    assert_eq!(world.yard_pin_name("Chicago"), Some("Chicago Company Yard"));
    // Healy's only curated pin is Fisher Fuel (travel_center): no yard pin,
    // no default freight facility, and no borrowed Nenana stand-in.
    assert_eq!(world.yard_pin_name("healy_ak_us"), None);
    let err = world
        .default_facility("healy_ak_us")
        .expect_err("Healy has no company_yard/terminal on its own pin");
    assert!(
        err.to_string().contains("no freight facilities"),
        "unexpected default_facility error: {err}"
    );
    assert!(world
        .yard_pin_name("anchorage_ak_us")
        .is_some_and(|name| name.contains("Port of Alaska")));
}

#[test]
fn test_home_base_offerability_follows_carrier_hiring() {
    use ff_core::models::carriers::{carrier, home_terminal_city_for, is_offerable_home_city};
    let world = world();
    // Healy: nationals hire in the lower 48 only; the Alaska regional hires
    // there out of Fairbanks, so it is offered. Canada stays closed.
    assert!(is_offerable_home_city(world, "healy_ak_us"));
    assert!(!is_offerable_home_city(world, "whitehorse_yt_ca"));
    assert!(!is_offerable_home_city(world, "surrey_bc_ca"));
    // Chicago: offered, into Northstar's own Chicago terminal.
    assert!(is_offerable_home_city(world, "chicago_il_us"));
    let northstar = carrier("northstar").expect("northstar");
    let home = home_terminal_city_for(Some(northstar), world, "Chicago").expect("home");
    assert_eq!(home, "chicago_il_us");
    let terminal = northstar.home_terminal(world, &home).expect("terminal");
    assert_eq!(terminal.name, "Northstar Freight Lines Chicago terminal");
    assert_eq!(terminal.service_area(), "Chicago, Illinois");
}

#[test]
fn test_regional_home_terminal_stays_inside_the_hiring_radius() {
    use ff_core::models::carriers::{carrier, carrier_catalog, home_terminal_city_for};
    let world = world();
    let prairie = carrier("prairie_link").expect("prairie link");
    // Out of range (and, for Surrey, out of country): no Prairie Link home,
    // never KC or Omaha.
    for city in ["seattle_wa_us", "surrey_bc_ca"] {
        assert!(world.cities.contains_key(city), "{city} missing");
        assert_eq!(
            home_terminal_city_for(Some(prairie), world, city),
            None,
            "{city}"
        );
    }
    assert_eq!(
        home_terminal_city_for(Some(prairie), world, "wichita_ks_us").as_deref(),
        Some("wichita_ks_us")
    );
    // No BC or Yukon city is ever a home terminal, and an Alaska terminal
    // only ever homes an Alaska career, from an Alaska carrier.
    for c in carrier_catalog().values() {
        for city in [
            "healy_ak_us",
            "anchorage_ak_us",
            "surrey_bc_ca",
            "whitehorse_yt_ca",
        ] {
            if let Some(home) = home_terminal_city_for(Some(c), world, city) {
                let st = world.cities[&home].state_code.to_ascii_uppercase();
                assert!(!matches!(st.as_str(), "BC" | "YT"), "{} -> {home}", c.key);
                if st == "AK" {
                    assert!(city.ends_with("_ak_us"), "{} -> {home}", c.key);
                    assert!(!c.is_national(), "{} -> {home}", c.key);
                }
            }
        }
    }
}

#[test]
fn test_freight_location_categories_are_live() {
    let world = world();
    let types: HashSet<&str> = world
        .cities
        .values()
        .flat_map(|city| city.locations.iter().map(|loc| loc.facility_type.as_str()))
        .collect();
    for expected in [
        "air_cargo",
        "automotive_plant",
        "chemical_petroleum_terminal",
        "cold_storage",
        "company_yard",
        "construction_materials_yard",
        "cross_dock",
        "dry_warehouse",
        "farm_elevator",
        "food_processor",
        "grocery_retail_dc",
        "intermodal_ramp",
        "lumber_paper",
        "manufacturing_plant",
        "mine_quarry",
        "parcel_hub",
        "port_terminal",
        "steel_industrial",
    ] {
        assert!(types.contains(expected), "{expected}");
    }
}

#[test]
fn test_each_metro_expands_to_representative_facilities() {
    let world = world();
    for city in world.cities.values() {
        if city.no_freight {
            assert!(
                city.locations.is_empty(),
                "{} must not invent freight locations",
                city.name
            );
            continue;
        }
        // Floor is 5, not 6: the geography gate can strip both the template
        // port terminal and the intermodal ramp from a remote town (rural
        // Nevada keeps five). Inventing a different filler facility to hold
        // the count at six would repeat the realism bug the gate fixes.
        // A STAND-IN market is the exception, and it is the point: not one of
        // its facilities has an endpoint the freight-site screen accepts, so it
        // is stamped with one company yard instead of four invented warehouses
        // (owner ruling, 2026-09-20).
        let templates: Vec<&str> = city
            .locations
            .iter()
            .filter(|loc| loc.template)
            .map(|loc| loc.facility_type.as_str())
            .collect();
        let curated = city.locations.iter().any(|loc| !loc.template);
        if is_stand_in_market(&city.key) {
            let parking_only_curated = curated
                && city.locations.iter().all(|loc| {
                    matches!(
                        loc.facility_type.as_str(),
                        "company_yard" | "terminal" | "travel_center" | "truck_parking"
                    )
                })
                && !city.locations.iter().any(|loc| loc.template);
            if parking_only_curated {
                // ALCAN: public truck lots / travel centers stay thin pins —
                // no invented warehouse skyline on a stand-in market.
                assert!(
                    city.locations.len() <= 2,
                    "{}: parking-only stand-in should stay thin, not {:?}",
                    city.name,
                    city.locations
                        .iter()
                        .map(|l| l.facility_type.as_str())
                        .collect::<Vec<_>>()
                );
            } else if !curated {
                assert!(
                    templates.len() <= 1,
                    "{}: a stand-in market gets one yard, not {:?}",
                    city.name,
                    templates
                );
                assert!(templates.iter().all(|kind| *kind == "company_yard"));
            } else if city.locations.iter().any(|loc| loc.template) {
                // Stand-in list + ordinary curated freight (Winona, …): full market.
                assert!(city.locations.len() >= 5);
            } else {
                // Curated-complete stand-in (Fairbanks job-board pins): authored
                // freight only — no invented warehouse skyline.
                assert!(
                    city.locations.iter().all(|loc| !loc.template),
                    "{}: curated-complete stand-in must not stamp templates",
                    city.name
                );
                assert!(
                    city.locations.len() >= 2,
                    "{}: curated-complete stand-in should keep authored pins",
                    city.name
                );
            }
        } else {
            assert!(
                city.locations.len() >= 5,
                "{}: metro must expand to representative facilities",
                city.name
            );
        }
        assert!(!city.market_tags.is_empty());
        let parking_only_curated = is_stand_in_market(&city.key)
            && curated
            && city.locations.iter().all(|loc| {
                matches!(
                    loc.facility_type.as_str(),
                    "company_yard" | "terminal" | "travel_center" | "truck_parking"
                )
            })
            && !city.locations.iter().any(|loc| loc.template);
        let curated_complete_stand_in = is_stand_in_market(&city.key)
            && curated
            && !city.locations.iter().any(|loc| loc.template)
            && city.locations.iter().any(|loc| {
                !matches!(
                    loc.facility_type.as_str(),
                    "company_yard" | "terminal" | "travel_center" | "truck_parking"
                )
            });
        if !parking_only_curated && !curated_complete_stand_in {
            assert!(city.locations.iter().any(|loc| loc.template));
        }
        assert!(city.locations.iter().any(|loc| {
            matches!(
                loc.facility_type.as_str(),
                "company_yard" | "terminal" | "travel_center" | "truck_parking"
            )
        }));
    }
}

#[test]
fn test_route_stops_have_trucker_relevant_types() {
    let world = world();
    let route = shortest(world, "San Antonio", "Dallas");
    let stops = route.stop_details();
    assert!(!stops.is_empty());
    assert!(stops
        .iter()
        .all(|stop| lookup(STOP_TYPE_LABELS, &stop.stop_type).is_some()));
    assert!(stops
        .iter()
        .any(|stop| stop.spoken_name().starts_with("travel center:")));
    assert!(stops.iter().all(|stop| !stop.source.is_empty()));
    assert!(stops.iter().all(|stop| !stop.actions.is_empty()));
    assert!(stops.iter().all(|stop| stop.curated()));
    assert!(stops
        .iter()
        .all(|stop| lookup(PARKING_CERTAINTY_LABELS, &stop.parking).is_some()));
    assert!(stops.iter().all(|stop| stop
        .directions
        .iter()
        .all(|d| set_contains(STOP_DIRECTIONS, d))));
    assert!(stops
        .iter()
        .all(|stop| stop.actions.iter().all(|a| set_contains(POI_ACTIONS, a))));
    assert!(stops.iter().all(|stop| {
        let defaults = lookup(DEFAULT_POI_ACTIONS, &stop.stop_type).unwrap();
        stop.actions.iter().all(|a| defaults.contains(&a.as_str()))
    }));

    let parking_route = shortest(world, "Los Angeles", "San Diego");
    assert!(parking_route
        .stop_details()
        .iter()
        .any(|stop| stop.stop_type == "public_rest_area"));
}

#[test]
fn test_public_rest_areas_do_not_imply_repair() {
    let world = world();
    let rest_area_actions: Vec<&Vec<String>> = world
        .legs
        .iter()
        .flat_map(|leg| leg.stops.iter())
        .filter(|stop| stop.stop_type == "public_rest_area")
        .map(|stop| &stop.actions)
        .collect();
    assert!(!rest_area_actions.is_empty());
    assert!(rest_area_actions
        .iter()
        .all(|actions| !actions.iter().any(|a| a == "repair")));
    assert!(rest_area_actions
        .iter()
        .all(|actions| !actions.iter().any(|a| a == "roadside_assistance")));
}

#[test]
fn test_route_stops_have_explicit_valid_positions() {
    let world = world();
    for leg in &world.legs {
        for stop in &leg.stops {
            assert!(
                0.0 < stop.at_mi && stop.at_mi < leg.miles,
                "{}-{}: {stop:?}",
                leg.a,
                leg.b
            );
            assert!(!stop.directions.is_empty());
            assert!(!stop.parking.is_empty());
        }
    }
}

#[test]
fn test_no_placeholder_pois_remain_in_current_route_network() {
    let world = world();
    let placeholders: Vec<(String, String, String)> = world
        .legs
        .iter()
        .flat_map(|leg| {
            leg.stops
                .iter()
                .filter(|stop| !stop.curated())
                .map(move |stop| (leg.a.clone(), leg.b.clone(), stop.name.clone()))
        })
        .collect();
    assert!(placeholders.is_empty(), "{placeholders:?}");

    let route = supported(world, "Memphis", "Nashville");
    assert!(route.metadata_complete(world));
}

#[test]
fn test_poi_names_are_curated_not_raw_osm_dump() {
    // Slow: `toll_events()` forces the corridor parse of every leg.
    let world = world();
    let raw_markers = [
        "osm_id",
        "amenity=",
        "highway=",
        "operator=",
        "node/",
        "way/",
    ];
    for city in world.cities.values() {
        for facility in &city.locations {
            let lowered = facility.name.to_lowercase();
            assert!(
                !raw_markers.iter().any(|m| lowered.contains(m)),
                "{}",
                facility.name
            );
            assert!(!facility.spoken_name().is_empty());
        }
    }
    for leg in &world.legs {
        for stop in &leg.stops {
            let lowered = stop.name.to_lowercase();
            assert!(
                !raw_markers.iter().any(|m| lowered.contains(m)),
                "{}",
                stop.name
            );
            assert!(!stop.spoken_name().is_empty());
        }
        for toll in leg.toll_events() {
            let lowered = toll.name.to_lowercase();
            assert!(
                !raw_markers.iter().any(|m| lowered.contains(m)),
                "{}",
                toll.name
            );
        }
    }
}

fn two_city_fixture(a_locations: serde_json::Value, legs: serde_json::Value) -> serde_json::Value {
    json!({
        "cities": {
            "A": {"state": "One", "region": "midwest", "lat": 40, "lon": -90, "locations": a_locations},
            "B": {
                "state": "One", "region": "midwest", "lat": 41, "lon": -91,
                "locations": [{"name": "B Yard", "type": "terminal", "cargo": ["general"]}],
            },
        },
        "legs": legs,
    })
}

#[test]
fn test_world_rejects_raw_source_text_in_player_poi_name() {
    let data = two_city_fixture(
        json!([{"name": "A Yard", "type": "terminal", "cargo": ["general"]}]),
        json!([{
            "from": "A", "to": "B", "miles": 80, "highway": "I-1", "terrain": "flat",
            "stops": [{"name": "amenity=fuel node/123", "type": "travel_center", "at_mi": 30, "source": "fixture"}],
        }]),
    );
    let err = World::from_value(data).expect_err("raw OSM text is refused");
    assert!(err.to_string().contains("raw OSM"), "{err}");
}

#[test]
fn test_world_rejects_raw_source_text_in_player_facility_name() {
    let data = two_city_fixture(
        json!([{"name": "warehouse way/123", "type": "terminal", "cargo": ["general"]}]),
        json!([]),
    );
    let err = World::from_value(data).expect_err("raw source text is refused");
    assert!(err.to_string().contains("raw source text"), "{err}");
}

#[test]
fn test_repair_action_requires_matching_service_metadata() {
    let data = two_city_fixture(
        json!([{"name": "A Yard", "type": "terminal", "cargo": ["general"]}]),
        json!([{
            "from": "A", "to": "B", "miles": 80, "highway": "I-1", "terrain": "flat",
            "stops": [{
                "name": "Example Service Plaza", "type": "service_plaza", "at_mi": 30,
                "source": "fixture source names emergency service provider",
                "actions": ["park", "save", "repair"], "services": ["parking"],
            }],
        }]),
    );
    let err = World::from_value(data).expect_err("repair without service metadata is refused");
    assert!(
        err.to_string().contains("matching source-backed service"),
        "{err}"
    );
}

#[test]
fn test_explicit_roadside_assistance_service_can_extend_plaza_actions() {
    let data = two_city_fixture(
        json!([{"name": "A Yard", "type": "terminal", "cargo": ["general"]}]),
        json!([{
            "from": "A", "to": "B", "miles": 80, "highway": "I-1", "terrain": "flat",
            "stops": [{
                "name": "Example Turnpike Service Plaza", "type": "service_plaza", "at_mi": 30,
                "source": "fixture source names authorized emergency road service",
                "actions": ["park", "save", "fuel", "break", "roadside_assistance"],
                "services": ["diesel", "parking", "roadside_assistance"],
            }],
        }]),
    );
    let world = World::from_value(data).unwrap();
    let stop = &world.legs[0].stops[0];
    assert!(stop.actions.iter().any(|a| a == "roadside_assistance"));
    assert!(stop.services.iter().any(|s| s == "roadside_assistance"));
}

#[test]
fn test_baked_completeness_flag_matches_field_computed_metadata() {
    // The route graph gates dispatch on a completeness flag baked at load from
    // raw corridor counts, so it never parses deferred detail. That flag must
    // agree exactly with computing it from a fully materialized leg -- if the two
    // ever drift, dispatch would offer or hide the wrong routes.
    //
    // Slow: forces the corridor parse of every leg.
    let world = world();
    let mut checked = 0;
    for leg in &world.legs {
        let flag = leg.meta_complete.expect("world legs carry the baked flag");
        let from_state = &world.cities[&leg.a].state;
        let to_state = &world.cities[&leg.b].state;
        // Force a full parse, then recompute from the real fields and compare.
        let field_computed = leg.metadata_complete_from_fields(from_state, to_state);
        assert_eq!(flag, field_computed, "{}->{}", leg.a, leg.b);
        checked += 1;
    }
    assert!(checked > 1000);
}

#[test]
fn test_lazy_leg_defers_corridor_until_read() {
    // A freshly loaded world leg carries no parsed corridor tuples; the first
    // read builds and caches them, and later reads hit the plain instance
    // attribute.
    let fresh = World::load_from(&data_dir()).unwrap();
    let leg = fresh
        .legs
        .iter()
        .find(|leg| leg.a == "chicago_il_us" && leg.b == "indianapolis_in_us")
        .unwrap();
    assert!(!leg.corridor_is_built());
    assert!(leg.has_deferred_source());

    let points = leg.route_points();
    assert!(!points.is_empty()); // materialized on first read
    assert!(leg.corridor_is_built());
    assert!(!leg.has_deferred_source());
    // Idempotent: a second read is the same cached object.
    assert!(std::ptr::eq(
        leg.grade_segments().as_ptr(),
        leg.grade_segments().as_ptr()
    ));
}

#[test]
fn test_corridor_metadata_supports_offline_itineraries() {
    let world = world();
    let route = world
        .route_from_cities(&["Chicago", "Indianapolis"])
        .unwrap();
    let leg = &route.legs[0];

    assert!(!leg.route_points().is_empty());
    assert_eq!(leg.route_points()[0].at_mi, 0.0);
    assert_eq!(leg.route_points().last().unwrap().at_mi, leg.miles);
    assert!(!leg.elevation_samples().is_empty());
    assert!(leg.elevation_samples()[0].elevation_ft > 500.0);
    assert!(!leg.grade_segments().is_empty());
    let terrains: HashSet<&str> = leg
        .grade_segments()
        .iter()
        .map(|s| s.terrain.as_str())
        .collect();
    assert_eq!(terrains, HashSet::from(["flat"]));
    let worst = leg
        .grade_segments()
        .iter()
        .map(|s| s.avg_grade_pct.abs())
        .fold(0.0, f64::max);
    assert!(worst < 0.2);
    let crossings: Vec<&str> = leg
        .state_crossings()
        .iter()
        .map(|c| c.state.as_str())
        .collect();
    assert_eq!(crossings, vec!["Indiana"]);
    // The OSM state-boundary relation intersects the archived I-65 route here.
    assert_eq!(leg.state_crossings()[0].at_mi, 26.7);
    assert!(leg.checkpoints().iter().any(|c| c.name == "Lafayette"));
    let total: f64 = leg.state_miles().iter().map(|m| m.miles).sum();
    assert_eq!(total, leg.miles);
}

#[test]
fn test_supported_routes_require_complete_corridor_metadata() {
    let world = world();
    for (start, end) in [
        ("Chicago", "Indianapolis"),
        ("Chicago", "St. Louis"),
        ("Memphis", "Little Rock"),
        ("San Antonio", "Dallas"),
        ("Des Moines", "Chicago"),
        ("Phoenix", "Los Angeles"),
        ("Denver", "Salt Lake City"),
        ("New York", "Boston"),
        ("Indianapolis", "Nashville"),
        ("Nashville", "Atlanta"),
        ("Kansas City", "Denver"),
        ("Dallas", "Albuquerque"),
    ] {
        let route = supported(world, start, end);
        assert!(route.metadata_complete(world));
    }

    for leg in &world.legs {
        // Dispatch requires routing metadata only; POIs are additive.
        assert!(world.leg_metadata_complete(leg), "{}-{}", leg.a, leg.b);
        let curated: Vec<_> = leg.stops.iter().filter(|stop| stop.curated()).collect();
        // Any POIs that are present must still be valid (source/actions/parking).
        assert!(
            curated.iter().all(|stop| !stop.source.is_empty()),
            "{}-{}",
            leg.a,
            leg.b
        );
        assert!(
            curated.iter().all(|stop| !stop.actions.is_empty()),
            "{}-{}",
            leg.a,
            leg.b
        );
        assert!(
            curated.iter().all(|stop| {
                stop.parking != "unknown"
                    || (stop.parking_spaces == 0
                        && stop.actions.iter().all(|action| action == "fuel"))
            }),
            "{}-{}",
            leg.a,
            leg.b
        );
        let route = world
            .route_from_cities(&[leg.a.as_str(), leg.b.as_str()])
            .unwrap();
        assert!(
            route.stop_details().iter().all(|stop| stop.curated()),
            "{}-{}",
            leg.a,
            leg.b
        );
    }
}

#[test]
fn test_tier_one_priority_corridors_keep_multi_stop_curated_fuel_support() {
    let world = world();
    for ((start, end), minimum_stops) in [
        (("Atlanta", "Dallas"), 3),
        (("Dallas", "Albuquerque"), 3),
        (("Dallas", "St. Louis"), 3),
        (("Kansas City", "Denver"), 3),
        (("San Francisco", "Salt Lake City"), 3),
        (("San Francisco", "Portland"), 3),
        (("Portland", "Salt Lake City"), 3),
    ] {
        let route = supported(world, start, end);
        let curated = route.stop_details();
        let fuel_capable: Vec<_> = curated
            .iter()
            .filter(|stop| stop.actions.iter().any(|a| a == "fuel"))
            .collect();
        assert!(curated.len() >= minimum_stops, "{start}-{end}");
        assert!(fuel_capable.len() >= 2, "{start}-{end}");
        assert!(
            curated.iter().any(|stop| stop.parking == "confirmed"),
            "{start}-{end}"
        );
        assert!(curated.iter().all(|stop| stop.curated()), "{start}-{end}");
    }
}

#[test]
fn test_southern_hos_pressure_corridors_have_added_safe_stops() {
    let world = world();
    let expected: &[((&str, &str), &[&str])] = &[
        (
            ("Dallas", "Albuquerque"),
            &[
                "Love's Travel Stop Wichita Falls",
                "Flying J Travel Center Tucumcari",
            ],
        ),
        (
            ("Dallas", "St. Louis"),
            &["Love's Travel Stop Ardmore", "Love's Travel Stop Rolla"],
        ),
        (
            ("Atlanta", "Dallas"),
            &[
                "Pilot Travel Center Tallapoosa",
                "Love's Travel Stop Heflin",
            ],
        ),
        (("Nashville", "Atlanta"), &["Flying J Travel Center Resaca"]),
    ];
    for ((start, end), names) in expected {
        let route = if *start == "Dallas" && *end == "St. Louis" {
            world
                .route_from_cities(&["Dallas", "St. Louis"])
                .expect("Dallas-St. Louis direct corridor")
        } else {
            supported(world, start, end)
        };
        let stops = route.stop_details();
        for name in *names {
            let stop = stops
                .iter()
                .find(|stop| stop.name == *name)
                .unwrap_or_else(|| panic!("{name} missing on {start}-{end}"));
            assert!(stop.curated());
            assert_eq!(stop.parking, "confirmed");
            for action in ["park", "save", "fuel", "break", "sleep"] {
                assert!(
                    stop.actions.iter().any(|a| a == action),
                    "{name} lacks {action}"
                );
            }
            assert!(stop.source.contains("2026-06-18"));
        }
    }
}

#[test]
fn route_stop_details_map_reversed_leg_miles() {
    let route = supported(world(), "Dallas", "St. Louis");
    let (index, leg, source_stop) = route
        .legs
        .iter()
        .enumerate()
        .find_map(|(index, leg)| {
            if index == 0 || route.cities[index] != leg.b {
                return None;
            }
            leg.stops
                .iter()
                .find(|stop| stop.curated())
                .map(|stop| (index, leg, stop))
        })
        .expect("a reversed supported-route leg with a curated stop");
    let route_offset: f64 = route.legs[..index].iter().map(|leg| leg.miles).sum();
    let expected_mi = route_offset + leg.miles - source_stop.at_mi;
    let route_stop = route
        .stop_details()
        .into_iter()
        .find(|stop| stop.name == source_stop.name && (stop.at_mi - expected_mi).abs() < 1e-9)
        .expect("the reversed stop is positioned from the route start");
    assert!((route_stop.at_mi - expected_mi).abs() < 1e-9);
}

#[test]
fn test_southern_sleep_stop_gaps_are_no_longer_extreme() {
    let world = world();
    let max_sleep_gap = |start: &str, end: &str| -> f64 {
        let route = supported(world, start, end);
        let mut points = vec![0.0];
        points.extend(
            route
                .stop_details()
                .into_iter()
                .filter(|stop| stop.actions.iter().any(|action| action == "sleep"))
                .map(|stop| stop.at_mi),
        );
        points.push(route.miles());
        points.sort_by(|a, b| a.partial_cmp(b).unwrap());
        points.windows(2).map(|w| w[1] - w[0]).fold(0.0, f64::max)
    };

    assert!(max_sleep_gap("Dallas", "Albuquerque") < 180.0);
    assert!(max_sleep_gap("Dallas", "St. Louis") < 200.0);
    assert!(max_sleep_gap("Atlanta", "Dallas") < 215.0);
    assert!(max_sleep_gap("Nashville", "Atlanta") < 120.0);
}

#[test]
fn test_a_leg_with_only_bobtail_stops_carries_the_rest_areas_on_its_road() {
    // Two bobtail-only Kwik Trips met the stop minimum for I-35 Owatonna to
    // Minneapolis, so the federal inventory's own rest areas on that road
    // were never curated in and the cab answered "no sleep-capable route
    // stop ahead" (owner, 2026-09-18). A loaded truck must find a night's
    // parking on the leg.
    let world = world();
    let route = supported(world, "Owatonna", "Minneapolis");
    let stops = route.stop_details();
    let sleepable: Vec<&str> = stops
        .iter()
        .filter(|stop| {
            stop.actions.iter().any(|a| a == "sleep") && stop.vehicle_access != "bobtail_only"
        })
        .map(|stop| stop.name.as_str())
        .collect();
    assert!(
        sleepable.contains(&"Heath Creek"),
        "Heath Creek rest area missing from I-35 north of Owatonna: {sleepable:?}"
    );
    let heath = stops
        .iter()
        .find(|stop| stop.name == "Heath Creek")
        .unwrap();
    assert_eq!(heath.stop_type, "public_rest_area");
    assert_eq!(heath.parking, "confirmed");
    assert!(heath.source.contains("Jason's Law"), "{}", heath.source);
}

#[test]
fn test_toll_metadata_is_explicit_and_separate_from_service_plazas() {
    let world = world();
    let route = world
        .route_from_cities(&["New York", "Philadelphia"])
        .unwrap();
    let tolls = route.toll_events();
    assert!(!tolls.is_empty());
    assert!(route.estimated_tolls() > 0.0);

    let toll_names: HashSet<&str> = tolls.iter().map(|event| event.name.as_str()).collect();
    let stop_names: HashSet<&str> = route
        .legs
        .iter()
        .flat_map(|leg| leg.stops.iter().map(|stop| stop.name.as_str()))
        .collect();
    assert!(toll_names.is_disjoint(&stop_names));

    let event = tolls
        .iter()
        .find(|event| event.road.starts_with("New Jersey Turnpike"))
        .expect("the Turnpike is priced");
    assert_eq!(event.authority, "New Jersey Turnpike Authority");
    assert_eq!(event.method, "ezpass");
    assert!(event.amount > 0.0);
    assert!(!event.estimated);
    assert!(event.source.to_lowercase().contains("toll"));

    let plazas: Vec<_> = route
        .legs
        .iter()
        .flat_map(|leg| leg.stops.iter())
        .filter(|stop| stop.stop_type == "service_plaza")
        .collect();
    assert!(!plazas.is_empty());
    assert!(plazas
        .iter()
        .all(|plaza| plaza.actions.iter().any(|a| a == "fuel")));
}

#[test]
fn test_world_rejects_missing_stop_position() {
    let data = json!({
        "cities": {
            "A": {"state": "Test", "region": "midwest", "locations": []},
            "B": {"state": "Test", "region": "midwest", "locations": []},
        },
        "legs": [{
            "from": "A", "to": "B", "miles": 100, "highway": "I-1", "terrain": "flat",
            "stops": ["Synthetic midpoint"],
        }],
    });
    let err = World::from_value(data).expect_err("a stop without at_mi is refused");
    assert!(err.to_string().contains("missing explicit at_mi"), "{err}");
}

#[test]
fn test_world_rejects_out_of_range_stop_position() {
    let data = json!({
        "cities": {
            "A": {"state": "Test", "region": "midwest", "locations": []},
            "B": {"state": "Test", "region": "midwest", "locations": []},
        },
        "legs": [{
            "from": "A", "to": "B", "miles": 100, "highway": "I-1", "terrain": "flat",
            "stops": [{"name": "Past the city", "type": "travel_center", "at_mi": 130}],
        }],
    });
    let err = World::from_value(data).expect_err("a stop past the city is refused");
    assert!(err.to_string().contains("outside leg mileage"), "{err}");
}

#[test]
fn test_route_describe_mentions_miles_and_highway() {
    let world = world();
    let route = shortest(world, "Chicago", "Indianapolis");
    let text = route.describe("");
    // 185, not 183: the curated figure was short of the road the corridor was
    // baked from, and the archive is a proven lower bound on it.
    assert!(text.contains("185"), "{text}");
    assert!(text.contains("I-65"), "{text}");
}

// -- graph integrity -----------------------------------------------------------

#[test]
fn test_every_city_has_coordinates_and_a_known_region() {
    // Python checked `city.region in REGION_WEIGHTS` (sim/weather); the
    // canonical region list is the same key set, enforced by the regions tests.
    let world = world();
    for city in world.cities.values() {
        assert!(
            REGIONS.contains(&city.region.as_str()),
            "{}: region {}",
            city.name,
            city.region
        );
        assert!(
            // Deadhorse (~70.2°N) is the current northern city.
            24.0 < city.lat && city.lat < 72.0,
            "{}: lat {}",
            city.name,
            city.lat
        );
        assert!(
            -170.0 < city.lon && city.lon < -66.0,
            "{}: lon {}",
            city.name,
            city.lon
        );
        if city.no_freight {
            assert!(
                city.locations.is_empty(),
                "{} must not invent freight locations",
                city.name
            );
            continue;
        }
        let floor = if is_stand_in_market(&city.key) { 1 } else { 2 };
        assert!(
            city.locations.len() >= floor,
            "{}: too few freight locations",
            city.name
        );
    }
}

#[test]
fn test_alcan_phase_a_tip_is_bidirectional() {
    let world = world();
    let south = world
        .shortest_route("surrey_bc_ca", "bellingham_wa_us", None, false)
        .expect("surrey loads")
        .expect("southbound ALCAN tip must route");
    assert_eq!(
        south.cities,
        vec![
            "surrey_bc_ca".to_string(),
            "blaine_wa_us".to_string(),
            "bellingham_wa_us".to_string()
        ]
    );
    let north = world
        .shortest_route("bellingham_wa_us", "surrey_bc_ca", None, false)
        .expect("bellingham loads")
        .expect("northbound ALCAN tip must route");
    assert_eq!(
        north.cities,
        vec![
            "bellingham_wa_us".to_string(),
            "blaine_wa_us".to_string(),
            "surrey_bc_ca".to_string()
        ]
    );
    // Authored reverse edges exist (FIX 1), not only undirected adjacency.
    let directed: Vec<_> = world
        .legs
        .iter()
        .map(|leg| (leg.a.as_str(), leg.b.as_str()))
        .collect();
    assert!(directed.contains(&("surrey_bc_ca", "blaine_wa_us")));
    assert!(directed.contains(&("blaine_wa_us", "bellingham_wa_us")));
    assert!(directed.contains(&("blaine_wa_us", "surrey_bc_ca")));
    assert!(directed.contains(&("bellingham_wa_us", "blaine_wa_us")));
    // Curated truck lots (FIX 3).
    let surrey = world.city("surrey_bc_ca").expect("surrey");
    assert!(surrey
        .locations
        .iter()
        .any(|loc| loc.name.contains("17768") || loc.name.contains("Truck Parking")));
    let blaine = world.city("blaine_wa_us").expect("blaine");
    assert!(blaine
        .locations
        .iter()
        .any(|loc| loc.name.contains("TA Express")));
}

#[test]
fn test_alcan_phase_a_inland_filament_to_dawson_creek() {
    let world = world();
    let north = world
        .shortest_route("surrey_bc_ca", "dawson_creek_bc_ca", None, false)
        .expect("surrey loads")
        .expect("inland ALCAN filament must reach Dawson Creek Mile 0");
    assert_eq!(
        north.cities,
        vec![
            "surrey_bc_ca".to_string(),
            "prince_george_bc_ca".to_string(),
            "dawson_creek_bc_ca".to_string(),
        ]
    );
    let south = world
        .shortest_route("dawson_creek_bc_ca", "surrey_bc_ca", None, false)
        .expect("dawson creek loads")
        .expect("southbound inland filament must route");
    assert_eq!(
        south.cities,
        vec![
            "dawson_creek_bc_ca".to_string(),
            "prince_george_bc_ca".to_string(),
            "surrey_bc_ca".to_string(),
        ]
    );
    // Fort St. John is the Alaska Hwy pass-through north of Mile 0 (not a skip).
    let fsj = world
        .shortest_route("dawson_creek_bc_ca", "fort_st_john_bc_ca", None, false)
        .expect("dawson creek loads")
        .expect("Dawson Creek must reach Fort St. John on the Alaska Highway");
    assert_eq!(
        fsj.cities,
        vec![
            "dawson_creek_bc_ca".to_string(),
            "fort_st_john_bc_ca".to_string(),
        ]
    );
    // Inland BC filament is authored as directed edges both ways (no border
    // on these legs — border_crossing stays Blaine POE only in source JSON).
    let directed: Vec<_> = world
        .legs
        .iter()
        .map(|leg| (leg.a.as_str(), leg.b.as_str()))
        .collect();
    for (a, b) in [
        ("surrey_bc_ca", "prince_george_bc_ca"),
        ("prince_george_bc_ca", "surrey_bc_ca"),
        ("prince_george_bc_ca", "dawson_creek_bc_ca"),
        ("dawson_creek_bc_ca", "prince_george_bc_ca"),
        ("dawson_creek_bc_ca", "fort_st_john_bc_ca"),
        ("fort_st_john_bc_ca", "dawson_creek_bc_ca"),
    ] {
        assert!(
            directed.contains(&(a, b)),
            "missing directed inland leg {a}->{b}"
        );
    }
    // Paid inland miles stay in the honest truck-routed band (not short-hop).
    let pg = world
        .legs
        .iter()
        .find(|leg| leg.a == "surrey_bc_ca" && leg.b == "prince_george_bc_ca")
        .expect("surrey->prince george");
    assert!(
        (400.0..=520.0).contains(&pg.miles),
        "Surrey–Prince George must be real Hwy 1/97 miles, got {}",
        pg.miles
    );
    let dc = world
        .legs
        .iter()
        .find(|leg| leg.a == "prince_george_bc_ca" && leg.b == "dawson_creek_bc_ca")
        .expect("prince george->dawson creek");
    assert!(
        (200.0..=300.0).contains(&dc.miles),
        "Prince George–Dawson Creek must be real Hwy 97 miles, got {}",
        dc.miles
    );
}

#[test]
fn test_alcan_phase_a_north_filament_to_whitehorse() {
    let world = world();
    let north = world
        .shortest_route("fort_st_john_bc_ca", "whitehorse_yt_ca", None, false)
        .expect("fort st john loads")
        .expect("ALCAN north filament must reach Whitehorse");
    assert_eq!(
        north.cities,
        vec![
            "fort_st_john_bc_ca".to_string(),
            "fort_nelson_bc_ca".to_string(),
            "watson_lake_yt_ca".to_string(),
            "whitehorse_yt_ca".to_string(),
        ]
    );
    let south = world
        .shortest_route("whitehorse_yt_ca", "fort_st_john_bc_ca", None, false)
        .expect("whitehorse loads")
        .expect("southbound ALCAN north filament must route");
    assert_eq!(
        south.cities,
        vec![
            "whitehorse_yt_ca".to_string(),
            "watson_lake_yt_ca".to_string(),
            "fort_nelson_bc_ca".to_string(),
            "fort_st_john_bc_ca".to_string(),
        ]
    );
    // Directed edges both ways on the north filament (Tok/Fairbanks covered by terminus test).
    let directed: Vec<_> = world
        .legs
        .iter()
        .map(|leg| (leg.a.as_str(), leg.b.as_str()))
        .collect();
    for (a, b) in [
        ("fort_st_john_bc_ca", "fort_nelson_bc_ca"),
        ("fort_nelson_bc_ca", "fort_st_john_bc_ca"),
        ("fort_nelson_bc_ca", "watson_lake_yt_ca"),
        ("watson_lake_yt_ca", "fort_nelson_bc_ca"),
        ("watson_lake_yt_ca", "whitehorse_yt_ca"),
        ("whitehorse_yt_ca", "watson_lake_yt_ca"),
    ] {
        assert!(
            directed.contains(&(a, b)),
            "missing directed north ALCAN leg {a}->{b}"
        );
    }
    // Tok/Fairbanks + Alcan / Beaver Creek border land in
    // test_alcan_phase_a_terminus_to_fairbanks (Phase A AK terminus slice).
    let fsj_fn = world
        .legs
        .iter()
        .find(|leg| leg.a == "fort_st_john_bc_ca" && leg.b == "fort_nelson_bc_ca")
        .expect("fsj->fort nelson");
    assert!(
        (220.0..=260.0).contains(&fsj_fn.miles),
        "Fort St. John–Fort Nelson must be real Alaska Highway miles, got {}",
        fsj_fn.miles
    );
    let fn_wl = world
        .legs
        .iter()
        .find(|leg| leg.a == "fort_nelson_bc_ca" && leg.b == "watson_lake_yt_ca")
        .expect("fort nelson->watson lake");
    assert!(
        (290.0..=350.0).contains(&fn_wl.miles),
        "Fort Nelson–Watson Lake must be real Alaska Highway miles, got {}",
        fn_wl.miles
    );
    let wl_wh = world
        .legs
        .iter()
        .find(|leg| leg.a == "watson_lake_yt_ca" && leg.b == "whitehorse_yt_ca")
        .expect("watson lake->whitehorse");
    assert!(
        (240.0..=310.0).contains(&wl_wh.miles),
        "Watson Lake–Whitehorse must be real Alaska Highway miles, got {}",
        wl_wh.miles
    );
}

#[test]
fn test_alcan_phase_a_terminus_to_fairbanks() {
    let world = world();
    let north = world
        .shortest_route("whitehorse_yt_ca", "fairbanks_ak_us", None, false)
        .expect("whitehorse loads")
        .expect("ALCAN Phase A terminus must reach Fairbanks");
    assert_eq!(
        north.cities,
        vec![
            "whitehorse_yt_ca".to_string(),
            "tok_ak_us".to_string(),
            "delta_junction_ak_us".to_string(),
            "fairbanks_ak_us".to_string(),
        ]
    );
    let south = world
        .shortest_route("fairbanks_ak_us", "whitehorse_yt_ca", None, false)
        .expect("fairbanks loads")
        .expect("southbound ALCAN terminus filament must route");
    assert_eq!(
        south.cities,
        vec![
            "fairbanks_ak_us".to_string(),
            "delta_junction_ak_us".to_string(),
            "tok_ak_us".to_string(),
            "whitehorse_yt_ca".to_string(),
        ]
    );
    // Continuous Lower 48 → Fairbanks through Blaine (no Anchorage; no teleport).
    let full = world
        .shortest_route("bellingham_wa_us", "fairbanks_ak_us", None, false)
        .expect("bellingham loads")
        .expect("Lower 48 must reach Fairbanks on the Phase A ALCAN corridor");
    assert!(
        full.cities.iter().any(|c| c == "blaine_wa_us"),
        "must enter Canada via Blaine Pacific Highway, got {:?}",
        full.cities
    );
    assert!(
        full.cities.iter().any(|c| c == "dawson_creek_bc_ca"),
        "must pass Dawson Creek Mile 0, got {:?}",
        full.cities
    );
    assert!(
        full.cities.iter().any(|c| c == "whitehorse_yt_ca"),
        "must pass Whitehorse, got {:?}",
        full.cities
    );
    assert!(
        full.cities.iter().any(|c| c == "delta_junction_ak_us"),
        "must pass Delta Junction, got {:?}",
        full.cities
    );
    assert!(
        !full.cities.iter().any(|c| c == "anchorage_ak_us"),
        "Anchorage is Phase B, not Phase A terminus"
    );
    let directed: Vec<_> = world
        .legs
        .iter()
        .map(|leg| (leg.a.as_str(), leg.b.as_str()))
        .collect();
    for (a, b) in [
        ("whitehorse_yt_ca", "tok_ak_us"),
        ("tok_ak_us", "whitehorse_yt_ca"),
        ("tok_ak_us", "delta_junction_ak_us"),
        ("delta_junction_ak_us", "tok_ak_us"),
        ("delta_junction_ak_us", "fairbanks_ak_us"),
        ("fairbanks_ak_us", "delta_junction_ak_us"),
    ] {
        assert!(
            directed.contains(&(a, b)),
            "missing directed terminus ALCAN leg {a}->{b}"
        );
    }
    assert!(!directed.contains(&("tok_ak_us", "fairbanks_ak_us")));
    assert!(!directed.contains(&("fairbanks_ak_us", "tok_ak_us")));
    let tok_delta = world
        .legs
        .iter()
        .find(|leg| leg.a == "tok_ak_us" && leg.b == "delta_junction_ak_us")
        .expect("tok->delta junction");
    let delta_fairbanks = world
        .legs
        .iter()
        .find(|leg| leg.a == "delta_junction_ak_us" && leg.b == "fairbanks_ak_us")
        .expect("delta junction->fairbanks");
    assert!((tok_delta.miles - 108.0).abs() < 0.5);
    assert!((delta_fairbanks.miles - 96.0).abs() < 0.5);
    assert!(
        (tok_delta.miles + delta_fairbanks.miles - 202.0).abs() > 1.0,
        "Tok split miles keep the authored 204-mile milepost total"
    );
    // Alcan / Beaver Creek border_crossing on both CA↔US directions.
    let yt = include_str!("../../../../data/world_data/ca/legs/YT.json");
    let ak = include_str!("../../../../data/world_data/us/legs/AK.json");
    assert!(
        yt.contains("\"from\": \"whitehorse_yt_ca\"")
            && yt.contains("\"to\": \"tok_ak_us\"")
            && yt.contains("alcan_beaver_creek")
            && yt.contains("through_freight")
            && yt.contains("\"cabotage\": \"forbidden\""),
        "CA→US Whitehorse→Tok must carry alcan_beaver_creek through_freight border stub"
    );
    assert!(
        ak.contains("\"from\": \"tok_ak_us\"")
            && ak.contains("\"to\": \"whitehorse_yt_ca\"")
            && ak.contains("alcan_beaver_creek")
            && ak.contains("through_freight")
            && ak.contains("\"cabotage\": \"forbidden\""),
        "US→CA Tok→Whitehorse must carry alcan_beaver_creek through_freight border stub"
    );
    let wh_tok = world
        .legs
        .iter()
        .find(|leg| leg.a == "whitehorse_yt_ca" && leg.b == "tok_ak_us")
        .expect("whitehorse->tok");
    assert!(
        (360.0..=420.0).contains(&wh_tok.miles),
        "Whitehorse–Tok must be real Alaska Highway miles, got {}",
        wh_tok.miles
    );
    let tok_delta = world
        .legs
        .iter()
        .find(|leg| leg.a == "tok_ak_us" && leg.b == "delta_junction_ak_us")
        .expect("tok->delta junction");
    assert!(
        (107.5..=108.5).contains(&tok_delta.miles),
        "Tok–Delta Junction must use Alaska Highway mileposts, got {}",
        tok_delta.miles
    );
    let delta_fairbanks = world
        .legs
        .iter()
        .find(|leg| leg.a == "delta_junction_ak_us" && leg.b == "fairbanks_ak_us")
        .expect("delta junction->fairbanks");
    assert!(
        (95.5..=96.5).contains(&delta_fairbanks.miles),
        "Delta Junction–Fairbanks must use Richardson Highway mileposts, got {}",
        delta_fairbanks.miles
    );
    // Stand-in fuel lots.
    let tok = world.city("tok_ak_us").expect("tok");
    assert!(tok
        .locations
        .iter()
        .any(|loc| loc.name.contains("Chevron") || loc.name.contains("Young")));
    let fb = world.city("fairbanks_ak_us").expect("fairbanks");
    assert!(fb
        .locations
        .iter()
        .any(|loc| loc.name.contains("Sourdough") || loc.name.contains("Airport")));
}

#[test]
fn test_alcan_border_identity_and_posted_speed_profiles() {
    let shards = [
        serde_json::from_str::<serde_json::Value>(include_str!(
            "../../../../data/world_data/us/legs/AK.json"
        ))
        .expect("AK legs JSON"),
        serde_json::from_str::<serde_json::Value>(include_str!(
            "../../../../data/world_data/ca/legs/YT.json"
        ))
        .expect("YT legs JSON"),
        serde_json::from_str::<serde_json::Value>(include_str!(
            "../../../../data/world_data/ca/legs/BC.json"
        ))
        .expect("BC legs JSON"),
    ];
    let whitehorse_to_tok = raw_world_leg(&shards, "whitehorse_yt_ca", "tok_ak_us");
    let tok_to_whitehorse = raw_world_leg(&shards, "tok_ak_us", "whitehorse_yt_ca");

    for leg in [whitehorse_to_tok, tok_to_whitehorse] {
        for metadata in [&leg["border_crossing"], &leg["corridor"]["border_crossing"]] {
            assert_eq!(metadata["id"].as_str(), Some("alcan_beaver_creek"));
            assert!(metadata["ports"]
                .as_array()
                .expect("border ports")
                .iter()
                .any(|port| port.as_str() == Some("alcan_beaver_creek")));
        }
    }

    let alcan = whitehorse_to_tok["corridor"]["checkpoints"]
        .as_array()
        .expect("Whitehorse→Tok checkpoints")
        .iter()
        .find(|checkpoint| checkpoint["name"].as_str() == Some("Alcan Port of Entry"))
        .expect("Alcan Port of Entry checkpoint");
    assert_eq!(alcan["at_mi"].as_f64(), Some(297.5));
    assert_eq!(alcan["state"].as_str(), Some("Alaska"));
    assert!(alcan["source"]
        .as_str()
        .expect("Alcan checkpoint source")
        .contains("4212502894"));
    assert!(alcan["source"]
        .as_str()
        .expect("Alcan checkpoint source")
        .contains("https://www.cbp.gov/about/contact/ports/alcan-alaska-3104"));

    let beaver = tok_to_whitehorse["corridor"]["checkpoints"]
        .as_array()
        .expect("Tok→Whitehorse checkpoints")
        .iter()
        .find(|checkpoint| checkpoint["name"].as_str() == Some("Beaver Creek Port of Entry"))
        .expect("Beaver Creek Port of Entry checkpoint");
    let beaver_mile = beaver["at_mi"].as_f64().expect("Beaver Creek at_mi");
    assert!(89.5 < beaver_mile && beaver_mile < 120.0);
    assert_eq!(beaver["state"].as_str(), Some("Yukon"));
    assert_eq!(beaver["lat"].as_f64(), Some(62.4089932));
    assert_eq!(beaver["lon"].as_f64(), Some(-140.859717));
    assert!(beaver["source"]
        .as_str()
        .expect("Beaver Creek checkpoint source")
        .contains("3531514528"));

    for (leg, at_mi) in [(tok_to_whitehorse, 89.5), (whitehorse_to_tok, 297.5)] {
        let crossings = leg["corridor"]["state_crossings"]
            .as_array()
            .expect("Alaska–Yukon state crossings");
        assert_eq!(crossings.len(), 1);
        assert_eq!(crossings[0]["at_mi"].as_f64(), Some(at_mi));
        assert_eq!(
            crossings[0]["place"].as_str(),
            Some("Alaska–Yukon border on the Alaska Highway at Alcan, Alaska")
        );
    }

    for leg in [tok_to_whitehorse, whitehorse_to_tok] {
        for section in ["checkpoints", "state_crossings"] {
            for entry in leg["corridor"][section]
                .as_array()
                .expect("border text array")
            {
                for field in ["name", "place", "source"] {
                    if let Some(text) = entry[field].as_str() {
                        assert!(!text.contains("Poker Creek"), "{section}.{field}: {text}");
                    }
                }
            }
        }
    }

    for (from, to) in [
        ("tok_ak_us", "whitehorse_yt_ca"),
        ("whitehorse_yt_ca", "tok_ak_us"),
        ("watson_lake_yt_ca", "whitehorse_yt_ca"),
        ("whitehorse_yt_ca", "watson_lake_yt_ca"),
        ("watson_lake_yt_ca", "fort_nelson_bc_ca"),
        ("fort_nelson_bc_ca", "watson_lake_yt_ca"),
    ] {
        let samples = raw_world_leg(&shards, from, to)["corridor"]["speed_limits"]
            .as_array()
            .expect("speed-limit samples");
        assert!(!samples.is_empty(), "{from}->{to} has no speed samples");
        for sample in samples {
            let mph = sample["mph"].as_f64().expect("speed sample mph");
            assert!(mph <= 65.0, "{from}->{to} has a {mph} mph sample");
        }
    }

    for (leg, label, sample_miles) in [
        (tok_to_whitehorse, "Tok→Whitehorse", [90.0, 95.0, 100.0]),
        (whitehorse_to_tok, "Whitehorse→Tok", [285.0, 290.0, 295.0]),
    ] {
        let samples = leg["corridor"]["speed_limits"]
            .as_array()
            .expect("speed-limit samples");
        for mile in sample_miles {
            let active = samples
                .iter()
                .rev()
                .find(|sample| {
                    sample["at_mi"]
                        .as_f64()
                        .is_some_and(|sample_mile| sample_mile <= mile)
                })
                .unwrap_or_else(|| panic!("{label} has no speed sample at mile {mile}"));
            assert_eq!(
                active["mph"].as_f64(),
                Some(55.0),
                "{label} must be 55 mph at mile {mile}"
            );
        }
    }
}

#[test]
fn test_alaska_b3_city_labels_and_metadata() {
    let world = world();
    for (key, name) in [
        ("delta_junction_ak_us", "Delta Junction"),
        ("valdez_ak_us", "Valdez"),
        ("seward_ak_us", "Seward"),
        ("soldotna_ak_us", "Soldotna"),
        ("kenai_ak_us", "Kenai"),
        ("homer_ak_us", "Homer"),
        ("coldfoot_ak_us", "Coldfoot"),
        ("deadhorse_ak_us", "Deadhorse"),
    ] {
        let city = world.city(key).unwrap_or_else(|_| panic!("{key}"));
        assert_eq!(city.name, name);
        assert_eq!(city.country, "US", "{key} country");
        assert_eq!(city.state_code, "AK", "{key} state code");
        assert_eq!(city.region, "pacific_northwest", "{key} region");
    }
}

#[test]
fn test_alaska_kenai_peninsula_routes_use_paid_miles_both_ways() {
    let world = world();
    for (start, end, cities, miles) in [
        (
            "anchorage_ak_us",
            "kenai_ak_us",
            &["anchorage_ak_us", "soldotna_ak_us", "kenai_ak_us"][..],
            158.0,
        ),
        (
            "kenai_ak_us",
            "anchorage_ak_us",
            &["kenai_ak_us", "soldotna_ak_us", "anchorage_ak_us"][..],
            158.0,
        ),
        (
            "anchorage_ak_us",
            "homer_ak_us",
            &["anchorage_ak_us", "soldotna_ak_us", "homer_ak_us"][..],
            222.0,
        ),
        (
            "homer_ak_us",
            "anchorage_ak_us",
            &["homer_ak_us", "soldotna_ak_us", "anchorage_ak_us"][..],
            222.0,
        ),
        (
            "soldotna_ak_us",
            "homer_ak_us",
            &["soldotna_ak_us", "homer_ak_us"][..],
            75.0,
        ),
        (
            "homer_ak_us",
            "soldotna_ak_us",
            &["homer_ak_us", "soldotna_ak_us"][..],
            75.0,
        ),
        (
            "seward_ak_us",
            "soldotna_ak_us",
            &["seward_ak_us", "soldotna_ak_us"][..],
            94.0,
        ),
        (
            "soldotna_ak_us",
            "seward_ak_us",
            &["soldotna_ak_us", "seward_ak_us"][..],
            94.0,
        ),
    ] {
        assert_route_sequence_and_paid_miles(world, start, end, cities, miles);
    }
}

#[test]
fn test_alaska_valdez_routes_use_paid_miles_both_ways() {
    let world = world();
    for (start, end, cities, miles) in [
        (
            "fairbanks_ak_us",
            "valdez_ak_us",
            &[
                "fairbanks_ak_us",
                "delta_junction_ak_us",
                "glennallen_ak_us",
                "valdez_ak_us",
            ][..],
            362.0,
        ),
        (
            "valdez_ak_us",
            "fairbanks_ak_us",
            &[
                "valdez_ak_us",
                "glennallen_ak_us",
                "delta_junction_ak_us",
                "fairbanks_ak_us",
            ][..],
            362.0,
        ),
        (
            "anchorage_ak_us",
            "valdez_ak_us",
            &[
                "anchorage_ak_us",
                "palmer_ak_us",
                "glennallen_ak_us",
                "valdez_ak_us",
            ][..],
            302.0,
        ),
        (
            "valdez_ak_us",
            "anchorage_ak_us",
            &[
                "valdez_ak_us",
                "glennallen_ak_us",
                "palmer_ak_us",
                "anchorage_ak_us",
            ][..],
            302.0,
        ),
    ] {
        assert_route_sequence_and_paid_miles(world, start, end, cities, miles);
    }
}

#[test]
fn test_alaska_dalton_routes_reach_deadhorse_both_ways() {
    let world = world();
    assert_route_sequence_and_paid_miles(
        world,
        "fairbanks_ak_us",
        "deadhorse_ak_us",
        &["fairbanks_ak_us", "coldfoot_ak_us", "deadhorse_ak_us"],
        498.0,
    );
    assert_route_sequence_and_paid_miles(
        world,
        "deadhorse_ak_us",
        "fairbanks_ak_us",
        &["deadhorse_ak_us", "coldfoot_ak_us", "fairbanks_ak_us"],
        498.0,
    );
}

#[test]
fn test_alcan_phase_b1_tok_cutoff_glenn_to_anchorage() {
    let world = world();
    let south = world
        .shortest_route("tok_ak_us", "anchorage_ak_us", None, false)
        .expect("tok loads")
        .expect("ALCAN Phase B1 must reach Anchorage via Glennallen and Palmer");
    assert_eq!(
        south.cities,
        vec![
            "tok_ak_us".to_string(),
            "glennallen_ak_us".to_string(),
            "palmer_ak_us".to_string(),
            "anchorage_ak_us".to_string(),
        ]
    );
    let north = world
        .shortest_route("anchorage_ak_us", "tok_ak_us", None, false)
        .expect("anchorage loads")
        .expect("northbound Phase B1 filament must route");
    assert_eq!(
        north.cities,
        vec![
            "anchorage_ak_us".to_string(),
            "palmer_ak_us".to_string(),
            "glennallen_ak_us".to_string(),
            "tok_ak_us".to_string(),
        ]
    );
    // Continuous Lower 48 → Anchorage through Blaine + Phase A corridor (no teleport).
    let full = world
        .shortest_route("bellingham_wa_us", "anchorage_ak_us", None, false)
        .expect("bellingham loads")
        .expect("Lower 48 must reach Anchorage on ALCAN + Phase B1");
    assert!(
        full.cities.iter().any(|c| c == "blaine_wa_us"),
        "must enter Canada via Blaine, got {:?}",
        full.cities
    );
    assert!(
        full.cities.iter().any(|c| c == "tok_ak_us"),
        "must pass Tok, got {:?}",
        full.cities
    );
    assert!(
        full.cities.iter().any(|c| c == "glennallen_ak_us"),
        "must pass Glennallen on Tok Cutoff/Glenn, got {:?}",
        full.cities
    );
    assert!(
        !full.cities.iter().any(|c| c == "fairbanks_ak_us"),
        "B1 Tok→Anchorage must not detour Fairbanks/Parks, got {:?}",
        full.cities
    );
    let directed: Vec<_> = world
        .legs
        .iter()
        .map(|leg| (leg.a.as_str(), leg.b.as_str()))
        .collect();
    for (a, b) in [
        ("tok_ak_us", "glennallen_ak_us"),
        ("glennallen_ak_us", "tok_ak_us"),
        ("glennallen_ak_us", "palmer_ak_us"),
        ("palmer_ak_us", "glennallen_ak_us"),
        ("palmer_ak_us", "anchorage_ak_us"),
        ("anchorage_ak_us", "palmer_ak_us"),
    ] {
        assert!(
            directed.contains(&(a, b)),
            "missing directed Phase B1 leg {a}->{b}"
        );
    }
    let tok_gl = world
        .legs
        .iter()
        .find(|leg| leg.a == "tok_ak_us" && leg.b == "glennallen_ak_us")
        .expect("tok->glennallen");
    assert!(
        (130.0..=150.0).contains(&tok_gl.miles),
        "Tok–Glennallen must be ~139 mi, got {}",
        tok_gl.miles
    );
    let gl_pa = world
        .legs
        .iter()
        .find(|leg| leg.a == "glennallen_ak_us" && leg.b == "palmer_ak_us")
        .expect("glennallen->palmer");
    assert!(
        (gl_pa.miles - 145.0).abs() < 0.5,
        "Glennallen–Palmer paid miles must be milepost 145, got {}",
        gl_pa.miles
    );
    let pa_anc = world
        .legs
        .iter()
        .find(|leg| leg.a == "palmer_ak_us" && leg.b == "anchorage_ak_us")
        .expect("palmer->anchorage");
    assert!(
        (pa_anc.miles - 42.0).abs() < 0.5,
        "Palmer–Anchorage paid miles must be milepost 42, got {}",
        pa_anc.miles
    );
    let glenn = world.city("glennallen_ak_us").expect("glennallen");
    assert!(
        glenn
            .locations
            .iter()
            .any(|loc| loc.name.contains("Hub of Alaska")),
        "Glennallen primary pin must be Hub of Alaska"
    );
    assert!(
        glenn
            .locations
            .iter()
            .any(|loc| loc.name.contains("Glennallen Fuel")),
        "Glennallen alternate pin must be Glennallen Fuel"
    );
    let palmer = world.city("palmer_ak_us").expect("palmer");
    assert!(
        !palmer
            .locations
            .iter()
            .any(|loc| loc.name.to_lowercase().contains("chevron")),
        "Palmer must not pin unverified Chevron truck stop"
    );
    let anc = world.city("anchorage_ak_us").expect("anchorage");
    assert!(
        anc.locations
            .iter()
            .any(|loc| loc.name.contains("Essential One")),
        "Anchorage fuel pin must be Essential One"
    );
    assert!(
        anc.locations
            .iter()
            .any(|loc| { loc.name.contains("Port of Alaska") || loc.name.contains("Ship Creek") }),
        "Anchorage delivery end must be Port/Ship Creek industrial, not a truck stop"
    );
    assert!(
        !anc.locations.iter().any(|loc| loc.name.contains("gakona")),
        "gakona must not appear"
    );
}

#[test]
fn test_alcan_phase_b2_parks_to_anchorage() {
    let world = world();
    let south = world
        .shortest_route("fairbanks_ak_us", "anchorage_ak_us", None, false)
        .expect("fairbanks loads")
        .expect("ALCAN Phase B2 Parks must reach Anchorage");
    assert_eq!(
        south.cities,
        vec![
            "fairbanks_ak_us".to_string(),
            "nenana_ak_us".to_string(),
            "healy_ak_us".to_string(),
            "wasilla_ak_us".to_string(),
            "anchorage_ak_us".to_string(),
        ]
    );
    assert!(
        !south.cities.iter().any(|c| c == "cantwell_ak_us"),
        "Cantwell must stay collapsed, got {:?}",
        south.cities
    );
    let north = world
        .shortest_route("anchorage_ak_us", "fairbanks_ak_us", None, false)
        .expect("anchorage loads")
        .expect("northbound Parks filament must route");
    assert_eq!(
        north.cities,
        vec![
            "anchorage_ak_us".to_string(),
            "wasilla_ak_us".to_string(),
            "healy_ak_us".to_string(),
            "nenana_ak_us".to_string(),
            "fairbanks_ak_us".to_string(),
        ]
    );
    // Mat-Su connector both ways; Palmer remains Mat-Su market, Wasilla pass-through.
    let directed: Vec<_> = world
        .legs
        .iter()
        .map(|leg| (leg.a.as_str(), leg.b.as_str()))
        .collect();
    for (a, b) in [
        ("fairbanks_ak_us", "nenana_ak_us"),
        ("nenana_ak_us", "fairbanks_ak_us"),
        ("nenana_ak_us", "healy_ak_us"),
        ("healy_ak_us", "nenana_ak_us"),
        ("healy_ak_us", "wasilla_ak_us"),
        ("wasilla_ak_us", "healy_ak_us"),
        ("wasilla_ak_us", "anchorage_ak_us"),
        ("anchorage_ak_us", "wasilla_ak_us"),
        ("palmer_ak_us", "wasilla_ak_us"),
        ("wasilla_ak_us", "palmer_ak_us"),
    ] {
        assert!(
            directed.contains(&(a, b)),
            "missing directed Phase B2 leg {a}->{b}"
        );
    }
    let fbx_nen = world
        .legs
        .iter()
        .find(|leg| leg.a == "fairbanks_ak_us" && leg.b == "nenana_ak_us")
        .expect("fairbanks->nenana");
    assert!(
        (fbx_nen.miles - 53.0).abs() < 0.5,
        "Fairbanks–Nenana must be Parks milepost ~53 (MP358→MP305), got {}",
        fbx_nen.miles
    );
    let nen_hea = world
        .legs
        .iter()
        .find(|leg| leg.a == "nenana_ak_us" && leg.b == "healy_ak_us")
        .expect("nenana->healy");
    assert!(
        (nen_hea.miles - 56.0).abs() < 0.5,
        "Nenana–Healy must be Parks milepost ~56 (MP305→MP249), got {}",
        nen_hea.miles
    );
    let hea_was = world
        .legs
        .iter()
        .find(|leg| leg.a == "healy_ak_us" && leg.b == "wasilla_ak_us")
        .expect("healy->wasilla");
    assert!(
        (hea_was.miles - 207.0).abs() < 0.5,
        "Healy–Wasilla (Cantwell collapsed) must be ~207, got {}",
        hea_was.miles
    );
    let was_anc = world
        .legs
        .iter()
        .find(|leg| leg.a == "wasilla_ak_us" && leg.b == "anchorage_ak_us")
        .expect("wasilla->anchorage");
    assert!(
        (was_anc.miles - 42.0).abs() < 0.5,
        "Wasilla–Anchorage must be Parks milepost ~42 (MP42→MP0), got {}",
        was_anc.miles
    );
    let pal_was = world
        .legs
        .iter()
        .find(|leg| leg.a == "palmer_ak_us" && leg.b == "wasilla_ak_us")
        .expect("palmer->wasilla");
    assert!(
        (10.0..=15.0).contains(&pal_was.miles),
        "Palmer–Wasilla connector must be ~11–13, got {}",
        pal_was.miles
    );
    let healy = world.city("healy_ak_us").expect("healy");
    assert!(
        healy
            .locations
            .iter()
            .any(|loc| loc.name.contains("Fisher Fuel")),
        "Healy thin pin must be Fisher Fuel"
    );
    let nenana = world.city("nenana_ak_us").expect("nenana");
    assert!(
        !nenana
            .locations
            .iter()
            .any(|loc| loc.name.to_lowercase().contains("a-frame")),
        "Nenana must not pin unverified A-Frame"
    );
    let wasilla = world.city("wasilla_ak_us").expect("wasilla");
    assert!(
        !wasilla.locations.iter().any(|loc| {
            let n = loc.name.to_lowercase();
            n.contains("holiday") || n.contains("three bears") || n.contains("fishers")
        }),
        "Wasilla must not pin unverified travel-center lots"
    );
    assert!(
        world.city("cantwell_ak_us").is_err(),
        "cantwell_ak_us must not exist while collapsed"
    );
}

#[test]
fn test_alcan_public_lots_use_travel_center_or_truck_parking() {
    let world = world();
    let cases = [
        ("blaine_wa_us", "TA Express", "travel_center"),
        ("surrey_bc_ca", "Truck Parking", "truck_parking"),
        ("tok_ak_us", "Chevron", "travel_center"),
        ("fairbanks_ak_us", "Sourdough", "travel_center"),
        ("glennallen_ak_us", "Hub of Alaska", "travel_center"),
        ("anchorage_ak_us", "Essential One", "travel_center"),
        ("healy_ak_us", "Fisher Fuel", "travel_center"),
        (
            "delta_junction_ak_us",
            "Three Bears Delta Junction",
            "travel_center",
        ),
        ("coldfoot_ak_us", "Coldfoot Camp Fuel", "travel_center"),
        ("seward_ak_us", "Essential One Seward", "travel_center"),
        ("soldotna_ak_us", "Essential One Soldotna", "travel_center"),
        ("homer_ak_us", "Essential One Homer", "travel_center"),
    ];
    for (city, needle, want) in cases {
        let c = world.city(city).unwrap_or_else(|_| panic!("{city}"));
        let loc = c
            .locations
            .iter()
            .find(|l| l.name.contains(needle))
            .unwrap_or_else(|| panic!("{city} missing {needle}"));
        assert_eq!(loc.facility_type, want, "{city} {needle} type");
    }
    // Stand-in placeholders stay company_yard; Port stays terminal.
    let palmer = world.city("palmer_ak_us").expect("palmer");
    assert!(palmer
        .locations
        .iter()
        .all(|l| l.facility_type == "company_yard"));
    let anc = world.city("anchorage_ak_us").expect("anchorage");
    assert!(anc
        .locations
        .iter()
        .any(|l| { l.name.contains("Port of Alaska") && l.facility_type == "terminal" }));
    // Stand-in ALCAN cities with only public-lot types must not grow a skyline.
    let tok = world.city("tok_ak_us").expect("tok");
    assert_eq!(
        tok.locations.len(),
        1,
        "Tok stand-in must keep a single curated lot, got {}",
        tok.locations.len()
    );
    let surrey = world.city("surrey_bc_ca").expect("surrey");
    assert_eq!(surrey.locations.len(), 1);
}

#[test]
fn test_alcan_retyped_lots_are_fuel_rest_only_not_freight() {
    use ff_core::data::world_constants::{
        facility_cargo_roles, lookup, DEFAULT_POI_ACTIONS, FACILITY_APPROACH_MILES,
        FACILITY_APPROACH_ROADS,
    };
    let world = world();
    let cases = [
        ("blaine_wa_us", "TA Express", "travel_center"),
        ("surrey_bc_ca", "Truck Parking", "truck_parking"),
        ("prince_george_bc_ca", "Husky / Esso", "travel_center"),
        ("dawson_creek_bc_ca", "Husky", "travel_center"),
        ("fort_st_john_bc_ca", "Smith Fuel", "travel_center"),
        ("fort_nelson_bc_ca", "Esso", "travel_center"),
        ("watson_lake_yt_ca", "Petro-Canada", "travel_center"),
        ("whitehorse_yt_ca", "McCrae", "travel_center"),
        ("tok_ak_us", "Chevron", "travel_center"),
        ("fairbanks_ak_us", "Sourdough", "travel_center"),
        ("glennallen_ak_us", "Hub of Alaska", "travel_center"),
        ("glennallen_ak_us", "Glennallen Fuel", "travel_center"),
        ("anchorage_ak_us", "Essential One", "travel_center"),
        ("healy_ak_us", "Fisher Fuel", "travel_center"),
        (
            "delta_junction_ak_us",
            "Three Bears Delta Junction",
            "travel_center",
        ),
        ("coldfoot_ak_us", "Coldfoot Camp Fuel", "travel_center"),
        ("seward_ak_us", "Essential One Seward", "travel_center"),
        ("soldotna_ak_us", "Essential One Soldotna", "travel_center"),
        ("homer_ak_us", "Essential One Homer", "travel_center"),
    ];
    assert_eq!(cases.len(), 19, "all Alaska corridor public lots");
    for (city, needle, want) in cases {
        let c = world.city(city).unwrap_or_else(|_| panic!("{city}"));
        let loc = c
            .locations
            .iter()
            .find(|l| l.name.contains(needle))
            .unwrap_or_else(|| panic!("{city} missing {needle}"));
        assert_eq!(loc.facility_type, want, "{city} {needle}");
        assert!(
            loc.ships.is_empty() && loc.receives.is_empty(),
            "{city} {needle} must ship/receive no freight"
        );
        let (ships, receives) = facility_cargo_roles(want).expect("roles table");
        assert!(
            ships.is_empty() && receives.is_empty(),
            "{want} cargo roles"
        );
        let actions = lookup(DEFAULT_POI_ACTIONS, want).expect("poi actions");
        assert!(actions.contains(&"park"), "{want} must support parking");
        assert!(
            actions.iter().any(|a| *a == "break" || *a == "sleep"),
            "{want} must support rest"
        );
        if want == "travel_center" {
            assert!(actions.contains(&"fuel"), "travel_center must support fuel");
        }
        assert!(
            lookup(FACILITY_APPROACH_MILES, want).is_some(),
            "{want} approach miles"
        );
        assert!(
            lookup(FACILITY_APPROACH_ROADS, want).is_some(),
            "{want} approach road"
        );
    }
}

#[test]
fn test_alaska_freight_endpoints_have_source_backed_roles() {
    let world = world();
    let cases = [
        (
            "valdez_ak_us",
            "Ryan J. Sontag Valdez Container Terminal",
            "terminal",
            &["container", "general", "construction"][..],
            &["container", "general", "construction"][..],
            "https://www.valdezak.gov/296/Ryan-J-Sontag-Valdez-Container-Terminal",
        ),
        (
            "kenai_ak_us",
            "Carlile Kenai Shipping Terminal",
            "cross_dock",
            &["general"][..],
            &["general"][..],
            "https://www.carlile.biz/contact-and-terminals/kenai/",
        ),
        (
            "seward_ak_us",
            "Alaska Railroad Seward Freight Dock",
            "terminal",
            &["general"][..],
            &["general"][..],
            "https://www.alaskarailroad.com/sites/default/files/FCTSHT_2024_Seward_Freight_Dock_Expansion_FactSheet.pdf",
        ),
        (
            "homer_ak_us",
            "Port of Homer Deep Water Freight Dock",
            "terminal",
            &["general"][..],
            &["general"][..],
            "https://www.cityofhomer-ak.gov/port/deep-water-dock-scheduling",
        ),
    ];
    for (city_key, name, facility_type, ships, receives, source_url) in cases {
        let city = world
            .city(city_key)
            .unwrap_or_else(|_| panic!("{city_key}"));
        let location = city
            .locations
            .iter()
            .find(|location| location.name == *name)
            .unwrap_or_else(|| panic!("{city_key} missing {name}"));
        assert_eq!(location.facility_type, *facility_type, "{name} type");
        assert_eq!(
            location.ships,
            ships
                .iter()
                .map(|cargo| cargo.to_string())
                .collect::<Vec<_>>(),
            "{name} ships"
        );
        assert_eq!(
            location.receives,
            receives
                .iter()
                .map(|cargo| cargo.to_string())
                .collect::<Vec<_>>(),
            "{name} receives"
        );
        assert!(location.roles.iter().any(|role| role == "shipper"));
        assert!(location.roles.iter().any(|role| role == "receiver"));
        assert!(location.source_note.contains(source_url), "{name} source");
    }
}

#[test]
fn test_alaska_carrier_lanes_reach_new_freight_markets() {
    use ff_core::models::carriers::carrier;
    use ff_core::models::jobs::{JobBoard, OfferOptions};

    let world = world();
    let cases = [
        ("anchorage_ak_us", "valdez_ak_us", "chatanika_freight"),
        ("anchorage_ak_us", "homer_ak_us", "chatanika_freight"),
        ("anchorage_ak_us", "kenai_ak_us", "chatanika_freight"),
        ("fairbanks_ak_us", "seward_ak_us", "chatanika_freight"),
        ("fairbanks_ak_us", "deadhorse_ak_us", "chatanika_freight"),
        ("anchorage_ak_us", "seward_ak_us", "knik_arm_cartage"),
    ];
    for (origin, destination, carrier_key) in cases {
        let mut board = JobBoard::seeded(world, 19);
        let job = board
            .offer_to(
                origin,
                destination,
                &[] as &[&str],
                OfferOptions {
                    level: 25,
                    carrier_key: Some(carrier_key),
                    ..Default::default()
                },
            )
            .unwrap_or_else(|| panic!("{carrier_key} cannot offer {origin}→{destination}"));
        assert_eq!(
            world.resolve_city_key(&job.destination),
            destination,
            "{carrier_key} destination"
        );
        assert!(
            carrier(carrier_key)
                .expect("carrier exists")
                .run_band_allows(job.distance_mi),
            "{carrier_key} run to {destination}: {} mi",
            job.distance_mi
        );
    }
}

#[test]
fn test_alaska_pass_through_fuel_towns_have_no_board_offers() {
    use ff_core::models::jobs::{JobBoard, OfferOptions};

    let world = world();
    for city_key in ["delta_junction_ak_us", "coldfoot_ak_us", "soldotna_ak_us"] {
        assert!(
            is_stand_in_market(city_key),
            "{city_key} must stay a stand-in"
        );
        let mut board = JobBoard::seeded(world, 19);
        let offers = board.offers(city_key, &[] as &[&str], OfferOptions::level(5));
        assert!(offers.is_empty(), "{city_key} must not offer freight");
    }
}

#[test]
fn test_alaska_stand_in_markets_preserve_authored_facilities() {
    let world = world();
    for city_key in ["delta_junction_ak_us", "coldfoot_ak_us", "soldotna_ak_us"] {
        let city = world
            .cities
            .get(city_key)
            .expect("Alaska pass-through city");
        assert!(
            is_stand_in_market(city_key),
            "{city_key} must be a stand-in"
        );
        assert!(
            has_only_public_lots(city),
            "{city_key} must not gain freight templates"
        );
    }

    let deadhorse = world.cities.get("deadhorse_ak_us").expect("Deadhorse city");
    assert!(is_stand_in_market("deadhorse_ak_us"));
    assert_eq!(deadhorse.locations.len(), 1);
    assert_eq!(deadhorse.locations[0].facility_type, "company_yard");
    assert!(deadhorse.locations[0].template);

    let homer = world.cities.get("homer_ak_us").expect("Homer city");
    assert!(is_stand_in_market("homer_ak_us"));
    assert!(homer.locations.iter().all(|location| !location.template));
    assert!(homer.locations.iter().any(|location| {
        location.facility_type == "terminal"
            && location.name == "Port of Homer Deep Water Freight Dock"
            && !location.ships.is_empty()
            && !location.receives.is_empty()
    }));
    assert!(homer.locations.iter().any(|location| {
        location.facility_type == "travel_center"
            && location.name == "Essential One Homer"
            && location.ships.is_empty()
            && location.receives.is_empty()
    }));

    for city_key in ["seward_ak_us", "valdez_ak_us", "kenai_ak_us"] {
        let city = world.cities.get(city_key).expect("Alaska freight city");
        assert!(
            !is_stand_in_market(city_key),
            "{city_key} has a real endpoint"
        );
        assert!(
            city.locations.iter().any(|location| location.template),
            "{city_key} must expand its representative market"
        );
        assert!(
            city.locations.iter().any(|location| {
                !location.template && (!location.ships.is_empty() || !location.receives.is_empty())
            }),
            "{city_key} must retain its authored freight endpoint"
        );
    }
}

#[test]
fn test_fairbanks_has_curated_freight_job_endpoints() {
    use ff_core::models::jobs::JobBoard;
    let world = world();
    let fb = world.city("fairbanks_ak_us").expect("fairbanks");
    let by_name = |needle: &str| {
        fb.locations
            .iter()
            .find(|l| l.name.contains(needle))
            .unwrap_or_else(|| panic!("Fairbanks missing {needle}"))
    };

    let costco = by_name("Costco");
    assert_eq!(costco.facility_type, "retail_distribution");
    assert!(costco.ships.is_empty(), "Costco must be receive-only");
    assert!(
        JobBoard::cargo_for_location(costco, "ships", None).is_empty(),
        "board must not re-inflate Costco ships from type defaults"
    );
    assert!(!JobBoard::cargo_for_location(costco, "receives", None).is_empty());
    assert!(costco.roles.iter().all(|r| r == "receiver"));
    for key in ["food", "retail", "refrigerated", "general"] {
        assert!(
            costco.receives.iter().any(|k| k == key),
            "Costco receives {key}"
        );
    }
    assert!(!costco.receives.iter().any(|k| k == "frozen"));

    let fred = by_name("Fred Meyer");
    assert_eq!(fred.facility_type, "retail_distribution");
    assert!(fred.ships.is_empty(), "Fred Meyer must be receive-only");
    assert!(fred.roles.iter().all(|r| r == "receiver"));
    for key in ["food", "retail", "refrigerated", "general"] {
        assert!(
            fred.receives.iter().any(|k| k == key),
            "Fred Meyer receives {key}"
        );
    }

    let sbs = by_name("Spenard");
    assert_eq!(sbs.facility_type, "construction_materials_yard");
    assert!(sbs.ships.is_empty(), "SBS must be receive-only");
    for key in ["construction", "lumber_paper", "steel"] {
        assert!(sbs.receives.iter().any(|k| k == key), "SBS receives {key}");
    }
    assert!(
        !sbs.receives.iter().any(|k| k == "bulk"),
        "SBS must not receive bulk"
    );

    let carlile = by_name("Carlile");
    assert_eq!(carlile.facility_type, "cross_dock");
    assert!(carlile.roles.iter().any(|r| r == "shipper"));
    assert!(carlile.roles.iter().any(|r| r == "receiver"));
    for key in ["general", "retail", "parcel", "container"] {
        assert!(
            carlile.ships.iter().any(|k| k == key),
            "Carlile ships {key}"
        );
        assert!(
            carlile.receives.iter().any(|k| k == key),
            "Carlile receives {key}"
        );
    }
    assert!(
        carlile.traits.iter().any(|t| t == "thin_outbound"),
        "Carlile must carry thin_outbound"
    );
    assert!(
        !JobBoard::cargo_for_location(carlile, "ships", None).is_empty(),
        "Carlile must remain a board shipper"
    );
    assert!(
        !JobBoard::cargo_for_location(carlile, "receives", None).is_empty(),
        "Carlile must remain a board receiver"
    );

    let sourdough = by_name("Sourdough");
    assert_eq!(sourdough.facility_type, "travel_center");
    assert!(sourdough.ships.is_empty() && sourdough.receives.is_empty());

    assert!(
        !fb.locations.iter().any(|l| l.template),
        "Fairbanks must not stamp invented template warehouses"
    );
    assert!(
        !fb.locations.iter().any(|l| l.name.contains("Petro Star")),
        "Petro Star Fairbanks Terminal is held; must not land yet"
    );

    // Pass-through / fuel-only corridor towns stay without freight endpoints.
    for city_key in [
        "healy_ak_us",
        "tok_ak_us",
        "glennallen_ak_us",
        "blaine_wa_us",
        "dawson_creek_bc_ca",
        "fort_st_john_bc_ca",
        "fort_nelson_bc_ca",
        "watson_lake_yt_ca",
        "whitehorse_yt_ca",
        "prince_george_bc_ca",
    ] {
        let city = world
            .city(city_key)
            .unwrap_or_else(|_| panic!("{city_key}"));
        assert!(
            city.locations
                .iter()
                .all(|loc| loc.ships.is_empty() && loc.receives.is_empty()),
            "{city_key} must stay empty of freight roles"
        );
    }
}

#[test]
fn test_legs_are_sane_and_unique() {
    let world = world();
    let mut seen: HashSet<(String, String)> = HashSet::new();
    for leg in &world.legs {
        assert!(
            world.cities.contains_key(&leg.a),
            "unknown endpoint {}",
            leg.a
        );
        assert!(
            world.cities.contains_key(&leg.b),
            "unknown endpoint {}",
            leg.b
        );
        assert!(
            matches!(leg.terrain.as_str(), "flat" | "hills" | "mountain"),
            "{leg:?}"
        );
        // Real metro-twin and drayage corridors (Newark-NYC ~11 mi, Norfolk-
        // Virginia Beach ~18, New Haven-Bridgeport ~21) are legitimate short
        // freight lanes; only ban truly trivial or cross-country single legs.
        assert!(
            leg.highway == "Anton Anderson Memorial Tunnel" || (10.0..=800.0).contains(&leg.miles),
            "absurd mileage: {leg:?}"
        );
        // Directed uniqueness: the graph may author both A→B and B→A (ALCAN
        // Phase A FIX 1 southbound reverse edges). Undirected adjacency still
        // walks either orientation via Leg::other.
        let pair = (leg.a.clone(), leg.b.clone());
        assert!(
            seen.insert(pair),
            "duplicate directed leg {}-{}",
            leg.a,
            leg.b
        );
    }
}

fn leg_terrain(world: &World, a: &str, b: &str) -> &'static str {
    // A famous corridor may now be a multi-leg chain (e.g. Knoxville-Nashville
    // runs Knoxville->Cookeville->Nashville). The pinned landform lives on
    // whichever leg actually crosses it, so return the strongest terrain along
    // the whole drive (mountain > hills > flat).
    let route = supported(world, a, b);
    let rank = |t: &str| match t {
        "flat" => 0,
        "hills" => 1,
        "mountain" => 2,
        _ => panic!("unknown terrain {t}"),
    };
    let best = route
        .legs
        .iter()
        .map(|leg| leg.terrain.as_str())
        .max_by_key(|t| rank(t))
        .unwrap();
    match best {
        "flat" => "flat",
        "hills" => "hills",
        _ => "mountain",
    }
}

#[test]
fn test_famous_corridors_have_real_terrain() {
    // Pin well-known trucking geography so it cannot drift back to flat.
    // Each entry names the grade or landform that earns the label.
    let world = world();
    let expected = [
        // the legendary grades
        (("Nashville", "Atlanta"), "mountain"), // I-24 Monteagle Mountain
        (("Knoxville", "Nashville"), "mountain"), // I-40 Cumberland Plateau
        (("Charlotte", "Knoxville"), "mountain"), // I-40 Pigeon River Gorge
        (("Philadelphia", "Pittsburgh"), "mountain"), // PA Turnpike Alleghenies
        (("Baltimore", "Pittsburgh"), "mountain"), // Sideling Hill country
        (("Sacramento", "Reno"), "mountain"),   // I-80 Donner Pass
        (("Denver", "Albuquerque"), "mountain"), // I-25 Raton Pass
        (("Boise", "Portland"), "mountain"),    // I-84 Cabbage Hill
        (("Spokane", "Seattle"), "mountain"),   // I-90 Snoqualmie Pass
        (("Spokane", "Boise"), "mountain"),     // US-95 White Bird grade
        // honest rolling country
        (("St. Louis", "Kansas City"), "hills"), // I-70 Missouri River hills
        (("Wichita", "Kansas City"), "hills"),   // I-35 Flint Hills
        (("Oklahoma City", "Dallas"), "hills"),  // I-35 Arbuckle Mountains
        (("Memphis", "Nashville"), "hills"),     // I-40 Highland Rim
        (("Milwaukee", "Minneapolis"), "hills"), // I-94 driftless coulees
        (("New York", "Boston"), "hills"),       // I-95 rolling Connecticut
        (("Richmond", "Raleigh"), "hills"),      // I-85 piedmont
        (("Phoenix", "Los Angeles"), "hills"),   // I-10 San Gorgonio Pass
        (("Amarillo", "Albuquerque"), "hills"),  // I-40 Clines Corners climb
        // genuinely flat country stays flat
        (("Kansas City", "Denver"), "flat"), // I-70 across the high plains
        (("Chicago", "St. Louis"), "flat"),  // I-55 Illinois prairie
        (("New Orleans", "Houston"), "flat"), // I-10 Gulf coastal plain
        (("Omaha", "Cheyenne"), "flat"),     // I-80 Platte River valley
        (("Jacksonville", "Miami"), "flat"), // I-95 Florida coast
    ];
    for ((a, b), terrain) in expected {
        assert_eq!(leg_terrain(world, a, b), terrain, "{a}-{b}");
    }
}

#[test]
fn test_dijkstra_connects_every_city_pair() {
    let world = world();
    let names = world.city_names();
    let mut seen: HashSet<String> = HashSet::from([names[0].clone()]);
    let mut stack = vec![names[0].clone()];
    while let Some(city) = stack.pop() {
        for leg in &world.legs {
            if leg.a == city && !seen.contains(&leg.b) {
                seen.insert(leg.b.clone());
                stack.push(leg.b.clone());
            } else if leg.b == city && !seen.contains(&leg.a) {
                seen.insert(leg.a.clone());
                stack.push(leg.a.clone());
            }
        }
    }
    let all: HashSet<String> = names.into_iter().collect();
    assert_eq!(seen, all);
}

#[test]
fn test_original_map_is_preserved_for_old_saves() {
    let world = world();
    for (a, b) in ORIGINAL_ADJACENT_PAIRS {
        assert!(
            world.route_from_cities(&[*a, *b]).is_some(),
            "old direct leg {a}-{b} no longer resolves"
        );
    }
}

#[test]
fn test_synthetic_facility_approaches_stay_within_the_local_band() {
    // Josh's ruling 2026-07-24: local deadheads run 1 to 9 miles. 776 baked
    // approach records carried up to 35 miles because the facility's geocoded
    // pin landed counties away (his Kenosha straight-line deadhead); the
    // synthetic single-leg route is clamped until the placement audit
    // re-geocodes them. Real multi-leg street chains are never clamped.
    let world = world();
    for (city_key, name) in [
        ("madison_wi_us", "Madison Cold Storage"),
        ("kenosha_wi_us", "Kenosha Cold Storage"),
        ("yakima_wa_us", "Washington Fruit & Produce"),
    ] {
        let route = world.facility_approach_route(city_key, name).unwrap();
        assert!(
            (1.0..=9.0).contains(&route.miles()),
            "{city_key}/{name}: {}",
            route.miles()
        );
    }
}

#[test]
fn test_world_data_contains_real_weigh_station_stops() {
    // A 2026-08 design audit found the enforcement/HOS scale checks, the
    // weigh-station-lane ambience, and their tests were all dormant: every one
    // keys off `Stop.type == "weigh_station"`, but no leg had ever shipped a
    // stop of that type -- only OSM interchange signage reading "Weigh
    // Station" was baked, never promoted to a stop. This pins the promotion so
    // the whole system cannot silently go dark again.
    let world = world();
    let weigh_stations: Vec<_> = world
        .legs
        .iter()
        .flat_map(|leg| leg.stops.iter())
        .filter(|stop| stop.stop_type == "weigh_station")
        .collect();
    // 87 sole-destination "Weigh Station" interchange signs were promoted
    // across 78 legs; a floor well under that catches a regression without
    // pinning the exact count to future map-enrichment batches.
    assert!(weigh_stations.len() >= 50);

    for stop in weigh_stations {
        // Real facility, not a placeholder: sourced, actioned, spoken cleanly.
        assert!(stop.curated());
        assert!(!stop.source.is_empty());
        assert_eq!(stop.actions, vec!["inspect".to_string()]);
        assert!(stop.spoken_name().starts_with("weigh station:"));
    }
}

#[test]
fn test_dispatch_detours_around_a_truck_advisory_where_a_road_exists() {
    // US-550 over Red Mountain Pass carries a truck advisory (CDOT-style
    // warnings and carrier policy -- verified 2026-08-20 that no statute
    // exists, so it is strong avoidance, never refusal). Through freight from
    // Farmington takes the real detour: US-160/491/191 through Cortez and
    // Monticello to Moab, the road carriers actually use.
    let world = world();
    let r = shortest(world, "farmington_nm_us", "grand_junction_co_us");
    assert!(r.cities.iter().any(|c| c == "moab_ut_us"), "{:?}", r.cities);
    assert!(
        !r.legs.iter().any(|leg| {
            let ends: BTreeSet<&str> = [leg.a.as_str(), leg.b.as_str()].into_iter().collect();
            ends == BTreeSet::from(["durango_co_us", "montrose_co_us"])
        }),
        "through freight was routed over the warned pass"
    );
}

#[test]
fn test_a_warned_pass_still_serves_its_own_endpoints() {
    // A pair of towns whose only road is the warned one still routes -- the
    // advisory is a warning, not a wall, and Durango-to-Montrose freight has
    // no other road.
    let world = world();
    let r = shortest(world, "durango_co_us", "montrose_co_us");
    assert_eq!(r.cities, vec!["durango_co_us", "montrose_co_us"]);
}

#[test]
fn test_the_detour_leg_is_dispatchable() {
    // The Cortez/Moab detour is supported freight metadata, not just a graph
    // edge: a supported route from Farmington north must exist and use it.
    let world = world();
    let r = supported(world, "farmington_nm_us", "salt_lake_city_ut_us");
    assert!(r.cities.iter().any(|c| c == "moab_ut_us"), "{:?}", r.cities);
}

/// A road cannot be shorter than the straight line between its endpoints.
///
/// 36 legs were, because `leg.miles` is curated and some of the numbers were
/// simply too small -- Poplar Bluff to Jonesboro stored 55 miles for a 95 mile
/// road, Altus to Wichita Falls 50 for 85.
///
/// Nothing caught it because nothing disagreed: the corridor bake rescales
/// every layer onto `leg.miles`, so the grades ended exactly at the stored
/// figure and `state_miles` summed to it precisely. They were the same number
/// wearing different hats, not independent witnesses. This test is the one
/// witness that owes the data nothing.
#[test]
fn test_no_leg_is_shorter_than_the_straight_line_between_its_cities() {
    let world = world();
    let mut offenders: Vec<(&str, &str, f64, f64)> = Vec::new();
    for leg in &world.legs {
        let (Some(a), Some(b)) = (world.cities.get(&leg.a), world.cities.get(&leg.b)) else {
            continue;
        };
        if leg.miles == 0.0 {
            continue;
        }
        let (lat1, lon1, lat2, lon2) = (
            a.lat.to_radians(),
            a.lon.to_radians(),
            b.lat.to_radians(),
            b.lon.to_radians(),
        );
        let straight = 3958.8
            * 2.0
            * (((lat2 - lat1) / 2.0).sin().powi(2)
                + lat1.cos() * lat2.cos() * ((lon2 - lon1) / 2.0).sin().powi(2))
            .sqrt()
            .min(1.0)
            .asin();
        // A whole-mile figure may round up to half a mile under the true road.
        if leg.miles < straight - 0.5 {
            offenders.push((&leg.a, &leg.b, leg.miles, (straight * 10.0).round() / 10.0));
        }
    }
    assert!(
        offenders.is_empty(),
        "{} legs shorter than their own straight line: {:?}",
        offenders.len(),
        &offenders[..offenders.len().min(5)]
    );
}

/// Every corridor layer is keyed to leg-miles, so a corrected mileage has to
/// carry all of them with it.
///
/// Correcting 176 leg lengths meant rescaling 49,128 along-route positions.
/// Missing one container would have left its rows at the old scale, which on a
/// leg that grew 35 miles means a checkpoint or a limit change landing well
/// before where it belongs -- or past the end entirely.
#[test]
fn test_nothing_on_a_leg_sits_past_the_end_of_it() {
    let mut offenders: Vec<(&str, &str, &str, f64, f64)> = Vec::new();
    for leg in &world().legs {
        if leg.miles == 0.0 {
            continue;
        }
        let limit = leg.miles + 1.0;
        let layers: [(&str, Vec<f64>); 4] = [
            (
                "speed_limits",
                leg.speed_limits().iter().map(|s| s.at_mi).collect(),
            ),
            (
                "grade end",
                leg.grade_segments().iter().map(|g| g.end_mi).collect(),
            ),
            (
                "interchanges",
                leg.interchanges().iter().map(|x| x.at_mi).collect(),
            ),
            (
                "landmarks",
                leg.landmarks().iter().map(|m| m.at_mi).collect(),
            ),
        ];
        for (label, values) in layers {
            for value in values {
                if value > limit {
                    offenders.push((&leg.a, &leg.b, label, value, leg.miles));
                }
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "{} positions past their leg end: {:?}",
        offenders.len(),
        &offenders[..offenders.len().min(5)]
    );
}

#[test]
fn load_time_of_the_shipped_world_is_reported() {
    // Not a Python test: the port brief asks for the eager load to be timed.
    let start = std::time::Instant::now();
    let world = World::load_from(&data_dir()).unwrap();
    let elapsed = start.elapsed();
    println!(
        "World::load_from({}) took {:?} ({} cities, {} legs)",
        data_dir().display(),
        elapsed,
        world.cities.len(),
        world.legs.len()
    );
    assert!(world.legs.len() > 1000);
}
