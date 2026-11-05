//! Read the posted-limit ways out of Geofabrik `.osm.pbf` extracts.
//!
//! Two passes, so no location index of the whole country is needed: the
//! first keeps the highway ways that carry `maxspeed` or `maxspeed:hgv` and
//! notes their node ids; the second resolves only those nodes. Blocks decode
//! in parallel. Extracts that overlap (two neighbouring states) are fine:
//! a way seen twice is kept once.

use std::collections::BTreeMap;
use std::path::Path;
use std::time::Instant;

use osmpbf::{BlobDecode, BlobReader, PrimitiveBlock};
use rayon::iter::{ParallelBridge, ParallelIterator};

use super::bake::way_limit;
use super::geo::OsmWay;
use crate::util::Result;

/// The road classes a leg's own limit can come from. Ramps (`*_link`) and
/// city streets stay out: their limits are what the screens exist to keep
/// off a highway.
pub const WAY_CLASSES: &[&str] = &["motorway", "trunk", "primary", "secondary", "tertiary"];

struct RawWay {
    id: i64,
    refs: Vec<i64>,
    route_ref: String,
    mph: Option<f64>,
    hgv: bool,
}

fn ways_of(block: &PrimitiveBlock) -> Vec<RawWay> {
    let mut out = Vec::new();
    for group in block.groups() {
        for way in group.ways() {
            let mut highway = None;
            let mut maxspeed = None;
            let mut maxspeed_hgv = None;
            let mut route_ref = String::new();
            for (k, v) in way.tags() {
                match k {
                    "highway" => highway = Some(v),
                    "maxspeed" => maxspeed = Some(v),
                    "maxspeed:hgv" => maxspeed_hgv = Some(v),
                    "ref" => route_ref = v.to_string(),
                    _ => {}
                }
            }
            if !highway.is_some_and(|h| WAY_CLASSES.contains(&h)) {
                continue;
            }
            if maxspeed.is_none() && maxspeed_hgv.is_none() {
                continue;
            }
            let (mph, hgv) = match way_limit(maxspeed, maxspeed_hgv) {
                Some((mph, hgv)) => (Some(mph), hgv),
                None => (None, false),
            };
            out.push(RawWay {
                id: way.id(),
                refs: way.refs().collect(),
                route_ref: route_ref.trim().to_string(),
                mph,
                hgv,
            });
        }
    }
    out
}

fn nodes_of(block: &PrimitiveBlock, wanted: &[i64]) -> Vec<(i64, f64, f64)> {
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

/// The extract's snapshot date (`YYYY-MM-DD`, UTC) from its header, which
/// Geofabrik stamps with the replication timestamp.
pub fn extract_date(path: &Path) -> Result<Option<String>> {
    let reader =
        BlobReader::from_path(path).map_err(|e| format!("cannot open {}: {e}", path.display()))?;
    for blob in reader {
        let blob = blob.map_err(|e| format!("{}: {e}", path.display()))?;
        if let Ok(BlobDecode::OsmHeader(header)) = blob.decode() {
            return Ok(header
                .osmosis_replication_timestamp()
                .and_then(|s| chrono::DateTime::from_timestamp(s, 0))
                .map(|t| t.format("%Y-%m-%d").to_string()));
        }
    }
    Ok(None)
}

/// Every posted-limit way in the extracts, with its coordinates.
pub fn read_ways(paths: &[impl AsRef<Path>]) -> Result<Vec<OsmWay>> {
    let mut raw: BTreeMap<i64, RawWay> = BTreeMap::new();
    for path in paths {
        let path = path.as_ref();
        let started = Instant::now();
        let ways = each_block(path, ways_of)?;
        println!(
            "  {}: {} posted-limit ways ({:.0} s)",
            path.display(),
            ways.len(),
            started.elapsed().as_secs_f64()
        );
        for way in ways {
            raw.entry(way.id).or_insert(way);
        }
    }
    let mut wanted: Vec<i64> = raw.values().flat_map(|w| w.refs.iter().copied()).collect();
    wanted.sort_unstable();
    wanted.dedup();
    let mut nodes: Vec<(i64, f64, f64)> = Vec::new();
    for path in paths {
        let path = path.as_ref();
        let started = Instant::now();
        nodes.extend(each_block(path, |b| nodes_of(b, &wanted))?);
        println!(
            "  {}: node locations resolved ({:.0} s)",
            path.display(),
            started.elapsed().as_secs_f64()
        );
    }
    nodes.sort_unstable_by_key(|n| n.0);
    nodes.dedup_by_key(|n| n.0);
    let locate = |id: i64| {
        nodes
            .binary_search_by_key(&id, |n| n.0)
            .ok()
            .map(|i| (nodes[i].1, nodes[i].2))
    };
    let mut out = Vec::with_capacity(raw.len());
    for (id, way) in raw {
        let coords: Vec<(f64, f64)> = way.refs.iter().filter_map(|r| locate(*r)).collect();
        if coords.len() >= 2 {
            out.push(OsmWay {
                id,
                route_ref: way.route_ref,
                mph: way.mph,
                hgv: way.hgv,
                coords,
            });
        }
    }
    Ok(out)
}
