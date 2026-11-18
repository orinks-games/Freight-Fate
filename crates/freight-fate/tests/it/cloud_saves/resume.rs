use super::*;
use ff_core::cloud_resume::CheckState;
use freight_fate::states::main_menu::MainMenuState;

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
        .record_synced_for_driver(&cloud_slot_name("Road Star"), 1, &hash, &identity().driver_id);
    app.ctx.services.cloud = service.clone();
    (service, path)
}

fn newer() -> SharedTransport {
    Arc::new(ClosureTransport(
        |url: &str, _: Option<&Value>, _: &[(String, String)], _: Option<&str>| {
            if url.contains("saveName=") {
                assert!(url.contains(&url_quote(&cloud_slot_name("Road Star"))));
                let (profile, signature) = money77_profile();
                let mut reply = make_cloud_reply(&profile, signature, 2);
                reply["saveName"] = json!(cloud_slot_name("Road Star"));
                Ok(reply)
            } else {
                Ok(json!({"saves": [
                    {"saveName": "Road Star", "revision": 99},
                    {"saveName": "Road Star (Career 3)", "revision": 100},
                    {"saveName": cloud_slot_name("Road Star"), "revision": 2, "createdAt": 123, "summary": "Road Star, level 1, 77,000 dollars"}
                ]}))
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
    assert_eq!(Profile::load(&path).unwrap().created_line, "2.0");
    assert!(path.with_extension("ffsave.bak").exists());
    assert_eq!(
        service.sync_state().slot(&cloud_slot_name("Road Star"))["revision"],
        json!(2)
    );
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
fn resume_only_tracks_this_lines_release_qualified_slot() {
    assert_eq!(cloud_slot_name("Road Star"), "Road Star (Career 2)");
    for other_slot in ["Road Star", "Road Star (Career 3)"] {
        let mut app = TestApp::new();
        let transport = FakeTransport::replying(json!({"saves": [
            {"saveName": other_slot, "revision": 99}
        ]}));
        let (service, path) = tracked(&mut app, transport.clone());
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
        assert_eq!(fs::read(&path).unwrap(), before);
        assert!(service.conflicts().is_empty());
    }

    let mut app = TestApp::new();
    let transport = FakeTransport::replying(json!({"saves": [
        {"saveName": cloud_slot_name("Road Star"), "revision": 2}
    ]}));
    let (service, path) = tracked(&mut app, transport.clone());
    let hash = service.sync_state().slot(&cloud_slot_name("Road Star"))["hash"]
        .as_str()
        .unwrap()
        .to_string();
    service.sync_state().forget(&cloud_slot_name("Road Star"));
    service.sync_state().record_synced("Road Star", 1, &hash);
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
    assert_eq!(fs::read(&path).unwrap(), before);
    assert_eq!(service.sync_state().slot("Road Star")["revision"], json!(1));
}

#[test]
fn another_account_and_legacy_sync_records_are_not_tracked() {
    for legacy in [false, true] {
        let mut app = TestApp::new();
        let transport =
            FakeTransport::replying(json!({"saves": [{"saveName": cloud_slot_name("Road Star"), "revision": 2}]}));
        let (service, path) = tracked(&mut app, transport.clone());
        if legacy {
            let slot = service.sync_state().slot(&cloud_slot_name("Road Star"));
            service
                .sync_state()
                .record_synced(&cloud_slot_name("Road Star"), 1, slot["hash"].as_str().unwrap());
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
    assert_eq!(
        service.conflicts()[&cloud_slot_name("Road Star")]["latestRevision"],
        json!(2)
    );
    assert_eq!(
        service.take_resume_lines(),
        ["Road Star changed on another device. Open Cloud backup to choose which copy to keep."]
    );
}

#[test]
fn same_revision_empty_cloud_and_untracked_careers_stay_local() {
    for (reply, tracked_slot) in [
        (
            json!({"saves": [{"saveName": cloud_slot_name("Road Star"), "revision": 1}]}),
            true,
        ),
        (json!({"saves": []}), true),
        (
            json!({"saves": [{"saveName": cloud_slot_name("Road Star"), "revision": 2}]}),
            false,
        ),
    ] {
        let mut app = TestApp::new();
        let transport = FakeTransport::replying(reply);
        let (service, path) = tracked(&mut app, transport.clone());
        if !tracked_slot {
            service.sync_state().forget(&cloud_slot_name("Road Star"));
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
                Ok(json!({"saves": [{"saveName": cloud_slot_name("Road Star"), "revision": 2}]}))
            }
        },
    ));
    let transports: Vec<SharedTransport> = vec![
        FakeTransport::failing(NetError::http(401)),
        FakeTransport::failing(NetError::Connection("offline".into())),
        bad_download,
    ];
    for transport in transports {
        let mut app = TestApp::new();
        let (service, path) = tracked(&mut app, transport);
        let before = fs::read(&path).unwrap();
        let sync = service.sync_state().slot(&cloud_slot_name("Road Star"));
        service.check_resume(true);
        assert_eq!(service.resume_state(), CheckState::Failed);
        assert_eq!(fs::read(path).unwrap(), before);
        assert_eq!(
            service.sync_state().slot(&cloud_slot_name("Road Star")),
            sync
        );
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
