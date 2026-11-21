//! Port of `freight_fate/cloud_saves.py` — optional cloud save backup:
//! careers mirrored to the player's Orinks account.
//!
//! This module is the *only* place that knows about the Orinks cloud save API.
//! After each local save the game hands the profile snapshot to
//! [`CloudSaves`], which uploads it (debounced, on a background thread) to
//! a revisioned slot on orinks.net under the same account-issued Driver ID and
//! token the drivers board already uses -- the player never handles a second
//! credential. Restores and conflict choices run through the Cloud backup menu
//! (the `cloud_save_states`).
//!
//! Everything here is best-effort and non-fatal by design, mirroring
//! `online_presence`: if the player is offline, the site is down, or the
//! feature is disabled, the game saves locally exactly as before. No error
//! ever propagates into the game loop.
//!
//! Sync model: last-write-wins with a conflict guard. Every upload names the
//! cloud revision it was based on; if another machine advanced the slot in the
//! meantime the server answers 409 and nothing is overwritten -- the slot is
//! marked conflicted here and the Cloud backup menu offers a spoken choice
//! between the two copies.
//!
//! Privacy: off by default and separate from public Profile sharing. Backups are
//! private to the player's own orinks.net account; only an allowlisted summary of the
//! latest accepted revision can supply detailed public statistics when Profile
//! sharing is independently enabled.
//!
//! The uploaded content is the profile JSON *without* the local HMAC signature
//! fields: the signing secret is per-machine. orinks.net validates that portable
//! payload before accepting a revision and signs it with Ed25519. Downloads are
//! hash-checked and signature-verified before any local file is touched; a
//! successful restore is immediately HMAC-signed for this installation.
//!
//! `Profile` lives outside this crate: the service takes profile snapshots as
//! JSON values, and [`restore_to_disk`] writes through the caller's hooks.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt;
use std::io::Read;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use serde_json::{Map, Value};

use crate::meaningful_play::{MeaningfulPlayReason, MeaningfulPlayStamp, MeaningfulPlayTracker};
use crate::net::{wait_seconds, Event, SharedTransport};
use crate::online_presence::{
    default_transport, join_with_timeout, py_str, truthy, OnlineIdentity,
};
use ff_core::models::profile::{Profile, LEGACY_SAVE_SUFFIX, SAVE_SUFFIX};
use ff_core::sim::real_traffic::{wall_clock, Clock};

// A save burst (delivery, achievement, rest) writes the file several times in
// a few seconds; the debounce collapses that into one upload.
pub const DEBOUNCE_S: f64 = 8.0;

// After a failed upload (site down, no network) retry on this cadence rather
// than every worker wake-up.
pub const RETRY_INTERVAL_S: f64 = 120.0;

// Matches MAX_SAVE_BYTES on the server; checked here so an oversized profile
// fails quietly in the log instead of with a rejected request.
pub const MAX_UPLOAD_BYTES: usize = 900 * 1024;

const WORKER_TICK_S: f64 = 60.0;

pub use ff_core::models::profile::origin::{cloud_content, SIGNATURE_FIELDS};

/// Raw ed25519 public keys by key id (`ff_core::cloud_save_integrity::public_keys`).
/// Passing `None` to the API calls below means the shipped table.
pub type PublicKeys = BTreeMap<String, Vec<u8>>;

/// The sanitized local file stem for a profile.
pub fn save_slot_name(profile_name: &str) -> String {
    let safe: String = profile_name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || matches!(c, ' ' | '-' | '_') {
                c
            } else {
                '_'
            }
        })
        .collect();
    let safe = safe.trim();
    if safe.is_empty() {
        "Driver".to_string()
    } else {
        safe.to_string()
    }
}

