//! `osm-stops`: the fuel stops, truck stops, rest areas and pullouts a leg
//! passes, in two steps.
//!
//! The listing step (`--pbf`) reads OpenStreetMap's candidates near each
//! leg's road: `amenity=fuel` that sells diesel or takes trucks,
//! `highway=rest_area|services`, and parking that takes heavy goods
//! vehicles. It prints them with the leg mile and how far off the road each
//! sits, and writes nothing to the map. A candidate is a lead, never a stop:
//! OpenStreetMap seldom says whether a pump is open all year or whether a
//! tractor-trailer fits.
//!
//! The placing step (`--stops FILE`) reads a curated list of stops, each with
//! its position and a source note from an agency or operator page, and puts
//! every one on each leg in `--only` whose road passes within its reach.
//! A leg's existing stops named in the list are replaced or dropped; others
//! are kept.
//! The road is the leg's archived geometry, or its stored route points when
//! it has none; a stop placed on coarse route points says so in its source.
//! A stop whose `season` is not year round is left out and reported: the
//! game has no closed season for a stop, and offering a shut pump strands a
//! driver.
//!
//! ```text
//! cargo tools osm-stops --pbf FILE --only "$L" [--near-mi 1.0]
//! cargo tools osm-stops --stops docs/stops/canada-alaska.json --only "$L" [--write]
//! ```

use super::features::{self, tag, Select};
use super::{corridor_mut, leg_id, leg_miles, pbf_paths, round, Grid, Only, Route, Run};
use crate::cli::{Args, Spec};
use crate::pyjson::{Object, Value};
use crate::util::Result;
use crate::world::osm::geo::LegLine;

const USAGE: &str = "usage: osm-stops (--pbf FILE [--near-mi MI] | --stops FILE) \
                     [--only from:to;from:to] [--write]";

/// How far off the road a listed candidate may sit.
pub const CANDIDATE_NEAR_MI: f64 = 1.0;
/// How far off the road a curated stop may sit and still be on the leg,
/// unless the stop names its own reach.
pub const PLACE_REACH_MI: f64 = 0.5;
/// A leg whose stored road has fewer vertices per 10 miles than this is a
/// coarse line, and a mile measured on it is approximate.
const COARSE_POINTS_PER_10_MI: f64 = 1.0;
const CELL_DEG: f64 = 0.1;

/// The leg's road as `(lat, lon)` and whether it is coarse route points.
pub fn leg_road(
    leg: &Value,
    archive: &std::collections::HashMap<String, LegLine>,
) -> Option<(LegLine, bool)> {
    if let Some(line) = archive.get(&leg_id(leg)) {
        return Some((line.clone(), false));
    }
    let points: Vec<(f64, f64)> = leg
        .get("corridor")
        .and_then(|c| c.get("route_points"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|p| Some((p.get("lat")?.as_f64()?, p.get("lon")?.as_f64()?)))
        .collect();
    if points.len() < 2 {
        return None;
    }
    let coarse = (points.len() as f64) < leg_miles(leg) / 10.0 * COARSE_POINTS_PER_10_MI;
    Some((LegLine::new(points), coarse))
}

/// Whether an OpenStreetMap element is a stop candidate, and what kind.
pub fn candidate_kind(t: &[(&str, &str)]) -> Option<&'static str> {
    let has = |k: &str| t.iter().find(|(tk, _)| *tk == k).map(|(_, v)| *v);
    let yes = |k: &str| has(k).is_some_and(|v| matches!(v, "yes" | "designated"));
    match (has("amenity"), has("highway")) {
        (Some("fuel"), _) if yes("fuel:diesel") || yes("fuel:HGV_diesel") || yes("hgv") => {
            Some("fuel")
        }
        (Some("fuel"), _) => Some("fuel (diesel not tagged)"),
        (_, Some("rest_area")) => Some("rest area"),
        (_, Some("services")) => Some("services"),
        (Some("parking"), _) if yes("hgv") => Some("truck parking"),
        _ => None,
    }
}

