//! Touch gestures: the iOS game's input surface.
//!
//! The iPhone and iPad game is the desktop game, spoken through Prism and
//! driven by the same states. `ios/ff_touch.m` recognizes the gestures (with
//! VoiceOver on, through a direct-interaction element) and queues a code for
//! each; [`TouchInput`] turns the codes into [`InputEvent`]s. A flick is one
//! step, a swipe and hold is continuous: a stroke held in a direction is a
//! held key (gas, brake, steering) until it lifts. Every other gesture
//! arrives as [`InputEvent::Gesture`], and [`route`] says what the app does
//! with it on the kind of screen on top ([`TouchMode`]): at the wheel the
//! drive runs the command the player's touch bindings give it, and every
//! other screen, or a gesture with no binding, gets the key a keyboard
//! player would press. Nothing here touches UIKit, so the whole mapping is
//! tested on every platform.

use crate::bindings::Action;
use crate::states::base::{InputEvent, Key, Mods};
pub use ff_core::touch_mode::{RoadOffer, TouchMode};

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

    /// The key a single press of this gesture stands for in a menu, if it
    /// is a press. A second finger during a hold is a driving command and
    /// nothing else. [`route`] has the keys that differ by screen.
    pub fn key(self) -> Option<Key> {
        Some(match self {
            Gesture::Tap => Key::Comma,
            Gesture::DoubleTap | Gesture::Activate => Key::Return,
            Gesture::SwipeUp | Gesture::Increment => Key::Up,
            // A deep swipe off the road is just a long flick down.
            Gesture::SwipeDown | Gesture::Decrement | Gesture::DeepSwipeDownBegan => Key::Down,
            Gesture::SwipeLeft => Key::Left,
            Gesture::SwipeRight => Key::Right,
            // Stop speech, the way VoiceOver's two-finger tap does.
            Gesture::TwoFingerTap => Key::LCtrl,
            Gesture::MagicTap => Key::Space,
            Gesture::TwoFingerSwipeUp => Key::F1,
            Gesture::TwoFingerSwipeDown | Gesture::Escape => Key::Escape,
            Gesture::TwoFingerSwipeLeft => Key::Comma,
            Gesture::TwoFingerSwipeRight => Key::Period,
            Gesture::ThreeFingerSwipeUp => Key::Home,
            Gesture::ThreeFingerSwipeDown => Key::End,
            Gesture::ThreeFingerSwipeLeft => Key::PageUp,
            Gesture::ThreeFingerSwipeRight => Key::PageDown,
            // What this screen can tell you: Tab on the job board and the
            // download screen.
            Gesture::ThreeFingerTap => Key::Tab,
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
        self.key().map(key_press).unwrap_or_default()
    }

    /// The name the agent server's `gesture` tool takes.
    pub fn name(self) -> &'static str {
        NAMES
            .iter()
            .find(|(gesture, _)| *gesture == self)
            .map_or("", |(_, name)| name)
    }

    /// The gesture an agent names.
    pub fn from_name(name: &str) -> Option<Gesture> {
        NAMES
            .iter()
            .find(|(_, n)| *n == name)
            .map(|(gesture, _)| *gesture)
    }

    /// Every gesture the agent server can send, by name.
    pub fn names() -> impl Iterator<Item = &'static str> {
        NAMES.iter().map(|(_, name)| *name)
    }
}

