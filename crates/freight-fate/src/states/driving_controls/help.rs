//! F1: the driving layout, spoken for whichever device is in use.
//!
//! The two help texts are long, and they are player-facing word for word, so
//! each `push_str` below is one Python string literal from
//! `driving_controls.py`. Keeping the seams where Python put them is what
//! makes a wording change diffable against the reference implementation.

use crate::app::GameContext;
use crate::bindings::Action;
use crate::states::driving::DrivingState;
use crate::states::driving_core::*;

impl DrivingState {
    /// `_objective_help()`.
    pub fn objective_help(&self, ctx: &GameContext) -> String {
        if self.phase == DRIVE_PHASE_PICKUP {
            return format!(
                "Pickup: drive to {}, stop at the gate, check in and load. ",
                self.pickup_facility_text(ctx)
            );
        }
        "At your destination, stop, then dock and deliver. ".to_string()
    }

    /// `_speak_driving_help()`: keyboard or controller layout, following the
    /// device in use.
    pub fn speak_driving_help(&mut self, ctx: &mut GameContext) {
        self.note_instruction_demonstrated(ctx, "help");
        if ctx.controller.device() == "controller" {
            self.speak_controller_help(ctx);
        } else if ctx.controller.device() == ff_core::input_hints::TOUCH {
            self.speak_touch_help(ctx);
        } else {
            self.speak_keyboard_help(ctx);
        }
    }

