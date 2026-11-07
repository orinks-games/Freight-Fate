//! Port of `freight_fate/online_activation.py` — device-code activation:
//! replaces the clipboard-paste credential setup.
//!
//! The old flow made a player copy a Driver ID and a token from the Orinks
//! driver setup page and paste both into the game (see
//! `online_presence::OnlineIdentity`). That is error-prone for a screen
//! reader user working across two applications with no shared clipboard
//! review. This module instead drives the OAuth-device-code style exchange
//! already live on orinks.net: the game asks for a short code, speaks it, the
//! player types that code into any browser (any device) and signs in there,
//! and the game polls until the site says the code was claimed.
//!
//! This is the *only* place that knows about the two activation endpoints. It
//! deliberately reuses `net::Transport`, `online_presence::http_json`, and
//! `online_presence::base_url` rather than forking them, so the game has
//! exactly one HTTP client and exactly one `FREIGHT_FATE_ONLINE_URL` override
//! for every Orinks endpoint.
//!
//! Both request/response shapes below are an existing, already-deployed server
//! contract -- do not change field names or status-code meanings here without
//! updating the server first.

use std::fmt;
use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use once_cell::sync::OnceCell;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::net::{NetError, Transport};
pub use crate::online_presence::base_url;
use crate::online_presence::OnlineIdentity;

/// An opaque, stable name for this computer, for the computer list.
///
/// orinks.net caps an account at ten computers and used to count
/// ACTIVATIONS: every freshly unzipped build that connected took another
/// slot, so a tester who takes a build a week filled the list with one PC
/// and was locked out (armstrong445, 2026-08-15). Sending this lets the
/// server replace that computer's entry instead of adding one.
///
/// Hashed, and never the hostname itself: the server only ever compares it
/// for equality, so there is no reason to hand over a name that might be a
/// person's. Python derived it from `uuid.getnode()` (the primary MAC) and
/// `platform.node()`: stable across a reinstall and across unzipping the
/// game somewhere else -- the two things that were minting duplicate rows.
///
/// On Windows this port reproduces that derivation exactly (the same
/// `UuidCreateSequential` node `uuid.getnode()` reads, and the same
/// `gethostname()`), so a computer activated by the Python build keeps its
/// row. Elsewhere there is no cheap MAC lookup without a new dependency, so
/// the seed is the hostname plus a random install key persisted once in
/// `data_dir` (`machine.key`): stable across reinstalls over the same data
/// directory, a fresh row when the game is unzipped somewhere new. That is
/// the documented deviation; the wire contract (32 lowercase hex characters
/// the server only ever compares) is unchanged.
///
/// This is NOT identity and never authenticates anything: it is a hint the
/// player triggers by deliberately connecting, and a wrong or missing one
/// costs only a duplicate row.
pub fn machine_key(data_dir: &Path) -> String {
    static KEY: OnceCell<String> = OnceCell::new();
    KEY.get_or_init(|| {
        let seed = format!("{}:{}", node_identity(data_dir), hostname());
        let digest = Sha256::digest(seed.as_bytes());
        hex::encode(digest)[..32].to_string()
    })
    .clone()
}

/// `f"{uuid.getnode():x}"` on Windows; the persisted install key elsewhere.
fn node_identity(data_dir: &Path) -> String {
    #[cfg(windows)]
    {
        let _ = data_dir;
        if let Some(node) = windows_node() {
            return format!("{node:x}");
        }
        install_key(data_dir)
    }
    #[cfg(not(windows))]
    {
        install_key(data_dir)
    }
}

