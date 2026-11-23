//! Whether this run is a phone or tablet.
//!
//! A phone build (`cfg(touch_screen)`: iPhone, iPad and Android) always is.
//! The agent server can also make a desktop run play as an iPhone
//! (`scenario` with `phone: true`), so the phone's menus, rows and help can
//! be driven where no phone build compiles. Game logic asks here; the SDL
//! shell, the native bridges and anything calling platform code stay
//! compile time, because a desktop run has no phone underneath.
//!
//! The forced fact is per thread: the game loop runs on one, and a test's
//! harness runs on its own, so a test that plays as a phone leaves its
//! neighbours on the desktop.

use std::cell::Cell;

thread_local! {
    static FORCED_PHONE: Cell<bool> = const { Cell::new(false) };
}

/// True on a phone or tablet, or when this run plays as an iPhone.
pub fn phone() -> bool {
    cfg!(touch_screen) || forced()
}

/// True on an iPhone or iPad, or when this run plays as one.
pub fn iphone() -> bool {
    cfg!(target_os = "ios") || forced()
}

/// Whether the radio reads the player's own playlists from the Playlists
/// folder. Phones and tablets leave them out: the folder sits inside the
/// app's private storage, where the player cannot put files, and the
/// YouTube entries need a yt-dlp the phone cannot run.
pub fn personal_playlists() -> bool {
    !phone()
}

fn forced() -> bool {
    FORCED_PHONE.with(Cell::get)
}

/// Play this thread's run as an iPhone (the agent server's `phone: true`),
/// or stop.
pub fn force_phone(on: bool) {
    FORCED_PHONE.with(|forced| forced.set(on));
}
