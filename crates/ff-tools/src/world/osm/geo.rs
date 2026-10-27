//! The road each leg drives, and the OpenStreetMap ways near it.
//!
//! The leg's road comes from the checked-in geometry archive
//! (`world_data/us/geometry/<state>.jsonl`), the one source that stays right
//! after a reroute. The ways come from a Geofabrik extract (see
//! [`super::pbf`]) and are bucketed into a coarse grid so each lookup only
//! measures the segments beside it.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use crate::util::{self, Result};

/// Grid cell size in degrees: about 550 m of latitude and 365 m of longitude
/// at the Canadian border, so a cell and its eight neighbours always cover
/// the widest search below (90 m).
pub const CELL_DEG: f64 = 0.005;
const EARTH_R_M: f64 = 6_371_000.0;
pub const M_PER_MI: f64 = 1609.344;

pub type Cell = (i64, i64);

pub fn cell(lat: f64, lon: f64) -> Cell {
    (
        (lat / CELL_DEG).floor() as i64,
        (lon / CELL_DEG).floor() as i64,
    )
}

/// A cell and its eight neighbours.
pub fn around(c: Cell) -> impl Iterator<Item = Cell> {
    (-1..=1).flat_map(move |dx| (-1..=1).map(move |dy| (c.0 + dx, c.1 + dy)))
}

pub fn haversine_m(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    let (p1, p2) = (lat1.to_radians(), lat2.to_radians());
    let dp = (lat2 - lat1).to_radians();
    let dl = (lon2 - lon1).to_radians();
    let a = (dp / 2.0).sin().powi(2) + p1.cos() * p2.cos() * (dl / 2.0).sin().powi(2);
    2.0 * EARTH_R_M * a.sqrt().min(1.0).asin()
}

/// One leg's archived road: `(lat, lon)` vertices and the metres run to each.
#[derive(Debug, Clone)]
pub struct LegLine {
    pub points: Vec<(f64, f64)>,
    pub cum_m: Vec<f64>,
}

impl LegLine {
    pub fn new(points: Vec<(f64, f64)>) -> Self {
        let mut cum_m = Vec::with_capacity(points.len());
        let mut run = 0.0;
        for (i, p) in points.iter().enumerate() {
            if i > 0 {
                let q = points[i - 1];
                run += haversine_m(q.0, q.1, p.0, p.1);
            }
            cum_m.push(run);
        }
        Self { points, cum_m }
    }

    pub fn length_m(&self) -> f64 {
        self.cum_m.last().copied().unwrap_or(0.0)
    }

    /// `(lat, lon)` at `mile` on the leg's own mileage scale.
    pub fn point_at(&self, mile: f64, leg_miles: f64) -> (f64, f64) {
        let raw_mi = self.length_m() / M_PER_MI;
        let scale = if leg_miles > 0.0 && raw_mi > 0.0 {
            leg_miles / raw_mi
        } else {
            1.0
        };
        let miles: Vec<f64> = self.cum_m.iter().map(|m| m / M_PER_MI * scale).collect();
        let i = miles
            .partition_point(|m| *m < mile)
            .clamp(1, self.points.len() - 1);
        let (m0, m1) = (miles[i - 1], miles[i]);
        let t = if m1 <= m0 {
            0.0
        } else {
            ((mile - m0) / (m1 - m0)).clamp(0.0, 1.0)
        };
        let (a, b) = (self.points[i - 1], self.points[i]);
        (a.0 + t * (b.0 - a.0), a.1 + t * (b.1 - a.1))
    }
}

/// Decode one archive record's `geom` (quantized deltas) to `(lat, lon)`.
pub fn decode_geom(geom: &serde_json::Value) -> Option<Vec<(f64, f64)>> {
    let q = geom.get("q")?.as_i64()?;
    let scale = 10f64.powi(i32::try_from(q).ok()?);
    let mut lat = geom.get("lat0")?.as_i64()?;
    let mut lon = geom.get("lon0")?.as_i64()?;
    let dlat = geom.get("dlat")?.as_array()?;
    let dlon = geom.get("dlon")?.as_array()?;
    let mut out = vec![(lat as f64 / scale, lon as f64 / scale)];
    for (a, b) in dlat.iter().zip(dlon) {
        lat += a.as_i64()?;
        lon += b.as_i64()?;
        out.push((lat as f64 / scale, lon as f64 / scale));
    }
    Some(out)
}

