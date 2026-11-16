//! The iPhone wheel fixes: cruise flicks with cruise off, the engine stop
//! guard, double tap answering the road, two-finger tap stopping speech,
//! the Status menu on the three-finger tap list, and Driving School read as
//! the wheel (`touch::route`, `states/driving_controls/touch.rs`).

use ff_core::sim::weather::WeatherKind;

use freight_fate::bindings::Action;
use freight_fate::playtest::harness::{PlaytestHarness, StartDelivery};
use freight_fate::states::base::{InputEvent, Key};
use freight_fate::states::driving::DrivingState;
use freight_fate::states::driving_core::{DRIVE_PHASE_DELIVERY, PULL_OVER_LIGHTS};
use freight_fate::states::driving_menu_states::{DrivingCommandsState, DrivingStatusState};
use freight_fate::states::driving_school::{DrivingSchoolState, SchoolDrivingState};
use freight_fate::states::driving_team::TeamMode;
use freight_fate::states::text_entry::TextEntryState;
use freight_fate::touch::{Gesture, RoadOffer, TouchInput, TouchMode};

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
        drive.trip.set_patrols(Vec::new());
        drive.trip.posts.clear();
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

fn touch(harness: &mut PlaytestHarness, input: &mut TouchInput, gesture: Gesture) {
    for event in input.handle(gesture).events {
        harness.app.handle_event(&event);
    }
}

fn speed_control_on(harness: &mut PlaytestHarness) -> bool {
    harness.with_drive(|drive, _| drive.speed_authority_engaged() || drive.speed_control_armed)
}

/// What `action`'s default key says on a fresh drive set up by `setup`.
fn by_key(name: &str, setup: impl Fn(&mut PlaytestHarness), action: Action) -> Vec<String> {
    let mut harness = a_drive(name);
    setup(&mut harness);
    let chord = action.default_chords()[0];
    harness.key(InputEvent::KeyDown {
        key: chord.key,
        mods: chord.mods,
        text: None,
        repeat: false,
    });
    harness.transcript()
}

// -- cruise flicks -------------------------------------------------------------------

#[test]
fn with_cruise_off_a_flick_down_does_nothing_and_says_nothing() {
    let mut harness = a_drive("Touch Flick Down");
    rolling(&mut harness, 45.0);
    let mut input = TouchInput::new();
    touch(&mut harness, &mut input, Gesture::SwipeDown);
    assert!(!speed_control_on(&mut harness));
    assert!(
        harness.transcript().is_empty(),
        "{:?}",
        harness.transcript()
    );
}

#[test]
fn with_cruise_off_a_flick_up_sets_this_speed_the_first_time() {
    let expected = by_key("Touch Flick Up Key", |h| rolling(h, 45.0), Action::Cruise);
    let mut harness = a_drive("Touch Flick Up");
    rolling(&mut harness, 45.0);
    let mut input = TouchInput::new();
    touch(&mut harness, &mut input, Gesture::SwipeUp);
    assert!(speed_control_on(&mut harness));
    assert_eq!(harness.transcript(), expected);
}

#[test]
fn with_cruise_off_a_flick_up_resumes_the_remembered_speed() {
    let remembered = |h: &mut PlaytestHarness| {
        rolling(h, 45.0);
        h.with_drive(|drive, _| drive.resume_target_mph = Some(55.0));
    };
    let expected = by_key("Touch Resume Key", remembered, Action::CruiseResume);
    assert!(
        expected.iter().any(|line| line.contains("Resuming cruise")),
        "{expected:?}"
    );
    let mut harness = a_drive("Touch Resume");
    remembered(&mut harness);
    let mut input = TouchInput::new();
    touch(&mut harness, &mut input, Gesture::SwipeUp);
    assert_eq!(harness.transcript(), expected);
    assert!(speed_control_on(&mut harness));
}

// -- the engine key ------------------------------------------------------------------

