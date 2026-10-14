//! Port of `freight_fate/net.py` — shared HTTPS helpers: one verified TLS
//! client per timeout tier, and speakable descriptions of network failures.
//!
//! Frozen Python builds could ship an OpenSSL whose CA paths were baked in on
//! the build machine, so `net.py` layered certifi's bundle on top of the
//! platform store. The Rust build talks TLS through rustls with the platform
//! verifier: the operating system's own roots (corporate proxy roots on
//! Windows included) on every platform, with no bundle to go stale.
//!
//! This module also owns the two things every online module used to reach
//! for separately in Python: the [`Transport`] trait (the `_http_json`
//! callable the services and their tests inject) and the [`NetError`] taxonomy
//! that stands in for `urllib.error.HTTPError` / `URLError` / `ssl` /
//! `socket` exceptions, so [`describe_error`] can speak the same sentences.

use std::fmt;
use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use once_cell::sync::Lazy;
use serde_json::Value;
use ureq::tls::{RootCerts, TlsConfig};
use ureq::Agent;

use ff_core::sim::real_traffic::{HttpTransport, TransportError};

pub mod testing;

// -- timeout tiers --------------------------------------------------------------

/// One shared client per timeout tier. Python built a fresh `urlopen` per
/// request against a cached `ssl_context()`; here the agent (connection pool
/// plus TLS config) is the cached thing, and the timeout is the only thing
/// that differs between the callers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Tier {
    /// orinks.net presence, activation, journal, cloud saves
    /// (`online_presence._REQUEST_TIMEOUT_S = 10`).
    Orinks,
    /// GitHub releases API and asset downloads (`updater.TIMEOUT = 15`).
    GitHub,
    /// Live weather and traffic feeds (`real_traffic.FETCH_TIMEOUT_S = 8`).
    Feeds,
}

impl Tier {
    /// The per-request timeout, in seconds, exactly as the Python module
    /// that owned each caller set it.
    pub fn timeout_s(self) -> f64 {
        match self {
            Tier::Orinks => 10.0,
            Tier::GitHub => 15.0,
            Tier::Feeds => 8.0,
        }
    }
}

fn build_agent(timeout_s: f64) -> Agent {
    let config = Agent::config_builder()
        .timeout_global(Some(Duration::from_secs_f64(timeout_s)))
        // Statuses are data here: the online modules read the server's JSON
        // error body out of a 4xx (Convex says *why* a paste was refused),
        // which an error-on-status client would have discarded.
        .http_status_as_error(false)
        .tls_config(
            TlsConfig::builder()
                .root_certs(RootCerts::PlatformVerifier)
                .build(),
        )
        .build();
    Agent::new_with_config(config)
}

static ORINKS_AGENT: Lazy<Agent> = Lazy::new(|| build_agent(Tier::Orinks.timeout_s()));
static GITHUB_AGENT: Lazy<Agent> = Lazy::new(|| build_agent(Tier::GitHub.timeout_s()));
static FEEDS_AGENT: Lazy<Agent> = Lazy::new(|| build_agent(Tier::Feeds.timeout_s()));

/// The update-download client: same TLS and status policy as the GitHub
/// tier, its 15 seconds kept on CONNECTING -- and no other deadline at
/// all. A fixed total deadline can never fit a 294 MB tester snapshot on
/// a real line: Python's `TIMEOUT = 15` was a per-operation socket timeout
/// that never bounded the whole transfer, and porting it as
/// `timeout_global` made every big update die mid-download ("timeout:
/// global") and the updater re-offer forever -- the tester "restart loop"
/// of 2026-08-31. Nor may `timeout_recv_response` stay: ureq charges the
/// body reads that follow against that same phase timer, so a bounded
/// response phase failed the retest at 25 percent ("timeout: receive
/// response"). A stalled transfer is the player's call: the download
/// screen speaks progress and Escape cancels.
static DOWNLOAD_AGENT: Lazy<Agent> = Lazy::new(|| {
    let timeout = Duration::from_secs_f64(Tier::GitHub.timeout_s());
    let config = Agent::config_builder()
        .timeout_connect(Some(timeout))
        .http_status_as_error(false)
        .tls_config(
            TlsConfig::builder()
                .root_certs(RootCerts::PlatformVerifier)
                .build(),
        )
        .build();
    Agent::new_with_config(config)
});

