//! The iPhone and iPad driving gestures (`bindings/touch.rs`,
//! `states/driving_controls/touch.rs`): a gesture at the wheel runs its
//! command directly, the same way the command's key does, and every other
//! screen still gets the key the gesture stands for.

use ff_core::sim::weather::WeatherKind;

use freight_fate::bindings::{Action, Chord, TouchCommand};
use freight_fate::playtest::harness::{PlaytestHarness, StartDelivery};
use freight_fate::states::base::{InputEvent, Key};
use freight_fate::states::driving::DrivingState;
use freight_fate::states::driving_menu_states::DrivingCommandsState;
use freight_fate::states::driving_pause_states::PauseMenuState;
use freight_fate::states::main_menu::{
    InputPracticeState, TouchCommandPickerState, TouchGesturesState,
};
use freight_fate::touch::{Gesture, TouchInput};

const MPH_PER_MPS: f64 = 2.236_936_292_054_402;

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

fn rolling(harness: &mut PlaytestHarness, mph: f64) {
    harness.with_drive(|drive, _| {
        drive.trip.truck.start_engine();
        drive.trip.truck.parking_brake = false;
        drive.trip.truck.velocity_mps = mph / MPH_PER_MPS;
    });
    harness.clear_speech();
}

/// Feed native gestures through the same translator the iOS shell uses.
fn touch(harness: &mut PlaytestHarness, input: &mut TouchInput, gesture: Gesture) {
    for event in input.handle(gesture).events {
        harness.app.handle_event(&event);
    }
}

fn speed_control_on(harness: &mut PlaytestHarness) -> bool {
    harness.with_drive(|drive, _| drive.speed_authority_engaged() || drive.speed_control_armed)
}

/// What pressing `action`'s default key says, and what `read` finds after,
/// on a fresh drive set up by `setup`. Run it before the gesture's harness:
/// a thread holds one game at a time.
fn by_key<R>(
    name: &str,
    setup: impl Fn(&mut PlaytestHarness),
    action: Action,
    read: impl FnOnce(&mut PlaytestHarness) -> R,
) -> (Vec<String>, R) {
    let mut harness = a_drive(name);
    setup(&mut harness);
    let chord = action.default_chords()[0];
    harness.key(InputEvent::KeyDown {
        key: chord.key,
        mods: chord.mods,
        text: None,
        repeat: false,
    });
    let found = read(&mut harness);
    (harness.transcript(), found)
}

#[test]
fn hold_gas_then_tap_a_second_finger_sets_cruise_with_the_pedal_down() {
    let (expected, _) = by_key(
        "Touch Cruise Key",
        |h| rolling(h, 24.0),
        Action::Cruise,
        |_| (),
    );
    let mut harness = a_drive("Touch Cruise");
    rolling(&mut harness, 24.0);
    let mut input = TouchInput::new();
    touch(&mut harness, &mut input, Gesture::HoldUpperBegan);
    assert!(harness.app.ctx.input.physically_down(Key::Up));

    touch(&mut harness, &mut input, Gesture::UpperHoldTap);
    assert!(harness.state_is::<DrivingState>(), "no menu in between");
    assert!(
        speed_control_on(&mut harness),
        "{:#?}",
        harness.transcript()
    );
    assert!(
        harness.app.ctx.input.physically_down(Key::Up),
        "the tap must not lift the pedal"
    );
    assert_eq!(harness.transcript(), expected);

    touch(&mut harness, &mut input, Gesture::HoldEnded);
    assert!(!harness.app.ctx.input.physically_down(Key::Up));
}

#[test]
fn the_cruise_tap_below_the_minimum_answers_as_the_key_does() {
    let (expected, engaged) = by_key(
        "Touch Cruise Slow Key",
        |h| rolling(h, 8.0),
        Action::Cruise,
        speed_control_on,
    );
    let mut harness = a_drive("Touch Cruise Slow");
    rolling(&mut harness, 8.0);
    let mut input = TouchInput::new();
    touch(&mut harness, &mut input, Gesture::HoldUpperBegan);
    touch(&mut harness, &mut input, Gesture::UpperHoldTap);
    assert_eq!(harness.transcript(), expected);
    assert_eq!(speed_control_on(&mut harness), engaged);
}

