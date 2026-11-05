//! The road as a polyline: curvature, curve detection, the curve-aware
//! simplifier and the archive's delta encoding.
//!
//! A port of the curve sampler the deleted `tools/straw_curve_sample.py`
//! held (`analyse_curvature`, `adaptive_simplify`, `encode_geometry`), with
//! its ratified tuning. Points are `(lon, lat)` throughout, as the router and
//! the archive both write them.

use ff_core::pyfmt::round_py_n;

use super::super::osm::geo::{haversine_m, M_PER_MI};

/// 1e-5 degree quantization (about 1.1 m) for the archive stream.
pub const QUANT_DEG: i32 = 5;
/// A vertex tighter than this radius is "curving" for a loaded semi.
pub const CURVE_RADIUS_FT: f64 = 3000.0;
/// Comfortable dry-loaded lateral acceleration for the advisory speed.
pub const A_LAT_G: f64 = 0.30;
/// Sliding-arc window for the radius estimate.
pub const RADIUS_WINDOW_M: f64 = 80.0;
/// Loose simplification on straights.
pub const DP_EPS_TANGENT_M: f64 = 30.0;
/// Keep-all margin around a curve so its edges re-fit identically.
pub const CURVE_PAD_M: f64 = 80.0;
/// Tangent vertex budget per leg (curves set their own floor).
pub const POINT_BUDGET: usize = 600;
/// A turn under this is neutral and sets no direction.
const SIGN_WOBBLE_DEG: f64 = 1.0;
/// Sustained opposing turn needed to split a curving run.
pub const SIGN_HYSTERESIS_DEG: f64 = 5.0;
/// A real curve turns at least this much.
pub const DEFLECTION_FLOOR_DEG: f64 = 8.0;
/// Top of the AASHTO friction table the advisory solves.
pub const ADVISORY_MAX_MPH: i64 = 80;
/// The first and last stretch of a leg, where curves are city streets.
pub const CONNECTOR_WINDOW_MI: f64 = 0.75;
/// The farthest the archived line may stray from the road: the distance
/// within which a way is taken to govern a point, so a line straying further
/// can no longer be matched to its own road.
const MAX_EPS_TANGENT_M: f64 = 90.0;

const EARTH_R_M: f64 = 6_371_000.0;
const FT_PER_M: f64 = 3.280_839_895;
const MPS_TO_MPH: f64 = 2.236_936_292_054_4;

/// Metres run to each vertex.
pub fn cumulative_m(coords: &[(f64, f64)]) -> Vec<f64> {
    let mut out = Vec::with_capacity(coords.len());
    let mut run = 0.0;
    for (i, p) in coords.iter().enumerate() {
        if i > 0 {
            let q = coords[i - 1];
            run += haversine_m(q.1, q.0, p.1, p.0);
        }
        out.push(run);
    }
    out
}

/// Equirectangular metres about the line's mean latitude.
pub fn to_local_m(coords: &[(f64, f64)]) -> Vec<(f64, f64)> {
    let lat0 = coords.iter().map(|c| c.1).sum::<f64>() / coords.len().max(1) as f64;
    let coslat = lat0.to_radians().cos();
    coords
        .iter()
        .map(|&(lon, lat)| {
            (
                lon.to_radians() * EARTH_R_M * coslat,
                lat.to_radians() * EARTH_R_M,
            )
        })
        .collect()
}

/// Signed heading change in degrees (+left, -right) from AB to BC.
fn turn_deg(a: (f64, f64), b: (f64, f64), c: (f64, f64)) -> f64 {
    if (b.0 - a.0).hypot(b.1 - a.1) < 1e-6 || (c.0 - b.0).hypot(c.1 - b.1) < 1e-6 {
        return 0.0;
    }
    let h1 = (b.1 - a.1).atan2(b.0 - a.0);
    let h2 = (c.1 - b.1).atan2(c.0 - b.0);
    let tau = std::f64::consts::TAU;
    (((h2 - h1 + std::f64::consts::PI) % tau + tau) % tau - std::f64::consts::PI).to_degrees()
}

