//! `hpms-traffic`: how busy the road is along each leg
//! (`corridor.traffic_aadt`) and how many lanes it carries each way (the
//! leg's `lanes`), from HPMS traffic counts.
//!
//! A port of the deleted `tools/build_traffic_aadt.py`. HPMS gives an AADT
//! and a through-lane count per road section. The leg asks for the sections
//! of its own class (interstate legs functional system 1 only, which keeps
//! frontage roads and parallel arterials out; other legs systems 2 to 4) in
//! 60-mile boxes along its archived road, snaps each section vertex within
//! 250 m of the road, and takes the median of every 5-mile window that has
//! at least four of them, rounded to the nearest 500. An interstate window
//! ignores counts under 3,000 (no interstate mainline carries so few), a
//! lone deep dip between two agreeing neighbours is carried across as a
//! snapping artifact, and a lane count is floored at what the volume needs.
//! A leg with no section on its road keeps what it has.

use rayon::prelude::*;
use std::collections::HashMap;

use super::Client;
use crate::cli::{Args, Spec};
use crate::pyjson::{Object, Value};
use crate::util::Result;
use crate::world::layers::{corridor_mut, highway, leg_id, leg_miles, Only, Route, Run};
use crate::world::osm::geo::haversine_m;

const USAGE: &str = "usage: hpms-traffic [--only from:to;from:to] [--refresh] [--write]";

pub const ENVELOPE_SPAN_MI: f64 = 60.0;
pub const ENVELOPE_PAD_M: f64 = 2500.0;
pub const MAX_ALLOWABLE_OFFSET_DEG: f64 = 0.0005;
pub const SNAP_CORRIDOR_M: f64 = 250.0;
pub const SAMPLE_STRIDE_MI: f64 = 5.0;
pub const AADT_ROUND: f64 = 500.0;
pub const PAGE_SIZE: usize = 2000;
pub const MIN_WINDOW_POINTS: usize = 4;
pub const INTERSTATE_AADT_FLOOR: f64 = 3000.0;
pub const LONE_DIP_RATIO: f64 = 0.35;
const GRID_DEG: f64 = 0.05;

pub fn source(accessed: &str) -> String {
    format!(
        "Derived from FHWA Highway Performance Monitoring System (HPMS) AADT and through-lane \
         counts: median of HPMS sections snapping within 250 m of a 5-mile window of the leg, \
         rounded to the nearest 500, lone dips between agreeing neighbours carried across. \
         Sections queried from the national ArcGIS Living Atlas layer (Federal User Community \
         hpms_v2_view) and snapped to checked-in route geometry, accessed {accessed}: \
         https://www.fhwa.dot.gov/policyinformation/hpms.cfm"
    )
}

pub fn is_interstate(highway: &str) -> bool {
    let h = highway.trim();
    let mut chars = h.chars();
    matches!(chars.next(), Some('I' | 'i')) && matches!(chars.next(), Some('-' | ' ' | '\t'))
}