/// The tags worth reading when judging a candidate.
const SHOWN: &[&str] = &[
    "name",
    "brand",
    "operator",
    "fuel:diesel",
    "hgv",
    "opening_hours",
    "seasonal",
    "website",
    "toilets",
];

struct Candidate {
    kind: &'static str,
    osm: String,
    lat: f64,
    lon: f64,
    tags: Vec<(String, String)>,
}

fn read_candidates(pbfs: &[std::path::PathBuf]) -> Result<Vec<Candidate>> {
    let keep = |t: &[(&str, &str)]| candidate_kind(t).is_some();
    let found = features::read(
        pbfs,
        &Select {
            node: Some(&keep),
            way: Some(&keep),
            relation: None,
        },
    )?;
    let pick = |tags: &Vec<(String, String)>| {
        let borrowed: Vec<(&str, &str)> =
            tags.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
        candidate_kind(&borrowed).unwrap_or("")
    };
    let mut out: Vec<Candidate> = found
        .nodes
        .into_iter()
        .map(|n| Candidate {
            kind: pick(&n.tags),
            osm: format!("node {}", n.id),
            lat: n.lat,
            lon: n.lon,
            tags: n.tags,
        })
        .collect();
    for w in found.ways {
        if w.coords.is_empty() {
            continue;
        }
        let n = w.coords.len() as f64;
        let lat = w.coords.iter().map(|c| c.0).sum::<f64>() / n;
        let lon = w.coords.iter().map(|c| c.1).sum::<f64>() / n;
        out.push(Candidate {
            kind: pick(&w.tags),
            osm: format!("way {}", w.id),
            lat,
            lon,
            tags: w.tags,
        });
    }
    Ok(out)
}

fn list(args: &Args, run: &mut Run, only: &Only) -> Result<i32> {
    let pbfs = pbf_paths(args, USAGE)?;
    let near: f64 = args
        .value("near-mi")
        .map(|v| {
            v.parse()
                .map_err(|_| format!("--near-mi {v} is not a number"))
        })
        .transpose()?
        .unwrap_or(CANDIDATE_NEAR_MI);
    let found = read_candidates(&pbfs)?;
    println!("{} candidates in the extracts", found.len());
    let mut grid = Grid::new(CELL_DEG);
    for (i, c) in found.iter().enumerate() {
        grid.insert_point(c.lat, c.lon, i as u32);
    }
    let archive = std::mem::take(&mut run.archive);
    let reach = (near / 69.0 / CELL_DEG).ceil() as i64 + 1;
    for leg in run.legs_mut()?.iter() {
        let id = leg_id(leg);
        if !only.wants(&id) {
            continue;
        }
        let Some((line, coarse)) = leg_road(leg, &archive) else {
            println!("{id}: no road stored");
            continue;
        };
        let route = Route::new(&line, leg_miles(leg));
        let mut rows: Vec<(f64, String)> = Vec::new();
        for i in grid.along(&route.points, reach) {
            let c = &found[i as usize];
            let (at, off, _, _) = route.project(c.lat, c.lon);
            if off > near {
                continue;
            }
            let shown: Vec<String> = SHOWN
                .iter()
                .filter_map(|k| tag(&c.tags, k).map(|v| format!("{k}={v}")))
                .collect();
            rows.push((
                at,
                format!(
                    "  {at:6.1} mi  {off:4.2} off  {:<26} {} ({:.5}, {:.5})  {}",
                    c.kind,
                    c.osm,
                    c.lat,
                    c.lon,
                    shown.join(" ")
                ),
            ));
        }
        rows.sort_by(|a, b| a.0.total_cmp(&b.0));
        let road = if coarse { ", coarse route points" } else { "" };
        println!(
            "{id} ({} mi{road}): {} candidates",
            leg_miles(leg),
            rows.len()
        );
        for (_, row) in rows {
            println!("{row}");
        }
    }
    run.archive = archive;
    Ok(0)
}

