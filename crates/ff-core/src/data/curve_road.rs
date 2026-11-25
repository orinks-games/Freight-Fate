//! The fifth curve screen: does the road under a sharp bend actually turn?
//!
//! The four screens in `data::curves` judge a record by its own numbers, by
//! its road class and by its terrain. None of them looks at the road itself,
//! and that is where the false sharp bend on Freeman Mill Road in Greensboro
//! hid (missing-roads pass, 2026-11-23): "Sharp left, too fast. Slow to 34
//! miles per hour." and a rollover, on a straight road. Freeman Mill Road is
//! divided, and where its two carriageways become one, OpenStreetMap's
//! centreline steps sideways a few metres: the leg's own archive turns 28
//! degrees one way and 29 the other 15 m apart. The curve sweep measured the
//! first half of that step as a 214 ft bend turning 28 degrees, and every
//! screen passed it, because a 214 ft bend turning 28 degrees is a perfectly
//! possible bend. It just is not there.
//!
//! So this screen reads the leg's archived road (`world_data/*/geometry/`,
//! the same polyline the sweep ran over) and asks a self-consistency
//! question, the kind AGENTS.md prefers to a threshold on extremity: across
//! the span the record claims, how far does the road's heading actually
//! change? A real corner turns the road and keeps it turned. A carriageway
//! step returns to the heading it had within a few tens of metres, so it is
//! cancelled out before the count (see [`cancel_carriageway_steps`]).
//!
//! It judges only the bends that are spoken as sharp or hairpin (advisory at
//! or below `SHARP_MAX_MPH`): those are the calls that brake the truck hard
//! and the radii that can roll it. Connector rows are exempt as everywhere
//! else. A leg with no archive is never judged, so a missing file keeps
//! every bend rather than dropping any.
//!
//! The bake is not edited. The screen runs where the other four do, at load
//! (and so at container bake time), and the share it rejects is written in
//! the `screens` entry of `curves.jsonl`'s meta line, which
//! `the_road_turn_screen_share_in_the_meta_is_current` keeps honest.

use std::collections::HashMap;
use std::path::Path;

use serde::Deserialize;

use super::curves::{CurveRecord, SHARP_MAX_MPH};

const METRES_PER_MILE: f64 = 1609.344;
const EARTH_RADIUS_M: f64 = 6_371_008.8;

/// How far either side of a record's span a vertex may sit and still belong
/// to it, in miles.
///
/// DERIVED from the bake's own precision: every `start_mi` and `end_mi` is
/// written to two decimals, so the true end of a span is anywhere within half
/// a hundredth of a mile of the number. Freeman Mill Road's step vertex sits
/// 3 m before its record's `start_mi`.
pub const SPAN_TOLERANCE_MI: f64 = 0.005;

/// The smallest heading change at one vertex that counts as part of a
/// carriageway step, in degrees. READ from the curve bake's own parameters
/// (`deflection_floor_deg` in `curves.jsonl`'s meta): the sweep does not call
/// anything gentler than 8 degrees a bend, so a step smaller than that never
/// made a record in the first place.
pub const STEP_MIN_TURN_DEG: f64 = 8.0;

/// What a pair of opposite turns may leave behind and still be a step rather
/// than a bend, in degrees: less than the sweep's own floor, the same 8.
pub const STEP_MAX_RESIDUAL_DEG: f64 = 8.0;

/// How close the two halves of a step sit, in metres. READ from the bake's
/// parameters (`eps_tangent_m`): 30 m is the shortest straight the sweep
/// resolves, so two opposite turns closer than that have no road between
/// them that the sweep could have measured.
pub const STEP_MAX_SPAN_M: f64 = 30.0;

