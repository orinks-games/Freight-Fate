//! Touch gestures at the wheel: a slot gesture runs the command the player's
//! touch bindings give it, with no menu in between. A flick is one step: a
//! side flick is one lane, an up or down flick one cruise step, a key turn
//! one engine start or stop.

use crate::app::GameContext;
use crate::bindings::{Action, TouchCommand};
use crate::states::driving::DrivingState;
use crate::touch::Gesture;

impl DrivingState {
    /// `true` when `gesture` is a driving slot and ran (or deliberately did
    /// nothing); a fixed gesture is left to the key it stands for.
    ///
    /// Same contract as a key: a gesture is a request, so its answer may cut
    /// the line in progress.
    pub fn handle_touch_gesture(&mut self, ctx: &mut GameContext, gesture: Gesture) -> bool {
        let previous = ctx.player_asked_begin();
        let taken = self.run_touch_gesture(ctx, gesture);
        ctx.player_asked_end(previous);
        taken
    }

    fn run_touch_gesture(&mut self, ctx: &mut GameContext, gesture: Gesture) -> bool {
        match gesture {
            // Gas releases the parking brake, then drives.
            Gesture::HoldUpperBegan => {
                if self.trip.truck.parking_brake {
                    self.toggle_parking_brake(ctx);
                }
                return false;
            }
            // Moving, the deep swipe is the emergency brake the app holds;
            // stopped, it sets the parking brake.
            Gesture::DeepSwipeDownBegan => {
                if self.trip.truck.speed_mph() >= ctx.settings.touch_parking_brake_mph {
                    return false;
                }
                if self.trip.truck.parking_brake {
                    ctx.say("Parking brake set.");
                } else {
                    self.toggle_parking_brake(ctx);
                }
                return true;
            }
            Gesture::RotateRight | Gesture::RotateLeft => {
                let start = gesture == Gesture::RotateRight;
                if self.trip.truck.engine_on == start {
                    ctx.say(if start {
                        "Engine already running."
                    } else {
                        "Engine already off."
                    });
                } else {
                    self.toggle_engine(ctx);
                }
                return true;
            }
            Gesture::SwipeLeft => {
                self.tap_lane_change(ctx, 1);
                return true;
            }
            Gesture::SwipeRight => {
                self.tap_lane_change(ctx, -1);
                return true;
            }
            _ => {}
        }
        let Some(command) = ctx.bindings.touch_command(gesture) else {
            return false;
        };
        // With speed control off, the cruise flicks resume the last speed
        // and set the current one.
        let engaged =
            self.speed_control_armed || self.keeper_mph.is_some() || self.cruise_mph.is_some();
        let command = match (gesture, command) {
            (Gesture::SwipeUp, TouchCommand::Action(Action::CruiseUp)) if !engaged => {
                TouchCommand::Action(Action::CruiseResume)
            }
            (Gesture::SwipeDown, TouchCommand::Action(Action::CruiseDown)) if !engaged => {
                TouchCommand::Action(Action::Cruise)
            }
            _ => command,
        };
        match command {
            TouchCommand::Action(Action::ShiftUp | Action::ShiftDown)
                if self.trip.truck.transmission.automatic =>
            {
                // The automatic picks its own gears; the flick asks which.
                let gear = self.gear_text();
                let mut chars = gear.chars();
                let gear = chars
                    .next()
                    .map(|first| first.to_uppercase().collect::<String>() + chars.as_str())
                    .unwrap_or_default();
                ctx.say(&format!("{gear}, automatic."));
            }
            // A flick is one lane, whatever the lane keeping.
            TouchCommand::Action(Action::SteerLeft) => self.tap_lane_change(ctx, 1),
            TouchCommand::Action(Action::SteerRight) => self.tap_lane_change(ctx, -1),
            TouchCommand::Action(action) => self.run_key_action(ctx, action),
            TouchCommand::Pause => {
                ctx.audio.horn_stop();
                self.trip.truck.horn_on = false;
                self.push_pause_menu(ctx);
            }
            TouchCommand::Nothing => {}
        }
        true
    }
}
