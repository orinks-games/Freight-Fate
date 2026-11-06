//! The map layers a reroute has to rebuild, beyond the road itself: lane
//! counts, posted clearances and weight limits, landmarks, villages, traffic
//! volumes and terrain class.
//!
//! Ports of the deleted Python layer builders (`bake_lane_segments.py`,
//! `build_interchanges_restrictions.py`, `bake_landmarks.py`,
//! `bake_villages.py`, `build_traffic_aadt.py`, `build_terrain_type.py`),
//! one `cargo tools` command each:
//!
//! ```text
//! cargo tools osm-lanes        --pbf FILE [--only a:b;c:d] [--write]
//! cargo tools osm-restrictions --pbf FILE [--only a:b;c:d] [--write]
//! cargo tools osm-landmarks    --pbf FILE [--only a:b;c:d] [--write]
//! cargo tools osm-villages     --pbf FILE [--only a:b;c:d] [--write]
//! cargo tools hpms-traffic     [--only a:b;c:d] [--write]
//! cargo tools hpms-terrain     [--only a:b;c:d] [--write]
//! ```
//!
//! What changed from the Python, for all of them: every leg reads its road
//! from the checked-in geometry archive (`world_data/us/geometry/`), the
//! road it drives after a reroute; a leg with no archived road is left as it
//! is and counted. The OpenStreetMap layers read a local `.osm.pbf` (the
//! weekly planet cut to the lower 48, see `osm-speed-limits`) instead of a
//! self-hosted Overpass, so a run needs no server and gives the same answer
//! twice. `--only` names legs in either direction. Each command is a dry run
//! until `--write`, which saves the world source and runs `index-world`;
//! re-bake `world.ffdata` afterwards.

pub mod features;
pub mod hpms;
pub mod landmarks;
pub mod lanes;
pub mod restrictions;
pub mod villages;

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use super::osm::geo::{self, LegLine, M_PER_MI};
use super::{load_world, save_world, source_root};
use crate::cli::Args;
use crate::pyjson::{Object, Value};
use crate::util::{self, Result};

pub fn leg_id(leg: &Value) -> String {
    let end = |k| leg.get(k).map(Value::py_str).unwrap_or_default();
    format!("{}:{}", end("from"), end("to"))
}

/// The `--only` set: `from:to` pairs, matched in either direction.
pub struct Only(Option<HashSet<String>>);

impl Only {
    pub fn parse(args: &Args) -> Self {
        Self(args.value("only").map(|o| {
            o.split(';')
                .map(str::trim)
                .filter(|p| p.contains(':'))
                .map(str::to_string)
                .collect()
        }))
    }

    pub fn wants(&self, id: &str) -> bool {
        let Some(set) = &self.0 else { return true };
        let (a, b) = id.split_once(':').unwrap_or_default();
        set.contains(id) || set.contains(&format!("{b}:{a}"))
    }

    pub fn is_some(&self) -> bool {
        self.0.is_some()
    }
}

/// The `--pbf` extracts, checked to exist.
pub fn pbf_paths(args: &Args, usage: &str) -> Result<Vec<PathBuf>> {
    let pbfs: Vec<PathBuf> = args.all("pbf").iter().map(PathBuf::from).collect();
    if pbfs.is_empty() {
        return Err(format!("--pbf is required\n{usage}"));
    }
    for path in &pbfs {
        if !path.is_file() {
            return Err(format!("no extract at {}", path.display()));
        }
    }
    Ok(pbfs)
}

/// The extract date for source notes: `--extract-date`, else the header's.
pub fn extract_date(args: &Args, pbfs: &[PathBuf]) -> Result<String> {
    match args.value("extract-date") {
        Some(d) => Ok(d.to_string()),
        None => super::osm::pbf::extract_date(&pbfs[0])?.ok_or_else(|| {
            "the extract's header carries no date; pass --extract-date YYYY-MM-DD".into()
        }),
    }
}

/// One leg's archived road on the leg's own mileage scale.
#[derive(Debug, Clone)]
pub struct Route {
    pub points: Vec<(f64, f64)>,
    /// Leg miles at each vertex (the archive's length rescaled to `miles`).
    pub cum_mi: Vec<f64>,
}

