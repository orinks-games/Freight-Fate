//! The first-run walkthrough: which step the new driver is on, and which
//! line each thing they do earns.
//!
//! The walkthrough teaches one drive's start: start the engine, release the
//! parking brake once the air is up, get into gear (a manual box only), and
//! roll. Each step has one reminder, spoken once after
//! [`REMINDER_AFTER_S`] seconds with nothing new said. [`FirstRun::next`] is
//! the only place the step moves; the game turns the [`FirstRunCue`] it
//! answers with into words, since the words name the player's own keys or
//! gestures.
//!
//! The rule the table keeps: one event gets one line. Air coming up is an
//! event of its own with its own line ("Air ready."), so it restarts the
//! reminder's wait; before that, a reminder timed from the engine start
//! landed a second after "Air ready." and said the same thing again (phone
//! walk W2, 2026-11-21).

/// Seconds with nothing new said before a step's one reminder.
pub const REMINDER_AFTER_S: f64 = 25.0;

/// Speed at which the walkthrough counts the truck as rolling.
pub const ROLLING_MPH: f64 = 20.0;

/// Where the new driver is in the walkthrough.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FirstRunStep {
    /// The engine is off.
    #[default]
    StartEngine,
    /// The engine runs; the parking brake is still set (air may still be
    /// building).
    ReleaseBrake,
    /// A manual box with the brake off: first gear is next.
    ShiftIntoGear,
    /// In gear with the brake off: the gas is next.
    Accelerate,
    /// Rolling. The walkthrough is over.
    Done,
}

impl FirstRunStep {
    /// The step's number as the agent server and older saves count it:
    /// 0 engine, 1 brake or gear, 2 accelerate, 3 done.
    pub fn number(self) -> i32 {
        match self {
            FirstRunStep::StartEngine => 0,
            FirstRunStep::ReleaseBrake | FirstRunStep::ShiftIntoGear => 1,
            FirstRunStep::Accelerate => 2,
            FirstRunStep::Done => 3,
        }
    }

    /// Whether this step has a reminder.
    fn reminds(self) -> bool {
        matches!(
            self,
            FirstRunStep::StartEngine | FirstRunStep::ReleaseBrake | FirstRunStep::ShiftIntoGear
        )
    }
}

/// Something the driver did, or the truck did, that the walkthrough hears.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FirstRunEvent {
    EngineStarted,
    /// Air pressure reached the parking-brake release threshold.
    AirReady,
    /// The parking brake came off; `automatic` is the gearbox.
    ParkingBrakeReleased {
        automatic: bool,
    },
    GearEngaged,
    /// The truck passed [`ROLLING_MPH`].
    Rolling,
    /// Time passed, in seconds.
    Elapsed(f64),
}

/// The line a transition earns. The game puts it in words.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FirstRunCue {
    /// After the engine start: what to do at air ready.
    AfterEngine,
    /// The brake is off with an automatic box: the gas is next.
    BrakeReleasedAutomatic,
    /// The brake is off with a manual box: first gear is next.
    BrakeReleasedManual,
    /// In gear: the gas is next.
    InGear,
    /// Rolling: the information keys, and the walkthrough is over.
    Rolling,
    /// The step's one reminder.
    Reminder(FirstRunStep),
}

/// The walkthrough's state: the step, and the wait toward its reminder.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FirstRun {
    step: FirstRunStep,
    /// Seconds since the last walkthrough line or event.
    quiet_s: f64,
    /// The current step's reminder has been spoken.
    reminded: bool,
}

impl FirstRun {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn step(&self) -> FirstRunStep {
        self.step
    }

    /// Move to `step` with a fresh wait toward its reminder.
    fn enter(&mut self, step: FirstRunStep) {
        self.step = step;
        self.quiet_s = 0.0;
        self.reminded = false;
    }