/// The sharpest turn a carriageway step makes, in degrees.
///
/// CALIBRATED on the shipped archive, because no standard speaks to how
/// OpenStreetMap draws a merge. Taking every adjacent pair of opposite turns
/// that meets the three rules above (1,570 legs, 7,395 pairs), the sharper
/// half falls under 15 degrees for 4,142 of them, 15 to 30 for 2,496, 30 to
/// 45 for 567, then 101 from 45 to 60, 13 from 60 to 75, and a second hump of
/// 56 from 75 to 90. That second hump is the real thing a step must not be
/// mistaken for: a route turning right onto one street and straight back
/// left onto the next, an offset junction a truck does have to steer. They
/// also step much farther sideways (10 to 26 m against a bulk under 10).
/// The limit sits at the bottom of the gap.
pub const STEP_MAX_TURN_DEG: f64 = 45.0;

/// The least share of its own deflection a record's road must turn.
///
/// CALIBRATED, and here are the two populations: the 287 sharp and hairpin
/// mainline records on dev's 1,570 legs, by how much of their claimed turn
/// the road under them makes. 146 sit within 2.5 percent of their claim and
/// 238 within 15; 26 make under a fifth of it (10 of them under 2.5
/// percent); between a fifth and a half there are only 10. The cut sits at
/// the bottom of that trough, a quarter, and the 2.0 branch and the
/// Appalachia build show the same shape (28 of 382, 39 of 834).
pub const MIN_TURN_SHARE: f64 = 0.25;

/// One leg's archived road, measured along its length.
#[derive(Debug, Clone)]
pub struct LegRoad {
    /// `(miles from city a, signed turn in degrees, right positive)` at each
    /// vertex, in order.
    turns: Vec<(f64, f64)>,
}

#[derive(Deserialize)]
struct GeometryRow {
    leg: String,
    miles: f64,
    geom: Geometry,
}

#[derive(Deserialize)]
struct Geometry {
    lat0: i64,
    lon0: i64,
    q: i32,
    dlat: Vec<i64>,
    dlon: Vec<i64>,
}

fn haversine_m(a: (f64, f64), b: (f64, f64)) -> f64 {
    let (la1, lo1, la2, lo2) = (
        a.0.to_radians(),
        a.1.to_radians(),
        b.0.to_radians(),
        b.1.to_radians(),
    );
    let h = ((la2 - la1) / 2.0).sin().powi(2)
        + la1.cos() * la2.cos() * ((lo2 - lo1) / 2.0).sin().powi(2);
    2.0 * EARTH_RADIUS_M * h.sqrt().asin()
}

/// Initial bearing from `a` to `b`, degrees clockwise from north.
fn bearing_deg(a: (f64, f64), b: (f64, f64)) -> f64 {
    let (la1, lo1, la2, lo2) = (
        a.0.to_radians(),
        a.1.to_radians(),
        b.0.to_radians(),
        b.1.to_radians(),
    );
    let y = (lo2 - lo1).sin() * la2.cos();
    let x = la1.cos() * la2.sin() - la1.sin() * la2.cos() * (lo2 - lo1).cos();
    y.atan2(x).to_degrees()
}

/// Signed change from heading `from` to heading `to`, in (-180, 180].
fn turn_between(from: f64, to: f64) -> f64 {
    (to - from + 540.0).rem_euclid(360.0) - 180.0
}

impl LegRoad {
    /// A road from its vertices in order and the leg's length in miles. The
    /// archive's own length is scaled to the leg's, the way every layer that
    /// places things on it by mile does.
    pub fn from_points(points: &[(f64, f64)], leg_miles: f64) -> Self {
        // Repeated points (the archive is quantized) carry no heading.
        let mut kept: Vec<(f64, f64)> = Vec::with_capacity(points.len());
        for &p in points {
            if kept.last() != Some(&p) {
                kept.push(p);
            }
        }
        let mut along = vec![0.0];
        for pair in kept.windows(2) {
            let last = *along.last().unwrap_or(&0.0);
            along.push(last + haversine_m(pair[0], pair[1]));
        }
        let raw_mi = along.last().copied().unwrap_or(0.0) / METRES_PER_MILE;
        let scale = if raw_mi > 0.0 && leg_miles > 0.0 {
            leg_miles / raw_mi
        } else {
            1.0
        };
        let turns = kept
            .windows(3)
            .enumerate()
            .map(|(i, w)| {
                let turn = turn_between(bearing_deg(w[0], w[1]), bearing_deg(w[1], w[2]));
                (along[i + 1] / METRES_PER_MILE * scale, turn)
            })
            .collect();
        Self { turns }
    }

