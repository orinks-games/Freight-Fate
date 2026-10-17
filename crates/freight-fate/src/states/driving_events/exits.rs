//! Highway exits: arming the signal, the exit lane, the countdown, the exit
//! speed assist, and the destination exit's own scan and announcement.

use crate::speech::EventPriority;
use ff_core::sim::trip_models::{
    RoadStop, TrafficPressure, TripEvent, TripEventKind, APPROACH_DECEL_MPS2, APPROACH_REACTION_S,
};
use ff_core::speech_pacing::SpeechCategory;

use crate::app::{GameContext, SayEvent};
use crate::bindings::Action;
use crate::states::driving::DrivingState;
use crate::states::driving_core::*;
use crate::states::driving_updates::live;

impl DrivingState {
    /// A posted-limit change on the mainline the truck is leaving: on the
    /// ramp, or at the gore of an exit it is taking this frame.
    ///
    /// The limit line fires as the odometer crosses the change, which at an
    /// exit can be the gore itself -- "Speed limit raised to 75." was spoken
    /// on top of "Exit speed 52. You take exit 167" (agent drive, Edwards,
    /// 2026-09-24). The ramp's own speed is the number that matters there.
    pub(crate) fn limit_change_of_road_left(
        &mut self,
        ctx: &mut GameContext,
        event: &TripEvent,
    ) -> bool {
        if event.kind != TripEventKind::GpsCue || !event.data.limit_change.unwrap_or(false) {
            return false;
        }
        if self.ramp_mi.is_some() {
            return true;
        }
        let Some(stop) = self.exit_stop.clone() else {
            return false;
        };
        !self.exit_signal_canceled
            && self.trip.position_mi >= stop.at_mi
            && self.trip.position_mi <= stop.at_mi + EXIT_COMMIT_WINDOW_MI
            && self.exit_intent_ready(ctx, &stop)
            && self.exit_lane_ready()
            && self.trip.truck.speed_mph() <= self.gore_acceptance_mph(Some(&stop))
    }

    /// The last mile to an exit the truck is set to take, or its ramp.
    pub(crate) fn in_exit_approach(&self) -> bool {
        if self.ramp_mi.is_some() {
            return true;
        }
        self.exit_stop.as_ref().is_some_and(|stop| {
            !self.exit_signal_canceled
                && stop.at_mi - self.trip.position_mi <= EXIT_APPROACH_QUIET_MI
        })
    }

    /// `_take_exit()`: the take-exit control.
    pub fn take_exit(&mut self, ctx: &mut GameContext) {
        self.toggle_exit_signal(ctx);
    }

