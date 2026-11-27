//! Lane-keeping availability state machine.
//!
//! Tracks whether lane-keeping sensors can read lane markings or have dropped
//! out, giving a clean edge for the dropout earcon and Agent MCP inspection.
//!
//! Nothing raises `DroppedOut` today. Lane keeping steers whenever it is
//! switched on, full or partial, in snow and in freezing rain (Orinks's
//! ruling, 2026-11-27): the snow and ice dropout handed the wheel back with a
//! tone alone and put trucks off the road (2.0 alpha report). The game
//! evaluates with `snow_covered` false; the machine, its scenario tag and
//! status key stay in place, and so does the tone asset.

use serde::{Deserialize, Serialize};

/// Why lane keeping dropped out.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum LaneKeepDropoutReason {
    /// Snow or ice obscures the painted lane markings.
    SnowCovered,
}

/// Explicit state of lane-keeping availability.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum LaneKeepAvailability {
    /// Lane keeping is turned off in settings.
    #[default]
    Off,
    /// Lane keeping is enabled and reading lane markings normally.
    Active,
    /// Lane keeping is enabled, but markings are obscured and assist dropped out.
    DroppedOut { reason: LaneKeepDropoutReason },
}

/// Inputs that drive [`LaneKeepAvailability::next`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LaneKeepStatusEvent {
    /// Frame evaluation of lane-keeping conditions.
    Evaluate {
        /// Whether lane keeping is enabled ("partial" or "full").
        enabled: bool,
        /// Whether the road surface is snow- or ice-covered enough to obscure
        /// lane lines.
        snow_covered: bool,
    },
}

impl LaneKeepAvailability {
    /// Pure state transition function.
    pub fn next(&self, event: LaneKeepStatusEvent) -> Self {
        match event {
            LaneKeepStatusEvent::Evaluate { enabled: false, .. } => Self::Off,
            LaneKeepStatusEvent::Evaluate {
                enabled: true,
                snow_covered: true,
            } => Self::DroppedOut {
                reason: LaneKeepDropoutReason::SnowCovered,
            },
            LaneKeepStatusEvent::Evaluate {
                enabled: true,
                snow_covered: false,
            } => Self::Active,
        }
    }

    /// True when this transition represents a fresh dropout (Active -> DroppedOut).
    pub fn dropped_out_from(previous: Self, current: Self) -> bool {
        matches!(previous, Self::Active) && matches!(current, Self::DroppedOut { .. })
    }

    /// True when lane-keep is currently dropped out.
    pub fn is_dropped_out(&self) -> bool {
        matches!(self, Self::DroppedOut { .. })
    }

    /// Short machine-readable tag for `DrivingObservation` and Agent MCP `status`.
    pub fn key(&self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Active => "active",
            Self::DroppedOut {
                reason: LaneKeepDropoutReason::SnowCovered,
            } => "dropped_out_snow",
        }
    }

    /// Parse a scenario tag into a staged state.
    pub fn from_tag(tag: &str) -> Option<Self> {
        match tag {
            "off" => Some(Self::Off),
            "active" => Some(Self::Active),
            "dropped_out_snow" => Some(Self::DroppedOut {
                reason: LaneKeepDropoutReason::SnowCovered,
            }),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transition_table() {
        let cases: &[(&str, LaneKeepAvailability, LaneKeepStatusEvent, &str, bool)] = &[
            (
                "disabled stays off",
                LaneKeepAvailability::Off,
                LaneKeepStatusEvent::Evaluate {
                    enabled: false,
                    snow_covered: false,
                },
                "off",
                false,
            ),
            (
                "enabled on clear road becomes active",
                LaneKeepAvailability::Off,
                LaneKeepStatusEvent::Evaluate {
                    enabled: true,
                    snow_covered: false,
                },
                "active",
                false,
            ),
            (
                "active on snow drops out and fires edge",
                LaneKeepAvailability::Active,
                LaneKeepStatusEvent::Evaluate {
                    enabled: true,
                    snow_covered: true,
                },
                "dropped_out_snow",
                true,
            ),
            (
                "staying dropped out does not refire edge",
                LaneKeepAvailability::DroppedOut {
                    reason: LaneKeepDropoutReason::SnowCovered,
                },
                LaneKeepStatusEvent::Evaluate {
                    enabled: true,
                    snow_covered: true,
                },
                "dropped_out_snow",
                false,
            ),
            (
                "clearing snow restores active",
                LaneKeepAvailability::DroppedOut {
                    reason: LaneKeepDropoutReason::SnowCovered,
                },
                LaneKeepStatusEvent::Evaluate {
                    enabled: true,
                    snow_covered: false,
                },
                "active",
                false,
            ),
            (
                "disabling while dropped out returns to off",
                LaneKeepAvailability::DroppedOut {
                    reason: LaneKeepDropoutReason::SnowCovered,
                },
                LaneKeepStatusEvent::Evaluate {
                    enabled: false,
                    snow_covered: true,
                },
                "off",
                false,
            ),
        ];

        for (label, start, event, expected_key, expected_edge) in cases {
            let after = start.next(*event);
            assert_eq!(after.key(), *expected_key, "case failed: {label}");
            assert_eq!(
                LaneKeepAvailability::dropped_out_from(*start, after),
                *expected_edge,
                "edge mismatch: {label}"
            );
        }
    }

    #[test]
    fn tag_round_trip() {
        for tag in ["off", "active", "dropped_out_snow"] {
            let state = LaneKeepAvailability::from_tag(tag).expect(tag);
            assert_eq!(state.key(), tag);
        }
        assert_eq!(LaneKeepAvailability::from_tag("rain"), None);
    }
}
