//! Rollover in the cab: the warning before a bend costs anything, and what
//! going over does to the run.
//!
//! The physics is `ff_core::sim::vehicle` (`roll.rs`): one sideways pull
//! against one threshold, for a mapped bend and an exit ramp's curve alike.
//! This is the game layer around it.
//!
//! **The warning comes before the cost.** A bend starts costing the truck at
//! the first of two speeds: where the load starts working against its straps
//! (the roll model's warning share of this load's threshold) and, with the
//! lane work the driver's, where the tires can no longer hold the lane and the
//! truck runs wide. The warning names that speed and fires while slowing to
//! it is still possible -- inside the bend at once, or on the approach once
//! the road left is no more than the truck needs to shed to it. It used to be
//! a flat 15 mph over the sign, and a hot ramp curve could cost the load and
//! put the truck on the shoulder with nothing said (agent drive, 2026-09-24:
//! 47 into a 35 ramp curve, partial lane keeping, cargo already moving).
//!
//! **Going over reuses the catastrophic path the game already has.** Damage
//! runs to the out-of-service wall and the freight to scrap, the truck is
//! stopped where it lies, and `recover_out_of_service` does what it does for
//! any truck that may not be driven: road service and the bill for an owner-
//! operator, a grounded tractor and a yard spare for a company driver. The
//! receiver refuses the load at the dock, as it refuses any load in that state.

use ff_core::data::curves::{
    bend_bank, min_radius_ft, superelevation_at, RouteCurve, SUPERELEVATION_BUILT,
};
use ff_core::models::cargo_condition::{cargo_condition_text, CARGO_REJECT_PCT};
use ff_core::models::enforcement::RECORD_CRASH;
use ff_core::sim::vehicle::{BrakeApplication, G, MPS_TO_MPH, ROLL_WARN_SHARE};
use ff_core::speech_pacing::SpeechCategory;

use crate::app::{GameContext, SayEvent};
use crate::states::driving::DrivingState;
use crate::states::driving_core::*;
use crate::states::driving_rest_states::record_hours;
use crate::states::driving_updates::live;

/// Seconds between a warning and the brakes going on. READ: the brake
/// reaction time the Green Book's stopping sight distance is built on (AASHTO
/// 2018, 3.2.2.2), which covers the 90th percentile of drivers.
pub const CURVE_WARN_REACTION_S: f64 = 2.5;
/// The deceleration a warned driver is assumed to use, m/s2, where the truck
/// can deliver it. READ: 11.2 ft/s2, the Green Book's stopping sight distance
/// rate, "comfortable for most drivers" (AASHTO 2018, 3.2.2.3).
pub const CURVE_WARN_DECEL_MPS2: f64 = 11.2 * 0.3048;

/// The ramp curve's identity for the once-per-curve warning. Mapped bends go
/// by their start milepost, which is never negative.
const RAMP_CURVE_ID: f64 = -1.0;

/// A curve the truck is in or heading into, as the warning sees it.
struct CurveInPlay {
    id: f64,
    /// Road to its start, miles; 0 inside it.
    ahead_mi: f64,
    radius_ft: f64,
    /// The bank the roll model credits.
    roll_bank: f64,
    /// The bank the lane model credits.
    lane_bank: f64,
    phrase: String,
    /// Where it ends, miles; None for the ramp curve, which lasts the ramp.
    end_mi: Option<f64>,
}

impl DrivingState {
    /// The bank the roll model credits a mapped bend: the same bank its
    /// advisory was posted with (`curves::bend_bank`, none on roads designed
    /// under 50 mph).
    pub fn bend_bank(&self, bend: &RouteCurve) -> f64 {
        bend_bank(
            (bend.min_radius_ft as f64).max(1.0),
            Some(self.trip.leg_design_speed_mph()),
        )
    }

    /// The bank the lane model credits a mapped bend (`update_lane`).
    fn lane_bank(&self, bend: &RouteCurve) -> f64 {
        superelevation_at(
            (bend.min_radius_ft as f64).max(1.0),
            self.trip.leg_design_speed_mph(),
        )
    }

    /// The advisory the cab speaks for a bend: the sign's, or the load's own
    /// number where the sign asks more than the load aboard takes without
    /// cost -- a sign the bake rounded up, a part-filled tank, a bend too
    /// tight for the lowest plaque. Never a number the truck cannot hold.
    pub fn spoken_advisory_mph(&self, curve: &RouteCurve) -> i64 {
        if curve.min_radius_ft <= 0 {
            return curve.advisory_mph;
        }
        self.load_holds_mph(
            curve.advisory_mph,
            curve.min_radius_ft as f64,
            self.bend_bank(curve),
        )
    }

