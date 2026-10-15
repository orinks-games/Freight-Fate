//! The one door out of the game and into a web browser.
//!
//! Three callers open a page: the driver-setup verification page during
//! device activation, the driver setup page from the Online hub, and the
//! bug-report form on GitHub. Every one of them
//! goes through [`open_url`], and [`open_url`] refuses unless something has
//! said, in so many words, that this process is the real game.
//!
//! # Why the default is "refuse"
//!
//! This started as an opt-in test seam: `open_url` called the real browser
//! unless a test had installed an override on its own thread. That is fail
//! OPEN -- a test that forgot the override, or code that ran on a worker
//! thread where a per-thread override does not exist, reached the live site
//! and put a real page in front of whoever was at the keyboard. It did
//! exactly that on 2026-08-24: the driver setup page opened in the owner's
//! browser while the suite was running.
//!
//! So the capability is now explicit and process-wide:
//!
//! * [`allow_real_browser`] is called once, by `main()`, and only by
//!   `main()`. A test binary has no `main()` of the game's, so no test
//!   process can ever be granted it.
//! * Until it is called, [`open_url`] records the address in
//!   [`refused_urls`] and panics. It never reaches a browser.
//! * [`set_open_url_override`] is still there, still per thread, and is
//!   still how a test asserts on what the game asked for. It is now a
//!   convenience rather than the thing standing between the suite and the
//!   live site: forgetting it fails the test instead of opening a page.
//!
//! The panic is deliberate. A test author who forgets the override has to
//! be told, and told at the moment it happens; a quiet `false` return would
//! read as "the browser could not be opened", which is a path the game
//! handles gracefully and nobody would ever look at. The real game cannot
//! hit it: the capability is installed as the first thing `main()` does,
//! before a window exists, let alone a menu.

use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

/// A stand-in for the browser: true when it "opened" the page.
pub type OpenUrl = Arc<dyn Fn(&str) -> bool + Send + Sync>;

/// Set once by `main()`. Process-wide on purpose -- a spawned worker sees
/// it exactly as the game loop does, which a thread-local could not manage.
static REAL_BROWSER_ALLOWED: AtomicBool = AtomicBool::new(false);

/// Every address a browser was asked for and refused, in order.
static REFUSED: Mutex<Vec<String>> = Mutex::new(Vec::new());

thread_local! {
    static OPEN_URL_OVERRIDE: RefCell<Option<OpenUrl>> = const { RefCell::new(None) };
}

/// "This process is the real game": from here on [`open_url`] may reach a
/// real browser.
///
/// Called from `main()` and nowhere else. Nothing undoes it -- a capability
/// that can be handed back is a capability a stray call can take away from
/// a player mid-session.
pub fn allow_real_browser() {
    REAL_BROWSER_ALLOWED.store(true, Ordering::SeqCst);
}

/// Whether a real browser may be reached in this process.
pub fn real_browser_allowed() -> bool {
    REAL_BROWSER_ALLOWED.load(Ordering::SeqCst)
}

/// Every address [`open_url`] refused, oldest first.
pub fn refused_urls() -> Vec<String> {
    REFUSED.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// Forget the refusals so far.
pub fn clear_refused_urls() {
    REFUSED.lock().unwrap_or_else(|e| e.into_inner()).clear();
}

/// Replace [`open_url`] for this thread (`monkeypatch.setattr(webbrowser,
/// "open", ...)`); `None` restores the default.
///
/// Per thread so that tests installing one are not exclusive with each
/// other. It is not a safety mechanism: the capability above is. A thread
/// without an override -- a spawned worker, say -- does not fall through to
/// a browser, it falls through to a refusal.
pub fn set_open_url_override(f: Option<OpenUrl>) {
    OPEN_URL_OVERRIDE.with(|slot| *slot.borrow_mut() = f);
}

/// `webbrowser.open(url)`: true when a browser was asked to open the page.
///
/// False is the Python "raised" case, the one moment the game knows for
/// certain that opening failed. Like `webbrowser.open`, a true answer does
/// not prove a window appeared -- a remote or streamed session is the normal
/// case where nothing happens -- which is why every caller keeps a spoken or
/// clipboard fallback armed either way.
///
/// # Panics
///
/// When neither [`allow_real_browser`] nor [`set_open_url_override`] has
/// been called, which outside the real game means a test reached for the
/// live web. The address is recorded in [`refused_urls`] first.
pub fn open_url(url: &str) -> bool {
    if let Some(f) = OPEN_URL_OVERRIDE.with(|slot| slot.borrow().clone()) {
        return f(url);
    }
    if !real_browser_allowed() {
        refuse(url);
    }
    webbrowser::open(url).is_ok()
}

#[cold]
fn refuse(url: &str) -> ! {
    REFUSED
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .push(url.to_string());
    panic!(
        "refusing to open {url} in a real web browser: this process never \
         called browser::allow_real_browser(), so it is not the game. If \
         this is a test, install a browser seam for the thread that opens \
         the page -- states_online_support::install_browser(true) -- and \
         assert on what it recorded."
    );
}
