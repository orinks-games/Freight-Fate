//! Port of `freight_fate/discord_presence.py` — optional Discord Rich
//! Presence: broad, privacy-safe "now playing" status.
//!
//! This module is the *only* place that knows about Discord or the
//! `discord-rich-presence` IPC crate. Gameplay code reports a small
//! [`PresenceState`] (two short, player-facing strings) and never touches the
//! socket, the dependency, or the throttle logic.
//!
//! Everything here is best-effort and non-fatal by design. If Discord is
//! closed, unavailable, disconnected mid-session, or the RPC client cannot be
//! built, the game must still start, play, and exit exactly as before -- so
//! every RPC interaction runs on a background thread and is wrapped so no
//! error ever propagates into the game loop.
//!
//! Privacy: presence shows broad activity only (menu, terminal, driving,
//! resting, delivering) plus high-level route and cargo context. It never
//! includes save file paths, the driver's chosen name, internal debug data,
//! or anything that is not already visible game content.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use discord_rich_presence::activity::{Activity, Timestamps};
use discord_rich_presence::{DiscordIpc, DiscordIpcClient};

use crate::net::{wait_seconds, Event};
use crate::online_presence::join_with_timeout;
use ff_core::pyfmt::round_py_int;
use ff_core::sim::real_traffic::{wall_clock, Clock};

// Discord application id for the "Freight Fate" rich-presence app. Register an
// application at https://discord.com/developers/applications and set its id here
// or via the FREIGHT_FATE_DISCORD_APP_ID environment variable. The placeholder
// below lets the integration run end to end; with an unregistered id Discord
// simply refuses the handshake and presence stays hidden (handled gracefully).
pub const DEFAULT_CLIENT_ID: &str = "1519334426453082162";

// Discord truncates the details/state lines at 128 characters.
pub const MAX_FIELD_LEN: usize = 128;

// Discord rate-limits presence updates (~5 per 20s). Coalesce changes and never
// push more than one update per this many seconds; identical states are dropped.
pub const MIN_UPDATE_INTERVAL_S: f64 = 15.0;

// A player who walks away -- most often by pausing mid-drive, but equally by
// leaving the game parked at a menu -- reports the identical snapshot for hours,
// and their Discord status keeps telling everyone they are 60% into a run they
// are not driving. After this long without any snapshot change the presence
// comes down and stays down; any change -- resuming, rolling again, a new leg --
// puts it straight back up. Deliberately the drivers board's idle sign-off
// clock, so a player hidden in one place is hidden in the other.
pub const IDLE_CLEAR_S: f64 = 30.0 * 60.0;

// How often the worker re-evaluates while idle (also flushes a throttled change).
const WORKER_TICK_S: f64 = MIN_UPDATE_INTERVAL_S;

// Backoff between connection attempts when Discord is not running.
const RECONNECT_INTERVAL_S: f64 = 30.0;

// How long quitting (or switching the feature off) waits for a worker that
// holds a live Discord client, so its presence can be cleared before the
// pipe goes. See `Inner::wait_for_worker` for why a worker without one is
// never waited for at all.
const WORKER_JOIN_S: Duration = Duration::from_secs(2);

/// A broad, player-facing activity snapshot reported by gameplay code.
///
/// `activity` is the headline line (e.g. "Driving a route"); `detail` is an
/// optional secondary line (e.g. "Chicago to Dallas, steel coils"). Both are
/// plain prose with no private data. Equality drives de-duplication, so two
/// identical snapshots never trigger a redundant Discord update.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PresenceState {
    pub activity: String,
    pub detail: String,
}

impl PresenceState {
    pub fn new(activity: &str, detail: &str) -> Self {
        Self {
            activity: activity.to_string(),
            detail: detail.to_string(),
        }
    }

    /// `PresenceState("...")` with the default empty detail.
    pub fn activity(activity: &str) -> Self {
        Self::new(activity, "")
    }
}

fn truncate(text: &str, limit: usize) -> String {
    let text = text.split_whitespace().collect::<Vec<_>>().join(" "); // collapse whitespace; keep it tidy
    if text.chars().count() <= limit {
        return text;
    }
    let keep = limit.saturating_sub(1);
    let head: String = text.chars().take(keep).collect();
    format!("{}…", head.trim_end()) // ellipsis
}

/// The `pypresence.update` kwargs: `details` always, `state` when the
/// detail line is non-empty, `start` when an elapsed counter was asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivityPayload {
    pub details: String,
    pub state: Option<String>,
    pub start: Option<i64>,
}

