//! The cab's continuous voice: the engine bed and its shift duck, road
//! joints, the reverse beeper, the air-fill loop, the jake growl, the
//! weather bed, and automatic retarder management.

use ff_core::engine_audio::{classify, EngineReading};
use ff_core::sim::vehicle::{
    TruckState, DRIVE_AXLE_LOAD_FRACTION, JAKE_LOCK_MARGIN, JAKE_RPM_FLOOR,
};

use crate::app::GameContext;
use crate::audio::{CH_AIR, CH_JAKE};
use crate::states::driving::DrivingState;
use crate::states::driving_core::*;
use crate::states::driving_updates::{
    shift_recovery_curve, AIR_FILL_REARM_PSI, AIR_FILL_VOLUME, AUTO_JAKE_OVER_MPH,
    AUTO_JAKE_RELEASE_MPH, AUTO_JAKE_REVERSE_S, AUTO_JAKE_STEP_S, AUTO_JAKE_UNDER_MPH,
    ENGINE_LOAD_SMOOTH_S, JAKE_CUE_HOLD_S, JAKE_LOOP_RPMS, JAKE_MIN_RPM, JAKE_RATE_MAX,
    JAKE_RATE_MIN, JAKE_STAGE_GAIN, JAKE_VOICE_NATIVE_RPM, SHIFT_DISENGAGE_DUCK,
    SHIFT_END_CLUNK_VOLUME, SHIFT_LOAD_CAP, SHIFT_LOAD_RECOVERY_S,
};

impl DrivingState {
    /// Wheel force the jake asks for at `stage`, right now.
    ///
    /// `ff_core` keeps `TruckState::jake_force_demand` and
    /// `jake_traction_cap` private, so the two readings the Python method
    /// got by poking `engine_brake_stage` and calling them are recomputed
    /// here from the same public inputs and the same published constants.
    fn jake_demand_at_stage(truck: &TruckState, stage: i32) -> f64 {
        if stage <= 0
            || !truck.engine_on
            || truck.throttle > 0.05
            || truck.transmission.in_neutral()
        {
            return 0.0;
        }
        let ratio = truck.transmission.drive_ratio().abs();
        if ratio == 0.0 || truck.velocity_mps.abs() <= 0.01 {
            return 0.0;
        }
        let specs = &truck.specs;
        let rpm_frac = (truck.rpm / specs.max_rpm).clamp(0.0, 1.0);
        let stage_frac = JAKE_STAGES.min(stage) as f64 / JAKE_STAGES as f64;
        let torque = specs.engine_brake_torque_nm
            * stage_frac
            * (JAKE_RPM_FLOOR + (1.0 - JAKE_RPM_FLOOR) * rpm_frac);
        torque * ratio * specs.driveline_efficiency / specs.wheel_radius_m
    }

    /// The highest stage the drive axle can hold right now (0..3).
    ///
    /// Per-stage retard scales linearly with cylinders, so the cap divides
    /// straight through the full-stage demand -- the same traction physics
    /// the pre-select gate uses, applied to stage selection.
    pub fn auto_jake_max_stage(&self) -> i32 {
        let truck = &self.trip.truck;
        let full_demand = Self::jake_demand_at_stage(truck, JAKE_STAGES);
        if full_demand <= 0.0 {
            return JAKE_STAGES;
        }
        let cap = truck.gross_mass_kg()
            * G
            * DRIVE_AXLE_LOAD_FRACTION
            * JAKE_LOCK_MARGIN
            * truck.effective_grip();
        // Python's int() truncates toward zero, as `as i32` does.
        ((JAKE_STAGES as f64 * cap / full_demand) as i32).clamp(0, JAKE_STAGES)
    }