/// One curated stop, as the list file carries it.
#[derive(Debug, Clone)]
pub struct Curated {
    pub record: Object,
    pub lat: f64,
    pub lon: f64,
    pub reach_mi: f64,
    pub season: String,
    /// The one way of travel that can turn in ("Northbound"), for a stop on
    /// one side of a divided road; `None` when both ways can.
    pub heading: Option<String>,
}

/// The list file: `{"stops": [{"name", "lat", "lon", "type", "source", ...}]}`.
/// `lat`, `lon`, `reach_mi`, `season` and `heading` steer placement and are not written
/// to the leg; every other key is the stop record as the game reads it.
pub fn parse_curated(text: &str) -> Result<Vec<Curated>> {
    let root = crate::pyjson::loads(text).map_err(|e| format!("stop list: {e}"))?;
    let stops = root
        .get("stops")
        .and_then(Value::as_array)
        .ok_or("stop list has no \"stops\" array")?;
    let mut out = Vec::new();
    for s in stops {
        let obj = s.as_object().ok_or("a stop is not an object")?;
        let name = obj.get("name").map(Value::py_str).unwrap_or_default();
        let num = |k: &str| obj.get(k).and_then(Value::as_f64);
        let (Some(lat), Some(lon)) = (num("lat"), num("lon")) else {
            return Err(format!("stop {name:?} has no lat and lon"));
        };
        for key in ["name", "type", "source", "actions", "services"] {
            if obj.get(key).is_none() {
                return Err(format!("stop {name:?} has no {key}"));
            }
        }
        let mut record = Object::new();
        for (k, v) in obj.iter() {
            if !matches!(
                k.as_str(),
                "lat" | "lon" | "reach_mi" | "season" | "heading"
            ) {
                record.insert(k.as_str(), v.clone());
            }
        }
        // The name is spoken: script a voice cannot read comes out, and a
        // name left with nothing readable is an error in the list.
        let spoken = crate::world::spoken_name::stray_capitals_fixed(
            &crate::world::spoken_name::latin_only(&name),
        );
        if spoken.is_empty() {
            return Err(format!("stop {name:?} has no name a voice can read"));
        }
        if spoken != name {
            record.insert("name", Value::Str(spoken));
        }
        out.push(Curated {
            record,
            lat,
            lon,
            reach_mi: num("reach_mi").unwrap_or(PLACE_REACH_MI),
            season: obj
                .get("season")
                .map(Value::py_str)
                .unwrap_or_else(|| "year round".to_string()),
            heading: obj.get("heading").map(Value::py_str),
        });
    }
    Ok(out)
}

/// How many of these stops say their truck access was read, and how many
/// say it was assumed (`vehicle_access_basis`).
pub fn access_tally<'a>(stops: impl Iterator<Item = &'a Curated>) -> (usize, usize) {
    let (mut read, mut assumed) = (0, 0);
    for c in stops {
        match c
            .record
            .get("vehicle_access_basis")
            .map(Value::py_str)
            .as_deref()
        {
            Some("read") => read += 1,
            Some("assumed") => assumed += 1,
            _ => {}
        }
    }
    (read, assumed)
}

/// The listed names this leg's road passes within reach of. Only these are
/// the list's on this leg: a stop elsewhere that happens to share a listed
/// name (a Petro-Canada in Washington, a Chevron anywhere) is another stop.
/// Season and heading do not matter here, so a stop that turned seasonal or
/// one-sided is still the list's to take off the leg.
pub fn owned_names(route: &Route, list: &[Curated]) -> Vec<String> {
    list.iter()
        .filter(|c| route.project(c.lat, c.lon).1 <= c.reach_mi)
        .map(|c| c.record.get("name").map(Value::py_str).unwrap_or_default())
        .collect()
}

