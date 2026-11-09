//! `osm-lanes`: how many lanes the road carries along each leg
//! (`corridor.lane_segments`), from the OpenStreetMap `lanes` tags.
//!
//! A port of the deleted `tools/bake_lane_segments.py`. Every quarter mile
//! along the archived road the nearest highway way within 90 m that carries
//! a clean `lanes` tag gives the sample its value, preferring a way that
//! carries the leg's route number. Samples collapse to a step function. An
//! untagged stretch is a gap, never a guess: the open segment closes at its
//! last matched sample. Segments under 0.3 mile are dropped unless they sit
//! within 0.4 mile of an exit, where lanes really do change fast, and equal
//! neighbours the drop left behind are merged. A leg with no tagged way
//! loses its `lane_segments` key altogether.

use std::collections::{HashMap, HashSet};

use super::features::{self, tag, Select, Tags};
use super::{corridor_mut, highway, leg_id, leg_miles, pbf_paths, round, Only, Run};
use crate::cli::{Args, Spec};
use crate::pyjson::{Object, Value};
use crate::util::Result;
use crate::world::osm::geo::{self, Cell, LegLine, M_PER_MI};

const USAGE: &str = "usage: osm-lanes --pbf FILE [--pbf FILE ...] [--only from:to;from:to] \
                     [--extract-date YYYY-MM-DD] [--snapshot NAME] [--write]";

pub const MATCH_CORRIDOR_M: f64 = 90.0;
pub const SAMPLE_STEP_M: f64 = 402.0;
pub const MIN_SEG_MI: f64 = 0.3;
pub const INTERCHANGE_TOL_MI: f64 = 0.4;
const LANES_MIN: i64 = 1;
const LANES_MAX: i64 = 10;
const CLASSES: &[&str] = &["motorway", "trunk", "primary", "secondary", "tertiary"];

fn source_note(snapshot: &str, date: &str) -> String {
    format!(
        "OpenStreetMap lanes tags on the corridor highway ways, read from the {snapshot} of \
         {date} and matched within 90 m of the archived route; total per the OSM lanes \
         semantics (directional on oneway carriageways). ODbL, (c) OpenStreetMap contributors."
    )
}

fn clean_int(raw: Option<&str>) -> Option<i64> {
    let s = raw?.trim();
    let digits = s.strip_prefix('-').unwrap_or(s);
    if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    s.parse().ok()
}

/// What one way says about its lanes, or `None` when it has no usable count.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct LaneValue {
    pub lanes: i64,
    pub forward: Option<i64>,
    pub backward: Option<i64>,
    pub oneway: bool,
}

pub fn lanes_from_tags(tags: &Tags) -> Option<LaneValue> {
    let ok = |n: &i64| (LANES_MIN..=LANES_MAX).contains(n);
    let lanes = clean_int(tag(tags, "lanes")).filter(ok)?;
    let oneway = tag(tags, "oneway")
        .map(|v| v.trim().to_lowercase())
        .is_some_and(|v| ["yes", "true", "1", "-1"].contains(&v.as_str()));
    Some(LaneValue {
        lanes,
        forward: clean_int(tag(tags, "lanes:forward")).filter(ok),
        backward: clean_int(tag(tags, "lanes:backward")).filter(ok),
        oneway,
    })
}

/// A lane-tagged way: its value, its `ref`, its line.
pub struct LaneWay {
    pub value: LaneValue,
    pub route_ref: String,
    pub coords: Vec<(f64, f64)>,
}

/// Lane ways bucketed by the fine grid of `osm::geo`.
pub struct LaneIndex {
    ways: Vec<LaneWay>,
    grid: HashMap<Cell, Vec<(u32, u32)>>,
}

