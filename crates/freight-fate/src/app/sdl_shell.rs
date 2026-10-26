//! The SDL side of the app: the window, the event pump translated into
//! [`InputEvent`]s, the clipboard, and drawing the visual display. Everything
//! `App` does that needs a display lives here so a headless `App` never
//! touches SDL at all.
//!
//! Drawing is paid for only when it shows: a frame identical to the last
//! one is not presented, and nothing is drawn while the window is minimized
//! or (on a Mac) entirely covered. The window used to be cleared and
//! presented sixty times a second whether anything changed or not.
//!
//! pygame fused `KEYDOWN` and its `unicode`; SDL2 delivers `KeyDown` and
//! then a separate `TextInput`. The pump pairs a `KeyDown` with the
//! `TextInput` that immediately follows it in the same batch (what pygame
//! does internally), so menus' first-letter jump, text entry, and the
//! `+`/`-` fallbacks keep working.

#[cfg(all(target_os = "macos", feature = "visuals"))]
use std::time::Instant;

use sdl2::clipboard::ClipboardUtil;
use sdl2::event::{Event, WindowEvent};
#[cfg(feature = "visuals")]
use sdl2::render::Texture;
use sdl2::render::WindowCanvas;
use sdl2::{EventPump, Sdl, VideoSubsystem};

use crate::controller::sdl::{axis_from_sdl, button_from_sdl};
use crate::states::base::{InputEvent, Key, Mods};
#[cfg(feature = "visuals")]
use crate::visuals::Visuals;

use super::context::Clipboard;
use super::WINDOW_SIZE;
/// SDL keycode to the game's key set, and `event.mod & KMOD_*`: Hearken's.
pub use hearken::sdl_shell::{key_from_keycode, mods_from};

mod display;

/// Keep drawing while the window is covered (macOS stops otherwise), for
/// capture software that records the window from behind other windows.
#[cfg(feature = "visuals")]
pub const ALWAYS_DRAW_ENV: &str = "FREIGHT_FATE_ALWAYS_DRAW";

/// The SDL clipboard (UTF-8 through `SDL_SetClipboardText`, so the whole
/// CF_TEXT-drops-non-ASCII workaround of `online_states.py` is gone).
pub struct SdlClipboard {
    util: ClipboardUtil,
}

impl Clipboard for SdlClipboard {
    fn get_text(&self) -> Option<String> {
        self.util.clipboard_text().ok()
    }

    fn set_text(&mut self, text: &str) -> bool {
        match self.util.set_clipboard_text(text) {
            Ok(()) => true,
            Err(e) => {
                log::debug!("clipboard write failed: {e}");
                false
            }
        }
    }
}

/// The window and its pumps.
pub struct SdlShell {
    pub sdl: Sdl,
    pub video: VideoSubsystem,
    canvas: WindowCanvas,
    pump: EventPump,
    /// The visual display, built the first time it draws.
    #[cfg(feature = "visuals")]
    visuals: Option<Visuals>,
    /// Its glyph atlas on the GPU.
    #[cfg(feature = "visuals")]
    atlas: Option<Texture>,
    /// The next frame must be drawn: the window was shown, uncovered,
    /// resized, or the display was switched on or off.
    redraw: bool,
    /// Minimized or hidden: nothing can be seen, so nothing is drawn.
    hidden: bool,
    /// Entirely covered by other windows, as AppKit last said, and when it
    /// was asked. Only a Mac is asked: SDL reports nothing of the kind.
    #[cfg(all(target_os = "macos", feature = "visuals"))]
    occluded: bool,
    #[cfg(all(target_os = "macos", feature = "visuals"))]
    occlusion_checked: Option<Instant>,
    /// A real Cocoa window, whose AppKit view can be asked about occlusion.
    /// The dummy driver has none, and the sdl2 crate panics when asked.
    /// False also when `FREIGHT_FATE_ALWAYS_DRAW` is set: capture software
    /// recording the window while it is covered needs it kept drawing.
    #[cfg(all(target_os = "macos", feature = "visuals"))]
    cocoa: bool,
    /// Whether the last frame drawn was the visual display (or the plain fill).
    #[cfg(feature = "visuals")]
    showing_visuals: bool,
    #[cfg(feature = "visuals")]
    geometry_failed: bool,
    #[cfg(target_os = "windows")]
    window_handle: Option<isize>,
    #[cfg(target_os = "ios")]
    touch: crate::touch::TouchInput,
    #[cfg(target_os = "ios")]
    keyboard_shown: bool,
    #[cfg(target_os = "ios")]
    text_field_open: bool,
}