/// Every archived leg line, keyed `from:to`.
pub fn load_archive(geometry_dir: &Path) -> Result<HashMap<String, LegLine>> {
    let mut out = HashMap::new();
    for path in util::glob_suffix(geometry_dir, ".jsonl") {
        let text = util::read_text(&path)?;
        for line in text.lines() {
            if line.trim().is_empty() || line.starts_with("{\"meta\"") {
                continue;
            }
            let rec: serde_json::Value = serde_json::from_str(line)
                .map_err(|e| format!("{}: {e}", util::rel(&path, geometry_dir)))?;
            let (Some(leg), Some(points)) = (
                rec.get("leg").and_then(|v| v.as_str()),
                rec.get("geom").and_then(decode_geom),
            ) else {
                continue;
            };
            if points.len() >= 2 {
                out.insert(leg.to_string(), LegLine::new(points));
            }
        }
    }
    Ok(out)
}

/// The grid cells within one cell of any leg vertex: the only places a way
/// can matter. Archive vertices sit closer together than a cell on every
/// leg checked, but a long straight hop is filled in so no cell is skipped.
pub fn corridor_cells<'a>(lines: impl Iterator<Item = &'a LegLine>) -> HashSet<Cell> {
    let mut cells = HashSet::new();
    for line in lines {
        for pair in line.points.windows(2) {
            for c in segment_cells(pair[0], pair[1]) {
                cells.extend(around(c));
            }
        }
    }
    cells
}

/// The cells a straight segment passes through, walked in half-cell steps
/// so a long rural segment costs its length and not its bounding box.
fn segment_cells(a: (f64, f64), b: (f64, f64)) -> impl Iterator<Item = Cell> {
    let span = ((b.0 - a.0).abs().max((b.1 - a.1).abs()) / (CELL_DEG / 2.0)).ceil();
    let steps = (span as usize).max(1);
    (0..=steps).map(move |s| {
        let t = s as f64 / steps as f64;
        cell(a.0 + t * (b.0 - a.0), a.1 + t * (b.1 - a.1))
    })
}

/// One OpenStreetMap way that carries a posted limit.
#[derive(Debug, Clone, PartialEq)]
pub struct OsmWay {
    pub id: i64,
    /// The way's `ref` tag (`"US 19;GA 300"`), or empty.
    pub route_ref: String,
    /// The posted limit in mph, `maxspeed:hgv` before `maxspeed`; `None`
    /// when the tag is there but carries no number ("signals", "none").
    pub mph: Option<f64>,
    /// Whether `mph` came from the truck tag.
    pub hgv: bool,
    /// The same reading where a bare number is km/h: the limit to use when
    /// the way lies in Canada (see [`super::region`]). Equal to `mph` and
    /// `hgv` when the tags name the unit or the way's jurisdiction.
    pub ca_mph: Option<f64>,
    pub ca_hgv: bool,
    pub coords: Vec<(f64, f64)>,
}

impl OsmWay {
    /// The limit and whether it is the truck tag's, read in Canada's unit
    /// or the United States'.
    pub fn limit(&self, in_canada: bool) -> Option<(f64, bool)> {
        if in_canada {
            self.ca_mph.map(|m| (m, self.ca_hgv))
        } else {
            self.mph.map(|m| (m, self.hgv))
        }
    }
}

/// Way segments bucketed by grid cell, kept only where a leg runs.
#[derive(Default)]
pub struct WayIndex {
    pub ways: Vec<OsmWay>,
    grid: HashMap<Cell, Vec<(u32, u32)>>,
}

