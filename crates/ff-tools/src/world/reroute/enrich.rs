//! `reroute-enrich`: rebuild what a rerouted leg dropped, and refuse to call
//! it finished while a layer is missing.
//!
//! Every layer is read again, against the NEW road, from the source that
//! made it in the first place:
//!
//! | layer | from |
//! |---|---|
//! | curves (`gameplay/curves.jsonl`) | the archived road's own curvature; `connector` from the class of the OpenStreetMap road under each apex |
//! | `grade_segments`, `elevation_samples` | Valhalla `/height` every 0.1 mile along the archived road |
//! | `state_miles`, `state_crossings` | OpenStreetMap state boundary relations |
//! | runaway ramps (`gameplay/ramps.jsonl`) | the checked-in escape-ramp cache |
//! | `interchanges` | `osm-exits`, same extract |
//! | `speed_limits` and the limit screen | `osm-speed-limits`, same extract |
//!
//! Landmarks, lane counts and traffic counts were carried by `reroute-leg`
//! where the road did not change; their builders are not ported yet, so on
//! the new stretch they stay empty until they are. They are reported here,
//! not judged.
//!
//! THE REFUSAL. A builder that finds nothing and exits zero is how this job
//! has failed before: the Python interchange builder once kept 0 of 59,924
//! ramp nodes and reported success. So the run ends by comparing each
//! rebuilt layer with what the leg carried before (the committed copy, from
//! git), and a layer that came back empty, or under half its old size, fails
//! the run by name. The grade profile is judged by coverage instead: its row
//! count follows how noisy the ground reads, but it must describe the whole
//! leg with no hole.

use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;

use ff_core::data::curves::{classify_connector, CONNECTOR_CORRIDOR_M};
use ff_core::pyfmt::round_py_n;

use super::super::osm::geo::{self, LegLine, M_PER_MI};
use super::states::{self, Crossing};
use super::{gameplay_dir, grades, leg_id, roads, shape, shard_path, valhalla};
use crate::cli::{Args, Spec};
use crate::pyjson::{self, Object, Value};
use crate::util::{self, Result};
use crate::world::{load_world, save_world, source_root};

const USAGE: &str = "usage: reroute-enrich (--leg from:to ... | --all-pending) --pbf FILE \
                     [--pbf FILE ...] [--extract-date YYYY-MM-DD] [--snapshot NAME] [--write]";

/// Layers this pass rebuilds, judged by count.
const REBUILT: &[&str] = &["speed_limits", "state_miles", "interchanges", "curves"];
/// Carried across by `reroute-leg`; reported, not judged.
const CARRIED: &[&str] = &["landmarks", "lane_segments", "traffic_aadt", "restrictions"];
/// How the made-of class is read along the leg.
const CLASS_STEP_M: f64 = 160.0;
/// Escape-ramp centroids farther than this are not on the road.
const RAMP_MATCH_M: f64 = 160.0;
/// Same-side ramps closer than this are one ramp.
const RAMP_DEDUP_MI: f64 = 0.3;

struct Picked {
    index: usize,
    id: String,
    coords: Vec<(f64, f64)>,
    miles: f64,
}

/// The class of road the leg spends most of its miles on, read every
/// [`CLASS_STEP_M`] from the nearest way within the connector corridor.
fn made_of(net: &roads::Network, coords: &[(f64, f64)], cum: &[f64]) -> Option<String> {
    let mut tally: BTreeMap<String, usize> = BTreeMap::new();
    let mut next = 0.0;
    for (i, &(lon, lat)) in coords.iter().enumerate() {
        if cum[i] < next {
            continue;
        }
        next = cum[i] + CLASS_STEP_M;
        if let Some((road, _)) = net.nearest(lat, lon, CONNECTOR_CORRIDOR_M) {
            let class = roads::rule_class(&road.class);
            if !class.ends_with("_link") {
                *tally.entry(class.to_string()).or_default() += 1;
            }
        }
    }
    tally.into_iter().max_by_key(|(_, n)| *n).map(|(c, _)| c)
}