    /// The exit speed the cab speaks, by the same rule as
    /// [`Self::spoken_advisory_mph`] for the ramp curve it governs.
    pub fn spoken_exit_mph(&self, exit_mph: f64) -> f64 {
        let posted = exit_mph.round() as i64;
        let held = self.load_holds_mph(
            posted,
            min_radius_ft(exit_mph).max(1.0),
            SUPERELEVATION_BUILT,
        );
        if held < posted {
            held as f64
        } else {
            exit_mph
        }
    }

    /// `posted`, or the fastest multiple of 5 below it this load takes a
    /// curve of `radius_ft` built with `bank` at without cost.
    ///
    /// Judged in the sign's own formula, the manual's `V^2 / 15R - e`, so a
    /// sign priced exactly at the warning share is kept rather than lost to
    /// the 15's rounding of g; and in 5 mph steps, because an advisory plaque
    /// "shall be a multiple of 5 mph" (MUTCD 11th ed. 2C.59, read). Floored at
    /// 5: a bend no plaque holds is still called with the slowest number.
    fn load_holds_mph(&self, posted: i64, radius_ft: f64, bank: f64) -> i64 {
        let takes = ROLL_WARN_SHARE * self.trip.truck.planning_roll_threshold_g();
        let asks = |mph: i64| (mph * mph) as f64 / (15.0 * radius_ft.max(1.0)) - bank.max(0.0);
        if asks(posted) <= takes + 1e-9 {
            return posted;
        }
        let mut mph = (posted - 1) / 5 * 5;
        while mph > 5 && asks(mph) > takes + 1e-9 {
            mph -= 5;
        }
        mph.max(5)
    }

    /// The ramp curve's radius and the bank the roll model credits it.
    ///
    /// The radius is the AASHTO minimum for the exit speed, which assumes the
    /// 8 percent bank the most permissive state builds; the roll model credits
    /// the 6 percent both cited manuals build to (`SUPERELEVATION_BUILT`), the
    /// cautious reading of the same geometry. The lane model credits the ramp
    /// no bank at all (`update_lane`), and the two are kept apart on purpose.
    pub fn ramp_curve_geometry(&self) -> Option<(f64, f64)> {
        let layout = self.ramp_layout?;
        Some((
            min_radius_ft(layout.curve_mph).max(1.0),
            SUPERELEVATION_BUILT,
        ))
    }

    /// How the lane work is shared, as `TruckState::curve_safe_mph` takes it:
    /// None with it automated, else whether something supplies the wheel a
    /// bend asks for.
    pub fn lane_steers(ctx: &GameContext) -> Option<bool> {
        ctx.settings
            .lane_is_manual()
            .then(|| ctx.settings.road_steers_the_bend())
    }

    /// The fastest the truck takes a curve before it costs anything, in mph
    /// (`TruckState::curve_safe_mph`, with this driver's lane keeping).
    pub fn curve_safe_mph(
        &self,
        ctx: &GameContext,
        radius_ft: f64,
        roll_bank: f64,
        lane_bank: f64,
    ) -> f64 {
        self.trip
            .truck
            .curve_safe_mph(radius_ft, roll_bank, lane_bank, Self::lane_steers(ctx))
    }

    /// [`Self::curve_safe_mph`] for a mapped bend.
    pub fn bend_safe_mph(&self, ctx: &GameContext, bend: &RouteCurve) -> f64 {
        self.curve_safe_mph(
            ctx,
            bend.min_radius_ft as f64,
            self.bend_bank(bend),
            self.lane_bank(bend),
        )
    }

    /// [`Self::curve_safe_mph`] for the ramp curve, or None off a laid-out
    /// ramp.
    pub fn ramp_curve_safe_mph(&self, ctx: &GameContext) -> Option<f64> {
        let (radius, bank) = self.ramp_curve_geometry()?;
        Some(self.curve_safe_mph(ctx, radius, bank, 0.0))
    }

    /// Road the truck needs to come down to `target_mph` once warned, miles:
    /// the reaction, then the Green Book rate or what this truck's brakes can
    /// do on this grade with this load, whichever is less.
    fn curve_warn_reach_mi(&self, target_mph: f64) -> f64 {
        let truck = &self.trip.truck;
        let v = truck.velocity_mps.max(0.0);
        let target = (target_mph / MPS_TO_MPH).clamp(0.0, v);
        let can = truck.braking_decel_mps2(BrakeApplication::Service(1.0)) + G * truck.grade
            - truck.surge_decel_penalty_mps2();
        let decel = CURVE_WARN_DECEL_MPS2.min(can).max(0.1);
        let metres = v * CURVE_WARN_REACTION_S + (v * v - target * target) / (2.0 * decel);
        // The road passes `scale` times faster than the truck slows.
        metres * self.trip.effective_time_scale().max(1.0) / METERS_PER_MILE
    }

