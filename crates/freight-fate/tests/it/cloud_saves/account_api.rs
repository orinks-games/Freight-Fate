use super::*;

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

#[test]
fn test_upload_rejected_for_impossible_money_names_the_career_and_offers_appeal() {
    let status = status_after_rejection("Road Star", "impossible_money");
    assert!(status.starts_with("Road Star: backup not accepted."));
    assert!(status.contains("flagged it for review"));
    assert!(status.contains("Report a problem"));
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
    assert!(status.contains("needs an update"));
    assert!(!status.contains("flagged"));
}

#[test]
fn test_upload_rejected_for_unsupported_version_speaks_the_same_schema_story() {
    assert!(status_after_rejection("Road Star", "unsupported_version").contains("needs an update"));
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