/// Translate a [`PresenceState`] into the RPC `update` payload.
///
/// Pure and side-effect free so it is trivially testable. Empty fields are
/// omitted so Discord does not render blank lines, and both visible lines are
/// clamped to Discord's 128-character limit. `start` (an epoch timestamp)
/// shows an elapsed-time counter for the whole session when provided.
pub fn format_activity(state: &PresenceState, start: Option<f64>) -> ActivityPayload {
    let detail = truncate(&state.detail, MAX_FIELD_LEN);
    ActivityPayload {
        details: truncate(&state.activity, MAX_FIELD_LEN),
        state: if detail.is_empty() {
            None
        } else {
            Some(detail)
        },
        start: start.map(|s| s.trunc() as i64),
    }
}

/// Build a privacy-safe driving snapshot from broad gameplay values.
///
/// Pure, so the route/cargo/progress wording is unit-testable without a live
/// game. `fraction` is route progress in 0..1; it is rounded to the nearest
/// five percent so a steadily advancing trip does not churn Discord updates.
/// Only public game content (city names, cargo label, truck model) is used --
/// never the driver name, save path, or internal state.
pub fn driving_presence(
    phase: &str,
    origin: &str,
    destination: &str,
    cargo: &str,
    fraction: f64,
    moving: bool,
    truck_label: &str,
) -> PresenceState {
    let clamped = fraction.clamp(0.0, 1.0);
    // Never "100% there" before the gate: the streets in to the dock round
    // to it on any long run, and a driver still on them is not there yet.
    let pct = (round_py_int(clamped * 20.0) * 5).clamp(0, 100);
    let pct = if clamped < 1.0 { pct.min(95) } else { pct };
    if phase == "pickup" {
        let activity = if origin.is_empty() {
            "Driving empty to a pickup".to_string()
        } else {
            format!("Driving empty to a pickup in {origin}")
        };
        // The deadhead line carries progress too: the drivers board treats a
        // long-unchanged snapshot as an idle (parked, player away) truck, so
        // an advancing deadhead must keep its snapshot moving.
        let mut bits: Vec<String> = Vec::new();
        if !cargo.is_empty() {
            bits.push(format!("Picking up {cargo}"));
        }
        bits.push(format!("{pct}% there"));
        return PresenceState::new(&activity, &bits.join(", "));
    }
    let verb = if moving { "Driving" } else { "Stopped" };
    let activity = if !origin.is_empty() && !destination.is_empty() {
        format!("{verb}: {origin} to {destination}")
    } else if !destination.is_empty() {
        format!("{verb} to {destination}")
    } else {
        format!("{verb} a route")
    };
    let mut bits: Vec<String> = Vec::new();
    if !cargo.is_empty() {
        bits.push(cargo.to_string());
    }
    bits.push(format!("{pct}% there"));
    if !truck_label.is_empty() {
        bits.push(truck_label.to_string());
    }
    PresenceState::new(&activity, &bits.join(", "))
}

/// The slice of the Discord RPC client this module relies on.
pub trait RpcClient: Send {
    fn connect(&mut self) -> Result<(), String>;
    fn update(&mut self, payload: &ActivityPayload) -> Result<(), String>;
    fn clear(&mut self) -> Result<(), String>;
    fn close(&mut self) -> Result<(), String>;
}

/// The real client over Discord's IPC pipe.
pub struct DiscordRpcClient {
    client: DiscordIpcClient,
}

impl DiscordRpcClient {
    pub fn new(client_id: &str) -> Result<Self, String> {
        DiscordIpcClient::new(client_id)
            .map(|client| Self { client })
            .map_err(|e| e.to_string())
    }
}

impl DiscordRpcClient {
    /// Read Discord's answer to the command just sent.
    ///
    /// The IPC crate writes SET_ACTIVITY and never reads the reply Discord
    /// sends back for every command, so a long session piled one unread
    /// reply per update into the pipe, and a command Discord refused (a
    /// rate limit, a payload it would not take) failed silently while the
    /// presence it was meant to change stayed up (2026-10-03: a driver shown
    /// at 0% for seven hours). Reading it keeps the pipe drained and turns a
    /// refusal into an error, which tears the client down and reconnects.
    /// This runs on the presence worker, never the game loop; nothing waits
    /// on a worker mid-command at quit (see `wait_for_worker`).
    fn await_reply(&mut self) -> Result<(), String> {
        let (op, reply) = self.client.recv().map_err(|e| e.to_string())?;
        check_reply(op, &reply)
    }
}