/// The client for streaming a release asset to disk. Everything else goes
/// through [`agent`]; downloads alone must outlive any total deadline.
pub fn download_agent() -> &'static Agent {
    &DOWNLOAD_AGENT
}

/// The shared client for a tier. `ssl_context()` in Python was
/// `lru_cache`d; these are built once per process the same way.
///
/// Holding an agent sends nothing -- it is a TLS config and an idle
/// connection pool -- so this is not where the network capability below is
/// checked. Every path that actually SENDS checks it: [`request`], and
/// `updater::apply::download`, which streams its body off this agent and so
/// calls [`require_real_network`] itself.
pub fn agent(tier: Tier) -> &'static Agent {
    match tier {
        Tier::Orinks => &ORINKS_AGENT,
        Tier::GitHub => &GITHUB_AGENT,
        Tier::Feeds => &FEEDS_AGENT,
    }
}

// -- who may reach the real network ------------------------------------------------
//
// Every byte this process sends anywhere -- orinks.net presence, activation,
// the journal outbox, cloud saves, the GitHub update check, and the live
// weather, traffic and parking feeds -- leaves through `request` below. It is
// the one door, so it is where the capability lives, exactly as
// `browser::open_url` is the one door to a web browser.
//
// # Why the default is "refuse"
//
// The seam used to be per service: a test injected a `Transport` and the
// service used it. That is fail OPEN in two ways at once.
//
// * `App::new_headless` builds `OnlinePresence`, `CloudSaves` and both
//   `JournalOutbox`es with `..Default::default()`, which is the LIVE
//   transport on background threads. Nothing an `install_transport` guard
//   does reaches them -- they carry their own. The only thing keeping a test
//   suite's heartbeats off orinks.net was that a pinned data directory yields
//   no driver identity; a test that adopted one and turned presence on would
//   have posted to the live site as the owner.
// * The live-data feeds have no injectable seam at the drive at all: a drive
//   with real weather on builds `RealWeatherProvider::with_nws(UreqTransport)`
//   and requests every city on the route before a test can swap in a fake.
//   Several cases do exactly that, and on 2026-08-24 the suite was measured
//   asking api.weather.gov about Chicago, Gary and Indianapolis from
//   `weather-city` and `weather-route` worker threads -- so what a drive was
//   carrying depended on whether the machine had a network and on what the
//   sky over Chicago was doing.
//
// So the capability is explicit and process-wide:
//
// * [`allow_real_network`] is called once, by `main()`, and only by `main()`.
//   A test binary has no `main()` of the game's, so no test process can ever
//   be granted it -- nothing to remember and nothing to forget.
// * Until it is called, [`request`] records the address in
//   [`refused_requests`] and panics. No socket is opened.
// * It is an `AtomicBool` rather than a thread-local on purpose. Every one of
//   these callers runs on a background worker -- presence, the outboxes,
//   cloud saves, `weather-<city>` -- and a per-thread guard is invisible to a
//   thread the test did not spawn. That is precisely the escape route
//   discipline cannot close.
//
// The panic is deliberate. A quiet error return reads to every caller here as
// "the network is down", which is a state the game handles gracefully and
// nobody would ever look at -- and for the weather provider it reads as
// literally `unavailable()`. On a worker thread the panic is not seen by the
// test runner, which is what [`refused_requests`] is for: the attempt is on
// the record whether or not anything was watching.

