//! Local geometry data (the data-layer half of `tests/test_local_geometry.py`;
//! the Trip case is ignored until `sim::trip` lands).

use crate::data_support::{read_json, world};
use ff_core::data::world_models::{Leg, Route};

const RAW_MARKERS: &[&str] = &[
    "osm_id",
    "amenity=",
    "highway=",
    "operator=",
    "node/",
    "way/",
    "source_ref",
];

#[test]
fn test_local_geometry_data_covers_supported_map() {
    let world = world();
    let data = read_json("local_geometry.json");
    let coverage = &data["coverage"];

    let decision = data["generated"]["routing_decision"].as_str().unwrap();
    assert!(decision.contains("OpenRouteService driving-hgv"));
    assert!(decision.contains("not ORS-certified HGV routes"));
    assert_eq!(coverage["targets"], 6144);
    // 1077, not the 1076 this pinned before the 2026-08-25 re-bake. The
    // shipped file predated a builder fix and had never been regenerated, so
    // one city service -- the Jonesboro garage -- was still carrying the
    // single-line fallback "Use Commerce Square for the local approach" where
    // the builder now finds a real eight-turn route. Same extracts, same
    // accessed date, and two consecutive rebuilds are byte-identical, so this
    // is the bake catching up rather than the data moving. The Python case
    // (`tests/test_local_geometry.py`) moved with the re-bake; this port was
    // written from the pre-re-bake numbers and did not.
    assert_eq!(coverage["turn_level"], 1077);
    assert_eq!(coverage["fallback"], 5067);
    assert_eq!(coverage["estimated"], 5067);
    assert_eq!(
        coverage["by_type"]["city_service"],
        serde_json::json!({"estimated": 792, "fallback": 792, "total": 1869, "turn_level": 1077})
    );
    assert_eq!(
        coverage["by_type"]["facility"],
        serde_json::json!({"estimated": 4275, "fallback": 4275, "total": 4275, "turn_level": 0})
    );

    // The coverage block records the sweep's own inventory; the map has grown
    // since, so today's world may only exceed it (new targets are simply not
    // covered until the next sweep re-runs).
    let services: usize = world
        .city_names()
        .iter()
        .map(|city| world.city_services(city).unwrap().len())
        .sum();
    let facilities: usize = world
        .city_names()
        .iter()
        .map(|city| world.cities[city].locations.len())
        .sum();
    assert!((services + facilities) as u64 >= coverage["targets"].as_u64().unwrap());
}

#[test]
fn test_local_geometry_records_are_clean_and_honest() {
    let world = world();
    let data = read_json("local_geometry.json");

    let mut retired = Vec::new();
    for (target_id, record) in data["geometries"].as_object().unwrap() {
        // Sweep ids predate the slug migration; the world canonicalizes them
        // at load. A record whose facility retired with map growth is inert.
        let geometry = world
            .local_geometry(&world.canonical_local_id(target_id))
            .unwrap();
        if geometry.is_none() {
            retired.push(target_id.clone());
            continue;
        }
        assert!(!record["name"].as_str().unwrap().is_empty());
        let segments = record["segments"].as_array().unwrap();
        assert!(!segments.is_empty());
        assert!(record["total_miles"].as_f64().unwrap() > 0.0);
        let mut parts = vec![
            record["name"].as_str().unwrap().to_string(),
            record["final_hint"].as_str().unwrap_or("").to_string(),
        ];
        parts.extend(
            segments
                .iter()
                .map(|s| s["road"].as_str().unwrap().to_string()),
        );
        parts.extend(
            segments
                .iter()
                .map(|s| s["cue"].as_str().unwrap().to_string()),
        );
        let spoken = parts.join(" ").to_lowercase();
        assert!(!RAW_MARKERS.iter().any(|m| spoken.contains(m)));
        if record["turn_level"].as_bool().unwrap() {
            assert!(!record["fallback"].as_bool().unwrap());
            assert_eq!(record["source_type"], "osm_local_road_graph");
            assert_eq!(record["target_type"], "city_service");
            assert!(!segments.is_empty());
        } else {
            assert!(record["fallback"].as_bool().unwrap());
            assert!(!record["fallback_reason"].as_str().unwrap().is_empty());
            assert_eq!(record["source_type"], "nearest_road_context");
        }
    }
    assert!(retired.len() <= 8, "{retired:?}");
}

#[test]
fn test_facility_geometry_stays_estimated_fallback() {
    let world = world();
    let facility = &world.city("Chicago").unwrap().locations[0];
    let geometry = world
        .facility_geometry("Chicago", &facility.name)
        .unwrap()
        .expect("Chicago's first facility has local geometry");

    assert!(!geometry.turn_level);
    assert!(geometry.fallback);
    assert!(geometry.estimated);
    assert!(geometry
        .fallback_reason
        .contains("representative freight-market coordinates"));
}

#[test]
fn test_local_geometry_trip_uses_local_turn_cues() {
    use ff_core::sim::trip::{Trip, TripOptions};
    use ff_core::sim::vehicle::TruckState;
    use ff_core::sim::weather::{WeatherKind, WeatherSystem};

    // city_service_route was retired with the drive-to-city-services feature;
    // this test only needs a turn-level Route, built straight from the local
    // geometry data it used to wrap.
    let world = world();
    let city = world.resolve_city_key("Chicago");
    let geometry = world
        .local_geometry(&format!("city_service:{city}:freight_market"))
        .unwrap()
        .expect("Chicago's freight market has turn-level geometry");
    assert!(geometry.turn_level);
    let legs: Vec<Leg> = geometry
        .segments
        .iter()
        .map(|segment| {
            Leg::local(
                &city,
                segment.miles,
                &segment.road,
                &segment.cue,
                segment.speed_mph,
            )
        })
        .collect();
    let route = Route::from_legs(vec![city.clone(); legs.len() + 1], legs);
    let mut weather = WeatherSystem::new("great_lakes", Some(1), None, None, true);
    weather.current = WeatherKind::Clear;
    let trip = Trip::new(
        route,
        TruckState::default(),
        weather,
        TripOptions {
            seed: Some(1),
            world: Some(world),
            ..Default::default()
        },
    );

    let cues: Vec<_> = trip
        .navigation_cues
        .iter()
        .filter(|cue| cue.kind == "local_turn")
        .collect();

    assert!(!cues.is_empty());
    assert!(cues[0].near_text.starts_with("Start on "));
    assert!(!cues
        .iter()
        .any(|cue| cue.near_text.to_lowercase().contains("merge onto")));
    // Directional bake: every boundary cue is a turn with a side or an
    // explicit continue, never the old directionless "Turn onto".
    assert!(cues[1..].iter().all(|cue| {
        cue.near_text.starts_with("Turn left onto")
            || cue.near_text.starts_with("Turn right onto")
            || cue.near_text.starts_with("Continue onto")
    }));
}
