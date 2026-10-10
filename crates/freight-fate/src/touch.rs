//! Touch gestures: the iOS game's input surface.
//!
//! The iPhone and iPad game is the desktop game, spoken through Prism and
//! driven by the same states. `ios/ff_touch.m` recognizes the gestures (with
//! VoiceOver on, through a direct-interaction element) and queues a code for
//! each; [`TouchInput`] turns the codes into [`InputEvent`]s. A flick is one
//! step, a swipe and hold is continuous: a stroke held in a direction is a
//! held key (gas, brake, steering) until it lifts. Every other gesture
//! arrives as
//! [`InputEvent::Gesture`]: the driving state runs the command the player's
//! touch bindings give it, and every other screen, or a gesture with no
//! binding, gets the key a keyboard player would press ([`Gesture::key`]).
//! Nothing here touches UIKit, so the whole mapping is tested on every
//! platform.

use crate::states::base::{InputEvent, Key, Mods};

/// A gesture recognized by the native touch surface.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Gesture {
    Tap,
    DoubleTap,
    SwipeUp,
    SwipeDown,
    SwipeLeft,
    SwipeRight,
    TwoFingerTap,
    TwoFingerSwipeUp,
    TwoFingerSwipeDown,
    TwoFingerSwipeLeft,
    TwoFingerSwipeRight,
    ThreeFingerSwipeUp,
    ThreeFingerSwipeDown,
    ThreeFingerSwipeLeft,
    ThreeFingerSwipeRight,
    /// Opens the driving command list, and reads a name field back.
    ThreeFingerTap,
    /// A swipe up and hold anywhere on the screen (gas).
    HoldUpperBegan,
    /// A swipe down and hold anywhere on the screen (brake).
    HoldLowerBegan,
    HoldEnded,
    /// VoiceOver's two-finger scrub.
    Escape,
    /// VoiceOver's two-finger double tap.
    MagicTap,
    /// VoiceOver's double tap, when direct touch is not active.
    Activate,
    /// VoiceOver's swipe up on the adjustable element.
    Increment,
    /// VoiceOver's swipe down on the adjustable element.
    Decrement,
    /// A second finger's tap while gas is held.
    UpperHoldTap,
    UpperHoldDoubleTap,
    UpperHoldSwipeUp,
    UpperHoldSwipeDown,
    UpperHoldSwipeLeft,
    UpperHoldSwipeRight,
    /// A second finger's tap while brake is held.
    LowerHoldTap,
    LowerHoldDoubleTap,
    LowerHoldSwipeUp,
    LowerHoldSwipeDown,
    LowerHoldSwipeLeft,
    LowerHoldSwipeRight,
    /// The old two-finger emergency brake hold; the native side no longer
    /// sends it, and the deep swipe took its place.
    EmergencyBrakeHoldBegan,
    /// Two or three fingers held anywhere (horn).
    HornHoldBegan,
    /// A swipe left and hold (steer left).
    HoldLeftBegan,
    /// A swipe right and hold (steer right).
    HoldRightBegan,
    /// A swipe down about three flicks deep: the emergency brake while
    /// moving, the parking brake when stopped.
    DeepSwipeDownBegan,
    /// One finger held still (straighten the wheel).
    StillHoldBegan,
    /// A second finger's swipe left and hold while a pedal is held.
    SecondSteerLeftBegan,
    SecondSteerRightBegan,
    /// The second finger's steering lifted, the pedal still held.
    SecondSteerEnded,
    /// Two fingers turned right like a key (start the engine).
    RotateRight,
    /// Two fingers turned left (stop the engine).
    RotateLeft,
    /// A hardware keyboard connected or went away.
    KeyboardConnected,
    KeyboardDisconnected,
}

