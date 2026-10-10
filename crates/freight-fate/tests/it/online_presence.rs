//! Port of `tests/test_online_presence.py`: the opt-in orinks.net
//! drivers-board service.
//!
//! These cover the behaviour that must hold regardless of whether the site is
//! even reachable: the disabled/off-by-default path, heartbeat and change
//! scheduling, the off-duty grace and sign-off, credential storage, and the
//! credential verification. A fake transport and an injected clock keep
//! every test deterministic and free of real sockets.

use std::fs;
use std::sync::{Arc, Mutex};

use serde_json::{json, Value};

use freight_fate::discord_presence::PresenceState;
use freight_fate::net::testing::{FakeTransport, ManualClock};
use freight_fate::net::{header, NetError, SharedTransport};
use freight_fate::online_presence::{
    base_url, client_version, client_version_for, fetch_board, fetch_mastodon_status,
    request_headers, set_profile_sharing, take_secret_store_timeout_notice, verify_identity,
    IdentityStore, MastodonStatus, MemoryStore, OnlineIdentity, OnlinePresence,
    OnlinePresenceOptions, RefusingStore, SecretStore, HEARTBEAT_INTERVAL_S, IDLE_SIGNOFF_S,
    MIN_CHANGE_INTERVAL_S, OFF_DUTY_GRACE_S, PACKAGE_VERSION, PAUSED_ACTIVITY, RADIO_CLAUSE,
    TOKEN_SERVICE,
};
use freight_fate::updater::BuildInfo;

fn identity() -> OnlineIdentity {
    OnlineIdentity::new("driver-testtest", &"t".repeat(48))
}

fn driving() -> PresenceState {
    PresenceState::new("Driving: Chicago to Dallas", "steel coils, 45% there")
}

fn resting() -> PresenceState {
    PresenceState::new("Resting at a stop", "steel coils, 45% there")
}

fn paused() -> PresenceState {
    PresenceState::new(PAUSED_ACTIVITY, "steel coils, 45% there")
}

/// A synchronous (non-threaded) service wired to a fake transport.
fn make_service(
    transport: &Arc<FakeTransport>,
    clock: &Arc<ManualClock>,
    enabled: bool,
    identity: Option<OnlineIdentity>,
) -> OnlinePresence {
    let shared: SharedTransport = transport.clone();
    OnlinePresence::new(OnlinePresenceOptions {
        enabled,
        identity,
        clock: clock.clock(),
        transport: shared,
        threaded: false,
        ..OnlinePresenceOptions::default()
    })
}

fn service(transport: &Arc<FakeTransport>, clock: &Arc<ManualClock>) -> OnlinePresence {
    make_service(transport, clock, true, Some(identity()))
}

fn last_activity(transport: &FakeTransport) -> String {
    transport.posts().last().unwrap()["activity"]
        .as_str()
        .unwrap()
        .to_string()
}

/// `isolated_data_dir` + `isolated_keyring`: a fresh data directory and an
/// in-memory secret store, as the conftest fixtures gave every test.
fn store() -> (tempfile::TempDir, Arc<MemoryStore>, IdentityStore) {
    let dir = tempfile::tempdir().unwrap();
    let keyring = MemoryStore::new();
    let store = IdentityStore::new(dir.path(), Some(keyring.clone()));
    (dir, keyring, store)
}

// -- disabled and unconfigured paths ------------------------------------------

#[test]
fn test_profile_sharing_defaults_off_without_setup() {
    // Settings().online_presence is False by default (the settings port
    // pins that); with no identity on disk the service stays dormant.
    let (_dir, _keyring, store) = store();
    assert!(store.load().is_none());
    let service = OnlinePresence::new(OnlinePresenceOptions {
        enabled: false,
        identity: store.load(),
        ..OnlinePresenceOptions::default()
    });
    assert!(!service.enabled());
}

#[test]
fn test_profile_sharing_posts_one_authoritative_boolean() {
    let transport = FakeTransport::replying(json!({"ok": true, "enabled": false}));
    assert_eq!(
        set_profile_sharing(&identity(), false, transport.as_ref()),
        "ok"
    );
    let request = &transport.requests()[0];
    assert!(request.url.ends_with("/api/freight-fate/profile-sharing"));
    assert_eq!(
        request.payload,
        Some(json!({"driverId": identity().driver_id, "enabled": false}))
    );
    assert!(request
        .header("Authorization")
        .unwrap()
        .starts_with("Bearer "));
}

#[test]
fn test_disabled_never_posts() {
    let transport = FakeTransport::new();
    let service = make_service(&transport, &ManualClock::new(), false, Some(identity()));
    service.start();
    service.update(Some(driving()));
    service.shutdown();
    assert!(transport.requests().is_empty());
}

