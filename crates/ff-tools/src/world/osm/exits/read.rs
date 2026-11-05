//! Read the motorway ramp network out of an `.osm.pbf` extract.
//!
//! Four passes over the file, so no location index of the whole country is
//! held: the tagged nodes (exit junctions, traffic controls, toll booths),
//! the motorway and ramp ways, the public roads that touch a ramp, and last
//! the locations of the ramp nodes. Blocks decode in parallel. The planet
//! cut can be pre-filtered to highways first (`osmium tags-filter FILE
//! w/highway n/highway n/barrier=toll_booth`), which makes every pass
//! several times faster and changes nothing read here.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::time::Instant;

use osmpbf::{BlobDecode, BlobReader, PrimitiveBlock};
use rayon::iter::{ParallelBridge, ParallelIterator};

use super::topo::{is_public_crossroad, parse_advisory_mph, Oneway};
use crate::util::Result;

/// A `highway=motorway_junction` node: where an exit leaves the motorway.
#[derive(Debug, Clone, PartialEq)]
pub struct Junction {
    pub id: i64,
    pub lat: f64,
    pub lon: f64,
    /// The `ref` tag, the signed exit number ("31B"), trimmed.
    pub exit_ref: String,
    pub name: String,
}

/// One `highway=motorway_link` way.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LinkWay {
    pub refs: Vec<i64>,
    pub oneway: Oneway,
    pub destination: String,
    pub destination_ref: String,
    /// `maxspeed:advisory` resolved for travel along the way's node order,
    /// and against it.
    pub advisory_along: Option<f64>,
    pub advisory_against: Option<f64>,
}

/// A traffic control node. `kind` is `signal`, `stop` or `give_way`.
#[derive(Debug, Clone, PartialEq)]
pub struct Control {
    pub id: i64,
    pub lat: f64,
    pub lon: f64,
    pub kind: &'static str,
}

/// Per junction node, the node before and after it on each road through it.
pub type Neighbours = HashMap<i64, Vec<(Option<i64>, Option<i64>)>>;

/// Everything the exit bake reads from OpenStreetMap.
#[derive(Debug, Default)]
pub struct RawNetwork {
    pub junctions: Vec<Junction>,
    pub links: Vec<LinkWay>,
    /// Ramp nodes that are also on a `highway=motorway` way.
    pub mainline: HashSet<i64>,
    /// Ramp nodes that are also on a `highway=trunk` way.
    pub trunk: HashSet<i64>,
    /// Ramp nodes shared with a public road (see [`is_public_crossroad`]).
    pub crossroad: HashSet<i64>,
    /// Crossroad nodes on a roundabout.
    pub roundabout: HashSet<i64>,
    pub toll: HashSet<i64>,
    pub controls: Vec<Control>,
    /// For each junction node, the motorway and trunk ways through it as the
    /// node before and after it (either may be absent at a way end).
    pub junction_neighbours: Neighbours,
    pub locs: HashMap<i64, (f64, f64)>,
}

/// Run `f` over every data block of `path` in parallel and gather the results.
fn each_block<T: Send>(
    path: &Path,
    f: impl Fn(&PrimitiveBlock) -> Vec<T> + Sync + Send,
) -> Result<Vec<T>> {
    let reader =
        BlobReader::from_path(path).map_err(|e| format!("cannot open {}: {e}", path.display()))?;
    reader
        .par_bridge()
        .map(|blob| -> Result<Vec<T>> {
            let blob = blob.map_err(|e| format!("{}: {e}", path.display()))?;
            match blob.decode() {
                Ok(BlobDecode::OsmData(block)) => Ok(f(&block)),
                Ok(_) => Ok(Vec::new()),
                Err(e) => Err(format!("{}: {e}", path.display())),
            }
        })
        .try_reduce(Vec::new, |mut a, mut b| {
            a.append(&mut b);
            Ok(a)
        })
}

enum TaggedNode {
    Junction(Junction),
    Control(Control),
    Toll(i64),
}