    /// A road from its vertex turns directly, `(mile, signed degrees)`.
    pub fn from_turns(turns: Vec<(f64, f64)>) -> Self {
        Self { turns }
    }
}

/// Every leg's archived road from the `geometry/*.jsonl` texts, keeping only
/// the legs `wanted` names (a cheap substring check skips the rest unparsed).
pub fn leg_roads_from(texts: &[String], wanted: &dyn Fn(&str) -> bool) -> HashMap<String, LegRoad> {
    let mut out = HashMap::new();
    for text in texts {
        for line in text.lines() {
            let Some(leg) = leg_name_in(line) else {
                continue;
            };
            if !wanted(leg) {
                continue;
            }
            let Ok(row) = serde_json::from_str::<GeometryRow>(line) else {
                continue;
            };
            let g = &row.geom;
            let unit = 10f64.powi(g.q);
            let (mut lat, mut lon) = (g.lat0, g.lon0);
            let mut points = Vec::with_capacity(g.dlat.len() + 1);
            points.push((lat as f64 / unit, lon as f64 / unit));
            for (dlat, dlon) in g.dlat.iter().zip(&g.dlon) {
                lat += dlat;
                lon += dlon;
                points.push((lat as f64 / unit, lon as f64 / unit));
            }
            out.insert(row.leg, LegRoad::from_points(&points, row.miles));
        }
    }
    out
}

/// The texts of every country's leg archive under a `world_data` directory
/// (`<country>/geometry/*.jsonl`), in a fixed order. A tree without the
/// archive answers with nothing, and the screen then keeps every bend.
pub fn read_leg_geometry(world_data: &Path) -> Vec<String> {
    let mut files = Vec::new();
    let Ok(countries) = std::fs::read_dir(world_data) else {
        return Vec::new();
    };
    for country in countries.flatten() {
        let Ok(shards) = std::fs::read_dir(country.path().join("geometry")) else {
            continue;
        };
        for shard in shards.flatten() {
            let path = shard.path();
            if path.extension().is_some_and(|ext| ext == "jsonl") {
                files.push(path);
            }
        }
    }
    files.sort();
    files
        .iter()
        .filter_map(|path| std::fs::read_to_string(path).ok())
        .collect()
}

/// The `"leg": "..."` value of an archive line, without parsing the rest.
fn leg_name_in(line: &str) -> Option<&str> {
    const KEY: &str = "\"leg\":";
    let after = &line[line.find(KEY)? + KEY.len()..];
    let start = line.len() - after.trim_start().strip_prefix('"')?.len();
    let len = line[start..].find('"')?;
    Some(&line[start..start + len])
}

/// The vertex turns with every carriageway step cancelled.
///
/// A step is two adjacent vertices turning opposite ways, each at least
/// [`STEP_MIN_TURN_DEG`] and both under [`STEP_MAX_TURN_DEG`], that cancel to
/// within [`STEP_MAX_RESIDUAL_DEG`] across no more than [`STEP_MAX_SPAN_M`],
/// with straight road either side: the vertex before and the vertex after
/// each turn less than the floor or sit farther off than the span. That last
/// rule is what keeps the end of a long curve from being paired with the
/// first vertex of the next one. The pair becomes one vertex carrying what is
/// left of it.
pub fn cancel_carriageway_steps(turns: &[(f64, f64)]) -> Vec<(f64, f64)> {
    let max_span_mi = STEP_MAX_SPAN_M / METRES_PER_MILE;
    let quiet = |neighbour: Option<&(f64, f64)>, at: f64| {
        neighbour.is_none_or(|&(mile, turn)| {
            turn.abs() < STEP_MIN_TURN_DEG || (mile - at).abs() > max_span_mi
        })
    };
    let mut out = Vec::with_capacity(turns.len());
    let mut i = 0;
    while i < turns.len() {
        if let Some(&(m2, t2)) = turns.get(i + 1) {
            let (m1, t1) = turns[i];
            let is_step = t1 * t2 < 0.0
                && t1.abs().min(t2.abs()) >= STEP_MIN_TURN_DEG
                && t1.abs().max(t2.abs()) < STEP_MAX_TURN_DEG
                && (t1 + t2).abs() <= STEP_MAX_RESIDUAL_DEG
                && m2 - m1 <= max_span_mi
                && quiet(i.checked_sub(1).and_then(|k| turns.get(k)), m1)
                && quiet(turns.get(i + 2), m2);
            if is_step {
                out.push(((m1 + m2) / 2.0, t1 + t2));
                i += 2;
                continue;
            }
        }
        out.push(turns[i]);
        i += 1;
    }
    out
}

