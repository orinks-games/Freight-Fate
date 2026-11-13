//! Hours of service, fatigue, and the microsleeps severe fatigue brings on.

use crate::speech::EventPriority;
use ff_core::models::enforcement;
use ff_core::models::jobs::hos_stops::STOP_MARGIN_MIN as HOS_STOP_MARGIN_MIN;
use ff_core::sim::alertness;
use ff_core::sim::fatigue_costs::{
    fatigue_stop_outcome, mark_officer_warned, officer_warned, FatigueStopOutcome,
    OFFICER_FATIGUE_WARNING,
};
use ff_core::speech_pacing::SpeechCategory;

use crate::app::{GameContext, SayEvent};
use crate::bindings::Action;
use crate::states::driving::DrivingState;
use crate::states::driving_core::*;

impl DrivingState {
    /// Advance the HOS shift clock and fatigue on game time, not wall time.
    pub fn update_hours_and_fatigue(&mut self, ctx: &mut GameContext, dt: f64) {
        let gm = dt * self.trip.effective_time_scale() / 60.0; // game minutes this frame
        let moving = self.trip.truck.speed_mph() > 5.0;
        let mode = ctx.settings.hos_mode.clone();

        // Traveling to find work is commercial repositioning, including
        // self-serve bobtail runs. Stopped time remains on duty.
        if moving {
            hos_mut_of(ctx).drive(gm);
        } else {
            hos_mut_of(ctx).on_duty(gm); // the 14-hour window runs even while parked
        }
        if !hos::HOS_NON_ENFORCED_MODES.contains(&mode.as_str()) && self.hazard_deadline.is_none() {
            let marks = tuning_for_time_scale(self.trip.time_scale).hos_warning_marks_min;
            // A countdown to a limit the drive ends inside is no use to the
            // driver: worked out only when a mark comes due.
            let destination_min = if hos_of(ctx).mark_due(&mode, marks) {
                self.hos_stop_advice(ctx)
                    .map(|advice| advice.destination_travel_min)
            } else {
                None
            };
            let warnings = hos_mut_of(ctx).check_warnings_with(&mode, marks, |limit| {
                destination_min
                    .is_some_and(|minutes| minutes + HOS_STOP_MARGIN_MIN <= limit.remaining_min)
            });
            for message in warnings {
                // The clock running down is the drive, not colour: even the
                // non-urgent countdown must not queue behind chatter, and if
                // something cuts it off it comes back. The alarm and the
                // rumble are for a violation only; a notice is calm.
                let urgent = hos::warning_is_urgent(&message);
                if urgent {
                    ctx.audio.play("ui/warning");
                    ctx.controller.rumble.alert();
                }
                let opts = SayEvent::new()
                    .interrupt(urgent)
                    .priority(if urgent {
                        EventPriority::Critical
                    } else {
                        EventPriority::Important
                    })
                    .category(if urgent {
                        SpeechCategory::Safety
                    } else {
                        SpeechCategory::Status
                    });
                ctx.say_event_with(message, opts);
            }
        }
        self.trip.hos_violation = !hos::HOS_NON_ENFORCED_MODES.contains(&mode.as_str())
            && hos_of(ctx).in_violation(&mode);
        if moving && self.hazard_deadline.is_none() {
            self.warn_last_hos_stop(ctx);
            self.maybe_hos_planning_hint(ctx);
        }

        // A team's other logbook rests, and while the co-driver drives the
        // player is in the bunk: no fatigue gained, some slept off.
        self.update_team_hours(ctx, gm);
        let co_driver = self.co_driver_driving();

        let night = is_night(self.trip.local_hour());
        let now_h = self.absolute_game_hour(ctx, None);
        if !co_driver {
            // Pressure-mode tuning scales how fast the day wears on you, and
            // an active food/drink buff slows accrual (data/buffs.py); neither
            // touches the HOS duty clock above. Sitting at the wheel stopped
            // is on duty too, and builds a third of the rate.
            let fatigue_mult = tuning_for_time_scale(self.trip.time_scale).fatigue_rate;
            let duty_mult = if moving {
                1.0
            } else {
                hos::ON_DUTY_FATIGUE_FACTOR
            };
            let p = profile_mut_of(ctx);
            let buff_rate = p.fatigue_buff_rate(now_h);
            p.fatigue = 100.0f64.min(
                p.fatigue
                    + hos::fatigue_rate_per_min(night) * gm * fatigue_mult * buff_rate * duty_mult,
            );
        }
        let mut worn_lines: Vec<(String, bool)> = Vec::new();
        for worn in profile_mut_of(ctx).expire_buffs(now_h) {
            let text = worn
                .get("worn_off")
                .and_then(|value| value.as_str())
                .filter(|text| !text.is_empty())
                .map(str::to_string)
                .unwrap_or_else(|| {
                    let label = worn
                        .get("label")
                        .and_then(|value| value.as_str())
                        .unwrap_or("buff")
                        .to_lowercase();
                    format!("The {label} has worn off.")
                });
            worn_lines.push((text, false));
        }
        // A stimulant's lift comes back as it wears off, here and nowhere
        // else short of a sleep, so a stop, a loop-back or an on-duty block
        // that crossed the due time is paid back on the first frame at the
        // wheel. A drink's own worn-off line above marks it; the coffee
        // break's has its own. That one line explains a jump in fatigue, so
        // it is never dropped for waiting on a busy voice, and it is spoken
        // ahead of any band cue the jump brings (agent playtest, 2026-11-13:
        // a loop-back line held it, "Getting drowsy." went first, and the
        // worn-off line timed out unheard).
        if let Some(line) = profile_mut_of(ctx).repay_stimulant_loan(now_h) {
            match worn_lines.iter_mut().find(|(text, _)| *text == line) {
                Some(found) => found.1 = true,
                None if !line.is_empty() => worn_lines.push((line, true)),
                None => {}
            }
        }
        for (text, lift) in worn_lines {
            let opts = if lift {
                SayEvent::queued().priority(EventPriority::Important).keep()
            } else {
                SayEvent::queued()
            };
            ctx.say_event_with(text, opts.category(SpeechCategory::Status));
        }
        self.trip.truck.engine_wear_buff_mult = self
            .rig_buffs
            .get("engine")
            .map(|buff| buff.rate)
            .unwrap_or(1.0);
        self.trip.truck.tire_wear_buff_mult = self
            .rig_buffs
            .get("tire")
            .map(|buff| buff.rate)
            .unwrap_or(1.0);
        let fatigue = profile_of(ctx).fatigue;
        // A band cue waits while a hazard is on the driver or the co-driver
        // has the wheel, and the band does not move past a cue nobody heard:
        // it is spoken once the road is clear. Dropping a band is silent, so
        // that may happen at any time.
        let alerts_clear = self.hazard_deadline.is_none() && !co_driver;
        let (band, cue) = self.alertness.next(fatigue);
        if cue.is_none() || alerts_clear {
            self.alertness = band;
            if let Some(cue) = cue {
                self.play_band_cue(ctx, cue);
            }
        }
        // periodic audio cues while drowsiness persists
        if moving && !co_driver && fatigue >= hos::FATIGUE_DROWSY {
            self.fatigue_cue_gm += gm;
            if self.fatigue_cue_gm >= 15.0 {
                self.fatigue_cue_gm = 0.0;
                if fatigue >= hos::FATIGUE_SEVERE {
                    ctx.audio.play_with("vehicle/rumble_strip", 0.8, 0.0);
                } else {
                    ctx.audio.play_with("driver/yawn", 0.8, 0.0);
                }
            }
        }
        if !co_driver {
            self.accrue_microsleep(ctx, gm, moving, fatigue);
        }
    }

