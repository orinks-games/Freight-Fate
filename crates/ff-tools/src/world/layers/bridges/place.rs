//! Where a named bridge sits on a leg: the bridge-tagged road ways that lie
//! along the leg's archived road, named, merged into one span a name, and
//! checked for water under them.

use std::collections::{BTreeMap, HashMap};

use super::super::features::{tag, Features, Tags};
use super::super::{round, Grid, Route};
use super::sheet::{Sheet, SheetBridge};
use crate::world::osm::geo::{haversine_m, M_PER_MI};
use crate::world::spoken_name::spoken_name;

/// A way is part of the leg's road when every vertex lies this close to the
/// archived road. The archive is simplified to within 30 m of the real road
/// (`eps_tangent_m`), and the far carriageway of a divided highway runs
/// 15 to 40 m off the near one on a bridge, so 80 m holds both while an
/// overpass, which meets the road at one point and leaves it, fails the
/// second test below.
pub const ON_ROAD_M: f64 = 80.0;
/// And the way runs along the road: its projected extent is at least this
/// share of its own length. A bridge crossing over or under the road
/// projects to a point.
pub const ALONG_SHARE: f64 = 0.6;
/// Pieces of one bridge closer than this along the road are one span: OSM
/// splits a bridge into several ways at each lane change or joint.
pub const JOIN_GAP_MI: f64 = 0.1;
/// Two carriageways' spans that agree within this at each end are one
/// record for both directions.
pub const TWIN_TOLERANCE_MI: f64 = 0.05;

/// Road classes a truck leg drives; a rail or foot bridge is never one.
const ROAD_CLASSES: &[&str] = &[
    "motorway",
    "motorway_link",
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
];

/// A bridge-tagged road way.
pub fn is_carrier(t: &[(&str, &str)]) -> bool {
    let get = |k: &str| t.iter().find(|(tk, _)| *tk == k).map(|(_, v)| *v);
    get("bridge").is_some_and(|b| b != "no")
        && get("highway").is_some_and(|h| ROAD_CLASSES.contains(&h))
}

/// A named bridge outline, `man_made=bridge`.
pub fn is_outline(t: &[(&str, &str)]) -> bool {
    t.iter().any(|(k, v)| *k == "man_made" && *v == "bridge")
}

/// Water a bridge can span: rivers, canals, tidal channels and the coast.
pub fn is_water(t: &[(&str, &str)]) -> bool {
    t.iter().any(|(k, v)| {
        (*k == "waterway" && matches!(*v, "river" | "canal" | "tidal_channel"))
            || (*k == "natural" && *v == "coastline")
    })
}

/// Words that make a way's own `name` the bridge's rather than the road's
/// ("Blue Water Bridge", "Garden City Skyway", "Pont Laviolette").
const BRIDGE_WORDS: &[&str] = &["Bridge", "Skyway", "Causeway", "Viaduct", "Pont"];
/// A name with one of these is a street or a ramp, not a bridge's own name
/// ("Bridge Street", "Blue Water Bridge Approach", "Highway 118 Overpass").
const NOT_A_BRIDGE_NAME: &[&str] = &[
    "Street", "Road", "Avenue", "Drive", "Lane", "Boulevard", "Way", "Rue", "Chemin",
    "Approach", "Overpass", "Overhead", "Underpass", "Interchange", "Flyover", "Ramp",
    "Exit", "Port Of Entry", "Port of Entry",
];

/// Whether a name reads as a bridge's own name.
pub fn reads_as_bridge(name: &str) -> bool {
    let words: Vec<&str> = name
        .split(|c: char| c.is_whitespace() || c == '-')
        .filter(|w| !w.is_empty())
        .collect();
    words.iter().any(|w| BRIDGE_WORDS.contains(w))
        && !NOT_A_BRIDGE_NAME
            .iter()
            .any(|bad| name.split_whitespace().any(|w| w == *bad) || name.contains(&format!(" {bad}")))
}

