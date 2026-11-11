//! Live cattle on the trailer: the five state machines a cattle load runs,
//! and the one struct that owns them.
//!
//! A cattle load differs from other freight in two ways. The load moves by
//! itself -- cattle shift their weight and lose their footing when the truck
//! brakes or corners hard -- and time costs condition even while the truck is
//! parked: federal law limits the animals to 28 hours on the trailer
//! (49 USC 80502), and a trailer standing in the heat warms them past what
//! they tolerate (the Beef Quality Assurance Transportation manual).
//!
//! Each machine is an enum with one `next(event)` function and a transition
//! table in its tests:
//!
//! - [`LoadPhase`]: Booked, Loading, Aboard, Over the limit, Unloading,
//!   Delivered.
//! - [`Footing`]: Settled, Restless, Scrambling, One down.
//! - [`LimitMark`]: the 28-hour limit's warnings.
//! - [`Exemption`]: the agricultural hours-of-service exemption near the
//!   pickup and the delivery (49 CFR 395.1(k)).
//! - [`Heat`]: Cool, Hot and moving, Hot and standing.
//!
//! [`LivestockLoad`] owns one of each and is the only place they move: the
//! driving layer feeds it a [`LivestockTick`] each frame (and one for every
//! stretch of time spent parked) and hears back what changed. Other freight
//! never builds one. Nothing here knows about the window, the speakers or the
//! profile.

mod exemption;
mod footing;
mod heat;
mod limit;
mod phase;
mod plan;
mod words;

#[cfg(test)]
mod tests;

pub use exemption::{Exemption, ExemptionEvent, EXEMPT_RADIUS_MI};
pub use footing::{Footing, FootingEvent};
pub use heat::{danger_temp_f, thi, Heat, HeatReading, ASSUMED_RELATIVE_HUMIDITY, THI_DANGER};
pub use limit::{LimitEvent, LimitMark, LIMIT_HOURS, LIMIT_WARNING_MARKS_MIN};
pub use phase::{LoadEvent, LoadPhase};
pub use plan::{cattle_run, livestock_lane_ok, CattleRun, CATTLE_RUN_MARGIN_H};
pub use words::{
    head_text, pounds, receiver_ruling_line, shrink_pct, CattleKind, LIVESTOCK_CONDITION_TEXT,
};

use serde::{Deserialize, Serialize};

use crate::sim::trip_route_helpers::haversine_distance_mi;

/// The cargo catalog key for live cattle.
pub const LIVESTOCK_CARGO_KEY: &str = "livestock";

// -- footing --------------------------------------------------------------------
//
// Assumed, not read: no published figure for the force at which standing
// cattle lose their footing was found (the design's open question). The BQA
// Transportation manual says only to pull away slowly and drive smoothly.
// These sit well under the 0.45 g that moves tied-down freight, because
// cattle stand on their own legs with nothing holding them: an ordinary
// firm stop unsettles them, and a hard one throws them.
/// Braking or cornering past this unsettles the cattle. Assumed.
pub const FOOTING_FIRM_G: f64 = 0.15;
/// Past this the cattle lose their footing. Assumed.
pub const FOOTING_SCRAMBLE_G: f64 = 0.28;
/// Game minutes of calm before restless cattle settle again. Assumed.
pub const FOOTING_CALM_MIN: f64 = 5.0;
/// Condition lost per real second of scrambling, at the threshold; it grows
/// with how far past the threshold the force is.
pub const SCRAMBLE_PCT_PER_S: f64 = 1.2;
/// Per real second of scrambling: the base chance one animal goes down,
/// plus this much more per g past the threshold. The draw comes from the
/// load's own seeded stream, so the same trip seed gives the same result.
pub const DOWN_CHANCE_BASE_PER_S: f64 = 0.04;
pub const DOWN_CHANCE_PER_EXCESS_G_S: f64 = 0.6;
/// Condition lost per game minute while an animal is down.
pub const DOWN_PCT_PER_MIN: f64 = 0.05;