    /// `_toggle_exit_signal()`: arm, confirm, or cancel the exit signal.
    pub fn toggle_exit_signal(&mut self, ctx: &mut GameContext) {
        if self.ramp_mi.is_some() {
            self.say_plain(ctx, "Already on the exit ramp.");
            return;
        }
        let selected = self.selected_rest_stop();
        let window = self.exit_window_mi();
        let selected_ahead = selected.as_ref().is_some_and(|stop| {
            let ahead = stop.at_mi - self.trip.position_mi;
            ahead > 0.0 && ahead <= window
        });
        // Explicit T selection outranks inferred destination bookkeeping.
        let mut stop = if selected_ahead {
            selected
        } else {
            match self.exit_stop.clone() {
                Some(stop) => Some(stop),
                None => self.upcoming_exit_stop(ctx),
            }
        };
        // ...and a nearer open scale outranks both: the inspection lane is
        // not optional, and arming the farther ramp is exactly what carried
        // a tester past the scale unarmed. The plan itself survives.
        let scale_claimed = self.scale_claiming_exit(ctx, stop.as_ref());
        if let Some(scale) = scale_claimed.clone() {
            stop = Some(scale);
        }
        let Some(stop) = stop else {
            self.say_plain(ctx, "No route exit to signal for yet.");
            return;
        };
        let responding_to_destination_callout = stop.stop_type == "delivery_destination"
            && self.destination_exit_response_s > 0.0
            && Self::destination_exit_key(&stop) == self.destination_exit_announced_key;
        if responding_to_destination_callout {
            // The shared event voice may now be reading a newer safety warning,
            // so do not stop it just to replace the earlier exit callout.
            self.destination_exit_response_s = 0.0;
        }
        self.exit_stop = Some(stop.clone());
        let ahead = stop.at_mi - self.trip.position_mi;
        if self.exit_signal_on {
            // This close to the gore, one stray press must not silently throw
            // the approach away (playtested: an X meant as "confirm" canceled
            // the signal and cost the exit). The first press keeps the signal
            // and says so; only a deliberate second press cancels.
            if ahead <= EXIT_CANCEL_GUARD_MI && !self.exit_cancel_armed {
                self.exit_cancel_armed = true;
                let stays = if self.exit_blinker_on() { "on" } else { "set" };
                self.say_plain(
                    ctx,
                    format!(
                        "Signal stays {stays}. Press {} again to cancel the exit.",
                        ctx.control_hint("take_exit")
                    ),
                );
                return;
            }
            let was_blinking = self.exit_blinker_on();
            self.exit_signal_on = false;
            ctx.audio.release_cue("vehicle/turn_signal");
            ctx.audio.release_cue(STEER_CUE_HOLD);
            if was_blinking {
                // The stalk clicking back, as when the steering cue ends.
                let volume = 1.0f64.min(STEER_CUE_CANCEL_VOL * self.cue_loudness(ctx));
                ctx.audio.play_with("vehicle/turn_signal_off", volume, 0.0);
            }
            self.steer_cue_active = false;
            self.steer_cue_hold_s = 0.0;
            self.exit_cancel_armed = false;
            self.exit_signal_canceled = true;
            self.canceled_exit_key = Some(Self::destination_exit_key(&stop));
            self.exit_stop = None;
            self.trip.exit_approach_mi = None;
            self.reset_exit_lane_state();
            // Letting the cap linger would leave automatic control crawling
            // at ramp speed down the open highway after the driver begged off.
            self.cruise_exit_mph = None;
            self.destination_exit_response_s = 0.0;
            let canceled_selected = self.is_selected_stop(Some(&stop));
            if canceled_selected {
                self.clear_selected_stop_intent();
            }
            self.set_status("Signal canceled.");
            self.say_plain(ctx, "Signal canceled.");
            return;
        }
        self.exit_signal_on = true;
        self.canceled_exit_key = None;
        self.exit_cancel_armed = false;
        self.exit_signal_canceled = false;
        // The player just signalled for an exit; count it toward retiring the
        // "press X to signal" instruction, and update what the stop callout
        // will say from here (research doc R7).
        self.note_instruction_demonstrated(ctx, "take_exit");
        self.refresh_exit_hint(ctx);
        // Re-arming after a cancel starts the distance anchors over; without
        // this the milestones already spoken stay marked and the second
        // approach runs silent.
        self.exit_countdown_said.clear();
        self.steer_cue_timer = 0.0;
        self.update_steering_lane_cue(ctx, 0.0);
        // X commits the truck to the exit wherever it is pressed, but the
        // blinker itself runs only from half a mile out (owner ruling,
        // 2026-09-24): "set" until then, "on" once it clicks.
        let signal = if self.exit_blinker_on() {
            "Signal on"
        } else {
            "Signal set"
        };
        let head = if scale_claimed.is_some() {
            format!("{signal} for the scale exit: {},", stop.name)
        } else if stop.stop_type == "delivery_destination" {
            let labeled = self.exit_phrase_of(ctx, &stop);
            let labeled = if labeled.is_empty() {
                stop.exit_label.clone()
            } else {
                labeled
            };
            // A labeled exit already names itself; don't repeat the
            // facility that the fallback phrase would have baked in.
            if labeled.is_empty() {
                format!("{signal} for the destination exit for {},", stop.name)
            } else {
                format!(
                    "{signal} for {labeled}, destination exit for {},",
                    stop.name
                )
            }
        } else {
            // Once the stop-ahead callout has named this facility in full this
            // leg, the exit signal speaks its proper name alone (research doc
            // R6).
            let facility = self.trip.name_facility(&stop.name, &stop.spoken_name());
            if stop.exit_label.is_empty() {
                format!("{signal} for the {facility} exit,")
            } else {
                format!("{signal} for {}, {facility},", stop.exit_label)
            }
        };
        let in_right_lane = self.in_right_lane_for_exit();
        // Name the ramp's ending now, while there is still a mile of
        // mainline to plan the braking on: a stop sign heard only on the
        // ramp cost real playtesters real cross-traffic damage.
        let ending = match self.ramp_control_for(ctx, &stop, None).as_str() {
            "signal" => " Ramp ends at a traffic light.",
            "stop" => " Ramp ends at a stop sign.",
            _ => "",
        };
        let ahead_text = ctx.settings.distance_text(ahead, true);
        // No ramp number here: it is braked for past the gore, where taking
        // the exit names it as the exit speed. Said a mile out it asked the
        // driver to shed on the mainline (realistic exit, 2026-09-24).
        self.cap_cruise_for_ramp(Some(&stop));
        let mut message = if ctx.settings.lane_is_automated() {
            self.exit_lane_entered = true;
            ctx.audio.play_with("ui/notify", 0.6, 0.0);
            // No lane hint: lane keeping moves to the right lane itself
            // (`keep_right_for_exit`), and what it does is the manual's to
            // explain, not the callout's (owner, 2026-10-14).
            format!("{head} {ahead_text} ahead.{ending}")
        } else {
            // The right lane now; the exit lane itself where it opens, which
            // the cab calls at the taper. Nothing at all when the truck is
            // already where it needs to be (owner's drive, I-70 into Denver
            // West, 2026-09-24: "I'm already in the right lane").
            let lane_hint = if in_right_lane {
                ""
            } else {
                " Move to the right lane."
            };
            format!("{head} {ahead_text} ahead.{lane_hint}{ending}")
        };
        if self.is_selected_stop(Some(&stop)) {
            self.selected_stop_assist_armed = ctx.settings.destination_approach_assist;
            if !self.selected_stop_assist_armed {
                message.push_str(" Stop at the entrance.");
            }
        }
        self.set_status(message.clone());
        if responding_to_destination_callout {
            // Queue behind whichever event is currently speaking. Usually that
            // is the exit callout; if a critical warning preempted it, the
            // warning must finish before the confirmation.
            //
            // And it may only come back while the exit is still THERE. A line
            // cut by an urgent warning is handed back so it finishes rather
            // than vanishing, which is right -- but "move right for the exit
            // lane" handed back after the gore is behind the truck instructs a
            // maneuver that no longer exists. That is the same fault the scale
            // exit line was given a validity check for on 21 August; this one
            // was missed because the report only ever named the scale.
            //
            // Rust: the predicate reads the live trip, which a 'static closure
            // cannot borrow, so it goes through `live` exactly as the scale
            // reminder does. The port had written the deviation note and then
            // left the gate off entirely, so the confirmation could be handed
            // back after the gore with nothing to refuse it.
            let exit_mi = stop.at_mi;
            self.refresh_live_facts();
            let mut opts = SayEvent::queued()
                .priority(EventPriority::Important)
                .valid(move || live::position_mi() < exit_mi);
            opts.category = Some(SpeechCategory::Navigation);
            ctx.say_event_with(message, opts);
        } else {
            self.say_plain(ctx, message);
        }
    }

