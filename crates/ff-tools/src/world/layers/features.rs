//! Read tagged nodes, ways and relations out of `.osm.pbf` extracts.
//!
//! The layer commands each want a different slice of the map (lane-tagged
//! highways, named rivers, park boundaries, town nodes), so the caller
//! passes three filters and the reader keeps what they accept. Passes, with
//! no location index of the whole country held: the tagged elements; the
//! member ways of the kept relations, when they are not kept already; then
//! the locations of every node those ways use. Blocks decode in parallel,
//! and an element seen in two overlapping extracts is kept once.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

use osmpbf::{PrimitiveBlock, RelMemberType};

use super::super::osm::pbf::each_block;
use crate::util::Result;

/// An element's tags, as `(key, value)` pairs.
pub type Tags = Vec<(String, String)>;

pub fn tag<'a>(tags: &'a Tags, key: &str) -> Option<&'a str> {
    tags.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
}

pub type Filter = dyn Fn(&[(&str, &str)]) -> bool + Sync;

/// What to keep. A filter that is `None` keeps nothing of that type.
pub struct Select<'a> {
    pub node: Option<&'a Filter>,
    pub way: Option<&'a Filter>,
    pub relation: Option<&'a Filter>,
}

#[derive(Debug, Clone)]
pub struct Node {
    pub id: i64,
    pub tags: Tags,
    pub lat: f64,
    pub lon: f64,
}

#[derive(Debug, Clone)]
pub struct Way {
    pub id: i64,
    pub tags: Tags,
    pub coords: Vec<(f64, f64)>,
}

#[derive(Debug, Clone)]
pub struct Relation {
    pub id: i64,
    pub tags: Tags,
    /// `(role, coords)` of every way member whose nodes were found.
    pub members: Vec<(String, Vec<(f64, f64)>)>,
}

#[derive(Debug, Default)]
pub struct Features {
    pub nodes: Vec<Node>,
    pub ways: Vec<Way>,
    pub relations: Vec<Relation>,
}

struct RawWay {
    id: i64,
    tags: Tags,
    refs: Vec<i64>,
}

struct RawRel {
    id: i64,
    tags: Tags,
    members: Vec<(String, i64)>,
}

enum Item {
    Node(Node),
    Way(RawWay),
    Rel(RawRel),
}

fn owned(tags: &[(&str, &str)]) -> Tags {
    tags.iter()
        .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
        .collect()
}

fn tagged(block: &PrimitiveBlock, select: &Select) -> Vec<Item> {
    let mut out = Vec::new();
    let mut buf: Vec<(&str, &str)> = Vec::new();
    for group in block.groups() {
        if let Some(keep) = select.node {
            for node in group.dense_nodes() {
                buf.clear();
                buf.extend(node.tags());
                if !buf.is_empty() && keep(&buf) {
                    out.push(Item::Node(Node {
                        id: node.id(),
                        tags: owned(&buf),
                        lat: node.lat(),
                        lon: node.lon(),
                    }));
                }
            }
            for node in group.nodes() {
                buf.clear();
                buf.extend(node.tags());
                if !buf.is_empty() && keep(&buf) {
                    out.push(Item::Node(Node {
                        id: node.id(),
                        tags: owned(&buf),
                        lat: node.lat(),
                        lon: node.lon(),
                    }));
                }
            }
        }
        if let Some(keep) = select.way {
            for way in group.ways() {
                buf.clear();
                buf.extend(way.tags());
                if !buf.is_empty() && keep(&buf) {
                    out.push(Item::Way(RawWay {
                        id: way.id(),
                        tags: owned(&buf),
                        refs: way.refs().collect(),
                    }));
                }
            }
        }
        if let Some(keep) = select.relation {
            for rel in group.relations() {
                buf.clear();
                buf.extend(rel.tags());
                if !buf.is_empty() && keep(&buf) {
                    let members = rel
                        .members()
                        .filter(|m| m.member_type == RelMemberType::Way)
                        .map(|m| (m.role().unwrap_or("").to_string(), m.member_id))
                        .collect();
                    out.push(Item::Rel(RawRel {
                        id: rel.id(),
                        tags: owned(&buf),
                        members,
                    }));
                }
            }
        }
    }
    out
}

