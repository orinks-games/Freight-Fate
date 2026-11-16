//! The iOS side of the window shell: the calls into the UIKit gesture and
//! VoiceOver bridge (`crates/freight-fate/ios/ff_touch.m`, compiled by
//! `build.rs`) that install it, hand it settings and drain its gesture queue.

extern "C" {
    fn ff_touch_install(window: *mut std::ffi::c_void) -> i32;
    fn ff_touch_next() -> i32;
    fn ff_touch_set_haptics(enabled: i32);
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

/// Lay the gesture surface (`ios/ff_touch.m`) over SDL's view.
pub(super) fn install_touch_surface(window: &sdl2::video::Window) {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    let ui_window = match window.window_handle().map(|handle| handle.as_raw()) {
        // sdl2 reports the UIWindow in the `ui_view` slot.
        Ok(RawWindowHandle::UiKit(handle)) => handle.ui_view.as_ptr(),
        _ => {
            log::warn!("touch: SDL gave no UIKit window; gestures are off");
            return;
        }
    };
    // SAFETY: the pointer is SDL's live UIWindow, and this runs on the main
    // thread, where SDL created it.
    if unsafe { ff_touch_install(ui_window) } == 0 {
        log::warn!("touch: the gesture surface could not be installed");
    }
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