#[test]
fn test_enabled_without_identity_stays_dormant() {
    let transport = FakeTransport::new();
    let service = make_service(&transport, &ManualClock::new(), true, None);
    assert!(!service.enabled());
    service.start();
    service.update(Some(driving()));
    service.shutdown();
    assert!(transport.requests().is_empty());
}

// -- heartbeat scheduling ------------------------------------------------------

#[test]
fn test_first_update_posts_immediately_with_credentials() {
    let transport = FakeTransport::new();
    let service = service(&transport, &ManualClock::new());
    service.start();
    service.update(Some(driving()));

    let request = &transport.requests()[0];
    assert_eq!(
        request.url,
        format!("{}/api/freight-fate/presence", base_url())
    );
    assert_eq!(
        request.payload,
        Some(json!({
            "driverId": identity().driver_id,
            "activity": driving().activity,
            "detail": driving().detail,
        }))
    );
    assert_eq!(
        request.headers,
        vec![(
            "Authorization".to_string(),
            format!("Bearer {}", identity().driver_token)
        )]
    );
}

#[test]
fn test_identical_state_reposts_only_on_the_heartbeat() {
    let transport = FakeTransport::new();
    let clock = ManualClock::new();
    let service = service(&transport, &clock);
    service.start();
    service.update(Some(driving()));
    assert_eq!(transport.posts().len(), 1);

    // The same snapshot again, before the heartbeat: nothing new goes out.
    clock.advance(HEARTBEAT_INTERVAL_S / 2.0);
    service.pump();
    assert_eq!(transport.posts().len(), 1);

    // Past the heartbeat the same snapshot is resent to keep the TTL alive.
    clock.advance(HEARTBEAT_INTERVAL_S / 2.0);
    service.pump();
    assert_eq!(transport.posts().len(), 2);
    assert_eq!(transport.posts()[1]["activity"], driving().activity);
}

#[test]
fn test_changes_are_throttled_then_flushed() {
    let transport = FakeTransport::new();
    let clock = ManualClock::new();
    let service = service(&transport, &clock);
    service.start();
    service.update(Some(driving()));

    // A change right away is throttled...
    clock.advance(MIN_CHANGE_INTERVAL_S / 2.0);
    service.update(Some(resting()));
    assert_eq!(transport.posts().len(), 1);

    // ...and flushes once the change window has passed.
    clock.advance(MIN_CHANGE_INTERVAL_S / 2.0);
    service.pump();
    assert_eq!(transport.posts().len(), 2);
    assert_eq!(transport.posts()[1]["activity"], resting().activity);
}

#[test]
fn test_failed_post_is_retried_on_the_heartbeat_schedule() {
    let transport = FakeTransport::failing(NetError::other("OSError", "offline"));
    let clock = ManualClock::new();
    let service = service(&transport, &clock);
    service.start();
    service.update(Some(driving()));
    assert_eq!(transport.request_count(), 1);

    // Not hammered while the site is down, not even on the change throttle:
    // the snapshot that failed is not a new change...
    clock.advance(1.0);
    service.pump();
    clock.advance(MIN_CHANGE_INTERVAL_S);
    service.pump();
    assert_eq!(transport.request_count(), 1);

    // ...but tried again a heartbeat later, and recovery works.
    transport.set_error(None);
    clock.advance(HEARTBEAT_INTERVAL_S);
    service.pump();
    assert_eq!(transport.request_count(), 2);
}

#[test]
fn test_a_refused_post_waits_for_the_heartbeat_but_a_new_change_does_not() {
    // A rejected driver (deleted, or a token rotated elsewhere) was retried
    // every change window forever: four requests a minute from one game.
    let transport = FakeTransport::failing(NetError::http(404));
    let clock = ManualClock::new();
    let service = service(&transport, &clock);
    service.start();
    service.update(Some(driving()));
    for _ in 0..4 {
        clock.advance(MIN_CHANGE_INTERVAL_S);
        service.pump();
    }
    assert_eq!(transport.request_count(), 1);

    // Something new to say still goes out on the change throttle.
    service.update(Some(resting()));
    assert_eq!(transport.request_count(), 2);
}

#[test]
fn test_a_failed_sign_off_is_retried_on_the_heartbeat() {
    let transport = FakeTransport::new();
    let clock = ManualClock::new();
    let service = service(&transport, &clock);
    service.start();
    service.update(Some(driving()));

    transport.set_error(Some(NetError::http(503)));
    service.update(None);
    clock.advance(OFF_DUTY_GRACE_S + 1.0);
    service.pump();
    assert_eq!(transport.request_count(), 2);

    // The worker used to wake every twentieth of a second and post again.
    clock.advance(1.0);
    service.pump();
    assert_eq!(transport.request_count(), 2);

    transport.set_error(None);
    clock.advance(HEARTBEAT_INTERVAL_S);
    service.pump();
    assert_eq!(last_activity(&transport), "");
    clock.advance(HEARTBEAT_INTERVAL_S);
    service.pump();
    assert_eq!(transport.request_count(), 3); // off the board, and quiet
}

