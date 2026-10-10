//! The baked container against the JSON tree it was baked from.
//!
//! One process, two worlds: the shipped `data/` tree read as JSON
//! through the loaders, and the same tree baked into a `world.ffdata` that
//! `FREIGHT_FATE_DATA_ROOT` then points the whole runtime at. Every loader
//! that has a baked path is asked the same question twice and has to answer
//! the same way -- if the two ever drift, the game ships data nobody tested.
//!
//! # Why these are `#[ignore]`, and where they DO run
//!
//! This is the most valuable suite in the repository to run and the worst
//! one to run on every save. Every other test in the suite reads the JSON
//! tree; players read the baked container. Nothing else would notice the two
//! coming apart, and the suite would stay green while it happened -- which
//! is not hypothetical: it is how the client came to advise 125 mph where
//! the JSON said 80.
//!
//! What it costs is a full rebake of all 1,291 legs, about twelve seconds,
//! and `data_map_correction` pays the same again: roughly a quarter of a
//! minute on a suite meant to answer in well under thirty. So it takes the
//! adversarial battery's shape -- deselected by default, run deliberately:
//!
//! ```text
//! cargo test -p ff-core --test data_baked -- --ignored
//! cargo test -p ff-core --test data_map_correction -- --ignored
//! ```
//!
//! Deselected must not mean unrun. A bad container reaches a tester through
//! a release build, so that is where the gate belongs: `tools/build_release.py`
//! already runs `ff-bake --check` on every build, and CI should run both of
//! these with `--ignored` whenever the data tree, the loaders or the baker
//! change. Nothing else stands between a bad bake and a player.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use ff_core::data::baked::{bake, bake_is_deterministic, BakedData, BAKED_FILE_NAME};
use ff_core::data::curves::{self, CurveRecord};
use ff_core::data::street_limits::{load_street_limits, load_street_limits_from, StreetLimits};
use ff_core::data::world::World;
use ff_core::data::world_local_data::load_facility_approaches;
use ff_core::data::{buffs, world_models::DataError};
use once_cell::sync::OnceCell;

/// How many legs get the full corridor comparison (the task's floor is 50).
const CORRIDOR_SAMPLE: usize = 60;
/// How many keys of each side map get compared one by one.
const SIDE_MAP_SAMPLE: usize = 100;

struct Fixture {
    json_dir: PathBuf,
    baked_dir: PathBuf,
}

static FIXTURE: OnceCell<Fixture> = OnceCell::new();

fn repo_root() -> PathBuf {
    // crates/ff-core -> repo root
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("repo root")
        .to_path_buf()
}

/// Bake the shipped tree once, then point the process at the container.
///
/// The env var is set *after* the bake, never before: the baker reads the
/// JSON tree through explicit paths, and a `FREIGHT_FATE_DATA_ROOT` pointing
/// at a not-yet-written container would let the process cache "there is no
/// container" for its whole life.
fn fixture() -> &'static Fixture {
    FIXTURE.get_or_init(|| {
        let json_dir = repo_root().join("data");
        assert!(
            json_dir.join("world_data").join("index.json").is_file(),
            "the shipped data tree is missing at {}",
            json_dir.display()
        );
        let temp = Box::leak(Box::new(
            tempfile::tempdir().expect("a temp dir for the baked container"),
        ));
        let baked_dir = temp.path().to_path_buf();
        let started = Instant::now();
        let report = bake(&json_dir, &baked_dir.join(BAKED_FILE_NAME)).expect("bake");
        println!("\n--- bake of the shipped data tree ---");
        print!("{}", report.table());
        println!(
            "{} legs, {} with corridor detail, baked in {:.2} s",
            report.legs,
            report.legs_with_corridor,
            started.elapsed().as_secs_f64()
        );
        std::env::set_var("FREIGHT_FATE_DATA_ROOT", &baked_dir);
        Fixture {
            json_dir,
            baked_dir,
        }
    })
}

fn json_world() -> World {
    World::load_from_json(&fixture().json_dir).expect("the JSON world loads")
}

/// The world the runtime would load in a shipped build.
fn baked_world() -> World {
    let world = World::load().expect("the baked world loads");
    assert_eq!(
        world.data_dir(),
        fixture().baked_dir.as_path(),
        "the process data root should be the baked container's directory"
    );
    world
}