/// Judge one IPC reply frame: a CLOSE frame (opcode 2) or an `ERROR` event is
/// Discord refusing the command; anything else is its acknowledgement.
pub fn check_reply(op: u32, reply: &serde_json::Value) -> Result<(), String> {
    if op == 2 {
        return Err(format!("Discord closed the connection: {reply}"));
    }
    if reply.get("evt").and_then(|e| e.as_str()) == Some("ERROR") {
        let message = reply
            .pointer("/data/message")
            .and_then(|m| m.as_str())
            .unwrap_or("no message");
        return Err(format!("Discord refused the command: {message}"));
    }
    Ok(())
}

impl RpcClient for DiscordRpcClient {
    fn connect(&mut self) -> Result<(), String> {
        self.client.connect().map_err(|e| e.to_string())
    }

    fn update(&mut self, payload: &ActivityPayload) -> Result<(), String> {
        let mut activity = Activity::new().details(&payload.details);
        if let Some(state) = &payload.state {
            activity = activity.state(state);
        }
        if let Some(start) = payload.start {
            activity = activity.timestamps(Timestamps::new().start(start));
        }
        self.client
            .set_activity(activity)
            .map_err(|e| e.to_string())?;
        self.await_reply()
    }

    fn clear(&mut self) -> Result<(), String> {
        self.client.clear_activity().map_err(|e| e.to_string())?;
        self.await_reply()
    }

    fn close(&mut self) -> Result<(), String> {
        self.client.close().map_err(|e| e.to_string())
    }
}

/// Builds an RPC client for an application id. `None` models the
/// dependency being absent entirely (`rpc_factory=None` in Python).
pub type RpcFactory = Arc<dyn Fn(&str) -> Result<Box<dyn RpcClient>, String> + Send + Sync>;

/// Build a real client. Errors when the IPC client cannot be constructed.
pub fn default_rpc_factory() -> RpcFactory {
    Arc::new(|client_id: &str| {
        DiscordRpcClient::new(client_id).map(|c| Box::new(c) as Box<dyn RpcClient>)
    })
}

/// Fully release a client. The Python teardown drained pypresence's private
/// asyncio loop; the IPC crate has none, so this is clear-then-close, every
/// step best-effort.
fn teardown_rpc(mut rpc: Box<dyn RpcClient>, send_close: bool) {
    if send_close {
        let _ = rpc.clear();
        let _ = rpc.close();
    }
}

/// Everything [`DiscordPresence::new`] takes. `Default` is the enabled,
/// real-client, threaded service the app builds.
pub struct DiscordPresenceOptions {
    pub enabled: bool,
    /// `None` reads `FREIGHT_FATE_DISCORD_APP_ID`, else [`DEFAULT_CLIENT_ID`].
    pub client_id: Option<String>,
    pub min_interval_s: f64,
    /// Seconds of an unchanged snapshot before the presence is taken down.
    pub idle_clear_s: f64,
    pub clock: Clock,
    /// `None` models the dependency being unavailable: the service stays dormant.
    pub rpc_factory: Option<RpcFactory>,
    /// Epoch seconds the session started; `None` means now.
    pub session_start: Option<f64>,
    pub threaded: bool,
}

impl Default for DiscordPresenceOptions {
    fn default() -> Self {
        Self {
            enabled: true,
            client_id: None,
            min_interval_s: MIN_UPDATE_INTERVAL_S,
            idle_clear_s: IDLE_CLEAR_S,
            clock: wall_clock(),
            rpc_factory: Some(default_rpc_factory()),
            session_start: None,
            threaded: true,
        }
    }
}

#[derive(Default)]
struct State {
    desired: Option<PresenceState>,
    last_sent: Option<PresenceState>,
    last_send_t: Option<f64>,
    /// When `desired` last genuinely changed; the idle clock reads from here.
    desired_changed_t: Option<f64>,
    /// Whether Discord is currently displaying our activity.
    showing: bool,
    rpc: Option<Box<dyn RpcClient>>,
    last_connect_attempt: Option<f64>,
}

struct Inner {
    available: bool,
    enabled: AtomicBool,
    client_id: String,
    min_interval: f64,
    idle_clear: f64,
    clock: Clock,
    rpc_factory: Option<RpcFactory>,
    session_start: f64,
    threaded: bool,
    state: Mutex<State>,
    wake: Event,
    stop: Event,
    thread: Mutex<Option<JoinHandle<()>>>,
    started: AtomicBool,
}

