//! Interchange data model, spoken phrasing and parse validation (the data
//! half of `tests/test_interchanges.py`; the cue wiring is in
//! `sim_interchanges.rs`).

use ff_core::data::world_models::{format_route_ref, join_destinations, Interchange};
use ff_core::data::world_parsing::{
    parse_interchange, parse_osm_advisory_speed, ramp_advisory_from_osm_tags,
};
use serde_json::{json, Value};

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| s.to_string()).collect()
}

#[test]
fn advisory_speed_normalizes_osm_units_and_rejects_malformed_values() {
    assert_eq!(parse_osm_advisory_speed("35 mph"), Some(35.0));
    assert!((parse_osm_advisory_speed("80 km/h").unwrap() - 49.7097).abs() < 0.001);
    assert!((parse_osm_advisory_speed("80").unwrap() - 49.7097).abs() < 0.001);
    for malformed in ["signals", "35;45", "walk", "0 mph", "250 km/h", ""] {
        assert_eq!(parse_osm_advisory_speed(malformed), None, "{malformed}");
    }
}

#[test]
fn interchange_preserves_directional_observations_and_requires_provenance() {
    let mut value = raw();
    {
        let obj = value.as_object_mut().unwrap();
        obj.insert("ramp_advisory_forward".into(), json!("30 mph"));
        obj.insert("ramp_advisory_backward".into(), json!("80 km/h"));
        obj.insert(
            "ramp_advisory_source".into(),
            json!("OpenStreetMap maxspeed:advisory tags (read)"),
        );
    }
    let parsed = parse_interchange(&value, 100.0, "A", "B", "I-1").unwrap();
    assert_eq!(parsed.ramp_advisory_mph_forward, Some(30.0));
    assert!((parsed.ramp_advisory_mph_backward.unwrap() - 49.7097).abs() < 0.001);

    value
        .as_object_mut()
        .unwrap()
        .remove("ramp_advisory_source");
    assert!(parse_interchange(&value, 100.0, "A", "B", "I-1").is_err());
}

/// Cleveland's exit 170A is signed 15 mph. The bake read the bare "15" as
/// km/h and stored 9.321, and the truck was held to 9 mph down the whole
/// ramp.
#[test]
fn a_baked_bare_number_read_as_kmh_loads_as_the_mph_on_the_sign() {
    let mut value = raw();
    {
        let obj = value.as_object_mut().unwrap();
        obj.insert("ramp_advisory_forward".into(), json!(30.0));
        obj.insert("ramp_advisory_backward".into(), json!(9.321));
        obj.insert(
            "ramp_advisory_source".into(),
            json!("OpenStreetMap maxspeed:advisory tags (read)"),
        );
    }
    let parsed = parse_interchange(&value, 100.0, "A", "B", "I-1").unwrap();
    assert_eq!(parsed.ramp_advisory_mph_forward, Some(30.0));
    assert_eq!(parsed.ramp_advisory_mph_backward, Some(15.0));
}

#[test]
fn osm_directional_advisory_has_priority_and_is_ramp_only() {
    let tags = json!({
        "maxspeed:advisory": "45 mph",
        "maxspeed:advisory:forward": "25 mph",
        "maxspeed:advisory:backward": "30 mph"
    });
    let tags = tags.as_object().unwrap();
    assert_eq!(ramp_advisory_from_osm_tags(tags, true, true), Some(25.0));
    assert_eq!(ramp_advisory_from_osm_tags(tags, false, true), Some(30.0));
    assert_eq!(ramp_advisory_from_osm_tags(tags, true, false), None);
}

// --- phrasing ---------------------------------------------------------------

#[test]
fn test_full_phrase_reads_naturally() {
    let ix = Interchange {
        at_mi: 72.7,
        exit_ref: "7".to_string(),
        destinations: strings(&["Trenton", "New York"]),
        via: "US 1 North".to_string(),
        highway: "I-95".to_string(),
        source: "OSM".to_string(),
        ..Default::default()
    };
    assert_eq!(
        ix.spoken_phrase(),
        "exit 7 for US-1 North toward Trenton and New York"
    );
    assert_eq!(
        ix.near_phrase(),
        "Exit 7 for US-1 North toward Trenton and New York now."
    );
}

#[test]
fn test_bare_exit_number_still_speaks() {
    let ix = Interchange {
        at_mi: 5.0,
        exit_ref: "42".to_string(),
        source: "OSM".to_string(),
        ..Default::default()
    };
    assert_eq!(ix.spoken_phrase(), "exit 42");
}

#[test]
fn test_destinations_without_exit_number() {
    let ix = Interchange {
        at_mi: 5.0,
        destinations: strings(&["Camden", "Shore Points"]),
        via: "NJ 129 South".to_string(),
        source: "OSM".to_string(),
        ..Default::default()
    };
    assert_eq!(
        ix.spoken_phrase(),
        "exit for NJ-129 South toward Camden and Shore Points"
    );
}

#[test]
fn test_named_junction_without_ref_or_destinations() {
    let ix = Interchange {
        at_mi: 5.0,
        name: "Scranton Beltway".to_string(),
        source: "OSM".to_string(),
        ..Default::default()
    };
    assert_eq!(ix.spoken_phrase(), "exit for Scranton Beltway");
}

#[test]
fn test_route_ref_formatting() {
    assert_eq!(format_route_ref("US 1 North"), "US-1 North");
    assert_eq!(format_route_ref("I 95"), "I-95");
    assert_eq!(format_route_ref("NJ 29 South"), "NJ-29 South");
    assert_eq!(
        format_route_ref("I 95 North;NJTP North"),
        "I-95 North and NJTP North"
    );
    assert_eq!(format_route_ref(""), "");
}

