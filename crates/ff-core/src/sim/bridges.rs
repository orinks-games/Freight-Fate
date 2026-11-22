//! A named bridge under the truck: where it is on the route, whether its
//! name is spoken, and the one state that says whether the truck is on it.
//!
//! The data is `corridor.bridges` (see `data::bridges`). `Trip` resolves
//! each leg's bridges by direction into route miles once per trip
//! ([`RouteBridge`]), decides which names are spoken (the spacing guard and
//! the toll that already named it, [`choose_spoken`]), and moves
//! [`BridgeCrossing`] every tick. That enum is the only place "on a bridge",
//! "over water" and "its name was spoken" live:
//!
//! | from            | event              | to                 | says            |
//! |-----------------|--------------------|--------------------|-----------------|
//! | `Road`          | `Moved` onto `i`   | `On { i }`         | `i`'s line, if spoken and not `last == i` |
//! | `Road`          | `Moved`, no span   | `Road`             | nothing         |
//! | `On { i }`      | `Moved` inside `i` | `On { i }`         | nothing         |
//! | `On { i }`      | `Moved` onto `j`   | `On { j }`         | `j`'s line, if spoken |
//! | `On { i }`      | `Moved`, no span   | `Road { last: i }` | nothing         |
//! | any             | `Placed` at a mile | `On` or `Road`     | nothing (a staged or resumed drive heard it already) |
//!
//! The sound follows the state: the river bed plays while the state is on
//! a bridge over water, and the joints sit at each end of the span (see
//! `Trip::bridge_joint_spans`).

/// One named bridge in route miles, resolved for the trip's direction.
#[derive(Debug, Clone, PartialEq)]
pub struct RouteBridge {
    pub from_mi: f64,
    pub to_mi: f64,
    pub name: String,
    pub water: bool,
    /// "Crossing the X Bridge.", or `None` where the name is not spoken
    /// (a longer bridge within [`SPACING_MI`], or a toll line named it).
    pub line: Option<String>,
}

impl RouteBridge {
    pub fn length_mi(&self) -> f64 {
        self.to_mi - self.from_mi
    }

    fn holds(&self, mile: f64) -> bool {
        self.from_mi <= mile && mile < self.to_mi
    }
}

/// One spoken bridge name in any this many route miles; the longer bridge
/// keeps its name (director, brief 2).
pub const SPACING_MI: f64 = 5.0;
/// A toll line naming the bridge this far before its span (or on it, or
/// just past it) says the name already.
pub const TOLL_NAMED_BEFORE_MI: f64 = 3.0;
pub const TOLL_NAMED_AFTER_MI: f64 = 1.0;
/// The line is spoken only when the truck reaches the span within this of
/// its start: a truck placed on the bridge's far half has passed the start.
pub const SPEAK_WINDOW_MI: f64 = 0.5;

/// A second span of the same name this close is the same crossing.
pub const SAME_NAME_MI: f64 = 15.0;

/// The line for a bridge: "Crossing the Confederation Bridge.", and
/// "Crossing Pont Laviolette." for a name that opens on the French word,
/// which carries its own article.
pub fn crossing_line(name: &str) -> String {
    if name.starts_with("Pont ") || name.starts_with("Pont-") {
        format!("Crossing {name}.")
    } else {
        format!("Crossing the {name}.")
    }
}

/// Decide which bridges speak. `tolls` are `(route mile, toll name)`.
/// Bridges are in mile order.
pub fn choose_spoken(bridges: &mut [RouteBridge], tolls: &[(f64, String)]) {
    for b in bridges.iter_mut() {
        b.line = Some(crossing_line(&b.name));
    }
    // A bridge in two spans under one name (the Thousand Islands Bridge's
    // American and Canadian spans) is named at the first.
    for i in 1..bridges.len() {
        let (before, rest) = bridges.split_at_mut(i);
        let b = &mut rest[0];
        if before
            .iter()
            .any(|p| p.name == b.name && b.from_mi - p.to_mi < SAME_NAME_MI)
        {
            b.line = None;
        }
    }
    // The toll that named it.
    for b in bridges.iter_mut() {
        let name = b.name.to_lowercase();
        let named = tolls.iter().any(|(at, toll)| {
            *at >= b.from_mi - TOLL_NAMED_BEFORE_MI
                && *at <= b.to_mi + TOLL_NAMED_AFTER_MI
                && toll.to_lowercase().contains(&name)
        });
        if named {
            b.line = None;
        }
    }
    // The spacing guard, longest first; the same bridge's other piece never
    // counts against it.
    let mut order: Vec<usize> = (0..bridges.len()).collect();
    order.sort_by(|&a, &b| {
        bridges[b]
            .length_mi()
            .total_cmp(&bridges[a].length_mi())
            .then(a.cmp(&b))
    });
    let mut kept: Vec<usize> = Vec::new();
    for i in order {
        if bridges[i].line.is_none() {
            continue;
        }
        let close = kept.iter().any(|&k| {
            bridges[k].name != bridges[i].name
                && (bridges[k].from_mi - bridges[i].from_mi).abs() < SPACING_MI
        });
        if close {
            bridges[i].line = None;
        } else {
            kept.push(i);
        }
    }
}

/// Whether the truck is on a named bridge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BridgeCrossing {
    /// On the road; `last` is the bridge it last left, so backing onto it
    /// again does not say its name twice.
    #[default]
    Road,
    RoadAfter {
        last: usize,
    },
    /// On bridge `index`; `water` when it spans water.
    On {
        index: usize,
        water: bool,
    },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BridgeEvent {
    /// The truck drove to `at_mi`.
    Moved { at_mi: f64 },
    /// The truck was put at `at_mi` (a staged or resumed drive): the state
    /// follows, and nothing is said.
    Placed { at_mi: f64 },
}

