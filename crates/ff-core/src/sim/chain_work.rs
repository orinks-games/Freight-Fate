//! Who does the chain work, and which chain-law areas the truck entered
//! under a posted law.
//!
//! With a co-driver at the wheel the chains went on at the control, then
//! wore out on dry road while the player slept, and the co-driver woke them
//! with "The truck took damage." (owner report, 2026-11-22). Roadside chain
//! work had two parties, the player and Facility stopping assistance, and a
//! co-driver fell between them. The rule is now one table: whoever holds
//! the wheel does the chain work.
//!
//! - [`ChainWorker`] is who answers a [`ChainCue`]: the driver, the assist,
//!   or the co-driver at the wheel. [`ChainWorker::answer`] is the only
//!   place that decides whether a cue is a line for the driver or a stop.
//! - [`ChainAreaEntry`] records, per chain-law area, whether a law was
//!   posted when the truck crossed into it. The chain control and the
//!   patrol cite only a truck that rolled in under a law: one let onto the
//!   grade on a bare road is not cited there when the snow begins.

use std::collections::BTreeSet;

/// The roadside step a chain stop pulls over for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChainStopKind {
    Install,
    Remove,
    Tighten,
    Repair,
}

impl ChainStopKind {
    /// The name the agent server's `status` reports.
    pub fn key(self) -> &'static str {
        match self {
            ChainStopKind::Install => "install",
            ChainStopKind::Remove => "remove",
            ChainStopKind::Tighten => "tighten",
            ChainStopKind::Repair => "repair",
        }
    }
}

/// Something about the chains that wants an answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChainCue {
    /// A posted law chains this rig, the chains are off, and the truck is
    /// near the start of the area or inside it.
    LawPosted,
    /// Chains on have run bare road (`ChainSetCue::BareRoad`).
    BareRoad,
    /// A chain is slapping the fender, inside a chain-law area.
    Slap,
    /// A chain wore through.
    Snapped,
}

impl ChainCue {
    /// The stop that answers this cue when someone other than the driver
    /// does the work.
    pub fn stop(self) -> ChainStopKind {
        match self {
            ChainCue::LawPosted => ChainStopKind::Install,
            ChainCue::BareRoad => ChainStopKind::Remove,
            ChainCue::Slap => ChainStopKind::Tighten,
            ChainCue::Snapped => ChainStopKind::Repair,
        }
    }
}

/// How a cue is answered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChainAnswer {
    /// The driver's own line for the cue, where one exists: the sign call,
    /// "Bare road. Stop and take the chains off.", the slap warning, the
    /// snap line.
    Speak,
    /// Pull over and do the work.
    Stop(ChainStopKind),
}

/// Who does the chain work.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChainWorker {
    /// The player, from the pause menu.
    Driver,
    /// Facility stopping assistance, with the player at the wheel.
    Assist,
    /// The co-driver at the wheel, whatever the assist setting says.
    CoDriver,
}

impl ChainWorker {
    /// Whoever holds the wheel: a co-driver there does the work; otherwise
    /// the assist setting picks the assist or the driver.
    pub fn pick(co_driver_at_wheel: bool, assist_on: bool) -> ChainWorker {
        match (co_driver_at_wheel, assist_on) {
            (true, _) => ChainWorker::CoDriver,
            (false, true) => ChainWorker::Assist,
            (false, false) => ChainWorker::Driver,
        }
    }

    /// The one place a chain cue is answered.
    pub fn answer(self, cue: ChainCue) -> ChainAnswer {
        match self {
            ChainWorker::Driver => ChainAnswer::Speak,
            ChainWorker::Assist | ChainWorker::CoDriver => ChainAnswer::Stop(cue.stop()),
        }
    }

    /// Whether the road's chain lines are spoken: the slap and snap
    /// warnings, and the assist's stop lines. The co-driver's work speaks
    /// only through their own notes.
    pub fn speaks(self) -> bool {
        self != ChainWorker::CoDriver
    }

    /// The name the agent server's `status` reports.
    pub fn key(self) -> &'static str {
        match self {
            ChainWorker::Driver => "driver",
            ChainWorker::Assist => "assist",
            ChainWorker::CoDriver => "co_driver",
        }
    }
}

/// Which chain-law areas the truck rolled into under a posted law.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChainAreaEntry {
    /// The area the truck was in at the last look; `None` before the first.
    inside: Option<Option<usize>>,
    under_law: BTreeSet<usize>,
}

