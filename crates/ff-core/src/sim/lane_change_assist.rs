//! Active blind-spot warning and assisted lane-change state machine.
//!
//! Models two related behaviors:
//! - On Balanced/All: directional blind-spot alert when steering toward an
//!   occupied adjacent lane.
//! - On All (full lane keeping): a steer tap into a blocked lane signals,
//!   waits for a safe gap (coordinating with PR #789 courtesy yielding), and
//!   glides smoothly into the target lane once clear.

use serde::{Deserialize, Serialize};

/// Seconds a waiting assisted lane change signals before timing out cleanly.
pub const ASSISTED_LANE_CHANGE_TIMEOUT_S: f64 = 12.0;

/// Minimum interval between repeated blind-spot warning chirps (seconds).
pub const BLIND_SPOT_CHIRP_COOLDOWN_S: f64 = 0.85;

/// Side of the truck for directional blind-spot cues.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum BlindSpotSide {
    Left,
    Right,
}

impl BlindSpotSide {
    /// Stereo pan for the directional blind-spot chirp (-1 left, +1 right).
    pub fn pan(self) -> f64 {
        match self {
            Self::Left => -0.75,
            Self::Right => 0.75,
        }
    }

    /// Direction delta in lane indices (+1 left, -1 right).
    pub fn direction(self) -> i64 {
        match self {
            Self::Left => 1,
            Self::Right => -1,
        }
    }

    pub fn from_direction(direction: i64) -> Option<Self> {
        if direction > 0 {
            Some(Self::Left)
        } else if direction < 0 {
            Some(Self::Right)
        } else {
            None
        }
    }
}

/// Explicit state of assisted lane changes and blind-spot alerts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub enum AssistedLaneChange {
    #[default]
    Idle,
    /// Player steered toward an occupied adjacent lane; blind-spot chirp active.
    BlindSpotAlert {
        side: BlindSpotSide,
        cooldown_s: f64,
    },
    /// On full lane keeping: signaling into `target_lane` and waiting for a
    /// safe gap (courteous traffic yields while `player_signal_lane` is set).
    WaitingForGap {
        target_lane: i64,
        side: BlindSpotSide,
        waited_s: f64,
    },
    /// Safe gap opened; smoothly gliding into `target_lane`.
    Gliding {
        target_lane: i64,
        side: BlindSpotSide,
    },
}

/// Events that drive [`AssistedLaneChange::next`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LaneChangeAssistEvent {
    /// Player steered toward an occupied lane on Balanced/All without queuing
    /// a full-assist wait (or while holding manual steer).
    BlindSpotDetected { side: BlindSpotSide },
    /// Player tapped steer on full lane keeping toward an occupied lane:
    /// signal and wait for a safe gap.
    TapWaitForGap {
        target_lane: i64,
        side: BlindSpotSide,
    },
    /// Player tapped steer on full lane keeping toward an already-open lane.
    TapGlideNow {
        target_lane: i64,
        side: BlindSpotSide,
    },
    /// Frame update while active.
    Tick {
        dt: f64,
        /// Whether the target lane is now open (and not closed by a zone).
        gap_open: bool,
        /// Whether steering is still aimed into an occupied adjacent lane.
        blind_spot_occupied: bool,
        /// Whether the lane change glide has completed (truck reached target).
        glide_complete: bool,
    },
    /// Player canceled (opposite tap, ramp entry, speed drop, or closure).
    Cancel,
}