// -- heat -----------------------------------------------------------------------
/// Condition lost per game minute while the truck stands in the heat, at the
/// danger line; each index point past it adds this much again.
pub const HEAT_PCT_PER_MIN: f64 = 0.03;

// -- the 28-hour limit ------------------------------------------------------------
/// Condition lost per game hour once the limit has run out. The law's cost
/// falls on the cattle, not on a ticket: in thirteen years one possible
/// violation went to prosecutors and nothing followed (GAO-26-108123).
pub const OVER_LIMIT_PCT_PER_H: f64 = 1.5;

/// How the truck is moving, and where, for one step of the load.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LivestockTick {
    /// Real seconds this step covers (zero for time spent parked off the
    /// physics, such as a rest at a stop).
    pub real_s: f64,
    /// Game minutes this step covers.
    pub game_min: f64,
    /// Braking the cattle feel, in g (engine brake included: an animal does
    /// not care what slowed the truck).
    pub decel_g: f64,
    /// The bend's sideways pull, in g, after the bank.
    pub lateral_g: f64,
    pub speed_mph: f64,
    pub outside_temp_f: f64,
    /// Where the truck is, for the exemption circles.
    pub lat: f64,
    pub lon: f64,
}

impl LivestockTick {
    /// A stretch of time spent parked: no forces, no movement.
    pub fn parked(game_min: f64, outside_temp_f: f64, lat: f64, lon: f64) -> Self {
        LivestockTick {
            real_s: 0.0,
            game_min,
            decel_g: 0.0,
            lateral_g: 0.0,
            speed_mph: 0.0,
            outside_temp_f,
            lat,
            lon,
        }
    }
}

/// One thing that changed in a step, for the driving layer to voice.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LivestockChange {
    Phase(LoadPhase),
    Footing { from: Footing, to: Footing },
    Limit(LimitMark),
    Exemption { from: Exemption, to: Exemption },
    Heat { from: Heat, to: Heat },
}

/// What one step did.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LivestockStep {
    pub changes: Vec<LivestockChange>,
    /// Condition lost this step, in the cargo meter's percent.
    pub condition_pct: f64,
}

/// A cattle load and its five machines.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LivestockLoad {
    pub kind: CattleKind,
    pub head: u32,
    pub phase: LoadPhase,
    pub footing: Footing,
    pub limit: LimitMark,
    pub exemption: Exemption,
    pub heat: Heat,
    /// The limit for this load: 28 hours now, a value so a 36-hour load on
    /// the owner's written request needs no new machine.
    pub limit_h: f64,
    /// Game minutes since the last gate shut.
    pub aboard_min: f64,
    /// Where the cattle were loaded and where they go, for the circles.
    pub pickup: (f64, f64),
    pub delivery: (f64, f64),
    /// Game minutes the cattle have been calm while restless.
    calm_min: f64,
    /// Seeded stream for the one-down draw.
    rng: u64,
    /// The first hot stop of this load has been said.
    pub hot_stop_said: bool,
    /// Times the driver has checked the cattle on this load.
    pub checks: u32,
}

impl LivestockLoad {
    /// A load booked and not yet on the trailer.
    pub fn booked(
        kind: CattleKind,
        weight_tons: f64,
        pickup: (f64, f64),
        delivery: (f64, f64),
        seed: i64,
    ) -> Self {
        LivestockLoad {
            kind,
            head: kind.head_for_tons(weight_tons),
            phase: LoadPhase::Booked,
            footing: Footing::Settled,
            limit: LimitMark::NotRunning,
            exemption: Exemption::NotInForce,
            heat: Heat::Cool,
            limit_h: LIMIT_HOURS,
            aboard_min: 0.0,
            pickup,
            delivery,
            calm_min: 0.0,
            rng: (seed as u64) ^ 0x9E37_79B9_7F4A_7C15,
            hot_stop_said: false,
            checks: 0,
        }
    }