fn container() -> Arc<BakedData> {
    BakedData::open(&fixture().baked_dir.join(BAKED_FILE_NAME)).expect("the container opens")
}

/// Evenly spread indices, so the sample crosses every state shard rather
/// than sitting in whichever one sorts first.
fn spread(total: usize, want: usize) -> Vec<usize> {
    if total == 0 {
        return Vec::new();
    }
    let want = want.min(total);
    let step = (total as f64 / want as f64).max(1.0);
    let mut out: Vec<usize> = (0..want).map(|i| ((i as f64) * step) as usize).collect();
    out.dedup();
    out
}

#[test]
#[ignore = "bake: rebuilds the whole baked container (~12s); run with --ignored -- see the module note"]
fn baked_cities_match_the_json_tree_field_for_field() {
    let json = json_world();
    let baked = baked_world();
    let json_keys: Vec<&String> = json.cities.keys().collect();
    let baked_keys: Vec<&String> = baked.cities.keys().collect();
    assert_eq!(baked_keys, json_keys, "city keys, in file order");
    assert!(json_keys.len() > 300, "{} cities", json_keys.len());
    for key in json_keys {
        let want = &json.cities[key];
        let got = &baked.cities[key];
        assert_eq!(got, want, "city {key}");
    }
}

#[test]
#[ignore = "bake: rebuilds the whole baked container (~12s); run with --ignored -- see the module note"]
fn baked_legs_carry_the_same_eager_fields() {
    let json = json_world();
    let baked = baked_world();
    assert_eq!(baked.legs.len(), json.legs.len(), "leg count");
    assert!(json.legs.len() > 1000, "{} legs", json.legs.len());
    for (index, (want, got)) in json.legs.iter().zip(baked.legs.iter()).enumerate() {
        assert_eq!(got.id, want.id, "leg {index} id");
        assert_eq!(got.a, want.a, "leg {index} a");
        assert_eq!(got.b, want.b, "leg {index} b");
        assert_eq!(got.miles, want.miles, "leg {index} miles");
        assert_eq!(got.highway, want.highway, "leg {index} highway");
        assert_eq!(got.terrain, want.terrain, "leg {index} terrain");
        assert_eq!(got.stops, want.stops, "leg {index} stops");
        assert_eq!(got.lanes, want.lanes, "leg {index} lanes");
        assert_eq!(got.divided, want.divided, "leg {index} divided");
        assert_eq!(
            got.truck_advisory, want.truck_advisory,
            "leg {index} truck advisory"
        );
        assert_eq!(
            got.meta_complete, want.meta_complete,
            "leg {index} meta_complete"
        );
        assert_eq!(got.local_cue, want.local_cue, "leg {index} local cue");
        assert_eq!(
            got.local_speed_mph, want.local_speed_mph,
            "leg {index} local speed"
        );
    }
}

#[test]
#[ignore = "bake: rebuilds the whole baked container (~12s); run with --ignored -- see the module note"]
fn a_spread_of_legs_has_identical_corridor_detail() {
    let json = json_world();
    let baked = baked_world();
    let picks = spread(json.legs.len(), CORRIDOR_SAMPLE);
    assert!(picks.len() >= 50, "{} legs sampled", picks.len());
    let mut states: std::collections::HashSet<String> = std::collections::HashSet::new();
    for index in picks {
        let want = json.legs[index].corridor();
        let got = baked.legs[index].corridor();
        let leg = format!("{} -> {}", json.legs[index].a, json.legs[index].b);
        // Field by field, so a failure names the field rather than dumping
        // two corridors at a reader.
        assert_eq!(got.route_points, want.route_points, "{leg} route points");
        assert_eq!(
            got.elevation_samples, want.elevation_samples,
            "{leg} elevation samples"
        );
        assert_eq!(got.grade_segments, want.grade_segments, "{leg} grades");
        assert_eq!(
            got.state_crossings, want.state_crossings,
            "{leg} state crossings"
        );
        assert_eq!(got.checkpoints, want.checkpoints, "{leg} checkpoints");
        assert_eq!(got.state_miles, want.state_miles, "{leg} state miles");
        assert_eq!(got.toll_events, want.toll_events, "{leg} tolls");
        assert_eq!(got.interchanges, want.interchanges, "{leg} interchanges");
        assert_eq!(got.speed_limits, want.speed_limits, "{leg} speed limits");
        assert_eq!(
            got.traffic_volumes, want.traffic_volumes,
            "{leg} traffic volumes"
        );
        assert_eq!(got.hpms_terrain, want.hpms_terrain, "{leg} hpms terrain");
        assert_eq!(got.landmarks, want.landmarks, "{leg} landmarks");
        assert_eq!(got.restrictions, want.restrictions, "{leg} restrictions");
        assert_eq!(got.lane_segments, want.lane_segments, "{leg} lane segments");
        assert_eq!(got, want, "{leg} corridor");
        if let Some(city) = json.cities.get(&json.legs[index].a) {
            states.insert(city.state_code.clone());
        }
    }
    assert!(
        states.len() >= 20,
        "the sample should cross the map, saw {} states",
        states.len()
    );
}

