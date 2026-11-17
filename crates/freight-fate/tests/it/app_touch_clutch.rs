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

const NEUTRAL_LINE: &str = "Neutral. Hold a pedal and flick up with a second finger for a gear.";

fn neutral_lines(harness: &PlaytestHarness) -> usize {
    harness
        .transcript()
        .iter()
        .filter(|line| line.contains("Neutral."))
        .count()
}

/// A manual rolling at `mph` in `gear`, parking brake off.
fn rolling(harness: &mut PlaytestHarness, gear: i32, mph: f64) {
    harness.with_drive(|drive, _| {
        drive.trip.truck.parking_brake = false;
        drive.trip.truck.transmission.gear = gear;
        drive.trip.truck.velocity_mps = mph / 2.236_936_292_054_402;
    });
    harness.clear_speech();
}

/// Holds the brake by touch (`Some(input)`) or with the Down key until the
/// truck has stood five seconds, failing on a stall. Returns whether the
/// neutral was taken before the truck stopped.
fn brake_to_a_stop(harness: &mut PlaytestHarness) -> bool {
    let mut stopped_for = 0.0;
    let mut neutral_while_rolling = false;
    for _ in 0..90 * 60 {
        frames(harness, 1.0 / 60.0);
        let (on, mph, gear) = harness.read_drive(|drive| {
            let truck = &drive.trip.truck;
            (truck.engine_on, truck.speed_mph(), truck.transmission.gear)
        });
        assert!(on, "stalled with the brake held at {mph:.1} mph");
        if gear == 0 && mph >= 0.1 {
            neutral_while_rolling = true;
        }
        if mph < 0.1 {
            stopped_for += 1.0 / 60.0;
            if stopped_for >= 5.0 {
                return neutral_while_rolling;
            }
        }
    }
    panic!("never came to a stop");
}

/// The step 4 drive's stall: a touch manual in tenth braking off the
/// highway takes neutral on the way down, says so once, and the lift
/// leaves the engine running.
#[test]
fn a_touch_manual_in_tenth_brakes_to_a_stop_in_neutral() {
    let mut harness = a_manual_truck("Touch Neutral Tenth", true);
    rolling(&mut harness, 10, 55.0);
    let mut input = freight_fate::touch::TouchInput::new();
    let mut touch = |harness: &mut PlaytestHarness, gesture| {
        for event in input.handle(gesture).events {
            harness.app.handle_event(&event);
        }
    };
    touch(&mut harness, Gesture::HoldLowerBegan);
    assert!(brake_to_a_stop(&mut harness), "neutral only at the stop");
    assert_eq!(gear(&harness), 0);
    assert_eq!(clutch(&harness).0, TouchClutch::Neutral);
    touch(&mut harness, Gesture::HoldEnded);
    frames(&mut harness, 2.0);
    let truck = harness.read_drive(|drive| (drive.trip.truck.engine_on, drive.trip.truck.stalled));
    assert_eq!(truck, (true, false));
    assert!(heard(&harness, NEUTRAL_LINE), "{:?}", harness.transcript());
    assert_eq!(neutral_lines(&harness), 1, "{:?}", harness.transcript());
    // First gear from there, by touch, with no grind; the clutch reads out.
    touch(&mut harness, Gesture::HoldLowerBegan);
    touch(&mut harness, Gesture::LowerHoldSwipeUp);
    assert_eq!(gear(&harness), 1);
    frames(&mut harness, 1.0);
    assert_eq!(clutch(&harness).0, TouchClutch::Out);
    assert!(!heard(&harness, "Grinding"), "{:?}", harness.transcript());
}

/// Coasting down in a tall gear with no pedal at all takes the same
/// neutral: no frame can carry the truck past it into a stall.
#[test]
fn a_touch_manual_coasting_down_in_sixth_takes_neutral() {
    let mut harness = a_manual_truck("Touch Neutral Coast", true);
    rolling(&mut harness, 6, 9.0);
    for _ in 0..60 * 60 {
        frames(&mut harness, 1.0 / 60.0);
        let (on, gear) = harness.read_drive(|drive| {
            (
                drive.trip.truck.engine_on,
                drive.trip.truck.transmission.gear,
            )
        });
        assert!(on, "stalled coasting");
        if gear == 0 {
            break;
        }
    }
    assert_eq!(gear(&harness), 0);
    assert_eq!(neutral_lines(&harness), 1, "{:?}", harness.transcript());
}

/// The 2026-09-28 rule on a keyboard is untouched: braking in a tall gear
/// holds idle, and the lift stalls.
#[test]
fn a_keyboard_manual_in_tenth_still_stalls_at_the_lift() {
    let mut harness = a_manual_truck("Keyboard Tenth Stall", false);
    rolling(&mut harness, 10, 55.0);
    harness
        .app
        .ctx
        .input
        .press(Key::Down, freight_fate::states::base::Mods::NONE);
    assert!(!brake_to_a_stop(&mut harness));
    assert_eq!(gear(&harness), 10);
    harness
        .app
        .ctx
        .input
        .release(Key::Down, freight_fate::states::base::Mods::NONE);
    frames(&mut harness, 2.0);
    assert!(harness.read_drive(|drive| drive.trip.truck.stalled));
    assert_eq!(neutral_lines(&harness), 0, "{:?}", harness.transcript());
}

