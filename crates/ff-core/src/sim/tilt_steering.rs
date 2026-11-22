//! Tilt steering on a phone: whether it is steering, the pose it calls
//! level, and how far a tilt steers.
//!
//! A Touch gestures switch, off by default. On, tilting the phone left or
//! right steers the way the controller's left stick does: with lane keeping
//! on full a tilt past half way changes one lane
//! ([`crate::sim::lane_push`]), with lane keeping off or partial it steers in
//! proportion. Swipes and flicks keep working either way.
//!
//! Level is the pose the phone is in when the truck starts being driven:
//! entering the drive, resuming it from a menu over it, coming back to the
//! app. It is taken from a quarter second of steady samples, and nothing is
//! steered until it is. The angle itself comes from
//! [`crate::sim::tilt_sensor`].
//!
//! One enum, one [`TiltSteering::next`]: the game feeds it the setting, the
//! sensor, whether the player is at the wheel, each sample, a request to take
//! level again, and the frame's clock. Nothing else moves it. The screen
//! lock ([`TiltSteering::locks_screen`]) and the steering value
//! ([`TiltSteering::steering`]) are read off the state.

/// Tilt inside this many degrees of level steers nothing.
pub const DEAD_ZONE_DEGREES: f64 = 4.0;
/// How long a run of steady samples takes level, in seconds.
pub const SETTLE_SECONDS: f64 = 0.25;
/// How far the samples of one settle may spread and still be steady.
pub const STEADY_SPREAD_DEGREES: f64 = 3.0;
/// How long without a sample before the reading counts as lost.
pub const LOST_AFTER_SECONDS: f64 = 1.0;
/// The "Tilt for full steering" row: its default, range and step.
pub const FULL_DEFAULT_DEGREES: f64 = 30.0;
pub const FULL_MIN_DEGREES: f64 = 15.0;
pub const FULL_MAX_DEGREES: f64 = 45.0;
pub const FULL_STEP_DEGREES: f64 = 5.0;

/// What a settle says when it takes level.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SettleFor {
    /// The drive started or resumed: nothing is said.
    #[default]
    Wheel,
    /// The player asked: "Level set."
    Request,
    /// Readings came back after a gap: "Tilt steering back."
    Return,
}

/// A run of steady samples on its way to becoming level.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Settle {
    /// When the settle began, or the last sample arrived.
    pub last_s: f64,
    /// When the current steady run began; `None` before its first sample.
    pub run_since_s: Option<f64>,
    pub sum: f64,
    pub count: u32,
    pub min: f64,
    pub max: f64,
    pub for_: SettleFor,
}

impl Settle {
    fn begin(at_s: f64, for_: SettleFor) -> Settle {
        Settle {
            last_s: at_s,
            for_,
            ..Settle::default()
        }
    }

    /// Fold in one sample; a sample that spreads the run too far starts a
    /// new run from itself.
    fn add(mut self, degrees: f64, at_s: f64) -> Settle {
        let spread = self.max.max(degrees) - self.min.min(degrees);
        if self.run_since_s.is_none() || spread > STEADY_SPREAD_DEGREES {
            self.run_since_s = Some(at_s);
            self.sum = 0.0;
            self.count = 0;
            self.min = degrees;
            self.max = degrees;
        }
        self.sum += degrees;
        self.count += 1;
        self.min = self.min.min(degrees);
        self.max = self.max.max(degrees);
        self.last_s = at_s;
        self
    }

    /// The level, once the run has lasted long enough.
    fn level(&self, at_s: f64) -> Option<f64> {
        let since = self.run_since_s?;
        (self.count >= 2 && at_s - since >= SETTLE_SECONDS)
            .then(|| self.sum / f64::from(self.count))
    }
}

