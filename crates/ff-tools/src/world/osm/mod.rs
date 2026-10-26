//! `osm-speed-limits`: re-read every leg's posted speed limits from a fresh
//! OpenStreetMap extract, and re-run the screen that names the short dips a
//! ramp or side street put in them.
//!
//! ```text
//! cargo tools osm-speed-limits --pbf us-latest.osm.pbf            # dry run
//! cargo tools osm-speed-limits --pbf us-latest.osm.pbf --write    # write it
//! cargo tools osm-speed-limits --pbf texas-latest.osm.pbf --only a:b;c:d
//! ```
//!
//! Inputs: Geofabrik extracts (`--pbf`, repeatable: one country file, or the
//! state files the map touches) and the route geometry archive
//! (`world_data/us/geometry/`), which is the road each leg drives. Any
//! `.osm.pbf` that covers the map works, such as the weekly planet file cut
//! to the lower 48 with `osmium extract`; `--snapshot` names it in the source
//! notes (default "Geofabrik extract"). Outputs, with `--write`:
//!
//! * `corridor.speed_limits` on each leg in `world_source/` (see [`bake`]),
//!   then `index-world` so `world_data/` follows;
//! * `world_data/us/gameplay/speed_limits.jsonl`, the same profiles before
//!   the anchor repair, gap markers included;
//! * `world_data/us/gameplay/limit_artifacts.jsonl`, the screen's named
//!   postings (see [`screen`]), which the loader folds.
//!
//! A leg is left as it is when its archive record is missing, when the
//! extract has no posted way along it (the sweep never regresses a leg to
//! the bare heuristic), or when a posting on it was set by hand (a `source`
//! that is not the OpenStreetMap read). Every such leg is counted in the
//! report. Re-bake `world.ffdata` afterwards.

pub mod bake;
pub mod geo;
pub mod pbf;
pub mod screen;

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use super::{load_world, save_world, source_root};
use crate::cli::{Args, Spec};
use crate::pyjson::{self, Object, Options, Value};
use crate::util::{self, Result};

const USAGE: &str = "usage: osm-speed-limits --pbf FILE [--pbf FILE ...] \
                     [--only from:to;from:to] [--extract-date YYYY-MM-DD] \
                     [--snapshot NAME] [--write]";

/// What `--snapshot` names when it is not given: the usual input.
const DEFAULT_SNAPSHOT: &str = "Geofabrik extract";

/// Every OpenStreetMap read this tool and the sweep before it wrote starts
/// so; anything else on a leg was set by hand and is kept.
const OSM_SOURCE_PREFIX: &str = "OpenStreetMap maxspeed tags";

fn world_source_note(snapshot: &str, date: &str) -> String {
    format!(
        "{OSM_SOURCE_PREFIX} on the corridor highway ways, read from the {snapshot} \
         of {date} and matched within 90 m of the archived route; normalized \
         to mph, maxspeed:hgv preferred where tagged."
    )
}

fn screen_source_note(snapshot: &str, date: &str) -> String {
    format!(
        "Derived: OpenStreetMap highway ways carrying maxspeed (maxspeed:hgv preferred), \
         {snapshot} of {date}, matched to the checked-in route geometry archive; a \
         posting is named when the nearest way along it is not the leg's highway posting \
         that limit. Thresholds calibrated on a hand-checked sample of 100 postings, \
         2026-10-17. Development-time screen, see `cargo tools osm-speed-limits`."
    )
}

fn leg_id(leg: &Value) -> String {
    let end = |k| leg.get(k).map(Value::py_str).unwrap_or_default();
    format!("{}:{}", end("from"), end("to"))
}

