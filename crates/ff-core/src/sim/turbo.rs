//! Diesel turbocharger boost and release state machine.
//!
//! Tracks turbine spool from engine RPM and load, with realistic spool-up
//! lag and faster spool-down on throttle lift or gear shifts. Trucks with
//! `turbo_level == 0.0` (quiet trainers and light city day cabs) stay
//! [`TurboPhase::Off`]. Pure ambience: nothing here is spoken.

/// Sound key for the continuous in-cab turbocharger spool loop.
pub const TURBO_SPOOL_KEY: &str = "vehicle/turbo_spool";
/// Sound key for the turbocharger lift-off / shift pressure release cue.
pub const TURBO_BLOWOFF_KEY: &str = "vehicle/turbo_blowoff";

/// Time constant (seconds) for boost building under load.
const SPOOL_UP_TAU_S: f64 = 0.65;
/// Time constant (seconds) for boost falling off throttle or during a shift.
const SPOOL_DOWN_TAU_S: f64 = 0.30;
/// Minimum boost before throttle lift or a shift triggers a release cue.
const BLOWOFF_MIN_BOOST: f64 = 0.42;
/// Minimum seconds between consecutive blow-off cues.
const BLOWOFF_COOLDOWN_S: f64 = 1.2;
/// Throttle threshold below which a lift-off releases boost.
const LIFT_THROTTLE: f64 = 0.12;
/// Boost threshold below which the spool loop fades completely out.
/// Set above flat-ground 75 mph cruise load even in a fully loaded standard
/// rig (~0.70 throttle at ~1,550 RPM) so steady cruising never leaves a
/// faint lingering turbo whistle, while acceleration and grades still spool.
const AUDIBLE_BOOST: f64 = 0.50;
/// Peak loop level at full boost and `turbo_level = 1.0`.
const MAX_SPOOL_LEVEL: f64 = 0.34;
/// Peak one-shot level for the release cue at full boost and `turbo_level = 1.0`.
const MAX_BLOWOFF_LEVEL: f64 = 0.34;
/// Loop fade time in milliseconds.
pub const TURBO_FADE_MS: u32 = 160;

/// Explicit phase of the turbocharger voice.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TurboPhase {
    Off,
    Idle,
    Spooling,
    Boosted,
    Release,
}

impl TurboPhase {
    pub fn name(self) -> &'static str {
        match self {
            TurboPhase::Off => "off",
            TurboPhase::Idle => "idle",
            TurboPhase::Spooling => "spooling",
            TurboPhase::Boosted => "boosted",
            TurboPhase::Release => "release",
        }
    }
}

/// Per-frame inputs to [`TurboState::step`].
#[derive(Clone, Copy, Debug)]
pub struct TurboInput {
    pub engine_on: bool,
    pub rpm: f64,
    pub idle_rpm: f64,
    pub max_rpm: f64,
    pub throttle: f64,
    pub shifting: bool,
    pub clutch_out: bool,
    /// Per-truck turbo prominence in `0.0..=1.0` (`0.0` = no audible turbo).
    pub turbo_level: f64,
    pub dt: f64,
}

/// What the runtime audio layer should play for this frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TurboOutput {
    /// Loop level (`0.0` means stop the loop).
    pub spool_level: f64,
    /// Playback rate for the spool loop.
    pub spool_rate: f64,
    /// One-shot volume if a lift-off or shift release occurred this frame.
    pub blowoff_volume: Option<f64>,
}

/// Persistent turbocharger boost state.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TurboState {
    pub boost: f64,
    pub phase: TurboPhase,
    pub blowoff_cooldown_s: f64,
}

impl Default for TurboState {
    fn default() -> Self {
        Self {
            boost: 0.0,
            phase: TurboPhase::Off,
            blowoff_cooldown_s: 0.0,
        }
    }
}

