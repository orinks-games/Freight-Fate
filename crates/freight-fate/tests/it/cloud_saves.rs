//! Port of `tests/test_cloud_saves.py` (the service and API tests; the
//! canonical-JSON and signature tests live in `ff_core::cloud_save_integrity`)
//! plus the pure parts of `tests/test_cloud_public_career.py`.
//!
//! These cover what must hold whether or not orinks.net is reachable: the
//! off-by-default path, the signature-stripped portable content form, debounce
//! and no-change skipping, the parent-revision conflict guard, and restores
//! (which must verify the server signature before anything touches disk). A
//! fake transport and an injected clock keep every test deterministic and
//! free of real sockets.
//!
//! `Profile` is not part of this crate, so profiles are the JSON dicts
//! `Profile.to_dict()` would produce, and the server signatures over them
//! were produced once by the Python reference (`cryptography`, the same
//! `[7u8; 32]` test key `ff_core`'s own tests use) and pinned here.

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use base64::Engine;
use serde_json::{json, Map, Value};

use ff_core::models::profile::{set_save_listener, Profile};
use freight_fate::cloud_saves::{
    backup_status, backup_summary, classify_upload_failure, cloud_content, cloud_slot_name,
    conflict_status, delete_save, download_save, list_saves, local_name_for_slot,
    profile_dict_from_content, recovery_status, rejection_status, restore_to_disk, save_slot_name,
    set_public_save, upload_save, url_quote, BackupAnnouncements, CloudAuthError, CloudSaves,
    CloudSavesOptions, DownloadError, PublicKeys, RestoreError, RestoreHooks, SavesList, SyncState,
    AUTH_HELP, AUTH_PAUSED_STATUS, DEBOUNCE_S, RETRY_INTERVAL_S,
};
use freight_fate::meaningful_play::MeaningfulPlayReason;
use freight_fate::net::testing::{ClosureTransport, FakeTransport, ManualClock};
use freight_fate::net::{NetError, SharedTransport, Transport};
use freight_fate::online_presence::{base_url, IdentityStore, MemoryStore, OnlineIdentity};

// -- the app-shell rig for the menu tests further down ----------------------------
use freight_fate::app::testing::TestApp;
use freight_fate::app::{share, GameContext, SharedState};
use freight_fate::states::base::{InputEvent, Key, Menu, State};
use freight_fate::states::city::{CityMenuState, BACKUP_RESULT_WAIT_S};
use freight_fate::states::cloud_save_states::{
    install_restored_profile, CloudBackupConsentState, CloudBackupState, CloudSlotState,
    ConfirmDeleteCloudState, CLOUD_DISCLOSURE, LEGACY_BACKUP_NOTICE,
};
use freight_fate::states::online_hub::OnlineHubState;
use freight_fate::states::online_states::set_identity_store_override;

mod account_api;
mod announcements;
mod carryover;
mod integrity;
mod manual_backup;
mod manual_freshness;
mod menus;
mod resume;
mod service;

const TEST_KEY_ID: &str = "test-key";
const TEST_PUBLIC_KEY_HEX: &str =
    "ea4a6c63e29c520abef5507b132ec5f9954776aebebe7b92421eea691446d22c";

fn identity() -> OnlineIdentity {
    OnlineIdentity::new("driver-testtest", &"t".repeat(48))
}

fn test_keys() -> PublicKeys {
    let mut keys = BTreeMap::new();
    keys.insert(
        TEST_KEY_ID.to_string(),
        hex::decode(TEST_PUBLIC_KEY_HEX).unwrap(),
    );
    keys
}

/// The server's answer to a retired driver token (see issue 64: a new
/// token issued to a second computer retires the one stored here).
fn auth_error(code: u16, error: Option<&str>) -> NetError {
    match error {
        Some(e) => NetError::http_json(code, &json!({"error": e})),
        None => NetError::http_json(code, &json!({})),
    }
}

fn conflict_error(latest_revision: Option<i64>) -> NetError {
    NetError::http_json(
        409,
        &json!({
            "error": "conflict",
            "latestRevision": latest_revision,
            "latestCreatedAt": 1_700_000_000_000i64,
            "latestSummary": "Rig Hauler, level 9, 88,000 dollars",
        }),
    )
}

/// The server's answer to a save the validator flatly refuses -- not a
/// connection problem, and not transient: the same input will fail again.
fn rejected_error(reason: &str) -> NetError {
    NetError::http_json(400, &json!({"error": reason}))
}

