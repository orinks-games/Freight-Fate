//! Discrete lanes on top of the drift model: the per-frame lane pass, tap
//! changes, crossings, a road that narrows under the truck, coned-off lanes,
//! and keep-right pressure.

use crate::speech::EventPriority;
use ff_core::data::curves::{advisory_with_bank_mph, superelevation_at};
use ff_core::pyfmt::fmt_grouped;
use ff_core::sim::lane::{in_lane_phrase, RoadConditions};
use ff_core::sim::trip_models::Zone;
use ff_core::speech_pacing::SpeechCategory;
use ff_core::speech_text::SpokenMessage;

use crate::app::{GameContext, SayEvent};
use crate::bindings::Action;
use crate::states::driving::DrivingState;
use crate::states::driving_core::*;
use crate::states::driving_events::ambient::Ambient;
use crate::states::driving_updates::{
    lane_count_words, CURVE_ASSIST_JAKE_FULL_MPH, JAKE_MIN_RPM, LANE_CROSS_REPEAT_S,
    SIDESWIPE_REPEAT_S,
};

impl DrivingState {
    pub fn update_lane(&mut self, ctx: &mut GameContext, dt: f64) {
        let mode = ctx.settings.lane_keeping.clone();
        let mut key_dir = 0i8;
        if self.held(ctx, Action::SteerLeft) {
            key_dir -= 1;
        }
        if self.held(ctx, Action::SteerRight) {
            key_dir += 1;
        }
        // The left stick provides analog steering when the keys are idle.
        let stick = if self.pad_live(ctx) {
            ctx.controller.steering()
        } else {
            0.0
        };
        self.lane.steer_input(key_dir, stick);
        if ctx.settings.lane_is_automated() {
            self.update_stick_lane_change(ctx, stick);
        }
        self.lane.straighten = self.held(ctx, Action::Straighten);
        // The exit ramp is a single lane; the mainline keeps its leg count.
        self.lane_before_narrow = Some(self.lane.lane);
        let count = if self.ramp_mi.is_some() {
            1
        } else {
            self.lane_count_here()
        };
        self.lane.set_lane_count(count);
        // A narrowing road renumbers the lanes under the truck, so this has to
        // run the moment the count changes and before anything polices where
        // the truck is.
        self.leave_a_lane_the_road_closed(ctx);
        // Use the real baked curve data when the truck is inside a curve.
        // The lane model carries a HEADING, so running wide is not a push
        // applied to the position -- it is what happens by itself when the
        // road turns and the truck does not. What it wants from here is the
        // road's real geometry: one over the radius, signed positive when the
        // road bends right.
        let active = self.trip.curve_at(self.trip.position_mi);
        let route_transition_owns_ramp =
            ctx.settings.route_transition_assist && self.ramp_mi.is_some();
        let mut curve = match active.as_ref().filter(|c| !c.connector) {
            Some(bend) => {
                let direction = if bend.direction == 'L' { -1.0 } else { 1.0 };
                direction / (bend.min_radius_ft as f64).max(1.0)
            }
            None => 0.0,
        };
        // The too-fast warning, mapped bend and ramp curve alike: from the
        // roll model and the lane's own grip, before either costs anything
        // (`driving_rollover`).
        self.update_curve_warning(ctx);
        let ramp_curve = self.ramp_curve_radius_ft();
        if let Some(ramp_radius) = ramp_curve {
            // The ramp's controlling curve, past its deceleration lane. Its
            // radius is DERIVED from the speed it is posted at, through the
            // same AASHTO point-mass control the curve bake uses, rather than
            // a flat push invented for the old model; see
            // `ramp_curve_radius_ft`.
            //
            // NOT on the facility street chain, which keeps `ramp_mi` set long
            // after the ramp is behind the truck: a ramp's curvature held over
            // city streets bent a road that is straight, and with nobody
            // steering it walked the truck into the median and wrote it off
            // (every chain destination in the approach sweep, first run of the
            // heading model).
            curve += 1.0 / ramp_radius;
        }
        self.update_curve_run(ctx, active.as_ref());
        // Curve speed assist: use the real advisory speed when one is active
        // instead of the old terrain heuristic. Hysteresis both places:
        // engage above advisory + 5, but once slowing, hold until the truck
        // is within 2 of advisory. Deciding both ways at one threshold
        // flip-flopped against cruise seven times a second (playtest
        // 2026-07-22).
        let mut curve_assisting = false;
        let mut excess_now: Option<f64> = None;
        // How far over the number this assist is answering, whatever named it
        // -- a baked bend's advisory or a ramp's own geometry. `excess_now`
        // cannot serve: it stays None on a ramp on purpose, because it is
        // what gates the retarder, and a ramp is not a grade. Kept apart, the
        // service brake below read the None as "nothing over" and pressed a
        // flat tenth of the pedal down a hot ramp at any speed (review
        // finding, 2026-09-19).
        let mut over_mph: f64 = 0.0;
        if ctx.settings.curve_speed_assist {
            match active.as_ref().filter(|c| !c.connector) {
                Some(bend) => {
                    let margin = if self.curve_assist_active { 2.0 } else { 5.0 };
                    curve_assisting =
                        self.trip.truck.speed_mph() > bend.advisory_mph as f64 + margin;
                    excess_now =
                        Some(0.0f64.max(self.trip.truck.speed_mph() - bend.advisory_mph as f64));
                    over_mph = excess_now.unwrap_or(0.0);
                }
                None => {
                    if curve != 0.0 && !route_transition_owns_ramp {
                        // No baked bend here, so the geometry the lane model
                        // is steering to is all there is: a ramp's own radius.
                        // Priced with the same advisory the curve bake uses,
                        // rather than the old "50 minus a push times twenty",
                        // which stopped meaning anything once `curve` became a
                        // curvature instead of a shove.
                        let radius_ft = 1.0 / curve.abs();
                        let mut heuristic = advisory_with_bank_mph(radius_ft, RAMP_MAX_MPH) as f64;
                        if self.curve_assist_active {
                            heuristic -= 3.0;
                        }
                        curve_assisting = self.trip.truck.speed_mph() > heuristic;
                        over_mph = 0.0f64.max(self.trip.truck.speed_mph() - heuristic);
                    }
                }
            }
        }
        // The corner is the DRUMS' work, always. The retarder appears here
        // only when the road is going downhill, and then it is the grade it is
        // answering, not the bend. See the block below for the reasoning and
        // the sources; the short version is that a corner needs a precise
        // target speed, which is what service brakes are for, and a retarder
        // drives only the tractor's rear wheels, which is the last axle you
        // want retarding mid-bend.
        let jake_capable = {
            let t = &self.trip.truck;
            t.engine_on
                && t.throttle < 0.05
                && !t.transmission.in_neutral()
                && !t.transmission.shifting()
                && t.transmission.clutch <= 0.5
                && t.grip >= 0.55
                && t.rpm >= JAKE_MIN_RPM
        };
        // A real downgrade is the retarder's own job -- holding a loaded
        // truck back on a grade is the one use every noise ordinance
        // leaves legal -- so the overspeed line does not apply there.
        // on_downgrade draws the same line the G readout and the
        // ordinance exemption already draw between level road and a
        // grade, and adaptive cruise now asks it too. Without this
        // carve-out the assist held a six percent descent on the drums
        // alone: past fade in four and a half minutes, 585 degrees at ten
        // (bench trace, 2026-08-11).
        //
        // Asked EVERY frame, not once when the assist engages. A bend is a
        // stretch of road, not a moment, and the grade under it changes
        // while the truck is in it: a corner that starts on a pitch and
        // finishes on the flat or on the way up used to keep whatever the
        // pitch bought for the whole corner, because the release below only
        // ever ran when the CORNER ended. Measured on the jake sweep,
        // 2026-08-24: 6.9 seconds barking on level road and 4.6 seconds
        // barking at a climb, per bend, which is the owner's "on uphill
        // ascents the truck should gain speed instead of using the engine
        // brakes". Asking per frame also subsumes the old latch-yield retry:
        // a yielded latch is still draining on the engage frame, so the
        // throttle test above fails once and passes as soon as it drains.
        let downhill = self.on_downgrade();
        // A GRADE is the only thing that raises the retarder here. Slowing
        // FOR a corner is the service brakes' job, however much speed the
        // corner wants off -- owner ruling 2026-08-11, narrowing the
        // jake-first ruling of 2026-07-22 to grades only.
        //
        // The training material is unambiguous and it is not about noise.
        // The CDL manual's rule for a curve is to reach a safe speed
        // BEFORE entering it and then pull through on gentle throttle,
        // because braking mid-corner is what locks a wheel and jackknifes
        // a trailer -- and a retarder drives only the tractor's rear
        // wheels, which is precisely the axle you do not want retarding
        // through a bend. Jacobs, who build the thing, draw the same line:
        // the engine brake is for SUSTAINED speed control and is "not a
        // substitute for a service braking system", because it cannot give
        // the precise control the drums give. A corner is a precise target
        // speed. A descent is sustained control. Only the descent qualifies.
        //
        // A bend ON a grade still retards, because that is the grade's
        // doing, not the corner's -- and it has to: without this the assist
        // held a six percent descent on the drums alone and went past fade
        // in four and a half minutes, 585 degrees at ten (bench trace,
        // 2026-08-11).
        //
        // Which grade, though. `on_downgrade` is geometry at two percent --
        // the spoken advisory's release edge, never a braking number -- so the
        // assist barked its way through every shallow dip that happened to
        // carry a bend ("the jake activates on every single descent it seems,
        // even shallow descent like 1-3 percent", owner, 2026-08-24). It comes
        // UP now only where `retarder_warranted` says the drums cannot hold
        // the hill on their own, which is grade against weight against speed
        // on the truck's own brake heat model. It stays up while the road is
        // still going down at all, so the pair is hysteresis rather than one
        // line decided twice -- the same shape the ramp cap below has, for the
        // same reason.
        let worth_the_bark = curve_assisting && excess_now.is_some() && downhill;
        let worth_raising = curve_assisting && excess_now.is_some() && self.retarder_warranted();
        // Town no-engine-brake zones close the jake to the assist as well
        // (real downgrades stay exempt); the service trim below answers.
        if worth_raising
            && !self.curve_assist_jake
            && jake_capable
            && !self.trip.truck.engine_brake()
            && self.assist_jake_allowed(ctx)
        {
            // Stage one is a token two cylinders: past this line it would
            // bark without taking off the speed that called for it.
            self.trip.truck.engine_brake_stage =
                if excess_now.unwrap_or(0.0) > CURVE_ASSIST_JAKE_FULL_MPH {
                    3
                } else {
                    2
                };
            self.curve_assist_jake = true;
        } else if self.curve_assist_jake && !worth_the_bark {
            // The corner ended, or the grade under it did. Either way the
            // reason is gone, so the retarder goes back at once. Release
            // only the jake WE engaged; the player's own selection (or
            // their mid-curve override) is never touched.
            if self.trip.truck.engine_brake() {
                self.trip.truck.engine_brake_stage = 0;
            }
            self.curve_assist_jake = false;
        }
        let jake_slowing = self.trip.truck.engine_brake()
            && self.trip.truck.throttle < 0.05
            && self.trip.truck.grip >= 0.55;
        // With the retarder slowing the truck the drums join from 10 over,
        // faded in across the mile an hour under it: switched at 10, the
        // pedal pumped on and off while the truck sat on the line, and every
        // application costs air.
        let service_share = if jake_slowing {
            excess_now.map_or(0.0, |excess| (excess - 9.0).clamp(0.0, 1.0))
        } else {
            1.0
        };
        // The spoken cues get a cooldown on top: even a legitimate slow
        // cycle (cruise pulling back up to the engage line) must not chant.
        self.curve_assist_cue_s = (self.curve_assist_cue_s - dt).max(0.0);
        if curve_assisting && service_share > 0.0 {
            // Sized from how far OVER the bend's advisory the truck is, not
            // from `curve`: that used to be a severity around 1.0 and is a
            // curvature around 0.002 since the lane model grew a heading, so
            // reading it here quietly turned a third of the brakes into
            // nothing at all. `over_mph`, not `excess_now`: a ramp has a
            // number to be over too, and reading the retarder's gate here
            // pressed a flat tenth of the pedal down every hot ramp.
            self.trip.truck.brake = self
                .trip
                .truck
                .brake
                .max(service_share * 0.35f64.min(0.1 + over_mph * 0.02));
        }
        // ON A RAMP, THE RAMP OWNS THE SPEECH. A ramp with no baked bend is
        // priced off its own radius above, so an exit taken over that number
        // engages this assist -- and route-transition assistance is already
        // braking for
        // the sign or the light at the end of it and already says so. With
        // the realistic preset both are on by default, so every hot ramp
        // spoke twice, back to back (logged playtest of the four 1.9 assists,
        // 2026-07-15). The extra announcement goes, and the surviving line is
        // the more useful one because it names WHAT is slowing the truck. The
        // synthetic ramp weight remains for the lane model, but it is not a
        // corner. Asking the curve assist to brake for it made two held
        // service applications fight over the same descent (Joshua's ramp
        // drive, 2026-08-28).
        let on_ramp = self.ramp_mi.is_some();
        // AND A BEND THE APPROACH SERVO OWNS. The curve call armed the servo
        // and, with callouts on, already said "Curve assistance
        // slowing" for this bend on the approach -- and with callouts off
        // the deceleration is meant to be the only word. Either way the
        // words here are spoken for; the reactive brake above still applies
        // as the backstop it always was.
        let servo_owns_bend = self.curve_servo.is_some();
        if curve_assisting {
            if !self.curve_assist_active
                && self.curve_assist_cue_s <= 0.0
                && !on_ramp
                && !servo_owns_bend
            {
                self.curve_assist_cue_s = CURVE_ASSIST_CUE_COOLDOWN_S;
                self.curve_assist_spoke = true;
                // ROUTE, not the ambient default: names an automation taking
                // the pedals (automation-handoff sweep, 2026-08-20, the
                // deferred 2026-08-15 audit).
                ctx.say_event_with(
                    "Slowing for the bend.",
                    SayEvent::queued()
                        .priority(EventPriority::Important)
                        .category(SpeechCategory::Confirmation),
                );
            }
        } else if self.curve_assist_active
            && self.curve_assist_cue_s <= 0.0
            && self.curve_assist_spoke
        {
            self.curve_assist_cue_s = CURVE_ASSIST_CUE_COOLDOWN_S;
            self.curve_assist_spoke = false;
            // ROUTE, not the ambient default: names the automation handing the
            // pedals back (automation-handoff sweep, 2026-08-20, the deferred
            // 2026-08-15 audit).
            ctx.say_event_with(
                "Curve assistance released.",
                SayEvent::queued()
                    .priority(EventPriority::Important)
                    .category(SpeechCategory::Confirmation),
            );
        } else if !curve_assisting {
            // Engagement that never spoke (the ramp case) simply ends.
            self.curve_assist_spoke = false;
        }
        self.curve_assist_active = curve_assisting;
        // Hysteresis on the ramp cap, for the same reason the curve assist has
        // it: decided both ways on the one threshold, a truck riding the ramp
        // limit announced itself slowing and released over and over down a
        // single ramp -- and re-made the brake application, and paid the air
        // for it, every time round (bench, 2026-08-11).
        //
        // The release number has its own name rather than borrowing the cruise
        // target's; see `ramp_assist_release_mph` for the whole story.
        let ramp_cap_mph = self.armed_ramp_mph(None);
        let ramp_hold_mph = if self.transition_assist_active {
            DrivingState::ramp_assist_release_mph(ramp_cap_mph)
        } else {
            ramp_cap_mph
        };
        // Not in the deceleration lane: the truck arrives there at road speed
        // by design, and the lane's own servo brakes it to the exit speed by
        // the curve (`update_deceleration_lane`). Lifting there as well only
        // announced a second assist for the same slowing.
        let transition_assisting = ctx.settings.route_transition_assist
            && self.ramp_mi.is_some()
            && !self.in_deceleration_lane()
            && self.trip.truck.speed_mph() > ramp_hold_mph;
        if transition_assisting {
            // A ramp cap is sustained speed control, not the bar's stop.
            // Lift first and let drag shed the excess; holding a service
            // floor here spent air all the way down the ramp. The terminal
            // servo below still owns the real stop and its full hold.
            self.trip.truck.throttle = 0.0;
            if !self.transition_assist_active {
                // Cruise would otherwise put power straight back on after
                // this lane pass. Pausing it also keeps the lift from
                // chattering against its own target until the bar is honored.
                self.pause_speed_control(ctx, true);
                // ROUTE, not the ambient default: names an automation taking
                // the pedals (automation-handoff sweep, 2026-08-20, the
                // deferred 2026-08-15 audit). Named for the terminal where
                // there is one; see `say_ramp_lift`.
                self.say_ramp_lift(ctx);
            }
        } else if self.transition_assist_active && !self.ramp_terminal_owns_the_stop() {
            // ROUTE, not the ambient default: names the automation handing the
            // pedals back (automation-handoff sweep, 2026-08-20, the deferred
            // 2026-08-15 audit).
            //
            // Not while the terminal still has a stop to make. The cap lets go
            // five under its number, but the terminal's servo is still braking
            // for the bar, so "released" at 15 mph with the stop sign ahead was
            // untrue (agent drive into Abilene, 2026-09-23). The terminal's own
            // "Stopped at the sign" is the release there.
            ctx.say_event_with(
                "Route-transition assistance released.",
                SayEvent::queued()
                    .priority(EventPriority::Important)
                    .category(SpeechCategory::Confirmation),
            );
        }
        self.transition_assist_active = transition_assisting;
        let wind = self.trip.weather.effects().wind;
        let speed_mps = self.trip.truck.velocity_mps;
        let grip = self.trip.truck.effective_grip();
        // Curve assistance is one assist, not two: it brakes for the bend AND
        // supplies the wheel the bend wants (owner, 2026-09-18 -- "turn assist
        // and curve assist should probably be merged"). Lane keeping is the
        // other job, holding the truck between the lines against wander and
        // wind, and the two are separate settings because they are separate
        // jobs: feed-forward off the road's shape, feedback off the driver's
        // error.
        //
        // Partial lane keeping supplies the bend's wheel too (owner ruling,
        // 2026-09-24): its correction was capped with the driver's key at the
        // steering limit, so a bend at its own advisory ran wide under
        // partial lane keeping whenever curve assistance was off. Lane changes
        // and speed stay the driver's; lane keeping off stays manual.
        let takes_the_bend = ctx.settings.road_steers_the_bend();
        // The bank the bend is built with, which is load the tires do not
        // carry -- the same `superelevation_at` the advisory was priced with,
        // so the ceiling and the number the cab speaks agree about the road.
        // A ramp keeps its own geometry and no bank: its cap comes off the
        // posted ramp speed, not a design speed the bake never recorded.
        let bank = active
            .as_ref()
            .filter(|c| !c.connector)
            .map(|bend| {
                superelevation_at(
                    (bend.min_radius_ft as f64).max(1.0),
                    self.trip.leg_design_speed_mph(),
                )
            })
            .unwrap_or(0.0);
        // A street corner bends the lane's road as well, added here and not
        // above so the bend and ramp speed assists read what they always did
        // (`street_corner_curvature`).
        let road = RoadConditions {
            curvature: curve + self.street_corner_curvature(),
            wind,
            grip,
            bank,
        };
        // The turn a hold toward follows the road for, from its call on
        // (`hold_turn_side`, `LaneKeeping::turn_in_play`).
        self.lane.turn_in_play = self.hold_turn_side();
        let off_road_event = self.lane.update(dt, speed_mps, road, &mode, takes_the_bend);
        if off_road_event {
            // The shoulder costs the truck whether or not anything warns about
            // it: the warning setting governs the sound and the line, never
            // the physics. It used to return first, so with the warning off
            // leaving the road was free, and the rest of this frame's lane
            // work (a lane change in progress, a merge) was skipped with it
            // (agent drive, 2026-09-24).
            self.trip.truck.add_damage(1.0, true);
            if ctx.settings.lane_departure_warning {
                let pan = self.lane_pan();
                ctx.audio.play_with("vehicle/rumble_strip", 1.0, pan);
                self.announce_off_pavement(ctx);
            }
        } else if self.road_position_band.is_some() && !self.off_pavement() {
            // Back on the pavement: the standing condition ended, so its one
            // transition line speaks and the band resets (research doc R12).
            self.road_position_band = None;
            // Quiet speaks the recovery; Urgent only omits routine status.
            ctx.say_event_with(
                "Back on the pavement.",
                SayEvent::queued()
                    .review(true)
                    .category(SpeechCategory::Status),
            );
        }
        self.cross_repeat_s = (self.cross_repeat_s - dt).max(0.0);
        self.sideswipe_cooldown_s = (self.sideswipe_cooldown_s - dt).max(0.0);
        if self.lane.crossed != 0 {
            self.on_lane_crossed(ctx);
        }
        self.hold_keeping_lane_change(ctx);
        self.update_tap_lane_change(ctx, dt);
        self.update_pass_return(ctx);
        self.update_merge(ctx, dt);
        self.update_keep_right(ctx, dt);
    }

