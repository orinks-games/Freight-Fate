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

#[test]
fn resetting_an_older_career_keeps_its_home_city_blank() {
    // A career from before the home city was recorded: a Northstar driver
    // whose home terminal is Chicago. The reset starts over there, and
    // Chicago must not quietly become the driver's home.
    let mut app = TestApp::new();
    let mut old = Profile::named_in("Legacy Home", "milwaukee_wi_us");
    assert_eq!(old.home_terminal_city, "chicago_il_us");
    old.home_city.clear();
    old.save().unwrap();
    app.push_state(MainMenuState::new());
    select::<MainMenuState>(&mut app, "Manage careers");
    key(&mut app, Key::Return);
    select::<CareerActionsState>(&mut app, "Reset this career");
    select::<ConfirmCareerActionState>(&mut app, "Yes, reset Legacy Home");
    let reset = Profile::load(&save_path_for("Legacy Home")).unwrap();
    assert_eq!(reset.home_city, "", "no home city was ever picked");
    assert_eq!(reset.home_terminal_city, "chicago_il_us");
    assert_eq!(reset.current_city, "chicago_il_us");
}

#[test]
fn resetting_a_career_starts_over_from_its_home_city() {
    let mut app = TestApp::new();
    let mut old = Profile::named_in("Picked Home", "chicago_il_us");
    old.home_city = "milwaukee_wi_us".to_string();
    old.save().unwrap();
    app.push_state(MainMenuState::new());
    select::<MainMenuState>(&mut app, "Manage careers");
    key(&mut app, Key::Return);
    select::<CareerActionsState>(&mut app, "Reset this career");
    select::<ConfirmCareerActionState>(&mut app, "Yes, reset Picked Home");
    let reset = Profile::load(&save_path_for("Picked Home")).unwrap();
    assert_eq!(reset.home_city, "milwaukee_wi_us");
    assert_eq!(reset.home_terminal_city, "chicago_il_us");
}

/// A company driver with no carrier, saved: the Fairbanks firing's result.
fn save_no_carrier_career(name: &str, home: &str, current: &str, terminal: &str) {
    let mut old = Profile::named_in(name, "chicago_il_us");
    old.carrier_key.clear();
    old.carrier_name.clear();
    old.business_status = "company_driver".to_string();
    old.home_city = home.to_string();
    old.current_city = current.to_string();
    old.home_terminal_city = terminal.to_string();
    old.parked_facility.clear();
    old.save().unwrap();
}

fn reset_career(app: &mut TestApp, name: &str) -> Profile {
    app.push_state(MainMenuState::new());
    select::<MainMenuState>(app, "Manage careers");
    key(app, Key::Return);
    select::<CareerActionsState>(app, "Reset this career");
    select::<ConfirmCareerActionState>(app, &format!("Yes, reset {name}"));
    Profile::load(&save_path_for(name)).unwrap()
}

fn assert_alaska_start(reset: &Profile) {
    let world = ff_core::data::world::get_world();
    let carrier = ff_core::models::carriers::carrier(&reset.carrier_key)
        .unwrap_or_else(|| panic!("no carrier {}", reset.carrier_key));
    assert!(
        ["fairbanks_ak_us", "anchorage_ak_us"].contains(&reset.home_terminal_city.as_str()),
        "{}",
        reset.home_terminal_city
    );
    assert_eq!(reset.current_city, reset.home_terminal_city);
    assert!(carrier
        .terminal_city_keys
        .iter()
        .all(|t| world.cities[t].state_code == "AK"));
    assert_ne!(reset.carrier_key, "northstar");
}

#[test]
fn resetting_a_fairbanks_career_with_no_carrier_stays_in_alaska() {
    let mut app = TestApp::new();
    save_no_carrier_career(
        "Ak Reset",
        "fairbanks_ak_us",
        "fairbanks_ak_us",
        "fairbanks_ak_us",
    );
    // The career list names no carrier and no truck stop for it.
    app.push_state(MainMenuState::new());
    select::<MainMenuState>(&mut app, "Manage careers");
    let row = labels::<ManageCareersState>(&app)[0].clone();
    assert!(
        row.contains("with no carrier") && row.contains("in Fairbanks"),
        "{row}"
    );
    assert!(
        !row.contains("Sourdough") && !row.contains("Northstar"),
        "{row}"
    );
    key(&mut app, Key::Return);
    select::<CareerActionsState>(&mut app, "Reset this career");
    select::<ConfirmCareerActionState>(&mut app, "Yes, reset Ak Reset");
    let reset = Profile::load(&save_path_for("Ak Reset")).unwrap();
    assert_alaska_start(&reset);
    assert_eq!(reset.home_city, "fairbanks_ak_us");
    let world = ff_core::data::world::get_world();
    assert!(ff_core::models::carriers::carrier(&reset.carrier_key)
        .unwrap()
        .hires_in(world, "fairbanks_ak_us"));
}

#[test]
fn resetting_a_no_carrier_career_with_no_home_city_uses_the_current_city() {
    let mut app = TestApp::new();
    save_no_carrier_career("Ak Blank", "", "anchorage_ak_us", "fairbanks_ak_us");
    let reset = reset_career(&mut app, "Ak Blank");
    assert_alaska_start(&reset);
    assert_eq!(reset.home_city, "", "no home city was ever picked");
    let world = ff_core::data::world::get_world();
    assert!(ff_core::models::carriers::carrier(&reset.carrier_key)
        .unwrap()
        .hires_in(world, "anchorage_ak_us"));
}

#[test]
fn resetting_a_chicago_career_with_no_carrier_takes_a_chicago_hiring_carrier() {
    let mut app = TestApp::new();
    save_no_carrier_career(
        "Il Reset",
        "chicago_il_us",
        "chicago_il_us",
        "chicago_il_us",
    );
    let reset = reset_career(&mut app, "Il Reset");
    let world = ff_core::data::world::get_world();
    let carrier = ff_core::models::carriers::carrier(&reset.carrier_key).expect("a carrier");
    assert!(carrier.hires_in(world, "chicago_il_us"), "{}", carrier.name);
    assert_eq!(reset.home_city, "chicago_il_us");
    assert!(!reset.is_unassigned_company_driver());
}
