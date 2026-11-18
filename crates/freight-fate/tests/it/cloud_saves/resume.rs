use super::*;
use ff_core::cloud_resume::CheckState;
use freight_fate::states::main_menu::{CareerAction, ConfirmCareerActionState, MainMenuState};
use std::time::Duration;

fn tracked(app: &mut TestApp, transport: SharedTransport) -> (CloudSaves, PathBuf) {
    let mut profile = Profile::named("Road Star");
    profile.set_money(5000.0);
    let path = profile.save().unwrap();
    let service = CloudSaves::new(CloudSavesOptions {
        enabled: true,
        identity: Some(identity()),
        transport,
        threaded: false,
        data_dir: ff_core::models::profile::data_dir(),
        public_keys: Some(test_keys()),
        ..Default::default()
    });
    let local = Profile::load(&path).unwrap();
    let (_, hash) = cloud_content(&Value::Object(local.to_dict()));
    service
        .sync_state()
        .record_synced_for_driver("Road Star", 1, &hash, &identity().driver_id);
    app.ctx.services.cloud = service.clone();
    (service, path)
}

fn newer() -> SharedTransport {
    Arc::new(ClosureTransport(
        |url: &str, _: Option<&Value>, _: &[(String, String)], _: Option<&str>| {
            if url.contains("saveName=") {
                let (profile, signature) = money77_profile();
                Ok(make_cloud_reply(&profile, signature, 2))
            } else {
                Ok(
                    json!({"saves": [{"saveName": "Road Star", "revision": 2, "createdAt": 123, "summary": "Road Star, level 1, 77,000 dollars"}]}),
                )
            }
        },
    ))
}

#[test]
fn off_by_default_never_lists() {
    let mut app = TestApp::new();
    assert!(!app.ctx.settings.cloud_resume);
    let transport = FakeTransport::revisions();
    let (service, _) = tracked(&mut app, transport.clone());
    let mut menu = MainMenuState::new();
    Menu::enter(&mut menu, &mut app.ctx);
    assert_eq!(service.resume_state(), CheckState::Off);
    assert_eq!(transport.request_count(), 0);
}

