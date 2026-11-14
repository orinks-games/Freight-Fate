//! `osm-landmarks`: the roadside features the ride-along names
//! (`corridor.landmarks`): parks, forests and wilderness the road enters,
//! rivers it crosses, mountain passes and museums it passes.
//!
//! A port of the deleted `tools/bake_landmarks.py` and its filter,
//! `tools/enrich_routes_landmarks.py`. What it reads: relations tagged
//! `boundary=national_park|protected_area`, ways tagged
//! `leisure=nature_reserve` and `waterway=river`, and nodes tagged
//! `tourism=museum` or `mountain_pass=yes`; unnamed ones are dropped. Where
//! each meets the leg, by geometry:
//!
//! * a zone (park, forest, wilderness, reserve) at the first road vertex
//!   inside its outer ring, spoken "Entering ...";
//! * a river where the road first crosses it (only names ending in
//!   "River", so creeks mistagged as rivers stay quiet);
//! * a pass or museum at its projection on the road, within 4 miles.
//!
//! Museums cluster in cities, so endpoint museums are dropped and at most
//! one mid-route museum is kept. Then the highest-ranked eight stay. The
//! categories other tools own (`highway_marker`, `billboard_sign`,
//! `village`) are kept as they are. Run `osm-villages` after this: it skips
//! names a landmark already speaks.

use std::collections::{BTreeMap, HashMap};
use std::sync::LazyLock;

use regex::Regex;

use super::features::{self, tag, Features, Select, Tags};
use super::{corridor_mut, leg_id, leg_miles, pbf_paths, round, Grid, Only, Route, Run};
use crate::cli::{Args, Spec};
use crate::pyjson::{Object, Value};
use crate::util::Result;

const USAGE: &str = "usage: osm-landmarks --pbf FILE [--pbf FILE ...] [--only from:to;from:to] \
                     [--per-leg N] [--extract-date YYYY-MM-DD] [--snapshot NAME] [--write]";

/// Landmark categories other tools own; a re-bake keeps them.
pub const CURATED: &[&str] = &["highway_marker", "billboard_sign", "village"];
pub const POINT_OFF_MI: f64 = 4.0;
const DEFAULT_PER_LEG: usize = 8;

const RAW_NAME_MARKERS: &[&str] = &[
    "osm",
    "amenity=",
    "highway=",
    "boundary=",
    "node/",
    "way/",
    "relation/",
    "wikidata=",
];

static ABBREVIATIONS: LazyLock<Vec<(Regex, &'static str)>> = LazyLock::new(|| {
    vec![
        (Regex::new(r"\bMt\.?(\s)").unwrap(), "Mount$1"),
        (Regex::new(r"\bMtn\.?\b").unwrap(), "Mountain"),
        (Regex::new(r"\bFt\.?(\s)").unwrap(), "Fort$1"),
        (Regex::new(r"\bNatl\.?\b").unwrap(), "National"),
    ]
});

/// A spoken-clean place name, or empty when it is unusable.
pub fn clean_name(raw: &str) -> String {
    let name = raw.replace('\n', " ");
    let name = name.split_whitespace().collect::<Vec<_>>().join(" ");
    if name.chars().count() < 2 {
        return String::new();
    }
    let low = name.to_lowercase();
    if RAW_NAME_MARKERS.iter().any(|m| low.contains(m)) {
        return String::new();
    }
    let mut name = name;
    for (pattern, with) in ABBREVIATIONS.iter() {
        name = pattern.replace_all(&name, *with).into_owned();
    }
    name.chars().take(80).collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Category {
    NationalPark,
    Wilderness,
    NationalForest,
    MountainPass,
    River,
    Museum,
    ProtectedArea,
}

impl Category {
    pub fn name(self) -> &'static str {
        match self {
            Category::NationalPark => "national_park",
            Category::Wilderness => "wilderness",
            Category::NationalForest => "national_forest",
            Category::MountainPass => "mountain_pass",
            Category::River => "river",
            Category::Museum => "museum",
            Category::ProtectedArea => "protected_area",
        }
    }

    /// Which features a capped leg keeps first.
    pub fn rank(self) -> u32 {
        match self {
            Category::NationalPark => 100,
            Category::Wilderness => 80,
            Category::NationalForest => 70,
            Category::MountainPass => 60,
            Category::River => 50,
            Category::Museum => 40,
            Category::ProtectedArea => 30,
        }
    }

    pub fn is_zone(self) -> bool {
        matches!(
            self,
            Category::NationalPark
                | Category::Wilderness
                | Category::NationalForest
                | Category::ProtectedArea
        )
    }
}

