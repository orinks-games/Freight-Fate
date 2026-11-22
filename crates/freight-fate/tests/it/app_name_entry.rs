//! Port of `tests/test_name_entry.py`: accessible review of the driver-name
//! text field.
//!
//! Regression coverage for the bug where left/right arrows did nothing while
//! typing a driver name for a new career: a screen reader user had no way to
//! step back through what they had typed to check it before pressing Enter.
//! These pin the standard accessible-text-field behavior: a review cursor
//! that speaks each character as it moves, Home/End jump-and-speak,
//! backspace that edits (and deletes) at the cursor rather than always the
//! end, and character echo -- including a "cap" marker so a capital letter
//! is distinguishable by ear from a lowercase one.
//!
//! The field is reached the way the Python tests reached it: through the
//! main menu's New career row, which pushes `NameEntryState`.

use freight_fate::app::testing::TestApp;
use freight_fate::states::base::{InputEvent, Key, Menu};
use freight_fate::states::main_menu::{MainMenuState, NameEntryState};
use freight_fate::states::text_entry::TOUCH_FIELD_HELP;
use freight_fate::touch::Gesture;

fn key_event(key: Key) -> InputEvent {
    InputEvent::key(key)
}

fn open_name_entry(app: &mut TestApp) {
    app.push_state(MainMenuState::new());
    loop {
        let on_new_career = {
            let state = app.state().unwrap();
            let state = state.borrow();
            let menu = state.as_any().downcast_ref::<MainMenuState>().unwrap();
            let core = menu.menu();
            core.items[core.index].text(menu, &app.ctx) == "New career"
        };
        if on_new_career {
            break;
        }
        app.dispatch_to_state(&key_event(Key::Down));
    }
    app.dispatch_to_state(&key_event(Key::Return));
    assert!(app
        .state()
        .unwrap()
        .borrow()
        .as_any()
        .is::<NameEntryState>());
    app.clear_speech();
}

fn type_text(app: &mut TestApp, text: &str) {
    for ch in text.chars() {
        app.dispatch_to_state(&InputEvent::typed(ch));
    }
}

fn name(app: &TestApp) -> String {
    let state = app.state().unwrap();
    let state = state.borrow();
    state
        .as_any()
        .downcast_ref::<NameEntryState>()
        .unwrap()
        .name()
}

fn cursor(app: &TestApp) -> usize {
    let state = app.state().unwrap();
    let state = state.borrow();
    state
        .as_any()
        .downcast_ref::<NameEntryState>()
        .unwrap()
        .cursor()
}

fn spoken(app: &TestApp) -> Vec<String> {
    app.main_lines()
}

#[test]
fn visible_name_help_follows_touch_and_hardware_keys() {
    let mut app = TestApp::new();
    app.dispatch_gesture(Gesture::KeyboardDisconnected);
    app.push_state(NameEntryState::new());
    assert!(app.main_lines().join(" ").contains(TOUCH_FIELD_HELP));
    assert_eq!(app.visible_lines().last().unwrap(), TOUCH_FIELD_HELP);

    app.dispatch_gesture(Gesture::KeyboardConnected);
    app.handle_event(&key_event(Key::Left));
    assert!(app.visible_lines().last().unwrap().contains("Control V"));

    app.dispatch_gesture(Gesture::Tap);
    assert_eq!(app.visible_lines().last().unwrap(), TOUCH_FIELD_HELP);
    app.shutdown();
}

#[test]
fn test_typed_characters_are_echoed_including_space_and_capitals() {
    let mut app = TestApp::new();
    open_name_entry(&mut app);
    type_text(&mut app, "Jo Ann");
    assert_eq!(name(&app), "Jo Ann");
    assert_eq!(spoken(&app), vec!["cap j", "o", "space", "cap a", "n", "n"]);
    app.shutdown();
}

#[test]
fn test_left_arrow_reviews_characters_back_to_front() {
    let mut app = TestApp::new();
    open_name_entry(&mut app);
    type_text(&mut app, "Cary");
    app.clear_speech();

    for _ in 0..4 {
        app.dispatch_to_state(&key_event(Key::Left));
    }

    assert_eq!(spoken(&app), vec!["y", "r", "a", "cap c"]);
    assert_eq!(cursor(&app), 0);
    // Reviewing does not change what was typed.
    assert_eq!(name(&app), "Cary");
    app.shutdown();
}

