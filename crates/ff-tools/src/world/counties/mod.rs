//! `census-counties`: write the county each stretch of every leg runs
//! through, for the location key that names it.
//!
//! ```text
//! cargo tools census-counties --shp tl_2021_us_county.shp            # dry run
//! cargo tools census-counties --shp tl_2021_us_county.shp --write    # write it
//! ```
//!
//! Input: the Census Bureau's TIGER/Line county file (`--shp`, with its
//! `.dbf` beside it), from
//! `https://www2.census.gov/geo/tiger/TIGER2021/COUNTY/tl_2021_us_county.zip`.
//! The 2021 vintage is the last with Connecticut's eight counties; from 2022
//! the Census files carry its planning regions instead, which no sign or
//! driver uses. TIGER/Line rather than the generalized cartographic files,
//! so a road that runs along a county line is placed against the surveyed
//! line, and county water area counts as the county (a bridge over a bay
//! stays in its county).
//!
//! Each leg's archived route (`world_data/us/geometry/`) is sampled every
//! tenth of a mile on the leg's own mileage scale; each change of county
//! between two samples is placed by bisection and rounded to a tenth.
//! A sample in no county (open water off the coast, or outside the country)
//! keeps the county before it. Output, with `--write`: `corridor.counties`
//! on each leg in `world_source/`, then `index-world`. Re-bake
//! `world.ffdata` afterwards.

pub mod shp;

use std::collections::HashMap;
use std::path::PathBuf;

use rayon::prelude::*;

use super::osm::geo::{self, LegLine};
use super::{load_world, save_world, source_root};
use crate::cli::{Args, Spec};
use crate::pyjson::{Object, Value};
use crate::util::{self, Result};
use shp::Polygon;

const USAGE: &str = "usage: census-counties --shp tl_2021_us_county.shp [--write]";

/// The sampling step along a leg, in leg miles: the precision `at_mi` is
/// stored at everywhere else in the corridor.
const STEP_MI: f64 = 0.1;

/// The grid the county lookup is bucketed on, in degrees.
const CELL_DEG: f64 = 0.25;

const SOURCE: &str = "Derived: the archived route geometry sampled every 0.1 mi against the \
                      U.S. Census Bureau TIGER/Line 2021 county boundaries (public domain); \
                      a county line is placed by bisection between samples.";

/// One county: its spoken name and its outline.
struct County {
    name: String,
    shape: Polygon,
}

/// The name a driver says: "Lake County", "Orleans Parish", and "Richmond
/// City" for an independent city, the way Virginia, Baltimore and St. Louis
/// say it. Census writes those "Richmond city". No article, so the line
/// reads "In Richmond City." the way Alt+1 reads "In District of Columbia."
pub fn spoken_county(name: &str, name_lsad: &str, lsad: &str) -> String {
    if lsad == "25" {
        if let Some(stem) = name_lsad.strip_suffix(" city") {
            return format!("{stem} City");
        }
    }
    if name_lsad.is_empty() {
        name.to_string()
    } else {
        name_lsad.to_string()
    }
}

struct Lookup {
    counties: Vec<County>,
    cells: HashMap<(i32, i32), Vec<usize>>,
}

fn cell_of(lon: f64, lat: f64) -> (i32, i32) {
    (
        (lon / CELL_DEG).floor() as i32,
        (lat / CELL_DEG).floor() as i32,
    )
}

impl Lookup {
    fn new(counties: Vec<County>) -> Self {
        let mut cells: HashMap<(i32, i32), Vec<usize>> = HashMap::new();
        for (i, county) in counties.iter().enumerate() {
            let (x0, y0, x1, y1) = county.shape.bbox;
            if county.shape.rings.is_empty() {
                continue;
            }
            let (cx0, cy0) = cell_of(x0, y0);
            let (cx1, cy1) = cell_of(x1, y1);
            for cx in cx0..=cx1 {
                for cy in cy0..=cy1 {
                    cells.entry((cx, cy)).or_default().push(i);
                }
            }
        }
        Self { counties, cells }
    }

    fn at(&self, lat: f64, lon: f64) -> Option<usize> {
        self.cells
            .get(&cell_of(lon, lat))?
            .iter()
            .copied()
            .find(|&i| self.counties[i].shape.contains(lon, lat))
    }
}

/// A leg's archived road on its own mileage scale.
struct Walker<'a> {
    line: &'a LegLine,
    miles: Vec<f64>,
}