/// A random 64-bit key minted once per data directory and kept in
/// `machine.key`; a missing or unreadable file mints a new one.
fn install_key(data_dir: &Path) -> String {
    let path = data_dir.join("machine.key");
    if let Ok(text) = fs::read_to_string(&path) {
        let trimmed = text.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let mut hasher = Sha256::new();
    hasher.update(nanos.to_le_bytes());
    hasher.update(std::process::id().to_le_bytes());
    hasher.update(hostname().as_bytes());
    let key = hex::encode(hasher.finalize())[..16].to_string();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let _ = fs::write(&path, &key);
    key
}

#[cfg(windows)]
fn windows_node() -> Option<u64> {
    // `uuid.getnode()` on Windows: the 48-bit node field of a sequential
    // UUID from rpcrt4, which is the primary adapter's MAC when there is one.
    // Declared here rather than through windows-sys so the crate's feature
    // set does not grow for one call.
    #[link(name = "rpcrt4")]
    extern "system" {
        fn UuidCreateSequential(uuid: *mut [u8; 16]) -> i32;
    }
    const RPC_S_OK: i32 = 0;
    const RPC_S_UUID_LOCAL_ONLY: i32 = 1824;
    const RPC_S_UUID_NO_ADDRESS: i32 = 1739;
    let mut raw = [0u8; 16];
    // SAFETY: UuidCreateSequential writes exactly one 16-byte UUID into the
    // buffer it is handed and touches nothing else.
    let status = unsafe { UuidCreateSequential(&mut raw) };
    if !matches!(
        status,
        RPC_S_OK | RPC_S_UUID_LOCAL_ONLY | RPC_S_UUID_NO_ADDRESS
    ) {
        return None;
    }
    // UUID(bytes_le=...).node: the last six bytes are the node, untouched by
    // the little-endian field swap.
    let node = raw[10..16]
        .iter()
        .fold(0u64, |acc, byte| (acc << 8) | u64::from(*byte));
    Some(node)
}

/// `platform.node()`: the bare network name of this computer.
pub fn hostname() -> String {
    #[cfg(windows)]
    {
        if let Some(name) = windows_gethostname() {
            return name;
        }
        std::env::var("COMPUTERNAME").unwrap_or_default()
    }
    #[cfg(not(windows))]
    {
        if let Some(name) = unix_gethostname() {
            return name;
        }
        std::env::var("HOSTNAME").unwrap_or_default()
    }
}

#[cfg(windows)]
fn windows_gethostname() -> Option<String> {
    // `socket.gethostname()` is Winsock's gethostname, which needs WSAStartup
    // once per process. WSADATA is opaque here (a 400-byte buffer is wider
    // than either ABI's struct).
    #[link(name = "ws2_32")]
    extern "system" {
        fn WSAStartup(version: u16, data: *mut u8) -> i32;
        fn gethostname(name: *mut u8, namelen: i32) -> i32;
    }
    let mut wsadata = [0u8; 512];
    // SAFETY: both calls write only within the buffers handed to them and
    // return an error code; WSAStartup's buffer is larger than WSADATA.
    unsafe {
        if WSAStartup(0x0202, wsadata.as_mut_ptr()) != 0 {
            return None;
        }
        let mut buf = [0u8; 256];
        if gethostname(buf.as_mut_ptr(), buf.len() as i32) != 0 {
            return None;
        }
        let end = buf.iter().position(|b| *b == 0).unwrap_or(buf.len());
        Some(String::from_utf8_lossy(&buf[..end]).into_owned())
    }
}

#[cfg(not(windows))]
fn unix_gethostname() -> Option<String> {
    extern "C" {
        fn gethostname(name: *mut std::os::raw::c_char, len: usize) -> std::os::raw::c_int;
    }
    let mut buf = [0u8; 256];
    // SAFETY: gethostname writes at most `len` bytes into the buffer.
    let rc = unsafe { gethostname(buf.as_mut_ptr() as *mut std::os::raw::c_char, buf.len()) };
    if rc != 0 {
        return None;
    }
    let end = buf.iter().position(|b| *b == 0).unwrap_or(buf.len());
    Some(String::from_utf8_lossy(&buf[..end]).into_owned())
}

// NATO phonetics for every letter the activation alphabet could ever contain.
// The alphabet itself (ABCDEFGHJKMNPQRTUVWXY346789, defined server-side)
// excludes O I L S Z 0 1 2 5 specifically so no two of these words are ever
// close enough to be confused for each other over a screen reader -- keeping
// the unused letters here too costs nothing and means this table never has
// to change if the server's alphabet ever does.
const PHONETIC: [&str; 26] = [
    "Alpha", "Bravo", "Charlie", "Delta", "Echo", "Foxtrot", "Golf", "Hotel", "India", "Juliett",
    "Kilo", "Lima", "Mike", "November", "Oscar", "Papa", "Quebec", "Romeo", "Sierra", "Tango",
    "Uniform", "Victor", "Whiskey", "Xray", "Yankee", "Zulu",
];

const DIGITS: [&str; 10] = [
    "zero", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine",
];

/// Spell an activation code letter-by-letter for a screen reader.
///
/// The game has no review cursor, so a player cannot step through a spoken
/// string character by character the way they can in a browser -- speaking
/// `WKQR-3468` once, as a word, gives them nothing to transcribe. This
/// returns NATO phonetics for letters and plain words for digits,
/// comma-separated (so a screen reader pauses between entries), with the
/// dash spoken too so a player copying the code by ear knows it belongs in
/// the string. Works on a code with or without the dash.
pub fn spell_code(code: &str) -> String {
    let mut words: Vec<String> = Vec::new();
    for ch in code.chars() {
        if ch == '-' {
            words.push("dash".to_string());
            continue;
        }
        let upper = ch.to_ascii_uppercase();
        if upper.is_ascii_uppercase() {
            words.push(PHONETIC[(upper as u8 - b'A') as usize].to_string());
        } else if ch.is_ascii_digit() {
            words.push(DIGITS[(ch as u8 - b'0') as usize].to_string());
        } else {
            // Defensive only: the server-issued alphabet never produces
            // anything else, but an unrecognised character still gets read
            // out verbatim instead of silently vanishing from the spelling.
            words.push(ch.to_string());
        }
    }
    words.join(", ")
}

/// An in-progress device-code activation.
///
/// `device_code` is the polling secret -- it is bound to this device and
/// never shown to the player, so it must never be logged or included in any
/// spoken or transcript-bound string. The `Debug` impl leaves it out so that
/// invariant is structural rather than a rule every future caller has to
/// remember: a stray `log::warn!("... {:?}", activation)` would otherwise
/// write the secret straight into the session log with nothing failing.
/// `user_code` is the short code the player reads back and types into a
/// browser. `expires_at` is a wall-clock (`time.time()`) deadline: the
/// server gives a relative `expires_in` in seconds, resolved to an absolute
/// time once, at start, so a caller checking it later doesn't need to
/// remember when the request was made.
#[derive(Clone, PartialEq)]
pub struct Activation {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub verification_uri_complete: String,
    pub expires_at: f64,
    pub interval: f64,
}

impl fmt::Debug for Activation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Activation")
            .field("user_code", &self.user_code)
            .field("verification_uri", &self.verification_uri)
            .field("verification_uri_complete", &self.verification_uri_complete)
            .field("expires_at", &self.expires_at)
            .field("interval", &self.interval)
            .finish_non_exhaustive()
    }
}

