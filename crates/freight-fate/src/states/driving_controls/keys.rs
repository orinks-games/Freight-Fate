//! `handle_event` / `_handle_key`: the whole discrete keyboard table at the
//! wheel, plus the assist-off tap lane change the arrows fall back to.

use crate::app::GameContext;
use crate::bindings::Action;
use crate::states::base::{InputEvent, Key};
use crate::states::driving::DrivingState;
use crate::states::driving_core::*;

impl DrivingState {
    /// `handle_event(event)`.
    ///
    /// Everything spoken from here down is an answer to a key the player
    /// pressed, so it may cut the line in progress even though unasked-for
    /// lines queue at the wheel. See `GameContext::player_asked`.
    pub fn handle_key_event(&mut self, ctx: &mut GameContext, event: &InputEvent) {
        let previous = ctx.player_asked_begin();
        self.handle_key(ctx, event);
        ctx.player_asked_end(previous);
    }

    /// `_handle_key(event)`: the table itself.
    ///
    /// Every key first resolves to the [`Action`] the player has it on
    /// (`ctx.bindings`), so a moved shortcut lands here without the table
    /// knowing. The fixed keys -- Control to stop the voice, Escape to pause,
    /// F2 for the command list, plus and minus, the radio dial, Enter, F1 --
    /// are matched on the key itself, the way they always were. Three orderings are load-bearing and
    /// kept: a chord is tried before the bare key (so Alt with a number reads
    /// a place instead of changing the engine brake), `+`/`-` fall back to the
    /// typed character, and the radio dial reads Ctrl before Shift so
    /// `Ctrl+Shift` still jumps a category.
    pub(crate) fn handle_key(&mut self, ctx: &mut GameContext, event: &InputEvent) {
        if let InputEvent::KeyUp { key, .. } = event {
            // Modifiers are ignored on the way up: a horn on Alt H must stop
            // whichever of the two keys lifts first.
            if ctx
                .bindings
                .chords(Action::Horn)
                .iter()
                .any(|c| c.key == *key)
            {
                ctx.audio.horn_stop();
                self.trip.truck.horn_on = false;
            }
            return;
        }
        let Some((key, mods, text)) = event.key_down() else {
            return;
        };
        // OS key-repeat must not walk the cruise dial: a held Plus used to
        // race the open-road target (and the live adaptive-cruise set speed)
        // to the ceiling in about a second and a half. Discrete presses keep
        // the old +5 / Ctrl+1 step; repeats are ignored.
        let repeat = event.key_repeat();

        let automatic = self.trip.truck.transmission.automatic;
        if !automatic && mods.shift {
            self.trip.truck.transmission.clutch = 1.0;
        }
        let plus = matches!(key, Key::Equals | Key::Plus | Key::KpPlus) || text == Some('+');
        let minus = matches!(key, Key::Minus | Key::KpMinus) || text == Some('-');

        if matches!(key, Key::LCtrl | Key::RCtrl) {
            ctx.stop_event_speech();
            self.warnings_stopped_by_player(ctx);
            self.set_status("Event voice stopped.");
            return;
        }
        if key == Key::Escape {
            ctx.audio.horn_stop();
            self.trip.truck.horn_on = false;
            self.push_pause_menu(ctx);
            return;
        }
        if key == Key::F2 {
            self.push_driving_commands(ctx);
            return;
        }
        if let Some(action) = ctx.bindings.action_for(key, mods) {
            if repeat && matches!(action, Action::CruiseUp | Action::CruiseDown) {
                return;
            }
            // The co-driver's truck: a control is a no-op, a question
            // still gets its answer.
            if self.co_driver_driving() && !co_driver_leaves_to_player(action) {
                return;
            }
            self.run_key_action(ctx, action);
            return;
        }
        if self.co_driver_driving() && (plus || minus || matches!(key, Key::Return | Key::KpEnter))
        {
            return;
        }
        if plus {
            if !repeat {
                self.adjust_cruise(ctx, 1, mods.ctrl);
            }
        } else if minus {
            if !repeat {
                self.adjust_cruise(ctx, -1, mods.ctrl);
            }
        } else if matches!(key, Key::Return | Key::KpEnter) {
            if self.assisted_facility_confirmation_ready(ctx) {
                self.open_ready_facility_arrival(ctx);
            }
        } else if matches!(key, Key::PageUp | Key::Semicolon) {
            // Page Down walks to the next station, Page Up to the previous,
            // matching the help browser's Page Up and Page Down paging; with
            // Ctrl they leap a whole category (25 AFN stations in a row buried
            // terrestrial for a linear tune). Semicolon and apostrophe stay as
            // secondary dial keys: Page keys are Fn chords on many laptops and
            // missing on 60 percent keyboards, which is also why the dial is
            // not on the shortcuts screen -- one chosen key would drop the
            // fallbacks. The dial originally lived on the brackets, which
            // message review now uses to switch categories. Shift raises or
            // lowers the radio volume instead of tuning (Jerry's request) --
            // checked only when Ctrl is absent, so Ctrl+Shift still falls
            // through to Ctrl's own category jump exactly as it did before
            // Shift existed.
            if mods.ctrl {
                self.jump_radio_category(ctx, -1);
            } else if mods.shift {
                self.adjust_radio_volume(ctx, 1);
            } else {
                self.tune_radio(ctx, -1);
            }
        } else if matches!(key, Key::PageDown | Key::Quote) {
            if mods.ctrl {
                self.jump_radio_category(ctx, 1);
            } else if mods.shift {
                self.adjust_radio_volume(ctx, -1);
            } else {
                self.tune_radio(ctx, 1);
            }
        } else if key == Key::F1 {
            self.speak_driving_help(ctx);
        }
    }

