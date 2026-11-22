//! A desktop run playing as an iPhone (`freight_fate::device`, the agent
//! server's `scenario` `phone: true`): the phone's rows come and go at run
//! time, and the input machine starts on the screen with no keyboard.

use crate::states_main_menu_support::labels;
use ff_core::input_hints::{KEYBOARD, TOUCH};
use freight_fate::app::testing::TestApp;
use freight_fate::states::main_menu::{MainMenuState, SettingsCategoryState, SettingsState};

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