/// One poll's outcome.
///
/// `status` is one of `"pending"` (keep waiting), `"ready"` (claimed --
/// `driver_id`, `token` and `display_name` are all set), `"expired"` (the
/// code timed out, or was over an account's device cap; either way the fix
/// is the same: start over with a new code), `"retry"` (a transient failure
/// -- network trouble, an HTTP status other than 400/410, a reply the caller
/// could not parse, or a "ready" body with no `driver_id`/`token` in it --
/// that a caller should just poll again on the next tick, exactly like
/// "pending"), or `"error"` (HTTP 400 specifically: the server rejected the
/// stored device_code as malformed. Retrying the same code can never fix
/// this, so it must not be presented to the player as "expired" or as
/// something a moment's wait will resolve, the way "retry" is).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PollResult {
    pub status: String,
    pub driver_id: Option<String>,
    pub token: Option<String>,
    pub display_name: Option<String>,
    /// The guest driver this computer was signed in as moved onto the
    /// account: its journal and achievements are the account's now.
    pub guest_moved: bool,
}

impl PollResult {
    fn status(status: &str) -> Self {
        Self {
            status: status.to_string(),
            ..Default::default()
        }
    }
}

/// `time.time()`.
fn wall_time() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

/// `float(value)` for a JSON number or numeric string.
fn as_float(value: &Value) -> Option<f64> {
    match value {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse().ok(),
        Value::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
        _ => None,
    }
}

fn required_str(reply: &Value, key: &str) -> Option<String> {
    match reply.get(key)? {
        Value::String(s) => Some(s.clone()),
        other => Some(crate::online_presence::py_str(other)),
    }
}

/// Ask orinks.net to mint a new device code, or `None` if it could not.
///
/// `None` covers every failure the player can't do anything about but wait
/// and retry: rate limiting (429), the endpoint being down (503), a
/// malformed reply, or a network error. The caller (the setup menu) is
/// expected to show one generic "couldn't reach Orinks, try again" message
/// for all of them -- there is nothing actionable to say differently.
pub fn start_activation(transport: &dyn Transport, data_dir: &Path) -> Option<Activation> {
    start_activation_as(transport, data_dir, None)
}

/// [`start_activation`] for a guest driver connecting an account: its id and
/// token go with the request, so orinks.net can move its journal and
/// achievements to the account that claims the code. A server that does not
/// know guests ignores both.
pub fn start_activation_as(
    transport: &dyn Transport,
    data_dir: &Path,
    guest: Option<&OnlineIdentity>,
) -> Option<Activation> {
    // An older server ignores an unknown field, so this is safe to send
    // before the site half is everywhere.
    let mut body = json!({"machine_key": machine_key(data_dir)});
    let headers = match guest {
        Some(guest) => {
            body["guest_driver_id"] = Value::from(guest.driver_id.as_str());
            guest.auth_headers()
        }
        None => Vec::new(),
    };
    let reply = match transport.call(
        &format!("{}/api/freight-fate/activate/start", base_url()),
        Some(&body),
        &headers,
        None,
    ) {
        Ok(reply) => reply,
        Err(e) => {
            log::warn!("Activation start failed: {e}");
            return None;
        }
    };
    let parsed = (|| {
        Some(Activation {
            device_code: required_str(&reply, "device_code")?,
            user_code: required_str(&reply, "user_code")?,
            verification_uri: required_str(&reply, "verification_uri")?,
            verification_uri_complete: required_str(&reply, "verification_uri_complete")?,
            expires_at: wall_time() + as_float(reply.get("expires_in")?)?,
            interval: as_float(reply.get("interval")?)?,
        })
    })();
    if parsed.is_none() {
        log::warn!("Activation start returned a malformed reply");
    }
    parsed
}