    /// Ramp speed for the exit this truck is actually taking.
    ///
    /// Every exit used to demand the same 45, and every cruise the same 40 --
    /// one number for a loop off a 55 and a directional connector off a 75
    /// alike. The owner drove it and said so (2026-08-21). This asks the road
    /// instead: `Trip.ramp_speed_at` reads the corridor limit and the baked
    /// `ramp_far_end`, and AASHTO's share does the rest.
    ///
    /// Falls back to the old constant when nothing is armed, so callers that
    /// ask out of context behave exactly as before.
    pub fn armed_ramp_mph(&self, stop: Option<&RoadStop>) -> f64 {
        // The caller may hand the stop in: the exit callout builds its
        // sentence BEFORE _exit_stop is assigned, and without it this fell
        // back to the old flat number and quietly undid the whole change.
        // Asked at the interchange that serves the stop when the bake matched
        // one, so the posted advisory and the far end are that ramp's and not
        // whichever exit sits nearest the stop's projected mile.
        let at_mi = stop
            .or(self.ramp_stop.as_ref())
            .or(self.exit_stop.as_ref())
            .map(|stop| stop.interchange_mi.unwrap_or(stop.at_mi));
        match at_mi {
            None => RAMP_MAX_MPH,
            Some(at_mi) => self.trip.ramp_speed_at(at_mi),
        }
    }

    /// How fast the truck may still be doing when it enters the exit.
    ///
    /// NOT the ramp's design speed, which is what you slow to ALONG it. A
    /// deceleration lane is a full lane beside the through lanes and exists
    /// so a driver leaves at road speed and sheds inside it -- demanding ramp
    /// speed at the gore makes the driver do the lane's job on the highway,
    /// which is the whole complaint (owner, 2026-08-21).
    ///
    /// So the gore accepts road speed, and the ramp's own number governs from
    /// there. Collapsing the two is what briefly made a ramp off a 55 stricter
    /// than the flat 45 it replaced -- tightening exactly where the change was
    /// supposed to loosen.
    pub fn gore_acceptance_mph(&mut self, stop: Option<&RoadStop>) -> f64 {
        let at_mi = stop
            .or(self.ramp_stop.as_ref())
            .or(self.exit_stop.as_ref())
            .map(|stop| stop.at_mi);
        let Some(at_mi) = at_mi else {
            return RAMP_MAX_MPH;
        };
        let (corridor, _) = self.trip.speed_limit_at(at_mi);
        // "Road speed" is what the posts let a truck do, not the number on the
        // sign to the decimal: the same leeway the enforcement layer gives
        // before a speed is a speed at all. Judged at the sign's exact number
        // the gate refused its own assist -- the exit speed assist stands down
        // once the truck is at or under acceptance, a downgrade then put it a
        // fraction over with cruise already paused, and the truck that did
        // everything right missed its exit. The old flat pair had this
        // headroom built in (45 accepted, 40 aimed for); this keeps it.
        //
        // Never below the old flat acceptance: a slow corridor must not make
        // taking an exit harder than it has ever been.
        RAMP_MAX_MPH.max(corridor + SPEEDING_LEEWAY_MPH)
    }

    /// What automatic control aims for on that ramp.
    ///
    /// A little under the ramp's own number, for the same reason the flat 40
    /// sat under the flat 45: the cruise loop has a two-mph brake deadband
    /// and downhill runs at it, and it must not leave the truck hovering
    /// just over the ramp's speed at the gore.
    pub fn armed_ramp_cruise_mph(&self, stop: Option<&RoadStop>) -> f64 {
        RAMP_MIN_DESIGN_MPH.max(self.armed_ramp_mph(stop) - RAMP_CRUISE_HEADROOM_MPH)
    }

