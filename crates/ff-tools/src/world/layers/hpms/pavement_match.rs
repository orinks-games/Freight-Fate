//! The matching half of `hpms-pavement`: HPMS sections placed on a leg's
//! road, and the per-direction runs of surface and structure built from
//! them. No network here, so every rule has a unit test.
//!
//! The rules, each learned from the five-leg dry run (2026-11-20):
//!
//! * A feature's geometry can be several pieces scattered over kilometres
//!   while its milepoints span a tenth of a mile (Pennsylvania), so only its
//!   longest piece is placed.
//! * A section is on the road when its vertices sit near it (median within
//!   [`MEDIAN_OFF_M`], none beyond [`MAX_OFF_M`]) AND it runs along it: its
//!   projected span must be most of its own length. A crossing road's
//!   tenth-mile section near an overpass sits close but spans nothing.
//! * Where several sections cover one spot (overlapping inventory records,
//!   a frontage road, an interchanging Interstate), the highest functional
//!   class wins, then the route nearest the leg's road. Interstate rows
//!   beat a frontage road beside them; where the leg leaves the Interstate,
//!   the road it is on is the only one near enough.
//! * HPMS rows are two-way (facility 2), one-way (1, digitized in travel
//!   direction), or the non-inventory side of a divided road (6, digitized
//!   in the INVENTORY direction, so it applies the other way). A two-way
//!   row applies both ways unless a non-inventory row at the same spot
//!   carries its own reading of the same item.
//! * Where the leg drives a non-inventory carriageway that parted from
//!   the inventory one (the Grapevine, Pennsylvania's Interstate 81), HPMS
//!   has no surface of its own for it, and the inventory row up to
//!   [`WIDE_MEDIAN_OFF_M`] away speaks for the section, marked
//!   `inventory_side`. Its structures are never borrowed: bridges sit
//!   where each carriageway has them.

use std::collections::HashMap;

use crate::world::layers::Route;
use crate::world::osm::geo::{haversine_m, M_PER_MI};

/// Bin width along the leg, in leg miles: about 26 ft, under the 20 ft
/// minimum an HPMS bridge must exceed only by rounding.
pub const BIN_MI: f64 = 0.005;
pub const MEDIAN_OFF_M: f64 = 75.0;
pub const MAX_OFF_M: f64 = 200.0;
/// How far the inventory carriageway may sit from a leg that drives the
/// other one, where a divided road's carriageways part.
pub const WIDE_MEDIAN_OFF_M: f64 = 250.0;
pub const WIDE_MAX_OFF_M: f64 = 400.0;
/// A section must span at least this share of its own length along the leg.
pub const MIN_SPAN_SHARE: f64 = 0.6;
const GRID_DEG: f64 = 0.01;

/// One HPMS row as the server answered it.
#[derive(Debug, Clone, Default)]
pub struct Section {
    pub route_id: String,
    pub f_system: i64,
    pub facility: i64,
    pub surface: Option<i64>,
    pub structure: Option<i64>,
    pub iri: Option<f64>,
    pub faulting: Option<f64>,
    pub year: Option<i64>,
    /// Year_Last_Construction and YEAR_LAST_IMPROVEMENT, as years.
    pub built: Option<i64>,
    pub improved: Option<i64>,
    /// `Begin_Point` and `End_Point`, miles along the state's route.
    pub begin_mi: Option<f64>,
    pub end_mi: Option<f64>,
    /// Pieces of `(lat, lon)`.
    pub paths: Vec<Vec<(f64, f64)>>,
}

/// A section placed on the leg.
#[derive(Debug, Clone)]
pub struct Placed {
    pub lo: f64,
    pub hi: f64,
    pub mean_off_m: f64,
    /// Digitized from the leg's `from` toward its `to`.
    pub along: bool,
    pub index: usize,
    /// Beyond the near limits, within the wide ones: an inventory row that
    /// only speaks where the leg drives the other carriageway.
    pub wide: bool,
}

