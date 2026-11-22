//! Engine-audio state: maps the truck's physical state to what the engine
//! should SOUND like, independent of any audio backend or sample set.
//!
//! Today the engine is one idle loop pitch-slid by rpm (`audio.engine_freq_mult`),
//! so it cannot tell a truck parked and warming up from one pulling a grade. This
//! module is the small brain that names the situation instead:
//!
//! ```text
//!   off         engine not running
//!   park_idle   parked, air still building -- the "neutral park" character
//!   ready_idle  parked, air up, drive-ready -- the FLIP target (Josh's cue)
//!   launch      pulling away from a stop
//!   cruise      rolling in gear
//!   reverse     backing up (the parking/backing mechanic keys off this)
//! ```
//!
//! The park_idle -> ready_idle FLIP fires exactly when the air system reaches the
//! governor release (`air_ready`): start cold, it fast-idles while it builds air,
//! then flips to the settled drive-ready idle -- the sound Josh liked in 896. A
//! separate `pressurizing` overlay flag (engine on, air not yet ready) drives the
//! air-fill loop regardless of which idle is playing.
//!
//! Pure logic: [`classify`] takes primitives so it is trivially testable, and
//! [`reading_from_truck`] adapts a live truck through the [`EngineTruck`]
//! trait. The playback layer (multisample selection, pitch tracking,
//! crossfades) consumes the result; this module owns the gameplay contract
//! that the engine voice and the parking feature both read.
//!
//! Port of `freight_fate/engine_audio.py`.

/// The named engine situations (also the contract the parking/backing
/// feature reads). [`EngineState::as_str`] gives the Python spelling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EngineState {
    Off,
    ParkIdle,
    ReadyIdle,
    Launch,
    Cruise,
    Reverse,
}

impl EngineState {
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Off => OFF,
            Self::ParkIdle => PARK_IDLE,
            Self::ReadyIdle => READY_IDLE,
            Self::Launch => LAUNCH,
            Self::Cruise => CRUISE,
            Self::Reverse => REVERSE,
        }
    }
}

// State names (also the contract the parking/backing feature reads).
pub const OFF: &str = "off";
pub const PARK_IDLE: &str = "park_idle";
pub const READY_IDLE: &str = "ready_idle";
pub const LAUNCH: &str = "launch";
pub const CRUISE: &str = "cruise";
pub const REVERSE: &str = "reverse";

// A truck slower than this is "stopped"; below LAUNCH_MPS but moving in gear it
// is still pulling away rather than cruising. LAUNCH_THROTTLE matches the
// throttle floor the vehicle uses to tell a real launch from a coasting creep.
pub const STOP_MPS: f64 = 0.3;
pub const LAUNCH_MPS: f64 = 2.5;
pub const LAUNCH_THROTTLE: f64 = 0.15;

// The Mack start settles near 700 RPM. Give the running sound a moment
// there before blending up to the simulated fast idle.
const CAB_START_SETTLE_RPM: f64 = 700.0;
const CAB_IDLE_HOLD_S: f64 = 0.30;
const CAB_IDLE_RISE_S: f64 = 0.65;
const CAB_IDLE_RISE_END_S: f64 = CAB_IDLE_HOLD_S + CAB_IDLE_RISE_S;
const CAB_IDLE_MAX_RPM: f64 = 1100.0;
const CAB_IDLE_MAX_LOAD: f64 = 0.02;

/// Ignition playback phases. The backend supplies the starter handle and
/// reports its completion. Prepared cab loops already have their own
/// playback position when that cue ends.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum IgnitionPlayback<H> {
    Inactive,
    OneShot(H),
    CabStarter(H),
    CabIdleRise { elapsed_s: f64 },
}

#[derive(Debug, Clone, Copy)]
pub enum IgnitionEvent<H> {
    PlayOneShot(H),
    PlayCabStarter(H),
    StarterFinished,
    Update { dt: f64, rpm: f64, load: f64 },
    Stop,
}

