//! The driving command list (`states/driving_menu_states/commands.rs`): F2 at
//! the wheel, and three-finger tap on a touch screen. Each row runs its
//! control the way the key does, after handing the drive back.

use ff_core::sim::weather::WeatherKind;

use freight_fate::bindings::{reserved_key_reason, Action, Chord};
use freight_fate::playtest::harness::{key_event, PlaytestHarness, StartDelivery};
use freight_fate::states::base::Key;
use freight_fate::states::driving::DrivingState;
use freight_fate::states::driving_menu_states::{DrivingCommandsState, DrivingStatusState};

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

fn last_main(harness: &PlaytestHarness) -> String {
    harness.app.main_lines().last().cloned().unwrap_or_default()
}

const TOP: [&str; 11] = [
    "Status menu",
    "Speed and cruise",
    "Road and stops",
    "Where am I",
    "Truck",
    "Gears and engine brake",
    "Radio and CB",
    "Hours and clock",
    "Copy the last spoken text",
    "Driving help",
    "Back to driving",
];

/// Open the group `label` from the top list.
fn open_group(harness: &mut PlaytestHarness, label: &str) {
    harness.select_menu_item(label);
    assert!(harness.state_is::<DrivingCommandsState>());
}

#[test]
fn f2_opens_the_top_list_in_its_fixed_order_on_the_status_menu() {
    let mut harness = a_drive("Command List");
    press(&mut harness, Key::F2);
    assert!(harness.state_is::<DrivingCommandsState>());
    assert_eq!(harness.menu_labels(), TOP);
    assert_eq!(harness.focused_label().as_deref(), Some("Status menu"));
}

#[test]
fn the_groups_list_the_one_shot_controls_and_not_the_held_ones() {
    let mut harness = a_drive("Command Groups");
    press(&mut harness, Key::F2);
    let mut labels = Vec::new();
    for group in &TOP[1..8] {
        open_group(&mut harness, group);
        labels.extend(harness.menu_labels());
        press(&mut harness, Key::Escape);
    }
    for wanted in [
        Action::Speed.label(),
        Action::SpeedLimit.label(),
        Action::Engine.label(),
        Action::TakeExit.label(),
        "Raise the cruise target",
        "Next radio station",
    ] {
        assert!(
            labels.iter().any(|l| l == wanted),
            "{wanted:?} not in {labels:#?}"
        );
    }
    for held in [
        Action::Accelerate,
        Action::Brake,
        Action::EmergencyBrake,
        Action::SteerLeft,
        Action::SteerRight,
        Action::Horn,
    ] {
        assert!(
            !labels.iter().any(|l| l == held.label()),
            "{:?} is held, yet listed: {labels:#?}",
            held
        );
    }
}

#[test]
fn a_command_in_a_group_closes_both_lists_and_runs() {
    let mut harness = a_drive("Command Group Run");
    press(&mut harness, Key::U);
    let by_key = last_main(&harness);
    harness.clear_speech();
    press(&mut harness, Key::F2);
    open_group(&mut harness, "Road and stops");
    assert_eq!(harness.focused_label().as_deref(), Some("Road ahead"));
    harness.select_menu_item("Road ahead");
    assert!(harness.state_is::<DrivingState>());
    assert_eq!(last_main(&harness), by_key);
}

#[test]
fn escape_from_a_group_lands_on_its_row_in_the_top_list() {
    let mut harness = a_drive("Command Group Back");
    press(&mut harness, Key::F2);
    open_group(&mut harness, "Hours and clock");
    assert_eq!(harness.focused_label().as_deref(), Some("Clock"));
    harness.clear_speech();
    press(&mut harness, Key::Escape);
    assert!(harness.state_is::<DrivingCommandsState>());
    assert_eq!(harness.menu_labels(), TOP);
    assert_eq!(harness.focused_label().as_deref(), Some("Hours and clock"));
    let said = harness.transcript();
    assert!(said.iter().any(|l| l == "Driving commands."), "{said:?}");
    // A second Escape returns to driving.
    press(&mut harness, Key::Escape);
    assert!(harness.state_is::<DrivingState>());
}

#[test]
fn radio_volume_changes_in_place_and_the_list_stays_open() {
    let mut harness = a_drive("Command Radio Volume");
    harness.app.ctx.settings.radio_volume = 0.5;
    press(&mut harness, Key::F2);
    open_group(&mut harness, "Radio and CB");
    while !harness
        .focused_label()
        .is_some_and(|label| label.starts_with("Radio volume"))
    {
        press(&mut harness, Key::Down);
    }
    assert_eq!(
        harness.focused_label().as_deref(),
        Some("Radio volume: 50 percent")
    );
    press(&mut harness, Key::Right);
    press(&mut harness, Key::Right);
    assert!(harness.state_is::<DrivingCommandsState>());
    let raised = harness.app.ctx.settings.radio_volume;
    assert!(raised > 0.5, "{raised}");
    assert_eq!(
        harness.focused_label(),
        Some(format!(
            "Radio volume: {} percent",
            (raised * 100.0).round() as i64
        ))
    );
    press(&mut harness, Key::Left);
    assert!(harness.app.ctx.settings.radio_volume < raised);
    // Enter reads it again and keeps the list.
    press(&mut harness, Key::Return);
    assert!(harness.state_is::<DrivingCommandsState>());
}

