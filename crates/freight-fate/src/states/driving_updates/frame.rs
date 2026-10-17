//! The frame loop itself (`DrivingUpdateMixin.update`), the safety-call
//! re-speak, and the retarder transcript trace.

use ff_core::speech_pacing::SpeechCategory;

use crate::speech::EventPriority;

use crate::app::{GameContext, SayEvent, TRANSCRIPT_TARGET};
use crate::audio::CH_BRAKE;
use crate::bindings::Action;
use crate::states::base::Key;
use crate::states::driving::DrivingState;
use crate::states::driving_core::*;
use crate::states::driving_pacenotes::PACENOTE_MARGIN_MPH;
use crate::states::driving_updates::{live, PAD_EMERGENCY_BRAKE};

impl DrivingState {
    /// Re-speak a safety call the player silenced before it finished.
    ///
    /// Ctrl is a screen-reader reflex and must always silence instantly --
    /// but a curve call cut mid-sentence is information the road still
    /// owes the driver (owner's worry, 2026-07-20: "how you gonna get it
    /// spoken?"). If Ctrl landed inside the call's speaking window, the
    /// call re-arms once and speaks again with a REFRESHED distance --
    /// provided the bend is still ahead and the truck is still above its
    /// advisory. Passed it, or slowed for it: stay quiet.
    pub fn update_critical_respeak(&mut self, ctx: &mut GameContext, dt: f64) {
        if self.critical_curve.is_none() {
            return;
        }
        self.critical_call_age_s += dt;
        let Some(respeak_at) = self.critical_respeak_at else {
            if self.critical_call_age_s > CRITICAL_CALL_WINDOW_S {
                self.critical_curve = None; // spoke to the end, most likely
            }
            return;
        };
        if self.critical_call_age_s < respeak_at {
            return;
        }
        let curve = self.critical_curve.take().expect("checked above");
        self.critical_respeak_at = None;
        let ahead = curve.start_mi - self.trip.position_mi;
        let speed = self.trip.truck.speed_mph();
        let floor = (curve.advisory_mph as f64 + PACENOTE_MARGIN_MPH)
            .min(self.trip.bend_costs_above_mph(&curve));
        if ahead <= 0.0 || speed <= floor {
            return;
        }
        // The pacenote speaks; the chime that preceded it does not (owner,
        // 2026-09-18). The spoken call carries the side and the number, and
        // the engine's lean carries the shape.
        let text = self.pacenote_text(ctx, &curve, ahead, speed);
        let mut opts = SayEvent::new().category(SpeechCategory::Navigation);
        // The refreshed call checks the bend before speaking; the rescue
        // path speaks it again later, so it has to check too.
        if let Some(valid) = self.curve_call_still_true(Some(&curve)) {
            self.refresh_live_facts();
            opts = opts.valid(move || valid.holds());
        }
        ctx.say_event_with(text, opts);
    }

    /// Called from the Ctrl handler: arm the one-shot refreshed re-speak
    /// when the silence landed inside a safety call's speaking window.
    pub fn note_critical_speech_stopped(&mut self) {
        if self.critical_curve.is_some()
            && self.critical_respeak_at.is_none()
            && self.critical_call_age_s < CRITICAL_CALL_WINDOW_S
        {
            self.critical_respeak_at = Some(self.critical_call_age_s + CRITICAL_RESPEAK_DELAY_S);
        }
    }

    /// Write a transcript line whenever the retarder changes stage.
    ///
    /// The jake is audible and nothing else. It speaks no line of its own,
    /// so "did the engine brake just come up on level road?" -- an ordinary
    /// playtest question, asked mid-drive by the owner on 2026-08-20 -- had
    /// no answer anywhere: not in the transcript, not in the session log,
    /// not in the save. The only way to settle it was to rebuild the road
    /// on a bench and hope the conditions matched.
    ///
    /// One line per CHANGE, never per frame, carrying the road that
    /// explains it: a stage rising on a real downgrade is the retarder
    /// doing its job, and the same stage rising on the flat is the bug the
    /// question was about. Prefixed like the pacer and ladder lines so the
    /// transcript stays sortable into "what the driver heard" and "what the
    /// truck did".
    pub fn trace_engine_brake(&mut self) {
        let stage = self.trip.truck.engine_brake_stage;
        if stage == self.traced_jake_stage {
            return;
        }
        self.traced_jake_stage = stage;
        let grade_pct = self.trip.grade_at(self.trip.position_mi) * 100.0;
        log::info!(
            target: TRANSCRIPT_TARGET,
            "[jake] stage {} at mile {:.1}, {:.0} mph, grade {:+.1} percent",
            stage,
            self.trip.position_mi,
            self.trip.truck.speed_mph(),
            grade_pct,
        );
    }

