//! Touch gestures at the wheel: which command each driving gesture runs.
//!
//! A gesture here is a slot holding one command, the way a pad button holds
//! one action, so a direct gesture replaces the trip through the command
//! list. The gestures that are not slots keep their meaning at the wheel:
//! the pedal holds, the steering swipes, double tap for whatever the road is
//! asking, two-finger tap to stop speech, two-finger swipes for pause, help
//! and message review, the three-finger radio dial, the command list and the
//! on-screen keyboard. The settings file stores only the slots the player
//! changed (`tap=fuel;magic_tap=none`); an entry for a gesture that is no
//! longer a slot is ignored on load.

use std::collections::HashMap;

use super::{entries, Action, KeyBindings};
use crate::touch::Gesture;

/// What a driving gesture does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TouchCommand {
    Action(Action),
    /// The pause menu, which is Escape rather than an [`Action`].
    Pause,
    /// The grouped driving command list the three-finger tap opens, for a
    /// player whose three-finger tap lands as one finger.
    CommandList,
    Nothing,
}

impl TouchCommand {
    pub fn label(self) -> &'static str {
        match self {
            TouchCommand::Action(action) => action.label(),
            TouchCommand::Pause => "Pause",
            TouchCommand::CommandList => "Driving command list",
            TouchCommand::Nothing => "Nothing",
        }
    }

    fn saved(self) -> &'static str {
        match self {
            TouchCommand::Action(action) => action.id(),
            TouchCommand::Pause => "pause",
            TouchCommand::CommandList => "command_list",
            TouchCommand::Nothing => "none",
        }
    }

    fn parse(text: &str) -> Option<TouchCommand> {
        match text {
            "pause" => Some(TouchCommand::Pause),
            "command_list" => Some(TouchCommand::CommandList),
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
        [
            TouchCommand::Nothing,
            TouchCommand::Pause,
            TouchCommand::CommandList,
        ]
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
        Gesture::MagicTap,
        "magic_tap",
        "Two-finger double tap",
        TouchCommand::Pause,
    ),
    (
        Gesture::ThreeFingerSwipeUp,
        "three_finger_swipe_up",
        "Three-finger flick up",
        run(Action::Route),
    ),
    (
        Gesture::ThreeFingerSwipeDown,
        "three_finger_swipe_down",
        "Three-finger flick down",
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
        TouchCommand::Nothing,
    ),
    (
        Gesture::LowerHoldDoubleTap,
        "bottom_hold_double_tap",
        "Brake hold, double tap with a second finger",
        TouchCommand::Nothing,
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
        Gesture::MagicTap => plain("a two-finger double tap"),
        Gesture::ThreeFingerSwipeUp => plain("a three-finger flick up"),
        Gesture::ThreeFingerSwipeDown => plain("a three-finger flick down"),
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

/// A slot gesture as an instruction that opens a sentence ("Tap", "Hold
/// gas and tap with a second finger"); `None` for a fixed one.
pub fn touch_gesture_instruction(gesture: Gesture) -> Option<&'static str> {
    Some(match gesture {
        Gesture::Tap => "Tap",
        Gesture::SwipeUp => "Flick up",
        Gesture::SwipeDown => "Flick down",
        Gesture::MagicTap => "Double tap with two fingers",
        Gesture::ThreeFingerSwipeUp => "Flick up with three fingers",
        Gesture::ThreeFingerSwipeDown => "Flick down with three fingers",
        Gesture::UpperHoldTap => "Hold gas and tap with a second finger",
        Gesture::UpperHoldDoubleTap => "Hold gas and double tap with a second finger",
        Gesture::UpperHoldSwipeUp => "Hold gas and flick up with a second finger",
        Gesture::UpperHoldSwipeDown => "Hold gas and flick down with a second finger",
        Gesture::UpperHoldSwipeLeft => "Hold gas and flick left with a second finger",
        Gesture::UpperHoldSwipeRight => "Hold gas and flick right with a second finger",
        Gesture::LowerHoldTap => "Hold the brake and tap with a second finger",
        Gesture::LowerHoldDoubleTap => "Hold the brake and double tap with a second finger",
        Gesture::LowerHoldSwipeUp => "Hold the brake and flick up with a second finger",
        Gesture::LowerHoldSwipeDown => "Hold the brake and flick down with a second finger",
        Gesture::LowerHoldSwipeLeft => "Hold the brake and flick left with a second finger",
        Gesture::LowerHoldSwipeRight => "Hold the brake and flick right with a second finger",
        _ => return None,
    })
}

/// Where a touch name looks for a command first.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Reach {
    /// The plain gesture first: the fixed wheel gestures, then a one-finger
    /// or multi-finger slot, then a second-finger slot.
    Plain,
    /// A pedal is held, so a second-finger slot comes before the rest.
    PedalHeld,
}