    /// Advance an assist-off tap change: signal clicks, then the flip.
    pub fn update_tap_lane_change(&mut self, ctx: &mut GameContext, dt: f64) {
        let Some(mut target) = self.lane_change_target else {
            if !self.steer_cue_active && !self.exit_blinker_on() {
                ctx.audio.release_cue("vehicle/turn_signal");
            }
            return;
        };
        let pan = if target > self.lane.lane { -0.6 } else { 0.6 };
        if !self.exit_blinker_on() {
            ctx.audio.update_cue("vehicle/turn_signal", 0.8, pan);
        }
        self.lane_signal_timer += dt;
        if self.lane_signal_timer >= LANE_SIGNAL_CLICK_S {
            self.lane_signal_timer = 0.0;
            if !self.exit_blinker_on() {
                ctx.audio.play_if_idle("vehicle/turn_signal", 0.8, pan);
            }
        }
        self.lane_change_timer -= dt;
        if self.lane_change_timer <= 0.0 {
            self.lane_change_target = None;
            if !self.exit_blinker_on() {
                ctx.audio.release_cue("vehicle/turn_signal");
            }
            target = target.min(self.lane.lane_count - 1);
            if target == self.lane.lane {
                // The lane it was moving to ended on the way: the truck never
                // left its own, so there is no new lane to check or name. Run
                // on as an arrival, the check found the vehicle ahead in this
                // lane and billed it as a sideswipe.
                return;
            }
            // Check the closure again on arrival, not just when the key was
            // pressed. A change takes seconds, and in those seconds the truck
            // can reach the cones or the road can narrow and renumber the
            // lanes -- either way the clamp above would otherwise land the
            // truck in the closed lane it was moving to avoid.
            if Some(target) == self.closed_lane_here() {
                ctx.audio.play("ui/error");
                let text = format!(
                    "The {} lane is closed. Staying in the {} lane.",
                    lane_label(target, self.lane.lane_count),
                    self.lane.lane_name()
                );
                ctx.say_event_with(text, SayEvent::new().category(SpeechCategory::Safety));
                return;
            }
            self.lane.lane = target;
            // The tap change crosses the same painted line: same marker roll.
            let volume = 1.0f64.min(0.7 * self.cue_loudness(ctx));
            ctx.audio.play_with("vehicle/lane_line_cross", volume, pan);
            self.finish_lane_change(ctx, false);
        }
    }