/// A leg's stops after the list's: the leg's own stops of an owned name are
/// replaced by `records`, everything else is kept, in mile order. `None`
/// when the list has nothing to do with this leg, so it is left as it is,
/// not even re-sorted.
pub fn merged_rows(
    existing: &[Value],
    records: Vec<Object>,
    owned: &[String],
) -> Option<Vec<Value>> {
    let name = |s: &Value| s.get("name").map(Value::py_str).unwrap_or_default();
    if records.is_empty() && !existing.iter().any(|s| owned.contains(&name(s))) {
        return None;
    }
    let mut rows: Vec<Value> = existing
        .iter()
        .filter(|s| !owned.contains(&name(s)))
        .cloned()
        .collect();
    rows.extend(records.into_iter().map(Value::Object));
    let at = |v: &Value| v.get("at_mi").and_then(Value::as_f64).unwrap_or(0.0);
    rows.sort_by(|a, b| at(a).total_cmp(&at(b)));
    Some(rows)
}

/// Whether the road at `at_mi` runs the way `heading` names.
pub fn runs(route: &Route, at_mi: f64, heading: &str) -> bool {
    let i = route
        .cum_mi
        .iter()
        .position(|m| *m >= at_mi)
        .unwrap_or(route.cum_mi.len() - 1)
        .max(1);
    let (a, b) = (route.points[i - 1], route.points[i]);
    let (north, east) = (b.0 - a.0, (b.1 - a.1) * a.0.to_radians().cos());
    match heading {
        "Northbound" => north > 0.0,
        "Southbound" => north < 0.0,
        "Eastbound" => east > 0.0,
        "Westbound" => east < 0.0,
        _ => true,
    }
}

/// The stop records one leg gets from the list, in mile order.
#[cfg(test)]
pub fn place_on_leg(route: &Route, miles: f64, coarse: bool, list: &[Curated]) -> Vec<Object> {
    placed_on_leg(route, miles, coarse, list)
        .into_iter()
        .map(|(_, r)| r)
        .collect()
}

/// [`place_on_leg`] with each record's index in the list.
fn placed_on_leg(
    route: &Route,
    miles: f64,
    coarse: bool,
    list: &[Curated],
) -> Vec<(usize, Object)> {
    let mut out: Vec<(usize, Object)> = Vec::new();
    for (k, c) in list
        .iter()
        .enumerate()
        .filter(|(_, c)| c.season == "year round")
    {
        let (at, off, _, _) = route.project(c.lat, c.lon);
        if off > c.reach_mi {
            continue;
        }
        if c.heading.as_deref().is_some_and(|h| !runs(route, at, h)) {
            continue;
        }
        // A stop at either end is in the end city, which the game serves
        // on its own; the loader takes a stop only strictly inside the leg.
        let at = round(at, 1);
        if at <= 0.0 || at >= miles {
            continue;
        }
        let mut record = c.record.clone();
        record.insert("at_mi", Value::Float(at));
        if coarse {
            let source = record.get("source").map(Value::py_str).unwrap_or_default();
            record.insert(
                "source",
                Value::Str(format!(
                    "{source} Mile measured on the leg's coarse stored route points, so it may be a few miles out."
                )),
            );
        }
        out.push((k, record));
    }
    let at = |r: &Object| r.get("at_mi").and_then(Value::as_f64).unwrap_or(0.0);
    out.sort_by(|a, b| at(&a.1).total_cmp(&at(&b.1)));
    out
}