/// The category of an element, name-led for protected areas (the name's
/// suffix is more reliable than the boundary tag), tag-led for points.
pub fn classify(tags: &Tags) -> Option<(Category, String)> {
    let name = clean_name(tag(tags, "name").unwrap_or(""));
    if name.is_empty() {
        return None;
    }
    let low = name.to_lowercase();
    let boundary = tag(tags, "boundary").unwrap_or("");
    let category = if low.ends_with("national park") {
        Category::NationalPark
    } else if low.ends_with("national forest") || low.ends_with("national grassland") {
        Category::NationalForest
    } else if low.ends_with("wilderness") || matches!(tag(tags, "protect_class"), Some("1a" | "1b"))
    {
        Category::Wilderness
    } else if boundary == "national_park" {
        Category::NationalPark
    } else if boundary == "protected_area" || tag(tags, "leisure") == Some("nature_reserve") {
        Category::ProtectedArea
    } else if tag(tags, "tourism") == Some("museum") {
        Category::Museum
    } else if tag(tags, "mountain_pass") == Some("yes") {
        Category::MountainPass
    } else if tag(tags, "waterway") == Some("river") {
        Category::River
    } else {
        return None;
    };
    Some((category, name))
}

/// The cue line the game speaks.
pub fn spoken(category: Category, name: &str) -> String {
    let low = name.to_lowercase();
    if category.is_zone() {
        return format!("Entering {name}");
    }
    match category {
        Category::Museum if low.contains("museum") => format!("{name} ahead"),
        Category::Museum => format!("Museum ahead: {name}"),
        Category::River if low.contains("river") => format!("Crossing the {name}"),
        Category::River => format!("Crossing the {name} River"),
        Category::MountainPass => format!("Approaching {name}"),
        _ => format!("{name} ahead"),
    }
}

/// Join open member arcs that share end points into rings.
pub fn assemble_rings(arcs: Vec<Vec<(f64, f64)>>) -> Vec<Vec<(f64, f64)>> {
    let arcs: Vec<Vec<(f64, f64)>> = arcs.into_iter().filter(|a| a.len() >= 2).collect();
    let mut used = vec![false; arcs.len()];
    let mut rings = Vec::new();
    for s in 0..arcs.len() {
        if used[s] {
            continue;
        }
        let mut ring = arcs[s].clone();
        used[s] = true;
        let mut changed = true;
        while changed && ring.first() != ring.last() {
            changed = false;
            for (i, arc) in arcs.iter().enumerate() {
                if used[i] {
                    continue;
                }
                let (head, tail) = (ring[0], ring[ring.len() - 1]);
                if arc[0] == tail {
                    ring.extend_from_slice(&arc[1..]);
                } else if arc[arc.len() - 1] == tail {
                    ring.extend(arc[..arc.len() - 1].iter().rev());
                } else if arc[arc.len() - 1] == head {
                    let mut front = arc[..arc.len() - 1].to_vec();
                    front.extend_from_slice(&ring);
                    ring = front;
                } else if arc[0] == head {
                    let mut front: Vec<(f64, f64)> = arc[1..].iter().rev().copied().collect();
                    front.extend_from_slice(&ring);
                    ring = front;
                } else {
                    continue;
                }
                used[i] = true;
                changed = true;
            }
        }
        rings.push(ring);
    }
    rings
}

/// One polygon ring with its edges banded by latitude, so a point test
/// reads only the edges that span the point's latitude.
pub struct Ring {
    bounds: (f64, f64, f64, f64),
    bands: HashMap<i64, Vec<Edge>>,
}

const BAND_DEG: f64 = 0.01;

/// One ring edge, `(lat, lon)` to `(lat, lon)`.
type Edge = ((f64, f64), (f64, f64));

