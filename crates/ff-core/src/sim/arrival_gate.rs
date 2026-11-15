//! The stop at a facility gate: from the trip ending at a delivery or pickup
//! gate to the dock or check-in opening.
//!
//! One enum, [`GateStop`], with one [`GateStop::next`] and nothing else
//! allowed to move it. Each transition yields at most one [`GateCue`]; the
//! driving state speaks it and keeps every other side effect (cruise, sound,
//! status, the menus).
//!
//! Without the destination approach assist the driver stops the truck. It
//! opens on its own at rest with the parking brake set, and the confirm key
//! opens it at rest with or without the brake. At rest with the brake off
//! for [`GATE_BRAKE_PROMPT_S`] of quiet, the gate says once what is missing;
//! that line re-arms only after the truck rolls above [`DELIVERY_PARK_MPH`]
//! and comes to rest again. With the assist on, the assist stops the truck,
//! sets the brake and holds for the confirm key.
//!
//! The rolling reminder's cadence ([`GATE_REMINDER_INTERVAL_S`]) is a timer
//! the driving state counts down; the machine says when the truck is rolling.

/// At or under this, the gate prompts the truck to stop.
pub const DELIVERY_PARK_MPH: f64 = 3.0;
/// Dock, settle and rest actions need a complete stop.
pub const DOCKING_MAX_MPH: f64 = 0.5;
/// How often a facility gate re-speaks its stop instruction while the truck
/// is still rolling past it. The one-shot warnings latch, so without a
/// cadence a player who overshot the gate at speed heard them once, minutes
/// ago, and got silence for the rest of the drive (playtest 2026-07-22: six
/// minutes and the on-time bonus lost three miles past a delivery entrance).
pub const GATE_REMINDER_INTERVAL_S: f64 = 10.0;
/// Real seconds at rest with the parking brake off, and with the driving
/// voice quiet, before the gate names the brake. Long enough that a driver
/// who stops and reaches for the brake or the confirm key hears nothing;
/// short enough that one who stopped and waits for the dock is not left in
/// silence (first player report, 1.9.6: stopped at the gate, brake off, a
/// minute of nothing). Quiet seconds, because a truck that stops while "Stop
/// and set the parking brake." is still being spoken used up the delay under
/// that line and heard the brake named twice back to back (recorded drive,
/// 2026-11-14).
pub const GATE_BRAKE_PROMPT_S: f64 = 3.0;

/// Where the stop at the gate stands.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum GateStop {
    /// The trip has not ended at a gate, or ended there with nothing said.
    #[default]
    Arriving,
    /// Above the park speed, told to stop at the gate. `crept` once the
    /// creep line has been heard, which is never said twice.
    Rolling { crept: bool },
    /// At or under the park speed, told to stop and set the parking brake.
    Creeping,
    /// At rest with the parking brake off, counting quiet seconds toward
    /// the brake line.
    Stopped { rest_s: f64 },
    /// At rest with the parking brake off, told to set it.
    StoppedTold,
    /// The assist stopped the truck, set the brake and waits for the key.
    Holding,
    /// The dock or check-in is opening.
    Open,
}

/// What happened at the gate this time round.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GateEvent {
    /// One frame at the gate with the driver stopping the truck. `quiet_s`
    /// is how much of the frame the driving voice was silent for: the whole
    /// frame, or nothing while a line is being spoken.
    Reading {
        speed_mph: f64,
        parking_brake: bool,
        quiet_s: f64,
    },
    /// One frame at the gate with the approach assist braking the truck.
    AssistReading { speed_mph: f64 },
    /// The confirm key (Enter or controller A).
    Confirm {
        speed_mph: f64,
        parking_brake: bool,
        assist: bool,
    },
    /// The trip no longer ends at this gate (the missed-gate loop-back).
    Reset,
}

/// The one cue a transition gives the driving state to speak.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GateCue {
    /// "Destination: X. Stop at the gate." (or "Pickup: ...").
    StopAtGate,
    /// "At X. Stop and set the parking brake."
    Creep,
    /// "Set the parking brake to dock." (or "to check in.")
    SetBrake,
    /// "Holding at the entrance."
    Hold,
    /// Open the dock or check-in.
    Open,
}