#[test]
fn a_turn_left_stops_the_engine_only_when_stopped() {
    let mut harness = a_drive("Touch Engine Guard");
    rolling(&mut harness, 30.0);
    harness.app.dispatch_gesture(Gesture::RotateLeft);
    assert!(harness.with_drive(|drive, _| drive.trip.truck.engine_on));
    assert!(
        harness.transcript().is_empty(),
        "{:?}",
        harness.transcript()
    );

    rolling(&mut harness, 0.0);
    harness.app.dispatch_gesture(Gesture::RotateLeft);
    assert!(!harness.with_drive(|drive, _| drive.trip.truck.engine_on));
    harness.app.dispatch_gesture(Gesture::RotateRight);
    assert!(harness.with_drive(|drive, _| drive.trip.truck.engine_on));
}

// -- double tap answers the road -----------------------------------------------------

#[test]
fn double_tap_with_nothing_called_answers_as_the_exit_key_does() {
    let quiet_road = |h: &mut PlaytestHarness| {
        rolling(h, 50.0);
        h.with_drive(|drive, _| drive.trip.stops.clear());
    };
    let expected = by_key("Touch Exit Nothing Key", quiet_road, Action::TakeExit);
    let mut harness = a_drive("Touch Exit Nothing");
    quiet_road(&mut harness);
    assert_eq!(
        harness.with_drive(|drive, ctx| drive.road_offer(ctx)),
        RoadOffer::None
    );
    let mut input = TouchInput::new();
    touch(&mut harness, &mut input, Gesture::DoubleTap);
    assert_eq!(harness.transcript(), expected);
}

#[test]
fn double_tap_takes_a_called_exit_as_the_exit_key_does() {
    let near_a_stop = |h: &mut PlaytestHarness| {
        rolling(h, 50.0);
        h.with_drive(|drive, _| {
            let first = drive.trip.stops.first().map(|stop| stop.at_mi);
            let first = first.expect("the assigned route carries en-route stops");
            drive.trip.position_mi = (first - 0.5).max(0.0);
        });
    };
    let expected = by_key("Touch Exit Key", near_a_stop, Action::TakeExit);
    let mut harness = a_drive("Touch Exit");
    near_a_stop(&mut harness);
    assert_eq!(
        harness.with_drive(|drive, ctx| drive.road_offer(ctx)),
        RoadOffer::Exit
    );
    let mut input = TouchInput::new();
    touch(&mut harness, &mut input, Gesture::DoubleTap);
    // Each drive rolls its own ramp control, so only the signal line itself
    // is compared.
    let signal = |lines: &[String]| -> Vec<String> {
        lines
            .iter()
            .map(|line| line.split(" ahead.").next().unwrap_or(line).to_string())
            .collect()
    };
    assert_eq!(signal(&harness.transcript()), signal(&expected));
    assert!(
        expected[0].starts_with("Signal on for exit"),
        "{expected:?}"
    );
    assert!(harness.with_drive(|drive, _| drive.exit_signal_on));
}

#[test]
fn double_tap_on_the_co_drivers_truck_does_nothing_as_the_exit_key_does() {
    let mut harness = a_drive("Touch Exit Co-Driver");
    harness.with_drive(|drive, ctx| {
        let first = drive.trip.stops.first().map(|stop| stop.at_mi);
        let first = first.expect("the assigned route carries en-route stops");
        drive.trip.position_mi = (first - 0.5).max(0.0);
        drive.hand_wheel_to_co_driver(ctx, TeamMode::RideAlong);
        drive.trip.truck.start_engine();
        drive.trip.truck.parking_brake = false;
        drive.trip.truck.velocity_mps = 50.0 / MPH_PER_MPS;
    });
    assert!(harness.read_drive(|drive| drive.co_driver_driving()));
    harness.clear_speech();
    let mut input = TouchInput::new();
    touch(&mut harness, &mut input, Gesture::DoubleTap);
    assert!(!harness.with_drive(|drive, _| drive.exit_signal_on));
    assert_eq!(harness.transcript(), Vec::<String>::new());
}