    /// The one sound, and for the higher bands the one line, for entering a
    /// fatigue band.
    fn play_band_cue(&mut self, ctx: &mut GameContext, cue: BandCue) {
        match cue {
            // The first sign of the day wearing on: a quiet yawn, no words.
            BandCue::Worn => ctx.audio.play_with("driver/yawn", 0.7, 0.0),
            BandCue::Drowsy => {
                self.fatigue_cue_gm = 0.0;
                ctx.audio.play_with("driver/yawn", 0.9, 0.0);
                // An instruction to act, not roadside colour: ROUTE keeps it
                // out from behind chatter and brings it back if it gets
                // talked over.
                ctx.say_event_with(
                    "Getting drowsy.",
                    SayEvent::queued()
                        .priority(EventPriority::Important)
                        .category(SpeechCategory::Safety),
                );
            }
            BandCue::Severe => {
                self.fatigue_cue_gm = 0.0;
                ctx.audio.play_with("vehicle/rumble_strip", 0.8, 0.0);
                ctx.say_event_with(
                    "Dangerously drowsy and drifting out of your lane.",
                    SayEvent::new().category(SpeechCategory::Safety),
                );
            }
        }
    }

    // -- microsleeps (severe fatigue) ----------------------------------------------

    /// Game-minutes between nods; shrinks from base toward the floor as
    /// exhaustion deepens past the severe threshold.
    pub fn microsleep_interval_gm(&self, fatigue: f64) -> f64 {
        alertness::microsleep_interval_gm(fatigue)
    }

