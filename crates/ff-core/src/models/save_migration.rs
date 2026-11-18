//! Save-format migrations: upgrade older save dicts to the current shape
//! (port of `freight_fate/models/save_migration.py`).
//!
//! `Profile::from_dict` runs [`migrate_save_data`] on every raw save dict
//! before parsing, so all entry points -- disk loads, cloud restores -- see the
//! current schema. Each migration works on the plain dict, never on a Profile
//! instance, and must tolerate missing or malformed fields: an old save that
//! survived loading before must keep loading after.

use serde_json::{Map, Value};

/// Flat per-profile condition fields written by save versions 4 and earlier,
/// before each owned truck kept its own record.
pub const LEGACY_TRUCK_FIELDS: [&str; 4] = [
    "truck_fuel_gal",
    "truck_damage_pct",
    "tire_wear_pct",
    "road_grime_pct",
];

/// `float(value or default)` over a raw save field: numbers, bools and
/// numeric strings coerce; anything else (missing, null, an object) is the
/// default. Shared by the save-reading models (`loyalty`, `enforcement`).
pub(crate) fn json_f64(value: Option<&Value>, default: f64) -> f64 {
    match value {
        Some(Value::Number(n)) => n.as_f64().unwrap_or(default),
        Some(Value::Bool(b)) => {
            if *b {
                1.0
            } else {
                0.0
            }
        }
        Some(Value::String(s)) => s.trim().parse().unwrap_or(default),
        _ => default,
    }
}

/// `int(value or default)` over a raw save field, truncating a float the way
/// `int()` does.
pub(crate) fn json_i64(value: Option<&Value>, default: i64) -> i64 {
    match value {
        Some(Value::Number(n)) => n
            .as_i64()
            .or_else(|| n.as_f64().map(|f| f.trunc() as i64))
            .unwrap_or(default),
        Some(Value::Bool(b)) => i64::from(*b),
        Some(Value::String(s)) => s.trim().parse().unwrap_or(default),
        _ => default,
    }
}

/// A JSON number that Python would have read as an `int` (not a float, not
/// a bool).
fn int_version(value: Option<&Value>) -> Option<i64> {
    match value {
        Some(Value::Number(n)) if n.is_i64() || n.is_u64() => n.as_i64(),
        _ => None,
    }
}

/// Return `(data upgraded to the current shape, whether anything changed)`.
pub fn migrate_save_data(data: Map<String, Value>) -> (Map<String, Value>, bool) {
    if let Some(version) = int_version(data.get("version")) {
        if version >= 5 {
            return (data, false);
        }
    }
    (migrate_to_per_truck_conditions(data), true)
}

/// Pre-per-truck save: flag the conversion, and leave the records to the
/// profile.
///
/// The fan-out itself lives in `profile._migrate_flat_conditions`, which is
/// the authority on a condition record's shape on this line -- it also carries
/// brake wear, engine wear and traction gear, which this module's older
/// four-field record knew nothing about. Building the records here would
/// satisfy the profile's "already migrated?" check with a record missing those
/// fields, and the wear would be lost on every legacy save. The flat fields are
/// deliberately left in place for that fan-out to read.
fn migrate_to_per_truck_conditions(mut data: Map<String, Value>) -> Map<String, Value> {
    data.insert("migration_notice_pending".to_string(), Value::Bool(true));
    data
}

#[cfg(test)]
mod tests {
    //! Ported from the pure cases of `tests/test_save_migration.py`; the rest
    //! of that file drives Profile.load, the truck shop and the notice
    //! screens.

    use super::*;
    use serde_json::json;

    fn map(value: Value) -> Map<String, Value> {
        value.as_object().expect("an object").clone()
    }

    #[test]
    fn test_migration_respects_long_range_tank_upgrade() {
        // The save_migration half: a version-4 save is flagged for conversion
        // and its flat fields are left for the profile's fan-out to read. The
        // fan-out assertions themselves need models::profile and trucks.
        let data = map(json!({
            "version": 4,
            "truck": "heavy_hauler",
            "business_status": "leased_owner_operator",
            "owned_trucks": ["rig", "heavy_hauler"],
            "upgrades": {"long_range_tank": 1},
            "truck_fuel_gal": 240.0,
        }));
        let (migrated, changed) = migrate_save_data(data);
        assert!(changed);
        assert_eq!(migrated["migration_notice_pending"], Value::Bool(true));
        assert_eq!(migrated["truck_fuel_gal"], 240.0);
        assert_eq!(migrated["version"], 4);
        // Nothing is stripped here; the profile reads and removes the flat
        // fields itself.
        assert!(LEGACY_TRUCK_FIELDS.contains(&"truck_fuel_gal"));
        assert!(migrated.contains_key("truck_fuel_gal"));
    }

    #[test]
    fn test_current_saves_pass_through_unchanged() {
        let data = map(json!({"version": 11, "name": "Modern"}));
        let (migrated, changed) = migrate_save_data(data.clone());
        assert!(!changed);
        assert_eq!(migrated, data);
        let (five, changed) = migrate_save_data(map(json!({"version": 5})));
        assert!(!changed);
        assert!(!five.contains_key("migration_notice_pending"));
    }