fn curve_rows(
    id: &str,
    p: &Picked,
    net: &roads::Network,
) -> (Vec<Value>, BTreeMap<&'static str, usize>) {
    let cum = shape::cumulative_m(&p.coords);
    let raw = cum[cum.len() - 1] / M_PER_MI;
    let scale = if raw > 0.0 { p.miles / raw } else { 1.0 };
    let curv = shape::analyse(&p.coords, &cum);
    let corridor = made_of(net, &p.coords, &cum);
    let hi = p.miles - shape::CONNECTOR_WINDOW_MI;
    let mut why: BTreeMap<&'static str, usize> = BTreeMap::new();
    let rows = curv
        .curves
        .iter()
        .map(|c| {
            let start = shape::at_mi(&cum, c.a, scale);
            let end = shape::at_mi(&cum, c.b, scale);
            let (lon, lat) = p.coords[c.apex];
            let near = net.nearest(lat, lon, CONNECTOR_CORRIDOR_M + 1e-9);
            let (read, reason) = classify_connector(
                near.map(|(_, d)| d),
                near.map(|(r, _)| roads::rule_class(&r.class)),
                corridor.as_deref(),
            );
            let window = end <= shape::CONNECTOR_WINDOW_MI || start >= hi;
            let mut row = crate::pyobj! {
                "leg" => id,
                "seq" => c.seq,
                "start_mi" => start,
                "end_mi" => end,
                "apex_mi" => shape::at_mi(&cum, c.apex, scale),
                "direction" => c.direction.to_string(),
                "min_radius_ft" => c.min_radius_ft,
                "deflection_deg" => c.deflection_deg,
                "advisory_mph" => c.advisory_mph,
            };
            if read || window {
                row.insert("connector", Value::Bool(true));
            }
            if read {
                row.insert("connector_source", Value::Str(reason.to_string()));
            }
            *why.entry(if read {
                reason
            } else if window {
                "window"
            } else {
                "mainline"
            })
            .or_default() += 1;
            Value::Object(row)
        })
        .collect();
    (rows, why)
}

fn ramp_rows(id: &str, p: &Picked) -> Result<Vec<Value>> {
    let path = util::repo_root().join("data/escape_ramps.json");
    let cache = pyjson::load_path(&path)?;
    let cum = shape::cumulative_m(&p.coords);
    let raw = cum[cum.len() - 1] / M_PER_MI;
    let scale = if raw > 0.0 { p.miles / raw } else { 1.0 };
    let local = shape::to_local_m(&p.coords);
    let mean_lat = p.coords.iter().map(|c| c.1).sum::<f64>() / p.coords.len() as f64;
    let coslat = mean_lat.to_radians().cos();
    let (mut lo_lat, mut hi_lat, mut lo_lon, mut hi_lon) = (90.0, -90.0, 180.0, -180.0);
    for &(lon, lat) in &p.coords {
        lo_lat = f64::min(lo_lat, lat - 0.01);
        hi_lat = f64::max(hi_lat, lat + 0.01);
        lo_lon = f64::min(lo_lon, lon - 0.01);
        hi_lon = f64::max(hi_lon, lon + 0.01);
    }
    let mut found: Vec<(f64, char, String, String)> = Vec::new();
    for ramp in cache
        .get("ramps")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let (Some(rlat), Some(rlon)) = (
            ramp.get("lat").and_then(Value::as_f64),
            ramp.get("lon").and_then(Value::as_f64),
        ) else {
            continue;
        };
        if !(lo_lat..=hi_lat).contains(&rlat) || !(lo_lon..=hi_lon).contains(&rlon) {
            continue;
        }
        let (best, d) = shape::nearest_vertex(&p.coords, rlat, rlon);
        if d > RAMP_MATCH_M {
            continue;
        }
        let j = if best + 1 < p.coords.len() {
            best + 1
        } else {
            best.saturating_sub(1)
        };
        let (hx, hy) = (local[j].0 - local[best].0, local[j].1 - local[best].1);
        let rx = rlon.to_radians() * 6_371_000.0 * coslat - local[best].0;
        let ry = rlat.to_radians() * 6_371_000.0 - local[best].1;
        let side = if hx * ry - hy * rx > 0.0 { 'L' } else { 'R' };
        let name = ramp
            .get("name")
            .map(Value::py_str)
            .filter(|n| !n.is_empty() && n != "None")
            .or_else(|| ramp.get("ref").map(Value::py_str))
            .unwrap_or_default();
        let source = ramp.get("source").map(Value::py_str).unwrap_or_default();
        found.push((
            round_py_n(cum[best] / M_PER_MI * scale, 1),
            side,
            name.trim().into(),
            source,
        ));
    }
    found.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut merged: Vec<(f64, char, String, String)> = Vec::new();
    for r in found {
        if let Some(last) = merged.last_mut() {
            if r.1 == last.1 && r.0 - last.0 <= RAMP_DEDUP_MI {
                if last.2.is_empty() && !r.2.is_empty() {
                    last.2 = r.2;
                }
                continue;
            }
        }
        merged.push(r);
    }
    Ok(merged
        .into_iter()
        .map(|(at, side, name, source)| {
            Value::Object(crate::pyobj! {
                "leg" => id, "at_mi" => at, "side" => side.to_string(),
                "name" => name, "source" => source,
            })
        })
        .collect())
}

