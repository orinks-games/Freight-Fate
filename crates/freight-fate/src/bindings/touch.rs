//! Touch gestures at the wheel: which command each driving gesture runs.
//!
//! A gesture here is a slot holding one command, the way a pad button holds
//! one action, so a direct gesture replaces the trip through the command
//! list. The gestures that are not slots keep their keys at the wheel: the
//! pedal holds, the steering swipes, double tap for Enter, two-finger swipes
//! for pause, help and message review, the three-finger radio dial, the
//! command list and the on-screen keyboard. The settings file stores only
//! the slots the player changed (`tap=fuel;magic_tap=none`).

use std::collections::HashMap;

use super::{entries, Action, KeyBindings};
use crate::touch::Gesture;

/// What a driving gesture does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TouchCommand {
    Action(Action),
    /// The pause menu, which is Escape rather than an [`Action`].
    Pause,
    Nothing,
}

impl TouchCommand {
    pub fn label(self) -> &'static str {
        match self {
            TouchCommand::Action(action) => action.label(),
            TouchCommand::Pause => "Pause",
            TouchCommand::Nothing => "Nothing",
        }
    }

    fn saved(self) -> &'static str {
        match self {
            TouchCommand::Action(action) => action.id(),
            TouchCommand::Pause => "pause",
            TouchCommand::Nothing => "none",
        }
    }

    fn parse(text: &str) -> Option<TouchCommand> {
        match text {
            "pause" => Some(TouchCommand::Pause),
            "none" => Some(TouchCommand::Nothing),
            id => Action::from_id(id)
                .filter(|action| {
                    !action.held() || matches!(action, Action::SteerLeft | Action::SteerRight)
                })
                .map(TouchCommand::Action),
        }
    }

    /// Every command a gesture can run, in the order the picker reads them.
    /// The held controls are left out: a tap cannot hold a pedal.
    pub fn all() -> impl Iterator<Item = TouchCommand> {
        [TouchCommand::Nothing, TouchCommand::Pause]
            .into_iter()
            .chain(
                Action::all()
                    .filter(|action| {
                        !action.held() || matches!(action, Action::SteerLeft | Action::SteerRight)
                    })
                    .map(TouchCommand::Action),
            )
    }
}

const fn run(action: Action) -> TouchCommand {
    TouchCommand::Action(action)
}

/// `(gesture, saved id, spoken name, default command)`, in screen order.
const SLOTS: &[(Gesture, &str, &str, TouchCommand)] = &[
    (Gesture::Tap, "tap", "Tap", run(Action::Speed)),
    (
        Gesture::SwipeUp,
        "swipe_up",
        "Flick up",
        run(Action::CruiseUp),
    ),
    (
        Gesture::SwipeDown,
        "swipe_down",
        "Flick down",
        run(Action::CruiseDown),
    ),
    (
        Gesture::TwoFingerTap,
        "two_finger_tap",
        "Two-finger tap",
        run(Action::Status),
    ),
    (
        Gesture::MagicTap,
        "magic_tap",
        "Two-finger double tap",
        TouchCommand::Pause,
    ),
    (
        Gesture::ThreeFingerSwipeUp,
        "three_finger_swipe_up",
        "Three-finger swipe up",
        run(Action::Route),
    ),
    (
        Gesture::ThreeFingerSwipeDown,
        "three_finger_swipe_down",
        "Three-finger swipe down",
        run(Action::Upcoming),
    ),
    (
        Gesture::UpperHoldTap,
        "top_hold_tap",
        "Gas hold, tap with a second finger",
        run(Action::Cruise),
    ),
    (
        Gesture::UpperHoldDoubleTap,
        "top_hold_double_tap",
        "Gas hold, double tap with a second finger",
        TouchCommand::Nothing,
    ),
    (
        Gesture::UpperHoldSwipeUp,
        "top_hold_swipe_up",
        "Gas hold, flick up with a second finger",
        run(Action::ShiftUp),
    ),
    (
        Gesture::UpperHoldSwipeDown,
        "top_hold_swipe_down",
        "Gas hold, flick down with a second finger",
        run(Action::ShiftDown),
    ),
    (
        Gesture::UpperHoldSwipeLeft,
        "top_hold_swipe_left",
        "Gas hold, flick left with a second finger",
        run(Action::SteerLeft),
    ),
    (
        Gesture::UpperHoldSwipeRight,
        "top_hold_swipe_right",
        "Gas hold, flick right with a second finger",
        run(Action::SteerRight),
    ),
    (
        Gesture::LowerHoldTap,
        "bottom_hold_tap",
        "Brake hold, tap with a second finger",
        run(Action::ParkingBrake),
    ),
    (
        Gesture::LowerHoldDoubleTap,
        "bottom_hold_double_tap",
        "Brake hold, double tap with a second finger",
        run(Action::Engine),
    ),
    (
        Gesture::LowerHoldSwipeUp,
        "bottom_hold_swipe_up",
        "Brake hold, flick up with a second finger",
        run(Action::ShiftUp),
    ),
    (
        Gesture::LowerHoldSwipeDown,
        "bottom_hold_swipe_down",
        "Brake hold, flick down with a second finger",
        run(Action::ShiftDown),
    ),
    (
        Gesture::LowerHoldSwipeLeft,
        "bottom_hold_swipe_left",
        "Brake hold, flick left with a second finger",
        run(Action::SteerLeft),
    ),
    (
        Gesture::LowerHoldSwipeRight,
        "bottom_hold_swipe_right",
        "Brake hold, flick right with a second finger",
        run(Action::SteerRight),
    ),
];

