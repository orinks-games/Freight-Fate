//! Android's touch surface: SDL's finger events through the gesture
//! recognizer, into the same [`TouchInput`] the iOS gestures feed.
//!
//! iOS recognizes gestures natively (`ios/ff_touch.m`); Android has no
//! UIKit, so SDL hands over each finger's raw events and
//! [`Recognizer`](crate::touch::recognizer::Recognizer) applies the iOS
//! rules to them. Positions are converted from pixels to points with the
//! screen's density (a point is 1/160 of an inch, the size of an Android
//! dp), so the settings' thresholds mean the same distance on both phones.
//! Each gesture gets the iOS haptic: a tick for a step, a bump for a hold
//! starting or ending, a heavy click for the deep swipe, a double click for
//! the engine key.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use sdl2::event::Event;
use sdl2::keyboard::Keycode;

use crate::states::base::InputEvent;
use crate::touch::recognizer::{Recognizer, Tuning};
use crate::touch::{Gesture, TouchInput};

/// Haptics on or off, from the Touch gestures settings.
static HAPTICS: AtomicBool = AtomicBool::new(true);
/// The thresholds from the settings, applied before the next event.
static TUNING: Mutex<Option<Tuning>> = Mutex::new(None);

pub(crate) fn set_haptics(enabled: bool) {
    HAPTICS.store(enabled, Ordering::Relaxed);
}

pub(crate) fn set_tuning(settings: &ff_core::settings::Settings) {
    let tuning = Tuning::new(
        settings.touch_flick_points,
        settings.touch_flick_ms,
        settings.touch_deep_swipe_factor,
        settings.touch_still_hold_ms,
        settings.touch_rotate_degrees,
    );
    *TUNING.lock().unwrap_or_else(|e| e.into_inner()) = Some(tuning);
}

/// How often the hardware keyboard is looked for, in frames' worth of ms.
const KEYBOARD_CHECK_MS: u32 = 1000;

pub(crate) struct AndroidTouch {
    recognizer: Recognizer,
    /// Pixels per point: the screen's density.
    density: f32,
    keyboard: Option<bool>,
    keyboard_checked_ms: Option<u32>,
}

impl AndroidTouch {
    pub(crate) fn new() -> Self {
        let density = crate::android::display_density();
        log::info!("touch: {density} pixels to the point");
        Self {
            recognizer: Recognizer::default(),
            density: if density > 0.0 { density } else { 1.0 },
            keyboard: None,
            keyboard_checked_ms: None,
        }
    }

    /// Practice gestures is open: report each hold's length as it lifts.
    pub(crate) fn set_report_travel(&mut self, on: bool) {
        self.recognizer.set_report_travel(on);
    }

    /// Recognize this batch's finger events and return the game events the
    /// gestures make, after `events` (the batch's keys and window events).
    /// `window` is the window's size in pixels; `now_ms` is SDL's clock.
    pub(crate) fn feed(
        &mut self,
        raw: &[Event],
        window: (u32, u32),
        now_ms: u32,
        touch: &mut TouchInput,
        mut events: Vec<InputEvent>,
    ) -> Vec<InputEvent> {
        if let Some(tuning) = TUNING.lock().unwrap_or_else(|e| e.into_inner()).take() {
            self.recognizer.set_tuning(tuning);
        }
        if events
            .iter()
            .any(|event| matches!(event, InputEvent::WindowFocusLost))
        {
            self.recognizer.cancel();
            touch.release_into(&mut events);
        }
        let mut gestures: Vec<Gesture> = Vec::new();
        self.check_keyboard(now_ms, &mut gestures);
        let (width, height) = (window.0 as f32, window.1 as f32);
        for event in raw {
            match *event {
                Event::FingerDown {
                    timestamp,
                    finger_id,
                    x,
                    y,
                    ..
                } => {
                    let (x, y) = self.points(x * width, y * height);
                    self.recognizer.down(finger_id, x, y, seconds(timestamp));
                }
                Event::FingerMotion {
                    timestamp,
                    finger_id,
                    x,
                    y,
                    ..
                } => {
                    let (x, y) = self.points(x * width, y * height);
                    self.recognizer.moved(finger_id, x, y, seconds(timestamp));
                }
                Event::FingerUp {
                    timestamp,
                    finger_id,
                    x,
                    y,
                    ..
                } => {
                    let (x, y) = self.points(x * width, y * height);
                    self.recognizer.up(finger_id, x, y, seconds(timestamp));
                }
                // Android's back (the back gesture or button), trapped so it
                // does not close the game: it goes back, as the two-finger
                // scrub does.
                Event::KeyDown {
                    keycode: Some(Keycode::AC_BACK),
                    repeat: false,
                    ..
                } => gestures.push(Gesture::Escape),
                _ => {}
            }
        }
        self.recognizer.tick(seconds(now_ms));
        gestures.extend(self.recognizer.drain());
        for gesture in gestures {
            if HAPTICS.load(Ordering::Relaxed) {
                if let Some(kind) = haptic_kind(gesture) {
                    crate::android::haptic(kind);
                }
            }
            events.extend(touch.handle(gesture).events);
        }
        events
    }

    fn points(&self, x: f32, y: f32) -> (f32, f32) {
        (x / self.density, y / self.density)
    }

    /// Report a hardware keyboard arriving or leaving, as iOS does.
    fn check_keyboard(&mut self, now_ms: u32, gestures: &mut Vec<Gesture>) {
        if self
            .keyboard_checked_ms
            .is_some_and(|at| now_ms.wrapping_sub(at) < KEYBOARD_CHECK_MS)
        {
            return;
        }
        self.keyboard_checked_ms = Some(now_ms);
        let connected = crate::android::hardware_keyboard_connected();
        if self.keyboard != Some(connected) {
            self.keyboard = Some(connected);
            gestures.push(if connected {
                Gesture::KeyboardConnected
            } else {
                Gesture::KeyboardDisconnected
            });
        }
    }
}

fn seconds(ms: u32) -> f64 {
    f64::from(ms) / 1000.0
}

/// The haptic each gesture gets, as `ios/ff_touch.m` assigns them:
/// 0 a bump (a hold starting or ending), 1 a tick (one step), 2 a warning
/// (the deep swipe), 3 a success (the engine key).
fn haptic_kind(gesture: Gesture) -> Option<i32> {
    Some(match gesture {
        Gesture::KeyboardConnected | Gesture::KeyboardDisconnected | Gesture::Travel(_) => {
            return None
        }
        Gesture::HoldUpperBegan
        | Gesture::HoldLowerBegan
        | Gesture::HoldLeftBegan
        | Gesture::HoldRightBegan
        | Gesture::StillHoldBegan
        | Gesture::HornHoldBegan
        | Gesture::SecondSteerLeftBegan
        | Gesture::SecondSteerRightBegan
        | Gesture::SecondSteerEnded
        | Gesture::HoldEnded => 0,
        Gesture::DeepSwipeDownBegan | Gesture::EmergencyBrakeHoldBegan => 2,
        Gesture::RotateRight | Gesture::RotateLeft => 3,
        _ => 1,
    })
}
