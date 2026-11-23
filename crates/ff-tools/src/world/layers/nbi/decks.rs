//! The matching half of `nbi-decks`: National Bridge Inventory records put
//! on the structures the pavement layer already has, and the deck tags
//! written onto those structures' lines. No network here, so every rule has
//! a unit test.
//!
//! An NBI record is a point, and a point near the road is not proof the
//! road is on that bridge: a road crossing over the leg's highway lands on
//! the line too. A record is kept for a structure only when all hold:
//!
//! * record type 1 (the route is carried ON the structure; the query asks
//!   for nothing else);
//! * its point projects onto the archived road within [`OFF_M`];
//! * it lands inside the structure, or within [`END_MI`] of its ends;
//! * its length (Item 49) agrees with the structure's measured span within
//!   [`LEN_MI`].
//!
//! Where several records fit one structure (twin bridges, one each way),
//! the one whose length fits best wins, then the nearer point. NBI has no
//! field for which carriageway a record is: Item 5E is the route number's
//! own suffix (I-35E), not a travel direction (Recording and Coding Guide,
//! page 4). A shared both-ways line whose two directions' best records
//! disagree on the deck is split into a forward line and a reverse line.

use super::super::hpms::pavement_match::RoadIndex;
use crate::world::osm::geo::M_PER_MI;

/// How far a record's point may sit from the archived road, in metres.
///
/// Calibrated on the six-leg dry run (2026-11-22, 939 miles in Wisconsin
/// and Illinois, California, Texas, Pennsylvania, Florida, the Carolinas):
/// among records landing on a structure, those whose length fits outnumber
/// those that do not by 357 to 150 within 60 m, and are outnumbered 34 to
/// 47 from 60 to 150 m. NBI places a bridge to the nearest hundredth of a
/// second only on the NHS and STRAHNET (Item 16), so a looser point is
/// expected.
pub const OFF_M: f64 = 60.0;

/// How far outside a structure's ends a point may land, in leg miles.
///
/// The archived road is simplified to within 30 m of the real one, which
/// can move a structure's derived end by about 0.02 mi (the reasoning
/// behind the layer's `SPAN_TOLERANCE_MI`). On the six-leg dry run, of the
/// length-fitting records within 40 m of the road, 64 landed 0 to 0.02 mi
/// outside an end and 10 from 0.02 to 0.05.
pub const END_MI: f64 = 0.02;

/// How far the NBI length may sit from the structure's measured span:
/// the layer's own `SPAN_TOLERANCE_MI` for the same derived span against
/// HPMS's length. On the six-leg dry run 365 of 426 records inside a
/// structure and within 40 m of the road agreed within 0.03 mi.
pub const LEN_MI: f64 = 0.05;

/// Item 43B codes for a movable bridge: 15 lift, 16 bascule, 17 swing.
pub fn movable(type_43b: i64) -> bool {
    matches!(type_43b, 15..=17)
}

/// One NBI record, the fields the match reads.
#[derive(Debug, Clone, Default)]
pub struct Record {
    /// `<state FIPS>-<structure number>`, spaces and leading zeros as NBI
    /// writes them, trimmed.
    pub id: String,
    pub lat: f64,
    pub lon: f64,
    /// Item 43B.
    pub type_43b: i64,
    /// Item 45, spans in the main unit.
    pub main_spans: Option<i64>,
    /// Item 48, metres.
    pub max_span_m: Option<f64>,
    /// Item 49, metres.
    pub length_m: Option<f64>,
    /// Item 107 and Item 108A, as NBI codes them.
    pub deck_107: String,
    pub surface_108a: String,
    /// Item 7 and Item 6A, for the report.
    pub carried: String,
    pub over: String,
}

impl Record {
    /// The deck as the layer names it (see `ff_core::data::pavement::Deck`).
    pub fn deck(&self) -> &'static str {
        match (self.deck_107.as_str(), self.surface_108a.as_str()) {
            ("3", _) => "open_grating",
            ("1" | "2", "0" | "1" | "2" | "3" | "4") => "concrete",
            (_, "6") => "asphalt",
            _ => "other",
        }
    }

    pub fn length_mi(&self) -> Option<f64> {
        self.length_m.map(|m| m / M_PER_MI)
    }
}

/// One `structures` line of the pavement layer, split.
#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub from_mi: f64,
    pub to_mi: f64,
    /// The two miles as written, so a rewrite leaves them byte for byte.
    pub miles_text: String,
    pub kind: String,
    /// `both`, `forward` or `reverse`.
    pub ways: &'static str,
    /// Every other tag, in order, without `deck=`, `nbi=` and `grate=`.
    pub tags: Vec<String>,
}

fn drop_ours(t: &str) -> bool {
    ["deck=", "nbi=", "grate="].iter().any(|p| t.starts_with(p))
}

