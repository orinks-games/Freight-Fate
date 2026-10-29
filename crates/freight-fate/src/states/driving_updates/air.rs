//! Held inputs and the air system's voice: the brake lockout, the air and
//! spring-brake announcements, the direction-change gesture, the over-rev
//! warning, the horn's protection valve, and the idle settle a menu needs.

use ff_core::speech_pacing::SpeechCategory;

use crate::speech::EventPriority;

use crate::app::{GameContext, SayEvent};
use crate::states::driving::DrivingState;
use crate::states::driving_core::*;
use crate::states::driving_updates::{live, OVERREV_GRACE_S, OVERREV_REPEAT_S};

impl DrivingState {
    pub fn maybe_say_air_brake_lockout(&mut self, ctx: &mut GameContext) {
        if self.brake_lockout_cue_timer > 0.0 {
            return;
        }
        self.brake_lockout_cue_timer = 4.0;
        // One standing condition -- "why the truck will not roll yet" --
        // with three mutually exclusive descriptions. A player who holds the
        // accelerator against the lockout re-triggers this every 4 seconds;
        // sharing the key means an unchanged reason (still parked, still
        // building air at the same psi) speaks once, while a genuine change
        // (engine started, psi climbed, brake released then reset) speaks
        // again because the text itself differs.
        //
        // ROUTE, not the ambient default, on all three: this line is the
        // reason the truck will not move under the driver's own throttle. As
        // AMBIENT it was droppable as stale chatter, and the adversarial
        // battery caught it being dropped the moment real traffic started
        // appearing on the leg -- the AADT spawn change put a brake-lights
        // advisory and an achievement ahead of it in the same channel, and
        // the lockout lost a race it had never been in. Same call as the toll
        // charge and the adaptive-cruise lines: a consequence is not colour.
        //
        // And each is spoken only while its own reason still holds: queued
        // behind other speech, the parking-brake line once spoke after the
        // brake was released and the truck was rolling (see
        // `live::set_move_lockout`).
        self.refresh_live_facts();
        let opts = |reason: u8| {
            SayEvent::queued()
                .priority(EventPriority::Important)
                .key("air_brake_lockout")
                .category(SpeechCategory::Status)
                .valid(move || live::move_lockout() == reason)
        };
        if !self.trip.truck.engine_on {
            self.set_status("Start the engine before releasing the brakes.");
            ctx.say_event_with("Engine off.", opts(live::LOCKOUT_ENGINE_OFF));
        } else if !self.trip.truck.air_ready() {
            self.set_status("Waiting for air pressure before the truck can move.");
            let psi = self.trip.truck.air_pressure_psi();
            ctx.say_event_with(
                format!("Air pressure {psi:.0} psi."),
                opts(live::LOCKOUT_AIR),
            );
        } else if self.trip.truck.parking_brake {
            let brake_hint = ctx.control_hint("parking_brake");
            self.set_status(format!(
                "Parking brake set. Press {brake_hint} to release it."
            ));
            ctx.say_event_with("Parking brake set.", opts(live::LOCKOUT_PARKING_BRAKE));
        }
    }

