//! A desktop run playing as an iPhone (`freight_fate::device`, the agent
//! server's `scenario` `phone: true`): the phone's rows come and go at run
//! time, and the input machine starts on the screen with no keyboard.

use crate::states_main_menu_support::{
    current_help, current_label, key, labels, move_to, with_state_mut,
};
use ff_core::input_hints::{KEYBOARD, TOUCH};
use freight_fate::app::testing::TestApp;
use freight_fate::controller::fakes::FakePad;
use freight_fate::states::base::{Key, Menu};
use freight_fate::states::main_menu::{MainMenuState, SettingsCategoryState, SettingsState};
use freight_fate::states::online_hub::OnlineHubState;

/// Plays this test's thread as a phone until dropped.
struct Phone;

impl Phone {
    fn on(app: &mut TestApp) -> Self {
        freight_fate::device::force_phone(true);
        app.ctx.controller.restart_input(true);
        Phone
    }
}

impl Drop for Phone {
    fn drop(&mut self) {
        freight_fate::device::force_phone(false);
    }
}

#[test]
fn a_desktop_run_keeps_its_rows() {
    let mut app = TestApp::new();
    assert!(!freight_fate::device::phone());
    assert_eq!(app.ctx.controller.device(), KEYBOARD);
    app.push_state(MainMenuState::new());
    assert!(labels::<MainMenuState>(&app)
        .iter()
        .any(|row| row == "Quit"));
    app.push_state(SettingsCategoryState::new("controls"));
    assert!(!labels::<SettingsCategoryState>(&app)
        .iter()
        .any(|row| row == "Touch gestures"));
}

#[test]
fn playing_as_a_phone_shows_the_phone_rows() {
    let mut app = TestApp::new();
    let _phone = Phone::on(&mut app);
    assert!(freight_fate::device::phone());
    assert!(freight_fate::device::iphone());
    assert!(!freight_fate::device::personal_playlists());
    assert_eq!(app.ctx.controller.device(), TOUCH);
    assert!(!app.ctx.controller.hardware_keyboard());

    app.push_state(MainMenuState::new());
    assert!(!labels::<MainMenuState>(&app)
        .iter()
        .any(|row| row == "Quit"));

    app.push_state(SettingsState::new());
    let categories = labels::<SettingsState>(&app);
    assert!(
        !categories.iter().any(|row| row == "Updates"),
        "{categories:?}"
    );

    app.push_state(SettingsCategoryState::new("controls"));
    assert!(labels::<SettingsCategoryState>(&app)
        .iter()
        .any(|row| row == "Touch gestures"));
    app.ctx.pop_state();
    app.ctx.run_deferred();

    app.push_state(SettingsCategoryState::new("audio"));
    assert!(!labels::<SettingsCategoryState>(&app)
        .iter()
        .any(|row| row.starts_with("Shuffle personal playlists")));
}

const DESKTOP_CONTROLS: &[&str] = &[
    "Units",
    "Transmission",
    "Controller",
    "Haptics",
    "Second layer bumper",
    "Visual display",
    "Letters run menu items",
    "Keyboard shortcuts",
    "Controller buttons",
    "Back",
];

/// The Controls rows by name, without their values.
fn controls(app: &TestApp) -> Vec<String> {
    labels::<SettingsCategoryState>(app)
        .into_iter()
        .map(|row| row.split(':').next().unwrap_or_default().to_string())
        .filter(|row| cfg!(feature = "visuals") || row != "Visual display")
        .collect()
}

fn expected(rows: &[&str]) -> Vec<String> {
    rows.iter()
        .filter(|row| cfg!(feature = "visuals") || **row != "Visual display")
        .map(|row| row.to_string())
        .collect()
}

/// One frame of the open screen, the way the game loop runs it.
fn frame(app: &mut TestApp) {
    with_state_mut::<SettingsCategoryState, _>(app, |s, ctx| s.update(ctx, 0.016));
}

#[test]
fn desktop_controls_keep_every_row() {
    let mut app = TestApp::new();
    app.push_state(SettingsCategoryState::new("controls"));
    assert_eq!(controls(&app), expected(DESKTOP_CONTROLS));
}