    /// A held drift carried the truck across a line: the wheel was the
    /// lane change. The tires roll the line's raised markers every time --
    /// physics does not rate-limit -- but the signal tone and the spoken
    /// lane name mark a DELIBERATE change only. Pinballing back and forth
    /// across the same line mid-bend repeats just the quieter thump
    /// (owner: "the thing dings at you as you're going").
    pub fn on_lane_crossed(&mut self, ctx: &mut GameContext) {
        let repeat = self.cross_repeat_s > 0.0;
        self.cross_repeat_s = LANE_CROSS_REPEAT_S;
        let pan = if self.lane.crossed > 0 { -0.6 } else { 0.6 };
        let volume = 1.0f64.min((if repeat { 0.4 } else { 0.7 }) * self.cue_loudness(ctx));
        ctx.audio.play_with("vehicle/lane_line_cross", volume, pan);
        if !repeat {
            ctx.audio.play_with("vehicle/signal_tone", 0.6, pan);
        }
        self.finish_lane_change(ctx, repeat);
    }

    /// The truck has just arrived in a new lane: check the space it moved
    /// into, resolve any dodgeable hazard, and reset keep-right pressure.
    pub fn finish_lane_change(&mut self, ctx: &mut GameContext, quiet: bool) {
        let pass_return = std::mem::take(&mut self.pass_returning);
        self.left_lane_s = 0.0;
        self.keep_right_nags = 0;
        let lane_index = self.lane.lane;
        let lane_count = self.lane.lane_count;
        // Lane keeping's move back keeps a real headway (`pass_return_blocker`)
        // and re-checked it every frame of the move, so dropping in behind a
        // vehicle that just went by is not contact.
        let other = if pass_return {
            self.pass_return_blocker(lane_index)
        } else {
            self.trip.traffic_manager.vehicle_in_lane(
                self.trip.position_mi,
                lane_index,
                DODGE_CLEARANCE_AHEAD_MI,
                DODGE_CLEARANCE_BEHIND_MI,
                0.0,
                0.0,
            )
        }
        .map(|vehicle| vehicle.vehicle_class.clone());
        if let Some(vehicle_class) = other {
            if self.trip.truck.speed_mph() > LANE_MIN_MPH {
                if self.sideswipe_cooldown_s > 0.0 {
                    // One contact, not three. A truck pinballing across the same
                    // painted line ran this branch on every crossing, so a single
                    // sideswipe was billed and announced repeatedly inside half a
                    // second (tester transcript, 2026-08-11). The damage and the
                    // warning both belong to the contact, and it happened once.
                    return;
                }
                self.sideswipe_cooldown_s = SIDESWIPE_REPEAT_S;
                ctx.audio.play("vehicle/collision");
                ctx.controller.rumble.impact(SIDESWIPE_DAMAGE);
                self.trip.truck.apply_collision(SIDESWIPE_DAMAGE, true);
                let text = format!(
                    "You sideswiped a {vehicle_class} in the {} lane! Truck damage {:.0} percent.",
                    self.lane.lane_name(),
                    self.trip.truck.damage_pct
                );
                ctx.say_event_with(text, SayEvent::new().category(SpeechCategory::Safety));
                return;
            }
        }
        if self.hazard_deadline.is_some() && self.hazard_dodgeable && lane_index != self.hazard_lane
        {
            // The swerve answered it, so the assist's application comes off
            // with the hazard rather than being left on a truck now clear of
            // it. Terse mode's whole confirmation is the hazard-clear earcon
            // that just played; failure is the collision sound and its
            // spoken damage line, so the outcome pair is never ambiguous
            // (R4, R14).
            let names = self.hazard_names_text();
            // Lane keeping's own pass is not the driver's swerve to praise:
            // say where the truck is now, the line a tap change ends on.
            let text = if self
                .passing
                .is_some_and(|(_, passed_in)| passed_in == lane_index)
            {
                format!(
                    "In {}, passing {names}.",
                    lane_phrase(lane_index, lane_count)
                )
            } else {
                format!("Clear of {names}.")
            };
            self.finish_hazard_clear(ctx, &text);
            return;
        }
        // Lane keeping back in its lane after going around traffic is the
        // end of the pass, and the pass itself is TRAFFIC: words at
        // standard only. At quiet "In the right lane." used to arrive on
        // its own, about a pass nobody had mentioned (speech mode audit,
        // 2026-10-03).
        let category = if pass_return {
            SpeechCategory::Traffic
        } else {
            SpeechCategory::Confirmation
        };
        if !quiet {
            // ROUTE, not the ambient default: the driver's only confirmation
            // of where a lane change landed, same class as the lane-open
            // precedent in driving_lane_gap.py (automation-handoff sweep,
            // 2026-08-20, the deferred 2026-08-15 audit).
            ctx.say_event_with(
                format!("{}.", in_lane_phrase(lane_index, lane_count)),
                SayEvent::queued()
                    .priority(EventPriority::Important)
                    .category(category),
            );
        }
    }