impl Gesture {
    /// The code `ios/ff_touch.m` queues for this gesture.
    pub fn from_code(code: i32) -> Option<Gesture> {
        Some(match code {
            0 => Gesture::Tap,
            1 => Gesture::DoubleTap,
            2 => Gesture::SwipeUp,
            3 => Gesture::SwipeDown,
            4 => Gesture::SwipeLeft,
            5 => Gesture::SwipeRight,
            6 => Gesture::TwoFingerTap,
            7 => Gesture::TwoFingerSwipeUp,
            8 => Gesture::TwoFingerSwipeDown,
            9 => Gesture::TwoFingerSwipeLeft,
            10 => Gesture::TwoFingerSwipeRight,
            11 => Gesture::ThreeFingerSwipeUp,
            12 => Gesture::ThreeFingerSwipeDown,
            14 => Gesture::HoldUpperBegan,
            15 => Gesture::HoldLowerBegan,
            16 => Gesture::HoldEnded,
            17 => Gesture::Escape,
            18 => Gesture::MagicTap,
            19 => Gesture::Activate,
            20 => Gesture::Increment,
            21 => Gesture::Decrement,
            22 => Gesture::ThreeFingerSwipeLeft,
            23 => Gesture::ThreeFingerSwipeRight,
            24 => Gesture::ThreeFingerTap,
            25 => Gesture::UpperHoldTap,
            26 => Gesture::UpperHoldDoubleTap,
            27 => Gesture::UpperHoldSwipeUp,
            28 => Gesture::UpperHoldSwipeDown,
            29 => Gesture::UpperHoldSwipeLeft,
            30 => Gesture::UpperHoldSwipeRight,
            31 => Gesture::LowerHoldTap,
            32 => Gesture::LowerHoldDoubleTap,
            33 => Gesture::LowerHoldSwipeUp,
            34 => Gesture::LowerHoldSwipeDown,
            35 => Gesture::LowerHoldSwipeLeft,
            36 => Gesture::LowerHoldSwipeRight,
            37 => Gesture::EmergencyBrakeHoldBegan,
            38 => Gesture::HornHoldBegan,
            39 => Gesture::HoldLeftBegan,
            40 => Gesture::HoldRightBegan,
            41 => Gesture::DeepSwipeDownBegan,
            42 => Gesture::StillHoldBegan,
            43 => Gesture::SecondSteerLeftBegan,
            44 => Gesture::SecondSteerRightBegan,
            45 => Gesture::SecondSteerEnded,
            46 => Gesture::RotateRight,
            47 => Gesture::RotateLeft,
            48 => Gesture::KeyboardConnected,
            49 => Gesture::KeyboardDisconnected,
            _ => return None,
        })
    }

    /// The key a single press of this gesture stands for, if it is a press.
    /// A second finger during a hold is a driving command and nothing else.
    pub fn key(self) -> Option<Key> {
        Some(match self {
            Gesture::Tap => Key::Comma,
            Gesture::DoubleTap | Gesture::Activate => Key::Return,
            Gesture::SwipeUp | Gesture::Increment => Key::Up,
            // A deep swipe off the road is just a long flick down.
            Gesture::SwipeDown | Gesture::Decrement | Gesture::DeepSwipeDownBegan => Key::Down,
            Gesture::SwipeLeft => Key::Left,
            Gesture::SwipeRight => Key::Right,
            Gesture::TwoFingerTap => Key::Tab,
            Gesture::MagicTap => Key::Space,
            Gesture::TwoFingerSwipeUp => Key::F1,
            Gesture::TwoFingerSwipeDown | Gesture::Escape => Key::Escape,
            Gesture::TwoFingerSwipeLeft => Key::Comma,
            Gesture::TwoFingerSwipeRight => Key::Period,
            Gesture::ThreeFingerSwipeUp => Key::Home,
            Gesture::ThreeFingerSwipeDown => Key::End,
            Gesture::ThreeFingerSwipeLeft => Key::PageUp,
            Gesture::ThreeFingerSwipeRight => Key::PageDown,
            Gesture::ThreeFingerTap => Key::F2,
            Gesture::HoldUpperBegan
            | Gesture::HoldLowerBegan
            | Gesture::HoldEnded
            | Gesture::EmergencyBrakeHoldBegan
            | Gesture::HornHoldBegan
            | Gesture::HoldLeftBegan
            | Gesture::HoldRightBegan
            | Gesture::StillHoldBegan
            | Gesture::SecondSteerLeftBegan
            | Gesture::SecondSteerRightBegan
            | Gesture::SecondSteerEnded
            | Gesture::RotateRight
            | Gesture::RotateLeft
            | Gesture::KeyboardConnected
            | Gesture::KeyboardDisconnected
            | Gesture::UpperHoldTap
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
            | Gesture::LowerHoldSwipeRight => return None,
        })
    }