#[test]
fn test_right_arrow_reviews_characters_front_to_back_after_reviewing_left() {
    let mut app = TestApp::new();
    open_name_entry(&mut app);
    type_text(&mut app, "Cary");
    // Walk the cursor back to the start first, as a player checking their
    // spelling would, then arrow forward again.
    for _ in 0..4 {
        app.dispatch_to_state(&key_event(Key::Left));
    }
    assert_eq!(cursor(&app), 0);

    app.clear_speech();
    for _ in 0..4 {
        app.dispatch_to_state(&key_event(Key::Right));
    }

    assert_eq!(spoken(&app), vec!["cap c", "a", "r", "y"]);
    assert_eq!(cursor(&app), 4);
    app.shutdown();
}

#[test]
fn test_left_arrow_at_start_of_empty_field_does_not_crash() {
    let mut app = TestApp::new();
    open_name_entry(&mut app);
    let audio = app.record_audio();
    app.dispatch_to_state(&key_event(Key::Left));
    assert_eq!(cursor(&app), 0);
    assert_eq!(name(&app), "");
    assert!(audio
        .borrow()
        .played
        .iter()
        .any(|(k, _, _)| k == "ui/error"));
    app.shutdown();
}

#[test]
fn test_right_arrow_at_end_of_typed_name_gives_boundary_feedback() {
    let mut app = TestApp::new();
    open_name_entry(&mut app);
    type_text(&mut app, "Cary");
    let audio = app.record_audio();
    app.clear_speech();

    app.dispatch_to_state(&key_event(Key::Right));

    assert_eq!(cursor(&app), 4);
    assert!(spoken(&app).is_empty()); // no character left to move over
    assert!(audio
        .borrow()
        .played
        .iter()
        .any(|(k, _, _)| k == "ui/error"));
    app.shutdown();
}

#[test]
fn test_home_and_end_jump_to_ends_and_speak_the_edge_character() {
    let mut app = TestApp::new();
    open_name_entry(&mut app);
    type_text(&mut app, "Cary");
    app.dispatch_to_state(&key_event(Key::Left));
    app.dispatch_to_state(&key_event(Key::Left));
    assert_eq!(cursor(&app), 2);

    app.clear_speech();
    app.dispatch_to_state(&key_event(Key::Home));
    assert_eq!(cursor(&app), 0);
    assert_eq!(spoken(&app), vec!["Start. cap c"]);

    app.clear_speech();
    app.dispatch_to_state(&key_event(Key::End));
    assert_eq!(cursor(&app), 4);
    assert_eq!(spoken(&app), vec!["End. y"]);
    app.shutdown();
}

#[test]
fn test_home_and_end_are_no_ops_when_the_cursor_is_already_there() {
    let mut app = TestApp::new();
    open_name_entry(&mut app);
    let audio = app.record_audio();
    // Fresh, empty field: cursor is already at 0, which is also the end.
    app.dispatch_to_state(&key_event(Key::Home));
    assert!(audio
        .borrow()
        .played
        .iter()
        .any(|(k, _, _)| k == "ui/error"));
    audio.borrow_mut().played.clear();
    app.dispatch_to_state(&key_event(Key::End));
    assert!(audio
        .borrow()
        .played
        .iter()
        .any(|(k, _, _)| k == "ui/error"));
    app.shutdown();
}

#[test]
fn test_backspace_deletes_the_character_before_the_cursor_and_speaks_it() {
    let mut app = TestApp::new();
    open_name_entry(&mut app);
    type_text(&mut app, "Cary");
    // Move the cursor to just after the "a" (index 2) and delete it, not
    // the last-typed "y" -- true cursor editing, not append-only.
    app.dispatch_to_state(&key_event(Key::Left));
    app.dispatch_to_state(&key_event(Key::Left));
    assert_eq!(cursor(&app), 2);

    app.clear_speech();
    app.dispatch_to_state(&key_event(Key::Backspace));

    assert_eq!(name(&app), "Cry");
    assert_eq!(cursor(&app), 1);
    assert_eq!(spoken(&app), vec!["Deleted a. Cry"]);
    app.shutdown();
}