/// Whether a slot gesture needs a pedal held.
pub fn touch_second_finger(gesture: Gesture) -> bool {
    matches!(
        gesture,
        Gesture::UpperHoldTap
            | Gesture::UpperHoldDoubleTap
            | Gesture::UpperHoldSwipeUp
            | Gesture::UpperHoldSwipeDown
            | Gesture::UpperHoldSwipeLeft
            | Gesture::UpperHoldSwipeRight
            | Gesture::LowerHoldTap
            | Gesture::LowerHoldDoubleTap
            | Gesture::LowerHoldSwipeUp
            | Gesture::LowerHoldSwipeDown
            | Gesture::LowerHoldSwipeLeft
            | Gesture::LowerHoldSwipeRight
    )
}

/// The fixed wheel gesture for `action`, as `(instruction, noun)`.
/// `cruise_flick` is whether flick up still holds Raise the cruise target,
/// which is what turns cruise on from off.
fn fixed_gesture(action: Action, cruise_flick: bool) -> Option<(&'static str, &'static str)> {
    Some(match action {
        Action::Engine => ("Turn two fingers right", "a two-finger turn to the right"),
        Action::Accelerate => ("Swipe up and hold", "a swipe up and hold"),
        Action::Brake => ("Swipe down and hold", "a swipe down and hold"),
        Action::EmergencyBrake => ("Swipe down deep and hold", "a deep swipe down"),
        Action::Horn => ("Hold two fingers", "a two-finger hold"),
        Action::SteerLeft => ("Flick left", "a flick left"),
        Action::SteerRight => ("Flick right", "a flick right"),
        Action::Straighten => (
            "Hold one finger still for half a second",
            "a still hold for half a second",
        ),
        Action::TakeExit => ("Double tap", "a double tap"),
        Action::Status => (
            "Tap with three fingers, then double tap",
            "a three-finger tap then a double tap",
        ),
        Action::Cruise | Action::CruiseResume if cruise_flick => ("Flick up", "a flick up"),
        _ => return None,
    })
}

impl KeyBindings {
    /// The slot gesture that runs `action`, searched in `reach` order.
    fn slot_for(&self, action: Action, reach: Reach) -> Option<Gesture> {
        let on =
            |gesture: &Gesture| self.touch_command(*gesture) == Some(TouchCommand::Action(action));
        let slots = || SLOTS.iter().map(|(gesture, ..)| *gesture);
        let plain = slots().find(|g| !touch_second_finger(*g) && on(g));
        let held = slots().find(|g| touch_second_finger(*g) && on(g));
        match reach {
            Reach::Plain => plain.or(held),
            Reach::PedalHeld => held.or(plain),
        }
    }

    fn cruise_flick(&self) -> bool {
        self.touch_command(Gesture::SwipeUp) == Some(TouchCommand::Action(Action::CruiseUp))
    }