/// Set once by `main()`. Process-wide: a spawned worker sees it exactly as
/// the game loop does, which a thread-local could not manage.
static REAL_NETWORK_ALLOWED: AtomicBool = AtomicBool::new(false);

/// Every request that was refused, `"METHOD url"`, in order.
static REFUSED_REQUESTS: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// "This process is the real game": from here on [`request`] may reach the
/// network.
///
/// Called from `main()` and nowhere else. Nothing undoes it -- a capability
/// that can be handed back is one a stray call can take away from a player
/// mid-session.
pub fn allow_real_network() {
    REAL_NETWORK_ALLOWED.store(true, Ordering::SeqCst);
}

/// Whether the real network may be reached in this process.
pub fn real_network_allowed() -> bool {
    REAL_NETWORK_ALLOWED.load(Ordering::SeqCst)
}

/// Every request [`request`] refused, oldest first, as `"METHOD url"`.
pub fn refused_requests() -> Vec<String> {
    REFUSED_REQUESTS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
}

/// Forget the refusals so far.
pub fn clear_refused_requests() {
    REFUSED_REQUESTS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clear();
}

/// Refuse, record and panic unless this process is the game.
///
/// [`request`] calls this for every caller that goes through it. The one
/// caller that does not -- the update downloader, which streams a release
/// archive straight off [`agent`] rather than buffering it -- calls it
/// directly, so there is no send path in the crate the capability does not
/// cover.
///
/// # Panics
///
/// When [`allow_real_network`] has not been called.
pub fn require_real_network(method: &str, url: &str) {
    if !real_network_allowed() {
        refuse_request(method, url);
    }
}

#[cold]
fn refuse_request(method: &str, url: &str) -> ! {
    REFUSED_REQUESTS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .push(format!("{method} {url}"));
    panic!(
        "refusing to send {method} {url} over the real network: this process \
         never called net::allow_real_network(), so it is not the game. If \
         this is a test, inject a transport -- net::testing::FakeTransport \
         for an orinks.net service, a fake fetch or provider for a live feed \
         -- into whatever is reaching for the wire, and assert on what it \
         recorded."
    );
}

// -- the error taxonomy -----------------------------------------------------------

/// What went wrong with an HTTP request, in the families Python's exception
/// types drew for `describe_error`. Carries the server's status and body
/// for an HTTP-level refusal because the online modules read both.
#[derive(Debug, Clone)]
pub enum NetError {
    /// `urllib.error.HTTPError`: the server answered with a non-2xx status.
    Http { code: u16, body: Vec<u8> },
    /// `ssl.SSLCertVerificationError`.
    CertVerification(String),
    /// `ssl.SSLError` (any other TLS failure).
    Tls(String),
    /// `socket.gaierror`: DNS resolution failed.
    HostNotFound(String),
    /// `TimeoutError`.
    Timeout(String),
    /// `ConnectionError` (refused, reset, aborted, broken pipe).
    Connection(String),
    /// Anything else: `str(e)`, with the exception's type name as the
    /// fallback when the message is empty.
    Other { type_name: String, message: String },
}

impl NetError {
    /// An HTTP error with an empty body, the shape most tests build.
    pub fn http(code: u16) -> Self {
        NetError::Http {
            code,
            body: Vec::new(),
        }
    }

    /// An HTTP error whose body is the given JSON document.
    pub fn http_json(code: u16, body: &Value) -> Self {
        NetError::Http {
            code,
            body: serde_json::to_vec(body).unwrap_or_default(),
        }
    }

    /// `OSError("message")` and friends: a generic failure.
    pub fn other(type_name: &str, message: &str) -> Self {
        NetError::Other {
            type_name: type_name.to_string(),
            message: message.to_string(),
        }
    }

    /// The HTTP status when this is an HTTP-level refusal.
    pub fn http_code(&self) -> Option<u16> {
        match self {
            NetError::Http { code, .. } => Some(*code),
            _ => None,
        }
    }

