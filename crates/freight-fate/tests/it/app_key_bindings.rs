//! Moving a driving control to another key or pad button: the shortcuts
//! screens under Settings, Gameplay, Controls, the table the drive reads,
//! and the spoken hints and F1 help that must name the moved key.

use crate::states_driving_menus_support::{a_drive, drive_ref, key as key_event, last, with_drive};
use crate::states_main_menu_support::*;
use ff_core::input_hints::CONTROLLER;
#[cfg(feature = "agent-server")]
use freight_fate::agent_server::{build_command, Command, KeySpec};
use freight_fate::app::testing::TestApp;
use freight_fate::bindings::Action;
use freight_fate::controller::fakes::FakePad;
use freight_fate::controller::{ControllerAxis, ControllerButton};
use freight_fate::states::base::{InputEvent, Key, Mods};
use freight_fate::states::driving_menu_states::DrivingCommandsState;
use freight_fate::states::main_menu::{
    help_page, help_pages, render_help_line, SettingsCategoryState, ShortcutDevice, ShortcutsState,
    HELP_PAGES,
};

type Shortcuts = ShortcutsState;

fn open_keyboard_shortcuts(app: &mut TestApp) {
    app.push_state(SettingsCategoryState::new("controls"));
    select::<SettingsCategoryState>(app, "Keyboard shortcuts");
    assert!(is::<Shortcuts>(app));
}

fn force_controller(app: &mut TestApp) {
    let c = &mut app.ctx.controller;
    c.set_enabled(true);
    c.bind_device(Box::new(FakePad::new(0)), "test pad");
    c.set_id_pending(false);
    c.active_device = CONTROLLER;
}

fn button(button: ControllerButton) -> InputEvent {
    InputEvent::ControllerButtonDown {
        button,
        instance_id: 0,
    }
}

// -- the keyboard screen ----------------------------------------------------------------

#[test]
fn test_controls_screen_lists_the_two_shortcut_screens() {
    let mut app = TestApp::new();
    app.push_state(SettingsCategoryState::new("controls"));
    let rows = labels::<SettingsCategoryState>(&app);
    assert!(rows.contains(&"Keyboard shortcuts".to_string()), "{rows:?}");
    assert!(rows.contains(&"Controller buttons".to_string()), "{rows:?}");
    // Back stays last.
    assert_eq!(rows.last().map(String::as_str), Some("Back"));
}

#[test]
fn test_keyboard_screen_names_every_control_with_its_key() {
    let mut app = TestApp::new();
    open_keyboard_shortcuts(&mut app);
    let rows = labels::<Shortcuts>(&app);
    assert_eq!(rows[0], "Accelerate: the Up arrow");
    assert!(
        rows.contains(&"Engine on or off: E".to_string()),
        "{rows:?}"
    );
    assert!(rows.contains(&"Resume the last cruise speed: Shift K".to_string()));
    assert!(rows.contains(&"The state: Alt 1".to_string()));
    // Pad-only controls stay off the keyboard list.
    assert!(
        !rows.iter().any(|r| r.starts_with("Cruise target up")),
        "{rows:?}"
    );
    assert!(rows
        .iter()
        .any(|r| r.starts_with("Reset every keyboard shortcut")));
    assert_eq!(rows.last().map(String::as_str), Some("Back"));
}

#[test]
fn test_enter_then_a_key_moves_the_control_and_saves_it() {
    let mut app = TestApp::new();
    open_keyboard_shortcuts(&mut app);
    move_to::<Shortcuts>(&mut app, "Engine on or off");
    app.clear_speech();
    key(&mut app, Key::Return);
    assert_eq!(
        last(&app),
        "Press the new key for Engine on or off. Escape keeps E."
    );
    assert_eq!(
        with_state::<Shortcuts, _>(&app, |s, _| s.capturing()),
        Some(Action::Engine)
    );

    key(&mut app, Key::Backslash);
    assert_eq!(last(&app), "Engine on or off is now backslash.");
    assert_eq!(with_state::<Shortcuts, _>(&app, |s, _| s.capturing()), None);
    assert_eq!(
        current_label::<Shortcuts>(&app),
        "Engine on or off: backslash"
    );
    assert_eq!(app.ctx.settings.key_bindings, "engine=backslash");
    assert_eq!(
        app.ctx.bindings.action_for(Key::Backslash, Mods::NONE),
        Some(Action::Engine)
    );
    assert_eq!(app.ctx.bindings.action_for(Key::E, Mods::NONE), None);
    // It reached the settings file, not only the running game.
    let on_disk = ff_core::settings::Settings::load();
    assert_eq!(on_disk.key_bindings, "engine=backslash");
}

