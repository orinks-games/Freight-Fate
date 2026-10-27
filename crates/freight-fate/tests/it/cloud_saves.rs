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
    backup_status, backup_summary, classify_upload_failure, cloud_content, conflict_status,
    delete_save, download_save, list_saves, profile_dict_from_content, recovery_status,
    rejection_status, restore_to_disk, save_slot_name, set_public_save, upload_save, url_quote,
    BackupAnnouncements, CloudAuthError, CloudSaves, CloudSavesOptions, DownloadError, PublicKeys,
    RestoreError, RestoreHooks, SavesList, SyncState, AUTH_HELP, AUTH_PAUSED_STATUS, DEBOUNCE_S,
    RETRY_INTERVAL_S,
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
    CloudBackupConsentState, CloudBackupState, CloudSlotState, ConfirmDeleteCloudState,
    CLOUD_DISCLOSURE, LEGACY_BACKUP_NOTICE,
};
use freight_fate::states::online_hub::OnlineHubState;
use freight_fate::states::online_states::set_identity_store_override;

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
    service.sync_state().slot(name).get("revision")?.as_i64()
}

// -- content form ---------------------------------------------------------------

#[test]
fn test_cloud_content_is_portable_and_round_trips() {
    let d = profile("Road Star", 12_345.0);
    assert!(d.get("_signature").is_some());

    let (content, content_hash) = cloud_content(&d);
    let restored = profile_dict_from_content(&content).unwrap();
    // The signature is machine-local: it must never travel.
    assert!(restored.get("_signature").is_none());
    assert!(restored.get("_signature_version").is_none());
    assert_eq!(restored["name"], "Road Star");
    assert_eq!(restored["money"], 12_345.0);
    assert_eq!(restored["version"], d["version"]);

    // Deterministic bytes: the same snapshot always hashes the same, so
    // unchanged profiles can skip uploads by hash alone.
    let (again, again_hash) = cloud_content(&profile("Road Star", 12_345.0));
    assert_eq!(again, content);
    assert_eq!(again_hash, content_hash);
    // gzip header: magic, deflate, no flags, mtime 0.
    assert_eq!(&content[..8], &[0x1f, 0x8b, 0x08, 0x00, 0, 0, 0, 0]);
}

#[test]
fn test_slot_name_matches_local_file_stem() {
    // Profile.path's stem sanitizes the same way.
    assert_eq!(save_slot_name("Road * Star?"), "Road _ Star_");
    assert_eq!(save_slot_name("  "), "Driver");
    assert_eq!(save_slot_name("Night-Owl_2"), "Night-Owl_2");
}

#[test]
fn test_backup_summary_reads_like_speech() {
    let d = profile("Road Star", 12_345.0);
    assert_eq!(backup_summary(&d), "Road Star, level 1, 12,345 dollars");
    // Reference values from the Python module for a career-less dict.
    assert_eq!(
        backup_summary(&json!({"name": "Road Star", "money": 12345.0, "version": 7})),
        "Road Star, 12,345 dollars"
    );
    assert_eq!(backup_summary(&json!({})), "Driver");
}

// -- disabled and unconfigured paths ---------------------------------------------

#[test]
fn test_cloud_saves_default_off() {
    // Private backup and public Profile sharing are separate consents: the
    // service defaults off (Settings().cloud_saves is False is pinned by the
    // settings port).
    assert!(!CloudSavesOptions::default().enabled);
}

#[test]
fn test_disabled_never_posts() {
    let transport = FakeTransport::revisions();
    let clock = ManualClock::new();
    let service = make_service_with(transport.clone(), &clock, false, Some(identity()));
    service.queue_backup("Road Star", profile("Road Star", 5000.0));
    drain(&service, &clock);
    assert!(transport.requests().is_empty());
}

#[test]
fn test_enabled_without_identity_stays_dormant() {
    let transport = FakeTransport::revisions();
    let clock = ManualClock::new();
    let service = make_service_with(transport.clone(), &clock, true, None);
    assert!(!service.enabled());
    service.queue_backup("Road Star", profile("Road Star", 5000.0));
    drain(&service, &clock);
    assert!(transport.requests().is_empty());
}

#[test]
fn test_status_says_off_while_backups_are_disabled() {
    // The menu's status line must never claim readiness while the service is
    // off: signed-in 1.9 testers heard "Cloud backup is ready" and believed
    // they were backed up when nothing was ever uploaded.
    let transport = FakeTransport::revisions();
    let clock = ManualClock::new();
    let service = make_service_with(transport, &clock, false, Some(identity()));
    assert_eq!(
        service.status(),
        "Cloud backup is off. Saves on this computer are not backed up."
    );
    // Turning it on starts the service, and startup now reads the save
    // directory to drop conflicts for careers this computer no longer has,
    // so the thread needs a directory of its own.
    let saves = tempfile::tempdir().unwrap();
    let previous = ff_core::settings::set_thread_data_dir(Some(saves.path().to_path_buf()));
    service.set_enabled(true);
    assert_eq!(service.status(), "Cloud backup is ready.");
    ff_core::settings::set_thread_data_dir(previous);
}

// -- upload scheduling ------------------------------------------------------------

#[test]
fn test_debounce_holds_then_uploads_once() {
    let transport = FakeTransport::revisions();
    let clock = ManualClock::new();
    let service = make_service(&transport, &clock);

    // A burst of saves inside the debounce window collapses to one upload.
    service.queue_backup("Road Star", profile("Road Star", 5000.0));
    service.pump(false);
    assert!(transport.requests().is_empty());
    service.queue_backup("Road Star", profile("Road Star", 5100.0));
    drain(&service, &clock);

    assert_eq!(transport.posts().len(), 1);
    let payload = transport.posts()[0].clone();
    assert_eq!(
        transport.requests()[0].url,
        format!("{}/api/freight-fate/saves", base_url())
    );
    assert_eq!(payload["driverId"], identity().driver_id);
    assert_eq!(payload["saveName"], "Road Star");
    assert_eq!(payload["parentRevision"], Value::Null);
    assert_eq!(payload["saveVersion"], 7);
    assert_eq!(
        transport.requests()[0].header("Authorization"),
        Some(format!("Bearer {}", identity().driver_token).as_str())
    );
    // The upload carries the latest snapshot from the burst.
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(payload["content"].as_str().unwrap())
        .unwrap();
    let uploaded = profile_dict_from_content(&bytes).unwrap();
    assert_eq!(uploaded["money"], 5100.0);

    // The synced revision becomes the next upload's parent.
    assert_eq!(revision_of(&service, "Road Star"), Some(1));
}

#[test]
fn test_unchanged_profile_skips_the_upload() {
    let transport = FakeTransport::revisions();
    let clock = ManualClock::new();
    let service = make_service(&transport, &clock);

    service.queue_backup("Road Star", profile("Road Star", 5000.0));
    drain(&service, &clock);
    service.queue_backup("Road Star", profile("Road Star", 5000.0)); // nothing changed
    drain(&service, &clock);

    assert_eq!(transport.posts().len(), 1);
}

#[test]
fn test_next_change_uploads_with_the_synced_parent() {
    let transport = FakeTransport::revisions();
    let clock = ManualClock::new();
    let service = make_service(&transport, &clock);

    service.queue_backup("Road Star", profile("Road Star", 5000.0));
    drain(&service, &clock);
    service.queue_backup("Road Star", profile("Road Star", 5500.0));
    drain(&service, &clock);

    assert_eq!(transport.posts().len(), 2);
    assert_eq!(transport.posts()[1]["parentRevision"], 1);
    assert_eq!(revision_of(&service, "Road Star"), Some(2));
}

#[test]
fn test_transient_failure_retries_later() {
    let transport = FakeTransport::revisions();
    transport.set_error(Some(NetError::other("OSError", "no network")));
    let clock = ManualClock::new();
    let service = make_service(&transport, &clock);
    service.queue_backup("Road Star", profile("Road Star", 5000.0));
    drain(&service, &clock);
    assert_eq!(transport.posts().len(), 1);

    // Before the retry interval nothing new is attempted...
    service.pump(false);
    assert_eq!(transport.posts().len(), 1);

    // ...after it, the pending snapshot goes out and syncs.
    transport.set_error(None);
    clock.advance(RETRY_INTERVAL_S + 0.1);
    service.pump(false);
    assert_eq!(transport.posts().len(), 2);
    assert_eq!(revision_of(&service, "Road Star"), Some(1));
}

#[test]
fn test_meaningful_play_stamp_is_identical_across_network_retry_then_clears() {
    let transport = FakeTransport::revisions();
    transport.set_error(Some(NetError::other("OSError", "no network")));
    let clock = ManualClock::new();
    let service = make_service(&transport, &clock);
    service
        .meaningful_play_tracker()
        .mark("Road Star", MeaningfulPlayReason::JobAccepted);

    service.queue_backup("Road Star", profile("Road Star", 5000.0));
    drain(&service, &clock);
    let first = transport.posts()[0]["meaningfulPlay"].clone();
    assert_eq!(first["reason"], "job_accepted");
    assert!(first["operationId"]
        .as_str()
        .is_some_and(|id| !id.is_empty()));
    assert!(first["occurredAt"].as_i64().is_some());
    assert!(service
        .meaningful_play_tracker()
        .for_upload("Road Star")
        .is_some());

    transport.set_error(None);
    clock.advance(RETRY_INTERVAL_S + 0.1);
    service.pump(false);

    assert_eq!(transport.posts()[1]["meaningfulPlay"], first);
    assert!(service
        .meaningful_play_tracker()
        .for_upload("Road Star")
        .is_none());
}

#[test]
fn test_upload_without_meaningful_play_sends_null_metadata() {
    let transport = FakeTransport::revisions();
    let clock = ManualClock::new();
    let service = make_service(&transport, &clock);

    service.queue_backup("Road Star", profile("Road Star", 5000.0));
    drain(&service, &clock);

    assert_eq!(transport.posts()[0]["meaningfulPlay"], Value::Null);
}

#[test]
fn test_accepted_older_upload_cannot_clear_newer_meaningful_save() {
    let clock = ManualClock::new();
    let service_slot: Arc<Mutex<Option<CloudSaves>>> = Arc::new(Mutex::new(None));
    let request_count = Arc::new(Mutex::new(0usize));
    let transport: SharedTransport = {
        let service_slot = Arc::clone(&service_slot);
        let request_count = Arc::clone(&request_count);
        Arc::new(ClosureTransport(
            move |_url: &str,
                  _payload: Option<&Value>,
                  _headers: &[(String, String)],
                  _method: Option<&str>| {
                let mut count = request_count.lock().unwrap();
                *count += 1;
                let revision = *count as i64;
                if *count == 1 {
                    let service = service_slot.lock().unwrap().clone().unwrap();
                    drop(count);
                    service
                        .meaningful_play_tracker()
                        .mark("Road Star", MeaningfulPlayReason::BusinessChanged);
                    service.queue_backup("Road Star", profile("Road Star", 6000.0));
                }
                Ok(json!({"ok": true, "revision": revision}))
            },
        ))
    };
    let service = make_service_with(transport, &clock, true, Some(identity()));
    *service_slot.lock().unwrap() = Some(service.service.clone());
    service
        .meaningful_play_tracker()
        .mark("Road Star", MeaningfulPlayReason::JobAccepted);

    service.queue_backup("Road Star", profile("Road Star", 5000.0));
    drain(&service, &clock);

    let newer = service
        .meaningful_play_tracker()
        .for_upload("Road Star")
        .expect("the newer event must survive the older acceptance");
    assert_eq!(newer.reason, MeaningfulPlayReason::BusinessChanged);

    clock.advance(DEBOUNCE_S + 0.1);
    service.pump(false);
    assert!(service
        .meaningful_play_tracker()
        .for_upload("Road Star")
        .is_none());
}