#[test]
fn a_phone_lists_keyboard_and_controller_rows_only_while_connected() {
    let mut app = TestApp::new();
    let _phone = Phone::on(&mut app);
    app.push_state(SettingsCategoryState::new("controls"));
    assert_eq!(
        controls(&app),
        expected(&[
            "Touch gestures",
            "Units",
            "Transmission",
            "Visual display",
            "Back"
        ])
    );

    // A keyboard connecting while the screen is open: its rows come in,
    // and the focused row stays focused, silently.
    move_to::<SettingsCategoryState>(&mut app, "Transmission");
    app.clear_speech();
    app.ctx.controller.set_hardware_keyboard(true);
    frame(&mut app);
    assert_eq!(
        controls(&app),
        expected(&[
            "Touch gestures",
            "Units",
            "Transmission",
            "Visual display",
            "Letters run menu items",
            "Keyboard shortcuts",
            "Back",
        ])
    );
    assert!(current_label::<SettingsCategoryState>(&app).starts_with("Transmission"));
    assert!(app.main_lines().is_empty(), "{:?}", app.main_lines());

    // Left and Right still answer for the row they are on.
    move_to::<SettingsCategoryState>(&mut app, "Letters run menu items");
    let before = app.ctx.settings.menu_accelerator_keys;
    key(&mut app, Key::Right);
    assert_ne!(app.ctx.settings.menu_accelerator_keys, before);
    assert!(current_label::<SettingsCategoryState>(&app).starts_with("Letters run menu items"));

    // The keyboard going away while its row is focused: the cursor lands
    // on a row that is still there and reads it.
    app.clear_speech();
    app.ctx.controller.set_hardware_keyboard(false);
    frame(&mut app);
    assert!(!controls(&app).iter().any(|row| row == "Keyboard shortcuts"));
    assert_eq!(app.main_lines().len(), 1, "{:?}", app.main_lines());

    // A controller: its four rows.
    force_controller(&mut app);
    frame(&mut app);
    assert_eq!(
        controls(&app),
        expected(&[
            "Touch gestures",
            "Units",
            "Transmission",
            "Controller",
            "Haptics",
            "Second layer bumper",
            "Visual display",
            "Controller buttons",
            "Back",
        ])
    );
}

#[test]
fn a_phone_with_controller_support_off_keeps_the_controller_row() {
    let mut app = TestApp::new();
    let _phone = Phone::on(&mut app);
    app.ctx.settings.controller_enabled = false;
    app.push_state(SettingsCategoryState::new("controls"));
    assert_eq!(
        controls(&app),
        expected(&[
            "Touch gestures",
            "Units",
            "Transmission",
            "Controller",
            "Visual display",
            "Back",
        ])
    );
}

#[test]
fn settings_help_names_gestures_on_a_phone() {
    let mut app = TestApp::new();
    let _phone = Phone::on(&mut app);
    app.push_state(SettingsCategoryState::new("controls"));
    move_to::<SettingsCategoryState>(&mut app, "Transmission");
    let help = current_help::<SettingsCategoryState>(&app);
    assert!(!help.contains("clutch,"), "{help}");
    assert!(help.contains("to shift up."), "{help}");
    app.ctx.pop_state();
    app.ctx.run_deferred();

    app.push_state(SettingsCategoryState::new("assistance"));
    for (row, wanted) in [
        ("Latching brake", "swipe down and hold again"),
        ("Curve callouts", "to list the next few"),
        ("Lane keeping", "flick left or right to change lanes"),
        (
            "Driving assistance preset",
            "a flick left or right changes lanes",
        ),
    ] {
        move_to::<SettingsCategoryState>(&mut app, row);
        let help = current_help::<SettingsCategoryState>(&app);
        assert!(help.contains(wanted), "{row}: {help}");
    }
}

#[test]
fn online_has_no_discord_row_on_a_phone() {
    let mut app = TestApp::new();
    let _phone = Phone::on(&mut app);
    let hub = OnlineHubState::new(&mut app.ctx);
    app.push_state(hub);
    let rows = labels::<OnlineHubState>(&app);
    assert!(
        !rows.iter().any(|row| row.starts_with("Discord")),
        "{rows:?}"
    );
    assert_eq!(rows.last().map(String::as_str), Some("Back"));
    // Right on Back changes nothing.
    move_to::<OnlineHubState>(&mut app, "Back");
    let before = app.ctx.settings.discord_presence;
    key(&mut app, Key::Right);
    assert_eq!(app.ctx.settings.discord_presence, before);
}

/// Pretend a controller is connected so the app's manager reports active.
fn force_controller(app: &mut TestApp) {
    let c = &mut app.ctx.controller;
    c.set_enabled(true);
    c.bind_device(Box::new(FakePad::new(0)), "test pad");
    c.set_id_pending(false);
    c.note_controller();
}
