//! Which exits a leg passes: its exit junctions, read off the extract and
//! placed on the leg's archived road.
//!
//! A port of the discovery half of the deleted `tools/build_interchanges.py`
//! (its local-extract path). An exit is the `highway=motorway_junction`
//! nodes of one exit number within 200 m of the archived line, one per
//! carriageway, collapsed into one record at their mean mile. What changed
//! from the Python: a junction must lie on a motorway or trunk way running
//! along the leg (a crossing freeway's junctions near the crossing are its
//! exits, not the leg's), and destinations come from the exit's own ramps,
//! walked from the gore, instead of any signed ramp within 350 m, which took
//! the mainline entrance ramps' signs too.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::LazyLock;

use regex::Regex;

use super::read::{Junction, Neighbours};
use super::topo::Topo;
use crate::world::osm::geo::{haversine_m, LegLine, M_PER_MI};

/// A junction this close to the archived line is on the leg's road.
pub const CORRIDOR_M: f64 = 200.0;
/// Nodes of one exit number farther apart than this are two exits (a leg
/// across a state line can pass two exits with one number).
pub const MIN_EXIT_SPACING_MI: f64 = 2.0;
/// Control cities per exit, for speech brevity.
pub const MAX_DESTINATIONS: usize = 3;
/// A gore this close to an exit junction belongs to it, for its signs.
pub const SIGN_GORE_M: f64 = 350.0;
/// A junction's road runs along the leg when its heading is within this of
/// the line's (either carriageway). Assumed: half a right angle, between a
/// parallel carriageway (0) and a crossing road (90).
pub const ALONG_MAX_DEG: f64 = 45.0;

const RAW_MARKERS: &[&str] = &["node/", "way/", "relation/", "amenity=", "highway="];
/// Lane-control and vehicle-class words that ride in OSM destination tags.
/// A destination made up only of these ("Cars Only", "Trucks - Buses") is
/// lane signage, not a place to head toward.
const VEHICLE_CONTROL_WORDS: &[&str] = &[
    "cars", "car", "trucks", "truck", "buses", "bus", "vehicles", "only", "no", "hov", "express",
    "local", "left", "right", "lane", "lanes", "exit", "toll", "ezpass",
];

static WORDS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[a-z]+").unwrap());

/// A sayable place from a tag value, or empty.
pub fn clean_place(value: &str) -> String {
    let text = value.split_whitespace().collect::<Vec<_>>().join(" ");
    let lowered = text.to_lowercase();
    if text.is_empty() || RAW_MARKERS.iter().any(|m| lowered.contains(m)) {
        return String::new();
    }
    let words: Vec<&str> = WORDS
        .find_iter(&lowered)
        .map(|m| m.as_str())
        .filter(|w| *w != "and")
        .collect();
    if !words.is_empty() && words.iter().all(|w| VEHICLE_CONTROL_WORDS.contains(w)) {
        return String::new();
    }
    text
}

/// The places in a `destination` tag, in order, once each.
pub fn split_destinations(raw: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for piece in raw.split(';') {
        let place = clean_place(piece);
        if !place.is_empty() && !out.contains(&place) {
            out.push(place);
        }
    }
    out
}

/// One leg's archived line, ready for nearest-point lookups.
pub struct Snapper<'a> {
    line: &'a LegLine,
    cells: HashMap<(i64, i64), Vec<usize>>,
    /// Leg miles per metre of line.
    scale: f64,
}

/// Snap cell in degrees; a cell and its neighbours cover 1 km and more.
const SNAP_DEG: f64 = 0.01;

fn snap_cell(lat: f64, lon: f64) -> (i64, i64) {
    (
        (lat / SNAP_DEG).floor() as i64,
        (lon / SNAP_DEG).floor() as i64,
    )
}

/// Where a point falls on the line.
#[derive(Debug, Clone, Copy)]
pub struct Snap {
    /// Miles from the leg's start, on the leg's own mileage.
    pub at_mi: f64,
    pub dist_m: f64,
    /// The line's heading there, as a local east/north unit vector.
    pub heading: (f64, f64),
}