fn tagged_node<'a>(
    id: i64,
    lat: f64,
    lon: f64,
    tags: impl Iterator<Item = (&'a str, &'a str)>,
) -> Option<TaggedNode> {
    let (mut highway, mut barrier, mut signals, mut exit_ref, mut name) = ("", "", "", "", "");
    for (k, v) in tags {
        match k {
            "highway" => highway = v,
            "barrier" => barrier = v,
            "traffic_signals" => signals = v,
            "ref" => exit_ref = v,
            "name" => name = v,
            _ => {}
        }
    }
    if barrier == "toll_booth" {
        return Some(TaggedNode::Toll(id));
    }
    let kind = match highway {
        "motorway_junction" => {
            return Some(TaggedNode::Junction(Junction {
                id,
                lat,
                lon,
                exit_ref: exit_ref.trim().to_string(),
                name: name.to_string(),
            }))
        }
        "traffic_signals" => "signal",
        "stop" => "stop",
        "give_way" => "give_way",
        _ => return None,
    };
    // A ramp meter meters the on-ramp flow; it is not the terminal's light.
    if signals == "ramp_meter" {
        return None;
    }
    Some(TaggedNode::Control(Control { id, lat, lon, kind }))
}

fn tagged_nodes_of(block: &PrimitiveBlock) -> Vec<TaggedNode> {
    let mut out = Vec::new();
    for group in block.groups() {
        for n in group.dense_nodes() {
            if n.tags().next().is_some() {
                out.extend(tagged_node(n.id(), n.lat(), n.lon(), n.tags()));
            }
        }
        for n in group.nodes() {
            out.extend(tagged_node(n.id(), n.lat(), n.lon(), n.tags()));
        }
    }
    out
}

enum MotorwayWay {
    Link(LinkWay),
    Motorway(Vec<i64>),
    Trunk(Vec<i64>),
}

fn motorway_ways_of(block: &PrimitiveBlock) -> Vec<MotorwayWay> {
    let mut out = Vec::new();
    for group in block.groups() {
        for way in group.ways() {
            let mut highway = "";
            let mut tags: Vec<(&str, &str)> = Vec::new();
            for (k, v) in way.tags() {
                if k == "highway" {
                    highway = v;
                }
                tags.push((k, v));
            }
            match highway {
                "motorway" => out.push(MotorwayWay::Motorway(way.refs().collect())),
                "trunk" => out.push(MotorwayWay::Trunk(way.refs().collect())),
                "motorway_link" => {
                    let get = |key: &str| {
                        tags.iter()
                            .find(|(k, _)| *k == key)
                            .map(|(_, v)| *v)
                            .unwrap_or("")
                    };
                    let advisory = |suffix: &str| {
                        [
                            format!("maxspeed:advisory:{suffix}"),
                            "maxspeed:advisory".into(),
                        ]
                        .iter()
                        .find_map(|key| parse_advisory_mph(get(key)))
                    };
                    out.push(MotorwayWay::Link(LinkWay {
                        refs: way.refs().collect(),
                        oneway: Oneway::parse(get("oneway")),
                        destination: get("destination").to_string(),
                        destination_ref: get("destination:ref").trim().to_string(),
                        advisory_along: advisory("forward"),
                        advisory_against: advisory("backward"),
                    }));
                }
                _ => {}
            }
        }
    }
    out
}

/// Ramp nodes on a public road, flagged when that road is a roundabout.
fn crossroads_of(block: &PrimitiveBlock, link_nodes: &HashSet<i64>) -> Vec<(i64, bool)> {
    let mut out = Vec::new();
    for group in block.groups() {
        for way in group.ways() {
            let (mut highway, mut access, mut roundabout) = ("", "", false);
            for (k, v) in way.tags() {
                match k {
                    "highway" => highway = v,
                    "access" => access = v,
                    "junction" => roundabout = matches!(v, "roundabout" | "circular"),
                    _ => {}
                }
            }
            if !is_public_crossroad(highway, access) {
                continue;
            }
            for id in way.refs() {
                if link_nodes.contains(&id) {
                    out.push((id, roundabout));
                }
            }
        }
    }
    out
}

fn locations_of(block: &PrimitiveBlock, wanted: &HashSet<i64>) -> Vec<(i64, f64, f64)> {
    let mut out = Vec::new();
    for group in block.groups() {
        for n in group.dense_nodes() {
            if wanted.contains(&n.id()) {
                out.push((n.id(), n.lat(), n.lon()));
            }
        }
        for n in group.nodes() {
            if wanted.contains(&n.id()) {
                out.push((n.id(), n.lat(), n.lon()));
            }
        }
    }
    out
}