    /// AMT retarder management: hold the target by stepping the stage.
    ///
    /// A retarder is a device for holding a loaded truck BACK. Auto mode
    /// therefore reaches for it when the truck is running over the number it
    /// is holding and gives it back when it is not -- it is not a switch that
    /// sits at two cylinders for the rest of the drive.
    pub fn update_auto_jake(&mut self, _ctx: &mut GameContext, dt: f64) {
        // Deliberately NOT gated on the stalk being up. Auto mode is allowed
        // to put the retarder all the way down (see the release below), and a
        // manager that stopped running the moment it did so could never bring
        // it back. J and Alt+J are what end auto mode; the stage is not.
        if !(self.auto_jake && self.trip.truck.transmission.automatic && self.trip.truck.engine_on)
        {
            return;
        }
        if self.trip.truck.throttle > 0.05 {
            return; // a throttle blip cuts the retarder; hold the stage for the return
        }
        let mut target = self
            .auto_jake_hold_mph
            .unwrap_or_else(|| self.trip.truck.speed_mph());
        // An engaged speed authority owns the number. Auto mode working to
        // whatever the driver happened to be doing when they armed it, while
        // cruise or the keeper works to a different one, is two controllers
        // with two answers: armed at 45 and then cruise set to 62, the
        // retarder sat at full stage on level road barking against a number
        // cruise had never heard of (jake sweep, 2026-08-24).
        //
        // The SET speed, not the working one. Cruise easing up to its number
        // after a resume, and cruise capping itself for a bend, a ramp, a
        // lower limit or a lead, are cruise doing its job on the drums -- a
        // target speed to arrive at, which is not an overspeed for a retarder
        // to snub. Descent control's ceiling reaches auto mode through this
        // same line, which is what the old `descent_control_active` branch
        // said in the one case it covered.
        //
        // Descent control's number, though, is not a target to arrive at: it
        // IS the grade, held. With cruise set at 85 above a 7 percent grade
        // the manager worked to the 85 and never raised a stage while cruise
        // held the truck at 68 on the drums alone (bench of the owner's run
        // into Denver, 2026-09-24). So it works to what descent control
        // holds -- the set speed under the posted cap and the hill's safe
        // descent speed -- which is the set speed itself on open road.
        if let Some(keeper) = self.keeper_mph {
            target = keeper;
        } else if self.cruise_mph.is_some() {
            target = self.descent_hold_mph();
        }
        self.auto_jake_cooldown_s = (self.auto_jake_cooldown_s - dt).max(0.0);
        self.auto_jake_reverse_s = (self.auto_jake_reverse_s - dt).max(0.0);
        let max_stage = self.auto_jake_max_stage();
        let stage = self.trip.truck.engine_brake_stage;
        let mut desired = stage;
        // A release goes through at once; a raise waits out the quiet time,
        // because the jake is loud and a rolling road would otherwise make it
        // chatter. The same asymmetry adaptive cruise draws, and for the same
        // reason: holding retard the truck no longer needs is what drags it
        // under the speed the controller is supposed to be keeping.
        let mut at_once = false;
        // Over the top of a gear descent control holds counts as over the
        // number, as it does for adaptive cruise's own staging.
        let err = self.trip.truck.speed_mph() - target;
        let err = self
            .held_gear_overspeed_mph()
            .map_or(err, |past_top| err.max(past_top));
        if self.on_climb() || (!self.on_downgrade() && err <= AUTO_JAKE_RELEASE_MPH) {
            // Two cases, one answer: the road is CLIMBING, where a hill
            // takes the speed off by itself and overspeed is the hill's to
            // eat; or the road is level and the truck is not running over its
            // number, so there is nothing for a retarder to do. Either way it
            // comes all the way off rather than walking down a stage at a
            // time. A genuine overspeed on LEVEL road still gets snubbed --
            // that is what the driver armed the manager for.
            //
            // The floor used to be stage one, with no reason written anywhere
            // for it, so auto mode kept two cylinders cut for the whole drive.
            // Every time cruise lifted off the fuel on level road the retarder
            // spoke, and on a climb it barked at a truck that was already
            // losing speed to the hill -- the owner's "on uphill ascents the
            // truck should gain speed instead of using the engine brakes"
            // (2026-08-24). On a real downgrade the stage stays: there the
            // retarder IS what is keeping the number, and dropping it puts the
            // whole hill onto the drums.
            desired = 0;
            at_once = true;
        } else if self.descent_safe_mph.is_some()
            && (self.retarder_warranted() || (err > AUTO_JAKE_OVER_MPH && self.on_downgrade()))
        {
            // A hill steep enough to have a safe descent speed: full retard,
            // set once at the top the way a driver sets it, and the drums
            // snub the rest. Walking it up a stage at a time let a loaded
            // truck run from 44 to 62 onto a seven percent pitch (bench of
            // the owner's run into Denver, 2026-09-24). Over that number on
            // the easier downgrade above the pitch, too: the speed comes off
            // on the engine brake, not the drums, as adaptive cruise takes it.
            desired = JAKE_STAGES;
            at_once = true;
        } else if err > AUTO_JAKE_OVER_MPH {
            desired = stage + 1;
        } else if err < -AUTO_JAKE_UNDER_MPH && self.descent_safe_mph.is_none() {
            // Held through the easier stretch between two steep pitches,
            // for the same reason adaptive cruise holds it there.
            desired = stage - 1;
        }
        let ceiling = if max_stage >= 1 { max_stage } else { 1 };
        desired = desired.min(ceiling).clamp(0, JAKE_STAGES);
        // A step back the other way waits out the longer reversal time, so
        // the manager cannot hunt between two stages: the agent's drive down
        // a 2.4 percent grade heard 1, 2, 3, 2, 1, 2, 3 inside half a minute
        // (2026-09-24), and the growl changing is all a driver hears of it.
        let step = (desired - stage).signum();
        let reversing =
            step != 0 && self.auto_jake_last_step != 0 && step != self.auto_jake_last_step;
        let held_for_reversal = reversing && !at_once && self.auto_jake_reverse_s > 0.0;
        if desired != stage && !held_for_reversal && (at_once || self.auto_jake_cooldown_s <= 0.0) {
            self.trip.truck.engine_brake_stage = desired;
            self.auto_jake_cooldown_s = AUTO_JAKE_STEP_S;
            self.auto_jake_reverse_s = AUTO_JAKE_REVERSE_S;
            self.auto_jake_last_step = if at_once { 0 } else { step };
        } else if stage > max_stage && max_stage >= 1 && self.auto_jake_cooldown_s <= 0.0 {
            // Traction shrank under the current stage (ice arrived): step
            // down immediately rather than grinding the drives loose.
            self.trip.truck.engine_brake_stage = max_stage;
            self.auto_jake_cooldown_s = AUTO_JAKE_STEP_S;
        }
    }

