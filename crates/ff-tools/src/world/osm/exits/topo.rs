//! The ramp graph and the walks over it: where an exit's ramps end, how
//! long they are, and what stands at the end.
//!
//! A port of the far-end, control and length halves of the deleted
//! `tools/build_interchanges_rampcontrols.py`. OSM tags controls that exist
//! and is silent where a ramp merges freely, so "no control" is never read
//! off a node; it is read off topology instead: an exit whose every ramp
//! chain rejoins a `highway=motorway` way has nothing at its far end but a
//! merge.

use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap, HashSet};

use super::read::{LinkWay, RawNetwork};
use crate::world::osm::geo::haversine_m;

/// Visited nodes per walk; a real ramp complex is far smaller.
pub const WALK_CAP: usize = 600;

/// The road classes a ramp can END at mid-link: the public street network
/// the facility chains are routed over, less `service`. A ramp that ends on
/// a service way still ends there (a dead end stops the walk whatever meets
/// it), so a frontage road drawn as service keeps its terminal.
pub const PUBLIC_CROSSROAD_HIGHWAYS: &[&str] = &[
    "trunk",
    "trunk_link",
    "primary",
    "primary_link",
    "secondary",
    "secondary_link",
    "tertiary",
    "tertiary_link",
    "unclassified",
    "residential",
    "living_street",
];

/// READ from OSM `access=*`: a way closed to a truck that has not been let in.
pub const CLOSED_ACCESS: &[&str] = &[
    "private",
    "no",
    "military",
    "permit",
    "residents",
    "employees",
    "emergency",
    "agricultural",
    "forestry",
];

/// Whether a way meeting a ramp mid-link is a public road it can end at.
pub fn is_public_crossroad(highway: &str, access: &str) -> bool {
    PUBLIC_CROSSROAD_HIGHWAYS.contains(&highway)
        && !CLOSED_ACCESS.contains(&access.trim().to_lowercase().as_str())
}

/// An OSM advisory speed in mph, or None. A bare number and the metric
/// suffixes are km/h (the strict subset the game's loader accepts); kept
/// only between 5 and 100 mph.
pub fn parse_advisory_mph(value: &str) -> Option<f64> {
    let text = value.trim().to_lowercase();
    if text.is_empty() {
        return None;
    }
    let mut factor = 0.621_371_192;
    let mut number = text.as_str();
    for (suffix, unit) in [
        ("mph", 1.0),
        ("km/h", 0.621_371_192),
        ("kmh", 0.621_371_192),
        ("kph", 0.621_371_192),
    ] {
        if let Some(head) = text.strip_suffix(suffix) {
            number = head.trim();
            factor = unit;
            break;
        }
    }
    let speed = number.parse::<f64>().ok()? * factor;
    (speed.is_finite() && (5.0..=100.0).contains(&speed)).then_some(speed)
}

/// A link way's `oneway` tag. Untagged is drawn-forward: `oneway` is not
/// implied on `motorway_link`, but ramps are overwhelmingly drawn in travel
/// direction, and the report counts how much rests on that.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Oneway {
    #[default]
    Untagged,
    Forward,
    Reverse,
    Both,
}

impl Oneway {
    pub fn parse(tag: &str) -> Self {
        match tag.trim().to_lowercase().as_str() {
            "yes" | "true" | "1" => Self::Forward,
            "-1" | "reverse" => Self::Reverse,
            "no" | "false" | "0" => Self::Both,
            _ => Self::Untagged,
        }
    }
}

/// One directed ramp edge: the next node, the way it is on, and whether it
/// runs along that way's node order.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Edge {
    pub to: i64,
    pub way: u32,
    pub along: bool,
}

/// Grid bucket in degrees. Searches here are 1.4 km at most, which a cell
/// and its eight neighbours cover everywhere in the lower 48.
const GRID_DEG: f64 = 0.02;

type Cells<T> = HashMap<(i64, i64), Vec<(f64, f64, T)>>;

/// Points bucketed by grid cell, for radius searches.
#[derive(Debug, Default)]
pub struct Grid<T> {
    cells: Cells<T>,
}

impl<T: Clone> Grid<T> {
    pub fn new(points: impl IntoIterator<Item = (f64, f64, T)>) -> Self {
        let mut cells: Cells<T> = HashMap::new();
        for (lat, lon, payload) in points {
            cells
                .entry(Self::key(lat, lon))
                .or_default()
                .push((lat, lon, payload));
        }
        Self { cells }
    }

    fn key(lat: f64, lon: f64) -> (i64, i64) {
        (
            (lat / GRID_DEG).floor() as i64,
            (lon / GRID_DEG).floor() as i64,
        )
    }