/// The leg's road with a grid over its segments, for fast projection.
pub struct RoadIndex<'a> {
    route: &'a Route,
    cells: HashMap<(i64, i64), Vec<usize>>,
    /// Leg miles per geometric mile (the archive is rescaled to the leg).
    pub scale: f64,
}

fn cell(lat: f64, lon: f64) -> (i64, i64) {
    (
        (lat / GRID_DEG).floor() as i64,
        (lon / GRID_DEG).floor() as i64,
    )
}

fn path_len_m(path: &[(f64, f64)]) -> f64 {
    path.windows(2)
        .map(|w| haversine_m(w[0].0, w[0].1, w[1].0, w[1].1))
        .sum()
}

impl<'a> RoadIndex<'a> {
    pub fn new(route: &'a Route) -> Self {
        let mut cells: HashMap<(i64, i64), Vec<usize>> = HashMap::new();
        for i in 1..route.points.len() {
            let (a, b) = (route.points[i - 1], route.points[i]);
            let span = ((b.0 - a.0).abs().max((b.1 - a.1).abs()) / (GRID_DEG / 2.0)).ceil();
            let steps = (span as usize).max(1);
            for s in 0..=steps {
                let t = s as f64 / steps as f64;
                let slot = cells
                    .entry(cell(a.0 + t * (b.0 - a.0), a.1 + t * (b.1 - a.1)))
                    .or_default();
                if slot.last() != Some(&i) {
                    slot.push(i);
                }
            }
        }
        let raw_mi = path_len_m(&route.points) / M_PER_MI;
        let scale = if raw_mi > 0.0 {
            route.total_mi() / raw_mi
        } else {
            1.0
        };
        Self {
            route,
            cells,
            scale,
        }
    }

    /// `(at_mi, off_m)` of the nearest point on the road within a cell or
    /// so, or `None` when the road is not near.
    pub fn project(&self, lat: f64, lon: f64) -> Option<(f64, f64)> {
        let (cy, cx) = cell(lat, lon);
        let cos = lat.to_radians().cos();
        let mut best: Option<(f64, f64)> = None;
        for dy in -1..=1 {
            for dx in -1..=1 {
                for &i in self.cells.get(&(cy + dy, cx + dx)).into_iter().flatten() {
                    let (ay, ax) = self.route.points[i - 1];
                    let (by, bx) = self.route.points[i];
                    let (vx, vy) = ((bx - ax) * cos, by - ay);
                    let len2 = vx * vx + vy * vy;
                    let t = if len2 == 0.0 {
                        0.0
                    } else {
                        (((lon - ax) * cos * vx + (lat - ay) * vy) / len2).clamp(0.0, 1.0)
                    };
                    let (qy, qx) = (ay + t * (by - ay), ax + t * (bx - ax));
                    let off = haversine_m(lat, lon, qy, qx);
                    if best.is_none_or(|b| off < b.1) {
                        let c = &self.route.cum_mi;
                        best = Some((c[i - 1] + t * (c[i] - c[i - 1]), off));
                    }
                }
            }
        }
        best
    }

    /// Place one section on the road, or `None` when it is not on it.
    pub fn place(&self, section: &Section, index: usize) -> Option<Placed> {
        let path = section
            .paths
            .iter()
            .max_by(|a, b| path_len_m(a).total_cmp(&path_len_m(b)))?;
        if path.len() < 2 {
            return None;
        }
        let mut hits = Vec::with_capacity(path.len());
        for &(lat, lon) in path {
            let (at, off) = self.project(lat, lon)?;
            if off > WIDE_MAX_OFF_M {
                return None;
            }
            hits.push((at, off));
        }
        let mut offs: Vec<f64> = hits.iter().map(|h| h.1).collect();
        offs.sort_by(f64::total_cmp);
        let near = offs[offs.len() / 2] <= MEDIAN_OFF_M && offs[offs.len() - 1] <= MAX_OFF_M;
        if !near && (section.facility != 2 || offs[offs.len() / 2] > WIDE_MEDIAN_OFF_M) {
            return None;
        }
        let lo = hits.iter().map(|h| h.0).fold(f64::INFINITY, f64::min);
        let hi = hits.iter().map(|h| h.0).fold(f64::NEG_INFINITY, f64::max);
        let own = path_len_m(path) / M_PER_MI * self.scale;
        if hi - lo < MIN_SPAN_SHARE * own - 0.002 || hi - lo > 1.5 * own + 0.05 {
            return None;
        }
        Some(Placed {
            lo,
            hi,
            mean_off_m: offs.iter().sum::<f64>() / offs.len() as f64,
            along: hits[hits.len() - 1].0 >= hits[0].0,
            index,
            wide: !near,
        })
    }
}

