//! Touch gestures at the wheel: a slot gesture runs the command the player's
//! touch bindings give it, with no menu in between. A flick is one step: a
//! side flick is one lane, an up or down flick one cruise step, a key turn
//! one engine start or stop. A double tap answers whatever the road is
//! asking for ([`RoadOffer`]).

use crate::app::GameContext;
use crate::bindings::{Action, TouchCommand};
use crate::states::driving::DrivingState;
use crate::touch::{Gesture, RoadOffer};
use ff_core::touch_mode::RoadFacts;

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
        if self.co_driver_driving()
            && matches!(
                gesture,
                Gesture::HoldUpperBegan
                    | Gesture::DeepSwipeDownBegan
                    | Gesture::RotateRight
                    | Gesture::RotateLeft
                    | Gesture::SwipeLeft
                    | Gesture::SwipeRight
            )
        {
            return true;
        }
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
                // The engine stops only with the truck stopped. Moving, the
                // turn is most likely fingers that slipped on a horn hold.
                if !start
                    && self.trip.truck.engine_on
                    && self.trip.truck.speed_mph() >= ctx.settings.touch_parking_brake_mph
                {
                    return true;
                }
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
            Gesture::DoubleTap => {
                match self.road_offer(ctx) {
                    // The co-driver's truck: the gate is theirs to stop at.
                    RoadOffer::Arrival if self.co_driver_driving() => {}
                    RoadOffer::Arrival => self.confirm_facility_arrival(ctx),
                    RoadOffer::Exit | RoadOffer::PullOver | RoadOffer::None => {
                        self.run_key_action(ctx, Action::TakeExit)
                    }
                }
                return true;
            }
            _ => {}
        }
        let Some(command) = ctx.bindings.touch_command(gesture) else {
            return false;
        };
        // With speed control off, flick up turns it on (at the remembered
        // speed, else this one) and flick down does nothing: a brake swipe
        // that lifts early reads as a flick down, and must not set cruise.
        let engaged =
            self.speed_control_armed || self.keeper_mph.is_some() || self.cruise_mph.is_some();
        let command = match (gesture, command) {
            (Gesture::SwipeUp, TouchCommand::Action(Action::CruiseUp)) if !engaged => {
                if self.resume_target_mph.is_some() {
                    TouchCommand::Action(Action::CruiseResume)
                } else {
                    TouchCommand::Action(Action::Cruise)
                }
            }
            (Gesture::SwipeDown, TouchCommand::Action(Action::CruiseDown)) if !engaged => {
                TouchCommand::Nothing
            }
            _ => command,
        };
        if let TouchCommand::Action(action) = command {
            if self.co_driver_driving()
                && !crate::states::driving_controls::keys::co_driver_leaves_to_player(action)
            {
                return true;
            }
        }
        match command {
            // The automatic picks its own gears, so a shift flick does
            // nothing and says nothing.
            TouchCommand::Action(Action::ShiftUp | Action::ShiftDown)
                if self.trip.truck.transmission.automatic => {}
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

    /// What a double tap answers right now, read off the gate, a trooper's
    /// pull-over and the exit window.
    pub fn road_offer(&self, ctx: &GameContext) -> RoadOffer {
        let window = self.exit_window_mi();
        let exit_in_window = self.ramp_mi.is_none()
            && (self.exit_signal_on
                || self.exit_stop.is_some()
                || self.trip.upcoming_stop(window).is_some()
                || self.destination_exit_response_s > 0.0);
        RoadOffer::from_road(RoadFacts {
            gate_asking: self.arrival_gate_query_text(ctx).is_some(),
            pull_over: self.pull_over.is_some(),
            exit_in_window,
        })
    }
}
