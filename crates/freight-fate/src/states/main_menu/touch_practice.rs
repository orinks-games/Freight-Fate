//! The gesture half of practice: what each iPhone and iPad gesture is
//! named. `InputPracticeState` is the one practice screen; it hands every
//! gesture here.

use crate::app::{GameContext, Say};
use crate::bindings::{touch_gesture_name, Action};
use crate::impl_state_for_menu;
use crate::states::base::{Menu, MenuCore, MenuItem};
use crate::touch::Gesture;

use super::InputPracticeState;

/// Leaving practice by touch: two back gestures in a row, with no clock. A
/// scrub is decided as the fingers lift and can take a second by itself, so
/// a time limit only makes the second one miss.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TouchLeave {
    #[default]
    Idle,
    /// One back gesture, and nothing since.
    Armed,
    Leave,
}

/// What happened since the last back gesture.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TouchLeaveEvent {
    Back,
    /// Any other gesture, key or button.
    Other,
}

impl TouchLeave {
    pub fn next(self, event: TouchLeaveEvent) -> Self {
        match (self, event) {
            (Self::Armed, TouchLeaveEvent::Back) => Self::Leave,
            (_, TouchLeaveEvent::Back) => Self::Armed,
            (_, TouchLeaveEvent::Other) => Self::Idle,
        }
    }
}

#[derive(Default)]
pub(super) struct GesturePractice {
    leave: TouchLeave,
    /// The swipe down being held, so its length can be read as it lifts.
    swipe_down: Option<Gesture>,
}

impl GesturePractice {
    /// Any key or button between two back gestures starts the count over.
    pub(super) fn other_input(&mut self) {
        self.leave = self.leave.next(TouchLeaveEvent::Other);
    }

    pub(super) fn leave_state(&self) -> TouchLeave {
        self.leave
    }

    /// Names one gesture; true once the second back gesture in a row asks
    /// to leave.
    pub(super) fn handle(&mut self, ctx: &mut GameContext, gesture: Gesture) -> bool {
        if matches!(gesture, Gesture::Escape | Gesture::TwoFingerSwipeDown) {
            self.leave = self.leave.next(TouchLeaveEvent::Back);
            if self.leave == TouchLeave::Leave {
                self.leave = TouchLeave::Idle;
                ctx.say("Leaving gesture practice.");
                return true;
            }
            ctx.say("Again to leave practice.");
        } else if let Gesture::Travel(points) = gesture {
            self.say_length(ctx, points);
        } else {
            if !matches!(
                gesture,
                Gesture::KeyboardConnected | Gesture::KeyboardDisconnected
            ) {
                self.leave = self.leave.next(TouchLeaveEvent::Other);
            }
            self.swipe_down = match gesture {
                Gesture::HoldLowerBegan | Gesture::DeepSwipeDownBegan => Some(gesture),
                Gesture::HoldUpperBegan
                | Gesture::HoldLeftBegan
                | Gesture::HoldRightBegan
                | Gesture::StillHoldBegan
                | Gesture::HornHoldBegan => None,
                // A second finger's gesture during the hold keeps it.
                _ => self.swipe_down,
            };
            self.say_gesture(ctx, gesture);
        }
        false
    }

    /// The length of a swipe down as it lifts, and where the deep swipe
    /// starts. Other holds have no length that changes what they do.
    fn say_length(&mut self, ctx: &mut GameContext, points: u16) {
        let text = match self.swipe_down.take() {
            Some(Gesture::DeepSwipeDownBegan) => format!("Deep swipe. Length {points}."),
            Some(_) => format!(
                "Length {points}. The deep swipe starts at {}.",
                ctx.settings.touch_deep_swipe_points()
            ),
            None => return,
        };
        // Queued: a quick hold and lift must not cut off the hold's name.
        ctx.say_with(text, Say::queued());
    }

    fn say_gesture(&self, ctx: &mut GameContext, gesture: Gesture) {
        let text = match gesture {
            Gesture::HoldUpperBegan => "Swipe up and hold: gas. Lift to coast.".to_string(),
            Gesture::HoldLowerBegan => "Swipe down and hold: brake.".to_string(),
            Gesture::HoldLeftBegan => "Swipe left and hold: steer left.".to_string(),
            Gesture::HoldRightBegan => "Swipe right and hold: steer right.".to_string(),
            Gesture::DeepSwipeDownBegan => {
                "Deep swipe down: emergency brake, or the parking brake when stopped.".to_string()
            }
            Gesture::StillHoldBegan => "Hold still: straighten the wheel.".to_string(),
            Gesture::EmergencyBrakeHoldBegan => "Emergency brake.".to_string(),
            Gesture::HornHoldBegan => "Two-finger hold: horn.".to_string(),
            Gesture::RotateRight => "Two-finger turn right: start the engine.".to_string(),
            Gesture::RotateLeft => {
                "Two-finger turn left: stop the engine when the truck is stopped.".to_string()
            }
            Gesture::SecondSteerLeftBegan => "Second finger, hold left: steer left.".to_string(),
            Gesture::SecondSteerRightBegan => "Second finger, hold right: steer right.".to_string(),
            Gesture::HoldEnded
            | Gesture::SecondSteerEnded
            | Gesture::KeyboardConnected
            | Gesture::KeyboardDisconnected
            | Gesture::Travel(_) => return,
            other => match ctx.bindings.touch_command(other) {
                Some(command) => format!(
                    "{}: {}.",
                    touch_gesture_name(other).unwrap_or("Gesture"),
                    command.label()
                ),
                None => format!("{}: {}.", fixed_name(other), fixed_binding(ctx, other)),
            },
        };
        ctx.say(&text);
    }
}

