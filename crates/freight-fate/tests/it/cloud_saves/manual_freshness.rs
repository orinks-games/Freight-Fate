use super::*;

fn remember_local(service: &CloudSaves) {
    let (_, hash) = cloud_content(&profile("Road Star", 4500.0));
    service
        .sync_state()
        .record_synced_for_driver("Road Star", 1, &hash, &identity().driver_id);
}

#[test]
fn a_cached_hash_cannot_hide_another_devices_newer_revision() {
    let transport = FakeTransport::replying(json!({
        "saves": [
            {"saveName": "Other Career", "revision": 99},
            {"saveName": "Road Star", "revision": 1},
            {"saveName": "Road Star", "revision": 2,
             "createdAt": 1_700_000_000_000i64,
             "summary": "Road Star, level 1, 4,000 dollars"}
        ]
    }));
    let service = make_service(&transport, &ManualClock::new());
    remember_local(&service);

    let token = service
        .backup_now("Road Star", profile("Road Star", 4500.0))
        .unwrap();

    assert_eq!(
        service.outcome_for("Road Star", token).as_deref(),
        Some("conflict")
    );
    assert!(
        transport.posts().is_empty(),
        "a stale copy must not be uploaded"
    );
    assert_eq!(transport.request_count(), 1);
    assert_eq!(service.sync_state().slot("Road Star")["revision"], 1);
    assert_eq!(service.conflicts()["Road Star"]["latestRevision"], 2);
    assert_eq!(
        service.conflicts()["Road Star"]["latestSummary"],
        "Road Star, level 1, 4,000 dollars"
    );
}

#[test]
fn a_deleted_cloud_slot_is_recreated_instead_of_claiming_it_matches() {
    let transport = FakeTransport::replying(json!({"saves": [], "ok": true, "revision": 1}));
    let service = make_service(&transport, &ManualClock::new());
    remember_local(&service);

    let token = service
        .backup_now("Road Star", profile("Road Star", 4500.0))
        .unwrap();

    assert_eq!(
        service.outcome_for("Road Star", token).as_deref(),
        Some("accepted")
    );
    assert_eq!(transport.posts().len(), 1);
    assert_eq!(transport.posts()[0]["parentRevision"], Value::Null);
}

#[test]
fn an_unreachable_or_unverifiable_cloud_cannot_be_called_a_match() {
    for reply in [json!({}), json!({"saves": [{"saveName": "Road Star"}]})] {
        let transport = FakeTransport::replying(reply);
        let clock = ManualClock::new();
        let service = make_service(&transport, &clock);
        remember_local(&service);

        let token = service
            .backup_now("Road Star", profile("Road Star", 4500.0))
            .unwrap();

        assert_eq!(
            service.outcome_for("Road Star", token).as_deref(),
            Some("network")
        );
        assert!(transport.posts().is_empty());
        transport.set_reply(Some(
            json!({"saves": [{"saveName": "Road Star", "revision": 1}]}),
        ));
        clock.advance(RETRY_INTERVAL_S + 0.1);
        service.pump(false);
        assert_eq!(
            service.outcome_for("Road Star", token).as_deref(),
            Some("unchanged")
        );
        assert!(transport.posts().is_empty());
    }
}

#[test]
fn a_failed_freshness_check_reports_auth_or_network_without_uploading() {
    for (error, outcome) in [
        (auth_error(401, None), "auth"),
        (NetError::other("OSError", "offline"), "network"),
    ] {
        let transport = FakeTransport::failing(error);
        let service = make_service(&transport, &ManualClock::new());
        remember_local(&service);

        let token = service
            .backup_now("Road Star", profile("Road Star", 4500.0))
            .unwrap();

        assert_eq!(
            service.outcome_for("Road Star", token).as_deref(),
            Some(outcome)
        );
        assert!(transport.posts().is_empty());
        assert_eq!(service.sync_state().slot("Road Star")["revision"], 1);
    }
}

#[test]
fn the_manual_menu_explains_how_to_restore_the_newer_copy() {
    let mut app = TestApp::new();
    let _identity = save_identity(&app);
    let local = Profile::named("Road Star");
    local.save().unwrap();
    let local = Profile::load(&local.path()).unwrap();
    let (_, hash) = cloud_content(&Value::Object(local.to_dict()));
    let entry = json!({"saveName": "Road Star", "revision": 2,
        "summary": "Road Star, level 1, 4,000 dollars"});
    let transport = FakeTransport::replying(json!({"saves": [entry.clone()]}));
    let service = install_cloud(&mut app, transport.clone(), true);
    service
        .sync_state()
        .record_synced_for_driver("Road Star", 1, &hash, &identity().driver_id);
    let mut slot = CloudSlotState::new(&mut app.ctx, "Road Star", vec![entry], None, None);
    slot.threaded = false;
    let slot = push(&mut app, slot);
    app.clear_speech();

    with_state::<CloudSlotState, _>(&slot, |state| {
        state.start_backup_now(&mut app.ctx);
        State::update(state, &mut app.ctx, 0.0);
    });

    let lines = app.main_lines();
    assert!(lines.iter().any(|line| line == "The cloud has a newer copy of this career. Nothing was changed. Choose Restore the latest backup to use it."), "{lines:?}");
    assert!(
        !lines
            .iter()
            .any(|line| line.contains("matches this computer")),
        "{lines:?}"
    );
    assert!(transport.posts().is_empty());
    assert!(Profile::load(&local.path()).is_ok());
}
