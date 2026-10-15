//! Port of `freight_fate/online_presence.py` — optional online presence: an
//! opt-in "on duty" heartbeat to orinks.net.
//!
//! This module is the *only* place that knows about the Orinks presence API.
//! Gameplay code reports the same broad [`PresenceState`] snapshots it
//! already builds for Discord Rich Presence; this service posts the latest one
//! to the live drivers board while the player is hauling, and clears it when
//! they stop. The board then shows lines like "Road Star -- Driving: Chicago to
//! Dallas -- steel coils, 45% there".
//!
//! Everything here is best-effort and non-fatal by design, mirroring
//! `discord_presence`: if the player is offline, the site is down, or the
//! feature is disabled, the game plays exactly as before. All network I/O
//! runs on a background thread and no error ever propagates into the game
//! loop.
//!
//! Privacy: the feature is off by default and only ever sends the broad
//! activity strings above, authenticated by account-issued credentials the
//! player receives once through device-code activation (see
//! `online_activation`). The driver's display name lives on the site, tied
//! to their Orinks account; the game never transmits profile names, save
//! data, or anything about the real player. Every request's User-Agent does
//! carry the game's build identity (see [`client_version`]) so the site can
//! tell which release a post came from -- moderation and bug triage data,
//! never shown publicly.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use serde_json::{json, Value};

use crate::discord_presence::PresenceState;
use crate::net::{self, wait_seconds, Event, Headers, NetError, SharedTransport, Tier, Transport};
use crate::updater::{self, BuildInfo};
use ff_core::sim::real_traffic::{wall_clock, Clock};

/// The package version this build reports from source (`freight_fate.__version__`).
pub const PACKAGE_VERSION: &str = env!("CARGO_PKG_VERSION");

// The www host, deliberately: the apex orinks.net answers API calls with a
// 307 redirect to www, and urllib will not re-send a POST body through a
// redirect -- so heartbeats against the apex fail with HTTPError 307.
pub const PRODUCTION_BASE_URL: &str = "https://www.orinks.net";

// Production, as of the 1.9 cutover (2026-09-20). The 1.9 test line spent
// the prerelease pointed at the staged orinks-net deployment
// (dev.orinks.net, its own backend) so testers could exercise the 1.9
// validator and profile fields without touching production accounts or the
// live board. That backend went to production with the server stack, so the
// game reads the real site again.
//
// The staging site stayed up after the cutover for builds that still carried
// the old value, and closed on 2026-10-02; those builds can no longer reach
// it. Their staging careers never followed them here: driver identities,
// cloud backups and public profiles lived on the staging deployment and do
// not exist on production, so a staging player on a post-cutover build
// started fresh.
pub const DEFAULT_BASE_URL: &str = PRODUCTION_BASE_URL;

// Presence is by far the biggest source of backend reads and writes -- a
// driver on a long haul beats for hours, and it is the single largest line in
// the site's database usage -- so beat every two and a half minutes. The board
// drops a driver six minutes after their last beat, which still absorbs one
// dropped request before anyone is called gone; keep this and PRESENCE_TTL_MS
// on the server in step, and widen the server's window first, or a single lost
// beat will blink a driver off the board.
//
// Only the keep-alive slows down. A change of activity still pushes within
// MIN_CHANGE_INTERVAL_S, so going on duty, pulling over, or starting a new leg
// still reaches the board in seconds.
pub const HEARTBEAT_INTERVAL_S: f64 = 150.0;

// When the activity changes (new leg, pulled over, back on the road), push
// the update sooner than the next heartbeat -- but never more often than this.
pub const MIN_CHANGE_INTERVAL_S: f64 = 15.0;

// A None snapshot must persist this long before the sign-off is sent. The
// hauling states report through brief sub-menus unevenly, and this grace
// stops a two-second detour (a status screen, a confirmation prompt) from
// bouncing the driver off and back onto the public board.
pub const OFF_DUTY_GRACE_S: f64 = 20.0;

// A truck parked on the road with the game left running reports the identical
// snapshot for hours, and would squat the live board indefinitely while its
// heartbeats run up the site's largest database cost. After this long without
// any snapshot change the service signs off and goes quiet; any change --
// rolling again, a new leg, pulling into a stop -- re-lists the driver within
// MIN_CHANGE_INTERVAL_S. While actually driving the snapshot ticks every five
// percent of route progress (deadheads included), which even the longest haul
// clears in well under this window. The server hides idle rows on the same
// clock (PRESENCE_IDLE_MS in orinks-net's freightFate.ts) so older builds
// that never stop beating age off the board too.
pub const IDLE_SIGNOFF_S: f64 = 30.0 * 60.0;

