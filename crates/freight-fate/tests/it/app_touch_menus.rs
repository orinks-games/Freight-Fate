//! Menus on a touch screen move the way VoiceOver does (`touch::route`'s
//! menu column, `Menu::handle_gesture`), and their help names gestures on a
//! touch screen and keys once a keyboard is back.

use freight_fate::app::testing::TestApp;
use freight_fate::states::base::{
    Key, MenuItem, SimpleMenuState, DEFAULT_INTRO_HELP, TOUCH_INTRO_HELP,
};
use freight_fate::states::main_menu::SettingsCategoryState;
use freight_fate::states::main_menu_help::HelpState;
use freight_fate::states::text_entry::TextEntryState;
use freight_fate::touch::Gesture;

use crate::states_city_support::*;

/// A game on a phone with no hardware keyboard.
fn on_touch() -> TestApp {
    let mut app = TestApp::new();
    app.dispatch_gesture(Gesture::KeyboardDisconnected);
    app
}

fn three_rows() -> SimpleMenuState {
    SimpleMenuState::new(
        "Rows",
        vec![
            MenuItem::inert("First")
                .help("Enter on the first.")
                .touch_help("Double tap on the first."),
            MenuItem::inert("Second"),
            MenuItem::inert("Third"),
        ],
    )
}

fn last_line(app: &TestApp) -> String {
    app.main_lines().last().cloned().unwrap_or_default()
}

#[test]
fn flick_right_and_left_walk_the_rows_and_up_and_down_do_not() {
    let mut app = on_touch();
    app.push_state(three_rows());
    app.dispatch_gesture(Gesture::SwipeRight);
    assert_eq!(current_label::<SimpleMenuState>(&app), "Second");
    app.dispatch_gesture(Gesture::SwipeLeft);
    assert_eq!(current_label::<SimpleMenuState>(&app), "First");

    // A plain row has no value: flick up and down leave the focus and say
    // nothing new.
    app.clear_speech();
    app.dispatch_gesture(Gesture::SwipeUp);
    app.dispatch_gesture(Gesture::SwipeDown);
    assert_eq!(current_label::<SimpleMenuState>(&app), "First");
    assert!(app.main_lines().is_empty(), "{:?}", app.main_lines());

    // A swipe and hold is the flick in its direction, once.
    app.dispatch_gesture(Gesture::HoldRightBegan);
    app.dispatch_gesture(Gesture::HoldEnded);
    assert_eq!(current_label::<SimpleMenuState>(&app), "Second");
    app.dispatch_gesture(Gesture::HoldLeftBegan);
    assert_eq!(current_label::<SimpleMenuState>(&app), "First");

    // VoiceOver's swipes on the adjustable element, with direct touch off,
    // are the only way through a list there, so they still move.
    app.dispatch_gesture(Gesture::Decrement);
    assert_eq!(current_label::<SimpleMenuState>(&app), "Second");
    app.dispatch_gesture(Gesture::Increment);
    assert_eq!(current_label::<SimpleMenuState>(&app), "First");
}

#[test]
fn flick_up_and_down_change_a_value() {
    let mut app = on_touch();
    app.push_state(SettingsCategoryState::new("audio"));
    move_to::<SettingsCategoryState>(&mut app, "Master volume");
    let before = app.ctx.settings.master_volume;
    app.dispatch_gesture(Gesture::SwipeUp);
    let raised = app.ctx.settings.master_volume;
    app.dispatch_gesture(Gesture::SwipeDown);
    app.dispatch_gesture(Gesture::SwipeDown);
    let lowered = app.ctx.settings.master_volume;
    assert!(raised > before || before >= 1.0, "{before} -> {raised}");
    assert!(lowered < raised, "{raised} -> {lowered}");
    assert!(current_label::<SettingsCategoryState>(&app).starts_with("Master volume"));
}

#[test]
fn a_tap_reads_the_focused_row_again() {
    let mut app = on_touch();
    app.ctx.say("An earlier message.");
    app.push_state(three_rows());
    app.dispatch_gesture(Gesture::SwipeRight);
    app.clear_speech();
    app.dispatch_gesture(Gesture::Tap);
    app.dispatch_gesture(Gesture::Tap);
    let row = current_text::<SimpleMenuState>(&app);
    assert_eq!(app.main_lines(), vec![row.clone(), row]);
}

#[test]
fn a_two_finger_double_tap_in_a_menu_does_nothing() {
    let mut app = on_touch();
    app.push_state(three_rows());
    app.dispatch_gesture(Gesture::SwipeRight);
    app.clear_speech();
    app.dispatch_gesture(Gesture::MagicTap);
    assert!(is::<SimpleMenuState>(&app));
    assert_eq!(current_label::<SimpleMenuState>(&app), "Second");
    assert!(app.main_lines().is_empty(), "{:?}", app.main_lines());
}