// -- going off duty -------------------------------------------------------------

#[test]
fn test_off_duty_signs_off_after_the_grace() {
    let transport = FakeTransport::new();
    let clock = ManualClock::new();
    let service = service(&transport, &clock);
    service.start();
    service.update(Some(driving()));

    service.update(None);
    assert_eq!(transport.posts().len(), 1); // grace running; still on the board

    clock.advance(OFF_DUTY_GRACE_S + 1.0);
    service.pump();
    assert_eq!(
        transport.posts().last().unwrap(),
        &json!({
            "driverId": identity().driver_id,
            "activity": "",
            "detail": "",
        })
    );
}

#[test]
fn test_brief_menu_detour_does_not_bounce_the_driver() {
    let transport = FakeTransport::new();
    let clock = ManualClock::new();
    let service = service(&transport, &clock);
    service.start();
    service.update(Some(driving()));

    // A two-second status-screen visit reports None, then driving again.
    service.update(None);
    clock.advance(2.0);
    service.update(Some(driving()));
    clock.advance(OFF_DUTY_GRACE_S);
    service.pump();

    assert!(transport
        .posts()
        .iter()
        .all(|post| !post["activity"].as_str().unwrap().is_empty())); // no sign-off sent
}

#[test]
fn test_off_duty_without_ever_posting_sends_nothing() {
    let transport = FakeTransport::new();
    let clock = ManualClock::new();
    let service = service(&transport, &clock);
    service.start();
    service.update(None);
    clock.advance(OFF_DUTY_GRACE_S + 1.0);
    service.pump();
    assert!(transport.requests().is_empty());
}

#[test]
fn test_idle_snapshot_signs_off_and_goes_quiet() {
    // A truck parked with the game left running (not paused) reports the
    // identical snapshot forever; after the idle window the service must
    // leave the board and stop spending heartbeats on it.
    let transport = FakeTransport::new();
    let clock = ManualClock::new();
    let service = service(&transport, &clock);
    service.start();
    service.update(Some(driving()));
    assert_eq!(transport.posts().len(), 1);

    // Half the window in: still a live driver, heartbeats keep the TTL alive.
    clock.advance(IDLE_SIGNOFF_S / 2.0);
    service.pump();
    assert_eq!(last_activity(&transport), driving().activity);

    // Window crossed: one sign-off, then silence on later heartbeat slots.
    clock.advance(IDLE_SIGNOFF_S / 2.0);
    service.pump();
    assert_eq!(last_activity(&transport), "");
    let sent = transport.posts().len();
    clock.advance(HEARTBEAT_INTERVAL_S * 2.0);
    service.pump();
    assert_eq!(transport.posts().len(), sent);
}

#[test]
fn test_snapshot_change_relists_an_idle_driver() {
    let transport = FakeTransport::new();
    let clock = ManualClock::new();
    let service = service(&transport, &clock);
    service.start();
    service.update(Some(driving()));
    clock.advance(IDLE_SIGNOFF_S);
    service.pump();
    assert_eq!(last_activity(&transport), "");

    // Rolling again changes the snapshot, which restarts the idle clock and
    // puts the driver back on the board within the change throttle.
    service.update(Some(resting()));
    clock.advance(MIN_CHANGE_INTERVAL_S);
    service.pump();
    assert_eq!(last_activity(&transport), resting().activity);

    // And the driver stays live from there: the next heartbeat still goes out.
    clock.advance(HEARTBEAT_INTERVAL_S);
    service.pump();
    assert_eq!(last_activity(&transport), resting().activity);
}

/// The board snapshot with the cab radio on a live stream, mid-song.
fn driving_to(song: &str) -> PresenceState {
    PresenceState::new(
        "Driving: Chicago to Dallas",
        &format!("steel coils, 45% there, {RADIO_CLAUSE}KVSC 88.1: {song}"),
    )
}