// The pause menu's on-duty activity. A pause is not the end of a shift, so a
// paused player stays on the drivers list (and nobody's duty watch calls them
// off duty for a bathroom break) -- but a paused game has nothing new to say,
// so the service posts this snapshot once and then sends no heartbeats at
// all. The server holds a row that said this for the idle window instead of
// the heartbeat one (PAUSED_ACTIVITY in orinks-net's freightFate.ts; keep the
// two strings equal), so after IDLE_SIGNOFF_S the pause ages off like a
// parked truck, with the one idle sign-off this service sends anyway.
// Resuming is a change like any other and re-lists the driver in seconds.
pub const PAUSED_ACTIVITY: &str = "Paused";

// The drivers-board detail ends with what the cab radio is playing, opened by
// this phrase. A live stream's song title changes every few minutes on its
// own, so the idle clock reads the snapshot without it: a truck parked with
// the radio on is as idle as one parked in silence, and used to beat (and
// rewrite the board) all night, one song at a time (2026-10-03). The server
// strips the same clause before dating a change (RADIO_CLAUSE in orinks-net's
// freightFate.ts; keep the two strings equal).
pub const RADIO_CLAUSE: &str = "listening to ";

const WORKER_TICK_S: f64 = HEARTBEAT_INTERVAL_S;

/// Overrides the Orinks site root, for development, tests and the staging
/// agent session.
pub const ONLINE_URL_ENV: &str = "FREIGHT_FATE_ONLINE_URL";

/// The Orinks site root, overridable for development and tests.
pub fn base_url() -> String {
    std::env::var(ONLINE_URL_ENV)
        .unwrap_or_else(|_| DEFAULT_BASE_URL.to_string())
        .trim_end_matches('/')
        .to_string()
}

/// The player's driver setup page on whichever Orinks site this build
/// talks to.
///
/// Defined once, because the path is not something a player can be expected
/// to remember or type: everything that sends them there has to name the
/// same address, and on a staged build it has to be the staged host.
pub fn setup_page_url() -> String {
    format!("{}/freight-fate/online/setup", base_url())
}

/// The build identity this game reports with every Orinks request.
///
/// Packaged builds report their release tag (`v1.8.0`, or
/// `nightly-20260711` on the dev channel); a source checkout, which has no
/// build stamp, reports `source-<version>`. The site records the latest
/// value per driver so moderation can tell which build a suspicious profile
/// or save came from. It says nothing about the player or the machine.
pub fn client_version() -> String {
    client_version_for(
        updater::load_build_info(PACKAGE_VERSION).as_ref(),
        PACKAGE_VERSION,
    )
}

/// [`client_version`] for a given build stamp (`None` from source).
pub fn client_version_for(build: Option<&BuildInfo>, version: &str) -> String {
    let tag = match build {
        Some(b) => b.tag.clone(),
        None => format!("source-{version}"),
    };
    // User-Agent product tokens must stay printable and space-free; version
    // strings already are, but a malformed build stamp must not be able to
    // break every request header.
    let clean: String = tag
        .chars()
        .filter(|c| ('!'..='~').contains(c))
        .take(64)
        .collect();
    if clean.is_empty() {
        "unknown".to_string()
    } else {
        clean
    }
}

/// The headers `_http_json` sends: the build's User-Agent, JSON content
/// type, the caller's extras, and -- only when the environment asks -- the
/// Vercel protection bypass.
pub fn request_headers(extra: &[(String, String)], bypass: Option<&str>) -> Headers {
    let mut all: Headers = vec![
        (
            "User-Agent".to_string(),
            format!("FreightFate/{}", client_version()),
        ),
        ("Content-Type".to_string(), "application/json".to_string()),
    ];
    for (k, v) in extra {
        if let Some(slot) = all
            .iter_mut()
            .find(|(name, _)| name.eq_ignore_ascii_case(k))
        {
            slot.1 = v.clone();
        } else {
            all.push((k.clone(), v.clone()));
        }
    }
    if let Some(bypass) = bypass.filter(|b| !b.is_empty()) {
        // Vercel preview deployments are behind Deployment Protection, which
        // answers an unauthenticated API call with a redirect to SSO. This
        // lets a test build reach one without the project having to turn that
        // protection off for everybody. Unset in every shipped build.
        all.push(("x-vercel-protection-bypass".to_string(), bypass.to_string()));
    }
    all
}

