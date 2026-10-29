//! The pad at the wheel: the plain button layer, the right-bumper modified
//! layer, and what happens when the controller is unplugged mid-drive.

use crate::app::GameContext;
use crate::bindings::Action;
use crate::controller::ControllerButton;
use crate::states::base::InputEvent;
use crate::states::driving::DrivingState;

/// How far the left stick goes over to ask for a lane change with lane
/// keeping on full, and how near the center it must return before the next.
const STICK_LANE_PUSH: f64 = 0.5;
const STICK_LANE_RESET: f64 = 0.25;

impl DrivingState {
    /// `handle_controller(event, manager)`.
    ///
    /// Same contract as the keyboard: a pad button is a request too, and the
    /// pad is the device where not being able to cut speech hurt most.
    pub fn handle_controller_event(&mut self, ctx: &mut GameContext, event: &InputEvent) {
        let previous = ctx.player_asked_begin();
        self.handle_controller_button(ctx, event);
        ctx.player_asked_end(previous);
    }

    /// `_handle_controller_button(event, manager)`.
    ///
    /// The button resolves to the [`Action`] the player has it on, plain or
    /// with the right bumper held. Two things stay on their buttons whatever
    /// the table says: Start pauses, and Back stops the event voice while it
    /// is speaking and reads help when it is not. The A button also confirms
    /// an arrival when one is ready, before whatever else it does, the way
    /// Enter does on the keyboard.
    pub(crate) fn handle_controller_button(&mut self, ctx: &mut GameContext, event: &InputEvent) {
        let button = match event {
            InputEvent::ControllerButtonUp { button, .. } => {
                if ctx
                    .bindings
                    .pad_action_for(*button, ctx.controller.modifier)
                    == Some(Action::Horn)
                    || ctx
                        .bindings
                        .pad_action_for(*button, !ctx.controller.modifier)
                        == Some(Action::Horn)
                {
                    ctx.audio.horn_stop();
                }
                self.trip.truck.horn_on = false; // release the horn button to stop it
                return;
            }
            InputEvent::ControllerButtonDown { button, .. } => *button,
            _ => return,
        };
        let modified = ctx.controller.modifier;
        if !modified {
            match button {
                ControllerButton::Start => {
                    ctx.audio.horn_stop();
                    self.trip.truck.horn_on = false;
                    self.push_pause_menu(ctx);
                    return;
                }
                ControllerButton::Back => {
                    // The pad had no way to stop the event voice at all -- every
                    // other button is bound, and Ctrl is a keyboard key -- so a
                    // controller-only driver had to reach for the keyboard to
                    // silence an announcement (Sarah R., 2026-08-16). Back stops
                    // it while it is speaking and keeps reading help when it is
                    // not: pressing Back mid-flood used to answer a driver who
                    // wanted quiet with a paragraph of help.
                    if ctx.event_voice_busy() {
                        ctx.stop_event_speech();
                        self.warnings_stopped_by_player(ctx);
                        self.set_status("Event voice stopped.");
                    } else {
                        self.speak_controller_help(ctx);
                    }
                    return;
                }
                ControllerButton::A
                    if self.assisted_facility_confirmation_ready(ctx)
                        && !self.co_driver_driving() =>
                {
                    self.open_ready_facility_arrival(ctx);
                    return;
                }
                _ => {}
            }
        }
        let Some(action) = ctx.bindings.pad_action_for(button, modified) else {
            return;
        };
        if self.co_driver_driving()
            && !crate::states::driving_controls::keys::co_driver_leaves_to_player(action)
        {
            return;
        }
        self.run_pad_action(ctx, action);
    }

