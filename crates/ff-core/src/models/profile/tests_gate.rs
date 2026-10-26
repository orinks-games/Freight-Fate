//! Ported from `tests/test_legacy_career_gate.py` (the gate; the menu cases
//! are ignored) and `tests/test_version.py`.

use std::path::PathBuf;

use serde_json::{json, Map, Value};

use super::tests::{load, read_save, with_data_dir, with_suffix, write_packed, write_text};
use super::*;

// -- tests/test_legacy_career_gate.py -------------------------------------------

/// A save exactly as a current 1.8-line build leaves it.
///
/// dev and every stable release write packed, version-5, signature-v1 saves
/// that already carry per-truck condition records (the mainline migrated by
/// shape without bumping the save version) -- so the version field, not the
/// record shape, is what tells the lines apart.
fn write_1_8_save(name: &str) -> PathBuf {
    let p = Profile::named(name);
    let path = p.save().unwrap();
    let mut data = read_save(&path);
    data.remove("created_line");
    data.remove("_signature");
    data.remove("_signature_version");
    data.insert("version".into(), json!(5));
    data.insert("_signature_version".into(), json!(1));
    let signature = signature_for(&data, None);
    data.insert("_signature".into(), json!(signature));
    write_packed(&path, &data);
    path
}

/// A plain-JSON save from long before the packed container (version 3).
fn write_ancient_json_save(name: &str) -> PathBuf {
    let p = Profile::named(name);
    let packed = p.save().unwrap();
    let mut data = read_save(&packed);
    std::fs::remove_file(&packed).unwrap();
    for field in [
        "created_line",
        "truck_conditions",
        "_signature",
        "_signature_version",
    ] {
        data.remove(field);
    }
    data.insert("version".into(), json!(3));
    let path = packed.with_extension("json");
    write_text(&path, &data);
    path
}

/// A current-version 1.9 save from before the created-on marker existed.
fn write_pre_marker_1_9_save(name: &str) -> PathBuf {
    let p = Profile::named(name);
    let path = p.save().unwrap();
    let mut data = read_save(&path);
    data.remove("created_line");
    data.remove("_signature");
    let signature = signature_for(&data, None);
    data.insert("_signature".into(), json!(signature));
    write_packed(&path, &data);
    path
}

fn dict(value: Value) -> Map<String, Value> {
    match value {
        Value::Object(map) => map,
        _ => unreachable!(),
    }
}

#[test]
fn test_marker_decides_when_present() {
    assert!(is_pre_1_9_save(&dict(json!({"version": 5}))));
    assert!(is_pre_1_9_save(&dict(json!({"version": 4}))));
    assert!(is_pre_1_9_save(&dict(json!({})))); // no version at all: ancient
    assert!(!is_pre_1_9_save(&dict(json!({"version": 6}))));
    assert!(!is_pre_1_9_save(&dict(json!({"version": SAVE_VERSION}))));
    // The marker wins over the version threshold in both directions: a future
    // line may keep version numbers while changing lines.
    assert!(!is_pre_1_9_save(&dict(
        json!({"version": 5, "created_line": "1.9"})
    )));
}

#[test]
fn test_new_saves_carry_the_created_on_marker() {
    with_data_dir(|_| {
        let path = Profile::named("Fresh Start").save().unwrap();
        let data = read_save(&path);
        assert_eq!(data["created_line"], json!("1.9"));
        // The marker is signed like every other field; a clean reload needs no
        // rewrite and keeps the marker.
        let loaded = load(&path);
        assert!(!loaded.needs_migration_resave);
        assert_eq!(loaded.created_line, "1.9");
    });
}

#[test]
fn test_1_8_save_refuses_to_load_and_stays_byte_for_byte_intact() {
    with_data_dir(|_| {
        let path = write_1_8_save("Old Timer");
        let before = std::fs::read(&path).unwrap();

        let refusal = Profile::load(&path);
        let Err(LoadError::LegacyCareer(err)) = refusal else {
            panic!("expected a legacy-career refusal");
        };

        assert_eq!(err.name, "Old Timer");
        assert_eq!(std::fs::read(&path).unwrap(), before);
        // Refused, not quarantined, not converted: no side files appear.
        assert!(!with_suffix(&path, ".ffsave.invalid").exists());
        assert!(!with_suffix(&path, ".ffsave.bak").exists());
        assert!(is_pre_1_9_save_file(&path));
    });
}

