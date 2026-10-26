//! `osm-exits`: re-read every leg's exits and their ramps from a fresh
//! OpenStreetMap extract.
//!
//! ```text
//! cargo tools osm-exits --pbf us-highways.osm.pbf --extract-date 2026-10-19 \
//!     --snapshot "OpenStreetMap planet file"            # dry run
//! cargo tools osm-exits --pbf us-highways.osm.pbf ... --write
//! cargo tools osm-exits --pbf texas-latest.osm.pbf --only a:b;c:d
//! ```
//!
//! A port of the deleted `tools/build_interchanges.py` (exits, signs, mile)
//! and its `--ramp-controls` mode (`tools/build_interchanges_rampcontrols.py`:
//! far end, terminal control, advisory speed, ramp length and the terminal
//! node the street chains start from), in one pass over one extract so the
//! exits and their ramps can no longer come from different OSM sets. See
//! [`discover`] and [`ramps`] for what changed from the Python.
//!
//! Each Interstate leg, and each other leg that already carries exits, is
//! read against its archived road (`world_data/us/geometry/`). A leg with no
//! archived road keeps what it has. What the run keeps from the old data:
//!
//! * an exit record whose `source` is not this tool's read (or the Python's
//!   it replaces) was set by hand, and stays; so does any ramp fact on a
//!   re-read exit whose source is not ours (the hand-set rule of
//!   `osm-speed-limits`);
//! * a ramp terminal a baked street chain starts from (a facility's exit
//!   chain or a road stop's approach) when the new read ends that ramp
//!   somewhere else: the chain is keyed by the node, so moving it would cut
//!   the chain off. Its source says so, and the report counts them, until
//!   the street chains are re-baked from the same extract;
//! * road stops stay linked to their exit (`interchange_mi`): a stop whose
//!   exit is re-read follows it to its new mile, and one whose exit is gone
//!   loses the link and keeps its signed number.
//!
//! With `--write` it writes the world source, the `ramp_length_bake` meta
//! and then runs `index-world`. Re-bake `world.ffdata` afterwards.

pub mod discover;
pub mod ramps;
pub mod read;
pub mod record;
pub mod topo;

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::LazyLock;

use regex::Regex;

use super::geo;
use super::pbf;
use crate::cli::{Args, Spec};
use crate::pyjson::{self, Value};
use crate::util::Result;
use crate::world::{load_world, save_world, source_root};
use discover::{DiscoverCounts, JunctionIndex, Snapper};
use ramps::RampCounts;
use record::{Carry, Sources};

const USAGE: &str = "usage: osm-exits --pbf FILE [--pbf FILE ...] [--only from:to;from:to] \
                     [--extract-date YYYY-MM-DD] [--snapshot NAME] [--write]";

const DEFAULT_SNAPSHOT: &str = "Geofabrik extract";

static INTERSTATE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^I-\d+$").unwrap());

fn leg_id(leg: &Value) -> String {
    let end = |k| leg.get(k).map(Value::py_str).unwrap_or_default();
    format!("{}:{}", end("from"), end("to"))
}

/// Every OSM node a baked street chain starts from: the key a re-read must
/// not move under it.
fn chain_terminals(root: &std::path::Path, world: &pyjson::Object) -> Result<HashSet<i64>> {
    fn walk(v: &Value, out: &mut HashSet<i64>) {
        match v {
            Value::Object(o) => {
                if let Some(n) = o.get("terminal_node").and_then(Value::as_i64) {
                    out.insert(n);
                }
                for x in o.values() {
                    walk(x, out);
                }
            }
            Value::Array(a) => a.iter().for_each(|x| walk(x, out)),
            _ => {}
        }
    }
    let mut out = HashSet::new();
    let approaches = root.join("data/facility_approaches.json");
    if approaches.is_file() {
        walk(&pyjson::load_path(&approaches)?, &mut out);
    }
    for leg in world
        .get("legs")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        for stop in leg
            .get("stops")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if let Some(chains) = stop.get("approach_chains") {
                walk(chains, &mut out);
            }
        }
    }
    Ok(out)
}

