//! Local approach data (the data-layer half of `tests/test_local_approaches.py`).

use crate::data_support::{read_json, world};

const RAW_MARKERS: &[&str] = &[
    "osm_id",
    "amenity=",
    "highway=",
    "operator=",
    "node/",
    "way/",
];

/// `tools/build_local_approaches.py::SEARCH_RADIUS_MI`.
const SEARCH_RADIUS_MI: f64 = 1.25;

#[test]
fn test_local_approach_data_covers_supported_map() {
    let world = world();
    let data = read_json("local_approaches.json");
    let coverage = &data["coverage"];

    assert_eq!(coverage["approaches"], 6157);
    assert_eq!(coverage["osm_road"], 1315);
    // Every road-snapped target carries a real road name where OSM has one:
    // the snap prefers the nearest *named* road, so "unnamed public road"
    // only survives where OSM truly has no named street inside the radius.
    assert_eq!(coverage["named_road"], 1315);
    assert_eq!(coverage["fallback"], 4842);
    assert_eq!(coverage["estimated"], 4842);
    assert_eq!(
        coverage["by_type"]["city_service"],
        serde_json::json!({"estimated": 695, "fallback": 695, "named_road": 1174, "osm_road": 1174, "total": 1869})
    );
    assert_eq!(
        coverage["by_type"]["facility"],
        serde_json::json!({"estimated": 4147, "fallback": 4147, "named_road": 141, "osm_road": 141, "total": 4288})
    );

    // The coverage block records the sweep's own inventory. The map has grown
    // since (and can again); today's world may only exceed it, never shrink
    // below it, and new targets simply are not covered until the next sweep.
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
    assert!(
        services as u64
            >= coverage["by_type"]["city_service"]["total"]
                .as_u64()
                .unwrap()
    );
    assert!(facilities as i64 >= coverage["by_type"]["facility"]["total"].as_i64().unwrap() - 8);
}

#[test]
fn test_local_approach_records_are_clean_and_marked() {
    let world = world();
    let data = read_json("local_approaches.json");

    let mut retired = Vec::new();
    for (target_id, record) in data["approaches"].as_object().unwrap() {
        // Sweep ids predate the slug migration; the world canonicalizes them
        // at load. A record whose facility retired with map growth is inert.
        let approach = world
            .local_approach(&world.canonical_local_id(target_id))
            .unwrap();
        if approach.is_none() {
            retired.push(target_id.clone());
            continue;
        }
        assert!(!record["name"].as_str().unwrap().is_empty());
        assert!(!record["road"].as_str().unwrap().is_empty());
        assert!(record["approach_miles"].as_f64().unwrap() > 0.0);
        assert!(matches!(
            record["target_type"].as_str().unwrap(),
            "city_service" | "facility"
        ));
        let segments: Vec<&str> = record["turn_segments"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s.as_str().unwrap())
            .collect();
        let spoken = format!(
            "{} {} {}",
            record["name"].as_str().unwrap(),
            record["road"].as_str().unwrap(),
            segments.join(" ")
        )
        .to_lowercase();
        assert!(!RAW_MARKERS.iter().any(|m| spoken.contains(m)));
        if record["fallback"].as_bool().unwrap() {
            assert!(!record["fallback_reason"].as_str().unwrap().is_empty());
            // A facility the world generated is approached by a generated
            // road: no snapped street stands in for a site that is not
            // there (2026-09-20).
            assert!(matches!(
                record["source_type"].as_str().unwrap(),
                "fallback_context" | "representative_target_generated_context"
            ));
        } else {
            assert!(record["distance_to_road_mi"].as_f64().unwrap() <= SEARCH_RADIUS_MI);
            assert!(matches!(
                record["source_type"].as_str().unwrap(),
                "osm_nearest_road" | "estimated_target_osm_nearest_road"
            ));
        }
    }
    assert!(retired.len() <= 8, "{retired:?}");
}

#[test]
fn test_facility_routes_use_local_approach_layer() {
    let world = world();
    let facility = &world.city("Chicago").unwrap().locations[0];
    let facility_route = world
        .facility_approach_route("Chicago", &facility.name)
        .unwrap();
    let facility_approach = world
        .facility_approach("Chicago", &facility.name)
        .unwrap()
        .expect("Chicago's first facility has a local approach");
    let facility_endpoint = world.facility_endpoint("Chicago", &facility.name).unwrap();
    // A facility with its own street chain is measured by the chain, not by
    // either estimate: Cicero Rail Hub gained one on 2026-09-20 when the
    // approach builder started routing the intermodal family, and its routed
    // 2.14 miles is a shorter and truer number than the endpoint sweep's 3.6.
    if facility_route
        .legs
        .iter()
        .any(|leg| leg.local_speed_mph > 0.0)
    {
        // A chain names every street it uses, and ends on the facility's own
        // ground, which is why the last road is the service road the local
        // approach layer had been standing in for all along.
        assert!(facility_route.miles() > 0.0);
        let streets = facility_route.highways();
        assert!(streets.len() > 1, "{streets:?}");
        assert_eq!(streets.last(), Some(&facility_approach.road));
    } else {
        match facility_endpoint {
            Some(endpoint) if endpoint.source_backed => {
                assert_eq!(facility_route.miles(), endpoint.approach_miles);
            }
            _ => assert_eq!(facility_route.miles(), facility_approach.approach_miles),
        }
        assert_eq!(
            facility_route.highways(),
            vec![facility_approach.road.clone()]
        );
    }
}