#[test]
fn a_second_finger_on_the_brake_does_nothing_until_assigned() {
    let parked = |h: &mut PlaytestHarness| {
        h.with_drive(|drive, _| {
            drive.trip.truck.velocity_mps = 0.0;
        });
        h.clear_speech();
    };
    // The defaults: the brake-hold tap and double tap are free.
    let mut harness = a_drive("Touch Brake Free");
    parked(&mut harness);
    let mut input = TouchInput::new();
    touch(&mut harness, &mut input, Gesture::HoldLowerBegan);
    let before = harness.with_drive(|d, _| (d.trip.truck.engine_on, d.trip.truck.parking_brake));
    touch(&mut harness, &mut input, Gesture::LowerHoldDoubleTap);
    touch(&mut harness, &mut input, Gesture::LowerHoldTap);
    let after = harness.with_drive(|d, _| (d.trip.truck.engine_on, d.trip.truck.parking_brake));
    assert_eq!(before, after);
    assert!(
        harness.transcript().is_empty(),
        "{:#?}",
        harness.transcript()
    );
    drop(harness);

    let (expected, engine_by_key) = by_key("Touch Engine Key", parked, Action::Engine, |h| {
        h.with_drive(|d, _| d.trip.truck.engine_on)
    });
    let mut harness = a_drive("Touch Engine");
    parked(&mut harness);
    harness.app.ctx.bindings.set_touch_command(
        Gesture::LowerHoldDoubleTap,
        TouchCommand::Action(Action::Engine),
    );
    let mut input = TouchInput::new();
    touch(&mut harness, &mut input, Gesture::HoldLowerBegan);
    assert!(harness.app.ctx.input.physically_down(Key::Down));
    let engine_before = harness.with_drive(|d, _| d.trip.truck.engine_on);
    touch(&mut harness, &mut input, Gesture::LowerHoldDoubleTap);
    // Same answer as the key, with gas named as what releases the brake.
    let expected: Vec<String> = expected
        .iter()
        .map(|line| {
            line.replace(
                "P releases the parking brake.",
                "swipe up and hold to release the parking brake and drive.",
            )
            .replace("Air ready. swipe up", "Air ready. Swipe up")
        })
        .collect();
    assert_eq!(harness.transcript(), expected);
    let engine_after = harness.with_drive(|d, _| d.trip.truck.engine_on);
    assert_eq!(engine_after, engine_by_key);
    assert_ne!(engine_before, engine_after);

    assert!(harness.app.ctx.input.physically_down(Key::Down));
    assert!(harness.state_is::<DrivingState>());
    drop(harness);

    let (expected, brake_by_key) = by_key("Touch Brake Key", parked, Action::ParkingBrake, |h| {
        h.with_drive(|d, _| d.trip.truck.parking_brake)
    });
    let mut harness = a_drive("Touch Brake");
    parked(&mut harness);
    harness.app.ctx.bindings.set_touch_command(
        Gesture::LowerHoldTap,
        TouchCommand::Action(Action::ParkingBrake),
    );
    let mut input = TouchInput::new();
    touch(&mut harness, &mut input, Gesture::HoldLowerBegan);
    touch(&mut harness, &mut input, Gesture::LowerHoldTap);
    assert_eq!(harness.transcript(), expected);
    assert_eq!(
        harness.with_drive(|d, _| d.trip.truck.parking_brake),
        brake_by_key
    );
    assert!(harness.app.ctx.input.physically_down(Key::Down));
}

#[test]
fn magic_tap_pauses_the_drive() {
    let mut harness = a_drive("Touch Pause");
    let mut input = TouchInput::new();
    touch(&mut harness, &mut input, Gesture::MagicTap);
    assert!(harness.state_is::<PauseMenuState>());
    // The pause menu answers it by resuming.
    touch(&mut harness, &mut input, Gesture::MagicTap);
    assert!(harness.state_is::<DrivingState>());
}

