//! Hours of service, fatigue, and the microsleeps severe fatigue brings on.

use crate::speech::EventPriority;
use ff_core::models::enforcement;
use ff_core::models::jobs::hos_stops::STOP_MARGIN_MIN as HOS_STOP_MARGIN_MIN;
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
        if moving && !co_driver {
            // Pressure-mode tuning scales how fast the day wears on you, and
            // an active food/drink buff slows accrual (data/buffs.py); neither
            // touches the HOS duty clock above.
            let fatigue_mult = tuning_for_time_scale(self.trip.time_scale).fatigue_rate;
            let p = profile_mut_of(ctx);
            let buff_rate = p.fatigue_buff_rate(now_h);
            p.fatigue = 100.0f64
                .min(p.fatigue + hos::fatigue_rate_per_min(night) * gm * fatigue_mult * buff_rate);
        }
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
            ctx.say_event_with(text, SayEvent::queued().category(SpeechCategory::Status));
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
        let alerts_clear = self.hazard_deadline.is_none() && !co_driver;
        if fatigue >= hos::FATIGUE_SEVERE && !self.severe_said && alerts_clear {
            self.severe_said = true;
            self.fatigue_cue_gm = 0.0;
            ctx.audio.play_with("vehicle/rumble_strip", 0.8, 0.0);
            ctx.say_event_with(
                "Dangerously drowsy and drifting out of your lane.",
                SayEvent::new().category(SpeechCategory::Safety),
            );
        } else if fatigue >= hos::FATIGUE_DROWSY && !self.drowsy_said && alerts_clear {
            self.drowsy_said = true;
            self.fatigue_cue_gm = 0.0;
            ctx.audio.play_with("driver/yawn", 0.9, 0.0);
            // An instruction to act, not roadside colour: ROUTE keeps it out
            // from behind chatter and brings it back if it gets talked over.
            ctx.say_event_with(
                "Getting drowsy.",
                SayEvent::queued()
                    .priority(EventPriority::Important)
                    .category(SpeechCategory::Safety),
            );
        }
        if fatigue < hos::FATIGUE_DROWSY {
            self.drowsy_said = false;
        }
        if fatigue < hos::FATIGUE_SEVERE {
            self.severe_said = false;
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

    // -- microsleeps (severe fatigue) ----------------------------------------------

    /// Game-minutes between nods; shrinks from base toward the floor as
    /// exhaustion deepens past the severe threshold.
    pub fn microsleep_interval_gm(&self, fatigue: f64) -> f64 {
        let span = 1.0f64.max(100.0 - hos::FATIGUE_SEVERE);
        let t = ((fatigue - hos::FATIGUE_SEVERE) / span).clamp(0.0, 1.0);
        MICROSLEEP_BASE_GM + (MICROSLEEP_MIN_GM - MICROSLEEP_BASE_GM) * t
    }

    /// Build toward the next involuntary nod-off while severely fatigued.
    pub fn accrue_microsleep(
        &mut self,
        ctx: &mut GameContext,
        gm: f64,
        moving: bool,
        fatigue: f64,
    ) {
        if self.microsleep_cooldown_gm > 0.0 {
            self.microsleep_cooldown_gm = (self.microsleep_cooldown_gm - gm).max(0.0);
        }
        if !moving || fatigue < hos::FATIGUE_SEVERE {
            self.microsleep_gm = 0.0;
            return;
        }
        // One demand on the driver at a time, and not right after the last nod.
        if self.microsleep_deadline.is_some()
            || self.hazard_deadline.is_some()
            || self.microsleep_cooldown_gm > 0.0
        {
            return;
        }
        self.microsleep_gm += gm;
        if self.microsleep_gm >= self.microsleep_interval_gm(fatigue) {
            self.microsleep_gm = 0.0;
            self.begin_microsleep(ctx);
        }
    }

    pub fn begin_microsleep(&mut self, ctx: &mut GameContext) {
        self.cancel_cruise(ctx, false); // the nod takes your hands off the wheel
        self.microsleep_deadline = Some(MICROSLEEP_REACTION_S);
        ctx.audio.play_with("vehicle/rumble_strip", 1.0, 0.0);
        ctx.controller.rumble.alert();
        ctx.say_event_with(
            "Nodding off. Steer or brake now!",
            SayEvent::new().category(SpeechCategory::Safety),
        );
    }

    pub fn update_microsleep(&mut self, ctx: &mut GameContext, dt: f64) {
        let Some(deadline) = self.microsleep_deadline else {
            return;
        };
        // Already crawling: the nod passes without leaving the road.
        if self.trip.truck.speed_mph() <= HAZARD_SAFE_MPH {
            self.resolve_microsleep(ctx, true);
            return;
        }
        // The line the truck just spoke is "Steer or brake now to stay awake",
        // and on a pad both of those are the stick and the left trigger --
        // neither of which is a key. A controller-only driver could not wake
        // up at all and drifted off the road every time (owner, 2026-08-16).
        // Parity with the keyboard is the bar: a held Down arrow already
        // counts as a reaction, so a held trigger does too.
        let pad_reacted = ctx.controller.active()
            && (ctx.controller.steering().abs() > 0.0 || ctx.controller.brake() > 0.05);
        let reacted = ctx.bindings.pressed(&ctx.input, Action::SteerLeft)
            || ctx.bindings.pressed(&ctx.input, Action::SteerRight)
            || ctx.bindings.pressed(&ctx.input, Action::Brake)
            || ctx.bindings.pressed(&ctx.input, Action::EmergencyBrake)
            || pad_reacted;
        if reacted {
            self.resolve_microsleep(ctx, false);
            return;
        }
        let left = deadline - dt;
        self.microsleep_deadline = Some(left);
        if left <= 0.0 {
            self.microsleep_deadline = None;
            self.microsleep_drift_off_road(ctx);
        }
    }

    pub fn resolve_microsleep(&mut self, ctx: &mut GameContext, silent: bool) {
        self.microsleep_deadline = None;
        self.microsleep_cooldown_gm = MICROSLEEP_COOLDOWN_GM;
        self.microsleep_misses = 0;
        if !silent {
            // ROUTE, not the ambient default: the outcome of a driver's own
            // reaction to an urgent warning, same class as the hazard-clear
            // precedent (automation-handoff sweep, 2026-08-20, the deferred
            // 2026-08-15 audit).
            ctx.say_event_with(
                "You caught it.",
                SayEvent::queued()
                    .priority(EventPriority::Important)
                    .category(SpeechCategory::Confirmation),
            );
        }
    }

    pub fn microsleep_drift_off_road(&mut self, ctx: &mut GameContext) {
        self.microsleep_misses += 1;
        self.microsleep_cooldown_gm = MICROSLEEP_COOLDOWN_GM;
        ctx.audio.play_with("vehicle/rumble_strip", 1.0, 0.0);
        self.trip
            .truck
            .add_damage(MICROSLEEP_SHOULDER_DAMAGE_PCT, true);
        self.trip.truck.velocity_mps *= 0.8; // wandering onto the shoulder scrubs speed
        let standing = self.record_fatigue_event(ctx);
        if self.microsleep_misses >= MICROSLEEP_FORCE_STOP_MISSES {
            self.microsleep_misses = 0;
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