/// Best-effort Discord Rich Presence service.
///
/// Gameplay calls [`update`](Self::update) with a [`PresenceState`]; it returns
/// immediately, never blocking the game loop. A worker thread owns all
/// socket I/O: it connects (retrying when Discord is closed), pushes the latest
/// state subject to de-duplication and throttling, and reconnects if Discord
/// goes away. [`shutdown`](Self::shutdown) clears the presence and joins the
/// worker.
///
/// The worker is optional (`threaded: false`) so tests can drive the exact
/// same connect/throttle/send logic synchronously with an injected clock and a
/// fake RPC client.
#[derive(Clone)]
pub struct DiscordPresence {
    inner: Arc<Inner>,
}

impl DiscordPresence {
    pub fn new(options: DiscordPresenceOptions) -> Self {
        let client_id = options.client_id.unwrap_or_else(|| {
            std::env::var("FREIGHT_FATE_DISCORD_APP_ID")
                .unwrap_or_else(|_| DEFAULT_CLIENT_ID.to_string())
        });
        // The feature is only live when it is enabled *and* there is a way to
        // talk to Discord. A missing dependency simply leaves it dormant.
        let available = options.rpc_factory.is_some();
        let session_start = options.session_start.unwrap_or_else(|| {
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs_f64())
                .unwrap_or(0.0)
        });
        Self {
            inner: Arc::new(Inner {
                available,
                enabled: AtomicBool::new(options.enabled && available),
                client_id,
                min_interval: options.min_interval_s.max(0.0),
                idle_clear: options.idle_clear_s.max(1.0),
                clock: options.clock,
                rpc_factory: options.rpc_factory,
                session_start,
                threaded: options.threaded,
                state: Mutex::new(State::default()),
                wake: Event::new(),
                stop: Event::new(),
                thread: Mutex::new(None),
                started: AtomicBool::new(false),
            }),
        }
    }

    // -- public API -----------------------------------------------------------

    pub fn enabled(&self) -> bool {
        self.inner.enabled.load(Ordering::SeqCst)
    }

    pub fn connected(&self) -> bool {
        self.inner.state.lock().unwrap().rpc.is_some()
    }

    /// Begin presence after app initialisation. Safe to call when disabled.
    pub fn start(&self) {
        if !self.enabled() || self.inner.started.load(Ordering::SeqCst) {
            return;
        }
        self.inner.started.store(true, Ordering::SeqCst);
        self.inner.stop.clear();
        if self.inner.threaded {
            let inner = Arc::clone(&self.inner);
            let handle = thread::Builder::new()
                .name("discord-presence".to_string())
                .spawn(move || inner.run())
                .ok();
            *self.inner.thread.lock().unwrap() = handle;
        } else {
            self.inner.pump();
        }
    }

    /// Run one connect-then-maybe-send cycle now, the way the worker thread
    /// does on its own tick.
    ///
    /// The threaded service pumps on a heartbeat whether or not the game has
    /// reported anything, which is what lets the idle clear fire while a
    /// paused game repeats the same snapshot. The non-threaded service pumps
    /// only on a change, so a test turning the clock forward over an
    /// *unchanged* snapshot has to drive it (the Python tests call `_pump`).
    pub fn tick(&self) {
        self.inner.pump();
    }

    /// Report the latest broad activity. Non-blocking and dedup-aware.
    pub fn update(&self, state: Option<PresenceState>) {
        let Some(state) = state else { return };
        if !self.enabled() {
            return;
        }
        {
            let mut st = self.inner.state.lock().unwrap();
            if st.desired.as_ref() == Some(&state) {
                return; // nothing changed; skip even the wakeup
            }
            st.desired = Some(state);
            // Any genuine change restarts the idle clock; the dedupe above
            // means a paused or parked game re-reporting the same snapshot
            // every frame does not.
            st.desired_changed_t = Some((self.inner.clock)());
        }
        if self.inner.threaded {
            self.inner.wake.set();
        } else {
            self.inner.pump();
        }
    }

    /// Clear the presence and stop the worker. Never raises.
    pub fn shutdown(&self) {
        self.inner.stop.set();
        self.inner.wake.set();
        let handle = self.inner.thread.lock().unwrap().take();
        if let Some(handle) = handle {
            self.inner.wait_for_worker(handle);
        }
        self.inner.close();
        self.inner.started.store(false, Ordering::SeqCst);
    }

    /// Toggle presence at runtime (e.g. from the settings menu).
    pub fn set_enabled(&self, enabled: bool) {
        let enabled = enabled && self.inner.available;
        if enabled == self.enabled() {
            return;
        }
        self.inner.enabled.store(enabled, Ordering::SeqCst);
        if enabled {
            // Re-show whatever the game last reported, reconnecting at once
            // rather than waiting out the idle backoff (this is a user action).
            // Throwing the switch is also proof the player is here, so the idle
            // clock starts over rather than hiding what it just put up.
            {
                let mut st = self.inner.state.lock().unwrap();
                st.last_sent = None;
                st.last_connect_attempt = None;
                st.desired_changed_t = Some((self.inner.clock)());
            }
            self.start();
        } else {
            let was_started = self.inner.started.load(Ordering::SeqCst);
            self.inner.stop.set();
            self.inner.wake.set();
            let handle = self.inner.thread.lock().unwrap().take();
            if let Some(handle) = handle {
                if was_started {
                    self.inner.wait_for_worker(handle);
                }
            }
            self.inner.close();
            self.inner.started.store(false, Ordering::SeqCst);
            self.inner.stop.clear();
        }
    }

    /// One connect-then-maybe-send cycle. Public so tests can step the
    /// synchronous service.
    pub fn pump(&self) {
        self.inner.pump();
    }
}

