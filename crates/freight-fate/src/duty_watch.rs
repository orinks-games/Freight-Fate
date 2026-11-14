//! Spoken notice when other drivers go on or off duty.
//!
//! The "Drivers on duty" screen already keeps itself current while it is
//! open; this service is that same watch running in the background, so a
//! player who wants to hear "Road Star is on duty" while they are hauling
//! can, without opening the list. Off by default: it is a spoken line that
//! arrives unasked, and one cached read of the public drivers list a minute
//! for every player who turns it on.
//!
//! The list is public (the same one the screen reads), so this works without
//! an orinks.net account. When the player has one, their own row is skipped:
//! hearing yourself set off is not news.
//!
//! Same shape as `online_presence`: a worker thread owns all HTTP, the game
//! loop only ever drains a queue of finished lines, and `threaded: false`
//! lets tests pump the exact same logic synchronously against a fake
//! transport and a manual clock. Nothing here can stall the loop or raise
//! into it.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use serde_json::Value;

use crate::net::{wait_seconds, Event, SharedTransport};
use crate::online_presence::{self, default_transport, join_with_timeout, OnlineIdentity};
use ff_core::sim::real_traffic::{wall_clock, Clock};

/// Seconds between reads of the drivers list. The backend answers from a
/// sixty-second snapshot, and a "came on duty" call two minutes late tells
/// the player the same thing as one a minute late -- while the watch runs
/// all session, so halving its reads halves the biggest share of the game's
/// online traffic. The open on-duty screen still checks every minute: a
/// player is looking at it, and it only polls while it is open.
pub const POLL_INTERVAL_S: f64 = 120.0;

/// Everything [`DutyWatch::new`] takes; `Default` is the disabled,
/// real-network, threaded service the app builds.
pub struct DutyWatchOptions {
    pub enabled: bool,
    /// The player's own driver id, left out of every announcement.
    pub own_driver_id: Option<String>,
    pub poll_s: f64,
    pub clock: Clock,
    pub transport: SharedTransport,
    pub threaded: bool,
}

impl Default for DutyWatchOptions {
    fn default() -> Self {
        Self {
            enabled: false,
            own_driver_id: None,
            poll_s: POLL_INTERVAL_S,
            clock: wall_clock(),
            transport: default_transport(),
            threaded: true,
        }
    }
}

#[derive(Default)]
struct State {
    /// The drivers on the list at the last successful read, by id, with the
    /// name each was listed under. `None` until the first read lands: that
    /// read seeds the set silently, so turning the watch on never reads the
    /// whole list out as arrivals.
    seen: Option<BTreeMap<String, String>>,
    last_read_t: Option<f64>,
    own_driver_id: Option<String>,
    /// Finished lines waiting for the game loop.
    announcements: Vec<String>,
}

struct Inner {
    enabled: AtomicBool,
    poll_s: f64,
    clock: Clock,
    transport: SharedTransport,
    threaded: bool,
    state: Mutex<State>,
    wake: Event,
    stop: Event,
    thread: Mutex<Option<JoinHandle<()>>>,
    started: AtomicBool,
}

/// Background watch on the public drivers list, speaking arrivals and
/// departures.
///
/// [`start`](Self::start) begins the worker; [`take_announcements`]
/// (Self::take_announcements) hands the loop whatever has been decided since
/// it last asked; [`shutdown`](Self::shutdown) stops the worker within a
/// bound. [`set_enabled`](Self::set_enabled) follows the setting at run time.
#[derive(Clone)]
pub struct DutyWatch {
    inner: Arc<Inner>,
}

impl DutyWatch {
    pub fn new(options: DutyWatchOptions) -> Self {
        Self {
            inner: Arc::new(Inner {
                enabled: AtomicBool::new(options.enabled),
                poll_s: options.poll_s.max(1.0),
                clock: options.clock,
                transport: options.transport,
                threaded: options.threaded,
                state: Mutex::new(State {
                    own_driver_id: options.own_driver_id,
                    ..State::default()
                }),
                wake: Event::new(),
                stop: Event::new(),
                thread: Mutex::new(None),
                started: AtomicBool::new(false),
            }),
        }
    }

    pub fn enabled(&self) -> bool {
        self.inner.enabled.load(Ordering::SeqCst)
    }

    /// Adopt (or drop) the player's own identity, so their own row is never
    /// announced.
    pub fn set_identity(&self, identity: Option<&OnlineIdentity>) {
        self.inner.state.lock().unwrap().own_driver_id = identity.map(|i| i.driver_id.clone());
    }

    /// Begin the worker after app initialisation. Safe when disabled.
    pub fn start(&self) {
        if !self.enabled() || self.inner.started.load(Ordering::SeqCst) {
            return;
        }
        self.inner.started.store(true, Ordering::SeqCst);
        self.inner.stop.clear();
        if self.inner.threaded {
            let inner = Arc::clone(&self.inner);
            let handle = thread::Builder::new()
                .name("duty-watch".to_string())
                .spawn(move || inner.run())
                .ok();
            *self.inner.thread.lock().unwrap() = handle;
        } else {
            self.inner.pump();
        }
    }