#[test]
fn test_ancient_plain_json_save_is_also_refused_untouched() {
    with_data_dir(|_| {
        let path = write_ancient_json_save("Ancient");
        let before = std::fs::read(&path).unwrap();
        assert!(matches!(
            Profile::load(&path),
            Err(LoadError::LegacyCareer(_))
        ));
        assert!(path.exists() && std::fs::read(&path).unwrap() == before);
        // No conversion to the packed container happened either.
        assert!(!path.with_extension("ffsave").exists());
    });
}

#[test]
fn test_pre_marker_1_9_save_loads_and_is_stamped() {
    with_data_dir(|_| {
        let path = write_pre_marker_1_9_save("Tester");
        let loaded = load(&path);
        assert_eq!(loaded.name, "Tester");
        assert_eq!(loaded.created_line, "1.9");
        // The load stamped the marker into the rewritten save, so the version
        // threshold backfill is only ever consulted once per career.
        let on_disk = read_save(&path);
        assert_eq!(on_disk["created_line"], json!("1.9"));
        assert!(!load(&path).needs_migration_resave);
    });
}

// `test_legacy_career_stays_listed_and_opens_the_notice` is live in `crates/freight-fate/tests/states_main_menu.rs`.

// `test_notice_start_new_career_opens_name_entry` is live in `crates/freight-fate/tests/states_main_menu.rs`.

// `test_new_career_will_not_overwrite_a_same_named_legacy_save` is live in `crates/freight-fate/tests/states_main_menu.rs`.

#[test]
fn test_package_version_matches_the_release_metadata() {
    // The crate version is the package version: the workspace's release
    // metadata (`package-version`, which the release tooling reads) carries
    // the same number.
    let manifest = std::fs::read_to_string(game_root().join("Cargo.toml")).unwrap();
    let line = manifest
        .lines()
        .find(|l| l.starts_with("package-version = "))
        .expect("a package version");
    assert!(
        line.contains(env!("CARGO_PKG_VERSION")),
        "{line} vs {}",
        env!("CARGO_PKG_VERSION")
    );
    assert!(!game_version().is_empty());
}

#[test]
fn test_dev_checkout_has_no_baked_version() {
    // A source checkout's executable is the test binary, which has no
    // build_info.json beside it -- the version must keep coming from the crate
    // metadata fallback, not a stray baked file.
    assert_eq!(baked_version(), None);
    assert!(!is_frozen());
}

#[test]
fn test_baked_version_read_from_build_info_beside_the_executable() {
    let tmp = tempfile::tempdir().unwrap();
    let exe = tmp.path().join("FreightFate.exe");
    std::fs::write(&exe, b"").unwrap();
    std::fs::write(
        tmp.path().join("build_info.json"),
        r#"{"tag": "v1.9.0", "channel": "stable", "package_version": "1.9.0"}"#,
    )
    .unwrap();
    assert_eq!(baked_version_beside(&exe), Some("1.9.0".to_string()));
}

#[test]
fn test_baked_version_missing_file_falls_through() {
    let tmp = tempfile::tempdir().unwrap();
    let exe = tmp.path().join("FreightFate.exe");
    std::fs::write(&exe, b"").unwrap();
    assert_eq!(baked_version_beside(&exe), None);
}

#[test]
fn test_baked_version_malformed_json_falls_through() {
    let tmp = tempfile::tempdir().unwrap();
    let exe = tmp.path().join("FreightFate.exe");
    std::fs::write(&exe, b"").unwrap();
    std::fs::write(tmp.path().join("build_info.json"), "not json").unwrap();
    assert_eq!(baked_version_beside(&exe), None);
}