impl<'a> Snapper<'a> {
    pub fn new(line: &'a LegLine, leg_miles: f64) -> Self {
        let mut cells: HashMap<(i64, i64), Vec<usize>> = HashMap::new();
        for (i, pair) in line.points.windows(2).enumerate() {
            let (a, b) = (
                snap_cell(pair[0].0, pair[0].1),
                snap_cell(pair[1].0, pair[1].1),
            );
            for y in a.0.min(b.0) - 1..=a.0.max(b.0) + 1 {
                for x in a.1.min(b.1) - 1..=a.1.max(b.1) + 1 {
                    cells.entry((y, x)).or_default().push(i);
                }
            }
        }
        let length = line.length_m();
        let scale = if length > 0.0 && leg_miles > 0.0 {
            leg_miles / length
        } else {
            1.0 / M_PER_MI
        };
        Self { line, cells, scale }
    }

    /// The nearest point on the line within about a kilometre, or None.
    pub fn snap(&self, lat: f64, lon: f64) -> Option<Snap> {
        let kx = lat.to_radians().cos();
        let mut best: Option<Snap> = None;
        for &i in self.cells.get(&snap_cell(lat, lon)).into_iter().flatten() {
            let (a, b) = (self.line.points[i], self.line.points[i + 1]);
            let (ax, ay) = ((a.1 - lon) * kx, a.0 - lat);
            let (dx, dy) = ((b.1 - a.1) * kx, b.0 - a.0);
            let seg2 = dx * dx + dy * dy;
            let t = if seg2 == 0.0 {
                0.0
            } else {
                (-(ax * dx + ay * dy) / seg2).clamp(0.0, 1.0)
            };
            let p = (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t);
            let d = haversine_m(lat, lon, p.0, p.1);
            if best.is_none_or(|s| d < s.dist_m) {
                let along = self.line.cum_m[i] + (self.line.cum_m[i + 1] - self.line.cum_m[i]) * t;
                let norm = seg2.sqrt();
                let heading = if norm > 0.0 {
                    (dx / norm, dy / norm)
                } else {
                    (0.0, 0.0)
                };
                best = Some(Snap {
                    at_mi: along * self.scale,
                    dist_m: d,
                    heading,
                });
            }
        }
        best
    }

    /// The grid cells (in [`Grid`] units) the line passes through.
    pub fn cells(&self) -> impl Iterator<Item = &(i64, i64)> {
        self.cells.keys()
    }
}

/// A local east/north unit vector from `a` to `b`.
pub fn heading(a: (f64, f64), b: (f64, f64)) -> Option<(f64, f64)> {
    let kx = a.0.to_radians().cos();
    let (dx, dy) = ((b.1 - a.1) * kx, b.0 - a.0);
    let norm = dx.hypot(dy);
    (norm > 0.0).then(|| (dx / norm, dy / norm))
}

/// Junctions bucketed in [`SNAP_DEG`] cells.
pub struct JunctionIndex<'a> {
    pub junctions: &'a [Junction],
    cells: HashMap<(i64, i64), Vec<usize>>,
}

impl<'a> JunctionIndex<'a> {
    pub fn new(junctions: &'a [Junction]) -> Self {
        let mut cells: HashMap<(i64, i64), Vec<usize>> = HashMap::new();
        for (i, j) in junctions.iter().enumerate() {
            cells.entry(snap_cell(j.lat, j.lon)).or_default().push(i);
        }
        Self { junctions, cells }
    }

    /// The junctions in any cell the line's index covers, in id order.
    pub fn along(&self, snapper: &Snapper) -> Vec<&'a Junction> {
        let mut found: Vec<usize> = snapper
            .cells()
            .flat_map(|c| self.cells.get(c).into_iter().flatten().copied())
            .collect();
        found.sort_unstable();
        found.dedup();
        found.into_iter().map(|i| &self.junctions[i]).collect()
    }
}

/// One exit before its ramps are read: its junction nodes and its mile.
#[derive(Debug, Clone)]
pub struct ExitNodes {
    pub at_mi: f64,
    pub exit_ref: String,
    pub name: String,
    /// `(node id, lat, lon)` of each carriageway's junction.
    pub nodes: Vec<(i64, f64, f64)>,
}

/// What the discovery pass dropped, for the report.
#[derive(Debug, Default, Clone, Copy)]
pub struct DiscoverCounts {
    pub near_line: usize,
    /// On no motorway or trunk way (a collector or ramp junction).
    pub off_mainline: usize,
    pub crossing_road: usize,
    pub at_leg_end: usize,
}