/// Radius in feet at vertex `i`: arc length over total heading change in a
/// window of about [`RADIUS_WINDOW_M`]. Stable under vertex spacing and the
/// 1.1 m quantization, where a three-point circle is not.
fn window_radius_ft(cum_m: &[f64], turn: &[f64], i: usize) -> f64 {
    let n = cum_m.len();
    let half = RADIUS_WINDOW_M / 2.0;
    let mut lo = i;
    while lo > 0 && cum_m[i] - cum_m[lo - 1] <= half {
        lo -= 1;
    }
    let mut hi = i;
    while hi < n - 1 && cum_m[hi + 1] - cum_m[i] <= half {
        hi += 1;
    }
    let lo = if i > 0 { lo.min(i - 1) } else { i };
    let hi = if i < n - 1 { hi.max(i + 1) } else { i };
    let arc = cum_m[hi] - cum_m[lo];
    let total: f64 = (lo + 1..=hi)
        .filter(|&k| k > 0 && k < n - 1)
        .map(|k| turn[k])
        .sum();
    let rad = total.abs().to_radians();
    if arc < 1e-6 || rad < 1e-4 {
        return f64::INFINITY;
    }
    arc / rad * FT_PER_M
}

/// Advisory speed at the apex, rounded to 5 mph and capped at the top of
/// the friction table it comes from.
pub fn advisory_mph(radius_ft: f64) -> i64 {
    let v = (A_LAT_G * 9.80665 * radius_ft / FT_PER_M).sqrt();
    let mph = (round_py_n(v * MPS_TO_MPH / 5.0, 0) * 5.0) as i64;
    mph.min(ADVISORY_MAX_MPH)
}

/// One detected curve, by vertex index into the analysed line.
#[derive(Debug, Clone, PartialEq)]
pub struct Curve {
    pub seq: i64,
    pub a: usize,
    pub b: usize,
    pub apex: usize,
    pub direction: char,
    pub min_radius_ft: i64,
    pub deflection_deg: f64,
    pub advisory_mph: i64,
}

pub struct Curvature {
    pub curving: Vec<bool>,
    pub curves: Vec<Curve>,
}