    /// The fixed key a hold keeps down: the pedals and the wheel.
    pub fn held_key(self) -> Option<Key> {
        match self {
            Gesture::HoldUpperBegan => Some(Key::Up),
            Gesture::HoldLowerBegan => Some(Key::Down),
            Gesture::HoldLeftBegan | Gesture::SecondSteerLeftBegan => Some(Key::Left),
            Gesture::HoldRightBegan | Gesture::SecondSteerRightBegan => Some(Key::Right),
            _ => None,
        }
    }

    /// A hold that steers, which full lane keeping leaves to the flicks.
    pub fn steers(self) -> bool {
        matches!(
            self,
            Gesture::HoldLeftBegan
                | Gesture::HoldRightBegan
                | Gesture::SecondSteerLeftBegan
                | Gesture::SecondSteerRightBegan
        )
    }

    /// The press that starts [`Self::held_key`].
    pub fn hold_events(self) -> Vec<InputEvent> {
        self.held_key().map(key_down).into_iter().collect()
    }

    /// The press and release of [`Self::key`], for a screen that did not
    /// take the gesture itself.
    pub fn key_events(self) -> Vec<InputEvent> {
        match self.key() {
            Some(key) => vec![
                key_down(key),
                InputEvent::KeyUp {
                    key,
                    mods: Mods::NONE,
                },
            ],
            None => Vec::new(),
        }
    }
}

/// What one gesture asks of the shell.
#[derive(Debug, Default, PartialEq)]
pub struct TouchOutput {
    pub events: Vec<InputEvent>,
}

/// The gesture translator, holding the key the first finger holds and the
/// one a second finger steers with.
#[derive(Debug, Default)]
pub struct TouchInput {
    held: Option<Key>,
    second: Option<Key>,
}

impl TouchInput {
    pub fn new() -> Self {
        Self::default()
    }

    /// The key a held finger is keeping down, if any.
    pub fn held(&self) -> Option<Key> {
        self.held
    }

    pub fn handle(&mut self, gesture: Gesture) -> TouchOutput {
        let mut out = TouchOutput::default();
        match gesture {
            // Gas, brake and steering are held keys, so the latching brake
            // and the reverse press-and-hold work exactly as they do on a
            // keyboard. The hold goes out as its gesture, and the app
            // presses the key, so the press is known to be the screen's.
            Gesture::HoldUpperBegan
            | Gesture::HoldLowerBegan
            | Gesture::HoldLeftBegan
            | Gesture::HoldRightBegan => {
                self.release_into(&mut out.events);
                out.events.push(InputEvent::Gesture(gesture));
                self.held = gesture.held_key();
            }
            Gesture::SecondSteerLeftBegan | Gesture::SecondSteerRightBegan => {
                self.release_second_into(&mut out.events);
                out.events.push(InputEvent::Gesture(gesture));
                self.second = gesture.held_key();
            }
            Gesture::SecondSteerEnded => self.release_second_into(&mut out.events),
            // A deep swipe grows out of the brake hold: the brake key lets
            // go, and the app holds what the deep swipe means here.
            Gesture::DeepSwipeDownBegan => {
                self.release_into(&mut out.events);
                out.events.push(InputEvent::Gesture(gesture));
            }
            // These holds use the player's current keyboard binding.  The
            // application resolves and holds that chord, because this small
            // platform-neutral translator deliberately does not own bindings.
            Gesture::EmergencyBrakeHoldBegan | Gesture::HornHoldBegan | Gesture::StillHoldBegan => {
                out.events.push(InputEvent::Gesture(gesture));
            }
            Gesture::HoldEnded => {
                self.release_second_into(&mut out.events);
                if self.held.is_some() {
                    self.release_into(&mut out.events);
                } else {
                    out.events.push(InputEvent::Gesture(gesture));
                }
            }
            other => out.events.push(InputEvent::Gesture(other)),
        }
        out
    }