#[test]
fn emergency_and_horn_holds_follow_rebound_keys() {
    let mut harness = a_drive("Touch held bindings");
    assert!(matches!(
        harness
            .app
            .ctx
            .bindings
            .set_chord(Action::EmergencyBrake, Chord::plain(Key::Backslash)),
        freight_fate::bindings::Rebind::Done
    ));
    let mut input = TouchInput::new();
    touch(&mut harness, &mut input, Gesture::EmergencyBrakeHoldBegan);
    assert!(harness.app.ctx.input.physically_down(Key::Backslash));
    touch(&mut harness, &mut input, Gesture::HoldEnded);
    assert!(!harness.app.ctx.input.physically_down(Key::Backslash));
    touch(&mut harness, &mut input, Gesture::HornHoldBegan);
    assert!(harness.app.ctx.input.physically_down(Key::H));
    touch(&mut harness, &mut input, Gesture::HoldEnded);
    assert!(!harness.app.ctx.input.physically_down(Key::H));
}

#[test]
fn touch_practice_takes_pedal_and_bound_holds_before_they_press_keys() {
    let mut harness = a_drive("Touch Practice Holds");
    assert!(matches!(
        harness
            .app
            .ctx
            .bindings
            .set_chord(Action::EmergencyBrake, Chord::plain(Key::Backslash)),
        freight_fate::bindings::Rebind::Done
    ));
    harness.app.push_state(InputPracticeState::gestures());
    harness.clear_speech();

    harness.app.dispatch_gesture(Gesture::HoldUpperBegan);
    assert!(
        harness
            .transcript()
            .iter()
            .any(|line| line == "Swipe up and hold: gas. Lift to coast."),
        "{:?}",
        harness.transcript()
    );
    assert!(!harness.app.ctx.input.physically_down(Key::Up));

    harness.clear_speech();
    harness
        .app
        .dispatch_gesture(Gesture::EmergencyBrakeHoldBegan);
    assert!(
        harness
            .transcript()
            .iter()
            .any(|line| line == "Emergency brake."),
        "{:?}",
        harness.transcript()
    );
    assert!(!harness.app.ctx.input.physically_down(Key::Backslash));
}

#[test]
fn touch_practice_names_every_fixed_gesture_command() {
    let mut harness = a_drive("Touch Practice Fixed");
    harness.app.push_state(InputPracticeState::gestures());

    harness.clear_speech();
    harness.app.dispatch_gesture(Gesture::SwipeRight);
    assert_eq!(
        harness.transcript(),
        vec!["Flick right: Change lanes right."]
    );

    for gesture in (0..=47).filter_map(Gesture::from_code) {
        if matches!(
            gesture,
            Gesture::Escape
                | Gesture::TwoFingerSwipeDown
                | Gesture::HoldEnded
                | Gesture::SecondSteerEnded
                | Gesture::KeyboardConnected
                | Gesture::KeyboardDisconnected
        ) {
            continue;
        }
        harness.clear_speech();
        harness.app.dispatch_gesture(gesture);
        let spoken = harness.transcript();
        assert!(
            !spoken.is_empty() && spoken.iter().all(|line| !line.contains("No command")),
            "{gesture:?}: {spoken:?}"
        );
    }
}

#[test]
fn gas_hold_swipe_left_changes_lanes_with_full_lane_keeping() {
    let mut harness = a_drive("Touch Lane Change");
    rolling(&mut harness, 55.0);
    harness.app.ctx.settings.lane_keeping = "full".to_string();
    harness.with_drive(|drive, _| {
        drive.lane.lane_count = 2;
        drive.lane.lane = 0;
    });
    let mut input = TouchInput::new();

    touch(&mut harness, &mut input, Gesture::HoldUpperBegan);
    touch(&mut harness, &mut input, Gesture::UpperHoldSwipeLeft);

    assert_eq!(
        harness.with_drive(|drive, _| drive.lane_change_target),
        Some(1)
    );
}

#[test]
fn single_gestures_run_their_default_commands() {
    for (gesture, action) in [
        (Gesture::Tap, Action::Speed),
        (Gesture::ThreeFingerSwipeUp, Action::Route),
    ] {
        let (expected, _) = by_key("Touch Single Key", |h| rolling(h, 30.0), action, |_| ());
        let mut harness = a_drive("Touch Single");
        rolling(&mut harness, 30.0);
        let mut input = TouchInput::new();
        touch(&mut harness, &mut input, gesture);
        assert_eq!(harness.transcript(), expected, "{gesture:?}");
    }
    // The road ahead: each drive rolls its own ramp controls, so only the
    // readout itself is compared.
    let mut harness = a_drive("Touch Road Ahead");
    // A few miles short of the first stop, so the road ahead has one.
    harness.with_drive(|drive, _| {
        let first = drive.trip.stops.first().map(|stop| stop.at_mi);
        let first = first.expect("the assigned route carries en-route stops");
        drive.trip.position_mi = (first - 3.0).max(0.0);
    });
    let mut input = TouchInput::new();
    touch(&mut harness, &mut input, Gesture::ThreeFingerSwipeDown);
    let said = harness.transcript();
    assert!(said.iter().any(|l| l.starts_with("Coming up")), "{said:?}");
}

