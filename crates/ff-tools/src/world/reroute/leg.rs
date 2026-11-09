//! `reroute-leg`: replace a leg's road with the one a truck would take.
//!
//! Asks the truck router for the leg (through its curated `route_via` pins),
//! checks the answer rides the road the leg is named for, reads the ground
//! height at every vertex, and writes the road into the geometry archive at
//! the curve-aware density every other leg carries. Then, on the leg:
//!
//! * `miles` becomes the router's distance, rounded to a whole mile, and
//!   `rerouted` is set;
//! * records read along the old road (exits, landmarks, lane and traffic
//!   counts, restrictions) are carried where the new road is the same
//!   pavement and dropped where it is not (see [`super::carry`]); billboards
//!   and highway markers, placed by hand, stay where they were;
//! * checkpoints and stops are re-placed by their own coordinates, a town or
//!   stop more than 3 miles off the new road dropped; a stop with no
//!   coordinates follows its mile, or its share of the leg where the old
//!   road is gone;
//! * the grade profile, posted limits, state lines and toll events, all read
//!   along the old road, are dropped, as are the leg's rows in the curve,
//!   runaway-ramp and limit-screen layers;
//! * new route points and elevation samples are written every 25 miles, so
//!   the builders have the new road's bounds.
//!
//! The leg is not finished until `reroute-enrich` has rebuilt what was
//! dropped: without a grade profile the truck has no grade at all.
//! `--check` lists any leg left between the two.

use ff_core::pyfmt::round_py_n;

use super::carry::{self, Line};
use super::shape;
use super::valhalla::{self, matches_shield};
use super::{gameplay_dir, leg_id, shard_path};
use crate::cli::{Args, Spec};
use crate::pyjson::{Object, Value};
use crate::util::Result;
use crate::world::{load_world, save_world, source_root};

const USAGE: &str = "usage: reroute-leg --leg from:to [--write] | reroute-leg --check";

/// A reroute must put at least this share of the leg on its own shield,
/// unless a curated pin says the route rides a signed connector there.
const MIN_ON_LABEL: f64 = 0.25;
/// A stop or checkpoint farther than this from the new road is no longer
/// passed.
const MAX_OFF_MI: f64 = 3.0;
/// Route points and stored elevation samples, one per this many miles.
const SAMPLE_MI: f64 = 25.0;
/// Placed by hand, by mile; kept as they are.
const CURATED_LANDMARKS: &[&str] = &["billboard_sign", "highway_marker"];
/// Read along the old road and re-read for the new one.
const CARRIED: &[&str] = &[
    "interchanges",
    "landmarks",
    "lane_segments",
    "traffic_aadt",
    "restrictions",
];
/// Read along the old road and rebuilt by `reroute-enrich` (or, for toll
/// events, re-curated by hand).
const DROPPED: &[&str] = &[
    "grade_segments",
    "speed_limits",
    "state_crossings",
    "state_miles",
    "toll_events",
];
/// Gameplay layers keyed to the old road's curves, ramps and postings.
const DROPPED_SHARDS: &[&str] = &[
    "curves.jsonl",
    "curve_artifacts.jsonl",
    "ramps.jsonl",
    "speed_limits.jsonl",
    "limit_artifacts.jsonl",
];

const ROUTE_SOURCE: &str = "Valhalla truck route over OpenStreetMap, resampled at development \
                            time (replaces the route this leg was first baked from).";

/// Legs rerouted and not yet finished.
pub fn pending(world: &Object) -> Vec<String> {
    world
        .get("legs")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|l| l.get("rerouted") == Some(&Value::Bool(true)))
        .filter(|l| {
            l.get("corridor")
                .and_then(|c| c.get("grade_segments"))
                .and_then(Value::as_array)
                .is_none_or(Vec::is_empty)
        })
        .map(leg_id)
        .collect()
}

fn has_pinned_label_segment(leg: &Value, highway: &str) -> bool {
    let re = regex::Regex::new(r"(?i)\bref\s+([^;]+)").unwrap();
    leg.get("route_via")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|p| p.get("note").map(Value::py_str))
        .any(|note| {
            re.captures(&note)
                .is_some_and(|c| matches_shield(c[1].trim(), highway))
        })
}