    /// `_speak_keyboard_help()`.
    ///
    /// Every key named here comes from the player's own table
    /// (`ctx.bindings`), so a moved shortcut is taught under its new name.
    pub fn speak_keyboard_help(&mut self, ctx: &mut GameContext) {
        let n = |action: Action| ctx.bindings.spoken(action);
        let up = n(Action::Accelerate);
        let down = n(Action::Brake);
        let objective_help = self.objective_help(ctx);
        let automatic_help = if ctx.settings.automatic_direction_changes == "deliberate" {
            format!(
                "In automatic with deliberate direction changes, stop, release \
                 {down}, then press and hold it again to reverse. While \
                 reversing, stop with {up}, release, then press and hold \
                 again for forward. A quick tap just brakes. "
            )
        } else {
            format!(
                "In automatic with simple direction changes, stop, release {down}, \
                 then press and hold it again to reverse. While reversing, \
                 stop with {up}, release, then hold it again for forward. A \
                 brake held through a stop just holds the truck. "
            )
        };
        let latch_help = if ctx.settings.pedal_latch != "off" {
            format!(
                "Tap the brake, then press again and hold half a second to latch it \
                 hands-free; a click and a spoken confirmation mark the catch. \
                 {down} once releases it; the accelerator releases it instantly. The \
                 throttle key never latches. "
            )
        } else {
            String::new()
        };

        let mut text = String::new();
        text.push_str(&format!("Hold {up} to accelerate, {down} to brake. "));
        text.push_str(&latch_help);
        text.push_str(&automatic_help);
        text.push_str(&format!(
            "Hold {} for the emergency brake, the hardest possible stop. ",
            n(Action::EmergencyBrake)
        ));
        text.push_str(&format!(
            "{} starts automatic speed control: adaptive cruise on open roads, ",
            n(Action::Cruise)
        ));
        text.push_str("the speed keeper in low-speed zones. Weather widens the gap. ");
        text.push_str("Cruise eases early for a sharp posted-limit drop, the keeper for ");
        text.push_str("the next turn or the next lower limit. ");
        text.push_str("Braking cancels the session. At the planned pickup it pauses and ");
        text.push_str("resumes once you depart. ");
        text.push_str("Plus and minus, including the keypad keys, change the open-road ");
        text.push_str(
            "target by five; it never holds more than five over the posted limit. Control ",
        );
        text.push_str("with plus or minus, by one. ");
        text.push_str(&format!(
            "{} resumes the last cruise speed. ",
            n(Action::CruiseResume)
        ));
        text.push_str(&format!(
            "Parked with the brake set, {} latches a high idle; plus and minus ",
            n(Action::Cruise)
        ));
        text.push_str("adjust it, and releasing the parking brake drops it. ");
        let exit = n(Action::TakeExit);
        text.push_str(&format!(
            "{exit} signals for the next announced route exit, by number when "
        ));
        text.push_str("known, or cancels that signal. Unless lane keeping is on full, move ");
        text.push_str("to the right lane, then steer right into the exit lane where it ");
        text.push_str("opens, just before the gore. Keep road speed to there, then brake to ");
        text.push_str("the exit speed before the ramp curve. Ramps usually end ");
        text.push_str(&format!(
            "at a traffic light or stop sign, called out on the way down. {exit} "
        ));
        text.push_str("also signals a pull-over when a trooper lights you up for speeding, ");
        text.push_str("a scale bypass, or unsafe equipment: signal, then brake to a stop. ");
        text.push_str("Ignoring the lights brings failure-to-stop ");
        text.push_str("warnings, then a stop that cancels the load: a major offense. ");
        text.push_str(&format!(
            "{} also speaks the date and season. ",
            n(Action::Clock)
        ));
        text.push_str(&format!(
            "{} toggles the in-cab radio. Page Down tunes to the next station, ",
            n(Action::Radio)
        ));
        text.push_str("Page Up to the previous; semicolon and apostrophe do the same. ");
        text.push_str("Control with the tuning keys jumps a category; Shift changes the ");
        text.push_str("radio volume in 10 percent steps, on or off. ");
        text.push_str(&format!(
            "{} saves or unsaves the station as a favorite. ",
            n(Action::RadioFavorite)
        ));
        text.push_str(&format!(
            "{} speaks station, volume, and streamer-safe status; {} speaks ",
            n(Action::RadioStatus),
            n(Action::RadioNowPlaying)
        ));
        text.push_str("the song when the station says. The Driver apps tablet has a ");
        text.push_str("Radio app to search the dial, tune by name, and keep favorites; ");
        text.push_str(&format!(
            "the {} status menu has a radio screen of receivable stations. ",
            n(Action::Status)
        ));
        text.push_str(&format!(
            "{} starts the engine, and stops it only below 5 miles per hour. ",
            n(Action::Engine)
        ));
        text.push_str("Air pressure must build before the truck can move. ");
        text.push_str(&format!(
            "{} sets or releases the parking brake. It needs 100 psi of air. ",
            n(Action::ParkingBrake)
        ));
        text.push_str(&objective_help);
        text.push_str(&format!(
            "{} speed, active speed-control mode, and target, and with the signal set, \
             how far to the exit. ",
            n(Action::Speed)
        ));
        text.push_str(&format!(
            "{} posted speed limit. {} the grade under the wheels, whether the ",
            n(Action::SpeedLimit),
            n(Action::Grade)
        ));
        text.push_str(&format!(
            "truck is holding it, and the next grade ahead. {} status menu. {} fuel. ",
            n(Action::Status),
            n(Action::Fuel)
        ));
        text.push_str(&format!(
            "{} clock, deadline, and the hours limit that comes first. ",
            n(Action::Clock)
        ));
        text.push_str(&format!(
            "{} time at the wheel so far, {} when your 30 minute break ",
            n(Action::HosWheel),
            n(Action::HosBreak)
        ));
        text.push_str(&format!(
            "is due, {} what ends this shift and where you can legally stop ",
            n(Action::HosDrive)
        ));
        text.push_str("before it. ");
        text.push_str(&format!(
            "{} progress, distance left, and where you are. ",
            n(Action::Route)
        ));
        text.push_str(&format!(
            "{} the state, {} the road, {} the town or the nearest ",
            n(Action::PlaceState),
            n(Action::PlaceRoad),
            n(Action::PlaceTown)
        ));
        text.push_str(&format!(
            "one, {} the direction. The keypad numbers work the same way. ",
            n(Action::PlaceDirection)
        ));
        text.push_str(&format!(
            "{} weather. {} lane position and whether the lane beside you is ",
            n(Action::Weather),
            n(Action::Lane)
        ));
        text.push_str(&format!(
            "open. {} turns the lane locator on and off: a soft tock once a ",
            n(Action::LaneLocator)
        ));
        text.push_str("beat, panned to where you sit in your lane, on lane keeping ");
        text.push_str("partial or off. ");
        text.push_str(&format!(
            "{} repeats the last driving announcement. ",
            n(Action::LastAnnouncement)
        ));
        text.push_str(&format!(
            "{} repeats the last CB chatter, with the distance as it is now. ",
            n(Action::Cb)
        ));
        text.push_str(
            "Comma repeats what was just said and keeps stepping back; Period moves \
             forward again. ",
        );
        text.push_str("Control with Comma or Period jumps to the oldest or newest message. ");
        text.push_str(
            "The bracket keys switch between all messages, general messages, and driving \
             events. ",
        );
        text.push_str("Control C copies the message you are on. ");
        text.push_str(&format!(
            "{} reads the road ahead that no other key answers: the exit your \
             signal is on for, the ramp ",
            n(Action::Upcoming)
        ));
        text.push_str("control coming up, the next imposed limit, the next stop, and ");
        text.push_str("the next bend that demands slowing. ");
        text.push_str("Bends that demand slowing are called before they arrive, like ");
        text.push_str(&format!(
            "Sharp left, half a mile, advise 35; {} gives one safe-speed number ",
            n(Action::SafeSpeed)
        ));
        text.push_str("with the bend in it. ");
        text.push_str(&format!(
            "The {} status menu includes a Driver apps tablet for navigation, ",
            n(Action::Status)
        ));
        text.push_str("weather, traffic, truck stops, road chatter, and ELD. ");
        text.push_str("Left or Right Control stops the driving event voice. ");
        let left = n(Action::SteerLeft);
        let right = n(Action::SteerRight);
        text.push_str(&format!(
            "{left} and {right} steer unless lane keeping is on full; steer "
        ));
        text.push_str(&format!(
            "across the lane line to change lanes. On full, tap {left} or {right}. "
        ));
        text.push_str(&format!(
            "Hold {} to point the truck straight down the road; where it sits in the lane stays yours to fix. ",
            n(Action::Straighten)
        ));
        text.push_str("Exits leave from the right lane. Change lanes or brake means a ");
        text.push_str("fixed object in your lane: take the open lane it names, or brake ");
        text.push_str("nearly to a stop and ease around. ");
        let rest = n(Action::Rest);
        text.push_str(&format!(
            "{rest} plans the recommended break or sleep stop when hours of service planning hints are on; otherwise it plans the next sleep-capable stop while rolling. {exit} "
        ));
        text.push_str(&format!(
            "signals for its exit. Stopped at a route stop, {rest} opens its menu: "
        ));
        text.push_str("fuel, break, sleep, inspect, roadside assistance, or save where ");
        text.push_str(&format!(
            "available. Fully stopped away from route points, {rest} opens the "
        ));
        text.push_str(&format!(
            "emergency shoulder-sleep warning instead. {} horn. ",
            n(Action::Horn)
        ));
        text.push_str(&format!(
            "{} engine brake; on an automatic it manages its own stage, and {}, ",
            n(Action::EngineBrake),
            n(Action::JakeStage1)
        ));
        text.push_str(&format!(
            "{}, {} take manual control. {} chooses whether {} runs the ",
            n(Action::JakeStage2),
            n(Action::JakeStage3),
            n(Action::AutoJake),
            n(Action::EngineBrake)
        ));
        text.push_str(&format!(
            "automatic mode. {} switches between automatic and manual ",
            n(Action::TransmissionMode)
        ));
        text.push_str("shifting. Escape pause menu. F2 lists every driving command by name. ");
        if !self.trip.truck.transmission.automatic {
            text.push_str(&format!(
                "Hold Left Shift for clutch, then {} to shift up or {} to shift down, \
                 {} for reverse, {} for neutral. ",
                n(Action::ShiftUp),
                n(Action::ShiftDown),
                n(Action::Reverse),
                n(Action::Neutral)
            ));
        }
        text.push_str(
            "Any of these keys can be moved under Settings, Gameplay, Controls, \
             Keyboard shortcuts.",
        );
        ctx.say(&text);
    }

