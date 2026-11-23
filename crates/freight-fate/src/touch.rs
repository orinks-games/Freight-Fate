//! Touch gestures: the iOS game's input surface.
//!
//! The iPhone and iPad game is the desktop game, spoken through Prism and
//! driven by the same states. The gestures come from `ios/ff_touch.m`'s
//! native recognizers (with VoiceOver on, through a direct-interaction
//! element), or, with New gesture reading on and always on Android, from
//! [`ff_core::touch::recognizer`] over raw fingers ([`fingers`]);
//! [`TouchInput`] turns them into [`InputEvent`]s. A flick is one
//! step, a swipe and hold is continuous: a stroke held in a direction is a
//! held key (gas, brake, steering) until it lifts. Every other gesture
//! arrives as [`InputEvent::Gesture`], and [`route`] says what the app does
//! with it on the kind of screen on top ([`TouchMode`]): at the wheel the
//! drive runs the command the player's touch bindings give it, menus move
//! the way VoiceOver does, and every other screen, or a gesture with no
//! binding, gets the key a keyboard player would press. Nothing here touches
//! UIKit, so the whole mapping is tested on every platform.

pub mod fingers;

/// The recognizer moved to `ff_core::touch`; this path keeps working.
pub mod recognizer {
    pub use ff_core::touch::recognizer::*;
}

use crate::bindings::Action;
use crate::states::base::{InputEvent, Key, Mods};
pub use ff_core::touch::{
    FingerEvent, FingerKind, Gesture, Haptic, MAX_TRAVEL_POINTS, TRAVEL_CODE_BASE,
};
pub use ff_core::touch_mode::{RoadOffer, TouchMode};

/// What a gesture means as keys: the game's side of [`Gesture`], which
/// lives in `ff-core` with the recognizer and knows nothing of keys.
pub trait GestureKeys: Copy {
    fn key(self) -> Option<Key>;
    fn held_key(self) -> Option<Key>;
    fn hold_events(self) -> Vec<InputEvent>;
    fn key_events(self) -> Vec<InputEvent>;
}

impl GestureKeys for Gesture {
    /// The key a single press of this gesture stands for, if it is a press.
    /// A second finger during a hold is a driving command and nothing else.
    /// [`route`] has the keys that differ by screen: menus move on the other
    /// axis, the way VoiceOver does.
    fn key(self) -> Option<Key> {
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
            | Gesture::Travel(_)
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
    fn held_key(self) -> Option<Key> {
        match self {
            Gesture::HoldUpperBegan => Some(Key::Up),
            Gesture::HoldLowerBegan => Some(Key::Down),
            Gesture::HoldLeftBegan | Gesture::SecondSteerLeftBegan => Some(Key::Left),
            Gesture::HoldRightBegan | Gesture::SecondSteerRightBegan => Some(Key::Right),
            _ => None,
        }
    }

    /// The press that starts [`Self::held_key`].
    fn hold_events(self) -> Vec<InputEvent> {
        self.held_key().map(key_down).into_iter().collect()
    }

    /// The press and release of [`Self::key`], for a screen that did not
    /// take the gesture itself.
    fn key_events(self) -> Vec<InputEvent> {
        self.key().map(key_press).unwrap_or_default()
    }
}

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
        // Only practice reads a swipe's length.
        G::Travel(_) => Route::Nothing,
        G::HoldEnded => Route::Lift,
        _ if mode == TouchMode::Menu => menu_route(gesture),
        // A tap, a magic tap and the message flicks would type a comma, a
        // space or a period into the name.
        G::Tap | G::MagicTap | G::TwoFingerSwipeLeft | G::TwoFingerSwipeRight
            if mode == TouchMode::Field =>
        {
            Route::Nothing
        }
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

/// A menu, the way VoiceOver moves: flick right and left step through the
/// rows, flick up and down change the value on the row. A swipe and hold,
/// and the deep swipe, are the flick in their direction, pressed once.
/// VoiceOver's own swipes on the adjustable element (with direct touch off)
/// stay on Up and Down, since they are then the only way through a list.
/// Tap reads the focused row again and the two-finger double tap pauses, so
/// both go to the screen and press nothing it does not take.
fn menu_route(gesture: Gesture) -> Route {
    use Gesture as G;
    match gesture {
        G::SwipeRight | G::HoldRightBegan | G::SecondSteerRightBegan => {
            Route::Screen(Some(Key::Down))
        }
        G::SwipeLeft | G::HoldLeftBegan | G::SecondSteerLeftBegan => Route::Screen(Some(Key::Up)),
        G::SwipeUp | G::HoldUpperBegan => Route::Screen(Some(Key::Right)),
        G::SwipeDown | G::HoldLowerBegan | G::DeepSwipeDownBegan => Route::Screen(Some(Key::Left)),
        G::Tap | G::MagicTap => Route::Screen(None),
        G::TwoFingerTap => Route::StopSpeech,
        G::StillHoldBegan | G::HornHoldBegan | G::EmergencyBrakeHoldBegan => Route::Nothing,
        other => Route::Screen(other.key()),
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
