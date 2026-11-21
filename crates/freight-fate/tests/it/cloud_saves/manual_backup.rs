use super::*;

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
        service
            .outcome_for(&cloud_slot_name("Road Star"), token)
            .as_deref(),
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
        service
            .outcome_for(&cloud_slot_name("Road Star"), token)
            .as_deref(),
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

    transport.set_reply(Some(json!({
        "saves": [{"saveName": cloud_slot_name("Road Star"), "revision": 1}]
    })));

    let token = service
        .backup_now("Road Star", profile("Road Star", 5000.0))
        .unwrap(); // nothing changed since

    assert_eq!(transport.posts().len(), 1); // the content-hash skip still holds
    assert_eq!(transport.request_count(), 2); // upload, then a fresh cloud revision check
    assert_eq!(
        service
            .outcome_for(&cloud_slot_name("Road Star"), token)
            .as_deref(),
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
        service
            .outcome_for(&cloud_slot_name("Road Star"), token)
            .as_deref(),
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
        service
            .outcome_for(&cloud_slot_name("Road Star"), token)
            .as_deref(),
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
        service
            .outcome_for(&cloud_slot_name("Road Star"), token)
            .as_deref(),
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
        service
            .outcome_for(&cloud_slot_name("Road Star"), token)
            .as_deref(),
        Some("conflict")
    );
    assert!(service
        .conflicts()
        .contains_key(&cloud_slot_name("Road Star")));
}

#[test]
fn test_backup_now_respects_a_recorded_conflict() {
    // The conflict guard is unchanged: a live cloud copy still blocks the
    // upload -- but the manual attempt reports it instead of staying silent.
    let transport = RoutedTransport::new(
        json!({"ok": true, "saves": [{"saveName": cloud_slot_name("Road Star"), "revision": 40}]}),
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
        .record_conflict(&cloud_slot_name("Road Star"), &conflict_map(Some(40)));

    let token = service
        .backup_now("Road Star", profile("Road Star", 5000.0))
        .unwrap();

    assert!(transport.posts().is_empty());
    assert_eq!(
        service
            .outcome_for(&cloud_slot_name("Road Star"), token)
            .as_deref(),
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
        service
            .outcome_for(&cloud_slot_name("Road Star"), token)
            .as_deref(),
        Some("accepted")
    );
    // A later attempt's poller never hears the old result.
    assert!(service
        .outcome_for(&cloud_slot_name("Road Star"), token + 1)
        .is_none());
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
        service
            .outcome_for(&cloud_slot_name("Road Star"), token)
            .as_deref(),
        Some("accepted")
    );
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

    transport.set_reply(Some(json!({
        "saves": [{"saveName": cloud_slot_name("Road Star"), "revision": 1}]
    })));

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