/// Layer sizes of the leg as committed, and its committed curve count.
fn before(world: &Object, leg: &Value, id: &str) -> BTreeMap<String, usize> {
    let mut out = BTreeMap::new();
    let root = util::repo_root();
    let shard = crate::world::shard_key(leg, world.get("cities").and_then(Value::as_object));
    let spec = format!("HEAD:data/world_source/legs/{shard}.json");
    if let Ok(text) = util::git(&root, &["show", &spec]) {
        if let Ok(old) = pyjson::loads(&text) {
            let legs = old
                .get("legs")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            if let Some(l) = legs.iter().find(|l| leg_id(l) == id) {
                for (k, v) in l
                    .get("corridor")
                    .and_then(Value::as_object)
                    .into_iter()
                    .flat_map(Object::iter)
                {
                    if let Some(a) = v.as_array() {
                        out.insert(k.clone(), a.len());
                    }
                }
            }
        }
    }
    if let Ok(text) = util::git(
        &root,
        &["show", "HEAD:data/world_data/us/gameplay/curves.jsonl"],
    ) {
        let needle = format!("\"leg\": \"{id}\"");
        out.insert(
            "curves".into(),
            text.lines().filter(|l| l.contains(&needle)).count(),
        );
    }
    out
}

fn sizes(leg: &Value, curves: usize) -> BTreeMap<String, usize> {
    let mut out: BTreeMap<String, usize> = leg
        .get("corridor")
        .and_then(Value::as_object)
        .into_iter()
        .flat_map(Object::iter)
        .filter_map(|(k, v)| v.as_array().map(|a| (k.clone(), a.len())))
        .collect();
    out.insert("curves".into(), curves);
    out
}

fn state_name(world: &Object, code: &str) -> String {
    let geo_path = util::repo_root().join("data/world_data/geo.json");
    pyjson::load_path(&geo_path)
        .ok()
        .and_then(|g| {
            g.get("countries")?
                .get("US")?
                .get("states")?
                .get(code)
                .map(Value::py_str)
        })
        .or_else(|| world.get("meta").map(|_| code.to_string()))
        .unwrap_or_else(|| code.to_string())
}

