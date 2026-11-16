//! The clutch a touch screen works for the player in a manual truck.
//!
//! A manual shift needs the clutch pressed, and the clutch is a held key
//! (Shift) or the controller's left bumper. A phone has neither, so a shift
//! asked for by touch presses the clutch itself, takes the gear, and lets the
//! clutch out again after the time an automatic's shift interrupts torque.
//! The player hears the gear and the clunk as it takes, as from a keyboard,
//! and never the grind.
//!
//! One enum, one [`TouchClutch::next`]: the driving state feeds it a shift,
//! each frame's time, and a cancel when the gearbox goes automatic, the
//! co-driver takes the wheel, or a keyboard or controller takes over.

use crate::sim::transmission::{shift_time_for, DOWNSHIFT_TIME, NEUTRAL, REVERSE};

/// Whether the touch clutch is pressed, and for how much longer.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum TouchClutch {
    /// Let out: the key or the controller decides the clutch.
    #[default]
    Out,
    /// Pressed for a shift, with this many seconds left before it comes out.
    Shifting { left_s: f64 },
}

/// What moves the touch clutch.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TouchClutchEvent {
    /// A shift was asked for by touch, from gear `from` to gear `to`.
    Shift { from: i32, to: i32 },
    /// This many seconds of driving passed.
    Tick { dt: f64 },
    /// The gearbox went automatic, the co-driver took the wheel, or a
    /// keyboard or controller took over.
    Cancel,
}

impl TouchClutch {
    /// The one place the touch clutch moves.
    pub fn next(self, event: TouchClutchEvent) -> TouchClutch {
        match (self, event) {
            (_, TouchClutchEvent::Cancel) => TouchClutch::Out,
            (_, TouchClutchEvent::Shift { from, to }) => TouchClutch::Shifting {
                left_s: hold_for(from, to),
            },
            (TouchClutch::Out, TouchClutchEvent::Tick { .. }) => TouchClutch::Out,
            (TouchClutch::Shifting { left_s }, TouchClutchEvent::Tick { dt }) => {
                let left_s = left_s - dt.max(0.0);
                if left_s > 0.0 {
                    TouchClutch::Shifting { left_s }
                } else {
                    TouchClutch::Out
                }
            }
        }
    }

    /// The pedal position it holds: fully pressed while shifting.
    pub fn pedal(self) -> f64 {
        match self {
            TouchClutch::Out => 0.0,
            TouchClutch::Shifting { .. } => 1.0,
        }
    }

    pub fn pressed(self) -> bool {
        matches!(self, TouchClutch::Shifting { .. })
    }

    /// The name the agent server's `status` reports.
    pub fn name(self) -> &'static str {
        match self {
            TouchClutch::Out => "out",
            TouchClutch::Shifting { .. } => "shifting",
        }
    }
}

/// How long the clutch stays in: the automatic's own interruption for the
/// same change, an upshift by the gear it engages, a downshift at the
/// rev-matched figure. Reverse is engaged from a stop, as first is.
fn hold_for(from: i32, to: i32) -> f64 {
    match to {
        REVERSE | NEUTRAL => shift_time_for(1),
        _ if from >= 1 && to < from => DOWNSHIFT_TIME,
        _ => shift_time_for(to),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::transmission::{SHIFT_TIME, SHIFT_TIME_LOW};
    use TouchClutch::{Out, Shifting};
    use TouchClutchEvent::{Cancel, Shift, Tick};

    #[test]
    fn transition_table() {
        let pressed = |left_s| Shifting { left_s };
        let cases: &[(TouchClutch, TouchClutchEvent, TouchClutch)] = &[
            (Out, Shift { from: 0, to: 1 }, pressed(SHIFT_TIME_LOW)),
            (Out, Shift { from: 9, to: 10 }, pressed(SHIFT_TIME)),
            (Out, Shift { from: 5, to: 4 }, pressed(DOWNSHIFT_TIME)),
            (
                Out,
                Shift {
                    from: 0,
                    to: REVERSE,
                },
                pressed(SHIFT_TIME_LOW),
            ),
            (
                Out,
                Shift {
                    from: REVERSE,
                    to: 1,
                },
                pressed(SHIFT_TIME_LOW),
            ),
            (
                pressed(0.1),
                Shift { from: 2, to: 3 },
                pressed(SHIFT_TIME_LOW),
            ),
            (Out, Tick { dt: 0.1 }, Out),
            (pressed(0.25), Tick { dt: 0.1 }, pressed(0.15)),
            (pressed(0.25), Tick { dt: 0.25 }, Out),
            (pressed(0.25), Tick { dt: 1.0 }, Out),
            (pressed(0.25), Tick { dt: -1.0 }, pressed(0.25)),
            (pressed(0.25), Cancel, Out),
            (Out, Cancel, Out),
        ];
        for (from, event, want) in cases {
            let got = from.next(*event);
            match (got, want) {
                (Shifting { left_s: a }, Shifting { left_s: b }) => {
                    assert!((a - b).abs() < 1e-9, "{from:?} + {event:?} = {got:?}")
                }
                _ => assert_eq!(got, *want, "{from:?} + {event:?}"),
            }
        }
    }

    #[test]
    fn the_pedal_is_down_only_while_shifting() {
        assert_eq!(Out.pedal(), 0.0);
        assert_eq!(Shifting { left_s: 0.2 }.pedal(), 1.0);
        assert_eq!(Out.name(), "out");
        assert_eq!(Shifting { left_s: 0.2 }.name(), "shifting");
    }
}