impl LaneIndex {
    pub fn build(ways: Vec<LaneWay>, wanted: &HashSet<Cell>) -> Self {
        let mut index = LaneIndex {
            ways: Vec::new(),
            grid: HashMap::new(),
        };
        for way in ways {
            let wi = index.ways.len() as u32;
            let mut kept = false;
            for (si, pair) in way.coords.windows(2).enumerate() {
                let mut seen: Vec<Cell> = Vec::new();
                for c in geo::segment_cells(pair[0], pair[1]) {
                    if !seen.contains(&c) && wanted.contains(&c) {
                        seen.push(c);
                        index.grid.entry(c).or_default().push((wi, si as u32));
                        kept = true;
                    }
                }
            }
            if kept {
                index.ways.push(way);
            }
        }
        index
    }

    pub fn len(&self) -> usize {
        self.ways.len()
    }

    /// The nearest way's value within the corridor, a way carrying one of
    /// `shield`'s numbers first.
    fn value_at(&self, lat: f64, lon: f64, shield: &[&str], coslat: f64) -> Option<&LaneValue> {
        let mut nearest: HashMap<u32, f64> = HashMap::new();
        for c in geo::around(geo::cell(lat, lon)) {
            for &(wi, si) in self.grid.get(&c).into_iter().flatten() {
                let coords = &self.ways[wi as usize].coords;
                let si = si as usize;
                let d = geo::seg_dist_m(lat, lon, coords[si], coords[si + 1], coslat);
                if d <= MATCH_CORRIDOR_M {
                    let slot = nearest.entry(wi).or_insert(f64::INFINITY);
                    *slot = slot.min(d);
                }
            }
        }
        let mut ranked: Vec<(bool, f64, u32)> = nearest
            .into_iter()
            .map(|(wi, d)| {
                let on = ref_matches(&self.ways[wi as usize].route_ref, shield);
                (on, d, wi)
            })
            .collect();
        ranked.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.total_cmp(&b.1)).then(a.2.cmp(&b.2)));
        ranked.first().map(|r| &self.ways[r.2 as usize].value)
    }
}

pub fn digit_groups(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = None;
    for (i, c) in text.char_indices() {
        match (c.is_ascii_digit(), start) {
            (true, None) => start = Some(i),
            (false, Some(s)) => {
                out.push(&text[s..i]);
                start = None;
            }
            _ => {}
        }
    }
    if let Some(s) = start {
        out.push(&text[s..]);
    }
    out
}

fn ref_matches(route_ref: &str, shield: &[&str]) -> bool {
    !shield.is_empty() && digit_groups(route_ref).iter().any(|d| shield.contains(d))
}

/// One baked lane segment.
#[derive(Debug, Clone, PartialEq)]
pub struct Segment {
    pub start_mi: f64,
    pub end_mi: f64,
    pub value: LaneValue,
}

