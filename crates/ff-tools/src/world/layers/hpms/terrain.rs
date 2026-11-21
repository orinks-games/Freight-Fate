//! `hpms-terrain`: the terrain class HPMS gives the road a leg drives
//! (`corridor.hpms_terrain`), which the curve screen reads to tell a real
//! mountain bend from a bad vertex.
//!
//! A port of the deleted `tools/build_terrain_type.py`. HPMS classes road
//! sections level, rolling or mountainous; a leg crosses many, so the leg
//! takes the most common class among the sections (at most 800) that
//! intersect its archived road's bounding box. The class is read; that one
//! value stands for the leg is ours, and the source says so.

use std::collections::HashMap;

use super::Client;
use crate::cli::{Args, Spec};
use crate::pyjson::Value;
use crate::util::Result;
use crate::world::layers::{corridor_mut, leg_id, leg_miles, Only, Route, Run};

const USAGE: &str = "usage: hpms-terrain [--only from:to;from:to] [--refresh] [--write]";

pub fn source(accessed: &str) -> String {
    format!(
        "{SOURCE_HEAD} Accessed {accessed}: https://www.fhwa.dot.gov/policyinformation/hpms.cfm"
    )
}

const SOURCE_HEAD: &str = "FHWA Highway Performance Monitoring System (HPMS) Terrain_Type, read \
     from the national ArcGIS Living Atlas layer (Federal User Community hpms_v2_view). The class \
     is HPMS's own; DERIVED here only in that one value stands for a whole leg -- the modal class \
     over the HPMS sections intersecting the leg's bounding box, since HPMS classifies sections \
     and a leg crosses many.";

/// What a Canadian leg's terrain class says it is.
pub const CANADA_SOURCE: &str =
    "DERIVED, not read: Canada has no national inventory like HPMS, so \
     the class is the terrain the leg's own grade profile spends the most miles in (the \
     reroute-enrich rule: sustained grade of 5 percent, or 4 percent with 1,000 ft of relief \
     within 5 miles, is mountain; 1.5 percent or 400 ft of relief is hills; else flat), mapped \
     flat to level, hills to rolling and mountain to mountainous. It reads the same elevation \
     as the grade, so it is no independent check on it.";

/// The HPMS class `(type, name)` for the terrain a leg's grade profile
/// spends the most miles in, or `None` with no profile.
pub fn relief_terrain(leg: &Value) -> Option<(i64, &'static str)> {
    let mut miles: Vec<(&str, f64)> = Vec::new();
    for seg in leg
        .get("corridor")
        .and_then(|c| c.get("grade_segments"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let terrain = seg.get("terrain").and_then(Value::as_str).unwrap_or("");
        let span = seg.get("end_mi").and_then(Value::as_f64).unwrap_or(0.0)
            - seg.get("start_mi").and_then(Value::as_f64).unwrap_or(0.0);
        let kind = match terrain {
            "flat" => "flat",
            "hills" => "hills",
            "mountain" => "mountain",
            _ => continue,
        };
        match miles.iter_mut().find(|(k, _)| *k == kind) {
            Some(slot) => slot.1 += span.max(0.0),
            None => miles.push((kind, span.max(0.0))),
        }
    }
    let (best, _) = miles
        .into_iter()
        .filter(|(_, m)| *m > 0.0)
        .max_by(|a, b| a.1.total_cmp(&b.1))?;
    Some(match best {
        "flat" => (1, "level"),
        "hills" => (2, "rolling"),
        _ => (3, "mountainous"),
    })
}

pub fn terrain_name(kind: i64) -> Option<&'static str> {
    match kind {
        1 => Some("level"),
        2 => Some("rolling"),
        3 => Some("mountainous"),
        _ => None,
    }
}

/// The most common class, the first seen winning a tie (as `Counter`).
pub fn modal(kinds: &[i64]) -> Option<(i64, usize)> {
    let mut counts: Vec<(i64, usize)> = Vec::new();
    for &k in kinds {
        match counts.iter_mut().find(|c| c.0 == k) {
            Some(c) => c.1 += 1,
            None => counts.push((k, 1)),
        }
    }
    let total = kinds.len();
    let mut best: Option<(i64, usize)> = None;
    for c in counts {
        if best.is_none_or(|b| c.1 > b.1) {
            best = Some(c);
        }
    }
    best.map(|b| (b.0, total))
}

fn query(client: &Client, route: &Route) -> Result<Vec<i64>> {
    let b = route.bounds();
    let v = client.query(&[
        ("where", "Terrain_Type IS NOT NULL".into()),
        ("geometry", super::envelope((b.1, b.0, b.3, b.2))),
        ("geometryType", "esriGeometryEnvelope".into()),
        ("inSR", "4326".into()),
        ("outSR", "4326".into()),
        ("spatialRel", "esriSpatialRelIntersects".into()),
        ("outFields", "Terrain_Type".into()),
        ("returnGeometry", "false".into()),
        ("resultRecordCount", "800".into()),
        ("f", "json".into()),
    ])?;
    Ok(v.get("features")
        .and_then(|f| f.as_array())
        .into_iter()
        .flatten()
        .filter_map(|f| f.pointer("/attributes/Terrain_Type")?.as_i64())
        .filter(|k| terrain_name(*k).is_some())
        .collect())
}