    /// Every payload within `radius_m`, with its distance, nearest first.
    pub fn near(&self, lat: f64, lon: f64, radius_m: f64) -> Vec<(f64, T)> {
        let (cy, cx) = Self::key(lat, lon);
        let mut found = Vec::new();
        for dy in -1..=1 {
            for dx in -1..=1 {
                for (plat, plon, payload) in
                    self.cells.get(&(cy + dy, cx + dx)).into_iter().flatten()
                {
                    let d = haversine_m(lat, lon, *plat, *plon);
                    if d <= radius_m {
                        found.push((d, payload.clone()));
                    }
                }
            }
        }
        found.sort_by(|a, b| a.0.total_cmp(&b.0));
        found
    }
}

/// The walk-ready ramp network.
pub struct Topo {
    pub links: Vec<LinkWay>,
    pub out: HashMap<i64, Vec<Edge>>,
    pub mainline: HashSet<i64>,
    pub trunk: HashSet<i64>,
    pub crossroad: HashSet<i64>,
    pub roundabout: HashSet<i64>,
    pub toll: HashSet<i64>,
    pub locs: HashMap<i64, (f64, f64)>,
    /// Gores: ramp nodes on the mainline with a ramp edge leaving them. An
    /// on-ramp's merge is on the mainline too, but only edges arrive there.
    pub gores: Grid<i64>,
    /// Every signal, stop and give-way node.
    pub controls: Grid<&'static str>,
    /// Signal and stop nodes that are members of a ramp way.
    pub link_controls: Grid<&'static str>,
    pub untagged_oneway: usize,
    pub gore_count: usize,
}

impl Topo {
    pub fn build(net: RawNetwork) -> Self {
        let mut out: HashMap<i64, Vec<Edge>> = HashMap::new();
        let mut untagged = 0;
        for (wi, link) in net.links.iter().enumerate() {
            let wi = wi as u32;
            let mut push = |a: i64, b: i64, along: bool| {
                out.entry(a).or_default().push(Edge {
                    to: b,
                    way: wi,
                    along,
                });
            };
            for pair in link.refs.windows(2) {
                let (a, b) = (pair[0], pair[1]);
                match link.oneway {
                    Oneway::Forward => push(a, b, true),
                    Oneway::Untagged => push(a, b, true),
                    Oneway::Reverse => push(b, a, false),
                    Oneway::Both => {
                        push(a, b, true);
                        push(b, a, false);
                    }
                }
            }
            if link.oneway == Oneway::Untagged {
                untagged += 1;
            }
        }
        let crossroad: HashSet<i64> = net.crossroad.difference(&net.mainline).copied().collect();
        let mut gore_ids: Vec<i64> = net
            .mainline
            .iter()
            .copied()
            .filter(|n| out.get(n).is_some_and(|e| !e.is_empty()))
            .collect();
        gore_ids.sort_unstable();
        let gore_count = gore_ids.len();
        let gores = Grid::new(
            gore_ids
                .iter()
                .filter_map(|id| net.locs.get(id).map(|(lat, lon)| (*lat, *lon, *id))),
        );
        let link_nodes: HashSet<i64> = net.links.iter().flat_map(|w| w.refs.clone()).collect();
        let controls = Grid::new(net.controls.iter().map(|c| (c.lat, c.lon, c.kind)));
        let link_controls = Grid::new(
            net.controls
                .iter()
                .filter(|c| c.kind != "give_way" && link_nodes.contains(&c.id))
                .map(|c| (c.lat, c.lon, c.kind)),
        );
        Self {
            links: net.links,
            out,
            mainline: net.mainline,
            trunk: net.trunk,
            crossroad,
            roundabout: net.roundabout,
            toll: net.toll,
            locs: net.locs,
            gores,
            controls,
            link_controls,
            untagged_oneway: untagged,
            gore_count,
        }
    }

    fn edges(&self, node: i64) -> &[Edge] {
        self.out.get(&node).map(Vec::as_slice).unwrap_or(&[])
    }