/// Third is in the launch regime: no neutral, no stall.
#[test]
fn a_touch_manual_in_third_brakes_to_a_stop_in_gear() {
    let mut harness = a_manual_truck("Touch Neutral Third", true);
    rolling(&mut harness, 3, 15.0);
    let mut input = freight_fate::touch::TouchInput::new();
    for event in input.handle(Gesture::HoldLowerBegan).events {
        harness.app.handle_event(&event);
    }
    assert!(!brake_to_a_stop(&mut harness));
    for event in input.handle(Gesture::HoldEnded).events {
        harness.app.handle_event(&event);
    }
    frames(&mut harness, 2.0);
    assert_eq!(gear(&harness), 3);
    assert!(harness.read_drive(|drive| drive.trip.truck.engine_on));
    assert_eq!(neutral_lines(&harness), 0, "{:?}", harness.transcript());
}

#[test]
fn a_touch_automatic_never_hears_the_neutral() {
    let mut harness = a_manual_truck("Touch Neutral Automatic", true);
    harness.app.ctx.settings.automatic_transmission = true;
    harness.with_drive(|drive, _| drive.trip.truck.transmission.automatic = true);
    rolling(&mut harness, 10, 55.0);
    let mut input = freight_fate::touch::TouchInput::new();
    for event in input.handle(Gesture::HoldLowerBegan).events {
        harness.app.handle_event(&event);
    }
    brake_to_a_stop(&mut harness);
    assert_eq!(neutral_lines(&harness), 0, "{:?}", harness.transcript());
    assert_eq!(clutch(&harness).0, TouchClutch::Out);
}

/// Three touch shifts out of neutral retire the instruction.
#[test]
fn the_neutral_line_drops_its_instruction_after_three_shifts_out_of_neutral() {
    let mut harness = a_manual_truck("Touch Neutral Retire", true);
    for _ in 0..3 {
        harness.app.dispatch_gesture(Gesture::LowerHoldSwipeUp);
        assert_eq!(gear(&harness), 1);
        frames(&mut harness, 1.0);
        harness.with_drive(|drive, _| drive.trip.truck.transmission.gear = 0);
    }
    rolling(&mut harness, 10, 3.0);
    frames(&mut harness, 2.0);
    assert_eq!(gear(&harness), 0);
    let said = harness.transcript();
    assert!(
        said.iter().any(|line| line.ends_with("] Neutral.")),
        "{said:?}"
    );
    assert!(!heard(&harness, NEUTRAL_LINE), "{said:?}");
}

fn governed(harness: &PlaytestHarness) -> (f64, f64) {
    harness.read_drive(|drive| {
        let truck = &drive.trip.truck;
        (truck.coupled_rpm(None), truck.specs.max_rpm)
    })
}

/// Slowed to 10 in tenth, the truck is in neutral; a touch flick up takes
/// the gear that fits the road speed, not first.
#[test]
fn a_touch_flick_up_from_neutral_while_rolling_takes_a_gear_that_fits() {
    let mut harness = a_manual_truck("Touch Neutral Rolling Gear", true);
    rolling(&mut harness, 0, 10.0);
    harness.app.dispatch_gesture(Gesture::LowerHoldSwipeUp);
    let gear = gear(&harness);
    assert!(gear > 1, "took gear {gear}");
    let (rpm, max) = governed(&harness);
    assert!(rpm <= max, "gear {gear} at {rpm:.0} rpm over {max:.0}");
    let lower = harness.read_drive(|drive| drive.trip.truck.coupled_rpm(Some(gear - 1)));
    assert!(lower > max, "gear {} would also fit", gear - 1);
    assert!(!heard(&harness, "Grinding"), "{:?}", harness.transcript());
}

#[test]
fn a_touch_flick_up_from_neutral_when_stopped_takes_first() {
    let mut harness = a_manual_truck("Touch Neutral Stopped Gear", true);
    harness.app.dispatch_gesture(Gesture::LowerHoldSwipeUp);
    assert_eq!(gear(&harness), 1);
}

#[test]
fn a_keyboard_shift_up_from_neutral_while_rolling_still_takes_first() {
    let mut harness = a_manual_truck("Keyboard Neutral Rolling Gear", false);
    rolling(&mut harness, 0, 10.0);
    harness
        .app
        .ctx
        .input
        .press(Key::LShift, freight_fate::states::base::Mods::NONE);
    frames(&mut harness, 1.0 / 60.0);
    harness.key(key_event(Key::W, Some('w')));
    assert_eq!(gear(&harness), 1);
}