impl AssistedLaneChange {
    /// Pure state transition function.
    pub fn next(&self, event: LaneChangeAssistEvent) -> Self {
        match event {
            LaneChangeAssistEvent::Cancel => Self::Idle,
            LaneChangeAssistEvent::BlindSpotDetected { side } => match *self {
                // Keep waiting/gliding if already in an assisted lane change.
                Self::WaitingForGap { .. } | Self::Gliding { .. } => *self,
                Self::BlindSpotAlert {
                    side: current,
                    cooldown_s,
                } if current == side && cooldown_s > 0.0 => *self,
                _ => Self::BlindSpotAlert {
                    side,
                    cooldown_s: BLIND_SPOT_CHIRP_COOLDOWN_S,
                },
            },
            LaneChangeAssistEvent::TapWaitForGap { target_lane, side } => match *self {
                // Tapping the opposite direction while waiting cancels cleanly.
                Self::WaitingForGap {
                    side: current_side, ..
                } if current_side != side => Self::Idle,
                Self::WaitingForGap {
                    target_lane: current,
                    ..
                } if current == target_lane => *self,
                _ => Self::WaitingForGap {
                    target_lane,
                    side,
                    waited_s: 0.0,
                },
            },
            LaneChangeAssistEvent::TapGlideNow { target_lane, side } => {
                Self::Gliding { target_lane, side }
            }
            LaneChangeAssistEvent::Tick {
                dt,
                gap_open,
                blind_spot_occupied,
                glide_complete,
            } => {
                let dt = dt.max(0.0);
                match *self {
                    Self::Idle => Self::Idle,
                    Self::BlindSpotAlert { side, cooldown_s } => {
                        let left = (cooldown_s - dt).max(0.0);
                        if left <= 0.0 && !blind_spot_occupied {
                            Self::Idle
                        } else {
                            Self::BlindSpotAlert {
                                side,
                                cooldown_s: left,
                            }
                        }
                    }
                    Self::WaitingForGap {
                        target_lane,
                        side,
                        waited_s,
                    } => {
                        if gap_open {
                            Self::Gliding { target_lane, side }
                        } else if waited_s + dt >= ASSISTED_LANE_CHANGE_TIMEOUT_S {
                            Self::Idle
                        } else {
                            Self::WaitingForGap {
                                target_lane,
                                side,
                                waited_s: waited_s + dt,
                            }
                        }
                    }
                    Self::Gliding { target_lane, side } => {
                        if glide_complete {
                            Self::Idle
                        } else {
                            Self::Gliding { target_lane, side }
                        }
                    }
                }
            }
        }
    }