impl<'a> Walker<'a> {
    fn new(line: &'a LegLine, leg_miles: f64) -> Self {
        let raw_mi = line.length_m() / geo::M_PER_MI;
        let scale = if leg_miles > 0.0 && raw_mi > 0.0 {
            leg_miles / raw_mi
        } else {
            1.0
        };
        let miles = line
            .cum_m
            .iter()
            .map(|m| m / geo::M_PER_MI * scale)
            .collect();
        Self { line, miles }
    }

    fn point(&self, mile: f64) -> (f64, f64) {
        let n = self.line.points.len();
        let i = self.miles.partition_point(|m| *m < mile).clamp(1, n - 1);
        let (m0, m1) = (self.miles[i - 1], self.miles[i]);
        let t = if m1 <= m0 {
            0.0
        } else {
            ((mile - m0) / (m1 - m0)).clamp(0.0, 1.0)
        };
        let (a, b) = (self.line.points[i - 1], self.line.points[i]);
        (a.0 + t * (b.0 - a.0), a.1 + t * (b.1 - a.1))
    }
}

/// `(at_mi, county index)` runs along one leg; empty when no sample lands
/// in any county.
fn leg_runs(line: &LegLine, leg_miles: f64, lookup: &Lookup) -> (Vec<(f64, usize)>, usize, usize) {
    let walk = Walker::new(line, leg_miles);
    let county_at = |mile: f64| {
        let (lat, lon) = walk.point(mile);
        lookup.at(lat, lon)
    };
    let steps = (leg_miles / STEP_MI).ceil().max(1.0) as usize;
    let mut runs: Vec<(f64, usize)> = Vec::new();
    let (mut samples, mut missed) = (0, 0);
    let mut last: Option<(f64, usize)> = None;
    for k in 0..=steps {
        let mile = (k as f64 * STEP_MI).min(leg_miles);
        samples += 1;
        let Some(here) = county_at(mile) else {
            missed += 1;
            continue;
        };
        match last {
            None => runs.push((0.0, here)),
            Some((_, before)) if before == here => {}
            Some((from, before)) => {
                // The line lies between the two samples: halve the gap
                // until it is under a hundredth of a mile.
                let (mut lo, mut hi) = (from, mile);
                while hi - lo > 0.01 {
                    let mid = (lo + hi) / 2.0;
                    if county_at(mid) == Some(before) {
                        lo = mid;
                    } else {
                        hi = mid;
                    }
                }
                let at = ff_core::pyfmt::round_py_n(hi, 1).clamp(0.0, leg_miles);
                // Two lines inside one tenth: the later county wins the slot.
                if runs.last().is_some_and(|r| r.0 >= at) {
                    runs.pop();
                }
                if runs.last().map(|r| r.1) != Some(here) {
                    runs.push((at, here));
                }
            }
        }
        last = Some((mile, here));
    }
    (runs, samples, missed)
}

fn leg_id(leg: &Value) -> String {
    let end = |k| leg.get(k).map(Value::py_str).unwrap_or_default();
    format!("{}:{}", end("from"), end("to"))
}

