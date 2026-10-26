//! What one exit's ramps do, per direction of travel: where they end, what
//! stands there, how long they are and what speed they post.
//!
//! The candidate gores are every gore within [`FAR_END_M`] of ANY of the
//! exit's junction nodes. The Python bake searched around one junction
//! node, the one its exit number matched first, so on an exit whose two
//! carriageways diverge at junctions far apart only that carriageway's
//! ramp was found: 3,456 of 18,165 exits had a ramp end both ways. The
//! exit's own nodes, one per carriageway, are the sibling search.

use std::collections::HashSet;

use super::discover::{gores_near, heading, ExitNodes, Snapper};
use super::topo::Topo;

/// A gore this close to an exit junction is one of its ramps. The gore
/// sits at the junction itself, so this is tight by design.
pub const FAR_END_M: f64 = 500.0;
/// A signal mapped per approach sits within a few dozen metres of the
/// intersection it controls; 120 m keeps a wide arterial's far-side heads
/// in range and stays far tighter than the exit-wide radius.
pub const TERMINAL_CONTROL_M: f64 = 120.0;
/// The exit-wide fallback: a control node on any ramp way this close to
/// the exit's junctions. The terminal stands at the ramp's far end,
/// routinely 300 m to 2 km from the junction.
pub const WIDE_CONTROL_M: f64 = 1400.0;
/// A ramp shorter than this is too short to hold even the Green Book 2018
/// Table 10-6 taper, so its "gore" is a mapping split, not a diverge.
pub const RAMP_LENGTH_MIN_FT: f64 = 300.0;
/// A ramp longer than this has walked onto a collector road or a
/// neighbour's chain.
pub const RAMP_LENGTH_MAX_FT: f64 = 1.5 * 5280.0;
pub const M_TO_FT: f64 = 3.28084;

/// Which reading set the control.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlRead {
    /// Tagged within [`TERMINAL_CONTROL_M`] of a walked terminal.
    Terminal,
    /// A control on a ramp way near the exit (no terminal reading).
    Wide,
    /// `none`, derived from an all-motorway far end.
    Merge,
}

/// One direction of travel's ramp.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Direction {
    pub length_ft: Option<f64>,
    /// `(node, lat, lon)` where the ramp meets a public road.
    pub terminal: Option<(i64, f64, f64)>,
    pub advisory_mph: Option<f64>,
}