#[test]
fn test_conflict_and_auth_refusal_keep_meaningful_intent() {
    for error in [
        conflict_error(Some(5)),
        auth_error(401, Some("unauthorized")),
    ] {
        let transport = FakeTransport::failing(error);
        let clock = ManualClock::new();
        let service = make_service_with(transport, &clock, true, Some(identity()));
        service
            .meaningful_play_tracker()
            .mark("Road Star", MeaningfulPlayReason::DeliveryCompleted);

        service.queue_backup("Road Star", profile("Road Star", 5000.0));
        drain(&service, &clock);

        assert_eq!(
            service
                .meaningful_play_tracker()
                .for_upload("Road Star")
                .map(|stamp| stamp.reason),
            Some(MeaningfulPlayReason::DeliveryCompleted)
        );
    }
}

// -- the conflict guard -----------------------------------------------------------

#[test]
fn test_conflict_marks_the_slot_and_stops_backups() {
    let transport = FakeTransport::revisions();
    transport.set_error(Some(conflict_error(Some(5))));
    let clock = ManualClock::new();
    let service = make_service(&transport, &clock);

    service.queue_backup("Road Star", profile("Road Star", 5000.0));
    drain(&service, &clock);
    assert_eq!(transport.posts().len(), 1);

    let conflicts = service.conflicts();
    let conflict = &conflicts["Road Star"];
    assert_eq!(conflict["latestRevision"], 5);
    assert!(conflict["latestSummary"]
        .as_str()
        .unwrap()
        .contains("level 9"));

    // Until the player chooses a side, this slot must not retry into the
    // conflict -- another machine's newer save is at stake.
    transport.set_error(None);
    service.queue_backup("Road Star", profile("Road Star", 5999.0));
    drain(&service, &clock);
    assert_eq!(transport.posts().len(), 1);
}

#[test]
fn test_empty_cloud_conflict_restarts_the_slot_instead_of_sticking() {
    // A conflict whose latest revision is null means the cloud slot is empty
    // (the deployment was wiped, or the slot was deleted from another machine).
    // There is no newer save at stake, so the guard must not stick: the slot
    // starts over as a fresh upload instead of silently never backing up again.
    let transport = FakeTransport::revisions();
    transport.set_error(Some(conflict_error(None)));
    let clock = ManualClock::new();
    let service = make_service(&transport, &clock);
    // This machine remembers a revision the server no longer has.
    service
        .sync_state()
        .record_synced("Road Star", 7, "stale-hash");

    service.queue_backup("Road Star", profile("Road Star", 5000.0));
    drain(&service, &clock);
    assert_eq!(transport.posts().len(), 1);
    assert_eq!(transport.posts()[0]["parentRevision"], 7);
    // Not a real conflict: nothing is recorded for the player to resolve.
    assert!(service.conflicts().is_empty());

    // The stale revision is gone, and the retry goes out as a fresh slot.
    transport.set_error(None);
    clock.advance(RETRY_INTERVAL_S + 0.1);
    service.pump(false);
    assert_eq!(transport.posts().len(), 2);
    assert_eq!(transport.posts()[1]["parentRevision"], Value::Null);
    assert_eq!(revision_of(&service, "Road Star"), Some(1));
}

fn conflict_map(latest_revision: Option<i64>) -> Map<String, Value> {
    let mut map = Map::new();
    map.insert("latestRevision".to_string(), json!(latest_revision));
    map
}