fn old_crossings(leg: &Value) -> Vec<Crossing> {
    leg.get("corridor")
        .and_then(|c| c.get("state_crossings"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|c| Crossing {
            at_mi: c.get("at_mi").and_then(Value::as_f64).unwrap_or(0.0),
            from_state: c.get("from_state").map(Value::py_str).unwrap_or_default(),
            state: c.get("state").map(Value::py_str).unwrap_or_default(),
            place: c.get("place").map(Value::py_str).unwrap_or_default(),
        })
        .collect()
}

pub fn main(argv: &[String]) -> Result<i32> {
    let args = Args::parse(
        argv,
        &Spec {
            values: &["leg", "pbf", "extract-date", "snapshot"],
            flags: &["write", "all-pending"],
        },
    )
    .map_err(|e| format!("{e}\n{USAGE}"))?;
    let pbfs: Vec<PathBuf> = args.all("pbf").iter().map(PathBuf::from).collect();
    if pbfs.is_empty() {
        return Err(format!("--pbf is required\n{USAGE}"));
    }
    let date = match args.value("extract-date") {
        Some(d) => d.to_string(),
        None => super::super::osm::pbf::extract_date(&pbfs[0])?
            .ok_or("the extract's header carries no date; pass --extract-date YYYY-MM-DD")?,
    };
    let snapshot = args.string("snapshot", "Geofabrik extract");
    let write = args.flag("write");
    let source = source_root();
    let mut world = load_world(&source)?;
    let mut wanted = super::wanted_legs(args.all("leg"));
    if args.flag("all-pending") {
        for id in super::leg::pending(&world) {
            if !wanted.contains(&id) {
                wanted.push(id);
            }
        }
    }
    if wanted.is_empty() {
        return Err(format!("nothing selected\n{USAGE}"));
    }

    let legs = world
        .get("legs")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut picked = Vec::new();
    let mut was = BTreeMap::new();
    for id in &wanted {
        let index = legs
            .iter()
            .position(|l| &leg_id(l) == id)
            .ok_or_else(|| format!("no such leg: {id}"))?;
        let leg = &legs[index];
        let shard = shard_path(&world, leg).ok_or("the leg's start city has no state")?;
        let record = super::archive_record(&shard, id)?
            .ok_or_else(|| format!("{id} has no archived road; run reroute-leg first"))?;
        let (coords, _) =
            shape::decode(&record["geom"]).ok_or("the archived road does not decode")?;
        let miles = leg.get("miles").and_then(Value::as_f64).unwrap_or(0.0);
        println!(
            "  {id:46} {:8} {miles} mi",
            leg.get("highway").map(Value::py_str).unwrap_or_default()
        );
        was.insert(id.clone(), before(&world, leg, id));
        picked.push(Picked {
            index,
            id: id.clone(),
            coords,
            miles,
        });
    }

    let lines: Vec<LegLine> = picked
        .iter()
        .map(|p| LegLine::new(p.coords.iter().map(|&(lon, lat)| (lat, lon)).collect()))
        .collect();
    let cells = geo::corridor_cells(lines.iter());
    println!(
        "reading roads and state lines from {} extract(s) dated {date}",
        pbfs.len()
    );
    let net = roads::read(&pbfs, &cells)?;
    println!(
        "  {} roads along the legs, {} state boundary ways",
        net.index.ways.len(),
        net.boundaries.len()
    );
    if net.index.ways.is_empty() {
        return Err(
            "no road in the extract lies along these legs; is this the right extract?".into(),
        );
    }

    let boundary_note = format!(
        "derived 2026-11-05: the leg's archived route geometry intersected with OpenStreetMap \
         U.S. state boundary relations (boundary=administrative, admin_level=4); at_mi = \
         cumulative geometry distance at each boundary crossing, rescaled to leg miles; state \
         miles = differences of those boundaries. OpenStreetMap, {snapshot} of {date}: \
         https://www.openstreetmap.org/."
    );
    let mut curve_counts = BTreeMap::new();
    let mut gameplay: Vec<(String, Vec<Value>, Vec<Value>)> = Vec::new();
    for p in &picked {
        let leg = &legs[p.index];
        let id = &p.id;
        println!("\n=== {id} ===");
        let (curves, why) = curve_rows(id, p, &net);
        let why: Vec<String> = why.iter().map(|(k, n)| format!("{n} {k}")).collect();
        println!("  curves: {} ({})", curves.len(), why.join(", "));
        curve_counts.insert(id.clone(), curves.len());
        let ramps = ramp_rows(id, p)?;
        println!("  runaway ramps: {}", ramps.len());

        let dense = grades::densify(&p.coords, grades::ELEVATION_STEP_MI);
        let heights = valhalla::heights_ft(&dense)?;
        let samples = grades::fine_samples(&dense, &heights, p.miles);
        let segs = grades::segments(&samples, p.miles);
        let terrains: HashSet<&str> = segs.iter().map(|s| s.terrain).collect();
        println!(
            "  grades: {} segments over {} readings ({:?})",
            segs.len(),
            dense.len(),
            terrains
        );
        let grade_source = grades::grade_source();
        let elevation_source = grades::elevation_source();
        let grade_rows: Vec<Value> = segs
            .iter()
            .map(|s| {
                Value::Object(crate::pyobj! {
                    "start_mi" => s.start_mi, "end_mi" => s.end_mi,
                    "avg_grade_pct" => s.avg_grade_pct, "terrain" => s.terrain,
                    "source" => grade_source.as_str(),
                })
            })
            .collect();
        let stored: Vec<Value> = grades::stored_samples(&samples)
            .into_iter()
            .map(|(at, e)| {
                Value::Object(crate::pyobj! {
                    "at_mi" => at, "elevation_ft" => e, "source" => elevation_source.as_str(),
                })
            })
            .collect();

        let from = leg.get("from").and_then(Value::as_str).unwrap_or_default();
        let code = world
            .get("cities")
            .and_then(|c| c.get(from))
            .and_then(|c| c.get("state"))
            .map(Value::py_str)
            .unwrap_or_default();
        let highway = leg.get("highway").map(Value::py_str).unwrap_or_default();
        let cum = shape::cumulative_m(&p.coords);
        let (state_miles, crossings) = states::state_context(
            &p.coords,
            &cum,
            p.miles,
            &state_name(&world, &code),
            &net,
            &highway,
            &old_crossings(leg),
        );
        for c in &crossings {
            println!(
                "  crosses into {} at mile {} ({})",
                c.state, c.at_mi, c.place
            );
        }
        let miles_rows: Vec<Value> = state_miles
            .iter()
            .map(|(s, m)| {
                Value::Object(crate::pyobj! {
                    "state" => s.as_str(), "miles" => *m, "source" => boundary_note.as_str(),
                })
            })
            .collect();
        let crossing_rows: Vec<Value> = crossings
            .iter()
            .map(|c| {
                Value::Object(crate::pyobj! {
                    "at_mi" => c.at_mi, "from_state" => c.from_state.as_str(),
                    "state" => c.state.as_str(), "place" => c.place.as_str(),
                    "source" => boundary_note.as_str(),
                })
            })
            .collect();

        let corridor = world
            .get_mut("legs")
            .and_then(Value::as_array_mut)
            .and_then(|l| l.get_mut(p.index))
            .and_then(Value::as_object_mut)
            .and_then(|o| o.get_mut("corridor"))
            .and_then(Value::as_object_mut)
            .ok_or_else(|| format!("{id} has no corridor"))?;
        corridor.insert("grade_segments", Value::Array(grade_rows));
        corridor.insert("elevation_samples", Value::Array(stored));
        corridor.insert("state_miles", Value::Array(miles_rows));
        if crossing_rows.is_empty() {
            corridor.remove("state_crossings");
        } else {
            corridor.insert("state_crossings", Value::Array(crossing_rows));
        }
        let tolls = corridor.get("tollway_detected") == Some(&Value::Bool(true));
        let events = corridor
            .get("toll_events")
            .and_then(Value::as_array)
            .is_some_and(|e| !e.is_empty());
        if tolls && !events {
            println!("  on a toll road with no toll events: wants curating");
        }
        gameplay.push((id.clone(), curves, ramps));
    }

    let only = wanted.join(";");
    let mut osm_args = vec![
        "--only".to_string(),
        only,
        "--extract-date".into(),
        date.clone(),
        "--snapshot".into(),
        snapshot.clone(),
    ];
    for p in &pbfs {
        osm_args.push("--pbf".into());
        osm_args.push(p.display().to_string());
    }
    if write {
        osm_args.push("--write".into());
        save_world(&world, &source)?;
        for (id, curves, ramps) in &gameplay {
            super::replace_leg_rows(&gameplay_dir().join("curves.jsonl"), id, curves.clone())?;
            super::replace_leg_rows(&gameplay_dir().join("ramps.jsonl"), id, ramps.clone())?;
        }
    }
    println!("\n=== exits (osm-exits) ===");
    let exits = super::super::osm::exits::main(&osm_args)?;
    println!("\n=== posted limits (osm-speed-limits) ===");
    let limits = super::super::osm::main(&osm_args)?;
    if write {
        crate::world::index::main(&[])?;
    }

    // Judge what each leg carries now: from disk when written, else from
    // this run's world (the delegated builders then report on their own).
    let world_now = if write { load_world(&source)? } else { world };
    let legs_now = world_now
        .get("legs")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut failures = 0;
    let interstate_re = regex::Regex::new(r"^I-\d+").map_err(|e| e.to_string())?;
    println!("\n\n=== what each leg carries now ===");
    for p in &picked {
        let Some(leg) = legs_now.iter().find(|l| leg_id(l) == p.id) else {
            continue;
        };
        let now = sizes(leg, curve_counts.get(&p.id).copied().unwrap_or(0));
        let old = &was[&p.id];
        let interstate = leg
            .get("highway")
            .map(Value::py_str)
            .is_some_and(|h| interstate_re.is_match(&h));
        println!("\n{}  ({} mi)", p.id, p.miles);
        let keys: std::collections::BTreeSet<&String> = old.keys().chain(now.keys()).collect();
        for key in keys {
            let (w, n) = (
                old.get(key).copied().unwrap_or(0),
                now.get(key).copied().unwrap_or(0),
            );
            let mut verdict = String::new();
            if key == "grade_segments" {
                let spans: Vec<(f64, f64)> = leg
                    .get("corridor")
                    .and_then(|c| c.get("grade_segments"))
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(|s| {
                        Some((s.get("start_mi")?.as_f64()?, s.get("end_mi")?.as_f64()?))
                    })
                    .collect();
                let gaps = grades::coverage_gaps(&spans, p.miles);
                if gaps.is_empty() {
                    verdict = "  covers the leg end to end".into();
                } else {
                    verdict = format!("  HOLES: {}", gaps.join("; "));
                    failures += 1;
                }
            } else if !write && (key == "interchanges" || key == "speed_limits") {
                verdict = "  (re-read on --write)".into();
            } else if REBUILT.contains(&key.as_str())
                && (key != "interchanges" || interstate || w > 0)
            {
                if n == 0 {
                    verdict = "  EMPTY: the builder did nothing".into();
                    failures += 1;
                } else if w > 0 && (n as f64) < w as f64 / 2.0 {
                    verdict = "  THIN: under half what it was".into();
                    failures += 1;
                }
            } else if CARRIED.contains(&key.as_str()) && n < w {
                verdict = "  (carried where the road is unchanged; the rest waits for its builder)"
                    .into();
            }
            println!("  {key:20} {w:5} -> {n:5}{verdict}");
        }
    }
    if exits != 0 || limits != 0 {
        println!("\nA delegated builder exited non-zero (exits {exits}, limits {limits}).");
        failures += 1;
    }
    if failures > 0 {
        println!(
            "\n{failures} layers came back empty, thin or with holes. That is what a builder \
             looks like when it silently did nothing: read the output above before trusting \
             these legs."
        );
        return Ok(1);
    }
    if !write {
        println!("\n(dry run; pass --write)");
    } else {
        println!("\nEvery rebuilt layer is present. Re-bake world.ffdata next.");
    }
    Ok(0)
}
