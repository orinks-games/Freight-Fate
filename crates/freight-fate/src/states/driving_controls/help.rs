//! F1: the driving layout, spoken for whichever device is in use.
//!
//! The help texts are long and player-facing word for word. Each spoken
//! sentence is one whole literal, joined with spaces, so the translation
//! catalog can carry every sentence as a unit instead of loose fragments.

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
        let emergency_brake = n(Action::EmergencyBrake);
        let cruise = n(Action::Cruise);
        let cruise_resume = n(Action::CruiseResume);
        let exit = n(Action::TakeExit);
        let clock = n(Action::Clock);
        let radio = n(Action::Radio);
        let favorite = n(Action::RadioFavorite);
        let radio_status = n(Action::RadioStatus);
        let now_playing = n(Action::RadioNowPlaying);
        let status = n(Action::Status);
        let engine = n(Action::Engine);
        let parking_brake = n(Action::ParkingBrake);
        let speed = n(Action::Speed);
        let speed_limit = n(Action::SpeedLimit);
        let grade = n(Action::Grade);
        let fuel = n(Action::Fuel);
        let hos_wheel = n(Action::HosWheel);
        let hos_break = n(Action::HosBreak);
        let hos_drive = n(Action::HosDrive);
        let route = n(Action::Route);
        let place_state = n(Action::PlaceState);
        let place_road = n(Action::PlaceRoad);
        let place_town = n(Action::PlaceTown);
        let place_direction = n(Action::PlaceDirection);
        let weather = n(Action::Weather);
        let lane = n(Action::Lane);
        let lane_locator = n(Action::LaneLocator);
        let last_announcement = n(Action::LastAnnouncement);
        let cb = n(Action::Cb);
        let copy_last = n(Action::CopyLastSpoken);
        let upcoming = n(Action::Upcoming);
        let safe_speed = n(Action::SafeSpeed);
        let left = n(Action::SteerLeft);
        let right = n(Action::SteerRight);
        let straighten = n(Action::Straighten);
        let rest = n(Action::Rest);
        let horn = n(Action::Horn);
        let engine_brake = n(Action::EngineBrake);
        let jake1 = n(Action::JakeStage1);
        let jake2 = n(Action::JakeStage2);
        let jake3 = n(Action::JakeStage3);
        let auto_jake = n(Action::AutoJake);
        let transmission_mode = n(Action::TransmissionMode);
        let window = n(Action::DriverWindow);
        let fan = n(Action::Fan);
        let climate = n(Action::Climate);

        // Each spoken sentence is one whole literal, so a catalog can
        // translate it as a unit. Sentences are joined with a space.
        let mut lines: Vec<String> = Vec::new();
        lines.push(format!("Hold {up} to accelerate, {down} to brake."));
        if ctx.settings.pedal_latch != "off" {
            lines.push(
                "Tap the brake, then press again and hold half a second to latch it \
                 hands-free; a click and a spoken confirmation mark the catch."
                    .to_string(),
            );
            lines.push(format!(
                "{down} once releases it; the accelerator releases it instantly."
            ));
            lines.push("The throttle key never latches.".to_string());
        }
        if ctx.settings.automatic_direction_changes == "deliberate" {
            lines.push(format!(
                "In automatic with deliberate direction changes, stop, release \
                 {down}, then press and hold it again to reverse."
            ));
            lines.push(format!(
                "While reversing, stop with {up}, release, then press and hold \
                 again for forward."
            ));
            lines.push("A quick tap just brakes.".to_string());
        } else {
            lines.push(format!(
                "In automatic with simple direction changes, stop, release {down}, \
                 then press and hold it again to reverse."
            ));
            lines.push(format!(
                "While reversing, stop with {up}, release, then hold it again for forward."
            ));
            lines.push("A brake held through a stop just holds the truck.".to_string());
        }
        lines.push(format!(
            "Hold {emergency_brake} for the emergency brake, the hardest possible stop."
        ));
        lines.push(format!(
            "{cruise} starts automatic speed control: adaptive cruise on open roads, \
             the speed keeper in low-speed zones."
        ));
        lines.push("Weather widens the gap.".to_string());
        lines.push(
            "Cruise eases early for a sharp posted-limit drop, the keeper for the next \
             turn or the next lower limit."
                .to_string(),
        );
        lines.push("Braking cancels the session.".to_string());
        lines.push("At the planned pickup it pauses and resumes once you depart.".to_string());
        lines.push(
            "Plus and minus, including the keypad keys, change the open-road target by \
             five; it never holds more than five over the posted limit."
                .to_string(),
        );
        lines.push("Control with plus or minus, by one.".to_string());
        lines.push(format!("{cruise_resume} resumes the last cruise speed."));
        lines.push(format!(
            "Parked with the brake set, {cruise} latches a high idle; plus and minus \
             adjust it, and releasing the parking brake drops it."
        ));
        lines.push(format!(
            "{exit} signals for the next announced route exit, by number when known, \
             or cancels that signal."
        ));
        lines.push(
            "Unless lane keeping is on full, move to the right lane, then steer right \
             into the exit lane where it opens, just before the gore."
                .to_string(),
        );
        lines.push(
            "Keep road speed to there, then brake to the exit speed before the ramp curve."
                .to_string(),
        );
        lines.push(
            "Ramps usually end at a traffic light or stop sign, called out on the way down."
                .to_string(),
        );
        lines.push(format!(
            "{exit} also signals a pull-over when a trooper lights you up for speeding, \
             a scale bypass, or unsafe equipment: signal, then brake to a stop."
        ));
        lines.push(
            "Ignoring the lights brings failure-to-stop warnings, then a stop that \
             cancels the load: a major offense."
                .to_string(),
        );
        lines.push(format!("{clock} also speaks the date and season."));
        lines.push(format!("{radio} toggles the in-cab radio."));
        lines.push(
            "Page Down tunes to the next station, Page Up to the previous; semicolon \
             and apostrophe do the same."
                .to_string(),
        );
        lines.push(
            "Control with the tuning keys jumps a category; Shift changes the radio \
             volume in 10 percent steps, on or off."
                .to_string(),
        );
        lines.push(format!(
            "{favorite} saves or unsaves the station as a favorite."
        ));
        lines.push(format!(
            "{radio_status} speaks station, volume, and streamer-safe status; \
             {now_playing} speaks the song when the station says."
        ));
        lines.push(format!(
            "The Driver apps tablet has a Radio app to search the dial, tune by name, \
             and keep favorites; the {status} status menu has a radio screen of \
             receivable stations."
        ));
        lines.push(format!(
            "{engine} starts the engine, and stops it only below 5 miles per hour."
        ));
        lines.push("Air pressure must build before the truck can move.".to_string());
        lines.push(format!(
            "{parking_brake} sets or releases the parking brake."
        ));
        lines.push("It needs 100 psi of air.".to_string());
        lines.push(self.objective_help(ctx).trim_end().to_string());
        lines.push(format!(
            "{speed} speed, active speed-control mode, and target, and with the signal \
             set, how far to the exit."
        ));
        lines.push(format!("{speed_limit} posted speed limit."));
        lines.push(format!(
            "{grade} the grade under the wheels, whether the truck is holding it, and \
             the next grade ahead."
        ));
        lines.push(format!("{status} status menu."));
        lines.push(format!("{fuel} fuel."));
        lines.push(format!(
            "{clock} clock, deadline, and the hours limit that comes first."
        ));
        lines.push(format!(
            "{hos_wheel} time at the wheel so far, {hos_break} when your 30 minute \
             break is due, {hos_drive} what ends this shift and where you can legally \
             stop before it."
        ));
        lines.push(format!(
            "{route} progress, distance left, and where you are."
        ));
        lines.push(format!(
            "{place_state} the state, {place_road} the road, {place_town} the town or \
             the nearest one, {place_direction} the direction."
        ));
        lines.push("The keypad numbers work the same way.".to_string());
        lines.push(format!("{weather} weather."));
        lines.push(format!(
            "{lane} lane position and whether the lane beside you is open."
        ));
        lines.push(format!(
            "{lane_locator} turns the lane locator on and off: a soft tock once a beat, \
             panned to where you sit in your lane, on lane keeping partial or off."
        ));
        lines.push(format!(
            "{last_announcement} repeats the last driving announcement."
        ));
        lines.push(format!(
            "{cb} repeats the last CB chatter, with the distance as it is now."
        ));
        lines.push(
            "Comma repeats what was just said and keeps stepping back; Period moves \
             forward again."
                .to_string(),
        );
        lines.push(
            "Control with Comma or Period jumps to the oldest or newest message.".to_string(),
        );
        lines.push(
            "The bracket keys switch between all messages, general messages, and driving \
             events."
                .to_string(),
        );
        lines.push("Control C copies the message you are on.".to_string());
        lines.push(format!("{copy_last} copies the last thing the game said."));
        lines.push(format!(
            "{upcoming} reads the road ahead that no other key answers: the exit your \
             signal is on for, the ramp control coming up, the next imposed limit, the \
             next stop, and the next bend that demands slowing."
        ));
        lines.push(format!(
            "Bends that demand slowing are called before they arrive, like Sharp left, \
             half a mile, advise 35; {safe_speed} gives one safe-speed number with the \
             bend in it."
        ));
        lines.push(format!(
            "The {status} status menu includes a Driver apps tablet for navigation, \
             weather, traffic, truck stops, road chatter, and ELD."
        ));
        lines.push("Left or Right Control stops the driving event voice.".to_string());
        lines.push(format!(
            "{left} and {right} steer unless lane keeping is on full; steer across the \
             lane line to change lanes."
        ));
        lines.push(format!("On full, tap {left} or {right}."));
        lines.push(format!(
            "Hold {straighten} to point the truck straight down the road; where it sits \
             in the lane stays yours to fix."
        ));
        lines.push("Exits leave from the right lane.".to_string());
        lines.push(
            "Change lanes or brake means a fixed object in your lane: take the open lane \
             it names, or brake nearly to a stop and ease around."
                .to_string(),
        );
        lines.push(format!(
            "While rolling, {rest} plans the next stop for what you choose: fuel, a break, \
             sleep, fuel and sleep, or food."
        ));
        lines.push(format!("{exit} signals for its exit."));
        lines.push(format!(
            "Stopped at a route stop, {rest} opens its menu: fuel, break, sleep, \
             inspect, roadside assistance, or save where available."
        ));
        lines.push(format!(
            "Fully stopped away from route points, {rest} opens the emergency \
             shoulder-sleep warning instead."
        ));
        lines.push(format!("{horn} horn."));
        lines.push(format!(
            "{window} lowers or raises the driver window, {fan} steps the fan, \
             {climate} switches heat and air conditioning."
        ));
        lines.push(format!(
            "{engine_brake} engine brake; on an automatic it manages its own stage, and \
             {jake1}, {jake2}, {jake3} take manual control."
        ));
        lines.push(format!(
            "{auto_jake} chooses whether {engine_brake} runs the automatic mode."
        ));
        lines.push(format!(
            "{transmission_mode} switches between automatic and manual shifting."
        ));
        lines.push("Escape pause menu.".to_string());
        lines.push("F2 lists every driving command by name.".to_string());
        lines.push("Shift F1 names what each key does.".to_string());
        if !self.trip.truck.transmission.automatic {
            let shift_up = n(Action::ShiftUp);
            let shift_down = n(Action::ShiftDown);
            let reverse = n(Action::Reverse);
            let neutral = n(Action::Neutral);
            lines.push(format!(
                "Hold Left Shift for clutch, then {shift_up} to shift up or {shift_down} \
                 to shift down, {reverse} for reverse, {neutral} for neutral."
            ));
        }
        lines.push(
            "Any of these keys can be moved under Settings, Gameplay, Controls, \
             Keyboard shortcuts."
                .to_string(),
        );
        ctx.say(&sentences(&lines));
    }

    /// `_speak_controller_help()`: controller layout help, spoken from the
    /// Back button or F1 on a pad. Buttons come from the player's own table.
    pub fn speak_controller_help(&mut self, ctx: &mut GameContext) {
        let n = |action: Action| ctx.bindings.pad_spoken(action);
        let c = |action: Action| capitalized(&ctx.bindings.pad_spoken(action));
        let manual = !self.trip.truck.transmission.automatic;
        // Each spoken sentence is one whole literal, so a catalog can
        // translate it as a unit. Sentences are joined with a space.
        let mut lines: Vec<String> = vec![
            "Right trigger is the gas, left trigger the brake; the left trigger fully in \
             is the hardest stop."
                .to_string(),
            "The left stick steers; with lane keeping on full, pushing it left or right changes one lane.".to_string(),
        ];
        if manual {
            let shift_up = n(Action::ShiftUp);
            let shift_down = n(Action::ShiftDown);
            lines.push(format!(
                "Hold the left bumper for the clutch; {shift_up} shifts up a gear, \
                 {shift_down} shifts down."
            ));
        } else if ctx.settings.automatic_direction_changes == "deliberate" {
            lines.push(
                "In automatic with deliberate direction changes, stop, let the left \
                 trigger return to neutral, then press and hold it again to reverse."
                    .to_string(),
            );
            lines.push(
                "While reversing, stop with the right trigger, let it return to \
                 neutral, then press and hold again for forward."
                    .to_string(),
            );
            lines.push("A quick tap just brakes.".to_string());
        } else {
            lines.push(
                "In automatic with simple direction changes, stop, let the left \
                 trigger return to neutral, then press and hold it again to reverse."
                    .to_string(),
            );
            lines.push(
                "While reversing, stop with the right trigger, release, then press it \
                 again for forward."
                    .to_string(),
            );
            lines.push("A brake held through a stop just holds the truck.".to_string());
        }
        let cruise = n(Action::Cruise);
        let cruise_cap = c(Action::Cruise);
        let cruise_down = c(Action::CruiseDown);
        let cruise_up = n(Action::CruiseUp);
        let take_exit = c(Action::TakeExit);
        let route = c(Action::Route);
        let weather = n(Action::Weather);
        let clock = n(Action::Clock);
        let speed = c(Action::Speed);
        let horn = c(Action::Horn);
        let engine_brake = n(Action::EngineBrake);
        let engine = c(Action::Engine);
        let fuel = n(Action::Fuel);
        let speed_limit = n(Action::SpeedLimit);
        let parking_brake = n(Action::ParkingBrake);
        let rest = n(Action::Rest);
        let status = c(Action::Status);
        let tune_prev = c(Action::RadioTunePrev);
        let tune_next = n(Action::RadioTuneNext);
        let volume_down = c(Action::RadioVolumeDown);
        let volume_up = n(Action::RadioVolumeUp);
        let category_prev = c(Action::RadioCategoryPrev);
        let category_next = n(Action::RadioCategoryNext);
        let radio = c(Action::Radio);
        let favorite = n(Action::RadioFavorite);
        let radio_status = n(Action::RadioStatus);
        let now_playing = n(Action::RadioNowPlaying);
        lines.push(format!(
            "{cruise_cap} starts automatic speed control: adaptive cruise on open roads, \
             the speed keeper in low-speed zones."
        ));
        lines.push(format!(
            "{cruise_down} or {cruise_up} change the open-road target by five."
        ));
        lines.push(
            "It pauses through the planned pickup and resumes once the loaded truck is \
             rolling."
                .to_string(),
        );
        lines.push(format!(
            "Parked with the brake set, {cruise} latches a high idle."
        ));
        lines.push(format!(
            "{take_exit} signals for the next announced exit, or a pull-over when a \
             trooper lights you up."
        ));
        lines.push(format!(
            "{route} reads your route and current location, {weather} the weather, \
             {clock} the clock with your full hours of service."
        ));
        lines.push(format!("{speed} speaks your speed."));
        lines.push(format!(
            "{horn} sounds the horn, {engine_brake} the engine brake."
        ));
        lines.push(format!(
            "{engine} starts or stops the engine, {fuel} reads fuel, {speed_limit} reads \
             the posted speed limit here and how far over you are, {parking_brake} sets \
             or releases the parking brake, {rest} plans a stop for fuel, a break, sleep or \
             food while rolling; stopped at a stop, it opens its actions."
        ));
        lines.push(
            "Away from route points while fully stopped, it opens emergency shoulder sleep."
                .to_string(),
        );
        lines.push(format!(
            "{status} opens the status menu, which also lists every driving command."
        ));
        lines.push("Hold Back and press Start to hear what each button does.".to_string());
        lines.push(format!(
            "{tune_prev} tunes to the previous station and {tune_next} to the next."
        ));
        lines.push(format!(
            "{volume_down} and {volume_up} lower and raise the radio volume."
        ));
        lines.push(format!(
            "{category_prev} or {category_next} changes the radio category."
        ));
        lines.push(format!(
            "{radio} turns the radio on or off, {favorite} saves a favorite, \
             {radio_status} reads radio status, and {now_playing} says what is playing."
        ));
        lines.push("Start pauses and unpauses.".to_string());
        lines.push(
            "The Back button stops the driving voice while it is speaking; when nothing \
             is being said, it repeats this help."
                .to_string(),
        );
        lines.push(self.objective_help(ctx).trim_end().to_string());
        lines.push(
            "Any of these buttons can be moved under Settings, Gameplay, Controls, \
             Controller buttons."
                .to_string(),
        );
        ctx.say(&sentences(&lines));
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

/// The help's sentences joined with spaces, each opening with a capital: a
/// key name leads some ("the Left arrow and the Right arrow steer"), and a
/// sentence that starts lowercase is neither good English nor found as its
/// own sentence by a translation.
fn sentences(lines: &[String]) -> String {
    lines
        .iter()
        .map(|line| {
            let mut chars = line.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().chain(chars).collect(),
                None => String::new(),
            }
        })
        .collect::<Vec<String>>()
        .join(" ")
}