#[cfg(target_os = "windows")]
fn hide_windows_window_for_process_exit(handle: isize, hide: impl FnOnce(isize)) {
    hide(handle);
}

#[cfg(target_os = "windows")]
fn window_handle(window: &sdl2::video::Window) -> Option<isize> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    let handle = window.window_handle().ok()?.as_raw();
    match handle {
        RawWindowHandle::Win32(handle) => Some(handle.hwnd.get()),
        _ => None,
    }
}

#[cfg(target_os = "windows")]
fn window_is_visible(handle: isize) -> bool {
    // SAFETY: IsWindowVisible only reads the WS_VISIBLE style of the window
    // and returns FALSE for a handle that is not a window; the handle came
    // from SDL's own window, which this shell keeps alive.
    unsafe { windows_sys::Win32::UI::WindowsAndMessaging::IsWindowVisible(handle as _) != 0 }
}

/// Run `force_show` when the window is not really on screen; returns
/// whether it had to.
///
/// A parent that launches with STARTUPINFO `wShowWindow = SW_HIDE` (Node's
/// `windowsHide`, which is how the Claude desktop app spawns MCP servers)
/// turns the process's FIRST `ShowWindow` into a hide, whatever it asked for
/// (Win32 `ShowWindow`: "nCmdShow is ignored the first time ... if the
/// program that launched the application provides a STARTUPINFO"). SDL's
/// initial show becomes that hide, SDL still believes the window is shown,
/// and its restore and raise are then no-ops: an operator-keys agent server
/// came up with no window at all, not even in Alt+Tab.
#[cfg(target_os = "windows")]
fn force_show_if_hidden(visible: bool, force_show: impl FnOnce()) -> bool {
    if !visible {
        force_show();
    }
    !visible
}

#[cfg(target_os = "windows")]
fn release_at_process_exit<T>(resource: T) {
    // SDL_DestroyRenderer/SDL_DestroyWindow can synchronously wait in the
    // Windows window stack after a long, frequently Alt-Tabbed session. The
    // process is already exiting, so the OS is the faster and safer owner of
    // final resource reclamation.
    std::mem::forget(resource);
}

#[cfg(not(target_os = "windows"))]
fn release_at_process_exit<T>(resource: T) {
    drop(resource);
}