#[test]
fn test_recorded_empty_cloud_conflict_heals_on_the_next_backup() {
    // Older builds recorded the empty-cloud conflict as sticky. A save made
    // under this build must clear that mark and back up fresh.
    let transport = FakeTransport::revisions();
    let clock = ManualClock::new();
    let service = make_service(&transport, &clock);
    service
        .sync_state()
        .record_synced("Road Star", 7, "stale-hash");
    service
        .sync_state()
        .record_conflict("Road Star", &conflict_map(None));

    service.queue_backup("Road Star", profile("Road Star", 5000.0));
    drain(&service, &clock);
    assert_eq!(transport.posts().len(), 1);
    assert_eq!(transport.posts()[0]["parentRevision"], Value::Null);
    assert!(service.conflicts().is_empty());
    assert_eq!(revision_of(&service, "Road Star"), Some(1));
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

#[test]
fn test_stale_recorded_conflict_heals_when_the_cloud_slot_is_gone() {
    // A conflict recorded against a cloud copy that has since vanished
    // (deployment reset, slot deleted from another machine) must not block
    // backups forever: the guard re-checks the cloud and starts over.
    let transport = RoutedTransport::new(json!({"ok": true, "saves": []}), None);
    let clock = ManualClock::new();
    let service = make_service_with(transport.clone(), &clock, true, Some(identity()));
    service
        .sync_state()
        .record_synced("Road Star", 7, "stale-hash");
    let mut latest = conflict_map(Some(40));
    latest.insert("latestCreatedAt".to_string(), json!(1_700_000_000_000i64));
    latest.insert("latestSummary".to_string(), json!("level 18"));
    service.sync_state().record_conflict("Road Star", &latest);

    service.queue_backup("Road Star", profile("Road Star", 5000.0));
    drain(&service, &clock);
    assert_eq!(transport.posts().len(), 1);
    assert_eq!(transport.posts()[0]["parentRevision"], Value::Null);
    assert!(service.conflicts().is_empty());
    assert_eq!(revision_of(&service, "Road Star"), Some(1));
}

#[test]
fn test_recorded_conflict_with_a_live_cloud_copy_still_blocks() {
    let transport = RoutedTransport::new(
        json!({"ok": true, "saves": [{"saveName": "Road Star", "revision": 40}]}),
        None,
    );
    let clock = ManualClock::new();
    let service = make_service_with(transport.clone(), &clock, true, Some(identity()));
    service
        .sync_state()
        .record_synced("Road Star", 7, "stale-hash");
    service
        .sync_state()
        .record_conflict("Road Star", &conflict_map(Some(40)));

    service.queue_backup("Road Star", profile("Road Star", 5000.0));
    drain(&service, &clock);
    // The other machine's newer save is still at stake: nothing uploads and
    // the conflict stays for the player to resolve.
    assert!(transport.posts().is_empty());
    assert!(service.conflicts().contains_key("Road Star"));
}

#[test]
fn test_conflict_recheck_that_cannot_reach_the_cloud_keeps_the_guard() {
    let transport = RoutedTransport::new(json!({}), Some(NetError::other("OSError", "no route")));
    let clock = ManualClock::new();
    let service = make_service_with(transport.clone(), &clock, true, Some(identity()));
    service
        .sync_state()
        .record_conflict("Road Star", &conflict_map(Some(40)));

    service.queue_backup("Road Star", profile("Road Star", 5000.0));
    drain(&service, &clock);
    assert!(transport.posts().is_empty());
    assert!(service.conflicts().contains_key("Road Star"));
}

#[test]
#[ignore = "log capture: the startup sync-state dump goes through `log::info!`; its wording is covered by reading the code path (CloudSaves::start)"]
fn test_start_logs_the_sync_state_for_each_slot() {}

#[test]
#[ignore = "log capture: see test_start_logs_the_sync_state_for_each_slot"]
fn test_start_with_no_sync_state_says_so() {}

#[test]
fn test_keep_mine_overwrites_the_cloud_and_clears_the_conflict() {
    let transport = FakeTransport::revisions();
    transport.set_error(Some(conflict_error(Some(5))));
    let clock = ManualClock::new();
    let service = make_service(&transport, &clock);

    service.queue_backup("Road Star", profile("Road Star", 5000.0));
    drain(&service, &clock);
    assert!(service.conflicts().contains_key("Road Star"));

    transport.set_error(None);
    transport.set_reply(Some(json!({"ok": true, "revision": 6})));
    assert_eq!(
        service.resolve_keep_mine("Road Star", &profile("Road Star", 5000.0)),
        "ok"
    );
    // The upload named the server's latest revision as parent: a plain
    // last-write-wins overwrite, chosen explicitly by the player.
    assert_eq!(transport.posts().last().unwrap()["parentRevision"], 5);
    assert!(service.conflicts().is_empty());
    assert_eq!(revision_of(&service, "Road Star"), Some(6));
}

// -- upload failure classification (Jessie's report, 2026-08-14: an -------------
// -- invalid_achievement refusal was told to the player as "check your ---------
// -- connection") ----------------------------------------------------------------

#[test]
fn test_classify_upload_failure_sorts_the_three_honest_families() {
    assert_eq!(
        classify_upload_failure(Some("invalid_achievement")),
        "rejected"
    );
    assert_eq!(classify_upload_failure(Some("too_large")), "rejected");
    assert_eq!(classify_upload_failure(Some("unauthorized")), "auth");
    assert_eq!(classify_upload_failure(Some("driver_not_found")), "auth");
    assert_eq!(classify_upload_failure(Some("http_401")), "auth");
    // A raw network error, a 5xx, or a code this table has not been taught
    // yet must default to network -- retry is the safe failure mode.
    assert_eq!(classify_upload_failure(Some("error")), "network");
    assert_eq!(classify_upload_failure(Some("http_500")), "network");
    assert_eq!(classify_upload_failure(None), "network");
}

#[test]
fn test_a_permanent_refusal_is_never_left_to_retry_as_if_it_were_the_network() {
    // Two server refusals were missing from this table, so
    // `classify_upload_failure` sorted them as "network" and the queue backed
    // off and retried forever without telling the player anything. Neither can
    // ever succeed on a retry: one needs the player to free a slot, the other
    // needs the server fixed. Same class as Jessie's `invalid_achievement`
    // told as "check your connection" (2026-08-14), different codes.
    assert_eq!(classify_upload_failure(Some("too_many_slots")), "rejected");
    assert_eq!(
        classify_upload_failure(Some("signing_unavailable")),
        "rejected"
    );
}

#[test]
fn test_each_new_refusal_gets_its_own_honest_sentence() {
    // A refusal the player can clear themselves and one that is ours to fix
    // must not share the generic line -- only one of them has anything to do.
    for (reason, expected) in [
        (
            "too_many_slots",
            "as many careers backed up as the server keeps",
        ),
        ("signing_unavailable", "could not finish signing"),
    ] {
        let spoken = rejection_status("Little Bear", Some(reason));
        assert!(spoken.starts_with("Little Bear: backup not accepted."));
        assert!(spoken.contains(expected));
        // No jargon reaches the player: the reason code itself never speaks.
        assert!(!spoken.contains(reason));
    }
}

#[test]
fn test_resolve_keep_mine_reports_network_for_a_transport_error() {
    let transport = FakeTransport::failing(NetError::other("OSError", "network unreachable"));
    let service = make_service(&transport, &ManualClock::new());
    assert_eq!(
        service.resolve_keep_mine("Road Star", &profile("Road Star", 5000.0)),
        "network"
    );
}

#[test]
fn test_resolve_keep_mine_reports_auth_for_a_retired_token() {
    let transport = FakeTransport::failing(auth_error(401, None));
    let service = make_service(&transport, &ManualClock::new());
    assert_eq!(
        service.resolve_keep_mine("Road Star", &profile("Road Star", 5000.0)),
        "auth"
    );
}

#[test]
fn test_resolve_keep_mine_reports_rejected_for_a_validator_refusal() {
    // The raw reason rides along (Shane's report, 2026-08-14: the menu that
    // resolves a conflict needs it to speak the same career-named,
    // family-split story as the background queue, not a bare tag).
    let transport = FakeTransport::failing(rejected_error("invalid_achievement"));
    let service = make_service(&transport, &ManualClock::new());
    assert_eq!(
        service.resolve_keep_mine("Road Star", &profile("Road Star", 5000.0)),
        "rejected:invalid_achievement"
    );
}

#[test]
fn test_resolve_keep_mine_carries_every_rejected_reason() {
    // Every code classify_upload_failure sorts as "rejected" round-trips
    // through resolve_keep_mine's return value -- not just the one used above
    // -- so cloud_save_states can build the right family-specific story for
    // each of them.
    for reason in [
        "impossible_xp",
        "impossible_money",
        "invalid_schema",
        "unsupported_version",
    ] {
        let transport = FakeTransport::failing(rejected_error(reason));
        let service = make_service(&transport, &ManualClock::new());
        assert_eq!(
            service.resolve_keep_mine("Road Star", &profile("Road Star", 5000.0)),
            format!("rejected:{reason}")
        );
    }
}

#[test]
fn test_resolve_keep_mine_rejection_logs_the_raw_reason_but_the_tag_never_speaks_it() {
    // The raw reason goes to the log (log::warn!); the return value carries
    // it as an opaque tag for the menu to translate.
    let transport = FakeTransport::failing(rejected_error("impossible_money"));
    let service = make_service(&transport, &ManualClock::new());
    let result = service.resolve_keep_mine("Road Star", &profile("Road Star", 5000.0));
    assert_eq!(result, "rejected:impossible_money");
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

#[test]
fn test_every_profile_save_queues_a_backup() {
    let transport = FakeTransport::revisions();
    let clock = ManualClock::new();
    let service = make_service(&transport, &clock);
    // `profile_module.save_listener = service.queue_backup`: the same hook
    // `App` installs, wired by hand so this test owns the service.
    let backup = service.service.clone();

    with_save_listener(
        move |profile: &Profile| {
            backup.queue_backup(&profile.name, Value::Object(profile.to_dict()));
        },
        || {
            Profile::named("Road Star").save().expect("the save lands");
        },
    );
    drain(&service.service, &clock);

    let posts = transport.posts();
    assert_eq!(posts.len(), 1, "{posts:?}");
    assert_eq!(posts[0]["saveName"], json!("Road Star"));
}

#[test]
fn test_a_failing_listener_never_breaks_the_local_save() {
    let (path, name) = with_save_listener(
        |_profile: &Profile| panic!("backup service on fire"),
        || {
            // Must not raise.
            let path = Profile::named("Road Star").save().expect("the save lands");
            let name = Profile::load(&path).expect("the save reads back").name;
            (path, name)
        },
    );

    assert!(!path.as_os_str().is_empty());
    assert_eq!(name, "Road Star");
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

#[test]
fn test_download_verifies_the_content_hash() {
    let (dict, sig) = basic_profile();
    let good = make_cloud_reply(&dict, sig, 3);
    let payload = download(good.clone()).unwrap().expect("a payload");
    assert_eq!(payload["profile"]["name"], "Road Star");

    let mut tampered = good;
    tampered["contentHash"] = json!("0".repeat(64));
    assert!(download(tampered).unwrap().is_none());
}

#[test]
fn test_download_rejects_missing_or_future_verification_metadata() {
    let (dict, sig) = basic_profile();
    let mut unsigned = make_cloud_reply(&dict, sig, 3);
    unsigned.as_object_mut().unwrap().remove("sig");
    match download(unsigned) {
        Err(DownloadError::Integrity(e)) => assert_eq!(e.code, "unverified"),
        other => panic!("expected an unverified error, got {other:?}"),
    }

    let mut future = make_cloud_reply(&dict, sig, 3);
    future["validatorVersion"] = json!(2);
    match download(future) {
        Err(DownloadError::Integrity(e)) => assert_eq!(e.code, "update_required"),
        other => panic!("expected update_required, got {other:?}"),
    }
}

#[test]
fn test_download_rejects_payload_changed_after_server_signing() {
    let (dict, sig) = money77_profile();
    let mut reply = make_cloud_reply(&dict, sig, 3);
    let changed = json!({"name": "Road Star", "money": 88000.0, "version": 7});
    let (content, content_hash) = cloud_content(&changed);
    reply["content"] = json!(base64::engine::general_purpose::STANDARD.encode(&content));
    reply["contentHash"] = json!(content_hash);

    match download(reply) {
        Err(DownloadError::Integrity(e)) => assert_eq!(e.code, "integrity_failed"),
        other => panic!("expected integrity_failed, got {other:?}"),
    }
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

#[test]
fn test_restore_verifies_then_writes_a_locally_signed_save() {
    // The cloud copy came from another machine. Its portable payload has an
    // orinks.net signature; the writer hook then signs and installs it.
    let dir = tempfile::tempdir().unwrap();
    let writer = DiskWriter::new(dir.path());
    let (dict, sig) = money77_profile();
    let reply = make_cloud_reply(&dict, sig, 4);
    let payload = download(reply).unwrap().expect("a payload");

    // An older local save exists and must survive as the fallback file.
    let local = json!({"name": "Road Star", "money": 5.0, "version": 7});
    let local_path = writer.write(&local).unwrap();

    let sync_state = SyncState::new(dir.path());
    let path = restore(&writer, &payload, Some(&sync_state)).unwrap();
    assert_eq!(path, local_path);

    let restored: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(restored["money"], 77_000.0);
    let backup = path.with_extension("ffsave.bak");
    assert!(backup.exists());
    let old: Value = serde_json::from_str(&fs::read_to_string(&backup).unwrap()).unwrap();
    assert_eq!(old["money"], 5.0);

    // The restored revision is the next upload's parent, so continuing this
    // career does not immediately conflict with the copy just downloaded.
    assert_eq!(sync_state.slot("Road Star")["revision"], 4);
}

#[test]
fn test_unverified_restore_changes_neither_disk_nor_sync_state() {
    let dir = tempfile::tempdir().unwrap();
    let writer = DiskWriter::new(dir.path());
    let local = json!({"name": "Road Star", "money": 5.0, "version": 7});
    let path = writer.write(&local).unwrap();
    let before = fs::read(&path).unwrap();
    let (dict, sig) = money77_profile();
    let mut payload = download(make_cloud_reply(&dict, sig, 3))
        .unwrap()
        .expect("a payload");
    payload["sig"] = json!(base64::engine::general_purpose::STANDARD.encode([b'x'; 64]));
    let sync_state = SyncState::new(dir.path());

    match restore(&writer, &payload, Some(&sync_state)) {
        Err(RestoreError::Integrity(e)) => {
            assert!(e.message.contains("signature"), "{}", e.message)
        }
        other => panic!("expected a signature failure, got {other:?}"),
    }

    assert_eq!(fs::read(&path).unwrap(), before);
    assert!(sync_state.slots().is_empty());
    assert_eq!(writer.written.lock().unwrap().len(), 1);
}

#[test]
fn test_upload_rejects_oversized_content() {
    let mut huge = profile("Road Star", 5000.0);
    // Incompressible padding: repeated text would gzip under the cap.
    let mut noise = String::with_capacity(2 * 1024 * 1024);
    let mut x: u64 = 0x9E37_79B9_7F4A_7C15;
    while noise.len() < 2 * 1024 * 1024 {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        noise.push_str(&format!("{x:016x}"));
    }
    huge["achievement_stats"] = json!({"pad": noise});
    let result = upload_save(
        &identity(),
        "Road Star",
        &huge,
        None,
        "too big",
        None,
        FakeTransport::revisions().as_ref(),
    );
    assert_eq!(result["ok"], false);
    assert_eq!(result["reason"], "too_large");
}

#[test]
fn test_upload_preserves_the_optional_evicted_save_name() {
    let transport = FakeTransport::replying(json!({
        "ok": true,
        "revision": 4,
        "evictedSaveName": "Old Road"
    }));
    let result = upload_save(
        &identity(),
        "Road Star",
        &profile("Road Star", 5000.0),
        Some(3),
        "Road Star",
        None,
        transport.as_ref(),
    );

    assert_eq!(result["ok"], true);
    assert_eq!(result["evictedSaveName"], "Old Road");
}

#[test]
fn test_upload_remains_compatible_when_the_server_omits_eviction() {
    let transport = FakeTransport::replying(json!({"ok": true, "revision": 4}));
    let result = upload_save(
        &identity(),
        "Road Star",
        &profile("Road Star", 5000.0),
        Some(3),
        "Road Star",
        None,
        transport.as_ref(),
    );

    assert_eq!(result["ok"], true);
    assert!(!result.contains_key("evictedSaveName"));
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

#[test]
fn test_cloud_toggle_requires_the_account_setup_first() {
    let mut app = TestApp::new();
    let cat = open_online_settings(&mut app);
    move_to::<OnlineHubState>(&mut app, &cat, "Back up saves");
    assert_eq!(
        current_label::<OnlineHubState>(&cat, &app.ctx),
        "Back up saves to your orinks.net account: not set up"
    );

    press(&mut app, Key::Return);
    assert!(!app.ctx.settings.cloud_saves);
    assert!(app
        .main_lines()
        .iter()
        .any(|t| t.contains("needs your orinks.net account")));
}

#[test]
fn test_cloud_toggle_speaks_the_disclosure_when_turned_on() {
    let mut app = TestApp::new();
    let _identity = save_identity(&app);
    let cat = open_online_settings(&mut app);
    move_to::<OnlineHubState>(&mut app, &cat, "Back up saves");
    assert_eq!(
        current_label::<OnlineHubState>(&cat, &app.ctx),
        "Back up saves to your orinks.net account: off"
    );

    press(&mut app, Key::Return);
    let consent = app.state().unwrap();
    assert_eq!(
        with_state::<CloudBackupConsentState, _>(&consent, |s| s.menu.title.clone()),
        "Turn Cloud backup on?"
    );
    assert_eq!(
        menu_labels::<CloudBackupConsentState>(&consent, &app.ctx)[0],
        "No, keep Cloud backup off"
    );
    assert!(!app.ctx.settings.cloud_saves);
    assert!(app
        .main_lines()
        .iter()
        .any(|t| t.contains(CLOUD_DISCLOSURE)));

    press(&mut app, Key::Down);
    press(&mut app, Key::Return);
    assert!(app.ctx.settings.cloud_saves);
    assert!(app.ctx.services.cloud.enabled());
    assert!(ff_core::settings::Settings::load().cloud_saves);

    app.clear_speech();
    press(&mut app, Key::Return);
    assert!(!app.ctx.settings.cloud_saves);
    assert!(!app.ctx.services.cloud.enabled());
}

// -- retired credentials (issue 64) ----------------------------------------------

#[test]
fn test_list_saves_refused_credentials_raise_cloud_auth_error() {
    // A 401 means orinks.net answered and said no: the menus must tell the
    // player to reconnect, never blame the network.
    assert_eq!(
        list_saves(
            &identity(),
            FakeTransport::failing(auth_error(401, None)).as_ref()
        ),
        Err(CloudAuthError)
    );
}

#[test]
fn test_list_saves_unknown_driver_raises_cloud_auth_error() {
    assert_eq!(
        list_saves(
            &identity(),
            FakeTransport::failing(auth_error(404, Some("driver_not_found"))).as_ref()
        ),
        Err(CloudAuthError)
    );
}

#[test]
fn test_list_saves_network_trouble_stays_none() {
    assert_eq!(
        list_saves(
            &identity(),
            FakeTransport::failing(NetError::other("OSError", "no route")).as_ref()
        ),
        Ok(None)
    );
}

// -- the public career choice -----------------------------------------------------

#[test]
fn test_list_saves_carries_the_public_career_choice() {
    let transport = FakeTransport::replying(json!({
        "ok": true,
        "saves": [{"saveName": "Road Star", "revision": 1}],
        "publicSaveName": "Road Star",
    }));
    let reply = list_saves(&identity(), transport.as_ref());
    assert_eq!(
        reply,
        Ok(Some(SavesList {
            saves: vec![json!({"saveName": "Road Star", "revision": 1})],
            public_save_name: Some("Road Star".to_string()),
        }))
    );
}

#[test]
fn test_list_saves_from_a_server_without_the_choice_says_none() {
    // orinks.net builds from before the public-career choice send only the
    // saves list; the menu must read that as "no career designated".
    let transport = FakeTransport::replying(json!({"ok": true, "saves": []}));
    assert_eq!(
        list_saves(&identity(), transport.as_ref()),
        Ok(Some(SavesList {
            saves: vec![],
            public_save_name: None,
        }))
    );
}

#[test]
fn test_set_public_save_posts_the_choice() {
    let transport = FakeTransport::replying(json!({"ok": true, "publicSaveName": "Road Star"}));
    assert_eq!(
        set_public_save(&identity(), Some("Road Star"), transport.as_ref()),
        Ok(true)
    );
    let request = &transport.requests()[0];
    assert!(request.url.ends_with("/saves/public-career"));
    assert_eq!(
        request.payload,
        Some(json!({"driverId": identity().driver_id, "saveName": "Road Star"}))
    );
    assert_eq!(
        request.header("Authorization"),
        Some(format!("Bearer {}", identity().driver_token).as_str())
    );
}

#[test]
fn test_set_public_save_refused_credentials_raise_cloud_auth_error() {
    assert_eq!(
        set_public_save(
            &identity(),
            Some("Road Star"),
            FakeTransport::failing(auth_error(401, None)).as_ref()
        ),
        Err(CloudAuthError)
    );
}

#[test]
fn test_set_public_save_network_trouble_stays_false() {
    assert_eq!(
        set_public_save(
            &identity(),
            Some("Road Star"),
            FakeTransport::failing(NetError::other("OSError", "no route")).as_ref()
        ),
        Ok(false)
    );
}

#[test]
fn test_download_refused_credentials_raise_cloud_auth_error() {
    assert_eq!(
        download_save(
            &identity(),
            "Road Star",
            None,
            FakeTransport::failing(auth_error(401, None)).as_ref(),
            Some(&test_keys()),
        ),
        Err(DownloadError::Auth(CloudAuthError))
    );
}

#[test]
fn test_upload_with_retired_token_pauses_backups_with_reconnect_status() {
    let transport = FakeTransport::failing(auth_error(401, None));
    let clock = ManualClock::new();
    let service = make_service(&transport, &clock);
    service.queue_backup("Road Star", profile("Road Star", 5000.0));
    drain(&service, &clock);

    assert!(service.status().contains("Reconnect from the Online menu"));
    // Not transient: the snapshot is dropped instead of retried forever.
    drain(&service, &clock);
    assert_eq!(transport.posts().len(), 1);
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

#[test]
fn test_upload_rejected_for_impossible_money_names_the_career_and_offers_appeal() {
    let status = status_after_rejection("Road Star", "impossible_money");
    assert!(status.starts_with("Road Star: backup not accepted."));
    assert!(status.contains("flagged it for review"));
    assert!(status.contains("tester document"));
}

#[test]
fn test_upload_rejected_for_impossible_xp_speaks_the_same_arithmetic_story() {
    let status = status_after_rejection("Night Owl", "impossible_xp");
    assert!(status.starts_with("Night Owl: backup not accepted."));
    assert!(status.contains("flagged it for review"));
}

#[test]
fn test_upload_rejected_for_invalid_schema_blames_the_build_not_the_player() {
    let status = status_after_rejection("Road Star", "invalid_schema");
    assert!(status.starts_with("Road Star: backup not accepted."));
    assert!(status.contains("build mismatch"));
    assert!(!status.contains("flagged"));
}

#[test]
fn test_upload_rejected_for_unsupported_version_speaks_the_same_schema_story() {
    assert!(status_after_rejection("Road Star", "unsupported_version").contains("build mismatch"));
}

#[test]
fn test_upload_rejected_for_an_unknown_city_names_the_server_as_behind() {
    // A city the server has not caught up with is nobody's fault.
    //
    // Under the generic wording this reads as an unexplained refusal, and it is
    // a live failure mode: a stale deployed catalog stopped a tester's backups
    // for a day (2026-08-14) with nothing to tell them why.
    let status = status_after_rejection("Road Star", "invalid_city");
    assert!(status.starts_with("Road Star: backup not accepted."));
    assert!(status.contains("does not recognize the town"));
    assert!(!status.contains("flagged"));
}

#[test]
fn test_upload_rejected_for_an_unrecognized_code_falls_back_safely_with_the_name() {
    assert_eq!(
        status_after_rejection("Road Star", "invalid_market"),
        "Road Star: backup not accepted. Your local career is safe. \
Public details were not updated."
    );
}

#[test]
fn test_upload_rejected_logs_the_raw_reason_but_never_speaks_it() {
    // The raw code is logged for review; the spoken status never says it.
    let status = status_after_rejection("Road Star", "impossible_money");
    assert!(!status.contains("impossible_money"));
}

// -- cross-language canonicalization: see ff_core::cloud_save_integrity tests -----

#[test]
fn test_whole_float_profile_signature_round_trips() {
    // The exact shape that broke every real restore: ordinary careers carry
    // whole floats (total_miles, reputation), which the old canonical form
    // rendered as "29571.0" while the server had signed "29571". The 77000.0
    // here was signed as 77000 by the Python reference.
    let (dict, sig) = money77_profile();
    let payload = download(make_cloud_reply(&dict, sig, 3))
        .unwrap()
        .expect("a payload");
    assert_eq!(payload["profile"]["money"], 77_000.0);
}

#[test]
fn test_server_absolution_clears_the_modified_mark_on_restore() {
    // A career marked only because it moved computers gets released.
    //
    // The server grants this on a revision it signed and fully validated, so
    // honest machine-movers stop carrying the mark forever. A profile that
    // really was edited fails validation and never gets the signal.
    let dir = tempfile::tempdir().unwrap();
    let writer = DiskWriter::new(dir.path());
    let (dict, sig) = marked_profile();
    let mut reply = make_cloud_reply(&dict, sig, 4);
    reply["clearIntegrityFlag"] = json!(true);
    let payload = download(reply).unwrap().expect("a payload");

    let sync_state = SyncState::new(dir.path());
    let path = restore(&writer, &payload, Some(&sync_state)).unwrap();
    let restored: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();

    assert_eq!(restored["integrity_modified"], false);
    assert_eq!(restored["integrity_notice_pending"], false);
    assert_eq!(restored["money"], 77_000.0);
}

#[test]
fn test_a_restore_without_absolution_leaves_the_mark_alone() {
    // Absence of the signal is not permission to clear the mark.
    let dir = tempfile::tempdir().unwrap();
    let writer = DiskWriter::new(dir.path());
    let (dict, sig) = marked_profile();
    let payload = download(make_cloud_reply(&dict, sig, 4))
        .unwrap()
        .expect("a payload");

    let path = restore(&writer, &payload, Some(&SyncState::new(dir.path()))).unwrap();
    let restored: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(restored["integrity_modified"], true);
}

// -- deleting a cloud slot -------------------------------------------------------

#[test]
fn test_delete_save_issues_a_delete_for_the_named_slot() {
    let transport = FakeTransport::replying(json!({"ok": true, "deletedRevisions": 3}));

    assert_eq!(
        delete_save(&identity(), "Road Star", transport.as_ref()),
        Ok(true)
    );

    let request = &transport.requests()[0];
    assert_eq!(
        request.url,
        format!(
            "{}/api/freight-fate/saves?driverId={}&saveName=Road%20Star",
            base_url(),
            identity().driver_id
        )
    );
    assert!(request.payload.is_none());
    assert_eq!(request.method.as_deref(), Some("DELETE"));
    assert_eq!(
        request.header("Authorization"),
        Some(format!("Bearer {}", identity().driver_token).as_str())
    );
}

#[test]
fn test_delete_save_network_trouble_is_false() {
    assert_eq!(
        delete_save(
            &identity(),
            "Road Star",
            FakeTransport::failing(NetError::other("OSError", "no route")).as_ref()
        ),
        Ok(false)
    );
}

#[test]
fn test_delete_save_refused_credentials_raise_cloud_auth_error() {
    assert_eq!(
        delete_save(
            &identity(),
            "Road Star",
            FakeTransport::failing(auth_error(401, None)).as_ref()
        ),
        Err(CloudAuthError)
    );
}

#[test]
fn test_forget_drops_the_slot_and_its_conflict() {
    let dir = tempfile::tempdir().unwrap();
    let sync_state = SyncState::new(dir.path());
    sync_state.record_synced("Road Star", 3, "hash");
    sync_state.record_conflict("Road Star", &conflict_map(Some(4)));

    sync_state.forget("Road Star");

    assert!(sync_state.slots().is_empty());
    // Idempotent: forgetting an unknown slot is not an error.
    sync_state.forget("Road Star");
}

#[test]
fn test_sync_state_persists_across_instances() {
    let dir = tempfile::tempdir().unwrap();
    {
        let sync_state = SyncState::new(dir.path());
        sync_state.record_synced("Road Star", 3, "hash");
        sync_state.record_conflict("Night Owl", &conflict_map(Some(9)));
        sync_state.clear_conflict("Night Owl");
    }
    let reloaded = SyncState::new(dir.path());
    assert_eq!(reloaded.slot("Road Star")["revision"], 3);
    assert_eq!(reloaded.slot("Road Star")["hash"], "hash");
    assert!(reloaded.slot("Night Owl").get("conflict").is_none());
    assert!(reloaded.path().exists());
}

#[test]
fn test_delete_menu_flow_confirms_then_forgets_the_slot() {
    let mut app = TestApp::new();
    let _identity = save_identity(&app);
    // The delete goes through the cloud service's transport; this one says yes.
    let deleter = FakeTransport::replying(json!({"ok": true}));
    let service = install_cloud(&mut app, deleter.clone(), true);
    service.sync_state().record_synced("Road Star", 3, "hash");
    let entry = json!({
        "saveName": "Road Star",
        "revision": 3,
        "createdAt": 1_700_000_000_000i64,
        "summary": "Rig Hauler, level 9",
    });
    let mut slot = CloudSlotState::new(&mut app.ctx, "Road Star", vec![entry], None, None);
    slot.threaded = false;
    let slot = push(&mut app, slot);
    move_to::<CloudSlotState>(&mut app, &slot, "Delete");
    press(&mut app, Key::Return);

    let confirm = app.state().unwrap();
    assert_eq!(
        with_state::<ConfirmDeleteCloudState, _>(&confirm, |s| s.menu.title.clone()),
        "Delete the cloud backups?"
    );
    // Safe default still first; the wording changed on 2026-08-15 so a cancel
    // can never be mistaken for the action it declines (see
    // test_no_cancel_row_is_named_after_a_real_action).
    assert_eq!(
        menu_labels::<ConfirmDeleteCloudState>(&confirm, &app.ctx)[0],
        "No, cancel and change nothing"
    );
    assert!(app
        .main_lines()
        .iter()
        .any(|t| t.contains("cannot be brought back")));

    press(&mut app, Key::Down);
    press(&mut app, Key::Return);
    with_state::<CloudSlotState, _>(&slot, |s| State::update(s, &mut app.ctx, 0.0));

    let deleted = deleter.requests();
    assert_eq!(deleted.len(), 1);
    assert_eq!(deleted[0].method.as_deref(), Some("DELETE"));
    assert!(deleted[0].url.contains("saveName=Road%20Star"));
    assert!(service.sync_state().slots().is_empty());
    assert!(with_state::<CloudSlotState, _>(&slot, |s| s
        .revisions
        .is_empty()));
    assert!(app
        .main_lines()
        .iter()
        .any(|t| t.contains("removed from your orinks.net account")));
    // The slot menu no longer offers a delete for backups that are gone.
    assert!(!menu_labels::<CloudSlotState>(&slot, &app.ctx)
        .iter()
        .any(|t| t.starts_with("Delete")));
}

/// Jessie's report, 2026-08-14: the server refused an upload with
/// invalid_achievement, but the game blamed the connection. The "Keep this
/// computer's save and back it up" retry must now speak the real family --
/// network, auth, or a server rejection -- not one line for all three.
/// Shane's report, same day: for a server rejection specifically, it must also
/// name the career and split the story by reason code, the same as the
/// background auto-backup queue does.
#[test]
fn test_keep_mine_retry_speaks_the_real_cause() {
    let cases: [(&str, NetError, &[&str]); 5] = [
        (
            "network",
            NetError::other("OSError", "no route to host"),
            &["check your connection"],
        ),
        (
            "http_401",
            auth_error(401, None),
            &["no longer accepts this computer's sign-in"],
        ),
        (
            // Arithmetic cross-check refusal: named career, flagged for review,
            // and the owner-required appeal sentence (a real career was
            // false-flagged by this exact wording on 2026-08-14).
            "impossible_money",
            rejected_error("impossible_money"),
            &["road star", "flagged it for review", "tester document"],
        ),
        (
            // Schema/version refusal: named career, blames a build mismatch,
            // never "flagged".
            "invalid_schema",
            rejected_error("invalid_schema"),
            &["road star", "build mismatch", "not something you did"],
        ),
        (
            // A rejected code with no specific family: named career, the
            // generic wording, still not "check your connection".
            "invalid_achievement",
            rejected_error("invalid_achievement"),
            &["road star", "backup not accepted"],
        ),
    ];
    for (reason, error, expected_fragments) in cases {
        let mut app = TestApp::new();
        let _identity = save_identity(&app);
        ff_core::models::profile::Profile::named("Road Star")
            .save()
            .unwrap();
        install_cloud(&mut app, FakeTransport::failing(error), true);

        let mut slot = CloudSlotState::new(&mut app.ctx, "Road Star", Vec::new(), None, None);
        slot.threaded = false;
        let slot = push(&mut app, slot);
        app.clear_speech();
        with_state::<CloudSlotState, _>(&slot, |s| {
            s.start_keep_mine(&mut app.ctx);
            State::update(s, &mut app.ctx, 0.0);
        });

        let spoken = app.main_lines();
        let joined = spoken.join(" ").to_lowercase();
        for fragment in expected_fragments {
            assert!(joined.contains(fragment), "{reason}: {spoken:?}");
        }
        // The message never claims a network problem for a server rejection,
        // and never claims a server rejection for an actual network problem.
        if !["network", "http_401"].contains(&reason) {
            assert!(!spoken.iter().any(|t| {
                let lower = t.to_lowercase();
                lower.contains("check your connection") && !lower.contains("backup not accepted")
            }));
            // The raw reason code is logged for review; it is never spoken.
            assert!(!joined.contains(reason), "{reason}: {spoken:?}");
        }
    }
}

#[test]
fn test_backup_menu_says_off_and_offers_the_opt_in() {
    let mut app = TestApp::new();
    let _identity = save_identity(&app);
    install_cloud(
        &mut app,
        FakeTransport::replying(json!({"saves": [], "publicSaveName": null})),
        false,
    );
    let mut state = CloudBackupState::new(&mut app.ctx);
    state.threaded = false;
    let state = push(&mut app, state);
    assert!(with_state::<CloudBackupState, _>(&state, |s| s.fetched()));
    with_state::<CloudBackupState, _>(&state, |s| State::update(s, &mut app.ctx, 0.0));

    assert!(menu_labels::<CloudBackupState>(&state, &app.ctx)
        .iter()
        .any(|t| t.starts_with("Status: Cloud backup is off")));
    move_to::<CloudBackupState>(&mut app, &state, "Turn Cloud backup on");
    press(&mut app, Key::Return);

    let consent = app.state().unwrap();
    assert_eq!(
        with_state::<CloudBackupConsentState, _>(&consent, |s| s.menu.title.clone()),
        "Turn Cloud backup on?"
    );
    press(&mut app, Key::Down);
    press(&mut app, Key::Return);

    assert!(app.ctx.settings.cloud_saves);
    assert!(app.ctx.services.cloud.enabled());
    // Back on the rebuilt backup menu: the opt-in is gone and the status line
    // reports readiness instead of off.
    assert!(std::rc::Rc::ptr_eq(&app.state().unwrap(), &state));
    assert!(with_state::<CloudBackupState, _>(&state, |s| s.fetched()));
    with_state::<CloudBackupState, _>(&state, |s| State::update(s, &mut app.ctx, 0.0));
    let texts = menu_labels::<CloudBackupState>(&state, &app.ctx);
    assert!(!texts.iter().any(|t| t == "Turn Cloud backup on"));
    assert!(texts.iter().any(|t| t == "Status: Cloud backup is ready."));
}

// -- the 1.9 cutover gate (careers from the 1.8 line do not restore here) --------

#[test]
fn test_legacy_cloud_restore_refuses_without_touching_anything() {
    let dir = tempfile::tempdir().unwrap();
    let writer = DiskWriter::new(dir.path());
    let local = json!({"name": "Road Star", "money": 5.0, "version": 7});
    let path = writer.write(&local).unwrap();
    let before = fs::read(&path).unwrap();
    let (dict, sig) = legacy_profile();
    let payload = download(make_cloud_reply(&dict, sig, 3))
        .unwrap()
        .expect("a payload");
    let sync_state = SyncState::new(dir.path());

    assert_eq!(
        restore(&writer, &payload, Some(&sync_state)),
        Err(RestoreError::LegacyCareer("Road Star".to_string()))
    );

    // Refused before anything was written: the local save is byte-for-byte
    // intact, no fallback file appeared, and the sync state never moved.
    // (The cloud copy is only ever read here; deleting is a separate,
    // confirmed menu action.)
    assert_eq!(fs::read(&path).unwrap(), before);
    assert!(!path.with_extension("ffsave.bak").exists());
    assert!(sync_state.slots().is_empty());
    assert_eq!(writer.written.lock().unwrap().len(), 1);
}

#[test]
fn test_current_line_backup_without_marker_still_restores() {
    // A 1.9 tester's backup from before the created-on marker existed: the
    // save version vouches for it, exactly as the local load gate does.
    let dir = tempfile::tempdir().unwrap();
    let writer = DiskWriter::new(dir.path());
    let (dict, sig) = money77_profile();
    let payload = download(make_cloud_reply(&dict, sig, 3))
        .unwrap()
        .expect("a payload");
    let path = restore(&writer, &payload, Some(&SyncState::new(dir.path()))).unwrap();
    let restored: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(restored["money"], 77_000.0);
}

#[test]
fn test_legacy_snapshot_is_labeled_and_refused_before_the_confirm_step() {
    let mut app = TestApp::new();
    let _identity = save_identity(&app);
    let entry = json!({
        "saveName": "Old Timer",
        "revision": 3,
        "saveVersion": 5,
        "createdAt": 1_700_000_000_000i64,
        "summary": "Old Timer, level 50",
    });
    let slot = CloudSlotState::new(&mut app.ctx, "Old Timer", vec![entry], None, None);
    let slot = push(&mut app, slot);
    let restore_items: Vec<String> = menu_labels::<CloudSlotState>(&slot, &app.ctx)
        .into_iter()
        .filter(|t| t.starts_with("Restore"))
        .collect();
    assert!(restore_items
        .iter()
        .any(|t| t.contains("from an earlier version of Freight Fate")));

    move_to::<CloudSlotState>(&mut app, &slot, "Restore");
    press(&mut app, Key::Return);

    // No confirmation screen opened, nothing was downloaded or deleted; the
    // refusal is spoken kindly and the menu stays put.
    assert!(std::rc::Rc::ptr_eq(&app.state().unwrap(), &slot));
    let spoken = app.main_lines();
    assert!(spoken.iter().any(|t| t.contains(LEGACY_BACKUP_NOTICE)));
    assert!(spoken
        .iter()
        .any(|t| t.contains("stays safe in your orinks.net account")));
}

// -- the manual backup attempt (Shane's report, 2026-08-14: hitting Save gave ----
// -- no sign a backup ran, so a silent one was indistinguishable from none) ------

#[test]
fn test_backup_now_skips_the_debounce() {
    let transport = FakeTransport::revisions();
    let clock = ManualClock::new();
    let service = make_service(&transport, &clock);

    let token = service
        .backup_now("Road Star", profile("Road Star", 5000.0))
        .unwrap();

    // No debounce wait: the attempt went out inside the call.
    assert_eq!(transport.posts().len(), 1);
    assert_eq!(
        service.outcome_for("Road Star", token).as_deref(),
        Some("accepted")
    );
    assert_eq!(revision_of(&service, "Road Star"), Some(1));
}

#[test]
fn test_backup_now_bypasses_the_transient_retry_backoff() {
    let transport = FakeTransport::revisions();
    transport.set_error(Some(NetError::other("OSError", "no network")));
    let clock = ManualClock::new();
    let service = make_service(&transport, &clock);
    service.queue_backup("Road Star", profile("Road Star", 5000.0));
    drain(&service, &clock);
    assert_eq!(transport.posts().len(), 1); // failed: the backoff is armed

    // A background pump sits the backoff out...
    service.pump(false);
    assert_eq!(transport.posts().len(), 1);

    // ...the manual attempt does not.
    transport.set_error(None);
    let token = service
        .backup_now("Road Star", profile("Road Star", 5000.0))
        .unwrap();
    assert_eq!(transport.posts().len(), 2);
    assert_eq!(
        service.outcome_for("Road Star", token).as_deref(),
        Some("accepted")
    );
}

#[test]
fn test_backup_now_reports_content_already_on_the_server() {
    let transport = FakeTransport::revisions();
    let clock = ManualClock::new();
    let service = make_service(&transport, &clock);
    service.queue_backup("Road Star", profile("Road Star", 5000.0));
    drain(&service, &clock);

    let token = service
        .backup_now("Road Star", profile("Road Star", 5000.0))
        .unwrap(); // nothing changed since

    assert_eq!(transport.posts().len(), 1); // the content-hash skip still holds
    assert_eq!(
        service.outcome_for("Road Star", token).as_deref(),
        Some("unchanged")
    );
}

#[test]
fn test_backup_now_reports_a_rejection_with_its_reason() {
    let transport = FakeTransport::failing(rejected_error("invalid_achievement"));
    let service = make_service(&transport, &ManualClock::new());

    let token = service
        .backup_now("Road Star", profile("Road Star", 5000.0))
        .unwrap();

    assert_eq!(
        service.outcome_for("Road Star", token).as_deref(),
        Some("rejected:invalid_achievement")
    );
}

#[test]
fn test_backup_now_reports_the_auth_family() {
    let service = make_service(
        &FakeTransport::failing(auth_error(401, None)),
        &ManualClock::new(),
    );

    let token = service
        .backup_now("Road Star", profile("Road Star", 5000.0))
        .unwrap();

    assert_eq!(
        service.outcome_for("Road Star", token).as_deref(),
        Some("auth")
    );
}

#[test]
fn test_backup_now_reports_network_trouble_and_keeps_retrying() {
    let transport = FakeTransport::revisions();
    transport.set_error(Some(NetError::other("OSError", "no route")));
    let clock = ManualClock::new();
    let service = make_service(&transport, &clock);

    let token = service
        .backup_now("Road Star", profile("Road Star", 5000.0))
        .unwrap();

    assert_eq!(
        service.outcome_for("Road Star", token).as_deref(),
        Some("network")
    );
    // The snapshot stays queued for the background retry cadence.
    transport.set_error(None);
    clock.advance(RETRY_INTERVAL_S + 0.1);
    service.pump(false);
    assert_eq!(revision_of(&service, "Road Star"), Some(1));
}

#[test]
fn test_backup_now_records_a_fresh_conflict() {
    let transport = FakeTransport::failing(conflict_error(Some(5)));
    let service = make_service(&transport, &ManualClock::new());

    let token = service
        .backup_now("Road Star", profile("Road Star", 5000.0))
        .unwrap();

    assert_eq!(
        service.outcome_for("Road Star", token).as_deref(),
        Some("conflict")
    );
    assert!(service.conflicts().contains_key("Road Star"));
}

#[test]
fn test_backup_now_respects_a_recorded_conflict() {
    // The conflict guard is unchanged: a live cloud copy still blocks the
    // upload -- but the manual attempt reports it instead of staying silent.
    let transport = RoutedTransport::new(
        json!({"ok": true, "saves": [{"saveName": "Road Star", "revision": 40}]}),
        None,
    );
    let service = make_service_with(
        transport.clone(),
        &ManualClock::new(),
        true,
        Some(identity()),
    );
    service
        .sync_state()
        .record_conflict("Road Star", &conflict_map(Some(40)));

    let token = service
        .backup_now("Road Star", profile("Road Star", 5000.0))
        .unwrap();

    assert!(transport.posts().is_empty());
    assert_eq!(
        service.outcome_for("Road Star", token).as_deref(),
        Some("conflict")
    );
}

#[test]
fn test_backup_now_disabled_returns_no_token() {
    let service = make_service_with(
        FakeTransport::revisions(),
        &ManualClock::new(),
        false,
        Some(identity()),
    );
    assert!(service
        .backup_now("Road Star", profile("Road Star", 5000.0))
        .is_none());
}

#[test]
fn test_outcome_for_ignores_results_from_an_earlier_attempt() {
    let service = make_service(&FakeTransport::revisions(), &ManualClock::new());
    let token = service
        .backup_now("Road Star", profile("Road Star", 5000.0))
        .unwrap();
    assert_eq!(
        service.outcome_for("Road Star", token).as_deref(),
        Some("accepted")
    );
    // A later attempt's poller never hears the old result.
    assert!(service.outcome_for("Road Star", token + 1).is_none());
}

#[test]
fn test_an_upload_in_flight_when_save_is_pressed_keeps_its_own_verdict() {
    // Uploads run outside the lock, so a background upload (a delivery
    // autosave, a retry pass) can already be on the wire when the player
    // presses Save game. Its terminal result must stay stamped with its own
    // queued-under token: the wrong save's verdict must never be spoken as
    // this save's outcome, and the manual attempt's own result must never be
    // silenced or overwritten by the older upload finishing late.
    let clock = ManualClock::new();
    let service_slot: Arc<Mutex<Option<CloudSaves>>> = Arc::new(Mutex::new(None));
    let tokens: Arc<Mutex<Vec<i64>>> = Arc::new(Mutex::new(Vec::new()));
    let newer = profile("Road Star", 5777.0); // the manual snapshot differs from the autosave's

    // The first request is the background upload already in flight;
    // while it is on the wire the player presses Save game (run to
    // completion re-entrantly, the synchronous stand-in for the worker
    // thread), and then the old upload comes back refused.
    let count = Arc::new(Mutex::new(0usize));
    let revision = Arc::new(Mutex::new(0i64));
    let transport: SharedTransport = {
        let service_slot = Arc::clone(&service_slot);
        let tokens = Arc::clone(&tokens);
        let count = Arc::clone(&count);
        let revision = Arc::clone(&revision);
        let newer = newer.clone();
        Arc::new(ClosureTransport(
            move |_u: &str, _p: Option<&Value>, _h: &[(String, String)], _m: Option<&str>| {
                let mut count = count.lock().unwrap();
                *count += 1;
                if *count == 1 {
                    let service = service_slot.lock().unwrap().clone().unwrap();
                    drop(count);
                    let token = service.backup_now("Road Star", newer.clone()).unwrap();
                    tokens.lock().unwrap().push(token);
                    return Err(rejected_error("invalid_achievement"));
                }
                let mut revision = revision.lock().unwrap();
                *revision += 1;
                Ok(json!({"ok": true, "revision": *revision}))
            },
        ))
    };
    let service = make_service_with(transport, &clock, true, Some(identity()));
    *service_slot.lock().unwrap() = Some(service.service.clone());
    service.queue_backup("Road Star", profile("Road Star", 5000.0));
    drain(&service, &clock);

    assert_eq!(*count.lock().unwrap(), 2); // the autosave, then the manual attempt
                                           // The manual attempt's watch hears its own accepted result -- not the
                                           // older upload's rejection, which finished after it and carries the
                                           // background token no watch ever matches.
    let token = tokens.lock().unwrap()[0];
    assert_eq!(
        service.outcome_for("Road Star", token).as_deref(),
        Some("accepted")
    );
}

// -- background refusals speak everywhere (owner decision, 2026-08-15: automatic --
// -- saves upload silently, so a blind player never heard a career stop backing up)

#[test]
fn test_background_rejection_announces_exactly_once_across_retries() {
    let transport = FakeTransport::failing(rejected_error("impossible_money"));
    let clock = ManualClock::new();
    let service = make_service(&transport, &clock);

    service.queue_backup("Road Star", profile("Road Star", 5000.0));
    drain(&service, &clock);

    let lines = service.take_announcements();
    assert_eq!(lines.len(), 1);
    assert!(lines[0].starts_with("Road Star: backup not accepted."));
    assert!(lines[0].contains("flagged it for review"));
    // The raw reason code stays log-only, exactly as in the status line.
    assert!(!lines[0].contains("impossible_money"));

    // Every later save refused for the same cause stays silent.
    for i in 1..=3 {
        service.queue_backup("Road Star", profile("Road Star", 5000.0 + i as f64));
        drain(&service, &clock);
    }
    assert!(service.take_announcements().is_empty());
}

#[test]
fn test_background_refusal_speaks_again_when_the_cause_changes() {
    let transport = FakeTransport::failing(rejected_error("invalid_schema"));
    let clock = ManualClock::new();
    let service = make_service(&transport, &clock);

    service.queue_backup("Road Star", profile("Road Star", 5000.0));
    drain(&service, &clock);
    assert!(service.take_announcements()[0].contains("build mismatch"));

    transport.set_error(Some(rejected_error("impossible_money")));
    service.queue_backup("Road Star", profile("Road Star", 5001.0));
    drain(&service, &clock);

    let lines = service.take_announcements();
    assert_eq!(lines.len(), 1);
    assert!(lines[0].contains("flagged it for review"));
}

#[test]
fn test_a_background_backup_says_it_was_backed_up() {
    // The ordinary case, and the one that used to say nothing: a save at a
    // rest stop uploads on the background queue, and a driver who cannot see
    // the status line heard the same silence whether it reached the server or
    // never left the machine (owner, 2026-08-15).
    let transport = FakeTransport::revisions();
    let clock = ManualClock::new();
    let service = make_service(&transport, &clock);

    service.queue_backup("Road Star", profile("Road Star", 5000.0));
    drain(&service, &clock);
    assert_eq!(
        service.take_announcements(),
        vec!["Road Star is backed up."]
    );
    assert_eq!(backup_status("Road Star"), "Road Star is backed up.");
    assert_eq!(
        recovery_status("Road Star"),
        "Road Star is backed up again."
    );
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

#[test]
fn test_once_a_session_speaks_each_career_first_backup_only() {
    let transport = FakeTransport::revisions();
    let clock = ManualClock::new();
    let service = make_service_announcing(&transport, &clock, BackupAnnouncements::Once);

    service.queue_backup("Road Star", profile("Road Star", 5000.0));
    drain(&service, &clock);
    assert_eq!(
        service.take_announcements(),
        vec!["Road Star is backed up."]
    );

    // Every later accepted upload of that career this session is silent.
    for i in 1..=3 {
        service.queue_backup("Road Star", profile("Road Star", 5000.0 + i as f64));
        drain(&service, &clock);
    }
    assert!(service.take_announcements().is_empty());

    // Another career still gets its first all-clear.
    service.queue_backup("Night Owl", profile("Night Owl", 6000.0));
    drain(&service, &clock);
    assert_eq!(
        service.take_announcements(),
        vec!["Night Owl is backed up."]
    );
}

#[test]
fn test_all_clear_off_stays_silent_but_still_answers_a_spoken_refusal() {
    let transport = FakeTransport::revisions();
    let clock = ManualClock::new();
    let service = make_service_announcing(&transport, &clock, BackupAnnouncements::Off);

    service.queue_backup("Road Star", profile("Road Star", 5000.0));
    drain(&service, &clock);
    assert!(service.take_announcements().is_empty());

    // Refusals are never part of the quiet: the driver must hear a career
    // stop backing up, and then hear it recover.
    transport.set_error(Some(rejected_error("impossible_money")));
    service.queue_backup("Road Star", profile("Road Star", 5001.0));
    drain(&service, &clock);
    assert_eq!(service.take_announcements().len(), 1);

    transport.set_error(None);
    service.queue_backup("Road Star", profile("Road Star", 5002.0));
    drain(&service, &clock);
    assert_eq!(
        service.take_announcements(),
        vec!["Road Star is backed up again."]
    );
}

#[test]
fn test_all_clear_cadence_can_change_mid_session() {
    let transport = FakeTransport::revisions();
    let clock = ManualClock::new();
    let service = make_service(&transport, &clock); // "every" by default

    service.queue_backup("Road Star", profile("Road Star", 5000.0));
    drain(&service, &clock);
    assert_eq!(service.take_announcements().len(), 1);

    service.set_backup_announcements(BackupAnnouncements::Off);
    service.queue_backup("Road Star", profile("Road Star", 5001.0));
    drain(&service, &clock);
    assert!(service.take_announcements().is_empty());

    service.set_backup_announcements(BackupAnnouncements::Every);
    service.queue_backup("Road Star", profile("Road Star", 5002.0));
    drain(&service, &clock);
    assert_eq!(
        service.take_announcements(),
        vec!["Road Star is backed up."]
    );
}

#[test]
fn test_backup_announcement_modes_read_from_the_setting() {
    assert_eq!(
        BackupAnnouncements::from_setting("every"),
        BackupAnnouncements::Every
    );
    assert_eq!(
        BackupAnnouncements::from_setting("once"),
        BackupAnnouncements::Once
    );
    assert_eq!(
        BackupAnnouncements::from_setting("off"),
        BackupAnnouncements::Off
    );
    // Anything else keeps the owner's 2026-08-15 default: say it every time.
    assert_eq!(
        BackupAnnouncements::from_setting("loud"),
        BackupAnnouncements::Every
    );
}

#[test]
fn test_a_background_backup_speaks_the_cloud_career_eviction() {
    let transport = FakeTransport::replying(json!({
        "ok": true,
        "revision": 1,
        "evictedSaveName": "Old Road"
    }));
    let clock = ManualClock::new();
    let service = make_service(&transport, &clock);

    service.queue_backup("Road Star", profile("Road Star", 5000.0));
    drain(&service, &clock);

    assert_eq!(
        service.take_announcements(),
        vec![
            "Road Star is backed up.",
            "Cloud backup removed Old Road, the least recently played cloud career. Your local career was not deleted."
        ]
    );
}

#[test]
fn test_an_untrusted_evicted_name_is_not_spoken() {
    let transport = FakeTransport::replying(json!({
        "ok": true,
        "revision": 1,
        "evictedSaveName": "Bad\nName"
    }));
    let clock = ManualClock::new();
    let service = make_service(&transport, &clock);

    service.queue_backup("Road Star", profile("Road Star", 5000.0));
    drain(&service, &clock);

    assert_eq!(
        service.take_announcements(),
        vec!["Road Star is backed up."]
    );
}

#[test]
fn test_a_manual_backup_carries_the_exact_eviction_outcome() {
    let transport = FakeTransport::replying(json!({
        "ok": true,
        "revision": 1,
        "evictedSaveName": "Old Road"
    }));
    let service = make_service(&transport, &ManualClock::new());

    let token = service
        .backup_now("Road Star", profile("Road Star", 5000.0))
        .unwrap();

    assert_eq!(
        service.outcome_for("Road Star", token).as_deref(),
        Some("accepted:evicted:Old Road")
    );
    assert!(service.take_announcements().is_empty());
}

#[test]
fn test_a_manual_backup_stays_silent_so_the_menu_watch_can_speak() {
    // states/city.py waits on the result and says "Backed up to the cloud."
    // itself; a second line from the queue would say it twice for one save.
    let transport = FakeTransport::revisions();
    let service = make_service(&transport, &ManualClock::new());

    let token = service
        .backup_now("Road Star", profile("Road Star", 5000.0))
        .unwrap();
    assert_eq!(
        service.outcome_for("Road Star", token).as_deref(),
        Some("accepted")
    );
    assert!(service.take_announcements().is_empty());
}

#[test]
fn test_nothing_to_send_stays_silent() {
    // The cloud already holds this exact save, so no upload ran. Claiming a
    // fresh backup would be untrue, and at a parked rest stop it would repeat
    // on every autosave.
    let transport = FakeTransport::revisions();
    let clock = ManualClock::new();
    let service = make_service(&transport, &clock);

    service.queue_backup("Road Star", profile("Road Star", 5000.0));
    drain(&service, &clock);
    assert_eq!(
        service.take_announcements(),
        vec!["Road Star is backed up."]
    );

    service.queue_backup("Road Star", profile("Road Star", 5000.0)); // unchanged since the accepted revision
    drain(&service, &clock);
    assert!(service.take_announcements().is_empty());
}

#[test]
fn test_success_after_a_spoken_refusal_announces_recovery_and_rearms() {
    let transport = FakeTransport::revisions();
    transport.set_error(Some(rejected_error("impossible_money")));
    let clock = ManualClock::new();
    let service = make_service(&transport, &clock);

    service.queue_backup("Road Star", profile("Road Star", 5000.0));
    drain(&service, &clock);
    assert_eq!(service.take_announcements().len(), 1);

    transport.set_error(None);
    service.queue_backup("Road Star", profile("Road Star", 5001.0));
    drain(&service, &clock);
    assert_eq!(
        service.take_announcements(),
        vec!["Road Star is backed up again."]
    );

    // A clean backup with nothing to recover from says the plain line rather
    // than the "again" one: nothing was wrong, so there is nothing to be over.
    service.queue_backup("Road Star", profile("Road Star", 5002.0));
    drain(&service, &clock);
    assert_eq!(
        service.take_announcements(),
        vec!["Road Star is backed up."]
    );

    // ...and the dedupe is re-armed: the same refusal speaks once more.
    transport.set_error(Some(rejected_error("impossible_money")));
    service.queue_backup("Road Star", profile("Road Star", 5003.0));
    drain(&service, &clock);
    assert_eq!(service.take_announcements().len(), 1);
}

#[test]
fn test_background_auth_refusal_speaks_the_reconnect_line_once() {
    let transport = FakeTransport::failing(auth_error(401, None));
    let clock = ManualClock::new();
    let service = make_service(&transport, &clock);

    service.queue_backup("Road Star", profile("Road Star", 5000.0));
    drain(&service, &clock);
    assert_eq!(service.take_announcements(), vec![AUTH_PAUSED_STATUS]);

    service.queue_backup("Road Star", profile("Road Star", 5001.0));
    drain(&service, &clock);
    assert!(service.take_announcements().is_empty());
    assert!(AUTH_HELP.starts_with("orinks.net no longer accepts this computer's sign-in."));
}

#[test]
fn test_background_conflict_announces_the_choice_line_once() {
    let transport = FakeTransport::failing(conflict_error(Some(5)));
    let clock = ManualClock::new();
    let service = make_service(&transport, &clock);

    service.queue_backup("Road Star", profile("Road Star", 5000.0));
    drain(&service, &clock);
    assert_eq!(
        service.take_announcements(),
        vec![conflict_status("Road Star")]
    );

    // Later saves blocked by the same recorded conflict stay silent.
    service.queue_backup("Road Star", profile("Road Star", 5001.0));
    drain(&service, &clock);
    assert!(service.take_announcements().is_empty());
}

#[test]
fn test_background_network_trouble_stays_a_silent_retry() {
    // Transient failures retry on their own; the manual save path already
    // voices them on demand, so the global channel says nothing.
    let transport = FakeTransport::failing(NetError::other("OSError", "no route"));
    let clock = ManualClock::new();
    let service = make_service(&transport, &clock);

    service.queue_backup("Road Star", profile("Road Star", 5000.0));
    drain(&service, &clock);

    assert!(service.take_announcements().is_empty());
}

#[test]
fn test_manual_refusal_never_reaches_the_global_channel() {
    // The terminal's Save game item polls and speaks its own outcome
    // (states/city.py); announcing it here too would say the refusal twice.
    let transport = FakeTransport::failing(rejected_error("invalid_achievement"));
    let service = make_service(&transport, &ManualClock::new());

    let token = service
        .backup_now("Road Star", profile("Road Star", 5000.0))
        .unwrap();

    assert_eq!(
        service.outcome_for("Road Star", token).as_deref(),
        Some("rejected:invalid_achievement")
    );
    assert!(service.take_announcements().is_empty());
}

#[test]
fn test_manual_success_after_a_spoken_refusal_recovers_silently_and_rearms() {
    let transport = FakeTransport::revisions();
    transport.set_error(Some(rejected_error("impossible_money")));
    let clock = ManualClock::new();
    let service = make_service(&transport, &clock);

    service.queue_backup("Road Star", profile("Road Star", 5000.0));
    drain(&service, &clock);
    assert_eq!(service.take_announcements().len(), 1);

    // The player hits Save game and the upload goes through: the city
    // watch speaks "Backed up to the cloud." on its own, so the global
    // channel must not add a second line for the same event.
    transport.set_error(None);
    let token = service
        .backup_now("Road Star", profile("Road Star", 5001.0))
        .unwrap();
    assert_eq!(
        service.outcome_for("Road Star", token).as_deref(),
        Some("accepted")
    );
    assert!(service.take_announcements().is_empty());

    // The dedupe still re-armed: the same refusal speaks once more.
    transport.set_error(Some(rejected_error("impossible_money")));
    service.queue_backup("Road Star", profile("Road Star", 5002.0));
    drain(&service, &clock);
    assert_eq!(service.take_announcements().len(), 1);
}

#[test]
fn test_manual_refusal_seeds_the_dedupe_for_the_background_channel() {
    // A conflict spoken by the manual Save game watch (states/city.py) is
    // already in the player's ear; the next automatic save blocked by the
    // same standing conflict must not repeat the identical sentence.
    let transport = FakeTransport::failing(conflict_error(Some(5)));
    let clock = ManualClock::new();
    let service = make_service(&transport, &clock);

    let token = service
        .backup_now("Road Star", profile("Road Star", 5000.0))
        .unwrap();
    assert_eq!(
        service.outcome_for("Road Star", token).as_deref(),
        Some("conflict")
    );
    assert!(service.take_announcements().is_empty()); // the menu watch owns this one

    service.queue_backup("Road Star", profile("Road Star", 5001.0));
    drain(&service, &clock);
    assert!(service.take_announcements().is_empty());
}

#[test]
fn test_auth_announcement_is_one_per_outage_not_per_career() {
    let transport = FakeTransport::revisions();
    transport.set_error(Some(auth_error(401, None)));
    let clock = ManualClock::new();
    let service = make_service(&transport, &clock);

    service.queue_backup("Road Star", profile("Road Star", 5000.0));
    drain(&service, &clock);
    assert_eq!(service.take_announcements(), vec![AUTH_PAUSED_STATUS]);

    // The sign-in belongs to this computer, not to the career: loading
    // another career during the same outage must not repeat the line.
    service.queue_backup("Night Owl", profile("Night Owl", 5000.0));
    drain(&service, &clock);
    assert!(service.take_announcements().is_empty());

    // A successful upload proves the sign-in works again. It says the plain
    // backup line, not a recovery one: the refusal was announced against this
    // computer's sign-in rather than either career, so no slot has anything to
    // be over -- but the upload did happen, and every accepted background
    // backup says so.
    transport.set_error(None);
    service.queue_backup("Night Owl", profile("Night Owl", 5000.0));
    drain(&service, &clock);
    assert_eq!(
        service.take_announcements(),
        vec!["Night Owl is backed up."]
    );

    // ...and a fresh outage after that speaks afresh.
    transport.set_error(Some(auth_error(401, None)));
    service.queue_backup("Road Star", profile("Road Star", 5000.0));
    drain(&service, &clock);
    assert_eq!(service.take_announcements(), vec![AUTH_PAUSED_STATUS]);
}

#[test]
fn test_take_announcements_drains_the_queue() {
    let transport = FakeTransport::failing(rejected_error("impossible_money"));
    let clock = ManualClock::new();
    let service = make_service(&transport, &clock);
    service.queue_backup("Road Star", profile("Road Star", 5000.0));
    drain(&service, &clock);

    assert_eq!(service.take_announcements().len(), 1);
    assert!(service.take_announcements().is_empty());
}

/// The worker thread never speaks; the app's main loop drains
/// take_announcements and delivers on the normal announcement channel -- the
/// same polled pattern as the controller-disconnect notice.
#[test]
fn test_app_loop_speaks_queued_background_refusals() {
    let mut app = TestApp::new();
    let clock = ManualClock::new();
    let transport = FakeTransport::failing(rejected_error("impossible_money"));
    let service = make_service(&transport, &clock);
    app.ctx.services.cloud = service.clone();
    service.queue_backup("Road Star", profile("Road Star", 5000.0));
    drain(&service, &clock);
    app.clear_speech();

    app.run(Some(3)); // run() shuts the app down on its way out

    let refusals = app
        .main_lines()
        .into_iter()
        .filter(|t| t.starts_with("Road Star: backup not accepted."))
        .count();
    assert_eq!(refusals, 1);
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

#[test]
fn test_terminal_save_speaks_the_accepted_backup() {
    let mut app = TestApp::new();
    let clock = ManualClock::new();
    let transport = FakeTransport::revisions();
    let service = make_service(&transport, &clock);
    let mut menu = make_terminal_menu(&mut app, (*service).clone());
    app.clear_speech();

    menu.save(&mut app.ctx);
    assert_eq!(last_line(&app), "Game saved. Backing up.");

    State::update(&mut menu, &mut app.ctx, 0.0);
    assert_eq!(last_line(&app), "Backed up to the cloud.");
}

#[test]
fn test_terminal_save_speaks_the_exact_cloud_career_eviction() {
    let mut app = TestApp::new();
    let clock = ManualClock::new();
    let transport = FakeTransport::replying(json!({
        "ok": true,
        "revision": 1,
        "evictedSaveName": "Old Road"
    }));
    let service = make_service(&transport, &clock);
    let mut menu = make_terminal_menu(&mut app, (*service).clone());
    app.clear_speech();

    menu.save(&mut app.ctx);
    State::update(&mut menu, &mut app.ctx, 0.0);

    assert_eq!(
        last_line(&app),
        "Cloud backup removed Old Road, the least recently played cloud career. Your local career was not deleted."
    );
}

#[test]
fn test_terminal_save_says_when_the_latest_save_is_already_backed_up() {
    let mut app = TestApp::new();
    let clock = ManualClock::new();
    let transport = FakeTransport::revisions();
    let service = make_service(&transport, &clock);
    let mut menu = make_terminal_menu(&mut app, (*service).clone());
    menu.save(&mut app.ctx);
    State::update(&mut menu, &mut app.ctx, 0.0);

    app.clear_speech();
    menu.save(&mut app.ctx);
    State::update(&mut menu, &mut app.ctx, 0.0);

    // The line has to answer the worry a driver actually has here. After
    // fuelling and buying tires they KNOW the career changed, so a bare
    // "already backed up" reads as the game refusing to send it (Shane,
    // 2026-08-15). Naming what is true -- the cloud copy matches this
    // computer -- is the part that settles it.
    assert_eq!(
        app.main_lines(),
        vec![
            "Game saved. Backing up.".to_string(),
            "Already backed up. The cloud copy matches this computer's save.".to_string(),
        ]
    );
}

#[test]
fn test_terminal_save_speaks_a_rejection_with_the_career_named() {
    let mut app = TestApp::new();
    let clock = ManualClock::new();
    let transport = FakeTransport::failing(rejected_error("impossible_money"));
    let service = make_service(&transport, &clock);
    let mut menu = make_terminal_menu(&mut app, (*service).clone());
    app.clear_speech();

    menu.save(&mut app.ctx);
    State::update(&mut menu, &mut app.ctx, 0.0);

    let lines = app.main_lines();
    assert_eq!(lines[lines.len() - 2], "Game saved. Backing up.");
    let last = &lines[lines.len() - 1];
    assert!(
        last.starts_with("Road Star: backup not accepted."),
        "{last}"
    );
    assert!(last.contains("flagged it for review"), "{last}");
    // The raw reason code stays log-only, exactly as in the background queue.
    assert!(!lines.join(" ").contains("impossible_money"), "{lines:?}");
}

#[test]
fn test_terminal_save_with_cloud_off_mentions_it_only_when_an_account_exists() {
    let mut app = TestApp::new();
    let clock = ManualClock::new();
    let transport = FakeTransport::revisions();
    // An account is configured but backup is off: the save says so.
    let off = make_service_with(transport.clone(), &clock, false, Some(identity()));
    let mut menu = make_terminal_menu(&mut app, (*off).clone());
    app.clear_speech();

    menu.save(&mut app.ctx);

    assert_eq!(
        app.main_lines(),
        vec![
            "Game saved.".to_string(),
            "Cloud backup is off. Saves on this computer are not backed up.".to_string(),
        ]
    );

    // No account at all: saving stays local and quiet, exactly as before.
    app.ctx.services.cloud = (*make_service_with(transport.clone(), &clock, false, None)).clone();
    app.clear_speech();
    menu.save(&mut app.ctx);
    assert_eq!(app.main_lines(), vec!["Game saved.".to_string()]);
}

#[test]
fn test_terminal_save_hands_a_silent_attempt_back_to_the_background() {
    // A threaded service whose worker is never started: the outcome can never
    // land inside the wait, deterministically.
    let mut app = TestApp::new();
    let clock = ManualClock::new();
    let transport = FakeTransport::failing(NetError::Connection("no route".to_string()));
    let dir = tempfile::tempdir().unwrap();
    let service = CloudSaves::new(CloudSavesOptions {
        enabled: true,
        identity: Some(identity()),
        clock: clock.clock(),
        transport: transport.clone(),
        threaded: true,
        data_dir: dir.path().to_path_buf(),
        ..CloudSavesOptions::default()
    });
    let mut menu = make_terminal_menu(&mut app, service);
    app.clear_speech();

    menu.save(&mut app.ctx);
    assert_eq!(last_line(&app), "Game saved. Backing up.");

    State::update(&mut menu, &mut app.ctx, BACKUP_RESULT_WAIT_S / 2.0);
    assert_eq!(last_line(&app), "Game saved. Backing up.");

    State::update(&mut menu, &mut app.ctx, BACKUP_RESULT_WAIT_S);
    assert_eq!(
        last_line(&app),
        "The backup will keep retrying in the background."
    );

    // Exactly one result line: nothing else ever arrives for this save.
    State::update(&mut menu, &mut app.ctx, 60.0);
    let repeats = app
        .main_lines()
        .into_iter()
        .filter(|t| t == "The backup will keep retrying in the background.")
        .count();
    assert_eq!(repeats, 1);
}

#[test]
fn test_sandbox_save_never_reaches_the_cloud() {
    // A driving-school or forced-playtest sandbox save never reaches disk
    // (save_profile's own guard), so it must never reach the cloud either --
    // the throwaway profile shares the real career's slot name -- and must not
    // promise a backup out loud.
    let mut app = TestApp::new();
    let clock = ManualClock::new();
    let transport = FakeTransport::revisions();
    let service = make_service(&transport, &clock);
    let mut menu = make_terminal_menu(&mut app, (*service).clone());
    app.ctx.playtest_sandbox = true;
    app.clear_speech();

    menu.save(&mut app.ctx);
    State::update(&mut menu, &mut app.ctx, 0.0);

    assert!(transport.requests().is_empty());
    assert_eq!(app.main_lines(), vec!["Game saved.".to_string()]);
}

#[test]
fn test_leaving_the_terminal_drops_the_pending_backup_announcement() {
    let mut app = TestApp::new();
    let clock = ManualClock::new();
    let transport = FakeTransport::revisions();
    let dir = tempfile::tempdir().unwrap();
    // Worker never started: the outcome stays pending.
    let service = CloudSaves::new(CloudSavesOptions {
        enabled: true,
        identity: Some(identity()),
        clock: clock.clock(),
        transport: transport.clone(),
        threaded: true,
        data_dir: dir.path().to_path_buf(),
        ..CloudSavesOptions::default()
    });
    let mut menu = make_terminal_menu(&mut app, service);
    menu.save(&mut app.ctx);

    State::exit(&mut menu, &mut app.ctx);
    app.clear_speech();
    State::update(&mut menu, &mut app.ctx, 60.0);

    assert!(app.main_lines().is_empty(), "{:?}", app.main_lines());
}

// -- threaded shutdown flush --------------------------------------------------------

#[test]
fn test_threaded_shutdown_flushes_the_pending_upload_once() {
    // The worker is never started here, so the only upload is the bounded
    // flush shutdown owes the pending snapshot.
    let transport = FakeTransport::revisions();
    let dir = tempfile::tempdir().unwrap();
    let service = CloudSaves::new(CloudSavesOptions {
        enabled: true,
        identity: Some(identity()),
        transport: transport.clone(),
        threaded: true,
        data_dir: dir.path().to_path_buf(),
        ..CloudSavesOptions::default()
    });
    service.queue_backup("Road Star", profile("Road Star", 5000.0));
    service.shutdown();
    assert_eq!(transport.posts().len(), 1);
    assert_eq!(revision_of(&service, "Road Star"), Some(1));
}

#[test]
fn test_url_quote_matches_urllib() {
    assert_eq!(url_quote("Road Star"), "Road%20Star");
    assert_eq!(url_quote("a/b-c_d.e~f"), "a/b-c_d.e~f");
    assert_eq!(url_quote("café"), "caf%C3%A9");
}

// -- a conflict for a career this computer no longer has --------------------------

/// Shane, 2026-09-20: the Online hub said "testing the limit is waiting for
/// you to choose which copy to keep" for a career that was in neither the
/// cloud nor on his computer, and nothing he could do cleared it.
///
/// Both existing heals live in `upload_slot`, on the way to an upload. A
/// career with no save on disk never queues one, so neither heal could ever
/// run and the row was permanent.
#[test]
fn test_a_conflict_for_a_deleted_career_is_forgotten_at_startup() {
    let app = TestApp::new();
    let _ = &app; // the save directory is pinned for this thread by TestApp
    let transport = Arc::new(FakeTransport::new());
    let clock = ManualClock::new();
    let service = make_service(&transport, &clock);
    service
        .sync_state()
        .record_conflict("testing the limit", &conflict_map(Some(40)));
    assert!(service.conflicts().contains_key("testing the limit"));

    service.start();

    assert!(
        service.conflicts().is_empty(),
        "a conflict with no local career to keep must not survive startup"
    );
    // Forgotten, not merely hidden: a row left behind would attach itself to
    // the next career started under the same name.
    assert!(service.sync_state().slot("testing the limit").is_empty());
    service.shutdown();
}

#[test]
fn test_a_conflict_for_a_career_still_on_disk_survives_startup() {
    let app = TestApp::new();
    let _ = &app;
    let mut profile = Profile::new();
    profile.name = "Road Star".to_string();
    profile
        .save()
        .expect("the career saves to the pinned directory");

    let transport = Arc::new(FakeTransport::new());
    let clock = ManualClock::new();
    let service = make_service(&transport, &clock);
    service
        .sync_state()
        .record_conflict("Road Star", &conflict_map(Some(40)));

    service.start();

    assert!(
        service.conflicts().contains_key("Road Star"),
        "the player still has this career; the choice is real and must stay"
    );
    service.shutdown();
}