    /// Where every chain leaving `gore` ends: the terminal kinds, whether a
    /// toll booth was passed, and the off-motorway terminal nodes (where a
    /// terminal control stands). Kinds: `motorway` (rejoins a mainline node,
    /// a merge), `crossroad` (touches a public road: an at-grade meeting;
    /// the walk stops rather than sailing through a diamond's intersection
    /// onto the on-ramp), `trunk` (ends on a trunk way; trunks carry signals
    /// too, so not a proven merge), `road-end` (the ramp data ends off the
    /// motorway network) and `overrun` (the walk cap was hit).
    pub fn walk_far_ends(&self, gore: i64) -> Walk {
        let mut walk = Walk::default();
        let mut seen = HashSet::from([gore]);
        let mut frontier = vec![gore];
        let mut steps = 0;
        while let Some(node) = frontier.pop() {
            for edge in self.edges(node) {
                let next = edge.to;
                if !seen.insert(next) {
                    continue;
                }
                walk.ways.push((edge.way, edge.along));
                steps += 1;
                if steps > WALK_CAP {
                    walk.kinds.insert("overrun");
                    return walk;
                }
                if self.toll.contains(&next) {
                    walk.tolled = true;
                }
                if self.mainline.contains(&next) {
                    walk.kinds.insert("motorway");
                    continue;
                }
                let trunk = self.trunk.contains(&next);
                if self.crossroad.contains(&next) {
                    walk.kinds.insert(if trunk { "trunk" } else { "crossroad" });
                    walk.terminals.insert(next);
                    continue;
                }
                if self.edges(next).is_empty() {
                    walk.kinds.insert(if trunk { "trunk" } else { "road-end" });
                    walk.terminals.insert(next);
                    continue;
                }
                frontier.push(next);
            }
        }
        if walk.kinds.is_empty() {
            walk.kinds.insert("road-end");
            walk.terminals.insert(gore);
        }
        walk
    }

    /// `(metres along the ramp, surface terminal node)` from `gore` to
    /// where its ramp ends, or None. The node is None when the ramp ends in
    /// a merge. Dijkstra over the directed graph, stopping where
    /// [`Self::walk_far_ends`] stops; the nearest surface end wins over the
    /// nearest merge. Edges with an unknown end location are skipped.
    pub fn ramp_end(&self, gore: i64) -> Option<RampEnd> {
        let mut best: HashMap<i64, f64> = HashMap::from([(gore, 0.0)]);
        let mut via: HashMap<i64, (i64, u32, bool)> = HashMap::new();
        let mut heap = BinaryHeap::from([HeapItem(0.0, gore)]);
        let mut merge: Option<(f64, i64)> = None;
        let mut popped = 0;
        let path_to = |end: i64, via: &HashMap<i64, (i64, u32, bool)>| {
            let mut ways = Vec::new();
            let mut at = end;
            while let Some(&(prev, way, along)) = via.get(&at) {
                ways.push((way, along));
                at = prev;
            }
            ways.reverse();
            ways
        };
        while let Some(HeapItem(dist, node)) = heap.pop() {
            if popped > WALK_CAP {
                break;
            }
            if dist > best.get(&node).copied().unwrap_or(f64::INFINITY) {
                continue;
            }
            popped += 1;
            if node != gore {
                if self.mainline.contains(&node) {
                    merge.get_or_insert((dist, node));
                    continue;
                }
                if self.crossroad.contains(&node) || self.edges(node).is_empty() {
                    return Some(RampEnd {
                        metres: dist,
                        terminal: Some(node),
                        ways: path_to(node, &via),
                    });
                }
            }
            let Some(a) = self.locs.get(&node) else {
                continue;
            };
            for edge in self.edges(node) {
                let Some(b) = self.locs.get(&edge.to) else {
                    continue;
                };
                let nd = dist + haversine_m(a.0, a.1, b.0, b.1);
                if nd < best.get(&edge.to).copied().unwrap_or(f64::INFINITY) {
                    best.insert(edge.to, nd);
                    via.insert(edge.to, (node, edge.way, edge.along));
                    heap.push(HeapItem(nd, edge.to));
                }
            }
        }
        merge.map(|(d, node)| RampEnd {
            metres: d,
            terminal: None,
            ways: path_to(node, &via),
        })
    }

    /// Control kinds tagged within `radius_m` of any walked terminal node:
    /// `signal`, `stop`, `give_way`, and `roundabout` when the terminal sits
    /// on a roundabout way. The walk says where the ramp ends, the tag says
    /// what stands there.
    pub fn controls_at(&self, terminals: &HashSet<i64>, radius_m: f64) -> HashSet<&'static str> {
        let mut kinds = HashSet::new();
        for t in terminals {
            if let Some((lat, lon)) = self.locs.get(t) {
                kinds.extend(
                    self.controls
                        .near(*lat, *lon, radius_m)
                        .into_iter()
                        .map(|(_, k)| k),
                );
            }
            if self.roundabout.contains(t) {
                kinds.insert("roundabout");
            }
        }
        kinds
    }
}

/// Where a ramp ends, from [`Topo::ramp_end`].
#[derive(Debug, Clone, PartialEq)]
pub struct RampEnd {
    pub metres: f64,
    /// The public road node it ends at; None for a merge.
    pub terminal: Option<i64>,
    /// The ramp ways on the way there, with the travel direction.
    pub ways: Vec<(u32, bool)>,
}