impl SdlShell {
    /// `pygame.init()` + `set_caption` + `set_mode(WINDOW_SIZE)`.
    pub fn new(title: &str) -> Result<Self, String> {
        // SDL on iOS offers the accelerometer as a joystick by default; the
        // game has no use for a tilt stick among its controllers.
        #[cfg(target_os = "ios")]
        sdl2::hint::set("SDL_ACCELEROMETER_AS_JOYSTICK", "0");
        // SDL locks a window wider than tall to landscape; the screen is one
        // touch surface, so follow however the player holds the device and
        // keep swipe directions matching their hand.
        #[cfg(target_os = "ios")]
        sdl2::hint::set(
            "SDL_IOS_ORIENTATIONS",
            "Portrait PortraitUpsideDown LandscapeLeft LandscapeRight",
        );
        let sdl = sdl2::init()?;
        let video = sdl.video()?;
        // Resizable, and drawn at the display's own resolution (Retina on
        // a Mac), so the road view and text stay sharp at any size.
        let window = video
            .window(title, WINDOW_SIZE.0, WINDOW_SIZE.1)
            .position_centered()
            .resizable()
            .allow_highdpi()
            .build()
            .map_err(|e| e.to_string())?;
        // The dummy driver (every headless run: CI, the agent server under
        // FREIGHT_FATE_NO_SPEECH, the playtest benches) has no native window,
        // and the sdl2 crate PANICS rather than erring when asked for one --
        // a windowed headless boot died here the day the handle arrived.
        #[cfg(target_os = "windows")]
        let window_handle = if video.current_video_driver() == "dummy" {
            None
        } else {
            window_handle(&window)
        };
        #[cfg(target_os = "windows")]
        if let Some(handle) = window_handle {
            super::win_keys::install(handle);
        }
        // A dummy window has no GPU surface. Automatic renderer selection can
        // still enter native graphics drivers before falling back to software;
        // AMD's driver fail-fasts there in restricted Windows environments.
        let canvas = window.into_canvas();
        let canvas = if video.current_video_driver() == "dummy" {
            canvas.software()
        } else {
            canvas
        };
        let canvas = canvas.build().map_err(|e| e.to_string())?;
        #[cfg(target_os = "windows")]
        if let Some(handle) = window_handle {
            log::info!(
                "window visible after startup: {}",
                window_is_visible(handle)
            );
        }
        let pump = sdl.event_pump()?;
        #[cfg(all(target_os = "macos", feature = "visuals"))]
        let cocoa = video.current_video_driver() == "cocoa"
            && std::env::var_os(ALWAYS_DRAW_ENV).is_none_or(|v| v.is_empty() || v == "0");
        // pygame delivered event.unicode for every key; SDL needs text
        // input running for TextInput events. On iOS starting text input
        // raises the on-screen keyboard, so there it waits for the player's
        // three-finger double tap; a hardware keyboard types regardless.
        #[cfg(not(target_os = "ios"))]
        video.text_input().start();
        #[cfg(target_os = "ios")]
        install_touch_surface(canvas.window());
        Ok(Self {
            sdl,
            video,
            canvas,
            pump,
            #[cfg(feature = "visuals")]
            visuals: None,
            #[cfg(feature = "visuals")]
            atlas: None,
            redraw: true,
            hidden: false,
            #[cfg(all(target_os = "macos", feature = "visuals"))]
            occluded: false,
            #[cfg(all(target_os = "macos", feature = "visuals"))]
            occlusion_checked: None,
            #[cfg(all(target_os = "macos", feature = "visuals"))]
            cocoa,
            #[cfg(feature = "visuals")]
            showing_visuals: false,
            #[cfg(feature = "visuals")]
            geometry_failed: false,
            #[cfg(target_os = "windows")]
            window_handle,
            #[cfg(target_os = "ios")]
            touch: crate::touch::TouchInput::new(),
            #[cfg(target_os = "ios")]
            keyboard_shown: false,
            #[cfg(target_os = "ios")]
            text_field_open: false,
        })
    }

    pub fn clipboard(&self) -> SdlClipboard {
        SdlClipboard {
            util: self.video.clipboard(),
        }
    }

    /// Minimize the window so it can never hold keyboard focus.
    ///
    /// The agent server calls this at launch: with the window focused, the
    /// operator's own typing in other apps leaks into the game whenever
    /// focus lands on it (found live -- the spaces in a chat message spoke
    /// the speed readout, over and over). Agent input is injected and does
    /// not need focus; the operator's keyboard must not have it.
    pub fn minimize(&mut self) {
        self.canvas.window_mut().minimize();
    }

    /// Bring a minimized window back and ask for focus, so the operator's
    /// keyboard lands in the game again. A window the launcher's
    /// STARTUPINFO kept hidden is shown first (see [`force_show_if_hidden`]).
    pub fn restore(&mut self) {
        #[cfg(target_os = "windows")]
        self.ensure_visible();
        self.canvas.window_mut().restore();
        self.canvas.window_mut().raise();
    }

    /// Show the window if Windows is keeping it hidden behind SDL's back.
    /// Hide first so SDL's shown flag is cleared and its show is not skipped;
    /// by now the launcher's one-time override is spent, so the show is
    /// honoured. Returns whether a show was forced; `None` without a native
    /// window (the dummy driver).
    #[cfg(target_os = "windows")]
    fn ensure_visible(&mut self) -> Option<bool> {
        let handle = self.window_handle?;
        let visible = window_is_visible(handle);
        let window = self.canvas.window_mut();
        let forced = force_show_if_hidden(visible, || {
            window.hide();
            window.show();
        });
        log::info!(
            "window visible: {visible}, forced show: {}",
            if forced { "yes" } else { "no" }
        );
        Some(forced)
    }

    /// Hand desktop focus back immediately and finish SDL at process exit.
    pub fn shutdown_for_process_exit(self) {
        #[cfg(target_os = "windows")]
        {
            if let Some(handle) = self.window_handle {
                hide_windows_window_for_process_exit(handle, |handle| unsafe {
                    use windows_sys::Win32::UI::WindowsAndMessaging::{ShowWindowAsync, SW_HIDE};

                    ShowWindowAsync(handle as _, SW_HIDE);
                });
            }
            release_at_process_exit(self);
        }

        #[cfg(not(target_os = "windows"))]
        {
            let mut shell = self;
            shell.video.text_input().stop();
            shell.canvas.window_mut().hide();
            release_at_process_exit(shell);
        }
    }