/// Where tilt steering stands.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum TiltSteering {
    /// The switch is off: swipes and flicks steer, the phone never moves.
    #[default]
    Off,
    /// The switch is on and the device has no motion sensor.
    NoSensor,
    /// On, and the player is not at the wheel (a menu, the pause menu,
    /// practice, or the app in the background).
    Waiting,
    /// At the wheel, taking level.
    Settling(Settle),
    /// Steering, from this level, with the last sample's time.
    Live { level: f64, last_s: f64 },
    /// No sample for a second: steering nothing. Keeps the level it had,
    /// if it had one, for `status`.
    Lost { level: Option<f64> },
}

/// What moves tilt steering.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TiltEvent {
    /// The Touch gestures switch.
    Setting { on: bool },
    /// Whether the device has a motion sensor.
    Sensor { present: bool },
    /// The player came to the wheel (`true`) or left it.
    Wheel { at_wheel: bool, at_s: f64 },
    /// One reading of the angle from flat, in degrees, right edge down
    /// positive.
    Sample { degrees: f64, at_s: f64 },
    /// "Set tilt level".
    SetLevel { at_s: f64 },
    /// The frame's clock, so a gap in the samples is noticed.
    Tick { at_s: f64 },
}

/// A line tilt steering says as it moves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TiltLine {
    LevelSet,
    NoSensor,
    Lost,
    Back,
}

impl TiltLine {
    pub fn text(self) -> &'static str {
        match self {
            TiltLine::LevelSet => "Level set.",
            TiltLine::NoSensor => "This device has no motion sensor.",
            TiltLine::Lost => "No tilt reading. Swipe to steer.",
            TiltLine::Back => "Tilt steering back.",
        }
    }
}

impl TiltSteering {
    /// The one place tilt steering moves. Returns the new state and at most
    /// one line to speak.
    pub fn next(self, event: TiltEvent) -> (TiltSteering, Option<TiltLine>) {
        use TiltEvent as E;
        use TiltSteering as S;
        match (self, event) {
            (_, E::Setting { on: false }) => (S::Off, None),
            (S::Off, E::Setting { on: true }) => (S::Waiting, None),
            (state, E::Setting { on: true }) => (state, None),
            (S::Off, E::Sensor { .. }) => (S::Off, None),
            (S::NoSensor, E::Sensor { present: false }) => (S::NoSensor, None),
            (_, E::Sensor { present: false }) => (S::NoSensor, Some(TiltLine::NoSensor)),
            (S::NoSensor, E::Sensor { present: true }) => (S::Waiting, None),
            (state, E::Sensor { present: true }) => (state, None),
            (
                S::Waiting,
                E::Wheel {
                    at_wheel: true,
                    at_s,
                },
            ) => (S::Settling(Settle::begin(at_s, SettleFor::Wheel)), None),
            (
                S::Settling(_) | S::Live { .. } | S::Lost { .. },
                E::Wheel {
                    at_wheel: false, ..
                },
            ) => (S::Waiting, None),
            (state, E::Wheel { .. }) => (state, None),
            (S::Settling(settle), E::Sample { degrees, at_s }) => {
                let settle = settle.add(degrees, at_s);
                match settle.level(at_s) {
                    Some(level) => (
                        S::Live {
                            level,
                            last_s: at_s,
                        },
                        match settle.for_ {
                            SettleFor::Wheel => None,
                            SettleFor::Request => Some(TiltLine::LevelSet),
                            SettleFor::Return => Some(TiltLine::Back),
                        },
                    ),
                    None => (S::Settling(settle), None),
                }
            }
            (S::Live { level, .. }, E::Sample { at_s, .. }) => (
                S::Live {
                    level,
                    last_s: at_s,
                },
                None,
            ),
            (S::Lost { .. }, E::Sample { degrees, at_s }) => (
                S::Settling(Settle::begin(at_s, SettleFor::Return).add(degrees, at_s)),
                None,
            ),
            (state, E::Sample { .. }) => (state, None),
            // A request from the command list arrives just before the drive
            // is back on top, so it settles from waiting as well.
            (S::Waiting | S::Settling(_) | S::Live { .. }, E::SetLevel { at_s }) => {
                (S::Settling(Settle::begin(at_s, SettleFor::Request)), None)
            }
            (state, E::SetLevel { .. }) => (state, None),
            (S::Settling(settle), E::Tick { at_s })
                if at_s - settle.last_s >= LOST_AFTER_SECONDS =>
            {
                // A gap while coming back stays quiet: it said so once.
                let line = (settle.for_ != SettleFor::Return).then_some(TiltLine::Lost);
                (S::Lost { level: None }, line)
            }
            (S::Live { level, last_s }, E::Tick { at_s })
                if at_s - last_s >= LOST_AFTER_SECONDS =>
            {
                (S::Lost { level: Some(level) }, Some(TiltLine::Lost))
            }
            (state, E::Tick { .. }) => (state, None),
        }
    }