    /// Loading at the shipper: Booked becomes Loading, then the last gate
    /// shuts and the cattle are Aboard. The limit starts and the exemption
    /// comes into force near the pickup.
    pub fn load(&mut self) -> Vec<LivestockChange> {
        let mut changes = Vec::new();
        self.move_phase(LoadEvent::StartLoading, &mut changes);
        self.move_phase(LoadEvent::GateShut, &mut changes);
        self.aboard_min = 0.0;
        self.limit = self.limit.next(LimitEvent::Start);
        changes.push(LivestockChange::Limit(self.limit));
        let from = self.exemption;
        self.exemption = self.exemption.next(ExemptionEvent::Loaded);
        if from != self.exemption {
            changes.push(LivestockChange::Exemption {
                from,
                to: self.exemption,
            });
        }
        changes
    }

    /// The receiver starts unloading: the limit stops and the exemption
    /// ends.
    pub fn start_unloading(&mut self) -> Vec<LivestockChange> {
        let mut changes = Vec::new();
        self.move_phase(LoadEvent::StartUnloading, &mut changes);
        self.limit = self.limit.next(LimitEvent::Stop);
        let from = self.exemption;
        self.exemption = self.exemption.next(ExemptionEvent::Unloading);
        if from != self.exemption {
            changes.push(LivestockChange::Exemption {
                from,
                to: self.exemption,
            });
        }
        changes
    }

    /// The count is finished and the receiver has ruled.
    pub fn delivered(&mut self) -> Vec<LivestockChange> {
        let mut changes = Vec::new();
        self.move_phase(LoadEvent::Ruled, &mut changes);
        changes
    }

    /// The driver walks the trailer. An animal that is down is got back on
    /// its feet; the answer says what was found.
    pub fn check(&mut self) -> bool {
        self.checks += 1;
        let was_down = self.footing == Footing::OneDown;
        self.footing = self.footing.next(FootingEvent::Checked);
        if was_down {
            self.calm_min = 0.0;
        }
        was_down
    }

    pub fn aboard(&self) -> bool {
        matches!(self.phase, LoadPhase::Aboard | LoadPhase::OverLimit)
    }

    pub fn hours_aboard(&self) -> f64 {
        self.aboard_min / 60.0
    }

    /// Minutes left on the limit; negative once it has run out.
    pub fn limit_left_min(&self) -> f64 {
        self.limit_h * 60.0 - self.aboard_min
    }

    /// The driver's hours are not counting.
    pub fn hours_exempt(&self) -> bool {
        self.exemption.exempt()
    }

    /// One step of the load: the clock, the circles, the heat and the
    /// footing, in that order, and what it cost.
    pub fn tick(&mut self, input: LivestockTick) -> LivestockStep {
        let mut step = LivestockStep::default();
        if !self.aboard() {
            return step;
        }
        let game_min = input.game_min.max(0.0);
        self.aboard_min += game_min;

        // The 28-hour limit.
        let mark = self.limit.next(LimitEvent::Tick {
            left_min: self.limit_left_min(),
        });
        if mark != self.limit {
            self.limit = mark;
            step.changes.push(LivestockChange::Limit(mark));
            if mark == LimitMark::Over {
                self.move_phase(LoadEvent::LimitRanOut, &mut step.changes);
            }
        }
        if self.phase == LoadPhase::OverLimit {
            step.condition_pct += OVER_LIMIT_PCT_PER_H * game_min / 60.0;
        }

        // The exemption circles.
        let from = self.exemption;
        self.exemption = self.exemption.next(ExemptionEvent::Position {
            from_pickup_mi: haversine_distance_mi(
                self.pickup.0,
                self.pickup.1,
                input.lat,
                input.lon,
            ),
            to_delivery_mi: haversine_distance_mi(
                input.lat,
                input.lon,
                self.delivery.0,
                self.delivery.1,
            ),
        });
        if from != self.exemption {
            step.changes.push(LivestockChange::Exemption {
                from,
                to: self.exemption,
            });
        }

        // Heat.
        let from = self.heat;
        self.heat = self.heat.next(HeatReading {
            temp_f: input.outside_temp_f,
            speed_mph: input.speed_mph,
        });
        if from != self.heat {
            step.changes.push(LivestockChange::Heat {
                from,
                to: self.heat,
            });
        }
        if self.heat == Heat::HotStanding {
            let over = (heat::thi_at_assumed_humidity(input.outside_temp_f) - THI_DANGER).max(0.0);
            step.condition_pct += HEAT_PCT_PER_MIN * (1.0 + over) * game_min;
        }

        // Footing.
        self.step_footing(&input, &mut step);
        step
    }