#[test]
fn the_fixed_gestures_keep_their_keys_at_the_wheel() {
    let mut harness = a_drive("Touch Fixed");
    let mut input = TouchInput::new();
    touch(&mut harness, &mut input, Gesture::ThreeFingerTap);
    assert!(harness.state_is::<DrivingCommandsState>());
    // Back out with the two-finger swipe down, which is Escape.
    touch(&mut harness, &mut input, Gesture::TwoFingerSwipeDown);
    assert!(harness.state_is::<DrivingState>());
}

#[test]
fn a_gesture_moved_to_another_command_runs_that_one() {
    let (expected, _) = by_key("Touch Moved Key", |_| {}, Action::Fuel, |_| ());
    let mut harness = a_drive("Touch Moved");
    harness.app.ctx.settings.touch_bindings = "tap=fuel;magic_tap=none".to_string();
    harness.app.ctx.apply_bindings();
    let mut input = TouchInput::new();
    touch(&mut harness, &mut input, Gesture::Tap);
    assert_eq!(harness.transcript(), expected);

    harness.clear_speech();
    touch(&mut harness, &mut input, Gesture::MagicTap);
    assert!(harness.state_is::<DrivingState>(), "set to nothing");
    assert!(
        harness.transcript().is_empty(),
        "{:?}",
        harness.transcript()
    );
}

#[test]
fn off_the_road_a_gesture_is_still_its_key() {
    let mut harness = PlaytestHarness::new();
    harness.app.push_state(TouchGesturesState::new());
    harness.clear_speech();
    let mut input = TouchInput::new();
    // Magic tap reaches the menu as itself, and presses no key there.
    let events = input.handle(Gesture::MagicTap).events;
    assert_eq!(events, vec![InputEvent::Gesture(Gesture::MagicTap)]);
    let first = harness.focused_label();
    touch(&mut harness, &mut input, Gesture::MagicTap);
    assert!(harness.state_is::<TouchGesturesState>());
    assert_eq!(harness.focused_label(), first);
    touch(&mut harness, &mut input, Gesture::SwipeRight);
    assert!(harness.state_is::<TouchGesturesState>());
    assert_ne!(
        harness.focused_label(),
        first,
        "flick right is Down on a menu"
    );
    touch(&mut harness, &mut input, Gesture::SwipeLeft);
    assert_eq!(harness.focused_label(), first);
}

#[test]
fn the_touch_gestures_screen_moves_a_gesture_and_saves_it() {
    let mut harness = PlaytestHarness::new();
    harness.app.push_state(TouchGesturesState::new());
    let labels = harness.menu_labels();
    for wanted in [
        "Tap: Speed",
        "Gas hold, tap with a second finger: Cruise",
        "Brake hold, double tap with a second finger: Nothing",
        "Two-finger double tap: Pause",
        "Reset every touch gesture to its default",
    ] {
        assert!(
            labels.iter().any(|l| l == wanted),
            "{wanted:?} not in {labels:#?}"
        );
    }
    assert!(!labels.iter().any(|l| l.starts_with("Swipe left")));

    harness.select_menu_item("Tap: Speed");
    assert!(harness.state_is::<TouchCommandPickerState>());
    assert!(
        harness
            .menu_labels()
            .iter()
            .all(|l| l != Action::Accelerate.label()),
        "a held control cannot go on a tap"
    );
    harness.select_menu_item(Action::Fuel.label());
    assert!(harness.state_is::<TouchGesturesState>());
    assert_eq!(harness.app.ctx.settings.touch_bindings, "tap=fuel");
    assert_eq!(
        harness.app.ctx.bindings.touch_command(Gesture::Tap),
        Some(TouchCommand::Action(Action::Fuel))
    );
    assert!(harness
        .transcript()
        .iter()
        .any(|l| l == &format!("Tap now runs {}.", Action::Fuel.label())));

    harness.select_menu_item("Reset every touch gesture to its default");
    assert_eq!(harness.app.ctx.settings.touch_bindings, "");
}

