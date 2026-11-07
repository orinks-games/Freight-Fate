//! The grade profile of a rerouted leg, from ground heights read along its
//! archived road.
//!
//! A port of `fine_grade_samples` and `grade_segments_from_samples` from the
//! deleted `tools/enrich_routes_ors.py`, and of `tools/terrain_rules.py`'s
//! terrain rule. Quarter-mile bins close on distance actually run, so a
//! sparse straight cannot collapse several bins onto one vertex and report a
//! 30 percent grade on flat interstate; heights are read every 0.1 mile, so
//! a long climb on a straight is not averaged away.

use ff_core::pyfmt::round_py_n;

use super::super::osm::geo::{haversine_m, M_PER_MI};

/// How far apart the ground is read.
pub const ELEVATION_STEP_MI: f64 = 0.1;
/// Fixed real-distance bin width for the grade.
const GRADE_BIN_MI: f64 = 0.25;
/// Relief is measured this far either side of an interval.
const TERRAIN_RELIEF_WINDOW_MI: f64 = 5.0;
/// Stored elevation samples, one per this many miles.
const STORED_SAMPLE_MI: f64 = 25.0;

pub fn elevation_source() -> String {
    format!(
        "Read from Valhalla /height (its own DEM tileset over OpenStreetMap) at \
         {ELEVATION_STEP_MI} mi spacing along the leg's checked-in route geometry."
    )
}

pub fn grade_source() -> String {
    format!(
        "Derived: rise over run between elevation samples 0.25 mi apart. {}",
        elevation_source()
    )
}

/// The archived road with a point at least every `step_mi`. Every archived
/// vertex is kept (that is where the road bends) and points are inserted on
/// the straights between them.
pub fn densify(coords: &[(f64, f64)], step_mi: f64) -> Vec<(f64, f64)> {
    let mut out = vec![coords[0]];
    for pair in coords.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        let span_mi = haversine_m(a.1, a.0, b.1, b.0) / M_PER_MI;
        let steps = ((span_mi / step_mi) as usize).max(1);
        for i in 1..steps {
            let t = i as f64 / steps as f64;
            out.push((a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t));
        }
        out.push(b);
    }
    out
}

/// Terrain from sustained steepness and nearby relief.
pub fn terrain_for(steep_pct: f64, relief_ft: f64) -> &'static str {
    let steep = steep_pct.abs();
    if steep >= 5.0 || (steep >= 4.0 && relief_ft >= 1000.0) {
        return "mountain";
    }
    if steep >= 1.5 || relief_ft >= 400.0 {
        return "hills";
    }
    "flat"
}

/// `(at_mi, elevation_ft)` at quarter-mile bin edges on the leg's mileage.
pub fn fine_samples(coords: &[(f64, f64)], elevations: &[f64], leg_miles: f64) -> Vec<(f64, f64)> {
    let mut cum = vec![0.0];
    for pair in coords.windows(2) {
        let last = cum[cum.len() - 1];
        cum.push(last + haversine_m(pair[0].1, pair[0].0, pair[1].1, pair[1].0) / M_PER_MI);
    }
    let total = cum[cum.len() - 1];
    let scale = if leg_miles > 0.0 && total > 0.0 {
        leg_miles / total
    } else {
        1.0
    };
    let mut out = vec![(0.0, elevations[0])];
    let mut acc = 0.0;
    let last = coords.len() - 1;
    for i in 1..coords.len() {
        acc += cum[i] - cum[i - 1];
        if acc < GRADE_BIN_MI && i != last {
            continue;
        }
        let at = round_py_n(cum[i] * scale, 3);
        let prev = out[out.len() - 1].0;
        // A stored segment rounds its ends to 0.1 mi, so a gap under 0.15
        // can round away to a zero-width segment the world refuses.
        if at - prev >= 0.15 {
            out.push((at, elevations[i]));
        } else if i == last {
            let n = out.len() - 1;
            out[n] = (at, elevations[i]);
        }
        acc = 0.0;
    }
    let n = out.len() - 1;
    out[n].0 = leg_miles;
    out
}

/// One stored grade segment.
#[derive(Debug, Clone, PartialEq)]
pub struct Segment {
    pub start_mi: f64,
    pub end_mi: f64,
    pub avg_grade_pct: f64,
    pub terrain: &'static str,
}

fn segment(start: f64, end: f64, grades: &[f64], terrain: &'static str) -> Segment {
    let mean = grades.iter().sum::<f64>() / grades.len() as f64;
    Segment {
        start_mi: round_py_n(start, 1),
        end_mi: round_py_n(end, 1),
        // The loader's realistic band; a blip over a short bin can read past it.
        avg_grade_pct: round_py_n(mean, 2).clamp(-14.0, 14.0),
        terrain,
    }
}