    /// How far this tilt steers, from -1 (full left) to 1 (full right):
    /// nothing unless live, nothing inside the dead zone, then a straight
    /// line to full at `full_degrees` from level.
    pub fn steering(self, degrees: f64, full_degrees: f64) -> f64 {
        match self {
            TiltSteering::Live { level, .. } => steering_curve(degrees - level, full_degrees),
            _ => 0.0,
        }
    }

    /// The screen keeps its orientation while the player is at the wheel.
    pub fn locks_screen(self) -> bool {
        matches!(
            self,
            TiltSteering::Settling(_) | TiltSteering::Live { .. } | TiltSteering::Lost { .. }
        )
    }

    /// Whether the switch is on (whatever the sensor).
    pub fn is_on(self) -> bool {
        self != TiltSteering::Off
    }

    /// The level steering is measured from, once there is one.
    pub fn level(self) -> Option<f64> {
        match self {
            TiltSteering::Live { level, .. } => Some(level),
            TiltSteering::Lost { level } => level,
            _ => None,
        }
    }

    /// The name the agent server's `status` reports.
    pub fn key(self) -> &'static str {
        match self {
            TiltSteering::Off => "off",
            TiltSteering::NoSensor => "no_sensor",
            TiltSteering::Waiting => "waiting",
            TiltSteering::Settling(_) => "settling",
            TiltSteering::Live { .. } => "live",
            TiltSteering::Lost { .. } => "lost",
        }
    }
}

/// The steering curve on an angle already measured from level.
pub fn steering_curve(from_level: f64, full_degrees: f64) -> f64 {
    let full = full_degrees.clamp(FULL_MIN_DEGREES, FULL_MAX_DEGREES);
    let size = from_level.abs();
    if !size.is_finite() || size <= DEAD_ZONE_DEGREES {
        return 0.0;
    }
    let value = ((size - DEAD_ZONE_DEGREES) / (full - DEAD_ZONE_DEGREES)).min(1.0);
    value.copysign(from_level)
}

/// The angle from level where the steering reaches `value` (0 to 1): what
/// Practice gestures and the help mean by half of full.
pub fn degrees_for(value: f64, full_degrees: f64) -> f64 {
    let full = full_degrees.clamp(FULL_MIN_DEGREES, FULL_MAX_DEGREES);
    DEAD_ZONE_DEGREES + value.clamp(0.0, 1.0) * (full - DEAD_ZONE_DEGREES)
}

#[cfg(test)]
mod tests {
    use super::*;
    use TiltEvent as E;
    use TiltSteering as S;

    fn settling(for_: SettleFor) -> TiltSteering {
        S::Settling(Settle::begin(0.0, for_))
    }

    fn live() -> TiltSteering {
        S::Live {
            level: 5.0,
            last_s: 1.0,
        }
    }