impl TurboState {
    /// Advance the turbocharger state by one frame and return audio parameters.
    pub fn step(&mut self, input: TurboInput) -> TurboOutput {
        let dt = input.dt.max(0.0);
        let level = input.turbo_level.clamp(0.0, 1.0);
        self.blowoff_cooldown_s = (self.blowoff_cooldown_s - dt).max(0.0);

        if !input.engine_on || level <= 0.0 {
            self.boost = 0.0;
            self.phase = TurboPhase::Off;
            return TurboOutput {
                spool_level: 0.0,
                spool_rate: 1.0,
                blowoff_volume: None,
            };
        }

        let rpm_span = (input.max_rpm - input.idle_rpm).max(100.0);
        let rpm_frac = ((input.rpm - input.idle_rpm) / rpm_span).clamp(0.0, 1.0);
        let throttle = input.throttle.clamp(0.0, 1.0);
        let unloaded = input.shifting || input.clutch_out;

        let target = if unloaded {
            0.0
        } else {
            rpm_frac.powf(0.85) * throttle
        };

        let lifted = (unloaded || throttle < LIFT_THROTTLE) && self.boost >= BLOWOFF_MIN_BOOST;
        let blowoff_volume = if lifted && self.blowoff_cooldown_s <= 0.0 {
            self.blowoff_cooldown_s = BLOWOFF_COOLDOWN_S;
            Some((self.boost * level * MAX_BLOWOFF_LEVEL).clamp(0.0, MAX_BLOWOFF_LEVEL))
        } else {
            None
        };

        if dt <= 0.0 {
            self.boost = target;
        } else {
            let tau = if target >= self.boost {
                SPOOL_UP_TAU_S
            } else {
                SPOOL_DOWN_TAU_S
            };
            let alpha = 1.0 - (-dt / tau).exp();
            self.boost += (target - self.boost) * alpha;
        }
        self.boost = self.boost.clamp(0.0, 1.0);

        self.phase = if blowoff_volume.is_some() {
            TurboPhase::Release
        } else if self.boost <= AUDIBLE_BOOST {
            TurboPhase::Idle
        } else if target > self.boost + 0.05 {
            TurboPhase::Spooling
        } else {
            TurboPhase::Boosted
        };

        let spool_level = if self.boost <= AUDIBLE_BOOST {
            0.0
        } else {
            let audible = (self.boost - AUDIBLE_BOOST) / (1.0 - AUDIBLE_BOOST);
            (audible * audible.sqrt() * level * MAX_SPOOL_LEVEL).clamp(0.0, MAX_SPOOL_LEVEL)
        };
        let spool_rate = 0.72 + 0.50 * self.boost;

        TurboOutput {
            spool_level,
            spool_rate,
            blowoff_volume,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_input() -> TurboInput {
        TurboInput {
            engine_on: true,
            rpm: 1_500.0,
            idle_rpm: 600.0,
            max_rpm: 2_200.0,
            throttle: 0.9,
            shifting: false,
            clutch_out: false,
            turbo_level: 0.85,
            dt: 0.1,
        }
    }

    #[test]
    fn off_when_engine_stopped_or_truck_has_no_turbo() {
        let mut s = TurboState::default();
        let out = s.step(TurboInput {
            engine_on: false,
            ..base_input()
        });
        assert_eq!(s.phase, TurboPhase::Off);
        assert_eq!(out.spool_level, 0.0);
        assert_eq!(out.blowoff_volume, None);

        let out_quiet = s.step(TurboInput {
            turbo_level: 0.0,
            ..base_input()
        });
        assert_eq!(s.phase, TurboPhase::Off);
        assert_eq!(out_quiet.spool_level, 0.0);
    }

    #[test]
    fn spools_up_under_load_and_blows_off_on_shift() {
        let mut s = TurboState::default();
        for _ in 0..20 {
            s.step(base_input());
        }
        assert!(s.boost > 0.5, "expected boost > 0.5, got {}", s.boost);
        assert_eq!(s.phase, TurboPhase::Boosted);

        let out = s.step(TurboInput {
            shifting: true,
            ..base_input()
        });
        assert_eq!(s.phase, TurboPhase::Release);
        assert!(out.blowoff_volume.is_some());

        // Cooldown prevents double blow-off on the very next frame.
        let out2 = s.step(TurboInput {
            shifting: true,
            ..base_input()
        });
        assert_eq!(out2.blowoff_volume, None);
    }

    #[test]
    fn idle_stays_silent() {
        let mut s = TurboState::default();
        let out = s.step(TurboInput {
            rpm: 600.0,
            throttle: 0.0,
            ..base_input()
        });
        assert_eq!(s.phase, TurboPhase::Idle);
        assert_eq!(out.spool_level, 0.0);
    }

    #[test]
    fn spool_rate_sweeps_across_boost_range() {
        let mut s = TurboState::default();
        let low = s.step(TurboInput {
            rpm: 900.0,
            throttle: 0.3,
            ..base_input()
        });
        for _ in 0..30 {
            s.step(TurboInput {
                rpm: 2_100.0,
                throttle: 1.0,
                ..base_input()
            });
        }
        let high = s.step(TurboInput {
            rpm: 2_100.0,
            throttle: 1.0,
            ..base_input()
        });
        assert!(low.spool_rate < 0.80, "low rate {}", low.spool_rate);
        assert!(high.spool_rate > 1.10, "high rate {}", high.spool_rate);
        assert!(high.spool_level > low.spool_level);
    }

    #[test]
    fn steady_highway_cruise_fades_spool_to_silence() {
        let mut s = TurboState::default();
        // Accelerate hard to build boost.
        for _ in 0..25 {
            s.step(TurboInput {
                rpm: 1_850.0,
                throttle: 1.0,
                ..base_input()
            });
        }
        assert!(s.boost > 0.6);

        // Settle into steady flat-ground 75 mph cruise (including a fully
        // loaded standard rig holding ~0.72 throttle at ~1,550 RPM).
        let mut last = TurboOutput {
            spool_level: 1.0,
            spool_rate: 1.0,
            blowoff_volume: None,
        };
        for _ in 0..30 {
            last = s.step(TurboInput {
                rpm: 1_550.0,
                throttle: 0.72,
                ..base_input()
            });
        }
        assert_eq!(s.phase, TurboPhase::Idle);
        assert_eq!(last.spool_level, 0.0);
    }
}
