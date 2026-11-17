//! `osm-restrictions`: the low bridges and weight limits a legal truck
//! still drives past (`corridor.restrictions`), from OpenStreetMap
//! `maxheight` and `maxweight` tags.
//!
//! A port of the deleted `tools/build_interchanges_restrictions.py`. A
//! mainline highway way (motorway, trunk, primary, secondary) whose tag
//! parses to a clearance from 9 to 16 feet, or a weight limit from 3 to
//! under 40 short tons, is snapped to the leg by its middle vertex: within
//! 100 m when its `ref` carries the leg's route number, 60 m when it does
//! not. It must also run along the leg (within 35 degrees of the road's
//! bearing there), which keeps a restricted street crossing over the highway
//! out. Tags within 0.3 mile of each other collapse to the lowest value at
//! the first mile. An empty list means the leg was swept and is clean.

use std::collections::HashMap;
use std::sync::LazyLock;

use regex::Regex;

use super::features::{self, tag, Select, Tags};
use super::{corridor_mut, highway, leg_id, leg_miles, pbf_paths, round, Only, Route, Run};
use crate::cli::{Args, Spec};
use crate::pyjson::{Object, Value};
use crate::util::Result;
use crate::world::osm::geo;

const USAGE: &str = "usage: osm-restrictions --pbf FILE [--pbf FILE ...] \
                     [--only from:to;from:to] [--extract-date YYYY-MM-DD] [--snapshot NAME] \
                     [--write]";

const CLASSES: &[&str] = &["motorway", "trunk", "primary", "secondary"];
pub const CORRIDOR_M: f64 = 100.0;
pub const CORRIDOR_REFLESS_M: f64 = 60.0;
pub const ALIGN_DEG: f64 = 35.0;
pub const MERGE_MI: f64 = 0.3;
const HEIGHT_MAX_FT: f64 = 16.0;
const HEIGHT_MIN_FT: f64 = 9.0;
const WEIGHT_MAX_TONS: f64 = 40.0;
const WEIGHT_MIN_TONS: f64 = 3.0;
const M_TO_FT: f64 = 3.28084;
const TONNE_TO_SHORT_TON: f64 = 1.1023113;
const LB_TO_SHORT_TON: f64 = 1.0 / 2000.0;
const KG_TO_SHORT_TON: f64 = 1.0 / 907.18474;

const UNTAGGED: &[&str] = &[
    "",
    "default",
    "none",
    "no",
    "unsigned",
    "no_indications",
    "no_sign",
    "below_default",
    "physical",
    "unknown",
];

static FEET_INCHES: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"^(\d+)\s*'\s*(\d+(?:\.\d+)?)?\s*"?$"#).unwrap());
static HEIGHT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(\d+(?:\.\d+)?)\s*(m|meter|meters|ft|feet)?$").unwrap());
static WEIGHT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(\d+(?:\.\d+)?)\s*(t|st|lbs|lb|kg)?$").unwrap());

fn source_note(snapshot: &str, date: &str) -> String {
    format!(
        "OpenStreetMap maxheight/maxweight tags on the corridor highway ways, read from the \
         {snapshot} of {date} and snapped to the archived route geometry. \
         https://www.openstreetmap.org/"
    )
}

fn normal(raw: &str) -> Option<String> {
    let text = raw.trim().to_lowercase().replace(',', ".");
    (!UNTAGGED.contains(&text.as_str())).then_some(text)
}

/// An OSM `maxheight` in feet: bare metres, "4.1 m", `13'6"`, "13.5 ft".
pub fn parse_maxheight_ft(raw: &str) -> Option<f64> {
    let text = normal(raw)?;
    if let Some(m) = FEET_INCHES.captures(&text) {
        let inches = m
            .get(2)
            .and_then(|g| g.as_str().parse::<f64>().ok())
            .unwrap_or(0.0);
        return Some(m[1].parse::<f64>().ok()? + inches / 12.0);
    }
    let m = HEIGHT.captures(&text)?;
    let value: f64 = m[1].parse().ok()?;
    Some(match m.get(2).map(|g| g.as_str()) {
        Some("ft" | "feet") => value,
        _ => value * M_TO_FT,
    })
}

