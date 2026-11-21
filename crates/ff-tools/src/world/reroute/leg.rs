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
use super::{leg_id, shard_path};
use crate::cli::{Args, Spec};
use crate::pyjson::{Object, Value};
use crate::util::Result;
use crate::world::{load_world, save_world, source_root};

const USAGE: &str =
    "usage: reroute-leg --leg from:to [--accept-miles] [--write] | reroute-leg --check";

/// A reroute must put at least this share of the leg on its own shield,
/// unless a curated pin says the route rides a signed connector there.
const MIN_ON_LABEL: f64 = 0.25;
/// A new road this much longer or shorter than the leg is refused without
/// `--accept-miles`: the Surrey to Blaine truck route came back 513 miles
/// against 16, the router avoiding the Pacific Highway crossing.
const MAX_MILES_CHANGE: f64 = 0.25;
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

/// The note a re-placed checkpoint's source ends with.
const REPLACED_NOTE: &str = " Re-placed on the rerouted road by its own coordinates";
/// Before re-placed checkpoints kept their citation, this replaced it.
const OLD_GENERIC_NOTE: &str = "Real town on ";

/// A re-placed checkpoint's source: its own citation, if it had one, then
/// where it now sits on the new road.
fn checkpoint_source(cited: &str, highway: &str, from: &str, to: &str, off: f64) -> String {
    let cited = cited.split(REPLACED_NOTE).next().unwrap_or_default().trim();
    let place = format!("{off:.2} mi off-route at closest approach");
    if cited.is_empty() || cited.starts_with(OLD_GENERIC_NOTE) {
        format!(
            "Real town on {highway} between {from} and {to}; position matched to the nearest \
             point on the leg's checked-in route geometry ({place})."
        )
    } else {
        format!("{cited}{REPLACED_NOTE} ({place}).")
    }
}

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

/// A leg's stored route points as `(lon, lat)`, the shape the archive
/// decodes to.
pub fn scaffold_line(leg: &Value) -> Vec<(f64, f64)> {
    leg.get("corridor")
        .and_then(|c| c.get("route_points"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|p| Some((p.get("lon")?.as_f64()?, p.get("lat")?.as_f64()?)))
        .collect()
}

pub fn main(argv: &[String]) -> Result<i32> {
    let args = Args::parse(
        argv,
        &Spec {
            values: &["leg"],
            flags: &["write", "check", "accept-miles"],
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
    if old_miles > 0.0
        && (route.miles - old_miles).abs() / old_miles > MAX_MILES_CHANGE
        && !args.flag("accept-miles")
    {
        println!(
            "  REFUSING: the router's road is {:.0}% {} than the leg. A reroute that far off is \
             usually the router avoiding something (a crossing, a pin on the wrong road), not a \
             better road; check it, then pass --accept-miles if it is right.",
            100.0 * (route.miles - old_miles).abs() / old_miles,
            if route.miles > old_miles {
                "longer"
            } else {
                "shorter"
            }
        );
        return Ok(1);
    }
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
    let old_record = super::archive_record(&shard, &wanted)?;
    let old_coords = match &old_record {
        Some(record) => {
            shape::decode(&record["geom"])
                .ok_or("the old road does not decode")?
                .0
        }
        None => {
            // A scaffold leg (the Alcan, Alaska, British Columbia and the
            // Yukon were laid out this way) has no archived road, only its
            // route points; they are the old road its records were read on.
            let points = scaffold_line(&leg);
            if points.len() < 2 {
                return Err(
                    "the leg has no archived road and fewer than two route points to carry its \
                     records from"
                        .into(),
                );
            }
            println!(
                "  no archived road: carrying from the {} scaffold route points",
                points.len()
            );
            points
        }
    };
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

    // Checkpoints: real places, re-placed by their own coordinates. A
    // checkpoint's own citation (a port of entry's agency page, an OSM node)
    // is kept; only the note on where it sits is rewritten.
    if let Some(rows) = corridor
        .get("checkpoints")
        .and_then(Value::as_array)
        .map(|r| r.to_vec())
    {
        let (kept, dropped) = carry::reposition(&rows, &new, MAX_OFF_MI);
        let mut out = Vec::new();
        for (mut row, off) in kept {
            if let Some(o) = row.as_object_mut() {
                let cited = o.get("source").map(Value::py_str).unwrap_or_default();
                o.insert(
                    "source",
                    Value::Str(checkpoint_source(&cited, &highway, from, to, off)),
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
    let mut record = old_record
        .clone()
        .unwrap_or_else(|| serde_json::json!({ "leg": wanted }));
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
    // Every country's folder: rows written before the layers were filed by
    // country (the Canadian legs' postings sat in the US folder) go too.
    for (folder, dir) in crate::world::country::gameplay_dirs() {
        for name in DROPPED_SHARDS {
            let path = dir.join(name);
            if path.is_file() {
                let n = super::replace_leg_rows(&path, &wanted, Vec::new())?;
                if n > 0 {
                    println!("  dropped {n} rows from {folder}/gameplay/{name}");
                }
            }
        }
    }
    save_world(&world, &source)?;
    crate::world::index::main(&[])?;
    println!("  wrote the new road. The leg is NOT finished: run reroute-enrich on it next.");
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_scaffold_leg_carries_from_its_route_points() {
        let leg = crate::pyjson::loads(
            r#"{"corridor": {"route_points": [
                {"at_mi": 0.0, "lat": 55.75847, "lon": -120.22868},
                {"at_mi": 7.5, "lat": 55.83302, "lon": -120.3608}]}}"#,
        )
        .unwrap();
        assert_eq!(
            scaffold_line(&leg),
            vec![(-120.22868, 55.75847), (-120.3608, 55.83302)]
        );
        let bare = crate::pyjson::loads(r#"{"corridor": {}}"#).unwrap();
        assert!(scaffold_line(&bare).is_empty());
    }

    #[test]
    fn a_re_placed_checkpoint_keeps_its_citation() {
        let cited = "U.S. Customs and Border Protection, Alcan Alaska Port of Entry (port 3104).";
        let once = checkpoint_source(
            cited,
            "Alaska Highway",
            "whitehorse_yt_ca",
            "tok_ak_us",
            0.0,
        );
        assert!(once.starts_with(cited), "{once}");
        assert!(once.contains("Re-placed on the rerouted road"));
        // A second reroute rewrites the note rather than stacking another.
        let twice = checkpoint_source(
            &once,
            "Alaska Highway",
            "whitehorse_yt_ca",
            "tok_ak_us",
            0.1,
        );
        assert_eq!(twice.matches("Re-placed").count(), 1);
        assert!(twice.contains("0.10 mi off-route"));
        let bare = checkpoint_source("", "BC 97", "a", "b", 0.0);
        assert!(bare.starts_with("Real town on BC 97 between a and b"));
    }
}
