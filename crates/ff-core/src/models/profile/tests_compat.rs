//! Ported from `tests/test_save_compat.py`: old save dicts keep parsing through
//! `Profile::from_dict` on this snapshot.
//!
//! `Profile::load` refuses pre-1.9 files outright (see `tests.rs`, the legacy
//! career gate), but the dict-level tolerance pinned here still matters:
//! cloud verification and every future migration run through `from_dict`,
//! which must keep absorbing missing, extra, and malformed fields without
//! crashing.

use serde_json::{json, Map, Value};

use super::*;
use crate::models::business_constants::COMPANY_DRIVER;
use crate::models::dispatch_policy::dispatch_policy;

/// The exact top-level shape a v1.7-era build (SAVE_VERSION 4) wrote.
///
/// No hos/duty_log blocks, no business-status fields, no tire wear, and
/// `owned_trucks` defaulted to the starter rig.
fn v4_nightly_payload() -> Map<String, Value> {
    let value = json!({
        "version": 4,
        "name": "Legacy Driver",
        "money": 12_345.0,
        "current_city": "Chicago",
        "truck_damage_pct": 12.0,
        "truck_fuel_gal": 90.0,
        "game_hours": 55.0,
        "tutorial_done": true,
        "truck": "rig",
        "owned_trucks": ["rig"],
        "upgrades": {"engine_tune": 1},
        "active_trip": null,
        "dispatch_board_cache": null,
        "fatigue": 20.0,
        "pay_advance": 0.0,
        "pay_advance_used_for_load": false,
        "career": {
            "xp": 12_000.0,
            "reputation": 74.0,
            "deliveries": 18,
            "on_time_deliveries": 16,
            "total_miles": 9_000.0,
            "total_earnings": 30_000.0,
        },
        "market": {"seed": 1234, "day": 2},
        "achievements": ["first_dispatch"],
        "achievement_stats": {},
    });
    match value {
        Value::Object(map) => map,
        _ => unreachable!(),
    }
}

#[test]
fn test_v4_nightly_save_loads_with_current_defaults() {
    let profile = Profile::from_dict(&v4_nightly_payload());

    assert_eq!(profile.name, "Legacy Driver");
    assert_eq!(profile.money, 12_345.0);
    assert_eq!(profile.career.level(), 6);
    assert_eq!(profile.career.dispatch_declines_used, 0);
    // fields added since v4 default cleanly
    assert_eq!(profile.business_status, COMPANY_DRIVER);
    assert_eq!(profile.tire_wear_pct(), 0.0);
    assert_eq!(profile.road_grime_pct(), 0.0);
    assert!(profile.trailer_programs.is_empty());
    assert_eq!(profile.hos.driving_min, 0.0); // fresh clock, not a violation
    assert!(profile.duty_log.segments.is_empty());
    // The flat condition set fanned out onto the rig record, fuel included
    // (the company driver's ACTIVE tractor is whatever the fleet assigns).
    assert_eq!(profile.truck_conditions["rig"]["damage_pct"], 12.0);
    assert_eq!(profile.truck_conditions["rig"]["fuel_gal"], 90.0);
    assert!(profile.migration_notice_pending);
    assert!(profile.needs_migration_resave);
}

#[test]
fn test_v4_save_joins_the_dispatch_autonomy_bands_at_its_level() {
    let mut profile = Profile::from_dict(&v4_nightly_payload());

    let policy = dispatch_policy(&profile); // level 6 company driver: new-hire band
    assert!(policy.assigns_load);
    assert!(policy.assigns_route);

    profile.career.xp = 25_000.0; // level 8+
    assert!(!dispatch_policy(&profile).assigns_load);
}

#[test]
fn test_v4_save_round_trips_to_current_version() {
    super::tests::with_data_dir(|_| {
        let profile = Profile::from_dict(&v4_nightly_payload());
        let data = profile.to_dict();

        assert_eq!(data["version"], json!(SAVE_VERSION));
        let reloaded = Profile::from_dict(&data);
        assert_eq!(reloaded.career.deliveries, 18);
        assert_eq!(reloaded.market.day, 2);
    });
}

#[test]
fn test_newer_save_with_unknown_fields_still_loads() {
    let mut data = v4_nightly_payload();
    data["career"]["future_counter"] = json!(7);
    data["market"]["future_flag"] = json!(true);
    data.insert(
        "some_future_top_level_field".to_string(),
        json!({"nested": 1}),
    );

    let profile = Profile::from_dict(&data);

    assert_eq!(profile.career.deliveries, 18);
    assert_eq!(profile.market.seed, 1234);
}

#[test]
fn test_corrupt_nested_payload_types_fall_back_to_defaults() {
    let mut data = v4_nightly_payload();
    data.insert("career".to_string(), json!("not-a-dict"));
    data.insert("market".to_string(), json!(42));

    let profile = Profile::from_dict(&data);

    assert_eq!(profile.career.deliveries, 0);
    assert!(!profile.market.multipliers.is_empty());
}

#[test]
fn from_dict_coerces_malformed_scalars_and_keeps_unknown_records() {
    let value = json!({
        "version": SAVE_VERSION,
        "created_line": "1.9",
        "name": "Odd Types",
        "money": "1200.5",
        "calendar_offset_days": 3.0,
        "tutorial_done": 1,
        "owned_trucks": "rig",
        "truck_conditions": {"rig": {"fuel_gal": 80.0, "grime_pct": 0.0}, "mystery": {"fuel_gal": 1.0, "grime_pct": 0.0}, "junk": 5},
        "upgrades": {"engine_tune": 2, "aero_kit": "x"},
        "active_buffs": {"not": "a list"},
        "achievement_stats": [1, 2],
        "hos": 7,
        "loyalty": null,
        "driving_record": null,
    });
    let Value::Object(map) = value else {
        unreachable!()
    };
    let profile = Profile::from_dict(&map);
    assert_eq!(profile.money, 1200.5);
    assert_eq!(profile.calendar_offset_days, 3);
    assert!(profile.tutorial_done);
    assert!(profile.owned_trucks.is_empty());
    assert_eq!(profile.truck_conditions.len(), 2);
    assert!(profile.truck_conditions.contains_key("mystery"));
    assert_eq!(profile.upgrades.get("engine_tune"), Some(&2));
    assert!(!profile.upgrades.contains_key("aero_kit"));
    assert!(profile.active_buffs.is_empty());
    assert!(profile.achievement_stats.is_empty());
    assert_eq!(profile.hos, crate::sim::hos::HosClock::new());
    assert_eq!(profile.loyalty.total_points, 0.0);
    assert_eq!(profile.driving_record.citations, 0);
    assert!(!profile.needs_migration_resave);
}