fn said(harness: &PlaytestHarness, text: &str) -> bool {
    harness
        .transcript()
        .iter()
        .any(|line| line.to_lowercase().contains(text))
}

#[test]
fn a_deep_swipe_at_a_stop_sets_the_parking_brake_and_gas_releases_it() {
    let mut harness = a_drive("Touch Deep Swipe Stopped");
    rolling(&mut harness, 0.0);
    let mut input = TouchInput::new();

    touch(&mut harness, &mut input, Gesture::HoldLowerBegan);
    touch(&mut harness, &mut input, Gesture::DeepSwipeDownBegan);
    assert!(harness.with_drive(|drive, _| drive.trip.truck.parking_brake));
    assert!(
        said(&harness, "parking brake set"),
        "{:?}",
        harness.transcript()
    );
    touch(&mut harness, &mut input, Gesture::HoldEnded);
    harness.with_drive(|drive, _| drive.trip.truck.set_air_pressure_psi(120.0));

    harness.clear_speech();
    touch(&mut harness, &mut input, Gesture::HoldUpperBegan);
    assert!(!harness.with_drive(|drive, _| drive.trip.truck.parking_brake));
    assert!(
        said(&harness, "parking brake released"),
        "{:?}",
        harness.transcript()
    );
    assert!(harness.app.ctx.input.physically_down(Key::Up));
}

#[test]
fn a_deep_swipe_while_moving_holds_the_emergency_brake() {
    let mut harness = a_drive("Touch Deep Swipe Moving");
    rolling(&mut harness, 40.0);
    assert!(matches!(
        harness
            .app
            .ctx
            .bindings
            .set_chord(Action::EmergencyBrake, Chord::plain(Key::Backslash)),
        freight_fate::bindings::Rebind::Done
    ));
    let mut input = TouchInput::new();

    touch(&mut harness, &mut input, Gesture::HoldLowerBegan);
    touch(&mut harness, &mut input, Gesture::DeepSwipeDownBegan);
    assert!(!harness.app.ctx.input.physically_down(Key::Down));
    assert!(harness.app.ctx.input.physically_down(Key::Backslash));
    assert!(!harness.with_drive(|drive, _| drive.trip.truck.parking_brake));
    touch(&mut harness, &mut input, Gesture::HoldEnded);
    assert!(!harness.app.ctx.input.physically_down(Key::Backslash));
}

#[test]
fn a_two_finger_turn_starts_and_stops_the_engine() {
    let mut harness = a_drive("Touch Engine Key");
    harness.with_drive(|drive, _| drive.trip.truck.engine_on = false);
    harness.app.dispatch_gesture(Gesture::RotateRight);
    assert!(harness.with_drive(|drive, _| drive.trip.truck.engine_on));
    harness.app.dispatch_gesture(Gesture::RotateRight);
    assert!(harness.with_drive(|drive, _| drive.trip.truck.engine_on));
    assert!(
        said(&harness, "already running"),
        "{:?}",
        harness.transcript()
    );
    harness.app.dispatch_gesture(Gesture::RotateLeft);
    assert!(!harness.with_drive(|drive, _| drive.trip.truck.engine_on));
}

#[test]
fn steering_holds_follow_the_lane_keeping_rule() {
    let mut harness = a_drive("Touch Steering Holds");
    rolling(&mut harness, 45.0);
    let mut input = TouchInput::new();

    harness.app.ctx.settings.lane_keeping = "full".to_string();
    touch(&mut harness, &mut input, Gesture::HoldLeftBegan);
    assert!(!harness.app.ctx.input.physically_down(Key::Left));
    assert!(
        said(&harness, "flick left or right"),
        "{:?}",
        harness.transcript()
    );
    touch(&mut harness, &mut input, Gesture::HoldEnded);

    for mode in ["off", "partial"] {
        harness.app.ctx.settings.lane_keeping = mode.to_string();
        touch(&mut harness, &mut input, Gesture::HoldRightBegan);
        assert!(harness.app.ctx.input.physically_down(Key::Right), "{mode}");
        touch(&mut harness, &mut input, Gesture::HoldEnded);
        assert!(!harness.app.ctx.input.physically_down(Key::Right), "{mode}");
    }

    touch(&mut harness, &mut input, Gesture::HoldUpperBegan);
    touch(&mut harness, &mut input, Gesture::SecondSteerLeftBegan);
    assert!(harness.app.ctx.input.physically_down(Key::Left));
    touch(&mut harness, &mut input, Gesture::SecondSteerEnded);
    assert!(!harness.app.ctx.input.physically_down(Key::Left));
    assert!(harness.app.ctx.input.physically_down(Key::Up));
}