impl Inner {
    fn enabled(&self) -> bool {
        self.enabled.load(Ordering::SeqCst)
    }

    /// Wait for the worker to finish, but only while waiting can achieve
    /// something.
    ///
    /// The point of waiting at all is [`close`](Self::close): a worker that
    /// owns a live client must be off the pipe before the presence is cleared,
    /// or it re-shows what we just cleared. When the worker holds no client
    /// there is nothing to hand back and nothing to race with, so waiting buys
    /// the player nothing -- and it is exactly then that waiting is expensive.
    ///
    /// Discord's IPC handshake is a blocking pipe read with no timeout, and
    /// Discord stops answering handshakes for a while when a game is launched
    /// several times in quick succession. A worker parked in that read cannot
    /// be interrupted and will not return inside any timeout we pick, so the
    /// old unconditional two-second join simply charged the player two silent
    /// seconds at quit -- measured on 2026-08-24 as the whole of a packaged
    /// launch's timing spread, 0.65 s of work stretched to 2.65 s. Nothing is
    /// deferred and nothing is skipped: a connected worker is still waited for
    /// and its presence still cleared.
    fn wait_for_worker(&self, handle: JoinHandle<()>) {
        if self.state.lock().unwrap().rpc.is_none() {
            return;
        }
        join_with_timeout(handle, WORKER_JOIN_S);
    }

    fn run(&self) {
        // `enabled` as well as `stop`, because a worker can outlive the stop
        // flag: `set_enabled(false)` sets it, tidies up, and clears it again,
        // and a worker parked in the handshake through all three wakes up to a
        // clear flag. Now it reads the switch the player actually threw and
        // leaves.
        while !self.stop.is_set() && self.enabled() {
            self.pump();
            wait_seconds(&self.wake, self.worker_wait());
            self.wake.clear();
        }
    }

    /// Seconds to sleep before the next pump.
    ///
    /// Short when a reported change is still waiting out the throttle window,
    /// otherwise a lazy heartbeat. Capped so a throttled update always flushes.
    fn worker_wait(&self) -> f64 {
        let st = self.state.lock().unwrap();
        let pending = st.desired.is_some() && st.desired != st.last_sent;
        if pending {
            if let Some(last) = st.last_send_t {
                let remaining = self.min_interval - ((self.clock)() - last);
                if remaining > 0.0 {
                    return remaining.min(WORKER_TICK_S);
                }
            }
        }
        WORKER_TICK_S
    }

