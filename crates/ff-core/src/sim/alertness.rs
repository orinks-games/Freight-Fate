//! The two fatigue state machines: the alertness band and the nod-off.
//!
//! Fatigue stays one number, 0 to 100, on the driver profile. What the
//! driver hears about it moves through two machines, each with one `next`
//! function and nothing else allowed to move it (`docs/fatigue-design.md`,
//! "The state machines"):
//!
//! * [`Alertness`] is the band the number sits in. Entering a band higher up
//!   gives one cue; a band is left downward only [`BAND_EXIT_MARGIN`] points
//!   below where it was entered, so a small lift does not replay the line.
//! * [`NodState`] is the involuntary nod-off severe fatigue brings on: the
//!   game minutes counted toward the next nod, the reaction window while one
//!   is open, the quiet period after it, and the misses toward the forced
//!   stop.
//!
//! The driving state keeps every side effect (sounds, speech, damage, the
//! record) and matches on what `next` returns. Both machines round-trip
//! through the trip snapshot as plain JSON.

use serde_json::{json, Value};

use super::hos::{FATIGUE_DROWSY, FATIGUE_SEVERE, FATIGUE_WORN};

/// How far below a band's entry the number must fall before the band is
/// left downward.
pub const BAND_EXIT_MARGIN: f64 = 5.0;

/// Real seconds to steer or brake once a nod begins.
pub const MICROSLEEP_REACTION_S: f64 = 2.2;
/// Game minutes between nods at the severe threshold...
pub const MICROSLEEP_BASE_GM: f64 = 9.0;
/// ...shrinking to this nearer total exhaustion.
pub const MICROSLEEP_MIN_GM: f64 = 3.0;
/// Game minutes of quiet after a nod resolves, however it resolved.
pub const MICROSLEEP_COOLDOWN_GM: f64 = 4.0;
/// Consecutive missed nods that force the stop.
pub const MICROSLEEP_FORCE_STOP_MISSES: u8 = 3;

// ---------------------------------------------------------------------------
// The band
// ---------------------------------------------------------------------------

/// How tired the driver is, as a band of the fatigue number.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Alertness {
    /// Below 40.
    #[default]
    Rested,
    /// 40 to 60.
    Worn,
    /// 60 to 80.
    Drowsy,
    /// 80 and up.
    Severe,
}

/// The one cue for entering a band from below.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BandCue {
    /// One quiet yawn, no speech.
    Worn,
    /// A yawn and "Getting drowsy."
    Drowsy,
    /// A rumble strip hit and "Dangerously drowsy and drifting out of your lane."
    Severe,
}

impl Alertness {
    pub const ALL: [Alertness; 4] = [
        Alertness::Rested,
        Alertness::Worn,
        Alertness::Drowsy,
        Alertness::Severe,
    ];

    /// The fatigue number at which the band is entered from below.
    pub fn entry(self) -> f64 {
        match self {
            Alertness::Rested => 0.0,
            Alertness::Worn => FATIGUE_WORN,
            Alertness::Drowsy => FATIGUE_DROWSY,
            Alertness::Severe => FATIGUE_SEVERE,
        }
    }

    /// The band a number falls in with no history: for rebuilding the band
    /// from an old save, and for readers that judge one moment (a
    /// billboard, the rest achievement) rather than track a drive.
    pub fn from_fatigue(fatigue: f64) -> Alertness {
        if fatigue >= FATIGUE_SEVERE {
            Alertness::Severe
        } else if fatigue >= FATIGUE_DROWSY {
            Alertness::Drowsy
        } else if fatigue >= FATIGUE_WORN {
            Alertness::Worn
        } else {
            Alertness::Rested
        }
    }

    fn below(self) -> Alertness {
        match self {
            Alertness::Rested | Alertness::Worn => Alertness::Rested,
            Alertness::Drowsy => Alertness::Worn,
            Alertness::Severe => Alertness::Drowsy,
        }
    }

    fn cue(self) -> Option<BandCue> {
        match self {
            Alertness::Rested => None,
            Alertness::Worn => Some(BandCue::Worn),
            Alertness::Drowsy => Some(BandCue::Drowsy),
            Alertness::Severe => Some(BandCue::Severe),
        }
    }

    /// The only place the band moves.
    ///
    /// Upward, the band follows the number at once and gives the cue of the
    /// band landed in, and only that one, however many it crossed. Downward
    /// it is silent, and each band is left only once the number is
    /// [`BAND_EXIT_MARGIN`] below that band's entry.
    pub fn next(self, fatigue: f64) -> (Alertness, Option<BandCue>) {
        let raw = Alertness::from_fatigue(fatigue);
        if raw > self {
            return (raw, raw.cue());
        }
        let mut band = self;
        while band != Alertness::Rested && fatigue < band.entry() - BAND_EXIT_MARGIN {
            band = band.below();
        }
        (band, None)
    }