#[test]
#[ignore = "bake: rebuilds the whole baked container (~12s); run with --ignored -- see the module note"]
fn baked_curves_match_the_screened_jsonl_table() {
    let json = json_world();
    let curves_text = std::fs::read_to_string(
        fixture()
            .json_dir
            .join("world_data/us/gameplay/curves.jsonl"),
    )
    .expect("curves.jsonl");
    let artifacts_text = std::fs::read_to_string(
        fixture()
            .json_dir
            .join("world_data/us/gameplay/curve_artifacts.jsonl"),
    )
    .expect("curve_artifacts.jsonl");
    let want: HashMap<String, Vec<CurveRecord>> =
        curves::build_from_sources(&curves_text, &artifacts_text, &json);
    let got = curves::load();
    assert!(want.len() > 100, "{} curve legs", want.len());
    assert_eq!(got.len(), want.len(), "legs carrying curves");
    for (key, rows) in &want {
        assert_eq!(got.get(key), Some(rows), "curves for {key}");
    }
    // And the legs the corridor sample covers, by name.
    for index in spread(json.legs.len(), CORRIDOR_SAMPLE) {
        let leg = &json.legs[index];
        let key = format!("{}:{}", leg.a, leg.b);
        assert_eq!(got.get(&key), want.get(&key), "curves for {key}");
    }
}

#[test]
#[ignore = "bake: rebuilds the whole baked container (~12s); run with --ignored -- see the module note"]
fn the_five_side_maps_match() {
    let json = json_world();
    let baked = baked_world();

    let want = json.city_service_data().expect("city services");
    let got = baked.city_service_data().expect("city services");
    assert_eq!(got.len(), want.len(), "city service cities");
    for index in spread(want.len(), SIDE_MAP_SAMPLE) {
        let (city, services) = want.get_index(index).expect("index");
        assert_eq!(got.get(city), Some(services), "city services for {city}");
    }

    let want = json.facility_approaches().expect("facility approaches");
    let got = baked.facility_approaches().expect("facility approaches");
    assert_eq!(got.len(), want.len(), "facility approaches");
    for index in spread(want.len(), SIDE_MAP_SAMPLE) {
        let (key, value) = want.get_index(index).expect("index");
        assert_eq!(got.get(key), Some(value), "facility approach {key}");
    }

    let want = json.facility_endpoints().expect("facility endpoints");
    let got = baked.facility_endpoints().expect("facility endpoints");
    assert_eq!(got.len(), want.len(), "facility endpoints");
    for index in spread(want.len(), SIDE_MAP_SAMPLE) {
        let (key, value) = want.get_index(index).expect("index");
        assert_eq!(got.get(key), Some(value), "facility endpoint {key}");
    }

    let want = json.local_approaches().expect("local approaches");
    let got = baked.local_approaches().expect("local approaches");
    assert_eq!(got.len(), want.len(), "local approaches");
    for index in spread(want.len(), SIDE_MAP_SAMPLE) {
        let (key, value) = want.get_index(index).expect("index");
        assert_eq!(got.get(key), Some(value), "local approach {key}");
    }

    let want = json.local_geometries().expect("local geometry");
    let got = baked.local_geometries().expect("local geometry");
    assert_eq!(got.len(), want.len(), "local geometries");
    for index in spread(want.len(), SIDE_MAP_SAMPLE) {
        let (key, value) = want.get_index(index).expect("index");
        assert_eq!(got.get(key), Some(value), "local geometry {key}");
    }
}

