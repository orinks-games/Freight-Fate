//! Ported from `tests/test_trucks.py`: truck catalog, garage upgrades, and
//! their effects on the physics model. The profile persistence cases need
//! `models::profile` and are ignored with the reason; the garage-state case
//! needs the app shell.

use super::*;
use crate::sim::vehicle::{TruckSpecs, TruckState};

fn drive(truck: &mut TruckState, seconds: f64) {
    let dt = 1.0 / 60.0;
    for _ in 0..((seconds / dt) as i64) {
        truck.auto_shift();
        truck.update(dt);
    }
}

fn make_auto_truck(specs: TruckSpecs) -> TruckState {
    let mut t = TruckState::new(specs);
    t.transmission.automatic = true;
    t.start_engine();
    t
}

fn upgrades(pairs: &[(&'static str, i64)]) -> HashMap<String, i64> {
    pairs.iter().map(|(k, v)| (k.to_string(), *v)).collect()
}

// -- spec building -----------------------------------------------------------------

#[test]
fn test_no_upgrades_returns_base_specs() {
    assert_eq!(
        build_truck_specs("rig", &NO_UPGRADES),
        TruckSpecs::default()
    );
}

#[test]
fn test_unknown_truck_falls_back_to_rig() {
    assert_eq!(
        build_truck_specs("hover_truck", &NO_UPGRADES),
        TruckSpecs::default()
    );
}

#[test]
fn test_engine_tune_adds_ten_percent_torque_per_tier() {
    let base = TruckSpecs::default();
    let t1 = build_truck_specs("rig", &[("engine_tune", 1)]);
    let t2 = build_truck_specs("rig", &[("engine_tune", 2)]);
    assert!((t1.max_torque_nm - base.max_torque_nm * 1.1).abs() < 1e-6);
    assert!((t2.max_torque_nm - base.max_torque_nm * 1.2).abs() < 1e-6);
}

#[test]
fn test_aero_kit_cuts_drag_twelve_percent() {
    let base = TruckSpecs::default();
    let s = build_truck_specs("rig", &[("aero_kit", 1)]);
    assert!((s.drag_coefficient - base.drag_coefficient * 0.88).abs() < 1e-9);
}

#[test]
fn test_long_range_tank_adds_fifty_gallons() {
    let s = build_truck_specs("rig", &[("long_range_tank", 1)]);
    assert_eq!(s.fuel_tank_gal, TruckSpecs::default().fuel_tank_gal + 50.0);
}

#[test]
fn test_reinforced_brakes_raise_fade_threshold() {
    let s = build_truck_specs("rig", &[("reinforced_brakes", 1)]);
    assert!(s.brake_fade_temp_c > TruckSpecs::default().brake_fade_temp_c);
}

#[test]
fn test_upgrades_stack() {
    let s = build_truck_specs(
        "rig",
        &upgrades(&[
            ("engine_tune", 2),
            ("aero_kit", 1),
            ("long_range_tank", 1),
            ("reinforced_brakes", 1),
        ]),
    );
    let base = TruckSpecs::default();
    assert!(s.max_torque_nm > base.max_torque_nm);
    assert!(s.drag_coefficient < base.drag_coefficient);
    assert!(s.fuel_tank_gal > base.fuel_tank_gal);
    assert!(s.brake_fade_temp_c > base.brake_fade_temp_c);
}

#[test]
fn test_heavy_hauler_tradeoffs() {
    let rig = &truck_model_or_panic("rig").specs;
    let hauler = &truck_model_or_panic("heavy_hauler").specs;
    assert!(hauler.max_torque_nm > rig.max_torque_nm);
    assert!(hauler.fuel_tank_gal > rig.fuel_tank_gal);
    assert!(hauler.drag_coefficient > rig.drag_coefficient);
    assert!(hauler.fuel_burn_factor > rig.fuel_burn_factor);
}

#[test]
fn test_truck_descriptions_explain_tradeoffs() {
    let rig = truck_model_or_panic("rig").description;
    let hauler = truck_model_or_panic("heavy_hauler").description;
    assert!(rig.contains("fuel economy"));
    assert!(hauler.contains("heavy loads"));
    assert!(hauler.contains("thirstier engine"));
}

#[test]
fn test_heavy_hauler_upgrades_apply_on_top() {
    let s = build_truck_specs("heavy_hauler", &[("long_range_tank", 1)]);
    assert_eq!(
        s.fuel_tank_gal,
        truck_model_or_panic("heavy_hauler").specs.fuel_tank_gal + 50.0
    );
}

#[test]
#[ignore = "wrong crate: ff-core cannot see the game crate, so this case belongs in crates/freight-fate/tests/ -- needs freight_fate::app (App, TruckShopState, UpgradeShopState)"]
fn test_garage_says_upgrades_are_fleet_wide() {
    // UpgradeShopState.intro_help says "apply to every tractor";
    // TruckShopState.intro_help says "fleet upgrades apply" and its current
    // text reads "thousand newton meters torque" and "gallon tank".
    unimplemented!("needs the app shell")
}

// -- physics effects ---------------------------------------------------------------

#[test]
fn test_engine_tune_accelerates_faster() {
    let mut stock = make_auto_truck(build_truck_specs("rig", &NO_UPGRADES));
    let mut tuned = make_auto_truck(build_truck_specs("rig", &[("engine_tune", 2)]));
    for t in [&mut stock, &mut tuned] {
        t.throttle = 1.0;
        drive(t, 45.0);
    }
    assert!(tuned.velocity_mps > stock.velocity_mps);
}

#[test]
fn test_aero_kit_raises_cruise_speed() {
    let mut stock = make_auto_truck(build_truck_specs("rig", &NO_UPGRADES));
    let mut sleek = make_auto_truck(build_truck_specs("rig", &[("aero_kit", 1)]));
    for t in [&mut stock, &mut sleek] {
        t.throttle = 1.0;
        drive(t, 120.0);
    }
    assert!(sleek.velocity_mps > stock.velocity_mps);
}

#[test]
fn test_reinforced_brakes_resist_fade_when_hot() {
    let mut stock = TruckState::default();
    let mut upgraded = TruckState::new(build_truck_specs("rig", &[("reinforced_brakes", 1)]));
    for t in [&mut stock, &mut upgraded] {
        t.velocity_mps = 25.0;
        t.brake = 1.0;
        t.brake_temp_c = 480.0; // past stock fade onset, below upgraded onset
    }
    assert!(upgraded.brake_force() > stock.brake_force());
}

#[test]
fn test_heavy_hauler_burns_more_fuel() {
    let mut rig = make_auto_truck(build_truck_specs("rig", &NO_UPGRADES));
    let mut hauler = make_auto_truck(build_truck_specs("heavy_hauler", &NO_UPGRADES));
    for t in [&mut rig, &mut hauler] {
        t.fuel_gal = 50.0;
        t.velocity_mps = 25.0;
        t.throttle = 0.0; // idle burn isolates the model's thirst factor
        t.update_fuel(60.0);
    }
    assert!(hauler.fuel_gal < rig.fuel_gal);
}

// -- profile persistence -----------------------------------------------------------

#[test]
fn test_profile_persists_truck_and_upgrades() {
    use crate::models::business_constants::LEASED_OWNER_OPERATOR;
    use crate::models::profile::{tests::with_data_dir, Profile};
    with_data_dir(|_| {
        let mut p = Profile::named("Garage Test");
        p.business_status = LEASED_OWNER_OPERATOR.to_string();
        p.truck = "heavy_hauler".to_string();
        p.owned_trucks = vec!["rig".to_string(), "heavy_hauler".to_string()];
        p.upgrades = [("engine_tune".to_string(), 2), ("aero_kit".to_string(), 1)]
            .into_iter()
            .collect();
        let path = p.save().unwrap();
        let loaded = Profile::load(&path).unwrap();
        assert_eq!(loaded.truck, "heavy_hauler");
        assert_eq!(loaded.owned_trucks, vec!["rig", "heavy_hauler"]);
        assert_eq!(loaded.upgrades, p.upgrades);
        let specs = loaded.truck_specs();
        let hauler = &truck_model_or_panic("heavy_hauler").specs;
        assert!((specs.max_torque_nm - hauler.max_torque_nm * 1.2).abs() < 1e-6);
    });
}

#[test]
fn test_old_save_without_truck_fields_loads_with_defaults() {
    use crate::models::profile::{
        decode_save_bytes, tests::with_data_dir, Profile, SIGNATURE_FIELD, SIGNATURE_VERSION_FIELD,
    };
    with_data_dir(|_| {
        let p = Profile::named("Legacy");
        let path = p.save().unwrap();
        let (mut data, _) = decode_save_bytes(&std::fs::read(&path).unwrap()).unwrap();
        std::fs::remove_file(&path).unwrap();
        for legacy_missing in [
            "truck",
            "owned_trucks",
            "upgrades",
            "market",
            "trailer_programs",
            "owned_trailers",
            "tire_wear_pct",
            "brake_wear_pct",
            "engine_wear_pct",
            "road_grime_pct",
            "active_buffs",
            SIGNATURE_FIELD,
            SIGNATURE_VERSION_FIELD,
        ] {
            data.remove(legacy_missing);
        }
        // An old install left this save as plain unsigned JSON.
        let legacy_path = path.with_extension("json");
        std::fs::write(
            &legacy_path,
            serde_json::to_string(&serde_json::Value::Object(data)).unwrap(),
        )
        .unwrap();
        let loaded = Profile::load(&legacy_path).unwrap();
        assert_eq!(loaded.truck, "rig");
        assert!(loaded.owned_trucks.is_empty());
        assert!(loaded.visible_owned_trucks().is_empty());
        assert!(loaded.upgrades.is_empty());
        assert!(loaded.active_trailer_programs().is_empty());
        assert!(loaded.visible_owned_trailers().is_empty());
        assert_eq!(loaded.tire_wear_pct(), 0.0);
        assert_eq!(loaded.brake_wear_pct(), 0.0);
        assert_eq!(loaded.engine_wear_pct(), 0.0);
        assert_eq!(loaded.road_grime_pct(), 0.0);
        assert!(loaded.active_buffs.is_empty());
        // Per-truck condition is reached through the flat names on this line;
        // they route to the active truck's record rather than a typed accessor.
        assert_eq!(loaded.truck_damage_pct(), 0.0);
        assert_eq!(
            loaded
                .truck_conditions
                .get("rig")
                .and_then(|r| r.get("damage_pct"))
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0),
            0.0
        );
        assert!(!loaded.market.multipliers.is_empty()); // fresh market seeded on load
    });
}

#[test]
fn test_truck_condition_round_trips_through_profile() {
    use crate::models::profile::Profile;
    let mut p = Profile::named("Wear Sync");
    p.set_truck_fuel_gal(120.0);
    p.set_truck_damage_pct(8.0);
    p.set_tire_wear_pct(12.0);
    p.set_brake_wear_pct(34.0);
    p.set_engine_wear_pct(5.5);
    let mut truck = TruckState::default();
    p.load_truck_condition(&mut truck);
    assert_eq!(truck.fuel_gal, 120.0);
    assert_eq!(truck.damage_pct, 8.0);
    assert_eq!(truck.tire_wear_pct, 12.0);
    assert_eq!(truck.brake_wear_pct, 34.0);
    assert_eq!(truck.engine_wear_pct, 5.5);

    truck.tire_wear_pct += 1.5;
    truck.brake_wear_pct += 2.0;
    truck.engine_wear_pct += 0.25;
    truck.fuel_gal -= 30.0;
    p.store_truck_condition(&truck);
    assert_eq!(p.truck_fuel_gal(), 90.0);
    assert_eq!(p.tire_wear_pct(), 13.5);
    assert_eq!(p.brake_wear_pct(), 36.0);
    assert_eq!(p.engine_wear_pct(), 5.75);
}

#[test]
fn test_company_driver_profile_uses_assigned_standard_tractor() {
    use crate::models::profile::Profile;
    let mut p = Profile::named("Assigned Rig");
    p.truck = "heavy_hauler".to_string();
    p.owned_trucks = vec!["rig".to_string(), "heavy_hauler".to_string()];
    p.upgrades = [
        ("engine_tune".to_string(), 2),
        ("long_range_tank".to_string(), 1),
    ]
    .into_iter()
    .collect();

    let specs = p.truck_specs();

    assert!(p.visible_owned_trucks().is_empty());
    assert_eq!(p.active_truck_key(), "rig");
    assert_eq!(specs, TruckSpecs::default());
}

#[test]
fn test_upgrade_catalog_prices_and_tiers() {
    assert_eq!(upgrade("engine_tune").unwrap().max_tier(), 2);
    for upgrade in UPGRADE_CATALOG {
        assert!(upgrade.max_tier() >= 1);
        assert!(upgrade.prices.iter().all(|price| *price > 0.0));
        assert!(!upgrade.description.is_empty());
    }
}

// -- catalogue shape ---------------------------------------------------------------

#[test]
fn test_catalog_keeps_the_python_order_and_keys() {
    let keys: Vec<&str> = TRUCK_CATALOG.keys().copied().collect();
    assert_eq!(keys.len(), 35);
    assert_eq!(keys[0], "rig");
    assert_eq!(keys[1], "heavy_hauler");
    assert_eq!(keys[2], "trainer_day_cab");
    assert_eq!(keys[34], "night_flag_aero");
    for model in TRUCK_CATALOG.values() {
        assert!(matches!(model.cab, CAB_DAY | CAB_SLEEPER));
        assert!(matches!(
            model.spec,
            SPEC_LIGHT | SPEC_STANDARD | SPEC_HEAVY
        ));
        assert!(!model.label.contains('_'));
    }
    assert_eq!(truck_model("rig").unwrap().label, "standard rig");
    assert!(truck_model("hover_truck").is_none());
}

#[test]
fn test_truck_condition_defaults_fresh_and_from_dict() {
    let fresh = TruckCondition::fresh("heavy_hauler", &NO_UPGRADES);
    assert_eq!(fresh.fuel_gal, 200.0);
    assert_eq!(fresh.damage_pct, 0.0);
    let with_tank = TruckCondition::fresh("rig", &[("long_range_tank", 1)]);
    assert_eq!(with_tank.fuel_gal, 200.0);
    assert_eq!(
        TruckCondition::from_dict(&serde_json::json!(null)),
        TruckCondition::default()
    );
    let partial = TruckCondition::from_dict(&serde_json::json!({"fuel_gal": 40, "unknown": 3}));
    assert_eq!(partial.fuel_gal, 40.0);
    assert_eq!(partial.grime_pct, 0.0);
    // Field order is the dataclass order (serde_json::Value sorts keys, so
    // read it off the serialized text).
    let text = serde_json::to_string(&partial).unwrap();
    let keys: Vec<&str> = text.split('"').skip(1).step_by(2).collect();
    assert_eq!(
        keys,
        ["fuel_gal", "damage_pct", "tire_wear_pct", "grime_pct"]
    );
}

#[test]
fn rated_mpg_orders_the_heavy_hauler_rig_and_aero_flagship() {
    let mpg = |key: &str| rated_mpg(truck_model(key).expect("catalog tractor"));
    let rig = mpg("rig");
    let heavy = mpg("heavy_hauler");
    let aero = mpg("night_flag_aero");
    assert!((6.0..=7.5).contains(&rig), "standard rig {rig}");
    assert!(
        heavy < rig && rig < aero,
        "heavy {heavy}, rig {rig}, aero {aero}"
    );
}

#[test]
fn turbo_eligibility_leaves_unknowns_and_off_models_silent_even_with_tune() {
    assert_eq!(turbo_level_for_model("unknown_future_tractor"), 0.0);
    for off_key in ["trainer_day_cab", "city_shuttle", "hand_me_down_sleeper"] {
        assert_eq!(turbo_level_for_model(off_key), 0.0);
        assert_eq!(
            build_truck_specs(off_key, &[("engine_tune", 2)]).turbo_level,
            0.0,
            "{off_key} must stay silent even with engine_tune tier 2"
        );
    }
    let mut enabled_count = 0usize;
    for model in TRUCK_CATALOG.values() {
        if model.specs.turbo_level > 0.0 {
            enabled_count += 1;
            assert!(
                model.specs.max_torque_nm >= 2_300.0,
                "{} enabled with unexpectedly low torque {}",
                model.key,
                model.specs.max_torque_nm
            );
        }
    }
    assert_eq!(enabled_count, 32);
}