    #[test]
    fn a_missing_or_non_integer_version_migrates() {
        // `isinstance(version, int)`: a float, a string, a bool or nothing at
        // all is not a current save.
        for version in [json!(null), json!("11"), json!(11.0), json!(true)] {
            let mut data = Map::new();
            if !version.is_null() {
                data.insert("version".to_string(), version.clone());
            }
            let (migrated, changed) = migrate_save_data(data);
            assert!(changed, "{version:?}");
            assert_eq!(migrated["migration_notice_pending"], Value::Bool(true));
        }
    }

    use crate::models::business_constants::LEASED_OWNER_OPERATOR;
    use crate::models::profile::{
        decode_save_bytes, signature_for, tests::with_data_dir, Profile, SAVE_VERSION,
    };
    use crate::profile_invariants::check_profile_invariants;
    use crate::sim::vehicle::TruckSpecs;
    use std::path::{Path, PathBuf};

    /// Write a save with flat condition fields, as builds before version 11 did.
    ///
    /// The default version 10 is a 1.9-line save from before per-truck
    /// condition records (a real tester career shape); these must keep
    /// migrating. Version 4 and earlier is the 1.8 line, which the load gate
    /// refuses. Written as plain JSON at the legacy `.json` path, the oldest
    /// container shape every one of these builds could read.
    fn write_flat_condition_save(
        truck: &str,
        owned: &[&str],
        fuel: f64,
        damage: f64,
        tires: f64,
        grime: f64,
        signed: bool,
    ) -> PathBuf {
        let p = Profile::named("Legacy");
        let packed_path = p.save().unwrap();
        let (mut data, _) = decode_save_bytes(&std::fs::read(&packed_path).unwrap()).unwrap();
        std::fs::remove_file(&packed_path).unwrap();
        for key in [
            "truck_conditions",
            "migration_notice_pending",
            "integrity_modified",
            "integrity_notice_pending",
            "created_line", // the marker postdates every flat-field build
            "_signature",
            "_signature_version",
        ] {
            data.remove(key);
        }
        data.insert("version".into(), json!(10));
        // The fan-out only treats `truck` as the driven tractor for an
        // owner-operator; a company driver runs whatever the carrier assigned.
        data.insert("business_status".into(), json!(LEASED_OWNER_OPERATOR));
        data.insert("truck".into(), json!(truck));
        data.insert("owned_trucks".into(), json!(owned));
        data.insert("truck_fuel_gal".into(), json!(fuel));
        data.insert("truck_damage_pct".into(), json!(damage));
        data.insert("tire_wear_pct".into(), json!(tires));
        data.insert("road_grime_pct".into(), json!(grime));
        if signed {
            data.insert("_signature_version".into(), json!(1));
            let signature = signature_for(&data, None);
            data.insert("_signature".into(), json!(signature));
        }
        let path = packed_path.with_extension("json");
        std::fs::write(&path, serde_json::to_string(&Value::Object(data)).unwrap()).unwrap();
        path
    }

    fn default_flat_save() -> PathBuf {
        write_flat_condition_save(
            "heavy_hauler",
            &["rig", "heavy_hauler"],
            120.0,
            40.0,
            10.0,
            60.0,
            true,
        )
    }

    fn profile_value(profile: &Profile) -> Value {
        Value::Object(profile.to_dict())
    }

    fn invalid_twin(path: &Path) -> PathBuf {
        PathBuf::from(format!("{}.invalid", path.display()))
    }

    #[test]
    fn test_pre_condition_1_9_save_converts_to_per_truck_records() {
        with_data_dir(|_| {
            let path = default_flat_save();
            let loaded = Profile::load(&path).unwrap();

            // Condition records are plain dicts on this line.
            let hauler = &loaded.truck_conditions["heavy_hauler"];
            assert_eq!(hauler["fuel_gal"], 120.0);
            assert_eq!(hauler["damage_pct"], 40.0);
            assert_eq!(hauler["tire_wear_pct"], 10.0);

            let rig = &loaded.truck_conditions["rig"];
            assert_eq!(rig["fuel_gal"], TruckSpecs::default().fuel_tank_gal);
            // Parked trucks inherit the one saved wear and damage set rather than
            // starting pristine -- a swap must not launder a beaten-up career.
            assert_eq!(rig["damage_pct"], 40.0);
            assert_eq!(rig["tire_wear_pct"], 10.0);

            // Road grime rides the active truck's record.
            assert_eq!(loaded.road_grime_pct(), 60.0);

            // The one-time conversion notice belongs to the 1.8-and-earlier format;
            // a 1.9-line save just converts quietly.
            assert!(!loaded.migration_notice_pending);
            assert!(check_profile_invariants(&profile_value(&loaded)).is_empty());
        });
    }