/// An OSM `maxweight` in US short tons: bare numbers and "t" are tonnes.
pub fn parse_maxweight_tons(raw: &str) -> Option<f64> {
    let text = normal(raw)?;
    let m = WEIGHT.captures(&text)?;
    let value: f64 = m[1].parse().ok()?;
    Some(match m.get(2).map(|g| g.as_str()) {
        Some("st") => value,
        Some("lb" | "lbs") => value * LB_TO_SHORT_TON,
        Some("kg") => value * KG_TO_SHORT_TON,
        _ => value * TONNE_TO_SHORT_TON,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    LowClearance,
    WeightLimit,
}

impl Kind {
    fn name(self) -> &'static str {
        match self {
            Kind::LowClearance => "low_clearance",
            Kind::WeightLimit => "weight_limit",
        }
    }
}

/// The bakeable restrictions a way's tags declare.
pub fn way_restrictions(tags: &Tags) -> Vec<(Kind, f64)> {
    let mut out = Vec::new();
    if let Some(ft) = tag(tags, "maxheight").and_then(parse_maxheight_ft) {
        if (HEIGHT_MIN_FT..=HEIGHT_MAX_FT).contains(&ft) {
            out.push((Kind::LowClearance, round(ft, 2)));
        }
    }
    if let Some(t) = tag(tags, "maxweight").and_then(parse_maxweight_tons) {
        if (WEIGHT_MIN_TONS..WEIGHT_MAX_TONS).contains(&t) {
            out.push((Kind::WeightLimit, round(t, 1)));
        }
    }
    out
}

pub struct RestrictionWay {
    pub kind: Kind,
    pub value: f64,
    pub route_ref: String,
    pub coords: Vec<(f64, f64)>,
}

fn bearing_deg(a: (f64, f64), b: (f64, f64)) -> f64 {
    let (lat1, lon1, lat2, lon2) = (
        a.0.to_radians(),
        a.1.to_radians(),
        b.0.to_radians(),
        b.1.to_radians(),
    );
    let dlon = lon2 - lon1;
    let x = dlon.sin() * lat2.cos();
    let y = lat1.cos() * lat2.sin() - lat1.sin() * lat2.cos() * dlon.cos();
    x.atan2(y).to_degrees().rem_euclid(360.0)
}

fn axis_diff_deg(a: f64, b: f64) -> f64 {
    let diff = (a - b).abs() % 180.0;
    diff.min(180.0 - diff)
}

fn segment_bearing(points: &[(f64, f64)], i: usize) -> Option<f64> {
    if points.len() < 2 {
        return None;
    }
    let a = points[i.saturating_sub(1)];
    let b = points[(i + 1).min(points.len() - 1)];
    (a != b).then(|| bearing_deg(a, b))
}

fn first_digits(text: &str) -> &str {
    super::lanes::digit_groups(text)
        .first()
        .copied()
        .unwrap_or("")
}

/// The leg's vertices by fine grid cell, for the nearest-vertex snap.
struct VertexGrid(HashMap<geo::Cell, Vec<usize>>);

impl VertexGrid {
    fn new(route: &Route) -> Self {
        let mut grid: HashMap<geo::Cell, Vec<usize>> = HashMap::new();
        for (i, p) in route.points.iter().enumerate() {
            grid.entry(geo::cell(p.0, p.1)).or_default().push(i);
        }
        Self(grid)
    }

    /// The nearest vertex within a cell's reach (which covers 100 m).
    fn nearest(&self, route: &Route, lat: f64, lon: f64) -> Option<(usize, f64)> {
        let mut best: Option<(usize, f64)> = None;
        for c in geo::around(geo::cell(lat, lon)) {
            for &i in self.0.get(&c).into_iter().flatten() {
                let p = route.points[i];
                let d = geo::haversine_m(lat, lon, p.0, p.1);
                if best.is_none_or(|b| d < b.1 || (d == b.1 && i < b.0)) {
                    best = Some((i, d));
                }
            }
        }
        best
    }
}

/// One advisory: the kind, the first mile of its cluster, the lowest value.
#[derive(Debug, Clone, PartialEq)]
pub struct Advisory {
    pub at_mi: f64,
    pub kind: Kind,
    pub value: f64,
}

pub fn assemble(ways: &[&RestrictionWay], route: &Route, highway: &str) -> Vec<Advisory> {
    if route.points.len() < 2 {
        return Vec::new();
    }
    let leg_miles = route.total_mi();
    let shield = first_digits(highway);
    let grid = VertexGrid::new(route);
    let mut points: Vec<(f64, f64, Kind)> = Vec::new();
    for way in ways {
        let mid = way.coords.len() / 2;
        let (wlat, wlon) = way.coords[mid];
        let Some((gi, d)) = grid.nearest(route, wlat, wlon) else {
            continue;
        };
        let on_shield = !shield.is_empty() && first_digits(&way.route_ref) == shield;
        let corridor = if on_shield {
            CORRIDOR_M
        } else {
            CORRIDOR_REFLESS_M
        };
        if d > corridor {
            continue;
        }
        if let (Some(wb), Some(gb)) = (
            segment_bearing(&way.coords, mid),
            segment_bearing(&route.points, gi),
        ) {
            if axis_diff_deg(wb, gb) > ALIGN_DEG {
                continue;
            }
        }
        let at_mi = route.cum_mi[gi];
        if !(0.0 < at_mi && at_mi < leg_miles) {
            continue;
        }
        points.push((at_mi, way.value, way.kind));
    }
    points.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
    let mut out = Vec::new();
    for kind in [Kind::LowClearance, Kind::WeightLimit] {
        let mut cluster: Vec<(f64, f64)> = Vec::new();
        let emit = |cluster: &[(f64, f64)]| Advisory {
            at_mi: round(cluster.iter().map(|c| c.0).fold(f64::MAX, f64::min), 1),
            kind,
            value: cluster.iter().map(|c| c.1).fold(f64::MAX, f64::min),
        };
        for &(at_mi, value, k) in &points {
            if k != kind {
                continue;
            }
            if cluster.last().is_some_and(|l| at_mi - l.0 > MERGE_MI) {
                out.push(emit(&cluster));
                cluster.clear();
            }
            cluster.push((at_mi, value));
        }
        if !cluster.is_empty() {
            out.push(emit(&cluster));
        }
    }
    out.sort_by(|a, b| a.at_mi.total_cmp(&b.at_mi));
    out
}

pub fn advisory_row(a: &Advisory, source: &str) -> Value {
    let mut row = Object::new();
    row.insert("at_mi", Value::Float(a.at_mi));
    row.insert("kind", Value::Str(a.kind.name().into()));
    row.insert("source", Value::Str(source.into()));
    match a.kind {
        Kind::LowClearance => row.insert("feet", Value::Float(round(a.value, 2))),
        Kind::WeightLimit => row.insert("tons", Value::Float(round(a.value, 1))),
    }
    Value::Object(row)
}

pub fn read_ways(pbfs: &[std::path::PathBuf]) -> Result<Vec<RestrictionWay>> {
    let keep = |t: &[(&str, &str)]| {
        let has = |k: &str| t.iter().find(|(tk, _)| *tk == k).map(|(_, v)| *v);
        has("highway").is_some_and(|h| CLASSES.contains(&h))
            && (has("maxheight").is_some() || has("maxweight").is_some())
    };
    let found = features::read(
        pbfs,
        &Select {
            node: None,
            way: Some(&keep),
            relation: None,
        },
    )?;
    let mut out = Vec::new();
    for w in found.ways {
        let route_ref = tag(&w.tags, "ref").unwrap_or("").trim().to_string();
        for (kind, value) in way_restrictions(&w.tags) {
            out.push(RestrictionWay {
                kind,
                value,
                route_ref: route_ref.clone(),
                coords: w.coords.clone(),
            });
        }
    }
    Ok(out)
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
    let pbfs = pbf_paths(&args, USAGE)?;
    let date = super::extract_date(&args, &pbfs)?;
    let snapshot = args.string("snapshot", "OpenStreetMap planet file");
    let only = Only::parse(&args);
    let mut run = Run::load()?;
    let ways = read_ways(&pbfs)?;
    println!("{} restriction tags under the bake thresholds", ways.len());
    // Ways by their middle vertex's coarse cell, so a leg looks only at its own.
    let mut grid = super::Grid::new(0.02);
    for (i, w) in ways.iter().enumerate() {
        let (lat, lon) = w.coords[w.coords.len() / 2];
        grid.insert_point(lat, lon, i as u32);
    }
    let note = source_note(&snapshot, &date);
    let archive = std::mem::take(&mut run.archive);
    let (mut changed, mut same, mut no_archive, mut with) = (0, 0, 0, 0);
    for leg in run.legs_mut()?.iter_mut() {
        let id = leg_id(leg);
        if !only.wants(&id) {
            continue;
        }
        let Some(line) = archive.get(&id) else {
            no_archive += 1;
            continue;
        };
        let route = Route::new(line, leg_miles(leg));
        let near: Vec<&RestrictionWay> = grid
            .along(&route.points, 1)
            .into_iter()
            .map(|i| &ways[i as usize])
            .collect();
        let found = assemble(&near, &route, &highway(leg));
        if !found.is_empty() {
            with += 1;
            if only.is_some() {
                for a in &found {
                    println!("  {id}: {} {} at mile {}", a.kind.name(), a.value, a.at_mi);
                }
            }
        }
        let rows = Value::Array(found.iter().map(|a| advisory_row(a, &note)).collect());
        let corridor = corridor_mut(leg)?;
        if corridor.get("restrictions") == Some(&rows) {
            same += 1;
        } else {
            changed += 1;
            corridor.insert("restrictions", rows);
        }
    }
    run.archive = archive;
    println!(
        "{changed} legs changed, {same} the same, {with} carry an advisory, {no_archive} with no \
         archived road kept."
    );
    run.finish(args.flag("write"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::osm::geo::LegLine;

    #[test]
    fn clearances_and_weights_parse_in_their_units() {
        assert!((parse_maxheight_ft("13'6\"").unwrap() - 13.5).abs() < 1e-9);
        assert!((parse_maxheight_ft("4.1").unwrap() - 13.451).abs() < 1e-3);
        assert_eq!(parse_maxheight_ft("13.5 ft"), Some(13.5));
        assert_eq!(parse_maxheight_ft("default"), None);
        assert!((parse_maxweight_tons("10").unwrap() - 11.023).abs() < 1e-3);
        assert_eq!(parse_maxweight_tons("20 st"), Some(20.0));
        assert_eq!(parse_maxweight_tons("40000 lbs"), Some(20.0));
    }

    #[test]
    fn a_bridge_along_the_road_bakes_and_one_across_it_does_not() {
        let line = LegLine::new((0..=20).map(|i| (30.0 + 0.01 * i as f64, -97.0)).collect());
        let route = Route::new(&line, 13.8);
        let along = RestrictionWay {
            kind: Kind::LowClearance,
            value: 13.5,
            route_ref: "US 77".into(),
            coords: vec![(30.0495, -97.0002), (30.0502, -97.0002), (30.051, -97.0002)],
        };
        let across = RestrictionWay {
            kind: Kind::WeightLimit,
            value: 10.0,
            route_ref: String::new(),
            coords: vec![(30.15, -97.001), (30.15, -97.0), (30.15, -96.999)],
        };
        let found = assemble(&[&along, &across], &route, "US-77");
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].kind, Kind::LowClearance);
        assert_eq!(found[0].value, 13.5);
    }
}
