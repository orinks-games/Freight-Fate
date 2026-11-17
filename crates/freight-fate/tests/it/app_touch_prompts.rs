//! On a touch screen the truck's prompts name gestures in sentences of their
//! own: no "Press a ...", no "... from the three-finger tap list to ...",
//! and no keyboard key. Each case stages one moment at the wheel on touch and
//! checks everything said.

use ff_core::input_hints::HINTS;
use ff_core::sim::weather::WeatherKind;

use freight_fate::playtest::harness::{PlaytestHarness, StartDelivery};
use freight_fate::states::driving::DrivingState;
use freight_fate::states::driving_core::{Instructor, Tutorial, PULL_OVER_LIGHTS};
use freight_fate::touch::{Gesture, RoadOffer};

const MPH_PER_MPS: f64 = 2.236_936_292_054_402;

/// A quiet drive, stopped with the engine off, with touch hints on.
fn a_touch_drive(name: &str, automatic: bool) -> PlaytestHarness {
    let mut harness = PlaytestHarness::new();
    harness.start_delivery(StartDelivery::named(name));
    harness.app.ctx.settings.automatic_transmission = automatic;
    harness.app.ctx.settings.driving_hints = true;
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
        drive.trip.truck.transmission.automatic = automatic;
        drive.trip.truck.transmission.gear = 0;
    });
    harness.app.dispatch_gesture(Gesture::KeyboardDisconnected);
    harness.app.dispatch_gesture(Gesture::Tap);
    assert!(harness.app.ctx.touch_hints());
    harness.clear_speech();
    harness
}

fn frames(harness: &mut PlaytestHarness, seconds: f64) {
    for _ in 0..(seconds * 60.0).ceil() as usize {
        harness.advance_frame_clock();
        harness.app.frame(1.0 / 60.0);
    }
}

fn fill_air(harness: &mut PlaytestHarness) {
    harness.with_drive(|drive, _| {
        drive.trip.truck.set_air_pressure_psi(120.0);
    });
}

/// Every line said, checked for keyboard wording; returns them joined.
fn touch_worded(harness: &PlaytestHarness) -> String {
    let said = harness.transcript();
    for line in &said {
        for bad in ["Press a", "press a", "tap list to"] {
            assert!(!line.contains(bad), "{bad:?} in {line:?}");
        }
        let words: Vec<&str> = line
            .split(|c: char| !c.is_alphanumeric())
            .filter(|w| !w.is_empty())
            .collect();
        for (_, (key, _)) in HINTS {
            let key_words: Vec<&str> = key.split(' ').collect();
            let named = words
                .windows(key_words.len())
                .any(|window| window == key_words.as_slice());
            // "the Up arrow" and its kin are never touch words; a lone
            // capital letter is a key only where it stands as a word.
            assert!(!named, "the key {key:?} in {line:?}");
        }
    }
    said.join("\n")
}

fn first_run(automatic: bool) -> String {
    let mut harness = a_touch_drive("Touch Coaching", automatic);
    harness.with_drive(|drive, ctx| {
        let mut tutorial = Tutorial::new();
        tutorial.begin(ctx);
        drive.tutorial = Some(Box::new(tutorial));
    });
    // Unanswered, the start line comes back as a reminder.
    frames(&mut harness, 26.0);
    harness.app.dispatch_gesture(Gesture::RotateRight);
    fill_air(&mut harness);
    frames(&mut harness, 1.0);
    if !automatic {
        // A manual left in neutral after the release is reminded of first.
        harness.app.dispatch_gesture(Gesture::HoldUpperBegan);
        harness.app.dispatch_gesture(Gesture::HoldEnded);
        frames(&mut harness, 26.0);
        harness.app.dispatch_gesture(Gesture::HoldLowerBegan);
        harness.app.dispatch_gesture(Gesture::LowerHoldSwipeUp);
        harness.app.dispatch_gesture(Gesture::HoldEnded);
        frames(&mut harness, 2.0);
    }
    harness.app.dispatch_gesture(Gesture::HoldUpperBegan);
    frames(&mut harness, 1.0);
    harness.with_drive(|drive, _| drive.trip.truck.velocity_mps = 25.0 / MPH_PER_MPS);
    frames(&mut harness, 0.5);
    touch_worded(&harness)
}