/// The share of the route's miles on the leg's shield, and the road it
/// spends the most miles on.
fn rides_label(route: &valhalla::Route, highway: &str) -> (f64, String) {
    let mut total = 0.0;
    let mut on = 0.0;
    let mut by_road: Vec<(String, f64)> = Vec::new();
    for (names, miles) in &route.maneuvers {
        total += miles;
        if names.iter().any(|n| matches_shield(n, highway)) {
            on += miles;
        }
        if let Some(first) = names.first() {
            match by_road.iter_mut().find(|(n, _)| n == first) {
                Some(slot) => slot.1 += miles,
                None => by_road.push((first.clone(), *miles)),
            }
        }
    }
    let dominant = by_road
        .into_iter()
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(n, _)| n)
        .unwrap_or_else(|| "(no named road)".into());
    (if total > 0.0 { on / total } else { 0.0 }, dominant)
}

fn append_source(row: &mut Value, note: &str) {
    if let Some(o) = row.as_object_mut() {
        let old = o.get("source").map(Value::py_str).unwrap_or_default();
        let old = old.trim().trim_end_matches(';').trim_end();
        let joined = if old.is_empty() {
            note.to_string()
        } else {
            format!("{old}; {note}")
        };
        o.insert("source", Value::Str(joined));
    }
}