    /// The lowest automatic control goes on the MAINLINE for an armed exit.
    ///
    /// Road speed less `EXIT_MAINLINE_EASE_MPH`, and never under the ramp's
    /// own cruise number. Real drivers do ease a little in the through lanes,
    /// but they diverge fast -- 58 to 70 mph at 70 mph sites (NCHRP Research
    /// Report 1081, 2024) -- and the ramp speed is reached in the
    /// deceleration lane past the gore, which is what that lane is for. The
    /// old floor was the ramp number itself, which put a truck at 44 mph in
    /// a 70 lane for the last half mile (owner-approved redesign,
    /// 2026-09-24). Road speed is read at the gore, so a work zone there
    /// counts.
    pub fn exit_approach_floor_mph(&mut self, stop: Option<&RoadStop>) -> f64 {
        let ramp_cruise = self.armed_ramp_cruise_mph(stop);
        let gore_mi = stop
            .or(self.exit_stop.as_ref())
            .or(self.ramp_stop.as_ref())
            .map(|stop| stop.at_mi);
        let Some(gore_mi) = gore_mi else {
            return ramp_cruise;
        };
        let (road, _) = self.trip.speed_limit_at(gore_mi);
        ramp_cruise.max(road - EXIT_MAINLINE_EASE_MPH)
    }

    /// The speed route-transition assistance stays engaged down to, once it
    /// has engaged at the ramp cap.
    ///
    /// A NAMED number of its own, deliberately, because the bug it fixes was
    /// this one being borrowed from [`Self::armed_ramp_cruise_mph`]. That is a
    /// cruise TARGET and floors at `RAMP_MIN_DESIGN_MPH` so cruise is never
    /// set below a ramp's design minimum -- correct for a target, wrong for a
    /// release threshold, which only ever has to sit below the engage
    /// threshold. With the floor in it, any ramp posted at or under
    /// `RAMP_MIN_DESIGN_MPH + RAMP_CRUISE_HEADROOM_MPH` clamped both numbers
    /// to the same value, the hysteresis band collapsed to a point, and the
    /// assist announced itself slowing and released on alternate frames all
    /// the way down (owner, live drive on a 30 mph ramp, 2026-09-18).
    pub fn ramp_assist_release_mph(cap_mph: f64) -> f64 {
        cap_mph - RAMP_CRUISE_HEADROOM_MPH
    }

    /// Ease automatic speed control for an armed exit.
    ///
    /// Arming an exit commits the truck to leaving the highway, and the gore
    /// only accepts road speed plus the posts' leeway, so the cruise target
    /// comes down to [`Self::exit_approach_floor_mph`] -- at most ten under
    /// road speed, never the ramp's number. The ramp's number is reached past
    /// the gore, in the deceleration lane, where taking the ramp pauses speed
    /// control.
    pub fn cap_cruise_for_ramp(&mut self, stop: Option<&RoadStop>) {
        let floor = self.exit_approach_floor_mph(stop);
        let Some(cruise_mph) = self.cruise_mph else {
            // Paused mid-session -- a zone keeper, or a planned-stop pause.
            // Remember the cap so cruise resumes under it, but say nothing:
            // the keeper is already holding a low zone speed.
            if self.speed_control_armed {
                if let Some(target) = self.speed_control_target_mph {
                    self.cruise_exit_mph = Some(target.min(floor));
                }
            }
            return;
        };
        let capped = cruise_mph.min(floor);
        if self
            .cruise_exit_mph
            .is_some_and(|existing| existing <= capped)
        {
            // The destination-exit announcement already capped cruise.
            return;
        }
        self.cruise_exit_mph = Some(capped);
        // Silent. The exit call used to explain how cruise would take the
        // ramp (hold road speed, ease, pause on the ramp); players do not
        // need the controller's plan read to them (owner, 2026-10-14: "Players
        // don't need to know how the speed control works"). The cap glides on
        // its own, see `ramp_approach_cap_mph`.
    }

    /// The armed exit's cap right now, measured off the road still left.
    ///
    /// The ramp target is where the truck has to BE at the gore. Applied the
    /// moment the exit is armed it is also where the truck goes immediately,
    /// and an exit arms as much as five miles out (further under time
    /// compression, which is what the arming window is sized in) -- so a
    /// driver heard the callout and then watched automatic control sit at 40
    /// for miles of open interstate with the exit nowhere near (tester
    /// report, Shane, 2026-08-15).
    ///
    /// Instead the cap glides: corridor speed stands until the exit is inside
    /// the road this truck needs to shed for it, then comes down along the
    /// deceleration itself, reaching its floor -- at most ten under road
    /// speed, see [`Self::exit_approach_floor_mph`] -- a little before the gore.
    /// The road is priced exactly as the keeper's ease prices it -- a reaction
    /// budget in real seconds at the speed the truck is doing, and a
    /// comfortable shed rate under that.
    ///
    /// In REAL miles, not compressed ones. Pricing the road through the
    /// effective time scale looked prudent and was the same report all over
    /// again: at high pacing the cap fell under a 65 mph cruise nine miles
    /// out, so signalling early was itself what slowed the truck (Shane,
    /// 2026-08-15, signalling nine miles before a truck stop). The clock is
    /// where that problem belongs and is now where it is solved --
    /// `Trip::armed_exit_decompression` puts the trip back on real time for
    /// the whole approach window, which is wider than this glide -- so by the
    /// time the cap has anything to say, the miles under it really are real
    /// ones.
    pub fn ramp_approach_cap_mph(&self) -> Option<f64> {
        let floor = self.cruise_exit_mph?;
        if self.ramp_mi.is_some() {
            return Some(floor); // already on the ramp: the number is the number
        }
        let stop = self.exit_stop.as_ref().or(self.ramp_stop.as_ref());
        let Some(stop) = stop else {
            return Some(floor);
        };
        let ahead = stop.at_mi - self.trip.position_mi;
        if ahead <= 0.0 {
            return Some(floor);
        }
        // Priced at the set speed, not the live one, so the cap cannot chase
        // its own slowing and hand the road back a mile an hour at a time.
        let speed = self
            .trip
            .truck
            .speed_mph()
            .max(self.cruise_mph.unwrap_or(0.0))
            .max(floor);
        let reaction_mi = APPROACH_REACTION_S * speed / 3600.0;
        let brake_m = 0.0f64.max(ahead - reaction_mi) * METERS_PER_MILE;
        let floor_mps = floor / MPH_PER_MPS;
        let allowed =
            (floor_mps * floor_mps + 2.0 * APPROACH_DECEL_MPS2 * brake_m).sqrt() * MPH_PER_MPS;
        Some(floor.max(allowed))
    }

