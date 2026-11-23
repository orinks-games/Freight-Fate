//! `hpms-pavement`: what the road is made of along each leg, and where it
//! leaves the ground (`corridor.pavement`), from HPMS Surface_Type and
//! Structure_Type.
//!
//! Every HPMS mainline row near the leg is fetched (functional systems 1 to
//! 5, two-way, one-way and non-inventory rows) in 20-mile boxes along the
//! archived road, placed on the road by projecting its geometry (see
//! `pavement_match`), and read in 0.005-mile bins each way. Like bins merge
//! into runs. A bin no section reads stays empty: nothing is filled in.
//!
//! The surface class and the structure are READ. The mileage is DERIVED:
//! each section's longest piece projected onto the archived road, rescaled
//! to the leg's miles. A run's roughness and faulting are the mean of the
//! readings inside it, also derived. The layer's `meta` says how much of the
//! leg each way has a reading.

use rayon::prelude::*;
use std::collections::{BTreeMap, HashSet};

use super::pavement_match::{
    assemble, flicker, gaps, structure_name, surface_name, Directions, LegPavement, Placed,
    RoadIndex, Section,
};
use super::traffic::envelopes_with;
use super::Client;
use crate::cli::{Args, Spec};
use crate::pyjson::Value;
use crate::util::Result;
use crate::world::layers::{corridor_mut, leg_id, leg_miles, round, Only, Route, Run};

const USAGE: &str =
    "usage: hpms-pavement [--only from:to;from:to] [--verbose] [--refresh] [--write]";

const BOX_SPAN_MI: f64 = 20.0;
const BOX_PAD_M: f64 = 400.0;
const PAGE_SIZE: usize = 2000;
const FIELDS: &str = "OBJECTID,Route_ID,F_SYSTEM,FACILITY_TYPE,SURFACE_TYPE,STRUCTURE_TYPE,IRI,\
                      Faulting,Year_Record,Year_Last_Construction,YEAR_LAST_IMPROVEMENT,\
                      Begin_Point,End_Point";
const WHERE: &str = "F_SYSTEM<=5 AND FACILITY_TYPE IN (1,2,6)";
/// A stretch this long changing class this often is flagged in the report.
const FLICKER_MI: f64 = 2.0;
const FLICKER_PER_MI: f64 = 5.0;

pub fn source(accessed: &str) -> String {
    format!(
        "FHWA Highway Performance Monitoring System (HPMS) Surface_Type (Item 49) and \
         Structure_Type (Item 4), codes per the HPMS Field Manual, Chapter 4 \
         (https://www.fhwa.dot.gov/policyinformation/hpms/fieldmanual/page06.cfm and page04.cfm), \
         read from the national ArcGIS Living Atlas layer (Federal User Community hpms_v2_view), \
         accessed {accessed}. The class and the structure are READ. Mileage is DERIVED: each \
         section's longest geometry piece projected onto the archived leg road and rescaled to \
         the leg's miles, read in 0.005-mile bins, the highest functional class nearest the road \
         winning where rows overlap. A run marked inventory_side is ASSUMED: the leg drives the \
         non-inventory carriageway there, which HPMS gives no surface of its own, and the inventory \
         carriageway's reading within 250 m is applied to it (meta inventory_share_* gives how \
         much of the leg). A structure's hpms_mi is READ: the milepoint length of the HPMS \
         sections behind it, each once. mile_scale is leg miles per measured mile of the archived \
         road. Each surface entry reads 'from_mi to_mi class', then forward or reverse when it \
         holds one way only, then iri=, faulting= and inventory_side where they apply, and on exposed concrete \
         built= and improved= (HPMS Year_Last_Construction and YEAR_LAST_IMPROVEMENT, READ, the \
         year of each date; a run splits where either changes); each \
         structure reads 'from_mi to_mi kind', the way, then hpms= (its HPMS length). iri (inches per mile) and faulting (inches) are DERIVED: the \
         mean of the section readings inside the run. Where HPMS has no reading nothing is \
         written. HPMS reports bridge-sized culverts as bridges."
    )
}

/// The year of an HPMS date field (epoch milliseconds, UTC).
fn year_of(v: Option<&serde_json::Value>) -> Option<i64> {
    use chrono::Datelike;
    let ms = v?.as_f64()?;
    chrono::DateTime::from_timestamp_millis(ms as i64).map(|d| i64::from(d.year()))
}