#[test]
fn test_a_new_song_does_not_keep_a_parked_truck_on_the_board() {
    // A truck parked with the radio on a live stream: the song title in the
    // snapshot changes every few minutes on its own. That kept a driver at
    // 0% on the board, beating, for seven hours (2026-10-03).
    let transport = FakeTransport::new();
    let clock = ManualClock::new();
    let service = service(&transport, &clock);
    service.start();
    let mut song = 0;
    service.update(Some(driving_to("song 0")));
    let step = 200.0;
    let mut t = 0.0;
    while t < IDLE_SIGNOFF_S {
        clock.advance(step);
        t += step;
        song += 1;
        service.update(Some(driving_to(&format!("song {song}"))));
        service.pump();
    }
    assert_eq!(
        last_activity(&transport),
        "",
        "the idle sign-off never went out"
    );

    // Signed off, the songs keep changing and nothing more is posted.
    let sent = transport.posts().len();
    for _ in 0..6 {
        clock.advance(step);
        song += 1;
        service.update(Some(driving_to(&format!("song {song}"))));
        service.pump();
    }
    assert_eq!(transport.posts().len(), sent);

    // Pulling out is a real change and re-lists the driver.
    service.update(Some(PresenceState::new(
        "Driving: Chicago to Dallas",
        &format!("steel coils, 50% there, {RADIO_CLAUSE}KVSC 88.1: song {song}"),
    )));
    clock.advance(MIN_CHANGE_INTERVAL_S);
    service.pump();
    assert_eq!(last_activity(&transport), "Driving: Chicago to Dallas");
}

#[test]
fn test_pause_posts_once_then_sends_no_heartbeats() {
    // A pause is not the end of a shift, so the driver must not sign off --
    // but a paused game has nothing new to say, so it must not spend a
    // heartbeat every two and a half minutes saying it either. The server
    // holds a paused row for the idle window without beats.
    let transport = FakeTransport::new();
    let clock = ManualClock::new();
    let service = service(&transport, &clock);
    service.start();
    service.update(Some(driving()));
    clock.advance(MIN_CHANGE_INTERVAL_S);

    service.update(Some(paused()));
    service.pump();
    assert_eq!(last_activity(&transport), PAUSED_ACTIVITY);
    let sent = transport.posts().len();

    // Heartbeat slots come and go: nothing is posted, and nothing is a
    // sign-off. Any other snapshot would have beaten here (see
    // test_identical_state_reposts_only_on_the_heartbeat).
    for _ in 0..4 {
        clock.advance(HEARTBEAT_INTERVAL_S);
        service.pump();
    }
    assert_eq!(transport.posts().len(), sent);

    // Resuming is a change like any other: the driver never left, and the
    // heartbeats pick up again from there.
    service.update(Some(driving()));
    clock.advance(MIN_CHANGE_INTERVAL_S);
    service.pump();
    assert_eq!(last_activity(&transport), driving().activity);
    clock.advance(HEARTBEAT_INTERVAL_S);
    service.pump();
    assert_eq!(last_activity(&transport), driving().activity);
    assert_eq!(transport.posts().len(), sent + 2);
}

#[test]
fn test_pause_left_for_the_idle_window_signs_off_once() {
    // A pause left for half an hour ages off the server's list like a
    // parked truck; the service sends the same single sign-off it does for
    // one, and then stays quiet.
    let transport = FakeTransport::new();
    let clock = ManualClock::new();
    let service = service(&transport, &clock);
    service.start();
    service.update(Some(driving()));
    clock.advance(MIN_CHANGE_INTERVAL_S);
    service.update(Some(paused()));
    service.pump();
    assert_eq!(last_activity(&transport), PAUSED_ACTIVITY);
    let sent = transport.posts().len();

    clock.advance(IDLE_SIGNOFF_S);
    service.pump();
    assert_eq!(last_activity(&transport), "");
    assert_eq!(transport.posts().len(), sent + 1);
    clock.advance(HEARTBEAT_INTERVAL_S * 2.0);
    service.pump();
    assert_eq!(transport.posts().len(), sent + 1);
}

#[test]
fn test_a_failed_idle_sign_off_from_a_pause_waits_for_the_heartbeat() {
    // Over a paused game the worker woke every twentieth of a second and
    // posted a failed idle sign-off again each time.
    let transport = FakeTransport::new();
    let clock = ManualClock::new();
    let service = service(&transport, &clock);
    service.start();
    service.update(Some(driving()));
    clock.advance(MIN_CHANGE_INTERVAL_S);
    service.update(Some(paused()));
    service.pump();
    let sent = transport.request_count();

    transport.set_error(Some(NetError::http(503)));
    clock.advance(IDLE_SIGNOFF_S);
    service.pump();
    assert_eq!(transport.request_count(), sent + 1);
    clock.advance(1.0);
    service.pump();
    assert_eq!(transport.request_count(), sent + 1);

    transport.set_error(None);
    clock.advance(HEARTBEAT_INTERVAL_S);
    service.pump();
    assert_eq!(transport.request_count(), sent + 2);
    assert_eq!(last_activity(&transport), "");
}

