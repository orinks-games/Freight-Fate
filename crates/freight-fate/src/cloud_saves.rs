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

/// Raw ed25519 public keys by key id (the shape
/// `ff_core::cloud_save_integrity::public_keys()` returns); `None` in the
/// calls below means the shipped table.
pub type PublicKeys = BTreeMap<String, Vec<u8>>;

/// The cloud slot for a profile: the same sanitized stem as its file name
/// (Profile.path), so slot and local file always pair up.
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
mod sync_state;
mod wording;

pub use api::{
    backup_summary, delete_save, download_save, list_saves, restore_to_disk, set_public_save,
    upload_save, url_quote, DownloadError, RestoreError, RestoreHooks, SavesList,
};
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
            .mark(&save_slot_name(profile_name), reason);
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
        let name = save_slot_name(profile_name);
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
        let name = save_slot_name(profile_name);
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

impl Inner {
    fn enabled(&self) -> bool {
        self.enabled.load(Ordering::SeqCst)
    }

    /// Drop conflicts for careers this computer no longer has.
    ///
    /// A conflict is a choice between this machine's save and the cloud's.
    /// With no local save there is no "keep mine" to offer, so the record
    /// cannot be resolved by anything the player does -- and the two heals
    /// in [`Inner::upload_slot`] never reach it, because both run on the way
    /// to an upload and a career with no save on disk never queues one. The
    /// Online hub went on saying "<career> is waiting for you to choose
    /// which copy to keep" for a career that was gone from the cloud AND
    /// from the computer, with no way to clear it (Shane, 2026-09-20).
    ///
    /// Forgetting rather than hiding matters: a stale row left in
    /// `cloud_saves.json` would attach itself to the next career started
    /// under the same name, and block ITS backups against a revision that
    /// was never its own.
    fn forget_conflicts_with_no_local_save(&self) {
        // Strip the known suffixes rather than taking file_stem(): a career
        // named "Run 1.9" would otherwise read back as "Run 1" and its live
        // conflict would be forgotten as stale.
        let on_disk: std::collections::BTreeSet<String> = Profile::list_saves()
            .iter()
            .filter_map(|path| path.file_name().map(|n| n.to_string_lossy().to_string()))
            .filter_map(|name| {
                name.strip_suffix(SAVE_SUFFIX)
                    .or_else(|| name.strip_suffix(LEGACY_SAVE_SUFFIX))
                    .map(str::to_string)
            })
            .collect();
        for (name, entry) in self.sync_state.slots() {
            if slot_conflict(&entry).is_none() || on_disk.contains(&name) {
                continue;
            }
            log::info!(
                "Cloud sync state for {name}: a conflict was waiting for a career this \
computer no longer has; forgetting it so the slot starts clean"
            );
            self.sync_state.forget(&name);
        }
    }