impl BridgeCrossing {
    /// The state after `event`, and the index of the bridge whose line is
    /// due. The only place the state moves.
    pub fn next(self, event: BridgeEvent, bridges: &[RouteBridge]) -> (BridgeCrossing, Option<usize>) {
        let (at, placed) = match event {
            BridgeEvent::Moved { at_mi } => (at_mi, false),
            BridgeEvent::Placed { at_mi } => (at_mi, true),
        };
        let under = bridges.iter().position(|b| b.holds(at));
        let Some(i) = under else {
            return match self {
                BridgeCrossing::On { index, .. } => (BridgeCrossing::RoadAfter { last: index }, None),
                other => (other, None),
            };
        };
        let on = BridgeCrossing::On {
            index: i,
            water: bridges[i].water,
        };
        let speaks = !placed
            && bridges[i].line.is_some()
            && at - bridges[i].from_mi <= SPEAK_WINDOW_MI
            && match self {
                BridgeCrossing::Road => true,
                BridgeCrossing::RoadAfter { last } => last != i,
                BridgeCrossing::On { index, .. } => index != i,
            };
        (on, speaks.then_some(i))
    }

    pub fn over_water(self) -> bool {
        matches!(self, BridgeCrossing::On { water: true, .. })
    }

    pub fn index(self) -> Option<usize> {
        match self {
            BridgeCrossing::On { index, .. } => Some(index),
            _ => None,
        }
    }

    /// The state's name, as the agent server reports it.
    pub fn key(self) -> &'static str {
        match self {
            BridgeCrossing::Road | BridgeCrossing::RoadAfter { .. } => "road",
            BridgeCrossing::On { water: true, .. } => "on_bridge_over_water",
            BridgeCrossing::On { water: false, .. } => "on_bridge",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bridge(from: f64, to: f64, name: &str, water: bool) -> RouteBridge {
        RouteBridge {
            from_mi: from,
            to_mi: to,
            name: name.into(),
            water,
            line: None,
        }
    }

    #[test]
    fn the_transition_table() {
        let mut b = vec![
            bridge(10.0, 18.0, "Confederation Bridge", true),
            bridge(30.0, 30.3, "Petitcodiac River Bridge", false),
        ];
        choose_spoken(&mut b, &[]);
        let moved = |at_mi| BridgeEvent::Moved { at_mi };
        use BridgeCrossing::*;
        let table: &[(BridgeCrossing, BridgeEvent, BridgeCrossing, Option<usize>)] = &[
            (Road, moved(5.0), Road, None),
            (Road, moved(10.1), On { index: 0, water: true }, Some(0)),
            (On { index: 0, water: true }, moved(14.0), On { index: 0, water: true }, None),
            (On { index: 0, water: true }, moved(18.2), RoadAfter { last: 0 }, None),
            (RoadAfter { last: 0 }, moved(17.9), On { index: 0, water: true }, None),
            (RoadAfter { last: 0 }, moved(30.1), On { index: 1, water: false }, Some(1)),
            (Road, BridgeEvent::Placed { at_mi: 10.1 }, On { index: 0, water: true }, None),
            // Dropped past the start: the line belonged to the road behind.
            (Road, moved(13.0), On { index: 0, water: true }, None),
        ];
        for (from, event, to, says) in table {
            assert_eq!(from.next(*event, &b), (*to, *says), "{from:?} {event:?}");
        }
        assert!(On { index: 0, water: true }.over_water());
        assert_eq!(On { index: 1, water: false }.key(), "on_bridge");
    }

    #[test]
    fn the_longer_bridge_keeps_its_name_within_five_miles() {
        let mut b = vec![
            bridge(1.0, 1.4, "Short Bridge", true),
            bridge(3.0, 5.0, "Long Bridge", true),
            bridge(8.5, 8.7, "Far Bridge", false),
        ];
        choose_spoken(&mut b, &[]);
        let spoken: Vec<bool> = b.iter().map(|x| x.line.is_some()).collect();
        assert_eq!(spoken, vec![false, true, true]);
        assert_eq!(b[1].line.as_deref(), Some("Crossing the Long Bridge."));
    }

    #[test]
    fn a_bridge_in_two_spans_is_named_once() {
        let mut b = vec![
            bridge(25.6, 26.5, "Thousand Islands Bridge", true),
            bridge(31.5, 32.2, "Thousand Islands Bridge", true),
        ];
        choose_spoken(&mut b, &[]);
        assert!(b[0].line.is_some() && b[1].line.is_none());
        assert_eq!(crossing_line("Pont Laviolette"), "Crossing Pont Laviolette.");
    }

    #[test]
    fn a_toll_that_named_the_bridge_says_it_once() {
        let mut b = vec![
            bridge(10.0, 11.0, "Blue Water Bridge", true),
            bridge(40.0, 41.0, "Gordie Howe International Bridge", true),
        ];
        choose_spoken(
            &mut b,
            &[
                (8.0, "Blue Water Bridge toll".into()),
                (30.0, "Gordie Howe International Bridge toll".into()),
            ],
        );
        assert!(b[0].line.is_none(), "toll two miles before names it");
        assert!(b[1].line.is_some(), "toll ten miles before is another stretch");
    }
}