    fn instruction_in(&self, action: Action, reach: Reach) -> Option<&'static str> {
        let fixed = fixed_gesture(action, self.cruise_flick()).map(|(instruction, _)| instruction);
        let slot = || {
            self.slot_for(action, reach)
                .and_then(touch_gesture_instruction)
        };
        match reach {
            Reach::Plain => fixed.or_else(slot),
            Reach::PedalHeld => slot().or(fixed),
        }
    }

    fn noun_in(&self, action: Action, reach: Reach) -> Option<String> {
        let fixed = fixed_gesture(action, self.cruise_flick()).map(|(_, noun)| noun.to_string());
        let slot = || self.slot_for(action, reach).and_then(touch_gesture_noun);
        match reach {
            Reach::Plain => fixed.or_else(slot),
            Reach::PedalHeld => slot().or(fixed),
        }
    }

    /// How a touch player runs `action`, as an instruction that opens a
    /// sentence. The plain gesture comes first: the fixed wheel gesture
    /// (flick left or right for a lane, flick up for cruise on, the pedals),
    /// then a one-finger slot, then a second-finger slot. `None` when only
    /// the driving command list has it.
    pub fn touch_instruction(&self, action: Action) -> Option<&'static str> {
        self.instruction_in(action, Reach::Plain)
    }

    /// [`Self::touch_instruction`] for a sentence about a held pedal: a
    /// second-finger slot comes first ("Cruise paused. Hold gas and tap
    /// with a second finger to cancel it.").
    pub fn touch_instruction_while_pedal(&self, action: Action) -> Option<&'static str> {
        self.instruction_in(action, Reach::PedalHeld)
    }

    /// How a touch player runs `action`, as a noun for a spoken prompt:
    /// the gesture [`Self::touch_gesture_for`] names, else its row on the
    /// driving command list.
    pub fn touch_noun(&self, action: Action) -> String {
        self.touch_gesture_for(action)
            .unwrap_or_else(|| format!("{} from the three-finger tap list", action.label()))
    }

    /// The gesture that runs `action` at the wheel, as a noun, in the same
    /// order as [`Self::touch_instruction`]; `None` when only the driving
    /// command list has it.
    pub fn touch_gesture_for(&self, action: Action) -> Option<String> {
        self.noun_in(action, Reach::Plain)
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
            Some("a double tap")
        );
        assert_eq!(
            b.hint_touch_phrase("status_menu").as_deref(),
            Some("a three-finger tap then a double tap")
        );
        assert_eq!(
            b.touch_noun(Action::Rest),
            "Rest stop from the three-finger tap list"
        );
        // A command with a fixed gesture keeps naming it when a slot holds
        // it too; a list command names the slot it was moved to.
        b.set_touch_command(Gesture::UpperHoldDoubleTap, run(Action::TakeExit));
        assert_eq!(
            b.hint_touch_phrase("take_exit").as_deref(),
            Some("a double tap")
        );
        b.set_touch_command(Gesture::UpperHoldDoubleTap, run(Action::Rest));
        assert_eq!(
            b.hint_touch_phrase("rest").as_deref(),
            Some("a second-finger double tap while holding gas")
        );
    }

    /// A phone player changes lanes with a plain flick and turns cruise on
    /// with a flick up, so those are named before the second-finger slots.
    #[test]
    fn the_plain_gesture_is_named_first() {
        let mut b = KeyBindings::default();
        for (action, instruction, noun) in [
            (Action::SteerLeft, "Flick left", "a flick left"),
            (Action::SteerRight, "Flick right", "a flick right"),
            (Action::Cruise, "Flick up", "a flick up"),
            (Action::CruiseUp, "Flick up", "a flick up"),
            (Action::CruiseDown, "Flick down", "a flick down"),
        ] {
            assert_eq!(b.touch_instruction(action), Some(instruction), "{action:?}");
            assert_eq!(
                b.touch_gesture_for(action).as_deref(),
                Some(noun),
                "{action:?}"
            );
            assert_eq!(b.touch_noun(action), noun, "{action:?}");
        }
        assert_eq!(
            b.hint_touch_phrase("cruise_set").as_deref(),
            Some("a flick up")
        );
        // A sentence about a held pedal names the second-finger slot.
        assert_eq!(
            b.touch_instruction_while_pedal(Action::Cruise),
            Some("Hold gas and tap with a second finger")
        );
        assert_eq!(
            b.touch_instruction_while_pedal(Action::SteerLeft),
            Some("Hold gas and flick left with a second finger")
        );
        // A command only the list has is named by its row, everywhere.
        assert_eq!(b.touch_instruction(Action::SpeedLimit), None);
        assert_eq!(b.touch_gesture_for(Action::SpeedLimit), None);
        assert_eq!(
            b.touch_noun(Action::SpeedLimit),
            "Posted speed limit from the three-finger tap list"
        );
        assert_eq!(
            b.hint_touch_phrase("engine_brake").as_deref(),
            Some("Engine brake from the three-finger tap list")
        );
        // Flick up moved elsewhere: cruise is the second-finger tap again.
        b.set_touch_command(Gesture::SwipeUp, run(Action::Fuel));
        assert_eq!(
            b.touch_instruction(Action::Cruise),
            Some("Hold gas and tap with a second finger")
        );
        assert_eq!(b.touch_instruction(Action::Fuel), Some("Flick up"));
        // A one-finger slot comes before a second-finger one.
        b.set_touch_command(Gesture::Tap, run(Action::Cruise));
        assert_eq!(b.touch_instruction(Action::Cruise), Some("Tap"));
    }

    #[test]
    fn the_defaults_are_the_agreed_layout() {
        let b = KeyBindings::default();
        let on = |g| b.touch_command(g);
        assert_eq!(on(Gesture::UpperHoldTap), Some(run(Action::Cruise)));
        assert_eq!(on(Gesture::LowerHoldTap), Some(TouchCommand::Nothing));
        assert_eq!(on(Gesture::LowerHoldDoubleTap), Some(TouchCommand::Nothing));
        assert_eq!(on(Gesture::UpperHoldDoubleTap), Some(TouchCommand::Nothing));
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
    }

    #[test]
    fn fixed_gestures_are_not_slots() {
        let b = KeyBindings::default();
        for gesture in [
            Gesture::DoubleTap,
            Gesture::TwoFingerTap,
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
        b.set_touch_command(Gesture::SwipeDown, TouchCommand::CommandList);
        let mut settings = Settings::default();
        b.store(&mut settings);
        assert_eq!(
            settings.touch_bindings,
            "tap=fuel;swipe_down=command_list;magic_tap=none;bottom_hold_swipe_up=pause"
        );
        assert_eq!(KeyBindings::from_settings(&settings), b);
        // Back to the default clears the override.
        b.set_touch_command(Gesture::Tap, run(Action::Speed));
        b.set_touch_command(Gesture::MagicTap, TouchCommand::Pause);
        b.set_touch_command(Gesture::SwipeDown, run(Action::CruiseDown));
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
        // Two-finger tap stops speech now; its old slot entry is dropped.
        assert_eq!(b.touch_command(Gesture::TwoFingerTap), None);
        assert!(b.is_default());
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
