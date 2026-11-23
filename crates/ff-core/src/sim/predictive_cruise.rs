//! Grade-aware predictive cruise state machine.
//!
//! Models the predictive phase of adaptive cruise and the low-speed keeper:
//! banking momentum before a climb, holding over a crest, pre-engaging the
//! engine brake before a downgrade, and ramping back up smoothly after a work
//! zone or green light.

use serde::{Deserialize, Serialize};

/// Default smooth-resume acceleration cap (mph per second) when rebuilding
/// speed out of a work zone or green light.
pub const SMOOTH_RESUME_MPH_PER_S: f64 = 3.0;

/// Minimum downgrade grade ahead (decimal, e.g. 0.025 = 2.5%) that triggers
/// jake-first anticipation.
pub const JAKE_ANTICIPATE_GRADE_MIN: f64 = 0.025;

/// Steep downgrade grade ahead (decimal, e.g. 0.045 = 4.5%) that warrants
/// stage-2 jake anticipation.
pub const JAKE_ANTICIPATE_STEEP_GRADE: f64 = 0.045;

/// Explicit state of grade-aware predictive cruise and smooth resumption.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub enum PredictiveCruiseState {
    #[default]
    Cruising,
    BuildingForClimb,
    HoldingCrest,
    /// Pre-engaging the engine brake before/at a downgrade crest so speed is
    /// shed on the retarder before service brakes are needed.
    JakeAnticipating {
        target_stage: i32,
    },
    /// Smoothly rebuilding speed out of a work zone or green light instead of
    /// demanding full throttle immediately.
    ResumingSmooth {
        target_mph: f64,
        ramp_mph_per_s: f64,
    },
}

/// Inputs that drive [`PredictiveCruiseState::next`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PredictiveCruiseEvent {
    /// Cruise/keeper disengaged or predictive cruise disabled.
    Disengaged,
    /// A work zone ended or a stopped traffic light turned green; ramp gently
    /// toward `target_mph`.
    TriggerSmoothResume { target_mph: f64 },
    /// Normal tick while cruise or keeper is active.
    Update {
        /// Current speed (mph).
        speed_mph: f64,
        /// Target speed (mph).
        target_mph: f64,
        /// Grade under the wheels (decimal, negative = downhill).
        grade_here: f64,
        /// Steepest climb grade ahead within the preview window (decimal >= 0).
        climb_ahead: f64,
        /// Steepest descent grade ahead within the preview window (decimal <= 0).
        descent_ahead: f64,
        /// Grade in the immediate crest window ahead (decimal).
        crest_preview: f64,
        /// Whether the truck can use the engine brake right now.
        jake_available: bool,
        /// Max jake stage permitted by the truck / descent mode (0..=3).
        jake_ceiling: i32,
    },
}

impl PredictiveCruiseState {
    /// Pure state transition function.
    pub fn next(&self, event: PredictiveCruiseEvent) -> Self {
        match event {
            PredictiveCruiseEvent::Disengaged => Self::Cruising,
            PredictiveCruiseEvent::TriggerSmoothResume { target_mph } => {
                if target_mph > 0.0 {
                    Self::ResumingSmooth {
                        target_mph,
                        ramp_mph_per_s: SMOOTH_RESUME_MPH_PER_S,
                    }
                } else {
                    Self::Cruising
                }
            }
            PredictiveCruiseEvent::Update {
                speed_mph,
                target_mph,
                grade_here,
                climb_ahead,
                descent_ahead,
                crest_preview,
                jake_available,
                jake_ceiling,
            } => {
                // Downgrade anticipation takes priority over smooth resume or
                // climb momentum when near or above target speed: never
                // accelerate into a downhill grade, and never drag the truck
                // far below target speed.
                let downhill_ahead = descent_ahead <= -JAKE_ANTICIPATE_GRADE_MIN
                    && climb_ahead < JAKE_ANTICIPATE_GRADE_MIN;
                let on_downhill = grade_here <= -JAKE_ANTICIPATE_GRADE_MIN;
                let fast_enough_to_anticipate = speed_mph >= target_mph - 2.0;
                if (downhill_ahead || on_downhill)
                    && fast_enough_to_anticipate
                    && jake_available
                    && jake_ceiling > 0
                {
                    let severity = (-descent_ahead).max(-grade_here);
                    let wanted = if severity >= JAKE_ANTICIPATE_STEEP_GRADE {
                        2.min(jake_ceiling)
                    } else {
                        1.min(jake_ceiling)
                    };
                    return Self::JakeAnticipating {
                        target_stage: wanted,
                    };
                }

                // Keep smooth resume active until the truck catches up to the
                // target speed (or the target drops below current speed).
                if let Self::ResumingSmooth {
                    target_mph: resume_target,
                    ramp_mph_per_s,
                } = *self
                {
                    let effective_target = resume_target.min(target_mph);
                    if speed_mph + 1.5 < effective_target {
                        return Self::ResumingSmooth {
                            target_mph: effective_target,
                            ramp_mph_per_s,
                        };
                    }
                }

                // Cresting a climb: hold without downshifting into the drop.
                if grade_here >= JAKE_ANTICIPATE_GRADE_MIN
                    && crest_preview < JAKE_ANTICIPATE_GRADE_MIN
                {
                    return Self::HoldingCrest;
                }

                // Approaching a climb on flat/mild ground: bank momentum.
                if climb_ahead >= JAKE_ANTICIPATE_GRADE_MIN
                    && grade_here < JAKE_ANTICIPATE_GRADE_MIN
                {
                    return Self::BuildingForClimb;
                }

                Self::Cruising
            }
        }
    }