impl GateStop {
    /// The only place the gate's stop moves.
    pub fn next(self, event: GateEvent) -> (GateStop, Option<GateCue>) {
        use GateStop::*;
        if self == Open {
            return match event {
                GateEvent::Reset => (Arriving, None),
                _ => (Open, None),
            };
        }
        match event {
            GateEvent::Reset => (Arriving, None),
            GateEvent::Confirm {
                speed_mph,
                parking_brake,
                assist,
            } => {
                let ready =
                    speed_mph <= DOCKING_MAX_MPH && (!assist || (self == Holding && parking_brake));
                if ready {
                    (Open, Some(GateCue::Open))
                } else {
                    (self, None)
                }
            }
            GateEvent::AssistReading { speed_mph } => {
                if self != Holding && speed_mph <= DOCKING_MAX_MPH {
                    (Holding, Some(GateCue::Hold))
                } else {
                    (self, None)
                }
            }
            GateEvent::Reading {
                speed_mph,
                parking_brake,
                quiet_s,
            } => self.read(speed_mph, parking_brake, quiet_s),
        }
    }

    fn read(
        self,
        speed_mph: f64,
        parking_brake: bool,
        quiet_s: f64,
    ) -> (GateStop, Option<GateCue>) {
        use GateStop::*;
        let at_rest = speed_mph <= DOCKING_MAX_MPH;
        if at_rest && parking_brake {
            return (Open, Some(GateCue::Open));
        }
        if speed_mph > DELIVERY_PARK_MPH {
            return match self {
                Arriving => (Rolling { crept: false }, Some(GateCue::StopAtGate)),
                Rolling { crept } => (Rolling { crept }, None),
                _ => (Rolling { crept: true }, None),
            };
        }
        match self {
            // First time at or under the park speed: the creep line, rest
            // or not.
            Arriving | Rolling { crept: false } => (Creeping, Some(GateCue::Creep)),
            // Told once, told stays: only rolling off re-arms the brake line.
            StoppedTold => (StoppedTold, None),
            // The assist's hold survives a reading; its key still opens.
            Holding => (Holding, None),
            Stopped { rest_s } if at_rest => {
                let rest_s = rest_s + quiet_s;
                if rest_s >= GATE_BRAKE_PROMPT_S {
                    (StoppedTold, Some(GateCue::SetBrake))
                } else {
                    (Stopped { rest_s }, None)
                }
            }
            Rolling { crept: true } | Creeping if at_rest => (Stopped { rest_s: quiet_s }, None),
            // Creeping, or a stop that rolled again before the line was due.
            _ => (Creeping, None),
        }
    }

    /// Whether the gate's own stop line (the creep line or the assist's
    /// hold) has been said: the "destination ahead" line is no longer worth
    /// rescuing behind it.
    pub fn stop_prompted(self) -> bool {
        !matches!(
            self,
            GateStop::Arriving | GateStop::Rolling { crept: false }
        )
    }

    /// Whether the gate is past its first line (the old say-once latch).
    pub fn told_to_stop(self) -> bool {
        self != GateStop::Arriving
    }

    /// Whether the assist is holding at the entrance for the confirm key.
    pub fn is_holding(self) -> bool {
        self == GateStop::Holding
    }

    /// Whether the truck is above the park speed, told to stop: the rolling
    /// reminder's cadence runs only here.
    pub fn is_rolling(self) -> bool {
        matches!(self, GateStop::Rolling { .. })
    }

    /// A stable key for the agent server's `status`.
    pub fn key(self) -> &'static str {
        match self {
            GateStop::Arriving => "arriving",
            GateStop::Rolling { crept: false } => "rolling",
            GateStop::Rolling { crept: true } => "rolling_after_creep",
            GateStop::Creeping => "creeping",
            GateStop::Stopped { .. } => "stopped",
            GateStop::StoppedTold => "stopped_told",
            GateStop::Holding => "holding",
            GateStop::Open => "open",
        }
    }
}

#[cfg(test)]
mod tests;