    fn step_footing(&mut self, input: &LivestockTick, step: &mut LivestockStep) {
        let force = input.decel_g.max(input.lateral_g).max(0.0);
        let from = self.footing;
        let mut event = None;
        if force >= FOOTING_SCRAMBLE_G {
            event = Some(FootingEvent::Thrown);
        } else if self.footing == Footing::Scrambling {
            event = Some(FootingEvent::ForceEnded);
        } else if force >= FOOTING_FIRM_G {
            event = Some(FootingEvent::FirmMove);
        } else if self.heat == Heat::HotStanding {
            event = Some(FootingEvent::HotStanding);
        }
        if let Some(event) = event {
            self.footing = self.footing.next(event);
        }
        match self.footing {
            Footing::Restless => {
                if force < FOOTING_FIRM_G && self.heat != Heat::HotStanding {
                    self.calm_min += input.game_min.max(0.0);
                    if self.calm_min >= FOOTING_CALM_MIN {
                        self.footing = self.footing.next(FootingEvent::Calmed);
                    }
                } else {
                    self.calm_min = 0.0;
                }
            }
            Footing::Scrambling => {
                self.calm_min = 0.0;
                let excess = (force - FOOTING_SCRAMBLE_G).max(0.0);
                let seconds = input.real_s.max(0.0);
                step.condition_pct +=
                    SCRAMBLE_PCT_PER_S * (1.0 + excess / FOOTING_SCRAMBLE_G) * seconds;
                let chance =
                    (DOWN_CHANCE_BASE_PER_S + DOWN_CHANCE_PER_EXCESS_G_S * excess) * seconds;
                if chance > 0.0 && self.draw() < chance {
                    self.footing = self.footing.next(FootingEvent::Fell);
                }
            }
            Footing::OneDown => {
                step.condition_pct += DOWN_PCT_PER_MIN * input.game_min.max(0.0);
            }
            Footing::Settled => self.calm_min = 0.0,
        }
        if from != self.footing {
            step.changes.push(LivestockChange::Footing {
                from,
                to: self.footing,
            });
        }
    }

    fn move_phase(&mut self, event: LoadEvent, changes: &mut Vec<LivestockChange>) {
        let next = self.phase.next(event);
        if next != self.phase {
            self.phase = next;
            changes.push(LivestockChange::Phase(next));
        }
    }

    /// A uniform draw in [0, 1) from the load's own stream (splitmix64), so
    /// the one-down roll never moves any other seeded stream in the trip.
    fn draw(&mut self) -> f64 {
        self.rng = self.rng.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.rng;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^= z >> 31;
        (z >> 11) as f64 / (1u64 << 53) as f64
    }

    /// Set any state directly, for the agent server's scenario tool and for
    /// tests. Not used by play.
    #[allow(clippy::too_many_arguments)]
    pub fn stage(
        &mut self,
        phase: Option<LoadPhase>,
        footing: Option<Footing>,
        exemption: Option<Exemption>,
        heat: Option<Heat>,
        hours_aboard: Option<f64>,
    ) {
        if let Some(phase) = phase {
            self.phase = phase;
        }
        if let Some(footing) = footing {
            self.footing = footing;
            self.calm_min = 0.0;
        }
        if let Some(exemption) = exemption {
            self.exemption = exemption;
        }
        if let Some(heat) = heat {
            self.heat = heat;
        }
        if let Some(hours) = hours_aboard {
            self.aboard_min = (hours * 60.0).max(0.0);
            // The marks already passed are not said again.
            self.limit = LimitMark::Running.next(LimitEvent::Tick {
                left_min: self.limit_left_min(),
            });
            if self.limit == LimitMark::Over && self.phase == LoadPhase::Aboard {
                self.phase = LoadPhase::OverLimit;
            }
        }
    }
}
