//! Key and button practice (`states/main_menu/input_practice.rs`): Shift F1
//! over any screen, every key and button named, nothing sent to the drive.

use ff_core::input_hints::CONTROLLER;
use ff_core::sim::weather::WeatherKind;

use freight_fate::controller::fakes::FakePad;
use freight_fate::controller::ControllerButton;
use freight_fate::playtest::harness::{key_event, PlaytestHarness, StartDelivery};
use freight_fate::states::base::{InputEvent, Key, Mods};
use freight_fate::states::driving::DrivingState;
use freight_fate::states::driving_pause_states::PauseMenuState;
use freight_fate::states::main_menu::InputPracticeState;

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

fn press(harness: &mut PlaytestHarness, key: Key) {
    harness.key(key_event(key, None));
}

fn open_practice(harness: &mut PlaytestHarness) {
    harness.key(InputEvent::key_mods(Key::F1, Mods::SHIFT));
    assert!(harness.state_is::<InputPracticeState>());
    harness.clear_speech();
}

fn last_main(harness: &PlaytestHarness) -> String {
    harness.app.main_lines().last().cloned().unwrap_or_default()
}

fn pad(harness: &mut PlaytestHarness) {
    let c = &mut harness.app.ctx.controller;
    c.set_enabled(true);
    c.bind_device(Box::new(FakePad::new(0)), "test pad");
    c.set_id_pending(false);
    c.active_device = CONTROLLER;
}

fn button(harness: &mut PlaytestHarness, button: ControllerButton) {
    harness
        .app
        .dispatch_controller(&InputEvent::ControllerButtonDown {
            button,
            instance_id: 0,
        });
}

fn button_up(harness: &mut PlaytestHarness, button: ControllerButton) {
    harness
        .app
        .dispatch_controller(&InputEvent::ControllerButtonUp {
            button,
            instance_id: 0,
        });
}

#[test]
fn shift_f1_mid_drive_names_keys_without_running_them() {
    let mut harness = a_drive("Practice Keys");
    let engine_before = harness.read_drive(|d| d.trip.truck.engine_on);
    open_practice(&mut harness);
    press(&mut harness, Key::E);
    assert_eq!(last_main(&harness), "E: Engine on or off.");
    assert_eq!(
        harness.read_drive(|d| d.trip.truck.engine_on),
        engine_before
    );
    press(&mut harness, Key::Comma);
    assert_eq!(last_main(&harness), "Comma: previous message.");
    press(&mut harness, Key::F2);
    assert_eq!(last_main(&harness), "F2: driving commands.");
    harness.key(InputEvent::key_mods(Key::K, Mods::SHIFT));
    assert_eq!(
        last_main(&harness),
        "Shift K: Resume the last cruise speed."
    );
}

#[test]
fn escape_twice_returns_to_the_drive() {
    let mut harness = a_drive("Practice Escape");
    open_practice(&mut harness);
    press(&mut harness, Key::Escape);
    assert!(harness.state_is::<InputPracticeState>());
    assert_eq!(
        last_main(&harness),
        "Escape: pause menu. Again to leave practice."
    );
    press(&mut harness, Key::Escape);
    assert!(harness.state_is::<DrivingState>());
}

#[test]
fn pad_buttons_are_named_on_both_layers() {
    let mut harness = a_drive("Practice Pad");
    pad(&mut harness);
    open_practice(&mut harness);
    button(&mut harness, ControllerButton::B);
    button_up(&mut harness, ControllerButton::B);
    assert_eq!(last_main(&harness), "B: Speed.");
    button(&mut harness, ControllerButton::RightShoulder);
    assert_eq!(
        last_main(&harness),
        "The right bumper: hold for the second layer of buttons."
    );
    button(&mut harness, ControllerButton::X);
    assert_eq!(
        last_main(&harness),
        "Right bumper plus X: Posted speed limit."
    );
    button_up(&mut harness, ControllerButton::X);
    button_up(&mut harness, ControllerButton::RightShoulder);
    button(&mut harness, ControllerButton::Start);
    button_up(&mut harness, ControllerButton::Start);
    assert!(harness.state_is::<InputPracticeState>());
    button(&mut harness, ControllerButton::Start);
    assert!(harness.state_is::<DrivingState>());
}

#[test]
fn the_pause_menu_opens_practice() {
    let mut harness = a_drive("Practice Pause");
    press(&mut harness, Key::Escape);
    assert!(harness.state_is::<PauseMenuState>());
    harness.select_menu_item("Practice keys and buttons");
    assert!(harness.state_is::<InputPracticeState>());
}

#[test]
fn back_plus_start_opens_practice_mid_drive_and_inside_names_itself() {
    let mut harness = a_drive("Practice Chord");
    pad(&mut harness);
    button(&mut harness, ControllerButton::Back);
    button(&mut harness, ControllerButton::Start);
    assert!(harness.state_is::<InputPracticeState>());
    button_up(&mut harness, ControllerButton::Start);
    button(&mut harness, ControllerButton::Start);
    assert!(harness.state_is::<InputPracticeState>());
    assert_eq!(
        last_main(&harness),
        "Back plus Start: key and button practice."
    );
}

#[test]
fn back_plus_start_opens_practice_over_the_pause_menu() {
    let mut harness = a_drive("Practice Chord Menu");
    press(&mut harness, Key::Escape);
    assert!(harness.state_is::<PauseMenuState>());
    pad(&mut harness);
    button(&mut harness, ControllerButton::Back);
    button(&mut harness, ControllerButton::Start);
    assert!(harness.state_is::<InputPracticeState>());
}

/// A pad that reports the closing Start twice, 16 ms apart, must not pause
/// the drive with the echo (seen on an Xbox One pad, 2026-10-13).
#[test]
fn a_bounced_start_leaving_practice_does_not_pause_the_drive() {
    let mut harness = a_drive("Practice Bounce");
    pad(&mut harness);
    open_practice(&mut harness);
    button(&mut harness, ControllerButton::Start);
    button_up(&mut harness, ControllerButton::Start);
    button(&mut harness, ControllerButton::Start);
    button_up(&mut harness, ControllerButton::Start);
    assert!(harness.state_is::<DrivingState>());
    button(&mut harness, ControllerButton::Start);
    button_up(&mut harness, ControllerButton::Start);
    assert!(harness.state_is::<DrivingState>());
}