/// Radius at every vertex, and the curves: each curving run split at every
/// sustained change of direction, so a serpentine gives its separate left
/// and right bends.
#[allow(clippy::needless_range_loop)] // a line-for-line port of index arithmetic
pub fn analyse(coords: &[(f64, f64)], cum_m: &[f64]) -> Curvature {
    let local = to_local_m(coords);
    let n = local.len();
    let mut turn = vec![0.0; n];
    for i in 1..n.saturating_sub(1) {
        turn[i] = turn_deg(local[i - 1], local[i], local[i + 1]);
    }
    let mut radius = vec![f64::INFINITY; n];
    for i in 1..n.saturating_sub(1) {
        radius[i] = window_radius_ft(cum_m, &turn, i);
    }
    let curving: Vec<bool> = radius.iter().map(|r| *r < CURVE_RADIUS_FT).collect();
    let sign_at = |k: usize| {
        if turn[k] > SIGN_WOBBLE_DEG {
            1
        } else if turn[k] < -SIGN_WOBBLE_DEG {
            -1
        } else {
            0
        }
    };
    let split_run = |i: usize, j: usize| {
        let mut segs = Vec::new();
        let mut seg_start = i;
        let mut cur = 0;
        let mut opp_start: Option<usize> = None;
        let mut opp_acc = 0.0;
        for k in i..j {
            let s = sign_at(k);
            if cur == 0 {
                if s != 0 {
                    cur = s;
                }
                continue;
            }
            if s == cur {
                opp_start = None;
                opp_acc = 0.0;
            } else if s == -cur {
                let start = *opp_start.get_or_insert(k);
                opp_acc += turn[k].abs();
                if opp_acc >= SIGN_HYSTERESIS_DEG {
                    segs.push((seg_start, start));
                    seg_start = start;
                    cur = -cur;
                    opp_start = None;
                    opp_acc = 0.0;
                }
            }
        }
        segs.push((seg_start, j));
        segs
    };
    let mut curves = Vec::new();
    let (mut i, mut seq) = (1, 0);
    while i + 1 < n {
        if !curving[i] {
            i += 1;
            continue;
        }
        let mut j = i;
        while j + 1 < n && curving[j] {
            j += 1;
        }
        for (a, b) in split_run(i, j) {
            let deflection: f64 = (a..b).map(|k| turn[k]).sum();
            if deflection.abs() < DEFLECTION_FLOOR_DEG {
                continue;
            }
            // The first vertex of minimum radius, as Python's min() picks it.
            let mut apex = a;
            for k in a..=b {
                if radius[k] < radius[apex] {
                    apex = k;
                }
            }
            let min_r = radius[apex];
            if !min_r.is_finite() {
                continue;
            }
            seq += 1;
            curves.push(Curve {
                seq,
                a,
                b,
                apex,
                direction: if deflection > 0.0 { 'L' } else { 'R' },
                min_radius_ft: round_py_n(min_r, 0) as i64,
                deflection_deg: round_py_n(deflection.abs(), 1),
                advisory_mph: advisory_mph(min_r),
            });
        }
        i = j;
    }
    Curvature { curving, curves }
}

fn douglas_peucker(local: &[(f64, f64)], lo: usize, hi: usize, eps: f64, keep: &mut Vec<bool>) {
    if hi <= lo + 1 {
        return;
    }
    let (ax, ay) = local[lo];
    let (bx, by) = local[hi];
    let (dx, dy) = (bx - ax, by - ay);
    let seg = dx.hypot(dy);
    let (mut worst_d, mut worst_i) = (-1.0, lo);
    for (i, &(px, py)) in local.iter().enumerate().take(hi).skip(lo + 1) {
        let d = if seg < 1e-9 {
            (px - ax).hypot(py - ay)
        } else {
            (dx * (ay - py) - (ax - px) * dy).abs() / seg
        };
        if d > worst_d {
            worst_d = d;
            worst_i = i;
        }
    }
    if worst_d > eps {
        keep[worst_i] = true;
        douglas_peucker(local, lo, worst_i, eps, keep);
        douglas_peucker(local, worst_i, hi, eps, keep);
    }
}

/// Vertex indices to archive: every curve vertex (padded) kept verbatim, the
/// straights simplified, loosening toward the budget but never past
/// [`MAX_EPS_TANGENT_M`] and never once loosening stops helping.
pub fn adaptive_simplify(coords: &[(f64, f64)], curving: &[bool], cum_m: &[f64]) -> Vec<usize> {
    let local = to_local_m(coords);
    let n = local.len();
    let mut base = vec![false; n];
    if n == 0 {
        return Vec::new();
    }
    base[0] = true;
    base[n - 1] = true;
    let mut k = 0;
    while k < n {
        if !curving[k] {
            k += 1;
            continue;
        }
        let mut j = k;
        while j < n && curving[j] {
            j += 1;
        }
        let mut lo = k;
        while lo > 0 && cum_m[k] - cum_m[lo - 1] <= CURVE_PAD_M {
            lo -= 1;
        }
        let mut hi = j - 1;
        while hi < n - 1 && cum_m[hi + 1] - cum_m[j - 1] <= CURVE_PAD_M {
            hi += 1;
        }
        for m in base.iter_mut().take(hi + 1).skip(lo) {
            *m = true;
        }
        k = j;
    }
    let build = |eps: f64| {
        let mut keep = base.clone();
        let ordered: Vec<usize> = (0..n).filter(|&i| base[i]).collect();
        for pair in ordered.windows(2) {
            if pair[1] > pair[0] + 1 {
                douglas_peucker(&local, pair[0], pair[1], eps, &mut keep);
            }
        }
        (0..n).filter(|&i| keep[i]).collect::<Vec<usize>>()
    };
    let mut eps = DP_EPS_TANGENT_M;
    let mut ordered = build(eps);
    while ordered.len() > POINT_BUDGET && eps < MAX_EPS_TANGENT_M {
        eps = (eps * 1.6).min(MAX_EPS_TANGENT_M);
        let grown = build(eps);
        if grown.len() >= ordered.len() {
            break;
        }
        ordered = grown;
    }
    ordered
}