    /// One keyboard action, whatever key it arrived on.
    ///
    /// The held controls (pedals, steering, the emergency brake, the
    /// trooper run) are polled each frame through the same table and do
    /// nothing here; steering's tap is the exception, because with lane
    /// keeping on full a tap of the steering key changes lanes.
    pub(crate) fn run_key_action(&mut self, ctx: &mut GameContext, action: Action) {
        let automatic = self.trip.truck.transmission.automatic;
        match action {
            Action::Engine => self.toggle_engine(ctx),
            Action::Neutral => {
                if automatic {
                    return;
                }
                let result = self.trip.truck.transmission.request_gear(0);
                if result.ok {
                    ctx.audio
                        .play_bank("vehicle/shift_manual", "vehicle/gear_shift");
                    ctx.say("Neutral.");
                }
            }
            Action::Reverse => {
                if !automatic {
                    self.manual_shift(ctx, REVERSE);
                }
            }
            Action::ShiftUp => {
                if automatic {
                    return;
                }
                let tr = &self.trip.truck.transmission;
                if tr.in_reverse() || tr.in_neutral() {
                    self.manual_shift(ctx, 1);
                } else if tr.gear < 10 {
                    let next = tr.gear + 1;
                    self.manual_shift(ctx, next);
                }
            }
            Action::ShiftDown => {
                let tr = &self.trip.truck.transmission;
                if !automatic && !tr.in_neutral() && tr.gear > 1 {
                    let next = tr.gear - 1;
                    self.manual_shift(ctx, next);
                }
            }
            Action::EngineBrake => self.toggle_engine_brake(ctx),
            Action::AutoJake => self.toggle_auto_jake_enabled(ctx),
            Action::PlaceState => self.speak_current_state(ctx),
            Action::PlaceRoad => self.speak_current_road(ctx),
            Action::PlaceTown => self.speak_current_town(ctx),
            Action::PlaceDirection => self.speak_current_direction(ctx),
            Action::JakeStage1 => self.select_jake_stage(ctx, 1),
            Action::JakeStage2 => self.select_jake_stage(ctx, 2),
            Action::JakeStage3 => self.select_jake_stage(ctx, 3),
            Action::CycleJake => self.cycle_jake_stage(ctx),
            Action::ParkingBrake => self.toggle_parking_brake(ctx),
            Action::DriverWindow => self.toggle_driver_window(ctx),
            Action::Fan => self.step_cab_fan(ctx),
            Action::Climate => self.toggle_cab_climate(ctx),
            Action::Horn => {
                ctx.audio.horn_start();
                self.trip.truck.horn_on = true;
                self.horn_scare_animals(ctx);
            }
            Action::TransmissionMode => {
                // The AMT's manual-mode button: flips the transmission
                // setting; the existing manual shift controls take over.
                ctx.settings.automatic_transmission = !ctx.settings.automatic_transmission;
            }
            Action::Rest => {
                if self.manual_facility_arrival_ready(ctx) {
                    self.open_ready_facility_arrival(ctx);
                } else {
                    self.try_rest_stop(ctx);
                }
            }
            Action::TakeExit => {
                if self.pull_over.is_some() {
                    self.signal_pull_over(ctx);
                } else {
                    self.take_exit(ctx);
                }
            }
            Action::Cruise => self.toggle_cruise(ctx),
            Action::CruiseResume => self.resume_cruise(ctx),
            Action::CruiseUp => self.adjust_cruise(ctx, 1, false),
            Action::CruiseDown => self.adjust_cruise(ctx, -1, false),
            Action::SteerLeft => {
                if ctx.settings.lane_is_automated() {
                    self.tap_lane_change(ctx, 1);
                }
            }
            Action::SteerRight => {
                if ctx.settings.lane_is_automated() {
                    self.tap_lane_change(ctx, -1);
                }
            }
            Action::Speed => self.speak_speed(ctx),
            Action::Status => self.push_driving_status(ctx),
            Action::Fuel => self.speak_fuel(ctx),
            // C for the CB, on the Alt layer that already answers one
            // narrow question at a time (Alt A/S/D hours, Alt 1 to 4
            // place). Plain C stays the clock, the same way plain S, D,
            // A, J and T keep theirs.
            Action::Cb => self.speak_last_cb_chatter(ctx),
            Action::Clock => self.speak_clock(ctx, false),
            // Shift+R used to read the next listed exit. Removed 2026-08-17:
            // the exit list is reference material the drive never asks the
            // player to act on, and it stays reachable on the status screen,
            // which is where reference material belongs. R answers the same
            // thing shifted or not, so a stray Shift is not silence.
            Action::Route => self.speak_route_status(ctx),
            Action::Weather => self.speak_weather(ctx),
            Action::Lane => {
                let text = self.lane_status_text();
                ctx.say(&text);
            }
            Action::HosBreak => self.speak_hos_break(ctx),
            Action::SpeedLimit => self.speak_speed_limit(ctx),
            Action::HosDrive => self.speak_hos_drive_left(ctx),
            Action::SafeSpeed => self.speak_safe_speed(ctx),
            Action::HosWheel => self.speak_hos_wheel_time(ctx),
            Action::LastAnnouncement => self.speak_last_announcement(ctx),
            // The key itself is answered on every screen before a state
            // sees it; this arm is the F2 list's row.
            Action::CopyLastSpoken => crate::states::base::copy_last_spoken(ctx),
            Action::Grade => self.speak_grade(ctx),
            Action::LaneLocator => self.toggle_lane_locator(ctx),
            Action::Upcoming => self.speak_upcoming(ctx, 15.0),
            Action::Radio => self.toggle_radio(ctx),
            Action::RadioFavorite => self.toggle_radio_favorite(ctx),
            Action::RadioTuneNext
            | Action::RadioTunePrev
            | Action::RadioVolumeUp
            | Action::RadioVolumeDown
            | Action::RadioCategoryNext
            | Action::RadioCategoryPrev => {}
            Action::RadioNowPlaying => self.speak_radio_now_playing(ctx),
            Action::RadioStatus => self.speak_radio_status(ctx),
            Action::Accelerate | Action::Brake | Action::EmergencyBrake | Action::Straighten => {}
        }
    }

