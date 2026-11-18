//! Which bumper holds the controller's second layer: the Settings >
//! Controls row "Second layer bumper" (`settings.left_bumper_layer`).
//!
//! The game is written for one layout: the right bumper held is the second
//! layer of pad buttons, and the left bumper is the clutch. A player who
//! finds the left bumper easier to hold can swap the two. The swap happens
//! at the two edges and nowhere in between:
//!
//! * coming in, [`swap_buttons`] turns the left bumper's presses into the
//!   right bumper's and back before anything reads them, so the bindings,
//!   the clutch, the second layer and Input practice all work unchanged;
//! * going out, [`swap_words`] turns "right bumper" into "left bumper" and
//!   back in every line the game speaks ([`GameContext::tr`]), so prompts,
//!   help and hints name the button the player actually holds.
//!
//! With the setting off both are the identity, so nothing changes for a
//! player who leaves it alone. Spoken lines are swapped before they are
//! translated, so with the setting on, a line that names a bumper finds no
//! translation and is spoken in English: the right button in the wrong
//! language, rather than the wrong button.
//!
//! [`GameContext::tr`]: crate::app::GameContext::tr

use std::borrow::Cow;

use super::ControllerButton;
use crate::states::base::InputEvent;

/// The other bumper for a bumper, and any other button as it is.
pub fn swap_button(button: ControllerButton) -> ControllerButton {
    match button {
        ControllerButton::LeftShoulder => ControllerButton::RightShoulder,
        ControllerButton::RightShoulder => ControllerButton::LeftShoulder,
        other => other,
    }
}

/// A batch of events from the controller, with the bumpers swapped when the
/// left bumper is the second layer.
pub fn swap_buttons(mut events: Vec<InputEvent>, left_bumper_layer: bool) -> Vec<InputEvent> {
    if left_bumper_layer {
        for event in &mut events {
            match event {
                InputEvent::ControllerButtonDown { button, .. }
                | InputEvent::ControllerButtonUp { button, .. } => *button = swap_button(*button),
                _ => {}
            }
        }
    }
    events
}

/// The bumper names, each with the one it becomes.
const SWAPS: [(&str, &str); 4] = [
    ("right bumper", "left bumper"),
    ("left bumper", "right bumper"),
    ("Right bumper", "Left bumper"),
    ("Left bumper", "Right bumper"),
];

/// `text` with "right bumper" and "left bumper" swapped when the left
/// bumper is the second layer; borrowed when nothing changes.
pub fn swap_words(text: &str, left_bumper_layer: bool) -> Cow<'_, str> {
    if !left_bumper_layer || !text.contains("bumper") {
        return Cow::Borrowed(text);
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while !rest.is_empty() {
        let next = SWAPS
            .iter()
            .filter_map(|(from, to)| rest.find(from).map(|at| (at, *from, *to)))
            .min_by_key(|(at, _, _)| *at);
        let Some((at, from, to)) = next else {
            out.push_str(rest);
            break;
        };
        out.push_str(&rest[..at]);
        out.push_str(to);
        rest = &rest[at + from.len()..];
    }
    Cow::Owned(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn with_the_setting_off_nothing_changes() {
        let line = "Press right bumper plus A to start the engine.";
        assert!(matches!(swap_words(line, false), Cow::Borrowed(_)));
        assert_eq!(swap_button(ControllerButton::A), ControllerButton::A);
        let events = vec![InputEvent::ControllerButtonDown {
            button: ControllerButton::LeftShoulder,
            instance_id: 0,
        }];
        assert_eq!(swap_buttons(events.clone(), false), events);
    }

    #[test]
    fn the_two_bumpers_trade_places() {
        let events = vec![
            InputEvent::ControllerButtonDown {
                button: ControllerButton::LeftShoulder,
                instance_id: 3,
            },
            InputEvent::ControllerButtonUp {
                button: ControllerButton::RightShoulder,
                instance_id: 3,
            },
            InputEvent::ControllerButtonDown {
                button: ControllerButton::A,
                instance_id: 3,
            },
        ];
        assert_eq!(
            swap_buttons(events, true),
            vec![
                InputEvent::ControllerButtonDown {
                    button: ControllerButton::RightShoulder,
                    instance_id: 3,
                },
                InputEvent::ControllerButtonUp {
                    button: ControllerButton::LeftShoulder,
                    instance_id: 3,
                },
                InputEvent::ControllerButtonDown {
                    button: ControllerButton::A,
                    instance_id: 3,
                },
            ]
        );
    }

    #[test]
    fn spoken_lines_name_the_button_the_player_holds() {
        assert_eq!(
            swap_words("Press right bumper plus A to start the engine.", true),
            "Press left bumper plus A to start the engine."
        );
        // Both in one line swap with each other, not into the same one.
        assert_eq!(
            swap_words(
                "Hold the left bumper for the clutch and the right bumper for a second layer.",
                true
            ),
            "Hold the right bumper for the clutch and the left bumper for a second layer."
        );
        assert_eq!(
            swap_words("Right bumper: hold for the second layer of buttons", true),
            "Left bumper: hold for the second layer of buttons"
        );
        assert_eq!(
            swap_words("Second layer bumper: left", true),
            "Second layer bumper: left"
        );
    }
}
