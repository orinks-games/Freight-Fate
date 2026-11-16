//! A manual truck on a touch screen: a shift asked for by touch works the
//! clutch itself (`ff_core::sim::touch_clutch`), and the engine start after a
//! stall in gear takes neutral first.

use ff_core::sim::touch_clutch::TouchClutch;
use ff_core::sim::transmission::{shift_time_for, DOWNSHIFT_TIME};
use ff_core::sim::weather::WeatherKind;

use freight_fate::playtest::harness::{key_event, PlaytestHarness, StartDelivery};
use freight_fate::states::base::Key;
use freight_fate::states::driving::DrivingState;
use freight_fate::states::driving_menu_states::DrivingCommandsState;
use freight_fate::touch::Gesture;

/// A stopped manual truck with the engine running, in neutral, with touch
/// hints on.
fn a_manual_truck(name: &str, touch: bool) -> PlaytestHarness {
    let mut harness = PlaytestHarness::new();
    harness.start_delivery(StartDelivery::named(name));
    harness.app.ctx.settings.automatic_transmission = false;
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
        drive.trip.truck.start_engine();
        drive.trip.truck.transmission.automatic = false;
        drive.trip.truck.transmission.gear = 0;
    });
    if touch {
        harness.app.dispatch_gesture(Gesture::KeyboardDisconnected);
        harness.app.dispatch_gesture(Gesture::Tap);
        assert!(harness.app.ctx.touch_hints());
    }
    harness.clear_speech();
    harness
}

fn gear(harness: &PlaytestHarness) -> i32 {
    harness.read_drive(|drive| drive.trip.truck.transmission.gear)
}

fn clutch(harness: &PlaytestHarness) -> (TouchClutch, f64) {
    harness.read_drive(|drive| (drive.touch_clutch, drive.trip.truck.transmission.clutch))
}

fn frames(harness: &mut PlaytestHarness, seconds: f64) {
    for _ in 0..(seconds * 60.0).ceil() as usize {
        harness.advance_frame_clock();
        harness.app.frame(1.0 / 60.0);
    }
}

fn heard(harness: &PlaytestHarness, text: &str) -> bool {
    harness.transcript().iter().any(|line| line.contains(text))
}

#[test]
fn a_touch_shift_up_and_down_takes_the_gear_with_no_grind() {
    let mut harness = a_manual_truck("Touch Clutch Shift", true);
    harness.app.dispatch_gesture(Gesture::LowerHoldSwipeUp);
    assert_eq!(gear(&harness), 1);
    let (touch, pedal) = clutch(&harness);
    assert!(touch.pressed(), "{touch:?}");
    assert_eq!(pedal, 1.0);
    // The clutch comes out after the automatic's own interruption.
    frames(&mut harness, shift_time_for(1) + 0.05);
    assert_eq!(clutch(&harness), (TouchClutch::Out, 0.0));

    harness.app.dispatch_gesture(Gesture::UpperHoldSwipeUp);
    assert_eq!(gear(&harness), 2);
    frames(&mut harness, shift_time_for(2) + 0.05);
    assert_eq!(clutch(&harness).0, TouchClutch::Out);

    harness.app.dispatch_gesture(Gesture::UpperHoldSwipeDown);
    assert_eq!(gear(&harness), 1);
    frames(&mut harness, DOWNSHIFT_TIME - 0.1);
    assert!(clutch(&harness).0.pressed(), "a downshift holds longer");
    frames(&mut harness, 0.2);
    assert_eq!(clutch(&harness), (TouchClutch::Out, 0.0));

    let said = harness.transcript().join("\n");
    assert!(!said.contains("Grinding"), "{said}");
    assert!(said.contains("gear 2"), "{said}");
}