fn place(args: &Args, run: &mut Run, only: &Only, path: &str) -> Result<i32> {
    let list = parse_curated(&crate::util::read_text(std::path::Path::new(path))?)?;
    for c in list.iter().filter(|c| c.season != "year round") {
        let name = c.record.get("name").map(Value::py_str).unwrap_or_default();
        println!("left out, open {}: {name}", c.season);
    }
    let archive = std::mem::take(&mut run.archive);
    let (mut placed, mut changed) = (0, 0);
    let mut used = vec![false; list.len()];
    for leg in run.legs_mut()?.iter_mut() {
        let id = leg_id(leg);
        if !only.wants(&id) {
            continue;
        }
        let Some((line, coarse)) = leg_road(leg, &archive) else {
            continue;
        };
        let miles = leg_miles(leg);
        let route = Route::new(&line, miles);
        let found = placed_on_leg(&route, miles, coarse, &list);
        for (k, _) in &found {
            used[*k] = true;
        }
        let records: Vec<Object> = found.into_iter().map(|(_, r)| r).collect();
        let owned = owned_names(&route, &list);
        placed += records.len();
        let summary: Vec<String> = records
            .iter()
            .map(|r| {
                format!(
                    "{} ({})",
                    r.get("name").map(Value::py_str).unwrap_or_default(),
                    r.get("at_mi").map(Value::py_str).unwrap_or_default()
                )
            })
            .collect();
        if !summary.is_empty() {
            println!("  {id}: {}", summary.join("; "));
        }
        let obj = leg
            .as_object_mut()
            .ok_or_else(|| format!("{id} is not an object"))?;
        let existing: Vec<Value> = obj
            .get("stops")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let Some(rows) = merged_rows(&existing, records, &owned) else {
            continue;
        };
        let rows = Value::Array(rows);
        if obj.get("stops") != Some(&rows) {
            changed += 1;
            obj.insert("stops", rows);
        }
        // Keep the corridor object a leg is expected to carry.
        let _ = corridor_mut(leg)?;
    }
    run.archive = archive;
    for (c, used) in list.iter().zip(&used) {
        if !*used && c.season == "year round" {
            let name = c.record.get("name").map(Value::py_str).unwrap_or_default();
            println!("on no leg in --only: {name}");
        }
    }
    println!("{placed} stops placed; {changed} legs changed.");
    let (read, assumed) = access_tally(list.iter().zip(&used).filter(|(_, u)| **u).map(|(c, _)| c));
    println!(
        "truck access on the placed stops: {read} read, {assumed} assumed{}",
        if assumed > read {
            " -- mostly assumed"
        } else {
            ""
        }
    );
    run.finish(args.flag("write"))
}