#[test]
fn test_shutdown_signs_off() {
    let transport = FakeTransport::new();
    let service = service(&transport, &ManualClock::new());
    service.start();
    service.update(Some(driving()));
    service.shutdown();
    assert_eq!(last_activity(&transport), "");
}

#[test]
fn test_disable_signs_off_and_reenable_resumes() {
    let transport = FakeTransport::new();
    let clock = ManualClock::new();
    let service = service(&transport, &clock);
    service.start();
    service.update(Some(driving()));

    service.set_enabled(false);
    assert_eq!(last_activity(&transport), "");
    assert!(!service.enabled());

    service.set_enabled(true);
    service.update(Some(driving()));
    assert_eq!(last_activity(&transport), driving().activity);
}

// -- identity storage ------------------------------------------------------------

#[test]
fn test_identity_round_trips_through_disk() {
    let (_dir, _keyring, store) = store();
    let identity = OnlineIdentity::new("road-star-abcd1234", &"s".repeat(68));
    store.save(&identity).unwrap();
    let loaded = store.load();
    assert_eq!(loaded, Some(identity));
}

#[test]
fn test_saved_identity_keeps_the_token_out_of_the_json_file() {
    // The public Driver ID stays on disk; the secret never does.
    //
    // Contributed by trodick in https://github.com/orinks-games/Freight-Fate/pull/133.
    let (_dir, keyring, store) = store();
    let identity = OnlineIdentity::new("road-star-abcd1234", &"s".repeat(68));
    store.save(&identity).unwrap();

    let payload = fs::read_to_string(store.path()).unwrap();
    assert!(payload.contains("\"driver_id\""));
    assert!(!payload.contains("driver_token"));
    assert!(!payload.contains(&identity.driver_token));
    assert!(!store.token_path().exists());
    assert_eq!(
        keyring
            .get_password(TOKEN_SERVICE, "road-star-abcd1234")
            .unwrap()
            .as_deref(),
        Some(identity.driver_token.as_str())
    );
}

#[test]
fn test_identity_falls_back_to_an_owner_only_file_without_a_secret_store() {
    let dir = tempfile::tempdir().unwrap();
    let store = IdentityStore::new(dir.path(), None);
    let identity = OnlineIdentity::new("road-star-abcd1234", &"s".repeat(68));
    if cfg!(windows) {
        let err = store.save(&identity).unwrap_err();
        assert!(err.to_string().contains("disabled on Windows"));
        assert!(!store.token_path().exists());
        assert!(!store.path().exists());
        return;
    }

    store.save(&identity).unwrap();

    let identity_file = store.path();
    let on_disk: Value =
        serde_json::from_str(&fs::read_to_string(&identity_file).unwrap()).unwrap();
    assert_eq!(
        on_disk,
        json!({"driver_id": identity.driver_id, "driver_token": identity.driver_token})
    );
    assert!(!store.token_path().exists());
    store.clear_cache(); // _next_session
    assert_eq!(store.load(), Some(identity));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&identity_file).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}

#[test]
fn test_fallback_ignores_a_stale_permissive_temp_file() {
    if cfg!(windows) {
        return; // Windows never writes a plaintext fallback identity
    }
    let dir = tempfile::tempdir().unwrap();
    let store = IdentityStore::new(dir.path(), None);
    let stale = store.path().with_extension("json.tmp");
    fs::create_dir_all(stale.parent().unwrap()).unwrap();
    fs::write(&stale, "stale, non-secret data").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&stale, fs::Permissions::from_mode(0o644)).unwrap();
    }

    let identity = OnlineIdentity::new("road-star-abcd1234", &"s".repeat(68));
    store.save(&identity).unwrap();

    assert_eq!(
        fs::read_to_string(&stale).unwrap(),
        "stale, non-secret data"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(store.path()).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}

#[test]
fn test_failed_legacy_migration_keeps_the_original_token() {
    // A store that refuses the token, and an identity file that cannot be
    // rewritten (Windows refuses the plaintext fallback outright; elsewhere
    // the directory is made read-only), must leave the legacy file intact.
    let dir = tempfile::tempdir().unwrap();
    let store = IdentityStore::new(dir.path(), Some(Arc::new(RefusingStore)));
    let path = store.path();
    let token = "s".repeat(68);
    fs::write(
        &path,
        json!({"driver_id": "road-star-abcd1234", "driver_token": token}).to_string(),
    )
    .unwrap();
    #[cfg(unix)]
    let restore_perms = {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o500)).unwrap();
        dir.path().to_path_buf()
    };

    assert_eq!(
        store.load(),
        Some(OnlineIdentity::new("road-star-abcd1234", &token))
    );
    store.clear_cache(); // _next_session

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&restore_perms, fs::Permissions::from_mode(0o700)).unwrap();
    }
    let payload: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(payload["driver_token"], token);
    assert_eq!(
        store.load(),
        Some(OnlineIdentity::new("road-star-abcd1234", &token))
    );
}