    /// `_update_air_brake_announcements(was_engine_on, was_ready, was_low,
    /// was_spring)`.
    ///
    /// Python carried a positional shim for older three-argument call sites;
    /// there are none in Rust, so the four readings are required.
    pub fn update_air_brake_announcements(
        &mut self,
        ctx: &mut GameContext,
        was_engine_on: bool,
        was_ready: bool,
        was_low: bool,
        was_spring: bool,
    ) {
        let _ = was_low;
        let terse = self.terse_speech(ctx);
        if self.trip.truck.air_low_warning()
            && self.trip.truck.engine_on
            && (!self.low_air_said || !was_engine_on)
        {
            self.low_air_said = true;
            ctx.audio.play_with("vehicle/low_air_buzzer", 0.7, 0.0);
            ctx.controller.rumble.alert();
            // Rolling, the driver needs the one action that matters: get
            // stopped while there is still air to stop with, because the
            // spring brakes will do it for them at 40 psi wherever they happen
            // to be. Parked, there is nothing to do but wait.
            let rolling = self.trip.truck.velocity_mps.abs() > 0.3;
            let psi = self.trip.truck.air_pressure_psi();
            let message = match (terse, rolling) {
                (true, _) => format!("Low air: {psi:.0} psi."),
                (false, true) => format!("Low air warning, {psi:.0} psi. Stop now."),
                (false, false) => format!("Low air warning, {psi:.0} psi."),
            };
            // Parked, this is a band readout; rolling, it is the last warning
            // before the spring brakes set on their own -- the same
            // urgency-decides-the-category shape as the HOS check above.
            let category = if rolling {
                SpeechCategory::Safety
            } else {
                SpeechCategory::Status
            };
            ctx.say_event_with(message, SayEvent::new().category(category));
        } else if self.trip.truck.air_pressure_psi()
            >= self.trip.truck.specs.air_low_warning_clear_psi
        {
            // Re-arm only once pressure has recovered clear of the warning
            // threshold (hysteresis), not merely ticked a fraction above it.
            // Heavy or repeated service braking otherwise leaves pressure
            // bouncing right around air_low_warning_psi while the compressor
            // catches up, and each bounce re-fired the full warning line.
            self.low_air_said = false;
        }

        if self.trip.truck.spring_brakes_active() && !was_spring && !self.spring_brake_said {
            self.spring_brake_said = true;
            ctx.audio.play_with("vehicle/low_air_buzzer", 0.9, 0.0);
            ctx.controller.rumble.alert();
            let message = if terse {
                "Spring brakes applied."
            } else {
                "Spring brakes applied from low air pressure."
            };
            // The low-air band is a STATUS readout; the spring brakes actually
            // setting is the emergency the band was warning about -- SAFETY.
            ctx.say_event_with(message, SayEvent::new().category(SpeechCategory::Safety));
        } else if !self.trip.truck.spring_brakes_active() {
            self.spring_brake_said = false;
        }

        if self.trip.truck.air_ready()
            && self.trip.truck.parking_brake
            && !was_ready
            && !self.air_ready_said
        {
            // The cue's whole job is "you can release the parking brake now", so
            // only announce while it is set. Once released (rolling, or braking to
            // a stop on arrival), a dip back across the threshold must not
            // re-announce it.
            self.air_ready_said = true;
            ctx.audio.play_with("vehicle/air_dryer_purge", 0.65, 0.0);
            let brake_hint = ctx.control_hint("parking_brake");
            self.set_status(format!(
                "Air ready. Press {brake_hint} to release the parking brake."
            ));
            let message = "Air ready.";
            // ROUTE, not the ambient default: same reasoning as the lockout
            // lines below -- this is the reason the truck can now move, a
            // standing instruction until the driver acts (automation-handoff
            // sweep, 2026-08-20, the deferred 2026-08-15 audit).
            ctx.say_event_with(
                message,
                SayEvent::queued()
                    .priority(EventPriority::Important)
                    .category(SpeechCategory::Status),
            );
            // air_ready is retired as an award (folded into "first_day" at
            // pickup completion, see city_pickup.py); the catalog entry and
            // id stay so the cloud validator's allow-list never sees a
            // removed id.
        } else if self.trip.truck.air_low_warning() {
            // Re-arm the ready cue only after a genuine depletion (low-air), not
            // the routine 100-125 psi compressor cycling: the parking-release
            // threshold sits at the cut-in pressure, so air_ready otherwise
            // flickers across it every cycle and re-announces back to back.
            self.air_ready_said = false;
        }
    }

    /// Return true when the current key state means backing up.
    ///
    /// `accel_held`/`brake_held` are the instantaneous (unsmoothed) press
    /// states used for the shift-gesture edge detection; on the keyboard they
    /// are the same as the ramped `accelerating`/`braking_key`.
    pub fn update_reverse_controls(
        &mut self,
        ctx: &mut GameContext,
        accelerating: bool,
        braking_key: bool,
        accel_held: bool,
        brake_held: bool,
        dt: f64,
    ) -> bool {
        // Deliberate direction changes use a fresh press (rising edge). Simple
        // direction changes keep the familiar behavior of holding the control
        // through the stop. Track both edges in either mode so changing the
        // setting during a drive cannot leave stale input state behind.
        let brake_edge = brake_held && !self.reverse_brake_held;
        let accel_edge = accel_held && !self.reverse_accel_held;
        self.reverse_brake_held = brake_held;
        self.reverse_accel_held = accel_held;
        if !self.trip.truck.transmission.automatic {
            self.direction_armed = String::new();
            self.direction_hold_s = 0.0;
            return self.trip.truck.transmission.in_reverse() && braking_key && !accelerating;
        }
        // One safe gesture for every direction change: a FRESH press observed
        // at a standstill arms it, and the gear engages only after the
        // control is held through a short beat. A press that lands while
        // still rolling is part of a stop and never arms; a hold that
        // predates the stop never arms; a quick confirm-tap at a stop -- how
        // a screen-reader driver checks the truck is holding -- just brakes.
        // (Owner-hit three ways on 2026-07-14: held through the stop,
        // feathered to a stop, and confirm-tapped at the yard.)
        let stopped = self.trip.truck.velocity_mps.abs() < 0.3;
        let in_reverse = self.trip.truck.transmission.in_reverse();
        let want = if in_reverse { "forward" } else { "reverse" };
        let control_edge = if in_reverse { accel_edge } else { brake_edge };
        let control_held = if in_reverse { accel_held } else { brake_held };
        let other_held = if in_reverse { brake_held } else { accel_held };
        if control_edge && stopped && !other_held {
            self.direction_armed = want.to_string();
            self.direction_hold_s = 0.0;
        }
        if self.direction_armed == want && control_held && stopped && !other_held {
            self.direction_hold_s += dt;
            if self.direction_hold_s >= DIRECTION_CHANGE_HOLD_S {
                self.direction_armed = String::new();
                self.direction_hold_s = 0.0;
                self.trip.truck.transmission.shift_timer = 0.0;
                ctx.audio
                    .play_bank_with("vehicle/shift_manual", "vehicle/gear_shift", 0.55, 0.0);
                if want == "forward" {
                    self.trip.truck.transmission.gear = 1;
                    self.set_status("Forward gear selected.");
                    // ROUTE, not the ambient default: the driver's only
                    // confirmation of which way the truck is now geared, and a
                    // missed one leaves a blind driver guessing direction
                    // (automation-handoff sweep, 2026-08-20, the deferred
                    // 2026-08-15 audit).
                    ctx.say_event_with(
                        "Forward gear selected.",
                        SayEvent::queued()
                            .priority(EventPriority::Important)
                            .category(SpeechCategory::Confirmation),
                    );
                    return false;
                }
                self.trip.truck.transmission.gear = REVERSE;
                self.cancel_cruise(ctx, false);
                self.set_status("Reverse selected. Backing slowly.");
                // No spoken line: the reverse beep is already running and it
                // keeps running the whole time the truck is in reverse, which
                // a one-shot sentence cannot do -- and it says the same thing
                // (owner, 2026-08-21). The status readout still carries the
                // words for anyone who asks for it. Coming back OUT of
                // reverse still speaks: nothing beeps for forward gear.
                return true;
            }
        } else {
            self.direction_armed = String::new();
            self.direction_hold_s = 0.0;
        }
        if self.trip.truck.transmission.in_reverse() {
            return braking_key && !accelerating;
        }
        false
    }