    #[test]
    fn the_transition_table() {
        let wheel = |at_wheel| E::Wheel {
            at_wheel,
            at_s: 0.0,
        };
        let table: &[(TiltSteering, TiltEvent, TiltSteering, Option<TiltLine>)] = &[
            // The switch.
            (S::Off, E::Setting { on: true }, S::Waiting, None),
            (live(), E::Setting { on: false }, S::Off, None),
            (S::NoSensor, E::Setting { on: false }, S::Off, None),
            (live(), E::Setting { on: true }, live(), None),
            // The sensor.
            (S::Off, E::Sensor { present: false }, S::Off, None),
            (
                S::Waiting,
                E::Sensor { present: false },
                S::NoSensor,
                Some(TiltLine::NoSensor),
            ),
            (S::NoSensor, E::Sensor { present: false }, S::NoSensor, None),
            (S::NoSensor, E::Sensor { present: true }, S::Waiting, None),
            (live(), E::Sensor { present: true }, live(), None),
            // The wheel.
            (S::Waiting, wheel(true), settling(SettleFor::Wheel), None),
            (S::Off, wheel(true), S::Off, None),
            (S::NoSensor, wheel(true), S::NoSensor, None),
            (live(), wheel(true), live(), None),
            (live(), wheel(false), S::Waiting, None),
            (S::Lost { level: None }, wheel(false), S::Waiting, None),
            (settling(SettleFor::Wheel), wheel(false), S::Waiting, None),
            // Samples.
            (
                S::Waiting,
                E::Sample {
                    degrees: 3.0,
                    at_s: 0.1,
                },
                S::Waiting,
                None,
            ),
            (
                live(),
                E::Sample {
                    degrees: 20.0,
                    at_s: 2.0,
                },
                S::Live {
                    level: 5.0,
                    last_s: 2.0,
                },
                None,
            ),
            (
                S::Lost { level: Some(5.0) },
                E::Sample {
                    degrees: 9.0,
                    at_s: 3.0,
                },
                S::Settling(Settle::begin(3.0, SettleFor::Return).add(9.0, 3.0)),
                None,
            ),
            // Set level.
            (
                live(),
                E::SetLevel { at_s: 0.0 },
                settling(SettleFor::Request),
                None,
            ),
            (
                S::Waiting,
                E::SetLevel { at_s: 0.0 },
                settling(SettleFor::Request),
                None,
            ),
            (S::Off, E::SetLevel { at_s: 0.0 }, S::Off, None),
            (
                S::Lost { level: None },
                E::SetLevel { at_s: 0.0 },
                S::Lost { level: None },
                None,
            ),
            // The clock.
            (live(), E::Tick { at_s: 1.9 }, live(), None),
            (
                live(),
                E::Tick { at_s: 2.0 },
                S::Lost { level: Some(5.0) },
                Some(TiltLine::Lost),
            ),
            (
                settling(SettleFor::Wheel),
                E::Tick { at_s: 0.5 },
                settling(SettleFor::Wheel),
                None,
            ),
            (
                settling(SettleFor::Wheel),
                E::Tick { at_s: 1.0 },
                S::Lost { level: None },
                Some(TiltLine::Lost),
            ),
            (
                settling(SettleFor::Return),
                E::Tick { at_s: 1.0 },
                S::Lost { level: None },
                None,
            ),
            (S::Waiting, E::Tick { at_s: 50.0 }, S::Waiting, None),
        ];
        for &(from, event, to, line) in table {
            assert_eq!(from.next(event), (to, line), "{from:?} on {event:?}");
        }
    }

    /// Feed samples every frame from `start`, for `seconds`, at `degrees`.
    fn run(
        mut state: TiltSteering,
        start: f64,
        seconds: f64,
        degrees: f64,
    ) -> (TiltSteering, Vec<TiltLine>) {
        let mut lines = Vec::new();
        let frames = (seconds * 60.0).round() as i64;
        for frame in 1..=frames {
            let at_s = start + frame as f64 / 60.0;
            for event in [E::Sample { degrees, at_s }, E::Tick { at_s }] {
                let (next, line) = state.next(event);
                state = next;
                lines.extend(line);
            }
        }
        (state, lines)
    }