#[derive(Debug, Clone, Default)]
pub struct RampFacts {
    pub control: Option<(&'static str, ControlRead)>,
    pub far_end: Option<&'static str>,
    pub forward: Direction,
    pub backward: Direction,
}

/// The run's accounting, printed and kept in the layer meta.
#[derive(Debug, Default, Clone, Copy)]
pub struct RampCounts {
    pub exits: usize,
    pub no_gore: usize,
    pub far_motorway: usize,
    pub far_surface: usize,
    pub terminal_reads: usize,
    pub wide_reads: usize,
    pub none_derived: usize,
    pub contradictions: usize,
    pub yieldish: usize,
    pub gores_measured: usize,
    pub too_short: usize,
    pub too_long: usize,
    pub unmeasured: usize,
    pub both_terminals: usize,
    pub one_terminal: usize,
}

fn screen_length(ft: Option<f64>, counts: &mut RampCounts) -> Option<f64> {
    match ft {
        None => {
            counts.unmeasured += 1;
            None
        }
        Some(ft) if ft < RAMP_LENGTH_MIN_FT => {
            counts.too_short += 1;
            None
        }
        Some(ft) if ft > RAMP_LENGTH_MAX_FT => {
            counts.too_long += 1;
            None
        }
        Some(ft) => Some(ft),
    }
}

/// Which way of travel a gore's ramp serves: true for the leg's own
/// direction (A to B). The ramp's first edge against the road's heading
/// there; None when either is unknown.
fn gore_forward(topo: &Topo, gore: i64, snapper: &Snapper) -> Option<bool> {
    let g = *topo.locs.get(&gore)?;
    let road = snapper.snap(g.0, g.1)?.heading;
    let ramp = topo
        .out
        .get(&gore)?
        .iter()
        .find_map(|e| topo.locs.get(&e.to).and_then(|n| heading(g, *n)))?;
    Some(road.0 * ramp.0 + road.1 * ramp.1 >= 0.0)
}

/// Read every ramp fact for one exit.
pub fn ramp_facts(
    topo: &Topo,
    exit: &ExitNodes,
    snapper: &Snapper,
    counts: &mut RampCounts,
) -> RampFacts {
    let mut facts = RampFacts::default();
    counts.exits += 1;
    let gores = gores_near(topo, exit, FAR_END_M);
    if gores.is_empty() {
        counts.no_gore += 1;
    }

    // The far end: one chain at a surface road makes the interchange a
    // surface one, whatever its other ramps do; a merge needs every gore
    // in range to walk clean.
    let mut verdicts: HashSet<Option<&'static str>> = HashSet::new();
    let mut terminals: HashSet<i64> = HashSet::new();
    for (_, gore) in &gores {
        let walk = topo.walk_far_ends(*gore);
        verdicts.insert(walk.verdict());
        terminals.extend(walk.terminals);
    }
    let far_end = if verdicts.contains(&Some("surface")) {
        Some("surface")
    } else if !verdicts.is_empty() && verdicts.iter().all(|v| *v == Some("motorway")) {
        Some("motorway")
    } else {
        None
    };

    // The control, best evidence first. Within a terminal: signal, then
    // roundabout, then stop, then yield (a give-way at a roundabout entry is
    // the roundabout's own furniture; a signalized one is worked as its
    // light).
    let kinds = topo.controls_at(&terminals, TERMINAL_CONTROL_M);
    if kinds.contains("give_way") || kinds.contains("roundabout") {
        counts.yieldish += 1;
    }
    let precise = ["signal", "roundabout", "stop", "give_way"]
        .into_iter()
        .find(|k| kinds.contains(k))
        .map(|k| if k == "give_way" { "yield" } else { k });
    if let Some(kind) = precise {
        facts.control = Some((kind, ControlRead::Terminal));
        counts.terminal_reads += 1;
    } else if far_end != Some("motorway") {
        let mut wide: HashSet<&'static str> = HashSet::new();
        for (_, lat, lon) in &exit.nodes {
            wide.extend(
                topo.link_controls
                    .near(*lat, *lon, WIDE_CONTROL_M)
                    .into_iter()
                    .map(|(_, k)| k),
            );
        }
        if let Some(kind) = ["signal", "stop"].into_iter().find(|k| wide.contains(k)) {
            facts.control = Some((kind, ControlRead::Wide));
            counts.wide_reads += 1;
        }
    }
    match far_end {
        Some("motorway")
            if facts
                .control
                .is_some_and(|(k, _)| k == "signal" || k == "stop") =>
        {
            // A control read AT a terminal of a proven merge: trust the
            // reading, bake no far end, count it.
            counts.contradictions += 1;
        }
        Some(end) => {
            facts.far_end = Some(end);
            if end == "motorway" {
                counts.far_motorway += 1;
                if facts.control.is_none() {
                    facts.control = Some(("none", ControlRead::Merge));
                    counts.none_derived += 1;
                }
            } else {
                counts.far_surface += 1;
            }
        }
        None => {}
    }

    // Per direction: the gore nearest the exit's junctions on that side.
    let mut nearest: [Option<i64>; 2] = [None, None];
    for (_, gore) in &gores {
        if let Some(forward) = gore_forward(topo, *gore, snapper) {
            let slot = &mut nearest[usize::from(forward)];
            if slot.is_none() {
                *slot = Some(*gore);
            }
        }
    }
    for (forward, gore) in [(false, nearest[0]), (true, nearest[1])] {
        let Some(gore) = gore else {
            continue;
        };
        counts.gores_measured += 1;
        let end = topo.ramp_end(gore);
        let mut dir = Direction::default();
        if let Some(end) = &end {
            dir.advisory_mph = end
                .ways
                .iter()
                .filter_map(|(w, along)| {
                    let link = &topo.links[*w as usize];
                    if *along {
                        link.advisory_along
                    } else {
                        link.advisory_against
                    }
                })
                .min_by(f64::total_cmp);
        }
        dir.length_ft = screen_length(end.as_ref().map(|e| e.metres * M_TO_FT), counts);
        if dir.length_ft.is_some() {
            // Only a surface end whose walk the length screen trusts: a
            // merge has no street to hand over to.
            if let Some(node) = end.and_then(|e| e.terminal) {
                if let Some((lat, lon)) = topo.locs.get(&node) {
                    dir.terminal = Some((node, *lat, *lon));
                }
            }
        }
        if forward {
            facts.forward = dir;
        } else {
            facts.backward = dir;
        }
    }
    match (
        facts.forward.terminal.is_some(),
        facts.backward.terminal.is_some(),
    ) {
        (true, true) => counts.both_terminals += 1,
        (false, false) => {}
        _ => counts.one_terminal += 1,
    }
    facts
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::osm::exits::read::{LinkWay, RawNetwork};
    use crate::world::osm::exits::topo::Oneway;
    use crate::world::osm::geo::LegLine;

    fn link(refs: &[i64]) -> LinkWay {
        LinkWay {
            refs: refs.to_vec(),
            oneway: Oneway::Forward,
            ..LinkWay::default()
        }
    }

    /// A north-south leg with a split interchange: the northbound off-ramp
    /// leaves at gore 2 (lat 30.500) and the southbound one at gore 6, 1.3
    /// km north (lat 30.512), each at its own junction node. Both end at a
    /// crossroad. A search round the northbound junction alone finds one.
    #[test]
    fn a_split_interchange_gets_both_ramp_ends() {
        let mut net = RawNetwork {
            links: vec![link(&[2, 10, 11]), link(&[6, 20, 21])],
            ..RawNetwork::default()
        };
        net.mainline.extend([2, 6]);
        net.crossroad.extend([11, 21]);
        let east = -97.0;
        net.locs.insert(2, (30.500, east));
        net.locs.insert(10, (30.502, east + 0.002));
        net.locs.insert(11, (30.504, east + 0.004));
        net.locs.insert(6, (30.512, east - 0.0002));
        net.locs.insert(20, (30.510, east - 0.002));
        net.locs.insert(21, (30.508, east - 0.004));
        let topo = Topo::build(net);
        let line = LegLine::new(vec![(30.0, east), (31.0, east)]);
        let snapper = Snapper::new(&line, 69.0);
        let mut counts = RampCounts::default();
        let one = ExitNodes {
            at_mi: 34.5,
            exit_ref: "34".into(),
            name: String::new(),
            nodes: vec![(2, 30.500, east)],
        };
        let facts = ramp_facts(&topo, &one, &snapper, &mut counts);
        assert!(facts.forward.terminal.is_some());
        assert!(facts.backward.terminal.is_none());
        let both = ExitNodes {
            nodes: vec![(2, 30.500, east), (6, 30.512, east - 0.0002)],
            ..one
        };
        let facts = ramp_facts(&topo, &both, &snapper, &mut counts);
        assert_eq!(facts.forward.terminal.map(|t| t.0), Some(11));
        assert_eq!(facts.backward.terminal.map(|t| t.0), Some(21));
        assert_eq!(facts.far_end, Some("surface"));
        assert_eq!(counts.both_terminals, 1);
    }
}