pub fn main(argv: &[String]) -> Result<i32> {
    let args = Args::parse(
        argv,
        &Spec {
            values: &["leg"],
            flags: &["write", "check"],
        },
    )
    .map_err(|e| format!("{e}\n{USAGE}"))?;
    let source = source_root();
    let mut world = load_world(&source)?;
    if args.flag("check") {
        let legs = pending(&world);
        println!("{} legs rerouted but not finished:", legs.len());
        for id in &legs {
            println!("  {id}");
        }
        return Ok(i32::from(!legs.is_empty()));
    }
    let wanted = args.required("leg").map_err(|e| format!("{e}\n{USAGE}"))?;
    let index = world
        .get("legs")
        .and_then(Value::as_array)
        .and_then(|legs| legs.iter().position(|l| leg_id(l) == wanted))
        .ok_or_else(|| format!("no such leg: {wanted}"))?;
    let leg = world
        .get("legs")
        .and_then(Value::as_array)
        .map(|l| l[index].clone())
        .unwrap_or(Value::Null);
    let (from, to) = wanted.split_once(':').unwrap_or_default();
    let start = super::city_point(&world, from).ok_or("the leg's start city has no coordinates")?;
    let end = super::city_point(&world, to).ok_or("the leg's end city has no coordinates")?;
    let highway = leg.get("highway").map(Value::py_str).unwrap_or_default();
    let old_miles = leg.get("miles").and_then(Value::as_f64).unwrap_or(0.0);

    let Some(route) = valhalla::route(start, end, &super::route_via(&leg))? else {
        println!("the router returned no route");
        return Ok(1);
    };
    let miles = round_py_n(route.miles, 0);
    println!("{wanted} ({highway})");
    println!(
        "  was {old_miles:.0} mi, the router says {:.1} mi over {} vertices; {}",
        route.miles,
        route.shape.len(),
        if route.has_toll {
            "it uses a toll road"
        } else {
            "no toll road"
        }
    );
    let (share, dominant) = rides_label(&route, &highway);
    println!(
        "  it rides {highway} for {:.0}% of its miles (most miles on: {dominant})",
        100.0 * share
    );
    if share < MIN_ON_LABEL && !has_pinned_label_segment(&leg, &highway) {
        println!(
            "  REFUSING: a reroute puts a leg back on the road it is named for, and this \
             route does not. Look at the leg, or pin it with route_via."
        );
        return Ok(1);
    }

    let corridor = leg
        .get("corridor")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let curated: Vec<Value> = corridor
        .get("landmarks")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|l| {
            l.get("category")
                .and_then(Value::as_str)
                .is_some_and(|c| CURATED_LANDMARKS.contains(&c))
        })
        .cloned()
        .collect();
    let beyond: Vec<&Value> = curated
        .iter()
        .filter(|l| {
            l.get("at_mi")
                .and_then(Value::as_f64)
                .is_some_and(|m| m > miles)
        })
        .collect();
    if !beyond.is_empty() {
        println!("  REFUSING: hand-placed landmarks lie past the new leg's end:");
        for l in beyond {
            println!(
                "    {} at mile {}",
                l.get("name").map(Value::py_str).unwrap_or_default(),
                l.get("at_mi").map(Value::py_str).unwrap_or_default()
            );
        }
        return Ok(1);
    }

    let elevations = valhalla::heights_ft(&route.shape)?;
    println!("  ground height read at all {} vertices", elevations.len());
    let cum = shape::cumulative_m(&route.shape);
    let curvature = shape::analyse(&route.shape, &cum);
    let kept_idx = shape::adaptive_simplify(&route.shape, &curvature.curving, &cum);
    let geom = shape::encode(&route.shape, &elevations, &kept_idx);
    let (new_coords, new_ele) = shape::decode(&geom).ok_or("the new road did not round-trip")?;
    println!(
        "  {} curves; archived at {} of {} vertices",
        curvature.curves.len(),
        kept_idx.len(),
        route.shape.len()
    );

    let shard = shard_path(&world, &leg).ok_or("the leg's start city has no state")?;
    let old_record = super::archive_record(&shard, &wanted)?
        .ok_or("the leg has no archived road to carry its records from")?;
    let (old_coords, _) =
        shape::decode(&old_record["geom"]).ok_or("the old road does not decode")?;
    let old = Line::new(&old_coords, old_miles);
    let new = Line::new(&new_coords, miles);

    let mut corridor = corridor;
    let mut report: Vec<String> = Vec::new();
    for key in CARRIED {
        let Some(rows) = corridor.get(key).and_then(Value::as_array).cloned() else {
            continue;
        };
        let (rows, keep_back) = if *key == "landmarks" {
            let read: Vec<Value> = rows
                .iter()
                .filter(|r| !curated.contains(r))
                .cloned()
                .collect();
            (read, curated.clone())
        } else {
            (rows, Vec::new())
        };
        let (mut kept, tally) = carry::carry_layer(&rows, &old, &new);
        kept.extend(keep_back);
        if *key == "landmarks" {
            kept.sort_by(|a, b| {
                let at = |r: &Value| r.get("at_mi").and_then(Value::as_f64).unwrap_or(0.0);
                at(a).total_cmp(&at(b))
            });
        }
        report.push(format!(
            "  {key}: kept {} on the shared road, dropped {}",
            tally.kept, tally.dropped
        ));
        corridor.insert(*key, Value::Array(kept));
    }
    let tolls = corridor
        .get("toll_events")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    for key in DROPPED {
        corridor.remove(key);
    }

    // Checkpoints: real towns, re-placed by their own coordinates.
    if let Some(rows) = corridor
        .get("checkpoints")
        .and_then(Value::as_array)
        .map(|r| r.to_vec())
    {
        let (kept, dropped) = carry::reposition(&rows, &new, MAX_OFF_MI);
        let mut out = Vec::new();
        for (mut row, off) in kept {
            if let Some(o) = row.as_object_mut() {
                o.insert(
                    "source",
                    Value::Str(format!(
                        "Real town on {highway} between {from} and {to}; position matched to \
                         the nearest point on the leg's checked-in route geometry ({off:.2} mi \
                         off-route at closest approach)."
                    )),
                );
            }
            out.push(row);
        }
        report.push(format!("  checkpoints: kept {}", out.len()));
        for (row, off) in dropped {
            report.push(format!(
                "    dropped checkpoint {} ({})",
                row.get("name").map(Value::py_str).unwrap_or_default(),
                if off.is_finite() {
                    format!("{off:.1} mi off the new road")
                } else {
                    "no coordinates".into()
                }
            ));
        }
        corridor.insert("checkpoints", Value::Array(out));
    }
    corridor.insert("tollway_detected", Value::Bool(route.has_toll));

    // Route points and stored elevation, every 25 miles of the new road.
    let new_cum = shape::cumulative_m(&new_coords);
    let raw_mi = new_cum[new_cum.len() - 1] / super::super::osm::geo::M_PER_MI;
    let scale = if raw_mi > 0.0 { miles / raw_mi } else { 1.0 };
    let (mut points, mut samples) = (Vec::new(), Vec::new());
    let mut last = -1e9;
    for (i, &(lon, lat)) in new_coords.iter().enumerate() {
        let at = (new_cum[i] / super::super::osm::geo::M_PER_MI * scale).min(miles);
        if at - last < SAMPLE_MI && i != 0 && i != new_coords.len() - 1 {
            continue;
        }
        last = at;
        let at = round_py_n(at, 2);
        points.push(Value::Object(crate::pyobj! {
            "at_mi" => at, "lat" => round_py_n(lat, 5), "lon" => round_py_n(lon, 5),
        }));
        samples.push(Value::Object(crate::pyobj! {
            "at_mi" => at, "elevation_ft" => round_py_n(new_ele[i], 1), "source" => ROUTE_SOURCE,
        }));
    }
    corridor.insert("route_points", Value::Array(points));
    corridor.insert("elevation_samples", Value::Array(samples));

    // Stops: by their coordinates; without, by their mile on the shared
    // road, else by their share of the leg.
    let mut stops_report = Vec::new();
    let stops: Vec<Value> = leg
        .get("stops")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let interior = |m: f64| round_py_n(m.clamp(0.1, (miles - 0.1).max(0.1)), 1);
    let mut new_stops = Vec::new();
    for stop in &stops {
        let name = stop.get("name").map(Value::py_str).unwrap_or_default();
        let mut stop = stop.clone();
        let has_coords = stop.get("lat").and_then(Value::as_f64).is_some()
            && stop.get("lon").and_then(Value::as_f64).is_some();
        if has_coords {
            let (kept, dropped) = carry::reposition(std::slice::from_ref(&stop), &new, MAX_OFF_MI);
            let Some((placed, _)) = kept.into_iter().next() else {
                let off = dropped.first().map(|d| d.1).unwrap_or(f64::INFINITY);
                stops_report.push(format!(
                    "    dropped stop {name} ({off:.1} mi off the new road)"
                ));
                continue;
            };
            stop = placed;
            let at = stop.get("at_mi").and_then(Value::as_f64).unwrap_or(0.0);
            if let Some(o) = stop.as_object_mut() {
                o.insert("at_mi", Value::Float(interior(at)));
            }
            append_source(
                &mut stop,
                "at_mi derived 2026-11-05: the stop's own coordinates projected onto the \
                 rerouted Valhalla truck geometry, rescaled to leg miles",
            );
        } else if let Some(at) = stop.get("at_mi").and_then(Value::as_f64) {
            let (lat, lon) = old.point_at(at);
            let (off, mile) = new.place(lat, lon);
            let (mile, note) = if off <= carry::SAME_ROAD_M {
                (mile, "at_mi derived 2026-11-05: the old mile's point on the old road, projected onto the rerouted Valhalla truck geometry (the stop has no coordinates)")
            } else {
                (
                    at * miles / old_miles.max(1.0),
                    "at_mi derived 2026-11-05: old at_mi x new miles / old miles (the stop has no coordinates, and the old road there was left)",
                )
            };
            if let Some(o) = stop.as_object_mut() {
                o.insert("at_mi", Value::Float(interior(mile)));
            }
            append_source(&mut stop, note);
        }
        if let Some(at) = stop.get("interchange_mi").and_then(Value::as_f64) {
            let (lat, lon) = old.point_at(at);
            let (off, mile) = new.place(lat, lon);
            if let Some(o) = stop.as_object_mut() {
                if off <= carry::SAME_ROAD_M {
                    o.insert("interchange_mi", Value::Float(round_py_n(mile, 1)));
                } else {
                    o.remove("interchange_mi");
                }
            }
        }
        new_stops.push(stop);
    }
    new_stops.sort_by(|a, b| {
        let at = |r: &Value| r.get("at_mi").and_then(Value::as_f64).unwrap_or(0.0);
        at(a).total_cmp(&at(b))
    });

    for line in &report {
        println!("{line}");
    }
    println!("  stops: kept {} of {}", new_stops.len(), stops.len());
    for line in &stops_report {
        println!("{line}");
    }
    if !tolls.is_empty() {
        println!(
            "  DROPPED {} hand-curated toll events; re-curate them if the new road tolls:",
            tolls.len()
        );
        for t in &tolls {
            println!(
                "    {} on {}",
                t.get("name").map(Value::py_str).unwrap_or_default(),
                t.get("road").map(Value::py_str).unwrap_or_default()
            );
        }
    }
    if route.has_toll {
        println!("  the new road tolls and the leg has no toll events: curate them");
    }

    if !args.flag("write") {
        println!("\n(dry run; pass --write)");
        return Ok(0);
    }
    let mut record = old_record.clone();
    record["geom"] = geom;
    record["miles"] = serde_json::json!(miles);
    record["highway"] = serde_json::json!(highway);
    super::write_archive_record(&shard, &wanted, &record)?;
    {
        let legs = world
            .get_mut("legs")
            .and_then(Value::as_array_mut)
            .ok_or("world_source has no legs")?;
        let o = legs[index]
            .as_object_mut()
            .ok_or("the leg is not an object")?;
        o.insert("miles", Value::Int(miles as i128));
        o.insert("rerouted", Value::Bool(true));
        o.insert("corridor", Value::Object(corridor));
        if !stops.is_empty() {
            o.insert("stops", Value::Array(new_stops));
        }
    }
    for name in DROPPED_SHARDS {
        let path = gameplay_dir().join(name);
        if path.is_file() {
            let n = super::replace_leg_rows(&path, &wanted, Vec::new())?;
            if n > 0 {
                println!("  dropped {n} rows from {name}");
            }
        }
    }
    save_world(&world, &source)?;
    crate::world::index::main(&[])?;
    println!("  wrote the new road. The leg is NOT finished: run reroute-enrich on it next.");
    Ok(0)
}