#[test]
fn a_still_hold_straightens_the_wheel_until_it_lifts() {
    let mut harness = a_drive("Touch Straighten");
    assert!(matches!(
        harness
            .app
            .ctx
            .bindings
            .set_chord(Action::Straighten, Chord::plain(Key::Backslash)),
        freight_fate::bindings::Rebind::Done
    ));
    let mut input = TouchInput::new();
    touch(&mut harness, &mut input, Gesture::StillHoldBegan);
    assert!(harness.app.ctx.input.physically_down(Key::Backslash));
    touch(&mut harness, &mut input, Gesture::HoldEnded);
    assert!(!harness.app.ctx.input.physically_down(Key::Backslash));
}

#[test]
fn a_second_finger_shift_flick_in_automatic_does_nothing_and_says_nothing() {
    let mut harness = a_drive("Touch Automatic Gear");
    rolling(&mut harness, 30.0);
    harness.with_drive(|drive, _| drive.trip.truck.transmission.automatic = true);
    let before = harness.with_drive(|drive, _| drive.gear_text());
    let mut input = TouchInput::new();
    touch(&mut harness, &mut input, Gesture::HoldUpperBegan);
    harness.clear_speech();
    for gesture in [Gesture::UpperHoldSwipeUp, Gesture::UpperHoldSwipeDown] {
        touch(&mut harness, &mut input, gesture);
    }
    assert!(
        harness.transcript().is_empty(),
        "{:?}",
        harness.transcript()
    );
    assert_eq!(harness.with_drive(|drive, _| drive.gear_text()), before);
}

#[test]
fn without_a_hardware_keyboard_hints_name_gestures() {
    let mut harness = a_drive("Touch No Keyboard");
    harness.app.dispatch_gesture(Gesture::KeyboardDisconnected);
    assert!(!harness.app.ctx.controller.hardware_keyboard());
    assert_eq!(
        harness.app.ctx.controller.device(),
        ff_core::input_hints::TOUCH
    );
    harness.app.dispatch_gesture(Gesture::KeyboardConnected);
    assert!(harness.app.ctx.controller.hardware_keyboard());
}

/// The cruise flicks with cruise ON: each flick steps the target one notch
/// in its direction and cruise stays on. The Rust side was never the fault
/// when an iPhone flick failed to step it -- a stroke that outlasted the
/// flick window arrived as a pedal hold instead (`ff_touch.m`).
#[test]
fn cruise_flicks_step_the_target_and_keep_cruise_on() {
    let mut harness = a_drive("Touch Cruise Steps");
    rolling(&mut harness, 45.0);
    let mut input = TouchInput::new();
    touch(&mut harness, &mut input, Gesture::HoldUpperBegan);
    touch(&mut harness, &mut input, Gesture::UpperHoldTap);
    touch(&mut harness, &mut input, Gesture::HoldEnded);
    for (gesture, target) in [
        (Gesture::SwipeUp, 50.0),
        (Gesture::SwipeUp, 55.0),
        (Gesture::SwipeDown, 50.0),
        (Gesture::SwipeDown, 45.0),
    ] {
        touch(&mut harness, &mut input, gesture);
        harness.app.tick(0.05);
        let (cruise, set) =
            harness.with_drive(|drive, _| (drive.cruise_mph, drive.speed_control_target_mph));
        assert_eq!(cruise, Some(target), "{gesture:?}");
        assert_eq!(set, Some(target), "{gesture:?}");
    }
}