    /// Toggle at run time (from the Online menu).
    ///
    /// Turning the watch off forgets the list it had, so turning it back on
    /// seeds afresh rather than reading out everyone who came and went in
    /// between.
    pub fn set_enabled(&self, enabled: bool) {
        if enabled == self.enabled() {
            return;
        }
        self.inner.enabled.store(enabled, Ordering::SeqCst);
        if enabled {
            {
                let mut st = self.inner.state.lock().unwrap();
                st.seen = None;
                st.last_read_t = None;
            }
            self.start();
        } else {
            self.inner.stop_worker();
            self.inner.state.lock().unwrap().seen = None;
        }
    }

    /// The lines decided since the last call, oldest first. Drained by the
    /// game loop every frame; empty almost always.
    pub fn take_announcements(&self) -> Vec<String> {
        std::mem::take(&mut self.inner.state.lock().unwrap().announcements)
    }

    /// Stop the worker. Never raises, never waits more than two seconds.
    pub fn shutdown(&self) {
        self.inner.stop_worker();
    }

    /// Read the list once if a read is due. Public so tests can drive the
    /// synchronous service step by step.
    pub fn pump(&self) {
        self.inner.pump();
    }
}

impl Inner {
    fn enabled(&self) -> bool {
        self.enabled.load(Ordering::SeqCst)
    }

    fn stop_worker(&self) {
        self.stop.set();
        self.wake.set();
        let handle = self.thread.lock().unwrap().take();
        if let Some(handle) = handle {
            join_with_timeout(handle, Duration::from_secs(2));
        }
        self.started.store(false, Ordering::SeqCst);
        self.stop.clear();
    }

    fn run(&self) {
        while !self.stop.is_set() {
            self.pump();
            wait_seconds(&self.wake, self.worker_wait());
            self.wake.clear();
        }
    }

    /// Seconds until the next read is due.
    fn worker_wait(&self) -> f64 {
        let now = (self.clock)();
        match self.state.lock().unwrap().last_read_t {
            Some(t) => (self.poll_s - (now - t)).max(0.05),
            None => 0.05,
        }
    }

    fn pump(&self) {
        if self.stop.is_set() || !self.enabled() {
            return;
        }
        let now = (self.clock)();
        {
            let st = self.state.lock().unwrap();
            if let Some(t) = st.last_read_t {
                if now - t < self.poll_s {
                    return;
                }
            }
        }
        // The read happens with the lock released: it is the slow part, and
        // the loop drains announcements under that lock.
        let board = online_presence::fetch_board(self.transport.as_ref());
        let mut st = self.state.lock().unwrap();
        st.last_read_t = Some(now);
        // Unreachable: keep what was known. A driver who left during the
        // outage is reported when the site answers again.
        let Some(board) = board else {
            return;
        };
        let mut current = BTreeMap::new();
        for entry in &board {
            let id = text(entry, "driverId");
            if id.is_empty() || st.own_driver_id.as_deref() == Some(id.as_str()) {
                continue;
            }
            let name = match entry.get("displayName") {
                None | Some(Value::Null) => "A driver".to_string(),
                Some(_) => text(entry, "displayName"),
            };
            current.insert(id, name);
        }
        if let Some(seen) = st.seen.as_ref() {
            let on: Vec<String> = current
                .iter()
                .filter(|(id, _)| !seen.contains_key(*id))
                .map(|(_, name)| name.clone())
                .collect();
            let off: Vec<String> = seen
                .iter()
                .filter(|(id, _)| !current.contains_key(*id))
                .map(|(_, name)| name.clone())
                .collect();
            if let Some(line) = duty_change_text(&on, &off) {
                st.announcements.push(line);
            }
        }
        st.seen = Some(current);
    }
}

/// `str(entry.get(key, ""))` for the list's string fields.
fn text(entry: &Value, key: &str) -> String {
    match entry.get(key) {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(s)) => s.clone(),
        Some(other) => online_presence::py_str(other),
    }
}

/// "Road Star", "Road Star and Night Owl", "Road Star, Night Owl and Big
/// Rig Bill".
fn name_list(names: &[String]) -> String {
    match names {
        [] => String::new(),
        [one] => one.clone(),
        [head @ .., last] => format!("{} and {last}", head.join(", ")),
    }
}

/// The spoken line for one read's changes, or `None` when nothing moved.
///
/// Arrivals first, departures after, one line for the whole read: several
/// drivers setting off within the same minute are one piece of news, not a
/// queue of them.
pub fn duty_change_text(on: &[String], off: &[String]) -> Option<String> {
    let mut parts = Vec::new();
    if !on.is_empty() {
        parts.push(format!(
            "{} {} on duty.",
            name_list(on),
            if on.len() == 1 { "is" } else { "are" }
        ));
    }
    if !off.is_empty() {
        parts.push(format!("{} went off duty.", name_list(off)));
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(" "))
    }
}