fn query_box(client: &Client, b: (f64, f64, f64, f64)) -> Result<Vec<(i64, Section)>> {
    let mut out = Vec::new();
    let mut offset = 0;
    loop {
        let v = client.query(&[
            ("where", WHERE.into()),
            ("geometry", super::envelope(b)),
            ("geometryType", "esriGeometryEnvelope".into()),
            ("inSR", "4326".into()),
            ("outSR", "4326".into()),
            ("spatialRel", "esriSpatialRelIntersects".into()),
            ("outFields", FIELDS.into()),
            ("returnGeometry", "true".into()),
            ("maxAllowableOffset", "0.00005".into()),
            ("geometryPrecision", "5".into()),
            ("orderByFields", "OBJECTID".into()),
            ("resultOffset", offset.to_string()),
            ("resultRecordCount", PAGE_SIZE.to_string()),
            ("f", "json".into()),
        ])?;
        let Some(features) = v.get("features").and_then(|f| f.as_array()) else {
            break;
        };
        for f in features {
            let attr = |k: &str| f.pointer(&format!("/attributes/{k}"));
            let int = |k: &str| attr(k).and_then(|x| x.as_f64()).map(|x| x as i64);
            let paths = f
                .pointer("/geometry/paths")
                .and_then(|p| p.as_array())
                .into_iter()
                .flatten()
                .filter_map(|path| path.as_array())
                .map(|path| {
                    path.iter()
                        .filter_map(|pt| Some((pt.get(1)?.as_f64()?, pt.get(0)?.as_f64()?)))
                        .collect()
                })
                .collect();
            out.push((
                int("OBJECTID").unwrap_or(-1),
                Section {
                    route_id: attr("Route_ID")
                        .and_then(|x| x.as_str())
                        .unwrap_or_default()
                        .to_string(),
                    f_system: int("F_SYSTEM").unwrap_or(9),
                    facility: int("FACILITY_TYPE").unwrap_or(0),
                    surface: int("SURFACE_TYPE"),
                    structure: int("STRUCTURE_TYPE"),
                    iri: attr("IRI").and_then(|x| x.as_f64()),
                    faulting: attr("Faulting").and_then(|x| x.as_f64()),
                    year: int("Year_Record"),
                    built: year_of(attr("Year_Last_Construction")),
                    improved: year_of(attr("YEAR_LAST_IMPROVEMENT")),
                    begin_mi: attr("Begin_Point").and_then(|x| x.as_f64()),
                    end_mi: attr("End_Point").and_then(|x| x.as_f64()),
                    paths,
                },
            ));
        }
        if features.is_empty()
            || v.get("exceededTransferLimit").and_then(|x| x.as_bool()) != Some(true)
        {
            break;
        }
        offset += PAGE_SIZE;
    }
    Ok(out)
}

/// The sections near one leg, each once.
fn fetch(client: &Client, route: &Route) -> Result<Vec<Section>> {
    let boxes = envelopes_with(route, BOX_SPAN_MI, BOX_PAD_M);
    let answers: Result<Vec<Vec<(i64, Section)>>> =
        boxes.par_iter().map(|b| query_box(client, *b)).collect();
    let mut seen = HashSet::new();
    Ok(answers?
        .into_iter()
        .flatten()
        .filter(|(id, _)| *id < 0 || seen.insert(*id))
        .map(|(_, s)| s)
        .collect())
}

/// The leg's pavement and its mile scale (leg miles per measured mile).
pub fn read_leg(client: &Client, route: &Route, miles: f64) -> Result<(LegPavement, f64)> {
    let sections = fetch(client, route)?;
    let index = RoadIndex::new(route);
    let placed: Vec<Placed> = sections
        .par_iter()
        .enumerate()
        .filter_map(|(i, s)| index.place(s, i))
        .collect();
    Ok((assemble(&sections, &placed, miles), index.scale))
}