/// The archive's record: quantized first point plus integer deltas, with
/// elevation in whole metres the same way.
pub fn encode(coords: &[(f64, f64)], elevations_ft: &[f64], idx: &[usize]) -> serde_json::Value {
    let scale = 10f64.powi(QUANT_DEG);
    let q = |x: f64| round_py_n(x * scale, 0) as i64;
    let lat: Vec<i64> = idx.iter().map(|&i| q(coords[i].1)).collect();
    let lon: Vec<i64> = idx.iter().map(|&i| q(coords[i].0)).collect();
    let ele: Vec<i64> = idx
        .iter()
        .map(|&i| round_py_n(elevations_ft[i] / FT_PER_M, 0) as i64)
        .collect();
    let deltas = |v: &[i64]| v.windows(2).map(|w| w[1] - w[0]).collect::<Vec<i64>>();
    serde_json::json!({
        "q": QUANT_DEG,
        "n": idx.len(),
        "lat0": lat[0],
        "lon0": lon[0],
        "ele0_m": ele[0],
        "dlat": deltas(&lat),
        "dlon": deltas(&lon),
        "dele_m": deltas(&ele),
    })
}

/// `(lon, lat)` vertices and their elevations in feet.
pub type Decoded = (Vec<(f64, f64)>, Vec<f64>);

/// `(lon, lat)` vertices and elevation in feet from an archive record.
pub fn decode(geom: &serde_json::Value) -> Option<Decoded> {
    let points = super::super::osm::geo::decode_geom(geom)?;
    let coords: Vec<(f64, f64)> = points.into_iter().map(|(lat, lon)| (lon, lat)).collect();
    let mut metres = geom.get("ele0_m")?.as_i64()?;
    let mut ele = vec![metres as f64 * FT_PER_M];
    for d in geom.get("dele_m")?.as_array()? {
        metres += d.as_i64()?;
        ele.push(metres as f64 * FT_PER_M);
    }
    if ele.len() != coords.len() {
        return None;
    }
    Some((coords, ele))
}

/// A vertex's position on the leg's own mileage scale.
pub fn at_mi(cum_m: &[f64], k: usize, mile_scale: f64) -> f64 {
    round_py_n(cum_m[k] / M_PER_MI * mile_scale, 2)
}

/// Index and distance (m) of the vertex nearest a point.
pub fn nearest_vertex(coords: &[(f64, f64)], lat: f64, lon: f64) -> (usize, f64) {
    let mut best = (0, f64::INFINITY);
    for (i, &(vlon, vlat)) in coords.iter().enumerate() {
        let d = haversine_m(lat, lon, vlat, vlon);
        if d < best.1 {
            best = (i, d);
        }
    }
    best
}