#[test]
fn test_a_chord_is_captured_whole() {
    let mut app = TestApp::new();
    open_keyboard_shortcuts(&mut app);
    move_to::<Shortcuts>(&mut app, "Horn");
    key(&mut app, Key::Return);
    // The modifier on its own is half a chord and waits.
    app.dispatch_to_state(&InputEvent::key_mods(Key::LAlt, Mods::ALT));
    assert_eq!(
        with_state::<Shortcuts, _>(&app, |s, _| s.capturing()),
        Some(Action::Horn)
    );
    app.dispatch_to_state(&InputEvent::key_mods(Key::Backslash, Mods::ALT));
    assert_eq!(last(&app), "Horn is now Alt backslash.");
    assert_eq!(app.ctx.settings.key_bindings, "horn=alt+backslash");
}

#[test]
fn test_a_taken_key_is_refused_by_name_and_escape_keeps_the_old_one() {
    let mut app = TestApp::new();
    open_keyboard_shortcuts(&mut app);
    move_to::<Shortcuts>(&mut app, "Engine on or off");
    key(&mut app, Key::Return);
    key(&mut app, Key::T);
    assert_eq!(
        last(&app),
        "T is already Rest stop. Press another, or keep E with Escape."
    );
    assert_eq!(
        with_state::<Shortcuts, _>(&app, |s, _| s.capturing()),
        Some(Action::Engine)
    );
    key(&mut app, Key::Return);
    assert_eq!(
        last(&app),
        "Enter confirms. Press another, or keep E with Escape."
    );
    key(&mut app, Key::Escape);
    assert_eq!(last(&app), "Kept E.");
    assert_eq!(with_state::<Shortcuts, _>(&app, |s, _| s.capturing()), None);
    assert_eq!(app.ctx.settings.key_bindings, "");
    // Escape while not capturing leaves the screen, as on every menu.
    key(&mut app, Key::Escape);
    assert!(is::<SettingsCategoryState>(&app));
}

#[test]
fn test_reset_puts_every_key_back() {
    let mut app = TestApp::new();
    app.ctx.settings.key_bindings = "engine=z;fuel=alt+q".to_string();
    app.ctx.apply_bindings();
    open_keyboard_shortcuts(&mut app);
    assert!(labels::<Shortcuts>(&app).contains(&"Fuel: Alt Q".to_string()));
    select::<Shortcuts>(&mut app, "Reset every keyboard shortcut");
    assert_eq!(
        last(&app),
        "Every keyboard shortcut is back to its default."
    );
    assert_eq!(app.ctx.settings.key_bindings, "");
    assert!(labels::<Shortcuts>(&app).contains(&"Fuel: F".to_string()));
}

// -- the controller screen ---------------------------------------------------------------

#[test]
fn test_controller_screen_captures_a_button_on_either_layer() {
    let mut app = TestApp::new();
    force_controller(&mut app);
    app.push_state(ShortcutsState::new(ShortcutDevice::Controller));
    let rows = labels::<Shortcuts>(&app);
    assert!(
        rows.contains(&"Engine on or off: right bumper plus A".to_string()),
        "{rows:?}"
    );
    assert!(rows.contains(&"Horn: the left stick click".to_string()));
    assert!(
        !rows.iter().any(|r| r.starts_with("Accelerate")),
        "{rows:?}"
    );

    move_to::<Shortcuts>(&mut app, "Engine on or off");
    key(&mut app, Key::Return);
    assert!(last(&app).starts_with("Press the new button for Engine on or off"));
    // The A button is the plain-layer shift up: refused by name.
    app.dispatch_controller(&button(ControllerButton::A));
    assert_eq!(
        last(&app),
        "The A button is already Shift up. Press another, or keep right bumper plus A with Escape."
    );
    // Start on the plain layer is pause and stays so.
    app.dispatch_controller(&button(ControllerButton::Start));
    assert!(last(&app).starts_with("Start is pause."), "{}", last(&app));
    // A paddle nobody uses lands.
    app.dispatch_controller(&button(ControllerButton::Paddle1));
    assert_eq!(last(&app), "Engine on or off is now paddle 1.");
    assert_eq!(app.ctx.settings.pad_bindings, "engine=paddle_1");
    assert_eq!(
        app.ctx
            .bindings
            .pad_action_for(ControllerButton::Paddle1, false),
        Some(Action::Engine)
    );
    assert_eq!(
        app.ctx.bindings.pad_action_for(ControllerButton::A, true),
        None
    );

    // With the right bumper held, the same button is the second layer.
    move_to::<Shortcuts>(&mut app, "Horn");
    key(&mut app, Key::Return);
    app.dispatch_controller(&button(ControllerButton::RightShoulder));
    assert_eq!(
        with_state::<Shortcuts, _>(&app, |s, _| s.capturing()),
        Some(Action::Horn)
    );
    app.dispatch_controller(&button(ControllerButton::Paddle2));
    assert_eq!(last(&app), "Horn is now right bumper plus paddle 2.");
    assert_eq!(
        app.ctx.settings.pad_bindings,
        "engine=paddle_1;horn=mod+paddle_2"
    );
}