/// `_http_json`: POST (or GET when `payload` is `None`) JSON to an Orinks
/// endpoint with the game's headers and decode the JSON reply.
pub fn http_json(
    url: &str,
    payload: Option<&Value>,
    headers: &[(String, String)],
    method: Option<&str>,
) -> Result<Value, NetError> {
    let bypass = std::env::var("FREIGHT_FATE_ONLINE_BYPASS").ok();
    let all = request_headers(headers, bypass.as_deref());
    net::request_json(Tier::Orinks, method, url, payload, &all)
}

/// The default [`Transport`]: [`http_json`] over the real network.
#[derive(Debug, Default, Clone, Copy)]
pub struct DefaultTransport;

impl Transport for DefaultTransport {
    fn call(
        &self,
        url: &str,
        payload: Option<&Value>,
        headers: &[(String, String)],
        method: Option<&str>,
    ) -> Result<Value, NetError> {
        http_json(url, payload, headers, method)
    }
}

/// The transport every service uses when none is injected.
pub fn default_transport() -> SharedTransport {
    Arc::new(DefaultTransport)
}

mod identity;

pub use identity::{
    allow_real_secret_store, clear_refused_secret_keys, real_secret_store_allowed,
    refused_secret_keys, secret_store_report, take_secret_store_timeout_notice, IdentityStore,
    KeyringStore, MemoryStore, OnlineIdentity, RefusingStore, SecretStore, TOKEN_SERVICE,
};

// -- verification and board helpers --------------------------------------------------

/// Check pasted credentials against Orinks without going on duty.
///
/// Posts one empty-activity presence request -- the server treats that as an
/// off-duty sign-off, so a valid pair changes nothing visible. Returns one
/// of `"ok"`, `"driver_not_found"` (the Driver ID is wrong),
/// `"unauthorized"` (the token is wrong or was rotated), `"rejected"`
/// (the server answered but refused the credentials for another reason,
/// e.g. a malformed paste), or `"error"` (network trouble; nothing
/// learned).
pub fn verify_identity(identity: &OnlineIdentity, transport: &dyn Transport) -> &'static str {
    let reply = transport.call(
        &format!("{}/api/freight-fate/presence", base_url()),
        Some(&json!({"driverId": identity.driver_id, "activity": "", "detail": ""})),
        &identity.auth_headers(),
        None,
    );
    match reply {
        Ok(reply) => {
            if truthy(reply.get("ok")) {
                "ok"
            } else {
                "error"
            }
        }
        Err(e @ NetError::Http { .. }) => {
            let code = e.http_code().unwrap_or(0);
            if code == 404 {
                return "driver_not_found";
            }
            if code == 401 {
                return "unauthorized";
            }
            // Keep the server's own explanation (Convex sends a JSON error body)
            // so a player-attached log tells us *why* a paste was refused.
            let body = e.body_excerpt();
            log::warn!("Online identity check failed: HTTP {code} {body}");
            if (400..500).contains(&code) {
                "rejected"
            } else {
                "error"
            }
        }
        Err(e) => {
            log::warn!("Online identity check failed: {e}");
            "error"
        }
    }
}

/// Set the authoritative public Profile-sharing state on orinks.net.
pub fn set_profile_sharing(
    identity: &OnlineIdentity,
    enabled: bool,
    transport: &dyn Transport,
) -> &'static str {
    let reply = transport.call(
        &format!("{}/api/freight-fate/profile-sharing", base_url()),
        Some(&json!({"driverId": identity.driver_id, "enabled": enabled})),
        &identity.auth_headers(),
        None,
    );
    match reply {
        Ok(reply) => {
            if truthy(reply.get("ok")) && reply.get("enabled") == Some(&Value::Bool(enabled)) {
                "ok"
            } else {
                "error"
            }
        }
        Err(NetError::Http { code, .. }) => {
            if code == 404 {
                return "driver_not_found";
            }
            if code == 401 {
                return "unauthorized";
            }
            log::warn!("Profile sharing update failed: HTTP {code}");
            "error"
        }
        Err(e) => {
            log::warn!("Profile sharing update failed: {e}");
            "error"
        }
    }
}