/// The `corridor.pavement` object.
pub fn to_value(p: &LegPavement, scale: f64, note: &str) -> Value {
    let way = |d: Directions| match d {
        Directions::Both => String::new(),
        d => format!(" {}", d.name()),
    };
    let surface = p
        .surface
        .iter()
        .filter_map(|r| {
            let mut line = format!(
                "{} {} {}{}",
                round(r.from_mi, 3),
                round(r.to_mi, 3),
                surface_name(r.surface)?,
                way(r.directions)
            );
            if let Some(iri) = r.iri {
                line.push_str(&format!(" iri={}", iri.round()));
            }
            if let Some(f) = r.faulting {
                line.push_str(&format!(" faulting={}", round(f, 2)));
            }
            if r.inventory_side {
                line.push_str(" inventory_side");
            }
            if let Some(y) = r.built {
                line.push_str(&format!(" built={y}"));
            }
            if let Some(y) = r.improved {
                line.push_str(&format!(" improved={y}"));
            }
            Some(Value::Str(line))
        })
        .collect();
    let structures = p
        .structures
        .iter()
        .filter_map(|r| {
            let mut line = format!(
                "{} {} {}{}",
                round(r.from_mi, 3),
                round(r.to_mi, 3),
                structure_name(r.structure)?,
                way(r.directions)
            );
            if let Some(mi) = r.hpms_mi {
                line.push_str(&format!(" hpms={}", round(mi, 3)));
            }
            Some(Value::Str(line))
        })
        .collect();
    let mut meta = crate::pyobj! {
        "read_share_forward" => round(p.read_forward, 3),
        "read_share_reverse" => round(p.read_reverse, 3),
        "inventory_share_forward" => round(p.inventory_forward, 3),
        "inventory_share_reverse" => round(p.inventory_reverse, 3),
        "sectioned_share" => round(p.sectioned, 3),
        "mile_scale" => round(scale, 4),
    };
    if let Some(y) = p.year {
        meta.insert("hpms_year", Value::Int(i128::from(y)));
    }
    meta.insert("source", Value::Str(note.to_string()));
    Value::Object(crate::pyobj! {
        "surface" => Value::Array(surface),
        "structures" => Value::Array(structures),
        "meta" => Value::Object(meta),
    })
}

/// Miles of each class each way (`both` counted once), for the report.
pub fn mix(p: &LegPavement) -> BTreeMap<&'static str, f64> {
    let mut out = BTreeMap::new();
    for r in &p.surface {
        if let Some(name) = surface_name(r.surface) {
            *out.entry(name).or_default() += r.to_mi - r.from_mi;
        }
    }
    out
}

fn report(id: &str, miles: f64, p: &LegPavement, verbose: bool) {
    let mut kinds: BTreeMap<String, (usize, f64)> = BTreeMap::new();
    for s in &p.structures {
        let e = kinds
            .entry(format!(
                "{} {}",
                structure_name(s.structure).unwrap_or("?"),
                s.directions.name()
            ))
            .or_default();
        e.0 += 1;
        e.1 += s.to_mi - s.from_mi;
    }
    let mix: Vec<String> = mix(p).iter().map(|(k, v)| format!("{k} {v:.1}")).collect();
    let kinds: Vec<String> = kinds
        .iter()
        .map(|(k, (n, m))| format!("{n} {k} ({m:.2} mi)"))
        .collect();
    let longest = p
        .structures
        .iter()
        .map(|s| s.to_mi - s.from_mi)
        .fold(0.0, f64::max);
    println!(
        "  {id} ({miles} mi, HPMS {}): read {:.0}% forward, {:.0}% reverse (of which off the \
         inventory side {:.0}% and {:.0}%), sectioned {:.0}%; \
         {} surface runs [{}]; structures: {} (longest {longest:.2} mi)",
        p.year.map_or("?".into(), |y| y.to_string()),
        100.0 * p.read_forward,
        100.0 * p.read_reverse,
        100.0 * p.inventory_forward,
        100.0 * p.inventory_reverse,
        100.0 * p.sectioned,
        p.surface.len(),
        mix.join(", "),
        if kinds.is_empty() {
            "none".into()
        } else {
            kinds.join(", ")
        },
    );
    for (a, z, n) in flicker(&p.surface, FLICKER_MI, FLICKER_PER_MI) {
        println!("    flicker: {n} runs between mile {a:.2} and {z:.2}");
    }
    let classes: Vec<String> = p
        .class_miles
        .iter()
        .map(|(k, v)| format!("F_SYSTEM {k}: {v:.1} mi"))
        .collect();
    println!("    read forward by class: {}", classes.join(", "));
    for reverse in [false, true] {
        let g: Vec<String> = gaps(p, miles, reverse, 0.5)
            .iter()
            .map(|(a, z)| format!("{a:.1}-{z:.1}"))
            .collect();
        if !g.is_empty() {
            let way = if reverse { "reverse" } else { "forward" };
            println!("    unread {way} (0.5 mi or more): {}", g.join(", "));
        }
    }
    if verbose {
        for r in &p.surface {
            println!(
                "    {:>7.2} {:>7.2} {:<34} {:<7} iri {:?} faulting {:?}{}",
                r.from_mi,
                r.to_mi,
                surface_name(r.surface).unwrap_or("?"),
                r.directions.name(),
                r.iri.map(f64::round),
                r.faulting,
                if r.inventory_side {
                    " (inventory side)"
                } else {
                    ""
                }
            );
        }
        for s in &p.structures {
            println!(
                "    {:>7.3} {:>7.3} {} {} (HPMS {:?} mi)",
                s.from_mi,
                s.to_mi,
                structure_name(s.structure).unwrap_or("?"),
                s.directions.name(),
                s.hpms_mi
            );
        }
    }
}

