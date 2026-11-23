//! Practice and the command list on a touch screen
//! (`states/main_menu/input_practice.rs`, `touch_practice.rs`, the pause
//! menu): the practice rows open gesture practice and name gestures, leaving
//! by touch has no clock, and the driving command list has a pause menu row
//! and a gesture slot of its own.

use ff_core::sim::weather::WeatherKind;

use freight_fate::bindings::TouchCommand;
use freight_fate::playtest::harness::{key_event, PlaytestHarness, StartDelivery};
use freight_fate::states::base::Key;
use freight_fate::states::driving::DrivingState;
use freight_fate::states::driving_menu_states::DrivingCommandsState;
use freight_fate::states::driving_pause_states::PauseMenuState;
use freight_fate::states::main_menu::{InputPracticeState, MainMenuState};
use freight_fate::touch::Gesture;

fn a_drive(name: &str) -> PlaytestHarness {
    let mut harness = PlaytestHarness::new();
    harness.start_delivery(StartDelivery::named(name));
    harness.with_drive(|drive, _| {
        drive.tutorial = None;
        drive.departure_checked = true;
        drive.trip.hazard_check_mi = 1e9;
        drive.trip.inspection_check_mi = 1e9;
        drive.trip.traffic_manager.rolling_bubble = false;
        drive.trip.set_npc_vehicles(Vec::new());
        drive.trip.traffic_pressures.clear();
        drive.trip.weather.current = WeatherKind::Clear;
    });
    harness.clear_speech();
    harness
}

/// The main menu on a phone with no keyboard, focused on the practice row.
fn touch_main_menu() -> PlaytestHarness {
    let mut harness = PlaytestHarness::new();
    harness.app.ctx.controller.note_touch();
    harness.app.ctx.controller.set_hardware_keyboard(false);
    harness.app.push_state(MainMenuState::new());
    harness
}

#[test]
fn the_practice_row_on_a_touch_screen_opens_gesture_practice() {
    let mut harness = touch_main_menu();
    let labels = harness.menu_labels();
    assert!(
        labels.iter().any(|label| label == "Practice gestures"),
        "{labels:?}"
    );
    assert!(
        !labels
            .iter()
            .any(|label| label == "Practice keys and buttons"),
        "{labels:?}"
    );
    let row = labels
        .iter()
        .position(|label| label == "Practice gestures")
        .unwrap();
    for _ in 0..row {
        harness.app.dispatch_gesture(Gesture::SwipeRight);
    }
    harness.clear_speech();
    harness.app.dispatch_gesture(Gesture::DoubleTap);
    assert!(harness.state_is::<InputPracticeState>());
    let entry = harness.transcript().join(" ");
    assert!(entry.contains("Practice gestures."), "{entry}");
    assert!(
        entry.ends_with("Flick down with two fingers twice to leave."),
        "{entry}"
    );

    // A flick is named as a flick, never as the key a menu flick presses.
    harness.clear_speech();
    harness.app.dispatch_gesture(Gesture::SwipeUp);
    let said = harness.transcript();
    assert_eq!(said.len(), 1, "{said:?}");
    assert!(said[0].starts_with("Flick up: "), "{said:?}");
}

#[test]
fn the_practice_row_with_a_keyboard_stays_key_practice() {
    let mut harness = PlaytestHarness::new();
    harness.app.push_state(MainMenuState::new());
    let labels = harness.menu_labels();
    assert!(
        labels
            .iter()
            .any(|label| label == "Practice keys and buttons"),
        "{labels:?}"
    );
}

#[test]
fn key_practice_names_gestures_too() {
    let mut harness = PlaytestHarness::new();
    harness.app.push_state(InputPracticeState::new());
    harness.clear_speech();
    harness.app.dispatch_gesture(Gesture::SwipeRight);
    assert_eq!(
        harness.transcript(),
        vec!["Flick right: Change lanes right."]
    );
    // And a key still names the key.
    harness.clear_speech();
    harness.key(key_event(Key::Up, None));
    assert_eq!(harness.transcript(), vec!["The Up arrow: Accelerate."]);
}

#[test]
fn leaving_practice_by_touch_takes_two_back_gestures_in_a_row() {
    let mut harness = PlaytestHarness::new();
    harness.app.ctx.controller.note_touch();
    harness.app.ctx.controller.set_hardware_keyboard(false);
    harness.app.push_state(InputPracticeState::gestures());

    harness.clear_speech();
    harness.app.dispatch_gesture(Gesture::Escape);
    assert_eq!(harness.transcript(), vec!["Again to leave practice."]);
    // Any other gesture in between starts the count over.
    harness.app.dispatch_gesture(Gesture::Tap);
    harness.clear_speech();
    harness.app.dispatch_gesture(Gesture::TwoFingerSwipeDown);
    assert_eq!(harness.transcript(), vec!["Again to leave practice."]);
    assert!(harness.state_is::<InputPracticeState>());

    // However long after the first, the second one leaves: practice keeps
    // no clock for touch, so time passing changes nothing.
    harness.advance_clock(10.0);
    assert!(harness.state_is::<InputPracticeState>());
    harness.clear_speech();
    harness.app.dispatch_gesture(Gesture::Escape);
    assert_eq!(harness.transcript()[0], "Leaving gesture practice.");
    assert!(!harness.state_is::<InputPracticeState>());
}

#[test]
fn the_pause_menu_on_touch_opens_the_command_list_second() {
    let mut harness = a_drive("Touch Pause Commands");
    harness.app.ctx.controller.set_hardware_keyboard(false);
    harness.app.dispatch_gesture(Gesture::MagicTap);
    assert!(harness.state_is::<PauseMenuState>());
    let labels = harness.menu_labels();
    assert_eq!(labels[0], "Resume driving", "{labels:?}");
    assert_eq!(labels[1], "Driving commands", "{labels:?}");

    harness.app.dispatch_gesture(Gesture::SwipeRight);
    harness.app.dispatch_gesture(Gesture::DoubleTap);
    assert!(harness.state_is::<DrivingCommandsState>());
    // Back from the list goes to the drive, not to the pause menu.
    harness.app.dispatch_gesture(Gesture::TwoFingerSwipeDown);
    assert!(harness.state_is::<DrivingState>());
}

#[test]
fn the_pause_menu_with_a_keyboard_has_no_command_list_row() {
    let mut harness = a_drive("Key Pause Commands");
    harness.key(key_event(Key::Escape, None));
    assert!(harness.state_is::<PauseMenuState>());
    assert!(!harness
        .menu_labels()
        .iter()
        .any(|label| label == "Driving commands"));
}

#[test]
fn a_gesture_slot_can_hold_the_command_list() {
    let mut harness = a_drive("Touch Command List Slot");
    harness
        .app
        .ctx
        .bindings
        .set_touch_command(Gesture::Tap, TouchCommand::CommandList);
    assert_eq!(TouchCommand::CommandList.label(), "Driving command list");
    assert!(TouchCommand::all().any(|command| command == TouchCommand::CommandList));
    harness.app.dispatch_gesture(Gesture::Tap);
    assert!(harness.state_is::<DrivingCommandsState>());
}