impl Route {
    pub fn new(line: &LegLine, leg_miles: f64) -> Self {
        let raw_mi = line.length_m() / M_PER_MI;
        let scale = if leg_miles > 0.0 && raw_mi > 0.0 {
            leg_miles / raw_mi
        } else {
            1.0
        };
        Self {
            points: line.points.clone(),
            cum_mi: line.cum_m.iter().map(|m| m / M_PER_MI * scale).collect(),
        }
    }

    pub fn total_mi(&self) -> f64 {
        self.cum_mi.last().copied().unwrap_or(0.0)
    }

    /// `(min_lat, min_lon, max_lat, max_lon)` of the road.
    pub fn bounds(&self) -> (f64, f64, f64, f64) {
        let mut b = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
        for &(lat, lon) in &self.points {
            b = (b.0.min(lat), b.1.min(lon), b.2.max(lat), b.3.max(lon));
        }
        b
    }

    /// The nearest point on the road to `(lat, lon)`, measured the way the
    /// Python did (the foot of the perpendicular in flat degrees, the
    /// distance to it by haversine): `(at_mi, off_mi, foot_lat, foot_lon)`.
    pub fn project(&self, lat: f64, lon: f64) -> (f64, f64, f64, f64) {
        let mut best = (f64::INFINITY, 0.0, lat, lon);
        for i in 1..self.points.len() {
            let (ay, ax) = self.points[i - 1];
            let (by, bx) = self.points[i];
            let (dx, dy) = (bx - ax, by - ay);
            let t = if dx == 0.0 && dy == 0.0 {
                0.0
            } else {
                (((lon - ax) * dx + (lat - ay) * dy) / (dx * dx + dy * dy)).clamp(0.0, 1.0)
            };
            let (qx, qy) = (ax + t * dx, ay + t * dy);
            let off = geo::haversine_m(lat, lon, qy, qx) / M_PER_MI;
            if off < best.0 {
                let at = self.cum_mi[i - 1] + t * (self.cum_mi[i] - self.cum_mi[i - 1]);
                best = (off, at, qy, qx);
            }
        }
        (best.1, best.0, best.2, best.3)
    }
}

/// A coarse grid of item indices, for "what is near this road" lookups.
pub struct Grid {
    pub deg: f64,
    cells: HashMap<(i64, i64), Vec<u32>>,
}

impl Grid {
    pub fn new(deg: f64) -> Self {
        Self {
            deg,
            cells: HashMap::new(),
        }
    }

    pub fn key(&self, lat: f64, lon: f64) -> (i64, i64) {
        (
            (lat / self.deg).floor() as i64,
            (lon / self.deg).floor() as i64,
        )
    }

    pub fn insert_point(&mut self, lat: f64, lon: f64, item: u32) {
        let key = self.key(lat, lon);
        let slot = self.cells.entry(key).or_default();
        if slot.last() != Some(&item) {
            slot.push(item);
        }
    }

    /// File `item` under every cell a polyline passes through.
    pub fn insert_line(&mut self, coords: &[(f64, f64)], item: u32) {
        if coords.len() == 1 {
            self.insert_point(coords[0].0, coords[0].1, item);
        }
        for pair in coords.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            let span = ((b.0 - a.0).abs().max((b.1 - a.1).abs()) / (self.deg / 2.0)).ceil();
            let steps = (span as usize).max(1);
            for s in 0..=steps {
                let t = s as f64 / steps as f64;
                self.insert_point(a.0 + t * (b.0 - a.0), a.1 + t * (b.1 - a.1), item);
            }
        }
    }

    /// File `item` under every cell of a bounding box.
    pub fn insert_box(&mut self, b: (f64, f64, f64, f64), item: u32) {
        let (lo, hi) = (self.key(b.0, b.1), self.key(b.2, b.3));
        for y in lo.0..=hi.0 {
            for x in lo.1..=hi.1 {
                self.cells.entry((y, x)).or_default().push(item);
            }
        }
    }

    /// Every item filed within `reach` cells of any vertex of the road (the
    /// road's segments walked so no cell is skipped), sorted and unique.
    pub fn along(&self, route: &[(f64, f64)], reach: i64) -> Vec<u32> {
        let mut keys: HashSet<(i64, i64)> = HashSet::new();
        for pair in route.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            let span = ((b.0 - a.0).abs().max((b.1 - a.1).abs()) / (self.deg / 2.0)).ceil();
            let steps = (span as usize).max(1);
            for s in 0..=steps {
                let t = s as f64 / steps as f64;
                let k = self.key(a.0 + t * (b.0 - a.0), a.1 + t * (b.1 - a.1));
                for dy in -reach..=reach {
                    for dx in -reach..=reach {
                        keys.insert((k.0 + dy, k.1 + dx));
                    }
                }
            }
        }
        let mut out: Vec<u32> = keys
            .iter()
            .filter_map(|k| self.cells.get(k))
            .flatten()
            .copied()
            .collect();
        out.sort_unstable();
        out.dedup();
        out
    }
}

