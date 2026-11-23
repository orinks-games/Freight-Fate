//! Quick left/right mirror & blind-spot glance state machine.
//!
//! Pressing the left or right mirror glance key shifts audio emphasis toward
//! that side of the truck for a short window while giving a terse blind-spot
//! readout, then automatically returns to `Forward`.

/// Duration of a quick mirror glance before automatically returning forward.
pub const MIRROR_GLANCE_DURATION_S: f64 = 2.5;
/// Gain multiplier applied to traffic audio on the glanced side.
pub const MIRROR_FOCUS_BOOST: f64 = 1.45;
/// Gain multiplier applied to traffic audio on the opposite side during a glance.
pub const MIRROR_OPPOSITE_ATTEN: f64 = 0.55;
/// Minimum absolute stereo pan threshold to treat a vehicle as left/right of cab.
const PAN_SIDE_THRESHOLD: f64 = 0.1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MirrorSide {
    Left,
    Right,
}

impl MirrorSide {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Left => "left",
            Self::Right => "right",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum MirrorGlance {
    #[default]
    Forward,
    Left {
        remaining_s: f64,
    },
    Right {
        remaining_s: f64,
    },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MirrorGlanceEvent {
    Glance(MirrorSide),
    Cancel,
    Tick(f64),
}

impl MirrorGlance {
    /// Deterministic state transition for a mirror glance.
    ///
    /// Glancing toward a side already being checked snaps back to `Forward`;
    /// glancing to the opposite side switches immediately with a fresh timer.
    pub fn next(&self, event: MirrorGlanceEvent) -> Self {
        match (*self, event) {
            (_, MirrorGlanceEvent::Cancel) => Self::Forward,
            (Self::Forward, MirrorGlanceEvent::Glance(MirrorSide::Left))
            | (Self::Right { .. }, MirrorGlanceEvent::Glance(MirrorSide::Left)) => Self::Left {
                remaining_s: MIRROR_GLANCE_DURATION_S,
            },
            (Self::Left { .. }, MirrorGlanceEvent::Glance(MirrorSide::Left)) => Self::Forward,
            (Self::Forward, MirrorGlanceEvent::Glance(MirrorSide::Right))
            | (Self::Left { .. }, MirrorGlanceEvent::Glance(MirrorSide::Right)) => Self::Right {
                remaining_s: MIRROR_GLANCE_DURATION_S,
            },
            (Self::Right { .. }, MirrorGlanceEvent::Glance(MirrorSide::Right)) => Self::Forward,
            (Self::Forward, MirrorGlanceEvent::Tick(_)) => Self::Forward,
            (Self::Left { remaining_s }, MirrorGlanceEvent::Tick(dt)) => {
                let left = remaining_s - dt.max(0.0);
                if left <= 0.0 {
                    Self::Forward
                } else {
                    Self::Left { remaining_s: left }
                }
            }
            (Self::Right { remaining_s }, MirrorGlanceEvent::Tick(dt)) => {
                let left = remaining_s - dt.max(0.0);
                if left <= 0.0 {
                    Self::Forward
                } else {
                    Self::Right { remaining_s: left }
                }
            }
        }
    }

    pub const fn active_side(&self) -> Option<MirrorSide> {
        match self {
            Self::Forward => None,
            Self::Left { .. } => Some(MirrorSide::Left),
            Self::Right { .. } => Some(MirrorSide::Right),
        }
    }

    pub const fn label(&self) -> &'static str {
        match self {
            Self::Forward => "forward",
            Self::Left { .. } => "left",
            Self::Right { .. } => "right",
        }
    }

    /// Gain multiplier for a vehicle at stereo `pan` (`-1.0` left, `+1.0` right).
    pub fn audio_gain_for_pan(&self, pan: f64) -> f64 {
        match self {
            Self::Forward => 1.0,
            Self::Left { .. } => {
                if pan < -PAN_SIDE_THRESHOLD {
                    MIRROR_FOCUS_BOOST
                } else if pan > PAN_SIDE_THRESHOLD {
                    MIRROR_OPPOSITE_ATTEN
                } else {
                    1.0
                }
            }
            Self::Right { .. } => {
                if pan > PAN_SIDE_THRESHOLD {
                    MIRROR_FOCUS_BOOST
                } else if pan < -PAN_SIDE_THRESHOLD {
                    MIRROR_OPPOSITE_ATTEN
                } else {
                    1.0
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transition_table() {
        let cases = [
            (
                "forward + glance left -> left",
                MirrorGlance::Forward,
                MirrorGlanceEvent::Glance(MirrorSide::Left),
                MirrorGlance::Left {
                    remaining_s: MIRROR_GLANCE_DURATION_S,
                },
            ),
            (
                "forward + glance right -> right",
                MirrorGlance::Forward,
                MirrorGlanceEvent::Glance(MirrorSide::Right),
                MirrorGlance::Right {
                    remaining_s: MIRROR_GLANCE_DURATION_S,
                },
            ),
            (
                "left + glance left -> forward",
                MirrorGlance::Left { remaining_s: 1.5 },
                MirrorGlanceEvent::Glance(MirrorSide::Left),
                MirrorGlance::Forward,
            ),
            (
                "right + glance right -> forward",
                MirrorGlance::Right { remaining_s: 1.5 },
                MirrorGlanceEvent::Glance(MirrorSide::Right),
                MirrorGlance::Forward,
            ),
            (
                "left + glance right -> right fresh",
                MirrorGlance::Left { remaining_s: 0.4 },
                MirrorGlanceEvent::Glance(MirrorSide::Right),
                MirrorGlance::Right {
                    remaining_s: MIRROR_GLANCE_DURATION_S,
                },
            ),
            (
                "right + glance left -> left fresh",
                MirrorGlance::Right { remaining_s: 0.4 },
                MirrorGlanceEvent::Glance(MirrorSide::Left),
                MirrorGlance::Left {
                    remaining_s: MIRROR_GLANCE_DURATION_S,
                },
            ),
            (
                "left + cancel -> forward",
                MirrorGlance::Left { remaining_s: 2.0 },
                MirrorGlanceEvent::Cancel,
                MirrorGlance::Forward,
            ),
            (
                "left + partial tick -> left decremented",
                MirrorGlance::Left { remaining_s: 2.0 },
                MirrorGlanceEvent::Tick(0.5),
                MirrorGlance::Left { remaining_s: 1.5 },
            ),
            (
                "left + expiring tick -> forward",
                MirrorGlance::Left { remaining_s: 0.5 },
                MirrorGlanceEvent::Tick(0.5),
                MirrorGlance::Forward,
            ),
            (
                "right + expiring tick -> forward",
                MirrorGlance::Right { remaining_s: 0.2 },
                MirrorGlanceEvent::Tick(0.5),
                MirrorGlance::Forward,
            ),
            (
                "forward + tick -> forward",
                MirrorGlance::Forward,
                MirrorGlanceEvent::Tick(1.0),
                MirrorGlance::Forward,
            ),
        ];

        for (label, start, event, expected) in cases {
            assert_eq!(start.next(event), expected, "{label}");
        }
    }

    #[test]
    fn audio_gain_biases_glanced_side() {
        let left = MirrorGlance::Left { remaining_s: 1.0 };
        assert!((left.audio_gain_for_pan(-0.6) - MIRROR_FOCUS_BOOST).abs() < 1e-9);
        assert!((left.audio_gain_for_pan(0.6) - MIRROR_OPPOSITE_ATTEN).abs() < 1e-9);
        assert!((left.audio_gain_for_pan(0.0) - 1.0).abs() < 1e-9);

        let right = MirrorGlance::Right { remaining_s: 1.0 };
        assert!((right.audio_gain_for_pan(0.6) - MIRROR_FOCUS_BOOST).abs() < 1e-9);
        assert!((right.audio_gain_for_pan(-0.6) - MIRROR_OPPOSITE_ATTEN).abs() < 1e-9);
        assert!((MirrorGlance::Forward.audio_gain_for_pan(-0.8) - 1.0).abs() < 1e-9);
    }
}
