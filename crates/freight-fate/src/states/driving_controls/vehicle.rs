//! The controls that move metal: the engine, the parking brake, the jake
//! stalk and its cylinder selector, and the manual gearbox.

use ff_core::speech_pacing::SpeechCategory;

use crate::app::{GameContext, Say};
use crate::bindings::Action;
use crate::states::driving::DrivingState;
use crate::states::driving_core::*;

impl DrivingState {
    /// `_toggle_engine_brake()`.
    ///
    /// J is the dash enable switch of a real Jacobs setup. On a manual box it
    /// engages at whatever stage the selector was left on, never surprising an
    /// icy descent with full retard the driver dialed back an hour ago. On an
    /// automatic box the retarder is managed like a real AMT integrates it: J
    /// arms AUTO mode, the controller picks and steps the stage to hold the
    /// engagement speed (owner design, 2026-07-22), and 1/2/3 take manual
    /// control back.
    ///
    /// Only the driver's own foot refuses it. Cruise's throttle is not a
    /// pedal the driver can release, and the downgrade call asks for J with
    /// cruise still pulling up to the crest (agent drive, I-70, 2026-09-25).
    pub fn toggle_engine_brake(&mut self, ctx: &mut GameContext) {
        let foot_on_it = self.held(ctx, Action::Accelerate)
            || (self.pad_live(ctx) && ctx.controller.throttle_target() > 0.05);
        if foot_on_it && !self.trip.truck.engine_brake() {
            ctx.say("Release the accelerator before turning the jake on.");
            return;
        }
        // Auto mode counts as ON even with the stage down: the manager puts
        // the retarder all the way off wherever there is nothing to hold, and
        // J has to switch the manager off rather than arm a second one.
        if self.trip.truck.engine_brake() || self.auto_jake {
            self.trip.truck.engine_brake_stage = 0;
            self.auto_jake = false;
            ctx.say("Jake off.");
            return;
        }
        // Turning it on is what retires the downgrade call's "set the engine
        // brake" advice (`descend_advice`).
        self.note_instruction_demonstrated(ctx, "engine_brake");
        if self.trip.truck.transmission.automatic && self.auto_jake_enabled {
            self.auto_jake = true;
            self.auto_jake_hold_mph = Some(5.0f64.max(self.trip.truck.speed_mph()));
            self.auto_jake_cooldown_s = 0.0;
            self.trip.truck.engine_brake_stage = 1; // the controller climbs from here
            ctx.say("Jake on, automatic.");
            return;
        }
        self.trip.truck.engine_brake_stage = self.jake_selected_stage;
        let word = jake_stage_word(self.trip.truck.engine_brake_stage);
        ctx.say(&format!("Jake on, stage {word}."));
    }

    /// `_toggle_auto_jake_enabled()`: Alt+J -- whether J arms retarder
    /// management on an automatic box.
    ///
    /// Off, the jake stalk behaves like the manual-box selector even on an
    /// AMT -- for the driver who wants to stage it by hand.
    pub fn toggle_auto_jake_enabled(&mut self, ctx: &mut GameContext) {
        self.auto_jake_enabled = !self.auto_jake_enabled;
        ctx.settings.automatic_jake = self.auto_jake_enabled;
        if !self.auto_jake_enabled && self.auto_jake {
            // Managing right now: hand the current stage over to the driver.
            self.auto_jake = false;
            let stage = 1.max(self.trip.truck.engine_brake_stage);
            self.jake_selected_stage = stage;
            ctx.say(&format!(
                "Automatic jake off; holding stage {}.",
                jake_stage_word(stage)
            ));
            return;
        }
        let state = if self.auto_jake_enabled { "on" } else { "off" };
        ctx.say(&format!("Automatic jake {state}."));
    }

    /// `_select_jake_stage(stage)`: 1, 2, 3 -- the cylinder selector, live
    /// only while the jake is on.
    ///
    /// With the jake off the number keys do nothing here, so they stay free
    /// for other bindings in other contexts (owner, 2026-07-21). On an
    /// automatic box a manual pick takes over from auto mode.
    pub fn select_jake_stage(&mut self, ctx: &mut GameContext, stage: i32) {
        // Armed auto mode answers even with its stage down -- picking a
        // cylinder count is how the driver takes it back.
        if !self.trip.truck.engine_brake() && !self.auto_jake {
            return;
        }
        let was_auto = self.auto_jake;
        self.auto_jake = false;
        // A pick is the driver taking the retarder over, whoever raised it.
        // Left owned by cruise or the bend assist, the stage went back to
        // zero the moment their reason ran out ("we see no reason for you to
        // be doing that", player report, 2026-10-14).
        self.cruise_jake_stage = 0;
        self.curve_assist_jake = false;
        self.jake_selected_stage = stage;
        self.trip.truck.engine_brake_stage = stage;
        let suffix = if was_auto { ", manual" } else { "" };
        ctx.say(&format!(
            "Jake stage {} selected{suffix}.",
            jake_stage_word(stage)
        ));
    }