/// Decode downloaded content back to a profile dict. Errors when the bytes
/// are not a gzipped profile object.
pub fn profile_dict_from_content(content: &[u8]) -> Result<Value, String> {
    let mut decoder = flate2::read::GzDecoder::new(content);
    let mut raw = Vec::new();
    decoder
        .read_to_end(&mut raw)
        .map_err(|e| format!("cloud save content is not a gzipped profile: {e}"))?;
    let text = std::str::from_utf8(&raw)
        .map_err(|e| format!("cloud save content is not a gzipped profile: {e}"))?;
    let data: Value = serde_json::from_str(text)
        .map_err(|e| format!("cloud save content is not a gzipped profile: {e}"))?;
    if !data.is_object() {
        return Err("cloud save content is not a profile object".to_string());
    }
    Ok(data)
}

mod api;
mod manual;
mod slot_name;
mod sync_state;
mod wording;
mod worker;

pub use api::{
    backup_summary, delete_save, download_save, list_saves, restore_to_disk, set_public_save,
    upload_save, url_quote, DownloadError, RestoreError, RestoreHooks, SavesList,
};
pub use slot_name::{cloud_slot_name, local_name_for_slot};
pub use sync_state::SyncState;
pub use wording::*;

use api::reason_of;
use sync_state::{json_int, slot_conflict};

mod resume;

/// orinks.net answered but refused this machine's driver credentials.
///
/// Returned (never swallowed into a generic `None`) so the Cloud backup
/// menus can tell the player to reconnect instead of blaming the network.
/// The usual cause: the account issued a fresh driver token to another
/// computer, which retires the token stored on this one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CloudAuthError;

impl fmt::Display for CloudAuthError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("orinks.net no longer accepts this computer's sign-in")
    }
}

impl std::error::Error for CloudAuthError {}

// The dedupe key for the auth announcement: a paused sign-in is a property
// of this computer, not of any one career, so it is announced once per
// outage rather than once per slot -- loading a second career during the
// same outage must not repeat the byte-identical line. save_slot_name()
// replaces "*", so no real slot can ever collide with this key.
const AUTH_ANNOUNCED_KEY: &str = "*auth*";

// -- the backup service ---------------------------------------------------------

/// Everything [`CloudSaves::new`] takes. `Default` is the disabled,
/// identity-less, real-network, threaded service; the app fills in the
/// identity, the setting and the data directory.
pub struct CloudSavesOptions {
    /// Override verification keys for an injected cloud transport.
    pub public_keys: Option<PublicKeys>,
    pub enabled: bool,
    pub identity: Option<OnlineIdentity>,
    pub debounce_s: f64,
    pub retry_s: f64,
    pub clock: Clock,
    pub transport: SharedTransport,
    pub threaded: bool,
    /// `None` builds a fresh [`SyncState`] in `data_dir`.
    pub sync_state: Option<Arc<SyncState>>,
    /// Where the sync state lives when `sync_state` is `None`.
    pub data_dir: PathBuf,
    /// How often the background all-clear is spoken; see [`BackupAnnouncements`].
    pub backup_announcements: BackupAnnouncements,
}

/// How often a background backup's all-clear ("<career> is backed up.") is
/// spoken. The `backup_announcements` setting, as the service reads it.
///
/// Every accepted upload was the owner's 2026-08-15 call (silence read as
/// failure to a driver who cannot see the status line), and stays the
/// default. A tester who saves often hears it a lot (MariahL, 2026-09-02),
/// so the other two tiers thin it: once per career per session, or never.
/// Refusals and the "backed up again" recovery line are not part of the
/// quiet at any tier -- a career silently stopping backing up is the very
/// failure the all-clear was added to rule out.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BackupAnnouncements {
    #[default]
    Every,
    Once,
    Off,
}

impl BackupAnnouncements {
    /// The setting's stored word; anything unrecognised is the default.
    pub fn from_setting(value: &str) -> Self {
        match value {
            "once" => Self::Once,
            "off" => Self::Off,
            _ => Self::Every,
        }
    }
}