    /// `update(dt)`: one frame of driving.
    pub fn update_frame(&mut self, ctx: &mut GameContext, dt: f64) {
        // The Python `State.update` this mixin overrode advanced the menu
        // music bed; a drive clears the rotation on entry, so the call is a
        // no-op here and stays only because every `State::update` owes it.
        ctx.update_music_rotation(dt);
        self.refresh_live_facts();
        self.trace_engine_brake();
        // The curve call prices a bend by who holds the lane in it.
        self.trip.lane_steers = Self::lane_steers(ctx);
        // A fresh loaded run out of a chain-capable origin starts on the
        // facility's streets. Decided on the first tick, never on a resume:
        // from_snapshot marks the check done and re-enters a chain itself.
        if !self.departure_checked {
            self.departure_checked = true;
            if !self.resumed {
                self.begin_departure_chain(ctx, true);
            }
        }
        // A pace change from the pause menu waits for the truck to stop.
        self.update_pace_change(ctx, dt);
        let tuning = tuning_for_time_scale(self.trip.time_scale);
        self.trip.hazard_scale =
            hos::hazard_scale(&ctx.settings.hos_mode) * tuning.hazard_frequency;
        self.trip.traffic_manager.hazard_scale = self.trip.hazard_scale;
        self.sync_radio_settings(ctx);
        // A new leg is a fresh road, so a once-per-leg tip earns one more
        // telling (Disposition.FIRST_OCCURRENCE).
        let leg = self.trip.current_leg_index() as i64;
        if leg != self.ladder_leg_index {
            self.ladder_leg_index = leg;
            ctx.reset_ladder_leg_memory();
        }
        if self.destination_exit_response_s > 0.0 {
            self.destination_exit_response_s = (self.destination_exit_response_s - dt).max(0.0);
            if self.destination_exit_response_s == 0.0 && self.exit_stop.is_none() {
                // A driver who stopped after the early callout must still get a
                // fresh, closer instruction once the normal window reaches them.
                self.destination_exit_announced_key = String::new();
            }
        }
        self.sync_weather_source(ctx);
        self.sync_weather_alerts(ctx);
        let ramp = dt * 2.2;
        self.brake_lockout_cue_timer = (self.brake_lockout_cue_timer - dt).max(0.0);
        // Controller triggers/clutch are analog held positions blended in below;
        // the keyboard keys keep their ramped behavior so both devices work.
        let pad_on = ctx.controller.active();
        let pad_throttle = if pad_on {
            ctx.controller.throttle()
        } else {
            0.0
        };
        let pad_brake = if pad_on { ctx.controller.brake() } else { 0.0 };
        let key_up = ctx.bindings.pressed(&ctx.input, Action::Accelerate);
        let mut key_down = ctx.bindings.pressed(&ctx.input, Action::Brake);
        let b_held = ctx.bindings.pressed(&ctx.input, Action::EmergencyBrake);
        // A latched brake reads as held right here, so everything
        // downstream -- the reverse gesture, cruise cancel, the hazard's
        // brake answer -- sees one truth. Microsleeps stay on the raw
        // keys: only a live reaction proves the driver awake.
        // The throttle key never latches: holding it is only for moving
        // and for the direction-change hold.
        key_down = self.update_pedal_latches(ctx, key_up, key_down, pad_throttle, dt);
        let accelerating = key_up || pad_throttle > 0.05;
        let hand_accelerating = accelerating;
        let braking_key = key_down || pad_brake > 0.05;
        // The shift gesture keys off a fresh press, so it reads the trigger's
        // instantaneous position rather than the smoothed accelerate/brake
        // values above -- otherwise the smoothing lag swallows a quick tap and
        // the release-then-press never registers as neutral in between.
        let accel_held = key_up
            || (if pad_on {
                ctx.controller.throttle_target()
            } else {
                0.0
            }) > 0.05;
        let brake_held = key_down
            || (if pad_on {
                ctx.controller.brake_target()
            } else {
                0.0
            }) > 0.05;
        let backing = self.update_reverse_controls(
            ctx,
            accelerating,
            braking_key,
            accel_held,
            brake_held,
            dt,
        );
        if accelerating && !backing && self.trip.truck.air_brakes_holding() {
            self.maybe_say_air_brake_lockout(ctx);
        } else if !self.trip.truck.air_brakes_holding() {
            // The lockout actually cleared (parking brake released, spring
            // brakes recovered) -- not merely the player's foot off the
            // pedal, which must NOT drop the key or the next press would
            // re-announce an unchanged reason. The next time it arms is a
            // fresh instance of the condition and gets its warning again,
            // even with identical wording (mirrors update_overrev's reset
            // of "engine_redline").
            ctx.reset_event_condition("air_brake_lockout");
        }
        if key_up && !backing && !self.trip.truck.transmission.in_reverse() {
            if self.trip.truck.engine_brake() {
                self.trip.truck.set_engine_brake(false);
                // And the AMT manager with it, or "Jake off" is a lie: auto
                // mode is allowed to sit at stage zero now, so an armed
                // manager left behind here would quietly bring the retarder
                // back a moment after the cab said it was off.
                self.auto_jake = false;
                // ROUTE, not the ambient default: an automation (the engine
                // brake) just released, and a driver who assumed it still held
                // needs to hear that (automation-handoff sweep, 2026-08-20,
                // the deferred 2026-08-15 audit).
                ctx.say_event_with(
                    "Jake off.",
                    SayEvent::queued()
                        .priority(EventPriority::Important)
                        .category(SpeechCategory::Confirmation),
                );
            }
            let t = &mut self.trip.truck;
            t.throttle = 1.0f64.min(t.throttle + ramp);
        } else if backing {
            let t = &mut self.trip.truck;
            t.throttle = 0.45f64.min(t.throttle + ramp);
        } else {
            let t = &mut self.trip.truck;
            t.throttle = 0.0f64.max(t.throttle - ramp * 2.0);
        }
        if pad_throttle > 0.05 && !backing && !self.trip.truck.transmission.in_reverse() {
            if self.trip.truck.engine_brake() {
                self.trip.truck.set_engine_brake(false);
                self.auto_jake = false; // see the keyboard branch above
                                        // ROUTE, not the ambient default: same as the keyboard branch
                                        // above (automation-handoff sweep, 2026-08-20, the deferred
                                        // 2026-08-15 audit).
                ctx.say_event_with(
                    "Jake off.",
                    SayEvent::queued()
                        .priority(EventPriority::Important)
                        .category(SpeechCategory::Confirmation),
                );
            }
            let t = &mut self.trip.truck;
            t.throttle = t.throttle.max(pad_throttle);
        }
        // Keyboard ramps the brake up and down; the analog trigger sets a direct
        // held floor on top of that.
        let braking_ramp =
            (key_down && !backing) || (accelerating && self.trip.truck.velocity_mps < -0.1);
        // The floor an assist is holding right now. The ramp decays the
        // pedal toward zero every frame the driver is not braking, and the
        // assists re-press their application further down the frame, so the
        // PHYSICS felt a steady pedal -- but the air model charges by the
        // RISE, and it saw 0.09 climb back to 0.20 sixty times a second. One
        // held snub was billed as six applications a second, which is the
        // Aberdeen stranding (2026-09-18) surviving its own fix one layer
        // down and stranding Albany Company Yard the same way (approach
        // sweep, 2026-09-20). A pedal an assist is holding is not a pedal
        // the driver let go of, so the decay stops there. Their own brake
        // key still cancels the assist, which is what drops the floor.
        //
        // The ramp terminal's servo was left out, and it is the one that
        // presses AFTER physics runs: the truck felt 0.09 of a 0.20 stop,
        // slowed at 0.43 m/s2 against the 0.6 planned, reached a stop bar at
        // 15 mph and had to slam the rest (agent drive into Abilene,
        // 2026-09-23). Only while it still has a terminal to stop at: a press
        // left over once the ramp is behind the truck held it on its brakes.
        let ramp_terminal_brake = if self.terminal_live() && !self.ramp_terminal_done {
            self.ramp_assist_brake
        } else {
            0.0
        };
        // The exit assists' deceleration-lane servo, by the same rule. It
        // stands itself down (to zero) the frame the lane is behind the truck.
        let decel_lane_brake = if self.in_deceleration_lane() {
            self.decel_lane_brake
        } else {
            0.0
        };
        let assist_floor = self
            .keeper_snub
            .max(self.aeb_brake)
            .max(self.destination_assist_brake)
            .max(if self.chain_stop.is_some() {
                self.chain_stop_brake
            } else {
                0.0
            })
            .max(ramp_terminal_brake)
            .max(decel_lane_brake)
            .max(self.curve_servo.as_ref().map_or(0.0, |servo| servo.brake))
            .clamp(0.0, 1.0);
        {
            let t = &mut self.trip.truck;
            if braking_ramp {
                t.brake = 1.0f64.min(t.brake + ramp * 1.5);
            } else {
                t.brake = assist_floor.max(t.brake - ramp * 3.0);
            }
            if pad_brake > 0.05 && !backing {
                t.brake = t.brake.max(pad_brake);
            }
        }
        let braking = braking_ramp || (pad_brake > 0.05 && !backing);
        // "not backing" matters more here than it looks: in automatic, holding
        // the left trigger from a stop is the gesture that shifts to reverse,
        // so without it every backing manoeuvre would slam the emergency
        // application on and flat-spot the tires for it.
        let emergency = b_held || (pad_brake >= PAD_EMERGENCY_BRAKE && !backing);
        // A real truck drops cruise at the first tap of the service brake.
        // Only the player's own pedal cancels here; the sim's automatic brake
        // ramps (reverse arrest, hazard events) go through their own cancels.
        if self.cruise_mph.is_some() && (braking_key || emergency) && !backing {
            self.cancel_cruise(ctx, false);
            // ROUTE, not the ambient default: the automation just released the
            // throttle (automation-handoff sweep, 2026-08-20, the deferred
            // 2026-08-15 audit).
            ctx.say_event_with(
                "Cruise off.",
                SayEvent::queued()
                    .priority(EventPriority::Important)
                    .category(SpeechCategory::Confirmation),
            );
        }
        // The same rule for facility stopping assistance driving the truck on
        // from a ramp terminal: the driver's own brake hands the pedals back.
        // Only while the entrance is still ahead -- at the point the assist
        // has already stopped the truck and the dock is opening.
        if self.approach_pull_ahead
            && (braking_key || emergency)
            && !backing
            && self.ramp_mi.is_some_and(|mi| mi > 0.0)
        {
            self.cancel_approach_pull_ahead(ctx);
        }
        // And for curve speed assistance's approach servo: the driver's own
        // brake takes the bend back for the rest of it.
        if self.curve_servo.is_some() && (braking_key || emergency) && !backing {
            self.cancel_curve_servo(ctx);
        }
        if emergency {
            // no ramp: slams to full application instantly, plus spring brakes
            if !self.trip.truck.emergency_brake && self.trip.truck.velocity_mps.abs() > 1.0 {
                if ctx.audio.has_asset("vehicle/ebrake") {
                    // The licensed cut: one big sustained air event.
                    ctx.audio.play_with("vehicle/ebrake", 0.9, 0.0);
                } else {
                    ctx.audio.play_with("vehicle/brake_air", 1.0, 0.0);
                }
            }
            let t = &mut self.trip.truck;
            t.throttle = 0.0;
            t.brake = 1.0;
        }
        self.trip.truck.emergency_brake = emergency;
        // Hard braking (emergency or heavy service) shudders the pad while it
        // lasts; the engine's TTL lets it lapse a few frames after we stop. Only
        // while moving *forward*: rolling backward, the sim ramps the service
        // brake to full on its own to arrest the reverse before shifting to
        // drive, and that must not read as a hard stop and buzz the whole time.
        if self.trip.truck.velocity_mps > 1.0 && (emergency || self.trip.truck.brake >= 0.85) {
            let level = if emergency {
                1.0
            } else {
                self.trip.truck.brake
            };
            ctx.controller.rumble.hard_brake(level);
        }
        // Brake sounds ride the application edges. A hysteresis flag (arm at
        // 0.05, release below 0.02) keeps a steady analog trigger -- or a held
        // key -- from retriggering frame after frame. The emergency brake
        // plays its own louder cue, so it only arms the flag.
        // PRESS: the mechanical clunk of the valve, leveled by press force
        // (locked spec 2026-07-21; the classic air chirp is the fallback).
        // RELEASE: the air bleeding back out -- the hiss bed held for a
        // length, and at a level, set by how hard the brakes were applied.
        // The release plays at any speed: braking to a halt then letting off
        // is exactly when a real rig gives its loudest pssht.
        if self.trip.truck.brake >= 0.05 {
            if !self.brake_air_hissed && !emergency && self.trip.truck.velocity_mps.abs() > 1.0 {
                let force = self.trip.truck.brake.clamp(0.0, 1.0);
                ctx.audio.play_bank_with(
                    "vehicle/brake_clunk",
                    "vehicle/brake_air",
                    0.35 + 0.35 * force,
                    0.0,
                );
            }
            self.brake_air_hissed = true;
            self.brake_peak_application = self.brake_peak_application.max(self.trip.truck.brake);
        } else if self.trip.truck.brake < 0.02 {
            let peak = self.brake_peak_application;
            // Locked spec levels (0.07-0.12 mix, "all quiet under the
            // engine"): a light release is a barely-there sigh below the
            // road bed, not a foreground pssht -- shipped 4x hot at first
            // and the owner heard every tap on a twisty descent. Feather
            // releases under the floor stay silent entirely.
            if self.brake_air_hissed
                && peak >= 0.15
                && !emergency
                && ctx.audio.has_asset("vehicle/brake_hiss_bed")
            {
                // Road noise masks the release at speed, exactly as in a real
                // cab: rolling releases fade toward inaudible while the big
                // pssht after braking to a stop keeps its full voice.
                let masking = 0.25f64.max(1.0 - self.trip.truck.velocity_mps.abs() / 20.0);
                ctx.audio.start_loop_with(
                    CH_BRAKE,
                    "vehicle/brake_hiss_bed",
                    (0.10 + 0.15 * peak) * masking,
                    0,
                );
                ctx.audio
                    .stop_loop_with(CH_BRAKE, (160.0 + 800.0 * peak) as u32);
            }
            self.brake_air_hissed = false;
            self.brake_peak_application = 0.0;
        }
        let desired_automatic = ctx.settings.automatic_transmission;
        if self.trip.truck.transmission.automatic != desired_automatic {
            self.trip.truck.transmission.automatic = desired_automatic;
            let mode = if desired_automatic {
                "automatic"
            } else {
                "manual"
            };
            ctx.say_event_with(
                format!("Transmission changed to {mode}."),
                SayEvent::new().category(SpeechCategory::Confirmation),
            );
        }
        if self.trip.truck.transmission.automatic && self.trip.truck.speed_mph().abs() > 1.0 {
            self.drove_automatic = true;
        }

        // The clutch is fixed on Shift, read as the key itself: the held-key
        // tracker sees the key before any modifier flag, and the harness holds
        // it the same way.
        let clutch_pressed = ctx.input.is_pressed(Key::LShift) || ctx.input.is_pressed(Key::RShift);
        let mut clutch_val: f64 = if clutch_pressed { 1.0 } else { 0.0 };
        if pad_on {
            clutch_val = clutch_val.max(ctx.controller.clutch());
        }
        self.trip.truck.transmission.clutch = if !self.trip.truck.transmission.automatic {
            clutch_val
        } else {
            0.0
        };
        // Neutral on a manual is a disengaged clutch too: nothing reaches the
        // wheels, so cruise and the keeper only raced the engine.
        let clutch_disengaged = self.trip.truck.transmission.clutch > 0.5
            || self.trip.truck.transmission.shifting()
            || (!self.trip.truck.transmission.automatic
                && self.trip.truck.transmission.in_neutral());
        self.update_lane(ctx, dt);
        // `update_lane` can complete a held-wheel crossing. Mirror that
        // discrete result before cruise reads the traffic bubble: steering
        // alone remains in the origin lane, while the crossing frame selects
        // traffic in the lane the truck has actually entered. Tap changes
        // retain their explicit destination intent below.
        self.trip.traffic_manager.player_lane = self.lane.lane;
        self.trip.traffic_manager.player_lane_target = self.lane_change_target;
        self.update_exit_preparation(ctx, dt);
        self.resume_speed_control_if_ready(ctx, braking);
        self.apply_chain_speed_cap(ctx);
        self.update_cruise(ctx, dt, braking, hand_accelerating, clutch_disengaged);
        self.update_keeper(ctx, dt, braking, hand_accelerating, clutch_disengaged);
        // The keeper's held snub, for the same reason and by the same rule:
        // `update_keeper` returns early on an overridden or shifting frame,
        // and the input ramp above has already bled the pedal it left behind
        // -- so without this the one application is re-made, and re-charged,
        // every time round. See apply_keeper_snub.
        self.apply_keeper_snub();
        // The hazard assist's held application belongs here with the other
        // assists' floors, ahead of the physics -- see apply_hazard_brake.
        // update_hazard, which decides it, runs at the end of the frame.
        self.apply_hazard_brake();
        // The destination arrival's pedals, for the same reason, and it is
        // the reason that cost three reports: this ran AFTER the physics
        // step, so the brake it set was never integrated -- the keyboard ramp
        // at the top of the next frame decayed it to nothing before
        // t.update() ran, and the physics saw 0.00 the whole way to the gate
        // while the assist's own bookkeeping said 0.40. Every earlier fix to
        // this assist tuned a number against a pedal the truck never felt.
        self.update_destination_approach_assist(ctx);
        // Facility stopping assistance's roadside chain stops, for the same
        // reason: the brake it holds has to reach the physics.
        self.update_chain_stop_assist(ctx, braking_key || emergency || hand_accelerating);
        // Curve assistance's approach servo, for the same reason: its
        // pedal has to be set where the physics will see it, and after cruise
        // and the keeper so the max it applies is the one that stands.
        self.update_curve_speed_servo(ctx);
        self.update_horn_protection(ctx);

        self.update_auto_jake(ctx, dt);
        self.track_driving_badges(ctx, dt);
        if self.trip.truck.transmission.automatic && self.trip.truck.engine_on {
            let new_gear = self.trip.truck.auto_shift();
            if new_gear.is_some() {
                ctx.audio
                    .play_bank_with("vehicle/shift_auto", "vehicle/gear_shift", 0.65, 0.0);
            }
        }

        let was_on = self.trip.truck.engine_on;
        let was_air_ready = self.trip.truck.air_ready();
        let was_low_air = self.trip.truck.air_low_warning();
        let was_spring_brake = self.trip.truck.spring_brakes_active();
        self.trip.truck.update(dt);
        self.update_air_brake_announcements(
            ctx,
            was_on,
            was_air_ready,
            was_low_air,
            was_spring_brake,
        );
        self.check_low_fuel_warning(ctx);
        self.check_fuel_range_warning(ctx);
        if was_on && !self.trip.truck.engine_on {
            ctx.audio.engine_stop();
            if self.trip.truck.stalled {
                let tr = &self.trip.truck.transmission;
                // A manual only stalls in fourth or taller, and restarting it
                // there with the clutch out stalls it again the next frame.
                // Shifting up from there only goes taller, so the line routes
                // through neutral, which needs no clutch, then up into first.
                // The pad has no neutral: it steps down to first on the
                // clutch instead (2026-09-28).
                let text = if tr.automatic || tr.in_neutral() {
                    format!(
                        "Engine stalled. Press {} to restart.",
                        ctx.control_hint("engine")
                    )
                } else if ctx.controller.device() == ff_core::input_hints::CONTROLLER {
                    format!(
                        "Engine stalled. Hold {}, press {} to restart, and shift down to first \
                         with {} before letting the clutch out.",
                        ctx.control_hint("clutch"),
                        ctx.control_hint("engine"),
                        ctx.control_name(Action::ShiftDown)
                    )
                } else {
                    format!(
                        "Engine stalled. Press {} for neutral and {} to restart, then hold {} \
                         and press {} for first gear.",
                        ctx.control_hint("neutral"),
                        ctx.control_hint("engine"),
                        ctx.control_hint("clutch"),
                        ctx.control_hint("gear_first")
                    )
                };
                ctx.say_event_with(text, SayEvent::new().category(SpeechCategory::Safety));
            } else if self.trip.truck.fuel_gal <= 0.0 {
                self.handle_out_of_fuel(ctx);
            }
        }

        // Keep the trip's spoken-distance units in step with a live settings
        // change; the setter only re-renders cues when the choice actually flips.
        self.trip.set_imperial(ctx.settings.imperial_units);
        let pos_before = self.trip.position_mi;
        // Tell the trip model which stop's exit is signaled or on the ramp so its
        // plan-cancelled warning can tell a driver who is taking the exit from one
        // who blew past it. Set before trip.update (which runs check_stops) and
        // before update_exit (which clears exit_stop on a miss), so on the exact
        // crossing tick the flag still reflects the armed exit.
        let active_exit = self.ramp_stop.clone().or_else(|| self.exit_stop.clone());
        self.trip.exit_in_progress = active_exit.as_ref().map(|stop| stop.key());
        // On the ramp the highway odometer holds and the ramp consumes the
        // movement instead; the trip records how far the truck rolled either way.
        self.trip.on_ramp = self.ramp_mi.is_some();
        // The yard is announced when the truck has turned into it, not when
        // it rolls over the driveway's milepost on its way past the turn.
        self.trip.yard_entry_held = self.driveway_turn_pending();
        for event in self.trip.update(dt) {
            self.handle_trip_event(ctx, &event);
        }
        if self.selected_stop_key.is_some()
            && self.trip.planned_stop_key != self.selected_stop_key
            && self.ramp_stop.is_none()
            && self.stop_chain.is_none()
        {
            // The trip model canceled a passed plan. Do not leave explicit
            // intent or its stopping assist armed for a later optional exit.
            self.clear_selected_stop_intent();
        }
        // Real seconds, before the crossing is judged against them. Only
        // seconds the cab was free: a hazard or a stop after the reminder
        // spends time the driver could not use to take the exit.
        if !self.enforcement_busy() {
            self.weigh_station_reminder_age_s += dt;
        }
        if self
            .trip
            .scale_reminder_hold_mi
            .is_some_and(|mi| self.trip.position_mi >= mi)
        {
            // Past the gore even if a busy cab kept the scale check from
            // running this frame: the real-time hold ends at the scale.
            self.trip.scale_reminder_hold_mi = None;
        }
        self.check_weigh_station_enforcement(ctx, pos_before);
        self.update_scale_reannounce(ctx);
        self.check_unsafe_damage_enforcement(ctx);
        self.check_destination_exit(ctx);
        self.check_gate_approach_warning(ctx, dt);
        self.update_turn_commitment(ctx, dt);
        let moved_mi = self.trip.last_moved_mi;
        // Advance and announce an already-active signal before adjudicating
        // this frame's stop-bar crossing. Otherwise a yellow-to-red boundary
        // uses the previous phase even though the real-time clock has moved.
        let ramp_was_active = self.ramp_mi.is_some();
        self.update_ramp_light(ctx, dt);
        self.update_exit_with_input(ctx, moved_mi, dt, hand_accelerating);
        if !ramp_was_active && self.ramp_mi.is_some() {
            // An exit can create its terminal above. Announce its seeded
            // starting phase now without charging this frame's time twice.
            self.update_ramp_light(ctx, 0.0);
        }
        self.update_departure_ramp(ctx, moved_mi);
        self.bound_departure_merge_recovery(moved_mi, dt);
        // Every clock pin is settled for the frame by here.
        self.trace_clock_override(dt);
        // Immediately after the exit watch, which is what turns a signaled
        // scale exit into a ramp. Only now can a scale crossing be told apart
        // from a check-in.
        self.resolve_weigh_station_bypass(ctx);
        // Reads the same last_moved_mi the exit watch just used, so the
        // distance it counts back is the distance the trip actually lost.
        self.update_wrong_way(ctx, dt);
        // After the trip has moved the truck and stepped the bubble, so the
        // voices sit where the vehicles are this frame.
        self.update_traffic_sounds(ctx, dt);
        // Right after the voices, and for the same reason: the lane the driver
        // moved out of is only open once the bubble has been stepped.
        self.update_lane_gap(ctx, dt);

        self.update_hours_and_fatigue(ctx, dt);
        self.update_audio(ctx, dt);
        self.update_announcements(ctx, dt);
        self.update_ambient_events(ctx, dt);
        self.update_critical_respeak(ctx, dt);
        self.update_hazard(ctx, dt);
        self.update_grade_advisory(ctx);
        self.update_microsleep(ctx, dt);
        // Damage bands run before the over-rev warning, so a redline call in
        // the same frame already names the band the truck just entered.
        self.update_damage_bands(ctx, dt);
        self.update_cargo_condition(ctx, dt);
        // After the cargo pass, so the bend's advisory is already on the truck
        // for the lateral wave. Returns immediately on any non-tank load.
        self.update_liquid_cues(ctx, dt);
        self.update_overrev(ctx, dt);
        // The watch runs first on purpose. If an officer opens a pull-over
        // this frame, update_speeding returns early on the live stop, so one
        // instance of speeding can never be charged twice -- once as a ticket
        // and again as a silent at-delivery strike.
        self.update_enforcement_watch(ctx, dt);
        self.update_speeding(ctx, dt, accel_held);
        self.update_engine_brake_zone(ctx, dt);
        self.update_pull_over(ctx, dt, braking || emergency);
        self.update_brake_heat_cue(ctx, dt);
        self.update_traction_cues(ctx);
        self.update_chain_reminder(ctx);
        self.update_chain_law(ctx);
        self.update_chain_checkpoint_hold(ctx);
        self.update_chain_carry(ctx);
        // The destination approach assist used to run here -- after the
        // physics step, which is why its brake never reached the truck. It
        // now runs with the other assists' pedal floors, ahead of t.update().
        if let Some(tutorial) = self.tutorial.as_mut() {
            tutorial.update(ctx, dt, &self.trip.truck);
        }
        if self.trip.finished {
            self.gate_reminder_s = (self.gate_reminder_s - dt).max(0.0);
            if self.departure_chain {
                // End of the origin's streets: merge onto the highway trip.
                self.finish_departure_chain(ctx);
            } else if self.stop_chain.is_some() {
                // At a road stop's lot, off its own streets.
                self.handle_stop_chain_end(ctx);
            } else if self.phase == DRIVE_PHASE_PICKUP {
                self.handle_pickup_gate(ctx);
            } else if self.ramp_mi.is_some() {
                // The ramp watch owns the arrival, so a trip that finishes
                // under the truck here is not the gate (Python `return`).
            } else if !self.destination_exit_taken {
                self.handle_missed_destination_exit(ctx);
            } else {
                self.handle_arrival_gate(ctx);
            }
        }
    }

