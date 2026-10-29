use super::*;

/// A snapshot of the model constants as of 2026-08-22 (the Python
/// `invariant_data()` output), standing in until the models land.
fn inputs() -> CatalogInputs {
    CatalogInputs {
        achievement_ids: vec!["first_delivery".into(), "antler_polisher".into()],
        achievement_labels: vec![("first_delivery".into(), "First delivery".into())],
        achievement_details: vec![(
            "first_delivery".into(),
            "road".into(),
            "Your first load, signed for and gone.".into(),
        )],
        achievement_categories: vec![("road".into(), "Out on the Road".into())],
        career_titles: vec!["Yard Trainee".into()],
        company_career_titles: vec!["Yard Trainee".into()],
        carrier_labels: vec![("northstar".into(), "Northstar Freight Lines".into())],
        carriers: vec![],
        reputation_bonus_max_share: 0.06,
        assigned_reposition_pay_fraction: 0.6,
        trailers: vec![TrailerRow {
            key: "dry_van".into(),
            label: "Dry van".into(),
            purchase_price: 42_000.0,
        }],
        starting_money: 5000.0,
        starting_money_max: 18000.0,
        pay_advance_limit: 1500.0,
        xp_per_mile_on_time: 1.6,
        xp_specialty_mult: 1.5,
        xp_streak_max_bonus: 0.45,
        xp_clean_bonus: 0.15,
        delivery_completion_xp: 150.0,
        level_xp: vec![0, 1000, 2500, 4500, 7000],
        market_cargo_keys: vec!["retail".into(), "general".into()],
        profile_fields: vec![
            "name".into(),
            "money".into(),
            "created_line".into(),
            "business_status".into(),
            "_signature".into(),
            "_signature_version".into(),
        ],
        career_fields: vec![
            "xp".into(),
            "deliveries".into(),
            "purchased_endorsements".into(),
        ],
        endorsements: vec![
            EndorsementRow {
                key: "refrigerated".into(),
                level: Some(2),
                label: "refrigerated".into(),
                tier: "certificate".into(),
            },
            EndorsementRow {
                key: "heavy_haul".into(),
                level: Some(3),
                label: "heavy-haul".into(),
                tier: "certificate".into(),
            },
            EndorsementRow {
                key: "hazmat".into(),
                level: None,
                label: "hazmat".into(),
                tier: "endorsement".into(),
            },
        ],
        fleet_tiers: vec![
            FleetTierRow {
                min_level: 1,
                label: "yard standard".into(),
            },
            FleetTierRow {
                min_level: 4,
                label: "regional fleet".into(),
            },
        ],
        truck_condition_fields: vec![
            "tire_wear_pct".into(),
            "brake_wear_pct".into(),
            "chain_wear_pct".into(),
            "chains_owned".into(),
            "damage_pct".into(),
            "engine_wear_pct".into(),
            "fuel_gal".into(),
            "grime_pct".into(),
            "tire_type".into(),
        ],
        source_save_version: 11,
        trucks: vec![("rig".into(), "standard rig".into(), 80_000.0)],
        upgrade_prices: vec![("engine_tune".into(), vec![12_000.0, 26_000.0])],
    }
}

#[test]
fn test_integrity_invariants_include_public_projection_labels() {
    let data = current_invariant_data().unwrap();
    assert!(data["sourceSaveVersion"].as_i64().unwrap() >= 1);
    assert_eq!(data["cityLabels"]["new_york_ny_us"], "New York, New York");
    assert_eq!(data["truckLabels"]["rig"], "standard rig");
    assert_eq!(data["levelXp"][0], 0);
    assert!(current_rendered_invariants().unwrap().ends_with('\n'));
}

/// The exported ceiling must sit at or above what record_delivery awards.
///
/// The server rejects a cloud backup whose XP exceeds
/// `deliveries * xpFlatPerDelivery + total_miles * xpPerMileMax`. If a
/// balance pass raises the game's rates past the exported figures, that
/// check starts convicting the drivers who played best -- which is exactly
/// how a hardcoded 1.2 per mile came to sit below the on-time rate. Drive
/// a spread of careers through the real award path and hold the line.
#[test]
fn test_exported_xp_ceiling_bounds_every_honest_career() {
    use crate::models::career::{Career, XP_PREMIUM_MULT, XP_SPECIALTY_MULT};
    use crate::pyrandom::PyRandom;

    let data = current_invariant_data().unwrap();
    let per_mile = data["xpPerMileMax"].as_f64().unwrap();
    let flat = data["xpFlatPerDelivery"].as_f64().unwrap();
    let mut rng = PyRandom::new_from_i64(7);

    for _ in 0..2_000 {
        let mut career = Career::default();
        let deliveries = rng.randint(1, 40);
        for _ in 0..deliveries {
            let miles = rng.uniform(1.0, 900.0);
            let on_time = rng.random() < 0.9;
            let damage_pct = *rng.choice(&[0.0, 0.5, 30.0]);
            let cargo_class_mult = *rng.choice(&[1.0, XP_PREMIUM_MULT, XP_SPECIALTY_MULT]);
            career.record_delivery(miles, 0.0, on_time, damage_pct, cargo_class_mult, 1.0);
        }
        let ceiling = career.deliveries as f64 * flat + career.total_miles * per_mile;
        assert!(
            career.xp <= ceiling,
            "{} XP over {} miles in {} deliveries breaches the exported ceiling {ceiling}",
            career.xp,
            career.total_miles,
            career.deliveries
        );
    }
}

