//! Cross-language parity for the file the orinks.net validator is built from.
//!
//! `profile_integrity_invariants.json` in `crates/ff-core/tests/` was first
//! rendered by the since-deleted Python exporter and is stored with LF
//! endings; the Rust exporter always writes LF.
//!
//! This surface decides whether a submitted career is arithmetically
//! possible. An export that drifts from what the game awards makes the
//! validator either reject honest players or accept impossible careers, so
//! the bytes are pinned rather than the shape: any change at all has to be a
//! change someone chose.
//!
//! When this test fails, that is the finding. Re-run
//!
//! ```text
//! cargo run -p ff-core --bin ff-invariants -- \
//!     crates/ff-core/tests/profile_integrity_invariants.json
//! ```
//!
//! and read the diff before committing it: a moved catalog key means
//! orinks.net needs the fresh export before the next build reaches players.

use std::path::{Path, PathBuf};
use std::process::Command;

use ff_core::profile_integrity_invariants::{
    current_rendered_invariants, invariant_data, CatalogInputs,
};
use serde_json::Value;

/// The committed export (first rendered by the Python exporter).
const PYTHON_EXPORT: &str = include_str!("../profile_integrity_invariants.json");

/// `include_str!` keeps whatever the checkout has; a repo cloned with
/// `core.autocrlf=true` hands back CRLF for a file stored as LF.
fn fixture() -> String {
    PYTHON_EXPORT.replace("\r\n", "\n")
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("the repo root is two levels above crates/ff-core")
}

/// The whole point of the port: the Rust rendering is the Python rendering.
#[test]
fn the_rust_export_is_byte_for_byte_the_python_export() {
    let rust = current_rendered_invariants().expect("the shipped catalogs render");
    let python = fixture();
    if rust == python {
        return;
    }
    // Name the key that moved rather than dumping 37 kB of JSON at the
    // reader: a catalog key is a balance pass the validator has not heard
    // about, `cityLabels` alone is a world-data edit.
    let left: Value = serde_json::from_str(&python).expect("the fixture is JSON");
    let right: Value = serde_json::from_str(&rust).expect("the rendering is JSON");
    let (left, right) = (
        left.as_object().expect("an object"),
        right.as_object().expect("an object"),
    );
    let mut moved: Vec<&str> = left
        .keys()
        .chain(right.keys())
        .filter(|key| left.get(*key) != right.get(*key))
        .map(String::as_str)
        .collect();
    moved.sort_unstable();
    moved.dedup();
    assert!(
        moved.is_empty(),
        "the Rust export differs from the Python export under {moved:?} -- \
         see the header of this file before re-generating the fixture"
    );
    // Same values, different bytes: a rendering difference (float repr,
    // escaping, indent), which is the harder kind of drift to spot.
    assert_eq!(
        rust, python,
        "the Rust and Python exports hold the same values but render \
         different bytes"
    );
}

/// The catalogs the export names are the ones the export was built from.
///
/// Cheap insurance that the fixture was not generated from a stale checkout:
/// the figures the validator does arithmetic with are re-derived here.
#[test]
fn the_fixture_carries_the_live_catalog_figures() {
    let fixture: Value = serde_json::from_str(&fixture()).expect("the fixture is JSON");
    let live = ff_core::profile_integrity_invariants::current_invariant_data()
        .expect("the shipped catalogs render");
    for key in [
        "startingMoney",
        "startingMoneyMax",
        "payAdvanceLimit",
        "xpPerMileMax",
        "xpFlatPerDelivery",
        "levelXp",
        "profileFields",
        "careerFields",
        "truckConditionFields",
        "sourceSaveVersion",
        "achievementIds",
        "achievementLabels",
        "achievementDetails",
        "achievementCategories",
        "careerTitles",
        "companyCareerTitles",
        "carrierLabels",
        "trailerCatalog",
        "truckPrices",
        "upgradePrices",
        "endorsements",
        "fleetTiers",
        "marketCargoKeys",
    ] {
        assert_eq!(fixture[key], live[key], "{key} moved");
    }
}