    /// A stable key for saves and the agent server's `status`.
    pub fn key(self) -> &'static str {
        match self {
            Alertness::Rested => "rested",
            Alertness::Worn => "worn",
            Alertness::Drowsy => "drowsy",
            Alertness::Severe => "severe",
        }
    }

    /// The band a [`key`](Self::key) names.
    pub fn from_key(key: &str) -> Option<Alertness> {
        Alertness::ALL
            .into_iter()
            .find(|band| band.key() == key.trim().to_ascii_lowercase())
    }

    /// The word the status readout puts after the number, from Worn up.
    /// Rested is not news, so it has none.
    pub fn readout_word(self) -> Option<&'static str> {
        match self {
            Alertness::Rested => None,
            Alertness::Worn => Some("worn"),
            Alertness::Drowsy => Some("drowsy"),
            Alertness::Severe => Some("severe"),
        }
    }
}

// ---------------------------------------------------------------------------
// The nod-off
// ---------------------------------------------------------------------------

/// Where the nod-off stands.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Nod {
    /// Counting game minutes toward the next nod.
    Awake { since_gm: f64 },
    /// The reaction window is open: real seconds left to steer or brake.
    Nodding { left_s: f64 },
    /// The quiet period after one resolved, in game minutes.
    Recovering { left_gm: f64 },
}

impl Default for Nod {
    fn default() -> Nod {
        Nod::Awake { since_gm: 0.0 }
    }
}

/// The nod-off machine: where it stands and the misses in a row.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct NodState {
    pub nod: Nod,
    /// Consecutive nods drifted off the road; a catch clears it.
    pub misses: u8,
}

/// What can happen to the nod-off.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum NodEvent {
    /// The game clock moved `gm` minutes with the player at the wheel.
    /// `busy` is a hazard waiting on the driver: one demand at a time.
    Tick {
        gm: f64,
        fatigue: f64,
        moving: bool,
        busy: bool,
    },
    /// Real time passed inside an open reaction window. `crawling` is the
    /// truck already slow enough that the nod passes without leaving the
    /// road.
    Window { dt_s: f64, crawling: bool },
    /// The driver steered or braked.
    Reacted,
    /// The co-driver took the wheel.
    WheelHandedOver,
    /// Open a reaction window now, whatever the count says: a play tester
    /// staging a nod, or a resumed trip that was saved mid-nod.
    Staged,
}

/// What a transition asks the driving state to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NodOutcome {
    None,
    /// A nod began: the warning.
    Began,
    /// The driver reacted in time.
    Caught,
    /// The truck was already crawling: no line.
    CaughtSilent,
    /// The window ran out: the rumble strip, damage and the record.
    DriftedOff,
    /// The window ran out for the third time in a row: the forced stop.
    ForcedStop,
}

/// Game minutes between nods; shrinks from the base toward the floor as
/// exhaustion deepens past the severe threshold.
pub fn microsleep_interval_gm(fatigue: f64) -> f64 {
    let span = 1.0f64.max(100.0 - FATIGUE_SEVERE);
    let t = ((fatigue - FATIGUE_SEVERE) / span).clamp(0.0, 1.0);
    MICROSLEEP_BASE_GM + (MICROSLEEP_MIN_GM - MICROSLEEP_BASE_GM) * t
}

impl NodState {
    /// True while a nod is waiting on the driver's hands. Every other demand
    /// on the driver (a turn, a stop, a lane gap) waits on this.
    pub fn demands_driver(&self) -> bool {
        matches!(self.nod, Nod::Nodding { .. })
    }

    fn resolved(&mut self, misses: u8, outcome: NodOutcome) -> NodOutcome {
        self.nod = Nod::Recovering {
            left_gm: MICROSLEEP_COOLDOWN_GM,
        };
        self.misses = misses;
        outcome
    }