fn slot(gesture: Gesture) -> Option<&'static (Gesture, &'static str, &'static str, TouchCommand)> {
    SLOTS.iter().find(|(g, ..)| *g == gesture)
}

/// Every gesture a driving command can go on, in screen order.
pub fn touch_slots() -> impl Iterator<Item = Gesture> {
    SLOTS.iter().map(|(gesture, ..)| *gesture)
}

/// What the screen reader calls a slot gesture; `None` for a fixed one.
pub fn touch_gesture_name(gesture: Gesture) -> Option<&'static str> {
    slot(gesture).map(|(_, _, name, _)| *name)
}

/// The moved slots a saved `touch_bindings` string describes; unreadable
/// entries are ignored.
pub(super) fn parse_touch(text: &str) -> HashMap<Gesture, TouchCommand> {
    let mut out = HashMap::new();
    for (id, value) in entries(text) {
        let gesture = SLOTS.iter().find(|(_, i, ..)| *i == id).map(|(g, ..)| *g);
        if let (Some(gesture), Some(command)) = (gesture, TouchCommand::parse(value)) {
            out.insert(gesture, command);
        }
    }
    out
}

pub(super) fn saved_touch(moved: &HashMap<Gesture, TouchCommand>) -> String {
    SLOTS
        .iter()
        .filter_map(|(gesture, id, ..)| {
            moved
                .get(gesture)
                .map(|command| format!("{id}={}", command.saved()))
        })
        .collect::<Vec<_>>()
        .join(";")
}

/// A slot gesture as a noun, to follow a verb in a spoken prompt ("press a
/// two-finger tap"); `None` for a fixed one.
pub fn touch_gesture_noun(gesture: Gesture) -> Option<String> {
    let held =
        |pedal: &str, motion: &str| Some(format!("a second-finger {motion} while holding {pedal}"));
    let plain = |noun: &str| Some(noun.to_string());
    match gesture {
        Gesture::Tap => plain("a tap"),
        Gesture::SwipeUp => plain("a flick up"),
        Gesture::SwipeDown => plain("a flick down"),
        Gesture::TwoFingerTap => plain("a two-finger tap"),
        Gesture::MagicTap => plain("a two-finger double tap"),
        Gesture::ThreeFingerSwipeUp => plain("a three-finger swipe up"),
        Gesture::ThreeFingerSwipeDown => plain("a three-finger swipe down"),
        Gesture::UpperHoldTap => held("gas", "tap"),
        Gesture::UpperHoldDoubleTap => held("gas", "double tap"),
        Gesture::UpperHoldSwipeUp => held("gas", "flick up"),
        Gesture::UpperHoldSwipeDown => held("gas", "flick down"),
        Gesture::UpperHoldSwipeLeft => held("gas", "flick left"),
        Gesture::UpperHoldSwipeRight => held("gas", "flick right"),
        Gesture::LowerHoldTap => held("brake", "tap"),
        Gesture::LowerHoldDoubleTap => held("brake", "double tap"),
        Gesture::LowerHoldSwipeUp => held("brake", "flick up"),
        Gesture::LowerHoldSwipeDown => held("brake", "flick down"),
        Gesture::LowerHoldSwipeLeft => held("brake", "flick left"),
        Gesture::LowerHoldSwipeRight => held("brake", "flick right"),
        _ => None,
    }
}

