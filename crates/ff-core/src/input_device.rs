//! Which input the spoken prompts name: the keyboard, a controller or the
//! touch screen.
//!
//! Prompts name the control the player is using ("press X", "D-pad down",
//! "a double tap"), so the game keeps track of what was used last and what
//! is plugged in. This is that machine. [`InputDevice::next`] is the only
//! place it moves, and [`InputDevice::device`] is the answer the prompts
//! read, as one of the [`crate::input_hints`] device names.
//!
//! The rules, which predate the machine and which its test table replays:
//!
//! * A key press, a touch, or a press on the bound controller makes that
//!   input the last one used.
//! * Prompts name the controller only while one is connected and it was
//!   used last. A controller that goes away and comes back is named again
//!   without a press, because nothing else was used meanwhile.
//! * Prompts name gestures when the screen was used last, and always while
//!   no hardware keyboard is connected: there is no key to name.
//! * Turning controller support off forgets the controller and goes back to
//!   the device the game starts on: the screen on a phone, else the
//!   keyboard.
//!
//! A gesture the screen plays as key presses is a touch, not a key press;
//! the caller sends [`InputEvent::Touch`] for it and no `KeyPress`.

use crate::input_hints::{CONTROLLER, KEYBOARD, TOUCH};

/// One input the prompts can name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Device {
    Keyboard,
    Controller,
    Touch,
}

impl Device {
    /// The [`crate::input_hints`] device name.
    pub fn key(self) -> &'static str {
        match self {
            Device::Keyboard => KEYBOARD,
            Device::Controller => CONTROLLER,
            Device::Touch => TOUCH,
        }
    }
}

/// What moves the machine.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputEvent {
    KeyPress,
    ControllerPress,
    Touch,
    KeyboardConnected,
    KeyboardDisconnected,
    ControllerConnected,
    ControllerDisconnected,
    /// Controller support switched off in Settings.
    ControllerOff,
}

/// The last input used and what is connected.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InputDevice {
    last: Device,
    keyboard: bool,
    controller: bool,
    phone: bool,
}

impl InputDevice {
    /// How the game starts: on a phone the screen with no keyboard, else
    /// the keyboard. No controller is bound yet either way.
    pub fn new(phone: bool) -> Self {
        Self {
            last: Self::start_device(phone),
            keyboard: !phone,
            controller: false,
            phone,
        }
    }

    fn start_device(phone: bool) -> Device {
        if phone {
            Device::Touch
        } else {
            Device::Keyboard
        }
    }

    /// Move on `event`. Returns the device the prompts name now when it
    /// changed, so a caller can answer one transition once.
    pub fn next(&mut self, event: InputEvent) -> Option<Device> {
        let before = self.device();
        match event {
            InputEvent::KeyPress => self.last = Device::Keyboard,
            // A press from a pad that is not bound never reaches the game.
            InputEvent::ControllerPress if self.controller => self.last = Device::Controller,
            InputEvent::ControllerPress => {}
            InputEvent::Touch => self.last = Device::Touch,
            InputEvent::KeyboardConnected => self.keyboard = true,
            InputEvent::KeyboardDisconnected => self.keyboard = false,
            InputEvent::ControllerConnected => self.controller = true,
            InputEvent::ControllerDisconnected => self.controller = false,
            InputEvent::ControllerOff => {
                self.controller = false;
                self.last = Self::start_device(self.phone);
            }
        }
        let after = self.device();
        (after != before).then_some(after)
    }

    /// The input the prompts name.
    pub fn device(&self) -> Device {
        if self.controller && self.last == Device::Controller {
            Device::Controller
        } else if self.last == Device::Touch || !self.keyboard {
            Device::Touch
        } else {
            Device::Keyboard
        }
    }

    /// The input used last, whether or not it is still connected.
    pub fn last_used(&self) -> Device {
        self.last
    }

    pub fn keyboard_connected(&self) -> bool {
        self.keyboard
    }

    pub fn controller_connected(&self) -> bool {
        self.controller
    }

    /// Whether this run is a phone or tablet.
    pub fn phone(&self) -> bool {
        self.phone
    }
}

#[cfg(test)]
mod tests {
    use super::Device::{Controller as C, Keyboard as K, Touch as T};
    use super::InputEvent::*;
    use super::*;

    /// (phone, events, device after each event). Each row replays a
    /// history through the rules the controller manager applied before the
    /// machine existed.
    #[allow(clippy::type_complexity)]
    const TABLE: &[(bool, &[(InputEvent, Device)])] = &[
        // Desktop: the keyboard until a pad is connected and pressed.
        (
            false,
            &[
                (Touch, T),
                (KeyPress, K),
                (ControllerPress, K),
                (ControllerConnected, K),
                (ControllerPress, C),
                (KeyPress, K),
                (ControllerPress, C),
            ],
        ),
        // A pad that drops and comes back is named again without a press.
        (
            false,
            &[
                (ControllerConnected, K),
                (ControllerPress, C),
                (ControllerDisconnected, K),
                (ControllerConnected, C),
            ],
        ),
        // Switching support off forgets the pad and goes back to keys.
        (
            false,
            &[
                (ControllerConnected, K),
                (ControllerPress, C),
                (ControllerOff, K),
                (ControllerConnected, K),
                (Touch, T),
                (ControllerOff, K),
            ],
        ),
        // Desktop with the keyboard unplugged names gestures whatever was
        // last used, except a connected pad.
        (
            false,
            &[
                (KeyboardDisconnected, T),
                (KeyPress, T),
                (ControllerConnected, T),
                (ControllerPress, C),
                (ControllerDisconnected, T),
                (KeyboardConnected, K),
            ],
        ),
        // Phone: gestures from the start; a hardware keyboard names keys
        // once it is used, and gestures again on the next touch.
        (
            true,
            &[
                (KeyPress, T),
                (KeyboardConnected, K),
                (Touch, T),
                (KeyPress, K),
                (KeyboardDisconnected, T),
                (KeyboardConnected, K),
            ],
        ),
        // Phone with a pad: named while used, and support off goes back to
        // the screen.
        (
            true,
            &[
                (ControllerConnected, T),
                (ControllerPress, C),
                (Touch, T),
                (ControllerPress, C),
                (ControllerOff, T),
            ],
        ),
    ];

    #[test]
    fn transition_table() {
        for (row, (phone, steps)) in TABLE.iter().enumerate() {
            let mut machine = InputDevice::new(*phone);
            for (step, (event, want)) in steps.iter().enumerate() {
                machine.next(*event);
                assert_eq!(
                    machine.device(),
                    *want,
                    "row {row} step {step}: after {event:?}"
                );
            }
        }
    }

    #[test]
    fn starts_on_the_screen_on_a_phone_and_the_keyboard_elsewhere() {
        assert_eq!(InputDevice::new(true).device(), T);
        assert!(!InputDevice::new(true).keyboard_connected());
        assert_eq!(InputDevice::new(false).device(), K);
        assert!(InputDevice::new(false).keyboard_connected());
    }

    #[test]
    fn next_reports_only_a_change() {
        let mut machine = InputDevice::new(false);
        assert_eq!(machine.next(KeyPress), None);
        assert_eq!(machine.next(Touch), Some(T));
        assert_eq!(machine.next(Touch), None);
        assert_eq!(machine.next(KeyPress), Some(K));
    }

    #[test]
    fn device_keys_are_the_hint_names() {
        assert_eq!(K.key(), "keyboard");
        assert_eq!(C.key(), "controller");
        assert_eq!(T.key(), "touch");
    }
}
