//! Touch gestures at the wheel: a slot gesture runs the command the player's
//! touch bindings give it, with no menu in between. A flick is one step: a
//! side flick is one lane, an up or down flick one cruise step, a key turn
//! one engine start or stop. A double tap answers whatever the road is
//! asking for ([`RoadOffer`]).

use crate::app::GameContext;
use crate::bindings::{Action, TouchCommand};
use crate::states::driving::DrivingState;
use crate::states::driving_updates::live;
use crate::touch::{Gesture, RoadOffer};
use ff_core::touch_mode::RoadFacts;

/// At or under this, the truck has come to rest for the stop's touch line.
const STOP_LINE_STANDSTILL_MPH: f64 = 0.05;

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
                    self.touch_toggle_engine(ctx);
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
                    // The co-driver's truck: the exits and the gate are
                    // theirs, as the exit key is.
                    _ if self.co_driver_driving() => {}
                    RoadOffer::Arrival => self.confirm_facility_arrival(ctx),
                    RoadOffer::Stop => {
                        live::set_stop_offer_open(false);
                        self.run_key_action(ctx, Action::Rest);
                        self.note_instruction_demonstrated(ctx, "rest");
                    }
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
            stop_waiting: self.stop_waiting().is_some(),
            exit_in_window,
        })
    }

    /// Stopped on a stop's ramp, where the rest stop command opens the
    /// stop (`try_rest_stop`'s stopped branch: docking speed, a stop within
    /// a mile and a half) or checks in at the scale. `Some(true)` for a
    /// scale. A truck stopped on the highway near a stop is not waiting at
    /// it: the rest stop command would open it too, but nothing asked.
    pub fn stop_waiting(&self) -> Option<bool> {
        let ramp = self.ramp_stop.as_ref()?;
        if self.pull_over.is_some()
            || self.trip.truck.speed_mph() > crate::states::driving_core::DOCKING_MAX_MPH
        {
            return None;
        }
        self.trip.nearest_stop_within(1.5)?;
        Some(ramp.stop_type == "weigh_station")
    }

    /// On touch, the one line when a double tap starts to mean the stop
    /// the truck is waiting at, until the rest instruction retires.
    pub fn update_touch_stop_offer(&mut self, ctx: &mut GameContext) {
        if self.road_offer(ctx) != RoadOffer::Stop {
            self.stop_offer_line_said = false;
            live::set_stop_offer_open(false);
            return;
        }
        // The offer begins at docking speed, but the line waits for a
        // standstill: rolling the last few feet, the ramp callout is often
        // still speaking and a queued line behind it can be dropped.
        if self.stop_offer_line_said
            || self.trip.truck.speed_mph() > STOP_LINE_STANDSTILL_MPH
            || !ctx.touch_hints()
            || self.instruction_retired(ctx, "rest")
        {
            return;
        }
        self.stop_offer_line_said = true;
        live::set_stop_offer_open(true);
        let line = if self.stop_waiting() == Some(true) {
            "Double tap to check in."
        } else {
            "Double tap to pull in."
        };
        ctx.say_event_with(
            line,
            crate::app::SayEvent::queued()
                .priority(crate::speech::EventPriority::Important)
                .category(ff_core::speech_pacing::SpeechCategory::Navigation)
                .valid(live::stop_offer_open),
        );
    }
}