#[test]
fn the_scrub_and_the_two_finger_flick_down_both_go_back() {
    for gesture in [Gesture::Escape, Gesture::TwoFingerSwipeDown] {
        let mut app = on_touch();
        app.push_state(three_rows());
        app.push_state(SettingsCategoryState::new("audio"));
        app.dispatch_gesture(gesture);
        assert!(is::<SimpleMenuState>(&app), "{gesture:?}");
        drop(app);
    }
}

#[test]
fn help_names_gestures_on_touch_and_keys_with_a_keyboard() {
    let mut app = on_touch();
    app.push_state(three_rows());
    app.dispatch_gesture(Gesture::TwoFingerSwipeUp);
    assert_eq!(last_line(&app), "Double tap on the first.");

    app.dispatch_gesture(Gesture::KeyboardConnected);
    key(&mut app, Key::F1);
    assert_eq!(last_line(&app), "Enter on the first.");

    drop(app);

    // A menu with nothing in it speaks its intro help.
    let mut app = on_touch();
    app.push_state(SimpleMenuState::new("Empty", Vec::new()));
    app.dispatch_gesture(Gesture::TwoFingerSwipeUp);
    assert_eq!(last_line(&app), TOUCH_INTRO_HELP);
    app.dispatch_gesture(Gesture::KeyboardConnected);
    key(&mut app, Key::F1);
    assert_eq!(last_line(&app), DEFAULT_INTRO_HELP);
}

#[test]
fn no_touch_intro_help_names_a_key() {
    let screens: Vec<SimpleMenuState> = vec![
        SimpleMenuState::readout("Readout", Vec::new()),
        SimpleMenuState::new("Plain", Vec::new()),
    ];
    for screen in screens {
        let mut app = on_touch();
        app.push_state(screen);
        let help = current_help::<SimpleMenuState>(&app);
        for word in ["Enter", "Escape", "Up and", "arrow", "Tab", "F1"] {
            assert!(!help.contains(word), "{help}");
        }
        drop(app);
    }
}

#[test]
fn how_to_play_names_gestures_and_a_tap_reads_the_line() {
    let mut app = on_touch();
    app.push_state(HelpState::new());
    let opening = app.main_lines().join(" ");
    assert!(opening.contains("Flick up for the next page"), "{opening}");
    assert!(!opening.contains("Escape"), "{opening}");

    app.dispatch_gesture(Gesture::SwipeUp);
    assert!(
        last_line(&app).starts_with("Page 2 of"),
        "{}",
        last_line(&app)
    );
    app.dispatch_gesture(Gesture::SwipeRight);
    let line = last_line(&app);
    app.clear_speech();
    app.dispatch_gesture(Gesture::Tap);
    assert_eq!(app.main_lines(), vec![line]);
}

#[test]
fn gestures_that_would_type_a_character_type_nothing_into_a_name() {
    let mut app = on_touch();
    app.push_state(TextEntryState::new("Name", "Name", |_, _| {}));
    for gesture in [
        Gesture::Tap,
        Gesture::MagicTap,
        Gesture::TwoFingerSwipeLeft,
        Gesture::TwoFingerSwipeRight,
    ] {
        app.dispatch_gesture(gesture);
    }
    let name = with_state::<TextEntryState, _>(&app, |s, _| s.name());
    assert_eq!(name, "");
}

#[test]
fn a_touch_menu_with_no_way_back_gains_a_back_row() {
    let mut app = on_touch();
    app.push_state(SimpleMenuState::new(
        "Prompt",
        vec![MenuItem::inert("Only")],
    ));
    app.push_state(three_rows());
    app.dispatch_gesture(Gesture::SwipeLeft);
    assert_eq!(current_label::<SimpleMenuState>(&app), "Back");
    app.clear_speech();
    app.dispatch_gesture(Gesture::DoubleTap);
    assert_eq!(current_label::<SimpleMenuState>(&app), "Only");
}

#[test]
fn a_menu_that_already_goes_back_gains_no_second_back_row() {
    let mut app = on_touch();
    app.push_state(SimpleMenuState::readout("Lines", vec!["One".to_string()]));
    app.dispatch_gesture(Gesture::SwipeLeft);
    assert_eq!(current_label::<SimpleMenuState>(&app), "Back");
    app.dispatch_gesture(Gesture::SwipeLeft);
    assert_eq!(current_label::<SimpleMenuState>(&app), "One");
}

#[test]
fn a_keyboard_menu_gains_no_back_row() {
    let mut app = TestApp::new();
    app.push_state(three_rows());
    key(&mut app, Key::Up);
    assert_eq!(current_label::<SimpleMenuState>(&app), "Third");
}