    /// The server's JSON error body as an object (`{}` when there is none
    /// or it is not an object) -- `cloud_saves._error_body`.
    pub fn error_body(&self) -> serde_json::Map<String, Value> {
        match self {
            NetError::Http { body, .. } => match serde_json::from_slice::<Value>(body) {
                Ok(Value::Object(map)) => map,
                _ => serde_json::Map::new(),
            },
            _ => serde_json::Map::new(),
        }
    }

    /// The first 500 characters of the body, decoded leniently -- what the
    /// identity check logs so a player-attached log tells us why a paste was
    /// refused.
    pub fn body_excerpt(&self) -> String {
        match self {
            NetError::Http { body, .. } => {
                String::from_utf8_lossy(body).chars().take(500).collect()
            }
            _ => String::new(),
        }
    }
}

impl fmt::Display for NetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NetError::Http { code, .. } => write!(f, "HTTP Error {code}"),
            NetError::CertVerification(m)
            | NetError::Tls(m)
            | NetError::HostNotFound(m)
            | NetError::Timeout(m)
            | NetError::Connection(m) => f.write_str(m),
            NetError::Other { type_name, message } => {
                if message.is_empty() {
                    f.write_str(type_name)
                } else {
                    f.write_str(message)
                }
            }
        }
    }
}

impl std::error::Error for NetError {}

impl From<io::Error> for NetError {
    fn from(err: io::Error) -> Self {
        classify_io(&err)
    }
}

fn classify_io(err: &io::Error) -> NetError {
    use io::ErrorKind::*;
    let message = err.to_string();
    match err.kind() {
        TimedOut => NetError::Timeout(message),
        ConnectionRefused | ConnectionReset | ConnectionAborted | NotConnected | BrokenPipe => {
            NetError::Connection(message)
        }
        _ => {
            // rustls surfaces handshake failures as io errors wrapping its
            // own error type; the text is the only handle on which one.
            let lower = message.to_ascii_lowercase();
            if lower.contains("certificate")
                || lower.contains("unknownissuer")
                || lower.contains("unknown issuer")
                || lower.contains("invalid peer")
            {
                NetError::CertVerification(message)
            } else if lower.contains("tls")
                || lower.contains("handshake")
                || lower.contains("alert")
            {
                NetError::Tls(message)
            } else {
                NetError::Other {
                    type_name: "OSError".to_string(),
                    message,
                }
            }
        }
    }
}

impl From<ureq::Error> for NetError {
    fn from(err: ureq::Error) -> Self {
        match err {
            ureq::Error::StatusCode(code) => NetError::http(code),
            ureq::Error::Timeout(t) => NetError::Timeout(format!("timeout: {t}")),
            ureq::Error::HostNotFound => NetError::HostNotFound("getaddrinfo failed".to_string()),
            ureq::Error::ConnectionFailed => NetError::Connection("connection failed".to_string()),
            ureq::Error::Io(io_err) => classify_io(&io_err),
            ureq::Error::Tls(msg) => {
                let lower = msg.to_ascii_lowercase();
                if lower.contains("certificate") || lower.contains("verif") {
                    NetError::CertVerification(msg.to_string())
                } else {
                    NetError::Tls(msg.to_string())
                }
            }
            other => NetError::Other {
                type_name: "URLError".to_string(),
                message: other.to_string(),
            },
        }
    }
}

impl From<serde_json::Error> for NetError {
    fn from(err: serde_json::Error) -> Self {
        NetError::Other {
            type_name: "JSONDecodeError".to_string(),
            message: err.to_string(),
        }
    }
}