    /// Build toward the next involuntary nod-off while severely fatigued.
    pub fn accrue_microsleep(
        &mut self,
        ctx: &mut GameContext,
        gm: f64,
        moving: bool,
        fatigue: f64,
    ) {
        // A staged nod (the agent server, or a trip saved mid-nod) opens on
        // the first rolling frame, with its warning.
        if self.nod_staged && moving && self.hazard_deadline.is_none() {
            self.nod_staged = false;
            let outcome = self.nod.next(NodEvent::Staged);
            self.apply_nod_outcome(ctx, outcome);
            return;
        }
        let outcome = self.nod.next(NodEvent::Tick {
            gm,
            fatigue,
            moving,
            busy: self.hazard_deadline.is_some(),
        });
        self.apply_nod_outcome(ctx, outcome);
    }

    /// Open a nod's reaction window now, warning and all.
    pub fn begin_microsleep(&mut self, ctx: &mut GameContext) {
        let outcome = self.nod.next(NodEvent::Staged);
        self.apply_nod_outcome(ctx, outcome);
    }

    pub fn update_microsleep(&mut self, ctx: &mut GameContext, dt: f64) {
        if !self.nod.demands_driver() {
            return;
        }
        // Already crawling: the nod passes without leaving the road.
        let crawling = self.trip.truck.speed_mph() <= HAZARD_SAFE_MPH;
        // The line the truck just spoke is "Steer or brake now to stay awake",
        // and on a pad both of those are the stick and the left trigger --
        // neither of which is a key. A controller-only driver could not wake
        // up at all and drifted off the road every time (owner, 2026-08-16).
        // Parity with the keyboard is the bar: a held Down arrow already
        // counts as a reaction, so a held trigger does too.
        let reacted = !crawling && {
            let pad_reacted = ctx.controller.active()
                && (ctx.controller.steering().abs() > 0.0 || ctx.controller.brake() > 0.05);
            ctx.bindings.pressed(&ctx.input, Action::SteerLeft)
                || ctx.bindings.pressed(&ctx.input, Action::SteerRight)
                || ctx.bindings.pressed(&ctx.input, Action::Brake)
                || ctx.bindings.pressed(&ctx.input, Action::EmergencyBrake)
                || pad_reacted
        };
        let outcome = if reacted {
            self.nod.next(NodEvent::Reacted)
        } else {
            self.nod.next(NodEvent::Window { dt_s: dt, crawling })
        };
        self.apply_nod_outcome(ctx, outcome);
    }