pub fn main(argv: &[String]) -> Result<i32> {
    let args = Args::parse(
        argv,
        &Spec {
            values: &["only"],
            flags: &["write", "refresh", "verbose"],
        },
    )
    .map_err(|e| format!("{e}\n{USAGE}"))?;
    let only = Only::parse(&args);
    let client = Client::new(args.flag("refresh"));
    let note = source(&super::today());
    let mut run = Run::load()?;
    let archive = std::mem::take(&mut run.archive);
    let (mut changed, mut same, mut none, mut failed, mut no_archive) = (0, 0, 0, 0, 0);
    let (mut leg_mi, mut read_mi, mut inventory_mi) = (0.0, 0.0, 0.0);
    let legs = run.legs_mut()?;
    let total = legs.iter().filter(|l| only.wants(&leg_id(l))).count();
    let mut done = 0;
    for leg in legs.iter_mut() {
        let id = leg_id(leg);
        if !only.wants(&id) {
            continue;
        }
        done += 1;
        let Some(line) = archive.get(&id) else {
            no_archive += 1;
            continue;
        };
        let miles = leg_miles(leg);
        let route = Route::new(line, miles);
        let (pavement, scale) = match read_leg(&client, &route, miles) {
            Ok(p) => p,
            Err(e) => {
                println!("  {id}: skipped, {e}");
                failed += 1;
                continue;
            }
        };
        leg_mi += miles;
        read_mi += miles * (pavement.read_forward + pavement.read_reverse) / 2.0;
        inventory_mi += miles * (pavement.inventory_forward + pavement.inventory_reverse) / 2.0;
        if only.is_some() {
            report(&id, miles, &pavement, args.flag("verbose"));
        } else if done % 50 == 0 {
            println!(
                "  [{done}/{total}] {id}: read {:.0}%",
                100.0 * pavement.read_forward
            );
        }
        let corridor = corridor_mut(leg)?;
        if pavement.surface.is_empty() && pavement.structures.is_empty() {
            none += 1;
            if corridor.remove("pavement").is_some() {
                changed += 1;
            }
            continue;
        }
        let value = to_value(&pavement, scale, &note);
        let unchanged = corridor.get("pavement").is_some_and(|old| {
            // The accessed date in the note moves every run; compare the rest.
            let strip = |v: &Value| {
                let mut v = v.clone();
                if let Some(Value::Object(m)) = v.as_object_mut().and_then(|o| o.get_mut("meta")) {
                    m.remove("source");
                }
                v
            };
            strip(old) == strip(&value)
        });
        if unchanged {
            same += 1;
        } else {
            changed += 1;
            corridor.insert("pavement", value);
        }
    }
    run.archive = archive;
    println!(
        "{changed} legs changed, {same} the same; {none} with no HPMS reading on the road and \
         {failed} failed kept as they were, {no_archive} with no archived road kept. Surface read \
         on {:.1}% of {leg_mi:.0} leg miles (mean of the two directions), {:.1}% of them \
         ASSUMED from the inventory carriageway; the rest has no reading and is left empty. {} \
         new answer(s) fetched.",
        100.0 * read_mi / leg_mi.max(1e-9),
        100.0 * inventory_mi / leg_mi.max(1e-9),
        client.fetched.load(std::sync::atomic::Ordering::Relaxed)
    );
    run.finish(args.flag("write"))
}
