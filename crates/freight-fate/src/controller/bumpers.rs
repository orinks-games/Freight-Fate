//! Which bumper holds the controller's second layer: the Settings >
//! Controls row "Second layer bumper" (`settings.left_bumper_layer`).
//!
//! The game is written for one layout: the right bumper held is the second
//! layer of pad buttons, and the left bumper is the clutch. A player who
//! finds the left bumper easier to hold can swap the two. The swap happens
//! at the two edges and nowhere in between:
//!
//! * coming in, [`BumperRoles`] turns the left bumper's presses into the
//!   right bumper's and back before anything reads them, so the bindings,
//!   the clutch, the second layer and Input practice all work unchanged. A
//!   press keeps the role it was given until its release, so changing the
//!   setting while a bumper is held never leaves the clutch or the second
//!   layer stuck;
//! * going out, [`swap_words`] turns "right bumper" into "left bumper" and
//!   back in the game's own control names and fixed instructions
//!   ([`GameContext::bumper_words`], [`GameContext::pad_name`]), so prompts,
//!   help and hints name the button the player actually holds. It is never
//!   run on a finished line, so a career, city or station name is left as
//!   it is, and a line spoken with the default layout keeps its
//!   translation. The swapped instructions are catalog entries of their own
//!   (`lang/extra.pot` for the help pages), spoken in English until a
//!   language translates them.
//!
//! With the setting off both are the identity, so nothing changes for a
//! player who leaves it alone.
//!
//! [`GameContext::bumper_words`]: crate::app::GameContext::bumper_words
//! [`GameContext::pad_name`]: crate::app::GameContext::pad_name

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

fn is_bumper(button: ControllerButton) -> bool {
    matches!(
        button,
        ControllerButton::LeftShoulder | ControllerButton::RightShoulder
    )
}

/// The role each held bumper was pressed as. A press takes its role from
/// the setting at that moment, and its release ends that same role, so the
/// game always hears a release for every press it heard.
#[derive(Debug, Default)]
pub struct BumperRoles {
    /// `(controller, physical bumper, role)` for each bumper held down.
    held: Vec<(u32, ControllerButton, ControllerButton)>,
}

impl BumperRoles {
    /// A batch of events from the controller, each bumper given the role it
    /// has under `left_bumper_layer`, or, for a release, the role its press
    /// was given.
    pub fn map(&mut self, mut events: Vec<InputEvent>, left_bumper_layer: bool) -> Vec<InputEvent> {
        let role_now = |button| {
            if left_bumper_layer {
                swap_button(button)
            } else {
                button
            }
        };
        for event in &mut events {
            match event {
                InputEvent::ControllerButtonDown {
                    button,
                    instance_id,
                } if is_bumper(*button) => {
                    let role = role_now(*button);
                    self.held
                        .retain(|(pad, held, _)| (*pad, *held) != (*instance_id, *button));
                    self.held.push((*instance_id, *button, role));
                    *button = role;
                }
                InputEvent::ControllerButtonUp {
                    button,
                    instance_id,
                } if is_bumper(*button) => {
                    let pressed = self
                        .held
                        .iter()
                        .position(|(pad, held, _)| (*pad, *held) == (*instance_id, *button));
                    *button = match pressed {
                        Some(at) => self.held.swap_remove(at).2,
                        None => role_now(*button),
                    };
                }
                _ => {}
            }
        }
        events
    }
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
        assert_eq!(BumperRoles::default().map(events.clone(), false), events);
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
            BumperRoles::default().map(events, true),
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

    fn down(button: ControllerButton) -> InputEvent {
        InputEvent::ControllerButtonDown {
            button,
            instance_id: 1,
        }
    }

    fn up(button: ControllerButton) -> InputEvent {
        InputEvent::ControllerButtonUp {
            button,
            instance_id: 1,
        }
    }

    #[test]
    fn a_bumper_held_through_a_setting_change_is_released_as_it_was_pressed() {
        use ControllerButton::{LeftShoulder, RightShoulder};
        let mut roles = BumperRoles::default();
        // Right bumper held as the second layer, then the setting flips
        // while it is down: its release, a frame later, still ends the
        // second layer.
        assert_eq!(
            roles.map(vec![down(RightShoulder)], false),
            vec![down(RightShoulder)]
        );
        assert_eq!(
            roles.map(vec![up(RightShoulder)], true),
            vec![up(RightShoulder)]
        );
        // The left bumper held as the clutch, then the setting flips: it
        // lets go of the clutch, not the second layer.
        assert_eq!(
            roles.map(vec![down(LeftShoulder)], false),
            vec![down(LeftShoulder)]
        );
        assert_eq!(
            roles.map(vec![up(LeftShoulder)], true),
            vec![up(LeftShoulder)]
        );
        // The other way round, from the swapped layout back to the default.
        assert_eq!(
            roles.map(vec![down(LeftShoulder)], true),
            vec![down(RightShoulder)]
        );
        assert_eq!(
            roles.map(vec![up(LeftShoulder)], false),
            vec![up(RightShoulder)]
        );
        // The next press takes the new setting.
        assert_eq!(
            roles.map(vec![down(LeftShoulder)], true),
            vec![down(RightShoulder)]
        );
        assert_eq!(
            roles.map(vec![up(LeftShoulder)], true),
            vec![up(RightShoulder)]
        );
        assert!(roles.held.is_empty());
    }

    #[test]
    fn two_controllers_keep_their_own_bumpers() {
        use ControllerButton::{LeftShoulder, RightShoulder};
        let on_two = |event: InputEvent| match event {
            InputEvent::ControllerButtonDown { button, .. } => InputEvent::ControllerButtonDown {
                button,
                instance_id: 2,
            },
            InputEvent::ControllerButtonUp { button, .. } => InputEvent::ControllerButtonUp {
                button,
                instance_id: 2,
            },
            event => event,
        };
        let mut roles = BumperRoles::default();
        roles.map(vec![down(LeftShoulder)], false);
        // Controller 2's press after the flip is the second layer; controller
        // 1's release is still its clutch.
        assert_eq!(
            roles.map(vec![on_two(down(LeftShoulder)), up(LeftShoulder)], true),
            vec![on_two(down(RightShoulder)), up(LeftShoulder)]
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