#[derive(Default)]
struct Tally {
    selected: usize,
    not_eligible: usize,
    no_archive: usize,
    changed: usize,
    unchanged: usize,
    cleared: Vec<String>,
    old_exits: usize,
    new_exits: usize,
    matched: usize,
    hand_set: usize,
    carry: Carry,
    stops_moved: usize,
    stops_kept: usize,
    stops_unlinked: usize,
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

    let root = crate::util::repo_root();
    let source = source_root();
    let mut world = load_world(&source)?;
    let lines = geo::load_archive(&root.join("data/world_data/us/geometry"))?;
    let terminals_in_use = chain_terminals(&root, &world)?;
    println!(
        "{} archived leg lines, {} street-chain terminals; reading {} extract(s) dated {date}",
        lines.len(),
        terminals_in_use.len(),
        pbfs.len()
    );
    let mut net = read::read_network(&pbfs)?;
    let junctions = std::mem::take(&mut net.junctions);
    let neighbours = std::mem::take(&mut net.junction_neighbours);
    let topo = topo::Topo::build(net);
    let index = JunctionIndex::new(&junctions);
    println!(
        "Ramp graph: {} gores over {} ramp ways ({} with no oneway tag, read as drawn forward)",
        topo.gore_count,
        topo.links.len(),
        topo.untagged_oneway
    );
    if topo.gore_count == 0 || junctions.is_empty() {
        return Err("no motorway exit in the extracts; is this the right extract?".into());
    }

    let sources = Sources::new(&snapshot, &date);
    let mut tally = Tally::default();
    let mut found = DiscoverCounts::default();
    let mut ramp_counts = RampCounts::default();
    let legs = world
        .get_mut("legs")
        .and_then(Value::as_array_mut)
        .ok_or("world_source has no legs")?;
    for leg in legs.iter_mut() {
        let id = leg_id(leg);
        if only.as_ref().is_some_and(|o| !o.contains(&id)) {
            continue;
        }
        tally.selected += 1;
        let highway = leg.get("highway").map(Value::py_str).unwrap_or_default();
        let old: Vec<Value> = leg
            .get("corridor")
            .and_then(|c| c.get("interchanges"))
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        if !INTERSTATE.is_match(&highway) && old.is_empty() {
            tally.not_eligible += 1;
            continue;
        }
        let Some(line) = lines.get(&id) else {
            tally.no_archive += 1;
            continue;
        };
        let miles = leg.get("miles").and_then(Value::as_f64).unwrap_or(0.0);
        let snapper = Snapper::new(line, miles);
        let candidates = index.along(&snapper);
        let exits = discover::exit_nodes(
            &candidates,
            &neighbours,
            &topo.locs,
            &snapper,
            miles,
            &mut found,
        );
        let fresh = record::read_leg(&topo, &snapper, exits, &highway, &sources, &mut ramp_counts);
        let merged = record::merge(&old, fresh, &terminals_in_use, &sources, &mut tally.carry);
        tally.old_exits += old.len();
        tally.new_exits += merged.records.len();
        tally.matched += merged.matched;
        tally.hand_set += merged.hand_set;
        if merged.records.is_empty() && !old.is_empty() {
            tally.cleared.push(format!("{id} ({} exits)", old.len()));
        }
        // Road stops follow their exit.
        if let Some(stops) = leg
            .as_object_mut()
            .and_then(|o| o.get_mut("stops"))
            .and_then(Value::as_array_mut)
        {
            for stop in stops.iter_mut() {
                let Some(stop) = stop.as_object_mut() else {
                    continue;
                };
                let Some(mi) = stop.get("interchange_mi").and_then(Value::as_f64) else {
                    continue;
                };
                match merged
                    .moved
                    .iter()
                    .find(|(old_mi, _)| (old_mi - mi).abs() <= 1e-6)
                {
                    Some((_, Some(new_mi))) => {
                        if (new_mi - mi).abs() <= 1e-6 {
                            tally.stops_kept += 1;
                        } else {
                            stop.insert("interchange_mi", Value::Float(*new_mi));
                            tally.stops_moved += 1;
                        }
                    }
                    Some((_, None)) => {
                        stop.remove("interchange_mi");
                        tally.stops_unlinked += 1;
                    }
                    None => tally.stops_kept += 1,
                }
            }
        }
        let rows = Value::Array(merged.records);
        let corridor = leg
            .as_object_mut()
            .ok_or_else(|| format!("{id} is not an object"))?
            .get_mut("corridor")
            .and_then(Value::as_object_mut)
            .ok_or_else(|| format!("{id} has no corridor"))?;
        if corridor.get("interchanges") == Some(&rows) {
            tally.unchanged += 1;
        } else {
            tally.changed += 1;
            corridor.insert("interchanges", rows);
        }
    }