    /// `_speak_controller_help()`: controller layout help, spoken from the
    /// Back button or F1 on a pad. Buttons come from the player's own table.
    pub fn speak_controller_help(&mut self, ctx: &mut GameContext) {
        let n = |action: Action| ctx.bindings.pad_spoken(action);
        let manual = !self.trip.truck.transmission.automatic;
        let gears = if manual {
            format!(
                "Hold the left bumper for the clutch; {} shifts up a gear, \
                 {} shifts down. ",
                n(Action::ShiftUp),
                n(Action::ShiftDown)
            )
        } else if ctx.settings.automatic_direction_changes == "deliberate" {
            "In automatic with deliberate direction changes, stop, let the left \
             trigger return to neutral, then press and hold it again to reverse. \
             While reversing, stop with the right trigger, let it return to \
             neutral, then press and hold again for forward. A quick tap just \
             brakes. "
                .to_string()
        } else {
            "In automatic with simple direction changes, stop, let the left \
             trigger return to neutral, then press and hold it again to reverse. \
             While reversing, stop with the right trigger, release, then press it \
             again for forward. A brake held through a stop just holds the truck. "
                .to_string()
        };
        let objective_help = self.objective_help(ctx);

        let mut text = String::new();
        text.push_str("Right trigger is the gas, left trigger the brake; the left trigger ");
        text.push_str("fully in is the hardest stop. The left stick steers unless lane ");
        text.push_str("keeping is on full. ");
        text.push_str(&gears);
        text.push_str(&format!(
            "{} starts automatic speed control: adaptive cruise on ",
            capitalized(&n(Action::Cruise))
        ));
        text.push_str(&format!(
            "open roads, the speed keeper in low-speed zones. {} or {} change the open-road ",
            capitalized(&n(Action::CruiseDown)),
            n(Action::CruiseUp)
        ));
        text.push_str("target by five. It pauses through the planned pickup and resumes ");
        text.push_str("once the loaded truck is rolling. Parked with the brake set, ");
        text.push_str(&format!("{} latches a high idle. ", n(Action::Cruise)));
        text.push_str(&format!(
            "{} signals for the next announced exit, or a pull-over ",
            capitalized(&n(Action::TakeExit))
        ));
        text.push_str("when a trooper lights you up. ");
        text.push_str(&format!(
            "{} reads your route and current location, {} the ",
            capitalized(&n(Action::Route)),
            n(Action::Weather)
        ));
        text.push_str(&format!(
            "weather, {} the clock with your full hours of service. ",
            n(Action::Clock)
        ));
        text.push_str(&format!(
            "{} speaks your speed. ",
            capitalized(&n(Action::Speed))
        ));
        text.push_str(&format!(
            "{} sounds the horn, {} the engine brake. ",
            capitalized(&n(Action::Horn)),
            n(Action::EngineBrake)
        ));
        text.push_str(&format!(
            "{} starts or stops the engine, {} reads fuel, {} reads the posted ",
            capitalized(&n(Action::Engine)),
            n(Action::Fuel),
            n(Action::SpeedLimit)
        ));
        text.push_str(&format!(
            "speed limit here and how far over you are, {} sets or releases the parking ",
            n(Action::ParkingBrake)
        ));
        text.push_str(&format!(
            "brake, {} plans a recommended break or sleep stop when hours of service planning hints are on, or the next sleep stop otherwise; stopped at a stop, it opens its actions. ",
            n(Action::Rest)
        ));
        text.push_str("Away from route points while fully stopped, it opens ");
        text.push_str(&format!(
            "emergency shoulder sleep. {} opens the status menu. ",
            capitalized(&n(Action::Status))
        ));
        text.push_str("Start pauses and unpauses. The Back button stops the driving voice ");
        text.push_str("while it is speaking; when nothing is being said, it repeats this help. ");
        text.push_str(&objective_help);
        text.push_str(
            "Any of these buttons can be moved under Settings, Gameplay, Controls, \
             Controller buttons.",
        );
        ctx.say(&text);
    }
}

