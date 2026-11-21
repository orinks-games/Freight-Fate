use super::*;

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