    /// `_reset_exit_lane_state()`.
    pub fn reset_exit_lane_state(&mut self) {
        self.exit_lane_entered = false;
        self.exit_taper_said = false;
        self.lane.exit_lane_open = false;
        self.exit_cancel_armed = false;
        self.exit_right_hold_s = 0.0;
        self.exit_right_taps = 0;
        self.exit_tap_hint_said = false;
        self.exit_countdown_said.clear();
    }

    /// In the rightmost travel lane, or changing into it: the only lane the
    /// exit lane opens beside.
    pub fn in_right_lane_for_exit(&self) -> bool {
        self.lane.lane == 0 || self.lane_change_target == Some(0)
    }

    /// Whether the truck is in the exit lane.
    ///
    /// The exit lane is the deceleration lane: an auxiliary lane with no
    /// through traffic that opens beside the right lane at its taper, just
    /// ahead of the gore. A driver moves to the rightmost travel lane on the
    /// approach, stays centred there, and moves into the exit lane only where
    /// it opens (MUTCD 11th ed. 2E.23 and 2E.25). This used to be a lateral
    /// offset inside the right lane that had to be held for miles, which had
    /// the driver pushing at the shoulder the whole approach and drifting in
    /// and out of "set" -- and the overcorrection off that edge carried a
    /// truck across into the left lane and back onto the right lane's traffic
    /// (owner's drive, I-70 East into Denver West, 2026-09-24).
    pub fn exit_lane_ready(&self) -> bool {
        self.in_right_lane_for_exit() && self.exit_lane_entered
    }

    /// Whether an exit lane is due beside this truck: an exit it is set to
    /// take, lane work that is the driver's, and the truck in the right lane
    /// without having taken the lane yet.
    pub fn exit_lane_due(&self, ctx: &GameContext, stop: &RoadStop) -> bool {
        ctx.settings.lane_is_manual()
            && self.exit_intent_ready(ctx, stop)
            && self.in_right_lane_for_exit()
            && !self.exit_lane_entered
    }

    /// Distance reminders for an armed exit, every steering mode.
    ///
    /// A canyon approach buries a single signal-on announcement under
    /// pacenotes and limit changes (owner playtest: signal at 4.7 miles,
    /// then silence until the miss). The countdown re-anchors the exit as
    /// it closes, and names the lane fix while there is road to make it.
    ///
    /// Terse speech opts out of the whole countdown: the player asked for
    /// the signal-on announcement to be the last word.
    pub fn update_exit_countdown(&mut self, ctx: &mut GameContext, stop: &RoadStop) {
        if self.terse_speech(ctx) {
            return;
        }
        let ahead = stop.at_mi - self.trip.position_mi;
        if ahead <= 0.0 {
            return;
        }
        let crossed: Vec<f64> = EXIT_COUNTDOWN_MILESTONES_MI
            .iter()
            .copied()
            .filter(|m| ahead <= *m && !self.exit_countdown_said.contains(m))
            .collect();
        if crossed.is_empty() {
            return;
        }
        // Time compression can cross several milestones in one frame:
        // mark them all, speak only the nearest.
        self.exit_countdown_said.extend(crossed.iter().copied());
        let nearest = crossed.iter().copied().fold(f64::INFINITY, f64::min);
        let distance = if nearest >= 1.0 {
            ctx.settings.distance_text(nearest, false)
        } else {
            ctx.settings.short_distance_text(nearest)
        };
        let name = if stop.stop_type == "delivery_destination" {
            "Destination exit".to_string()
        } else {
            format!("Exit for {}", stop.spoken_name())
        };
        // What this exit is still owed, in the order it has to be done. The
        // signal only ever reaches here on a destination exit, because every
        // other exit is armed BY signalling for it -- and it leads, because a
        // truck squarely in the exit lane with no signal set is a miss, and
        // the anchor that said only "Destination exit in half a mile." was the
        // last word before the loop-back (agent drive into Payson,
        // 2026-09-19). See `exit_signal_instruction`.
        let mut owed = String::new();
        if ctx.settings.lane_is_manual() && !self.exit_signal_on {
            owed.push(' ');
            owed.push_str(&self.exit_signal_instruction());
        }
        // The right lane, and only while the truck is not in it. The exit lane
        // itself is asked for where it opens, at the taper.
        if !self.in_right_lane_for_exit() {
            owed.push_str(&right_lane_request(ctx));
            if self
                .active_exit_pressure(stop)
                .is_some_and(|p| p.intensity >= 0.35)
            {
                owed.push_str(" Traffic is tight.");
            }
        }
        ctx.audio.play_with("ui/notify", 0.6, 0.0);
        let mut opts = SayEvent::queued().priority(EventPriority::Important);
        opts.category = Some(SpeechCategory::Navigation);
        ctx.say_event_with(format!("{name} in {distance}.{owed}"), opts);
    }