fn timed<T>(label: &str, path: &Path, f: impl FnOnce() -> Result<T>) -> Result<T> {
    let started = Instant::now();
    let out = f()?;
    println!(
        "  {}: {label} ({:.0} s)",
        path.display(),
        started.elapsed().as_secs_f64()
    );
    Ok(out)
}

/// Read the ramp network from every extract. Overlapping extracts are fine:
/// node ids are global, so a node or way seen twice is kept once.
pub fn read_network(paths: &[impl AsRef<Path>]) -> Result<RawNetwork> {
    let mut net = RawNetwork::default();
    let mut seen_junction: HashSet<i64> = HashSet::new();
    let mut seen_control: HashSet<i64> = HashSet::new();
    for path in paths {
        let path = path.as_ref();
        let nodes = timed("tagged nodes", path, || each_block(path, tagged_nodes_of))?;
        for node in nodes {
            match node {
                TaggedNode::Junction(j) => {
                    if seen_junction.insert(j.id) {
                        net.junctions.push(j);
                    }
                }
                TaggedNode::Control(c) => {
                    if seen_control.insert(c.id) {
                        net.controls.push(c);
                    }
                }
                TaggedNode::Toll(id) => {
                    net.toll.insert(id);
                }
            }
        }
    }
    net.junctions.sort_by_key(|j| j.id);
    net.controls.sort_by_key(|c| c.id);

    let junction_ids: HashSet<i64> = net.junctions.iter().map(|j| j.id).collect();
    let mut motorway_nodes: HashSet<i64> = HashSet::new();
    let mut trunk_nodes: HashSet<i64> = HashSet::new();
    let mut seen_links: HashSet<Vec<i64>> = HashSet::new();
    for path in paths {
        let path = path.as_ref();
        let ways = timed("motorway and ramp ways", path, || {
            each_block(path, motorway_ways_of)
        })?;
        for way in ways {
            let motorway = matches!(way, MotorwayWay::Motorway(_));
            match way {
                MotorwayWay::Link(link) => {
                    if link.refs.len() >= 2 && seen_links.insert(link.refs.clone()) {
                        net.links.push(link);
                    }
                }
                MotorwayWay::Motorway(refs) | MotorwayWay::Trunk(refs) => {
                    for (i, id) in refs.iter().enumerate() {
                        if junction_ids.contains(id) {
                            let before = i.checked_sub(1).map(|b| refs[b]);
                            let after = refs.get(i + 1).copied();
                            let entry = net.junction_neighbours.entry(*id).or_default();
                            if !entry.contains(&(before, after)) {
                                entry.push((before, after));
                            }
                        }
                    }
                    if motorway {
                        motorway_nodes.extend(refs);
                    } else {
                        trunk_nodes.extend(refs);
                    }
                }
            }
        }
    }
    // The extracts' block order is not fixed, so sort for a stable bake.
    net.links.sort_by(|a, b| a.refs.cmp(&b.refs));
    let link_nodes: HashSet<i64> = net.links.iter().flat_map(|w| w.refs.clone()).collect();
    net.mainline = link_nodes
        .iter()
        .copied()
        .filter(|n| motorway_nodes.contains(n))
        .collect();
    net.trunk = link_nodes
        .iter()
        .copied()
        .filter(|n| trunk_nodes.contains(n))
        .collect();
    drop(motorway_nodes);
    drop(trunk_nodes);

    for path in paths {
        let path = path.as_ref();
        let found = timed("public roads meeting a ramp", path, || {
            each_block(path, |b| crossroads_of(b, &link_nodes))
        })?;
        for (id, roundabout) in found {
            net.crossroad.insert(id);
            if roundabout {
                net.roundabout.insert(id);
            }
        }
    }

    let mut wanted = link_nodes;
    for pairs in net.junction_neighbours.values() {
        for (a, b) in pairs {
            wanted.extend(a.iter().chain(b.iter()));
        }
    }
    for path in paths {
        let path = path.as_ref();
        let found = timed("ramp node locations", path, || {
            each_block(path, |b| locations_of(b, &wanted))
        })?;
        for (id, lat, lon) in found {
            net.locs.entry(id).or_insert((lat, lon));
        }
    }
    println!(
        "  {} exit junctions, {} ramp ways over {} nodes, {} traffic controls, {} toll booths",
        net.junctions.len(),
        net.links.len(),
        net.locs.len(),
        net.controls.len(),
        net.toll.len()
    );
    Ok(net)
}
