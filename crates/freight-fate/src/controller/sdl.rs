//! The SDL side of the controller seam: [`PadDevice`] / [`PadSubsystem`]
//! over `sdl2::controller`, the factory the app hands the manager, and the
//! conversions between SDL's enums and the game's.

use sdl2::controller::{Axis, Button, GameController};
use sdl2::GameControllerSubsystem;

use super::{ControllerAxis, ControllerButton, PadDevice, PadSubsystem, PadSubsystemFactory};

/// An opened `GameController`. Dropping it closes the device, which is what
/// `Controller.quit()` did.
pub struct SdlPad {
    controller: GameController,
}

impl PadDevice for SdlPad {
    fn instance_id(&self) -> u32 {
        self.controller.instance_id()
    }

    fn attached(&self) -> bool {
        self.controller.attached()
    }

    fn rumble(&mut self, low: u16, high: u16, duration_ms: u32) {
        // Driver dependent: a pad without motors answers with an error,
        // which is not worth a log line per frame.
        let _ = self.controller.set_rumble(low, high, duration_ms);
    }

    fn stop_rumble(&mut self) {
        let _ = self.controller.set_rumble(0, 0, 0);
    }

    fn mapping(&self) -> Option<String> {
        let mapping = self.controller.mapping();
        (!mapping.is_empty()).then_some(mapping)
    }
}

/// The game controller subsystem. Dropping it quits the subsystem
/// (`sdl_controller.quit()`).
pub struct SdlPadSubsystem {
    subsystem: GameControllerSubsystem,
}

impl SdlPadSubsystem {
    pub fn new(subsystem: GameControllerSubsystem) -> Self {
        Self { subsystem }
    }
}

impl PadSubsystem for SdlPadSubsystem {
    fn count(&self) -> u32 {
        self.subsystem.num_joysticks().unwrap_or(0)
    }

    fn is_controller(&self, index: u32) -> bool {
        self.subsystem.is_game_controller(index)
    }

    fn name_for_index(&self, index: u32) -> Option<String> {
        self.subsystem.name_for_index(index).ok()
    }

    fn open(&self, index: u32) -> Option<Box<dyn PadDevice>> {
        match self.subsystem.open(index) {
            Ok(controller) => Some(Box::new(SdlPad { controller })),
            Err(e) => {
                log::debug!("Could not open controller at slot {index}: {e}");
                None
            }
        }
    }
}

/// A factory that brings the SDL game controller subsystem up on demand.
/// Every failure (no SDL, dummy drivers) is "no subsystem": keyboard only.
pub fn sdl_factory(sdl: sdl2::Sdl) -> PadSubsystemFactory {
    Box::new(move || match sdl.game_controller() {
        Ok(subsystem) => Some(Box::new(SdlPadSubsystem::new(subsystem)) as Box<dyn PadSubsystem>),
        Err(e) => {
            log::info!("Controller subsystem unavailable; keyboard only: {e}");
            None
        }
    })
}

pub fn button_from_sdl(button: Button) -> ControllerButton {
    match button {
        Button::A => ControllerButton::A,
        Button::B => ControllerButton::B,
        Button::X => ControllerButton::X,
        Button::Y => ControllerButton::Y,
        Button::Back => ControllerButton::Back,
        Button::Guide => ControllerButton::Guide,
        Button::Start => ControllerButton::Start,
        Button::LeftStick => ControllerButton::LeftStick,
        Button::RightStick => ControllerButton::RightStick,
        Button::LeftShoulder => ControllerButton::LeftShoulder,
        Button::RightShoulder => ControllerButton::RightShoulder,
        Button::DPadUp => ControllerButton::DPadUp,
        Button::DPadDown => ControllerButton::DPadDown,
        Button::DPadLeft => ControllerButton::DPadLeft,
        Button::DPadRight => ControllerButton::DPadRight,
        Button::Misc1 => ControllerButton::Misc1,
        Button::Paddle1 => ControllerButton::Paddle1,
        Button::Paddle2 => ControllerButton::Paddle2,
        Button::Paddle3 => ControllerButton::Paddle3,
        Button::Paddle4 => ControllerButton::Paddle4,
        Button::Touchpad => ControllerButton::Touchpad,
    }
}

pub fn axis_from_sdl(axis: Axis) -> ControllerAxis {
    match axis {
        Axis::LeftX => ControllerAxis::LeftX,
        Axis::LeftY => ControllerAxis::LeftY,
        Axis::RightX => ControllerAxis::RightX,
        Axis::RightY => ControllerAxis::RightY,
        Axis::TriggerLeft => ControllerAxis::TriggerLeft,
        Axis::TriggerRight => ControllerAxis::TriggerRight,
    }
}

/// Controller detection that takes this long and finds nothing has stalled
/// on a device, not set up a pad: a few hundred milliseconds is usual.
pub const STALLED_DETECTION: std::time::Duration = std::time::Duration::from_secs(5);

/// Whether a detection that took `elapsed` and bound no pad stalled.
pub fn detection_stalled(elapsed: std::time::Duration, pad_found: bool) -> bool {
    !pad_found && elapsed >= STALLED_DETECTION
}

/// Redo a stalled controller detection without DirectInput.
///
/// Issue 519: on one player's PC the controller step of start-up took 40
/// seconds with no controller found, and the game then froze for 40 seconds
/// at a time whenever a Bluetooth keyboard was turned off or the PC locked:
/// each device change makes SDL enumerate again, on the game loop.
/// DirectInput's enumeration is the step known to stall on a paired
/// Bluetooth device that is switched off. Xbox pads still come through
/// XInput and PlayStation and Switch pads through HIDAPI; only an older
/// DirectInput-only pad is lost, on a PC where it cost a 40-second freeze.
/// Both timings go in the log, so a stall that is not DirectInput shows.
/// SDL reads the hint on Windows alone; elsewhere the second detection only
/// adds its timing to the log.
pub fn redo_a_stalled_detection(
    manager: &mut crate::controller::ControllerManager,
    elapsed: std::time::Duration,
) {
    if !manager.enabled() || !detection_stalled(elapsed, manager.connected()) {
        return;
    }
    log::warn!(
        "Controller detection took {:.1} s and found no controller; detecting again without DirectInput",
        elapsed.as_secs_f64()
    );
    manager.set_enabled(false);
    sdl2::hint::set("SDL_DIRECTINPUT_ENABLED", "0");
    let again = std::time::Instant::now();
    manager.set_enabled(true);
    log::warn!(
        "Controller detection without DirectInput took {:.1} s",
        again.elapsed().as_secs_f64()
    );
}

#[cfg(test)]
mod stalled_detection_tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn only_a_long_detection_that_found_nothing_counts_as_stalled() {
        assert!(detection_stalled(Duration::from_secs(40), false));
        assert!(detection_stalled(STALLED_DETECTION, false));
        assert!(!detection_stalled(Duration::from_millis(300), false));
        // A pad that took a while to come up is a pad, and keeps DirectInput.
        assert!(!detection_stalled(Duration::from_secs(40), true));
    }

    #[test]
    fn a_quick_detection_is_left_alone() {
        let mut manager = crate::controller::ControllerManager::detached(true, false);
        redo_a_stalled_detection(&mut manager, Duration::from_millis(50));
        assert!(manager.enabled());
    }
}