/// What one walk from a gore found.
#[derive(Debug, Default)]
pub struct Walk {
    pub kinds: HashSet<&'static str>,
    pub tolled: bool,
    pub terminals: HashSet<i64>,
    /// The ramp ways walked, in walk order, with the travel direction.
    pub ways: Vec<(u32, bool)>,
}

impl Walk {
    /// One gore's verdict: `motorway`, `surface`, or None (no call). A toll
    /// plaza on an all-motorway chain vetoes free flow, since booths stop
    /// every truck, and an overrun proves nothing.
    pub fn verdict(&self) -> Option<&'static str> {
        if self.kinds.contains("overrun") {
            return None;
        }
        if self.kinds.len() == 1 && self.kinds.contains("motorway") {
            return (!self.tolled).then_some("motorway");
        }
        Some("surface")
    }
}

#[derive(PartialEq)]
struct HeapItem(f64, i64);

impl Eq for HeapItem {}

impl Ord for HeapItem {
    fn cmp(&self, other: &Self) -> Ordering {
        // A min-heap on distance, node id breaking ties so runs agree.
        other
            .0
            .total_cmp(&self.0)
            .then_with(|| other.1.cmp(&self.1))
    }
}

impl PartialOrd for HeapItem {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::osm::exits::read::Control;

    fn link(refs: &[i64]) -> LinkWay {
        LinkWay {
            refs: refs.to_vec(),
            oneway: Oneway::Forward,
            ..LinkWay::default()
        }
    }

    /// A diamond: mainline 1-2-3, off-ramp 2-10-11 to the crossroad at 11,
    /// on-ramp 11-12-3 back on. Node 0.001 degrees is about 111 m.
    fn diamond() -> RawNetwork {
        let mut net = RawNetwork {
            links: vec![link(&[2, 10, 11]), link(&[11, 12, 3])],
            ..RawNetwork::default()
        };
        net.mainline.extend([2, 3]);
        net.crossroad.insert(11);
        for (id, lat) in [(2, 0.0), (10, 0.002), (11, 0.004), (12, 0.006), (3, 0.008)] {
            net.locs.insert(id, (30.0 + lat, -97.0 + lat / 4.0));
        }
        net.controls.push(Control {
            id: 99,
            lat: 30.004,
            lon: -97.0 + 0.001,
            kind: "signal",
        });
        net
    }

    #[test]
    fn a_diamond_ends_at_its_crossroad_and_reads_the_light_there() {
        let topo = Topo::build(diamond());
        let walk = topo.walk_far_ends(2);
        assert_eq!(walk.verdict(), Some("surface"));
        assert_eq!(walk.terminals, HashSet::from([11]));
        assert!(topo.controls_at(&walk.terminals, 120.0).contains("signal"));
        let end = topo.ramp_end(2).unwrap();
        assert_eq!(end.terminal, Some(11));
        assert!((430.0..470.0).contains(&end.metres), "{}", end.metres);
        assert_eq!(end.ways, vec![(0, true), (0, true)]);
        // The on-ramp's merge has no edge leaving it: it is not a gore.
        assert_eq!(topo.gore_count, 1);
    }

    #[test]
    fn a_ramp_that_rejoins_a_motorway_is_a_merge_and_a_toll_vetoes_it() {
        let mut net = RawNetwork {
            links: vec![link(&[2, 10, 3])],
            ..RawNetwork::default()
        };
        net.mainline.extend([2, 3]);
        for (id, lat) in [(2, 0.0), (10, 0.002), (3, 0.004)] {
            net.locs.insert(id, (30.0 + lat, -97.0));
        }
        let topo = Topo::build(net);
        assert_eq!(topo.walk_far_ends(2).verdict(), Some("motorway"));
        assert_eq!(topo.ramp_end(2).unwrap().terminal, None);
        let mut net = RawNetwork {
            links: vec![link(&[2, 10, 3])],
            ..RawNetwork::default()
        };
        net.mainline.extend([2, 3]);
        net.toll.insert(10);
        assert_eq!(Topo::build(net).walk_far_ends(2).verdict(), None);
    }

    #[test]
    fn advisories_parse_like_the_loader() {
        assert_eq!(parse_advisory_mph("35 mph"), Some(35.0));
        let kmh = parse_advisory_mph("50").unwrap();
        assert!((kmh - 31.07).abs() < 0.01, "{kmh}");
        assert_eq!(parse_advisory_mph("200 mph"), None);
        assert_eq!(parse_advisory_mph("signals"), None);
    }

    #[test]
    fn service_and_private_ways_are_not_crossroads() {
        assert!(is_public_crossroad("secondary", ""));
        assert!(!is_public_crossroad("service", ""));
        assert!(!is_public_crossroad("residential", "private"));
    }
}