#[test]
fn the_first_run_in_an_automatic_coaches_gestures() {
    let said = first_run(true);
    for line in [
        "Your first run. Turn two fingers right to start the engine.",
        "Reminder: turn two fingers right to start the engine.",
        "At air ready, swipe up and hold to release the parking brake and drive.",
        "Parking brake released.",
        "Rolling. Tap for your speed. Tap with three fingers, then double tap, for a full report. \
         Flick up with two fingers for all the controls. Brake hard at a hazard warning. Swipe \
         down deep and hold to stop fast.",
    ] {
        assert!(said.contains(line), "{line:?} not in:\n{said}");
    }
}

#[test]
fn the_first_run_in_a_manual_coaches_the_second_finger_for_first() {
    let said = first_run(false);
    for line in [
        "At air ready, swipe up and hold to release the parking brake, then flick up with a \
         second finger for first gear.",
        "Parking brake released. Shift into first gear.",
        "Reminder: hold a pedal and flick up with a second finger for first gear.",
        "In gear. Swipe up and hold to accelerate.",
    ] {
        assert!(said.contains(line), "{line:?} not in:\n{said}");
    }
    assert!(!said.contains("clutch"), "{said}");
}

#[test]
fn a_stall_names_the_turn_and_the_flick() {
    let mut harness = a_touch_drive("Touch Stall", false);
    harness.with_drive(|drive, _| {
        drive.trip.truck.start_engine();
        drive.trip.truck.parking_brake = false;
        drive.trip.truck.transmission.gear = 6;
        drive.trip.truck.transmission.clutch = 0.0;
    });
    harness.clear_speech();
    frames(&mut harness, 1.0);
    let said = touch_worded(&harness);
    assert!(
        said.contains("Engine stalled. Turn two fingers right"),
        "{said}"
    );
}

#[test]
fn a_trooper_stop_names_the_double_tap() {
    let mut harness = a_touch_drive("Touch Trooper", true);
    harness.with_drive(|drive, ctx| {
        drive.trip.truck.start_engine();
        drive.trip.truck.parking_brake = false;
        drive.trip.truck.velocity_mps = 70.0 / MPH_PER_MPS;
        drive.begin_pull_over(ctx, 55.0);
    });
    frames(&mut harness, 0.5);
    let said = touch_worded(&harness);
    assert!(
        said.contains("Double tap to signal, then stop on the shoulder."),
        "{said}"
    );
    harness.with_drive(|drive, ctx| {
        drive.pull_over = Some(PULL_OVER_LIGHTS.to_string());
        drive.try_rest_stop(ctx);
    });
    let said = touch_worded(&harness);
    assert!(
        said.contains("Resolve the police stop first. Double tap to signal the trooper stop."),
        "{said}"
    );
}

#[test]
fn the_gate_names_the_deep_swipe_for_the_parking_brake() {
    let mut harness = a_touch_drive("Touch Gate", true);
    harness.with_drive(|drive, ctx| drive.say_gate_brake_line(ctx, false));
    frames(&mut harness, 0.5);
    let said = touch_worded(&harness);
    assert!(
        said.contains("Swipe down deep to set the parking brake and dock."),
        "{said}"
    );
}

#[test]
fn back_from_a_rest_stop_gas_releases_the_brake() {
    let mut harness = a_touch_drive("Touch Rest", true);
    let lines = harness.with_drive(|drive, ctx| {
        vec![
            drive.engine_entry_instruction(ctx),
            drive.parking_brake_hint_tail(ctx),
            drive.air_start_instruction(ctx),
        ]
    });
    assert_eq!(
        lines[..2],
        [
            "Turn two fingers right to start the engine.",
            "At air ready, swipe up and hold to release the parking brake and drive.",
        ]
    );
    assert!(
        lines[2].ends_with(
            "psi. At 100 psi, swipe up and hold to release the parking brake and drive."
        ),
        "{lines:?}"
    );
}

