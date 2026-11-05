//! The one-time Driving assistance picker that opens a launch until it is
//! answered (owner, 2026-09-30).

use crate::states_main_menu_support::*;
use ff_core::settings::Settings;
use freight_fate::app::testing::TestApp;
use freight_fate::states::assist_picker::AssistPickerState;
use freight_fate::states::base::Key;
use freight_fate::states::main_menu::MainMenuState;

type Picker = AssistPickerState;

/// The picker, owed, over settings on `preset` ("custom" for hand-set ones).
fn picker_on(preset: &str) -> TestApp {
    let mut app = TestApp::new();
    app.ctx.settings.assist_preset_chosen = false;
    if preset == "custom" {
        app.ctx.settings.apply_driving_assistance_preset("balanced");
        app.ctx.settings.curve_speed_assist = false;
        app.ctx.settings.driving_assistance_preset = "custom".to_string();
    } else {
        assert!(app.ctx.settings.apply_driving_assistance_preset(preset));
    }
    app.push_state(AssistPickerState::new());
    app
}

/// Answered: the flag is set in memory AND on disk, and the main menu is up.
fn assert_answered(app: &TestApp) {
    assert!(app.ctx.settings.assist_preset_chosen);
    assert!(Settings::load().assist_preset_chosen);
    assert!(is::<MainMenuState>(app));
}

#[test]
fn a_launch_opens_on_the_picker_until_it_is_answered() {
    // Answered, a launch opens on the main menu: `app_smoke` pins that.
    let mut app = TestApp::new();
    app.ctx.settings.assist_preset_chosen = false;
    app.run(Some(2));
    let first = &app.main_lines()[0];
    assert_eq!(
        first,
        "Driving assistance. How much should the truck do for you? You can \
         change this later in Settings, Gameplay, Driving assistance."
    );
}

#[test]
fn enter_applies_the_preset_and_spends_the_picker() {
    for (from, choose, preset, lane_keeping) in [
        ("realistic", "All assists", "all", "full"),
        ("all", "Balanced", "balanced", "partial"),
        ("all", "Realistic", "realistic", "off"),
    ] {
        let mut app = picker_on(from);
        select::<Picker>(&mut app, choose);
        assert_eq!(app.ctx.settings.driving_assistance_preset, preset);
        assert_eq!(app.ctx.settings.lane_keeping, lane_keeping);
        assert_eq!(Settings::load().driving_assistance_preset, preset);
        assert_answered(&app);
    }
}

#[test]
fn escape_keeps_what_the_player_has() {
    for preset in ["balanced", "custom"] {
        let mut app = picker_on(preset);
        key(&mut app, Key::Escape);
        assert_eq!(app.ctx.settings.driving_assistance_preset, preset);
        if preset == "custom" {
            assert!(
                !app.ctx.settings.curve_speed_assist,
                "hand-set assists kept"
            );
        }
        assert_answered(&app);
    }
}

#[test]
fn the_cursor_starts_on_the_preset_already_set() {
    for (preset, start) in [("all", 0), ("balanced", 1), ("realistic", 2)] {
        let app = picker_on(preset);
        assert!(!labels::<Picker>(&app)[0].starts_with("Keep"), "{preset}");
        assert_eq!(index::<Picker>(&app), start, "{preset}");
    }
}

#[test]
fn custom_assists_are_offered_first_and_kept_by_enter() {
    let mut app = picker_on("custom");
    assert_eq!(labels::<Picker>(&app)[0], "Keep my custom assists");
    assert_eq!(index::<Picker>(&app), 0);
    key(&mut app, Key::Return);
    assert_eq!(app.ctx.settings.driving_assistance_preset, "custom");
    assert!(!app.ctx.settings.curve_speed_assist);
    assert_answered(&app);
}

#[test]
fn every_row_says_what_the_truck_does() {
    let app = picker_on("all");
    let rows = labels_and_help::<Picker>(&app);
    let labels: Vec<&str> = rows.iter().map(|(label, _)| label.as_str()).collect();
    assert_eq!(
        labels,
        [
            "All assists: the truck steers. Recommended for your first drives.",
            "Balanced: you steer, with help.",
            "Realistic: you drive.",
        ]
    );
    // The steer keys are read in the player's own bindings, never as tokens.
    assert!(rows[0].1.starts_with(
        "It holds the lane, steers the bends and street corners, moves out of closed lanes, and \
         takes your exits and open weigh stations. A tap of "
    ));
    assert!(rows[0]
        .1
        .ends_with("changes lanes. Speed, rest and fuel stops, and the docks are yours."));
    assert!(!rows[0].1.contains("{{"), "{}", rows[0].1);
    assert_eq!(
        rows[1].1,
        "It eases you through bends and back from a drift. Street corners, lane \
         changes, exit signals and speed are yours. It stops for you at your destination."
    );
    assert_eq!(
        rows[2].1,
        "A modern truck's safety systems and nothing more. Every bend, corner and \
         exit is yours."
    );
}