#[test]
fn a_shift_from_the_command_list_on_touch_works_the_clutch() {
    let mut harness = a_manual_truck("Touch Clutch List", true);
    harness.app.dispatch_gesture(Gesture::ThreeFingerTap);
    assert!(harness.state_is::<DrivingCommandsState>());
    while harness.focused_label().as_deref() != Some("Gears and engine brake") {
        harness.app.dispatch_gesture(Gesture::SwipeRight);
    }
    harness.app.dispatch_gesture(Gesture::DoubleTap);
    assert_eq!(harness.focused_label().as_deref(), Some("Shift up"));
    harness.app.dispatch_gesture(Gesture::DoubleTap);
    assert!(harness.state_is::<DrivingState>());
    assert_eq!(gear(&harness), 1);
    assert!(!heard(&harness, "Grinding"), "{:?}", harness.transcript());
}

#[test]
fn a_keyboard_shift_without_shift_still_grinds() {
    let mut harness = a_manual_truck("Keyboard Clutch Grind", false);
    harness.key(key_event(Key::W, Some('w')));
    assert_eq!(gear(&harness), 0);
    assert!(
        heard(&harness, "Grinding gears"),
        "{:?}",
        harness.transcript()
    );
    assert_eq!(clutch(&harness).0, TouchClutch::Out);
}

#[test]
fn a_keyboard_takes_the_clutch_back_mid_shift() {
    let mut harness = a_manual_truck("Touch Clutch Keyboard", true);
    harness.app.dispatch_gesture(Gesture::LowerHoldSwipeUp);
    assert!(clutch(&harness).0.pressed());
    // Any key on a hardware keyboard hands the hints, and the clutch, back.
    harness.app.dispatch_gesture(Gesture::KeyboardConnected);
    harness.key(key_event(Key::Space, Some(' ')));
    assert!(!harness.app.ctx.touch_hints());
    frames(&mut harness, 1.0 / 60.0);
    assert_eq!(clutch(&harness), (TouchClutch::Out, 0.0));
}

#[test]
fn the_automatic_lets_the_touch_clutch_out() {
    let mut harness = a_manual_truck("Touch Clutch Automatic", true);
    harness.app.dispatch_gesture(Gesture::LowerHoldSwipeUp);
    assert!(clutch(&harness).0.pressed());
    harness.app.ctx.settings.automatic_transmission = true;
    frames(&mut harness, 1.0 / 60.0);
    assert_eq!(clutch(&harness).0, TouchClutch::Out);
}

#[test]
fn a_touch_restart_after_a_stall_in_fifth_takes_neutral_then_first() {
    let mut harness = a_manual_truck("Touch Clutch Stall", true);
    harness.with_drive(|drive, _| {
        drive.trip.truck.transmission.gear = 5;
        drive.trip.truck.stall();
    });
    harness.clear_speech();
    harness.app.dispatch_gesture(Gesture::RotateRight);
    assert_eq!(gear(&harness), 0);
    assert!(harness.read_drive(|drive| drive.trip.truck.engine_on));
    let said = harness.transcript();
    let neutral = said.iter().position(|line| line == "Neutral.");
    let running = said
        .iter()
        .position(|line| line.starts_with("Engine running."));
    assert!(
        matches!((neutral, running), (Some(n), Some(r)) if n < r),
        "{said:?}"
    );
    frames(&mut harness, 2.0);
    harness.app.dispatch_gesture(Gesture::LowerHoldSwipeUp);
    assert_eq!(gear(&harness), 1);
    assert!(!heard(&harness, "Grinding"), "{:?}", harness.transcript());
}

#[test]
fn the_touch_stall_line_names_the_turn_and_the_flick() {
    let mut harness = a_manual_truck("Touch Clutch Stall Line", true);
    harness.with_drive(|drive, _| {
        drive.trip.truck.transmission.gear = 6;
        drive.trip.truck.transmission.clutch = 0.0;
        drive.trip.truck.parking_brake = false;
    });
    harness.clear_speech();
    frames(&mut harness, 1.0);
    let said = harness.transcript().join("\n");
    assert!(
        said.contains(
            "Engine stalled. Turn two fingers right to restart, then hold a pedal and flick up \
             with a second finger for first gear."
        ),
        "{said}"
    );
}
