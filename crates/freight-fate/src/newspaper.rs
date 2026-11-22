//! The driver newspaper on the network side: fetching the issue orinks.net
//! published, keeping it in the data directory, and handing the screens the
//! issue the game holds (`ff_core::newspaper::PaperState`).
//!
//! The site's read answers not-found until the paper is published, so a
//! game that has never been given an issue holds nothing, and nothing in the
//! game mentions a newspaper. There is no setting of its own.
//!
//! A worker thread does the fetch, at launch and again when the app is
//! opened and the held issue is more than five minutes old; the game loop only
//! reads the state under a lock. A failed fetch keeps what is held and says
//! nothing. The fetch needs Online services on and a process that is the
//! game (`net::real_network_allowed`); the agent and playtest sandboxes point
//! the site at a `.invalid` address, which `net` never sends to, so neither
//! they nor the tests reach orinks.net. `threaded: false` runs the same fetch
//! inline for tests, against a fake transport.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use ff_core::newspaper::{parse_issue, parse_issue_value, PaperEvent, PaperIssue, PaperState};
use serde_json::{json, Value};

use crate::net::{self, NetError, SharedTransport};
use crate::online_presence::{base_url, default_transport, join_with_timeout};

/// How long quitting waits for a fetch in flight.
pub const SHUTDOWN_WAIT: Duration = Duration::from_secs(2);

/// The read on whichever Orinks site this build talks to.
pub fn news_url() -> String {
    format!("{}/api/freight-fate/news", base_url())
}

/// Seconds since the Unix epoch.
fn now_s() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0.0, |d| d.as_secs_f64())
}

pub struct NewspaperOptions {
    /// Online services: whether the fetch may run at all.
    pub enabled: bool,
    /// Where the last issue is kept. `None` keeps nothing on disk.
    pub cache_path: Option<PathBuf>,
    pub transport: SharedTransport,
    /// False fetches inline on the calling thread (tests).
    pub threaded: bool,
    /// Whether this process may reach the network. The game's `main()`
    /// grants it; nothing else does.
    pub network_allowed: fn() -> bool,
}

impl Default for NewspaperOptions {
    fn default() -> Self {
        Self {
            enabled: false,
            cache_path: None,
            transport: default_transport(),
            threaded: true,
            network_allowed: net::real_network_allowed,
        }
    }
}

struct Inner {
    enabled: AtomicBool,
    cache_path: Option<PathBuf>,
    transport: SharedTransport,
    threaded: bool,
    network_allowed: fn() -> bool,
    state: Mutex<PaperState>,
    /// A fetch is in flight; a second one is not started beside it.
    fetching: AtomicBool,
    stopping: AtomicBool,
    thread: Mutex<Option<JoinHandle<()>>>,
}

/// The held issue and the fetch that keeps it current. Cheap to clone.
#[derive(Clone)]
pub struct Newspaper {
    inner: Arc<Inner>,
}

impl Default for Newspaper {
    fn default() -> Self {
        Self::new(NewspaperOptions::default())
    }
}

impl Newspaper {
    /// Build the service and read the saved issue, if there is one, so the
    /// app opens at once and works offline.
    pub fn new(options: NewspaperOptions) -> Self {
        let state = options
            .cache_path
            .as_deref()
            .and_then(read_cache)
            .map_or(PaperState::NoIssue, |(issue, at)| {
                PaperState::NoIssue.next(PaperEvent::Received { issue, at })
            });
        Self {
            inner: Arc::new(Inner {
                enabled: AtomicBool::new(options.enabled),
                cache_path: options.cache_path,
                transport: options.transport,
                threaded: options.threaded,
                network_allowed: options.network_allowed,
                state: Mutex::new(state),
                fetching: AtomicBool::new(false),
                stopping: AtomicBool::new(false),
                thread: Mutex::new(None),
            }),
        }
    }

    /// The issue held now, if any.
    pub fn issue(&self) -> Option<PaperIssue> {
        self.inner.lock().issue().cloned()
    }

    pub fn holding(&self) -> bool {
        self.inner.lock().issue().is_some()
    }

    /// For the agent server's `status`.
    pub fn status(&self) -> String {
        self.inner.lock().status()
    }

    /// Follow the Online services switch.
    pub fn set_enabled(&self, enabled: bool) {
        self.inner.enabled.store(enabled, Ordering::SeqCst);
    }

    /// The launch fetch.
    pub fn start(&self) {
        self.inner.fetch();
    }

    /// Opening the app: fetch again if the held issue is over five minutes old.
    /// What is held now is what the screen shows; a newer issue is there
    /// the next time it opens.
    pub fn refresh_if_stale(&self) {
        if self.inner.lock().refresh_due(now_s()) {
            self.inner.fetch();
        }
    }

