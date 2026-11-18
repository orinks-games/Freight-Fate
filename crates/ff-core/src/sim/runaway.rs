//! Whether the truck is in a runaway: one state, two spoken edges.
//!
//! Past [`RUNAWAY_SPEED_MPH`] the truck takes damage every second
//! (`TruckState::update_wear`). That damage was silent until issue 533, so a
//! run could end worse off with nothing said about why. This machine is the
//! one place the runaway's state moves: the driving state feeds it the road
//! speed each frame and speaks what it returns, and the agent server's
//! `observe` reports it.
//!
//! The way out sits [`RUNAWAY_CLEAR_MARGIN_MPH`] under the way in, so a truck
//! hovering at the threshold cannot repeat the pair of lines. No damage
//! accrues inside that margin, so the damage spoken on the way out is final.

use crate::sim::vehicle::RUNAWAY_SPEED_MPH;

/// How far under the threshold the speed has to fall to end a runaway.
pub const RUNAWAY_CLEAR_MARGIN_MPH: f64 = 3.0;

/// In a runaway or not.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Runaway {
    /// Under the threshold, or back under it by the margin.
    #[default]
    Clear,
    /// Past the threshold: the truck is taking damage.
    Running,
}

/// What the machine is told.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RunawayEvent {
    /// This frame's road speed, with the player at the wheel.
    Speed { mph: f64 },
    /// The runaway ended some other way: the truck wrecked, the run ended,
    /// or the co-driver has the wheel. Nothing is said.
    Abandoned,
}

/// A transition the caller speaks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunawayTransition {
    /// Past the threshold: "Runaway! The truck is taking damage."
    Began,
    /// Back under by the margin: "Back under control. Damage N percent."
    Ended,
}

impl Runaway {
    /// Move the machine; returns the transition to speak, if any.
    pub fn next(&mut self, event: RunawayEvent) -> Option<RunawayTransition> {
        let (state, transition) = match (*self, event) {
            (Runaway::Clear, RunawayEvent::Speed { mph }) if mph > RUNAWAY_SPEED_MPH => {
                (Runaway::Running, Some(RunawayTransition::Began))
            }
            (Runaway::Running, RunawayEvent::Speed { mph })
                if mph < RUNAWAY_SPEED_MPH - RUNAWAY_CLEAR_MARGIN_MPH =>
            {
                (Runaway::Clear, Some(RunawayTransition::Ended))
            }
            (_, RunawayEvent::Abandoned) => (Runaway::Clear, None),
            (state, RunawayEvent::Speed { .. }) => (state, None),
        };
        *self = state;
        transition
    }

    /// The name `observe` reports.
    pub fn key(self) -> &'static str {
        match self {
            Runaway::Clear => "clear",
            Runaway::Running => "running",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn speed(mph: f64) -> RunawayEvent {
        RunawayEvent::Speed { mph }
    }

    #[test]
    fn test_transition_table() {
        use Runaway::*;
        use RunawayTransition::*;
        let t = RUNAWAY_SPEED_MPH;
        let out = t - RUNAWAY_CLEAR_MARGIN_MPH;
        let cases = [
            (Clear, speed(t - 10.0), Clear, None),
            (Clear, speed(t), Clear, None),
            (Clear, speed(t + 0.1), Running, Some(Began)),
            (Running, speed(t + 5.0), Running, None),
            (Running, speed(t - 0.5), Running, None),
            (Running, speed(out), Running, None),
            (Running, speed(out - 0.1), Clear, Some(Ended)),
            (Running, RunawayEvent::Abandoned, Clear, None),
            (Clear, RunawayEvent::Abandoned, Clear, None),
        ];
        for (from, event, to, said) in cases {
            let mut state = from;
            assert_eq!(state.next(event), said, "{from:?} on {event:?}");
            assert_eq!(state, to, "{from:?} on {event:?}");
        }
    }

    #[test]
    fn test_hovering_at_the_threshold_speaks_the_pair_once() {
        let mut state = Runaway::default();
        let mut said = Vec::new();
        for i in 0..200 {
            let wobble = if i % 2 == 0 { 0.4 } else { -0.4 };
            said.extend(state.next(speed(RUNAWAY_SPEED_MPH + wobble)));
        }
        said.extend(state.next(speed(60.0)));
        assert_eq!(
            said,
            vec![RunawayTransition::Began, RunawayTransition::Ended]
        );
    }
}