pub fn main(argv: &[String]) -> Result<i32> {
    let args = Args::parse(
        argv,
        &Spec {
            values: &["pbf", "only", "near-mi", "stops"],
            flags: &["write"],
        },
    )
    .map_err(|e| format!("{e}\n{USAGE}"))?;
    let only = Only::parse(&args);
    let mut run = Run::load()?;
    match args.value("stops") {
        Some(path) => {
            let path = path.to_string();
            place(&args, &mut run, &only, &path)
        }
        None => list(&args, &mut run, &only),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn candidates_are_fuel_rest_areas_and_truck_parking() {
        assert_eq!(
            candidate_kind(&[("amenity", "fuel"), ("fuel:diesel", "yes")]),
            Some("fuel")
        );
        assert_eq!(
            candidate_kind(&[("amenity", "fuel")]),
            Some("fuel (diesel not tagged)")
        );
        assert_eq!(
            candidate_kind(&[("highway", "rest_area")]),
            Some("rest area")
        );
        assert_eq!(
            candidate_kind(&[("amenity", "parking"), ("hgv", "designated")]),
            Some("truck parking")
        );
        assert_eq!(candidate_kind(&[("amenity", "parking")]), None);
        assert_eq!(candidate_kind(&[("tourism", "motel")]), None);
    }

    #[test]
    fn access_is_tallied_read_and_assumed() {
        let list = parse_curated(
            r#"{"stops": [
                {"name": "A", "lat": 1, "lon": 1, "type": "fuel_station", "source": "s",
                 "actions": ["fuel"], "services": ["diesel"], "vehicle_access_basis": "read"},
                {"name": "B", "lat": 1, "lon": 1, "type": "fuel_station", "source": "s",
                 "actions": ["fuel"], "services": ["diesel"], "vehicle_access_basis": "assumed"},
                {"name": "C", "lat": 1, "lon": 1, "type": "fuel_station", "source": "s",
                 "actions": ["fuel"], "services": ["diesel"], "vehicle_access_basis": "assumed"},
                {"name": "D", "lat": 1, "lon": 1, "type": "fuel_station", "source": "s",
                 "actions": ["fuel"], "services": ["diesel"]}
            ]}"#,
        )
        .unwrap();
        assert_eq!(access_tally(list.iter()), (1, 2));
    }

    fn list() -> Vec<Curated> {
        parse_curated(
            r#"{"stops": [
                {"name": "Near", "lat": 30.5, "lon": -97.003, "type": "fuel_station",
                 "source": "Operator page.", "actions": ["fuel"], "services": ["diesel"]},
                {"name": "Far", "lat": 30.5, "lon": -96.9, "type": "fuel_station",
                 "source": "Operator page.", "actions": ["fuel"], "services": ["diesel"]},
                {"name": "Summer", "lat": 30.2, "lon": -97.0, "type": "fuel_station",
                 "season": "May to September",
                 "source": "Operator page.", "actions": ["fuel"], "services": ["diesel"]}
            ]}"#,
        )
        .unwrap()
    }

    #[test]
    fn a_curated_stop_lands_on_the_legs_its_road_passes() {
        let line = LegLine::new(vec![(30.0, -97.0), (31.0, -97.0)]);
        let route = Route::new(&line, 100.0);
        let placed = place_on_leg(&route, 100.0, false, &list());
        let names: Vec<String> = placed
            .iter()
            .map(|r| r.get("name").map(Value::py_str).unwrap_or_default())
            .collect();
        assert_eq!(names, vec!["Near"], "far and seasonal stops stay off");
        assert_eq!(placed[0].get("at_mi").and_then(Value::as_f64), Some(50.0));
        assert!(placed[0].get("lat").is_none() && placed[0].get("season").is_none());
    }

    #[test]
    fn a_stop_far_from_the_list_keeps_its_shared_name() {
        // "Near" is listed at (30.5, -97.003). A leg 300 miles away holds a
        // stop of the same name: it is not the list's, so the leg is left
        // alone. The leg the listed stop lies on gets it replaced.
        let far = Route::new(&LegLine::new(vec![(35.0, -97.0), (36.0, -97.0)]), 69.0);
        let owned = owned_names(&far, &list());
        assert!(owned.is_empty(), "{owned:?}");
        let theirs =
            crate::pyjson::loads(r#"[{"name": "Near", "at_mi": 10.0, "source": "theirs"}]"#)
                .unwrap();
        let theirs = theirs.as_array().unwrap().clone();
        assert!(merged_rows(&theirs, Vec::new(), &owned).is_none());

        let near = Route::new(&LegLine::new(vec![(30.0, -97.0), (31.0, -97.0)]), 100.0);
        let owned = owned_names(&near, &list());
        assert!(owned.contains(&"Near".to_string()));
        let records = place_on_leg(&near, 100.0, false, &list());
        let rows = merged_rows(&theirs, records, &owned).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(
            rows[0].get("source").map(Value::py_str).as_deref(),
            Some("Operator page.")
        );
    }

    #[test]
    fn a_one_sided_stop_is_on_the_leg_that_passes_its_side() {
        let mut one = list();
        one.truncate(1);
        one[0].heading = Some("Northbound".into());
        let north = Route::new(&LegLine::new(vec![(30.0, -97.0), (31.0, -97.0)]), 100.0);
        let south = Route::new(&LegLine::new(vec![(31.0, -97.0), (30.0, -97.0)]), 100.0);
        assert_eq!(place_on_leg(&north, 100.0, false, &one).len(), 1);
        assert!(place_on_leg(&south, 100.0, false, &one).is_empty());
    }

    #[test]
    fn a_mile_on_coarse_route_points_says_so() {
        let line = LegLine::new(vec![(30.0, -97.0), (31.0, -97.0)]);
        let route = Route::new(&line, 100.0);
        let placed = place_on_leg(&route, 100.0, true, &list());
        let source = placed[0]
            .get("source")
            .map(Value::py_str)
            .unwrap_or_default();
        assert!(source.contains("coarse stored route points"), "{source}");
    }

    #[test]
    fn a_stop_without_a_source_is_refused() {
        let err = parse_curated(
            r#"{"stops": [{"name": "X", "lat": 1, "lon": 2, "type": "fuel_station",
                "actions": [], "services": []}]}"#,
        )
        .unwrap_err();
        assert!(err.contains("source"), "{err}");
    }
}