/// A secret store that counts reads, for the once-per-process cache test.
struct CountingStore {
    inner: Arc<MemoryStore>,
    reads: Mutex<Vec<String>>,
}

impl SecretStore for CountingStore {
    fn set_password(&self, service: &str, user: &str, password: &str) -> Result<(), String> {
        self.inner.set_password(service, user, password)
    }

    fn get_password(&self, service: &str, user: &str) -> Result<Option<String>, String> {
        self.reads.lock().unwrap().push(user.to_string());
        self.inner.get_password(service, user)
    }

    fn delete_password(&self, service: &str, user: &str) -> Result<(), String> {
        self.inner.delete_password(service, user)
    }
}

#[test]
fn test_the_secret_store_is_read_once_and_not_once_per_menu_frame() {
    let dir = tempfile::tempdir().unwrap();
    let counting = Arc::new(CountingStore {
        inner: MemoryStore::new(),
        reads: Mutex::new(Vec::new()),
    });
    let store = IdentityStore::new(dir.path(), Some(counting.clone()));

    let identity = OnlineIdentity::new("road-star-abcd1234", &"s".repeat(68));
    store.save(&identity).unwrap();
    store.clear_cache(); // _next_session

    for _ in 0..20 {
        assert_eq!(store.load(), Some(identity.clone()));
    }
    assert_eq!(counting.reads.lock().unwrap().len(), 1);
}

#[test]
fn test_a_secret_store_that_refuses_falls_back_instead_of_failing() {
    // The real headless-Linux shape: keyring imports, every call raises.
    let dir = tempfile::tempdir().unwrap();
    let store = IdentityStore::new(dir.path(), Some(Arc::new(RefusingStore)));
    let identity = OnlineIdentity::new("road-star-abcd1234", &"s".repeat(68));
    if cfg!(windows) {
        let err = store.save(&identity).unwrap_err();
        assert!(err.to_string().contains("disabled on Windows"));
        assert!(!store.token_path().exists());
        assert!(!store.path().exists());
        return;
    }

    store.save(&identity).unwrap();

    let payload: Value = serde_json::from_str(&fs::read_to_string(store.path()).unwrap()).unwrap();
    assert_eq!(payload["driver_token"], identity.driver_token);
    assert!(!store.token_path().exists());
    store.clear_cache();
    assert_eq!(store.load(), Some(identity));
}

#[test]
fn test_a_token_written_by_an_older_build_moves_into_the_secret_store() {
    let (_dir, keyring, store) = store();
    let path = store.path();
    fs::write(
        &path,
        json!({"driver_id": "road-star-abcd1234", "driver_token": "s".repeat(68)}).to_string(),
    )
    .unwrap();

    let loaded = store.load();
    assert_eq!(
        loaded,
        Some(OnlineIdentity::new("road-star-abcd1234", &"s".repeat(68)))
    );
    // Loading is what migrates: nobody has to re-paste their credentials.
    assert!(!fs::read_to_string(&path).unwrap().contains("driver_token"));
    assert!(!store.token_path().exists());
    assert!(keyring
        .get_password(TOKEN_SERVICE, "road-star-abcd1234")
        .unwrap()
        .is_some());
    assert_eq!(store.load(), loaded);
}

#[test]
fn test_a_fallback_token_file_is_cleared_once_a_secret_store_appears() {
    if cfg!(windows) {
        return; // Windows never writes a plaintext fallback token
    }
    let dir = tempfile::tempdir().unwrap();
    let identity = OnlineIdentity::new("road-star-abcd1234", &"s".repeat(68));
    let no_store = IdentityStore::new(dir.path(), None);
    fs::create_dir_all(no_store.path().parent().unwrap()).unwrap();
    fs::write(
        no_store.path(),
        json!({"driver_id": identity.driver_id}).to_string(),
    )
    .unwrap();
    fs::write(no_store.token_path(), &identity.driver_token).unwrap();
    assert!(no_store.token_path().exists());

    let store = IdentityStore::new(dir.path(), Some(MemoryStore::new()));
    assert_eq!(store.load(), Some(identity.clone()));
    assert!(!store.token_path().exists());
    assert_eq!(store.load(), Some(identity));
}