/// The current public drivers board, or `None` when unreachable.
///
/// Each entry has `displayName`, `activity`, `detail` and `updatedAt`
/// (epoch milliseconds). Called from a background thread by the in-game
/// "Drivers on duty" view; never called on the game loop.
pub fn fetch_board(transport: &dyn Transport) -> Option<Vec<Value>> {
    let reply = transport
        .call(
            &format!("{}/api/freight-fate/presence", base_url()),
            None,
            &[],
            None,
        )
        .map_err(|e| log::debug!("Online presence board fetch failed: {e}"))
        .ok()?;
    match reply.get("drivers") {
        Some(Value::Array(items)) => Some(items.clone()),
        _ => None,
    }
}

/// Every driver with a public profile, on duty or not, in the order the site
/// reads them: on duty first, then by when they were last on duty. `None`
/// when the site could not be reached.
pub fn fetch_directory(transport: &dyn Transport) -> Option<Vec<Value>> {
    let reply = transport
        .call(
            &format!("{}/api/freight-fate/directory", base_url()),
            None,
            &[],
            None,
        )
        .map_err(|e| log::debug!("Driver directory fetch failed: {e}"))
        .ok()?;
    match reply.get("drivers") {
        Some(Value::Array(items)) => Some(items.clone()),
        _ => None,
    }
}

/// The site's answer when asked for one driver's public profile.
#[derive(Debug, Clone, PartialEq)]
pub enum ProfileFetch {
    /// The profile as the site sent it: `driver`, `snapshot`, `presence`,
    /// `achievementCount`, `recentAchievements`, `events`.
    Profile(Value),
    /// The site answered, and this driver has no public profile: unknown,
    /// private, or held back. The site does not say which, and neither
    /// does the game.
    NotPublic,
    /// No usable answer.
    Unreachable,
}

/// One driver's public profile, as the in-game profile screen reads it.
///
/// Public data over the same unauthenticated door as the drivers list, so
/// it works with or without the player's own account. Called from a
/// background thread; never on the game loop.
pub fn fetch_driver_profile(driver_id: &str, transport: &dyn Transport) -> ProfileFetch {
    // Driver ids are slugs (letters, digits, dash, underscore). Anything else
    // has no business in a path, so it is dropped rather than encoded.
    let slug: String = driver_id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    if slug.is_empty() {
        return ProfileFetch::NotPublic;
    }
    let url = format!("{}/api/freight-fate/drivers/{slug}", base_url());
    match transport.call(&url, None, &[], None) {
        Ok(reply) if reply.get("driver").is_some_and(Value::is_object) => {
            ProfileFetch::Profile(reply)
        }
        Ok(other) => {
            log::debug!("Driver profile fetch for {slug} answered without a driver: {other}");
            ProfileFetch::Unreachable
        }
        Err(e) if e.http_code() == Some(404) => ProfileFetch::NotPublic,
        Err(e) => {
            log::debug!("Driver profile fetch for {slug} failed: {e}");
            ProfileFetch::Unreachable
        }
    }
}

/// Python truthiness of a JSON value (`bool(reply.get("ok"))`).
pub(crate) fn truthy(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().map(|f| f != 0.0).unwrap_or(true),
        Some(Value::String(s)) => !s.is_empty(),
        Some(Value::Array(a)) => !a.is_empty(),
        Some(Value::Object(o)) => !o.is_empty(),
    }
}

/// `str(value)` for the scalar JSON values a reply can carry.
pub(crate) fn py_str(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Bool(true) => "True".to_string(),
        Value::Bool(false) => "False".to_string(),
        Value::Null => "None".to_string(),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                i.to_string()
            } else if let Some(f) = n.as_f64() {
                ff_core::pyfmt::py_str_float(f)
            } else {
                n.to_string()
            }
        }
        other => other.to_string(),
    }
}

// -- presence service ---------------------------------------------------------

/// Everything [`OnlinePresence::new`] takes; `Default` is the disabled,
/// identity-less, real-network, threaded service the app builds.
pub struct OnlinePresenceOptions {
    pub enabled: bool,
    pub identity: Option<OnlineIdentity>,
    pub heartbeat_s: f64,
    pub min_change_s: f64,
    pub off_duty_grace_s: f64,
    pub idle_signoff_s: f64,
    pub clock: Clock,
    pub transport: SharedTransport,
    pub threaded: bool,
}