    /// The coned-off lane index in the truck's own lane numbering.
    ///
    /// Asked of the trip, which reads the closure's side against the lanes
    /// the road has here, and told the count the truck is actually steering
    /// in -- the exit ramp is one lane whatever the mainline carries. This
    /// is the only place the driving state learns which lane is shut, so no
    /// two checks can answer differently.
    pub fn closed_lane_here(&self) -> Option<i64> {
        self.trip.closed_lane_at(None, Some(self.lane.lane_count))
    }

    /// The nearest lane the truck may legally be in, or None if there is
    /// no such lane.
    pub fn open_lane_beside(&self, closed: i64) -> Option<i64> {
        let count = self.lane.lane_count;
        if count < 2 {
            return None;
        }
        let candidate = if closed > 0 { closed - 1 } else { closed + 1 };
        if !(0..count).contains(&candidate) || candidate == closed {
            return None;
        }
        Some(candidate)
    }

    /// Move the truck out of a closure it never drove into, and say so
    /// whenever the road itself -- closure or not -- just forced a move.
    ///
    /// The road, not the driver, can put a truck in coned-off lanes: where a
    /// stretch narrows, the lane count renumbers the lanes under the truck
    /// and the count clamp drops it a lane over, which can be the closed one.
    /// Shane's Detroit-Mansfield run is what that sounds like -- told the
    /// right lane was closed, moved left, and then put back in the closed
    /// lane with nothing to do about it. Whenever the road moves the truck
    /// into a closure it is moved straight back out and told so; only a lane
    /// the driver steered into is theirs to answer for.
    ///
    /// A narrowing stretch with no work zone at all used to say nothing --
    /// `LaneKeeping::set_lane_count` clamps the lane index silently, so a
    /// driver in the soon-to-vanish lane was simply moved with no warning
    /// (Darren, 2026-08-14). That gets the same never-dropped treatment as
    /// the closure call above, just without "closed": the road narrowed, not
    /// a work zone. Skipped on the exit ramp, whose own single-lane count and
    /// its own announcements are a different situation entirely.
    pub fn leave_a_lane_the_road_closed(&mut self, ctx: &mut GameContext) {
        let count = self.lane.lane_count;
        let was = self.lane_count_seen;
        self.lane_count_seen = Some(count);
        let Some(was) = was else {
            return; // the road did not change under the truck
        };
        if was == count {
            return;
        }
        let closed = self.closed_lane_here();
        if let Some(closed) = closed {
            if self.lane.lane == closed {
                let Some(open_lane) = self.open_lane_beside(closed) else {
                    return;
                };
                let open_name = lane_label(open_lane, count);
                self.lane.recentre(open_lane);
                self.lane_change_target = None;
                self.merge_deadline = None;
                let volume = 1.0f64.min(0.7 * self.cue_loudness(ctx));
                ctx.audio.play_with("vehicle/lane_line_cross", volume, 0.0);
                let mut closed_name = lane_label(closed, count).to_string();
                closed_name[..1].make_ascii_uppercase();
                let text = format!("{closed_name} lane closed. In the {open_name} lane.");
                ctx.say_event_with(text, SayEvent::new().category(SpeechCategory::Safety));
                return;
            }
        }
        let before_lane = self.lane_before_narrow;
        if self.ramp_mi.is_none()
            && before_lane.is_some_and(|before| before >= count && before != self.lane.lane)
        {
            let volume = 1.0f64.min(0.7 * self.cue_loudness(ctx));
            ctx.audio.play_with("vehicle/lane_line_cross", volume, 0.0);
            // Naming the lane is only worth saying while there is another one
            // it could have been. Narrowing to one lane already said which
            // lane you are in by saying there is only one.
            let moved = if count <= 1 {
                String::new()
            } else {
                format!(" Now in the {} lane.", lane_label(self.lane.lane, count))
            };
            let text = format!("The road narrows to {}.{moved}", lane_count_words(count));
            ctx.say_event_with(text, SayEvent::new().category(SpeechCategory::Safety));
        }
    }