#[test]
fn test_controller_screen_lists_radio_rows_and_captures_a_stick_flick() {
    let mut app = TestApp::new();
    force_controller(&mut app);
    app.push_state(ShortcutsState::new(ShortcutDevice::Controller));
    let rows = labels::<Shortcuts>(&app);
    assert!(rows.contains(&"Tune to the next radio station: right stick right".to_string()));
    assert!(rows.contains(&"Radio on or off: right bumper plus right stick up".to_string()));
    move_to::<Shortcuts>(&mut app, "Tune to the next radio station");
    key(&mut app, Key::Return);
    app.dispatch_controller(&InputEvent::axis(ControllerAxis::RightX, 32767));
    assert_eq!(last(&app), "Kept right stick right.");
}

// -- the drive reads the table ----------------------------------------------------------

#[test]
fn test_a_moved_key_works_at_the_wheel_and_the_old_one_does_not() {
    let mut app = TestApp::new();
    app.ctx.settings.key_bindings = "fuel=z".to_string();
    app.ctx.apply_bindings();
    let d = a_drive(&mut app);
    app.clear_speech();
    with_drive(&d, |d| d.handle_key_event(&mut app.ctx, &key_event(Key::F)));
    assert!(!last(&app).starts_with("Fuel"), "{}", last(&app));
    with_drive(&d, |d| d.handle_key_event(&mut app.ctx, &key_event(Key::Z)));
    assert!(last(&app).starts_with("Fuel"), "{}", last(&app));
}

#[test]
fn test_a_moved_chord_does_not_shadow_the_bare_key() {
    // Shift K resumes cruise; with it moved to Alt Z, Shift K falls back to
    // K's own action rather than going dead, the way an unbound chord always
    // has (Shift for the clutch never stopped W shifting up).
    let mut app = TestApp::new();
    app.ctx.settings.key_bindings = "cruise_resume=alt+z".to_string();
    app.ctx.apply_bindings();
    assert_eq!(
        app.ctx.bindings.action_for(Key::K, Mods::SHIFT),
        Some(Action::Cruise)
    );
    assert_eq!(
        app.ctx.bindings.action_for(Key::Z, Mods::ALT),
        Some(Action::CruiseResume)
    );
}

#[test]
fn test_a_moved_pedal_is_the_one_polled() {
    let mut app = TestApp::new();
    app.ctx.settings.key_bindings = "accelerate=w".to_string();
    app.ctx.apply_bindings();
    app.ctx.input.press(Key::Up, Mods::NONE);
    assert!(!app.ctx.bindings.pressed(&app.ctx.input, Action::Accelerate));
    app.ctx.input.release(Key::Up, Mods::NONE);
    app.ctx.input.press(Key::W, Mods::NONE);
    assert!(app.ctx.bindings.pressed(&app.ctx.input, Action::Accelerate));
    // A held chord needs its modifier down too.
    app.ctx.input.release(Key::W, Mods::NONE);
    app.ctx.settings.key_bindings = "accelerate=alt+x".to_string();
    app.ctx.apply_bindings();
    app.ctx.input.press(Key::X, Mods::NONE);
    assert!(!app.ctx.bindings.pressed(&app.ctx.input, Action::Accelerate));
    app.ctx.input.release(Key::X, Mods::NONE);
    app.ctx.input.press(Key::LAlt, Mods::ALT);
    app.ctx.input.press(Key::X, Mods::ALT);
    assert!(app.ctx.bindings.pressed(&app.ctx.input, Action::Accelerate));
}