#[test]
fn double_tap_signals_for_a_trooper() {
    let mut harness = a_drive("Touch Pull Over");
    rolling(&mut harness, 50.0);
    harness.with_drive(|drive, _| drive.pull_over = Some(PULL_OVER_LIGHTS.to_string()));
    assert_eq!(
        harness.with_drive(|drive, ctx| drive.road_offer(ctx)),
        RoadOffer::PullOver
    );
    let mut input = TouchInput::new();
    touch(&mut harness, &mut input, Gesture::DoubleTap);
    assert!(harness.with_drive(|drive, _| drive.pull_over_signaled));
    assert_eq!(harness.transcript(), vec!["Signaling onto the shoulder."]);
}

#[test]
fn double_tap_at_the_gate_confirms_arrival() {
    let mut harness = a_drive("Touch Arrival");
    harness.app.ctx.settings.destination_approach_assist = false;
    harness.with_drive(|drive, _| {
        drive.phase = DRIVE_PHASE_DELIVERY;
        drive.destination_exit_taken = true;
        drive.trip.finished = true;
        drive.trip.position_mi = drive.trip.total_miles();
        drive.trip.truck.engine_on = true;
        drive.trip.truck.velocity_mps = 0.0;
    });
    harness.clear_speech();
    assert_eq!(
        harness.with_drive(|drive, ctx| drive.road_offer(ctx)),
        RoadOffer::Arrival
    );
    let mut input = TouchInput::new();
    touch(&mut harness, &mut input, Gesture::DoubleTap);
    assert!(harness.read_drive(|drive| drive.arrival_menu_open));
}

#[test]
fn touch_prompts_name_the_double_tap_for_the_exit() {
    let mut harness = a_drive("Touch Exit Hint");
    harness.app.dispatch_gesture(Gesture::KeyboardDisconnected);
    harness.app.dispatch_gesture(Gesture::Tap);
    assert_eq!(harness.app.ctx.control_hint("take_exit"), "a double tap");
    assert_eq!(
        harness.app.ctx.control_hint("status_menu"),
        "a three-finger tap then a double tap"
    );
}

#[test]
fn a_touch_drive_hears_double_tap_in_the_exit_callout_and_a_keyboard_brings_keys_back() {
    let mut harness = a_drive("Touch Exit Callout");
    rolling(&mut harness, 55.0);
    harness.with_drive(|drive, _| {
        let first = drive.trip.stops.first().map(|stop| stop.at_mi);
        let first = first.expect("the assigned route carries en-route stops");
        drive.trip.position_mi = (first - 1.2).max(0.0);
    });
    harness.app.dispatch_gesture(Gesture::KeyboardDisconnected);
    harness.app.dispatch_gesture(Gesture::Tap);
    harness.advance_frame_clock();
    harness.app.frame(1.0 / 60.0);
    assert_eq!(
        harness.read_drive(|drive| drive.trip.exit_hint.clone()),
        ff_core::speech_text::TOUCH_EXIT_HINT
    );
    harness.clear_speech();
    let mut heard = false;
    for _ in 0..60 * 90 {
        harness.advance_frame_clock();
        harness.app.frame(1.0 / 60.0);
        if harness
            .transcript()
            .iter()
            .any(|line| line.contains("to signal for the exit."))
        {
            heard = true;
            break;
        }
    }
    let said = harness.transcript().join("\n");
    assert!(heard, "{said}");
    assert!(
        said.contains(" Double tap to signal for the exit."),
        "{said}"
    );
    assert!(!said.contains("Press a double tap"), "{said}");
    // A key on a hardware keyboard: the next frame names keys again.
    harness.app.dispatch_gesture(Gesture::KeyboardConnected);
    let chord = Action::Speed.default_chords()[0];
    harness.key(InputEvent::KeyDown {
        key: chord.key,
        mods: chord.mods,
        text: None,
        repeat: false,
    });
    assert_ne!(
        harness.app.ctx.controller.device(),
        ff_core::input_hints::TOUCH
    );
    harness.advance_frame_clock();
    harness.app.frame(1.0 / 60.0);
    assert_eq!(
        harness.read_drive(|drive| drive.trip.exit_hint.clone()),
        harness.app.ctx.control_hint("take_exit")
    );
    assert_ne!(
        harness.read_drive(|drive| drive.trip.exit_hint.clone()),
        ff_core::speech_text::TOUCH_EXIT_HINT
    );
}