/// Point-to-segment distance in metres on a local flat projection.
fn seg_dist_m(lat: f64, lon: f64, a: (f64, f64), b: (f64, f64), coslat: f64) -> f64 {
    let proj = |p: (f64, f64)| {
        (
            p.1.to_radians() * EARTH_R_M * coslat,
            p.0.to_radians() * EARTH_R_M,
        )
    };
    let (ax, ay) = proj(a);
    let (bx, by) = proj(b);
    let (px, py) = proj((lat, lon));
    let (dx, dy) = (bx - ax, by - ay);
    let seg2 = dx * dx + dy * dy;
    if seg2 < 1e-9 {
        return (px - ax).hypot(py - ay);
    }
    let t = (((px - ax) * dx + (py - ay) * dy) / seg2).clamp(0.0, 1.0);
    (px - (ax + t * dx)).hypot(py - (ay + t * dy))
}

impl WayIndex {
    /// Index `ways`, keeping only segments that cross a `wanted` cell. Ways
    /// with no such segment are dropped, so the index holds the corridor and
    /// nothing else.
    pub fn build(ways: Vec<OsmWay>, wanted: &HashSet<Cell>) -> Self {
        let mut index = WayIndex::default();
        for way in ways {
            let wi = index.ways.len() as u32;
            let mut kept = false;
            for (si, pair) in way.coords.windows(2).enumerate() {
                let mut seen: Vec<Cell> = Vec::new();
                for c in segment_cells(pair[0], pair[1]) {
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

    /// Every way within `tol_m` of the point, with its distance (nearest
    /// segment per way), in way-id order so ties break the same way every run.
    pub fn near(&self, lat: f64, lon: f64, tol_m: f64, coslat: f64) -> Vec<(&OsmWay, f64)> {
        let mut best: HashMap<u32, f64> = HashMap::new();
        for c in around(cell(lat, lon)) {
            for &(wi, si) in self.grid.get(&c).into_iter().flatten() {
                let coords = &self.ways[wi as usize].coords;
                let si = si as usize;
                let d = seg_dist_m(lat, lon, coords[si], coords[si + 1], coslat);
                if d < tol_m {
                    let slot = best.entry(wi).or_insert(f64::INFINITY);
                    *slot = slot.min(d);
                }
            }
        }
        let mut out: Vec<(&OsmWay, f64)> = best
            .into_iter()
            .map(|(wi, d)| (&self.ways[wi as usize], d))
            .collect();
        out.sort_by_key(|(w, _)| w.id);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_the_archive_encoding() {
        let geom = serde_json::json!({"q": 5, "lat0": 3000000, "lon0": -9700000,
            "dlat": [100, -50], "dlon": [0, 25]});
        let pts = decode_geom(&geom).unwrap();
        assert_eq!(
            pts,
            vec![(30.0, -97.0), (30.001, -97.0), (30.0005, -96.99975)]
        );
    }

    #[test]
    fn near_finds_a_way_beside_the_road_and_not_one_across_town() {
        let way = |id, lat: f64| OsmWay {
            id,
            route_ref: String::new(),
            mph: Some(55.0),
            hgv: false,
            ca_mph: Some(55.0),
            ca_hgv: false,
            coords: vec![(lat, -97.01), (lat, -96.99)],
        };
        let line = LegLine::new(vec![(30.0, -97.0), (30.01, -97.0)]);
        let wanted = corridor_cells(std::iter::once(&line));
        // 0.0005 deg of latitude is 55 m; 0.01 is over a kilometre.
        let index = WayIndex::build(vec![way(1, 30.0005), way(2, 30.02)], &wanted);
        let coslat = 30f64.to_radians().cos();
        let near = index.near(30.0, -97.0, 90.0, coslat);
        assert_eq!(near.len(), 1);
        assert_eq!(near[0].0.id, 1);
        assert!((near[0].1 - 55.6).abs() < 1.0, "{}", near[0].1);
    }

    #[test]
    fn point_at_follows_the_leg_mileage() {
        let line = LegLine::new(vec![(30.0, -97.0), (31.0, -97.0)]);
        let (lat, _) = line.point_at(50.0, 100.0);
        assert!((lat - 30.5).abs() < 1e-9);
    }
}