    /// Drain the event queue into game events. `None` when the batch was
    /// lost to a failure inside the pump (the Python loop's
    /// `pygame.event.get()` guard: a controller hot-plug, notably a Bluetooth
    /// resume, can make SDL's internal instance-id map inconsistent; losing
    /// this batch of events is survivable, crashing the game is not).
    pub fn poll(&mut self) -> Option<Vec<InputEvent>> {
        let pump = &mut self.pump;
        let raw = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            pump.poll_iter().collect::<Vec<Event>>()
        }))
        .ok()?;
        for event in &raw {
            if let Event::Window { win_event, .. } = event {
                self.note_window_event(win_event);
            }
        }
        #[cfg(not(target_os = "ios"))]
        let events = translate_events(raw);
        #[cfg(target_os = "ios")]
        let events = self.with_touch(translate_events(raw));
        Some(events)
    }

    /// Follow the active screen: raise the on-screen keyboard when a text
    /// field opens and lower it when the field goes away.
    #[cfg(target_os = "ios")]
    pub fn set_text_field(&mut self, open: bool) {
        if open == self.text_field_open {
            return;
        }
        self.text_field_open = open;
        if open != self.keyboard_shown {
            self.keyboard_shown = open;
            if open {
                self.video.text_input().start();
            } else {
                self.video.text_input().stop();
            }
        }
    }

    /// Text input always runs off iOS; there is no keyboard to raise.
    #[cfg(not(target_os = "ios"))]
    pub fn set_text_field(&mut self, _open: bool) {}

    /// Append this frame's gestures to the SDL events. A
    /// lost focus (the app leaving the foreground) lets go of a held pedal.
    #[cfg(target_os = "ios")]
    fn with_touch(&mut self, mut events: Vec<InputEvent>) -> Vec<InputEvent> {
        if events
            .iter()
            .any(|event| matches!(event, InputEvent::WindowFocusLost))
        {
            self.touch.release_into(&mut events);
        }
        while let Some(gesture) = next_gesture() {
            events.extend(self.touch.handle(gesture).events);
        }
        events
    }

    /// Pump the event queue once and discard what arrives. Quit calls this
    /// while services shut down so macOS keeps reading the still-open
    /// window as responsive (issue 266); events during quit are ignored on
    /// purpose -- the game is leaving.
    pub fn pump_during_quit(&mut self) {
        let _ = self.poll();
    }
}

#[cfg(target_os = "ios")]
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

#[cfg(target_os = "ios")]
pub(crate) fn set_touch_haptics(enabled: bool) {
    // SAFETY: the UIKit shim accepts an integer flag on the app thread.
    unsafe { ff_touch_set_haptics(i32::from(enabled)) };
}

#[cfg(not(target_os = "ios"))]
pub(crate) fn set_touch_haptics(_enabled: bool) {}

/// Hand the gesture thresholds in the settings to the native recognizers.
#[cfg(target_os = "ios")]
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

#[cfg(not(target_os = "ios"))]
pub(crate) fn set_touch_tuning(_settings: &ff_core::settings::Settings) {}