impl Line {
    pub fn parse(text: &str) -> Option<Self> {
        let mut w = text.split_whitespace();
        let (a, z) = (w.next()?, w.next()?);
        let (from_mi, to_mi) = (a.parse().ok()?, z.parse().ok()?);
        let kind = w.next()?.to_string();
        let mut ways = "both";
        let mut tags = Vec::new();
        for t in w {
            match t {
                "forward" => ways = "forward",
                "reverse" => ways = "reverse",
                "both" => ways = "both",
                t if drop_ours(t) => {}
                t => tags.push(t.to_string()),
            }
        }
        Some(Line {
            from_mi,
            to_mi,
            miles_text: format!("{a} {z}"),
            kind,
            ways,
            tags,
        })
    }

    pub fn holds(&self, forward: bool) -> bool {
        match self.ways {
            "forward" => forward,
            "reverse" => !forward,
            _ => true,
        }
    }

    /// The text with `kept`'s tags, the way `hpms-pavement` writes a line.
    pub fn text(&self, ways: &str, kept: Option<&Kept>) -> String {
        let mut out = format!("{} {}", self.miles_text, self.kind);
        if ways != "both" {
            out.push(' ');
            out.push_str(ways);
        }
        for t in &self.tags {
            out.push(' ');
            out.push_str(t);
        }
        if let Some(k) = kept {
            out.push_str(&format!(" deck={} nbi={}", k.deck, k.id));
            if let Some((a, z)) = k.grate {
                out.push_str(&format!(
                    " grate={}:{}",
                    super::super::round(a.max(self.from_mi), 3),
                    super::super::round(z.min(self.to_mi), 3)
                ));
            }
        }
        out
    }
}

/// A record on the road: `(at_mi, off_m)` from the road index.
#[derive(Debug, Clone, Copy)]
pub struct OnRoad {
    pub record: usize,
    pub at_mi: f64,
    pub off_m: f64,
}

pub fn place(index: &RoadIndex, records: &[Record]) -> Vec<OnRoad> {
    records
        .iter()
        .enumerate()
        .filter_map(|(i, r)| {
            let (at_mi, off_m) = index.project(r.lat, r.lon)?;
            Some(OnRoad {
                record: i,
                at_mi,
                off_m,
            })
        })
        .collect()
}

/// A structure as one direction crosses it: touching pieces of one kind
/// joined, the way `Pavement::structures_for` joins them.
#[derive(Debug, Clone)]
pub struct Group {
    pub from_mi: f64,
    pub to_mi: f64,
    pub members: Vec<usize>,
}

pub fn groups(lines: &[Line], forward: bool) -> Vec<Group> {
    let mut order: Vec<usize> = (0..lines.len())
        .filter(|&i| lines[i].holds(forward) && lines[i].kind != "tunnel")
        .collect();
    order.sort_by(|&a, &b| lines[a].from_mi.total_cmp(&lines[b].from_mi));
    let mut out: Vec<Group> = Vec::new();
    for i in order {
        let l = &lines[i];
        match out.last_mut() {
            Some(g) if lines[g.members[0]].kind == l.kind && l.from_mi <= g.to_mi + 0.0051 => {
                g.to_mi = g.to_mi.max(l.to_mi);
                g.members.push(i);
            }
            _ => out.push(Group {
                from_mi: l.from_mi,
                to_mi: l.to_mi,
                members: vec![i],
            }),
        }
    }
    out
}

/// What one direction of one structure kept.
#[derive(Debug, Clone, PartialEq)]
pub struct Kept {
    pub record: usize,
    pub id: String,
    pub deck: &'static str,
    /// The movable span, leg miles, for an open grating on a movable bridge.
    pub grate: Option<(f64, f64)>,
    pub len_err_mi: f64,
    pub off_m: f64,
}

/// Why a record near the road was not kept, for the report.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Reject {
    /// Farther than [`OFF_M`] from the road.
    Far,
    /// On the road but on no structure of the layer (most are roads
    /// crossing over it).
    NoStructure,
    /// On a structure whose span its length does not fit.
    Length,
    /// Fitted, but another record fitted the same structure better.
    Outfitted,
}

impl Reject {
    pub fn name(self) -> &'static str {
        match self {
            Reject::Far => "farther than 60 m from the road",
            Reject::NoStructure => "on no structure of the layer",
            Reject::Length => "length does not fit the structure",
            Reject::Outfitted => "another record fits the structure better",
        }
    }
}

/// The derived movable span: Item 45 spans of Item 48's length, centred
/// on the structure. NBI's point may be the bridge's beginning (Item 16),
/// so it does not mark the centre; a movable span over a channel usually
/// sits mid-structure. `scale` is leg miles per measured mile.
pub fn grate_span(r: &Record, g: &Group, scale: f64) -> Option<(f64, f64)> {
    if r.deck() != "open_grating" || !movable(r.type_43b) {
        return None;
    }
    let spans = r.main_spans.filter(|n| *n > 0)? as f64;
    let len = spans * r.max_span_m.filter(|m| *m > 0.0)? / M_PER_MI * scale;
    let mid = (g.from_mi + g.to_mi) / 2.0;
    let (a, z) = (
        (mid - len / 2.0).max(g.from_mi),
        (mid + len / 2.0).min(g.to_mi),
    );
    (z > a).then_some((a, z))
}