    /// `_update_exit_preparation(keys, dt)`.
    pub fn update_exit_preparation(&mut self, ctx: &mut GameContext, dt: f64) {
        // Past the gore the exit's braking happens here, ahead of the physics
        // step like every other assist's pedal; it stands itself down off
        // the deceleration lane.
        self.update_deceleration_lane(ctx);
        // The lane pass ran first this frame: a steer across the line the
        // exit lane opened is the truck in it. The lane is reopened below only
        // while it still stands.
        let steered_in = std::mem::take(&mut self.lane.entered_exit_lane);
        self.lane.exit_lane_open = false;
        let Some(stop) = self.exit_stop.clone() else {
            self.reset_exit_lane_state();
            return;
        };
        if steered_in && self.ramp_mi.is_none() {
            self.exit_lane_entered = true;
            let volume = 1.0f64.min(0.7 * self.cue_loudness(ctx));
            ctx.audio.play_with("vehicle/lane_line_cross", volume, 0.6);
        }
        if self.ramp_mi.is_some() {
            self.reset_exit_lane_state();
            return;
        }
        // The signal is how a driver COMMITS to an exit -- but with lane
        // keeping automated they never press it, because the game itself says
        // "lane keeping will take this exit". Gating the speed assist on the
        // signal therefore switched it off for exactly the preset that
        // promises the most help: the announcement said "adaptive cruise will
        // ease to 40 for the ramp", nothing eased, and the truck went through
        // the gore at 53 and missed the exit (owner playtest, Denver->
        // Silverthorne, 2026-08-19). Automated lane keeping IS the commitment.
        let automated = ctx.settings.lane_is_automated();
        let committed = self.exit_signal_on || automated;
        if committed {
            self.update_exit_countdown(ctx, &stop);
            self.update_exit_speed_assist(ctx, &stop);
        }
        if automated {
            self.keep_right_for_exit(ctx, &stop);
            return;
        }
        if !self.exit_signal_on {
            return;
        }
        let ahead = stop.at_mi - self.trip.position_mi;
        if ahead < -EXIT_COMMIT_WINDOW_MI {
            return;
        }

        // The exit lane opens at its taper, just ahead of the gore, and stays
        // open through the gore window. The cab calls it once, where the exit
        // direction sign stands; before that there is nothing to steer into,
        // and a truck already in the right lane is told nothing at all.
        let due = self.exit_lane_due(ctx, &stop) && ahead <= EXIT_TAPER_MI;
        if due && !self.exit_taper_said {
            self.exit_taper_said = true;
            ctx.audio.play_with("ui/notify", 0.6, 0.0);
            // Never handed back once the truck is in the lane (the take line
            // cuts it) or past the end of the gore window.
            let window_end = stop.at_mi + EXIT_COMMIT_WINDOW_MI;
            let mut opts =
                SayEvent::new().valid(move || !live::on_ramp() && live::position_mi() < window_end);
            opts.category = Some(SpeechCategory::Navigation);
            ctx.say_event_with("Exit lane opening. Steer right into it.", opts);
        }
        self.lane.exit_lane_open = due && self.exit_taper_said;

        let right = self.held(ctx, Action::SteerRight);
        // A quick tap is how full-lane-keeping players change lanes; when the
        // lane work is yours it only nudges the wheel and never reaches the
        // exit lane. Two taps at an open exit lane earn the how-to, once, so
        // the silence never reads as broken keys.
        if right {
            self.exit_right_hold_s += dt;
        } else {
            if self.exit_right_hold_s > 0.0 && self.exit_right_hold_s <= EXIT_TAP_HOLD_S {
                self.exit_right_taps += 1;
            }
            self.exit_right_hold_s = 0.0;
        }
        if self.exit_right_taps >= 2 && self.lane.exit_lane_open && !self.exit_tap_hint_said {
            self.exit_tap_hint_said = true;
            let text = format!("Hold {} to steer in.", ctx.control_name(Action::SteerRight));
            self.say_plain(ctx, text);
        }
        // No "Exit lane set" or "lost": crossing into the lane is the truck
        // taking the exit, and the ramp line says so. No line at the gore. It used to say "Stay under" the ramp's number,
        // which the gate itself never asked for (it accepts road speed), and
        // the ramp's speed belongs past the gore anyway, where taking the
        // ramp names it. Without the number all it restated was the half-mile
        // anchor a tenth of a mile earlier (realistic exit, 2026-09-24).
    }

