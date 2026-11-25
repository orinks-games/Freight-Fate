//! Chain controls: whether a chain law is posted, and how the truck's chains
//! stand against the road under them.
//!
//! A player was told to chain up under a cloudy sky, then the chains broke
//! on dry ground and damaged the truck (player report, 2026-11-21). The law
//! was `max(sky, alerts)`, so a Winter Storm Watch posted Level 1 a day
//! before any snow, and nothing said when the law lifted or when the chains
//! were grinding on bare road.
//!
//! Two enums, each with one `next`:
//!
//! - [`ChainControl`] is the law. A real chain control is posted on road
//!   conditions and comes down when the road clears (Caltrans R-1 to R-3;
//!   CDOT posts Level 1 "when snow is covering any part of the traveled
//!   portion of pavement on an ascending grade"). So here: snow on the road
//!   posts Level 1, ice posts Level 2, and a bare road (dry or wet) posts
//!   nothing, whatever the alerts say. A live blizzard, ice storm or
//!   freezing rain warning only raises a law the road already posted, from
//!   1 to 2; a watch raises nothing.
//! - [`ChainSet`] is the chains against the road. It emits one cue,
//!   [`ChainSetCue::BareRoad`], once chains on have run
//!   [`CHAIN_OFF_BARE_MI`] of bare road, and re-arms only after the road
//!   returns to snow or ice or the chains come off. Past
//!   [`CHAIN_BARE_GRACE_MI`] it reads `on_bare_wearing`: the set wears at
//!   the bare rate from there.
//!
//! `truck.chains_on` and `truck.chains_just_snapped` stay the physics'
//! inputs (wear, grip, the snap), and `truck.chain_bare_mi` is the bare
//! stretch's one counter: the physics keeps it, `ChainSet` reads it.

/// Bare road a chained truck runs before it is told to take them off.
pub const CHAIN_OFF_BARE_MI: f64 = 0.5;

pub use crate::sim::vehicle::CHAIN_BARE_GRACE_MI;

/// What the road says at the truck: its surface ("dry", "wet", "snow",
/// "ice") and whether a live warning calls for chains on every truck.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RoadReading<'a> {
    pub surface: &'a str,
    /// A live blizzard, ice storm or freezing rain warning
    /// (`real_weather_alerts::alerts_raise_chain_law`).
    pub alert_raises: bool,
}

/// Snow or ice on the road: the only thing that posts a chain law, and the
/// only road chains are made for.
pub fn snow_or_ice(surface: &str) -> bool {
    matches!(surface, "snow" | "ice")
}

/// The law a road reading posts: 0 on a bare road, 1 on snow, 2 on ice or
/// on snow under a warning that chains every truck.
pub fn posted_level(reading: RoadReading<'_>) -> i64 {
    match reading.surface {
        "ice" => 2,
        "snow" if reading.alert_raises => 2,
        "snow" => 1,
        _ => 0,
    }
}

/// Whether a chain law is posted.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ChainControl {
    /// No law: the road is bare.
    #[default]
    Clear,
    /// A law is posted at `level` (1 or 2).
    Posted { level: i64 },
}

/// How the law moved on a reading.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChainControlChange {
    /// A law went up from clear.
    Posted(i64),
    /// Level 1 went to 2.
    Raised(i64),
    /// Level 2 went to 1.
    Lowered(i64),
    /// The road cleared and the law came down.
    Lifted,
}

impl ChainControl {
    /// The state a reading alone puts the law in: a resumed save or a
    /// fresh trip starts here.
    pub fn from_reading(reading: RoadReading<'_>) -> ChainControl {
        match posted_level(reading) {
            0 => ChainControl::Clear,
            level => ChainControl::Posted { level },
        }
    }

    /// The one place the law moves.
    pub fn next(self, reading: RoadReading<'_>) -> (ChainControl, Option<ChainControlChange>) {
        let to = ChainControl::from_reading(reading);
        let change = match (self, to) {
            (ChainControl::Clear, ChainControl::Posted { level }) => {
                Some(ChainControlChange::Posted(level))
            }
            (ChainControl::Posted { .. }, ChainControl::Clear) => Some(ChainControlChange::Lifted),
            (ChainControl::Posted { level: from }, ChainControl::Posted { level })
                if level > from =>
            {
                Some(ChainControlChange::Raised(level))
            }
            (ChainControl::Posted { level: from }, ChainControl::Posted { level })
                if level < from =>
            {
                Some(ChainControlChange::Lowered(level))
            }
            _ => None,
        };
        (to, change)
    }

    /// 0 = no law, 1 = Level 1, 2 = Level 2.
    pub fn level(self) -> i64 {
        match self {
            ChainControl::Clear => 0,
            ChainControl::Posted { level } => level,
        }
    }

    /// The name the agent server's `status` reports.
    pub fn key(self) -> &'static str {
        match self.level() {
            0 => "clear",
            1 => "level_1",
            _ => "level_2",
        }
    }
}

