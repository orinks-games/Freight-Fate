//! Facility endpoint data (the data-layer half of
//! `tests/test_facility_endpoints.py`).

use std::collections::HashSet;

use crate::data_support::{read_json, world};

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
fn test_facility_endpoint_data_covers_supported_facilities() {
    let world = world();
    let data = read_json("facility_endpoints.json");
    let coverage = &data["coverage"];

    // The counts move with every sweep (ROADMAP keeps their history), so what
    // is checked is that they add up: every facility is sourced or a
    // fallback, and every sourced row carries the screen's verdict.
    let n = |v: &serde_json::Value| v.as_u64().unwrap();
    assert!(n(&coverage["facilities"]) > 0);
    assert_eq!(
        n(&coverage["source_backed"]) + n(&coverage["fallback"]),
        n(&coverage["facilities"])
    );
    let screen = &coverage["screen"];
    assert_eq!(screen["not_screened"], 0);
    assert_eq!(
        n(&screen["passed"]) + n(&screen["refused"]),
        n(&coverage["source_backed"])
    );
    assert!(n(&screen["trade_assumed"]) <= n(&screen["passed"]));
    assert_eq!(coverage["nearest_road_context"], 0);
    assert_eq!(coverage["turn_level_geometry"], 0);
    assert_eq!(coverage["gate_yard_dock_hints"], 0);

    // The sweep predates the slug migration and the map expansion: its
    // records must keep resolving onto today's facilities (legacy-id
    // translation), while facilities added since the sweep are simply not
    // covered yet. A few records retire when map growth replaces a template
    // facility with a real one (Gulfport/Mobile), never more than a handful.
    let facilities: HashSet<String> = world
        .city_names()
        .iter()
        .flat_map(|city| world.cities[city].locations.iter().map(|l| l.id.clone()))
        .collect();
    let mut resolved: HashSet<String> = HashSet::new();
    let mut missing: Vec<String> = Vec::new();
    for facility_id in data["endpoints"].as_object().unwrap().keys() {
        match world.facility_by_id(facility_id) {
            Ok(location) => {
                resolved.insert(location.id.clone());
            }
            Err(_) => missing.push(facility_id.clone()),
        }
    }
    assert!(resolved.is_subset(&facilities));
    assert!(
        resolved.len() as i64 >= coverage["facilities"].as_i64().unwrap() - 8,
        "{:?}",
        &missing[..missing.len().min(10)]
    );
}

#[test]
fn test_facility_endpoint_records_are_clean_and_honest() {
    let world = world();
    let data = read_json("facility_endpoints.json");

    for (facility_id, record) in data["endpoints"].as_object().unwrap() {
        if world.facility_by_id(facility_id).is_err() {
            continue; // facility retired by map growth; record is inert
        }
        let endpoint = world
            .facility_endpoint(record["city"].as_str().unwrap(), facility_id)
            .unwrap();
        assert!(endpoint.is_some());
        let spoken = format!(
            "{} {} {}",
            record["facility_name"].as_str().unwrap(),
            record["endpoint_name"].as_str().unwrap(),
            record["approach_road"].as_str().unwrap()
        )
        .to_lowercase();
        assert!(!RAW_MARKERS.iter().any(|m| spoken.contains(m)));
        assert!(!record["source_note"].as_str().unwrap().is_empty());
        assert!(!record["gate_hint"].as_bool().unwrap());
        assert!(!record["yard_hint"].as_bool().unwrap());
        assert!(!record["dock_hint"].as_bool().unwrap());
        assert!(!record["turn_level_geometry"].as_bool().unwrap());
        if record["source_backed"].as_bool().unwrap() {
            assert!(!record["fallback"].as_bool().unwrap());
            assert_eq!(record["source_type"], "osm_facility_endpoint");
            // The screen verdict rides in the row, so a railway line that
            // found no replacement is never mistaken for a yard gate.
            let verdict = record["endpoint_screen"].as_str().unwrap();
            assert!(verdict == "passed" || verdict == "refused");
            if verdict == "refused" {
                assert!(!record["endpoint_screen_reason"]
                    .as_str()
                    .unwrap()
                    .is_empty());
            }
            if let Some(kind) = record.get("match_kind") {
                assert!(kind == "read" || kind == "assumed");
            }
            assert!(record["approach_miles"].as_f64().unwrap() <= 8.0);
            assert!(record["approach_miles"].as_f64().unwrap() > 0.0);
            assert_eq!(record["approach_road"], "local facility access road");
            assert!(record["source_note"]
                .as_str()
                .unwrap()
                .contains("not claimed by this layer"));
        } else {
            assert!(record["fallback"].as_bool().unwrap());
            assert!(!record["fallback_reason"].as_str().unwrap().is_empty());
            assert_eq!(record["source_type"], "representative_fallback");
            let estimated = record
                .get("estimated")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            if estimated {
                // Estimated-near-city pins keep a real offset; they must not
                // claim source-backed OSM at zero miles.
                assert!(record["approach_miles"].as_f64().unwrap() > 0.0);
                assert!(record["source_note"]
                    .as_str()
                    .unwrap()
                    .contains("Estimated-near-city"));
                assert!(record["fallback_reason"]
                    .as_str()
                    .unwrap()
                    .to_lowercase()
                    .contains("estimated near city"));
            } else {
                assert_eq!(record["approach_miles"].as_f64().unwrap(), 0.0);
            }
        }
    }
}

#[test]
fn test_facility_route_prefers_source_backed_endpoint_when_available() {
    let world = world();
    // A sourced endpoint with NO street chain (it sits a few blocks from the
    // city anchor, under the chain floor). The Abilene energy terminal this
    // used to pin gained a chain in the 2026-09-17 re-sweep.
    let facility = world
        .facility_by_id("muncie-in-us:cross_dock:muncie-cross-dock")
        .unwrap();
    let endpoint = world
        .facility_endpoint("muncie_in_us", &facility.id)
        .unwrap()
        .expect("the Muncie cross-dock has an endpoint");
    let route = world
        .facility_approach_route("muncie_in_us", &facility.name)
        .unwrap();

    assert!(endpoint.source_backed);
    assert!((route.miles() - endpoint.approach_miles).abs() < 1e-9);
    let approach = world
        .facility_approach("muncie_in_us", &facility.name)
        .unwrap()
        .unwrap();
    assert_eq!(route.highways(), vec![approach.road.clone()]);
}

#[test]
fn test_facility_route_falls_back_to_local_approach_for_representative_endpoint() {
    let world = world();
    let facility = world
        .facility_by_id("abilene:grocery_retail_dc:abilene-grocery-distribution-center")
        .unwrap();
    let endpoint = world
        .facility_endpoint("Abilene", &facility.id)
        .unwrap()
        .expect("the Abilene grocery DC has an endpoint");
    let approach = world
        .facility_approach("Abilene", &facility.name)
        .unwrap()
        .expect("the Abilene grocery DC has a local approach");
    let route = world
        .facility_approach_route("Abilene", &facility.name)
        .unwrap();

    assert!(endpoint.fallback);
    assert!((route.miles() - approach.approach_miles).abs() < 1e-9);
    assert_eq!(route.highways(), vec![approach.road.clone()]);
}
