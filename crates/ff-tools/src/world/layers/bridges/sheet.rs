//! The curated bridge sheet, `docs/bridges/*.json`: the bridges written
//! whatever their length, each with its OpenStreetMap ways, the legs
//! expected to cross it and a citation.
//!
//! ```json
//! {"bridges": [{"name": "Confederation Bridge", "over": "Northumberland Strait",
//!   "water": true, "osm": ["way/123", "relation/456"],
//!   "legs": ["charlottetown_pe_ca:moncton_nb_ca"],
//!   "citation": "Strait Crossing Bridge Limited, confederationbridge.com"}]}
//! ```
//!
//! `osm` lists the bridge's own road ways, or a `type=bridge` relation
//! whose members are, or a `man_made=bridge` outline that holds them.

use std::collections::HashMap;
use std::path::Path;

use serde::Deserialize;

use super::super::features::Features;
use crate::util::Result;

#[derive(Debug, Clone, Deserialize)]
pub struct SheetBridge {
    pub name: String,
    #[serde(default)]
    pub over: String,
    #[serde(default)]
    pub water: bool,
    pub osm: Vec<String>,
    #[serde(default)]
    pub legs: Vec<String>,
    pub citation: String,
}

#[derive(Deserialize)]
struct File {
    bridges: Vec<SheetBridge>,
}

#[derive(Default)]
pub struct Sheet {
    pub bridges: Vec<SheetBridge>,
    /// Road way id -> entry index, resolved against the extract.
    by_way: HashMap<i64, usize>,
}

/// `way/123` -> ("way", 123).
fn element(s: &str) -> Option<(&str, i64)> {
    let (kind, id) = s.split_once('/')?;
    Some((kind, id.parse().ok()?))
}

impl Sheet {
    pub fn load(dir: &Path) -> Result<Self> {
        let mut bridges = Vec::new();
        let mut paths: Vec<_> = std::fs::read_dir(dir)
            .map_err(|e| format!("{}: {e}", dir.display()))?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|x| x == "json"))
            .collect();
        paths.sort();
        for path in paths {
            let text =
                std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            let file: File =
                serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
            for b in &file.bridges {
                if b.osm.iter().any(|o| element(o).is_none()) {
                    return Err(format!(
                        "{}: {} has an osm entry that is not kind/id",
                        path.display(),
                        b.name
                    ));
                }
            }
            bridges.extend(file.bridges);
        }
        Ok(Self {
            bridges,
            by_way: HashMap::new(),
        })
    }

    /// Resolve each entry to its road ways: listed ways, the members of a
    /// listed relation, and the road ways inside a listed outline.
    pub fn resolve(&mut self, found: &Features, carriers: &[(i64, (f64, f64))]) {
        let outlines: HashMap<i64, &Vec<(f64, f64)>> =
            found.ways.iter().map(|w| (w.id, &w.coords)).collect();
        let relations: HashMap<i64, &Vec<i64>> = found
            .relations
            .iter()
            .map(|r| (r.id, &r.member_ids))
            .collect();
        for (i, b) in self.bridges.iter().enumerate() {
            for o in &b.osm {
                let Some((kind, id)) = element(o) else {
                    continue;
                };
                match kind {
                    "relation" => {
                        for m in relations.get(&id).into_iter().flat_map(|v| v.iter()) {
                            self.by_way.insert(*m, i);
                        }
                    }
                    "way" => {
                        self.by_way.insert(id, i);
                        // A listed outline holds the road ways inside it.
                        if let Some(ring) = outlines
                            .get(&id)
                            .filter(|r| r.len() >= 4 && r.first() == r.last())
                        {
                            for (wid, mid) in carriers {
                                if ring_contains(ring, mid.0, mid.1) {
                                    self.by_way.insert(*wid, i);
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    pub fn entry_for_way(&self, way: i64) -> Option<&SheetBridge> {
        self.by_way.get(&way).map(|&i| &self.bridges[i])
    }

    /// Entries no leg placed, for the report.
    pub fn unplaced<'a>(&'a self, placed: &'a [String]) -> impl Iterator<Item = &'a SheetBridge> {
        self.bridges.iter().filter(|b| !placed.contains(&b.name))
    }
}

fn ring_contains(r: &[(f64, f64)], lat: f64, lon: f64) -> bool {
    let mut inside = false;
    let mut j = r.len() - 1;
    for i in 0..r.len() {
        let ((yi, xi), (yj, xj)) = (r[i], r[j]);
        if (yi > lat) != (yj > lat) && lon < (xj - xi) * (lat - yi) / (yj - yi) + xi {
            inside = !inside;
        }
        j = i;
    }
    inside
}