    /// The one transition function: the event, and the line it earns.
    pub fn next(&mut self, event: FirstRunEvent) -> Option<FirstRunCue> {
        use FirstRunEvent as E;
        use FirstRunStep as S;
        match (self.step, event) {
            (S::StartEngine, E::EngineStarted) => {
                self.enter(S::ReleaseBrake);
                Some(FirstRunCue::AfterEngine)
            }
            // "Air ready." is its own line; the reminder waits from it.
            (S::ReleaseBrake, E::AirReady) => {
                self.quiet_s = 0.0;
                None
            }
            (S::ReleaseBrake, E::ParkingBrakeReleased { automatic: true }) => {
                self.enter(S::Accelerate);
                Some(FirstRunCue::BrakeReleasedAutomatic)
            }
            (S::ReleaseBrake, E::ParkingBrakeReleased { automatic: false }) => {
                self.enter(S::ShiftIntoGear);
                Some(FirstRunCue::BrakeReleasedManual)
            }
            // A manual driver may shift before releasing the brake.
            (S::ReleaseBrake | S::ShiftIntoGear, E::GearEngaged) => {
                self.enter(S::Accelerate);
                Some(FirstRunCue::InGear)
            }
            (S::Accelerate, E::Rolling) => {
                self.enter(S::Done);
                Some(FirstRunCue::Rolling)
            }
            (step, E::Elapsed(dt)) if step.reminds() && !self.reminded => {
                self.quiet_s += dt;
                if self.quiet_s > REMINDER_AFTER_S {
                    self.reminded = true;
                    Some(FirstRunCue::Reminder(step))
                } else {
                    None
                }
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use FirstRunCue as C;
    use FirstRunEvent as E;
    use FirstRunStep as S;

    /// Every transition: (start, event, step after, line).
    #[test]
    fn transition_table() {
        let table = [
            (
                S::StartEngine,
                E::EngineStarted,
                S::ReleaseBrake,
                Some(C::AfterEngine),
            ),
            (S::StartEngine, E::AirReady, S::StartEngine, None),
            (S::StartEngine, E::GearEngaged, S::StartEngine, None),
            (S::StartEngine, E::Rolling, S::StartEngine, None),
            (S::ReleaseBrake, E::EngineStarted, S::ReleaseBrake, None),
            (S::ReleaseBrake, E::AirReady, S::ReleaseBrake, None),
            (
                S::ReleaseBrake,
                E::ParkingBrakeReleased { automatic: true },
                S::Accelerate,
                Some(C::BrakeReleasedAutomatic),
            ),
            (
                S::ReleaseBrake,
                E::ParkingBrakeReleased { automatic: false },
                S::ShiftIntoGear,
                Some(C::BrakeReleasedManual),
            ),
            (
                S::ReleaseBrake,
                E::GearEngaged,
                S::Accelerate,
                Some(C::InGear),
            ),
            (
                S::ShiftIntoGear,
                E::GearEngaged,
                S::Accelerate,
                Some(C::InGear),
            ),
            (
                S::ShiftIntoGear,
                E::ParkingBrakeReleased { automatic: false },
                S::ShiftIntoGear,
                None,
            ),
            (
                S::Accelerate,
                E::ParkingBrakeReleased { automatic: true },
                S::Accelerate,
                None,
            ),
            (S::Accelerate, E::Rolling, S::Done, Some(C::Rolling)),
            (S::Done, E::Rolling, S::Done, None),
            (S::Done, E::EngineStarted, S::Done, None),
        ];
        for (start, event, after, cue) in table {
            let mut run = FirstRun::new();
            run.enter(start);
            assert_eq!(run.next(event), cue, "{start:?} + {event:?}");
            assert_eq!(run.step(), after, "{start:?} + {event:?}");
        }
    }

    #[test]
    fn each_step_reminds_once_after_the_wait() {
        for step in [S::StartEngine, S::ReleaseBrake, S::ShiftIntoGear] {
            let mut run = FirstRun::new();
            run.enter(step);
            assert_eq!(run.next(E::Elapsed(REMINDER_AFTER_S - 1.0)), None);
            assert_eq!(run.next(E::Elapsed(2.0)), Some(C::Reminder(step)));
            assert_eq!(run.next(E::Elapsed(100.0)), None, "{step:?} reminds once");
        }
        for step in [S::Accelerate, S::Done] {
            let mut run = FirstRun::new();
            run.enter(step);
            assert_eq!(
                run.next(E::Elapsed(100.0)),
                None,
                "{step:?} has no reminder"
            );
        }
    }

    /// Phone walk W2: the air came up about as the engine start's reminder
    /// fell due, and "Air ready." was followed at once by "Reminder: air is
    /// ready". Air ready restarts the wait.
    #[test]
    fn air_ready_restarts_the_reminder_wait() {
        let mut run = FirstRun::new();
        run.next(E::EngineStarted);
        assert_eq!(run.next(E::Elapsed(REMINDER_AFTER_S - 0.5)), None);
        assert_eq!(run.next(E::AirReady), None);
        assert_eq!(
            run.next(E::Elapsed(1.0)),
            None,
            "no reminder on top of air ready"
        );
        assert_eq!(
            run.next(E::Elapsed(REMINDER_AFTER_S)),
            Some(C::Reminder(S::ReleaseBrake))
        );
    }

    #[test]
    fn step_numbers_match_the_old_stages() {
        assert_eq!(S::StartEngine.number(), 0);
        assert_eq!(S::ReleaseBrake.number(), 1);
        assert_eq!(S::ShiftIntoGear.number(), 1);
        assert_eq!(S::Accelerate.number(), 2);
        assert_eq!(S::Done.number(), 3);
    }
}