/// A short, speakable reason for a failed HTTP request.
///
/// Spoken to the player after lines like "Could not reach the update
/// server", so each message is a complete plain sentence.
pub fn describe_error(e: &NetError) -> String {
    match e {
        NetError::Http { code, .. } => format!("The server answered with error {code}."),
        NetError::CertVerification(_) => "The secure connection could not be verified.".to_string(),
        NetError::Tls(_) => "The secure connection failed.".to_string(),
        NetError::HostNotFound(_) => "The server address could not be found.".to_string(),
        NetError::Timeout(_) => "The connection timed out.".to_string(),
        NetError::Connection(_) => "The connection was refused or dropped.".to_string(),
        NetError::Other { type_name, message } => {
            let text = message.trim();
            if text.is_empty() {
                format!("{type_name}.")
            } else {
                format!("{text}.")
            }
        }
    }
}

// -- raw requests -------------------------------------------------------------------

/// One header list, in the order it is sent. A `Vec` rather than a map so
/// tests can assert on it the way the Python tests assert on the dict.
pub type Headers = Vec<(String, String)>;

/// The value of `name` in `headers`, matched case-insensitively.
pub fn header<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.as_str())
}

/// A raw HTTP reply: status plus the full body.
#[derive(Debug, Clone)]
pub struct RawResponse {
    pub status: u16,
    pub body: Vec<u8>,
}

/// Send one request on `tier`'s shared client. `method` is `GET` when
/// there is no body and `POST` when there is, unless overridden. A status
/// of 400 or above is returned as [`NetError::Http`] with the body attached,
/// which is how `urllib` raised `HTTPError`.
///
/// # Panics
///
/// When [`allow_real_network`] has not been called, which outside the real
/// game means something in a test process reached for the live web. The
/// address is recorded in [`refused_requests`] first and no socket is opened.
pub fn request(
    tier: Tier,
    method: Option<&str>,
    url: &str,
    body: Option<&[u8]>,
    headers: &[(String, String)],
) -> Result<RawResponse, NetError> {
    let method = method
        .map(|m| m.to_ascii_uppercase())
        .unwrap_or_else(|| if body.is_some() { "POST" } else { "GET" }.to_string());
    // Before the agent is built, before a name is resolved: nothing leaves
    // this process until something has said it is the game.
    if !real_network_allowed() {
        refuse_request(&method, url);
    }
    let agent = agent(tier);
    let mut response = match (method.as_str(), body) {
        ("GET", _) => {
            let mut req = agent.get(url);
            for (k, v) in headers {
                req = req.header(k.as_str(), v.as_str());
            }
            req.call()?
        }
        ("DELETE", None) => {
            let mut req = agent.delete(url);
            for (k, v) in headers {
                req = req.header(k.as_str(), v.as_str());
            }
            req.call()?
        }
        ("DELETE", Some(data)) => {
            let mut req = agent.delete(url).force_send_body();
            for (k, v) in headers {
                req = req.header(k.as_str(), v.as_str());
            }
            req.send(data)?
        }
        ("PUT", data) => {
            let mut req = agent.put(url);
            for (k, v) in headers {
                req = req.header(k.as_str(), v.as_str());
            }
            req.send(data.unwrap_or(&[]))?
        }
        ("PATCH", data) => {
            let mut req = agent.patch(url);
            for (k, v) in headers {
                req = req.header(k.as_str(), v.as_str());
            }
            req.send(data.unwrap_or(&[]))?
        }
        (_, data) => {
            let mut req = agent.post(url);
            for (k, v) in headers {
                req = req.header(k.as_str(), v.as_str());
            }
            req.send(data.unwrap_or(&[]))?
        }
    };
    let status = response.status().as_u16();
    if tier == Tier::Orinks {
        if let Some(date) = response.headers().get("date") {
            note_server_date(date.to_str().unwrap_or_default());
        }
    }
    let body = response
        .body_mut()
        .with_config()
        .limit(64 * 1024 * 1024)
        .read_to_vec()?;
    if status >= 400 {
        return Err(NetError::Http { code: status, body });
    }
    Ok(RawResponse { status, body })
}

// -- the server's clock ------------------------------------------------------------------