#[test]
fn test_a_moved_pad_button_works_at_the_wheel() {
    let mut app = TestApp::new();
    force_controller(&mut app);
    app.ctx.settings.pad_bindings = "fuel=paddle_1".to_string();
    app.ctx.apply_bindings();
    let d = a_drive(&mut app);
    app.clear_speech();
    // Right bumper plus B was fuel; it is nothing now.
    app.ctx.controller.modifier = true;
    with_drive(&d, |d| {
        d.handle_controller_event(&mut app.ctx, &button(ControllerButton::B))
    });
    assert!(!last(&app).starts_with("Fuel"), "{}", last(&app));
    app.ctx.controller.modifier = false;
    with_drive(&d, |d| {
        d.handle_controller_event(&mut app.ctx, &button(ControllerButton::Paddle1))
    });
    assert!(last(&app).starts_with("Fuel"), "{}", last(&app));
}

#[test]
fn test_rebound_radio_button_works_at_the_wheel() {
    let mut app = TestApp::new();
    force_controller(&mut app);
    app.ctx.settings.pad_bindings = "radio=paddle_1".to_string();
    app.ctx.apply_bindings();
    let d = a_drive(&mut app);
    with_drive(&d, |d| {
        d.handle_controller_event(&mut app.ctx, &button(ControllerButton::Paddle1))
    });
    assert!(with_drive(&d, |d| d.radio.enabled));
}

// -- what the game says about the moved key ---------------------------------------------

#[test]
fn test_spoken_hints_and_f1_help_name_the_moved_key() {
    let mut app = TestApp::new();
    assert_eq!(app.ctx.control_hint("engine"), "E");
    assert_eq!(app.ctx.control_hint("accelerate"), "the Up arrow");
    app.ctx.settings.key_bindings = "engine=z;accelerate=w;fuel=alt+q".to_string();
    app.ctx.apply_bindings();
    assert_eq!(app.ctx.control_hint("engine"), "Z");
    assert_eq!(app.ctx.control_hint("accelerate"), "W");
    // A hint for a fixed control keeps the table's wording.
    assert_eq!(app.ctx.control_hint("confirm"), "Enter");

    let d = a_drive(&mut app);
    app.clear_speech();
    with_drive(&d, |d| d.speak_keyboard_help(&mut app.ctx));
    let help = last(&app);
    assert!(
        help.contains("Hold W to accelerate, the Down arrow to brake."),
        "{help}"
    );
    assert!(help.contains("Z starts the engine"), "{help}");
    assert!(help.contains("Alt Q fuel."), "{help}");
    assert!(!help.contains("E starts the engine"), "{help}");
    assert!(help.contains("Keyboard shortcuts."), "{help}");
}

#[test]
fn test_controller_help_and_hints_name_the_moved_button() {
    let mut app = TestApp::new();
    force_controller(&mut app);
    assert_eq!(app.ctx.control_hint("engine"), "right bumper plus A");
    app.ctx.settings.pad_bindings = "engine=paddle_1".to_string();
    app.ctx.apply_bindings();
    assert_eq!(app.ctx.control_hint("engine"), "paddle 1");

    let d = a_drive(&mut app);
    app.clear_speech();
    with_drive(&d, |d| d.speak_controller_help(&mut app.ctx));
    let help = last(&app);
    assert!(
        help.contains("Paddle 1 starts or stops the engine"),
        "{help}"
    );
    assert!(help.contains("Controller buttons."), "{help}");
    assert!(
        help.contains(
            "Right stick left tunes to the previous station and right stick right to the next."
        ),
        "{help}"
    );
    assert!(
        help.contains("Right stick down and right stick up lower and raise the radio volume."),
        "{help}"
    );
    assert!(
        help.contains("Right bumper plus right stick up turns the radio on or off"),
        "{help}"
    );
    assert!(!help.contains("Hold the right bumper with"), "{help}");
    let status = help.find("opens the status menu").expect("status sentence");
    let radio = help.find("tunes to the previous station").expect("radio");
    let rest = help
        .find("emergency shoulder sleep")
        .expect("rest sentence");
    assert!(rest < status && status < radio, "{help}");
}

#[test]
fn test_command_list_names_a_controller_radio_binding() {
    let mut app = TestApp::new();
    force_controller(&mut app);
    let drive = a_drive(&mut app);
    app.push_state(DrivingCommandsState::with_drive(drive_ref(&drive)));
    move_to::<DrivingCommandsState>(&mut app, "Radio on or off");
    let help = current_help::<DrivingCommandsState>(&app);
    assert!(
        help.contains("On the controller: right bumper plus right stick up."),
        "{help}"
    );
}

#[test]
fn test_the_help_page_points_at_the_shortcut_screens() {
    let app = TestApp::new();
    let joined = help_pages(&app.ctx)
        .into_iter()
        .flat_map(|(_title, lines)| lines)
        .collect::<Vec<String>>()
        .join(" ");
    assert!(
        joined.contains("Keyboard shortcuts or Controller buttons"),
        "{joined}"
    );
}