    /// Short machine-readable tag for `DrivingObservation` and Agent MCP `status`.
    pub fn key(&self) -> &'static str {
        match self {
            Self::Cruising => "cruising",
            Self::BuildingForClimb => "building_for_climb",
            Self::HoldingCrest => "holding_crest",
            Self::JakeAnticipating { .. } => "jake_anticipating",
            Self::ResumingSmooth { .. } => "resuming_smooth",
        }
    }

    /// Parse a scenario tag into a staged state.
    pub fn from_tag(tag: &str) -> Option<Self> {
        match tag {
            "cruising" => Some(Self::Cruising),
            "building_for_climb" => Some(Self::BuildingForClimb),
            "holding_crest" => Some(Self::HoldingCrest),
            "jake_anticipating" => Some(Self::JakeAnticipating { target_stage: 1 }),
            "resuming_smooth" => Some(Self::ResumingSmooth {
                target_mph: 65.0,
                ramp_mph_per_s: SMOOTH_RESUME_MPH_PER_S,
            }),
            _ => None,
        }
    }

    /// Stage requested when in [`Self::JakeAnticipating`], otherwise `0`.
    pub fn anticipated_jake_stage(&self) -> i32 {
        match *self {
            Self::JakeAnticipating { target_stage } => target_stage,
            _ => 0,
        }
    }

    /// Whether smooth resumption is currently active.
    pub fn is_resuming_smooth(&self) -> bool {
        matches!(self, Self::ResumingSmooth { .. })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat_update() -> PredictiveCruiseEvent {
        PredictiveCruiseEvent::Update {
            speed_mph: 65.0,
            target_mph: 65.0,
            grade_here: 0.0,
            climb_ahead: 0.0,
            descent_ahead: 0.0,
            crest_preview: 0.0,
            jake_available: true,
            jake_ceiling: 3,
        }
    }

    #[test]
    fn transition_table() {
        let cases: &[(&str, PredictiveCruiseState, PredictiveCruiseEvent, &str)] = &[
            (
                "flat cruise stays cruising",
                PredictiveCruiseState::Cruising,
                flat_update(),
                "cruising",
            ),
            (
                "climb ahead banks momentum",
                PredictiveCruiseState::Cruising,
                PredictiveCruiseEvent::Update {
                    speed_mph: 65.0,
                    target_mph: 65.0,
                    grade_here: 0.0,
                    climb_ahead: 0.04,
                    descent_ahead: 0.0,
                    crest_preview: 0.0,
                    jake_available: true,
                    jake_ceiling: 3,
                },
                "building_for_climb",
            ),
            (
                "top of climb holds over crest",
                PredictiveCruiseState::BuildingForClimb,
                PredictiveCruiseEvent::Update {
                    speed_mph: 60.0,
                    target_mph: 65.0,
                    grade_here: 0.04,
                    climb_ahead: 0.0,
                    descent_ahead: 0.0,
                    crest_preview: 0.005,
                    jake_available: true,
                    jake_ceiling: 3,
                },
                "holding_crest",
            ),
            (
                "downgrade ahead pre-engages jake",
                PredictiveCruiseState::HoldingCrest,
                PredictiveCruiseEvent::Update {
                    speed_mph: 65.0,
                    target_mph: 65.0,
                    grade_here: 0.0,
                    climb_ahead: 0.0,
                    descent_ahead: -0.05,
                    crest_preview: -0.04,
                    jake_available: true,
                    jake_ceiling: 3,
                },
                "jake_anticipating",
            ),
            (
                "downgrade without jake stays cruising",
                PredictiveCruiseState::Cruising,
                PredictiveCruiseEvent::Update {
                    speed_mph: 65.0,
                    target_mph: 65.0,
                    grade_here: 0.0,
                    climb_ahead: 0.0,
                    descent_ahead: -0.05,
                    crest_preview: -0.04,
                    jake_available: false,
                    jake_ceiling: 0,
                },
                "cruising",
            ),
            (
                "work zone or green light starts smooth resume",
                PredictiveCruiseState::Cruising,
                PredictiveCruiseEvent::TriggerSmoothResume { target_mph: 65.0 },
                "resuming_smooth",
            ),
            (
                "smooth resume stays active while below target",
                PredictiveCruiseState::ResumingSmooth {
                    target_mph: 65.0,
                    ramp_mph_per_s: SMOOTH_RESUME_MPH_PER_S,
                },
                PredictiveCruiseEvent::Update {
                    speed_mph: 48.0,
                    target_mph: 65.0,
                    grade_here: 0.0,
                    climb_ahead: 0.0,
                    descent_ahead: 0.0,
                    crest_preview: 0.0,
                    jake_available: true,
                    jake_ceiling: 3,
                },
                "resuming_smooth",
            ),
            (
                "smooth resume finishes once at target",
                PredictiveCruiseState::ResumingSmooth {
                    target_mph: 65.0,
                    ramp_mph_per_s: SMOOTH_RESUME_MPH_PER_S,
                },
                PredictiveCruiseEvent::Update {
                    speed_mph: 64.0,
                    target_mph: 65.0,
                    grade_here: 0.0,
                    climb_ahead: 0.0,
                    descent_ahead: 0.0,
                    crest_preview: 0.0,
                    jake_available: true,
                    jake_ceiling: 3,
                },
                "cruising",
            ),
            (
                "downgrade near target interrupts smooth resume",
                PredictiveCruiseState::ResumingSmooth {
                    target_mph: 65.0,
                    ramp_mph_per_s: SMOOTH_RESUME_MPH_PER_S,
                },
                PredictiveCruiseEvent::Update {
                    speed_mph: 63.2,
                    target_mph: 65.0,
                    grade_here: -0.03,
                    climb_ahead: 0.0,
                    descent_ahead: -0.04,
                    crest_preview: -0.04,
                    jake_available: true,
                    jake_ceiling: 3,
                },
                "jake_anticipating",
            ),
            (
                "well below target on downgrade does not drag jake",
                PredictiveCruiseState::Cruising,
                PredictiveCruiseEvent::Update {
                    speed_mph: 50.0,
                    target_mph: 65.0,
                    grade_here: -0.03,
                    climb_ahead: 0.0,
                    descent_ahead: -0.04,
                    crest_preview: -0.04,
                    jake_available: true,
                    jake_ceiling: 3,
                },
                "cruising",
            ),
            (
                "disengage resets to cruising",
                PredictiveCruiseState::JakeAnticipating { target_stage: 2 },
                PredictiveCruiseEvent::Disengaged,
                "cruising",
            ),
        ];

        for (label, start, event, expected_key) in cases {
            let after = start.next(*event);
            assert_eq!(after.key(), *expected_key, "case failed: {label}");
        }
    }

    #[test]
    fn steep_descent_requests_stage_two_mild_requests_stage_one() {
        let mild = PredictiveCruiseState::Cruising.next(PredictiveCruiseEvent::Update {
            speed_mph: 65.0,
            target_mph: 65.0,
            grade_here: 0.0,
            climb_ahead: 0.0,
            descent_ahead: -0.03,
            crest_preview: -0.03,
            jake_available: true,
            jake_ceiling: 3,
        });
        assert_eq!(mild.anticipated_jake_stage(), 1);

        let steep = PredictiveCruiseState::Cruising.next(PredictiveCruiseEvent::Update {
            speed_mph: 65.0,
            target_mph: 65.0,
            grade_here: 0.0,
            climb_ahead: 0.0,
            descent_ahead: -0.06,
            crest_preview: -0.06,
            jake_available: true,
            jake_ceiling: 3,
        });
        assert_eq!(steep.anticipated_jake_stage(), 2);
    }

    #[test]
    fn tag_round_trip() {
        for tag in [
            "cruising",
            "building_for_climb",
            "holding_crest",
            "jake_anticipating",
            "resuming_smooth",
        ] {
            let state = PredictiveCruiseState::from_tag(tag).expect(tag);
            assert_eq!(state.key(), tag);
        }
        assert_eq!(PredictiveCruiseState::from_tag("bogus"), None);
    }
}