/// `Profile(name=...)`.to_dict() in miniature: the fields the service and
/// the summary read, plus a local signature the cloud form must strip.
fn profile(name: &str, money: f64) -> Value {
    json!({
        "name": name,
        "money": money,
        "version": 7,
        "career": {"xp": 0.0},
        "_signature": "local-hmac",
        "_signature_version": 1,
    })
}

struct Service {
    service: CloudSaves,
    _dir: tempfile::TempDir,
}

impl std::ops::Deref for Service {
    type Target = CloudSaves;
    fn deref(&self) -> &CloudSaves {
        &self.service
    }
}

/// A synchronous (non-threaded) service wired to a fake transport and a
/// fresh data directory (the `isolated_data_dir` fixture).
fn make_service_with(
    transport: SharedTransport,
    clock: &Arc<ManualClock>,
    enabled: bool,
    identity: Option<OnlineIdentity>,
) -> Service {
    let dir = tempfile::tempdir().unwrap();
    let service = CloudSaves::new(CloudSavesOptions {
        enabled,
        identity,
        clock: clock.clock(),
        transport,
        threaded: false,
        data_dir: dir.path().to_path_buf(),
        ..CloudSavesOptions::default()
    });
    Service { service, _dir: dir }
}

fn make_service(transport: &Arc<FakeTransport>, clock: &Arc<ManualClock>) -> Service {
    make_service_with(transport.clone(), clock, true, Some(identity()))
}

/// Let the debounce pass and pump once, as the worker would.
fn drain(service: &CloudSaves, clock: &Arc<ManualClock>) {
    clock.advance(DEBOUNCE_S + 0.1);
    service.pump(false);
}

fn revision_of(service: &CloudSaves, name: &str) -> Option<i64> {
    service
        .sync_state()
        .slot(&cloud_slot_name(name))
        .get("revision")?
        .as_i64()
}

fn conflict_map(latest_revision: Option<i64>) -> Map<String, Value> {
    let mut map = Map::new();
    map.insert("latestRevision".to_string(), json!(latest_revision));
    map
}

/// Serves the slot-list GET and the upload POST from one fake.
struct RoutedTransport {
    list_reply: Value,
    list_error: Option<NetError>,
    upload_reply: Value,
    requests: Mutex<Vec<(String, Option<Value>)>>,
}

impl RoutedTransport {
    fn new(list_reply: Value, list_error: Option<NetError>) -> Arc<Self> {
        Arc::new(Self {
            list_reply,
            list_error,
            upload_reply: json!({"ok": true, "revision": 1}),
            requests: Mutex::new(Vec::new()),
        })
    }

    fn posts(&self) -> Vec<Value> {
        self.requests
            .lock()
            .unwrap()
            .iter()
            .filter_map(|(_, p)| p.clone())
            .collect()
    }
}

impl Transport for RoutedTransport {
    fn call(
        &self,
        url: &str,
        payload: Option<&Value>,
        _headers: &[(String, String)],
        _method: Option<&str>,
    ) -> Result<Value, NetError> {
        self.requests
            .lock()
            .unwrap()
            .push((url.to_string(), payload.cloned()));
        if payload.is_none() {
            // the list fetch
            if let Some(e) = &self.list_error {
                return Err(e.clone());
            }
            return Ok(self.list_reply.clone());
        }
        Ok(self.upload_reply.clone())
    }
}

// -- the save-listener hook -------------------------------------------------------

/// A temp save directory and `listener` installed for the duration of
/// `body`, so a real `Profile::save()` writes somewhere disposable
/// (Python's `tmp_path` fixture, which `conftest.py` applied to every test).
///
/// Both the directory and the hook belong to the calling THREAD, so these
/// cases need no lock and cannot take each other's listener however many of
/// them run at once.
fn with_save_listener<T>(
    listener: impl Fn(&Profile) + Send + Sync + 'static,
    body: impl FnOnce() -> T,
) -> T {
    let tmp = tempfile::tempdir().expect("a temp dir");
    let _guard = freight_fate::app::testing::DataDirGuard::pin(tmp.path().join("data"));
    set_save_listener(Some(Arc::new(listener)));
    let result = body();
    set_save_listener(None);
    result
}

// -- download and restore ----------------------------------------------------------

/// `make_cloud_reply(profile_dict)`: the server's content reply for a dict
/// whose signature (over `canonical_profile(portable)`) is pinned from the
/// Python reference.
fn make_cloud_reply(profile_dict: &Value, sig_b64: &str, revision: i64) -> Value {
    let (content, content_hash) = cloud_content(profile_dict);
    json!({
        "ok": true,
        "saveName": save_slot_name(profile_dict.get("name").and_then(Value::as_str).unwrap_or("Driver")),
        "revision": revision,
        "saveVersion": profile_dict.get("version").cloned().unwrap_or(json!(0)),
        "contentHash": content_hash,
        "sizeBytes": content.len(),
        "summary": backup_summary(profile_dict),
        "createdAt": 1_700_000_000_000i64,
        "content": base64::engine::general_purpose::STANDARD.encode(&content),
        "sig": sig_b64,
        "keyId": TEST_KEY_ID,
        "signedAt": "2026-07-13T12:00:00.000Z",
        "validatorVersion": 1,
    })
}