impl Default for CloudSavesOptions {
    fn default() -> Self {
        Self {
            public_keys: None,
            enabled: false,
            identity: None,
            debounce_s: DEBOUNCE_S,
            retry_s: RETRY_INTERVAL_S,
            clock: wall_clock(),
            transport: default_transport(),
            threaded: true,
            sync_state: None,
            data_dir: PathBuf::from("."),
            backup_announcements: BackupAnnouncements::Every,
        }
    }
}

#[derive(Clone)]
struct Pending {
    snapshot: Arc<Value>,
    meaningful_play: Option<MeaningfulPlayStamp>,
    queued_at: f64,
    token: i64,
}

#[derive(Default)]
struct State {
    // slot name -> (profile dict snapshot, queued-at time, attempt token).
    // The token rides with the snapshot so an upload's terminal result is
    // always recorded against the attempt that queued it, never against a
    // manual attempt that started while it was in flight.
    pending: HashMap<String, Pending>,
    retry_at: Option<f64>,
    // Manual "Save game" attempts (backup_now): the latest attempt token
    // handed out per slot, and the outcome recorded when an upload for
    // that slot reaches a terminal result.
    attempts: HashMap<String, i64>,
    outcomes: HashMap<String, (i64, String)>,
    // Spoken lines the background queue owes the player (drained by the
    // app's main loop through take_announcements), and per slot the
    // refusal cause already announced this session -- retries refused
    // for the same cause stay silent until the cause changes or the
    // slot uploads successfully. The worker thread writes, the main loop
    // drains.
    announcements: Vec<String>,
    announced_causes: HashMap<String, String>,
    // How often the all-clear speaks, and the careers whose all-clear has
    // been spoken this session (the "once" tier's memory).
    backup_announcements: BackupAnnouncements,
    all_clear_spoken: HashSet<String>,
    // Careers the server accepted after the owner reviewed them: the main
    // loop drains these through take_absolved and clears the loaded
    // career's "changed outside the game" mark.
    absolved: Vec<String>,
    status: String,
}

struct Inner {
    public_keys: Option<PublicKeys>,
    resume: Mutex<resume::Resume>,
    transfer: RwLock<()>,
    identity: Mutex<Option<OnlineIdentity>>,
    enabled: AtomicBool,
    debounce: f64,
    retry: f64,
    clock: Clock,
    transport: SharedTransport,
    threaded: bool,
    sync_state: Arc<SyncState>,
    meaningful_play: MeaningfulPlayTracker,
    state: Mutex<State>,
    wake: Event,
    stop: Event,
    thread: Mutex<Option<JoinHandle<()>>>,
    started: AtomicBool,
}

/// Best-effort save uploader for the player's Orinks account.
///
/// Gameplay never calls this directly: the profile save path invokes the
/// save listener after every successful local save, and
/// [`queue_backup`](Self::queue_backup) takes the snapshot and returns
/// immediately. A worker thread owns all HTTP: it debounces bursts of saves,
/// skips uploads whose content already matches the cloud, and records
/// conflicts for the Cloud backup menu to resolve.
/// [`shutdown`](Self::shutdown) flushes the pending upload briefly so
/// quitting right after a delivery still backs it up.
///
/// The worker is optional (`threaded: false`) so tests can drive the exact
/// same logic synchronously with an injected clock and transport.
#[derive(Clone)]
pub struct CloudSaves {
    inner: Arc<Inner>,
}

