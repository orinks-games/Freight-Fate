//! Manage careers: deleting a career from an earlier version, and what a
//! delete does to the career's cloud backups.

use crate::states_main_menu_support::*;
use crate::states_online_support::{identity, install_cloud, install_identity};
use ff_core::models::profile::{save_path_for, Profile};
use freight_fate::app::testing::TestApp;
use freight_fate::net::testing::FakeTransport;
use freight_fate::states::base::{Key, Menu};
use freight_fate::states::main_menu::{
    CareerActionsState, ConfirmCareerActionState, MainMenuState, ManageCareersState,
};
use serde_json::json;

#[test]
fn a_career_from_an_earlier_version_can_be_deleted_from_manage_careers() {
    let mut app = TestApp::new();
    let path = write_1_8_save("Old Timer");
    app.push_state(MainMenuState::new());
    // Nothing loads, but the old career is still somewhere to clear it from.
    select::<MainMenuState>(&mut app, "Manage careers");
    assert_eq!(
        labels::<ManageCareersState>(&app)[0],
        "Old Timer: career from an earlier version of Freight Fate"
    );
    key(&mut app, Key::Return);
    // It cannot load, so there is nothing to reset: straight to the delete.
    assert!(is::<ConfirmCareerActionState>(&app));
    assert_eq!(
        labels::<ConfirmCareerActionState>(&app),
        ["Yes, delete Old Timer", "No, keep this career"]
    );
    app.clear_speech();
    key(&mut app, Key::Return);
    assert!(!path.exists());
    assert!(is::<MainMenuState>(&app));
    assert!(app.main_lines().iter().any(|l| l == "Old Timer deleted."));
    assert!(!labels::<MainMenuState>(&app)
        .iter()
        .any(|l| l == "Manage careers" || l == "Choose career"));
}

/// Doomed, saved and backed up from this computer, on the delete screen.
fn backed_up_career_on_the_delete_screen(
    app: &mut TestApp,
    transport: &std::sync::Arc<FakeTransport>,
) {
    Profile::named_in("Doomed", "Denver").save().unwrap();
    install_cloud(app, transport.clone(), true)
        .sync_state()
        .record_synced("Doomed", 3, "hash");
    app.push_state(MainMenuState::new());
    select::<MainMenuState>(app, "Manage careers");
    key(app, Key::Return);
    select::<CareerActionsState>(app, "Delete this career");
    assert!(is::<ConfirmCareerActionState>(app));
    with_state_mut::<ConfirmCareerActionState, _>(app, |s, _| s.threaded = false);
}

#[test]
fn deleting_a_backed_up_career_can_remove_its_cloud_backups_too() {
    let mut app = TestApp::new();
    let _guard = install_identity(&app, Some(&identity()));
    let transport = FakeTransport::replying(json!({"ok": true}));
    backed_up_career_on_the_delete_screen(&mut app, &transport);
    assert!(app
        .main_lines()
        .join(" ")
        .contains("choose whether they go too"));
    assert_eq!(
        labels::<ConfirmCareerActionState>(&app),
        [
            "Yes, delete Doomed and its cloud backups",
            "Yes, delete Doomed from this computer only",
            "No, keep this career",
        ]
    );
    app.clear_speech();
    key(&mut app, Key::Return);
    with_state_mut::<ConfirmCareerActionState, _>(&mut app, |s, ctx| Menu::update(s, ctx, 0.0));
    app.ctx.run_deferred();

    let deletes: Vec<_> = transport
        .requests()
        .into_iter()
        .filter(|r| r.method.as_deref() == Some("DELETE"))
        .collect();
    assert_eq!(deletes.len(), 1);
    assert!(deletes[0].url.contains("saveName=Doomed"));
    assert!(!save_path_for("Doomed").exists());
    assert!(is::<MainMenuState>(&app));
    let said = app.main_lines().join(" ");
    assert!(said.contains("Doomed deleted from this computer"), "{said}");
    assert!(
        said.contains("Every cloud backup of it was removed"),
        "{said}"
    );
    assert!(app
        .ctx
        .cloud_saves_service()
        .sync_state()
        .slot("Doomed")
        .is_empty());
}

#[test]
fn deleting_from_this_computer_only_keeps_the_backups_and_frees_the_name() {
    let mut app = TestApp::new();
    let _guard = install_identity(&app, Some(&identity()));
    let transport = FakeTransport::replying(json!({"ok": true}));
    backed_up_career_on_the_delete_screen(&mut app, &transport);
    select::<ConfirmCareerActionState>(&mut app, "Yes, delete Doomed from this computer only");

    assert_eq!(transport.request_count(), 0);
    assert!(is::<MainMenuState>(&app));
    let said = app.main_lines().join(" ");
    assert!(said.contains("Its cloud backups were kept"), "{said}");
    // A new career named Doomed must not upload as the next revision of the
    // deleted one's backups.
    assert!(app
        .ctx
        .cloud_saves_service()
        .sync_state()
        .slot("Doomed")
        .is_empty());
}

/// Jerry, 2026-09-21: a career said it was deleted and stayed. The save's
/// file name need not match the career name (a copied or renamed file), and
/// the delete aimed at the file the name implies instead of the one loaded.
#[test]
fn deleting_removes_the_file_the_career_was_loaded_from() {
    let mut app = TestApp::new();
    let saved = Profile::named_in("Tiger", "Denver").save().unwrap();
    let renamed = saved.with_file_name("Tiger backup.ffsave");
    std::fs::rename(&saved, &renamed).unwrap();
    app.push_state(MainMenuState::new());
    select::<MainMenuState>(&mut app, "Manage careers");
    assert!(labels::<ManageCareersState>(&app)[0].starts_with("Tiger: level 1"));
    key(&mut app, Key::Return);
    select::<CareerActionsState>(&mut app, "Delete this career");
    select::<ConfirmCareerActionState>(&mut app, "Yes, delete Tiger");
    assert!(!renamed.exists());
    assert!(!labels::<MainMenuState>(&app)
        .iter()
        .any(|l| l == "Manage careers"));
}

#[test]
fn resetting_a_career_from_a_renamed_file_leaves_one_career() {
    let mut app = TestApp::new();
    let saved = Profile::named_in("Tiger", "Denver").save().unwrap();
    let renamed = saved.with_file_name("Tiger backup.ffsave");
    std::fs::rename(&saved, &renamed).unwrap();
    app.push_state(MainMenuState::new());
    select::<MainMenuState>(&mut app, "Manage careers");
    key(&mut app, Key::Return);
    select::<CareerActionsState>(&mut app, "Reset this career");
    select::<ConfirmCareerActionState>(&mut app, "Yes, reset Tiger");
    assert!(!renamed.exists());
    assert!(saved.exists());
    select::<MainMenuState>(&mut app, "Manage careers");
    assert_eq!(labels::<ManageCareersState>(&app).len(), 2); // Tiger, Back
}
