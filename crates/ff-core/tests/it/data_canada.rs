//! The British Columbia and Yukon legs as rebuilt by the `cargo tools`
//! pipeline: each carries its own archived road, a grade profile, read
//! speed postings, read province lines and curves from the Canadian
//! gameplay folder.

use crate::data_support::{data_dir, world};
use ff_core::data::curves::leg_curves;
use ff_core::data::world_models::Leg;

/// The legs rebuilt from scaffolds: every leg that starts or ends in
/// British Columbia or Yukon. The Alaska Highway between Whitehorse and Tok
/// is split at Haines Junction, where the Haines Highway leaves it.
const REBUILT: &[&str] = &[
    "dawson_creek_bc_ca:fort_st_john_bc_ca",
    "dawson_creek_bc_ca:prince_george_bc_ca",
    "fort_nelson_bc_ca:fort_st_john_bc_ca",
    "fort_nelson_bc_ca:watson_lake_yt_ca",
    "fort_st_john_bc_ca:dawson_creek_bc_ca",
    "fort_st_john_bc_ca:fort_nelson_bc_ca",
    "prince_george_bc_ca:dawson_creek_bc_ca",
    "prince_george_bc_ca:surrey_bc_ca",
    "surrey_bc_ca:blaine_wa_us",
    "surrey_bc_ca:prince_george_bc_ca",
    "watson_lake_yt_ca:fort_nelson_bc_ca",
    "watson_lake_yt_ca:whitehorse_yt_ca",
    "whitehorse_yt_ca:haines_junction_yt_ca",
    "whitehorse_yt_ca:watson_lake_yt_ca",
    "whitehorse_yt_ca:skagway_ak_us",
    "haines_junction_yt_ca:whitehorse_yt_ca",
    "haines_junction_yt_ca:tok_ak_us",
    "haines_junction_yt_ca:haines_ak_us",
    "blaine_wa_us:surrey_bc_ca",
    "tok_ak_us:haines_junction_yt_ca",
    "haines_ak_us:haines_junction_yt_ca",
    "skagway_ak_us:whitehorse_yt_ca",
];

fn leg(id: &str) -> &'static Leg {
    let (a, b) = id.split_once(':').unwrap();
    world()
        .legs
        .iter()
        .find(|l| l.a == a && l.b == b)
        .unwrap_or_else(|| panic!("{id} is not in the world"))
}

fn archived_ids(path: &str) -> Vec<String> {
    let text = std::fs::read_to_string(data_dir().join(path)).unwrap();
    text.lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter_map(|r| r.get("leg").and_then(|v| v.as_str()).map(String::from))
        .collect()
}

#[test]
fn every_canadian_leg_has_an_archived_road_in_its_countrys_folder() {
    let mut ids = archived_ids("world_data/ca/geometry/bc.jsonl");
    ids.extend(archived_ids("world_data/ca/geometry/yt.jsonl"));
    ids.extend(archived_ids("world_data/us/geometry/wa.jsonl"));
    ids.extend(archived_ids("world_data/us/geometry/ak.jsonl"));
    for id in REBUILT {
        assert!(ids.iter().any(|i| i == id), "{id} has no archived road");
    }
    for path in [
        "world_data/ca/geometry/bc.jsonl",
        "world_data/ca/geometry/yt.jsonl",
    ] {
        for id in archived_ids(path) {
            let from = id.split(':').next().unwrap_or_default();
            assert!(
                from.ends_with("_ca"),
                "{id} starts in the US but is filed in {path}"
            );
        }
    }
}

#[test]
fn every_canadian_leg_has_a_grade_profile_that_covers_it() {
    for id in REBUILT {
        let leg = leg(id);
        let segments = leg.grade_segments();
        assert!(!segments.is_empty(), "{id} has no grade profile");
        let end = segments.iter().map(|s| s.end_mi).fold(0.0, f64::max);
        assert!(
            end >= leg.miles * 0.9,
            "{id}: grades end at mile {end:.1} of {:.1}",
            leg.miles
        );
    }
}

#[test]
fn curves_load_from_the_canadian_gameplay_folder() {
    assert!(!leg_curves("dawson_creek_bc_ca:fort_st_john_bc_ca", true).is_empty());
    assert!(!leg_curves("fort_nelson_bc_ca:watson_lake_yt_ca", true).is_empty());
}

#[test]
fn province_and_border_lines_are_read_with_their_names() {
    let yukon = leg("fort_nelson_bc_ca:watson_lake_yt_ca");
    assert!(
        yukon.state_crossings().iter().any(|c| c.state == "Yukon"),
        "{:?}",
        yukon.state_crossings()
    );
    let border = leg("haines_junction_yt_ca:tok_ak_us");
    assert!(
        border
            .state_crossings()
            .iter()
            .any(|c| c.from_state == "Yukon" && c.state == "Alaska"),
        "{:?}",
        border.state_crossings()
    );
    let miles: f64 = border.state_miles().iter().map(|s| s.miles).sum();
    assert!(
        (miles - border.miles).abs() <= 1.0,
        "state miles {miles} of {}",
        border.miles
    );
}

#[test]
fn canadian_postings_are_read_or_marked_as_gaps() {
    let mut read = 0;
    for id in REBUILT {
        for s in leg(id).speed_limits() {
            assert!(
                !s.source.starts_with("Coarse "),
                "{id} still carries a coarse posting at mile {}",
                s.at_mi
            );
            if s.mph.is_some() && !s.source.to_ascii_lowercase().contains("assumed") {
                read += 1;
            }
        }
    }
    assert!(read > 100, "only {read} read postings on the Canadian legs");
}