impl Default for OnlinePresenceOptions {
    fn default() -> Self {
        Self {
            enabled: false,
            identity: None,
            heartbeat_s: HEARTBEAT_INTERVAL_S,
            min_change_s: MIN_CHANGE_INTERVAL_S,
            off_duty_grace_s: OFF_DUTY_GRACE_S,
            idle_signoff_s: IDLE_SIGNOFF_S,
            clock: wall_clock(),
            transport: default_transport(),
            threaded: true,
        }
    }
}

#[derive(Default)]
struct PresenceState2 {
    desired: Option<PresenceState>,
    last_sent: Option<PresenceState>,
    last_send_t: Option<f64>,
    on_board: bool,
    none_since: Option<f64>,
    desired_changed_t: Option<f64>,
    // The last post failed and nothing has changed since. Its retry waits for
    // the heartbeat: a refused driver or an unreachable site used to be asked
    // again on every change window (or, for a sign-off, every worker wake).
    failed: bool,
}

struct Inner {
    identity: Mutex<Option<OnlineIdentity>>,
    enabled: AtomicBool,
    heartbeat: f64,
    min_change: f64,
    off_duty_grace: f64,
    idle_signoff: f64,
    clock: Clock,
    transport: SharedTransport,
    threaded: bool,
    state: Mutex<PresenceState2>,
    wake: Event,
    stop: Event,
    thread: Mutex<Option<JoinHandle<()>>>,
    started: AtomicBool,
}

/// Best-effort heartbeat sender for the live drivers board.
///
/// Gameplay calls [`update`](Self::update) with the active state's on-duty
/// [`PresenceState`] (or `None` when the player is not hauling); it returns
/// immediately. A worker thread owns all HTTP: it posts a heartbeat every
/// [`HEARTBEAT_INTERVAL_S`], pushes activity changes sooner (throttled), and
/// posts one empty-activity request to leave the board when the player goes
/// off duty. [`shutdown`](Self::shutdown) sends that sign-off too.
///
/// The worker is optional (`threaded: false`) so tests can drive the exact
/// same schedule/send logic synchronously with an injected clock and
/// transport.
#[derive(Clone)]
pub struct OnlinePresence {
    inner: Arc<Inner>,
}

