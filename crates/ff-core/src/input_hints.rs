//! Device-aware control hints for spoken prompts.
//!
//! Freight Fate speaks a lot of instructions that name a control -- "press P
//! to release the parking brake", "hold the Up arrow to accelerate". With
//! controller support those prompts must name the *right* control for
//! whichever device the player is actually using. This module keeps a single
//! table mapping a semantic action to its keyboard phrase and its controller
//! phrase, plus one lookup function, so the surrounding sentences stay
//! natural and there is exactly one place to edit a control name.
//!
//! The phrases are fragments meant to slot into a sentence after a verb,
//! e.g. `format!("Press {} to take it.", control_hint("take_exit", device))`.
//! The keyboard side matches the wording the game already used.
//!
//! Port of `freight_fate/input_hints.py`.

pub const KEYBOARD: &str = "keyboard";
pub const CONTROLLER: &str = "controller";
/// The iPhone or iPad screen, with no keyboard or controller in use.
pub const TOUCH: &str = "touch";

/// action -> touch phrase, for the controls that are fixed gestures (the
/// pedal holds, the deep swipe, the key turn, double tap, the two-finger
/// swipes). A flick is one step; a swipe and hold is continuous. Every other touch
/// phrase comes from the player's touch bindings or the driving command
/// list, in `freight-fate`'s bindings.
pub const TOUCH_HINTS: &[(&str, &str)] = &[
    // Every gas line reads "hold {}", so the phrase leaves the hold out.
    ("accelerate", "a swipe up"),
    ("brake", "a swipe down and hold"),
    ("emergency_brake", "a deep swipe down"),
    ("horn", "a two-finger hold"),
    ("engine", "a two-finger turn to the right"),
    ("confirm", "a double tap"),
    ("pause", "a two-finger swipe down"),
    ("help", "a two-finger swipe up"),
];

/// action -> (keyboard phrase, controller phrase)
pub const HINTS: &[(&str, (&str, &str))] = &[
    ("accelerate", ("the Up arrow", "the right trigger")),
    ("brake", ("the Down arrow", "the left trigger")),
    ("emergency_brake", ("B", "the left trigger fully")),
    ("clutch", ("Left Shift", "the left bumper")),
    ("gear_first", ("W", "the A button")),
    ("gears", ("W and Q", "the A and X buttons")),
    ("reverse", ("Backspace", "the X button")),
    ("neutral", ("N", "neutral")),
    ("engine", ("E", "right bumper plus A")),
    ("parking_brake", ("P", "right bumper plus Y")),
    ("confirm", ("Enter", "controller A")),
    ("take_exit", ("X", "D-pad down")),
    ("rest", ("T", "right bumper plus D-pad down")),
    ("cruise_set", ("K", "the Y button")),
    (
        "cruise_adjust",
        ("plus and minus", "right bumper plus D-pad left or right"),
    ),
    ("speed", ("Space", "the B button")),
    ("status_menu", ("Tab", "right bumper plus Start")),
    ("fuel", ("F", "right bumper plus B")),
    ("clock", ("C", "D-pad right")),
    ("route", ("R", "D-pad up")),
    ("weather", ("V", "D-pad left")),
    ("lane", ("L", "the left stick")),
    ("horn", ("H", "the left stick click")),
    ("engine_brake", ("J", "the right stick click")),
    ("pause", ("Escape", "Start")),
    ("help", ("F1", "the Back button")),
    (
        "stop_event_voice",
        ("Left or Right Control", "the Back button"),
    ),
];

/// Phrase naming `action`'s control for the active input `device`.
///
/// Falls back to the keyboard phrase for an unknown device, and returns the
/// action name itself if it is not in the table (so a typo is audible in a
/// test rather than crashing a prompt mid-drive).
pub fn control_hint(action: &str, device: &str) -> String {
    if device == TOUCH {
        if let Some((_, phrase)) = TOUCH_HINTS.iter().find(|(name, _)| *name == action) {
            return phrase.to_string();
        }
    }
    let (kb, pad) = HINTS
        .iter()
        .find(|(name, _)| *name == action)
        .map(|(_, phrases)| *phrases)
        .unwrap_or((action, action));
    if device == CONTROLLER {
        pad.to_string()
    } else {
        kb.to_string()
    }
}

#[cfg(test)]
mod tests {
    //! The Python suite has no `test_input_hints.py`; the importers
    //! (`test_controller`, `test_microsleep`, `test_scale_check_in_guidance`)
    //! all drive the table through `App()`. These pin the lookup contract.
    use super::*;

    #[test]
    fn test_keyboard_and_controller_phrases() {
        assert_eq!(control_hint("take_exit", KEYBOARD), "X");
        assert_eq!(control_hint("take_exit", CONTROLLER), "D-pad down");
        assert_eq!(control_hint("parking_brake", KEYBOARD), "P");
        assert_eq!(
            control_hint("parking_brake", CONTROLLER),
            "right bumper plus Y"
        );
        assert_eq!(control_hint("confirm", KEYBOARD), "Enter");
        assert_eq!(control_hint("confirm", CONTROLLER), "controller A");
    }

    #[test]
    fn test_unknown_device_falls_back_to_the_keyboard_phrase() {
        assert_eq!(control_hint("accelerate", "wheel"), "the Up arrow");
    }

    #[test]
    fn test_unknown_action_is_audible_not_fatal() {
        assert_eq!(control_hint("teleport", KEYBOARD), "teleport");
        assert_eq!(control_hint("accelerate", TOUCH), "a swipe up");
        assert_eq!(control_hint("take_exit", TOUCH), "X");
        assert_eq!(control_hint("teleport", CONTROLLER), "teleport");
    }

    #[test]
    fn test_every_action_has_two_distinct_phrases() {
        for (name, (kb, pad)) in HINTS {
            assert!(!kb.is_empty() && !pad.is_empty(), "{name}");
        }
        let mut names: Vec<&str> = HINTS.iter().map(|(n, _)| *n).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), HINTS.len(), "duplicate action in the table");
    }
}