impl Ring {
    pub fn new(points: &[(f64, f64)]) -> Self {
        let mut bounds = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
        let mut bands: HashMap<i64, Vec<Edge>> = HashMap::new();
        let n = points.len();
        for i in 0..n {
            let (a, b) = (points[i], points[(i + n - 1) % n]);
            bounds = (
                bounds.0.min(a.0),
                bounds.1.min(a.1),
                bounds.2.max(a.0),
                bounds.3.max(a.1),
            );
            let lo = (a.0.min(b.0) / BAND_DEG).floor() as i64;
            let hi = (a.0.max(b.0) / BAND_DEG).floor() as i64;
            for band in lo..=hi {
                bands.entry(band).or_default().push((a, b));
            }
        }
        Self { bounds, bands }
    }

    pub fn bounds(&self) -> (f64, f64, f64, f64) {
        self.bounds
    }

    /// Ray-cast point in polygon, as the Python wrote it.
    pub fn contains(&self, lat: f64, lon: f64) -> bool {
        let b = self.bounds;
        if lat < b.0 || lat > b.2 || lon < b.1 || lon > b.3 {
            return false;
        }
        let band = (lat / BAND_DEG).floor() as i64;
        let mut inside = false;
        for &((yi, xi), (yj, xj)) in self.bands.get(&band).into_iter().flatten() {
            let dy = if yj - yi == 0.0 { 1e-12 } else { yj - yi };
            if ((yi > lat) != (yj > lat)) && lon < (xj - xi) * (lat - yi) / dy + xi {
                inside = !inside;
            }
        }
        inside
    }
}

enum Shape {
    Point(f64, f64),
    Line(Vec<(f64, f64)>),
    Zone(Vec<Ring>),
}

struct Feature {
    osm: String,
    category: Category,
    name: String,
    shape: Shape,
}

fn collect(found: Features) -> Vec<Feature> {
    let mut out = Vec::new();
    for r in found.relations {
        let Some((category, name)) = classify(&r.tags) else {
            continue;
        };
        if !category.is_zone() {
            continue;
        }
        let arcs = r
            .members
            .into_iter()
            .filter(|(role, _)| role == "outer" || role.is_empty())
            .map(|(_, c)| c)
            .collect();
        let rings: Vec<Ring> = assemble_rings(arcs).iter().map(|r| Ring::new(r)).collect();
        if !rings.is_empty() {
            out.push(Feature {
                osm: format!("relation {}", r.id),
                category,
                name,
                shape: Shape::Zone(rings),
            });
        }
    }
    for w in found.ways {
        let Some((category, name)) = classify(&w.tags) else {
            continue;
        };
        let shape = if category == Category::River {
            if !name.to_lowercase().trim_end().ends_with("river") {
                continue;
            }
            Shape::Line(w.coords)
        } else if category.is_zone() {
            Shape::Zone(vec![Ring::new(&w.coords)])
        } else {
            continue;
        };
        out.push(Feature {
            osm: format!("way {}", w.id),
            category,
            name,
            shape,
        });
    }
    for n in found.nodes {
        let Some((category, name)) = classify(&n.tags) else {
            continue;
        };
        if matches!(category, Category::Museum | Category::MountainPass) {
            out.push(Feature {
                osm: format!("node {}", n.id),
                category,
                name,
                shape: Shape::Point(n.lat, n.lon),
            });
        }
    }
    out
}

