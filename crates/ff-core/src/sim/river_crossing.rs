//! The water under the truck and the river's name, in one state.
//!
//! A player heard the river under the truck with no name at 11 of 18
//! crossings (2026-11-27): the water played at every river, but the name was
//! thinned like scenery, by the chatter budget and the town spacing. The
//! ruling is one water sound, one name. So the name is no longer a roadside
//! callout of its own: it comes from the same transition that starts the
//! water, and the two cannot disagree.
//!
//! `Trip` resolves the route's rivers once ([`RouteRiver`]) and moves
//! [`RiverCrossing`] every tick. Its `water` is the [`Water`] state the
//! river bed follows; `river` is the river that water is for.
//!
//! | from                  | event                       | to                    | says |
//! |-----------------------|-----------------------------|-----------------------|------|
//! | `Away`                | `Tick` near river `i`       | `Over`, river `i`     | `i`'s name, unless a named bridge covers `i` |
//! | `Away`                | `Tick` on a water bridge    | `Over`, no river      | nothing (the bridge says its own line) |
//! | `Away`                | `Tick`, near nothing        | `Away`                | nothing |
//! | `Over`, river `i`     | `Tick` near `i`             | `Over`, river `i`     | nothing |
//! | `Over`, river `i`     | `Tick` near river `j`       | `Over`, river `j`     | nothing: the water runs on as one sound |
//! | `Over`                | `Tick`, hold runs out       | `Away`                | nothing |
//! | any                   | `Placed`                    | `Over` or `Away`      | nothing (a staged or resumed drive) |
//!
//! Every river whose name is not said is reported as [`RiverCall::Withheld`]
//! with the reason, so a log settles the next report.

use crate::sim::outside::{Water, WaterEvent, RIVER_HOLD_S, RIVER_SPAN_MI};

/// One river on the route in route miles, resolved for the trip's direction.
#[derive(Debug, Clone, PartialEq)]
pub struct RouteRiver {
    pub at_mi: f64,
    /// The river's name as the map writes it ("Chalk River").
    pub name: String,
    /// The line: the landmark's own words with a full stop ("Chalk River.").
    pub spoken: String,
    /// A named bridge spans this crossing, so the bridge's line (or the
    /// toll line that named it) is the crossing's one name.
    pub under_bridge: bool,
}

/// Why a river's name was not said at its water.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Withheld {
    /// A named bridge spans the crossing.
    Bridge,
    /// The water from the river before it was still playing.
    MergedWater,
}

impl Withheld {
    /// The reason's key, as the debug log writes it: `named_bridge` (a
    /// named bridge covers the crossing) or `merged_water` (its water ran on
    /// from the river before).
    pub fn key(self) -> &'static str {
        match self {
            Withheld::Bridge => "named_bridge",
            Withheld::MergedWater => "merged_water",
        }
    }
}

/// What a transition asks for, by index into the route's rivers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RiverCall {
    Say(usize),
    Withheld(usize, Withheld),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RiverEvent {
    /// Real time passed with the truck at `at_mi`; `water_bridge` is whether
    /// it is on a named bridge over water, which is water its whole length.
    Tick {
        dt_real_s: f64,
        at_mi: f64,
        water_bridge: bool,
    },
    /// The truck was put at `at_mi`: the state follows and nothing is said.
    Placed { at_mi: f64, water_bridge: bool },
}

/// The water under the truck, the river it is for, and the last river named.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct RiverCrossing {
    pub water: Water,
    /// The river the water playing now (or last) was for.
    pub river: Option<usize>,
    /// The last river whose name was said.
    pub last_named: Option<usize>,
}

/// The river within [`RIVER_SPAN_MI`] of `at_mi`, nearest first. `rivers`
/// is sorted by mile.
pub fn river_near(rivers: &[RouteRiver], at_mi: f64) -> Option<usize> {
    let start = rivers.partition_point(|r| r.at_mi < at_mi - RIVER_SPAN_MI);
    rivers[start..]
        .iter()
        .enumerate()
        .take_while(|(_, r)| r.at_mi <= at_mi + RIVER_SPAN_MI)
        .min_by(|(_, a), (_, b)| (a.at_mi - at_mi).abs().total_cmp(&(b.at_mi - at_mi).abs()))
        .map(|(i, _)| start + i)
}

