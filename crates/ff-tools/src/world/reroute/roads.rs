//! What a rerouted leg's road is made of, read from an OpenStreetMap extract:
//! the class and route number of every road along it, and the state lines
//! it crosses.
//!
//! Four passes over the extract, so memory follows the corridor and not the
//! country: the nodes inside the leg's grid cells, the state boundary
//! relations, the ways (highways touching the corridor, and every member of a
//! state boundary), then the boundary ways' remaining nodes. A state line is
//! often one straight segment kilometres long, so its nodes cannot be found
//! by position the way a road's can.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use osmpbf::{BlobDecode, BlobReader, PrimitiveBlock, RelMemberType};
use rayon::iter::{ParallelBridge, ParallelIterator};

use super::super::osm::geo::{self, Cell, OsmWay, WayIndex};
use crate::util::Result;

/// A road near the leg: its OSM class and `ref`.
#[derive(Debug, Clone)]
pub struct Road {
    pub class: String,
    pub route_ref: String,
}

/// One state boundary way: its points and the states either side.
#[derive(Debug, Clone)]
pub struct Boundary {
    pub coords: Vec<(f64, f64)>,
    pub states: Vec<String>,
}

pub struct Network {
    /// Roads indexed for nearest lookups; `roads[&way.id]` is the class.
    pub index: WayIndex,
    pub roads: HashMap<i64, Road>,
    pub boundaries: Vec<Boundary>,
}

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

fn nodes_where(
    block: &PrimitiveBlock,
    keep: &(impl Fn(i64, f64, f64) -> bool + Sync),
) -> Vec<(i64, f64, f64)> {
    let mut out = Vec::new();
    for group in block.groups() {
        for n in group.dense_nodes() {
            if keep(n.id(), n.lat(), n.lon()) {
                out.push((n.id(), n.lat(), n.lon()));
            }
        }
        for n in group.nodes() {
            if keep(n.id(), n.lat(), n.lon()) {
                out.push((n.id(), n.lat(), n.lon()));
            }
        }
    }
    out
}

/// `(state name, member way ids)` for each `admin_level=4` boundary.
fn state_relations(block: &PrimitiveBlock) -> Vec<(String, Vec<i64>)> {
    let mut out = Vec::new();
    for group in block.groups() {
        for rel in group.relations() {
            let (mut admin, mut level, mut name) = (false, false, None);
            for (k, v) in rel.tags() {
                match k {
                    "boundary" => admin = v == "administrative",
                    "admin_level" => level = v == "4",
                    "name" => name = Some(v.to_string()),
                    _ => {}
                }
            }
            let Some(name) = name.filter(|_| admin && level) else {
                continue;
            };
            let ways = rel
                .members()
                .filter(|m| m.member_type == RelMemberType::Way)
                .map(|m| m.member_id)
                .collect();
            out.push((name, ways));
        }
    }
    out
}

enum RawWay {
    Road(i64, Road, Vec<i64>),
    Border(i64, Vec<i64>),
}

fn ways_of(
    block: &PrimitiveBlock,
    near: &HashMap<i64, (f64, f64)>,
    borders: &HashSet<i64>,
) -> Vec<RawWay> {
    let mut out = Vec::new();
    for group in block.groups() {
        for way in group.ways() {
            if borders.contains(&way.id()) {
                out.push(RawWay::Border(way.id(), way.refs().collect()));
                continue;
            }
            let mut class = None;
            let mut route_ref = String::new();
            for (k, v) in way.tags() {
                match k {
                    "highway" => class = Some(v.to_string()),
                    "ref" => route_ref = v.trim().to_string(),
                    _ => {}
                }
            }
            let Some(class) = class else { continue };
            let refs: Vec<i64> = way.refs().collect();
            if refs.iter().any(|r| near.contains_key(r)) {
                out.push(RawWay::Road(way.id(), Road { class, route_ref }, refs));
            }
        }
    }
    out
}