/// Where the road first crosses a line: `(at_mi, lat, lon)`.
fn first_crossing(route: &Route, line: &[(f64, f64)]) -> Option<(f64, f64, f64)> {
    let mut best: Option<(usize, f64, f64, f64)> = None; // route segment, t, lat, lon
    for j in 1..line.len() {
        let ((cy, cx), (dy, dx)) = (line[j - 1], line[j]);
        let (lo_y, hi_y) = (cy.min(dy), cy.max(dy));
        let (lo_x, hi_x) = (cx.min(dx), cx.max(dx));
        for i in 1..route.points.len() {
            let ((ay, ax), (by, bx)) = (route.points[i - 1], route.points[i]);
            if ay.max(by) < lo_y || ay.min(by) > hi_y || ax.max(bx) < lo_x || ax.min(bx) > hi_x {
                continue;
            }
            let (rx, ry) = (bx - ax, by - ay);
            let (sx, sy) = (dx - cx, dy - cy);
            let den = rx * sy - ry * sx;
            if den.abs() < 1e-12 {
                continue;
            }
            let (qx, qy) = (cx - ax, cy - ay);
            let t = (qx * sy - qy * sx) / den;
            let u = (qx * ry - qy * rx) / den;
            if (0.0..=1.0).contains(&t)
                && (0.0..=1.0).contains(&u)
                && best.is_none_or(|b| (i, t) < (b.0, b.1))
            {
                best = Some((i, t, ay + t * ry, ax + t * rx));
            }
        }
    }
    let (i, t, lat, lon) = best?;
    let at = route.cum_mi[i - 1] + t * (route.cum_mi[i] - route.cum_mi[i - 1]);
    Some((round(at, 1), lat, lon))
}

fn zone_entry(route: &Route, rings: &[Ring]) -> Option<(f64, f64, f64)> {
    route
        .points
        .iter()
        .enumerate()
        .find(|(_, p)| rings.iter().any(|r| r.contains(p.0, p.1)))
        .map(|(i, p)| (round(route.cum_mi[i], 1), p.0, p.1))
}

/// One placed landmark.
#[derive(Debug, Clone)]
pub struct Placed {
    pub name: String,
    pub category: Category,
    pub at_mi: f64,
    pub lat: f64,
    pub lon: f64,
    pub source: String,
}

impl Placed {
    pub fn row(&self) -> Value {
        let kind = if self.category.is_zone() {
            "zone"
        } else {
            "point"
        };
        Value::Object(crate::pyobj! {
            "name" => self.name.as_str(),
            "category" => self.category.name(),
            "kind" => kind,
            "at_mi" => self.at_mi,
            "lat" => round(self.lat, 5),
            "lon" => round(self.lon, 5),
            "spoken" => spoken(self.category, &self.name),
            "source" => self.source.as_str(),
        })
    }
}

/// Spatial lookup over every feature the extract carries.
pub struct Catalog {
    features: Vec<Feature>,
    grid: Grid,
}

impl Catalog {
    fn new(features: Vec<Feature>) -> Self {
        let mut grid = Grid::new(0.05);
        for (i, f) in features.iter().enumerate() {
            let i = i as u32;
            match &f.shape {
                Shape::Point(lat, lon) => grid.insert_point(*lat, *lon, i),
                Shape::Line(coords) => grid.insert_line(coords, i),
                Shape::Zone(rings) => {
                    for r in rings {
                        grid.insert_box(r.bounds(), i);
                    }
                }
            }
        }
        Self { features, grid }
    }

    pub fn len(&self) -> usize {
        self.features.len()
    }