#[test]
fn delete_and_reset_wait_for_pending_resume() {
    let mut app = TestApp::new();
    let profile = Profile::named("Road Star");
    let path = profile.save().unwrap();
    let before = fs::read(&path).unwrap();
    let (started_tx, started_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let release_rx = Mutex::new(release_rx);
    let service = CloudSaves::new(CloudSavesOptions {
        enabled: true,
        identity: Some(identity()),
        transport: Arc::new(ClosureTransport(
            move |_: &str, _: Option<&Value>, _: &[(String, String)], _: Option<&str>| {
                started_tx.send(()).unwrap();
                release_rx
                    .lock()
                    .unwrap()
                    .recv_timeout(Duration::from_secs(5))
                    .unwrap();
                Ok(json!({"saves": []}))
            },
        )),
        threaded: true,
        data_dir: ff_core::models::profile::data_dir(),
        ..Default::default()
    });
    app.ctx.services.cloud = service.clone();
    service.check_resume(true);
    started_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    for action in [CareerAction::Delete, CareerAction::Reset] {
        let mut confirmation = ConfirmCareerActionState::new(path.clone(), profile.clone(), action);
        Menu::enter(&mut confirmation, &mut app.ctx);
        app.clear_speech();
        Menu::handle_event(
            &mut confirmation,
            &mut app.ctx,
            &InputEvent::key(Key::Return),
        );
        assert_eq!(fs::read(&path).unwrap(), before);
        assert!(app
            .main_lines()
            .iter()
            .any(|line| line == freight_fate::cloud_saves::RESUME_CHECKING));
    }
    release_tx.send(()).unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while service.resume_pending() && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(!service.resume_pending());
}

#[test]
fn clean_newer_cloud_is_verified_installed_and_continued() {
    let mut app = TestApp::new();
    let (service, path) = tracked(&mut app, newer());
    app.ctx.settings.cloud_resume = true;
    let mut menu = MainMenuState::new();
    Menu::enter(&mut menu, &mut app.ctx);
    Menu::update(&mut menu, &mut app.ctx, 0.0);
    assert_eq!(
        service.resume_state(),
        CheckState::Done {
            pulled: 1,
            conflicts: 0
        }
    );
    assert_eq!(Profile::load(&path).unwrap().money(), 77000.0);
    assert!(path.with_extension("ffsave.bak").exists());
    assert_eq!(service.sync_state().slot("Road Star")["revision"], json!(2));
    assert!(app
        .main_lines()
        .iter()
        .any(|line| line == "Picked up Road Star where you left off on another device."));
    let continue_row = menu
        .menu()
        .items
        .iter()
        .position(|item| item.text(&menu, &app.ctx).starts_with("Continue"))
        .unwrap();
    menu.menu_mut().index = continue_row;
    // The menu action must read the replacement, not the profile scanned before the check.
    Menu::handle_event(&mut menu, &mut app.ctx, &InputEvent::key(Key::Return));
    assert_eq!(app.ctx.profile.as_ref().unwrap().money(), 77000.0);
}

#[test]
fn another_account_and_legacy_sync_records_are_not_tracked() {
    for legacy in [false, true] {
        let mut app = TestApp::new();
        let transport =
            FakeTransport::replying(json!({"saves": [{"saveName": "Road Star", "revision": 2}]}));
        let (service, path) = tracked(&mut app, transport.clone());
        if legacy {
            let slot = service.sync_state().slot("Road Star");
            service
                .sync_state()
                .record_synced("Road Star", 1, slot["hash"].as_str().unwrap());
        } else {
            service.set_identity(Some(OnlineIdentity::new("another-driver", "another-token")));
        }
        let before = fs::read(&path).unwrap();
        service.check_resume(true);
        assert_eq!(
            service.resume_state(),
            CheckState::Done {
                pulled: 0,
                conflicts: 0
            }
        );
        assert_eq!(transport.request_count(), 1);
        assert_eq!(fs::read(path).unwrap(), before);
        assert!(service.conflicts().is_empty());
    }
}

#[test]
fn normalized_resume_stays_clean_for_the_next_revision() {
    let mut app = TestApp::new();
    let revision = Arc::new(std::sync::atomic::AtomicI64::new(2));
    let remote_revision = revision.clone();
    let transport = Arc::new(ClosureTransport(
        move |url: &str, _: Option<&Value>, _: &[(String, String)], _: Option<&str>| {
            let revision = remote_revision.load(std::sync::atomic::Ordering::SeqCst);
            if url.contains("saveName=") {
                let (profile, signature) = money77_profile();
                let mut reply = make_cloud_reply(&profile, signature, revision);
                reply["saveName"] = json!(save_slot_name("Road Star"));
                Ok(reply)
            } else {
                Ok(
                    json!({"saves": [{"saveName": save_slot_name("Road Star"), "revision": revision}]}),
                )
            }
        },
    ));
    let (service, path) = tracked(&mut app, transport);
    service.check_resume(true);
    assert_eq!(
        service.resume_state(),
        CheckState::Done {
            pulled: 1,
            conflicts: 0
        }
    );
    let installed = Profile::load(&path).unwrap();
    let (_, installed_hash) = cloud_content(&Value::Object(installed.to_dict()));
    let (_, downloaded_hash) = cloud_content(&money77_profile().0);
    assert_ne!(installed_hash, downloaded_hash);
    assert_eq!(
        service.sync_state().slot(&save_slot_name("Road Star"))["hash"],
        installed_hash
    );
    revision.store(3, std::sync::atomic::Ordering::SeqCst);
    service.check_resume(true);
    assert_eq!(
        service.resume_state(),
        CheckState::Done {
            pulled: 1,
            conflicts: 0
        }
    );
    assert_eq!(
        service.sync_state().slot(&save_slot_name("Road Star"))["revision"],
        3
    );
    assert!(service.conflicts().is_empty());
}

#[test]
fn continue_uses_the_most_recent_remote_career_after_multiple_pickups() {
    let mut app = TestApp::new();
    let transport = Arc::new(ClosureTransport(
        |url: &str, _: Option<&Value>, _: &[(String, String)], _: Option<&str>| {
            for (name, signature) in [
                ("Recent Driver", "2jyPu1Pi2yhN76JT9IwBiR5a2VGDWaB94dc8479ebuVdJPCWbJyAmDquw2TKlVelm7aVuXg/4/Os24oAGKCqAw=="),
                ("Older Driver", "4aasPJ7yxa67rGo6PTUrTt2UDa6vK0aEVoiLfJ8YvNazfTscn6fupDiJfo3hAfya3XYPHGWMybpZeWSHXSBGBA=="),
            ] {
                if url.contains("saveName=") && url.contains(&url_quote(&save_slot_name(name))) {
                    let profile = json!({"name": name, "money": 77000.0, "version": 7});
                    let mut reply = make_cloud_reply(&profile, signature, 2);
                    reply["saveName"] = json!(save_slot_name(name));
                    return Ok(reply);
                }
            }
            Ok(json!({"saves": [
                {"saveName": save_slot_name("Recent Driver"), "revision": 2, "createdAt": 200},
                {"saveName": save_slot_name("Older Driver"), "revision": 2, "createdAt": 100}
            ]}))
        },
    ));
    let (service, unlisted_path) = tracked(&mut app, transport);
    fs::remove_file(unlisted_path).unwrap();
    for (name, local_time) in [("Recent Driver", 200), ("Older Driver", 100)] {
        let profile = Profile::named(name);
        let path = profile.save().unwrap();
        let local = Profile::load(&path).unwrap();
        let (_, hash) = cloud_content(&Value::Object(local.to_dict()));
        service.sync_state().record_synced_for_driver(
            &save_slot_name(name),
            1,
            &hash,
            &identity().driver_id,
        );
        fs::File::open(&path)
            .unwrap()
            .set_times(
                fs::FileTimes::new()
                    .set_modified(std::time::UNIX_EPOCH + Duration::from_secs(local_time)),
            )
            .unwrap();
    }
    app.ctx.settings.cloud_resume = true;
    let mut menu = MainMenuState::new();
    Menu::enter(&mut menu, &mut app.ctx);
    Menu::update(&mut menu, &mut app.ctx, 0.0);
    assert_eq!(
        service.resume_state(),
        CheckState::Done {
            pulled: 2,
            conflicts: 0
        }
    );
    let continue_row = menu
        .menu()
        .items
        .iter()
        .position(|item| item.text(&menu, &app.ctx).starts_with("Continue"))
        .unwrap();
    menu.menu_mut().index = continue_row;
    Menu::handle_event(&mut menu, &mut app.ctx, &InputEvent::key(Key::Return));
    assert_eq!(app.ctx.profile.as_ref().unwrap().name, "Recent Driver");
}

#[test]
fn older_pickup_preserves_continue_order_and_invalid_times_are_ignored() {
    for (created_at, valid) in [
        (json!(1_700_000_000_000i64), true),
        (json!("1700000000000"), true),
        (Value::Null, false),
        (json!("invalid"), false),
        (json!(9_000_000_000_000i64), false),
    ] {
        let mut app = TestApp::new();
        let transport = Arc::new(ClosureTransport(
            move |url: &str, _: Option<&Value>, _: &[(String, String)], _: Option<&str>| {
                if url.contains("saveName=") {
                    let (profile, signature) = money77_profile();
                    let mut reply = make_cloud_reply(&profile, signature, 2);
                    reply["saveName"] = json!(save_slot_name("Road Star"));
                    Ok(reply)
                } else {
                    Ok(json!({"saves": [
                        {"saveName": save_slot_name("Road Star"), "revision": 2, "createdAt": created_at},
                        {"saveName": save_slot_name("Recent local"), "revision": 1}
                    ]}))
                }
            },
        ));
        let (service, path) = tracked(&mut app, transport);
        let recent = Profile::named("Recent local");
        let recent_path = recent.save().unwrap();
        let recent = Profile::load(&recent_path).unwrap();
        let (_, hash) = cloud_content(&Value::Object(recent.to_dict()));
        service.sync_state().record_synced_for_driver(
            &save_slot_name("Recent local"),
            1,
            &hash,
            &identity().driver_id,
        );
        fs::File::options()
            .write(true)
            .open(&recent_path)
            .unwrap()
            .set_modified(std::time::UNIX_EPOCH + Duration::from_millis(1_700_100_000_000))
            .unwrap();
        let before = std::time::SystemTime::now();
        app.ctx.settings.cloud_resume = true;
        let mut menu = MainMenuState::new();
        Menu::enter(&mut menu, &mut app.ctx);
        Menu::update(&mut menu, &mut app.ctx, 0.0);
        assert_eq!(
            service.resume_state(),
            CheckState::Done {
                pulled: 1,
                conflicts: 0
            }
        );
        let modified = fs::metadata(path).unwrap().modified().unwrap();
        if valid {
            assert_eq!(
                modified,
                std::time::UNIX_EPOCH + Duration::from_millis(1_700_000_000_000)
            );
        } else {
            assert!(modified >= before - Duration::from_secs(1));
        }
        let continue_row = menu
            .menu()
            .items
            .iter()
            .position(|item| item.text(&menu, &app.ctx).starts_with("Continue"))
            .unwrap();
        menu.menu_mut().index = continue_row;
        Menu::handle_event(&mut menu, &mut app.ctx, &InputEvent::key(Key::Return));
        assert_eq!(
            app.ctx.profile.as_ref().unwrap().name,
            if valid { "Recent local" } else { "Road Star" }
        );
    }
}

#[test]
fn unsupported_backup_does_not_stop_a_compatible_career_resuming() {
    let mut app = TestApp::new();
    let transport = Arc::new(ClosureTransport(
        |url: &str, _: Option<&Value>, _: &[(String, String)], _: Option<&str>| {
            if url.contains("saveName=") {
                if url.contains(&url_quote(&save_slot_name("Unsupported Driver"))) {
                    let profile = json!({"name": "Unsupported Driver", "money": 77000.0, "version": 7, "created_line": "99.0"});
                    let mut reply = make_cloud_reply(&profile, "e+6E6dmvfZHmSUtWaTrao0wPxHOSMdS18/eVVsR9dCNuZmfHQmnoXYxL8FkntU4AiTIilL13ep4HO3BVr69KCw==", 2);
                    reply["saveName"] = json!(save_slot_name("Unsupported Driver"));
                    return Ok(reply);
                }
                let (profile, signature) = money77_profile();
                let mut reply = make_cloud_reply(&profile, signature, 2);
                reply["saveName"] = json!(save_slot_name("Road Star"));
                return Ok(reply);
            }
            Ok(json!({"saves": [
                {"saveName": save_slot_name("Unsupported Driver"), "revision": 2, "createdAt": 100},
                {"saveName": save_slot_name("Road Star"), "revision": 2, "createdAt": 200}
            ]}))
        },
    ));
    let (service, path) = tracked(&mut app, transport);
    let unsupported = Profile::named("Unsupported Driver");
    let unsupported_path = unsupported.save().unwrap();
    let unsupported = Profile::load(&unsupported_path).unwrap();
    let (_, hash) = cloud_content(&Value::Object(unsupported.to_dict()));
    service.sync_state().record_synced_for_driver(
        &save_slot_name("Unsupported Driver"),
        1,
        &hash,
        &identity().driver_id,
    );
    let before = fs::read(&unsupported_path).unwrap();
    service.check_resume(true);
    assert_eq!(
        service.resume_state(),
        CheckState::Done {
            pulled: 1,
            conflicts: 0
        }
    );
    assert_eq!(fs::read(unsupported_path).unwrap(), before);
    assert_eq!(
        service
            .sync_state()
            .slot(&save_slot_name("Unsupported Driver"))["revision"],
        json!(1)
    );
    assert_eq!(Profile::load(&path).unwrap().money(), 77000.0);
    assert_eq!(
        service.take_resume_lines(),
        ["Picked up Road Star where you left off on another device."]
    );
}

#[test]
fn dirty_local_records_the_existing_conflict_choice() {
    let mut app = TestApp::new();
    let (service, path) = tracked(&mut app, newer());
    let mut local = Profile::load(&path).unwrap();
    local.set_money(12345.0);
    local.save().unwrap();
    service.check_resume(true);
    assert_eq!(
        service.resume_state(),
        CheckState::Done {
            pulled: 0,
            conflicts: 1
        }
    );
    assert_eq!(Profile::load(&path).unwrap().money(), 12345.0);
    assert_eq!(service.conflicts()["Road Star"]["latestRevision"], json!(2));
    assert_eq!(
        service.take_resume_lines(),
        ["Road Star changed on another device. Open Cloud backup to choose which copy to keep."]
    );
}

#[test]
fn same_revision_empty_cloud_and_untracked_careers_stay_local() {
    for (reply, tracked_slot) in [
        (
            json!({"saves": [{"saveName": "Road Star", "revision": 1}]}),
            true,
        ),
        (json!({"saves": []}), true),
        (
            json!({"saves": [{"saveName": "Road Star", "revision": 2}]}),
            false,
        ),
    ] {
        let mut app = TestApp::new();
        let transport = FakeTransport::replying(reply);
        let (service, path) = tracked(&mut app, transport.clone());
        if !tracked_slot {
            service.sync_state().forget("Road Star");
        }
        let before = fs::read(&path).unwrap();
        service.check_resume(true);
        assert_eq!(
            service.resume_state(),
            CheckState::Done {
                pulled: 0,
                conflicts: 0
            }
        );
        assert_eq!(fs::read(path).unwrap(), before);
        assert_eq!(transport.request_count(), 1);
        assert!(service.take_resume_lines().is_empty());
    }
}

#[test]
fn auth_network_and_bad_signature_leave_disk_and_sync_untouched() {
    let bad_download = Arc::new(ClosureTransport(
        |url: &str, _: Option<&Value>, _: &[(String, String)], _: Option<&str>| {
            if url.contains("saveName=") {
                Ok(json!({"content": "broken"}))
            } else {
                Ok(json!({"saves": [{"saveName": "Road Star", "revision": 2}]}))
            }
        },
    ));
    let transports: Vec<(SharedTransport, CheckState)> = vec![
        (
            FakeTransport::failing(NetError::http(401)),
            CheckState::Failed,
        ),
        (
            FakeTransport::failing(NetError::Connection("offline".into())),
            CheckState::Failed,
        ),
        (
            bad_download,
            CheckState::Done {
                pulled: 0,
                conflicts: 0,
            },
        ),
    ];
    for (transport, expected) in transports {
        let mut app = TestApp::new();
        let (service, path) = tracked(&mut app, transport);
        let before = fs::read(&path).unwrap();
        let sync = service.sync_state().slot("Road Star");
        service.check_resume(true);
        assert_eq!(service.resume_state(), expected);
        assert_eq!(fs::read(path).unwrap(), before);
        assert_eq!(service.sync_state().slot("Road Star"), sync);
        assert!(service.take_resume_lines().is_empty());
    }
}

#[test]
fn online_toggle_is_hidden_without_backup_and_saves_independently() {
    let mut app = TestApp::new();
    let _identity = save_identity(&app);
    let mut hub = OnlineHubState::new(&mut app.ctx);
    Menu::enter(&mut hub, &mut app.ctx);
    assert!(!hub
        .menu()
        .items
        .iter()
        .any(|item| item.text(&hub, &app.ctx).starts_with("Resume careers")));
    app.ctx.settings.cloud_saves = true;
    hub.refresh(&mut app.ctx, true);
    let index = hub
        .menu()
        .items
        .iter()
        .position(|item| item.text(&hub, &app.ctx).starts_with("Resume careers"))
        .unwrap();
    assert_eq!(index, 11);
    hub.menu_mut().index = index;
    Menu::handle_event(&mut hub, &mut app.ctx, &InputEvent::key(Key::Right));
    assert!(app.ctx.settings.cloud_resume);
    assert!(app.ctx.settings.cloud_saves);
    assert!(app
        .main_lines()
        .iter()
        .any(|line| line.contains("Resume careers from other devices: on")));
    Menu::handle_event(&mut hub, &mut app.ctx, &InputEvent::key(Key::Return));
    assert!(!app.ctx.settings.cloud_resume);
}

#[test]
fn scenario_can_set_cloud_resume() {
    let scenario = freight_fate::playtest::scenario::Scenario::from_json(
        json!({"settings": {"cloud_resume": true}})
            .as_object()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(scenario.settings["cloud_resume"], json!(true));
}

#[test]
fn conditional_resume_row_keeps_left_and_right_on_the_right_toggle() {
    for backup in [false, true] {
        for key in [Key::Left, Key::Right] {
            let mut app = TestApp::new();
            app.ctx.settings.cloud_saves = backup;
            app.ctx.settings.discord_presence = false;
            let mut hub = OnlineHubState::new(&mut app.ctx);
            Menu::enter(&mut hub, &mut app.ctx);
            let resume = hub
                .menu()
                .items
                .iter()
                .position(|item| item.text(&hub, &app.ctx).starts_with("Resume careers"));
            assert_eq!(resume.is_some(), backup);
            let discord = hub
                .menu()
                .items
                .iter()
                .position(|item| item.text(&hub, &app.ctx).starts_with("Discord presence:"))
                .unwrap();
            hub.menu_mut().index = discord;
            Menu::handle_event(&mut hub, &mut app.ctx, &InputEvent::key(key));
            assert!(app.ctx.settings.discord_presence);
            assert!(!app.ctx.settings.cloud_resume);
            assert_eq!(app.ctx.settings.cloud_saves, backup);
            if let Some(index) = resume {
                hub.menu_mut().index = index;
                Menu::handle_event(&mut hub, &mut app.ctx, &InputEvent::key(key));
                assert!(app.ctx.settings.cloud_resume);
                assert!(app.ctx.settings.discord_presence);
                assert!(app.ctx.settings.cloud_saves);
            }
        }
    }
}

#[test]
fn resume_lines_and_online_row_translate_when_spoken() {
    use ff_core::l10n::{Catalog, Translator};

    let catalog = Catalog::from_po("es", r#"
msgid "Picked up {name} where you left off on another device."
msgstr "PICKED {name}."

msgid "{name} changed on another device. Open Cloud backup to choose which copy to keep."
msgstr "CONFLICT {name}."

msgid "Checking careers on your other devices. Try Continue shortly."
msgstr "CHECKING."

msgid "Resume careers from other devices: {on_off}"
msgstr "RESUME {on_off}"

msgid "on"
msgstr "ENABLED"

msgid "Checks for newer progress when you return to the main menu. Restore each career here once first; conflicting saves need your choice in Cloud backup."
msgstr "RESUME HELP."
"#).unwrap();
    let translator = Arc::new(Translator::new(catalog));
    for dirty in [false, true] {
        let mut app = TestApp::new();
        let (service, path) = tracked(&mut app, newer());
        if dirty {
            let mut profile = Profile::load(&path).unwrap();
            profile.set_money(12345.0);
            profile.save().unwrap();
        }
        service.check_resume(true);
        app.ctx.translator = translator.clone();
        let mut menu = MainMenuState::new();
        Menu::update(&mut menu, &mut app.ctx, 0.0);
        let expected = if dirty {
            "CONFLICT Road Star."
        } else {
            "PICKED Road Star."
        };
        assert!(app.main_lines().iter().any(|line| line == expected));
    }
    let mut app = TestApp::new();
    app.ctx.translator = translator;
    app.ctx.say(freight_fate::cloud_saves::RESUME_CHECKING);
    assert!(app.main_lines().iter().any(|line| line == "CHECKING."));
    app.ctx.settings.cloud_saves = true;
    app.ctx.settings.cloud_resume = true;
    let mut hub = OnlineHubState::new(&mut app.ctx);
    Menu::enter(&mut hub, &mut app.ctx);
    let index = hub
        .menu()
        .items
        .iter()
        .position(|item| item.text(&hub, &app.ctx).starts_with("Resume careers"))
        .unwrap();
    hub.menu_mut().index = index;
    Menu::speak_current(&mut hub, &mut app.ctx);
    let help = Menu::current_help(&hub, &app.ctx);
    app.ctx.say(&help);
    assert!(app
        .main_lines()
        .iter()
        .any(|line| line.starts_with("RESUME ENABLED")));
    assert!(app.main_lines().iter().any(|line| line == "RESUME HELP."));
}