impl OnlinePresence {
    pub fn new(options: OnlinePresenceOptions) -> Self {
        let enabled = options.enabled && options.identity.is_some();
        Self {
            inner: Arc::new(Inner {
                identity: Mutex::new(options.identity),
                enabled: AtomicBool::new(enabled),
                heartbeat: options.heartbeat_s.max(1.0),
                min_change: options.min_change_s.max(0.0),
                off_duty_grace: options.off_duty_grace_s.max(0.0),
                idle_signoff: options.idle_signoff_s.max(1.0),
                clock: options.clock,
                transport: options.transport,
                threaded: options.threaded,
                state: Mutex::new(PresenceState2::default()),
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

    /// Adopt freshly confirmed credentials (from the setup flow).
    pub fn set_identity(&self, identity: Option<OnlineIdentity>) {
        let none = identity.is_none();
        *self.inner.identity.lock().unwrap() = identity;
        if none {
            self.set_enabled(false);
        }
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
                .name("online-presence".to_string())
                .spawn(move || inner.run())
                .ok();
            *self.inner.thread.lock().unwrap() = handle;
        } else {
            self.inner.pump();
        }
    }

    /// Report the latest on-duty snapshot; `None` means off duty.
    pub fn update(&self, state: Option<PresenceState>) {
        if !self.enabled() {
            return;
        }
        {
            let mut st = self.inner.state.lock().unwrap();
            if state == st.desired {
                return;
            }
            // Any genuine change restarts the idle clock; the dedupe above
            // means a parked truck re-reporting the same snapshot does not,
            // and neither does the radio moving on to its next song.
            if drive_key(state.as_ref()) != drive_key(st.desired.as_ref()) {
                st.desired_changed_t = Some((self.inner.clock)());
            }
            st.desired = state;
            st.failed = false;
        }
        if self.inner.threaded {
            self.inner.wake.set();
        } else {
            self.inner.pump();
        }
    }

    /// Toggle at runtime (from the settings menu).
    pub fn set_enabled(&self, enabled: bool) {
        let enabled = enabled && self.inner.identity.lock().unwrap().is_some();
        if enabled == self.enabled() {
            return;
        }
        self.inner.enabled.store(enabled, Ordering::SeqCst);
        if enabled {
            {
                // Throwing the switch is proof the player is here, so the
                // idle clock starts over rather than keeping them off.
                let mut st = self.inner.state.lock().unwrap();
                st.last_sent = None;
                st.last_send_t = None;
                st.desired_changed_t = Some((self.inner.clock)());
            }
            self.start();
        } else {
            self.inner.stop_worker();
            self.inner.sign_off(0.0); // fire and forget; never stalls the menu
        }
    }

    /// Leave the board and stop the worker. Never raises.
    pub fn shutdown(&self) {
        self.inner.stop_worker();
        self.inner.sign_off(2.0);
    }

    /// Send at most one request: a change, a heartbeat, or a sign-off.
    /// Public so tests can drive the synchronous service step by step.
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

    /// Seconds until the next pump: the rest of the off-duty grace when a
    /// sign-off is brewing, soon for a pending change (once the change
    /// throttle allows it), otherwise the next scheduled heartbeat.
    fn worker_wait(&self) -> f64 {
        let now = (self.clock)();
        let st = self.state.lock().unwrap();
        let pending = st.desired != st.last_sent;
        if st.desired.is_none() {
            let Some(none_since) = st.none_since else {
                return WORKER_TICK_S;
            };
            if !st.on_board {
                return WORKER_TICK_S;
            }
            let until_grace = self.off_duty_grace - (now - none_since);
            let until_retry = match (st.failed, st.last_send_t) {
                (true, Some(t)) => self.heartbeat - (now - t),
                _ => 0.0,
            };
            return until_grace.max(until_retry).max(0.05);
        }
        // Idle and already signed off: nothing to send until a change, and a
        // new song is not one. (Idle but still on the board falls through, so
        // the sign-off -- or a failed sign-off's retry -- runs on the
        // heartbeat cadence like any post.)
        if !st.on_board && idle_for(&st, now) >= self.idle_signoff {
            return WORKER_TICK_S;
        }
        // Paused and listed as such: nothing to send until the idle sign-off.
        // Once that is due it falls through too, so a failed one waits out the
        // heartbeat instead of being posted again on every 50 ms wake.
        if !pending
            && st.on_board
            && idle_for(&st, now) < self.idle_signoff
            && st
                .desired
                .as_ref()
                .is_some_and(|d| d.activity == PAUSED_ACTIVITY)
        {
            return (self.idle_signoff - idle_for(&st, now)).max(0.05);
        }
        let Some(last_send_t) = st.last_send_t else {
            return WORKER_TICK_S;
        };
        let until_heartbeat = self.heartbeat - (now - last_send_t);
        if pending && !st.failed {
            let until_change = self.min_change - (now - last_send_t);
            return until_heartbeat.min(until_change).max(0.05);
        }
        until_heartbeat.max(0.05)
    }

    fn pump(&self) {
        if self.stop.is_set() || !self.enabled() || self.identity.lock().unwrap().is_none() {
            return;
        }
        let now = (self.clock)();
        let (desired, last_sent, on_board, none_since, last_send_t, idle, failed) = {
            let st = self.state.lock().unwrap();
            (
                st.desired.clone(),
                st.last_sent.clone(),
                st.on_board,
                st.none_since,
                st.last_send_t,
                idle_for(&st, now),
                st.failed,
            )
        };
        let since_send = last_send_t.map(|t| now - t);

        let Some(desired) = desired else {
            // Off duty: after a short grace (so a transient sub-menu does not
            // bounce the driver off the board), one sign-off request.
            if !on_board {
                return;
            }
            let none_since = match none_since {
                Some(t) => t,
                None => {
                    self.state.lock().unwrap().none_since = Some(now);
                    now
                }
            };
            if now - none_since < self.off_duty_grace {
                return;
            }
            if failed && since_send.is_some_and(|s| s < self.heartbeat) {
                return;
            }
            let ok = self.post("", "");
            let mut st = self.state.lock().unwrap();
            if ok {
                st.on_board = false;
                st.last_sent = None;
            }
            st.failed = !ok;
            st.last_send_t = Some(now);
            return;
        };

        self.state.lock().unwrap().none_since = None;
        let changed = Some(&desired) != last_sent.as_ref();
        if idle >= self.idle_signoff {
            // The same drive for this long means a parked truck and an absent
            // player: leave the board and stop heartbeating, whatever the
            // radio has moved on to since. Any real change restarts the idle
            // clock, so the next one re-lists the driver.
            if on_board {
                if failed && since_send.is_some_and(|s| s < self.heartbeat) {
                    return;
                }
                let ok = self.post("", "");
                let mut st = self.state.lock().unwrap();
                if ok {
                    st.on_board = false;
                }
                st.failed = !ok;
                st.last_send_t = Some(now);
            }
            return;
        }
        let due = if changed && !failed && since_send.is_none_or(|s| s >= self.min_change) {
            true // send the change now
        } else if desired.activity == PAUSED_ACTIVITY && !failed {
            // A paused game has said its one word; the server holds a paused
            // row for the idle window without beats, and the idle sign-off
            // above is the next thing it hears.
            false
        } else {
            // steady-state heartbeat keeps the TTL alive
            since_send.is_none_or(|s| s >= self.heartbeat)
        };
        if !due {
            return; // nothing due yet
        }

        let ok = self.post(&desired.activity, &desired.detail);
        let mut st = self.state.lock().unwrap();
        if ok {
            st.on_board = true;
            st.last_sent = Some(desired);
        }
        st.failed = !ok;
        // Count failures as attempts too, so an unreachable site is retried on
        // the heartbeat schedule instead of every worker wake-up.
        st.last_send_t = Some(now);
    }

    fn post(&self, activity: &str, detail: &str) -> bool {
        let identity = match self.identity.lock().unwrap().clone() {
            Some(id) => id,
            None => return false,
        };
        let reply = self.transport.call(
            &format!("{}/api/freight-fate/presence", base_url()),
            Some(&json!({
                "driverId": identity.driver_id,
                "activity": activity,
                "detail": detail,
            })),
            &identity.auth_headers(),
            None,
        );
        match reply {
            Ok(reply) => truthy(reply.get("ok")),
            Err(e) => {
                log::debug!("Online presence post failed: {e}");
                false
            }
        }
    }

    /// Best-effort empty-activity post so the board drops us promptly.
    ///
    /// Called from the game loop (settings toggle) and from shutdown, so the
    /// post runs on its own short-lived thread: a slow or unreachable site
    /// must never freeze the game. The board's TTL cleans up anyway if the
    /// post is lost. Synchronous mode keeps tests deterministic.
    fn sign_off(self: &Arc<Self>, wait_s: f64) {
        {
            let mut st = self.state.lock().unwrap();
            if !st.on_board {
                return;
            }
            st.on_board = false;
            st.last_sent = None;
        }
        if !self.threaded {
            self.post("", "");
            return;
        }
        let inner = Arc::clone(self);
        let done = Arc::new(Event::new());
        let flag = Arc::clone(&done);
        let _ = thread::Builder::new()
            .name("online-sign-off".to_string())
            .spawn(move || {
                inner.post("", "");
                flag.set();
            });
        if wait_s > 0.0 {
            wait_seconds(&done, wait_s);
        }
    }
}

/// The part of a snapshot that says what the truck is doing: everything but
/// the radio clause, which changes with every song on its own.
fn drive_key(state: Option<&PresenceState>) -> Option<(&str, &str)> {
    state.map(|s| (s.activity.as_str(), without_radio(&s.detail)))
}

/// `detail` with the trailing radio clause (see [`RADIO_CLAUSE`]) removed.
pub fn without_radio(detail: &str) -> &str {
    if detail.starts_with(RADIO_CLAUSE) {
        return "";
    }
    match detail.find(&format!(", {RADIO_CLAUSE}")) {
        Some(i) => &detail[..i],
        None => detail,
    }
}

/// Seconds the desired snapshot's drive has gone unchanged.
fn idle_for(st: &PresenceState2, now: f64) -> f64 {
    match st.desired_changed_t {
        Some(t) => now - t,
        None => 0.0,
    }
}

/// `thread.join(timeout=...)`: wait for the worker, but never hang the game
/// on it. A thread that outlives the wait is detached.
pub(crate) fn join_with_timeout(handle: JoinHandle<()>, timeout: Duration) {
    let deadline = std::time::Instant::now() + timeout;
    while !handle.is_finished() && std::time::Instant::now() < deadline {
        thread::sleep(Duration::from_millis(5));
    }
    if handle.is_finished() {
        let _ = handle.join();
    }
}