    /// The leg's landmarks, ranked and capped, in mile order.
    pub fn bake(&self, route: &Route, per_leg: usize, note: &str) -> Vec<Placed> {
        let mut found: BTreeMap<String, Placed> = BTreeMap::new();
        for i in self.grid.along(&route.points, 2) {
            let f = &self.features[i as usize];
            let (placed, how) = match &f.shape {
                Shape::Point(lat, lon) => {
                    let (at, off, plat, plon) = route.project(*lat, *lon);
                    if off > POINT_OFF_MI {
                        continue;
                    }
                    ((round(at, 1), plat, plon), "nearest projection of")
                }
                Shape::Line(coords) => match first_crossing(route, coords) {
                    Some(c) => (c, "crossing of the road with"),
                    None => continue,
                },
                Shape::Zone(rings) => match zone_entry(route, rings) {
                    Some(z) => (z, "first road vertex inside"),
                    None => continue,
                },
            };
            let rec = Placed {
                name: f.name.clone(),
                category: f.category,
                at_mi: placed.0,
                lat: placed.1,
                lon: placed.2,
                source: format!(
                    "derived: name and category read from the OpenStreetMap tags of {} ({}); \
                     placed at the {how} it on the leg's archived route geometry, rescaled to \
                     leg miles. {note}",
                    f.osm, f.name
                ),
            };
            let better = match found.get(&f.name) {
                None => true,
                Some(prev) => {
                    rec.category.rank() > prev.category.rank()
                        || (rec.category.rank() == prev.category.rank() && rec.at_mi < prev.at_mi)
                }
            };
            if better {
                found.insert(f.name.clone(), rec);
            }
        }
        let miles = route.total_mi();
        let buf = 12f64.min(miles / 4.0);
        let mut museums: Vec<&Placed> = found
            .values()
            .filter(|r| r.category == Category::Museum && buf <= r.at_mi && r.at_mi <= miles - buf)
            .collect();
        museums.sort_by(|a, b| {
            (a.at_mi - miles / 2.0)
                .abs()
                .total_cmp(&(b.at_mi - miles / 2.0).abs())
        });
        museums.truncate(1);
        let mut pool: Vec<&Placed> = found
            .values()
            .filter(|r| r.category != Category::Museum)
            .chain(museums)
            .collect();
        pool.sort_by(|a, b| {
            b.category
                .rank()
                .cmp(&a.category.rank())
                .then(a.at_mi.total_cmp(&b.at_mi))
                .then(a.name.cmp(&b.name))
        });
        pool.truncate(per_leg);
        let mut out: Vec<Placed> = pool.into_iter().cloned().collect();
        out.sort_by(|a, b| a.at_mi.total_cmp(&b.at_mi).then(a.name.cmp(&b.name)));
        out
    }
}

pub fn read_catalog(pbfs: &[std::path::PathBuf]) -> Result<Catalog> {
    fn has<'a>(t: &[(&'a str, &'a str)], k: &str) -> Option<&'a str> {
        t.iter().find(|(tk, _)| *tk == k).map(|(_, v)| *v)
    }
    let node = |t: &[(&str, &str)]| {
        has(t, "name").is_some()
            && (has(t, "tourism") == Some("museum") || has(t, "mountain_pass") == Some("yes"))
    };
    let way = |t: &[(&str, &str)]| {
        has(t, "name").is_some()
            && (has(t, "waterway") == Some("river") || has(t, "leisure") == Some("nature_reserve"))
    };
    let relation = |t: &[(&str, &str)]| {
        has(t, "name").is_some()
            && matches!(has(t, "boundary"), Some("national_park" | "protected_area"))
    };
    let found = features::read(
        pbfs,
        &Select {
            node: Some(&node),
            way: Some(&way),
            relation: Some(&relation),
        },
    )?;
    Ok(Catalog::new(collect(found)))
}

/// Replace a leg's OpenStreetMap landmarks, keeping the curated ones.
pub fn merge(corridor: &mut Object, placed: &[Placed]) -> bool {
    let mut rows: Vec<Value> = corridor
        .get("landmarks")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|lm| {
            lm.get("category")
                .and_then(Value::as_str)
                .is_some_and(|c| CURATED.contains(&c))
        })
        .cloned()
        .collect();
    rows.extend(placed.iter().map(Placed::row));
    let at = |v: &Value| v.get("at_mi").and_then(Value::as_f64).unwrap_or(0.0);
    rows.sort_by(|a, b| at(a).total_cmp(&at(b)));
    let rows = Value::Array(rows);
    if corridor.get("landmarks") == Some(&rows) {
        return false;
    }
    corridor.insert("landmarks", rows);
    true
}