fn basic_profile() -> (Value, &'static str) {
    (
        json!({"name": "Road Star", "version": 7}),
        "Ulu+7My0ZjhwAEvRGMvtQ6c7yBmoU8yLnd8ZFZvf6ZmRmHAm28Xz0a9hdoOg4t+Me+CpQH8Z1+dHTuUDMA3/Bg==",
    )
}

fn money77_profile() -> (Value, &'static str) {
    (
        json!({"name": "Road Star", "money": 77000.0, "version": 7}),
        "vYzgfLZuMvkkJo/rZb46w4dpDBCnWBskW3F+AKdpRJunDdBTrfu6ZB9al+yyw/RIQHZW1vMXj9bMqlWyJfOtBw==",
    )
}

fn marked_profile() -> (Value, &'static str) {
    (
        json!({"name": "Road Star", "money": 77000.0, "version": 7, "integrity_modified": true, "integrity_notice_pending": true}),
        "Ev0L12qBi1IjAcK1zsh/0LcYdtga2E3/ScSC1CoRyr/8TQBVxaxqkCixqNSuzHF6yMAvJ3uURoGk2vO1n6KKBA==",
    )
}

fn legacy_profile() -> (Value, &'static str) {
    (
        json!({"name": "Road Star", "money": 77000.0, "version": 5}),
        "i8B4Im44clNWW+rPXStu9RQMO5fOFTSH1bs5JReEBfwdOrQb3HIguw/98K0ZSl+MJPoFvXSTRoDBcBxV4SjdCw==",
    )
}

fn download(reply: Value) -> Result<Option<Value>, DownloadError> {
    download_save(
        &identity(),
        "Road Star",
        None,
        FakeTransport::replying(reply).as_ref(),
        Some(&test_keys()),
    )
}

/// A stand-in for the profile-side writer: installs the dict as JSON at
/// `dir/<slot>.ffsave`, keeping the old file as `.ffsave.bak`, and records
/// what it was asked to write.
struct DiskWriter {
    dir: PathBuf,
    written: Mutex<Vec<Value>>,
}

impl DiskWriter {
    fn new(dir: &std::path::Path) -> Self {
        Self {
            dir: dir.to_path_buf(),
            written: Mutex::new(Vec::new()),
        }
    }

    fn path(&self, profile: &Value) -> PathBuf {
        let name = profile
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("Driver");
        self.dir.join(format!("{}.ffsave", save_slot_name(name)))
    }

    fn write(&self, profile: &Value) -> Result<PathBuf, String> {
        self.written.lock().unwrap().push(profile.clone());
        let path = self.path(profile);
        let tmp = path.with_extension("ffsave.tmp");
        let backup = path.with_extension("ffsave.bak");
        fs::write(&tmp, profile.to_string()).map_err(|e| e.to_string())?;
        if path.exists() {
            let _ = fs::remove_file(&backup);
            fs::rename(&path, &backup).map_err(|e| e.to_string())?;
        }
        fs::rename(&tmp, &path).map_err(|e| e.to_string())?;
        Ok(path)
    }
}

/// `restore_to_disk` through a `DiskWriter`, with the test signing keys.
fn restore(
    writer: &DiskWriter,
    payload: &Value,
    sync_state: Option<&SyncState>,
) -> Result<PathBuf, RestoreError> {
    let write = |profile: &Value| writer.write(profile);
    let hooks = RestoreHooks {
        is_legacy: &is_pre_1_9_save,
        write: &write,
    };
    restore_to_disk(payload, sync_state, &hooks, Some(&test_keys()))
}

/// `is_pre_1_9_save`: a save version from the 1.8 line with no created-on
/// marker.
fn is_pre_1_9_save(profile: &Value) -> bool {
    let version = profile.get("version").and_then(Value::as_i64).unwrap_or(0);
    version <= 5 && profile.get("created_line").is_none()
}

// -- settings menu (app shell) ----------------------------------------------------

/// `IDENTITY.save()`: the test identity in a memory-backed identity store the
/// menus read through `OnlineIdentity.load()`. Cleared on drop.
struct SavedIdentity;

impl Drop for SavedIdentity {
    fn drop(&mut self) {
        set_identity_store_override(None);
    }
}

