//! The plumbing the online menus share: the worker-thread mailbox, the
//! browser opener, the orinks.net transport the menus call outside the
//! long-lived services, and the identity store behind `OnlineIdentity.load()`
//! / `.save()`. Each has a test seam where the Python tests monkeypatched a
//! module attribute (`webbrowser.open`, `online_presence.set_profile_sharing`,
//! `OnlineIdentity.load`, `threading.Thread`).
//!
//! The clipboard ladder (`pygame.scrap`, `pbcopy`, a hidden Tk root, the
//! read-back verify) collapses to `GameContext::write_clipboard_text`: the
//! SDL clipboard is one UTF-8 call on every platform, and a headless run has
//! an in-memory clipboard, so nothing here needs a fallback of its own.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

use ff_core::models::profile::data_dir;

use crate::app::GameContext;
use crate::net::SharedTransport;
use crate::online_presence::{default_transport, IdentityStore, OnlineIdentity};
use crate::states::base::{InputEvent, Key, Menu};

// -- the worker mailbox ---------------------------------------------------------------

/// A one-slot mailbox a worker thread posts into and `update(dt)` drains
/// (the Python `self._outcome` attribute, written from the thread and read
/// on the game loop).
pub struct Mailbox<T>(Arc<Mutex<Option<T>>>);

impl<T> Clone for Mailbox<T> {
    fn clone(&self) -> Self {
        Mailbox(Arc::clone(&self.0))
    }
}

impl<T> Default for Mailbox<T> {
    fn default() -> Self {
        Mailbox(Arc::new(Mutex::new(None)))
    }
}

impl<T> Mailbox<T> {
    pub fn new() -> Self {
        Self::default()
    }

    /// Leave `value` for the next `take`, replacing anything unread.
    pub fn post(&self, value: T) {
        *self.0.lock().unwrap_or_else(|e| e.into_inner()) = Some(value);
    }

    /// `outcome, self._outcome = self._outcome, None`.
    pub fn take(&self) -> Option<T> {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).take()
    }
}

/// Run `job` on a named daemon thread, or inline when `threaded` is false
/// (the tests' `ImmediateThread`): the job then posts to its mailbox before
/// this returns, and the next `update` drains it exactly as it would a
/// thread's answer. Returns the handle of a spawned thread.
pub fn run_worker(
    threaded: bool,
    name: &str,
    job: impl FnOnce() + Send + 'static,
) -> Option<thread::JoinHandle<()>> {
    if !threaded {
        job();
        return None;
    }
    thread::Builder::new()
        .name(name.to_string())
        .spawn(job)
        .ok()
}

// -- the browser -----------------------------------------------------------------------
//
// The opener itself lives in `crate::browser`, because `main_menu`'s Report
// a problem row needs the same door and has no business importing the online
// menus to find it. Re-exported here so every online screen keeps reaching
// for `open_url` where it always did.
//
// Note what changed under these two names: the browser is no longer reached
// unless the process said it is the game (`browser::allow_real_browser`,
// called by `main()`). A test that installs no override no longer gets a
// real page in a real browser -- it gets a refusal and a panic naming the
// address. See `crate::browser`.

pub use crate::browser::{open_url, set_open_url_override};

// -- the orinks.net transport -------------------------------------------------------

thread_local! {
    static TRANSPORT_OVERRIDE: std::cell::RefCell<Option<SharedTransport>> =
        const { std::cell::RefCell::new(None) };
}

/// The transport behind the one-shot orinks.net calls these menus make
/// (`set_profile_sharing`, `fetch_board`, `start_activation`,
/// `poll_activation`): the real network unless a test
/// installed a fake.
pub fn online_transport() -> SharedTransport {
    TRANSPORT_OVERRIDE
        .with(|slot| slot.borrow().clone())
        .unwrap_or_else(default_transport)
}

/// Replace [`online_transport`] for tests; `None` restores the real network.
///
/// Per thread. See [`identity_store`].
pub fn set_online_transport_override(transport: Option<SharedTransport>) {
    TRANSPORT_OVERRIDE.with(|slot| *slot.borrow_mut() = transport);
}

// -- the identity store -------------------------------------------------------------------

thread_local! {
    static IDENTITY_STORE: std::cell::RefCell<Option<(PathBuf, Arc<IdentityStore>)>> =
        const { std::cell::RefCell::new(None) };
    static IDENTITY_STORE_OVERRIDE: std::cell::RefCell<Option<Arc<IdentityStore>>> =
        const { std::cell::RefCell::new(None) };
}