// -- two-finger tap stops speech ----------------------------------------------------

#[test]
fn two_finger_tap_stops_speech_at_the_wheel_and_in_menus() {
    let mut harness = a_drive("Touch Stop Speech");
    assert_eq!(
        harness
            .app
            .ctx
            .bindings
            .touch_command(Gesture::TwoFingerTap),
        None,
        "two-finger tap is not a slot any more"
    );
    let before = harness.app.speech().stop_event_calls();
    harness.app.dispatch_gesture(Gesture::TwoFingerTap);
    assert!(harness.state_is::<DrivingState>(), "no Status menu");
    assert!(harness.app.speech().stop_event_calls() > before);

    harness.app.dispatch_gesture(Gesture::ThreeFingerTap);
    assert!(harness.state_is::<DrivingCommandsState>());
    let before = harness.app.speech().stop_main_calls();
    harness.app.dispatch_gesture(Gesture::TwoFingerTap);
    assert!(harness.app.speech().stop_main_calls() > before);
    assert!(harness.state_is::<DrivingCommandsState>());
}

#[test]
fn three_finger_tap_then_double_tap_opens_the_status_menu() {
    let mut harness = a_drive("Touch Status Row");
    harness.app.dispatch_gesture(Gesture::ThreeFingerTap);
    assert!(harness.state_is::<DrivingCommandsState>());
    assert_eq!(
        harness.menu_labels().first().map(String::as_str),
        Some(Action::Status.label())
    );
    harness.app.dispatch_gesture(Gesture::DoubleTap);
    assert!(
        harness.state_is::<DrivingStatusState>(),
        "{:?}",
        harness.app.visible_lines()
    );
}

#[test]
fn in_the_list_a_flick_up_raises_radio_volume_and_a_two_finger_flick_down_leaves_the_group() {
    let mut harness = a_drive("Touch List Groups");
    harness.app.ctx.settings.radio_volume = 0.5;
    harness.app.dispatch_gesture(Gesture::KeyboardDisconnected);
    harness.app.dispatch_gesture(Gesture::ThreeFingerTap);
    while harness.focused_label().as_deref() != Some("Radio and CB") {
        harness.app.dispatch_gesture(Gesture::SwipeRight);
    }
    harness.app.dispatch_gesture(Gesture::DoubleTap);
    while !harness
        .focused_label()
        .is_some_and(|label| label.starts_with("Radio volume"))
    {
        harness.app.dispatch_gesture(Gesture::SwipeRight);
    }
    harness.app.dispatch_gesture(Gesture::SwipeUp);
    harness.app.dispatch_gesture(Gesture::SwipeUp);
    assert!(harness.state_is::<DrivingCommandsState>());
    assert!(harness.app.ctx.settings.radio_volume > 0.5);
    harness.app.dispatch_gesture(Gesture::TwoFingerSwipeDown);
    assert!(harness.state_is::<DrivingCommandsState>());
    assert_eq!(harness.focused_label().as_deref(), Some("Radio and CB"));
    harness.app.dispatch_gesture(Gesture::TwoFingerSwipeDown);
    assert!(harness.state_is::<DrivingState>());
}

#[test]
fn on_touch_a_row_names_its_gesture_or_has_no_help_of_its_own() {
    let mut harness = a_drive("Touch List Help");
    harness.app.dispatch_gesture(Gesture::KeyboardDisconnected);
    harness.app.dispatch_gesture(Gesture::ThreeFingerTap);
    while harness.focused_label().as_deref() != Some("Speed and cruise") {
        harness.app.dispatch_gesture(Gesture::SwipeRight);
    }
    harness.app.dispatch_gesture(Gesture::DoubleTap);
    harness.clear_speech();
    harness.app.dispatch_gesture(Gesture::TwoFingerSwipeUp);
    assert_eq!(
        harness.transcript().last().map(String::as_str),
        Some("At the wheel: a tap.")
    );
    harness.app.dispatch_gesture(Gesture::SwipeRight);
    harness.clear_speech();
    harness.app.dispatch_gesture(Gesture::TwoFingerSwipeUp);
    assert_eq!(
        harness.transcript().last().map(String::as_str),
        Some("Posted speed limit.")
    );
}

