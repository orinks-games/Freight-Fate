//! World data and route graph tests (port of `tests/test_world.py`).

use std::collections::{BTreeSet, HashSet};

use crate::data_support::{data_dir, shortest, supported, world};
use ff_core::data::regions::REGIONS;
use ff_core::data::world::{max_alternate_miles, World};
use ff_core::data::world_constants::{
    is_stand_in_market, lookup, set_contains, DEFAULT_POI_ACTIONS, FREIGHT_LOCATION_TYPES,
    PARKING_CERTAINTY_LABELS, POI_ACTIONS, STOP_DIRECTIONS, STOP_TYPE_LABELS,
};
use ff_core::data::world_models::Route;
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
            assert!(!loc.roles.is_empty());
            assert!(!loc.ships.is_empty() || !loc.receives.is_empty());
        }
    }
}

#[test]
fn test_mall_of_america_is_a_receive_only_stop_in_bloomington() {
    // The owner asked for it by name (2026-10-24): a real mall at its real
    // position, which takes freight in at its dock and never ships a load.
    let world = world();
    let mall = world
        .facility_location("minneapolis_mn_us", "Mall of America")
        .unwrap();
    assert_eq!(mall.facility_type, "shopping_mall");
    assert_eq!(mall.locality, "Bloomington");
    assert!(mall.ships.is_empty(), "{:?}", mall.ships);
    assert!(mall.receives.iter().any(|k| k == "retail"));
    assert_eq!(mall.roles, vec!["receiver".to_string()]);
    // Bloomington, about nine miles south of the Minneapolis city point.
    assert!((mall.lat - 44.8547).abs() < 0.01 && (mall.lon + 93.2422).abs() < 0.01);
    assert_eq!(
        ff_core::models::jobs::facility_text(
            &mall.facility_type,
            &mall.name,
            "Minneapolis",
            &mall.locality
        ),
        "Mall of America near Bloomington in Minneapolis"
    );
}

#[test]
fn test_home_terminal_prefers_explicit_terminal_and_falls_back_to_yard() {
    let world = world();
    let explicit = world.home_terminal("Nashville").unwrap();
    let fallback = world.home_terminal("Chicago").unwrap();

    assert_eq!(explicit.name, "Music City Freight");
    assert_eq!(explicit.label(), "company terminal");
    assert_eq!(
        explicit.spoken_name(),
        "company terminal: Music City Freight"
    );
    assert_eq!(explicit.service_area(), "Nashville, Tennessee");
    assert_eq!(fallback.name, "Chicago Company Yard");
    assert_eq!(fallback.label(), "company yard");
    assert_eq!(fallback.spoken_name(), "company yard: Chicago Company Yard");
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
        if is_stand_in_market(&city.key) && !curated {
            assert!(
                templates.len() <= 1,
                "{}: a stand-in market gets one yard, not {:?}",
                city.name,
                templates
            );
            assert!(templates.iter().all(|kind| *kind == "company_yard"));
        } else {
            assert!(city.locations.len() >= 5);
        }
        assert!(!city.market_tags.is_empty());
        assert!(city.locations.iter().any(|loc| loc.template));
        assert!(city
            .locations
            .iter()
            .any(|loc| loc.facility_type == "company_yard" || loc.facility_type == "terminal"));
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
            curated.iter().all(|stop| stop.parking != "unknown"),
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

    let event = tolls[0];
    assert_eq!(event.road, "New Jersey Turnpike");
    assert_eq!(event.authority, "New Jersey Turnpike Authority");
    assert_eq!(event.method, "ticket_system");
    assert!(event.amount > 0.0);
    assert!(event.estimated);
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
            24.0 < city.lat && city.lat < 50.0,
            "{}: lat {}",
            city.name,
            city.lat
        );
        assert!(
            -125.0 < city.lon && city.lon < -66.0,
            "{}: lon {}",
            city.name,
            city.lon
        );
        let floor = if is_stand_in_market(&city.key) && city.locations.iter().all(|l| l.template) {
            1
        } else {
            2
        };
        assert!(
            city.locations.len() >= floor,
            "{}: too few freight locations",
            city.name
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
            (10.0..=800.0).contains(&leg.miles),
            "absurd mileage: {leg:?}"
        );
        let pair = if leg.a <= leg.b {
            (leg.a.clone(), leg.b.clone())
        } else {
            (leg.b.clone(), leg.a.clone())
        };
        assert!(seen.insert(pair), "duplicate leg {}-{}", leg.a, leg.b);
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