    /// Riding a coned-off lane: one urgent warning, then the barrels win.
    pub fn update_merge(&mut self, ctx: &mut GameContext, dt: f64) {
        let zone = self.trip.active_closure(None);
        let closed = self.closed_lane_here();
        let Some(closed) = closed else {
            self.merge_deadline = None;
            return;
        };
        if self.lane.lane != closed || self.trip.truck.speed_mph() < LANE_MIN_MPH {
            self.merge_deadline = None;
            return;
        }
        if self.lane.lane_count < 2 {
            // Nowhere to merge to: the road under this closure runs one lane
            // our side. Zone placement will not do this any more, but a saved
            // trip or a stretch that narrows mid-zone still can, and ordering
            // a driver into a lane that does not exist -- then charging them
            // for staying put -- is the worst thing this code could do.
            self.merge_deadline = None;
            return;
        }
        let Some(open_lane) = self.open_lane_beside(closed) else {
            self.merge_deadline = None;
            return;
        };
        let open_name = lane_label(open_lane, self.lane.lane_count);
        if let Some(zone) = zone.as_ref() {
            if zone.reason != "construction" {
                // Still in the taper: the lane is closing, not closed. Say so
                // once, and leave the barrel clock for the work zone itself.
                let taper_key = format!("{}:{:.2}", zone.reason, zone.start_mi);
                if self.merge_taper_warned.as_deref() != Some(taper_key.as_str()) {
                    self.merge_taper_warned = Some(taper_key);
                    ctx.audio.play("ui/warning");
                    let text = format!(
                        "The {} lane closes at the work zone ahead. Move to the {open_name} lane.",
                        lane_label(closed, self.lane.lane_count)
                    );
                    ctx.say_event_with(text, SayEvent::new().category(SpeechCategory::Safety));
                }
                return;
            }
        }
        if self.merge_deadline.is_none() {
            self.merge_deadline = Some(MERGE_WINDOW_S);
            ctx.audio.play("ui/warning");
            ctx.controller.rumble.alert();
            let text = format!(
                "You are in the closed {} lane! Move to the {open_name} lane!",
                lane_label(closed, self.lane.lane_count)
            );
            ctx.say_event_with(text, SayEvent::new().category(SpeechCategory::Safety));
            return;
        }
        if self
            .lane_change_target
            .is_some_and(|target| target != closed)
        {
            return; // already moving over: hold the countdown
        }
        let left = self.merge_deadline.unwrap_or(0.0) - dt;
        self.merge_deadline = Some(left);
        if left <= 0.0 {
            self.merge_deadline = None;
            self.lane.recentre(open_lane);
            self.lane_change_target = None;
            ctx.audio.play("vehicle/collision");
            ctx.controller.rumble.impact(MERGE_BARRELS_DAMAGE);
            self.trip.truck.apply_collision(MERGE_BARRELS_DAMAGE, true);
            let text = format!(
                "You plowed through the barrels and lurched into the {open_name} lane. The truck \
                 took damage, now {:.0} percent.",
                self.trip.truck.damage_pct
            );
            ctx.say_event_with(text, SayEvent::new().category(SpeechCategory::Safety));
            if let Some(zone) = zone.as_ref() {
                self.cite_barrel_strike(ctx, zone);
            }
        }
    }