#[test]
fn test_backspace_on_empty_field_gives_boundary_feedback_not_a_crash() {
    let mut app = TestApp::new();
    open_name_entry(&mut app);
    let audio = app.record_audio();
    app.dispatch_to_state(&key_event(Key::Backspace));
    assert_eq!(name(&app), "");
    assert!(audio
        .borrow()
        .played
        .iter()
        .any(|(k, _, _)| k == "ui/error"));
    app.shutdown();
}

#[test]
fn test_typing_inserts_at_the_cursor_not_only_at_the_end() {
    let mut app = TestApp::new();
    open_name_entry(&mut app);
    type_text(&mut app, "Crry");
    // Fix the typo (missing "a" after "C") by moving the cursor back between
    // "C" and the first "r", then inserting there.
    for _ in 0..3 {
        app.dispatch_to_state(&key_event(Key::Left));
    }
    assert_eq!(cursor(&app), 1);

    app.clear_speech();
    app.dispatch_to_state(&InputEvent::typed('a'));

    assert_eq!(name(&app), "Carry");
    assert_eq!(cursor(&app), 2);
    assert_eq!(spoken(&app), vec!["a"]);
    app.shutdown();
}

#[test]
fn test_deleting_the_last_character_reports_empty() {
    let mut app = TestApp::new();
    open_name_entry(&mut app);
    type_text(&mut app, "A");
    app.clear_speech();
    app.dispatch_to_state(&key_event(Key::Backspace));
    assert_eq!(name(&app), "");
    assert_eq!(cursor(&app), 0);
    assert_eq!(spoken(&app), vec!["Deleted cap a. Empty."]);
    app.shutdown();
}

#[test]
fn test_lines_show_a_cursor_marker_at_the_review_position() {
    let mut app = TestApp::new();
    open_name_entry(&mut app);
    type_text(&mut app, "Cary");
    app.dispatch_to_state(&key_event(Key::Left));
    app.dispatch_to_state(&key_event(Key::Left));
    assert_eq!(cursor(&app), 2);
    let lines = app.visible_lines();
    assert!(lines.iter().any(|line| line == "Driver name: Ca|ry"));
    app.shutdown();
}

/// F2 reads the whole field, "Empty." when nothing is typed.
#[test]
fn f2_reads_the_whole_field() {
    let mut app = TestApp::new();
    open_name_entry(&mut app);
    app.dispatch_to_state(&key_event(Key::F2));
    assert_eq!(spoken(&app), vec!["Empty."]);
    type_text(&mut app, "Jo");
    app.clear_speech();
    app.dispatch_to_state(&key_event(Key::F2));
    assert_eq!(spoken(&app), vec!["Jo"]);
    app.shutdown();
}

/// The field stops taking characters at MAX_LEN.
#[test]
fn the_field_caps_at_max_len() {
    let mut app = TestApp::new();
    open_name_entry(&mut app);
    type_text(&mut app, &"a".repeat(30));
    assert_eq!(
        name(&app).chars().count(),
        freight_fate::states::text_entry::MAX_LEN
    );
    app.shutdown();
}

#[test]
fn help_in_the_name_field_reads_its_opening_line_again() {
    let mut app = TestApp::new();
    app.dispatch_gesture(Gesture::KeyboardDisconnected);
    open_name_entry(&mut app);
    app.clear_speech();
    app.dispatch_gesture(Gesture::TwoFingerSwipeUp);
    assert_eq!(
        app.main_lines(),
        vec![format!(
            "New career. Type your driver name. {TOUCH_FIELD_HELP}"
        )]
    );
    // Flick up and down stay silent.
    app.clear_speech();
    app.dispatch_gesture(Gesture::SwipeUp);
    app.dispatch_gesture(Gesture::SwipeDown);
    assert!(app.main_lines().is_empty(), "{:?}", app.main_lines());
    assert_eq!(name(&app), "");
}

#[test]
fn an_empty_name_starts_the_career_as_driver() {
    let mut app = TestApp::new();
    open_name_entry(&mut app);
    app.dispatch_to_state(&key_event(Key::Return));
    let start = app.state().unwrap();
    let start = start.borrow();
    let start = start
        .as_any()
        .downcast_ref::<freight_fate::states::main_menu::CareerStartState>()
        .expect("Career start opens");
    assert_eq!(start.driver_name, "Driver");
}