/// The exits on one leg, ordered by mile, before spacing.
pub fn exit_nodes(
    junctions: &[&Junction],
    neighbours: &Neighbours,
    locs: &HashMap<i64, (f64, f64)>,
    snapper: &Snapper,
    leg_miles: f64,
    counts: &mut DiscoverCounts,
) -> Vec<ExitNodes> {
    let cos_max = ALONG_MAX_DEG.to_radians().cos();
    let mut by_key: BTreeMap<String, Vec<(f64, &Junction)>> = BTreeMap::new();
    for j in junctions {
        let Some(snap) = snapper.snap(j.lat, j.lon) else {
            continue;
        };
        if snap.dist_m > CORRIDOR_M {
            continue;
        }
        counts.near_line += 1;
        let headings: Vec<(f64, f64)> = neighbours
            .get(&j.id)
            .into_iter()
            .flatten()
            .filter_map(|(before, after)| {
                let a = before
                    .and_then(|n| locs.get(&n))
                    .copied()
                    .unwrap_or((j.lat, j.lon));
                let b = after
                    .and_then(|n| locs.get(&n))
                    .copied()
                    .unwrap_or((j.lat, j.lon));
                heading(a, b)
            })
            .collect();
        if headings.is_empty() {
            counts.off_mainline += 1;
            continue;
        }
        let along = headings
            .iter()
            .any(|h| (h.0 * snap.heading.0 + h.1 * snap.heading.1).abs() >= cos_max);
        if !along {
            counts.crossing_road += 1;
            continue;
        }
        if !(1.0 < snap.at_mi && snap.at_mi < leg_miles - 1.0) {
            counts.at_leg_end += 1;
            continue;
        }
        let exit_ref: String = j.exit_ref.split_whitespace().collect();
        let key = if exit_ref.is_empty() {
            format!("mi:{}", (snap.at_mi / 0.5).round_ties_even() as i64)
        } else {
            format!("ref:{exit_ref}")
        };
        by_key.entry(key).or_default().push((snap.at_mi, j));
    }
    let mut exits = Vec::new();
    for mut same in by_key.into_values() {
        same.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.id.cmp(&b.1.id)));
        let mut groups: Vec<Vec<(f64, &Junction)>> = vec![vec![same[0]]];
        for item in same.into_iter().skip(1) {
            let last = groups.last().and_then(|g| g.last()).map_or(item.0, |l| l.0);
            if item.0 - last > MIN_EXIT_SPACING_MI {
                groups.push(Vec::new());
            }
            if let Some(g) = groups.last_mut() {
                g.push(item);
            }
        }
        for group in groups {
            let at_mi = group.iter().map(|g| g.0).sum::<f64>() / group.len() as f64;
            let exit_ref = group
                .iter()
                .map(|g| g.1.exit_ref.split_whitespace().collect::<String>())
                .find(|r| !r.is_empty())
                .unwrap_or_default();
            let name = group
                .iter()
                .map(|g| clean_place(&g.1.name))
                .find(|n| !n.is_empty())
                .unwrap_or_default();
            exits.push(ExitNodes {
                at_mi,
                exit_ref,
                name,
                nodes: group.iter().map(|g| (g.1.id, g.1.lat, g.1.lon)).collect(),
            });
        }
    }
    exits.sort_by(|a, b| a.at_mi.total_cmp(&b.at_mi));
    exits
}

/// Gores within `radius_m` of any of an exit's junctions, with the distance
/// to the nearest one, nearest first, each once.
pub fn gores_near(topo: &Topo, exit: &ExitNodes, radius_m: f64) -> Vec<(f64, i64)> {
    let mut best: BTreeMap<i64, f64> = BTreeMap::new();
    for (_, lat, lon) in &exit.nodes {
        for (d, gore) in topo.gores.near(*lat, *lon, radius_m) {
            let slot = best.entry(gore).or_insert(f64::INFINITY);
            *slot = slot.min(d);
        }
    }
    let mut out: Vec<(f64, i64)> = best.into_iter().map(|(g, d)| (d, g)).collect();
    out.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
    out
}

/// The exit's signs: places from the `destination` tags of the ramps its
/// own gores lead onto, and the first `destination:ref`.
pub fn signs(topo: &Topo, exit: &ExitNodes) -> (Vec<String>, String) {
    let mut dests: Vec<String> = Vec::new();
    let mut via = String::new();
    let mut seen_ways: HashSet<u32> = HashSet::new();
    for (_, gore) in gores_near(topo, exit, SIGN_GORE_M) {
        let walk = topo.walk_far_ends(gore);
        for (way, _) in walk.ways {
            if !seen_ways.insert(way) {
                continue;
            }
            let link = &topo.links[way as usize];
            for place in split_destinations(&link.destination) {
                if !dests.contains(&place) {
                    dests.push(place);
                }
            }
            if via.is_empty() && !link.destination_ref.is_empty() {
                via = link.destination_ref.clone();
            }
        }
    }
    dests.truncate(MAX_DESTINATIONS);
    (dests, via)
}