impl KeyBindings {
    /// How a touch player runs `action`, as a noun for a spoken prompt:
    /// the first gesture it is on, else the fixed pedal and steering
    /// gestures, else its row on the driving command list.
    pub fn touch_noun(&self, action: Action) -> String {
        if action == Action::Engine {
            return "a two-finger turn to the right".to_string();
        }
        let on_gesture = SLOTS
            .iter()
            .map(|(gesture, ..)| *gesture)
            .find(|gesture| self.touch_command(*gesture) == Some(TouchCommand::Action(action)))
            .and_then(touch_gesture_noun);
        if let Some(noun) = on_gesture {
            return noun;
        }
        match action {
            Action::Accelerate => "a swipe up and hold".to_string(),
            Action::Brake => "a swipe down and hold".to_string(),
            Action::EmergencyBrake => "a deep swipe down".to_string(),
            Action::Horn => "a two-finger hold".to_string(),
            Action::SteerLeft => "a flick left".to_string(),
            Action::SteerRight => "a flick right".to_string(),
            Action::Straighten => "a still hold for half a second".to_string(),
            Action::TakeExit => "the exit command from the three-finger tap list".to_string(),
            other => format!("{} from the three-finger tap list", other.label()),
        }
    }

    /// The touch phrase for a `control_hint` action id; `None` leaves the
    /// fixed table's wording (the pedal holds and fixed gestures).
    pub fn hint_touch_phrase(&self, hint: &str) -> Option<String> {
        Some(match hint {
            "gears" => format!(
                "{} and {}",
                self.touch_noun(Action::ShiftUp),
                self.touch_noun(Action::ShiftDown)
            ),
            "gear_first" => self.touch_noun(Action::ShiftUp),
            other => {
                let action = super::hint_action(other)?;
                if action.held() {
                    return None;
                }
                self.touch_noun(action)
            }
        })
    }

    /// The command a slot gesture runs today; `None` for a fixed gesture,
    /// which keeps its key.
    pub fn touch_command(&self, gesture: Gesture) -> Option<TouchCommand> {
        let (_, _, _, default) = slot(gesture)?;
        Some(self.touch.get(&gesture).copied().unwrap_or(*default))
    }

    /// Put `command` on `gesture`. Several gestures may run one command.
    pub fn set_touch_command(&mut self, gesture: Gesture, command: TouchCommand) {
        let Some((_, _, _, default)) = slot(gesture) else {
            return;
        };
        if *default == command {
            self.touch.remove(&gesture);
        } else {
            self.touch.insert(gesture, command);
        }
    }

    pub fn reset_touch(&mut self) {
        self.touch.clear();
    }