/// The money rule's floor is this figure; a new profile must equal it.
#[test]
fn test_exported_starting_money_matches_a_fresh_career() {
    let data = current_invariant_data().unwrap();
    assert_eq!(
        data["startingMoney"].as_f64().unwrap(),
        crate::models::profile::Profile::new().money()
    );
}

/// The server's money ceiling credits the richest career start.
///
/// The owner-operator option opens with more cash than the company-driver
/// default; a ceiling built on the default rejected every honest
/// owner-operator backup as impossible_money until earnings outgrew the gap.
#[test]
fn test_exported_starting_money_max_covers_every_start_option() {
    use crate::models::start_options::all_start_options;

    let data = current_invariant_data().unwrap();
    let exported = data["startingMoneyMax"].as_f64().unwrap();
    let mut richest = f64::MIN;
    for option in all_start_options() {
        assert!(option.starting_money <= exported, "{}", option.key);
        richest = richest.max(option.starting_money);
    }
    assert_eq!(exported, richest);
}

/// The export must describe the record the game actually writes.
///
/// The server checks every truck_conditions record against this list and
/// rejects a save carrying a key it does not know. The list used to come
/// off the TruckCondition dataclass, which this line stopped using --
/// records are plain dicts, and they grew brake wear, engine wear and
/// traction gear while the dataclass kept four fields.
#[test]
fn test_exported_condition_fields_match_a_real_record() {
    let mut profile = crate::models::profile::Profile::new();
    profile.provision_truck_condition("rig", None);
    let mut written: Vec<String> = profile.truck_conditions["rig"].keys().cloned().collect();
    written.sort();
    let data = current_invariant_data().unwrap();
    let exported: Vec<String> = data["truckConditionFields"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    assert_eq!(exported, written);
}

#[test]
fn test_exported_profile_fields_include_the_created_on_marker() {
    // The 1.9 cutover regen must teach the server allow-list `created_line`.
    // Every 1.9 upload carries the marker, so a validator built from an
    // export without it would reject every honest backup on the schema check.
    let data = current_invariant_data().unwrap();
    let fields = data["profileFields"].as_array().unwrap();
    assert!(fields.contains(&Value::from("created_line")));
    // The local-only signature keys never ride the export.
    assert!(!fields.contains(&Value::from("_signature")));
    assert!(fields.contains(&Value::from("version")));
}

#[test]
fn test_exported_public_profile_fields_ride_the_allow_lists() {
    let data = current_invariant_data().unwrap();
    assert!(data["profileFields"]
        .as_array()
        .unwrap()
        .contains(&Value::from("business_status")));
    assert!(data["careerFields"]
        .as_array()
        .unwrap()
        .contains(&Value::from("purchased_endorsements")));
}

/// The site derives each driver's endorsements from this table.
///
/// Hold it to the real unlock path: a career levelled to each exported
/// threshold must hold exactly the endorsements the export promises at
/// that level, or the public profile starts crediting training the carrier
/// never sponsored (or hiding training it did).
#[test]
fn test_exported_endorsements_match_what_the_career_actually_unlocks() {
    use crate::models::career::{Career, LEVEL_XP};
    use std::collections::BTreeSet;

    let data = current_invariant_data().unwrap();
    let endorsements = data["endorsements"].as_object().unwrap();
    for level in 1..=LEVEL_XP.len() as i64 {
        let career = Career {
            xp: LEVEL_XP[level as usize - 1],
            ..Career::default()
        };
        assert_eq!(career.level(), level);
        // A row without a level is course-only: the site must never
        // level-derive it, and neither does the career.
        let expected: BTreeSet<&str> = endorsements
            .iter()
            .filter(|(_, entry)| {
                entry
                    .get("level")
                    .and_then(Value::as_i64)
                    .is_some_and(|lvl| level >= lvl)
            })
            .map(|(key, _)| key.as_str())
            .collect();
        let held: BTreeSet<&str> = career.endorsements().into_iter().collect();
        assert_eq!(held, expected, "level {level}");
    }

    // A self-paid course unlocks ahead of the sponsored level, which is
    // why purchased_endorsements has to reach the server at all.
    let early = Career {
        xp: 0.0,
        purchased_endorsements: vec!["heavy_haul".to_string()],
        ..Career::default()
    };
    assert!(early.endorsements().contains("heavy_haul"));
}

/// The site names a company driver's fleet tier from these bands.
#[test]
fn test_exported_fleet_tiers_match_the_carrier_fleet_bands() {
    use crate::models::carrier_fleet::fleet_tier_for_level;

    let data = current_invariant_data().unwrap();
    let tiers = data["fleetTiers"].as_array().unwrap();
    // every level maps to a band
    assert_eq!(tiers[0]["minLevel"].as_i64().unwrap(), 1);
    let min_levels: Vec<i64> = tiers
        .iter()
        .map(|t| t["minLevel"].as_i64().unwrap())
        .collect();
    let mut sorted = min_levels.clone();
    sorted.sort_unstable();
    assert_eq!(min_levels, sorted);
    for level in 1..31 {
        let expected = fleet_tier_for_level(level).label;
        let banded = tiers
            .iter()
            .rfind(|t| level >= t["minLevel"].as_i64().unwrap())
            .unwrap()["label"]
            .as_str()
            .unwrap();
        assert_eq!(banded, expected, "level {level}");
    }
}

/// The site links a profile's carrier to its page by name, so every carrier
/// a save can name -- the owner-operator start's included -- needs a page,
/// and each page's pay is the plan the settlement really pays from.
#[test]
fn test_every_carrier_a_save_can_name_has_a_carrier_page() {
    use crate::models::start_options::pay_plan_for_key;

    let data = current_invariant_data().unwrap();
    let carriers = data["carriers"].as_object().unwrap();
    let page_names: Vec<&str> = carriers
        .values()
        .map(|row| row["name"].as_str().unwrap())
        .collect();
    for (key, name) in data["carrierLabels"].as_object().unwrap() {
        assert!(page_names.contains(&name.as_str().unwrap()), "{key}");
    }
    for (key, row) in carriers {
        let plan = pay_plan_for_key(Some(key));
        assert_eq!(
            row["pay"]["payShare"].as_f64().unwrap(),
            plan.pay_share,
            "{key}"
        );
        assert_eq!(
            row["pay"]["minPerMile"].as_f64().unwrap(),
            plan.min_per_mile,
            "{key}"
        );
        assert_eq!(
            row["pay"]["stopPay"].as_f64().unwrap(),
            plan.stop_pay,
            "{key}"
        );
        assert_eq!(
            row["pay"]["onTimeBonusShare"].as_f64().unwrap(),
            plan.on_time_bonus_share,
            "{key}"
        );
    }
}

#[test]
fn the_xp_ceiling_terms_follow_the_python_arithmetic() {
    // Python invariant_data() on 2026-08-22: xpPerMileMax 4.002,
    // xpFlatPerDelivery 250.12499999999997. Against the live catalogs,
    // not the fixture: the point is that Rust's float arithmetic lands on
    // Python's exact repr, and a fixture that copies the same constants
    // would pass without ever asking the game.
    let data = current_invariant_data().unwrap();
    assert_eq!(data["xpPerMileMax"], 4.002);
    assert_eq!(data["xpFlatPerDelivery"], 250.12499999999997);
    assert_eq!(data["startingMoney"], 5000);
    assert_eq!(data["startingMoneyMax"], 18000);
}

#[test]
fn city_labels_drop_a_missing_state() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join("us")).unwrap();
    fs::write(
        dir.path().join("index.json"),
        r#"{"countries": [{"code": "US", "path": "us", "cities": "cities.json"}]}"#,
    )
    .unwrap();
    fs::write(
        dir.path().join("us/cities.json"),
        r#"{"cities": {"nowhere_us": {"spoken_city": "Nowhere"}, "austin_tx_us": {"spoken_city": "Austin", "state": "TX"}}}"#,
    )
    .unwrap();
    fs::write(
        dir.path().join("geo.json"),
        r#"{"countries": {"US": {"name": "United States", "states": {"TX": "Texas"}}}}"#,
    )
    .unwrap();
    let labels = city_labels(dir.path()).unwrap();
    assert_eq!(labels["nowhere_us"], "Nowhere");
    assert_eq!(labels["austin_tx_us"], "Austin, Texas");
}

#[test]
fn rendered_invariants_are_sorted_two_space_json() {
    let text = rendered_invariants(&world_data_root(), &inputs()).unwrap();
    assert!(text.starts_with(
        "{\n  \"achievementCategories\": [\n    {\n      \"key\": \"road\",\n      \"title\": \"Out on the Road\"\n    }\n  ],\n  \"achievementDetails\": {\n    \"first_delivery\": {\n      \"category\": \"road\",\n"
    ));
    assert!(text.contains("\n  \"achievementIds\": [\n    \"antler_polisher\",\n"));
    assert!(text.contains("\n  \"endorsements\": {\n    \"hazmat\": {\n      \"label\": \"hazmat\",\n      \"tier\": \"endorsement\"\n    },\n    \"heavy_haul\": {\n      \"label\": \"heavy-haul\",\n      \"level\": 3,\n      \"tier\": \"certificate\"\n    },"));
    assert!(text.ends_with("}\n"));
}