// -- the How to play pages follow the table and the device --------------------------------

#[test]
fn test_every_help_placeholder_names_a_control() {
    let app = TestApp::new();
    for (title, lines) in help_pages(&app.ctx) {
        for line in lines {
            assert!(!line.contains("{{"), "{title}: {line}");
        }
    }
    // The static text carries placeholders, so a raw read is not a manual.
    assert!(HELP_PAGES
        .iter()
        .flat_map(|(_, lines)| lines.iter())
        .any(|line| line.contains("{{engine}}")));
}

#[test]
fn test_help_pages_name_the_moved_key() {
    let mut app = TestApp::new();
    let (_, lines) = help_page(
        &app.ctx,
        freight_fate::states::main_menu::controls_help_page(),
    );
    assert!(
        lines
            .iter()
            .any(|l| l.starts_with("S speaks the posted speed limit")),
        "{lines:?}"
    );
    app.ctx.settings.key_bindings = "speed_limit=alt+z;engine=f5".to_string();
    app.ctx.apply_bindings();
    let (_, lines) = help_page(
        &app.ctx,
        freight_fate::states::main_menu::controls_help_page(),
    );
    assert!(
        lines
            .iter()
            .any(|l| l.starts_with("Alt Z speaks the posted speed limit")),
        "{lines:?}"
    );
    assert!(
        !lines.iter().any(|l| l.starts_with("S speaks")),
        "{lines:?}"
    );
    assert_eq!(
        render_help_line(&app.ctx, "{{engine}} starts the engine."),
        "F5 starts the engine."
    );
    // A pinned device ignores the one in use; an unknown id reads aloud.
    assert_eq!(
        render_help_line(&app.ctx, "{{pad:engine}} starts it."),
        "Right bumper plus A starts it."
    );
    assert_eq!(
        render_help_line(&app.ctx, "{{teleport}} beams."),
        "{{teleport}} beams."
    );
}

#[test]
fn test_help_pages_follow_the_controller_when_it_is_in_use() {
    let mut app = TestApp::new();
    force_controller(&mut app);
    let (_, lines) = help_page(
        &app.ctx,
        freight_fate::states::main_menu::controls_help_page(),
    );
    // A control the pad has names the button; one it lacks keeps the key.
    assert!(
        lines
            .iter()
            .any(|l| l.starts_with("Right bumper plus X speaks the posted speed limit")),
        "{lines:?}"
    );
    assert!(
        lines
            .iter()
            .any(|l| l.starts_with("The B button speaks your speed")),
        "{lines:?}"
    );
    assert!(
        lines
            .iter()
            .any(|l| l.starts_with("Alt A speaks time at the wheel")),
        "{lines:?}"
    );
    // The How to play screen itself reads the rendered line.
    let mut help = freight_fate::states::main_menu::HelpState::at_page(
        freight_fate::states::main_menu::controls_help_page(),
    );
    app.clear_speech();
    freight_fate::states::base::State::handle_event(
        &mut help,
        &mut app.ctx,
        &InputEvent::key(Key::Down),
    );
    freight_fate::states::base::State::handle_event(
        &mut help,
        &mut app.ctx,
        &InputEvent::key(Key::Down),
    );
    assert!(
        last(&app).starts_with("Right bumper plus X speaks"),
        "{}",
        last(&app)
    );
}

// -- the agent server presses a control by name ------------------------------------------

#[cfg(feature = "agent-server")]
#[test]
fn test_press_tool_resolves_a_control_through_the_players_table() {
    let mut app = TestApp::new();
    let args = serde_json::from_value(serde_json::json!({"key": "cruise_resume"})).unwrap();
    let Command::Press { key, .. } = build_command("press", &args).unwrap() else {
        panic!("not a press");
    };
    assert_eq!(key, KeySpec::Action(Action::CruiseResume));
    assert_eq!(
        key.resolve(&app.ctx.bindings).unwrap(),
        (Key::K, Some('k'), Mods::SHIFT)
    );
    app.ctx.settings.key_bindings = "cruise_resume=f7".to_string();
    app.ctx.apply_bindings();
    assert_eq!(
        key.resolve(&app.ctx.bindings).unwrap(),
        (Key::F7, None, Mods::NONE)
    );
    // A key by name is untouched by the table.
    let plain = KeySpec::Key {
        key: Key::K,
        text: Some('k'),
    };
    assert_eq!(
        plain.resolve(&app.ctx.bindings).unwrap(),
        (Key::K, Some('k'), Mods::NONE)
    );
}