#[test]
fn test_join_destinations_oxford_comma() {
    assert_eq!(join_destinations(&strings(&["Trenton"])), "Trenton");
    assert_eq!(
        join_destinations(&strings(&["Trenton", "New York"])),
        "Trenton and New York"
    );
    assert_eq!(join_destinations(&strings(&["A", "B", "C"])), "A, B, and C");
    assert_eq!(join_destinations(&[]), "");
}

// --- parsing / validation ---------------------------------------------------

/// `tests/test_interchanges.py::_raw`.
fn raw() -> Value {
    json!({
        "at_mi": 10.0,
        "exit_ref": "7",
        "destinations": ["Trenton"],
        "via": "US 1 North",
        "source": "OSM",
    })
}

fn raw_with(overrides: Value) -> Value {
    let mut base = raw();
    let map = base.as_object_mut().expect("object");
    for (key, value) in overrides.as_object().expect("object") {
        map.insert(key.clone(), value.clone());
    }
    base
}

#[test]
fn test_parse_round_trips_fields() {
    let ix = parse_interchange(&raw(), 50.0, "A", "B", "I-95").unwrap();
    assert_eq!(ix.at_mi, 10.0);
    assert_eq!(ix.exit_ref, "7");
    assert_eq!(ix.destinations, strings(&["Trenton"]));
    assert_eq!(ix.via, "US 1 North");
    assert_eq!(ix.highway, "I-95"); // inherited from the leg default
}

#[test]
fn test_parse_accepts_string_destination() {
    let ix = parse_interchange(
        &raw_with(json!({"destinations": "Trenton"})),
        50.0,
        "A",
        "B",
        "I-95",
    )
    .unwrap();
    assert_eq!(ix.destinations, strings(&["Trenton"]));
}

#[test]
fn test_parse_requires_something_sayable() {
    let err = parse_interchange(
        &raw_with(json!({"exit_ref": "", "destinations": [], "name": ""})),
        50.0,
        "A",
        "B",
        "I-95",
    )
    .unwrap_err();
    assert!(err.to_string().contains("no exit ref"), "{err}");
}

#[test]
fn test_parse_requires_source() {
    let err =
        parse_interchange(&raw_with(json!({"source": ""})), 50.0, "A", "B", "I-95").unwrap_err();
    assert!(err.to_string().contains("no source"), "{err}");
}

#[test]
fn test_parse_rejects_at_mi_out_of_range() {
    let err =
        parse_interchange(&raw_with(json!({"at_mi": 99.0})), 50.0, "A", "B", "I-95").unwrap_err();
    assert!(err.to_string().contains("outside leg mileage"), "{err}");
}

#[test]
fn test_parse_rejects_raw_osm_text() {
    let err = parse_interchange(
        &raw_with(json!({"destinations": ["node/12345"]})),
        50.0,
        "A",
        "B",
        "I-95",
    )
    .unwrap_err();
    assert!(err.to_string().contains("raw OSM"), "{err}");
}

// --- exit labels ------------------------------------------------------------

#[test]
fn test_interchange_exit_label_property() {
    assert_eq!(
        Interchange {
            at_mi: 5.0,
            exit_ref: "7".to_string(),
            source: "x".to_string(),
            ..Default::default()
        }
        .exit_label(),
        "exit 7"
    );
    assert_eq!(
        Interchange {
            at_mi: 5.0,
            destinations: strings(&["Camden"]),
            source: "x".to_string(),
            ..Default::default()
        }
        .exit_label(),
        ""
    );
}

// --- text cleanup: exit-ref whitespace + via-redundant destinations ---------

#[test]
fn test_parse_normalizes_exit_ref_whitespace() {
    let ix = parse_interchange(
        &raw_with(json!({"exit_ref": "103 B"})),
        50.0,
        "A",
        "B",
        "I-70",
    )
    .unwrap();
    assert_eq!(ix.exit_ref, "103B");
    assert_eq!(ix.exit_label(), "exit 103B");
}

#[test]
fn test_spoken_phrase_drops_via_redundant_destination() {
    let ix = Interchange {
        at_mi: 5.0,
        exit_ref: "101A".to_string(),
        via: "I 70".to_string(),
        destinations: strings(&["I 70 East", "Parsons Avenue"]),
        source: "x".to_string(),
        ..Default::default()
    };
    assert_eq!(
        ix.spoken_phrase(),
        "exit 101A for I-70 toward Parsons Avenue"
    );
}

#[test]
fn test_spoken_phrase_via_only_when_all_destinations_redundant() {
    let ix = Interchange {
        at_mi: 5.0,
        exit_ref: "101A".to_string(),
        via: "I 70".to_string(),
        destinations: strings(&["I 70 East"]),
        source: "x".to_string(),
        ..Default::default()
    };
    assert_eq!(ix.spoken_phrase(), "exit 101A for I-70");
}

#[test]
fn test_spoken_phrase_keeps_unrelated_destinations() {
    let ix = Interchange {
        at_mi: 5.0,
        exit_ref: "7".to_string(),
        via: "US 1 North".to_string(),
        destinations: strings(&["Trenton", "New York"]),
        source: "x".to_string(),
        ..Default::default()
    };
    assert_eq!(
        ix.spoken_phrase(),
        "exit 7 for US-1 North toward Trenton and New York"
    );
}