impl<H: Copy> IgnitionPlayback<H> {
    pub fn next(self, event: IgnitionEvent<H>) -> Self {
        use IgnitionEvent as Event;
        match (self, event) {
            (_, Event::Stop) | (Self::OneShot(_), Event::StarterFinished) => Self::Inactive,
            (Self::Inactive, Event::PlayOneShot(handle)) => Self::OneShot(handle),
            (Self::Inactive, Event::PlayCabStarter(handle)) => Self::CabStarter(handle),
            (Self::CabStarter(_), Event::StarterFinished) => Self::CabIdleRise { elapsed_s: 0.0 },
            (Self::CabIdleRise { elapsed_s }, Event::Update { dt, rpm, load }) => {
                let elapsed_s = elapsed_s + dt.max(0.0);
                if elapsed_s >= CAB_IDLE_RISE_END_S
                    || rpm > CAB_IDLE_MAX_RPM
                    || load > CAB_IDLE_MAX_LOAD
                {
                    Self::Inactive
                } else {
                    Self::CabIdleRise { elapsed_s }
                }
            }
            _ => self,
        }
    }

    pub fn starter(self) -> Option<H> {
        match self {
            Self::OneShot(sound) | Self::CabStarter(sound) => Some(sound),
            Self::Inactive | Self::CabIdleRise { .. } => None,
        }
    }

    /// Pitch and band selection for the brief transition out of the cab
    /// startup recording. The caller also retains the unmodified live RPM.
    pub fn audible_rpm(self, rpm: f64, load: f64) -> f64 {
        if rpm > CAB_IDLE_MAX_RPM || load > CAB_IDLE_MAX_LOAD {
            return rpm;
        }
        let elapsed_s = match self {
            Self::CabStarter(_) => 0.0,
            Self::CabIdleRise { elapsed_s } => elapsed_s,
            Self::Inactive | Self::OneShot(_) => return rpm,
        };
        let parked = rpm.min(CAB_START_SETTLE_RPM);
        let t = ((elapsed_s - CAB_IDLE_HOLD_S) / CAB_IDLE_RISE_S).clamp(0.0, 1.0);
        parked + (rpm - parked) * t * t * (3.0 - 2.0 * t)
    }
}

/// The physical signals the engine sound depends on, as plain values.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EngineReading {
    pub engine_on: bool,
    pub stalled: bool,
    pub rpm: f64,
    pub throttle: f64,
    pub speed_mps: f64,
    pub in_reverse: bool,
    pub in_neutral: bool,
    /// parking brake or spring brakes are set
    pub parked_brakes_holding: bool,
    /// air pressure at/above the governor release
    pub air_ready: bool,
}

/// What to play: the named state plus the air-fill overlay flag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EngineVoice {
    pub state: EngineState,
    pub pressurizing: bool,
}

/// Name the engine situation. See the module docs for the states.
pub fn classify(r: &EngineReading) -> EngineVoice {
    if !r.engine_on || r.stalled {
        return EngineVoice {
            state: EngineState::Off,
            pressurizing: false,
        };
    }

    // The air-fill loop plays whenever the engine is running below governor
    // release, regardless of which engine state is playing over it.
    let building = !r.air_ready;

    if r.in_reverse {
        return EngineVoice {
            state: EngineState::Reverse,
            pressurizing: building,
        };
    }

    let stationary = r.speed_mps < STOP_MPS;
    let parked = r.in_neutral || r.parked_brakes_holding;

    if stationary && parked {
        // The flip: fast park idle while air builds, drive-ready idle once up.
        let state = if building {
            EngineState::ParkIdle
        } else {
            EngineState::ReadyIdle
        };
        return EngineVoice {
            state,
            pressurizing: building,
        };
    }

    if stationary {
        // In gear, stopped, brakes off: on the throttle it is a launch; off it,
        // it is holding a ready idle (foot on the brake at a light).
        let state = if r.throttle > LAUNCH_THROTTLE {
            EngineState::Launch
        } else {
            EngineState::ReadyIdle
        };
        return EngineVoice {
            state,
            pressurizing: building,
        };
    }

    // Rolling: still launching until up to speed, then cruising.
    if r.speed_mps < LAUNCH_MPS && !r.in_neutral {
        return EngineVoice {
            state: EngineState::Launch,
            pressurizing: building,
        };
    }
    EngineVoice {
        state: EngineState::Cruise,
        pressurizing: building,
    }
}