    /// `_cycle_jake_stage()`: controller -- modifier plus the jake button
    /// walks 1 -> 2 -> 3 -> 1.
    pub fn cycle_jake_stage(&mut self, ctx: &mut GameContext) {
        if !self.trip.truck.engine_brake() && !self.auto_jake {
            return;
        }
        let next = self.trip.truck.engine_brake_stage % JAKE_STAGES + 1;
        self.select_jake_stage(ctx, next);
    }

    /// `_shift_relative(delta)`: controller next/previous gear -- step one
    /// gear from the current one.
    pub fn shift_relative(&mut self, ctx: &mut GameContext, delta: i32) {
        let tr = &self.trip.truck.transmission;
        if tr.automatic {
            return;
        }
        let target = REVERSE.max(tr.num_gears().min(tr.gear + delta));
        if target != tr.gear {
            self.manual_shift(ctx, target);
        }
    }

    /// `_toggle_engine()`: E.
    pub fn toggle_engine(&mut self, ctx: &mut GameContext) {
        if ctx.audio.engine_starting() {
            // Ignition still in progress; ignore mashed presses so the crank,
            // shutdown, and loop sounds cannot stack on top of each other.
            return;
        }
        if self.trip.truck.engine_on {
            if self.trip.truck.speed_mph() > ENGINE_SHUTDOWN_SAFE_MPH {
                ctx.audio.play("ui/error");
                self.set_status("Engine shutdown blocked: slow down first.");
                // Retired with the engine key: a driver who has started it a
                // few times hears the short form.
                if self.instruction_retired(ctx, "engine") {
                    ctx.say("Too fast to shut down.");
                } else {
                    let speed = ctx.settings.speed_text(self.trip.truck.speed_mph());
                    let limit = ctx.settings.speed_text(ENGINE_SHUTDOWN_SAFE_MPH);
                    ctx.say(&format!(
                        "Unsafe to shut the engine off at {speed}. Brake below {limit} first."
                    ));
                }
                return;
            }
            self.trip.truck.stop_engine();
            ctx.audio.engine_stop();
            self.set_status("Engine off.");
            ctx.say_with(
                "Engine off.",
                Say::new().category(SpeechCategory::Confirmation),
            );
            return;
        }
        if self.trip.truck.start_engine() {
            self.note_instruction_demonstrated(ctx, "engine");
            ctx.audio.engine_start();
            if self.trip.truck.air_low_warning() {
                // A cold start is low on air by definition, but sounding the
                // buzzer here buries the crank. Hold it until the ignition
                // hands off; by then the compressor has usually pushed past
                // the warning line and the buzzer honestly has nothing left
                // to say (the spoken air readout below carries the state
                // either way). The haptic alert still lands immediately.
                self.pending_low_air_buzzer = true;
                ctx.controller.rumble.alert();
                self.low_air_said = true;
            }
            self.set_status("Engine running.");
            let instruction = self.air_start_instruction(ctx);
            ctx.say_with(
                format!("Engine running. {instruction}"),
                Say::new().category(SpeechCategory::Confirmation),
            );
            if let Some(tutorial) = self.tutorial.as_mut() {
                tutorial.on_engine_started(ctx);
            }
            return;
        }
        ctx.audio.play("ui/error");
        if self.trip.truck.fuel_gal <= 0.0 {
            // never a dead end: the roadside rescue always comes
            self.handle_out_of_fuel(ctx);
        } else {
            ctx.say("The engine will not start.");
        }
    }

    /// `_air_start_instruction()`.
    ///
    /// Names the parking brake key (and, with the brake off, the accelerator)
    /// until the player has released the brake a few times; then just the air.
    pub fn air_start_instruction(&self, ctx: &mut GameContext) -> String {
        let psi = self.trip.truck.air_pressure_psi();
        if self.terse_speech(ctx) || self.instruction_retired(ctx, "parking_brake") {
            if self.trip.truck.air_ready() {
                return "Air ready.".to_string();
            }
            return format!("Air {psi:.0} psi.");
        }
        let brake_hint = ctx.control_hint("parking_brake");
        if self.trip.truck.parking_brake {
            if self.trip.truck.air_ready() {
                return format!("Air ready. {brake_hint} releases the parking brake.");
            }
            return format!(
                "Air {psi:.0} psi. At 100 psi, {brake_hint} releases the parking brake."
            );
        }
        format!(
            "Air ready. Hold {} to drive.",
            ctx.control_hint("accelerate")
        )
    }