/// Check whether `activation`'s code has been claimed yet.
///
/// Runs on a timer while the player waits at the setup screen, so no
/// error may ever escape to the caller -- a transient network blip must
/// not crash the menu. Never logs or otherwise surfaces
/// `activation.device_code`.
///
/// "error" is reserved for HTTP 400 alone. Everything else this function
/// cannot make sense of -- a dropped connection, a timeout, an SSL error, a
/// 5xx, an unexpected status code, a 200 body that is not even a mapping --
/// comes back as "retry" instead, so a caller (`OnlineSetupState`) can
/// just poll again on the next tick rather than forcing the player through
/// a fresh activation code for what is very likely a momentary blip.
pub fn poll_activation(activation: &Activation, transport: &dyn Transport) -> PollResult {
    let reply = match transport.call(
        &format!("{}/api/freight-fate/activate/poll", base_url()),
        Some(&json!({"device_code": activation.device_code})),
        &[],
        None,
    ) {
        Ok(reply) => reply,
        Err(NetError::Http { code: 410, .. }) => {
            // Covers both a timed-out code and an over-cap redeem -- the
            // player learns the real reason (too many computers on the
            // account) in the browser at claim time, so the game just
            // treats both as "get a new code".
            return PollResult::status("expired");
        }
        Err(NetError::Http { code: 400, .. }) => {
            // Malformed device_code is not "expired": retrying the same code
            // can never fix it, so it must surface as a distinct status
            // rather than telling the player to just wait it out.
            log::warn!("Activation poll rejected the stored device_code as malformed");
            return PollResult::status("error");
        }
        Err(NetError::Http { code, .. }) => {
            // Any other HTTP status (429, 5xx, ...) is treated as transient: none
            // of them mean the code itself is bad, so the same code can just be
            // polled again.
            log::warn!("Activation poll failed: HTTP {code}");
            return PollResult::status("retry");
        }
        Err(e) => {
            // Timeouts, TLS errors, and anything else that is not an HTTP
            // status are all connectivity trouble, not a verdict on the
            // code -- transient, same as an HTTP 5xx above.
            log::warn!("Activation poll failed: {e}");
            return PollResult::status("retry");
        }
    };

    // A 200 body that isn't even a mapping (null, a list, a bare string --
    // anything a broken deploy or a middlebox could hand back) must land here
    // too, not panic: this whole block, not just the HTTP branch above, is
    // the "never let an error escape" guarantee the docstring makes.
    let Value::Object(map) = &reply else {
        log::warn!("Activation poll returned a non-object reply: {reply}");
        return PollResult::status("retry");
    };
    let status = map.get("status").and_then(Value::as_str);
    match status {
        Some("ready") => {
            let driver_id = map.get("driver_id").filter(|v| truthy(v)).map(value_text);
            let token = map.get("token").filter(|v| truthy(v)).map(value_text);
            match (driver_id, token) {
                (Some(driver_id), Some(token)) => PollResult {
                    status: "ready".to_string(),
                    driver_id: Some(driver_id),
                    token: Some(token),
                    display_name: map
                        .get("display_name")
                        .filter(|v| !v.is_null())
                        .map(value_text),
                    guest_moved: map.get("guest_moved") == Some(&Value::Bool(true)),
                },
                _ => {
                    // A "ready" that carries no identity is not a claim the caller
                    // can act on: saving it would write a null identity, tell the
                    // player "Connected to orinks.net", and then quietly send
                    // "Bearer None" on every heartbeat until the next launch drops
                    // it. Treat it the same as any other body this module cannot
                    // make sense of -- transient, keep polling -- so a broken
                    // deploy or a body-rewriting middlebox resolves itself once
                    // the real claim lands, and expiry (not a wrong success) is
                    // the worst outcome.
                    log::warn!("Activation poll returned a ready reply with no identity");
                    PollResult::status("retry")
                }
            }
        }
        Some("pending") => PollResult::status("pending"),
        other => {
            // An unrecognised 200 body (a mapping missing "status", or an unknown
            // value) is most plausibly a transient server or proxy hiccup rather than
            // evidence about the code itself, so it gets the same "poll again" status
            // as a network blip -- not the terminal "error" that only HTTP 400 gets.
            log::warn!("Activation poll returned an unexpected status: {other:?}");
            PollResult::status("retry")
        }
    }
}

fn truthy(value: &Value) -> bool {
    crate::online_presence::truthy(Some(value))
}

fn value_text(value: &Value) -> String {
    crate::online_presence::py_str(value)
}