    pub fn update_overrev(&mut self, ctx: &mut GameContext, dt: f64) {
        if !self.trip.truck.over_revving() {
            self.overrev_s = 0.0;
            self.overrev_warn_due = OVERREV_GRACE_S;
            // Off the limiter. The next time the engine goes there is a fresh
            // event and gets its warning again, even at the same wear number.
            ctx.reset_event_condition("engine_redline");
            return;
        }
        self.overrev_s += dt;
        if self.overrev_s < self.overrev_warn_due {
            return;
        }
        self.overrev_warn_due = self.overrev_s + OVERREV_REPEAT_S;
        ctx.audio.play("ui/warning");
        ctx.controller.rumble.alert();
        // Speak the meter that is actually moving. Over-revving has charged
        // ENGINE WEAR since the wear meters landed (see
        // ENGINE_WEAR_OVER_REV_PCT_PER_S, "was the damage_pct redline
        // penalty"), but this warning went on reading damage_pct -- which for
        // most drivers sits at zero. The line told the player nothing was
        // being harmed while real harm accumulated, and for a player who only
        // has the spoken word that is the whole readout, not a detail.
        // Where damage has separately put the truck in a band, name the band
        // too, so the number and its meaning never travel apart.
        let band = self.damage_band_clause(ctx);
        let band_clause = if band.is_empty() {
            String::new()
        } else {
            format!(" Truck is in {band}.")
        };
        let wear = self.trip.truck.engine_wear_pct;
        let message = if self.terse_speech(ctx) {
            format!("Redline. Engine wear {wear:.0} percent.{band_clause}")
        } else {
            format!("Engine at redline. Engine wear {wear:.0} percent.{band_clause}")
        };
        // A standing condition: the engine is still at redline and the driver
        // already knows. Repeating it earns the voice only when the wear
        // number it carries has actually moved.
        ctx.say_event_with(
            message,
            SayEvent::new()
                .key("engine_redline")
                .category(SpeechCategory::Status),
        );
    }

    /// The pressure protection valve, audibly: below its threshold the
    /// horn dies mid-blast and the brakes keep their air (FMVSS 121 -- see
    /// `TruckState::horn_available`). Say why once; the driver hearing
    /// the horn cut out otherwise reads as a broken speaker.
    pub fn update_horn_protection(&mut self, ctx: &mut GameContext) {
        if self.trip.truck.horn_on && !self.trip.truck.horn_available() {
            ctx.audio.horn_stop();
            self.trip.truck.horn_on = false;
            ctx.say_event_with(
                "Horn cut out, low air pressure.",
                SayEvent::queued()
                    .priority(EventPriority::Important)
                    .category(SpeechCategory::Status),
            );
        }
    }

    /// Snap engine RPM and audio to idle for a menu-driven stop.
    ///
    /// You are stopped -- for a trooper, a dock, a scale, a pickup gate --
    /// not parked for the night: the engine keeps running, but it must not
    /// keep sounding like highway load. The frame loop that eases the rev
    /// down between frames stops running the instant a menu takes over the
    /// driving state, so whatever was left over from braking to the stop --
    /// a lagging throttle, RPM still catching up to idle -- would otherwise
    /// hang in the engine loop for the whole encounter. Set the engine band
    /// directly rather than through the full audio update, which also
    /// drives the radio, lane cues, and weather bed -- none of which belong
    /// in this one-off sync.
    pub fn settle_engine_to_idle(&mut self, ctx: &mut GameContext) {
        self.trip.truck.throttle = 0.0;
        self.trip.truck.rpm = self.trip.truck.specs.idle_rpm;
        ctx.audio.set_engine_rpm_with(self.trip.truck.rpm, 0.0);
    }
}