    /// The curves the warning is about: the one under the truck, and the
    /// next inside the truck's own braking reach.
    ///
    /// Both, not the first of them: in a run of bends the next one comes
    /// into reach while the truck is still in the last, and looking only
    /// under the truck left it unwarned until the truck was in it and the
    /// load already moving (bend sweep, US-60 Salt River Canyon, 2026-09-24).
    ///
    /// On the approach, only a curve nothing else is already slowing for: a
    /// mapped bend once its call has gone out (the call names it first, and
    /// from the call on the clock runs real) and not while curve assistance
    /// has it; the ramp curve not while an assist is braking the lane down to
    /// the exit speed, unless the driver's foot is overriding it. Inside the
    /// curve, whatever is driving.
    fn curves_in_play(&self, ctx: &GameContext) -> Vec<CurveInPlay> {
        if self.ramp_mi.is_some() {
            let Some((radius_ft, roll_bank)) = self.ramp_curve_geometry() else {
                return Vec::new();
            };
            let ahead_mi = if self.ramp_curve_radius_ft().is_some() {
                Some(0.0)
            } else {
                let assisted = Self::ramp_speed_assisted(ctx) && !Self::driver_accelerating(ctx);
                self.deceleration_lane_left_mi().filter(|_| !assisted)
            };
            return ahead_mi
                .map(|ahead_mi| CurveInPlay {
                    id: RAMP_CURVE_ID,
                    ahead_mi,
                    radius_ft,
                    roll_bank,
                    lane_bank: 0.0,
                    phrase: "Ramp curve".to_string(),
                    end_mi: None,
                })
                .into_iter()
                .collect();
        }
        let position = self.trip.position_mi;
        let underfoot = self
            .trip
            .curve_at(position)
            .filter(|c| !c.connector)
            .map(|bend| (0.0, bend));
        // Look as far as the truck needs to shed to a crawl; anything further
        // has time left to be warned about later.
        let ahead = if ctx.settings.curve_speed_assist {
            None
        } else {
            self.trip
                .next_curve_within(self.curve_warn_reach_mi(0.0))
                .filter(|(_, bend)| !bend.connector && self.trip.curve_called(bend))
        };
        underfoot
            .into_iter()
            .chain(ahead)
            .map(|(ahead_mi, bend)| CurveInPlay {
                id: bend.start_mi,
                ahead_mi,
                radius_ft: bend.min_radius_ft as f64,
                roll_bank: self.bend_bank(&bend),
                lane_bank: self.lane_bank(&bend),
                phrase: self.pacenote_phrase(&bend),
                end_mi: Some(bend.start_mi.max(bend.end_mi)),
            })
            .collect()
    }

    /// Say once per curve that it is being taken too fast for this load,
    /// while there is still road to fix it.
    pub fn update_curve_warning(&mut self, ctx: &mut GameContext) {
        let curves = self.curves_in_play(ctx);
        if curves.is_empty() {
            self.curve_warned_mi = None;
            return;
        }
        let speed = self.trip.truck.speed_mph();
        // Inside a bend the truck is gaining speed in -- a downgrade under it
        // -- the warning is priced at where it will be once the driver has
        // reacted, or it comes the frame the load starts moving rather than
        // before (bend sweep, 2026-09-24: CA-299's 6.6 percent into a 50
        // bend). Not while curve assistance has the bend: it brakes before
        // the truck gets there.
        let gaining = if ctx.settings.curve_speed_assist {
            0.0
        } else {
            self.trip.truck.net_accel_mph_per_s().max(0.0) * CURVE_WARN_REACTION_S
        };
        for curve in curves {
            // Once per curve, in road order: a bend at or behind the last one
            // warned has had its word.
            let had_its_word = self.curve_warned_mi.is_some_and(|warned| {
                warned == curve.id
                    || (curve.id != RAMP_CURVE_ID && warned != RAMP_CURVE_ID && curve.id < warned)
            });
            if had_its_word {
                continue;
            }
            let safe = self.curve_safe_mph(ctx, curve.radius_ft, curve.roll_bank, curve.lane_bank);
            let soon = if curve.ahead_mi > 0.0 {
                speed
            } else {
                speed + gaining
            };
            if soon <= safe {
                continue;
            }
            if curve.ahead_mi > 0.0 && curve.ahead_mi > self.curve_warn_reach_mi(safe) {
                continue;
            }
            self.curve_warned_mi = Some(curve.id);
            let slow_to = safe.floor().max(1.0);
            // True while the truck is still in or short of the bend and still
            // over the number. Cut by the next bend's warning in a run of
            // esses, it used to come back after it: "Sharp right, too fast.
            // Slow to 27" and then "Sharp left, too fast. Slow to 31" for a
            // bend already behind, so the last number heard was the wrong
            // one (bend sweep, US-62, 2026-09-24).
            let end_mi = curve.end_mi;
            let still_true = move || {
                live::speed_mph() > slow_to
                    && end_mi.map_or_else(live::on_ramp, |end| live::position_mi() <= end)
            };
            ctx.say_event_with(
                format!(
                    "{}, too fast. Slow to {}.",
                    curve.phrase,
                    ctx.settings.speed_text(slow_to)
                ),
                SayEvent::new()
                    .category(SpeechCategory::Safety)
                    .valid(still_true),
            );
            return;
        }
    }