/// Greedily keep the richest exits (an exit number, then the most signs),
/// dropping any within [`MIN_EXIT_SPACING_MI`] of one already kept, so the
/// spoken cues do not crowd each other. `richness` is per exit.
pub fn space_out<T>(
    exits: Vec<T>,
    at_mi: impl Fn(&T) -> f64,
    richness: impl Fn(&T) -> usize,
) -> Vec<T> {
    let mut order: Vec<usize> = (0..exits.len()).collect();
    order.sort_by(|&a, &b| {
        richness(&exits[b])
            .cmp(&richness(&exits[a]))
            .then(at_mi(&exits[a]).total_cmp(&at_mi(&exits[b])))
    });
    let mut keep: Vec<usize> = Vec::new();
    for i in order {
        let mi = at_mi(&exits[i]);
        if keep
            .iter()
            .all(|&k| (mi - at_mi(&exits[k])).abs() >= MIN_EXIT_SPACING_MI)
        {
            keep.push(i);
        }
    }
    keep.sort_by(|&a, &b| at_mi(&exits[a]).total_cmp(&at_mi(&exits[b])));
    let keep: HashSet<usize> = keep.into_iter().collect();
    exits
        .into_iter()
        .enumerate()
        .filter(|(i, _)| keep.contains(i))
        .map(|(_, e)| e)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lane_signs_and_raw_tags_are_not_places() {
        assert_eq!(clean_place("  Fort   Worth "), "Fort Worth");
        assert_eq!(clean_place("Trucks - Buses"), "");
        assert_eq!(clean_place("Buses And Cars Only"), "");
        assert_eq!(clean_place("highway=primary"), "");
        assert_eq!(
            split_destinations("Dallas;Fort Worth;Dallas;Cars Only"),
            vec!["Dallas", "Fort Worth"]
        );
    }

    fn junction(id: i64, lat: f64, exit_ref: &str) -> Junction {
        Junction {
            id,
            lat,
            lon: -97.0001,
            exit_ref: exit_ref.into(),
            name: String::new(),
        }
    }

    #[test]
    fn two_carriageways_make_one_exit_and_a_crossing_road_none() {
        // A north-south leg, 100 miles over about 69 miles of line.
        let line = LegLine::new(vec![(30.0, -97.0), (31.0, -97.0)]);
        let snapper = Snapper::new(&line, 100.0);
        let js = [
            junction(1, 30.5, "50 A"),
            junction(2, 30.501, "50A"),
            junction(3, 30.7, "60"),
        ];
        let mut neighbours = HashMap::new();
        let mut locs = HashMap::new();
        // 1 and 2 sit on north-south carriageways, 3 on an east-west road.
        neighbours.insert(1, vec![(Some(11), Some(12))]);
        neighbours.insert(2, vec![(Some(22), Some(21))]);
        neighbours.insert(3, vec![(Some(31), Some(32))]);
        locs.insert(11, (30.49, -97.0001));
        locs.insert(12, (30.51, -97.0001));
        locs.insert(21, (30.49, -97.0001));
        locs.insert(22, (30.51, -97.0001));
        locs.insert(31, (30.7, -97.01));
        locs.insert(32, (30.7, -96.99));
        let refs: Vec<&Junction> = js.iter().collect();
        let mut counts = DiscoverCounts::default();
        let exits = exit_nodes(&refs, &neighbours, &locs, &snapper, 100.0, &mut counts);
        assert_eq!(exits.len(), 1);
        assert_eq!(exits[0].exit_ref, "50A");
        assert_eq!(exits[0].nodes.len(), 2);
        assert!((exits[0].at_mi - 50.05).abs() < 0.01, "{}", exits[0].at_mi);
        assert_eq!(counts.crossing_road, 1);
    }

    #[test]
    fn the_richest_exit_keeps_its_place() {
        let exits = vec![(10.0, 1usize), (11.0, 3), (14.0, 0)];
        let kept = space_out(exits, |e| e.0, |e| e.1);
        assert_eq!(kept, vec![(11.0, 3), (14.0, 0)]);
    }
}