fn way_refs(block: &PrimitiveBlock, wanted: &[i64]) -> Vec<(i64, Vec<i64>)> {
    let mut out = Vec::new();
    for group in block.groups() {
        for way in group.ways() {
            if wanted.binary_search(&way.id()).is_ok() {
                out.push((way.id(), way.refs().collect()));
            }
        }
    }
    out
}

fn node_locations(block: &PrimitiveBlock, wanted: &[i64]) -> Vec<(i64, f64, f64)> {
    let mut out = Vec::new();
    for group in block.groups() {
        for node in group.dense_nodes() {
            if wanted.binary_search(&node.id()).is_ok() {
                out.push((node.id(), node.lat(), node.lon()));
            }
        }
        for node in group.nodes() {
            if wanted.binary_search(&node.id()).is_ok() {
                out.push((node.id(), node.lat(), node.lon()));
            }
        }
    }
    out
}

fn timed<T>(path: &Path, what: &str, f: impl FnOnce() -> Result<T>) -> Result<T> {
    let started = Instant::now();
    let out = f()?;
    println!(
        "  {}: {what} ({:.0} s)",
        path.display(),
        started.elapsed().as_secs_f64()
    );
    Ok(out)
}

/// Everything the filters accept, with coordinates.
pub fn read(paths: &[PathBuf], select: &Select) -> Result<Features> {
    let mut nodes: BTreeMap<i64, Node> = BTreeMap::new();
    let mut ways: BTreeMap<i64, RawWay> = BTreeMap::new();
    let mut rels: BTreeMap<i64, RawRel> = BTreeMap::new();
    for path in paths {
        let items = timed(path, "tagged elements read", || {
            each_block(path, |b| tagged(b, select))
        })?;
        for item in items {
            match item {
                Item::Node(n) => {
                    nodes.entry(n.id).or_insert(n);
                }
                Item::Way(w) => {
                    ways.entry(w.id).or_insert(w);
                }
                Item::Rel(r) => {
                    rels.entry(r.id).or_insert(r);
                }
            }
        }
    }

    // Member ways the way filter did not keep.
    let mut member_ids: Vec<i64> = rels
        .values()
        .flat_map(|r| r.members.iter().map(|m| m.1))
        .filter(|id| !ways.contains_key(id))
        .collect();
    member_ids.sort_unstable();
    member_ids.dedup();
    let mut member_refs: BTreeMap<i64, Vec<i64>> = BTreeMap::new();
    if !member_ids.is_empty() {
        for path in paths {
            let found = timed(path, "relation member ways read", || {
                each_block(path, |b| way_refs(b, &member_ids))
            })?;
            for (id, refs) in found {
                member_refs.entry(id).or_insert(refs);
            }
        }
    }

    let mut wanted: Vec<i64> = ways
        .values()
        .flat_map(|w| w.refs.iter().copied())
        .chain(member_refs.values().flatten().copied())
        .collect();
    wanted.sort_unstable();
    wanted.dedup();
    let mut located: Vec<(i64, f64, f64)> = Vec::new();
    if !wanted.is_empty() {
        for path in paths {
            located.extend(timed(path, "node locations resolved", || {
                each_block(path, |b| node_locations(b, &wanted))
            })?);
        }
    }
    located.sort_unstable_by_key(|n| n.0);
    located.dedup_by_key(|n| n.0);
    let line = |refs: &[i64]| -> Vec<(f64, f64)> {
        refs.iter()
            .filter_map(|r| {
                located
                    .binary_search_by_key(r, |n| n.0)
                    .ok()
                    .map(|i| (located[i].1, located[i].2))
            })
            .collect()
    };

    let relations = rels
        .into_values()
        .map(|r| Relation {
            id: r.id,
            members: r
                .members
                .iter()
                .filter_map(|(role, id)| {
                    let refs = ways
                        .get(id)
                        .map(|w| w.refs.as_slice())
                        .or_else(|| member_refs.get(id).map(Vec::as_slice))?;
                    let coords = line(refs);
                    (coords.len() >= 2).then(|| (role.clone(), coords))
                })
                .collect(),
            tags: r.tags,
        })
        .collect();
    let ways = ways
        .into_values()
        .filter_map(|w| {
            let coords = line(&w.refs);
            (coords.len() >= 2).then_some(Way {
                id: w.id,
                tags: w.tags,
                coords,
            })
        })
        .collect();
    Ok(Features {
        nodes: nodes.into_values().collect(),
        ways,
        relations,
    })
}
