use super::*;

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
            &["road star", "flagged it for review", "report a problem"],
        ),
        (
            // Schema/version refusal: named career, blames a build mismatch,
            // never "flagged".
            "invalid_schema",
            rejected_error("invalid_schema"),
            &["road star", "needs an update", "not something you did"],
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