/// The slice of a live truck the engine voice reads. The Python adapter was
/// duck-typed over `TruckState` and its transmission; `sim::TruckState`
/// implements this so [`reading_from_truck`] works on the real thing.
pub trait EngineTruck {
    fn engine_on(&self) -> bool;
    fn stalled(&self) -> bool;
    fn rpm(&self) -> f64;
    fn throttle(&self) -> f64;
    fn velocity_mps(&self) -> f64;
    fn in_reverse(&self) -> bool;
    fn in_neutral(&self) -> bool;
    fn parking_brake(&self) -> bool;
    fn spring_brakes_active(&self) -> bool;
    fn air_ready(&self) -> bool;
}

/// Adapt a live truck to an [`EngineReading`].
pub fn reading_from_truck<T: EngineTruck + ?Sized>(truck: &T) -> EngineReading {
    EngineReading {
        engine_on: truck.engine_on(),
        stalled: truck.stalled(),
        rpm: truck.rpm(),
        throttle: truck.throttle(),
        speed_mps: truck.velocity_mps().abs(),
        in_reverse: truck.in_reverse(),
        in_neutral: truck.in_neutral(),
        parked_brakes_holding: truck.parking_brake() || truck.spring_brakes_active(),
        air_ready: truck.air_ready(),
    }
}

#[cfg(test)]
mod tests {
    //! Engine-audio state classifier: the contract the engine voice and the
    //! parking/backing feature both read.
    use super::*;

    #[test]
    fn ignition_playback_transitions() {
        use IgnitionEvent as Event;
        use IgnitionPlayback::{CabIdleRise, CabStarter, Inactive, OneShot};

        let update = |dt, rpm, load| Event::Update { dt, rpm, load };
        let initial_rise = CabIdleRise { elapsed_s: 0.0 };
        for (phase, event, expected) in [
            (Inactive, Event::PlayOneShot(1), OneShot(1)),
            (OneShot(1), Event::PlayOneShot(2), OneShot(1)),
            (OneShot(1), Event::StarterFinished, Inactive),
            (Inactive, Event::PlayCabStarter(1), CabStarter(1)),
            (CabStarter(1), Event::PlayCabStarter(2), CabStarter(1)),
            (CabStarter(1), Event::StarterFinished, initial_rise),
            (CabStarter(1), Event::Stop, Inactive),
            (
                initial_rise,
                update(0.3, 900.0, 0.0),
                CabIdleRise { elapsed_s: 0.3 },
            ),
            (initial_rise, update(1.0, 900.0, 0.0), Inactive),
            (initial_rise, update(0.0, 900.0, 0.5), Inactive),
            (initial_rise, Event::Stop, Inactive),
        ] {
            assert_eq!(phase.next(event), expected, "{phase:?} and {event:?}");
        }
    }

    #[test]
    fn cab_start_holds_then_rises_toward_fast_idle_without_delaying_throttle() {
        let crank = IgnitionPlayback::CabStarter(1);
        assert_eq!(crank.audible_rpm(900.0, 0.0), 700.0);
        let rise = crank.next(IgnitionEvent::StarterFinished);
        assert_eq!(rise.audible_rpm(600.0, 0.0), 600.0);
        assert_eq!(rise.audible_rpm(900.0, 0.5), 900.0);
        let midpoint = rise.next(IgnitionEvent::Update {
            dt: CAB_IDLE_HOLD_S + CAB_IDLE_RISE_S / 2.0,
            rpm: 900.0,
            load: 0.0,
        });
        assert!((midpoint.audible_rpm(900.0, 0.0) - 800.0).abs() < 1e-6);
    }

    fn reading() -> EngineReading {
        EngineReading {
            engine_on: true,
            stalled: false,
            rpm: 600.0,
            throttle: 0.0,
            speed_mps: 0.0,
            in_reverse: false,
            in_neutral: true,
            parked_brakes_holding: true,
            air_ready: true,
        }
    }