    /// Let go of a held key, as when the app leaves the foreground.
    pub fn release_into(&mut self, events: &mut Vec<InputEvent>) {
        self.release_second_into(events);
        if let Some(key) = self.held.take() {
            events.push(InputEvent::KeyUp {
                key,
                mods: Mods::NONE,
            });
        }
    }

    fn release_second_into(&mut self, events: &mut Vec<InputEvent>) {
        if let Some(key) = self.second.take() {
            events.push(InputEvent::KeyUp {
                key,
                mods: Mods::NONE,
            });
        }
    }
}

fn key_down(key: Key) -> InputEvent {
    let text = match key {
        Key::Space => Some(' '),
        Key::Comma => Some(','),
        Key::Period => Some('.'),
        _ => None,
    };
    InputEvent::KeyDown {
        key,
        mods: Mods::NONE,
        text,
        repeat: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    impl Gesture {
        fn hold_events_output(self) -> TouchOutput {
            TouchOutput {
                events: self.hold_events(),
            }
        }
    }

    fn pressed(out: &TouchOutput) -> Vec<Key> {
        out.events
            .iter()
            .filter_map(|event| match event {
                InputEvent::KeyDown { key, .. } => Some(*key),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn every_native_code_round_trips_and_unknown_codes_are_ignored() {
        for code in (0..50).filter(|code| *code != 13) {
            assert!(Gesture::from_code(code).is_some(), "code {code}");
        }
        // 13 was the three-finger double tap, retired with the keyboard toggle.
        assert_eq!(Gesture::from_code(13), None);
        assert_eq!(Gesture::from_code(50), None);
        assert_eq!(Gesture::from_code(-1), None);
    }

    #[test]
    fn discrete_gestures_reach_the_state_as_gestures() {
        let mut touch = TouchInput::new();
        for gesture in [
            Gesture::Tap,
            Gesture::MagicTap,
            Gesture::ThreeFingerTap,
            Gesture::UpperHoldTap,
        ] {
            assert_eq!(
                touch.handle(gesture).events,
                vec![InputEvent::Gesture(gesture)]
            );
        }
        assert_eq!(touch.held(), None);
    }

    #[test]
    fn menu_gestures_press_and_release_the_menu_keys() {
        for (gesture, key) in [
            (Gesture::SwipeUp, Key::Up),
            (Gesture::SwipeDown, Key::Down),
            (Gesture::DeepSwipeDownBegan, Key::Down),
            (Gesture::DoubleTap, Key::Return),
            (Gesture::Activate, Key::Return),
            (Gesture::TwoFingerSwipeDown, Key::Escape),
            (Gesture::Escape, Key::Escape),
            (Gesture::Increment, Key::Up),
            (Gesture::Decrement, Key::Down),
            (Gesture::TwoFingerSwipeUp, Key::F1),
            (Gesture::ThreeFingerTap, Key::F2),
        ] {
            assert_eq!(
                gesture.key_events(),
                vec![
                    InputEvent::KeyDown {
                        key,
                        mods: Mods::NONE,
                        text: None,
                        repeat: false,
                    },
                    InputEvent::KeyUp {
                        key,
                        mods: Mods::NONE,
                    },
                ],
                "{gesture:?}"
            );
        }
    }

    #[test]
    fn a_second_finger_during_a_hold_presses_no_key() {
        assert!(Gesture::UpperHoldTap.key_events().is_empty());
        assert!(Gesture::LowerHoldSwipeRight.key_events().is_empty());
    }

    #[test]
    fn review_gestures_carry_the_typed_character() {
        assert!(matches!(
            Gesture::TwoFingerSwipeLeft.key_events().first(),
            Some(InputEvent::KeyDown {
                key: Key::Comma,
                text: Some(','),
                ..
            })
        ));
        assert!(matches!(
            Gesture::MagicTap.key_events().first(),
            Some(InputEvent::KeyDown {
                key: Key::Space,
                text: Some(' '),
                ..
            })
        ));
    }

    #[test]
    fn holds_keep_the_pedal_down_until_the_finger_lifts() {
        let mut touch = TouchInput::new();
        let out = touch.handle(Gesture::HoldUpperBegan);
        assert_eq!(
            out.events,
            vec![InputEvent::Gesture(Gesture::HoldUpperBegan)]
        );
        assert_eq!(
            pressed(&Gesture::HoldUpperBegan.hold_events_output()),
            vec![Key::Up]
        );
        assert_eq!(touch.held(), Some(Key::Up));

        let out = touch.handle(Gesture::HoldEnded);
        assert_eq!(
            out.events,
            vec![InputEvent::KeyUp {
                key: Key::Up,
                mods: Mods::NONE,
            }]
        );
        assert_eq!(touch.held(), None);
    }

    #[test]
    fn emergency_brake_and_horn_holds_are_left_for_live_bindings() {
        for gesture in [
            Gesture::EmergencyBrakeHoldBegan,
            Gesture::HornHoldBegan,
            Gesture::StillHoldBegan,
        ] {
            let mut touch = TouchInput::new();
            assert_eq!(gesture.held_key(), None);
            assert_eq!(
                touch.handle(gesture).events,
                vec![InputEvent::Gesture(gesture)]
            );
            assert_eq!(touch.held(), None);
            assert_eq!(
                touch.handle(Gesture::HoldEnded).events,
                vec![InputEvent::Gesture(Gesture::HoldEnded)]
            );
        }
    }

    #[test]
    fn a_new_hold_releases_the_old_one_first() {
        let mut touch = TouchInput::new();
        touch.handle(Gesture::HoldUpperBegan);
        let out = touch.handle(Gesture::HoldLowerBegan);
        assert_eq!(
            out.events,
            vec![
                InputEvent::KeyUp {
                    key: Key::Up,
                    mods: Mods::NONE,
                },
                InputEvent::Gesture(Gesture::HoldLowerBegan),
            ]
        );
        assert_eq!(touch.held(), Some(Key::Down));
    }

    #[test]
    fn a_lift_without_a_pedal_reaches_the_live_binding_handler() {
        let mut touch = TouchInput::new();
        assert_eq!(
            touch.handle(Gesture::HoldEnded).events,
            vec![InputEvent::Gesture(Gesture::HoldEnded)]
        );
    }

    #[test]
    fn steering_holds_keep_the_wheel_key_down_until_the_finger_lifts() {
        let mut touch = TouchInput::new();
        touch.handle(Gesture::HoldLeftBegan);
        assert_eq!(touch.held(), Some(Key::Left));
        assert!(Gesture::HoldLeftBegan.steers());
        assert_eq!(
            touch.handle(Gesture::HoldEnded).events,
            vec![InputEvent::KeyUp {
                key: Key::Left,
                mods: Mods::NONE,
            }]
        );
    }

    #[test]
    fn a_second_finger_steers_under_the_held_pedal() {
        let mut touch = TouchInput::new();
        touch.handle(Gesture::HoldUpperBegan);
        assert_eq!(
            touch.handle(Gesture::SecondSteerRightBegan).events,
            vec![InputEvent::Gesture(Gesture::SecondSteerRightBegan)]
        );
        assert_eq!(
            touch.handle(Gesture::SecondSteerEnded).events,
            vec![InputEvent::KeyUp {
                key: Key::Right,
                mods: Mods::NONE,
            }]
        );
        assert_eq!(touch.held(), Some(Key::Up));
        touch.handle(Gesture::SecondSteerLeftBegan);
        assert_eq!(
            touch.handle(Gesture::HoldEnded).events,
            vec![
                InputEvent::KeyUp {
                    key: Key::Left,
                    mods: Mods::NONE,
                },
                InputEvent::KeyUp {
                    key: Key::Up,
                    mods: Mods::NONE,
                },
            ]
        );
    }

    #[test]
    fn a_deep_swipe_lets_the_brake_key_go_first() {
        let mut touch = TouchInput::new();
        touch.handle(Gesture::HoldLowerBegan);
        assert_eq!(
            touch.handle(Gesture::DeepSwipeDownBegan).events,
            vec![
                InputEvent::KeyUp {
                    key: Key::Down,
                    mods: Mods::NONE,
                },
                InputEvent::Gesture(Gesture::DeepSwipeDownBegan),
            ]
        );
        assert_eq!(touch.held(), None);
        assert_eq!(
            touch.handle(Gesture::HoldEnded).events,
            vec![InputEvent::Gesture(Gesture::HoldEnded)]
        );
    }
}