/// The world, the archive and the per-leg loop every layer command shares.
pub struct Run {
    pub world: Object,
    pub archive: HashMap<String, LegLine>,
}

impl Run {
    pub fn load() -> Result<Self> {
        let world = load_world(&source_root())?;
        let archive = geo::load_archive(&util::repo_root().join("data/world_data/us/geometry"))?;
        Ok(Self { world, archive })
    }

    pub fn legs_mut(&mut self) -> Result<&mut Vec<Value>> {
        self.world
            .get_mut("legs")
            .and_then(Value::as_array_mut)
            .ok_or_else(|| "world_source has no legs".to_string())
    }

    /// Save the source and refresh `world_data/`, or say it was a dry run.
    pub fn finish(&self, write: bool) -> Result<i32> {
        if !write {
            println!("(dry run; pass --write to update the map data)");
            return Ok(0);
        }
        let n = save_world(&self.world, &source_root())?;
        println!("{n} world source file(s) changed.");
        super::index::main(&[])?;
        println!("Re-bake world.ffdata next.");
        Ok(0)
    }
}

/// The leg's corridor object, created when it is missing.
pub fn corridor_mut(leg: &mut Value) -> Result<&mut Object> {
    let id = leg_id(leg);
    let obj = leg
        .as_object_mut()
        .ok_or_else(|| format!("{id} is not an object"))?;
    if !matches!(obj.get("corridor"), Some(Value::Object(_))) {
        obj.insert("corridor", Value::Object(Object::new()));
    }
    obj.get_mut("corridor")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| format!("{id} has no corridor"))
}

pub fn leg_miles(leg: &Value) -> f64 {
    leg.get("miles").and_then(Value::as_f64).unwrap_or(0.0)
}

pub fn highway(leg: &Value) -> String {
    leg.get("highway").map(Value::py_str).unwrap_or_default()
}

/// Python's `round(x, n)` on a float, as the data was written.
pub fn round(x: f64, n: i32) -> f64 {
    ff_core::pyfmt::round_py_n(x, n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_matches_either_direction() {
        let args = Args::parse(
            &["--only".into(), "a:b;c:d".into()],
            &crate::cli::Spec {
                values: &["only"],
                flags: &[],
            },
        )
        .unwrap();
        let only = Only::parse(&args);
        assert!(only.wants("a:b") && only.wants("b:a") && only.wants("d:c"));
        assert!(!only.wants("a:c"));
    }

    #[test]
    fn project_finds_the_foot_and_the_mile() {
        let line = LegLine::new(vec![(30.0, -97.0), (31.0, -97.0)]);
        let route = Route::new(&line, 100.0);
        let (at, off, lat, lon) = route.project(30.5, -96.99);
        assert!((at - 50.0).abs() < 1e-6, "{at}");
        assert!((lat - 30.5).abs() < 1e-9 && (lon + 97.0).abs() < 1e-9);
        assert!((off - 0.596).abs() < 0.01, "{off}");
    }

    #[test]
    fn grid_finds_items_along_the_road_only() {
        let mut grid = Grid::new(0.05);
        grid.insert_point(30.02, -97.0, 1);
        grid.insert_point(32.0, -97.0, 2);
        grid.insert_line(&[(30.5, -97.2), (30.5, -96.8)], 3);
        let near = grid.along(&[(30.0, -97.0), (30.6, -97.0)], 1);
        assert_eq!(near, vec![1, 3]);
    }
}