#[test]
fn out_of_fuel_names_the_turn_for_the_restart() {
    let mut harness = a_touch_drive("Touch Fuel", true);
    harness.with_drive(|drive, ctx| drive.handle_out_of_fuel(ctx));
    frames(&mut harness, 0.5);
    let said = touch_worded(&harness);
    assert!(
        said.contains("Turn two fingers right to restart the engine."),
        "{said}"
    );
}

#[test]
fn a_keyboard_brings_the_keyboard_lines_back() {
    let mut harness = a_touch_drive("Touch Then Keys", true);
    harness.app.dispatch_gesture(Gesture::KeyboardConnected);
    harness.key(freight_fate::states::base::InputEvent::KeyDown {
        key: freight_fate::states::base::Key::Space,
        mods: Default::default(),
        text: None,
        repeat: false,
    });
    assert!(!harness.app.ctx.touch_hints());
    let line = harness.with_drive(|drive, ctx| drive.engine_entry_instruction(ctx));
    assert_eq!(line, "Press E to start the engine.");
}

/// Stopped on `stop_type`'s ramp: the first route stop of that type, or the
/// first stop retyped, with the truck on it at a standstill.
fn stopped_on_a_ramp(harness: &mut PlaytestHarness, stop_type: Option<&str>) {
    harness.with_drive(|drive, _| {
        drive.trip.truck.start_engine();
        drive.trip.truck.parking_brake = false;
        drive.trip.truck.velocity_mps = 0.0;
        let mut stop = drive
            .trip
            .stops
            .iter()
            .find(|s| s.stop_type != "weigh_station")
            .cloned()
            .expect("the route has a stop");
        if let Some(kind) = stop_type {
            stop.stop_type = kind.to_string();
        }
        drive.trip.position_mi = stop.at_mi;
        drive.ramp_stop = Some(stop);
        drive.ramp_mi = Some(0.05);
    });
    harness.clear_speech();
}

#[test]
fn stopped_at_a_stops_ramp_end_a_double_tap_opens_it() {
    let mut harness = a_touch_drive("Touch Stop Offer", true);
    stopped_on_a_ramp(&mut harness, None);
    assert_eq!(
        harness.with_drive(|drive, ctx| drive.road_offer(ctx)),
        RoadOffer::Stop
    );
    harness.with_drive(|drive, ctx| drive.update_touch_stop_offer(ctx));
    frames(&mut harness, 0.2);
    let said = touch_worded(&harness);
    assert!(said.contains("Double tap to pull in."), "{said}");
    harness.app.dispatch_gesture(Gesture::DoubleTap);
    assert!(
        !harness.state_is::<DrivingState>(),
        "{:#?}",
        harness.transcript()
    );
}

#[test]
fn stopped_on_a_scale_the_line_is_check_in() {
    let mut harness = a_touch_drive("Touch Scale Offer", true);
    stopped_on_a_ramp(&mut harness, Some("weigh_station"));
    assert_eq!(
        harness.with_drive(|drive, ctx| drive.road_offer(ctx)),
        RoadOffer::Stop
    );
    harness.with_drive(|drive, ctx| drive.update_touch_stop_offer(ctx));
    frames(&mut harness, 0.2);
    let said = touch_worded(&harness);
    assert!(said.contains("Double tap to check in."), "{said}");
}

#[test]
fn rolling_past_a_stop_or_stopped_on_the_highway_is_not_a_stop_offer() {
    let mut harness = a_touch_drive("Touch No Stop Offer", true);
    stopped_on_a_ramp(&mut harness, None);
    harness.with_drive(|drive, _| drive.trip.truck.velocity_mps = 30.0 / MPH_PER_MPS);
    assert_ne!(
        harness.with_drive(|drive, ctx| drive.road_offer(ctx)),
        RoadOffer::Stop
    );
    // Stopped beside a stop with no ramp under the truck.
    harness.with_drive(|drive, _| {
        drive.trip.truck.velocity_mps = 0.0;
        drive.ramp_stop = None;
        drive.ramp_mi = None;
    });
    assert_ne!(
        harness.with_drive(|drive, ctx| drive.road_offer(ctx)),
        RoadOffer::Stop
    );
}