/// The last `Date` orinks.net sent, and when on this machine's monotonic
/// clock it arrived. Badges tied to a real-world date read this instead of
/// the system clock, which the player can set to any day they like.
static SERVER_CLOCK: Mutex<Option<(DateTime<Utc>, Instant)>> = Mutex::new(None);

/// Record an HTTP `Date` header (RFC 2822 form, "Sun, 28 Sep 2026 14:05:09
/// GMT"). One that does not parse is ignored.
pub fn note_server_date(header: &str) {
    if let Ok(date) = DateTime::parse_from_rfc2822(header) {
        *SERVER_CLOCK.lock().unwrap_or_else(|p| p.into_inner()) =
            Some((date.with_timezone(&Utc), Instant::now()));
    }
}

/// orinks.net's idea of now: its last `Date`, carried forward on the
/// monotonic clock. `None` until the game has heard from the server this
/// session, so an offline game never has one.
pub fn server_now() -> Option<DateTime<Utc>> {
    if let Some(at) = testing::server_time_override() {
        return at;
    }
    let reading = *SERVER_CLOCK.lock().unwrap_or_else(|p| p.into_inner());
    reading.and_then(|(date, heard)| {
        chrono::Duration::from_std(heard.elapsed())
            .ok()
            .map(|since| date + since)
    })
}

/// `json.loads(resp.read().decode("utf-8"))`.
pub fn decode_json(body: &[u8]) -> Result<Value, NetError> {
    let text = std::str::from_utf8(body).map_err(|e| NetError::Other {
        type_name: "UnicodeDecodeError".to_string(),
        message: e.to_string(),
    })?;
    Ok(serde_json::from_str(text)?)
}

/// Send JSON (or GET when `payload` is `None`) and decode the JSON reply.
pub fn request_json(
    tier: Tier,
    method: Option<&str>,
    url: &str,
    payload: Option<&Value>,
    headers: &[(String, String)],
) -> Result<Value, NetError> {
    let data = match payload {
        Some(value) => Some(serde_json::to_vec(value)?),
        None => None,
    };
    let response = request(tier, method, url, data.as_deref(), headers)?;
    decode_response_json(response)
}

fn decode_response_json(response: RawResponse) -> Result<Value, NetError> {
    if !(200..300).contains(&response.status) {
        return Err(NetError::Http {
            code: response.status,
            body: response.body,
        });
    }
    decode_json(&response.body)
}

#[cfg(test)]
mod response_tests {
    use super::*;

    #[test]
    fn json_transport_rejects_non_success_statuses() {
        let response = RawResponse {
            status: 302,
            body: br#"{"status":"answered"}"#.to_vec(),
        };
        assert!(matches!(
            decode_response_json(response),
            Err(NetError::Http { code: 302, .. })
        ));
    }

    #[test]
    fn json_transport_accepts_success_status_and_valid_json() {
        let response = RawResponse {
            status: 200,
            body: br#"{"status":"answered"}"#.to_vec(),
        };
        assert_eq!(
            decode_response_json(response).unwrap(),
            serde_json::json!({"status": "answered"})
        );
    }
}

// -- the injectable transport ----------------------------------------------------------

/// The `Transport` callable of `online_presence.py`: posts (or gets, when
/// `payload` is `None`) JSON and returns the decoded JSON reply. Injected in
/// tests; the default (`online_presence::DefaultTransport`) stamps the
/// game's User-Agent and talks to `Tier::Orinks`.
///
/// `method` is the `_http_delete` wrapper's `method="DELETE"`; `None` means
/// the usual POST-or-GET rule.
pub trait Transport: Send + Sync {
    fn call(
        &self,
        url: &str,
        payload: Option<&Value>,
        headers: &[(String, String)],
        method: Option<&str>,
    ) -> Result<Value, NetError>;
}

/// The `Transport` everything reaches for when none is injected.
pub type SharedTransport = Arc<dyn Transport>;

// -- the live-data transport --------------------------------------------------------------

