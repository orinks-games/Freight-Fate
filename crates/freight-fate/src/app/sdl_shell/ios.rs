//! The iOS side of the window shell: the calls into the UIKit gesture and
//! VoiceOver bridge (`crates/freight-fate/ios/ff_touch.m`, compiled by
//! `build.rs`) that install it, hand it settings and drain its gesture queue,
//! and [`SharedFingers`], the raw finger queue (`ios/ff_fingers.m`) read by
//! the shared recognizer while Touch gestures, New gesture reading is on.

use std::sync::Mutex;

use crate::states::base::InputEvent;
use crate::touch::recognizer::{Recognizer, Tuning};
use crate::touch::{fingers, FingerEvent, FingerKind, Gesture, TouchInput};

extern "C" {
    fn ff_use_hardware_vertical_arrows(window: *mut std::ffi::c_void);
    fn ff_touch_install(window: *mut std::ffi::c_void) -> i32;
    fn ff_touch_voice_over_running() -> i32;
    fn ff_touch_direct_interaction_suspended() -> i32;
    fn ff_touch_software_keyboard_shown() -> i32;
    fn ff_touch_set_field_review(label: *const u8, label_len: usize, text: *const u8, len: usize);
    fn ff_refresh_orientations(window: *mut std::ffi::c_void);
    #[cfg(feature = "visuals")]
    fn ff_phone_safe_top_fraction(window: *mut std::ffi::c_void) -> f64;
    fn ff_touch_next() -> i32;
    fn ff_touch_set_haptics(enabled: i32);
    fn ff_touch_haptic(kind: i32);
    fn ff_fingers_set_shared(on: i32);
    fn ff_fingers_next(
        finger: *mut i64,
        kind: *mut i32,
        x: *mut f32,
        y: *mut f32,
        t: *mut f64,
    ) -> i32;
    fn ff_fingers_now() -> f64;
    fn ff_touch_set_report_travel(enabled: i32);
    fn ff_touch_set_tuning(
        flick_points: f64,
        flick_ms: f64,
        deep_factor: f64,
        still_ms: f64,
        rotate_degrees: f64,
    );
}

pub(crate) fn voice_over_running() -> bool {
    // SAFETY: a UIKit status query on the app thread, with no arguments.
    unsafe { ff_touch_voice_over_running() != 0 }
}

pub(crate) fn direct_interaction_suspended() -> bool {
    // SAFETY: reads the UIKit touch surface on the app thread.
    unsafe { ff_touch_direct_interaction_suspended() != 0 }
}

pub(crate) fn software_keyboard_shown() -> bool {
    // SAFETY: reads keyboard notification state on the app thread.
    unsafe { ff_touch_software_keyboard_shown() != 0 }
}

pub(super) fn set_field_review(text: Option<&str>, label: impl FnOnce() -> String) {
    // SAFETY: UIKit is called on the app thread and copies the UTF-8 bytes
    // within the call. A null text pointer removes the element.
    unsafe {
        if let Some(text) = text {
            let label = label();
            ff_touch_set_field_review(label.as_ptr(), label.len(), text.as_ptr(), text.len());
        } else {
            ff_touch_set_field_review(std::ptr::null(), 0, std::ptr::null(), 0);
        }
    }
}

pub(crate) fn set_touch_haptics(enabled: bool) {
    // SAFETY: the UIKit shim accepts an integer flag on the app thread.
    unsafe { ff_touch_set_haptics(i32::from(enabled)) };
}

/// Tell the gesture surface whether to report how far each hold travelled.
pub(super) fn set_report_travel(enabled: bool) {
    // SAFETY: the UIKit shim stores an integer flag on the app thread.
    unsafe { ff_touch_set_report_travel(i32::from(enabled)) };
}

/// The thresholds for the shared recognizer, taken before its next read.
static SHARED_TUNING: Mutex<Option<Tuning>> = Mutex::new(None);

/// Hand the gesture thresholds in the settings to the native recognizers,
/// and to the shared one.
pub(crate) fn set_touch_tuning(settings: &ff_core::settings::Settings) {
    *SHARED_TUNING.lock().unwrap_or_else(|e| e.into_inner()) = Some(fingers::tuning(settings));
    // SAFETY: the UIKit shim copies five plain numbers on the app thread.
    unsafe {
        ff_touch_set_tuning(
            settings.touch_flick_points,
            settings.touch_flick_ms,
            settings.touch_deep_swipe_factor,
            settings.touch_still_hold_ms,
            settings.touch_rotate_degrees,
        )
    };
}

fn ui_window(window: &sdl2::video::Window) -> Option<*mut std::ffi::c_void> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    match window.window_handle().map(|handle| handle.as_raw()) {
        // sdl2 reports the UIWindow in the `ui_view` slot.
        Ok(RawWindowHandle::UiKit(handle)) => Some(handle.ui_view.as_ptr()),
        _ => None,
    }
}