/// Read the roads within the `cells` and every state boundary.
pub fn read(paths: &[impl AsRef<Path>], cells: &HashSet<Cell>) -> Result<Network> {
    let mut near: HashMap<i64, (f64, f64)> = HashMap::new();
    let mut state_of_way: HashMap<i64, Vec<String>> = HashMap::new();
    for path in paths {
        let path = path.as_ref();
        let keep = |_: i64, lat: f64, lon: f64| cells.contains(&geo::cell(lat, lon));
        for (id, lat, lon) in each_block(path, |b| nodes_where(b, &keep))? {
            near.insert(id, (lat, lon));
        }
        for (state, ways) in each_block(path, state_relations)? {
            for w in ways {
                let states = state_of_way.entry(w).or_default();
                if !states.contains(&state) {
                    states.push(state.clone());
                }
            }
        }
    }
    let borders: HashSet<i64> = state_of_way.keys().copied().collect();
    let mut roads: HashMap<i64, (Road, Vec<i64>)> = HashMap::new();
    let mut border_refs: HashMap<i64, Vec<i64>> = HashMap::new();
    for path in paths {
        for raw in each_block(path.as_ref(), |b| ways_of(b, &near, &borders))? {
            match raw {
                RawWay::Road(id, road, refs) => {
                    roads.entry(id).or_insert((road, refs));
                }
                RawWay::Border(id, refs) => {
                    border_refs.entry(id).or_insert(refs);
                }
            }
        }
    }
    let wanted: HashSet<i64> = border_refs
        .values()
        .flatten()
        .filter(|r| !near.contains_key(r))
        .copied()
        .collect();
    let mut far: HashMap<i64, (f64, f64)> = HashMap::new();
    for path in paths {
        let keep = |id: i64, _: f64, _: f64| wanted.contains(&id);
        for (id, lat, lon) in each_block(path.as_ref(), |b| nodes_where(b, &keep))? {
            far.insert(id, (lat, lon));
        }
    }
    let locate = |r: &i64| near.get(r).or_else(|| far.get(r)).copied();

    let mut ids: Vec<i64> = roads.keys().copied().collect();
    ids.sort_unstable();
    let mut ways = Vec::new();
    let mut classes = HashMap::new();
    for id in ids {
        let Some((road, refs)) = roads.remove(&id) else {
            continue;
        };
        // A way leaving the corridor keeps its corridor run; the rest of it
        // was never read and is not needed.
        let coords: Vec<(f64, f64)> = refs.iter().filter_map(|r| near.get(r).copied()).collect();
        if coords.len() < 2 {
            continue;
        }
        ways.push(OsmWay {
            id,
            route_ref: road.route_ref.clone(),
            mph: None,
            hgv: false,
            ca_mph: None,
            ca_hgv: false,
            coords,
        });
        classes.insert(id, road);
    }
    let mut border_ids: Vec<i64> = border_refs.keys().copied().collect();
    border_ids.sort_unstable();
    let boundaries = border_ids
        .into_iter()
        .filter_map(|id| {
            let coords: Vec<(f64, f64)> = border_refs[&id].iter().filter_map(locate).collect();
            (coords.len() >= 2).then(|| Boundary {
                coords,
                states: state_of_way.remove(&id).unwrap_or_default(),
            })
        })
        .collect();
    Ok(Network {
        index: WayIndex::build(ways, cells),
        roads: classes,
        boundaries,
    })
}

/// The class a curve rule compares: OSM's own for the named classes and
/// ramps, `service_other` for everything below residential.
pub fn rule_class(class: &str) -> &str {
    match class {
        "motorway" | "trunk" | "primary" | "secondary" | "tertiary" | "unclassified"
        | "residential" => class,
        c if c.ends_with("_link") => c,
        _ => "service_other",
    }
}

impl Network {
    /// The nearest road within `tol_m`: `(class, ref, metres)`.
    pub fn nearest(&self, lat: f64, lon: f64, tol_m: f64) -> Option<(&Road, f64)> {
        let coslat = lat.to_radians().cos();
        self.index
            .near(lat, lon, tol_m, coslat)
            .into_iter()
            .filter_map(|(w, d)| self.roads.get(&w.id).map(|r| (r, d)))
            .min_by(|a, b| a.1.total_cmp(&b.1))
    }

    /// Any road within `tol_m` whose `ref` passes `test`.
    pub fn any_ref_near(
        &self,
        lat: f64,
        lon: f64,
        tol_m: f64,
        test: impl Fn(&str) -> bool,
    ) -> bool {
        let coslat = lat.to_radians().cos();
        self.index
            .near(lat, lon, tol_m, coslat)
            .into_iter()
            .any(|(w, _)| w.route_ref.split(';').any(|r| test(r.trim())))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classes_below_residential_share_one_rank() {
        assert_eq!(rule_class("motorway"), "motorway");
        assert_eq!(rule_class("motorway_link"), "motorway_link");
        assert_eq!(rule_class("service"), "service_other");
        assert_eq!(rule_class("living_street"), "service_other");
    }
}