#[test]
fn test_failed_fallback_replacement_keeps_the_existing_identity() {
    if cfg!(windows) {
        return; // Windows never writes a plaintext fallback identity
    }
    let dir = tempfile::tempdir().unwrap();
    let store = IdentityStore::new(dir.path(), None);
    let original = OnlineIdentity::new("road-star-original", &"a".repeat(68));
    store.save(&original).unwrap();
    let replacement = OnlineIdentity::new("road-star-replaced", &"b".repeat(68));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o500)).unwrap();
    }

    assert!(store.save(&replacement).is_err());
    store.clear_cache();

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o700)).unwrap();
    }
    assert_eq!(store.load(), Some(original));
}

// -- packaging guard --------------------------------------------------------------

#[test]
fn test_the_secret_store_report_passes_on_a_source_checkout() {
    let (ok, detail) = freight_fate::online_presence::secret_store_report();
    assert!(ok, "{detail}");
    assert!(!detail.is_empty());
}

#[test]
#[ignore = "keyring entry-point metadata is a Python packaging concern; the Rust build links its backends in"]
fn test_the_secret_store_report_fails_when_the_backends_are_not_packaged() {}

#[test]
#[ignore = "keyring is a compile-time dependency of the Rust build; it cannot be absent"]
fn test_the_secret_store_report_fails_without_keyring_at_all() {}

#[test]
#[ignore = "the Nuitka build that needed keyring flags was deleted with the Python game"]
fn test_the_release_build_asks_for_keyrings_backends_and_metadata() {}

#[test]
fn test_missing_or_malformed_identity_loads_as_none() {
    let (_dir, _keyring, store) = store();
    assert!(store.load().is_none());
    let path = store.path();
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(
        &path,
        json!({"driver_id": "x", "driver_token": "short"}).to_string(),
    )
    .unwrap();
    assert!(store.load().is_none());
    fs::write(&path, "not json").unwrap();
    assert!(store.load().is_none());
}

// -- verification and board helpers ----------------------------------------------

#[test]
fn test_base_url_env_override() {
    // `base_url()` reads the environment on every call, so the override
    // is checked through the same path the dev workflow uses. The lock is
    // what keeps this out of the online-hub tests' variable: it is one
    // process-global environment and four tests write this name.
    let _env = freight_fate::app::testing::env_lock();
    std::env::set_var("FREIGHT_FATE_ONLINE_URL", "http://localhost:3000/");
    let url = base_url();
    std::env::remove_var("FREIGHT_FATE_ONLINE_URL");
    assert_eq!(url, "http://localhost:3000");
}

#[test]
fn test_bypass_header_absent_without_the_env_var() {
    let headers = request_headers(&[], None);
    assert!(header(&headers, "x-vercel-protection-bypass").is_none());
    assert_eq!(header(&headers, "Content-Type"), Some("application/json"));
}

#[test]
fn test_bypass_header_present_when_the_env_var_is_set() {
    let headers = request_headers(&[], Some("secret-bypass-token"));
    assert_eq!(
        header(&headers, "X-vercel-protection-bypass"),
        Some("secret-bypass-token")
    );
}

#[test]
fn test_verify_identity_ok_posts_an_off_duty_signoff() {
    let transport = FakeTransport::replying(json!({"ok": true, "cleared": true}));
    assert_eq!(verify_identity(&identity(), transport.as_ref()), "ok");
    let request = &transport.requests()[0];
    assert!(request.url.ends_with("/api/freight-fate/presence"));
    // Empty activity means "off duty": validating never puts us on the board.
    assert_eq!(
        request.payload,
        Some(json!({"driverId": identity().driver_id, "activity": "", "detail": ""}))
    );
    assert_eq!(
        request.header("Authorization"),
        Some(format!("Bearer {}", identity().driver_token).as_str())
    );
}

#[test]
fn test_verify_identity_maps_the_failure_modes() {
    let verify = |transport: Arc<FakeTransport>| verify_identity(&identity(), transport.as_ref());
    assert_eq!(
        verify(FakeTransport::failing(NetError::http(404))),
        "driver_not_found"
    );
    assert_eq!(
        verify(FakeTransport::failing(NetError::http(401))),
        "unauthorized"
    );
    // Other 4xx codes mean the server answered and refused the credentials
    // (issue 63: a malformed paste came back as HTTP 400), which must not be
    // reported to the player as a connection problem.
    assert_eq!(
        verify(FakeTransport::failing(NetError::http(400))),
        "rejected"
    );
    assert_eq!(
        verify(FakeTransport::failing(NetError::http(422))),
        "rejected"
    );
    assert_eq!(verify(FakeTransport::failing(NetError::http(500))), "error");
    assert_eq!(
        verify(FakeTransport::failing(NetError::other("OSError", ""))),
        "error"
    );
    assert_eq!(
        verify(FakeTransport::replying(json!({"ok": false}))),
        "error"
    );
}