/// A flick is never a pedal press: it must not hold gas or brake keys, so
/// the brake-cancels-cruise rule cannot fire from one.
#[test]
fn a_cruise_flick_presses_no_pedal() {
    let mut harness = a_drive("Touch Cruise No Pedal");
    rolling(&mut harness, 45.0);
    let mut input = TouchInput::new();
    touch(&mut harness, &mut input, Gesture::HoldUpperBegan);
    touch(&mut harness, &mut input, Gesture::UpperHoldTap);
    touch(&mut harness, &mut input, Gesture::HoldEnded);
    for gesture in [Gesture::SwipeUp, Gesture::SwipeDown] {
        touch(&mut harness, &mut input, gesture);
        assert!(
            !harness.app.ctx.input.physically_down(Key::Up),
            "{gesture:?}"
        );
        assert!(
            !harness.app.ctx.input.physically_down(Key::Down),
            "{gesture:?}"
        );
    }
}

#[test]
fn practice_reads_the_length_of_a_swipe_down_and_a_deep_swipe() {
    let mut harness = PlaytestHarness::new();
    harness.app.push_state(InputPracticeState::gestures());
    let mut input = TouchInput::new();

    touch(&mut harness, &mut input, Gesture::HoldLowerBegan);
    harness.clear_speech();
    // The phone reports the length just before the lift.
    touch(&mut harness, &mut input, Gesture::Travel(64));
    touch(&mut harness, &mut input, Gesture::HoldEnded);
    assert_eq!(
        harness.transcript(),
        vec!["Length 64. The deep swipe starts at 90."]
    );

    touch(&mut harness, &mut input, Gesture::HoldLowerBegan);
    touch(&mut harness, &mut input, Gesture::DeepSwipeDownBegan);
    harness.clear_speech();
    touch(&mut harness, &mut input, Gesture::Travel(112));
    touch(&mut harness, &mut input, Gesture::HoldEnded);
    assert_eq!(harness.transcript(), vec!["Deep swipe. Length 112."]);

    // A swipe up has no length that changes what it does.
    touch(&mut harness, &mut input, Gesture::HoldUpperBegan);
    harness.clear_speech();
    touch(&mut harness, &mut input, Gesture::Travel(70));
    touch(&mut harness, &mut input, Gesture::HoldEnded);
    assert!(
        harness.transcript().is_empty(),
        "{:?}",
        harness.transcript()
    );
}

#[test]
fn the_deep_swipe_row_steps_clamps_and_tunes_the_touch_surface() {
    let mut harness = PlaytestHarness::new();
    harness.app.push_state(TouchGesturesState::new());
    let mut input = TouchInput::new();
    while harness.focused_label().as_deref() != Some("Deep swipe starts at: 90") {
        touch(&mut harness, &mut input, Gesture::SwipeRight);
        assert!(harness.state_is::<TouchGesturesState>());
    }
    harness.clear_speech();
    // Flick up raises it and the row reads itself again.
    touch(&mut harness, &mut input, Gesture::SwipeUp);
    let heard = harness.transcript();
    assert!(
        heard.len() == 1 && heard[0].starts_with("Deep swipe starts at: 100."),
        "{heard:?}"
    );
    let tuning = freight_fate::app::sdl_shell::last_touch_tuning().expect("tuned");
    assert!((tuning[0] * tuning[2] - 100.0).abs() < 1e-9, "{tuning:?}");

    for _ in 0..15 {
        touch(&mut harness, &mut input, Gesture::SwipeDown);
    }
    assert_eq!(
        harness.focused_label().as_deref(),
        Some("Deep swipe starts at: 60")
    );
    right(&mut harness);
    assert_eq!(
        harness.focused_label().as_deref(),
        Some("Deep swipe starts at: 70")
    );
    for _ in 0..15 {
        right(&mut harness);
    }
    assert_eq!(
        harness.focused_label().as_deref(),
        Some("Deep swipe starts at: 180")
    );
    assert_eq!(harness.app.ctx.settings.touch_deep_swipe_factor, 6.0);
    let tuning = freight_fate::app::sdl_shell::last_touch_tuning().expect("tuned");
    assert_eq!(tuning[2], 6.0);
}

/// Right arrow on a screen with no drive under it.
fn right(harness: &mut PlaytestHarness) {
    for event in freight_fate::touch::key_press(Key::Right) {
        harness.app.handle_event(&event);
    }
}