impl DrivingState {
    /// The touch-screen layout, from the player's own touch bindings.
    pub fn speak_touch_help(&mut self, ctx: &mut GameContext) {
        // The core rule comes first; everything after it is an example.
        let mut text = String::from(
            "A flick is one step. A swipe and hold is continuous. Everything works anywhere \
             on the screen. Swipe up and hold for gas; lift to coast. Swipe down and hold to \
             brake. Swipe down three times as deep for the emergency brake; stopped, that \
             sets the parking brake, and gas releases it. ",
        );
        text.push_str(if ctx.settings.lane_is_automated() {
            "Flick left or right to change lanes; with lane keeping on full, flicks are the \
             only lane control. "
        } else {
            "Flick left or right to change lanes, or swipe left or right and hold to steer. "
        });
        text.push_str(
            "Hold one finger still for half a second to straighten the wheel. With cruise \
             off, flick up to resume the last speed and flick down to set the current speed. \
             Turn two fingers right like a key to start the engine, left to stop it. Hold two \
             fingers for the horn; turning them cancels it. While holding a pedal, a second \
             finger's flicks and holds work the same way, and in automatic its shift flicks \
             say the gear. ",
        );
        for gesture in crate::bindings::touch_slots() {
            let (Some(name), Some(command)) = (
                crate::bindings::touch_gesture_name(gesture),
                ctx.bindings.touch_command(gesture),
            ) else {
                continue;
            };
            if command != crate::bindings::TouchCommand::Nothing {
                text.push_str(&format!("{name}: {}. ", command.label()));
            }
        }
        text.push_str(
            "Two-finger swipe down pauses, two-finger swipe up is this help, two-finger \
             swipes left and right review messages, and three-finger swipes left and right \
             tune the radio. A three-finger tap lists every driving command: swipe to one \
             and double tap to run it. ",
        );
        text.push_str(&self.objective_help(ctx));
        text.push_str("Gestures can be moved in Settings, Gameplay, Controls, Touch gestures.");
        ctx.say(&text);
    }
}

/// "the A button" at the start of a sentence.
fn capitalized(phrase: &str) -> String {
    let mut chars = phrase.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}