impl RiverCrossing {
    /// The state after `event`, and what it asks for. The only place the
    /// water and the river's name move.
    pub fn next(
        self,
        event: RiverEvent,
        rivers: &[RouteRiver],
    ) -> (RiverCrossing, Option<RiverCall>) {
        match event {
            RiverEvent::Placed {
                at_mi,
                water_bridge,
            } => {
                let near = river_near(rivers, at_mi);
                let water = if near.is_some() || water_bridge {
                    Water::Over {
                        hold_s: RIVER_HOLD_S,
                    }
                } else {
                    Water::Away
                };
                let state = RiverCrossing {
                    water,
                    river: near.or(self.river),
                    last_named: self.last_named,
                };
                (state, None)
            }
            RiverEvent::Tick {
                dt_real_s,
                at_mi,
                water_bridge,
            } => {
                let near = river_near(rivers, at_mi);
                let water = self.water.next(WaterEvent::Tick {
                    dt_real_s,
                    near: near.is_some() || water_bridge,
                });
                let mut state = RiverCrossing { water, ..self };
                let Some(i) = near else {
                    return (state, None);
                };
                if self.river == Some(i) {
                    return (state, None);
                }
                state.river = Some(i);
                let call = if rivers[i].under_bridge {
                    RiverCall::Withheld(i, Withheld::Bridge)
                } else if self.water.is_over() {
                    RiverCall::Withheld(i, Withheld::MergedWater)
                } else {
                    state.last_named = Some(i);
                    RiverCall::Say(i)
                };
                (state, Some(call))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn river(at: f64, name: &str, under_bridge: bool) -> RouteRiver {
        RouteRiver {
            at_mi: at,
            name: name.into(),
            spoken: format!("{name}."),
            under_bridge,
        }
    }

    fn tick(dt: f64, at: f64) -> RiverEvent {
        RiverEvent::Tick {
            dt_real_s: dt,
            at_mi: at,
            water_bridge: false,
        }
    }

    #[test]
    fn the_transition_table() {
        let rivers = vec![
            river(10.0, "Chalk River", false),
            river(10.5, "Indian River", false),
            river(20.0, "Saint Lawrence River", true),
            river(30.0, "Seine River", false),
        ];
        let s = RiverCrossing::default();

        // Away, near nothing: nothing.
        let (s, call) = s.next(tick(0.1, 5.0), &rivers);
        assert_eq!(s.water, Water::Away);
        assert_eq!(call, None);

        // Away to Over at a river: its name.
        let (s, call) = s.next(tick(0.1, 10.0), &rivers);
        assert!(s.water.is_over());
        assert_eq!(call, Some(RiverCall::Say(0)));
        assert_eq!(s.last_named, Some(0));

        // Over to Over at the same river: nothing.
        let (s, call) = s.next(tick(0.1, 10.1), &rivers);
        assert!(s.water.is_over());
        assert_eq!(call, None);

        // A second river inside the first's hold: one water, the first name.
        let (s, call) = s.next(tick(1.0, 10.5), &rivers);
        assert!(s.water.is_over());
        assert_eq!(call, Some(RiverCall::Withheld(1, Withheld::MergedWater)));
        assert_eq!(s.last_named, Some(0));
        let (s, call) = s.next(tick(0.1, 10.55), &rivers);
        assert_eq!(call, None, "reported once");

        // The hold runs out: Away, nothing said.
        let (s, call) = s.next(tick(RIVER_HOLD_S + 0.1, 12.0), &rivers);
        assert_eq!(s.water, Water::Away);
        assert_eq!(call, None);

        // A river a named bridge spans: the water, and the bridge's line.
        let (s, call) = s.next(tick(0.1, 20.0), &rivers);
        assert!(s.water.is_over());
        assert_eq!(call, Some(RiverCall::Withheld(2, Withheld::Bridge)));
        let (s, _) = s.next(tick(RIVER_HOLD_S + 0.1, 22.0), &rivers);
        assert_eq!(s.water, Water::Away);

        // A water bridge with no river point: water, no river name.
        let (s, call) = s.next(
            RiverEvent::Tick {
                dt_real_s: 0.1,
                at_mi: 25.0,
                water_bridge: true,
            },
            &rivers,
        );
        assert!(s.water.is_over());
        assert_eq!(call, None);
        let (s, _) = s.next(tick(RIVER_HOLD_S + 0.1, 26.0), &rivers);

        // Placed on a river: the water, no name.
        let (s, call) = s.next(
            RiverEvent::Placed {
                at_mi: 30.0,
                water_bridge: false,
            },
            &rivers,
        );
        assert!(s.water.is_over());
        assert_eq!(call, None);
        assert_eq!(s.river, Some(3));
        // And the next tick on it says nothing either.
        let (_, call) = s.next(tick(0.1, 30.05), &rivers);
        assert_eq!(call, None);
    }

    #[test]
    fn two_separate_water_sounds_get_two_names() {
        let rivers = vec![
            river(10.0, "Muskrat River", false),
            river(13.0, "Petawawa River", false),
        ];
        let s = RiverCrossing::default();
        let (s, a) = s.next(tick(0.1, 10.0), &rivers);
        let (s, _) = s.next(tick(RIVER_HOLD_S + 0.1, 12.0), &rivers);
        let (_, b) = s.next(tick(0.1, 13.0), &rivers);
        assert_eq!((a, b), (Some(RiverCall::Say(0)), Some(RiverCall::Say(1))));
    }

    #[test]
    fn the_nearest_river_in_the_span_is_the_one_under_the_truck() {
        let rivers = vec![river(10.0, "A", false), river(10.2, "B", false)];
        assert_eq!(river_near(&rivers, 10.05), Some(0));
        assert_eq!(river_near(&rivers, 10.15), Some(1));
        assert_eq!(river_near(&rivers, 10.5), None);
        assert_eq!(river_near(&[], 10.0), None);
    }
}