/// The agent server's names, one per gesture the screen can send, plus the
/// keyboard notices, so a session can put spoken hints on gestures.
const NAMES: &[(Gesture, &str)] = &[
    (Gesture::KeyboardDisconnected, "keyboard_disconnected"),
    (Gesture::KeyboardConnected, "keyboard_connected"),
    (Gesture::Tap, "tap"),
    (Gesture::DoubleTap, "double_tap"),
    (Gesture::SwipeUp, "flick_up"),
    (Gesture::SwipeDown, "flick_down"),
    (Gesture::SwipeLeft, "flick_left"),
    (Gesture::SwipeRight, "flick_right"),
    (Gesture::TwoFingerTap, "two_finger_tap"),
    (Gesture::MagicTap, "two_finger_double_tap"),
    (Gesture::Escape, "two_finger_scrub"),
    (Gesture::TwoFingerSwipeUp, "two_finger_flick_up"),
    (Gesture::TwoFingerSwipeDown, "two_finger_flick_down"),
    (Gesture::TwoFingerSwipeLeft, "two_finger_flick_left"),
    (Gesture::TwoFingerSwipeRight, "two_finger_flick_right"),
    (Gesture::HornHoldBegan, "two_finger_hold"),
    (Gesture::RotateRight, "two_finger_turn_right"),
    (Gesture::RotateLeft, "two_finger_turn_left"),
    (Gesture::ThreeFingerTap, "three_finger_tap"),
    (Gesture::ThreeFingerSwipeUp, "three_finger_flick_up"),
    (Gesture::ThreeFingerSwipeDown, "three_finger_flick_down"),
    (Gesture::ThreeFingerSwipeLeft, "three_finger_flick_left"),
    (Gesture::ThreeFingerSwipeRight, "three_finger_flick_right"),
    (Gesture::HoldUpperBegan, "gas_hold"),
    (Gesture::HoldLowerBegan, "brake_hold"),
    (Gesture::HoldLeftBegan, "steer_left_hold"),
    (Gesture::HoldRightBegan, "steer_right_hold"),
    (Gesture::DeepSwipeDownBegan, "deep_swipe"),
    (Gesture::StillHoldBegan, "still_hold"),
    (Gesture::HoldEnded, "lift"),
    (Gesture::UpperHoldTap, "gas_hold_tap"),
    (Gesture::UpperHoldDoubleTap, "gas_hold_double_tap"),
    (Gesture::UpperHoldSwipeUp, "gas_hold_flick_up"),
    (Gesture::UpperHoldSwipeDown, "gas_hold_flick_down"),
    (Gesture::UpperHoldSwipeLeft, "gas_hold_flick_left"),
    (Gesture::UpperHoldSwipeRight, "gas_hold_flick_right"),
    (Gesture::LowerHoldTap, "brake_hold_tap"),
    (Gesture::LowerHoldDoubleTap, "brake_hold_double_tap"),
    (Gesture::LowerHoldSwipeUp, "brake_hold_flick_up"),
    (Gesture::LowerHoldSwipeDown, "brake_hold_flick_down"),
    (Gesture::LowerHoldSwipeLeft, "brake_hold_flick_left"),
    (Gesture::LowerHoldSwipeRight, "brake_hold_flick_right"),
    (Gesture::SecondSteerLeftBegan, "second_finger_steer_left"),
    (Gesture::SecondSteerRightBegan, "second_finger_steer_right"),
    (Gesture::SecondSteerEnded, "second_finger_lift"),
    (Gesture::EmergencyBrakeHoldBegan, "emergency_brake_hold"),
    (Gesture::Activate, "voiceover_double_tap"),
    (Gesture::Increment, "voiceover_swipe_up"),
    (Gesture::Decrement, "voiceover_swipe_down"),
];

/// What the app does with one gesture on one kind of screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Route {
    /// The hardware keyboard came or went; hints follow it.
    Keyboard,
    /// Practice gestures names it, and nothing moves.
    Practice,
    /// Nothing happens and nothing is said.
    Nothing,
    /// Stop speech, whatever the screen.
    StopSpeech,
    /// Press and release this key; the screen is not asked first.
    Key(Key),
    /// The screen answers the gesture if it wants to; otherwise press this
    /// key, or do nothing.
    Screen(Option<Key>),
    /// Hold this key until the finger lifts.
    HoldKey(Key),
    /// Steer with this key until the finger lifts, unless full lane keeping
    /// leaves the lanes to the flicks.
    Steer(Key),
    /// The drive answers first (gas releases a set parking brake), then the
    /// key is held.
    DriveThenHoldKey(Key),
    /// The drive answers first (a deep swipe at a stop is the parking
    /// brake); if it lets the gesture through, the action's bound chord is
    /// held.
    DriveThenBoundHold(Action),
    /// Hold the chord the player has this action on until the finger lifts.
    BoundHold(Action),
    /// A finger lifted: let go of a bound hold, or tell the screen.
    Lift,
}

/// The one place a gesture's meaning is chosen, by the screen's mode.
pub fn route(mode: TouchMode, gesture: Gesture) -> Route {
    use Gesture as G;
    let wheel = mode == TouchMode::Wheel;
    match gesture {
        G::KeyboardConnected | G::KeyboardDisconnected => Route::Keyboard,
        _ if mode == TouchMode::Practice => Route::Practice,
        G::HoldEnded => Route::Lift,
        G::TwoFingerTap if wheel => Route::Key(Key::LCtrl),
        G::TwoFingerTap => Route::StopSpeech,
        G::HoldUpperBegan if wheel => Route::DriveThenHoldKey(Key::Up),
        G::DeepSwipeDownBegan if wheel => Route::DriveThenBoundHold(Action::EmergencyBrake),
        G::StillHoldBegan if wheel => Route::BoundHold(Action::Straighten),
        G::HornHoldBegan if wheel => Route::BoundHold(Action::Horn),
        G::EmergencyBrakeHoldBegan if wheel => Route::BoundHold(Action::EmergencyBrake),
        G::StillHoldBegan | G::HornHoldBegan | G::EmergencyBrakeHoldBegan => Route::Nothing,
        G::ThreeFingerTap if mode != TouchMode::Menu => Route::Screen(Some(Key::F2)),
        held if held.steers() && wheel => Route::Steer(held.held_key().unwrap_or(Key::Left)),
        held => match held.held_key() {
            Some(key) => Route::HoldKey(key),
            None => Route::Screen(held.key()),
        },
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

/// A key pressed and let go, as a gesture that stands for one.
pub fn key_press(key: Key) -> Vec<InputEvent> {
    vec![
        key_down(key),
        InputEvent::KeyUp {
            key,
            mods: Mods::NONE,
        },
    ]
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
mod tests;