/// Sample, step, drop slivers, merge. Returns the segments and the miles
/// they cover.
pub fn build_segments(
    line: &LegLine,
    leg_miles: f64,
    index: &LaneIndex,
    highway: &str,
    interchange_mi: &[f64],
) -> (Vec<Segment>, f64) {
    let raw_mi = line.length_m() / M_PER_MI;
    let scale = if leg_miles > 0.0 && raw_mi > 0.0 {
        leg_miles / raw_mi
    } else {
        1.0
    };
    let mile = |m: f64| round(m / M_PER_MI * scale, 1);
    let total_mi = mile(line.length_m());
    let shield = digit_groups(highway);
    let mean_lat = line.points.iter().map(|p| p.0).sum::<f64>() / line.points.len() as f64;
    let coslat = mean_lat.to_radians().cos();

    let mut samples: Vec<(f64, Option<&LaneValue>)> = Vec::new();
    let mut last_m = -1e9;
    let n = line.points.len();
    for (i, &(lat, lon)) in line.points.iter().enumerate() {
        if line.cum_m[i] - last_m < SAMPLE_STEP_M && i != n - 1 {
            continue;
        }
        last_m = line.cum_m[i];
        samples.push((
            mile(line.cum_m[i]),
            index.value_at(lat, lon, &shield, coslat),
        ));
    }

    // Step function: a gap closes the open segment at its last matched
    // sample; a change closes it at the transition.
    let mut segs: Vec<Segment> = Vec::new();
    let mut cur: Option<(Segment, f64)> = None; // (segment, last matched mile)
    for (at_mi, val) in samples {
        let Some(val) = val else {
            if let Some((mut s, last)) = cur.take() {
                s.end_mi = last;
                segs.push(s);
            }
            continue;
        };
        if let Some((s, last)) = cur.as_mut() {
            if &s.value == val {
                *last = at_mi;
                continue;
            }
        }
        if let Some((mut s, _)) = cur.take() {
            s.end_mi = at_mi;
            segs.push(s);
        }
        cur = Some((
            Segment {
                start_mi: at_mi,
                end_mi: at_mi,
                value: val.clone(),
            },
            at_mi,
        ));
    }
    if let Some((mut s, _)) = cur {
        s.end_mi = total_mi;
        segs.push(s);
    }

    let kept: Vec<Segment> = segs
        .into_iter()
        .filter(|s| {
            let length = s.end_mi - s.start_mi;
            if length <= 0.0 {
                return false;
            }
            let at_exit = interchange_mi.iter().any(|m| {
                (s.start_mi - m).abs() <= INTERCHANGE_TOL_MI
                    || (s.end_mi - m).abs() <= INTERCHANGE_TOL_MI
            });
            length >= MIN_SEG_MI || at_exit
        })
        .collect();
    let mut merged: Vec<Segment> = Vec::new();
    for s in kept {
        if let Some(last) = merged.last_mut() {
            if last.value == s.value && (last.end_mi - s.start_mi).abs() <= 0.15 {
                last.end_mi = s.end_mi;
                continue;
            }
        }
        merged.push(s);
    }
    let covered = merged.iter().map(|s| s.end_mi - s.start_mi).sum::<f64>();
    for s in &mut merged {
        s.start_mi = round(s.start_mi, 1);
        s.end_mi = round(s.end_mi, 1);
    }
    (merged, round(covered, 1))
}

pub fn segment_row(s: &Segment, source: &str) -> Value {
    let mut row = Object::new();
    row.insert("start_mi", Value::Float(s.start_mi));
    row.insert("end_mi", Value::Float(s.end_mi));
    row.insert("lanes", Value::Int(i128::from(s.value.lanes)));
    if let Some(f) = s.value.forward {
        row.insert("lanes_forward", Value::Int(i128::from(f)));
    }
    if let Some(b) = s.value.backward {
        row.insert("lanes_backward", Value::Int(i128::from(b)));
    }
    if s.value.oneway {
        row.insert("oneway", Value::Bool(true));
    }
    row.insert("source", Value::Str(source.to_string()));
    Value::Object(row)
}