fn sign_of(direction: char) -> f64 {
    if direction == 'R' {
        1.0
    } else {
        -1.0
    }
}

/// How far the road turns, in the record's own direction, across the span
/// the record claims. `turns` is already step-cancelled; `others` are the
/// leg's other mainline records.
///
/// A vertex in the rounding margin at either end that turns AGAINST this
/// record and sits inside a neighbour that turns its way is the neighbour's:
/// a right bend ending where a left corner begins does not get charged with
/// the corner, only because the two spans were rounded to the same
/// hundredth.
pub fn road_turn_across(record: &CurveRecord, turns: &[(f64, f64)], others: &[CurveRecord]) -> f64 {
    let sign = sign_of(record.direction);
    let (lo, hi) = (
        record.start_mi - SPAN_TOLERANCE_MI,
        record.end_mi + SPAN_TOLERANCE_MI,
    );
    let (core_lo, core_hi) = (
        record.start_mi + SPAN_TOLERANCE_MI,
        record.end_mi - SPAN_TOLERANCE_MI,
    );
    let mut total = 0.0;
    for &(mile, turn) in turns {
        if mile < lo || mile > hi {
            continue;
        }
        let in_margin = mile < core_lo || mile > core_hi;
        if in_margin
            && turn * sign < 0.0
            && others.iter().any(|o| {
                mile >= o.start_mi - SPAN_TOLERANCE_MI
                    && mile <= o.end_mi + SPAN_TOLERANCE_MI
                    && turn * sign_of(o.direction) > 0.0
            })
        {
            continue;
        }
        total += turn * sign;
    }
    total
}

/// Why a record fails this screen, or `None` when it is kept.
pub fn verdict(record: &CurveRecord, turns: &[(f64, f64)], others: &[CurveRecord]) -> Option<f64> {
    if record.connector || record.advisory_mph > SHARP_MAX_MPH || record.deflection_deg <= 0.0 {
        return None;
    }
    let turned = road_turn_across(record, turns, others);
    (turned.abs() < MIN_TURN_SHARE * record.deflection_deg).then_some(turned)
}