#[test]
fn the_public_profile_catalogs_are_derived_from_live_game_catalogs() {
    use ff_core::achievements::{ACHIEVEMENTS, CATEGORIES};
    use ff_core::models::career_ladder::{CAREER_RANKS, COMPANY_CAREER_RANKS};
    use ff_core::models::start_options::all_start_options;
    use ff_core::models::trailers::TRAILER_CATALOG;

    let exported = ff_core::profile_integrity_invariants::current_invariant_data()
        .expect("the shipped catalogs render");

    let titles: Vec<&str> = CAREER_RANKS.iter().map(|rank| rank.title).collect();
    assert_eq!(exported["careerTitles"], serde_json::json!(titles));
    let company_titles: Vec<&str> = COMPANY_CAREER_RANKS.iter().map(|rank| rank.title).collect();
    assert_eq!(
        exported["companyCareerTitles"],
        serde_json::json!(company_titles)
    );

    for option in all_start_options() {
        assert_eq!(
            exported["carrierLabels"][option.key], option.carrier_name,
            "{} carrier label moved",
            option.key
        );
    }
    for trailer in TRAILER_CATALOG {
        assert_eq!(
            exported["trailerCatalog"][trailer.key]["label"],
            trailer.label
        );
        assert_eq!(
            exported["trailerCatalog"][trailer.key]["purchasePrice"],
            trailer.purchase_price
        );
    }
    for achievement in ACHIEVEMENTS {
        assert_eq!(
            exported["achievementLabels"][achievement.id], achievement.name,
            "{} achievement label moved",
            achievement.id
        );
        // What the profile page says a badge was for, hidden badges included:
        // a badge on a profile has already been earned.
        assert_eq!(
            exported["achievementDetails"][achievement.id]["description"], achievement.description,
            "{} achievement description moved",
            achievement.id
        );
        assert_eq!(
            exported["achievementDetails"][achievement.id]["category"], achievement.category,
            "{} achievement category moved",
            achievement.id
        );
        // The inspiration field is the song behind the badge: a maintainer
        // note, never public.
        assert!(
            exported["achievementDetails"][achievement.id]
                .get("inspiration")
                .is_none(),
            "{} exported its inspiration",
            achievement.id
        );
    }
    let categories: Vec<Value> = CATEGORIES
        .iter()
        .map(|category| serde_json::json!({"key": category.id, "title": category.title}))
        .collect();
    assert_eq!(exported["achievementCategories"], Value::Array(categories));
}

/// The shipped code path, end to end: the binary writes the file, and its
/// `--check` form passes on it and fails on drift.
#[test]
fn ff_invariants_writes_and_checks_the_export() {
    let dir = tempfile::tempdir().expect("a temp dir");
    let out = dir.path().join("profile_integrity_invariants.json");
    let data_dir = repo_root().join("data");

    let exporter = || {
        let mut command = Command::new(env!("CARGO_BIN_EXE_ff-invariants"));
        command
            .arg(&out)
            .arg("--data-dir")
            .arg(&data_dir)
            .arg("--quiet");
        command
    };

    let wrote = exporter().status().expect("ff-invariants runs");
    assert!(wrote.success(), "ff-invariants failed to write the export");
    assert_eq!(
        std::fs::read_to_string(&out).expect("the export was written"),
        fixture(),
        "ff-invariants wrote something other than the Python export"
    );

    let checked = exporter()
        .arg("--check")
        .status()
        .expect("ff-invariants runs");
    assert!(checked.success(), "--check called a fresh export stale");

    // A file the Python exporter wrote on Windows: same JSON, CRLF endings.
    // --check is looking for a catalog that moved, not for line endings.
    std::fs::write(&out, fixture().replace('\n', "\r\n")).expect("rewrite the export");
    let crlf = exporter()
        .arg("--check")
        .status()
        .expect("ff-invariants runs");
    assert!(
        crlf.success(),
        "--check called a CRLF copy of the same export stale"
    );

    // One moved figure is drift, and --check has to catch it.
    let drifted = fixture().replace("\"startingMoney\": 5000", "\"startingMoney\": 5001");
    assert_ne!(drifted, fixture(), "the drift edit found nothing to change");
    std::fs::write(&out, &drifted).expect("rewrite the export");
    let stale = exporter()
        .arg("--check")
        .status()
        .expect("ff-invariants runs");
    assert!(
        !stale.success(),
        "--check passed an export with drifted money"
    );
}