#[test]
fn test_fetch_board_returns_drivers_or_none() {
    let drivers =
        json!([{"displayName": "Road Star", "activity": "Driving", "detail": "", "updatedAt": 1}]);
    assert_eq!(
        fetch_board(FakeTransport::replying(json!({"drivers": drivers})).as_ref()),
        Some(drivers.as_array().unwrap().clone())
    );
    assert!(fetch_board(FakeTransport::replying(json!({})).as_ref()).is_none());
    assert!(fetch_board(FakeTransport::failing(NetError::other("OSError", "")).as_ref()).is_none());
}

#[test]
fn test_fetch_mastodon_status_reads_the_link_state() {
    let transport =
        FakeTransport::replying(json!({"ok": true, "linked": true, "handle": "@rig@m.social"}));
    assert_eq!(
        fetch_mastodon_status(&identity(), transport.as_ref()),
        Some(MastodonStatus {
            linked: true,
            handle: "@rig@m.social".to_string()
        })
    );
    assert!(transport.requests()[0]
        .url
        .ends_with("/api/freight-fate/mastodon/status?driverId=driver-testtest"));
    let linked_no_handle =
        FakeTransport::replying(json!({"ok": true, "linked": true, "handle": null}));
    assert_eq!(
        fetch_mastodon_status(&identity(), linked_no_handle.as_ref()),
        Some(MastodonStatus {
            linked: true,
            handle: String::new()
        })
    );
    assert!(fetch_mastodon_status(
        &identity(),
        FakeTransport::replying(json!({"ok": false})).as_ref()
    )
    .is_none());
    assert!(fetch_mastodon_status(
        &identity(),
        FakeTransport::failing(NetError::other("OSError", "")).as_ref()
    )
    .is_none());
}

// -- build identity reporting --------------------------------------------------

#[test]
fn test_client_version_reports_source_checkout_without_a_build_stamp() {
    // Tests run from a source checkout, so there is no build_info.json and
    // the reported identity must be the source form, not a bogus stable tag.
    assert_eq!(client_version(), format!("source-{PACKAGE_VERSION}"));
}

#[test]
fn test_client_version_reports_the_packaged_build_tag() {
    assert_eq!(
        client_version_for(
            Some(&BuildInfo::new("nightly-20260711", "dev", "")),
            PACKAGE_VERSION
        ),
        "nightly-20260711"
    );

    // A mangled stamp must not be able to break the request header: spaces
    // and control characters are dropped rather than sent.
    assert_eq!(
        client_version_for(
            Some(&BuildInfo::new("bad tag\n", "dev", "")),
            PACKAGE_VERSION
        ),
        "badtag"
    );
}

#[test]
fn test_default_transport_stamps_the_build_in_the_user_agent() {
    let headers = request_headers(&[], None);
    assert_eq!(
        header(&headers, "User-agent"),
        Some(format!("FreightFate/{}", client_version()).as_str())
    );
}

/// A secret store that never answers -- the Mac Keychain ACL prompt shape.
struct HungStore;

impl SecretStore for HungStore {
    fn set_password(&self, _: &str, _: &str, _: &str) -> Result<(), String> {
        loop {
            std::thread::park();
        }
    }

    fn get_password(&self, _: &str, _: &str) -> Result<Option<String>, String> {
        loop {
            std::thread::park();
        }
    }

    fn delete_password(&self, _: &str, _: &str) -> Result<(), String> {
        loop {
            std::thread::park();
        }
    }
}

#[test]
fn test_a_hung_secret_store_times_out_so_launch_can_continue() {
    let _ = take_secret_store_timeout_notice();
    let dir = tempfile::tempdir().unwrap();
    let token = "s".repeat(68);
    fs::write(
        dir.path().join("online.json"),
        json!({"driver_id": "road-star-abcd1234"}).to_string(),
    )
    .unwrap();
    // Plaintext fallback so load can still succeed after the keychain times out.
    fs::write(dir.path().join("online.token"), &token).unwrap();

    let store = IdentityStore::new(dir.path(), Some(Arc::new(HungStore)))
        .with_secret_timeout(std::time::Duration::from_millis(80));
    let started = std::time::Instant::now();
    let loaded = store.load();
    let elapsed = started.elapsed();

    assert!(
        elapsed < std::time::Duration::from_secs(2),
        "hung keychain waited {elapsed:?}"
    );
    assert_eq!(
        loaded,
        Some(OnlineIdentity::new("road-star-abcd1234", &token))
    );
    assert!(
        take_secret_store_timeout_notice(),
        "the title screen needs a spoken notice after a keychain timeout"
    );
}
