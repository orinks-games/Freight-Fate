//! The iOS side of the window shell: the calls into the UIKit gesture and
//! VoiceOver bridge (`crates/freight-fate/ios/ff_touch.m`, compiled by
//! `build.rs`) that install it, hand it settings and drain its gesture queue.

extern "C" {
    fn ff_use_hardware_vertical_arrows(window: *mut std::ffi::c_void);
    fn ff_touch_install(window: *mut std::ffi::c_void) -> i32;
    #[cfg(feature = "visuals")]
    fn ff_phone_safe_top_fraction(window: *mut std::ffi::c_void) -> f64;
    fn ff_touch_next() -> i32;
    fn ff_touch_set_haptics(enabled: i32);
    fn ff_touch_set_report_travel(enabled: i32);
    fn ff_touch_set_tuning(
        flick_points: f64,
        flick_ms: f64,
        deep_factor: f64,
        still_ms: f64,
        rotate_degrees: f64,
    );
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

/// Hand the gesture thresholds in the settings to the native recognizers.
pub(crate) fn set_touch_tuning(settings: &ff_core::settings::Settings) {
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