    /// One pad action, whatever button it arrived on.
    fn run_pad_action(&mut self, ctx: &mut GameContext, action: Action) {
        match action {
            Action::ShiftUp => self.shift_relative(ctx, 1),
            Action::ShiftDown => self.shift_relative(ctx, -1),
            Action::Speed => self.speak_speed(ctx),
            Action::Cruise => self.toggle_cruise(ctx),
            Action::CruiseResume => self.resume_cruise(ctx),
            Action::Horn => {
                ctx.audio.horn_start();
                self.trip.truck.horn_on = true;
                self.horn_scare_animals(ctx);
            }
            Action::EngineBrake => self.toggle_engine_brake(ctx),
            Action::AutoJake => self.toggle_auto_jake_enabled(ctx),
            Action::Route => self.speak_route_status(ctx),
            Action::TakeExit => {
                if self.pull_over.is_some() {
                    self.signal_pull_over(ctx);
                } else {
                    self.take_exit(ctx);
                }
            }
            Action::Weather => self.speak_weather(ctx),
            // A pad has no room for the three keyboard hours keys, so this one
            // keeps the whole hours-of-service report it always spoke.
            Action::Clock => self.speak_clock(ctx, true),
            Action::Rest => {
                if self.manual_facility_arrival_ready(ctx) {
                    self.open_ready_facility_arrival(ctx);
                } else {
                    self.try_rest_stop(ctx);
                }
            }
            Action::CruiseDown => self.adjust_cruise(ctx, -1, false),
            Action::CruiseUp => self.adjust_cruise(ctx, 1, false),
            Action::Engine => self.toggle_engine(ctx),
            Action::Fuel => self.speak_fuel(ctx),
            // The pad had no answer to "what is the limit here" at all, so a
            // controller-only driver had to reach for the keyboard's S to ask
            // the one question enforcement acts on (Sarah R., 2026-08-16).
            Action::SpeedLimit => self.speak_speed_limit(ctx),
            Action::ParkingBrake => self.toggle_parking_brake(ctx),
            Action::DriverWindow => self.toggle_driver_window(ctx),
            Action::Fan => self.step_cab_fan(ctx),
            Action::Climate => self.toggle_cab_climate(ctx),
            Action::CycleJake => self.cycle_jake_stage(ctx),
            Action::JakeStage1 => self.select_jake_stage(ctx, 1),
            Action::JakeStage2 => self.select_jake_stage(ctx, 2),
            Action::JakeStage3 => self.select_jake_stage(ctx, 3),
            Action::Status => self.push_driving_status(ctx),
            Action::SafeSpeed => self.speak_safe_speed(ctx),
            Action::Lane => {
                let text = self.lane_status_text();
                ctx.say(&text);
            }
            Action::LaneLocator => self.toggle_lane_locator(ctx),
            Action::Grade => self.speak_grade(ctx),
            Action::Upcoming => self.speak_upcoming(ctx, 15.0),
            Action::LastAnnouncement => self.speak_last_announcement(ctx),
            Action::CopyLastSpoken => crate::states::base::copy_last_spoken(ctx),
            Action::Cb => self.speak_last_cb_chatter(ctx),
            Action::HosWheel => self.speak_hos_wheel_time(ctx),
            Action::HosBreak => self.speak_hos_break(ctx),
            Action::HosDrive => self.speak_hos_drive_left(ctx),
            Action::PlaceState => self.speak_current_state(ctx),
            Action::PlaceRoad => self.speak_current_road(ctx),
            Action::PlaceTown => self.speak_current_town(ctx),
            Action::PlaceDirection => self.speak_current_direction(ctx),
            Action::Neutral | Action::Reverse | Action::TransmissionMode => {}
            Action::Radio => self.toggle_radio(ctx),
            Action::RadioFavorite => self.toggle_radio_favorite(ctx),
            Action::RadioTuneNext => self.tune_radio(ctx, 1),
            Action::RadioTunePrev => self.tune_radio(ctx, -1),
            Action::RadioVolumeUp => self.adjust_radio_volume(ctx, 1),
            Action::RadioVolumeDown => self.adjust_radio_volume(ctx, -1),
            Action::RadioCategoryNext => self.jump_radio_category(ctx, 1),
            Action::RadioCategoryPrev => self.jump_radio_category(ctx, -1),
            Action::RadioStatus => self.speak_radio_status(ctx),
            Action::RadioNowPlaying => self.speak_radio_now_playing(ctx),
            Action::Accelerate
            | Action::Brake
            | Action::EmergencyBrake
            | Action::SteerLeft
            | Action::SteerRight
            | Action::Straighten => {}
        }
    }

    /// Lane keeping on full holds the lane, so the left stick does not steer;
    /// pushing it past halfway asks for one lane change that way, as a tap
    /// of an arrow key does. It has to come back near the center before it
    /// asks again, so a stick held over changes one lane, not every lane.
    pub fn update_stick_lane_change(&mut self, ctx: &mut GameContext, stick: f64) {
        if stick.abs() < STICK_LANE_RESET {
            self.stick_lane_request = 0;
            return;
        }
        if stick.abs() < STICK_LANE_PUSH || self.stick_lane_request != 0 {
            return;
        }
        let side: i8 = if stick < 0.0 { -1 } else { 1 };
        self.stick_lane_request = side;
        // +1 moves left, the way the Left arrow asks.
        self.tap_lane_change(ctx, -i64::from(side));
    }

    /// `on_controller_disconnect()`.
    ///
    /// Pause so an unplugged pad mid-drive does not leave the truck rolling.
    pub fn handle_controller_disconnect(&mut self, ctx: &mut GameContext) {
        ctx.audio.horn_stop();
        self.trip.truck.horn_on = false;
        self.push_pause_menu(ctx);
    }
}