    /// Short machine-readable tag for `DrivingObservation` and Agent MCP `status`.
    pub fn key(&self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::BlindSpotAlert {
                side: BlindSpotSide::Left,
                ..
            } => "blind_spot_left",
            Self::BlindSpotAlert {
                side: BlindSpotSide::Right,
                ..
            } => "blind_spot_right",
            Self::WaitingForGap {
                side: BlindSpotSide::Left,
                ..
            } => "waiting_for_gap_left",
            Self::WaitingForGap {
                side: BlindSpotSide::Right,
                ..
            } => "waiting_for_gap_right",
            Self::Gliding {
                side: BlindSpotSide::Left,
                ..
            } => "gliding_left",
            Self::Gliding {
                side: BlindSpotSide::Right,
                ..
            } => "gliding_right",
        }
    }

    /// Parse a scenario tag into a staged state.
    pub fn from_tag(tag: &str) -> Option<Self> {
        match tag {
            "idle" => Some(Self::Idle),
            "blind_spot_left" => Some(Self::BlindSpotAlert {
                side: BlindSpotSide::Left,
                cooldown_s: BLIND_SPOT_CHIRP_COOLDOWN_S,
            }),
            "blind_spot_right" => Some(Self::BlindSpotAlert {
                side: BlindSpotSide::Right,
                cooldown_s: BLIND_SPOT_CHIRP_COOLDOWN_S,
            }),
            "waiting_for_gap_left" => Some(Self::WaitingForGap {
                target_lane: 1,
                side: BlindSpotSide::Left,
                waited_s: 0.0,
            }),
            "waiting_for_gap_right" => Some(Self::WaitingForGap {
                target_lane: 0,
                side: BlindSpotSide::Right,
                waited_s: 0.0,
            }),
            "gliding_left" => Some(Self::Gliding {
                target_lane: 1,
                side: BlindSpotSide::Left,
            }),
            "gliding_right" => Some(Self::Gliding {
                target_lane: 0,
                side: BlindSpotSide::Right,
            }),
            _ => None,
        }
    }

    /// Target lane being signaled into while waiting for a gap, if any.
    pub fn waiting_target(&self) -> Option<(i64, BlindSpotSide)> {
        match *self {
            Self::WaitingForGap {
                target_lane, side, ..
            } => Some((target_lane, side)),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transition_table() {
        let cases: &[(&str, AssistedLaneChange, LaneChangeAssistEvent, &str)] = &[
            (
                "blind spot left triggers alert",
                AssistedLaneChange::Idle,
                LaneChangeAssistEvent::BlindSpotDetected {
                    side: BlindSpotSide::Left,
                },
                "blind_spot_left",
            ),
            (
                "blind spot alert clears after cooldown when lane is clear",
                AssistedLaneChange::BlindSpotAlert {
                    side: BlindSpotSide::Left,
                    cooldown_s: 0.2,
                },
                LaneChangeAssistEvent::Tick {
                    dt: 0.3,
                    gap_open: true,
                    blind_spot_occupied: false,
                    glide_complete: false,
                },
                "idle",
            ),
            (
                "tap into blocked lane enters waiting_for_gap",
                AssistedLaneChange::Idle,
                LaneChangeAssistEvent::TapWaitForGap {
                    target_lane: 1,
                    side: BlindSpotSide::Left,
                },
                "waiting_for_gap_left",
            ),
            (
                "waiting_for_gap transitions to gliding when gap opens",
                AssistedLaneChange::WaitingForGap {
                    target_lane: 1,
                    side: BlindSpotSide::Left,
                    waited_s: 1.0,
                },
                LaneChangeAssistEvent::Tick {
                    dt: 0.1,
                    gap_open: true,
                    blind_spot_occupied: false,
                    glide_complete: false,
                },
                "gliding_left",
            ),
            (
                "waiting_for_gap times out cleanly",
                AssistedLaneChange::WaitingForGap {
                    target_lane: 1,
                    side: BlindSpotSide::Left,
                    waited_s: ASSISTED_LANE_CHANGE_TIMEOUT_S - 0.1,
                },
                LaneChangeAssistEvent::Tick {
                    dt: 0.2,
                    gap_open: false,
                    blind_spot_occupied: true,
                    glide_complete: false,
                },
                "idle",
            ),
            (
                "opposite tap cancels waiting_for_gap",
                AssistedLaneChange::WaitingForGap {
                    target_lane: 1,
                    side: BlindSpotSide::Left,
                    waited_s: 1.0,
                },
                LaneChangeAssistEvent::TapWaitForGap {
                    target_lane: 0,
                    side: BlindSpotSide::Right,
                },
                "idle",
            ),
            (
                "gliding finishes back to idle",
                AssistedLaneChange::Gliding {
                    target_lane: 1,
                    side: BlindSpotSide::Left,
                },
                LaneChangeAssistEvent::Tick {
                    dt: 0.1,
                    gap_open: true,
                    blind_spot_occupied: false,
                    glide_complete: true,
                },
                "idle",
            ),
            (
                "cancel resets to idle",
                AssistedLaneChange::Gliding {
                    target_lane: 1,
                    side: BlindSpotSide::Left,
                },
                LaneChangeAssistEvent::Cancel,
                "idle",
            ),
        ];

        for (label, start, event, expected_key) in cases {
            let after = start.next(*event);
            assert_eq!(after.key(), *expected_key, "case failed: {label}");
        }
    }

    #[test]
    fn tag_round_trip() {
        for tag in [
            "idle",
            "blind_spot_left",
            "blind_spot_right",
            "waiting_for_gap_left",
            "waiting_for_gap_right",
            "gliding_left",
            "gliding_right",
        ] {
            let state = AssistedLaneChange::from_tag(tag).expect(tag);
            assert_eq!(state.key(), tag);
        }
        assert_eq!(AssistedLaneChange::from_tag("unknown"), None);
    }
}