pub struct TouchPracticeOfferState {
    menu: MenuCore<Self>,
}

impl TouchPracticeOfferState {
    pub fn new() -> Self {
        Self {
            menu: MenuCore::new("Practice touch gestures?"),
        }
    }
}

impl Default for TouchPracticeOfferState {
    fn default() -> Self {
        Self::new()
    }
}

impl Menu for TouchPracticeOfferState {
    fn menu(&self) -> &MenuCore<Self> {
        &self.menu
    }

    fn menu_mut(&mut self) -> &mut MenuCore<Self> {
        &mut self.menu
    }

    fn announce_entry(&mut self, ctx: &mut GameContext) {
        // Queued, so the departure line spoken just before is not cut off.
        let text = format!(
            "Before your first drive, practice the touch gestures? Practice names each \
             gesture without moving the truck. {}",
            self.current_text(ctx)
        );
        ctx.say_with(text, Say::queued());
    }

    fn build_items(&mut self, _ctx: &mut GameContext) -> Vec<MenuItem<Self>> {
        vec![
            MenuItem::new("Practice gestures", |_s: &mut Self, ctx| {
                ctx.pop_state();
                ctx.push_state(InputPracticeState::gestures());
            }),
            MenuItem::new("Skip and start driving", |s: &mut Self, ctx| s.go_back(ctx)).goes_back(),
        ]
    }
}

impl_state_for_menu!(TouchPracticeOfferState);

fn fixed_name(gesture: Gesture) -> &'static str {
    match gesture {
        Gesture::Tap => "Tap",
        Gesture::DoubleTap | Gesture::Activate => "Double tap",
        Gesture::SwipeUp => "Flick up",
        Gesture::SwipeDown => "Flick down",
        Gesture::Increment => "Swipe up",
        Gesture::Decrement => "Swipe down",
        Gesture::SwipeLeft => "Flick left",
        Gesture::SwipeRight => "Flick right",
        Gesture::TwoFingerTap => "Two-finger tap",
        Gesture::MagicTap => "Two-finger double tap",
        Gesture::TwoFingerSwipeUp => "Two-finger flick up",
        Gesture::TwoFingerSwipeLeft => "Two-finger flick left",
        Gesture::TwoFingerSwipeRight => "Two-finger flick right",
        Gesture::ThreeFingerTap => "Three-finger tap",
        Gesture::ThreeFingerSwipeUp => "Three-finger flick up",
        Gesture::ThreeFingerSwipeDown => "Three-finger flick down",
        Gesture::ThreeFingerSwipeLeft => "Three-finger flick left",
        Gesture::ThreeFingerSwipeRight => "Three-finger flick right",
        _ => "Gesture",
    }
}

fn fixed_binding(ctx: &GameContext, gesture: Gesture) -> String {
    match gesture {
        Gesture::HoldUpperBegan => ctx.control_name(Action::Accelerate),
        Gesture::HoldLowerBegan => ctx.control_name(Action::Brake),
        Gesture::EmergencyBrakeHoldBegan => ctx.control_name(Action::EmergencyBrake),
        Gesture::HornHoldBegan => ctx.control_name(Action::Horn),
        Gesture::MagicTap => "pause".to_string(),
        other => fixed_command(other).to_string(),
    }
}

/// What a gesture that cannot be moved does, in the touch help's words.
fn fixed_command(gesture: Gesture) -> &'static str {
    match gesture {
        Gesture::DoubleTap | Gesture::Activate => "Take an exit, pull over or confirm arrival",
        Gesture::TwoFingerTap => "Stop speech",
        Gesture::SwipeLeft => "Change lanes left",
        Gesture::SwipeRight => "Change lanes right",
        Gesture::Increment => "Move up",
        Gesture::Decrement => "Move down",
        Gesture::TwoFingerSwipeUp => "Help",
        Gesture::TwoFingerSwipeLeft => "Previous message",
        Gesture::TwoFingerSwipeRight => "Next message",
        Gesture::ThreeFingerSwipeLeft => "Previous radio station",
        Gesture::ThreeFingerSwipeRight => "Next radio station",
        Gesture::ThreeFingerTap => "Driving command list",
        _ => "No command",
    }
}

#[cfg(test)]
mod tests {
    use super::{TouchLeave, TouchLeaveEvent};

    #[test]
    fn touch_leave_transitions() {
        use TouchLeave::*;
        use TouchLeaveEvent::*;
        let table = [
            (Idle, Back, Armed),
            (Idle, Other, Idle),
            (Armed, Back, Leave),
            (Armed, Other, Idle),
            (Leave, Back, Armed),
            (Leave, Other, Idle),
        ];
        for (from, event, to) in table {
            assert_eq!(from.next(event), to, "{from:?} + {event:?}");
        }
    }
}