/// Ask UIKit to read the supported orientations again, after tilt steering
/// changed the hint SDL answers them from (`ios/ff_orientation.m`).
pub(super) fn refresh_orientations(window: &sdl2::video::Window) {
    let Some(ui_window) = ui_window(window) else {
        return;
    };
    // SAFETY: the pointer is SDL's live UIWindow, on the main thread.
    unsafe { ff_refresh_orientations(ui_window) };
}

/// Set up the keyboard path and lay the gesture surface over SDL's view.
pub(super) fn install_touch_surface(window: &sdl2::video::Window) {
    let Some(ui_window) = ui_window(window) else {
        log::warn!("touch: SDL gave no UIKit window; gestures are off");
        return;
    };
    // SAFETY: the pointer is SDL's live UIWindow, and this runs on the main
    // thread, where SDL created it.
    let installed = unsafe {
        ff_use_hardware_vertical_arrows(ui_window);
        ff_touch_install(ui_window)
    };
    if installed == 0 {
        log::warn!("touch: the gesture surface could not be installed");
    }
}

#[cfg(feature = "visuals")]
pub(super) fn phone_safe_top(window: &sdl2::video::Window, output_height: u32) -> f32 {
    let Some(ui_window) = ui_window(window) else {
        return 0.0;
    };
    // SAFETY: SDL owns the live UIWindow; the main thread reads it while the
    // SDL window is borrowed.
    (unsafe { ff_phone_safe_top_fraction(ui_window) } * f64::from(output_height)) as f32
}

pub(super) fn next_gesture() -> Option<crate::touch::Gesture> {
    loop {
        // SAFETY: a plain queue pop, no arguments.
        let code = unsafe { ff_touch_next() };
        if code < 0 {
            return None;
        }
        if let Some(gesture) = crate::touch::Gesture::from_code(code) {
            return Some(gesture);
        }
    }
}

/// The shared recognizer over the touch surface's raw fingers, and the
/// switch between it and the native recognizers.
pub(super) struct SharedFingers {
    /// What Settings asks for; applied at the next read.
    wanted: bool,
    /// What the touch surface was last told.
    on: bool,
    recognizer: Recognizer,
}

impl SharedFingers {
    pub(super) fn new() -> Self {
        Self {
            wanted: false,
            on: false,
            recognizer: Recognizer::default(),
        }
    }

    pub(super) fn want(&mut self, on: bool) {
        self.wanted = on;
    }

    pub(super) fn set_report_travel(&mut self, on: bool) {
        self.recognizer.set_report_travel(on);
    }

    /// The touches were taken away (the app lost focus): every hold ends,
    /// at the next read.
    pub(super) fn cancel(&mut self) {
        self.recognizer.cancel();
    }

    /// Switch recognizers if Settings changed, then read this frame's
    /// fingers into `events`.
    pub(super) fn read(&mut self, touch: &mut TouchInput, events: &mut Vec<InputEvent>) {
        if let Some(tuning) = SHARED_TUNING
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
        {
            self.recognizer.set_tuning(tuning);
        }
        if self.wanted != self.on {
            self.switch(touch, events);
        }
        if !self.on {
            return;
        }
        let mut queued = Vec::new();
        while let Some(event) = next_finger() {
            queued.push(event);
        }
        // SAFETY: reads the media clock, no arguments.
        let now = unsafe { ff_fingers_now() };
        for gesture in fingers::read(&mut self.recognizer, queued, now) {
            if let Some(haptic) = gesture.haptic() {
                // SAFETY: a plain integer, on the main thread; the shim
                // checks the haptics setting itself.
                unsafe { ff_touch_haptic(haptic.code()) };
            }
            events.extend(touch.handle(gesture).events);
        }
    }

    /// Hand the fingers to the other recognizer. Whatever a finger was
    /// holding lets go first, a pedal or a bound hold like the horn, so a
    /// switch mid-drive never leaves a key down.
    fn switch(&mut self, touch: &mut TouchInput, events: &mut Vec<InputEvent>) {
        self.on = self.wanted;
        self.recognizer.cancel();
        self.recognizer.drain();
        touch.release_into(events);
        events.extend(touch.handle(Gesture::HoldEnded).events);
        // SAFETY: a plain flag, on the main thread, where UIKit is touched.
        unsafe { ff_fingers_set_shared(i32::from(self.on)) };
        log::info!(
            "touch: gestures read by the {} recognizer",
            if self.on { "shared" } else { "native" }
        );
    }
}

fn next_finger() -> Option<FingerEvent> {
    loop {
        let (mut finger, mut kind, mut x, mut y, mut t) = (0i64, 0i32, 0f32, 0f32, 0f64);
        // SAFETY: five out pointers to live locals; the shim writes them
        // only when it returns 1.
        let found = unsafe { ff_fingers_next(&mut finger, &mut kind, &mut x, &mut y, &mut t) };
        if found == 0 {
            return None;
        }
        if let Some(kind) = FingerKind::from_code(kind) {
            return Some(FingerEvent {
                finger,
                kind,
                x,
                y,
                t,
            });
        }
    }
}