    /// `_toggle_parking_brake()`: P.
    pub fn toggle_parking_brake(&mut self, ctx: &mut GameContext) {
        if self.trip.truck.parking_brake {
            // Trying to leave, even if low air keeps the brake locked: stop
            // fast-forwarding so build-up time is not billed at waiting pace.
            self.trip.waiting = false;
            if self.trip.truck.release_parking_brake() {
                ctx.audio.play_with("vehicle/brake_release", 0.65, 0.0);
                self.note_instruction_demonstrated(ctx, "parking_brake");
                self.set_status("Parking brake released.");
                let psi = self.trip.truck.air_pressure_psi();
                ctx.say_with(
                    format!("Parking brake released. Air pressure {psi:.0} psi."),
                    Say::new().category(SpeechCategory::Confirmation),
                );
                if let Some(tutorial) = self.tutorial.as_mut() {
                    tutorial.on_parking_brake_released(ctx);
                }
            } else {
                ctx.audio.play("ui/error");
                self.set_status("Parking brake locked: build air pressure first.");
                let psi = self.trip.truck.air_pressure_psi();
                if self.terse_speech(ctx) {
                    ctx.say_with(
                        format!("Parking brake set. Air pressure {psi:.0} psi."),
                        Say::new().category(SpeechCategory::Confirmation),
                    );
                } else {
                    ctx.say(&format!("Parking brake set. Air {psi:.0} psi."));
                }
            }
            return;
        }
        self.trip.truck.set_parking_brake();
        self.trip.truck.throttle = 0.0;
        self.cancel_cruise_for_parking_brake(ctx);
        let speed = self.trip.truck.speed_mph();
        if speed > DYNAMITE_MIN_MPH {
            // Dynamiting the brakes: pulling the valve at speed is NOT
            // impossible in a real truck -- it is the emergency backup, and
            // it is violent. The springs slam the drive axle, the tires
            // flat-spot against the pavement, and the tread bill scales with
            // how fast you were going (owner design question, 2026-07-24:
            // realism says allowed-with-consequences, never impossible). No
            // waiting fast-forward while still rolling.
            self.trip.truck.tire_wear_pct =
                100.0f64.min(self.trip.truck.tire_wear_pct + speed * FLAT_SPOT_WEAR_PCT_PER_MPH);
            ctx.audio.play_with("vehicle/tire_screech", 0.9, 0.0);
            ctx.audio.play_with("vehicle/brake_set", 0.9, 0.0);
            ctx.controller.rumble.alert();
            self.set_status("Parking brake dynamited at speed!");
            let spoken = ctx.settings.speed_text(speed);
            ctx.say(&format!(
                "You dynamited the parking brake at {spoken}! The spring brakes slam the drive \
                 axle and the tires grind flat spots into the tread."
            ));
            return;
        }
        if speed <= PARKING_BRAKE_SETTLE_MAX_MPH {
            secure_truck_for_stopped_menu_at(self, ctx, PARKING_BRAKE_SETTLE_MAX_MPH);
        }
        // The player's own brake press means deliberate waiting; auto-sets at
        // trip start, rest stops, and arrivals never arm the fast-forward.
        self.trip.waiting = true;
        ctx.audio.play_with("vehicle/brake_set", 0.65, 0.0);
        self.set_status("Parking brake set.");
        let slowing = if self.trip.truck.speed_mph() > DOCKING_MAX_MPH {
            " Truck still slowing."
        } else {
            ""
        };
        let psi = self.trip.truck.air_pressure_psi();
        ctx.say_with(
            format!("Parking brake set. Air pressure {psi:.0} psi.{slowing}"),
            Say::new().category(SpeechCategory::Confirmation),
        );
    }

    /// `_manual_shift(gear)`.
    ///
    /// Through the truck, not the gearbox: the speed-dependent guards (a
    /// reverse selection while rolling forward) need road speed, which the
    /// transmission has no way to know.
    pub fn manual_shift(&mut self, ctx: &mut GameContext, gear: i32) {
        let rolling_reverse =
            gear == REVERSE && self.trip.truck.speed_mph() > REVERSE_ENGAGE_MAX_MPH;
        let result = self.trip.truck.request_gear(gear);
        if result.ok {
            // The lever: kachunk. The second half -- the gear taking as the
            // clutch comes back in -- is played by update_audio when it
            // actually happens, the way an automatic's engagement is.
            ctx.audio
                .play_bank("vehicle/shift_manual", "vehicle/gear_shift");
            self.manual_engage_clunk_pending = true;
            ctx.say(&result.message);
            if let Some(tutorial) = self.tutorial.as_mut() {
                tutorial.on_gear_engaged(ctx);
            }
        } else if rolling_reverse {
            ctx.audio.play("vehicle/gear_grind");
            let damage = self.trip.truck.damage_pct;
            ctx.say(&format!(
                "{} That crash of gears cost the driveline, damage {damage:.0} percent.",
                result.message
            ));
        } else if result.grind {
            ctx.audio.play("vehicle/gear_grind");
            ctx.say(&format!(
                "Grinding gears! Hold {} to press the clutch first.",
                ctx.control_hint("clutch")
            ));
        } else {
            ctx.say(&result.message);
        }
    }
}