    /// Every side effect of the nod-off, one per outcome.
    fn apply_nod_outcome(&mut self, ctx: &mut GameContext, outcome: NodOutcome) {
        match outcome {
            NodOutcome::None | NodOutcome::CaughtSilent => {}
            NodOutcome::Began => {
                self.cancel_cruise(ctx, false); // the nod takes your hands off the wheel
                ctx.audio.play_with("vehicle/rumble_strip", 1.0, 0.0);
                ctx.controller.rumble.alert();
                ctx.say_event_with(
                    "Nodding off. Steer or brake now!",
                    SayEvent::new().category(SpeechCategory::Safety),
                );
            }
            NodOutcome::Caught => {
                // ROUTE, not the ambient default: the outcome of a driver's
                // own reaction to an urgent warning, same class as the
                // hazard-clear precedent (automation-handoff sweep,
                // 2026-08-20, the deferred 2026-08-15 audit).
                ctx.say_event_with(
                    "You caught it.",
                    SayEvent::queued()
                        .priority(EventPriority::Important)
                        .category(SpeechCategory::Confirmation),
                );
            }
            NodOutcome::DriftedOff => self.microsleep_drift_off_road(ctx, false),
            NodOutcome::ForcedStop => self.microsleep_drift_off_road(ctx, true),
        }
    }

    /// The window ran out: the truck wanders onto the shoulder. `forced` is
    /// the third miss in a row, which ends in the forced stop.
    pub fn microsleep_drift_off_road(&mut self, ctx: &mut GameContext, forced: bool) {
        ctx.audio.play_with("vehicle/rumble_strip", 1.0, 0.0);
        self.trip.truck.add_damage_from(
            MICROSLEEP_SHOULDER_DAMAGE_PCT,
            true,
            "drifting off the road asleep",
        );
        self.trip.truck.velocity_mps *= 0.8; // wandering onto the shoulder scrubs speed
        let standing = self.record_fatigue_event(ctx);
        if forced {
            self.trip.truck.throttle = 0.0;
            self.trip.truck.brake = 1.0;
            ctx.audio.play_with("vehicle/tire_screech", 0.9, 0.0);
            let out_of_service = self.fatigue_out_of_service(ctx);
            ctx.say_event_with(
                format!(
                    "You cannot stay awake. You drift onto the shoulder and jolt awake on the \
                     brakes. {standing} {out_of_service}"
                ),
                SayEvent::new().category(SpeechCategory::Safety),
            );
        } else {
            let damage = self.trip.truck.damage_pct;
            ctx.say_event_with(
                format!(
                    "You nodded off and drifted onto the rumble strip. Truck damage {damage:.0} \
                     percent. {standing}"
                ),
                SayEvent::new().category(SpeechCategory::Safety),
            );
        }
        // A second run-off on top of a serious violation just suspended the
        // CDL: the drive ends here instead of carrying on.
        self.end_drive_if_licence_pulled(ctx);
    }

    /// Book a run-off-road fatigue event and say what it cost.
    ///
    /// Falling asleep at the wheel is not a scrape: 49 CFR 392.3 forbids
    /// driving impaired by fatigue, and to a carrier this is a preventable
    /// safety incident. The first one costs standing; from the second on it
    /// is a violation the license answers for.
    pub fn record_fatigue_event(&mut self, ctx: &mut GameContext) -> String {
        if ctx.profile.is_none() || self.enforcement_bypassed(ctx) {
            return String::new();
        }
        self.fatigue_events += 1;
        let hit = enforcement::FATIGUE_EVENT_REPUTATION_HIT;
        {
            let p = profile_mut_of(ctx);
            p.career.reputation = 0.0f64.max(p.career.reputation - hit);
        }
        self.log_fatigue_event(ctx)
    }