/// A name for an overpass, ramp or interchange, which a driver does not
/// hear as crossing a bridge ("Highway 118 Overpass at Akerley Blvd.").
fn labels_a_ramp(name: &str) -> bool {
    ["Overpass", "Overhead", "Underpass", "Interchange", "Flyover", "Ramp", "Approach", "Connector"]
        .iter()
        .any(|w| name.contains(w))
}

/// The spoken name an element's tags give a bridge under `key` (and
/// `key:en`).
fn named(tags: &Tags, key: &str) -> Option<String> {
    let raw = tag(tags, key)?;
    let name = spoken_name(raw, tag(tags, &format!("{key}:en")));
    (!name.trim().is_empty()).then_some(name)
}

/// A bridge's name as it is said: a deck or a bracketed note is dropped
/// ("George Washington Bridge Upper Level" is the George Washington
/// Bridge, "I-70 Missouri River (Rocheport) Bridge" loses the bracket).
/// `None` when what is left names no bridge ("Jubilee Parkway", the road
/// over one), so an OSM name is spoken only when it is a bridge's.
pub fn bridge_name(name: &str) -> Option<String> {
    let mut out = String::with_capacity(name.len());
    let mut depth = 0usize;
    for c in name.chars() {
        match c {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    let mut out = out.split_whitespace().collect::<Vec<_>>().join(" ");
    for deck in [" Upper Level", " Lower Level", " Upper Deck", " Lower Deck"] {
        if let Some(stem) = out.strip_suffix(deck) {
            out = stem.to_string();
        }
    }
    let words: Vec<&str> = out
        .split(|c: char| c.is_whitespace() || c == '-')
        .filter(|w| !w.is_empty())
        .collect();
    let says_bridge = words
        .iter()
        .any(|w| BRIDGE_WORDS.contains(w) || *w == "Crossing")
        || out.contains("Bridge of Honor");
    says_bridge.then_some(out)
}

struct Outline {
    id: i64,
    name: String,
    ring: Vec<(f64, f64)>,
}

impl Outline {
    fn contains(&self, lat: f64, lon: f64) -> bool {
        let mut inside = false;
        let r = &self.ring;
        let mut j = r.len().wrapping_sub(1);
        for i in 0..r.len() {
            let ((yi, xi), (yj, xj)) = (r[i], r[j]);
            if (yi > lat) != (yj > lat) && lon < (xj - xi) * (lat - yi) / (yj - yi) + xi {
                inside = !inside;
            }
            j = i;
        }
        inside
    }
}

/// A bridge-tagged road way with the name the map gives it.
pub struct Carrier {
    pub id: i64,
    pub coords: Vec<(f64, f64)>,
    pub oneway: i8,
    /// `(spoken name, how it was named)`, when the map names it.
    pub name: Option<(String, String)>,
    /// The way's own `name` (usually the road's), for `--survey` only.
    pub road_name: String,
    pub length_m: f64,
}

pub struct Water {
    pub name: String,
    pub coords: Vec<(f64, f64)>,
}

/// Every carrier and water line in the extract, with a spatial index.
pub struct Catalog {
    pub carriers: Vec<Carrier>,
    pub water: Vec<Water>,
    carrier_grid: Grid,
    water_grid: Grid,
}

fn line_length_m(coords: &[(f64, f64)]) -> f64 {
    coords
        .windows(2)
        .map(|p| haversine_m(p[0].0, p[0].1, p[1].0, p[1].1))
        .sum()
}

fn oneway(tags: &Tags) -> i8 {
    match tag(tags, "oneway") {
        Some("yes" | "true" | "1") => 1,
        Some("-1" | "reverse") => -1,
        _ if tag(tags, "highway").is_some_and(|h| h == "motorway" || h == "motorway_link") => 1,
        _ => 0,
    }
}

impl Catalog {
    pub fn new(found: Features) -> Self {
        let mut outlines: Vec<Outline> = Vec::new();
        let mut raw_carriers = Vec::new();
        let mut water = Vec::new();
        for w in found.ways {
            let pairs: Vec<(&str, &str)> =
                w.tags.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
            if is_carrier(&pairs) {
                raw_carriers.push(w);
            } else if is_outline(&pairs) {
                if let Some(name) = named(&w.tags, "name").and_then(|n| bridge_name(&n)) {
                    outlines.push(Outline {
                        id: w.id,
                        name,
                        ring: w.coords,
                    });
                }
            } else if is_water(&pairs) {
                water.push(Water {
                    name: tag(&w.tags, "name").unwrap_or("").to_string(),
                    coords: w.coords,
                });
            }
        }
        // A type=bridge relation names its member ways.
        let mut by_relation: HashMap<i64, (String, i64)> = HashMap::new();
        for r in &found.relations {
            if let Some(name) = named(&r.tags, "name").and_then(|n| bridge_name(&n)) {
                for id in &r.member_ids {
                    by_relation.insert(*id, (name.clone(), r.id));
                }
            }
        }
        let mut outline_grid = Grid::new(0.05);
        for (i, o) in outlines.iter().enumerate() {
            outline_grid.insert_line(&o.ring, i as u32);
        }
        let carriers: Vec<Carrier> = raw_carriers
            .into_iter()
            .map(|w| {
                let mid = w.coords[w.coords.len() / 2];
                let name = named(&w.tags, "bridge:name")
                    .and_then(|n| bridge_name(&n))
                    .map(|n| (n, format!("read from the bridge:name tag of way {}", w.id)))
                    .or_else(|| {
                        // The way's own name, only where it is the bridge's.
                        named(&w.tags, "name")
                            .filter(|n| reads_as_bridge(n))
                            .and_then(|n| bridge_name(&n))
                            .map(|n| {
                            (n, format!("read from the name of way {}, a bridge's name", w.id))
                        })
                    })
                    .or_else(|| {
                        by_relation.get(&w.id).map(|(n, rid)| {
                            (
                                n.clone(),
                                format!("read from the name of bridge relation {rid}"),
                            )
                        })
                    })
                    .or_else(|| {
                        outline_grid
                            .along(&[mid, mid], 0)
                            .into_iter()
                            .map(|i| &outlines[i as usize])
                            .find(|o| o.contains(mid.0, mid.1))
                            .map(|o| {
                                (
                                    o.name.clone(),
                                    format!("read from the name of bridge outline way {}", o.id),
                                )
                            })
                    });
                Carrier {
                    road_name: tag(&w.tags, "name").unwrap_or("").to_string(),
                    id: w.id,
                    oneway: oneway(&w.tags),
                    length_m: line_length_m(&w.coords),
                    coords: w.coords,
                    name,
                }
            })
            .collect();
        let mut carrier_grid = Grid::new(0.05);
        for (i, c) in carriers.iter().enumerate() {
            carrier_grid.insert_line(&c.coords, i as u32);
        }
        let mut water_grid = Grid::new(0.05);
        for (i, w) in water.iter().enumerate() {
            water_grid.insert_line(&w.coords, i as u32);
        }
        Self {
            carriers,
            water,
            carrier_grid,
            water_grid,
        }
    }

    /// The water line a carrier crosses, by name ("" when unnamed).
    fn water_under(&self, coords: &[(f64, f64)]) -> Option<&str> {
        for i in self.water_grid.along(coords, 0) {
            let w = &self.water[i as usize];
            if crosses(coords, &w.coords) {
                return Some(w.name.as_str());
            }
        }
        None
    }
}

fn segments_cross(a: (f64, f64), b: (f64, f64), c: (f64, f64), d: (f64, f64)) -> bool {
    let (rx, ry) = (b.1 - a.1, b.0 - a.0);
    let (sx, sy) = (d.1 - c.1, d.0 - c.0);
    let den = rx * sy - ry * sx;
    if den.abs() < 1e-15 {
        return false;
    }
    let (qx, qy) = (c.1 - a.1, c.0 - a.0);
    let t = (qx * sy - qy * sx) / den;
    let u = (qx * ry - qy * rx) / den;
    (0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&u)
}

fn crosses(a: &[(f64, f64)], b: &[(f64, f64)]) -> bool {
    a.windows(2)
        .any(|p| b.windows(2).any(|q| segments_cross(p[0], p[1], q[0], q[1])))
}

/// One piece of a bridge on the leg, leg miles.
#[derive(Debug, Clone)]
struct Piece {
    from_mi: f64,
    to_mi: f64,
    /// 1 forward, -1 reverse, 0 both.
    dir: i8,
    way: i64,
}

/// A named bridge placed on one leg.
#[derive(Debug, Clone)]
pub struct Placed {
    pub from_mi: f64,
    pub to_mi: f64,
    pub name: String,
    pub over: String,
    pub water: bool,
    pub ways: &'static str,
    pub curated: bool,
    pub osm: String,
    /// Real length in feet (leg miles back to measured).
    pub length_ft: f64,
    pub source: String,
    /// The one carriageway it was found on when that side stands for
    /// both (1 forward, -1 reverse), else 0.
    pub found_on: i8,
}

impl Placed {
    pub fn row(&self) -> crate::pyjson::Value {
        let mut obj = crate::pyobj! {
            "from_mi" => round(self.from_mi, 3),
            "to_mi" => round(self.to_mi, 3),
            "name" => self.name.as_str(),
        };
        if !self.over.is_empty() {
            obj.insert("over", self.over.as_str().into());
        }
        if self.water {
            obj.insert("water", true.into());
        }
        if self.ways != "both" {
            obj.insert("ways", self.ways.into());
        }
        if self.curated {
            obj.insert("curated", true.into());
        }
        obj.insert("osm", self.osm.as_str().into());
        obj.insert("source", self.source.as_str().into());
        crate::pyjson::Value::Object(obj)
    }
}

/// Union touching intervals, in order.
fn union(mut spans: Vec<(f64, f64)>) -> Vec<(f64, f64)> {
    spans.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut out: Vec<(f64, f64)> = Vec::new();
    for (a, b) in spans {
        match out.last_mut() {
            Some(last) if a <= last.1 + JOIN_GAP_MI => last.1 = last.1.max(b),
            _ => out.push((a, b)),
        }
    }
    out
}

/// Every named bridge on `route`, by name and span. `bar_ft` keeps an
/// uncurated bridge only at that real length or more.
pub fn bake(
    catalog: &Catalog,
    sheet: &Sheet,
    route: &Route,
    bar_ft: f64,
    note: &str,
    survey: bool,
) -> Vec<Placed> {
    let raw_mi = route
        .points
        .windows(2)
        .map(|p| haversine_m(p[0].0, p[0].1, p[1].0, p[1].1))
        .sum::<f64>()
        / M_PER_MI;
    let scale = if raw_mi > 0.0 {
        route.total_mi() / raw_mi
    } else {
        1.0
    };
    // (name, curated entry) -> pieces
    let mut groups: BTreeMap<String, (Option<&SheetBridge>, String, Vec<Piece>)> = BTreeMap::new();
    for i in catalog.carrier_grid.along(&route.points, 1) {
        let c = &catalog.carriers[i as usize];
        let curated = sheet.entry_for_way(c.id);
        let (name, how) = match (curated, &c.name) {
            (Some(entry), _) => (
                entry.name.clone(),
                format!("read from the curated sheet ({})", entry.citation),
            ),
            (None, Some((n, how))) if !labels_a_ramp(n) => (n.clone(), how.clone()),
            // The survey lists unnamed bridges by the way's own name, to
            // find a curated bridge's ways; never written.
            (None, None) if survey => (format!("? {} (way {})", c.road_name, c.id), String::new()),
            (None, _) => continue,
        };
        let mut ats = Vec::with_capacity(c.coords.len());
        let mut on_road = true;
        for &(lat, lon) in &c.coords {
            let (at, off, _, _) = route.project(lat, lon);
            if off * M_PER_MI > ON_ROAD_M {
                on_road = false;
                break;
            }
            ats.push(at);
        }
        if !on_road || ats.is_empty() {
            continue;
        }
        let lo = ats.iter().copied().fold(f64::INFINITY, f64::min);
        let hi = ats.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        let length_mi = c.length_m / M_PER_MI * scale;
        if length_mi <= 0.0 || hi - lo < ALONG_SHARE * length_mi {
            continue;
        }
        // Which way along the leg the way runs, for a one-way carriageway.
        let runs_forward = ats.last() >= ats.first();
        let dir = match c.oneway {
            0 => 0,
            1 if runs_forward => 1,
            -1 if !runs_forward => 1,
            _ => -1,
        };
        let entry = groups
            .entry(name)
            .or_insert_with(|| (curated, how, Vec::new()));
        if entry.0.is_none() && curated.is_some() {
            entry.0 = curated;
        }
        entry.2.push(Piece {
            from_mi: lo,
            to_mi: hi,
            dir,
            way: c.id,
        });
    }
    let mut out = Vec::new();
    for (name, (curated, how, pieces)) in groups {
        let side = |want: i8| -> Vec<(f64, f64)> {
            union(
                pieces
                    .iter()
                    .filter(|p| p.dir == 0 || p.dir == want)
                    .map(|p| (p.from_mi, p.to_mi))
                    .collect(),
            )
        };
        let (fwd, rev) = (side(1), side(-1));
        // Each carriageway's spans; one side found alone stands for both
        // (its twin runs alongside, off the archived road).
        let found_on: i8 = match (fwd.is_empty(), rev.is_empty()) {
            (true, false) => -1,
            (false, true) => 1,
            _ => 0,
        };
        let records: Vec<((f64, f64), &'static str)> = if fwd.is_empty() || rev.is_empty() {
            let one = if fwd.is_empty() { &rev } else { &fwd };
            one.iter().map(|s| (*s, "both")).collect()
        } else if fwd.len() == rev.len()
            && fwd.iter().zip(&rev).all(|(a, b)| {
                (a.0 - b.0).abs() <= TWIN_TOLERANCE_MI && (a.1 - b.1).abs() <= TWIN_TOLERANCE_MI
            })
        {
            fwd.iter()
                .zip(&rev)
                .map(|(a, b)| ((a.0.min(b.0), a.1.max(b.1)), "both"))
                .collect()
        } else {
            fwd.iter()
                .map(|s| (*s, "forward"))
                .chain(rev.iter().map(|s| (*s, "reverse")))
                .collect()
        };
        for ((from, to), ways) in records {
            let length_ft = (to - from) / scale * 5280.0;
            if curated.is_none() && length_ft < bar_ft {
                continue;
            }
            let mut way_ids: Vec<i64> = pieces
                .iter()
                .filter(|p| p.from_mi < to + JOIN_GAP_MI && p.to_mi > from - JOIN_GAP_MI)
                .map(|p| p.way)
                .collect();
            way_ids.sort_unstable();
            way_ids.dedup();
            let coords: Vec<Vec<(f64, f64)>> = way_ids
                .iter()
                .filter_map(|id| catalog.carriers.iter().find(|c| c.id == *id))
                .map(|c| c.coords.clone())
                .collect();
            let water_name = coords.iter().find_map(|c| catalog.water_under(c));
            let (over, water, over_how) = match curated {
                Some(e) if !e.over.is_empty() => (
                    e.over.clone(),
                    e.water,
                    "what it spans read from the curated sheet",
                ),
                _ => match water_name {
                    Some(n) => (
                        n.to_string(),
                        true,
                        "water derived from a waterway or coastline the bridge ways cross",
                    ),
                    None => (
                        String::new(),
                        false,
                        "no waterway or coastline found under it (assumed dry)",
                    ),
                },
            };
            let osm = way_ids
                .iter()
                .map(|id| format!("way/{id}"))
                .collect::<Vec<_>>()
                .join(" ");
            let kind = if curated.is_some() {
                "curated"
            } else {
                "over the length bar"
            };
            out.push(Placed {
                from_mi: from,
                to_mi: to,
                over,
                water,
                ways,
                curated: curated.is_some(),
                length_ft,
                source: format!(
                    "Name {how}; span derived by projecting its ways ({osm}) onto the leg's \
                     archived road, rescaled to leg miles ({kind}, {:.0} ft); {over_how}. {note}",
                    length_ft
                ),
                osm,
                name: name.clone(),
                found_on,
            });
        }
    }
    // A sliver far from the rest of a bridge of the same name is a stray
    // way the road brushes past, not a second bridge.
    let longest = |name: &str, out: &[Placed]| {
        out.iter()
            .filter(|p| p.name == name)
            .map(|p| p.length_ft)
            .fold(0.0, f64::max)
    };
    let keep: Vec<bool> = out
        .iter()
        .map(|p| p.length_ft >= SLIVER_SHARE * longest(&p.name, &out))
        .collect();
    let mut keep = keep.into_iter();
    out.retain(|_| keep.next().unwrap_or(true));
    split_twins(&mut out);
    out.sort_by(|a, b| a.from_mi.total_cmp(&b.from_mi));
    out
}

/// Two bridges under different names, each found on one carriageway only
/// and side by side, are the two directions' own bridges (the I-90
/// floating bridges: the Lacey V. Murrow carries the eastbound road, the
/// Homer M. Hadley the westbound), so each holds only for the side it was
/// found on instead of standing for both.
fn split_twins(out: &mut [Placed]) {
    for i in 0..out.len() {
        for j in 0..out.len() {
            let (a, b) = (&out[i], &out[j]);
            if a.found_on != 1 || b.found_on != -1 || a.name == b.name {
                continue;
            }
            let overlap = a.to_mi.min(b.to_mi) - a.from_mi.max(b.from_mi);
            let shorter = (a.to_mi - a.from_mi).min(b.to_mi - b.from_mi);
            if overlap > 0.5 * shorter {
                out[i].ways = "forward";
                out[j].ways = "reverse";
            }
        }
    }
}

/// A piece under this share of the longest piece of the same name on the
/// leg is dropped (the Confederation Bridge's 72-foot stray, 16 miles from
/// the bridge on Charlottetown to Moncton).
const SLIVER_SHARE: f64 = 0.05;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn touching_pieces_are_one_span() {
        assert_eq!(
            union(vec![(2.0, 2.5), (1.0, 1.2), (1.25, 1.9), (4.0, 4.1)]),
            vec![(1.0, 2.5), (4.0, 4.1)]
        );
    }

    #[test]
    fn a_carrier_is_a_road_bridge() {
        assert!(is_carrier(&[("bridge", "yes"), ("highway", "trunk")]));
        assert!(!is_carrier(&[("bridge", "yes"), ("railway", "rail")]));
        assert!(!is_carrier(&[("bridge", "no"), ("highway", "trunk")]));
        assert!(is_water(&[("waterway", "river")]));
        assert!(reads_as_bridge("Blue Water Bridge"));
        assert!(reads_as_bridge("Pont Laviolette"));
        assert!(reads_as_bridge("Garden City Skyway"));
        assert!(!reads_as_bridge("Bridge Street"));
        assert!(!reads_as_bridge("Blue Water Bridge Approach"));
        assert!(!reads_as_bridge("Rue du Pont"));
        assert!(!reads_as_bridge("Trans-Canada Highway"));
        assert!(labels_a_ramp("Highway 118 Overpass at Akerley Blvd."));
        assert!(!is_water(&[("waterway", "ditch")]));
    }

    #[test]
    fn a_bridge_name_is_said_without_its_deck_or_bracket() {
        assert_eq!(
            bridge_name("George Washington Bridge Upper Level").as_deref(),
            Some("George Washington Bridge")
        );
        assert_eq!(
            bridge_name("Lance Corporal Leon Deraps I-70 Missouri River (Rocheport) Bridge")
                .as_deref(),
            Some("Lance Corporal Leon Deraps I-70 Missouri River Bridge")
        );
        assert_eq!(
            bridge_name("Saint John River High Level Crossing").as_deref(),
            Some("Saint John River High Level Crossing")
        );
        assert_eq!(bridge_name("Jubilee Parkway"), None);
    }

    #[test]
    fn lines_that_cross_are_found() {
        let a = [(0.0, 0.0), (1.0, 1.0)];
        let b = [(0.0, 1.0), (1.0, 0.0)];
        let c = [(2.0, 2.0), (3.0, 3.0)];
        assert!(crosses(&a, &b));
        assert!(!crosses(&a, &c));
    }
}