/// Which way along the leg a reading holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Directions {
    Both,
    Forward,
    Reverse,
}

impl Directions {
    pub fn name(self) -> &'static str {
        match self {
            Directions::Both => "both",
            Directions::Forward => "forward",
            Directions::Reverse => "reverse",
        }
    }
}

/// HPMS Surface_Type, by name (HPMS Field Manual, Item 49).
pub fn surface_name(code: i64) -> Option<&'static str> {
    Some(match code {
        1 => "unpaved",
        2 => "asphalt",
        3 => "jointed_plain_concrete",
        4 => "jointed_reinforced_concrete",
        5 => "continuous_concrete",
        6 => "asphalt_over_asphalt",
        7 => "asphalt_over_jointed_concrete",
        8 => "asphalt_over_continuous_concrete",
        9 => "unbonded_jointed_concrete_overlay",
        10 => "bonded_concrete_overlay",
        11 => "other",
        _ => return None,
    })
}

/// Surface_Type codes 3, 4, 5, 9 and 10: concrete on top.
pub fn exposed_concrete(code: i64) -> bool {
    matches!(code, 3 | 4 | 5 | 9 | 10)
}

/// HPMS Structure_Type, by name (HPMS Field Manual, Item 4).
pub fn structure_name(code: i64) -> Option<&'static str> {
    Some(match code {
        1 => "bridge",
        2 => "tunnel",
        3 => "causeway",
        _ => return None,
    })
}