    #[test]
    fn test_pre_condition_1_9_save_is_rewritten_to_disk_on_load() {
        with_data_dir(|_| {
            let path = default_flat_save();
            let loaded = Profile::load(&path).unwrap();
            // The conversion re-homes the career in the packed container; the old
            // plain-JSON file stays behind only as a .json.bak rollback copy.
            assert!(!path.exists());
            let path = loaded.path();
            let (on_disk, _) = decode_save_bytes(&std::fs::read(&path).unwrap()).unwrap();
            assert_eq!(on_disk["version"], json!(SAVE_VERSION));
            assert!(on_disk.contains_key("truck_conditions"));
            for legacy in LEGACY_TRUCK_FIELDS {
                assert!(!on_disk.contains_key(legacy), "{legacy}");
            }
            // Grime rides on the truck that got dirty, like every other kind of
            // wear, so the migrated figure lands in the records rather than on
            // the profile.
            assert_eq!(
                on_disk["truck_conditions"]["heavy_hauler"]["grime_pct"],
                60.0
            );
            // The rewrite also stamps the created-on marker, so this career never
            // needs the save-version backfill test again.
            assert_eq!(on_disk["created_line"], "2.0");
            // The rewritten save loads cleanly, is validly signed, and migrates no more.
            let again = Profile::load(&path).unwrap();
            assert!(!again.needs_migration_resave);
            assert_eq!(again.truck_conditions["heavy_hauler"]["fuel_gal"], 120.0);
        });
    }

    #[test]
    fn test_signed_flat_condition_save_is_not_quarantined() {
        with_data_dir(|_| {
            let path = default_flat_save();
            let loaded = Profile::load(&path).unwrap(); // a signature mismatch would mark
            assert_eq!(loaded.name, "Legacy");
            assert!(!loaded.integrity_modified);
            assert!(!invalid_twin(&path).exists());
        });
    }

    #[test]
    fn test_migration_clamps_impossible_legacy_values() {
        with_data_dir(|_| {
            let path = write_flat_condition_save("rig", &["rig"], 9_000.0, 250.0, -5.0, 60.0, true);
            let loaded = Profile::load(&path).unwrap();
            let rig = &loaded.truck_conditions["rig"];
            assert_eq!(rig["fuel_gal"], TruckSpecs::default().fuel_tank_gal);
            assert_eq!(rig["damage_pct"], 100.0);
            assert_eq!(rig["tire_wear_pct"], 0.0);
        });
    }

    // `test_migration_notice_shows_once_then_enters_world` is live in `crates/freight-fate/tests/states_main_menu.rs`.

    // `test_migration_notice_escape_also_acknowledges` is live in `crates/freight-fate/tests/states_main_menu.rs`.

    // `test_modified_notice_shows_once_then_enters_world` is live in `crates/freight-fate/tests/states_main_menu.rs`.

    // `test_bought_truck_starts_fresh_and_each_keeps_its_own_condition` is live in `crates/freight-fate/tests/states_city_shops.rs`.

    #[test]
    fn test_invariants_flag_bad_per_truck_records() {
        with_data_dir(|_| {
            let mut p = Profile::named("Bad Fleet");
            p.owned_trucks = vec!["rig".into(), "heavy_hauler".into()];
            p.provision_truck_condition("heavy_hauler", None);
            p.provision_truck_condition("rig", None);
            p.truck_conditions["heavy_hauler"].insert("fuel_gal".into(), json!(9_000.0));
            p.truck_conditions["rig"].insert("damage_pct".into(), json!(-5.0));
            let violations = check_profile_invariants(&profile_value(&p));
            assert!(violations.iter().any(|v| v.code == "fuel_range"));
            // Wear and damage share one code here; the detail names the bad meter.
            assert!(violations
                .iter()
                .any(|v| v.code == "condition_range" && v.detail.contains("damage")));
        });
    }

    #[test]
    fn test_condition_round_trips_through_save_and_load() {
        with_data_dir(|_| {
            let mut p = Profile::named("Round Trip");
            p.truck = "rig".into();
            p.set_truck_fuel_gal(77.0);
            p.set_truck_damage_pct(3.5);
            let path = p.save().unwrap();
            let loaded = Profile::load(&path).unwrap();
            assert_eq!(loaded.truck_fuel_gal(), 77.0);
            assert_eq!(loaded.truck_damage_pct(), 3.5);
            assert!(!loaded.migration_notice_pending);
            assert!(check_profile_invariants(&profile_value(&loaded)).is_empty());
        });
    }

    #[test]
    fn test_unknown_truck_key_condition_is_preserved() {
        with_data_dir(|_| {
            let mut p = Profile::named("Future Fleet");
            p.owned_trucks = vec!["rig".into(), "hover_truck".into()];
            p.provision_truck_condition("hover_truck", None);
            p.truck_conditions["hover_truck"].insert("damage_pct".into(), json!(12.0));
            let path = p.save().unwrap();
            let loaded = Profile::load(&path).unwrap();
            assert_eq!(loaded.truck_conditions["hover_truck"]["damage_pct"], 12.0);
            assert!(check_profile_invariants(&profile_value(&loaded)).is_empty());
        });
    }
}