fn interchange_miles(leg: &Value) -> Vec<f64> {
    leg.get("corridor")
        .and_then(|c| c.get("interchanges"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|ic| ic.get("at_mi").and_then(Value::as_f64))
        .collect()
}

pub fn read_index(pbfs: &[std::path::PathBuf], lines: &[&LegLine]) -> Result<LaneIndex> {
    let keep = |t: &[(&str, &str)]| {
        let has = |k: &str| t.iter().find(|(tk, _)| *tk == k).map(|(_, v)| *v);
        has("highway").is_some_and(|h| CLASSES.contains(&h)) && has("lanes").is_some()
    };
    let found = features::read(
        pbfs,
        &Select {
            node: None,
            way: Some(&keep),
            relation: None,
        },
    )?;
    let ways = found
        .ways
        .into_iter()
        .filter_map(|w| {
            Some(LaneWay {
                value: lanes_from_tags(&w.tags)?,
                route_ref: tag(&w.tags, "ref").unwrap_or("").trim().to_string(),
                coords: w.coords,
            })
        })
        .collect();
    let cells = geo::corridor_cells(lines.iter().copied());
    Ok(LaneIndex::build(ways, &cells))
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
    let ids: Vec<String> = run
        .world
        .get("legs")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(leg_id)
        .filter(|id| only.wants(id))
        .collect();
    let lines: Vec<&LegLine> = ids.iter().filter_map(|id| run.archive.get(id)).collect();
    let index = read_index(&pbfs, &lines)?;
    println!("{} lane-tagged ways along the selected legs", index.len());
    let note = source_note(&snapshot, &date);

    let archive = std::mem::take(&mut run.archive);
    let (mut changed, mut same, mut cleared, mut no_archive) = (0, 0, 0, 0);
    let (mut covered, mut total) = (0.0, 0.0);
    for leg in run.legs_mut()?.iter_mut() {
        let id = leg_id(leg);
        if !only.wants(&id) {
            continue;
        }
        let Some(line) = archive.get(&id) else {
            no_archive += 1;
            continue;
        };
        let miles = leg_miles(leg);
        let (segs, cov) =
            build_segments(line, miles, &index, &highway(leg), &interchange_miles(leg));
        covered += cov;
        total += miles;
        let corridor = corridor_mut(leg)?;
        if segs.is_empty() {
            if corridor.remove("lane_segments").is_some() {
                cleared += 1;
            }
            continue;
        }
        let rows = Value::Array(segs.iter().map(|s| segment_row(s, &note)).collect());
        if corridor.get("lane_segments") == Some(&rows) {
            same += 1;
        } else {
            changed += 1;
            corridor.insert("lane_segments", rows);
        }
    }
    run.archive = archive;
    let pct = if total > 0.0 {
        100.0 * covered / total
    } else {
        0.0
    };
    println!(
        "{changed} legs changed, {same} the same, {cleared} lost a lane profile with no tagged \
         way left, {no_archive} with no archived road kept. Lane tags cover {covered:.0} of \
         {total:.0} miles ({pct:.0}%)."
    );
    run.finish(args.flag("write"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tags(pairs: &[(&str, &str)]) -> Tags {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect()
    }

    #[test]
    fn lane_tags_parse_as_osm_means_them() {
        assert_eq!(lanes_from_tags(&tags(&[("lanes", "2;3")])), None);
        assert_eq!(lanes_from_tags(&tags(&[("lanes", "12")])), None);
        let v = lanes_from_tags(&tags(&[
            ("lanes", "3"),
            ("lanes:forward", "1"),
            ("lanes:backward", "2"),
        ]))
        .unwrap();
        assert_eq!(
            (v.lanes, v.forward, v.backward, v.oneway),
            (3, Some(1), Some(2), false)
        );
        assert!(
            lanes_from_tags(&tags(&[("lanes", "2"), ("oneway", "yes")]))
                .unwrap()
                .oneway
        );
    }

    #[test]
    fn a_gap_closes_the_segment_and_a_sliver_goes() {
        // A ten-mile road due north; two-lane way for the first four miles,
        // nothing for a mile, three-lane way after.
        let mut pts = vec![];
        let n = 100;
        for i in 0..=n {
            pts.push((30.0 + (10.0 / 69.09) * i as f64 / n as f64, -97.0));
        }
        let line = LegLine::new(pts);
        let way = |lanes, from: f64, to: f64| LaneWay {
            value: LaneValue {
                lanes,
                forward: None,
                backward: None,
                oneway: true,
            },
            route_ref: "I 35".into(),
            coords: vec![
                (30.0 + from / 69.09, -97.0003),
                (30.0 + to / 69.09, -97.0003),
            ],
        };
        let cells = geo::corridor_cells(std::iter::once(&line));
        let index = LaneIndex::build(vec![way(2, 0.0, 4.0), way(3, 5.0, 10.0)], &cells);
        let (segs, _) = build_segments(&line, 10.0, &index, "I-35", &[]);
        assert_eq!(segs.len(), 2, "{segs:?}");
        assert_eq!(segs[0].value.lanes, 2);
        assert!(segs[0].end_mi <= 4.1 && segs[1].start_mi >= 4.9, "{segs:?}");
        assert_eq!(segs[1].end_mi, 10.0);
    }
}