    report(&tally, &found, &ramp_counts);
    let meta = record::layer_meta(&world, &sources, &ramp_counts, &tally.carry, &pbfs);
    world.insert("ramp_length_bake", Value::Object(meta));
    if !args.flag("write") {
        println!("(dry run; pass --write to update the map data)");
        return Ok(0);
    }
    save_world(&world, &source)?;
    super::super::index::main(&[])?;
    println!("Wrote the world source and world_data/. Re-bake world.ffdata next.");
    Ok(0)
}

fn report(t: &Tally, found: &DiscoverCounts, r: &RampCounts) {
    println!(
        "{} legs read: {} changed, {} the same; {} not Interstates and with no exits, {} with \
         no archived road, kept as they were.",
        t.selected, t.changed, t.unchanged, t.not_eligible, t.no_archive
    );
    println!(
        "Exits: {} before, {} after; {} re-read exits matched an old one by number and mile, \
         {} set by hand kept.",
        t.old_exits, t.new_exits, t.matched, t.hand_set
    );
    println!(
        "Junctions: {} within {} m of a leg's road; dropped: {} on no motorway or trunk way, \
         {} on a crossing road, {} within a mile of a leg end; {} exit numbers seen twice on one leg and kept as two \
         exits.",
        found.near_line,
        discover::CORRIDOR_M,
        found.off_mainline,
        found.crossing_road,
        found.at_leg_end,
        found.split_numbers
    );
    if !t.cleared.is_empty() {
        println!(
            "{} legs found no exit junction on their road and were cleared: {}",
            t.cleared.len(),
            t.cleared.join(", ")
        );
    }
    println!(
        "Ramps over {} exits: {} merge onto a motorway ({} given control none, derived), {} \
         end at a surface road, {} had no gore in range.",
        r.exits, r.far_motorway, r.none_derived, r.far_surface, r.no_gore
    );
    println!(
        "  Controls: {} read at the walked terminal ({} exits end at a yield or roundabout), \
         {} exit-wide reads, {} read signals or stops at a proven merge (kept, no far end).",
        r.terminal_reads, r.yieldish, r.wide_reads, r.contradictions
    );
    println!(
        "  Ramp ends: {} exits both ways, {} one way. Lengths: {} gores measured, {} too \
         short and {} too long dropped, {} reached no end.",
        r.both_terminals, r.one_terminal, r.gores_measured, r.too_short, r.too_long, r.unmeasured
    );
    println!(
        "Kept: {} ramp facts set by hand; {} ramp ends a street chain starts from, where the \
         new read ends elsewhere ({} of them on exits the read no longer has).",
        t.carry.hand_set_facts, t.carry.chain_terminals_kept, t.carry.chain_terminals_lost
    );
    println!(
        "Road stops: {} followed their exit to a new mile, {} unchanged, {} lost the link \
         (their exit is gone).",
        t.stops_moved, t.stops_kept, t.stops_unlinked
    );
}