/// A hand-assembled `CatalogInputs` renders something the validator must
/// never be built from -- which is why the exporter only uses `current()`.
///
/// Not a style point: the audit that found this gap found four tests passing
/// against fixture numbers, two of them a false green. The fixture and the
/// live catalogs have to be visibly different things.
#[test]
fn a_fixture_input_is_not_the_live_export() {
    let live = CatalogInputs::current();
    let mut fixture_inputs = live.clone();
    fixture_inputs.starting_money_max = 5_000.0;
    let root = ff_core::profile_integrity_invariants::world_data_root();
    assert_ne!(
        invariant_data(&root, &fixture_inputs).unwrap(),
        invariant_data(&root, &live).unwrap()
    );
}

/// orinks.net refuses a version 2 delivery whose cargo key it cannot find
/// in `cargoLabels`, so every cargo a job can carry has to be there.
///
/// A job's cargo is a `&'static CargoType`, and the catalog is the only
/// static that holds one, so the catalog walk is the whole set; the world's
/// facilities and the carriers' favored freight are walked as well, because
/// the board picks a cargo by those keys.
#[test]
fn every_cargo_a_job_can_carry_is_in_the_export() {
    use ff_core::data::world::World;
    use ff_core::data::world_constants::FACILITY_CARGO_ROLES;
    use ff_core::models::jobs::CARGO_CATALOG;
    use ff_core::models::start_options::company_start_options;

    let data = ff_core::profile_integrity_invariants::current_invariant_data()
        .expect("the shipped catalogs render");
    let labels = data["cargoLabels"]
        .as_object()
        .expect("the export carries cargoLabels");
    let mut keys: Vec<String> = CARGO_CATALOG.keys().map(|key| key.to_string()).collect();
    for (_, ships, receives) in FACILITY_CARGO_ROLES {
        keys.extend(
            ships
                .iter()
                .chain(receives.iter())
                .map(|key| key.to_string()),
        );
    }
    for option in company_start_options() {
        keys.extend(
            option
                .cargo_weight_bonus
                .iter()
                .map(|(key, _)| key.to_string()),
        );
    }
    let world = World::load_from(&crate::data_support::data_dir()).expect("the world loads");
    for city in world.cities.values() {
        for location in &city.locations {
            keys.extend(location.cargo.iter().cloned());
            keys.extend(location.ships.iter().cloned());
            keys.extend(location.receives.iter().cloned());
        }
    }
    keys.sort();
    keys.dedup();
    let missing: Vec<&String> = keys
        .iter()
        .filter(|key| !labels.contains_key(*key))
        .collect();
    assert!(
        missing.is_empty(),
        "cargo keys a job can carry but the export lacks: {missing:?}"
    );
    for (key, cargo) in CARGO_CATALOG.iter() {
        assert_eq!(labels[*key], cargo.label, "the export's label for {key}");
    }
}

/// Every weather key a delivery can send is listed, and fits the site's
/// pattern for one.
#[test]
fn every_weather_key_is_in_the_export() {
    use ff_core::sim::weather::WeatherKind;
    let data = ff_core::profile_integrity_invariants::current_invariant_data()
        .expect("the shipped catalogs render");
    let keys: Vec<&str> = data["weatherKeys"]
        .as_array()
        .expect("the export carries weatherKeys")
        .iter()
        .filter_map(Value::as_str)
        .collect();
    assert_eq!(keys.len(), WeatherKind::ALL.len());
    for kind in WeatherKind::ALL {
        let key = kind.key();
        assert!(keys.contains(&key), "{key} is not in weatherKeys");
        assert!(
            !key.is_empty()
                && key.len() <= 24
                && key.chars().all(|ch| ch.is_ascii_lowercase() || ch == '_'),
            "{key} does not fit the site's weather key pattern"
        );
        assert_eq!(data["weatherLabels"][key], kind.value());
    }
}