/// Where the truck's chains stand against the road.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ChainSet {
    /// In the side box (or none aboard).
    #[default]
    Stowed,
    /// On, over snow or ice.
    OnSnow,
    /// On, over bare road. `cued` once the bare-road cue went out for this
    /// stretch; `wearing` once the stretch has run past its grace
    /// (`CHAIN_BARE_GRACE_MI`) and the set wears at the bare rate. The miles
    /// are the truck's (`TruckState::chain_bare_mi`), the stretch's one
    /// owner; this state only reads them.
    OnBare { cued: bool, wearing: bool },
    /// Worn through and off the wheels; a repair link puts them back on.
    Broken,
}

/// What moves the chains.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ChainSetEvent {
    Installed {
        surface_snow: bool,
    },
    Removed,
    /// The road under the truck: snow or ice, or bare, with the miles the
    /// chains have run on this bare stretch (`TruckState::chain_bare_mi`).
    Road {
        snow: bool,
        bare_mi: f64,
    },
    WornThrough,
    Repaired {
        surface_snow: bool,
    },
}

/// The one cue the chains give.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChainSetCue {
    /// Chains on have run `CHAIN_OFF_BARE_MI` of bare road.
    BareRoad,
}

impl ChainSet {
    /// The state a resumed drive rebuilds from what it stores: the chains
    /// on or not, and the road under them now. A bare stretch starts its
    /// grace again, since its miles are not saved.
    pub fn from_truck(chains_on: bool, surface_snow: bool) -> ChainSet {
        Self::on(chains_on, surface_snow)
    }

    fn on(chains_on: bool, surface_snow: bool) -> ChainSet {
        match (chains_on, surface_snow) {
            (false, _) => ChainSet::Stowed,
            (true, true) => ChainSet::OnSnow,
            (true, false) => ChainSet::OnBare {
                cued: false,
                wearing: false,
            },
        }
    }

    /// The one place the chains move.
    pub fn next(self, event: ChainSetEvent) -> (ChainSet, Option<ChainSetCue>) {
        use ChainSet::*;
        use ChainSetEvent::*;
        match (self, event) {
            (_, Removed) => (Stowed, None),
            (OnSnow | OnBare { .. }, WornThrough) => (Broken, None),
            (state, WornThrough) => (state, None),
            (_, Installed { surface_snow }) => (Self::on(true, surface_snow), None),
            (Broken, Repaired { surface_snow }) => (Self::on(true, surface_snow), None),
            (state, Repaired { .. }) => (state, None),
            (Stowed | Broken, Road { .. }) => (self, None),
            (OnSnow | OnBare { .. }, Road { snow: true, .. }) => (OnSnow, None),
            (OnSnow, Road { snow: false, .. }) => (Self::on(true, false), None),
            (
                OnBare { cued, .. },
                Road {
                    snow: false,
                    bare_mi,
                },
            ) => {
                let wearing = bare_mi > CHAIN_BARE_GRACE_MI;
                let cue = !cued && bare_mi >= CHAIN_OFF_BARE_MI;
                (
                    OnBare {
                        cued: cued || cue,
                        wearing,
                    },
                    cue.then_some(ChainSetCue::BareRoad),
                )
            }
        }
    }

    /// The name the agent server's `status` reports.
    pub fn key(self) -> &'static str {
        match self {
            ChainSet::Stowed => "stowed",
            ChainSet::OnSnow => "on_snow",
            ChainSet::OnBare { wearing: false, .. } => "on_bare",
            ChainSet::OnBare { wearing: true, .. } => "on_bare_wearing",
            ChainSet::Broken => "broken",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn road(surface: &str, alert_raises: bool) -> RoadReading<'_> {
        RoadReading {
            surface,
            alert_raises,
        }
    }

    #[test]
    fn law_level_follows_the_road() {
        // An alert alone under cloud posts nothing; snow posts 1, ice 2, and
        // a warning that chains every truck raises snow to 2.
        assert_eq!(posted_level(road("dry", true)), 0);
        assert_eq!(posted_level(road("wet", true)), 0);
        assert_eq!(posted_level(road("snow", false)), 1);
        assert_eq!(posted_level(road("snow", true)), 2);
        assert_eq!(posted_level(road("ice", false)), 2);
    }