    /// `_tap_lane_change(direction)`.
    ///
    /// Assist-off lane change: a timed drift across the line, +1 moves left,
    /// -1 moves right. With steering assist on, the held wheel does this
    /// instead, so the tap handler never runs there.
    pub fn tap_lane_change(&mut self, ctx: &mut GameContext, direction: i64) {
        if self.microsleep_deadline.is_some() {
            return; // the held-key wake-up check owns the arrows right now
        }
        if self.ramp_mi.is_some() {
            ctx.say("On the exit ramp. No lanes to change.");
            return;
        }
        if self.lane_change_target.is_some() {
            ctx.say("Still changing lanes.");
            return;
        }
        if !self.trip.truck.engine_on || self.trip.truck.speed_mph() < LANE_MIN_MPH {
            let minimum = ctx.settings.speed_text(LANE_MIN_MPH);
            ctx.say(&format!(
                "Lane changes need the engine running and at least {minimum}."
            ));
            return;
        }
        let lane_count = self.lane.lane_count;
        let target = self.lane.lane + direction;
        if !(0..lane_count).contains(&target) {
            // Answer the side that was asked for. Naming the lane the driver
            // is already in ("you are already in the right lane") is no answer
            // at all to someone asking to go left.
            let side = if direction > 0 { "left" } else { "right" };
            ctx.say(&format!("No lane to your {side} here."));
            return;
        }
        // The taper counts: that is where the lane is closing, and letting a
        // driver move into it there is how they ended up inside the cones.
        // Asked of the trip so a jam laid over the roadwork cannot hide the
        // closure, and so the answer follows a road that widens or narrows.
        if Some(target) == self.trip.closed_lane_at(None, Some(lane_count)) {
            let closure = self.trip.active_closure(None);
            let name = lane_label(target, lane_count);
            let closing = closure.is_some_and(|zone| zone.reason != "construction");
            ctx.audio.play("ui/error");
            if closing {
                ctx.say(&format!("The {name} lane closes at the work zone ahead."));
            } else {
                ctx.say(&format!("The {name} lane is closed here."));
            }
            return;
        }
        // Lane keeping on full makes the move, so it checks the lane first,
        // the same reading the L key gives. A tap back right while lane
        // keeping was still passing a trooper put the truck into its side
        // (issue #356); the move waits for the lane to open instead.
        if let Some(blocked) = self.lane_blocked_text(target) {
            ctx.audio.play("ui/error");
            ctx.say(&blocked);
            return;
        }
        self.lane_change_target = Some(target);
        self.pass_returning = false; // the driver's own move now
        self.keeping_lane_change = false;
        self.lane_change_timer = LANE_TAP_CHANGE_S;
        self.lane_signal_timer = 0.0;
        let pan = if direction > 0 { -0.6 } else { 0.6 };
        if !self.exit_blinker_on() {
            ctx.audio.play_if_idle("vehicle/turn_signal", 0.8, pan);
        }
        ctx.say(&format!(
            "Changing to the {} lane.",
            lane_label(target, lane_count)
        ));
    }
}

/// The keys that still work while the co-driver drives: the readouts, the
/// screens, and the radio. Everything that moves the truck is theirs.
pub fn co_driver_leaves_to_player(action: Action) -> bool {
    !matches!(
        action,
        Action::Accelerate
            | Action::Brake
            | Action::EmergencyBrake
            | Action::SteerLeft
            | Action::SteerRight
            | Action::Straighten
            | Action::Engine
            | Action::ParkingBrake
            | Action::Horn
            | Action::Cruise
            | Action::CruiseResume
            | Action::CruiseUp
            | Action::CruiseDown
            | Action::TakeExit
            | Action::Rest
            | Action::EngineBrake
            | Action::AutoJake
            | Action::JakeStage1
            | Action::JakeStage2
            | Action::JakeStage3
            | Action::CycleJake
            | Action::ShiftUp
            | Action::ShiftDown
            | Action::Neutral
            | Action::Reverse
            | Action::TransmissionMode
    )
}
