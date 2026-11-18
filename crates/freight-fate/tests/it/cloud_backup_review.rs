//! A career carrying the "changed outside the game" mark is held for the
//! owner's review before it backs up: the game says it is review-aware, stops
//! retrying a held or declined career, speaks the hold once, and clears the
//! mark when the server accepts the career after review.

use std::sync::Arc;

use serde_json::{json, Value};

use ff_core::models::profile::origin::{forget_origin, origin_for, record_origin};
use ff_core::models::profile::Profile;
use freight_fate::app::testing::TestApp;
use freight_fate::cloud_saves::{
    classify_upload_failure, rejection_status, upload_save, CloudSaves, CloudSavesOptions,
    DEBOUNCE_S, RETRY_INTERVAL_S,
};
use freight_fate::net::testing::{FakeTransport, ManualClock};
use freight_fate::net::NetError;
use freight_fate::online_presence::OnlineIdentity;

fn identity() -> OnlineIdentity {
    OnlineIdentity::new("driver-testtest", &"t".repeat(48))
}

fn profile(name: &str, money: f64) -> Value {
    json!({"name": name, "money": money, "version": 7, "career": {"xp": 0.0}})
}

fn service(
    transport: &Arc<FakeTransport>,
    clock: &Arc<ManualClock>,
) -> (CloudSaves, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let service = CloudSaves::new(CloudSavesOptions {
        enabled: true,
        identity: Some(identity()),
        clock: clock.clock(),
        transport: transport.clone(),
        threaded: false,
        data_dir: dir.path().to_path_buf(),
        ..CloudSavesOptions::default()
    });
    (service, dir)
}

fn drain(service: &CloudSaves, clock: &Arc<ManualClock>) {
    clock.advance(DEBOUNCE_S + 0.1);
    service.pump(false);
}

fn upload(transport: &FakeTransport) -> serde_json::Map<String, Value> {
    upload_save(
        &identity(),
        "Road Star",
        &profile("Road Star", 5000.0),
        Some(3),
        "Road Star",
        None,
        transport,
    )
}

#[test]
fn upload_says_it_understands_review() {
    let transport = FakeTransport::replying(json!({"ok": true, "revision": 4}));
    upload(&transport);
    assert_eq!(transport.posts()[0]["reviewAware"], true);
}

#[test]
fn upload_carries_the_clear_flag_only_when_the_server_sets_it() {
    let cleared =
        FakeTransport::replying(json!({"ok": true, "revision": 4, "clearIntegrityFlag": true}));
    assert_eq!(upload(&cleared)["clearIntegrityFlag"], true);

    let not =
        FakeTransport::replying(json!({"ok": true, "revision": 4, "clearIntegrityFlag": false}));
    assert!(!upload(&not).contains_key("clearIntegrityFlag"));
}

#[test]
fn a_declined_career_is_a_refusal_with_its_own_line() {
    assert_eq!(classify_upload_failure(Some("review_declined")), "rejected");
    assert_eq!(
        rejection_status("Road Star", Some("review_declined")),
        "Road Star: backup declined after review. This career no longer backs up to your \
orinks.net account. Your local career is safe."
    );
}

#[test]
fn a_declined_career_speaks_once_and_is_not_retried() {
    let transport = FakeTransport::failing(NetError::http_json(
        403,
        &json!({"error": "review_declined"}),
    ));
    let clock = ManualClock::new();
    let (service, _dir) = service(&transport, &clock);

    service.queue_backup("Road Star", profile("Road Star", 5000.0));
    drain(&service, &clock);
    let lines = service.take_announcements();
    assert_eq!(lines.len(), 1);
    assert!(lines[0].starts_with("Road Star: backup declined after review."));

    // No backoff retry: the snapshot was dropped, not kept for later.
    clock.advance(RETRY_INTERVAL_S * 3.0);
    service.pump(false);
    assert_eq!(transport.request_count(), 1);

    // A second declined save in the same session says nothing.
    service.queue_backup("Road Star", profile("Road Star", 5001.0));
    drain(&service, &clock);
    assert_eq!(transport.request_count(), 2);
    assert!(service.take_announcements().is_empty());
}

#[test]
fn an_accepted_review_clears_the_loaded_careers_mark_silently() {
    let mut app = TestApp::new();
    let mut career = Profile::named_in("Road Star", "Chicago");
    career.integrity_modified = true;
    career.integrity_notice_pending = true;
    app.ctx.profile = Some(career);
    record_origin("Road Star", &"a".repeat(64));
    assert!(origin_for("Road Star").is_some());

    let transport =
        FakeTransport::replying(json!({"ok": true, "revision": 1, "clearIntegrityFlag": true}));
    let clock = ManualClock::new();
    let (service, _dir) = service(&transport, &clock);
    app.ctx.services.cloud = service.clone();
    service.queue_backup("Road Star", profile("Road Star", 5000.0));
    service.queue_backup("Someone Else", profile("Someone Else", 1.0));
    drain(&service, &clock);
    service.take_announcements(); // the ordinary all-clears
    app.clear_speech();

    app.tick(0.0);

    let loaded = app.ctx.profile.as_ref().unwrap();
    assert!(!loaded.integrity_modified);
    assert!(!loaded.integrity_notice_pending);
    let on_disk = Profile::load(&loaded.path()).unwrap();
    assert!(!on_disk.integrity_modified);
    assert!(app.main_lines().is_empty(), "{:?}", app.main_lines());
    assert!(service.take_absolved().is_empty());
    // Accepted: where it arrived from is no longer vouched for.
    assert_eq!(origin_for("Road Star"), None);
}

#[test]
fn a_marked_copy_names_the_backup_it_arrived_as() {
    let saves = tempfile::tempdir().unwrap();
    let previous = ff_core::settings::set_thread_data_dir(Some(saves.path().to_path_buf()));
    let origin = "0123456789abcdef".repeat(4);
    let send = |dict: Value| {
        let transport = FakeTransport::replying(json!({"ok": true, "revision": 4}));
        upload_save(
            &identity(),
            "Road Star",
            &dict,
            Some(3),
            "Road Star",
            None,
            &*transport,
        );
        transport.posts()[0].get("copiedFrom").cloned()
    };
    let mut marked = profile("Road Star", 5000.0);
    marked["integrity_modified"] = json!(true);

    // No record: nothing to name.
    assert_eq!(send(marked.clone()), None);

    record_origin("Road Star", &origin);
    assert_eq!(send(marked.clone()), Some(json!(origin)));
    // Unmarked snapshots and other careers never carry it.
    assert_eq!(send(profile("Road Star", 5000.0)), None);
    let mut other = profile("Other Rig", 5000.0);
    other["integrity_modified"] = json!(true);
    assert_eq!(send(other), None);

    // A record that is not a content hash is never sent.
    record_origin("Road Star", &origin.to_uppercase());
    assert_eq!(send(marked.clone()), None);

    forget_origin("Road Star");
    assert_eq!(send(marked), None);
    ff_core::settings::set_thread_data_dir(previous);
}