    /// Mirror the readings a `say_event(valid=...)` gate needs (see
    /// [`crate::states::driving_updates::live`]).
    ///
    /// Public because a test that moves the truck by hand has to stand in for
    /// the frame that would otherwise have carried the reading across.
    pub fn refresh_live_facts(&self) {
        live::set_overspeed_active(self.overspeed_active);
        live::set_pull_over_active(self.trip.pull_over_active);
        live::set_damage_pct(self.trip.truck.damage_pct);
        live::set_position_mi(self.trip.position_mi);
        live::set_speed_mph(self.trip.truck.speed_mph());
        live::set_hazard_active(self.hazard_deadline.is_some());
        live::set_move_lockout(if !self.trip.truck.engine_on {
            live::LOCKOUT_ENGINE_OFF
        } else if !self.trip.truck.air_ready() {
            live::LOCKOUT_AIR
        } else if self.trip.truck.parking_brake {
            live::LOCKOUT_PARKING_BRAKE
        } else {
            0
        });
        live::set_arrival_menu_open(self.arrival_menu_open);
        live::set_gate_stop_prompted(self.arrival_full_stop_said);
        live::set_on_ramp(self.ramp_mi.is_some());
        live::set_posted_limit_mph(match self.trip.entered_zone.as_ref() {
            Some(zone) => zone.limit_mph,
            None => self.trip.corridor_limit_at(self.trip.position_mi),
        });
        live::set_ramp_holding(
            self.ramp_mi.is_some()
                && !self.ramp_terminal_done
                && (self.ramp_waiting_at_sign || self.ramp_waiting_at_light),
        );
    }
}