    #[test]
    fn test_engine_off_is_off() {
        let r = EngineReading {
            engine_on: false,
            ..reading()
        };
        assert_eq!(classify(&r).state, EngineState::Off);
        assert_eq!(classify(&r).state.as_str(), OFF);
    }

    #[test]
    fn test_stalled_is_off() {
        let r = EngineReading {
            stalled: true,
            ..reading()
        };
        assert_eq!(classify(&r).state, EngineState::Off);
    }

    #[test]
    fn test_parked_while_air_builds_is_park_idle_with_pressurizing() {
        let v = classify(&EngineReading {
            air_ready: false,
            ..reading()
        });
        assert_eq!(v.state, EngineState::ParkIdle);
        assert_eq!(v.state.as_str(), PARK_IDLE);
        assert!(v.pressurizing);
    }

    #[test]
    fn test_park_idle_flips_to_ready_idle_when_air_is_up() {
        // The Josh flip: same parked truck, air now at governor release.
        let v = classify(&EngineReading {
            air_ready: true,
            ..reading()
        });
        assert_eq!(v.state, EngineState::ReadyIdle);
        assert_eq!(v.state.as_str(), READY_IDLE);
        assert!(!v.pressurizing);
    }

    #[test]
    fn test_reverse_wins_over_idle_and_still_shows_air_fill() {
        let v = classify(&EngineReading {
            in_reverse: true,
            air_ready: false,
            ..reading()
        });
        assert_eq!(v.state, EngineState::Reverse);
        assert_eq!(v.state.as_str(), REVERSE);
        assert!(v.pressurizing);
    }

    #[test]
    fn test_in_gear_stopped_on_throttle_is_launch() {
        let v = classify(&EngineReading {
            in_neutral: false,
            parked_brakes_holding: false,
            throttle: 0.5,
            ..reading()
        });
        assert_eq!(v.state, EngineState::Launch);
        assert_eq!(v.state.as_str(), LAUNCH);
    }

    #[test]
    fn test_in_gear_stopped_off_throttle_holds_ready_idle() {
        let v = classify(&EngineReading {
            in_neutral: false,
            parked_brakes_holding: false,
            throttle: 0.0,
            ..reading()
        });
        assert_eq!(v.state, EngineState::ReadyIdle);
    }

    #[test]
    fn test_rolling_slow_in_gear_is_launch() {
        let v = classify(&EngineReading {
            in_neutral: false,
            parked_brakes_holding: false,
            speed_mps: 1.5,
            ..reading()
        });
        assert_eq!(v.state, EngineState::Launch);
    }

    #[test]
    fn test_rolling_up_to_speed_is_cruise() {
        let v = classify(&EngineReading {
            in_neutral: false,
            parked_brakes_holding: false,
            speed_mps: 15.0,
            ..reading()
        });
        assert_eq!(v.state, EngineState::Cruise);
        assert_eq!(v.state.as_str(), CRUISE);
    }

    /// A parked truck as the adapter sees it: engine started, air up, parking
    /// brake set. The Python test drove a real `TruckState` through
    /// `start_engine()` / `set_air_ready(parking_brake=True)`; that end-to-end
    /// check belongs with the `sim` port, which implements [`EngineTruck`].
    struct ParkedTruck;

    impl EngineTruck for ParkedTruck {
        fn engine_on(&self) -> bool {
            true
        }
        fn stalled(&self) -> bool {
            false
        }
        fn rpm(&self) -> f64 {
            600.0
        }
        fn throttle(&self) -> f64 {
            0.0
        }
        fn velocity_mps(&self) -> f64 {
            -0.0
        }
        fn in_reverse(&self) -> bool {
            false
        }
        fn in_neutral(&self) -> bool {
            true
        }
        fn parking_brake(&self) -> bool {
            true
        }
        fn spring_brakes_active(&self) -> bool {
            false
        }
        fn air_ready(&self) -> bool {
            true
        }
    }

    #[test]
    fn test_reading_from_a_parked_truck_classifies_and_flips() {
        // A freshly parked truck starts below governor release, then reaches it.
        let v = classify(&reading_from_truck(&ParkedTruck));
        assert_eq!(v.state, EngineState::ReadyIdle);
        assert!(!v.pressurizing);
    }
}