    /// Put an issue in place, or take it away, as if the site had answered.
    /// For the agent server's `scenario` and the tests. Memory only: the
    /// saved copy is left alone, since tests share a data directory.
    pub fn stage(&self, issue: Option<PaperIssue>) {
        let event = match issue {
            Some(issue) => PaperEvent::Received { issue, at: now_s() },
            None => PaperEvent::Withdrawn,
        };
        let mut state = self.inner.lock();
        *state = std::mem::take(&mut *state).next(event);
    }

    /// Stop: no new fetch starts, and one in flight gets at most
    /// [`SHUTDOWN_WAIT`]. Idempotent.
    pub fn shutdown(&self) {
        self.inner.stopping.store(true, Ordering::SeqCst);
        let handle = self
            .inner
            .thread
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
        if let Some(handle) = handle {
            join_with_timeout(handle, SHUTDOWN_WAIT);
        }
    }
}

impl Inner {
    fn lock(&self) -> std::sync::MutexGuard<'_, PaperState> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn fetch(self: &Arc<Self>) {
        if !self.enabled.load(Ordering::SeqCst)
            || self.stopping.load(Ordering::SeqCst)
            || !(self.network_allowed)()
            || self.fetching.swap(true, Ordering::SeqCst)
        {
            return;
        }
        if !self.threaded {
            self.fetch_once();
            return;
        }
        let inner = Arc::clone(self);
        let spawned = thread::Builder::new()
            .name("newspaper".to_string())
            .spawn(move || inner.fetch_once());
        match spawned {
            Ok(handle) => {
                let mut slot = self.thread.lock().unwrap_or_else(|e| e.into_inner());
                // The last fetch has finished (fetching was false), so its
                // handle only needs collecting.
                if let Some(done) = slot.replace(handle) {
                    let _ = done.join();
                }
            }
            Err(e) => {
                log::warn!("Newspaper fetch not started: {e}");
                self.fetching.store(false, Ordering::SeqCst);
            }
        }
    }

    fn fetch_once(&self) {
        let reply = self.transport.call(&news_url(), None, &[], None);
        let event = match reply {
            Ok(body) => match parse_issue_value(&body) {
                Ok(issue) => {
                    log::info!("Newspaper: issue {}", issue.issue);
                    Some((PaperEvent::Received { issue, at: now_s() }, body))
                }
                Err(why) => {
                    log::warn!("Newspaper: the site's issue was refused ({why:?})");
                    None
                }
            },
            Err(NetError::Http { code: 404, .. }) => {
                log::info!("Newspaper: no issue published");
                self.apply(PaperEvent::Withdrawn, None);
                self.fetching.store(false, Ordering::SeqCst);
                return;
            }
            Err(e) => {
                log::info!("Newspaper not fetched: {e}");
                None
            }
        };
        if !self.stopping.load(Ordering::SeqCst) {
            match event {
                Some((event, body)) => self.apply(event, Some(body)),
                None => self.apply(PaperEvent::FetchFailed, None),
            }
        }
        self.fetching.store(false, Ordering::SeqCst);
    }

    /// Move the state, and keep the disk copy in step with it.
    fn apply(&self, event: PaperEvent, body: Option<Value>) {
        let mut state = self.lock();
        let next = std::mem::take(&mut *state).next(event);
        if let Some(path) = &self.cache_path {
            match &next {
                PaperState::NoIssue => {
                    if path.exists() {
                        if let Err(e) = std::fs::remove_file(path) {
                            log::warn!("Newspaper copy not removed: {e}");
                        }
                    }
                }
                PaperState::Holding { held_at, .. } => {
                    if let Some(body) = &body {
                        write_cache(path, body, *held_at);
                    }
                }
            }
        }
        *state = next;
    }
}

/// The saved copy: the site's body and when it arrived.
fn write_cache(path: &Path, body: &Value, held_at: f64) {
    let text = json!({"heldAt": held_at, "body": body}).to_string();
    let temp = path.with_extension("json.tmp");
    let saved = std::fs::write(&temp, text).and_then(|()| std::fs::rename(&temp, path));
    if let Err(e) = saved {
        log::warn!("Newspaper copy not saved: {e}");
        let _ = std::fs::remove_file(&temp);
    }
}

fn read_cache(path: &Path) -> Option<(PaperIssue, f64)> {
    let text = std::fs::read_to_string(path).ok()?;
    let saved: Value = serde_json::from_str(&text).ok()?;
    let held_at = saved.get("heldAt").and_then(Value::as_f64).unwrap_or(0.0);
    match saved.get("body").map(parse_issue_value) {
        Some(Ok(issue)) => Some((issue, held_at)),
        _ => {
            log::warn!("Newspaper: the saved copy could not be read");
            None
        }
    }
}

/// Read an issue from a file in the site's body shape (the agent server's
/// `scenario`).
pub fn issue_from_file(path: &Path) -> Result<PaperIssue, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("could not read {}: {e}", path.display()))?;
    parse_issue(&text)
        .map_err(|why| format!("{} is not a version 1 issue: {why:?}", path.display()))
}