#[test]
#[ignore = "bake: rebuilds the whole baked container (~12s); run with --ignored -- see the module note"]
fn a_side_map_named_by_path_falls_through_to_the_container() {
    // `--smoke` and the tools reach past `World` and name a file directly.
    // In a release that file is not on disk, and the answer still has to be
    // the real data rather than an empty map that reads as thin coverage.
    let fixture = fixture();
    let want = load_facility_approaches(&fixture.json_dir.join("facility_approaches.json"))
        .expect("json approaches");
    let got = load_facility_approaches(&fixture.baked_dir.join("facility_approaches.json"))
        .expect("baked approaches");
    assert!(!got.is_empty(), "the container stood in for the loose file");
    assert_eq!(got.len(), want.len(), "facility approaches");
    for index in spread(want.len(), SIDE_MAP_SAMPLE) {
        let (key, value) = want.get_index(index).expect("index");
        assert_eq!(got.get(key), Some(value), "facility approach {key}");
    }
}

fn same_limits(got: &StreetLimits, want: &StreetLimits) {
    let want_states: Vec<&str> = want.states().collect();
    let got_states: Vec<&str> = got.states().collect();
    assert_eq!(
        got_states, want_states,
        "street limit states, in file order"
    );
    assert!(want_states.len() >= 45, "{} states", want_states.len());
    for state in want_states {
        assert_eq!(got.get(state), want.get(state), "street limits for {state}");
        assert_eq!(
            got.facility_street_mph(state),
            want.facility_street_mph(state),
            "facility street mph for {state}"
        );
    }
}

#[test]
#[ignore = "bake: rebuilds the whole baked container (~12s); run with --ignored -- see the module note"]
fn street_limits_buffs_and_the_radio_catalogs_match_in_full() {
    let json_dir = &fixture().json_dir;

    let want = load_street_limits_from(&json_dir.join("street_limits.json")).expect("limits");
    same_limits(load_street_limits(), &want);

    let want = buffs::parse_catalog(
        &std::fs::read_to_string(json_dir.join("buffs.json")).expect("buffs.json"),
    )
    .expect("buff catalog");
    let got = buffs::buff_catalog();
    assert_eq!(got.len(), want.len(), "buff count");
    for (id, buff) in &want {
        assert_eq!(got.get(id), Some(buff), "buff {id}");
    }

    let want = ff_core::radio::load_full_catalog(json_dir).expect("radio catalog");
    let got = ff_core::radio::default_radio_catalog();
    assert_eq!(got.len(), want.len(), "radio station count");
    assert!(want.len() > 100, "{} stations", want.len());
    for (index, station) in want.iter().enumerate() {
        assert_eq!(&got[index], station, "station {}", station.id);
    }
}

#[test]
#[ignore = "bake: rebuilds the whole baked container (~12s); run with --ignored -- see the module note"]
fn the_container_holds_every_section_and_nothing_stray() {
    let container = container();
    let mut want = vec![
        "cities",
        "city_services",
        "corridors",
        "curves",
        "facility_approaches",
        "facility_endpoints",
        "legs",
        "local_approaches",
        "local_geometry",
        "text:buffs.json",
        "text:channel3000.json",
        "text:radio_catalog.json",
        "text:radio_community.json",
        "text:radio_imported.json",
        "text:street_limits.json",
    ];
    want.sort_unstable();
    assert_eq!(container.section_names(), want);
}

#[test]
#[ignore = "bake: rebuilds the whole baked container (~12s); run with --ignored -- see the module note"]
fn baking_twice_gives_the_same_bytes() {
    assert!(
        bake_is_deterministic(&fixture().json_dir).expect("bake"),
        "two bakes of one tree must be byte identical, or --check is useless"
    );
}