pub fn main(argv: &[String]) -> Result<i32> {
    let args = Args::parse(
        argv,
        &Spec {
            values: &["shp"],
            flags: &["write"],
        },
    )
    .map_err(|e| format!("{e}\n{USAGE}"))?;
    let shp_path = PathBuf::from(
        args.value("shp")
            .ok_or(format!("--shp is required\n{USAGE}"))?,
    );
    let dbf_path = shp_path.with_extension("dbf");
    let shapes = shp::read_polygons(&shp_path)?;
    let table = shp::read_table(&dbf_path)?;
    if shapes.len() != table.len() {
        return Err(format!(
            "{} has {} shapes but {} has {} rows",
            shp_path.display(),
            shapes.len(),
            dbf_path.display(),
            table.len()
        ));
    }
    let column = |row: &[(String, String)], key: &str| {
        row.iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.clone())
            .unwrap_or_default()
    };
    let counties: Vec<County> = shapes
        .into_iter()
        .zip(&table)
        .map(|(shape, row)| County {
            name: spoken_county(
                &column(row, "NAME"),
                &column(row, "NAMELSAD"),
                &column(row, "LSAD"),
            ),
            shape,
        })
        .collect();
    println!(
        "{} counties read from {}",
        counties.len(),
        shp_path.display()
    );
    let lookup = Lookup::new(counties);

    let root = util::repo_root();
    let source = source_root();
    let mut world = load_world(&source)?;
    let lines = geo::load_archive(&root.join("data/world_data/us/geometry"))?;
    let legs = world
        .get_mut("legs")
        .and_then(Value::as_array_mut)
        .ok_or("world_source has no legs")?;
    let computed: Vec<Option<(Vec<(f64, usize)>, usize, usize)>> = legs
        .par_iter()
        .map(|leg| {
            let line = lines.get(&leg_id(leg))?;
            let miles = leg.get("miles").and_then(Value::as_f64)?;
            Some(leg_runs(line, miles, &lookup))
        })
        .collect();
    let (mut changed, mut same, mut no_archive, mut no_county) = (0, 0, 0, 0);
    let (mut samples, mut missed, mut runs_total) = (0usize, 0usize, 0usize);
    for (leg, result) in legs.iter_mut().zip(computed) {
        let Some((runs, leg_samples, leg_missed)) = result else {
            no_archive += 1;
            continue;
        };
        samples += leg_samples;
        missed += leg_missed;
        if runs.is_empty() {
            no_county += 1;
            continue;
        }
        runs_total += runs.len();
        let rows: Vec<Value> = runs
            .iter()
            .map(|(at, i)| {
                let mut row = Object::new();
                row.insert("at_mi", Value::Float(*at));
                row.insert("name", Value::Str(lookup.counties[*i].name.clone()));
                row.insert("source", Value::Str(SOURCE.to_string()));
                Value::Object(row)
            })
            .collect();
        let rows = Value::Array(rows);
        let id = leg_id(leg);
        let corridor = leg
            .as_object_mut()
            .ok_or_else(|| format!("{id} is not an object"))?
            .get_mut("corridor")
            .and_then(Value::as_object_mut)
            .ok_or_else(|| format!("{id} has no corridor"))?;
        if corridor.get("counties") == Some(&rows) {
            same += 1;
        } else {
            changed += 1;
            corridor.insert("counties", rows);
        }
    }
    println!(
        "{changed} legs changed, {same} the same, {no_archive} with no archived road and \
         {no_county} with no county kept as they were; {runs_total} county runs."
    );
    println!(
        "{missed} of {samples} samples ({:.2}%) fell in no county and kept the county before them.",
        if samples == 0 {
            0.0
        } else {
            100.0 * missed as f64 / samples as f64
        }
    );
    if !args.flag("write") {
        println!("(dry run; pass --write to update the map data)");
        return Ok(0);
    }
    save_world(&world, &source)?;
    super::index::main(&[])?;
    println!("Wrote the world source and world_data/. Re-bake world.ffdata next.");
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counties_are_named_the_way_drivers_say_them() {
        assert_eq!(spoken_county("Lake", "Lake County", "06"), "Lake County");
        assert_eq!(
            spoken_county("Orleans", "Orleans Parish", "15"),
            "Orleans Parish"
        );
        assert_eq!(
            spoken_county("Richmond", "Richmond city", "25"),
            "Richmond City"
        );
        assert_eq!(
            spoken_county("Carson City", "Carson City", "25"),
            "Carson City"
        );
        assert_eq!(
            spoken_county("District of Columbia", "District of Columbia", "00"),
            "District of Columbia"
        );
    }

    fn square(x0: f64, x1: f64) -> Polygon {
        Polygon {
            rings: vec![vec![(x0, 0.0), (x1, 0.0), (x1, 1.0), (x0, 1.0), (x0, 0.0)]],
            bbox: (x0, 0.0, x1, 1.0),
        }
    }

    #[test]
    fn a_county_line_is_placed_between_samples() {
        let lookup = Lookup::new(vec![
            County {
                name: "West County".into(),
                shape: square(-1.0, 0.0),
            },
            County {
                name: "East County".into(),
                shape: square(0.0, 1.0),
            },
        ]);
        // Due east along latitude 0.5, from lon -0.5 to 0.5.
        let line = LegLine::new(vec![(0.5, -0.5), (0.5, 0.5)]);
        let miles = line.length_m() / geo::M_PER_MI;
        let (runs, _, missed) = leg_runs(&line, miles, &lookup);
        assert_eq!(missed, 0);
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0], (0.0, 0));
        assert_eq!(runs[1].1, 1);
        assert!(
            (runs[1].0 - miles / 2.0).abs() <= 0.1,
            "{runs:?} on {miles}"
        );
    }
}