/// Distance in metres from a point to the polyline, and the metres run
/// along it to the foot of the perpendicular.
pub fn project(coords: &[(f64, f64)], cum_m: &[f64], lat: f64, lon: f64) -> (f64, f64) {
    let coslat = lat.to_radians().cos();
    let to = |p: (f64, f64)| {
        (
            (p.0 - lon).to_radians() * EARTH_R_M * coslat,
            (p.1 - lat).to_radians() * EARTH_R_M,
        )
    };
    let mut best = (f64::INFINITY, 0.0);
    for i in 1..coords.len() {
        let (ax, ay) = to(coords[i - 1]);
        let (bx, by) = to(coords[i]);
        let (dx, dy) = (bx - ax, by - ay);
        let len2 = dx * dx + dy * dy;
        let t = if len2 < 1e-9 {
            0.0
        } else {
            (-(ax * dx + ay * dy) / len2).clamp(0.0, 1.0)
        };
        let d = (ax + t * dx).hypot(ay + t * dy);
        if d < best.0 {
            best = (d, cum_m[i - 1] + t * (cum_m[i] - cum_m[i - 1]));
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A quarter circle of `radius_m` about (0, 0) in metres, as lon/lat
    /// near 42 N, between straights.
    fn bend(radius_m: f64) -> Vec<(f64, f64)> {
        let deg_lat = |m: f64| m / EARTH_R_M * 180.0 / std::f64::consts::PI;
        let coslat = 42f64.to_radians().cos();
        let deg_lon = |m: f64| deg_lat(m) / coslat;
        let mut pts = Vec::new();
        for i in 0..20 {
            pts.push((
                -88.0 + deg_lon(-1000.0 + i as f64 * 50.0),
                42.0 + deg_lat(-radius_m),
            ));
        }
        for i in 0..=30 {
            let a = -std::f64::consts::FRAC_PI_2 + i as f64 / 30.0 * std::f64::consts::FRAC_PI_2;
            pts.push((
                -88.0 + deg_lon(radius_m * a.cos()),
                42.0 + deg_lat(radius_m * a.sin()),
            ));
        }
        for i in 1..20 {
            pts.push((-88.0 + deg_lon(radius_m), 42.0 + deg_lat(i as f64 * 50.0)));
        }
        pts
    }

    #[test]
    fn a_quarter_circle_reads_as_one_left_curve_of_its_radius() {
        let coords = bend(300.0);
        let cum = cumulative_m(&coords);
        let c = analyse(&coords, &cum);
        assert_eq!(c.curves.len(), 1, "{:?}", c.curves);
        let curve = &c.curves[0];
        assert_eq!(curve.direction, 'L');
        let ft = 300.0 * FT_PER_M;
        assert!(
            (curve.min_radius_ft as f64 - ft).abs() / ft < 0.15,
            "{}",
            curve.min_radius_ft
        );
        assert!((curve.deflection_deg - 90.0).abs() < 8.0);
    }

    #[test]
    fn the_simplifier_keeps_the_curve_and_thins_the_straights() {
        let coords = bend(300.0);
        let cum = cumulative_m(&coords);
        let c = analyse(&coords, &cum);
        let idx = adaptive_simplify(&coords, &c.curving, &cum);
        assert!(idx.len() < coords.len());
        for k in 20..=50 {
            if c.curving[k] {
                assert!(idx.contains(&k), "curve vertex {k} dropped");
            }
        }
    }

    #[test]
    fn encode_and_decode_round_trip() {
        let coords = vec![(-87.9, 42.5), (-87.89, 42.49), (-87.88, 42.48)];
        let ele = vec![600.0, 610.0, 590.0];
        let geom = encode(&coords, &ele, &[0, 1, 2]);
        let (back, feet) = decode(&geom).unwrap();
        for (a, b) in coords.iter().zip(&back) {
            assert!((a.0 - b.0).abs() < 1e-5 && (a.1 - b.1).abs() < 1e-5);
        }
        assert!((feet[1] - 610.0).abs() < 2.0);
    }

    #[test]
    fn the_advisory_stops_at_the_top_of_its_table() {
        assert_eq!(advisory_mph(100_000.0), ADVISORY_MAX_MPH);
        assert_eq!(advisory_mph(64.0), 15);
    }
}