/// Consecutive same-terrain intervals grouped into segments, keeping the
/// real up and down rather than one leg-long average.
pub fn segments(samples: &[(f64, f64)], leg_miles: f64) -> Vec<Segment> {
    if samples.len() < 3 {
        let (a, b) = (samples[0], samples[samples.len() - 1]);
        let run = (b.0 - a.0).max(0.1);
        let grade = (b.1 - a.1) / (run * 5280.0) * 100.0;
        return vec![segment(
            0.0,
            leg_miles,
            &[grade],
            terrain_for(grade, (b.1 - a.1).abs()),
        )];
    }
    let relief = |lo: f64, hi: f64| {
        let (mut min, mut max) = (f64::INFINITY, f64::NEG_INFINITY);
        for &(at, e) in samples {
            if lo <= at && at <= hi {
                min = min.min(e);
                max = max.max(e);
            }
        }
        if min.is_finite() {
            max - min
        } else {
            0.0
        }
    };
    let intervals: Vec<(f64, f64, f64, &'static str)> = samples
        .windows(2)
        .map(|w| {
            let run = (w[1].0 - w[0].0).max(0.1);
            let grade = (w[1].1 - w[0].1) / (run * 5280.0) * 100.0;
            let r = relief(
                w[0].0 - TERRAIN_RELIEF_WINDOW_MI,
                w[1].0 + TERRAIN_RELIEF_WINDOW_MI,
            );
            (w[0].0, w[1].0, grade, terrain_for(grade, r))
        })
        .collect();
    let mut out = Vec::new();
    let (mut start, mut end, mut grades, mut terrain) = (
        intervals[0].0,
        intervals[0].1,
        vec![intervals[0].2],
        intervals[0].3,
    );
    for &(s, e, g, t) in &intervals[1..] {
        if t == terrain {
            end = e;
            grades.push(g);
        } else {
            out.push(segment(start, end, &grades, terrain));
            (start, end, grades, terrain) = (s, e, vec![g], t);
        }
    }
    out.push(segment(start, end, &grades, terrain));
    out
}

/// The stored elevation samples: the same read, one every 25 miles.
pub fn stored_samples(samples: &[(f64, f64)]) -> Vec<(f64, f64)> {
    let mut out = Vec::new();
    let mut last = -1e9;
    for (i, &(at, e)) in samples.iter().enumerate() {
        if at - last < STORED_SAMPLE_MI && i != samples.len() - 1 {
            continue;
        }
        last = at;
        out.push((round_py_n(at, 2), round_py_n(e, 1)));
    }
    out
}

/// Stretches the profile does not describe, worded for the report.
pub fn coverage_gaps(segs: &[(f64, f64)], leg_miles: f64) -> Vec<String> {
    const SLOP_MI: f64 = 0.2;
    if segs.is_empty() {
        return vec!["no grade profile at all".into()];
    }
    let mut gaps = Vec::new();
    if segs[0].0 > SLOP_MI {
        gaps.push(format!("starts at mile {}, not 0", segs[0].0));
    }
    for w in segs.windows(2) {
        if w[1].0 - w[0].1 > SLOP_MI {
            gaps.push(format!("nothing between mile {} and {}", w[0].1, w[1].0));
        }
    }
    let last = segs[segs.len() - 1].1;
    if leg_miles - last > SLOP_MI {
        gaps.push(format!("ends at mile {last}, not {leg_miles}"));
    }
    gaps
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_flat_road_is_one_flat_segment_end_to_end() {
        let coords: Vec<(f64, f64)> = (0..=100)
            .map(|i| (-88.0, 42.0 + i as f64 * 0.0015))
            .collect();
        let dense = densify(&coords, ELEVATION_STEP_MI);
        let ele = vec![650.0; dense.len()];
        let samples = fine_samples(&dense, &ele, 10.0);
        let segs = segments(&samples, 10.0);
        assert_eq!(segs.len(), 1);
        assert_eq!(segs[0].terrain, "flat");
        assert_eq!((segs[0].start_mi, segs[0].end_mi), (0.0, 10.0));
        let spans: Vec<(f64, f64)> = segs.iter().map(|s| (s.start_mi, s.end_mi)).collect();
        assert!(coverage_gaps(&spans, 10.0).is_empty());
    }

    #[test]
    fn a_steady_climb_reads_its_grade() {
        let coords: Vec<(f64, f64)> = (0..=100)
            .map(|i| (-88.0, 42.0 + i as f64 * 0.0015))
            .collect();
        let dense = densify(&coords, ELEVATION_STEP_MI);
        let cum = super::super::shape::cumulative_m(&dense);
        // 3 percent: 0.03 ft up per ft along.
        let ele: Vec<f64> = cum.iter().map(|m| 600.0 + m * 3.280_84 * 0.03).collect();
        let miles = cum[cum.len() - 1] / M_PER_MI;
        let samples = fine_samples(&dense, &ele, miles);
        let segs = segments(&samples, miles);
        assert!(
            segs.iter().all(|s| (s.avg_grade_pct - 3.0).abs() < 0.2),
            "{segs:?}"
        );
        assert!(segs.iter().all(|s| s.terrain == "hills"));
    }

    #[test]
    fn a_hole_in_the_profile_is_named() {
        let gaps = coverage_gaps(&[(0.0, 4.0), (5.0, 10.0)], 10.0);
        assert_eq!(gaps, vec!["nothing between mile 4 and 5".to_string()]);
    }
}