/// The leg's records with every sharp bend its own road does not make taken
/// out. With no road for the leg, everything is kept.
pub fn screen_against_road(records: &[CurveRecord], road: Option<&LegRoad>) -> Vec<CurveRecord> {
    let Some(road) = road else {
        return records.to_vec();
    };
    let turns = cancel_carriageway_steps(&road.turns);
    let mainline: Vec<CurveRecord> = records.iter().filter(|r| !r.connector).copied().collect();
    records
        .iter()
        .filter(|record| {
            let others: Vec<CurveRecord> =
                mainline.iter().filter(|o| *o != *record).copied().collect();
            verdict(record, &turns, &others).is_none()
        })
        .copied()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(
        start_mi: f64,
        end_mi: f64,
        direction: char,
        radius: i64,
        defl: f64,
        adv: i64,
    ) -> CurveRecord {
        CurveRecord {
            start_mi,
            apex_mi: start_mi,
            end_mi,
            direction,
            advisory_mph: adv,
            min_radius_ft: radius,
            deflection_deg: defl,
            connector: false,
        }
    }

    /// Miles of a vertex `metres` past mile 10.
    fn at(metres: f64) -> f64 {
        10.0 + metres / METRES_PER_MILE
    }

    #[test]
    fn a_carriageway_step_cancels_to_what_it_leaves() {
        let turns = vec![
            (at(0.0), -0.1),
            (at(37.0), -28.1),
            (at(52.0), 29.0),
            (at(69.0), -1.0),
        ];
        let out = cancel_carriageway_steps(&turns);
        assert_eq!(out.len(), 3);
        assert!((out[1].1 - 0.9).abs() < 1e-9);
    }

    #[test]
    fn the_end_of_a_long_curve_is_not_a_step() {
        // A left curve built of several vertices, then the first right vertex
        // of the next bend: the -20 and +16 cancel, but the -30 just before
        // says the road was already turning.
        let turns = vec![
            (at(0.0), -30.0),
            (at(8.0), -20.0),
            (at(16.0), 16.0),
            (at(41.0), 8.0),
        ];
        assert_eq!(cancel_carriageway_steps(&turns), turns);
    }

    #[test]
    fn an_offset_junction_is_not_a_step() {
        // Right onto one street, 20 m, left onto the next: a truck steers both.
        let turns = vec![(at(0.0), 88.0), (at(20.0), -87.0)];
        assert_eq!(cancel_carriageway_steps(&turns), turns);
    }

    #[test]
    fn opposite_turns_too_far_apart_are_two_bends() {
        let turns = vec![(at(0.0), 20.0), (at(45.0), -20.0)];
        assert_eq!(cancel_carriageway_steps(&turns), turns);
    }

    /// The table: each case is a record, the road under it, and whether the
    /// screen keeps it, with the reason.
    #[test]
    fn kept_and_rejected_records_with_their_reasons() {
        let mi = |m: f64| at(m);
        struct Case {
            why: &'static str,
            record: CurveRecord,
            others: Vec<CurveRecord>,
            road: Vec<(f64, f64)>,
            kept: bool,
        }
        let cases = vec![
            Case {
                why: "Freeman Mill Road: the record holds half a carriageway step; the road does not turn",
                record: rec(mi(55.0), mi(330.0), 'R', 214, 28.2, 30),
                others: vec![rec(mi(-80.0), mi(-8.0), 'L', 530, 16.6, 50)],
                road: vec![(mi(40.0), -28.1), (mi(55.0), 29.0), (mi(72.0), -1.0), (mi(345.0), -0.8)],
                kept: false,
            },
            Case {
                why: "a real corner: the road turns what the record says",
                record: rec(mi(0.0), mi(80.0), 'L', 200, 41.3, 35),
                others: vec![],
                road: vec![(mi(10.0), -12.0), (mi(30.0), -15.0), (mi(55.0), -14.0)],
                kept: true,
            },
            Case {
                why: "a step wholly inside the span: plus and minus cancel, nothing is left",
                record: rec(mi(0.0), mi(40.0), 'L', 82, 54.9, 20),
                others: vec![],
                road: vec![(mi(18.0), 55.0), (mi(19.0), -55.0)],
                kept: false,
            },
            Case {
                why: "a record whose road turns the other way is a different fault, kept for now",
                record: rec(mi(0.0), mi(16.0), 'R', 145, 15.2, 25),
                others: vec![],
                road: vec![(mi(6.0), 15.0), (mi(18.0), -78.0)],
                kept: true,
            },
            Case {
                why: "a corner vertex on the shared boundary goes to the neighbour that turns its way",
                record: rec(mi(0.0), mi(96.0), 'R', 141, 10.6, 25),
                others: vec![rec(mi(96.0), mi(160.0), 'L', 260, 100.0, 35)],
                road: vec![(mi(1.0), 5.0), (mi(71.0), 6.0), (mi(93.0), -94.0)],
                kept: true,
            },
            Case {
                why: "a moderate bend is not judged: only sharp calls are",
                record: rec(mi(0.0), mi(40.0), 'L', 530, 16.6, 50),
                others: vec![],
                road: vec![],
                kept: true,
            },
        ];
        for case in cases {
            let road = LegRoad::from_turns(case.road.clone());
            let mut all = case.others.clone();
            all.push(case.record);
            let kept = screen_against_road(&all, Some(&road)).contains(&case.record);
            assert_eq!(kept, case.kept, "{}", case.why);
        }
    }

    /// The regression this screen exists for: the archive of Greensboro to
    /// Pinehurst (points 645 to 662) and the four records the sweep baked
    /// there, miles shifted so the first point is mile 0. Freeman Mill Road
    /// is straight here; the step at points 654 and 655 is where its two
    /// carriageways become one.
    #[test]
    fn freeman_mill_road_has_no_sharp_bend() {
        let points = [
            (36.05886, -79.80130),
            (36.05908, -79.80122),
            (36.05942, -79.80102),
            (36.05944, -79.80101),
            (36.05986, -79.80081),
            (36.06040, -79.80056),
            (36.06048, -79.80048),
            (36.06055, -79.80045),
            (36.06093, -79.80033),
            (36.06125, -79.80023),
            (36.06138, -79.80027),
            (36.06153, -79.80022),
            (36.06392, -79.79948),
            (36.06447, -79.79932),
            (36.06484, -79.79920),
            (36.06516, -79.79905),
            (36.06568, -79.79877),
            (36.06600, -79.79856),
        ];
        let road = LegRoad::from_points(&points, 0.0);
        let first = 73.7528;
        let leg = |start: f64, end: f64, dir: char, radius: i64, defl: f64, adv: i64| {
            rec(start - first, end - first, dir, radius, defl, adv)
        };
        let seq33 = leg(73.83, 73.88, 'R', 715, 9.6, 55);
        let seq34 = leg(73.88, 73.93, 'L', 530, 16.6, 50);
        let seq35 = leg(73.94, 74.12, 'R', 214, 28.2, 30);
        let seq36 = leg(74.16, 74.28, 'R', 917, 31.5, 65);
        let kept = screen_against_road(&[seq33, seq34, seq35, seq36], Some(&road));
        assert!(
            !kept.contains(&seq35),
            "the 30 mph right on Freeman Mill Road"
        );
        assert_eq!(kept, vec![seq33, seq34, seq36], "nothing else moves");
    }

    #[test]
    fn a_leg_with_no_archive_keeps_everything() {
        let lone = rec(1.0, 1.01, 'R', 200, 30.0, 30);
        assert_eq!(screen_against_road(&[lone], None), vec![lone]);
    }

    #[test]
    fn a_ramp_is_never_judged() {
        let mut ramp = rec(1.0, 1.01, 'R', 200, 30.0, 30);
        ramp.connector = true;
        let road = LegRoad::from_turns(vec![]);
        assert_eq!(screen_against_road(&[ramp], Some(&road)), vec![ramp]);
    }

    #[test]
    fn the_archive_decodes_and_measures_a_square_corner() {
        // North 100 m, then east 100 m: one right turn of 90 degrees.
        let d = 100.0 / 111_195.0;
        let pts = [(0.0, 0.0), (d, 0.0), (d, d)];
        let road = LegRoad::from_points(&pts, 0.0);
        assert_eq!(road.turns.len(), 1);
        assert!((road.turns[0].1 - 90.0).abs() < 0.5, "{:?}", road.turns);
    }

    #[test]
    fn a_leg_name_is_read_without_parsing_the_line() {
        let line = r#"{"geom": {"n": 2}, "highway": "US-25", "leg": "a_x:b_y", "miles": 3.0}"#;
        assert_eq!(leg_name_in(line), Some("a_x:b_y"));
        assert_eq!(leg_name_in(r#"{"meta": {}}"#), None);
    }
}