fn hand_set(leg: &Value) -> bool {
    leg.get("corridor")
        .and_then(|c| c.get("speed_limits"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .any(|s| {
            !matches!(s.get("mph"), None | Some(Value::Null))
                && !s
                    .get("source")
                    .map(Value::py_str)
                    .unwrap_or_default()
                    .starts_with(OSM_SOURCE_PREFIX)
        })
}

fn short_hash(payload: &str) -> String {
    let digest = Sha256::digest(payload.as_bytes());
    format!("sha256:{}", &hex::encode(digest)[..12])
}

fn jsonl(meta: Object, rows: &[Value]) -> String {
    let sorted = Options::compact().sorted();
    let payload: Vec<String> = rows.iter().map(|r| pyjson::dumps(r, &sorted)).collect();
    let payload = payload.join("\n");
    let mut meta = meta;
    meta.insert("data_version", Value::Str(short_hash(&payload)));
    let head = pyjson::dumps(&Value::Object(crate::pyobj! { "meta" => meta }), &sorted);
    if payload.is_empty() {
        head + "\n"
    } else {
        format!("{head}\n{payload}\n")
    }
}

/// Existing shard rows by leg id, so a partial run keeps the others.
fn shard_rows(path: &Path) -> Result<BTreeMap<String, Vec<Value>>> {
    let mut out: BTreeMap<String, Vec<Value>> = BTreeMap::new();
    let Some(text) = util::read_text_lf(path) else {
        return Ok(out);
    };
    for line in text.lines() {
        if line.trim().is_empty() || line.starts_with("{\"meta\"") {
            continue;
        }
        let row = pyjson::loads(line).map_err(|e| format!("{}: {e}", path.display()))?;
        let leg = row.get("leg").map(Value::py_str).unwrap_or_default();
        out.entry(leg).or_default().push(row);
    }
    Ok(out)
}

fn shard_row(id: &str, s: &bake::Sample, source: &str) -> Value {
    let mut row = crate::pyobj! { "leg" => id, "at_mi" => s.at_mi };
    match s.mph {
        Some(mph) => {
            row.insert("mph", Value::Int(i128::from(mph)));
            row.insert("hgv", Value::Bool(s.hgv));
            row.insert("source", Value::Str(source.to_string()));
        }
        None => {
            row.insert("mph", Value::Null);
            row.insert("hgv", Value::Bool(false));
        }
    }
    Value::Object(row)
}

#[derive(Default)]
struct Tally {
    selected: usize,
    changed: usize,
    unchanged: usize,
    no_reading: usize,
    hand_set: usize,
    no_archive: usize,
    repaired: usize,
    gaps: usize,
    hgv: usize,
    postings: usize,
}

pub fn main(argv: &[String]) -> Result<i32> {
    let args = Args::parse(
        argv,
        &Spec {
            values: &["pbf", "only", "extract-date", "snapshot"],
            flags: &["write"],
        },
    )
    .map_err(|e| format!("{e}\n{USAGE}"))?;
    let pbfs: Vec<PathBuf> = args.all("pbf").iter().map(PathBuf::from).collect();
    if pbfs.is_empty() {
        return Err(format!("--pbf is required\n{USAGE}"));
    }
    for path in &pbfs {
        if !path.is_file() {
            return Err(format!("no extract at {}", path.display()));
        }
    }
    let date = match args.value("extract-date") {
        Some(d) => d.to_string(),
        None => pbf::extract_date(&pbfs[0])?
            .ok_or("the extract's header carries no date; pass --extract-date YYYY-MM-DD")?,
    };
    let snapshot = args
        .value("snapshot")
        .unwrap_or(DEFAULT_SNAPSHOT)
        .to_string();
    let only: Option<HashSet<String>> = args.value("only").map(|o| {
        o.split(';')
            .filter(|p| p.contains(':'))
            .map(str::to_string)
            .collect()
    });

    let root = util::repo_root();
    let source = source_root();
    let gameplay = root.join("data/world_data/us/gameplay");
    let mut world = load_world(&source)?;
    let lines = geo::load_archive(&root.join("data/world_data/us/geometry"))?;
    println!(
        "{} archived leg lines; reading {} extract(s) dated {date}",
        lines.len(),
        pbfs.len()
    );
    let cells = geo::corridor_cells(lines.values());
    let ways = pbf::read_ways(&pbfs)?;
    let total_ways = ways.len();
    let index = geo::WayIndex::build(ways, &cells);
    println!(
        "{} posted-limit ways in the extracts, {} along the map's roads",
        total_ways,
        index.ways.len()
    );

    if index.ways.is_empty() {
        return Err(
            "no posted-limit way in the extracts lies along any leg; is this the right \
                    extract?"
                .into(),
        );
    }
    let note = world_source_note(&snapshot, &date);
    let mut tally = Tally::default();
    let mut full: BTreeMap<String, Vec<Value>> = shard_rows(&gameplay.join("speed_limits.jsonl"))?;
    let legs = world
        .get_mut("legs")
        .and_then(Value::as_array_mut)
        .ok_or("world_source has no legs")?;
    for leg in legs.iter_mut() {
        let id = leg_id(leg);
        let alt = {
            let (a, b) = id.split_once(':').unwrap_or_default();
            format!("{b}:{a}")
        };
        if only
            .as_ref()
            .is_some_and(|o| !o.contains(&id) && !o.contains(&alt))
        {
            continue;
        }
        tally.selected += 1;
        if hand_set(leg) {
            tally.hand_set += 1;
            continue;
        }
        let Some(line) = lines.get(&id) else {
            tally.no_archive += 1;
            continue;
        };
        let highway = leg.get("highway").map(Value::py_str).unwrap_or_default();
        let miles = leg.get("miles").and_then(Value::as_f64).unwrap_or(0.0);
        let samples = bake::bake_leg(&highway, line, miles, &index);
        full.insert(
            id.clone(),
            samples.iter().map(|s| shard_row(&id, s, &note)).collect(),
        );
        let mut profile = samples;
        tally.repaired += bake::repair(&highway, miles, &mut profile);
        if profile.is_empty() {
            tally.no_reading += 1;
            continue;
        }
        tally.postings += profile.iter().filter(|s| s.mph.is_some()).count();
        tally.gaps += profile.iter().filter(|s| s.mph.is_none()).count();
        tally.hgv += profile.iter().filter(|s| s.hgv).count();
        let rows = Value::Array(bake::world_rows(&profile, &note));
        let corridor = leg
            .as_object_mut()
            .ok_or_else(|| format!("{id} is not an object"))?
            .get_mut("corridor")
            .and_then(Value::as_object_mut)
            .ok_or_else(|| format!("{id} has no corridor"))?;
        if corridor.get("speed_limits") == Some(&rows) {
            tally.unchanged += 1;
        } else {
            tally.changed += 1;
            corridor.insert("speed_limits", rows);
        }
    }
    println!(
        "{} legs read: {} changed, {} the same, {} with no posted way kept as they were, \
         {} set by hand kept, {} with no archived road kept.",
        tally.selected,
        tally.changed,
        tally.unchanged,
        tally.no_reading,
        tally.hand_set,
        tally.no_archive
    );
    println!(
        "New profiles: {} postings ({} from truck limits), {} coverage gaps; the anchor \
         repair dropped {}.",
        tally.postings, tally.hgv, tally.gaps, tally.repaired
    );

    // The screen reads the profiles just baked, on every leg.
    let views: Vec<screen::LegView> = world
        .get("legs")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|leg| {
            Some(screen::LegView {
                id: leg_id(leg),
                highway: leg.get("highway").map(Value::py_str).unwrap_or_default(),
                miles: leg.get("miles").and_then(Value::as_f64)?,
                corridor: leg.get("corridor")?,
            })
        })
        .collect();
    let (artifacts, counts) = screen::screen(&views, &lines, &index);
    println!(
        "Screen: {} short drop-and-return postings checked, {} on their highway, {} mixed, \
         {} named ({} kept by a nearby place, {} fold at load); {} on legs with no archived \
         road not checked.",
        counts.candidates,
        counts.on_highway,
        counts.mixed,
        counts.named,
        counts.place_guarded,
        counts.folds,
        counts.skipped_candidates
    );

    if !args.flag("write") {
        println!("(dry run; pass --write to update the map data)");
        return Ok(0);
    }
    if only.is_some() {
        println!("The screen covers the whole map, so it is written only on a full run.");
    }
    save_world(&world, &source)?;
    super::index::main(&[])?;

    let names: Vec<Value> = pbfs
        .iter()
        .map(|p| {
            Value::Str(
                p.file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default(),
            )
        })
        .collect();
    let all_rows: Vec<Value> = full.into_values().flatten().collect();
    let speed_meta = crate::pyobj! {
        "schema" => 1i64,
        "layer" => "speed_limits",
        "kind" => "read",
        "source" => format!("OpenStreetMap via {snapshot} of {date} (ODbL, (c) OpenStreetMap contributors)"),
        "params" => crate::pyobj! {
            "match_corridor_m" => bake::MATCH_CORRIDOR_M,
            "sample_m" => bake::SAMPLE_M,
            "speed_gap_mi" => bake::SPEED_GAP_MI,
        },
    };
    util::write_text(
        &gameplay.join("speed_limits.jsonl"),
        &jsonl(speed_meta, &all_rows),
    )?;

    if only.is_none() {
        let mut reasons = Object::new();
        for (k, v) in screen::REASONS {
            reasons.insert(*k, Value::Str((*v).to_string()));
        }
        let fold_ratio = if counts.candidates == 0 {
            0.0
        } else {
            ff_core::pyfmt::round_py_n(counts.folds as f64 / counts.candidates as f64, 3)
        };
        let meta = crate::pyobj! {
            "schema" => 1i64,
            "layer" => "limit_artifacts",
            "kind" => "derived",
            "source" => screen_source_note(&snapshot, &date),
            "reasons" => reasons,
            "params" => crate::pyobj! {
                "max_run_mi" => screen::MAX_RUN_MI,
                "step_mi" => screen::STEP_MI,
                "near_m" => screen::NEAR_M,
                "on_highway_min" => screen::ON_HIGHWAY_MIN,
                "verdict_min" => screen::VERDICT_MIN,
                "place_guard_mi" => ff_core::data::world_constants::LIMIT_ARTIFACT_PLACE_GUARD_MI,
            },
            "inputs" => crate::pyobj! { "extracts" => Value::Array(names), "extract_date" => date.as_str() },
            "counts" => crate::pyobj! {
                "candidates" => counts.candidates,
                "on_highway" => counts.on_highway,
                "mixed" => counts.mixed,
                "named" => counts.named,
                "place_guarded" => counts.place_guarded,
                "folds" => counts.folds,
                "skipped_candidates" => counts.skipped_candidates,
                "fold_ratio" => fold_ratio,
            },
        };
        util::write_text(
            &gameplay.join("limit_artifacts.jsonl"),
            &jsonl(meta, &artifacts),
        )?;
    }
    println!(
        "Wrote the world source, world_data/ and the gameplay layers. Re-bake world.ffdata next."
    );
    Ok(0)
}