    /// `_update_audio(dt=0.0)`: the whole continuous soundscape for a frame.
    /// A zero-length update is an immediate sync (menus, tests).
    pub fn update_audio(&mut self, ctx: &mut GameContext, dt: f64) {
        self.sync_radio_power(ctx);
        self.update_cab_audio(ctx, dt);
        // The engine's pan is the STEERING GUIDE now, owned by
        // `update_lane_guidance_audio`, and is deliberately not touched here.
        // It used to carry the truck's lane position; that readout moved to
        // the road bed when the owner ruled the guide onto the engine
        // (2026-09-18), because two meanings on one channel is one too many.
        if self.trip.truck.engine_on && !ctx.audio.engine_running() {
            // Catch-up sync (resuming a running-engine trip, returning from a
            // menu): bring the loop up without replaying the ignition crank.
            ctx.audio.engine_start_with(false);
        } else if !self.trip.truck.engine_on && ctx.audio.engine_running() {
            // The mirror sync: the engine went off outside this frame loop
            // (a rest-menu shutdown), so drop the loop without a second
            // shutdown clunk. Without this the loop plays on with the engine
            // off -- inaudible under the old RPM-weighted band volumes, but
            // plainly audible with the constant-volume BASS engine loop.
            ctx.audio.engine_stop_with(false);
        }
        // A shift briefly unloads the engine, but the old 0.08 clamp cut loop
        // gain by roughly forty percent and made repeated shifts sound like the
        // engine was ducking or nearly dropping out. Cap the load to a
        // perceptible torque easing while shifting, then -- once the shift ends
        // -- ease the cap back to full over SHIFT_LOAD_RECOVERY_S along the
        // recovery curve, so the return "under load" is a shaped glide rather
        // than a single-frame snap.
        // A real shift is kachunk -- sigh -- kachunk: never a LOADED
        // glissando sliding through the change (the meow), but never a
        // frozen hang either (the owner's 2026-07-24 catch: the voice used
        // to hold the pre-shift rpm for the whole interrupt, then cliff).
        // Automatic: the voice follows the live physics rpm, which eases
        // unloaded toward the new gear's road speed -- ducked to 0.35 the
        // whole way, it reads as the real between-gears fall -- and the
        // engagement plays its own soft clunk as the load swells back.
        // Manual: the player owns the revs while the clutch is out (blips
        // and rev-matching stay audible, and the physics already sinks
        // toward idle), so only the load ducks -- the engine falls back
        // unloaded and swells back in when the clutch hooks up.
        let cap;
        let duck;
        let automatic = self.trip.truck.transmission.automatic;
        let shifting = self.trip.truck.transmission.shifting();
        let manual_clutch_out = !automatic && self.trip.truck.transmission.clutch > 0.5;
        if (automatic && shifting) || manual_clutch_out {
            self.shift_recover_t = 0.0;
            cap = SHIFT_LOAD_CAP;
            duck = SHIFT_DISENGAGE_DUCK;
            if automatic {
                // Marker only: an auto shift is in flight. The voice follows
                // the live physics rpm, which already sighs down toward the
                // new gear's road speed through the interrupt (vehicle
                // _update_rpm) -- ducked and unloaded, it reads as the real
                // between-gears fall, not the old frozen hang (owner,
                // 2026-07-24).
                self.shift_hold_rpm = Some(self.trip.truck.rpm);
            }
        } else if self.shift_recover_t < 1.0 {
            let step = if SHIFT_LOAD_RECOVERY_S > 0.0 {
                dt / SHIFT_LOAD_RECOVERY_S
            } else {
                1.0
            };
            self.shift_recover_t = 1.0f64.min(self.shift_recover_t + step);
            let recovered = shift_recovery_curve(self.shift_recover_t);
            cap = SHIFT_LOAD_CAP + (1.0 - SHIFT_LOAD_CAP) * recovered;
            duck = SHIFT_DISENGAGE_DUCK + (1.0 - SHIFT_DISENGAGE_DUCK) * recovered;
            if self.shift_hold_rpm.is_some() {
                // Engagement: the gear takes. The interrupt's clunk played a
                // second ago at shift START, so without this the actual
                // moment the truck picks the load back up was silent.
                ctx.audio.play_bank_with(
                    "vehicle/shift_auto",
                    "vehicle/gear_shift",
                    SHIFT_END_CLUNK_VOLUME,
                    0.0,
                );
                self.shift_hold_rpm = None;
            }
        } else {
            cap = 1.0;
            duck = 1.0;
            self.shift_hold_rpm = None;
        }
        ctx.audio.set_engine_duck(duck);
        // A manual shift is kachunk -- sigh -- kachunk too. The lever played
        // at the request; this is the gear taking, the moment the clutch is
        // back in and the lever time is up. Owed only while a gear is still
        // selected: a shift taken to neutral engages nothing.
        if !automatic && self.manual_engage_clunk_pending {
            let tr = &self.trip.truck.transmission;
            if tr.in_neutral() {
                self.manual_engage_clunk_pending = false;
            } else if !tr.shifting() && tr.clutch <= 0.5 {
                ctx.audio.play_bank_with(
                    "vehicle/shift_manual",
                    "vehicle/gear_shift",
                    SHIFT_END_CLUNK_VOLUME,
                    0.0,
                );
                self.manual_engage_clunk_pending = false;
            }
        }
        let target_load = self.trip.truck.throttle.clamp(0.0, 1.0);
        if dt <= 0.0 {
            // Direct callers and tests use a zero-length update to request an
            // immediate audio sync.
            self.engine_audio_throttle = target_load;
        } else {
            let blend = 1.0f64.min(dt / ENGINE_LOAD_SMOOTH_S);
            self.engine_audio_throttle += (target_load - self.engine_audio_throttle) * blend;
        }
        let engine_load = self.engine_audio_throttle.min(cap);
        ctx.audio
            .set_engine_rpm_with(self.trip.truck.rpm, engine_load);
        ctx.audio.set_road_noise(self.trip.truck.velocity_mps);

        // Road texture follows real wheel travel, not the trip model's compressed
        // route distance. Ramps are outside the highway soundscape.
        if dt > 0.0 && self.trip.truck.velocity_mps > 5.0 && !self.trip.on_ramp {
            self.road_joint_accumulator_m += self.trip.truck.velocity_mps * dt;
            if self.road_joint_accumulator_m >= self.next_joint_distance_m {
                self.road_joint_accumulator_m %= self.next_joint_distance_m;
                self.next_joint_distance_m = self.road_texture_rng.uniform(14.0, 18.0);

                let severity = 1.0f64.min(self.trip.truck.velocity_mps / 30.0);
                ctx.audio
                    .play_with("vehicle/road_joint", 0.015 * severity, 0.0);
                ctx.controller.rumble.joint(severity);
            }
        }

        if self.trip.truck.engine_on && self.trip.truck.transmission.in_reverse() {
            if !self.reverse_cue_active {
                ctx.audio.reverse_start();
                self.reverse_cue_active = true;
            }
        } else if self.reverse_cue_active {
            ctx.audio.reverse_stop();
            self.reverse_cue_active = false;
        }
        // Air-fill overlay: the compressor charging the tanks below governor
        // release, whatever idle or drive state plays over it. Ends -- with the
        // fast idle settling -- at the park_idle -> ready_idle flip. Hysteresis
        // (AIR_FILL_REARM_PSI) keeps routine brake dips just under the 100 psi
        // line from fluttering the hiss; a genuine low-air build still plays.
        let voice = classify(&Self::engine_reading(&self.trip.truck));
        let deep_fill = self.trip.truck.air_pressure_psi()
            <= self.trip.truck.specs.air_parking_release_psi - AIR_FILL_REARM_PSI;
        if self.trip.truck.engine_on && voice.pressurizing && (self.air_cue_active || deep_fill) {
            // The compressor spins with the engine: the fill hiss waits out
            // the ignition crank and starts once the engine is actually
            // running at idle, not the instant E is pressed.
            if !self.air_cue_active && !ctx.audio.engine_starting() {
                ctx.audio
                    .start_loop_with(CH_AIR, "vehicle/air_pressurize", AIR_FILL_VOLUME, 400);
                self.air_cue_active = true;
            }
        } else if self.air_cue_active {
            ctx.audio.stop_loop_with(CH_AIR, 700);
            self.air_cue_active = false;
        }
        // The jake's growl: only while it genuinely retards -- engine on, off
        // throttle, coupled, rolling, revs up -- and never through a shift or
        // a pressed clutch (the real jake cuts out and resumes higher).
        let jake_active = {
            let t = &self.trip.truck;
            t.engine_on
                && t.engine_brake()
                && t.throttle < 0.05
                && !t.transmission.in_neutral()
                && !t.transmission.shifting()
                && t.transmission.clutch <= 0.5
                && t.velocity_mps.abs() > 3.0
                && t.rpm >= JAKE_MIN_RPM
        };
        if jake_active {
            let rpm = self.trip.truck.rpm;
            let nearest = JAKE_LOOP_RPMS
                .iter()
                .copied()
                .min_by(|a, b| {
                    (*a as f64 - rpm)
                        .abs()
                        .partial_cmp(&(*b as f64 - rpm).abs())
                        .expect("finite rpm")
                })
                .expect("the loop table is not empty");
            let stage = self
                .trip
                .truck
                .engine_brake_stage
                .clamp(1, JAKE_STAGE_GAIN.len() as i32);
            let rpm_span = 1.0f64.max(2200.0 - JAKE_MIN_RPM);
            let growth = 0.5 + 0.5 * 1.0f64.min((rpm - JAKE_MIN_RPM) / rpm_span);
            let volume = JAKE_STAGE_GAIN[stage as usize - 1] * growth;
            let key = format!("engine/jake_{nearest}");
            // Compare what will really SOUND, not the band we asked for. On
            // the classic voice every band maps to one synth cut, so caching
            // the band key restarted that same file over itself every time
            // rpm crossed a boundary -- which on a grade is constantly.
            let sounding = ctx.audio.voice_key(&key);
            self.jake_cue_idle_s = 0.0;
            if Some(sounding.as_str()) != self.jake_cue_key.as_deref() {
                ctx.audio.start_loop_with(CH_JAKE, &key, volume, 120);
                self.jake_cue_key = Some(sounding);
            } else {
                ctx.audio.set_loop_volume(CH_JAKE, volume);
            }
            // Pitch follows the revs, not the band: the one voice that
            // sounds is a 1600 rpm cut whatever band was asked for, so
            // without this the growl held one note down a whole grade and a
            // manual driver had nothing to shift by (tester ask,
            // 2026-09-14).
            let rate = (rpm / JAKE_VOICE_NATIVE_RPM).clamp(JAKE_RATE_MIN, JAKE_RATE_MAX);
            ctx.audio.set_loop_rate(CH_JAKE, rate);
        } else if self.jake_cue_key.is_some() {
            // The gap is silence, but the loop is held through it (see
            // JAKE_CUE_HOLD_S). A zero-length sync is not a gap: it stops.
            self.jake_cue_idle_s += dt;
            if dt <= 0.0 || self.jake_cue_idle_s >= JAKE_CUE_HOLD_S {
                ctx.audio.stop_loop_with(CH_JAKE, 150);
                self.jake_cue_key = None;
            } else {
                ctx.audio.set_loop_volume(CH_JAKE, 0.0);
            }
        }
        // The cold-start low-air buzzer waits out the ignition crank so the
        // start itself stays audible; if the compressor has already built past
        // the warning line by handoff, there is nothing left to warn about.
        if self.pending_low_air_buzzer && !ctx.audio.engine_starting() {
            self.pending_low_air_buzzer = false;
            if self.trip.truck.engine_on && self.trip.truck.air_low_warning() {
                ctx.audio.play_with("vehicle/low_air_buzzer", 0.55, 0.0);
            }
        }
        let eff = self.trip.weather.effects();
        ctx.audio.set_weather(eff.sound);
        ctx.audio.set_wind(eff.wind);
        self.update_lane_guidance_audio(ctx, dt);
        let rumble = self.lane.rumble_level();
        self.update_edge_ladder_audio(ctx);
        self.update_transverse_strips(ctx);
        self.update_lane_locator_audio(ctx, dt);
        // Exit signaling owns the blinker independently of the locator and
        // alignment. Ordinary steering clicks still defer to the locator.
        self.update_steering_lane_cue(ctx, dt);
        if rumble > 0.0 && ctx.settings.lane_is_manual() {
            // Harsh, continuous pad buzz while over the rumble strip; refreshed
            // each frame, it stops on its own once steered back off.
            ctx.controller.rumble.rumble_strip(rumble);
        }
        let night = is_night(self.trip.local_hour());
        if night {
            ctx.audio.set_ambient(Some("ambient/night"));
        } else {
            ctx.audio.set_ambient(None);
        }
        // Before the dial check: the stations are on the air whether or not
        // this cab is listening, which is what lets a re-tune pick a station
        // up where it got to.
        self.advance_radio_airtime(dt);
        if self.radio.enabled && self.trip.truck.engine_on {
            // The dial has left what the cab plays: check reception now,
            // before playback retunes to the fallback and hides the loss. In
            // Synthesized mode the fallback is the Roadhouse, which playback
            // took up in the frames between checks, so a station fading out
            // switched with nothing said (2026-09-28).
            if self.radio.tuned_station().id != self.radio_station_id {
                self.radio_signal_timer = 0.0;
            }
            self.update_radio_reception(ctx, dt);
            self.update_radio_playback(ctx, night, dt);
            self.update_radio_fringe(ctx, dt);
        } else {
            self.stop_radio_fringe(ctx);
        }
        if self.trip.weather.should_thunder() {
            ctx.audio.play("weather/thunder");
        }
    }

    /// `engine_audio.reading_from_truck(truck)`: `TruckState` does not
    /// implement the `EngineTruck` adapter trait, so the same reading is
    /// assembled here from its public state.
    fn engine_reading(truck: &TruckState) -> EngineReading {
        EngineReading {
            engine_on: truck.engine_on,
            stalled: truck.stalled,
            rpm: truck.rpm,
            throttle: truck.throttle,
            speed_mps: truck.velocity_mps.abs(),
            in_reverse: truck.transmission.in_reverse(),
            in_neutral: truck.transmission.in_neutral(),
            parked_brakes_holding: truck.parking_brake || truck.spring_brakes_active(),
            air_ready: truck.air_ready(),
        }
    }
}