    /// One connect-then-maybe-send cycle. Swallows all RPC errors.
    fn pump(&self) {
        if self.stop.is_set() || !self.enabled() {
            return;
        }
        if !self.ensure_connected() {
            return;
        }
        // The socket write runs outside the lock, as the Python update did,
        // so a slow pipe never stalls the game loop's next `update()`.
        let (desired, now, mut rpc) = {
            let mut st = self.state.lock().unwrap();
            let Some(desired) = st.desired.clone() else {
                return; // nothing reported yet
            };
            let now = (self.clock)();
            let idle_for = st.desired_changed_t.map_or(0.0, |t| now - t);
            if idle_for >= self.idle_clear {
                // The same snapshot for this long means the player has walked
                // away, so take the presence down instead of leaving a stale
                // "60% there" up all evening. `last_sent` keeps the idle
                // snapshot so the next real change is still seen as a change
                // and re-shows it.
                drop(st);
                self.hide(now);
                return;
            }
            if st.last_sent.as_ref() == Some(&desired) {
                return; // nothing new to show (de-dupe)
            }
            if let Some(last) = st.last_send_t {
                if now - last < self.min_interval {
                    return; // throttled; the worker re-checks after the window closes
                }
            }
            (desired, now, st.rpc.take())
        };
        let payload = format_activity(&desired, Some(self.session_start));
        let result = match rpc.as_mut() {
            Some(rpc) => rpc.update(&payload),
            None => Err("not connected".to_string()),
        };
        let mut st = self.state.lock().unwrap();
        match result {
            Ok(()) => {
                st.rpc = rpc;
                st.last_sent = Some(desired);
                st.last_send_t = Some(now);
                st.showing = true;
            }
            Err(e) => {
                log::debug!("Discord presence update failed; will reconnect: {e}");
                st.last_send_t = None;
                st.last_sent = None;
                drop(st);
                if let Some(rpc) = rpc {
                    teardown_rpc(rpc, true);
                }
            }
        }
    }

    fn ensure_connected(&self) -> bool {
        let Some(factory) = &self.rpc_factory else {
            return false;
        };
        {
            let mut st = self.state.lock().unwrap();
            if st.rpc.is_some() {
                return true;
            }
            let now = (self.clock)();
            if let Some(last) = st.last_connect_attempt {
                if now - last < RECONNECT_INTERVAL_S {
                    return false; // back off; Discord is probably not running
                }
            }
            st.last_connect_attempt = Some(now);
        }
        let mut rpc = match factory(&self.client_id) {
            Ok(rpc) => rpc,
            Err(e) => {
                log::debug!("Discord not available; presence stays hidden: {e}");
                return false;
            }
        };
        if let Err(e) = rpc.connect() {
            log::debug!("Discord not available; presence stays hidden: {e}");
            // A failed handshake must not leave the pipe half-open; tear the
            // client down without sending anything (e.g. a wrong app id).
            teardown_rpc(rpc, false);
            return false;
        }
        // The handshake is a blocking read that can outlast the reason for it:
        // quitting no longer waits on a worker with no client (see
        // `wait_for_worker`), and switching Discord status off in Settings
        // clears the stop flag again once it has tidied up. A reply arriving
        // after either must be dropped -- otherwise it shows the player as
        // playing a game they have shut, or as playing at all after they asked
        // not to be. Both switches are read, not just the stop flag.
        if self.stop.is_set() || !self.enabled() {
            teardown_rpc(rpc, false);
            return false;
        }
        self.state.lock().unwrap().rpc = Some(rpc);
        true
    }

    /// Take an idle presence down, once. Leaves `last_sent` alone so the next
    /// genuine change still reads as a change and puts the activity back up.
    fn hide(&self, now: f64) {
        let mut rpc = {
            let mut st = self.state.lock().unwrap();
            if !st.showing {
                return; // already hidden; wait for a change rather than re-clearing
            }
            st.rpc.take()
        };
        let result = match rpc.as_mut() {
            Some(rpc) => rpc.clear(),
            None => Err("not connected".to_string()),
        };
        let mut st = self.state.lock().unwrap();
        match result {
            Ok(()) => {
                st.rpc = rpc;
                st.showing = false;
                // A clear spends the same rate-limit budget as an update, so it
                // counts against the throttle a resume will have to wait out.
                st.last_send_t = Some(now);
            }
            Err(e) => {
                log::debug!("Discord presence clear failed; will reconnect: {e}");
                st.last_send_t = None;
                st.last_sent = None;
                st.showing = false;
                drop(st);
                if let Some(rpc) = rpc {
                    teardown_rpc(rpc, true);
                }
            }
        }
    }

    fn close(&self) {
        let rpc = {
            let mut st = self.state.lock().unwrap();
            st.last_send_t = None;
            st.last_sent = None;
            st.showing = false;
            st.rpc.take()
        };
        if let Some(rpc) = rpc {
            teardown_rpc(rpc, true);
        }
    }
}