fn save_identity(app: &TestApp) -> SavedIdentity {
    let store = IdentityStore::new(
        &app.data_dir.path().join("identity"),
        Some(MemoryStore::new()),
    );
    store.save(&identity()).unwrap();
    set_identity_store_override(Some(Arc::new(store)));
    // The app's services were built before the identity existed; the setup
    // flow is what would have handed it to them.
    app.ctx.services.cloud.set_identity(Some(identity()));
    SavedIdentity
}

/// `app.cloud = make_service(...)`: a cloud service over `transport`, driven
/// inline, carrying the test identity.
fn install_cloud(app: &mut TestApp, transport: SharedTransport, enabled: bool) -> CloudSaves {
    let service = CloudSaves::new(CloudSavesOptions {
        enabled,
        identity: Some(identity()),
        transport,
        threaded: false,
        data_dir: ff_core::models::profile::data_dir(),
        ..CloudSavesOptions::default()
    });
    app.ctx.services.cloud = service.clone();
    service
}

fn push<S: State + 'static>(app: &mut TestApp, state: S) -> SharedState {
    let shared = share(state);
    app.push_shared(shared.clone());
    shared
}

fn with_state<T: State + 'static, R>(shared: &SharedState, f: impl FnOnce(&mut T) -> R) -> R {
    let mut state = shared.borrow_mut();
    f(state.as_any_mut().downcast_mut::<T>().expect("state type"))
}

fn menu_labels<T: Menu + State>(shared: &SharedState, ctx: &GameContext) -> Vec<String> {
    let state = shared.borrow();
    let typed = state.as_any().downcast_ref::<T>().expect("state type");
    typed
        .menu()
        .items
        .iter()
        .map(|item| item.text(typed, ctx))
        .collect()
}

fn current_label<T: Menu + State>(shared: &SharedState, ctx: &GameContext) -> String {
    let state = shared.borrow();
    let typed = state.as_any().downcast_ref::<T>().expect("state type");
    let core = typed.menu();
    core.items[core.index].text(typed, ctx)
}

fn press(app: &mut TestApp, key: Key) {
    app.dispatch_to_state(&InputEvent::key(key));
}

fn move_to<T: Menu + State>(app: &mut TestApp, shared: &SharedState, prefix: &str) {
    for _ in 0..32 {
        if current_label::<T>(shared, &app.ctx).starts_with(prefix) {
            return;
        }
        press(app, Key::Down);
    }
    panic!("no row starting with {prefix:?}");
}

/// `open_online_settings`: the Online hub (pushed directly here; the Settings
/// picker that points at it belongs to the main-menu port).
fn open_online_settings(app: &mut TestApp) -> SharedState {
    let hub = OnlineHubState::new(&mut app.ctx);
    push(app, hub)
}

// -- rejected-upload status split (Shane's report, 2026-08-14: with more than -----
// -- one career backed up he could not tell which one was refused, or why) --------

fn status_after_rejection(name: &str, reason: &str) -> String {
    let transport = FakeTransport::failing(rejected_error(reason));
    let clock = ManualClock::new();
    let service = make_service(&transport, &clock);
    service.queue_backup(name, profile(name, 5000.0));
    drain(&service, &clock);
    service.status()
}

// -- how often the all-clear speaks (MariahL, 2026-09-02: "you hear a lot that --
// -- your character is backed up ... maybe only say it every so often")

fn make_service_announcing(
    transport: &Arc<FakeTransport>,
    clock: &Arc<ManualClock>,
    mode: BackupAnnouncements,
) -> Service {
    let dir = tempfile::tempdir().unwrap();
    let service = CloudSaves::new(CloudSavesOptions {
        enabled: true,
        identity: Some(identity()),
        clock: clock.clock(),
        transport: transport.clone(),
        threaded: false,
        data_dir: dir.path().to_path_buf(),
        backup_announcements: mode,
        ..CloudSavesOptions::default()
    });
    Service { service, _dir: dir }
}

// -- the Save game item at the terminal speaks the backup result ------------------

/// `make_terminal_menu(app, service)`: the terminal menu over an injected
/// cloud service. The state is not entered -- `enter()` starts music and warms
/// live weather, and `save` needs neither.
fn last_line(app: &TestApp) -> String {
    app.main_lines().last().cloned().unwrap_or_default()
}

fn make_terminal_menu(app: &mut TestApp, service: CloudSaves) -> CityMenuState {
    app.ctx.services.cloud = service;
    app.ctx.profile = Some(ff_core::models::profile::Profile::named_in(
        "Road Star",
        "Chicago",
    ));
    CityMenuState::new(&app.ctx, false)
}