#[test]
fn a_shift_row_on_the_keyboard_still_needs_the_clutch() {
    let mut harness = a_drive("Command Shift Grind");
    harness.app.ctx.settings.automatic_transmission = false;
    harness.with_drive(|drive, _| {
        drive.trip.truck.start_engine();
        drive.trip.truck.transmission.automatic = false;
        drive.trip.truck.transmission.gear = 0;
    });
    press(&mut harness, Key::F2);
    open_group(&mut harness, "Gears and engine brake");
    harness.select_menu_item("Shift up");
    assert!(harness.state_is::<DrivingState>());
    assert!(
        harness
            .transcript()
            .iter()
            .any(|l| l.starts_with("Grinding gears")),
        "{:?}",
        harness.transcript()
    );
}

#[test]
fn a_row_returns_to_the_drive_and_answers_as_its_key_does() {
    let mut harness = a_drive("Command Speed");
    press(&mut harness, Key::Space);
    let by_key = last_main(&harness);
    assert!(!by_key.is_empty());

    harness.clear_speech();
    press(&mut harness, Key::F2);
    open_group(&mut harness, "Speed and cruise");
    harness.select_menu_item(Action::Speed.label());
    assert!(harness.state_is::<DrivingState>());
    assert_eq!(last_main(&harness), by_key);
}

#[test]
fn a_row_that_opens_a_screen_opens_it_over_the_drive() {
    let mut harness = a_drive("Command Status");
    press(&mut harness, Key::F2);
    harness.select_menu_item(Action::Status.label());
    assert!(harness.state_is::<DrivingStatusState>());
    press(&mut harness, Key::Escape);
    assert!(harness.state_is::<DrivingState>());
}

#[test]
fn escape_closes_the_list_without_a_command() {
    let mut harness = a_drive("Command Escape");
    press(&mut harness, Key::F2);
    harness.clear_speech();
    press(&mut harness, Key::Escape);
    assert!(harness.state_is::<DrivingState>());
    assert_eq!(last_main(&harness), "Back to driving.");
}

#[test]
fn the_copy_row_copies_what_was_said_before_the_list_opened() {
    let mut harness = a_drive("Command Copy");
    harness.app.ctx.say("Weigh station in 2 miles.");
    press(&mut harness, Key::F2);
    harness.clear_speech();
    harness.select_menu_item(Action::CopyLastSpoken.label());
    assert!(harness.state_is::<DrivingState>());
    assert_eq!(
        harness.app.ctx.clipboard.get_text().as_deref(),
        Some("Weigh station in 2 miles.")
    );
    assert_eq!(last_main(&harness), "Copied.");
}

#[test]
fn the_copy_key_in_the_open_list_copies_the_heading_then_the_focused_row() {
    use freight_fate::states::base::{InputEvent, Mods};
    let copy = InputEvent::key_mods(
        Key::C,
        Mods {
            shift: true,
            ctrl: true,
            alt: false,
        },
    );
    let mut harness = a_drive("Command Copy Key");
    press(&mut harness, Key::F2);
    let second = harness.menu_labels().get(1).cloned().unwrap();
    // The first copy gives the line the list opened with.
    harness.key(copy.clone());
    assert!(harness.state_is::<DrivingCommandsState>());
    assert_eq!(
        harness.app.ctx.clipboard.get_text().as_deref(),
        Some("Driving commands.")
    );
    // A focus move copies the focused row.
    press(&mut harness, Key::Down);
    harness.key(copy);
    assert!(harness.state_is::<DrivingCommandsState>());
    assert_eq!(
        harness.app.ctx.clipboard.get_text().unwrap_or_default(),
        format!("{second}.")
    );
}

#[test]
fn f2_cannot_be_given_to_another_control() {
    assert!(reserved_key_reason(&Chord::plain(Key::F2)).is_some());
    assert!(reserved_key_reason(&Chord::alt(Key::F2)).is_some());
}

/// The pad has no F2, so the status menu carries the list; a command run
/// from it answers over the road, not over the status menu.
#[test]
fn the_status_menu_opens_the_list_in_its_own_place() {
    let mut harness = a_drive("Command Status");
    press(&mut harness, Key::Tab);
    assert!(harness.state_is::<DrivingStatusState>());
    harness.select_menu_item("Driving commands");
    assert!(harness.state_is::<DrivingCommandsState>());
    open_group(&mut harness, "Speed and cruise");
    harness.select_menu_item(Action::Speed.label());
    assert!(harness.state_is::<DrivingState>());
}

/// Clock run from the list on a pad: the pointer to the keyboard hours keys
/// is kept for the keyboard, where those keys exist.
#[test]
fn clock_on_a_controller_does_not_point_at_the_keyboard_hours_keys() {
    use ff_core::input_hints::CONTROLLER;
    use freight_fate::controller::fakes::FakePad;
    let mut harness = a_drive("Command Clock Pad");
    let c = &mut harness.app.ctx.controller;
    c.set_enabled(true);
    c.bind_device(Box::new(FakePad::new(0)), "test pad");
    c.set_id_pending(false);
    c.active_device = CONTROLLER;
    harness.press_key(Key::C, None);
    assert!(
        !last_main(&harness).contains("Hours of service moved"),
        "{}",
        last_main(&harness)
    );
    harness.app.ctx.controller.active_device = ff_core::input_hints::KEYBOARD;
    harness.press_key(Key::C, None);
    assert!(
        last_main(&harness).contains("Hours of service moved"),
        "{}",
        last_main(&harness)
    );
}
