//! Which side of the Canada–United States border a point lies on, from the
//! province and territory boundaries in an OpenStreetMap extract.
//!
//! The Canadian `admin_level=4` relations are read as one set of edges, and a
//! point is in Canada when a ray from it crosses those edges an odd number of
//! times. A border two provinces share is counted twice and cancels, so the
//! set traces Canada's outline without assembling rings. A province whose
//! member ways do not close (the extract cut part of it off) would break the
//! count, so it is left out, and a point inside its box is not judged.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use super::features::{self, tag, Select};
use crate::util::Result;

/// `(lat1, lon1, lat2, lon2)`.
type Edge = (f64, f64, f64, f64);

/// A way's points, `(lat, lon)`.
type Line = Vec<(f64, f64)>;

/// `(min_lat, min_lon, max_lat, max_lon)`.
type Bbox = (f64, f64, f64, f64);

#[derive(Debug, Default)]
pub struct Provinces {
    edges: Vec<Edge>,
    unjudged: Vec<Bbox>,
    pub read: Vec<String>,
    pub left_out: Vec<String>,
}

fn key(p: (f64, f64)) -> (i64, i64) {
    ((p.0 * 1e7).round() as i64, (p.1 * 1e7).round() as i64)
}

/// Whether the ways close: every end point is shared by an even number of
/// way ends.
fn closes(ways: &[Line]) -> bool {
    let mut ends: HashMap<(i64, i64), usize> = HashMap::new();
    for w in ways {
        if let (Some(a), Some(b)) = (w.first(), w.last()) {
            *ends.entry(key(*a)).or_default() += 1;
            *ends.entry(key(*b)).or_default() += 1;
        }
    }
    !ends.is_empty() && ends.values().all(|n| n % 2 == 0)
}

impl Provinces {
    /// The provinces named in `canadian` (English names, as `geo` spells them).
    pub fn from_ways(named: Vec<(String, Vec<Line>)>) -> Self {
        let mut out = Provinces::default();
        for (name, ways) in named {
            if closes(&ways) {
                for w in &ways {
                    out.edges
                        .extend(w.windows(2).map(|p| (p[0].0, p[0].1, p[1].0, p[1].1)));
                }
                out.read.push(name);
            } else {
                let pts = ways.iter().flatten();
                let bbox = pts.fold(
                    (f64::MAX, f64::MAX, f64::MIN, f64::MIN),
                    |(a, b, c, d), &(lat, lon)| (a.min(lat), b.min(lon), c.max(lat), d.max(lon)),
                );
                out.unjudged.push(bbox);
                out.left_out.push(name);
            }
        }
        out.read.sort();
        out.left_out.sort();
        out
    }

    pub fn read(pbf: &PathBuf, canadian: &HashSet<String>) -> Result<Self> {
        let keep = |t: &[(&str, &str)]| {
            let has = |k: &str| t.iter().find(|(tk, _)| *tk == k).map(|(_, v)| *v);
            has("boundary") == Some("administrative") && has("admin_level") == Some("4")
        };
        let found = features::read(
            std::slice::from_ref(pbf),
            &Select {
                node: None,
                way: None,
                relation: Some(&keep),
            },
        )?;
        let mut named: HashMap<String, Vec<Line>> = HashMap::new();
        for rel in found.relations {
            // Quebec's own `name` is "Québec"; `geo` spells it in English.
            let name = tag(&rel.tags, "name:en")
                .or_else(|| tag(&rel.tags, "name"))
                .unwrap_or("")
                .to_string();
            if !canadian.contains(&name) {
                continue;
            }
            named
                .entry(name)
                .or_default()
                .extend(rel.members.into_iter().map(|(_, coords)| coords));
        }
        Ok(Self::from_ways(named.into_iter().collect()))
    }

    /// Whether the point is in Canada; `None` where it cannot be judged.
    pub fn in_canada(&self, lat: f64, lon: f64) -> Option<bool> {
        if self
            .unjudged
            .iter()
            .any(|b| lat >= b.0 && lon >= b.1 && lat <= b.2 && lon <= b.3)
        {
            return None;
        }
        let crossings = self
            .edges
            .iter()
            .filter(|(y1, x1, y2, x2)| {
                (*y1 > lat) != (*y2 > lat) && x1 + (lat - y1) * (x2 - x1) / (y2 - y1) > lon
            })
            .count();
        Some(crossings % 2 == 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(lat0: f64, lon0: f64, lat1: f64, lon1: f64) -> Vec<Line> {
        vec![
            vec![(lat0, lon0), (lat0, lon1), (lat1, lon1)],
            vec![(lat1, lon1), (lat1, lon0), (lat0, lon0)],
        ]
    }

    #[test]
    fn a_shared_border_cancels_and_the_outline_stays() {
        let p = Provinces::from_ways(vec![
            ("West".into(), square(49.0, -120.0, 60.0, -110.0)),
            ("East".into(), square(49.0, -110.0, 60.0, -100.0)),
        ]);
        assert_eq!(p.in_canada(50.0, -115.0), Some(true));
        assert_eq!(p.in_canada(50.0, -105.0), Some(true));
        assert_eq!(p.in_canada(48.0, -105.0), Some(false));
    }

    #[test]
    fn a_province_that_does_not_close_is_not_judged() {
        let mut open = square(49.0, -90.0, 56.0, -75.0);
        open.pop();
        let p = Provinces::from_ways(vec![("Cut".into(), open)]);
        assert_eq!(p.left_out, vec!["Cut".to_string()]);
        assert_eq!(p.in_canada(50.0, -80.0), None);
        assert_eq!(p.in_canada(40.0, -80.0), Some(false));
    }
}