/// `(xmin, ymin, xmax, ymax)` boxes covering the road in 60-mile spans.
pub fn envelopes(route: &Route) -> Vec<(f64, f64, f64, f64)> {
    let mut spans: Vec<Vec<(f64, f64)>> = Vec::new();
    let mut span: Vec<(f64, f64)> = Vec::new();
    let mut start = 0.0;
    for (p, cum) in route.points.iter().zip(&route.cum_mi) {
        span.push(*p);
        if cum - start >= ENVELOPE_SPAN_MI {
            spans.push(std::mem::replace(&mut span, vec![*p]));
            start = *cum;
        }
    }
    if span.len() > 1 {
        spans.push(span);
    }
    spans
        .iter()
        .map(|chunk| {
            let (mut lo_lat, mut lo_lon, mut hi_lat, mut hi_lon) =
                (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
            for &(lat, lon) in chunk {
                lo_lat = lo_lat.min(lat);
                hi_lat = hi_lat.max(lat);
                lo_lon = lo_lon.min(lon);
                hi_lon = hi_lon.max(lon);
            }
            let mid = (lo_lat + hi_lat) / 2.0;
            let lat_pad = ENVELOPE_PAD_M / 111_320.0;
            let lon_pad = ENVELOPE_PAD_M / (111_320.0 * mid.to_radians().cos().max(0.2));
            (
                lo_lon - lon_pad,
                lo_lat - lat_pad,
                hi_lon + lon_pad,
                hi_lat + lat_pad,
            )
        })
        .collect()
}

/// One HPMS section: AADT, through lanes, and its vertices as `(lat, lon)`.
pub struct Section {
    pub aadt: f64,
    pub through: f64,
    pub vertices: Vec<(f64, f64)>,
}

fn query_box(client: &Client, b: (f64, f64, f64, f64), filter: &str) -> Result<Vec<Section>> {
    let mut out = Vec::new();
    let mut offset = 0;
    loop {
        let v = client.query(&[
            ("where", format!("({filter}) AND AADT>0")),
            ("geometry", super::envelope(b)),
            ("geometryType", "esriGeometryEnvelope".into()),
            ("inSR", "4326".into()),
            ("outSR", "4326".into()),
            ("spatialRel", "esriSpatialRelIntersects".into()),
            ("outFields", "AADT,THROUGH_LANES".into()),
            ("returnGeometry", "true".into()),
            ("maxAllowableOffset", MAX_ALLOWABLE_OFFSET_DEG.to_string()),
            ("geometryPrecision", "5".into()),
            ("resultOffset", offset.to_string()),
            ("resultRecordCount", PAGE_SIZE.to_string()),
            ("f", "json".into()),
        ])?;
        let Some(features) = v.get("features").and_then(|f| f.as_array()) else {
            break;
        };
        for f in features {
            let attr = |k: &str| {
                f.pointer(&format!("/attributes/{k}"))
                    .and_then(|x| x.as_f64())
            };
            let vertices = f
                .pointer("/geometry/paths")
                .and_then(|p| p.as_array())
                .into_iter()
                .flatten()
                .filter_map(|path| path.as_array())
                .flatten()
                .filter_map(|pt| Some((pt.get(1)?.as_f64()?, pt.get(0)?.as_f64()?)))
                .collect();
            out.push(Section {
                aadt: attr("AADT").unwrap_or(0.0),
                through: attr("THROUGH_LANES").unwrap_or(0.0),
                vertices,
            });
        }
        if v.get("exceededTransferLimit").and_then(|x| x.as_bool()) != Some(true) {
            break;
        }
        offset += PAGE_SIZE;
    }
    Ok(out)
}

/// The fewest lanes each way a volume can move on.
pub fn lane_floor(aadt: f64, interstate: bool) -> i64 {
    let mut lanes = if interstate { 2 } else { 1 };
    if aadt >= 100_000.0 {
        lanes = lanes.max(3);
    }
    if aadt >= 160_000.0 {
        lanes = lanes.max(4);
    }
    lanes
}

/// `(at_mi, aadt, lanes each way)` for every section vertex on the road.
pub fn snap(sections: &[Section], route: &Route, interstate: bool) -> Vec<(f64, f64, i64)> {
    let key = |lat: f64, lon: f64| {
        (
            (lat / GRID_DEG).floor() as i64,
            (lon / GRID_DEG).floor() as i64,
        )
    };
    let mut grid: HashMap<(i64, i64), Vec<usize>> = HashMap::new();
    for (i, p) in route.points.iter().enumerate() {
        grid.entry(key(p.0, p.1)).or_default().push(i);
    }
    let mut out = Vec::new();
    for s in sections {
        if s.aadt <= 0.0 {
            continue;
        }
        let per_dir = if s.through > 0.0 {
            ((s.through / 2.0).round_ties_even() as i64).max(1)
        } else {
            2
        };
        let per_dir = per_dir.max(lane_floor(s.aadt, interstate));
        for &(lat, lon) in &s.vertices {
            let (cy, cx) = key(lat, lon);
            let mut best = (f64::INFINITY, 0.0);
            for dy in -1..=1 {
                for dx in -1..=1 {
                    for &i in grid.get(&(cy + dy, cx + dx)).into_iter().flatten() {
                        let p = route.points[i];
                        let d = haversine_m(lat, lon, p.0, p.1);
                        if d < best.0 {
                            best = (d, route.cum_mi[i]);
                        }
                    }
                }
            }
            if best.0 <= SNAP_CORRIDOR_M {
                out.push((best.1, s.aadt, per_dir));
            }
        }
    }
    out
}

fn median(values: &mut [f64]) -> f64 {
    values.sort_by(f64::total_cmp);
    values[values.len() / 2]
}

/// One profile step.
#[derive(Debug, Clone, PartialEq)]
pub struct Step {
    pub at_mi: f64,
    pub aadt: f64,
    pub lanes: i64,
}

pub fn assemble(points: &[(f64, f64, i64)], leg_miles: f64, interstate: bool) -> Vec<Step> {
    let points: Vec<&(f64, f64, i64)> = points
        .iter()
        .filter(|p| !interstate || p.1 >= INTERSTATE_AADT_FLOOR)
        .collect();
    if points.is_empty() {
        return Vec::new();
    }
    let mut picked: Vec<(f64, f64, i64)> = Vec::new();
    let mut mile = 0.0;
    while mile <= leg_miles + 1e-9 {
        let window: Vec<&&(f64, f64, i64)> = points
            .iter()
            .filter(|p| (p.0 - mile).abs() <= SAMPLE_STRIDE_MI)
            .collect();
        if window.len() >= MIN_WINDOW_POINTS {
            let aadt = median(&mut window.iter().map(|p| p.1).collect::<Vec<_>>());
            let lanes = median(&mut window.iter().map(|p| p.2 as f64).collect::<Vec<_>>()) as i64;
            picked.push((
                crate::world::layers::round(leg_miles.min(mile), 1),
                aadt,
                lanes,
            ));
        }
        mile += SAMPLE_STRIDE_MI;
    }
    for i in 1..picked.len().saturating_sub(1) {
        let (prev, next) = (picked[i - 1], picked[i + 1]);
        let lo = prev.1.min(next.1);
        if picked[i].1 < LONE_DIP_RATIO * lo && prev.1.max(next.1) <= 2.0 * lo {
            picked[i] = (picked[i].0, prev.1, prev.2);
        }
    }
    let mut profile: Vec<Step> = Vec::new();
    for (at_mi, aadt, lanes) in picked {
        let rounded = (aadt / AADT_ROUND).round_ties_even() * AADT_ROUND;
        if rounded < AADT_ROUND {
            continue;
        }
        if profile
            .last()
            .is_some_and(|p| p.aadt == rounded && p.lanes == lanes)
        {
            continue;
        }
        profile.push(Step {
            at_mi,
            aadt: rounded,
            lanes,
        });
    }
    profile
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
    let note = source(&super::today());
    let mut run = Run::load()?;
    let archive = std::mem::take(&mut run.archive);
    let (mut changed, mut same, mut none, mut failed, mut no_archive) = (0, 0, 0, 0, 0);
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
        let interstate = is_interstate(&highway(leg));
        let filter = if interstate {
            "F_SYSTEM=1"
        } else {
            "F_SYSTEM>=2 AND F_SYSTEM<=4"
        };
        let boxes = envelopes(&route);
        let sections: Result<Vec<Vec<Section>>> = boxes
            .par_iter()
            .map(|b| query_box(&client, *b, filter))
            .collect();
        let sections: Vec<Section> = match sections {
            Ok(s) => s.into_iter().flatten().collect(),
            Err(e) => {
                println!("  {id}: skipped, {e}");
                failed += 1;
                continue;
            }
        };
        let profile = assemble(&snap(&sections, &route, interstate), miles, interstate);
        if profile.is_empty() {
            none += 1;
            continue;
        }
        let mut lanes: Vec<f64> = profile.iter().map(|s| s.lanes as f64).collect();
        let leg_lanes = median(&mut lanes) as i64;
        if only.is_some() || done % 50 == 0 {
            println!(
                "  [{done}/{total}] {id}: {} steps, {leg_lanes} lanes each way",
                profile.len()
            );
        }
        let rows = Value::Array(
            profile
                .iter()
                .map(|s| {
                    Value::Object(crate::pyobj! {
                        "at_mi" => s.at_mi,
                        "aadt" => s.aadt,
                        "lanes" => s.lanes,
                        "source" => note.as_str(),
                    })
                })
                .collect(),
        );
        let obj: &mut Object = leg.as_object_mut().ok_or("leg is not an object")?;
        let lanes_same = obj.get("lanes").and_then(Value::as_i64) == Some(leg_lanes);
        obj.insert("lanes", Value::Int(i128::from(leg_lanes)));
        let corridor = corridor_mut(leg)?;
        if lanes_same && corridor.get("traffic_aadt") == Some(&rows) {
            same += 1;
        } else {
            changed += 1;
            corridor.insert("traffic_aadt", rows);
        }
    }
    run.archive = archive;
    println!(
        "{changed} legs changed, {same} the same; {none} with no HPMS section on the road and \
         {failed} failed kept as they were, {no_archive} with no archived road kept. {} new \
         answer(s) fetched.",
        client.fetched.load(std::sync::atomic::Ordering::Relaxed)
    );
    run.finish(args.flag("write"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interstate_legs_are_spotted_by_their_prefix() {
        assert!(is_interstate("I-94"));
        assert!(is_interstate(" i 80"));
        assert!(!is_interstate("US-41"));
        assert!(!is_interstate("IL-53"));
    }

    #[test]
    fn a_window_needs_support_and_a_lone_dip_is_carried_across() {
        let mut points = Vec::new();
        for i in 0..60 {
            let at = i as f64 * 0.5;
            // Miles 13 to 17 read a stray frontage road at 2,000.
            let aadt = if (13.0..=17.0).contains(&at) {
                2_000.0
            } else {
                40_000.0
            };
            points.push((at, aadt, 2));
        }
        let profile = assemble(&points, 30.0, false);
        assert_eq!(profile.len(), 1, "{profile:?}");
        assert_eq!(profile[0].aadt, 40_000.0);
        assert!(assemble(&points[..3], 30.0, false).is_empty());
    }

    #[test]
    fn heavy_volume_floors_the_lane_count() {
        assert_eq!(lane_floor(120_000.0, false), 3);
        assert_eq!(lane_floor(5_000.0, true), 2);
        assert_eq!(lane_floor(200_000.0, true), 4);
    }
}