/// Lay the gesture surface (`ios/ff_touch.m`) over SDL's view.
#[cfg(target_os = "ios")]
fn install_touch_surface(window: &sdl2::video::Window) {
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

#[cfg(target_os = "ios")]
fn next_gesture() -> Option<crate::touch::Gesture> {
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

/// SDL events to game events, pairing each `KeyDown` with the `TextInput`
/// that follows it.
pub fn translate_events(raw: Vec<Event>) -> Vec<InputEvent> {
    translate_events_with(raw, cfg!(target_os = "ios"))
}

/// [`translate_events`], with the iOS on-screen keyboard's order allowed.
///
/// UIKit's keyboard reaches SDL as a finished edit: each character's key
/// press and release first, then one `TextInput` for the lot -- or, while
/// SDL believes a hardware keyboard is attached (the Simulator, always),
/// the `TextInput` alone. With `soft_keyboard` set, each character finds the
/// unpaired press of its own key earlier in the batch, and a character with
/// none becomes a press of its own, so typed names are not lost.
pub fn translate_events_with(raw: Vec<Event>, soft_keyboard: bool) -> Vec<InputEvent> {
    let mut out = Vec::with_capacity(raw.len());
    let mut pending_text: Option<usize> = None; // index in `out` of the last KeyDown
    for event in raw {
        match event {
            Event::KeyDown {
                keycode,
                keymod,
                repeat,
                ..
            } => {
                let key = keycode.map(key_from_keycode).unwrap_or(Key::Other(0));
                out.push(InputEvent::KeyDown {
                    key,
                    mods: mods_from(keymod),
                    text: None,
                    repeat,
                });
                pending_text = Some(out.len() - 1);
                continue;
            }
            Event::TextInput { text, .. } => {
                if let Some(index) = pending_text.take() {
                    if let Some(InputEvent::KeyDown { text: slot, .. }) = out.get_mut(index) {
                        *slot = text.chars().next();
                    }
                } else if soft_keyboard {
                    pair_typed_text(&mut out, &text);
                }
                continue;
            }
            Event::KeyUp {
                keycode, keymod, ..
            } => {
                let key = keycode.map(key_from_keycode).unwrap_or(Key::Other(0));
                out.push(InputEvent::KeyUp {
                    key,
                    mods: mods_from(keymod),
                });
            }
            Event::ControllerButtonDown { which, button, .. } => {
                out.push(InputEvent::ControllerButtonDown {
                    button: button_from_sdl(button),
                    instance_id: which,
                });
            }
            Event::ControllerButtonUp { which, button, .. } => {
                out.push(InputEvent::ControllerButtonUp {
                    button: button_from_sdl(button),
                    instance_id: which,
                });
            }
            Event::ControllerAxisMotion {
                which, axis, value, ..
            } => {
                out.push(InputEvent::ControllerAxis {
                    axis: axis_from_sdl(axis),
                    value,
                    instance_id: which,
                });
            }
            Event::ControllerDeviceAdded { which, .. } => {
                out.push(InputEvent::ControllerAdded {
                    device_index: which,
                });
            }
            Event::ControllerDeviceRemoved { which, .. } => {
                out.push(InputEvent::ControllerRemoved { instance_id: which });
            }
            Event::Window {
                win_event: WindowEvent::FocusGained,
                ..
            } => out.push(InputEvent::WindowFocusGained),
            Event::Window {
                win_event: WindowEvent::FocusLost,
                ..
            } => out.push(InputEvent::WindowFocusLost),
            Event::Quit { .. } => out.push(InputEvent::Quit),
            _ => {}
        }
        pending_text = None;
    }
    out
}

/// Give each typed character to the unpaired press of its key, or a press
/// of its own when the batch has none.
fn pair_typed_text(out: &mut Vec<InputEvent>, text: &str) {
    let mut searched_to = 0;
    for ch in text.chars() {
        let key = Key::from_char(ch);
        let found = out[searched_to..].iter().position(
            |event| matches!(event, InputEvent::KeyDown { key: k, text: None, .. } if *k == key),
        );
        match found {
            Some(offset) => {
                let index = searched_to + offset;
                if let InputEvent::KeyDown { text: slot, .. } = &mut out[index] {
                    *slot = Some(ch);
                }
                searched_to = index + 1;
            }
            None => {
                out.push(InputEvent::KeyDown {
                    key,
                    mods: Mods::NONE,
                    text: Some(ch),
                    repeat: false,
                });
                out.push(InputEvent::KeyUp {
                    key,
                    mods: Mods::NONE,
                });
                searched_to = out.len();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sdl2::keyboard::{Keycode, Mod};

    #[test]
    fn keydown_pairs_with_the_following_text_input() {
        let raw = vec![
            Event::KeyDown {
                timestamp: 0,
                window_id: 0,
                keycode: Some(Keycode::A),
                scancode: None,
                keymod: Mod::LSHIFTMOD,
                repeat: false,
            },
            Event::TextInput {
                timestamp: 0,
                window_id: 0,
                text: "A".to_string(),
            },
            Event::KeyDown {
                timestamp: 0,
                window_id: 0,
                keycode: Some(Keycode::LEFT),
                scancode: None,
                keymod: Mod::NOMOD,
                repeat: false,
            },
        ];
        let events = translate_events(raw);
        assert_eq!(
            events,
            vec![
                InputEvent::KeyDown {
                    key: Key::A,
                    mods: Mods::SHIFT,
                    text: Some('A'),
                    repeat: false
                },
                InputEvent::key(Key::Left),
            ]
        );
    }

    fn down(keycode: Keycode) -> Event {
        Event::KeyDown {
            timestamp: 0,
            window_id: 0,
            keycode: Some(keycode),
            scancode: None,
            keymod: Mod::NOMOD,
            repeat: false,
        }
    }

    fn up(keycode: Keycode) -> Event {
        Event::KeyUp {
            timestamp: 0,
            window_id: 0,
            keycode: Some(keycode),
            scancode: None,
            keymod: Mod::NOMOD,
            repeat: false,
        }
    }

    fn text(text: &str) -> Event {
        Event::TextInput {
            timestamp: 0,
            window_id: 0,
            text: text.to_string(),
        }
    }

    fn typed(events: &[InputEvent]) -> String {
        events
            .iter()
            .filter_map(|event| match event {
                InputEvent::KeyDown { text, .. } => *text,
                _ => None,
            })
            .collect()
    }

    #[test]
    fn soft_keyboard_text_after_the_release_reaches_its_press() {
        let raw = vec![down(Keycode::I), up(Keycode::I), text("i")];
        let events = translate_events_with(raw, true);
        assert_eq!(typed(&events), "i");
        assert_eq!(events.len(), 2, "{events:?}");
    }

    #[test]
    fn soft_keyboard_text_with_no_press_types_on_its_own() {
        let events = translate_events_with(vec![text("Jo")], true);
        assert_eq!(typed(&events), "Jo");
        assert!(matches!(
            events.first(),
            Some(InputEvent::KeyDown { key: Key::J, .. })
        ));
    }

    #[test]
    fn soft_keyboard_text_skips_a_backspace_before_it() {
        let raw = vec![
            down(Keycode::BACKSPACE),
            up(Keycode::BACKSPACE),
            down(Keycode::A),
            up(Keycode::A),
            text("a"),
        ];
        let events = translate_events_with(raw, true);
        assert!(matches!(
            events.first(),
            Some(InputEvent::KeyDown {
                key: Key::Backspace,
                text: None,
                ..
            })
        ));
        assert_eq!(typed(&events), "a");
    }

    #[test]
    fn desktop_drops_text_that_no_press_is_waiting_for() {
        assert!(translate_events_with(vec![text("x")], false).is_empty());
    }

    #[test]
    fn mods_collapse_left_and_right() {
        assert_eq!(mods_from(Mod::RCTRLMOD), Mods::CTRL);
        assert!(mods_from(Mod::LALTMOD | Mod::LSHIFTMOD).alt);
        assert_eq!(mods_from(Mod::NOMOD), Mods::NONE);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn final_windows_shutdown_does_not_run_a_blocking_resource_destructor() {
        use std::cell::Cell;
        use std::rc::Rc;

        struct DropNotice(Rc<Cell<bool>>);
        impl Drop for DropNotice {
            fn drop(&mut self) {
                self.0.set(true);
            }
        }

        let dropped = Rc::new(Cell::new(false));
        release_at_process_exit(DropNotice(Rc::clone(&dropped)));
        assert!(
            !dropped.get(),
            "Windows ran the synchronous SDL-style destructor during final exit"
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn final_windows_shutdown_uses_the_nonblocking_hide_path() {
        use std::cell::Cell;

        let async_hide_called = Cell::new(false);
        hide_windows_window_for_process_exit(123, |handle| {
            assert_eq!(handle, 123);
            async_hide_called.set(true);
        });

        assert!(async_hide_called.get());
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn only_a_window_windows_kept_hidden_is_force_shown() {
        use std::cell::Cell;

        let shows = Cell::new(0);
        assert!(!force_show_if_hidden(true, || shows.set(shows.get() + 1)));
        assert_eq!(shows.get(), 0, "a visible window was hidden and shown");
        assert!(force_show_if_hidden(false, || shows.set(shows.get() + 1)));
        assert_eq!(shows.get(), 1, "a hidden window was left hidden");
    }

    #[cfg(target_os = "windows")]
    const HIDDEN_LAUNCH_REPORT: &str = "FREIGHT_FATE_HIDDEN_LAUNCH_REPORT";

    /// The child half of [`a_hidden_launch_still_ends_with_a_visible_window`];
    /// does nothing unless that test launched it.
    #[cfg(target_os = "windows")]
    #[test]
    #[ignore = "child process of a_hidden_launch_still_ends_with_a_visible_window"]
    fn hidden_launch_child() {
        let Some(report) = std::env::var_os(HIDDEN_LAUNCH_REPORT) else {
            return;
        };
        let mut shell = SdlShell::new("Freight Fate - hidden launch check").expect("window");
        let handle = shell.window_handle.expect("a native window");
        let created = window_is_visible(handle);
        // The forced show goes without activation, so the check never takes
        // the desktop's focus from whoever is using it. Not before creation:
        // the launch override applies to SDL's plain first show.
        sdl2::hint::set("SDL_WINDOW_NO_ACTIVATION_WHEN_SHOWN", "1");
        let forced = shell.ensure_visible() == Some(true);
        let now = window_is_visible(handle);
        std::fs::write(report, format!("{created} {forced} {now}")).expect("report");
        shell.shutdown_for_process_exit();
    }

    /// Launch the way the desktop app launches an MCP server (STARTUPINFO
    /// `SW_HIDE`; std's `Command` cannot set `wShowWindow` on stable) and
    /// check SDL's window is hidden by it and shown again by the fix.
    #[cfg(target_os = "windows")]
    #[test]
    #[ignore = "opens a real window on the desktop for a moment; run by name"]
    fn a_hidden_launch_still_ends_with_a_visible_window() {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::Foundation::CloseHandle;
        use windows_sys::Win32::System::Threading::{
            CreateProcessW, GetExitCodeProcess, WaitForSingleObject, CREATE_UNICODE_ENVIRONMENT,
            PROCESS_INFORMATION, STARTF_USESHOWWINDOW, STARTUPINFOW,
        };
        use windows_sys::Win32::UI::WindowsAndMessaging::SW_HIDE;

        let dir = tempfile::tempdir().expect("temp dir");
        let report = dir.path().join("report.txt");
        let exe = std::env::current_exe().expect("test exe");
        let mut command_line: Vec<u16> = format!(
            "\"{}\" app::sdl_shell::tests::hidden_launch_child --exact --ignored --test-threads=1",
            exe.display()
        )
        .encode_utf16()
        .chain([0])
        .collect();
        // The inherited environment, minus a headless run's dummy video
        // driver, plus where to write the report.
        let mut environment: Vec<u16> = Vec::new();
        for (key, value) in std::env::vars_os() {
            if key.eq_ignore_ascii_case("SDL_VIDEODRIVER") {
                continue;
            }
            environment.extend(key.encode_wide());
            environment.push(u16::from(b'='));
            environment.extend(value.encode_wide());
            environment.push(0);
        }
        environment.extend(HIDDEN_LAUNCH_REPORT.encode_utf16());
        environment.push(u16::from(b'='));
        environment.extend(report.as_os_str().encode_wide());
        environment.extend([0, 0]);

        // SAFETY: STARTUPINFOW and PROCESS_INFORMATION are plain C structs
        // of integers and pointers, for which all-zero is the documented
        // empty value. The command line and environment buffers are
        // NUL-terminated and outlive the call; the process and thread
        // handles are closed exactly once below.
        let exit_code = unsafe {
            let mut startup: STARTUPINFOW = std::mem::zeroed();
            startup.cb = std::mem::size_of::<STARTUPINFOW>() as u32;
            startup.dwFlags = STARTF_USESHOWWINDOW;
            startup.wShowWindow = SW_HIDE as u16;
            let mut process: PROCESS_INFORMATION = std::mem::zeroed();
            let created = CreateProcessW(
                std::ptr::null(),
                command_line.as_mut_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                0,
                CREATE_UNICODE_ENVIRONMENT,
                environment.as_ptr().cast(),
                std::ptr::null(),
                &startup,
                &mut process,
            );
            assert_ne!(created, 0, "CreateProcessW failed");
            WaitForSingleObject(process.hProcess, 60_000);
            let mut code = u32::MAX;
            GetExitCodeProcess(process.hProcess, &mut code);
            CloseHandle(process.hThread);
            CloseHandle(process.hProcess);
            code
        };
        assert_eq!(exit_code, 0, "the child test failed");
        let report = std::fs::read_to_string(&report).expect("child report");
        assert_eq!(
            report, "false true true",
            "expected: hidden by the launch, forced, then visible \
             (created visible, forced, visible now)"
        );
    }
}