pub fn main(argv: &[String]) -> Result<i32> {
    let args = Args::parse(
        argv,
        &Spec {
            values: &["pbf", "only", "per-leg", "extract-date", "snapshot"],
            flags: &["write"],
        },
    )
    .map_err(|e| format!("{e}\n{USAGE}"))?;
    let pbfs = pbf_paths(&args, USAGE)?;
    let date = super::extract_date(&args, &pbfs)?;
    let snapshot = args.string("snapshot", "OpenStreetMap planet file");
    let per_leg: usize = args
        .number("per-leg", DEFAULT_PER_LEG)
        .map_err(|e| e.to_string())?;
    let only = Only::parse(&args);
    let mut run = Run::load()?;
    let catalog = read_catalog(&pbfs)?;
    println!("{} named landmark features in the extracts", catalog.len());
    let note = format!("OpenStreetMap, {snapshot} of {date}: https://www.openstreetmap.org/.");
    let archive = std::mem::take(&mut run.archive);
    let (mut changed, mut same, mut placed_n, mut no_archive) = (0, 0, 0, 0);
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
        let placed = catalog.bake(&route, per_leg, &note);
        placed_n += placed.len();
        if only.is_some() {
            let list: Vec<String> = placed
                .iter()
                .map(|p| format!("{} ({})", spoken(p.category, &p.name), p.at_mi))
                .collect();
            println!("  {id}: {}", list.join("; "));
        }
        if merge(corridor_mut(leg)?, &placed) {
            changed += 1;
        } else {
            same += 1;
        }
    }
    run.archive = archive;
    println!(
        "{placed_n} landmarks placed; {changed} legs changed, {same} the same, {no_archive} with \
         no archived road kept."
    );
    run.finish(args.flag("write"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::osm::geo::LegLine;

    fn tags(pairs: &[(&str, &str)]) -> Tags {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect()
    }

    #[test]
    fn names_are_cleaned_for_speech() {
        assert_eq!(
            clean_name("Mt. Hood  National\nForest"),
            "Mount Hood National Forest"
        );
        assert_eq!(clean_name("Ft Worth"), "Fort Worth");
        assert_eq!(clean_name("way/123"), "");
        assert_eq!(clean_name("x"), "");
    }

    #[test]
    fn the_name_decides_a_protected_area() {
        let forest = tags(&[
            ("boundary", "protected_area"),
            ("name", "Coconino National Forest"),
        ]);
        assert_eq!(classify(&forest).unwrap().0, Category::NationalForest);
        let river = tags(&[("waterway", "river"), ("name", "Flint River")]);
        let (c, n) = classify(&river).unwrap();
        assert_eq!(spoken(c, &n), "Crossing the Flint River");
        let museum = tags(&[("tourism", "museum"), ("name", "Route 66 Museum")]);
        let (c, n) = classify(&museum).unwrap();
        assert_eq!(spoken(c, &n), "Route 66 Museum ahead");
    }

    #[test]
    fn rings_close_from_open_arcs_and_contain_their_middle() {
        let arcs = vec![
            vec![(0.0, 0.0), (0.0, 1.0)],
            vec![(1.0, 1.0), (0.0, 1.0)],
            vec![(1.0, 1.0), (1.0, 0.0), (0.0, 0.0)],
        ];
        let rings = assemble_rings(arcs);
        assert_eq!(rings.len(), 1);
        assert_eq!(rings[0].first(), rings[0].last());
        let ring = Ring::new(&rings[0]);
        assert!(ring.contains(0.5, 0.5));
        assert!(!ring.contains(1.5, 0.5));
    }

    #[test]
    fn a_road_crossing_a_river_and_entering_a_forest_places_both() {
        let line = LegLine::new((0..=40).map(|i| (30.0 + 0.025 * i as f64, -97.0)).collect());
        let route = Route::new(&line, 69.0);
        let features = vec![
            Feature {
                osm: "way 1".into(),
                category: Category::River,
                name: "Brazos River".into(),
                shape: Shape::Line(vec![(30.2, -97.1), (30.2, -96.9)]),
            },
            Feature {
                osm: "relation 2".into(),
                category: Category::NationalForest,
                name: "Sam Houston National Forest".into(),
                shape: Shape::Zone(vec![Ring::new(&[
                    (30.5, -97.2),
                    (30.5, -96.8),
                    (30.7, -96.8),
                    (30.7, -97.2),
                ])]),
            },
        ];
        let placed = Catalog::new(features).bake(&route, 8, "");
        assert_eq!(placed.len(), 2);
        assert_eq!(placed[0].name, "Brazos River");
        assert!((placed[0].at_mi - 13.8).abs() < 0.2, "{}", placed[0].at_mi);
        assert_eq!(placed[1].category, Category::NationalForest);
        assert!((placed[1].at_mi - 34.5).abs() < 0.2, "{}", placed[1].at_mi);
    }
}