    /// Charge the citation for knocking down work zone barrels.
    ///
    /// Charged where the chain-law citation is, and for the same reason:
    /// this is written on the spot, mid-drive, and asking a driver who has
    /// just taken a collision to also run a shoulder stop stacks two demands
    /// on one moment. The record entry is a serious violation, not just
    /// money -- the offense endangers the crew, not the truck.
    ///
    /// Charged once per work zone. The barrels can catch a truck several
    /// times over one closure (the tester's log has two strikes in eight
    /// seconds); that is one refusal to merge, so it is one citation. The
    /// damage still lands every time.
    pub fn cite_barrel_strike(&mut self, ctx: &mut GameContext, zone: &Zone) {
        // An open lane is what makes this the driver's fault. A truck with
        // nowhere to merge cannot be charged for staying where it is -- the
        // merge update returns long before here in that case, and this second
        // reading is deliberate: no future edit above may make this reachable.
        if self.lane.lane_count < 2 || self.enforcement_bypassed(ctx) {
            return;
        }
        let key = format!("barrels:{:.1}", zone.start_mi);
        if self.enforcement_events.contains(&key) {
            return;
        }
        self.enforcement_events.insert(key);
        if ctx.profile.is_none() {
            return;
        }
        // NOT doubled for the construction zone, unlike every other citation.
        // The zone multiplier exists because states double an ordinary moving
        // violation committed in roadwork. This offense only exists inside
        // roadwork, and its base is Missouri RSMo 304.585, which is already the
        // work-zone-specific penalty and caps a first offense at 1,000. Passing
        // construction_zone=True here would charge twice for the same
        // aggravating fact and put every first offense at double the statutory
        // maximum. Priors still escalate it.
        let fine = citation_fine(
            ff_core::models::enforcement::WORK_ZONE_BARRELS_FINE,
            career_citations(profile_of(ctx)),
            false,
            None,
        );
        let money = {
            let p = profile_mut_of(ctx);
            p.spend(fine);
            p.money()
        };
        self.ticket_fines_paid += fine;
        let saw_it = match self.trip.active_post_at(self.trip.position_mi) {
            Some(post) => format!("A trooper working this {} saw it", post.reason()),
            None => "The work crew called it in".to_string(),
        };
        let ladder = self.log_enforcement(
            ctx,
            fine,
            true,
            false,
            "Drove through the barrels in a work zone",
        );
        ctx.audio.play("ui/error");
        let tail = if ladder.is_empty() {
            String::new()
        } else {
            format!(" {ladder}")
        };
        ctx.say_event_with(
            // No doubled-for-the-zone clause: this citation is not doubled,
            // because its amount is already the roadwork penalty.
            format!(
                "{saw_it}. Driving through the barrels is a citation, {} dollars, and it goes on \
                 your safety record. You have {} dollars.{tail}",
                fmt_grouped(fine, 0),
                fmt_grouped(money, 0)
            ),
            // Money rides ROUTE's never-dropped contract: a busy stretch must
            // not age a citation out of the queue.
            SayEvent::queued()
                .priority(EventPriority::Important)
                .category(SpeechCategory::Money),
        );
        // A second serious violation just suspended the CDL: the drive ends
        // here instead of carrying on.
        self.end_drive_if_licence_pulled(ctx);
    }