    #[test]
    fn a_quarter_second_of_steady_samples_takes_level_quietly() {
        let (state, lines) = run(settling(SettleFor::Wheel), 0.0, 0.2, 12.0);
        assert_eq!(state.key(), "settling");
        let (state, lines2) = run(state, 0.2, 0.1, 12.0);
        assert_eq!(state.level(), Some(12.0));
        assert!(lines.is_empty() && lines2.is_empty());
    }

    #[test]
    fn a_moving_phone_waits_for_a_steady_run() {
        let mut state = settling(SettleFor::Wheel);
        // Sweeping two degrees a frame never settles.
        for frame in 1..=60 {
            let at_s = f64::from(frame) / 60.0;
            state = state
                .next(E::Sample {
                    degrees: f64::from(frame) * 2.0,
                    at_s,
                })
                .0;
        }
        assert_eq!(state.key(), "settling");
        let (state, _) = run(state, 1.0, 0.3, -6.0);
        assert_eq!(state.level(), Some(-6.0));
    }

    #[test]
    fn asking_and_coming_back_say_so_when_level_is_taken() {
        let (state, lines) = run(settling(SettleFor::Request), 0.0, 0.3, 0.0);
        assert_eq!(state.key(), "live");
        assert_eq!(lines, vec![TiltLine::LevelSet]);
        let lost = S::Lost { level: Some(4.0) };
        let (state, lines) = run(lost, 0.0, 0.3, 8.0);
        assert_eq!(state.level(), Some(8.0));
        assert_eq!(lines, vec![TiltLine::Back]);
    }

    #[test]
    fn a_second_without_samples_is_lost_once() {
        let (state, _) = run(settling(SettleFor::Wheel), 0.0, 0.5, 0.0);
        let mut state = state;
        let mut lines = Vec::new();
        for frame in 1..=180 {
            let (next, line) = state.next(E::Tick {
                at_s: 0.5 + f64::from(frame) / 60.0,
            });
            state = next;
            lines.extend(line);
        }
        assert_eq!(state, S::Lost { level: Some(0.0) });
        assert_eq!(lines, vec![TiltLine::Lost]);
    }

    #[test]
    fn the_steering_curve() {
        let table: &[(f64, f64)] = &[
            (0.0, 0.0),
            (3.9, 0.0),
            (4.0, 0.0),
            (-4.0, 0.0),
            (17.0, 0.5),
            (-17.0, -0.5),
            (30.0, 1.0),
            (-30.0, -1.0),
            (60.0, 1.0),
            (-90.0, -1.0),
        ];
        for &(degrees, value) in table {
            let got = steering_curve(degrees, 30.0);
            assert!((got - value).abs() < 1e-9, "{degrees}: {got}");
        }
        assert!((steering_curve(15.0, 15.0) - 1.0).abs() < 1e-9);
        assert!((steering_curve(24.5, 45.0) - 0.5).abs() < 1e-9);
        assert_eq!(steering_curve(f64::NAN, 30.0), 0.0);
        assert!((degrees_for(0.5, 30.0) - 17.0).abs() < 1e-9);
    }

    #[test]
    fn only_a_live_state_steers_and_it_steers_from_its_level() {
        assert_eq!(S::Waiting.steering(40.0, 30.0), 0.0);
        assert_eq!(S::Lost { level: Some(0.0) }.steering(40.0, 30.0), 0.0);
        assert!((live().steering(22.0, 30.0) - 0.5).abs() < 1e-9);
        assert_eq!(live().steering(8.0, 30.0), 0.0);
    }

    #[test]
    fn the_screen_locks_at_the_wheel_only() {
        for (state, locked) in [
            (S::Off, false),
            (S::NoSensor, false),
            (S::Waiting, false),
            (settling(SettleFor::Wheel), true),
            (live(), true),
            (S::Lost { level: None }, true),
        ] {
            assert_eq!(state.locks_screen(), locked, "{state:?}");
        }
    }
}