pub fn main(argv: &[String]) -> Result<i32> {
    let args = Args::parse(
        argv,
        &Spec {
            values: &["only"],
            flags: &["write", "refresh"],
        },
    )
    .map_err(|e| format!("{e}\n{USAGE}"))?;
    let only = Only::parse(&args);
    let client = Client::new(args.flag("refresh"));
    let accessed = super::today();
    let mut run = Run::load()?;
    let archive = std::mem::take(&mut run.archive);
    let world = run.world.clone();
    let mut canadian = 0usize;
    let mut tally: HashMap<&str, usize> = HashMap::new();
    let (mut changed, mut same, mut none, mut failed, mut no_archive) = (0, 0, 0, 0, 0);
    for leg in run.legs_mut()?.iter_mut() {
        let id = leg_id(leg);
        if !only.wants(&id) {
            continue;
        }
        let Some(line) = archive.get(&id) else {
            no_archive += 1;
            continue;
        };
        if crate::world::country::leg_touches(&world, leg, "CA") {
            // HPMS is the US federal inventory and holds no Canadian road.
            let Some((kind, name)) = relief_terrain(leg) else {
                none += 1;
                continue;
            };
            *tally.entry(name).or_default() += 1;
            canadian += 1;
            if only.is_some() {
                println!("  {id}: {name} (derived from its own grade profile)");
            }
            let row = Value::Object(crate::pyobj! {
                "type" => kind,
                "name" => name,
                "sections" => 0i64,
                "source" => CANADA_SOURCE,
            });
            let corridor = corridor_mut(leg)?;
            if corridor.get("hpms_terrain") == Some(&row) {
                same += 1;
            } else {
                changed += 1;
                corridor.insert("hpms_terrain", row);
            }
            continue;
        }
        let route = Route::new(line, leg_miles(leg));
        let kinds = match query(&client, &route) {
            Ok(k) => k,
            Err(e) => {
                println!("  {id}: skipped, {e}");
                failed += 1;
                continue;
            }
        };
        let Some((kind, sections)) = modal(&kinds) else {
            none += 1;
            continue;
        };
        let name = terrain_name(kind).unwrap_or("level");
        *tally.entry(name).or_default() += 1;
        if only.is_some() {
            println!("  {id}: {name} ({sections} sections)");
        }
        let row = Value::Object(crate::pyobj! {
            "type" => kind,
            "name" => name,
            "sections" => sections,
            "source" => source(&accessed),
        });
        let corridor = corridor_mut(leg)?;
        if corridor.get("hpms_terrain") == Some(&row) {
            same += 1;
        } else {
            changed += 1;
            corridor.insert("hpms_terrain", row);
        }
    }
    run.archive = archive;
    let mut kinds: Vec<_> = tally.into_iter().collect();
    kinds.sort();
    let kinds: Vec<String> = kinds.iter().map(|(k, n)| format!("{n} {k}")).collect();
    println!(
        "{changed} legs changed, {same} the same ({}); {none} with no HPMS terrain and {failed} \
         failed kept as they were, {no_archive} with no archived road kept. {} new answer(s) \
         fetched.",
        kinds.join(", "),
        client.fetched.load(std::sync::atomic::Ordering::Relaxed)
    );
    if canadian > 0 {
        println!(
            "{canadian} Canadian legs DERIVED from their own grade profile (HPMS covers only US \
             roads): their terrain class is not a second source for the level-ground screen."
        );
    }
    run.finish(args.flag("write"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_canadian_leg_takes_the_terrain_it_spends_most_miles_in() {
        let leg = crate::pyjson::loads(
            r#"{"corridor": {"grade_segments": [
                {"start_mi": 0.0, "end_mi": 10.0, "terrain": "flat"},
                {"start_mi": 10.0, "end_mi": 14.0, "terrain": "mountain"},
                {"start_mi": 14.0, "end_mi": 30.0, "terrain": "hills"}]}}"#,
        )
        .unwrap();
        assert_eq!(relief_terrain(&leg), Some((2, "rolling")));
        let bare = crate::pyjson::loads(r#"{"corridor": {}}"#).unwrap();
        assert_eq!(relief_terrain(&bare), None);
    }

    #[test]
    fn the_modal_class_wins_and_a_tie_keeps_the_first() {
        assert_eq!(modal(&[1, 3, 3, 2]), Some((3, 4)));
        assert_eq!(modal(&[2, 1, 1, 2]), Some((2, 4)));
        assert_eq!(modal(&[]), None);
    }
}
