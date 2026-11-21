use super::*;

fn remember_local(service: &CloudSaves) {
    let (_, hash) = cloud_content(&profile("Road Star", 4500.0));
    service.sync_state().record_synced_for_driver(
        &cloud_slot_name("Road Star"),
        1,
        &hash,
        &identity().driver_id,
    );
}

#[test]
fn a_cached_hash_cannot_hide_another_devices_newer_revision() {
    let transport = FakeTransport::replying(json!({
        "saves": [
            {"saveName": "Other Career", "revision": 99},
            {"saveName": cloud_slot_name("Road Star"), "revision": 1},
            {"saveName": cloud_slot_name("Road Star"), "revision": 2,
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
        service
            .outcome_for(&cloud_slot_name("Road Star"), token)
            .as_deref(),
        Some("stale")
    );
    assert!(
        transport.posts().is_empty(),
        "a stale copy must not be uploaded"
    );
    assert_eq!(transport.request_count(), 1);
    assert_eq!(
        service.sync_state().slot(&cloud_slot_name("Road Star"))["revision"],
        1
    );
    assert_eq!(
        service.conflicts()[&cloud_slot_name("Road Star")]["latestRevision"],
        2
    );
    assert_eq!(
        service.conflicts()[&cloud_slot_name("Road Star")]["latestSummary"],
        "Road Star, level 1, 4,000 dollars"
    );
    assert_eq!(
        service.status(),
        freight_fate::cloud_saves::STALE_BACKUP_STATUS
    );
}

#[test]
fn a_recorded_stale_copy_becomes_a_real_conflict_after_local_changes() {
    let transport = FakeTransport::replying(json!({
        "saves": [{"saveName": cloud_slot_name("Road Star"), "revision": 2}]
    }));
    let service = make_service(&transport, &ManualClock::new());
    remember_local(&service);

    for (money, expected) in [(4500.0, "stale"), (4500.0, "stale"), (4600.0, "conflict")] {
        let token = service
            .backup_now("Road Star", profile("Road Star", money))
            .unwrap();
        assert_eq!(
            service
                .outcome_for(&cloud_slot_name("Road Star"), token)
                .as_deref(),
            Some(expected)
        );
        if expected == "stale" {
            assert_eq!(
                service.status(),
                freight_fate::cloud_saves::STALE_BACKUP_STATUS
            );
        } else {
            assert_eq!(
                service.status(),
                conflict_status(&cloud_slot_name("Road Star"))
            );
        }
    }
    assert!(transport.posts().is_empty());
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
        service
            .outcome_for(&cloud_slot_name("Road Star"), token)
            .as_deref(),
        Some("accepted")
    );
    assert_eq!(transport.posts().len(), 1);
    assert_eq!(transport.posts()[0]["parentRevision"], Value::Null);
}

#[test]
fn an_unreachable_or_unverifiable_cloud_cannot_be_called_a_match() {
    for reply in [
        json!({}),
        json!({"saves": [{"saveName": cloud_slot_name("Road Star")}]}),
    ] {
        let transport = FakeTransport::replying(reply);
        let clock = ManualClock::new();
        let service = make_service(&transport, &clock);
        remember_local(&service);

        let token = service
            .backup_now("Road Star", profile("Road Star", 4500.0))
            .unwrap();

        assert_eq!(
            service
                .outcome_for(&cloud_slot_name("Road Star"), token)
                .as_deref(),
            Some("network")
        );
        assert!(transport.posts().is_empty());
        transport.set_reply(Some(
            json!({"saves": [{"saveName": cloud_slot_name("Road Star"), "revision": 1}]}),
        ));
        clock.advance(RETRY_INTERVAL_S + 0.1);
        service.pump(false);
        assert_eq!(
            service
                .outcome_for(&cloud_slot_name("Road Star"), token)
                .as_deref(),
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
            service
                .outcome_for(&cloud_slot_name("Road Star"), token)
                .as_deref(),
            Some(outcome)
        );
        assert!(transport.posts().is_empty());
        assert_eq!(
            service.sync_state().slot(&cloud_slot_name("Road Star"))["revision"],
            1
        );
    }
}

#[test]
fn the_manual_menu_distinguishes_stale_copies_from_two_sided_conflicts() {
    for stale in [true, false] {
        let mut app = TestApp::new();
        let _identity = save_identity(&app);
        let mut local = Profile::named("Road Star");
        local.set_money(if stale { 4500.0 } else { 4600.0 });
        local.save().unwrap();
        let local = Profile::load(&local.path()).unwrap();
        let local_bytes = fs::read(local.path()).unwrap();
        let mut synced = Value::Object(local.to_dict());
        synced["money"] = json!(4500.0);
        let (_, hash) = cloud_content(&synced);
        let entry = json!({"saveName": cloud_slot_name("Road Star"), "revision": 2,
        "summary": "Road Star, level 1, 4,000 dollars"});
        let transport = if stale {
            FakeTransport::replying(json!({"saves": [entry.clone()]}))
        } else {
            FakeTransport::failing(conflict_error(Some(2)))
        };
        let service = install_cloud(&mut app, transport.clone(), true);
        service.sync_state().record_synced_for_driver(
            &cloud_slot_name("Road Star"),
            1,
            &hash,
            &identity().driver_id,
        );
        let mut slot = CloudSlotState::new(
            &mut app.ctx,
            &cloud_slot_name("Road Star"),
            vec![entry],
            None,
            None,
        );
        slot.threaded = false;
        let slot = push(&mut app, slot);
        app.clear_speech();

        with_state::<CloudSlotState, _>(&slot, |state| {
            state.start_backup_now(&mut app.ctx);
            State::update(state, &mut app.ctx, 0.0);
        });

        let lines = app.main_lines();
        let expected = if stale {
            "The cloud has a newer copy of this career. Nothing was changed. Choose Use the cloud copy on this computer to pick it up."
        } else {
            "The cloud copy changed on another computer, so nothing was changed. Choose which copy to keep."
        };
        let heard = lines
            .iter()
            .find(|line| line.as_str() == expected)
            .unwrap_or_else(|| panic!("{lines:?}"));
        println!(
            "{}: {heard}",
            if stale {
                "Stale local copy"
            } else {
                "Two-sided conflict"
            }
        );
        let status = with_state::<CloudSlotState, _>(&slot, |state| state.status.clone());
        if stale {
            assert_eq!(status, expected);
            assert_eq!(service.status(), expected);
            let labels = menu_labels::<CloudSlotState>(&slot, &app.ctx);
            assert!(
                labels
                    .iter()
                    .any(|label| label.starts_with("Use the cloud copy on this computer")),
                "{labels:?}"
            );
            assert!(
                !labels
                    .iter()
                    .any(|label| label.starts_with("Restore the latest backup")),
                "{labels:?}"
            );
        } else {
            assert_eq!(
                status,
                "The cloud copy changed on another computer. Nothing was changed."
            );
            assert_eq!(
                service.status(),
                conflict_status(&cloud_slot_name("Road Star"))
            );
            assert!(
                !lines
                    .iter()
                    .any(|line| line.contains("Choose Use the cloud copy on this computer")),
                "{lines:?}"
            );
        }
        assert!(
            !lines
                .iter()
                .any(|line| line.contains("matches this computer")),
            "{lines:?}"
        );
        assert_eq!(transport.posts().len(), usize::from(!stale));
        assert_eq!(Profile::load(&local.path()).unwrap().money(), local.money());
        assert_eq!(fs::read(local.path()).unwrap(), local_bytes);
    }
}

#[test]
fn the_terminal_handles_the_stale_outcome_without_calling_it_a_real_conflict() {
    let mut app = TestApp::new();
    let clock = ManualClock::new();
    let transport = FakeTransport::revisions();
    let service = make_service(&transport, &clock);
    let mut menu = make_terminal_menu(&mut app, (*service).clone());
    menu.save(&mut app.ctx);
    State::update(&mut menu, &mut app.ctx, 0.0);
    transport.set_reply(Some(
        json!({"saves": [{"saveName": cloud_slot_name("Road Star"), "revision": 2}]}),
    ));
    app.clear_speech();

    menu.save(&mut app.ctx);
    State::update(&mut menu, &mut app.ctx, 0.0);

    assert_eq!(
        last_line(&app),
        freight_fate::cloud_saves::STALE_BACKUP_STATUS
    );
    assert_eq!(transport.posts().len(), 1);
}