impl ChainAreaEntry {
    /// One look at the truck: the area it is in now, and the law's level.
    ///
    /// Crossing into an area with a law posted marks it. The first look
    /// counts as a crossing, so a resumed save inside an area under a law
    /// is held to it, as before. A law that comes down forgets every mark:
    /// one posted again later posts onto a truck already on the grade.
    pub fn observe(&mut self, area: Option<usize>, level: i64) {
        if level <= 0 {
            self.under_law.clear();
        }
        let entered = match self.inside {
            None => area,
            Some(before) if before != area => area,
            Some(_) => None,
        };
        if let Some(area) = entered.filter(|_| level > 0) {
            self.under_law.insert(area);
        }
        self.inside = Some(area);
    }

    /// Whether the truck rolled into `area` under a posted law.
    pub fn under_law(&self, area: usize) -> bool {
        self.under_law.contains(&area)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chain_worker_table() {
        use ChainAnswer::*;
        use ChainCue::*;
        use ChainStopKind::*;
        use ChainWorker::*;
        let cases: &[(ChainWorker, ChainCue, ChainAnswer)] = &[
            (Driver, LawPosted, Speak),
            (Driver, BareRoad, Speak),
            (Driver, Slap, Speak),
            (Driver, Snapped, Speak),
            (Assist, LawPosted, Stop(Install)),
            (Assist, BareRoad, Stop(Remove)),
            (Assist, Slap, Stop(Tighten)),
            (Assist, Snapped, Stop(Repair)),
            (CoDriver, LawPosted, Stop(Install)),
            (CoDriver, BareRoad, Stop(Remove)),
            (CoDriver, Slap, Stop(Tighten)),
            (CoDriver, Snapped, Stop(Repair)),
        ];
        for &(worker, cue, answer) in cases {
            assert_eq!(worker.answer(cue), answer, "{worker:?} on {cue:?}");
        }
    }

    #[test]
    fn the_wheel_picks_the_worker() {
        use ChainWorker::*;
        assert_eq!(ChainWorker::pick(true, false), CoDriver);
        assert_eq!(ChainWorker::pick(true, true), CoDriver);
        assert_eq!(ChainWorker::pick(false, true), Assist);
        assert_eq!(ChainWorker::pick(false, false), Driver);
        assert!(Driver.speaks() && Assist.speaks() && !CoDriver.speaks());
        assert_eq!(
            [Driver.key(), Assist.key(), CoDriver.key()],
            ["driver", "assist", "co_driver"]
        );
        use ChainStopKind::*;
        assert_eq!(
            [Install, Remove, Tighten, Repair].map(ChainStopKind::key),
            ["install", "remove", "tighten", "repair"]
        );
    }

    #[test]
    fn area_entry_transition_table() {
        // (looks so far: area, level), then the area asked about and the answer.
        type Looks = &'static [(Option<usize>, i64)];
        let cases: &[(Looks, usize, bool)] = &[
            // Rolled in under a law.
            (&[(None, 1), (Some(0), 1)], 0, true),
            // Rolled in bare, and the snow began on the grade.
            (&[(None, 0), (Some(0), 0), (Some(0), 1)], 0, false),
            // First look inside under a law: a resumed save keeps the law.
            (&[(Some(0), 2)], 0, true),
            // First look inside on a bare road, then the law posts.
            (&[(Some(0), 0), (Some(0), 1)], 0, false),
            // The law lifted on the grade and posted again.
            (
                &[(None, 1), (Some(0), 1), (Some(0), 0), (Some(0), 1)],
                0,
                false,
            ),
            // Raised from 1 to 2 on the grade: still the law it rolled in under.
            (&[(None, 1), (Some(0), 1), (Some(0), 2)], 0, true),
            // Out of one area and into the next under a law.
            (&[(Some(0), 0), (None, 1), (Some(1), 1)], 1, true),
            (&[(Some(0), 0), (None, 1), (Some(1), 1)], 0, false),
            // Back out and in again under the law.
            (&[(Some(0), 0), (None, 1), (Some(0), 1)], 0, true),
        ];
        for &(looks, area, want) in cases {
            let mut entry = ChainAreaEntry::default();
            for &(at, level) in looks {
                entry.observe(at, level);
            }
            assert_eq!(entry.under_law(area), want, "{looks:?} asking {area}");
        }
    }
}