#[test]
fn three_finger_tap_in_a_name_field_reads_it_back() {
    let mut harness = PlaytestHarness::new();
    let mut field = TextEntryState::new("Name", "Name", |_, _| {});
    field.entry.name = "Rook".chars().collect();
    field.entry.cursor = 4;
    harness.app.push_state(field);
    assert_eq!(harness.app.touch_mode(), TouchMode::Field);
    harness.clear_speech();
    harness.app.dispatch_gesture(Gesture::ThreeFingerTap);
    assert_eq!(harness.transcript(), vec!["Rook"]);
}

// -- Driving School is the wheel -----------------------------------------------------

fn a_lesson(name: &str) -> PlaytestHarness {
    let mut harness = PlaytestHarness::new();
    harness.start_delivery(StartDelivery::named(name));
    harness.app.push_state(DrivingSchoolState::new());
    harness.select_menu_item_starting("Lesson 1");
    assert!(harness.state_is::<SchoolDrivingState>());
    assert_eq!(harness.app.touch_mode(), TouchMode::Wheel);
    harness.clear_speech();
    harness
}

fn with_lesson<R>(harness: &mut PlaytestHarness, f: impl FnOnce(&mut DrivingState) -> R) -> R {
    let state = harness.app.ctx.state().expect("the lesson");
    let mut borrowed = state.borrow_mut();
    let school = borrowed
        .as_any_mut()
        .downcast_mut::<SchoolDrivingState>()
        .expect("a practice drive");
    f(school.drive_mut())
}

fn bound_key(harness: &PlaytestHarness, action: Action) -> Key {
    harness.app.ctx.bindings.chords(action)[0].key
}

#[test]
fn in_a_lesson_a_deep_swipe_while_moving_holds_the_emergency_brake() {
    let mut harness = a_lesson("Touch School Deep Swipe");
    with_lesson(&mut harness, |drive| {
        drive.trip.truck.start_engine();
        drive.trip.truck.parking_brake = false;
        drive.trip.truck.velocity_mps = 30.0 / MPH_PER_MPS;
    });
    let emergency = bound_key(&harness, Action::EmergencyBrake);
    let mut input = TouchInput::new();
    touch(&mut harness, &mut input, Gesture::HoldLowerBegan);
    touch(&mut harness, &mut input, Gesture::DeepSwipeDownBegan);
    assert!(!harness.app.ctx.input.physically_down(Key::Down));
    assert!(harness.app.ctx.input.physically_down(emergency));
    touch(&mut harness, &mut input, Gesture::HoldEnded);
    assert!(!harness.app.ctx.input.physically_down(emergency));
}

#[test]
fn in_a_lesson_a_still_hold_straightens_the_wheel() {
    let mut harness = a_lesson("Touch School Straighten");
    let straighten = bound_key(&harness, Action::Straighten);
    let mut input = TouchInput::new();
    touch(&mut harness, &mut input, Gesture::StillHoldBegan);
    assert!(harness.app.ctx.input.physically_down(straighten));
    touch(&mut harness, &mut input, Gesture::HoldEnded);
    assert!(!harness.app.ctx.input.physically_down(straighten));
}

#[test]
fn in_a_lesson_gas_releases_the_parking_brake_first() {
    let mut harness = a_lesson("Touch School Gas");
    with_lesson(&mut harness, |drive| {
        drive.trip.truck.start_engine();
        drive.trip.truck.set_air_pressure_psi(120.0);
        drive.trip.truck.parking_brake = true;
        drive.trip.truck.velocity_mps = 0.0;
    });
    let mut input = TouchInput::new();
    touch(&mut harness, &mut input, Gesture::HoldUpperBegan);
    assert!(!with_lesson(&mut harness, |drive| drive
        .trip
        .truck
        .parking_brake));
    assert!(harness.app.ctx.input.physically_down(Key::Up));
}