/// The store `OnlineIdentity.load()` and `.save()` read and write: the
/// platform keyring over the data directory, built once per data directory
/// and kept for the life of the thread. The Python token cache was a class
/// attribute, so a long-lived store is what keeps the Online hub's per-frame
/// labels from costing a secret-store round trip each.
///
/// # Why these four seams are per thread
///
/// This one and the three above (`open_url`, the transport, and the cache
/// below) are all *stand in for the outside world* seams: a test installs a
/// fake and takes it away again. As process-globals they made every test
/// that installed one exclusive with every other, which is a large part of
/// why the whole suite ran behind a single lock at one-core speed.
///
/// Per thread they are exactly as correct and no longer exclusive. Nothing
/// in the game ever installs an override, so a worker thread still gets the
/// real browser, the real network and the platform keyring; and a test that
/// installs one runs its services with `threaded: false`, which runs the job
/// inline on the very thread that installed it.
///
/// The cache is per thread for the same reason it was a cache: each test has
/// its own data directory, and one shared slot keyed by directory missed on
/// almost every lookup once tests ran at once -- rebuilding a platform
/// keyring store per call, which is the round trip the cache exists to
/// avoid.
pub fn identity_store() -> Arc<IdentityStore> {
    if let Some(store) = IDENTITY_STORE_OVERRIDE.with(|slot| slot.borrow().clone()) {
        return store;
    }
    let dir = data_dir();
    IDENTITY_STORE.with(|slot| {
        let mut cached = slot.borrow_mut();
        match cached.as_ref() {
            Some((cached_dir, store)) if *cached_dir == dir => Arc::clone(store),
            _ => {
                let store = Arc::new(IdentityStore::platform(&dir));
                *cached = Some((dir, Arc::clone(&store)));
                store
            }
        }
    })
}

/// Replace [`identity_store`] for tests (`monkeypatch.setattr(OnlineIdentity,
/// "load", ...)`): a store over a memory secret store stands in for the
/// platform one. `None` restores the platform store.
pub fn set_identity_store_override(store: Option<Arc<IdentityStore>>) {
    IDENTITY_STORE_OVERRIDE.with(|slot| *slot.borrow_mut() = store);
}

/// `OnlineIdentity.load()`.
pub fn load_identity() -> Option<OnlineIdentity> {
    identity_store().load()
}

/// `identity.save()`.
pub fn save_identity(identity: &OnlineIdentity) -> std::io::Result<()> {
    identity_store().save(identity)
}

// -- time ------------------------------------------------------------------------------------

/// `time.time()`.
pub fn wall_time() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

// -- the Menu defaults a screen overrides around ------------------------------------------
//
// A Rust trait method has no `super()`: a screen that overrides `enter`,
// `go_back` or `handle_event` to add one step cannot then call the provided
// body. These are that body, copied from `states::base::menu`, so the
// screens here add their step and delegate.
// TODO(lead): belongs in states::base::menu as `Menu::default_enter` /
// `default_go_back` / `default_handle_event` every overriding screen can reach.

/// The provided `Menu::enter`: build the rows, clamp the cursor, play the
/// open sound, announce.
pub fn menu_default_enter<M: Menu>(menu: &mut M, ctx: &mut GameContext) {
    let items = menu.build_items(ctx);
    let core = menu.menu_mut();
    core.items = items;
    core.index = core.index.min(core.items.len().saturating_sub(1));
    if let Some(key) = menu.menu().open_sound_key.clone() {
        ctx.audio.play(&key);
    }
    menu.announce_entry(ctx);
}

/// The provided `Menu::go_back`: the back sound, then pop.
pub fn menu_default_go_back<M: Menu>(_menu: &mut M, ctx: &mut GameContext) {
    ctx.audio.play("ui/menu_back");
    ctx.pop_state();
}

/// The provided `Menu::handle_event`: arrows, Home/End, Enter, Escape, F1,
/// the Control stop, first-letter jump.
pub fn menu_default_handle_event<M: Menu>(menu: &mut M, ctx: &mut GameContext, event: &InputEvent) {
    let Some((key, _mods, text)) = event.key_down() else {
        return;
    };
    match key {
        Key::Down => menu.move_by(ctx, 1),
        Key::Up => menu.move_by(ctx, -1),
        Key::Home => menu.jump(ctx, 0),
        Key::End => {
            let last = menu.menu().items.len().saturating_sub(1);
            menu.jump(ctx, last);
        }
        Key::Return | Key::Space | Key::KpEnter => menu.activate(ctx),
        Key::Escape => menu.go_back(ctx),
        Key::F1 => {
            let help = menu.current_help(ctx);
            ctx.say(&help);
        }
        Key::LCtrl | Key::RCtrl => ctx.stop_speech(),
        _ => {
            if let Some(ch) = text.filter(|ch| ch.is_alphanumeric()) {
                let lower: String = ch.to_lowercase().collect();
                menu.first_letter_jump(ctx, &lower);
            }
        }
    }
}
