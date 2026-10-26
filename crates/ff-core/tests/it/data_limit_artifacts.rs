//! The speed-limit artifact fold against the real world.
//!
//! `tools/screen_limit_artifacts.py` names short drop-and-return postings that
//! OpenStreetMap shows off the leg's own highway (a ramp, a side street), and
//! the loader folds each one unless a village or town sits beside it. These
//! tests round-trip that list against the loaded data, so a stale list or a
//! loader that stopped folding fails here instead of drifting quietly.

use std::collections::HashMap;

use crate::data_support::world;
use ff_core::data::data_resources::read_data_text;
use ff_core::data::world_constants::{
    LIMIT_ARTIFACT_PLACE_CHECKPOINTS, LIMIT_ARTIFACT_PLACE_GUARD_MI, LIMIT_EXPLAINING_CATEGORIES,
};
use ff_core::data::world_models::Leg;
use serde_json::Value;

fn list() -> (Value, Vec<Value>) {
    let text = read_data_text("world_data/us/gameplay/limit_artifacts.jsonl")
        .expect("limit_artifacts.jsonl ships with the world data");
    let mut meta = Value::Null;
    let mut rows = Vec::new();
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        let row: Value = serde_json::from_str(line).expect("a JSON row");
        match row.get("meta") {
            Some(m) => meta = m.clone(),
            None => rows.push(row),
        }
    }
    (meta, rows)
}

fn legs_by_key() -> HashMap<String, &'static Leg> {
    world()
        .legs
        .iter()
        .map(|leg| (format!("{}:{}", leg.a, leg.b), leg.as_ref()))
        .collect()
}

fn has_posting(leg: &Leg, at_mi: f64, mph: f64) -> bool {
    leg.speed_limits()
        .iter()
        .any(|s| s.mph == Some(mph) && (s.at_mi - at_mi).abs() < 0.005)
}

fn place_beside(leg: &Leg, start: f64, end: f64) -> bool {
    let near = |at: f64| {
        at >= start - LIMIT_ARTIFACT_PLACE_GUARD_MI && at <= end + LIMIT_ARTIFACT_PLACE_GUARD_MI
    };
    leg.landmarks()
        .iter()
        .any(|lm| LIMIT_EXPLAINING_CATEGORIES.contains(&lm.category.as_str()) && near(lm.at_mi))
        || leg.checkpoints().iter().any(|cp| {
            LIMIT_ARTIFACT_PLACE_CHECKPOINTS.contains(&cp.checkpoint_type.as_str())
                && near(cp.at_mi)
        })
}

#[test]
fn the_loader_folds_as_many_postings_as_the_screen_counted() {
    let (meta, rows) = list();
    assert!(!rows.is_empty());
    let expected = meta["counts"]["folds"].as_u64().expect("meta counts.folds");
    let folded: usize = world()
        .legs
        .iter()
        .map(|leg| leg.corridor().speed_limits_folded)
        .sum();
    assert_eq!(folded as u64, expected);
}

#[test]
fn every_named_posting_is_folded_or_has_a_place_beside_it() {
    let (_, rows) = list();
    let legs = legs_by_key();
    for row in &rows {
        let key = row["leg"].as_str().unwrap();
        let leg = legs
            .get(key)
            .unwrap_or_else(|| panic!("{key} is not a leg"));
        let at = row["at_mi"].as_f64().unwrap();
        let mph = row["mph"].as_f64().unwrap();
        let run = row["run_mi"].as_f64().unwrap();
        if has_posting(leg, at, mph) {
            assert!(
                place_beside(leg, at, at + run),
                "{key} mile {at}: named, nothing beside it, and still posted"
            );
        }
    }
}

#[test]
fn the_ramps_from_the_sample_are_gone_and_its_towns_stay() {
    let legs = legs_by_key();
    let (_, rows) = list();
    let named = |key: &str, at: f64| {
        rows.iter()
            .any(|r| r["leg"] == key && (r["at_mi"].as_f64().unwrap() - at).abs() < 0.005)
    };
    for (key, at, mph) in KNOWN_ARTIFACTS {
        assert!(named(key, *at), "{key} mile {at} is no longer named");
        assert!(
            !has_posting(legs[*key], *at, *mph),
            "{key} mile {at}: the {mph} posting survived"
        );
    }
    for (key, at, mph) in KNOWN_REAL {
        assert!(
            has_posting(legs[*key], *at, *mph),
            "{key} mile {at}: the real {mph} zone was folded"
        );
    }
}

/// Artifacts from the hand-checked sample (2026-10-17) that the fold removes:
/// the 15 mph loop US-95 picks up in Las Vegas, a 45 ramp on I-80 out of
/// Reno, and a 35 side street on OR-99W out of Portland.
const KNOWN_ARTIFACTS: &[(&str, f64, f64)] = &[
    ("reno_nv_us:las_vegas_nv_us", 434.7, 15.0),
    ("reno_nv_us:fernley_nv_us", 5.4, 45.0),
    ("portland_or_us:newberg_or_us", 16.0, 35.0),
];
/// Real town zones from the same sample that must stay: Goldfield and Beatty
/// on US-95, and Plummer on US-95 in Idaho, which the screen names (the map
/// tags the highway there with a different limit) and the town beside it keeps.
const KNOWN_REAL: &[(&str, f64, f64)] = &[
    ("reno_nv_us:las_vegas_nv_us", 259.6, 25.0),
    ("reno_nv_us:las_vegas_nv_us", 326.0, 25.0),
    ("lewiston_id_us:coeur_d_alene_id_us", 82.2, 25.0),
];