    #[test]
    fn chain_control_transition_table() {
        use ChainControl::*;
        use ChainControlChange as C;
        let p1 = Posted { level: 1 };
        let p2 = Posted { level: 2 };
        let cases: &[(
            ChainControl,
            RoadReading,
            ChainControl,
            Option<ChainControlChange>,
        )] = &[
            (Clear, road("dry", false), Clear, None),
            (Clear, road("wet", true), Clear, None),
            (Clear, road("snow", false), p1, Some(C::Posted(1))),
            (Clear, road("snow", true), p2, Some(C::Posted(2))),
            (Clear, road("ice", false), p2, Some(C::Posted(2))),
            (p1, road("snow", false), p1, None),
            (p1, road("snow", true), p2, Some(C::Raised(2))),
            (p1, road("ice", false), p2, Some(C::Raised(2))),
            (p1, road("dry", false), Clear, Some(C::Lifted)),
            (p1, road("wet", true), Clear, Some(C::Lifted)),
            (p2, road("ice", true), p2, None),
            (p2, road("snow", false), p1, Some(C::Lowered(1))),
            (p2, road("dry", true), Clear, Some(C::Lifted)),
        ];
        for &(from, reading, to, change) in cases {
            assert_eq!(from.next(reading), (to, change), "{from:?} on {reading:?}");
        }
        assert_eq!(Clear.key(), "clear");
        assert_eq!(p1.key(), "level_1");
        assert_eq!(p2.key(), "level_2");
    }

    #[test]
    fn chain_set_transition_table() {
        use ChainSet::*;
        use ChainSetEvent::*;
        let bare = |cued, wearing| OnBare { cued, wearing };
        let road = |snow, bare_mi| Road { snow, bare_mi };
        let cue = Some(ChainSetCue::BareRoad);
        let cases: &[(ChainSet, ChainSetEvent, ChainSet, Option<ChainSetCue>)] = &[
            (Stowed, road(false, 0.0), Stowed, None),
            (Stowed, Removed, Stowed, None),
            (Stowed, WornThrough, Stowed, None),
            (Stowed, Repaired { surface_snow: true }, Stowed, None),
            (Stowed, Installed { surface_snow: true }, OnSnow, None),
            (
                Stowed,
                Installed {
                    surface_snow: false,
                },
                bare(false, false),
                None,
            ),
            (OnSnow, road(true, 0.0), OnSnow, None),
            (OnSnow, road(false, 0.01), bare(false, false), None),
            (OnSnow, WornThrough, Broken, None),
            (OnSnow, Removed, Stowed, None),
            // Inside the grace: not cued yet, then cued, still not wearing.
            (
                bare(false, false),
                road(false, 0.4),
                bare(false, false),
                None,
            ),
            (bare(false, false), road(false, 0.5), bare(true, false), cue),
            (bare(true, false), road(false, 1.9), bare(true, false), None),
            (bare(true, false), road(false, 2.0), bare(true, false), None),
            // Past the grace: wearing at the bare rate.
            (bare(true, false), road(false, 2.01), bare(true, true), None),
            (bare(true, true), road(false, 9.0), bare(true, true), None),
            // A stretch that jumped past both at once (a frame after a stop)
            // cues and wears together.
            (bare(false, false), road(false, 2.5), bare(true, true), cue),
            // Snow ends the stretch; the next one gets a fresh grace.
            (bare(true, true), road(true, 0.0), OnSnow, None),
            (bare(true, true), Removed, Stowed, None),
            (bare(true, true), WornThrough, Broken, None),
            (
                bare(true, true),
                Installed {
                    surface_snow: false,
                },
                bare(false, false),
                None,
            ),
            (Broken, road(false, 0.0), Broken, None),
            (Broken, Repaired { surface_snow: true }, OnSnow, None),
            (
                Broken,
                Repaired {
                    surface_snow: false,
                },
                bare(false, false),
                None,
            ),
            (Broken, Removed, Stowed, None),
            (Broken, Installed { surface_snow: true }, OnSnow, None),
        ];
        for &(from, event, to, out) in cases {
            assert_eq!(from.next(event), (to, out), "{from:?} on {event:?}");
        }
        assert_eq!(bare(false, false).key(), "on_bare");
        assert_eq!(bare(true, true).key(), "on_bare_wearing");
    }

    #[test]
    fn bare_road_cues_once_per_stretch() {
        // The truck's counter: miles on this bare stretch, 0 on snow.
        let mut set = ChainSet::from_truck(true, true);
        let mut cues = Vec::new();
        let mut bare_mi = 0.0;
        let mut drive = |set: &mut ChainSet, snow: bool, from: f64, to: f64| {
            let mut mile = from;
            while mile <= to {
                bare_mi = if snow { 0.0 } else { bare_mi + 0.05 };
                let (next, cue) = set.next(ChainSetEvent::Road { snow, bare_mi });
                *set = next;
                if cue.is_some() {
                    cues.push(mile);
                }
                mile += 0.05;
            }
        };
        drive(&mut set, false, 0.0, 3.0);
        assert_eq!(set.key(), "on_bare_wearing");
        drive(&mut set, true, 3.05, 4.0);
        drive(&mut set, false, 4.05, 6.0);
        assert_eq!(cues.len(), 2, "{cues:?}");
        assert!((cues[0] - 0.45).abs() < 0.06, "{cues:?}");
        assert!((cues[1] - 4.5).abs() < 0.06, "{cues:?}");
    }
}