/// One direction's outcome: the kept record per group, and every placed
/// record's fate.
#[derive(Debug, Default)]
pub struct Matched {
    pub kept: Vec<(Group, Option<Kept>)>,
    /// Per placed record (same order as the input): `None` when it was kept
    /// for some structure.
    pub fate: Vec<Option<Reject>>,
    /// Groups more than one distinct record fitted.
    pub ties: usize,
    /// Of those, ties whose fitting records disagree on the deck.
    pub tie_disagree: usize,
}

pub fn match_direction(
    lines: &[Line],
    forward: bool,
    on_road: &[OnRoad],
    records: &[Record],
    scale: f64,
) -> Matched {
    let mut out = Matched {
        fate: vec![Some(Reject::Far); on_road.len()],
        ..Default::default()
    };
    for (i, p) in on_road.iter().enumerate() {
        if p.off_m <= OFF_M {
            out.fate[i] = Some(Reject::NoStructure);
        }
    }
    for g in groups(lines, forward) {
        let span = (g.to_mi - g.from_mi) / scale;
        let mut fits: Vec<(usize, f64)> = Vec::new();
        for (i, p) in on_road.iter().enumerate() {
            let near =
                p.off_m <= OFF_M && p.at_mi >= g.from_mi - END_MI && p.at_mi <= g.to_mi + END_MI;
            if !near {
                continue;
            }
            let err = records[p.record]
                .length_mi()
                .map_or(f64::INFINITY, |l| (l - span).abs());
            if err <= LEN_MI {
                fits.push((i, err));
            } else if out.fate[i] == Some(Reject::NoStructure) {
                out.fate[i] = Some(Reject::Length);
            }
        }
        fits.sort_by(|a, b| {
            a.1.total_cmp(&b.1)
                .then(on_road[a.0].off_m.total_cmp(&on_road[b.0].off_m))
        });
        let mut ids: Vec<&str> = fits
            .iter()
            .map(|(i, _)| records[on_road[*i].record].id.as_str())
            .collect();
        ids.sort_unstable();
        ids.dedup();
        if ids.len() > 1 {
            out.ties += 1;
            let mut decks: Vec<&str> = fits
                .iter()
                .map(|(i, _)| records[on_road[*i].record].deck())
                .collect();
            decks.sort_unstable();
            decks.dedup();
            if decks.len() > 1 {
                out.tie_disagree += 1;
            }
        }
        for (k, (i, _)) in fits.iter().enumerate() {
            if k == 0 {
                out.fate[*i] = None;
            } else if out.fate[*i].is_some() {
                out.fate[*i] = Some(Reject::Outfitted);
            }
        }
        let kept = fits.first().map(|&(i, err)| {
            let p = on_road[i];
            let r = &records[p.record];
            Kept {
                record: p.record,
                id: r.id.clone(),
                deck: r.deck(),
                grate: grate_span(r, &g, scale),
                len_err_mi: err,
                off_m: p.off_m,
            }
        });
        out.kept.push((g, kept));
    }
    out
}

/// The leg's new structure lines from both directions' matches, and how
/// many shared lines were split because their two directions disagreed.
pub fn rewrite(lines: &[Line], fwd: &Matched, rev: &Matched) -> (Vec<String>, usize) {
    let kept_for = |m: &Matched, i: usize| -> Option<Kept> {
        m.kept
            .iter()
            .find(|(g, _)| g.members.contains(&i))
            .and_then(|(_, k)| k.clone())
    };
    let mut out = Vec::new();
    let mut split = 0;
    for (i, l) in lines.iter().enumerate() {
        if l.kind == "tunnel" {
            out.push(l.text(l.ways, None));
            continue;
        }
        match l.ways {
            "forward" => out.push(l.text("forward", kept_for(fwd, i).as_ref())),
            "reverse" => out.push(l.text("reverse", kept_for(rev, i).as_ref())),
            _ => {
                let (f, r) = (kept_for(fwd, i), kept_for(rev, i));
                let agree = match (&f, &r) {
                    (Some(a), Some(b)) => a.deck == b.deck && a.grate == b.grate,
                    (None, None) => true,
                    _ => false,
                };
                if agree {
                    // The better fit of the two speaks for both ways.
                    let best = match (f, r) {
                        (Some(a), Some(b)) => {
                            Some(if (b.len_err_mi, b.off_m) < (a.len_err_mi, a.off_m) {
                                b
                            } else {
                                a
                            })
                        }
                        _ => None,
                    };
                    out.push(l.text("both", best.as_ref()));
                } else {
                    split += 1;
                    out.push(l.text("forward", f.as_ref()));
                    out.push(l.text("reverse", r.as_ref()));
                }
            }
        }
    }
    (out, split)
}

#[cfg(test)]
mod tests;
