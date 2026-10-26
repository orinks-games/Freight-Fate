//! Mode-specific driving pressure without removing truck mechanics.
//!
//! Port of `freight_fate/sim/driving_modes.py`.

use crate::sim::hos::WARNING_THRESHOLDS_MIN;

/// Pressure tuning for one driving-mode pacing.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DrivingModeTuning {
    pub name: &'static str,
    pub hazard_frequency: f64,
    pub reaction_window: f64,
    pub collision_damage: f64,
    pub fatigue_rate: f64,
    pub ambient_spacing_s: f64,
    pub routine_speech_interval_s: f64,
    /// Game minutes before an hours-of-service limit at which the countdown
    /// speaks. One mark about six real minutes out when the clock is
    /// compressed: three marks at 10x went by in a quarter of an hour of
    /// play, which a player found unrealistically fast (2026-10-17). Real
    /// time keeps all three, at their real spacing.
    pub hos_warning_marks_min: &'static [f64],
}

// The three pacing settings the row offers. "Realistic" (40x) was retired on
// 2026-08-19: it carried standard's pressure tuning field for field and
// differed only in compressing the clock twice as hard, so the row's one
// realistic-sounding choice was also its least realistic -- real driving is
// 1x. Retiring it costs no tuning, because there was none of its own to
// lose; a save still carrying 40x migrates to standard in Settings.load,
// and any other scale still falls through to standard's pressure here.
//
// "Real time" (1x, added 2026-08-22) is the choice that name was reaching
// for: the driving clock runs at wall-clock speed. It is a clock, not a
// difficulty, so it carries standard's pressure field for field -- the same
// hazards, damage, fatigue and speech spacing, spread over real hours.
const STANDARD: DrivingModeTuning = DrivingModeTuning {
    name: "standard",
    hazard_frequency: 1.0,
    reaction_window: 1.0,
    collision_damage: 1.0,
    fatigue_rate: 1.0,
    ambient_spacing_s: 2.5,
    routine_speech_interval_s: 12.0,
    hos_warning_marks_min: &[120.0],
};

const RELAXED: DrivingModeTuning = DrivingModeTuning {
    name: "relaxed",
    hazard_frequency: 0.55,
    reaction_window: 1.5,
    collision_damage: 0.6,
    fatigue_rate: 0.8,
    ambient_spacing_s: 5.0,
    routine_speech_interval_s: 18.0,
    hos_warning_marks_min: &[60.0],
};

const REAL_TIME: DrivingModeTuning = DrivingModeTuning {
    name: "real time",
    hos_warning_marks_min: &WARNING_THRESHOLDS_MIN,
    ..STANDARD
};

/// The pacing row: `(time_scale, tuning)`.
const MODES: [(f64, DrivingModeTuning); 3] = [(10.0, RELAXED), (20.0, STANDARD), (1.0, REAL_TIME)];

/// Return deterministic tuning, defaulting custom scales to Standard.
pub fn tuning_for_time_scale(time_scale: f64) -> DrivingModeTuning {
    MODES
        .iter()
        .find(|(scale, _)| *scale == time_scale)
        .map(|(_, tuning)| *tuning)
        .unwrap_or(STANDARD)
}

pub fn mode_name(time_scale: f64) -> &'static str {
    tuning_for_time_scale(time_scale).name
}

#[cfg(test)]
mod tests {
    //! Ported from the pure parts of `tests/test_driving_modes.py`; the
    //! Trip- and App-backed cases belong to the sim::trip and app-shell
    //! buckets.
    use super::*;

    #[test]
    fn test_driving_mode_tuning_keeps_standard_baseline_and_softens_only_relaxed() {
        let relaxed = tuning_for_time_scale(10.0);
        let standard = tuning_for_time_scale(20.0);

        assert_eq!([relaxed.name, standard.name], ["relaxed", "standard"]);
        assert!(relaxed.reaction_window > standard.reaction_window);
        assert!(relaxed.collision_damage < standard.collision_damage);
        assert!(relaxed.fatigue_rate < standard.fatigue_rate);
        assert!(relaxed.ambient_spacing_s > standard.ambient_spacing_s);
        assert!(relaxed.routine_speech_interval_s > standard.routine_speech_interval_s);

        // The retired Realistic scale, and any other custom one, still resolves
        // to standard's pressure rather than raising -- a save or a bench that
        // sets the raw trip value has to keep driving. 40x is reachable in play
        // regardless: PARKED_TIME_SCALE_MULT doubles standard while parked.
        assert_eq!(tuning_for_time_scale(40.0).name, "standard");
    }

    #[test]
    fn test_real_time_is_standard_pressure_on_the_real_clock() {
        // Real time (1x) differs from standard only in the clock. It carries
        // standard's pressure tuning field for field, the same way the retired
        // Realistic did: the row's third choice is a clock, not a difficulty.
        let real = tuning_for_time_scale(1.0);
        let standard = tuning_for_time_scale(20.0);

        assert_eq!(real.name, "real time");
        assert_eq!(standard.name, "standard");
        assert_eq!(real.hazard_frequency, standard.hazard_frequency);
        assert_eq!(real.reaction_window, standard.reaction_window);
        assert_eq!(real.collision_damage, standard.collision_damage);
        assert_eq!(real.fatigue_rate, standard.fatigue_rate);
        assert_eq!(real.ambient_spacing_s, standard.ambient_spacing_s);
        assert_eq!(
            real.routine_speech_interval_s,
            standard.routine_speech_interval_s
        );
    }

    #[test]
    fn test_mode_name_follows_the_tuning() {
        assert_eq!(mode_name(10.0), "relaxed");
        assert_eq!(mode_name(1.0), "real time");
        assert_eq!(mode_name(33.0), "standard");
    }
}