    /// Put the truck over if the bend is asking more than its threshold.
    /// Runs after the bend's geometry is on the truck for this frame.
    pub fn update_rollover(&mut self, ctx: &mut GameContext) {
        if self.recovering || self.trip.truck.roll_share() < 1.0 {
            return;
        }
        self.roll_over(ctx);
    }

    /// The truck goes over: any freight aboard is scrap, the truck may not be
    /// driven, and the run carries on the way it does after any other
    /// out-of-service event.
    pub fn roll_over(&mut self, ctx: &mut GameContext) {
        let on_ramp = self.ramp_mi.is_some();
        ctx.audio.play("vehicle/collision");
        ctx.controller.rumble.impact(1.0);
        self.curve_servo = None;
        self.disarm_speed_control(ctx);
        {
            let truck = &mut self.trip.truck;
            truck.velocity_mps = 0.0;
            truck.throttle = 0.0;
            let to_the_wall = (DAMAGE_OUT_OF_SERVICE_PCT - truck.damage_pct).max(0.0);
            truck.add_damage(to_the_wall, true);
            truck.add_cargo_damage(100.0);
        }
        // This line names what happened to the load, so the condition cue
        // must not say it again a frame later.
        self.cargo_cue_at = self.cargo_cue_at.max(CARGO_REJECT_PCT);
        // A deadhead or a bobtail has no load and no receiver to name.
        let loaded = self.trip.truck.trailer_attached && self.trip.truck.cargo_kg > 0.0;
        let words = cargo_condition_text(
            self.trip.truck.cargo_damage_pct,
            self.trip.truck.cargo_vocabulary(),
        );
        let place = if on_ramp { "ramp curve" } else { "bend" };
        let recorded = self.record_crash(ctx, place);
        let message = if self.terse_speech(ctx) {
            let record = if recorded {
                " A crash on your record."
            } else {
                ""
            };
            if loaded {
                format!("Rolled over in the {place}. Load {words}.{record}")
            } else {
                format!("Rolled over in the {place}.{record}")
            }
        } else {
            let record = if recorded {
                " It goes on your driving record as a crash."
            } else {
                ""
            };
            if loaded {
                format!(
                    "The truck rolled over in the {place}. The load is {words}, and the \
                     receiver will refuse it.{record}"
                )
            } else {
                format!("The truck rolled over in the {place}.{record}")
            }
        };
        ctx.say_event_with(message, SayEvent::new().category(SpeechCategory::Safety));
        // The wall is reached here, not by the band watcher: settlement
        // grades the run by the deepest band, and the recovery line below is
        // the announcement for where the truck lands.
        self.worst_damage_band = self.worst_damage_band.max(DAMAGE_BAND_OUT_OF_SERVICE);
        self.damage_band = DAMAGE_BAND_OUT_OF_SERVICE;
        self.recover_out_of_service(ctx);
    }

    /// Book the rollover on the driving record as a crash (owner ruling,
    /// 2026-09-24). A motor carrier lists every accident on its register
    /// (49 CFR 390.15), and 390.5 counts one where a vehicle is towed away,
    /// which a truck on its side always is. It weighs on the safety record and
    /// on reputation as a serious event does (`record_reputation_penalty`,
    /// `score_for_profile`). Returns whether it was booked: not without a
    /// career, and not with hours of service off, where the record is not
    /// kept for the fatigue event either.
    fn record_crash(&mut self, ctx: &mut GameContext, place: &str) -> bool {
        if ctx.profile.is_none() || self.enforcement_bypassed(ctx) {
            return false;
        }
        let hours = record_hours(ctx, self);
        let where_ = self.record_place(ctx);
        let record = &mut profile_mut_of(ctx).driving_record;
        record.record_crash(hours);
        record.note(
            RECORD_CRASH,
            &format!("Rolled the truck over in a {place}"),
            0.0,
            hours,
            &where_,
        );
        true
    }
}