    /// The first gesture that runs `action`, as the screen reader says it.
    pub fn touch_spoken(&self, action: Action) -> Option<&'static str> {
        SLOTS
            .iter()
            .find(|(gesture, ..)| {
                self.touch_command(*gesture) == Some(TouchCommand::Action(action))
            })
            .map(|(_, _, name, _)| *name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ff_core::settings::Settings;

    #[test]
    fn touch_hints_name_the_gesture_or_the_command_list() {
        let mut b = KeyBindings::default();
        assert_eq!(
            b.hint_touch_phrase("engine").as_deref(),
            Some("a two-finger turn to the right")
        );
        assert_eq!(b.hint_touch_phrase("accelerate"), None);
        assert_eq!(
            b.hint_touch_phrase("take_exit").as_deref(),
            Some("the exit command from the three-finger tap list")
        );
        assert_eq!(
            b.touch_noun(Action::Rest),
            "Rest stop from the three-finger tap list"
        );
        b.set_touch_command(Gesture::UpperHoldDoubleTap, run(Action::TakeExit));
        assert_eq!(
            b.hint_touch_phrase("take_exit").as_deref(),
            Some("a second-finger double tap while holding gas")
        );
    }

    #[test]
    fn the_defaults_are_the_agreed_layout() {
        let b = KeyBindings::default();
        let on = |g| b.touch_command(g);
        assert_eq!(on(Gesture::UpperHoldTap), Some(run(Action::Cruise)));
        assert_eq!(on(Gesture::LowerHoldTap), Some(run(Action::ParkingBrake)));
        assert_eq!(on(Gesture::LowerHoldDoubleTap), Some(run(Action::Engine)));
        assert_eq!(on(Gesture::UpperHoldSwipeUp), Some(run(Action::ShiftUp)));
        assert_eq!(
            on(Gesture::UpperHoldSwipeDown),
            Some(run(Action::ShiftDown))
        );
        assert_eq!(
            on(Gesture::UpperHoldSwipeLeft),
            Some(run(Action::SteerLeft))
        );
        assert_eq!(
            on(Gesture::UpperHoldSwipeRight),
            Some(run(Action::SteerRight))
        );
        assert_eq!(on(Gesture::MagicTap), Some(TouchCommand::Pause));
        assert_eq!(on(Gesture::Tap), Some(run(Action::Speed)));
        assert_eq!(on(Gesture::SwipeUp), Some(run(Action::CruiseUp)));
        assert_eq!(on(Gesture::TwoFingerTap), Some(run(Action::Status)));
    }

    #[test]
    fn fixed_gestures_are_not_slots() {
        let b = KeyBindings::default();
        for gesture in [
            Gesture::DoubleTap,
            Gesture::SwipeLeft,
            Gesture::SwipeRight,
            Gesture::TwoFingerSwipeDown,
            Gesture::TwoFingerSwipeUp,
            Gesture::TwoFingerSwipeLeft,
            Gesture::ThreeFingerSwipeLeft,
            Gesture::ThreeFingerTap,
            Gesture::HoldUpperBegan,
            Gesture::HoldEnded,
            Gesture::Escape,
            Gesture::Activate,
        ] {
            assert_eq!(b.touch_command(gesture), None, "{gesture:?}");
            assert_eq!(touch_gesture_name(gesture), None, "{gesture:?}");
        }
    }

    #[test]
    fn no_default_is_a_held_control() {
        for (gesture, _, _, command) in SLOTS {
            if let TouchCommand::Action(action) = command {
                assert!(
                    !action.held() || matches!(action, Action::SteerLeft | Action::SteerRight),
                    "{gesture:?}"
                );
            }
            assert!(TouchCommand::all().any(|c| c == *command), "{gesture:?}");
        }
    }

    #[test]
    fn moved_gestures_round_trip_through_settings() {
        let mut b = KeyBindings::default();
        b.set_touch_command(Gesture::Tap, run(Action::Fuel));
        b.set_touch_command(Gesture::MagicTap, TouchCommand::Nothing);
        b.set_touch_command(Gesture::LowerHoldSwipeUp, TouchCommand::Pause);
        let mut settings = Settings::default();
        b.store(&mut settings);
        assert_eq!(
            settings.touch_bindings,
            "tap=fuel;magic_tap=none;bottom_hold_swipe_up=pause"
        );
        assert_eq!(KeyBindings::from_settings(&settings), b);
        // Back to the default clears the override.
        b.set_touch_command(Gesture::Tap, run(Action::Speed));
        b.set_touch_command(Gesture::MagicTap, TouchCommand::Pause);
        b.reset_touch();
        assert!(b.is_default());
    }

    #[test]
    fn held_controls_and_garbage_are_ignored_on_load() {
        let settings = Settings {
            touch_bindings: "tap=accelerate;swipe_up=teleport;wiggle=fuel;;two_finger_tap=fuel"
                .into(),
            ..Settings::default()
        };
        let b = KeyBindings::from_settings(&settings);
        assert_eq!(b.touch_command(Gesture::Tap), Some(run(Action::Speed)));
        assert_eq!(
            b.touch_command(Gesture::SwipeUp),
            Some(run(Action::CruiseUp))
        );
        assert_eq!(
            b.touch_command(Gesture::TwoFingerTap),
            Some(run(Action::Fuel))
        );
    }

    #[test]
    fn a_moved_command_names_its_new_gesture() {
        let mut b = KeyBindings::default();
        assert_eq!(
            b.touch_spoken(Action::Cruise),
            Some("Gas hold, tap with a second finger")
        );
        b.set_touch_command(Gesture::UpperHoldTap, TouchCommand::Nothing);
        assert_eq!(b.touch_spoken(Action::Cruise), None);
        b.set_touch_command(Gesture::Tap, run(Action::Cruise));
        assert_eq!(b.touch_spoken(Action::Cruise), Some("Tap"));
    }
}