    /// Slow an armed exit toward ramp speed, in EVERY steering mode.
    ///
    /// This used to sit below the lane-work early return, so it never ran
    /// with `lane_keeping` on full -- and the All assists preset selects full
    /// lane keeping, which meant the easiest preset silently disabled one of
    /// the assists it had just turned on.
    pub fn update_exit_speed_assist(&mut self, ctx: &mut GameContext, stop: &RoadStop) {
        if !ctx.settings.exit_speed_assist {
            return;
        }
        let ahead = stop.at_mi - self.trip.position_mi;
        if !(ahead > 0.0 && ahead <= ff_core::sim::trip_models::EXIT_SPEED_ASSIST_START_MI) {
            return;
        }
        let controlled = self.cruise_mph.is_some() || self.keeper_mph.is_some();
        // A controller already holding the road gets its brake deadband over
        // the gate before the assist takes the pedals from it: cruise set at
        // a 45 whose gore accepts 45 hovers a fraction over it, and the assist
        // paused cruise a mile and a half out for 0.3 mph, then held the truck
        // fifteen under the road for the rest of the approach (every-assist
        // audit, 2026-09-24). Cruise's own exit glide brings it under.
        let margin = if controlled {
            EXIT_ASSIST_CONTROLLER_MARGIN_MPH
        } else {
            0.0
        };
        if self.trip.truck.speed_mph() <= self.gore_acceptance_mph(Some(stop)) + margin {
            if controlled {
                // Nothing to shed and a controller already holding the road:
                // leave it. This used to pause speed control the moment the
                // exit came inside its reach, whether or not it had anything
                // to brake for -- right for the old flat 45, where every
                // truck at road speed was over it, wrong now that the gore
                // accepts road speed. Paused with nothing to do, the assist
                // coasted; on a 3.7 percent downgrade the truck ran from 60 to
                // 69 with "automatic speed control paused" the only thing the
                // status said (owner, Spokane, twice, 2026-08-21/22). Cruise
                // holds the grade and its own glide eases to its exit floor
                // at the gore, which is what the callout promised.
                return;
            }
            // Under what the gore accepts with nobody on the pedals. HOLD
            // the exit floor to the gore rather than handing back an empty
            // one: left alone the
            // truck coasted the rest of the approach down to a dead stop in
            // the through lane, a quarter mile short of its own exit -- worst
            // at real-time pacing, where the coast has the most seconds to
            // finish.
            self.hold_exit_approach_speed();
            return;
        }
        if self.cruise_mph.is_some() || self.keeper_mph.is_some() {
            // Over what the gore accepts: the assist takes the pedals for the
            // ramp; the session is not its to end. Disarming here was the
            // first of the three places that left both controllers dead for
            // the rest of the run (Shane, 2026-08-15) -- and the keeper has to
            // come off too, or it fights the assist's own brake. A destination
            // exit still holds like any arrival; every other exit is a
            // transit stop.
            let transit = stop.stop_type != "delivery_destination";
            self.pause_speed_control(ctx, transit);
        }
        // Faded in over the first mile an hour past the line. A full 0.35 the
        // moment the truck crossed it pumped the pedal on a downgrade, where
        // gravity put the truck straight back over after each application,
        // and every application costs air.
        let over = self.trip.truck.speed_mph() - self.gore_acceptance_mph(Some(stop)) - margin;
        self.trip.truck.brake = self.trip.truck.brake.max(0.35 * over.min(1.0));
        if self.assist_exit_slowing_said {
            return;
        }
        self.assist_exit_slowing_said = true;
        // Never name a key this driver's settings do not give them: with lane
        // drift off a tap changes lanes, and holding Right does nothing.
        // And only a lane the truck is not already in.
        let lane_text = if self.in_right_lane_for_exit() {
            String::new()
        } else {
            right_lane_request(ctx)
        };
        // Never "confirm": there is no confirm action, and an X pressed to
        // obey it cancels the signal instead.
        let mut opts = SayEvent::queued().priority(EventPriority::Important);
        opts.category = Some(SpeechCategory::Confirmation);
        ctx.say_event_with(format!("Slowing.{lane_text}"), opts);
    }

    /// Keep the truck at its exit floor on an approach the assist is running.
    ///
    /// A light, bounded throttle and never a brake. It stands down the moment
    /// the driver is on a pedal of their own, because slowing further for
    /// their own gore is their call; the driver can always ask for more than
    /// this, and the assist's own brake above gore acceptance caps the other
    /// end. The floor is [`Self::exit_approach_floor_mph`], at most ten under
    /// road speed: holding the RAMP's number here is what left trucks
    /// crawling down the through lane.
    /// Never above the posted limit where the truck is: the floor reads the
    /// gore's number, and a work zone that ends before the gore still binds.
    /// Says nothing: the slowing line already named who has the pedal, and
    /// holding the speed it announced is the same assist finishing its job.
    pub fn hold_exit_approach_speed(&mut self) {
        let position = self.trip.position_mi;
        let (here, _) = self.trip.speed_limit_at(position);
        let target = self.exit_approach_floor_mph(None).min(here);
        let t = &mut self.trip.truck;
        if !t.engine_on || t.stalled || t.air_brakes_holding() {
            return;
        }
        if t.brake > 0.01 || t.emergency_brake || t.transmission.in_reverse() {
            return;
        }
        // Clutch in or out of gear on a manual: throttle only revs the
        // engine, and it held the revs up through the driver's downshift.
        let tr = &t.transmission;
        if !tr.automatic && (tr.clutch > 0.5 || tr.shifting() || tr.in_neutral()) {
            return;
        }
        let short_by = target - t.speed_mph();
        if short_by <= 0.0 {
            return; // coasting between the target and the gore's limit is fine
        }
        // A firmer gain than the old ramp-speed hold needed: at the exit
        // floor a loaded truck wants most of this throttle just to stay
        // rolling, and a tenth per mile per hour sagged it four under.
        t.throttle = t.throttle.max(EXIT_HOLD_MAX_THROTTLE.min(short_by / 3.0));
    }