#[derive(Debug, Clone, PartialEq)]
pub struct SurfaceRun {
    pub from_mi: f64,
    pub to_mi: f64,
    pub surface: i64,
    pub directions: Directions,
    pub iri: Option<f64>,
    pub faulting: Option<f64>,
    /// Read off the inventory carriageway while the leg drives the other
    /// one, which HPMS gives no reading of its own: the section's value.
    pub inventory_side: bool,
    /// The section's build and improvement years, on exposed concrete only
    /// (the groove rule's input; nothing reads them on asphalt).
    pub built: Option<i64>,
    pub improved: Option<i64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StructureRun {
    pub from_mi: f64,
    pub to_mi: f64,
    pub structure: i64,
    pub directions: Directions,
    /// HPMS's own length of the sections behind the run (their milepoint
    /// spans, each section once): read, beside the derived ends.
    pub hpms_mi: Option<f64>,
}

/// Everything one leg's sections say.
#[derive(Debug, Clone, Default)]
pub struct LegPavement {
    pub surface: Vec<SurfaceRun>,
    pub structures: Vec<StructureRun>,
    /// Share of the leg's bins with a surface reading, each way.
    pub read_forward: f64,
    pub read_reverse: f64,
    /// Share of the leg, each way, read off the inventory carriageway for
    /// the other one (`inventory_side`): an assumption, counted apart.
    pub inventory_forward: f64,
    pub inventory_reverse: f64,
    /// Share with any mainline section at all, read or not.
    pub sectioned: f64,
    /// The most common HPMS data year among the readings used.
    pub year: Option<i64>,
    /// Forward miles read, by the functional system of the reading.
    pub class_miles: std::collections::BTreeMap<i64, f64>,
}

#[derive(Clone, Copy)]
enum Item {
    Surface,
    Structure,
}

fn has(s: &Section, item: Item) -> bool {
    match item {
        Item::Surface => s.surface.is_some_and(|c| surface_name(c).is_some()),
        Item::Structure => s.structure.is_some_and(|c| structure_name(c).is_some()),
    }
}

/// Does `p` speak for `forward` travel, given everything else at the spot?
fn applies(
    p: &Placed,
    s: &Section,
    forward: bool,
    here: &[(&Placed, &Section)],
    item: Item,
) -> bool {
    match s.facility {
        1 => p.along == forward,
        6 => p.along != forward,
        _ => {
            let split = here
                .iter()
                .any(|(_, o)| o.facility == 6 && o.f_system == s.f_system && has(o, item));
            !split || p.along == forward
        }
    }
}

/// Runs of equal value along one direction's bins, as `(first, last, value)`.
fn runs<T: PartialEq + Copy>(bins: &[Option<T>]) -> Vec<(usize, usize, T)> {
    let mut out: Vec<(usize, usize, T)> = Vec::new();
    for (i, v) in bins.iter().enumerate() {
        let Some(v) = *v else { continue };
        match out.last_mut() {
            Some(last) if last.1 + 1 == i && last.2 == v => last.1 = i,
            _ => out.push((i, i, v)),
        }
    }
    out
}

/// Split two directions' bins into a shared stream and two one-way streams.
fn split<T: PartialEq + Copy>(
    fwd: &[Option<T>],
    rev: &[Option<T>],
) -> [(Directions, Vec<Option<T>>); 3] {
    let both = fwd
        .iter()
        .zip(rev)
        .map(|(a, b)| if a == b { *a } else { None })
        .collect();
    let only = |mine: &[Option<T>], other: &[Option<T>]| {
        mine.iter()
            .zip(other)
            .map(|(a, b)| if a == b { None } else { *a })
            .collect()
    };
    [
        (Directions::Both, both),
        (Directions::Forward, only(fwd, rev)),
        (Directions::Reverse, only(rev, fwd)),
    ]
}

fn mean(values: impl Iterator<Item = f64>) -> Option<f64> {
    let (sum, n) = values.fold((0.0, 0usize), |(s, n), v| (s + v, n + 1));
    (n > 0).then(|| sum / n as f64)
}

/// Build one leg's runs from its sections.
pub fn assemble(sections: &[Section], placed: &[Placed], leg_miles: f64) -> LegPavement {
    let n = ((leg_miles / BIN_MI).ceil() as usize).max(1);
    let mut cover: Vec<Vec<usize>> = vec![Vec::new(); n];
    for (k, p) in placed.iter().enumerate() {
        let first = (p.lo / BIN_MI - 0.5).ceil().max(0.0) as i64;
        let last = ((p.hi / BIN_MI - 0.5).floor() as i64).min(n as i64 - 1);
        for b in first..=last {
            cover[b as usize].push(k);
        }
    }
    // Per direction (0 forward, 1 reverse): the chosen surface section and
    // structure code in each bin.
    let mut surf: [Vec<Option<(usize, bool)>>; 2] = [vec![None; n], vec![None; n]];
    let mut strc: [Vec<Option<(i64, usize)>>; 2] = [vec![None; n], vec![None; n]];
    let mut sectioned = 0usize;
    for (b, ks) in cover.iter().enumerate() {
        let all: Vec<(&Placed, &Section)> = ks
            .iter()
            .map(|&k| (&placed[k], &sections[placed[k].index]))
            .collect();
        let (wide, here): (Vec<_>, Vec<_>) = all.into_iter().partition(|(p, _)| p.wide);
        if here.is_empty() {
            continue;
        }
        sectioned += 1;
        let top = here.iter().map(|(_, s)| s.f_system).min().unwrap_or(9);
        // The leg's own road: the nearest top-class carriageway row, or the
        // non-inventory side where that is all that runs here (a divided
        // road whose carriageways part, as on the Grapevine).
        let nearest = |own_side: bool| {
            here.iter()
                .filter(|(_, s)| s.f_system == top && (s.facility != 6 || !own_side))
                .min_by(|a, b| a.0.mean_off_m.total_cmp(&b.0.mean_off_m))
                .map(|(_, s)| s.route_id.as_str())
        };
        let far_side = nearest(true).is_none();
        let Some(road) = nearest(true).or_else(|| nearest(false)) else {
            continue;
        };
        // On the far carriageway the inventory row's reading stands for the
        // section, as HPMS reports it, both ways.
        let inventory = far_side
            .then(|| {
                wide.iter()
                    .filter(|(_, s)| s.f_system == top && has(s, Item::Surface))
                    .min_by(|a, b| a.0.mean_off_m.total_cmp(&b.0.mean_off_m))
                    .map(|(p, _)| (p.index, true))
            })
            .flatten();
        let mine = |(_, s): &&(&Placed, &Section)| {
            s.f_system == top && (s.route_id == road || s.facility == 6)
        };
        for (d, forward) in [(0, true), (1, false)] {
            surf[d][b] = here
                .iter()
                .filter(mine)
                .filter(|(p, s)| {
                    has(s, Item::Surface) && applies(p, s, forward, &here, Item::Surface)
                })
                .min_by(|a, b| a.0.mean_off_m.total_cmp(&b.0.mean_off_m))
                .map(|(p, _)| (p.index, false))
                .or(inventory);
            strc[d][b] = here
                .iter()
                .filter(mine)
                .filter(|(p, s)| {
                    has(s, Item::Structure) && applies(p, s, forward, &here, Item::Structure)
                })
                .filter_map(|(p, s)| Some((s.structure?, p.index)))
                .min();
        }
    }
    type Reading = (i64, bool, Option<i64>, Option<i64>);
    let code = |d: usize| -> Vec<Option<Reading>> {
        surf[d]
            .iter()
            .map(|k| {
                k.and_then(|(k, side)| {
                    let s = &sections[k];
                    let code = s.surface?;
                    let (built, improved) = if exposed_concrete(code) {
                        (s.built, s.improved)
                    } else {
                        (None, None)
                    };
                    Some((code, side, built, improved))
                })
            })
            .collect()
    };
    let (fwd_codes, rev_codes) = (code(0), code(1));
    let mut surface = Vec::new();
    for (dirs, stream) in split(&fwd_codes, &rev_codes) {
        let side = if dirs == Directions::Reverse { 1 } else { 0 };
        for (a, z, (value, inventory_side, built, improved)) in runs(&stream) {
            let picks = || {
                surf[side][a..=z]
                    .iter()
                    .flatten()
                    .map(|&(k, _)| &sections[k])
            };
            surface.push(SurfaceRun {
                from_mi: a as f64 * BIN_MI,
                to_mi: ((z + 1) as f64 * BIN_MI).min(leg_miles),
                surface: value,
                inventory_side,
                built,
                improved,
                directions: dirs,
                iri: mean(picks().filter_map(|s| s.iri).filter(|v| *v > 0.0)),
                faulting: mean(picks().filter_map(|s| s.faulting)),
            });
        }
    }
    surface.sort_by(|a, b| a.from_mi.total_cmp(&b.from_mi));
    let kinds = |d: usize| -> Vec<Option<i64>> { strc[d].iter().map(|k| k.map(|k| k.0)).collect() };
    let mut structures = Vec::new();
    for (dirs, stream) in split(&kinds(0), &kinds(1)) {
        let side = if dirs == Directions::Reverse { 1 } else { 0 };
        for (a, z, value) in runs(&stream) {
            structures.push(StructureRun {
                from_mi: a as f64 * BIN_MI,
                to_mi: ((z + 1) as f64 * BIN_MI).min(leg_miles),
                structure: value,
                directions: dirs,
                hpms_mi: hpms_length(sections, strc[side][a..=z].iter().flatten().map(|k| k.1)),
            });
        }
    }
    structures.sort_by(|a, b| a.from_mi.total_cmp(&b.from_mi));
    let mut years: HashMap<i64, usize> = HashMap::new();
    for (k, _) in surf.iter().flatten().flatten() {
        if let Some(y) = sections[*k].year {
            *years.entry(y).or_default() += 1;
        }
    }
    let mut class_miles = std::collections::BTreeMap::new();
    for (k, _) in surf[0].iter().flatten() {
        *class_miles.entry(sections[*k].f_system).or_insert(0.0) += BIN_MI;
    }
    let share =
        |v: &[Option<(usize, bool)>]| v.iter().filter(|x| x.is_some()).count() as f64 / n as f64;
    let borrowed = |v: &[Option<(usize, bool)>]| {
        v.iter().filter(|x| x.is_some_and(|x| x.1)).count() as f64 / n as f64
    };
    LegPavement {
        read_forward: share(&surf[0]),
        read_reverse: share(&surf[1]),
        inventory_forward: borrowed(&surf[0]),
        inventory_reverse: borrowed(&surf[1]),
        sectioned: sectioned as f64 / n as f64,
        year: years
            .into_iter()
            .max_by_key(|(y, c)| (*c, *y))
            .map(|(y, _)| y),
        surface,
        structures,
        class_miles,
    }
}

/// The summed milepoint length of the distinct sections behind a run, or
/// `None` when any lacks its milepoints.
fn hpms_length(sections: &[Section], picks: impl Iterator<Item = usize>) -> Option<f64> {
    let mut seen: Vec<(&str, u64, u64)> = Vec::new();
    let mut total = 0.0;
    for k in picks {
        let s = &sections[k];
        let (a, z) = (s.begin_mi?, s.end_mi?);
        let key = (s.route_id.as_str(), a.to_bits(), z.to_bits());
        if !seen.contains(&key) {
            seen.push(key);
            total += (z - a).abs();
        }
    }
    (!seen.is_empty()).then_some(total)
}

/// Stretches at least `min_mi` long with no surface reading going forward
/// (the reverse side is reported only where it differs), `(from_mi, to_mi)`.
pub fn gaps(p: &LegPavement, leg_miles: f64, reverse: bool, min_mi: f64) -> Vec<(f64, f64)> {
    let mut read: Vec<(f64, f64)> = p
        .surface
        .iter()
        .filter(|r| match r.directions {
            Directions::Both => true,
            Directions::Forward => !reverse,
            Directions::Reverse => reverse,
        })
        .map(|r| (r.from_mi, r.to_mi))
        .collect();
    read.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut out = Vec::new();
    let mut at = 0.0;
    for (a, z) in read.into_iter().chain([(leg_miles, leg_miles)]) {
        if a - at >= min_mi {
            out.push((at, a));
        }
        at = f64::max(at, z);
    }
    out
}

/// Stretches where the surface class changes at least `per_mile` times a
/// mile for `over_mi` or more: the mark of two carriageways' readings
/// interleaved. `(from_mi, to_mi, changes)`.
pub fn flicker(runs: &[SurfaceRun], over_mi: f64, per_mile: f64) -> Vec<(f64, f64, usize)> {
    let mut forward: Vec<&SurfaceRun> = runs
        .iter()
        .filter(|r| r.directions != Directions::Reverse)
        .collect();
    forward.sort_by(|a, b| a.from_mi.total_cmp(&b.from_mi));
    let mut out = Vec::new();
    let mut start = 0;
    // Short runs back to back, each under 1/per_mile long, make a stretch.
    let short = 1.0 / per_mile;
    while start < forward.len() {
        let mut end = start;
        while end < forward.len()
            && forward[end].to_mi - forward[end].from_mi < short
            && (end == start || (forward[end].from_mi - forward[end - 1].to_mi).abs() < 0.011)
        {
            end += 1;
        }
        if end > start {
            let (a, z) = (forward[start].from_mi, forward[end - 1].to_mi);
            if z - a >= over_mi {
                out.push((a, z, end - start));
            }
            start = end;
        } else {
            start += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::osm::geo::LegLine;

    /// A straight north-south road, 1 degree of latitude (~69 mi).
    fn road() -> Route {
        let pts: Vec<(f64, f64)> = (0..=100).map(|i| (30.0 + i as f64 * 0.01, -97.0)).collect();
        Route::new(&LegLine::new(pts), 69.0)
    }

    fn sec(lat0: f64, lat1: f64, lon: f64, facility: i64, surface: Option<i64>) -> Section {
        let steps = 4;
        let path = (0..=steps)
            .map(|i| (lat0 + (lat1 - lat0) * i as f64 / steps as f64, lon))
            .collect();
        Section {
            route_id: "I".into(),
            f_system: 1,
            facility,
            surface,
            paths: vec![path],
            ..Default::default()
        }
    }

    fn build(sections: &[Section], miles: f64) -> LegPavement {
        let route = road();
        let index = RoadIndex::new(&route);
        let placed: Vec<Placed> = sections
            .iter()
            .enumerate()
            .filter_map(|(i, s)| index.place(s, i))
            .collect();
        assemble(sections, &placed, miles)
    }

    #[test]
    fn a_crossing_road_and_a_far_road_are_not_placed() {
        let route = road();
        let index = RoadIndex::new(&route);
        // East-west, 0.1 mi long, centred on the road: near but not along.
        let cross = Section {
            paths: vec![vec![(30.5, -97.0008), (30.5, -96.9992)]],
            ..sec(0.0, 0.0, 0.0, 2, Some(2))
        };
        assert!(index.place(&cross, 0).is_none());
        // Along it but 500 m off.
        assert!(index
            .place(&sec(30.1, 30.2, -96.9948, 2, Some(2)), 0)
            .is_none());
        let on = index
            .place(&sec(30.1, 30.2, -97.0003, 2, Some(2)), 0)
            .unwrap();
        assert!(
            (on.lo - 6.9).abs() < 0.01 && (on.hi - 13.8).abs() < 0.01,
            "{on:?}"
        );
        assert!(on.along);
    }

    #[test]
    fn the_longest_piece_of_a_scattered_feature_is_the_one_placed() {
        let route = road();
        let index = RoadIndex::new(&route);
        let mut s = sec(30.1, 30.101, -97.0, 2, Some(2));
        s.paths.push(vec![(30.3, -97.0), (30.3001, -97.0)]);
        let p = index.place(&s, 0).unwrap();
        assert!(p.hi < 7.0, "{p:?}");
    }

    #[test]
    fn interstate_beats_a_frontage_road_and_runs_merge() {
        let mut frontage = sec(30.0, 30.2, -96.9995, 2, Some(3));
        frontage.f_system = 3;
        frontage.route_id = "FR".into();
        let legs = build(
            &[
                sec(30.0, 30.1, -97.0, 2, Some(7)),
                sec(30.1, 30.2, -97.0, 2, Some(7)),
                frontage,
            ],
            69.0,
        );
        assert_eq!(legs.surface.len(), 1, "{:?}", legs.surface);
        let run = &legs.surface[0];
        assert_eq!((run.surface, run.directions), (7, Directions::Both));
        assert!((run.to_mi - 13.8).abs() < 0.01);
        assert!((legs.read_forward - 0.2).abs() < 0.01);
    }

    #[test]
    fn a_non_inventory_reading_splits_the_directions() {
        // Inventory row digitized north (forward), the non-inventory row
        // also digitized north but holding the southbound side.
        let legs = build(
            &[
                sec(30.0, 30.1, -97.0, 2, Some(3)),
                sec(30.0, 30.1, -97.0004, 6, Some(2)),
            ],
            69.0,
        );
        let dirs: Vec<(i64, Directions)> = legs
            .surface
            .iter()
            .map(|r| (r.surface, r.directions))
            .collect();
        assert_eq!(
            dirs,
            vec![(3, Directions::Forward), (2, Directions::Reverse)]
        );
    }

    #[test]
    fn a_non_inventory_row_without_a_reading_leaves_both_ways_read() {
        let legs = build(
            &[
                sec(30.0, 30.1, -97.0, 2, Some(3)),
                sec(30.0, 30.1, -97.0004, 6, None),
            ],
            69.0,
        );
        assert_eq!(legs.surface.len(), 1);
        assert_eq!(legs.surface[0].directions, Directions::Both);
    }

    #[test]
    fn a_one_way_row_holds_its_own_direction_and_a_bridge_is_a_structure() {
        let mut bridge = sec(30.05, 30.051, -97.0, 2, None);
        bridge.structure = Some(1);
        (bridge.begin_mi, bridge.end_mi) = (Some(3.10), Some(3.17));
        let twin = bridge.clone();
        let legs = build(
            &[
                sec(30.1, 30.0, -97.0, 1, Some(2)), // digitized south: reverse
                bridge,
                twin,
            ],
            69.0,
        );
        assert_eq!(legs.surface[0].directions, Directions::Reverse);
        assert_eq!(legs.read_forward, 0.0);
        let b = &legs.structures[0];
        assert_eq!((b.structure, b.directions), (1, Directions::Both));
        assert!((b.to_mi - b.from_mi - 0.07).abs() < 0.01, "{b:?}");
        // The duplicate row counts once in the read length.
        assert!((b.hpms_mi.unwrap() - 0.07).abs() < 1e-9, "{b:?}");
    }

    #[test]
    fn a_split_carriageway_keeps_its_own_side_and_nothing_is_borrowed() {
        // Only the non-inventory side runs along the road here; its bridge
        // holds for the way it carries, and the inventory side 400 m away
        // is too far to speak for it.
        let mut far = sec(30.0, 30.1, -96.996, 2, Some(3));
        far.route_id = "I-P".into();
        let mut near = sec(30.0, 30.1, -97.0, 6, None);
        near.route_id = "I-S".into();
        let mut bridge = sec(30.05, 30.052, -97.0, 6, None);
        bridge.structure = Some(1);
        let legs = build(&[far.clone(), near.clone(), bridge.clone()], 69.0);
        assert!(legs.surface.is_empty(), "{:?}", legs.surface);
        assert_eq!(legs.structures.len(), 1);
        // 150 m away, the inventory row speaks for the section both ways,
        // marked as such; its bridges are still not borrowed.
        far.paths = sec(30.0, 30.1, -96.9984, 2, None).paths;
        let mut far_bridge = sec(30.02, 30.022, -96.9984, 2, None);
        far_bridge.structure = Some(1);
        let legs = build(&[far, near, bridge, far_bridge], 69.0);
        assert_eq!(legs.surface.len(), 1, "{:?}", legs.surface);
        let run = &legs.surface[0];
        assert!(run.inventory_side && run.directions == Directions::Both);
        assert_eq!(legs.structures.len(), 1);
        assert_eq!(legs.structures[0].directions, Directions::Reverse);
    }

    #[test]
    fn interleaved_classes_are_flagged() {
        let runs: Vec<SurfaceRun> = (0..40)
            .map(|i| SurfaceRun {
                from_mi: i as f64 * 0.1,
                to_mi: (i + 1) as f64 * 0.1,
                surface: if i % 2 == 0 { 2 } else { 7 },
                directions: Directions::Both,
                iri: None,
                faulting: None,
                inventory_side: false,
                built: None,
                improved: None,
            })
            .collect();
        assert_eq!(flicker(&runs, 2.0, 5.0), vec![(0.0, 4.0, 40)]);
        assert!(flicker(&runs[..10], 2.0, 5.0).is_empty());
    }

    #[test]
    fn concrete_runs_split_on_their_years_and_asphalt_keeps_none() {
        let dated = |lat0, lat1, surface, built, improved| Section {
            built,
            improved,
            ..sec(lat0, lat1, -97.0, 2, Some(surface))
        };
        let legs = build(
            &[
                dated(30.0, 30.1, 3, Some(1988), None),
                dated(30.1, 30.2, 3, Some(1988), None),
                dated(30.2, 30.3, 3, Some(1988), Some(2015)),
                dated(30.3, 30.4, 2, Some(1990), None),
                dated(30.4, 30.5, 2, Some(2001), Some(2010)),
            ],
            69.0,
        );
        let got: Vec<(i64, Option<i64>, Option<i64>)> = legs
            .surface
            .iter()
            .map(|r| (r.surface, r.built, r.improved))
            .collect();
        assert_eq!(
            got,
            vec![
                (3, Some(1988), None),
                (3, Some(1988), Some(2015)),
                (2, None, None)
            ]
        );
    }
}