impl CloudSaves {
    pub fn new(options: CloudSavesOptions) -> Self {
        let enabled = options.enabled && options.identity.is_some();
        let meaningful_play = MeaningfulPlayTracker::new(&options.data_dir);
        let sync_state = options
            .sync_state
            .unwrap_or_else(|| Arc::new(SyncState::new(&options.data_dir)));
        Self {
            inner: Arc::new(Inner {
                public_keys: options.public_keys,
                resume: Mutex::new(resume::Resume::default()),
                transfer: RwLock::new(()),
                identity: Mutex::new(options.identity),
                enabled: AtomicBool::new(enabled),
                debounce: options.debounce_s.max(0.0),
                retry: options.retry_s.max(1.0),
                clock: options.clock,
                transport: options.transport,
                threaded: options.threaded,
                sync_state,
                meaningful_play,
                state: Mutex::new(State {
                    status: "Cloud backup is ready.".to_string(),
                    backup_announcements: options.backup_announcements,
                    ..State::default()
                }),
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

    pub fn identity(&self) -> Option<OnlineIdentity> {
        self.inner.identity.lock().unwrap().clone()
    }

    /// The sync state this service reads and records.
    pub fn sync_state(&self) -> &Arc<SyncState> {
        &self.inner.sync_state
    }

    /// The pending meaningful-play intents shared with gameplay hooks.
    pub fn meaningful_play_tracker(&self) -> &MeaningfulPlayTracker {
        &self.inner.meaningful_play
    }

    /// Mark a profile-name event against the sanitized cloud slot.
    pub fn mark_meaningful_play(&self, profile_name: &str, reason: MeaningfulPlayReason) {
        self.inner
            .meaningful_play
            .mark(&cloud_slot_name(profile_name), reason);
    }

    /// The transport this service uploads through (the menus' worker threads
    /// reuse it for list/delete/download).
    pub fn transport(&self) -> &SharedTransport {
        &self.inner.transport
    }

    /// Adopt freshly confirmed credentials (from the setup flow).
    pub fn set_identity(&self, identity: Option<OnlineIdentity>) {
        let none = identity.is_none();
        *self.inner.identity.lock().unwrap() = identity;
        if none {
            self.set_enabled(false);
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
            self.start();
        } else {
            self.inner.stop_worker();
            self.inner.state.lock().unwrap().pending.clear();
        }
    }

    /// How often the background all-clear speaks (from the settings menu).
    /// Takes effect on the next accepted upload; the "once" tier's memory of
    /// which careers were already announced this session is kept, so
    /// stepping through the tiers never repeats an all-clear.
    pub fn set_backup_announcements(&self, mode: BackupAnnouncements) {
        self.inner.state.lock().unwrap().backup_announcements = mode;
    }

    /// Begin the worker after app initialisation. Safe when disabled.
    pub fn start(&self) {
        if !self.enabled() || self.inner.started.load(Ordering::SeqCst) {
            return;
        }
        self.inner.started.store(true, Ordering::SeqCst);
        self.inner.forget_conflicts_with_no_local_save();
        self.inner.log_sync_state();
        self.inner.stop.clear();
        if self.inner.threaded {
            let inner = Arc::clone(&self.inner);
            let handle = thread::Builder::new()
                .name("cloud-saves".to_string())
                .spawn(move || inner.run())
                .ok();
            *self.inner.thread.lock().unwrap() = handle;
        }
    }

    /// Snapshot a just-saved profile for upload; returns immediately.
    /// `profile_name` is the profile's name (the slot is derived from it);
    /// `snapshot` is `profile.to_dict()`.
    pub fn queue_backup(&self, profile_name: &str, snapshot: Value) {
        if !self.enabled() {
            return;
        }
        let name = cloud_slot_name(profile_name);
        let meaningful_play = self.inner.meaningful_play.for_upload(&name);
        {
            let mut st = self.inner.state.lock().unwrap();
            // Token 0: a background save, which no manual watch ever matches.
            st.pending.insert(
                name,
                Pending {
                    snapshot: Arc::new(snapshot),
                    meaningful_play,
                    queued_at: (self.inner.clock)(),
                    token: 0,
                },
            );
        }
        if self.inner.threaded {
            self.inner.wake.set();
        } else {
            self.pump(false);
        }
    }

    /// Snapshot a just-saved profile and attempt its upload promptly.
    ///
    /// The manual "Save game" path (Shane's report, 2026-08-14: a silent
    /// background upload is indistinguishable from no backup for a screen
    /// reader user). Like [`queue_backup`](Self::queue_backup), but the
    /// snapshot is queued as already past the debounce, the transient-retry
    /// backoff is lifted for this attempt, and the worker is woken
    /// immediately. Every other semantic -- the content-hash skip, conflict,
    /// rejection, and auth handling -- is exactly the background queue's.
    ///
    /// Returns an attempt token the caller can poll through
    /// [`outcome_for`](Self::outcome_for) without ever blocking, or `None`
    /// when the service is off.
    pub fn backup_now(&self, profile_name: &str, snapshot: Value) -> Option<i64> {
        if !self.enabled() {
            return None;
        }
        let name = cloud_slot_name(profile_name);
        let meaningful_play = self.inner.meaningful_play.for_upload(&name);
        let token = {
            let mut st = self.inner.state.lock().unwrap();
            let token = st.attempts.get(&name).copied().unwrap_or(0) + 1;
            st.attempts.insert(name.clone(), token);
            // Queued as already debounce-old, so the next pump owes it an
            // attempt instead of a wait.
            st.pending.insert(
                name,
                Pending {
                    snapshot: Arc::new(snapshot),
                    meaningful_play,
                    queued_at: (self.inner.clock)() - self.inner.debounce,
                    token,
                },
            );
            // A manual save is the player asking now: this attempt does not
            // sit out a backoff armed by an earlier transient failure.
            st.retry_at = None;
            token
        };
        if self.inner.threaded {
            self.inner.wake.set();
        } else {
            self.pump(false);
        }
        Some(token)
    }

    /// The recorded outcome of a [`backup_now`](Self::backup_now) attempt,
    /// or `None` while it is still in flight. Never blocks.
    ///
    /// Outcomes: `"accepted"`, `"unchanged"` (the cloud already holds
    /// this exact content), `"conflict"` (recorded for the Cloud backup
    /// menu), `"stale"` (unchanged local copy, newer cloud revision),
    /// `"auth"`, `"network"` (still retrying in the background),
    /// or `"rejected:<reason>"`.
    pub fn outcome_for(&self, name: &str, token: i64) -> Option<String> {
        let st = self.inner.state.lock().unwrap();
        match st.outcomes.get(name) {
            Some((recorded, outcome)) if *recorded >= token => Some(outcome.clone()),
            _ => None,
        }
    }

    /// Drain the spoken lines the background queue owes the player.
    ///
    /// Automatic saves (rest stops, motels, deliveries, sleep, business
    /// actions) upload with no menu watching, so a refusal used to reach
    /// only the passive status line -- a blind player never heard that the
    /// career had stopped backing up. The worker thread never speaks;
    /// it queues lines here, and the app's main loop drains them every
    /// frame (the same polled pattern as ControllerManager.take_disconnect)
    /// onto the normal announcement channel. Thread-safe; empty almost
    /// always.
    pub fn take_announcements(&self) -> Vec<String> {
        let mut st = self.inner.state.lock().unwrap();
        std::mem::take(&mut st.announcements)
    }

    /// Slot names whose upload came back with `clearIntegrityFlag`: the
    /// owner reviewed the career and accepted it. Drained by the main loop,
    /// which clears the mark on the loaded career so its next backup goes up
    /// unmarked. Same polled pattern as [`take_announcements`](Self::take_announcements).
    pub fn take_absolved(&self) -> Vec<String> {
        let mut st = self.inner.state.lock().unwrap();
        std::mem::take(&mut st.absolved)
    }

    /// Flush the pending upload briefly and stop the worker. Never raises.
    pub fn shutdown(&self) {
        self.configure_resume(false);
        self.inner.stop_worker();
        if !self.enabled() {
            return;
        }
        let has_pending = !self.inner.state.lock().unwrap().pending.is_empty();
        if !has_pending {
            return;
        }
        if !self.inner.threaded {
            self.inner.pump(true);
            return;
        }
        // Quitting must not hang on a dead network: one bounded attempt.
        let inner = Arc::clone(&self.inner);
        let done = Arc::new(Event::new());
        let flag = Arc::clone(&done);
        let spawned = thread::Builder::new()
            .name("cloud-saves-flush".to_string())
            .spawn(move || {
                inner.pump(true);
                flag.set();
            });
        if spawned.is_ok() {
            done.wait(Duration::from_secs(5));
        }
    }

    /// Slots the server refused to overwrite, for the Cloud backup menu.
    pub fn conflicts(&self) -> BTreeMap<String, Map<String, Value>> {
        self.inner
            .sync_state
            .slots()
            .into_iter()
            .filter_map(|(name, entry)| slot_conflict(&entry).map(|c| (name, c)))
            .collect()
    }

    /// Short persistent player-facing result for the Cloud backup menu.
    pub fn status(&self) -> String {
        // Never claim readiness while the service is off: 1.9 testers heard
        // "ready" with the setting off and believed they were backed up.
        if !self.enabled() {
            return "Cloud backup is off. Saves on this computer are not backed up.".to_string();
        }
        self.inner.state.lock().unwrap().status.clone()
    }

    /// Upload every due pending slot. `force` ignores the debounce and
    /// retry backoff (shutdown flush).
    pub fn pump(&self, force: bool) {
        self.inner.pump(force);
    }

    /// Conflict choice: overwrite the cloud with this machine's save.
    ///
    /// Called from a menu worker thread. Uploads with the server's latest
    /// revision as parent, which the conflict entry recorded. Returns
    /// `"ok"` on success, or the classified failure family the caller
    /// needs to speak the real cause instead of always blaming the
    /// connection (Jessie's report, 2026-08-14; see
    /// [`classify_upload_failure`]): `"auth"`, `"conflict"` (the cloud
    /// moved again since this conflict was recorded), `"network"`, or --
    /// for a server rejection -- `"rejected:<reason>"`, carrying the raw
    /// reason code so the caller can build the same career-named,
    /// family-split story as the background queue via
    /// [`rejection_status`] (this menu is the exact button a conflicted
    /// tester presses, so a bare "rejected" tag with no career name or
    /// cause was not enough; see `cloud_save_states`).
    pub fn resolve_keep_mine(&self, name: &str, profile_dict: &Value) -> String {
        let Some(identity) = self.identity() else {
            return "network".to_string();
        };
        let slot = self.inner.sync_state.slot(name);
        let parent = slot_conflict(&slot).and_then(|c| json_int(c.get("latestRevision")));
        let meaningful_play = self.inner.meaningful_play.for_upload(name);
        let result = upload_save(
            &identity,
            name,
            profile_dict,
            parent,
            &backup_summary(profile_dict),
            meaningful_play.as_ref(),
            self.inner.transport.as_ref(),
        );
        if truthy(result.get("ok")) {
            self.inner.sync_state.record_synced_for_driver(
                name,
                json_int(result.get("revision")).unwrap_or(0),
                result
                    .get("contentHash")
                    .and_then(Value::as_str)
                    .unwrap_or(""),
                &identity.driver_id,
            );
            self.inner.sync_state.clear_conflict(name);
            self.inner.note_absolved(name, &result);
            if let Some(stamp) = &meaningful_play {
                self.inner
                    .meaningful_play
                    .clear_if_accepted(name, &stamp.operation_id);
            }
            return "ok".to_string();
        }
        if reason_of(&result) == Some("conflict") {
            // The cloud moved again since the conflict was recorded; refresh
            // the details so the menu speaks current numbers.
            self.inner.sync_state.record_conflict(name, &result);
            return "conflict".to_string();
        }
        let reason = reason_of(&result).map(str::to_string);
        log::warn!(
            "Cloud keep-mine upload of {name} failed: {}",
            reason.as_deref().unwrap_or("None")
        );
        let family = classify_upload_failure(reason.as_deref());
        if family == "rejected" {
            // Carry the raw reason through the return value the caller
            // already treats as an opaque tag, so it can speak the same
            // career-named, family-split story cases 1-4 speak -- never the
            // raw code itself, which stays log-only (logged just above).
            return format!("rejected:{}", reason.unwrap_or_default());
        }
        family.to_string()
    }
}