    /// Left-lane time is legitimate while passing slower right-lane
    /// traffic, while construction has the right lane coned off, or while
    /// lane keeping's own move back is held or was just called off.
    pub fn keep_right_justified(&self) -> bool {
        if self.closed_lane_here() == Some(0) || self.keep_right_excused_s > 0.0 {
            return true;
        }
        let speed = self.trip.truck.speed_mph();
        self.trip
            .traffic_manager
            .vehicle_in_lane(
                self.trip.position_mi,
                0,
                PASSING_LOOKAHEAD_MI,
                0.05,
                0.0,
                0.0,
            )
            .is_some_and(|slower| slower.speed_mph < speed + 3.0)
    }

    /// Camping the left lane draws CB grumbling: keep right except to pass.
    pub fn update_keep_right(&mut self, ctx: &mut GameContext, dt: f64) {
        self.keep_right_excused_s = (self.keep_right_excused_s - dt).max(0.0);
        if self.lane.lane_count < 2
            || self.lane.lane != self.lane.lane_count - 1
            || self.trip.truck.speed_mph() < KEEP_RIGHT_MIN_MPH
            || self.ramp_mi.is_some()
        {
            self.left_lane_s = 0.0;
            self.keep_right_nags = 0;
            return;
        }
        if self.keep_right_justified() {
            self.left_lane_s = (self.left_lane_s - dt).max(0.0);
            return;
        }
        self.left_lane_s += dt;
        let threshold = KEEP_RIGHT_NAG_S + self.keep_right_nags as f64 * KEEP_RIGHT_REPEAT_S;
        if self.left_lane_s < threshold {
            return;
        }
        self.keep_right_nags += 1;
        if self.keep_right_nags == 1 {
            let grumble = "You have been riding the left lane a while. Keep right except to pass.";
            // Repeatable with Alt C like any other CB call. No post and no
            // distance in it, so the repeat says it back word for word.
            self.last_cb_chatter = Some(CbChatterRecall {
                text: grumble.to_string(),
                post: None,
            });
            self.speak_ambient_event(
                ctx,
                SpokenMessage::new(grumble),
                // Advice from the CB, not a rule the game enforces: a tip,
                // said once a leg at standard and not at all quieter.
                Ambient::new()
                    .sound(Some("events/cb_radio_chatter"))
                    .category(Some(SpeechCategory::Coaching)),
            );
        } else {
            ctx.audio.play_with("traffic/car_pass", 0.9, 0.5);
            self.speak_ambient_event(
                ctx,
                SpokenMessage::new("Traffic is stacking up and passing you on the right."),
                Ambient::new()
                    .sound(Some("events/cb_radio_chatter"))
                    .category(Some(SpeechCategory::Traffic)),
            );
        }
    }
}