    /// One line per known slot at startup: the kept session logs only go
    /// back two sessions, so a stall whose conflict was recorded earlier
    /// would otherwise leave no trace in the log a tester shares.
    fn log_sync_state(&self) {
        let slots = self.sync_state.slots();
        if slots.is_empty() {
            log::info!("Cloud sync state: no careers have synced from this computer yet");
            return;
        }
        for (name, entry) in slots {
            let synced = match json_int(entry.get("revision")) {
                Some(revision) => format!("last synced revision {revision}"),
                None => "no revision synced yet".to_string(),
            };
            match slot_conflict(&entry) {
                None => log::info!("Cloud sync state for {name}: {synced}"),
                Some(conflict) => log::info!(
                    "Cloud sync state for {name}: {synced}; a conflict against cloud \
revision {} is waiting in the Cloud backup menu",
                    conflict
                        .get("latestRevision")
                        .map(py_str)
                        .unwrap_or_else(|| "None".to_string())
                ),
            }
        }
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
            self.pump(false);
            wait_seconds(&self.wake, self.worker_wait());
            self.wake.clear();
        }
    }

    fn worker_wait(&self) -> f64 {
        let now = (self.clock)();
        let st = self.state.lock().unwrap();
        if st.pending.is_empty() {
            return WORKER_TICK_S;
        }
        let oldest = st
            .pending
            .values()
            .map(|p| p.queued_at)
            .fold(f64::INFINITY, f64::min);
        if let Some(retry_at) = st.retry_at {
            return (retry_at - now).max(0.05);
        }
        (self.debounce - (now - oldest)).max(0.05)
    }

    fn pump(&self, force: bool) {
        if !self.enabled() || self.identity.lock().unwrap().is_none() {
            return;
        }
        let now = (self.clock)();
        let due: Vec<(String, Pending)> = {
            let st = self.state.lock().unwrap();
            if !force && st.retry_at.is_some_and(|at| now < at) {
                return;
            }
            st.pending
                .iter()
                .filter(|(_, p)| force || now - p.queued_at >= self.debounce)
                .map(|(name, p)| (name.clone(), p.clone()))
                .collect()
        };
        for (name, pending) in due {
            if self.stop.is_set() && !force {
                return;
            }
            self.upload_slot(&name, pending);
        }
    }

    /// Drop a handled snapshot -- unless a newer save replaced it while
    /// the upload was in flight, which must stay queued.
    fn done_with(&self, name: &str, snapshot: &Arc<Value>) {
        let mut st = self.state.lock().unwrap();
        if let Some(current) = st.pending.get(name) {
            if Arc::ptr_eq(&current.snapshot, snapshot) {
                st.pending.remove(name);
            }
        }
    }

    /// Record a terminal upload result under the attempt token its
    /// snapshot was queued with (0 for background saves, which no poller
    /// ever matches). Uploads run outside the lock, so an upload already in
    /// flight when a newer manual attempt starts finishes carrying its own
    /// older token: it must neither answer for the newer attempt nor
    /// overwrite the newer attempt's recorded result.
    fn note_outcome(&self, name: &str, token: i64, outcome: &str) {
        let mut st = self.state.lock().unwrap();
        let keep = match st.outcomes.get(name) {
            Some((current, _)) => token >= *current,
            None => true,
        };
        if keep {
            st.outcomes
                .insert(name.to_string(), (token, outcome.to_string()));
        }
    }

    /// Record a terminal upload refusal, and queue its spoken line when
    /// the background queue owes one.
    ///
    /// The token gates the speaking, never the bookkeeping: every terminal
    /// refusal records its cause, manual or background, so whichever
    /// channel told the player first -- this one, or the Save game watch
    /// (states/city.py) that owns token > 0 attempts -- the player hears
    /// each standing cause exactly once. Only a background attempt
    /// (token 0) appends the line; a manual refusal is the menu's to
    /// speak. A recorded cause stays silent until it changes or the slot
    /// uploads successfully (see announce_recovery). The auth cause
    /// dedupes machine-wide under AUTH_ANNOUNCED_KEY: a paused sign-in
    /// belongs to this computer, not to whichever career saved first.
    /// Transient network failures never arrive here -- they retry
    /// silently.
    fn announce_refusal(&self, name: &str, token: i64, cause: &str, message: &str) {
        let key = if cause == "auth" {
            AUTH_ANNOUNCED_KEY
        } else {
            name
        };
        let mut st = self.state.lock().unwrap();
        if st.announced_causes.get(key).map(String::as_str) == Some(cause) {
            return;
        }
        st.announced_causes
            .insert(key.to_string(), cause.to_string());
        if token == 0 {
            st.announcements.push(message.to_string());
        }
    }

    /// An upload was accepted: say so, and clear any recorded refusal so
    /// later trouble speaks afresh.
    ///
    /// Success used to be silent unless it followed an announced refusal,
    /// which left the ordinary case with nothing to hear at all. A save at a
    /// rest stop backs up in the background, and a driver who cannot see the
    /// status line got the same nothing whether the career reached the server
    /// or never left the machine -- silence reading as failure is the whole
    /// complaint behind the refusal lines above, and it applied just as much
    /// to the path that worked (owner, 2026-08-15).
    ///
    /// Same principle as announce_refusal: the token gates the speaking,
    /// never the bookkeeping. Only a background save (token 0) speaks here --
    /// a manual save's outcome is already spoken by the Save game watch
    /// (states/city.py `_backup_outcome_text`), and a second line for the
    /// same event would say it twice.
    ///
    /// `uploaded` marks a real accepted upload, which also proves this
    /// computer's sign-in works, so it re-arms the machine-wide auth
    /// announcement -- silently, since the auth line named no career and
    /// reconnecting speaks its own confirmation. The "unchanged" path never
    /// contacts the server: it leaves the auth record alone, and it stays
    /// silent unless it is clearing a refusal, because nothing was sent and
    /// claiming a fresh backup would be untrue.
    fn announce_success(&self, name: &str, token: i64, uploaded: bool) {
        let mut st = self.state.lock().unwrap();
        if uploaded {
            st.announced_causes.remove(AUTH_ANNOUNCED_KEY);
        }
        let recovered = st.announced_causes.remove(name).is_some();
        if token != 0 {
            return;
        }
        if recovered {
            // Answers a refusal the driver heard: never thinned by the
            // cadence setting.
            st.announcements.push(recovery_status(name));
        } else if uploaded {
            let first_this_session = st.all_clear_spoken.insert(name.to_string());
            let speak = match st.backup_announcements {
                BackupAnnouncements::Every => true,
                BackupAnnouncements::Once => first_this_session,
                BackupAnnouncements::Off => false,
            };
            if speak {
                st.announcements.push(backup_status(name));
            }
        }
    }

    fn announce_eviction(&self, outcome: &str, token: i64) {
        if token != 0 {
            return;
        }
        let Some(name) = outcome.strip_prefix("accepted:evicted:") else {
            return;
        };
        self.state
            .lock()
            .unwrap()
            .announcements
            .push(eviction_status(name));
    }

    fn note_absolved(&self, name: &str, result: &Map<String, Value>) {
        if truthy(result.get("clearIntegrityFlag")) {
            self.state.lock().unwrap().absolved.push(name.to_string());
        }
    }

    fn set_status(&self, message: &str) {
        self.state.lock().unwrap().status = message.to_string();
    }

    fn upload_slot(&self, name: &str, pending: Pending) {
        let _transfer = self.transfer.read().unwrap();
        if !self
            .state
            .lock()
            .unwrap()
            .pending
            .get(name)
            .is_some_and(|current| Arc::ptr_eq(&current.snapshot, &pending.snapshot))
        {
            return;
        }
        let Pending {
            snapshot,
            meaningful_play,
            token,
            ..
        } = pending;
        let mut slot = self.sync_state.slot(name);
        let (_, content_hash) = cloud_content(&snapshot);
        match slot_conflict(&slot) {
            Some(conflict) if json_int(conflict.get("latestRevision")).is_none() => {
                // Recorded by an older build against an empty cloud slot (wiped
                // deployment, or deleted from another machine). No newer save
                // exists to protect, so start the slot over instead of staying
                // silent forever.
                self.sync_state.forget(name);
                slot = Map::new();
            }
            Some(conflict) => {
                // A known conflict names a real cloud revision -- but that copy
                // may have vanished since it was recorded (deployment reset, or
                // the slot deleted from another machine), and then there is
                // nothing left to protect. Re-check before staying silent.
                if self.cloud_slot_exists(name) {
                    // Still there: the player resolves it from the Cloud backup
                    // menu. Drop the snapshot -- the local file is still the
                    // source of truth for "keep mine".
                    self.done_with(name, &snapshot);
                    let outcome = manual::backup_conflict_outcome(
                        &slot,
                        &content_hash,
                        json_int(conflict.get("latestRevision")),
                        token,
                    );
                    self.note_outcome(name, token, outcome);
                    let message = if outcome == "stale" {
                        STALE_BACKUP_STATUS.to_string()
                    } else {
                        conflict_status(name)
                    };
                    self.set_status(&message);
                    self.announce_refusal(name, token, outcome, &message);
                    return;
                }
                log::info!(
                    "Cloud backup of {name} was blocked by a conflict whose cloud \
copy no longer exists; restarting the slot fresh"
                );
                self.sync_state.forget(name);
                slot = Map::new();
            }
            None => {}
        }
        if slot.get("hash").and_then(Value::as_str) == Some(content_hash.as_str()) {
            if token != 0 {
                if self.finish_cached_manual_backup(
                    name,
                    &snapshot,
                    token,
                    &content_hash,
                    &mut slot,
                ) {
                    return;
                }
            } else {
                self.done_with(name, &snapshot);
                self.note_outcome(name, token, "unchanged");
                self.announce_success(name, token, false);
                return;
            }
        }
        let identity = match self.identity.lock().unwrap().clone() {
            Some(id) => id,
            None => {
                self.finish_auth_backup(name, &snapshot, token);
                return;
            }
        };
        let result = upload_save(
            &identity,
            name,
            &snapshot,
            json_int(slot.get("revision")),
            &backup_summary(&snapshot),
            meaningful_play.as_ref(),
            self.transport.as_ref(),
        );
        if truthy(result.get("ok")) {
            let revision = json_int(result.get("revision")).unwrap_or(0);
            self.sync_state.record_synced_for_driver(
                name,
                revision,
                result
                    .get("contentHash")
                    .and_then(Value::as_str)
                    .unwrap_or(""),
                &identity.driver_id,
            );
            self.done_with(name, &snapshot);
            self.note_absolved(name, &result);
            if let Some(stamp) = &meaningful_play {
                self.meaningful_play
                    .clear_if_accepted(name, &stamp.operation_id);
            }
            self.state.lock().unwrap().retry_at = None;
            self.set_status("Latest backup accepted and server-verified.");
            let outcome = accepted_outcome(&result);
            self.note_outcome(name, token, &outcome);
            self.announce_success(name, token, true);
            self.announce_eviction(&outcome, token);
            log::info!("Cloud backup of {name} uploaded as revision {revision}");
            return;
        }
        if reason_of(&result) == Some("conflict") {
            if json_int(result.get("latestRevision")).is_none() {
                // The cloud slot is empty -- the staging deployment was wiped,
                // or the slot was deleted from another machine -- so there is
                // no newer save to protect. Drop the stale revision and let the
                // retry pass re-create the slot from this machine's save.
                self.sync_state.forget(name);
                self.state.lock().unwrap().retry_at = Some((self.clock)() + self.retry);
                self.note_outcome(name, token, "network");
                log::info!(
                    "Cloud backup of {name} named a revision the cloud no longer \
has; restarting the slot fresh"
                );
                return;
            }
            self.sync_state.record_conflict(name, &result);
            self.done_with(name, &snapshot);
            self.note_outcome(name, token, "conflict");
            self.set_status(&conflict_status(name));
            self.announce_refusal(name, token, "conflict", &conflict_status(name));
            log::warn!(
                "Cloud backup of {name} skipped: the cloud copy is newer (revision {})",
                result
                    .get("latestRevision")
                    .map(py_str)
                    .unwrap_or_else(|| "None".to_string())
            );
            return;
        }
        let reason = reason_of(&result).map(str::to_string);
        let family = classify_upload_failure(reason.as_deref());
        if family == "auth" {
            // The credentials were retired (usually by connecting another
            // computer); every retry would fail identically, and the player
            // can only fix it by reconnecting.
            self.set_status(AUTH_PAUSED_STATUS);
            self.done_with(name, &snapshot);
            self.note_outcome(name, token, "auth");
            self.announce_refusal(name, token, "auth", AUTH_PAUSED_STATUS);
            return;
        }
        if family == "rejected" {
            // Not transient: retrying with the same inputs cannot succeed.
            // The raw reason code is logged for review but never spoken --
            // only the honest, career-named story below is.
            let reason_text = reason.clone().unwrap_or_default();
            log::warn!("Cloud backup of {name} was rejected: {reason_text}");
            let status = rejection_status(name, reason.as_deref());
            self.set_status(&status);
            self.done_with(name, &snapshot);
            let cause = format!("rejected:{reason_text}");
            self.note_outcome(name, token, &cause);
            self.announce_refusal(name, token, &cause, &status);
            return;
        }
        // Transient (network, 5xx): keep the snapshot, back off.
        self.state.lock().unwrap().retry_at = Some((self.clock)() + self.retry);
        self.note_outcome(name, token, "network");
    }

    /// Whether the cloud still holds any revision of this slot. Errs on
    /// the side of `true`: an unreachable or refusing server must keep the
    /// conflict guard in place.
    fn cloud_slot_exists(&self, name: &str) -> bool {
        let identity = match self.identity.lock().unwrap().clone() {
            Some(id) => id,
            None => return true,
        };
        match list_saves(&identity, self.transport.as_ref()) {
            Err(CloudAuthError) => true,
            Ok(None) => true,
            Ok(Some(list)) => list
                .saves
                .iter()
                .any(|entry| entry.get("saveName").and_then(Value::as_str) == Some(name)),
        }
    }
}