#[test]
#[ignore = "bake: rebuilds the whole baked container (~12s); run with --ignored -- see the module note"]
fn a_container_from_another_format_version_is_refused() {
    let bytes = std::fs::read(fixture().baked_dir.join(BAKED_FILE_NAME)).expect("read container");
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join(BAKED_FILE_NAME);
    let mut wrong = bytes.clone();
    let version = u32::from_le_bytes(wrong[8..12].try_into().expect("4 bytes"));
    wrong[8..12].copy_from_slice(&(version + 1).to_le_bytes());
    std::fs::write(&path, &wrong).expect("write");
    let err: DataError = BakedData::open(&path).expect_err("a bad version is refused");
    let message = err.to_string();
    assert!(
        message.contains(&format!("format {}", version + 1)),
        "{message}"
    );
    assert!(message.contains("ff-bake"), "{message}");

    // Truncation is refused too, rather than half-read.
    let path = dir.path().join("truncated.ffdata");
    std::fs::write(&path, &bytes[..bytes.len() / 2]).expect("write");
    let err = BakedData::open(&path).expect_err("a truncated container is refused");
    assert!(err.to_string().contains("truncated"), "{err}");
}

#[test]
#[ignore = "bake: rebuilds the whole baked container (~12s); run with --ignored -- see the module note"]
fn the_baked_world_loads_far_faster_than_the_json_tree() {
    let fixture = fixture();
    let started = Instant::now();
    let json = World::load_from_json(&fixture.json_dir).expect("json world");
    let json_ms = started.elapsed().as_secs_f64() * 1000.0;

    // Cold: a container this process has never mapped, so the eager section
    // is decompressed and decoded here rather than being someone else's
    // already-warm `OnceCell`. This is what a player's first launch pays.
    let path = fixture.baked_dir.join(BAKED_FILE_NAME);
    let started = Instant::now();
    let cold = BakedData::open(&path).expect("open");
    let cold_world = World::from_baked(cold, fixture.baked_dir.clone()).expect("baked world");
    let cold_ms = started.elapsed().as_secs_f64() * 1000.0;

    let started = Instant::now();
    let baked = World::load().expect("baked world");
    let baked_ms = started.elapsed().as_secs_f64() * 1000.0;

    // Split the cold number: how much is reading the container, and how much
    // is the derived tables `World` rebuilds either way.
    let raw = BakedData::open(&path).expect("open");
    let started = Instant::now();
    let cities = raw.take_cities().expect("cities");
    let legs = raw.take_legs().expect("legs");
    let decode_ms = started.elapsed().as_secs_f64() * 1000.0;

    let container_bytes = std::fs::metadata(&path).expect("container").len();
    println!("\n--- World::load() ---");
    println!(
        "JSON tree      {json_ms:8.1} ms  ({} legs)",
        json.legs.len()
    );
    println!(
        "baked, cold map{cold_ms:8.1} ms  ({} legs, {container_bytes} bytes on disk)",
        cold_world.legs.len()
    );
    println!(
        "baked, mapped  {baked_ms:8.1} ms  ({} legs)",
        baked.legs.len()
    );
    println!(
        "  of which decode {decode_ms:5.1} ms  ({} cities, {} legs read out of the file)",
        cities.len(),
        legs.len()
    );
    // Measured 2026-08-22 on the owner's machine: 23.7 ms cold against
    // 248 ms of JSON in `--release`, 47 ms against 449 ms in the test
    // profile. The bar here is a regression guard with room for a loaded CI
    // box, not a benchmark; the ratio below is the part that means something
    // on any machine.
    assert!(
        cold_ms < 150.0,
        "a cold baked world should load in well under 150 ms, took {cold_ms:.1} ms"
    );
    assert!(
        cold_ms * 4.0 < json_ms,
        "the baked world should load several times faster than the JSON tree          ({cold_ms:.1} ms against {json_ms:.1} ms)"
    );

    // One leg's corridor, cold, is the latency that matters while driving.
    let index = baked.legs.len() / 2;
    let started = Instant::now();
    let detail = baked.legs[index].corridor();
    let corridor_ms = started.elapsed().as_secs_f64() * 1000.0;
    println!(
        "one cold corridor{corridor_ms:6.2} ms  ({} route points, {} landmarks)",
        detail.route_points.len(),
        detail.landmarks.len()
    );

    assert!(
        corridor_ms < 5.0,
        "one leg's corridor should decompress in under 5 ms, took {corridor_ms:.2} ms"
    );
}
