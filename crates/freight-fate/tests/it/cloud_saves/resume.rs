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
    service.sync_state().record_synced("Road Star", 1, &hash);
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
    let transports: Vec<SharedTransport> = vec![
        FakeTransport::failing(NetError::http(401)),
        FakeTransport::failing(NetError::Connection("offline".into())),
        bad_download,
    ];
    for transport in transports {
        let mut app = TestApp::new();
        let (service, path) = tracked(&mut app, transport);
        let before = fs::read(&path).unwrap();
        let sync = service.sync_state().slot("Road Star");
        service.check_resume(true);
        assert_eq!(service.resume_state(), CheckState::Failed);
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