    /// The only place the nod-off moves.
    pub fn next(&mut self, event: NodEvent) -> NodOutcome {
        match event {
            NodEvent::WheelHandedOver => {
                self.nod = Nod::Awake { since_gm: 0.0 };
                NodOutcome::None
            }
            NodEvent::Staged => {
                if self.demands_driver() {
                    return NodOutcome::None;
                }
                self.nod = Nod::Nodding {
                    left_s: MICROSLEEP_REACTION_S,
                };
                NodOutcome::Began
            }
            NodEvent::Reacted => match self.nod {
                Nod::Nodding { .. } => self.resolved(0, NodOutcome::Caught),
                _ => NodOutcome::None,
            },
            NodEvent::Window { dt_s, crawling } => {
                let Nod::Nodding { left_s } = self.nod else {
                    return NodOutcome::None;
                };
                if crawling {
                    return self.resolved(0, NodOutcome::CaughtSilent);
                }
                let left_s = left_s - dt_s;
                if left_s > 0.0 {
                    self.nod = Nod::Nodding { left_s };
                    return NodOutcome::None;
                }
                let misses = self.misses.saturating_add(1);
                if misses >= MICROSLEEP_FORCE_STOP_MISSES {
                    self.resolved(0, NodOutcome::ForcedStop)
                } else {
                    self.resolved(misses, NodOutcome::DriftedOff)
                }
            }
            NodEvent::Tick {
                gm,
                fatigue,
                moving,
                busy,
            } => self.tick(gm, fatigue, moving, busy),
        }
    }

    fn tick(&mut self, gm: f64, fatigue: f64, moving: bool, busy: bool) -> NodOutcome {
        // The quiet period runs down on the game clock whatever the truck
        // is doing, and the minute it ends the count starts in that same
        // tick.
        if let Nod::Recovering { left_gm } = self.nod {
            let left_gm = (left_gm - gm).max(0.0);
            if left_gm > 0.0 {
                self.nod = Nod::Recovering { left_gm };
                return NodOutcome::None;
            }
            self.nod = Nod::Awake { since_gm: 0.0 };
        }
        let Nod::Awake { since_gm } = self.nod else {
            return NodOutcome::None; // a window is open; it has its own clock
        };
        if !moving || fatigue < FATIGUE_SEVERE {
            self.nod = Nod::Awake { since_gm: 0.0 };
            return NodOutcome::None;
        }
        if busy {
            return NodOutcome::None; // one demand on the driver at a time
        }
        let since_gm = since_gm + gm;
        if since_gm >= microsleep_interval_gm(fatigue) {
            self.nod = Nod::Nodding {
                left_s: MICROSLEEP_REACTION_S,
            };
            return NodOutcome::Began;
        }
        self.nod = Nod::Awake { since_gm };
        NodOutcome::None
    }

    /// The variant's key for saves and the agent server.
    pub fn key(&self) -> &'static str {
        match self.nod {
            Nod::Awake { .. } => "awake",
            Nod::Nodding { .. } => "nodding",
            Nod::Recovering { .. } => "recovering",
        }
    }

    /// The variant's timer and its unit, as the agent server reports it.
    pub fn timer(&self) -> (f64, &'static str) {
        match self.nod {
            Nod::Awake { since_gm } => (since_gm, "game minutes counted"),
            Nod::Nodding { left_s } => (left_s, "seconds left"),
            Nod::Recovering { left_gm } => (left_gm, "game minutes left"),
        }
    }

    /// The snapshot's shape: `{"state": "awake", "since_gm": 3.5, "misses": 1}`.
    pub fn to_json(&self) -> Value {
        let (key, field, value) = match self.nod {
            Nod::Awake { since_gm } => ("awake", "since_gm", since_gm),
            Nod::Nodding { left_s } => ("nodding", "left_s", left_s),
            Nod::Recovering { left_gm } => ("recovering", "left_gm", left_gm),
        };
        json!({"state": key, field: value, "misses": self.misses})
    }

    /// Read back what [`to_json`](Self::to_json) wrote; `None` for anything
    /// else, so the caller can fall back to a fresh, awake driver.
    pub fn from_json(value: &Value) -> Option<NodState> {
        let data = value.as_object()?;
        let number = |key: &str| {
            data.get(key)
                .and_then(Value::as_f64)
                .filter(|v| v.is_finite())
        };
        let nod = match data.get("state")?.as_str()? {
            "awake" => Nod::Awake {
                since_gm: number("since_gm")?.max(0.0),
            },
            "nodding" => Nod::Nodding {
                left_s: number("left_s")?.max(0.0),
            },
            "recovering" => Nod::Recovering {
                left_gm: number("left_gm")?.max(0.0),
            },
            _ => return None,
        };
        let misses = data
            .get("misses")
            .and_then(Value::as_u64)
            .unwrap_or(0)
            .min(u64::from(MICROSLEEP_FORCE_STOP_MISSES - 1)) as u8;
        Some(NodState { nod, misses })
    }
}

#[cfg(test)]
mod tests;
