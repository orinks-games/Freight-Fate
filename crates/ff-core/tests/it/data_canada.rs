//! The Canadian legs as built by the `cargo tools` pipeline (the BC and
//! Yukon pilot, then each corridor): each carries its own archived road, a grade profile, read
//! speed postings, read province lines and curves from the Canadian
//! gameplay folder.

use crate::data_support::{data_dir, world};
use ff_core::data::curves::leg_curves;
use ff_core::data::world_models::Leg;

/// Every leg that starts or ends in Canada: the BC and Yukon legs rebuilt
/// from scaffolds and every corridor built since.
fn canadian_legs() -> Vec<String> {
    world()
        .legs
        .iter()
        .filter(|l| l.a.ends_with("_ca") || l.b.ends_with("_ca"))
        .map(|l| format!("{}:{}", l.a, l.b))
        .collect()
}

fn geometry_files(country: &str) -> Vec<String> {
    let dir = data_dir().join(format!("world_data/{country}/geometry"));
    let mut files: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".jsonl"))
        .map(|n| format!("world_data/{country}/geometry/{n}"))
        .collect();
    files.sort();
    files
}

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
    let ca = geometry_files("ca");
    let mut ids: Vec<String> = Vec::new();
    for path in ca.iter().chain(geometry_files("us").iter()) {
        ids.extend(archived_ids(path));
    }
    let legs = canadian_legs();
    assert!(legs.len() > 100, "only {} Canadian legs", legs.len());
    for id in &legs {
        assert!(ids.iter().any(|i| i == id), "{id} has no archived road");
    }
    for path in &ca {
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
    for id in canadian_legs() {
        let leg = leg(&id);
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
    for id in canadian_legs() {
        for s in leg(&id).speed_limits() {
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

#[test]
fn prairie_province_lines_are_read_with_their_names() {
    let line = leg("lloydminster_ab_ca:north_battleford_sk_ca");
    assert!(
        line.state_crossings()
            .iter()
            .any(|c| c.from_state == "Alberta" && c.state == "Saskatchewan"),
        "{:?}",
        line.state_crossings()
    );
    let border = leg("winnipeg_mb_ca:grand_forks_nd_us");
    assert!(
        border
            .state_crossings()
            .iter()
            .any(|c| c.from_state == "Manitoba" && c.state == "North Dakota"),
        "{:?}",
        border.state_crossings()
    );
}

#[test]
fn the_lower_post_weave_reads_as_one_entry_into_yukon() {
    let weave = leg("fort_nelson_bc_ca:watson_lake_yt_ca");
    let lines: Vec<(f64, &str, &str)> = weave
        .state_crossings()
        .iter()
        .map(|c| (c.at_mi, c.state.as_str(), c.place.as_str()))
        .collect();
    assert_eq!(lines, vec![(311.7, "Yukon", "Lower Post")]);
    let miles: Vec<(&str, f64)> = weave
        .state_miles()
        .iter()
        .map(|m| (m.state.as_str(), m.miles))
        .collect();
    assert_eq!(miles[0], ("British Columbia", 311.7));
    assert_eq!(miles.len(), 2);
    // A true in-and-out of British Columbia is left alone.
    assert_eq!(
        leg("watson_lake_yt_ca:whitehorse_yt_ca")
            .state_crossings()
            .len(),
        2
    );
}

#[test]
fn the_weave_screen_changes_no_us_leg() {
    let world = world();
    let mut checked = 0;
    for path in std::fs::read_dir(data_dir().join("world_data/us/legs")).unwrap() {
        let text = std::fs::read_to_string(path.unwrap().path()).unwrap();
        let shard: serde_json::Value =
            serde_json::from_str(text.trim_start_matches('\u{feff}')).unwrap();
        for raw in shard["legs"].as_array().unwrap() {
            let (a, b) = (raw["from"].as_str().unwrap(), raw["to"].as_str().unwrap());
            let us = [a, b]
                .iter()
                .all(|s| world.cities.get(*s).is_some_and(|c| c.country == "US"));
            if !us {
                continue;
            }
            let stored = raw["corridor"]["state_crossings"]
                .as_array()
                .map_or(0, Vec::len);
            let leg = world.legs.iter().find(|l| l.a == a && l.b == b).unwrap();
            assert_eq!(leg.state_crossings().len(), stored, "{a} to {b}");
            checked += 1;
        }
    }
    assert!(checked > 1000, "only {checked} US legs checked");
}

#[test]
fn canadian_postings_are_driven_at_the_number_on_the_sign() {
    // Fort Nelson to Watson Lake is Canadian end to end: every read posting
    // is a whole km/h sign.
    let mut signs = 0;
    for s in leg("fort_nelson_bc_ca:watson_lake_yt_ca").speed_limits() {
        let Some(mph) = s.mph else { continue };
        let kmh = mph * ff_core::units::MILES_TO_KM;
        assert!(
            (kmh - kmh.round()).abs() < 1e-6,
            "mile {}: {kmh} km/h",
            s.at_mi
        );
        assert_eq!(kmh.round() % 10.0, 0.0, "mile {}: {kmh} km/h", s.at_mi);
        signs += 1;
    }
    assert!(signs > 5);
    // Alaska's side of the border stays in mph.
    let tok = leg("haines_junction_yt_ca:tok_ak_us");
    let alaska: Vec<f64> = tok
        .speed_limits()
        .iter()
        .filter(|s| s.at_mi > 202.0)
        .filter_map(|s| s.mph)
        .collect();
    assert!(!alaska.is_empty());
    assert!(alaska.iter().all(|mph| mph % 5.0 == 0.0), "{alaska:?}");
}

#[test]
fn a_canadian_ramp_advisory_keeps_its_km_h() {
    // A 50 km/h ramp sign is 31.07 mph, not the 50 mph the US bare-number
    // screen would make of it.
    let leg = leg("vancouver_bc_ca:abbotsford_bc_ca");
    let advisories: Vec<f64> = leg
        .interchanges()
        .iter()
        .flat_map(|x| [x.ramp_advisory_mph_forward, x.ramp_advisory_mph_backward])
        .flatten()
        .collect();
    assert!(!advisories.is_empty());
    assert!(advisories.iter().all(|mph| *mph < 40.0), "{advisories:?}");
}