    /// `_active_exit_pressure(stop)`.
    pub fn active_exit_pressure(&self, stop: &RoadStop) -> Option<TrafficPressure> {
        let sample_mi = self.trip.position_mi.min(stop.at_mi);
        let pressure = self.trip.traffic_pressure_at(Some(sample_mi))?;
        if pressure.kind != "exit" {
            return None;
        }
        if pressure.start_mi <= stop.at_mi && stop.at_mi <= pressure.end_mi + 0.2 {
            return Some(pressure);
        }
        None
    }

    /// Arming and announcement window for exits, scaled like zone warnings.
    ///
    /// At speed under time compression a fixed window shrinks to nothing in
    /// real terms -- at 74 mph on standard pacing, 5 miles is about 14 real
    /// seconds, and it was half that on the retired Realistic setting: not
    /// enough to hear the callout, arm the exit, and brake to ramp speed.
    /// Scale the window so it covers roughly `EXIT_WARNING_REAL_S` of real
    /// time at the current pace.
    pub fn exit_window_mi(&self) -> f64 {
        let speed = self.trip.truck.speed_mph().max(30.0);
        let miles = EXIT_WARNING_REAL_S * speed * self.trip.effective_time_scale() / 3600.0;
        EXIT_WINDOW_MI.max(miles.min(EXIT_WINDOW_MAX_MI))
    }

    /// `_upcoming_exit_stop()`.
    pub fn upcoming_exit_stop(&mut self, ctx: &mut GameContext) -> Option<RoadStop> {
        let window = self.exit_window_mi();
        let stop = self.trip.upcoming_stop(window).cloned();
        let Some(destination) = self.destination_exit_stop(ctx) else {
            return stop;
        };
        let ahead = destination.at_mi - self.trip.position_mi;
        let announced_destination_is_actionable = ahead > 0.0
            && self.destination_exit_response_s > 0.0
            && Self::destination_exit_key(&destination) == self.destination_exit_announced_key;
        if announced_destination_is_actionable {
            // X responds to the exit just named, even if an optional stop has
            // since entered the ordinary lookahead window.
            return Some(destination);
        }
        if !(ahead > 0.0 && ahead <= window) {
            return stop;
        }
        match stop {
            None => Some(destination),
            Some(stop) if destination.at_mi <= stop.at_mi => Some(destination),
            Some(stop) => Some(stop),
        }
    }

    /// `_exit_intent_ready(stop)`.
    /// The one sentence a manual approach owes: set the signal.
    ///
    /// The signal is what COMMITS the truck to an exit -- [`Self::exit_intent_ready`]
    /// grants a destination exit on the signal alone once the lane work is the
    /// driver's -- and until 2026-09-19 nothing on the approach said so. The
    /// announcement and both distance anchors named the lane and the ramp
    /// speed, the driver moved right and slowed as told, and the first mention
    /// of a signal in the whole run was "You missed the exit for the Payson
    /// metro freight market. The turn signal was not set." (agent drive,
    /// AZ-260 into Payson). Then the loop-back line names the control -- so the
    /// game only ever explained the gate after it had closed.
    ///
    /// Named while the instruction is still being taught and bare once the
    /// player has armed enough signals to retire it (research doc R7), but
    /// never dropped the way the stop callout drops it: an optional stop costs
    /// a driver who ignores it nothing, and this one costs the loop-back.
    pub fn exit_signal_instruction(&self) -> String {
        if self.trip.exit_hint.is_empty() {
            return "Signal for it.".to_string();
        }
        format!("Press {} to signal.", self.trip.exit_hint)
    }

    pub fn exit_intent_ready(&self, ctx: &GameContext, stop: &RoadStop) -> bool {
        if self.exit_signal_canceled {
            return false;
        }
        if self.exit_signal_on {
            return true;
        }
        stop.stop_type == "delivery_destination" && ctx.settings.lane_is_automated()
    }
}

/// Ask for the right lane, when the move is the driver's. Lane keeping on
/// full makes it itself (`keep_right_for_exit`), so it is not asked for: a
/// tap into a lane lane keeping is waiting on is a sideswipe.
fn right_lane_request(ctx: &GameContext) -> String {
    if ctx.settings.lane_is_automated() {
        String::new()
    } else {
        " Move to the right lane.".to_string()
    }
}