    // -- what fatigue costs off the wheel -------------------------------------------

    /// Relaxed pacing, which halves slow work and lets the first officer
    /// warn instead of order (`docs/fatigue-design.md`, "Modes").
    pub fn relaxed_pacing(&self) -> bool {
        tuning_for_time_scale(self.trip.time_scale).name == "relaxed"
    }

    /// The band right now, for the costs that read it at a stop: the road's
    /// band moved on, silently, by whatever fatigue did since the last frame
    /// at the wheel (a break, a meal, the work itself).
    pub fn current_band(&self, ctx: &GameContext) -> Alertness {
        let fatigue = ctx.profile.as_ref().map_or(0.0, |p| p.fatigue);
        self.alertness.next(fatigue).0
    }

    /// The minutes work the driver does by hand takes now: `base_min` in the
    /// current band (`Alertness::hand_work_min`). Every row, help line and
    /// result line that reads such a task's minutes reads this figure.
    pub fn hand_work_min(&self, ctx: &GameContext, base_min: f64) -> f64 {
        self.current_band(ctx)
            .hand_work_min(base_min, self.relaxed_pacing())
    }

    /// Whether a stop that lets the driver off will still end in the fatigue
    /// order, so its own result is worded to lead into one.
    pub fn fatigue_order_due(&self, ctx: &GameContext) -> bool {
        if ctx.profile.is_none() || self.co_driver_driving() {
            return false;
        }
        fatigue_stop_outcome(
            self.current_band(ctx),
            self.relaxed_pacing(),
            officer_warned(&profile_of(ctx).achievement_stats),
            self.enforcement_bypassed(ctx),
            false,
        ) == FatigueStopOutcome::OutOfService
    }

    /// What an officer does about the driver's fatigue, once a stop's own
    /// result is settled: the line that follows the stop's result, empty for
    /// nothing. `out_of_service_before` is [`Self::out_of_service_count`] as
    /// the stop began, so a stop that already put the driver out of service
    /// adds no second order. The co-driver at the wheel is the one the
    /// officer talks to, so the player's fatigue is not in it.
    pub fn officer_sees_fatigue(
        &mut self,
        ctx: &mut GameContext,
        out_of_service_before: i64,
    ) -> String {
        if ctx.profile.is_none() || self.co_driver_driving() {
            return String::new();
        }
        let outcome = fatigue_stop_outcome(
            self.current_band(ctx),
            self.relaxed_pacing(),
            officer_warned(&profile_of(ctx).achievement_stats),
            self.enforcement_bypassed(ctx),
            self.out_of_service_count > out_of_service_before,
        );
        match outcome {
            FatigueStopOutcome::None => String::new(),
            FatigueStopOutcome::Warning => {
                mark_officer_warned(&mut profile_mut_of(ctx).achievement_stats);
                OFFICER_FATIGUE_WARNING.to_string()
            }
            FatigueStopOutcome::OutOfService => {
                // Ten hours parked is an overnight: the engine does not idle
                // through it, the same as the roadside hours order.
                shut_down_engine(self, ctx);
                let order = self.fatigue_out_of_service(ctx);
                format!("{order}{}", wake_air_instruction(self))
            }
        }
    }

    /// The third miss in a row is the fatigue out-of-service order.
    pub fn fatigue_out_of_service(&mut self, ctx: &mut GameContext) -> String {
        if self.enforcement_bypassed(ctx) {
            return "Stop and sleep before you wreck.".to_string();
        }
        self.trip.truck.velocity_mps = 0.0;
        self.trip.truck.set_parking_brake();
        self.place_out_of_service(ctx);
        format!(
            "Out of service for fatigue, {:.0} hours off duty. It is now {}. Driving and duty \
             clocks reset, and the delivery deadline kept counting.",
            enforcement::FATIGUE_OUT_OF_SERVICE_HOURS,
            clock_text(self.trip.local_hour())
        )
    }
}