/// [`HttpTransport`] for the live weather / traffic / parking providers in
/// `ff_core`, on the `Feeds` tier. Implemented once here and reused.
#[derive(Debug, Default, Clone, Copy)]
pub struct UreqTransport;

impl UreqTransport {
    fn send(
        &self,
        method: Option<&str>,
        url: &str,
        body: Option<&[u8]>,
        headers: &[(&str, &str)],
    ) -> Result<Vec<u8>, TransportError> {
        let owned: Headers = headers
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        match request(Tier::Feeds, method, url, body, &owned) {
            Ok(reply) => Ok(reply.body),
            Err(e) => Err(TransportError::new(describe_error(&e))),
        }
    }
}

impl HttpTransport for UreqTransport {
    fn get(
        &self,
        url: &str,
        headers: &[(&str, &str)],
        _timeout_s: f64,
    ) -> Result<Vec<u8>, TransportError> {
        self.send(Some("GET"), url, None, headers)
    }

    fn post(
        &self,
        url: &str,
        body: &[u8],
        headers: &[(&str, &str)],
        _timeout_s: f64,
    ) -> Result<Vec<u8>, TransportError> {
        self.send(Some("POST"), url, Some(body), headers)
    }
}

// -- a small shared thread primitive -----------------------------------------------------------

/// `threading.Event`: a flag a worker can wait on with a timeout. Every
/// background service here (presence, cloud saves, discord, journal) is
/// built from two of these -- `_wake` and `_stop` -- so they share one
/// implementation.
#[derive(Debug, Default)]
pub struct Event {
    state: Mutex<bool>,
    cv: Condvar,
}

impl Event {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set(&self) {
        let mut guard = self.state.lock().unwrap_or_else(|e| e.into_inner());
        *guard = true;
        self.cv.notify_all();
    }

    pub fn clear(&self) {
        let mut guard = self.state.lock().unwrap_or_else(|e| e.into_inner());
        *guard = false;
    }

    pub fn is_set(&self) -> bool {
        *self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Block until set or `timeout` elapses; returns whether it was set.
    pub fn wait(&self, timeout: Duration) -> bool {
        let guard = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if *guard {
            return true;
        }
        let (guard, _) = self
            .cv
            .wait_timeout_while(guard, timeout, |set| !*set)
            .unwrap_or_else(|e| e.into_inner());
        *guard
    }
}

/// `threading.Event.wait(seconds)` with a float timeout; negative or NaN
/// waits are treated as zero.
pub fn wait_seconds(event: &Event, seconds: f64) -> bool {
    let secs = if seconds.is_finite() && seconds > 0.0 {
        seconds
    } else {
        0.0
    };
    event.wait(Duration::from_secs_f64(secs))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The download client must never carry a total deadline: one bounded
    /// every update download at the same place regardless of connection
    /// speed, so the 294 MB tester snapshot always died mid-transfer and
    /// the updater re-offered it forever. Connecting and the response
    /// headers keep the GitHub tier's timeout; the body must be unbounded.
    #[test]
    fn download_agent_has_no_total_deadline() {
        let config = download_agent().config();
        assert_eq!(config.timeouts().global, None);
        // No response-phase deadline either: ureq charges body reads
        // against it, which re-broke the download at 25 percent.
        assert_eq!(config.timeouts().recv_response, None);
        let expected = Some(Duration::from_secs_f64(Tier::GitHub.timeout_s()));
        assert_eq!(config.timeouts().connect, expected);
    }

    /// The request tiers keep their deadlines: an API call that hangs must
    /// still fail fast.
    #[test]
    fn request_tiers_keep_their_global_deadlines() {
        for tier in [Tier::Orinks, Tier::GitHub, Tier::Feeds] {
            assert_eq!(
                agent(tier).config().timeouts().global,
                Some(Duration::from_secs_f64(tier.timeout_s())),
                "{tier:?}"
            );
        }
    }
}
