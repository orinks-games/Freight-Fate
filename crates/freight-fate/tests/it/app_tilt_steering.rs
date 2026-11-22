//! Tilt steering through the real loop: the Touch gestures rows, level taken
//! as the drive comes up, one lane per tilt with lane keeping on full,
//! steering in proportion with it off, a gap in the readings, and Set tilt
//! level (`app::tilt`, `ff_core::sim::tilt_steering`).

use ff_core::sim::lane_push::LanePush;
use ff_core::sim::weather::WeatherKind;

use freight_fate::playtest::harness::{PlaytestHarness, StartDelivery};
use freight_fate::states::main_menu::TouchGesturesState;
use freight_fate::touch::{Gesture, TouchInput};

const MPH_PER_MPS: f64 = 2.236_936_292_054_402;

fn a_tilt_drive(name: &str, lane_keeping: &str) -> PlaytestHarness {
    let mut harness = PlaytestHarness::new();
    harness.app.ctx.settings.touch_tilt_steering = true;
    harness.app.ctx.settings.lane_keeping = lane_keeping.to_string();
    harness.app.ctx.tilt.set_agent_pose(Some(0.0));
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
        drive.trip.truck.start_engine();
        drive.trip.truck.parking_brake = false;
        drive.trip.truck.velocity_mps = 55.0 / MPH_PER_MPS;
    });
    frames(&mut harness, 0.5);
    harness.clear_speech();
    harness
}

fn frames(harness: &mut PlaytestHarness, seconds: f64) {
    for _ in 0..(seconds * 60.0).ceil() as usize {
        harness.advance_frame_clock();
        harness.app.frame(1.0 / 60.0);
    }
}

fn tilt(harness: &mut PlaytestHarness, degrees: Option<f64>, seconds: f64) {
    harness.app.ctx.tilt.set_agent_pose(degrees);
    frames(harness, seconds);
}

fn heard(harness: &PlaytestHarness, text: &str) -> bool {
    harness.transcript().iter().any(|line| line.contains(text))
}

#[test]
fn the_drive_takes_level_and_a_held_tilt_asks_for_one_lane() {
    let mut harness = a_tilt_drive("Tilt One Lane", "full");
    assert_eq!(harness.app.ctx.tilt.state().key(), "live");
    assert_eq!(harness.app.ctx.tilt.state().level(), Some(0.0));
    assert!(harness.transcript().is_empty());
    let asked = |harness: &PlaytestHarness| harness.read_drive(|drive| drive.lane_change_target);
    assert_eq!(asked(&harness), None);
    // Past half of full (17 degrees at the default 30), held for seconds.
    harness.app.ctx.tilt.set_agent_pose(Some(-22.0));
    frames(&mut harness, 1.0 / 60.0);
    let first = asked(&harness);
    assert!(first.is_some(), "{:?}", harness.transcript());
    assert_eq!(
        harness.read_drive(|drive| drive.lane_push),
        LanePush::AskedLeft
    );
    // Held: no second request lands while it stays over.
    let mut targets = vec![first];
    for _ in 0..240 {
        frames(&mut harness, 1.0 / 60.0);
        let target = asked(&harness);
        if target.is_some() && target != *targets.last().unwrap() {
            targets.push(target);
        }
    }
    assert_eq!(targets.len(), 1, "{targets:?}");
    // Back to level, and the push is free again.
    tilt(&mut harness, Some(0.0), 0.5);
    assert_eq!(
        harness.read_drive(|drive| drive.lane_push),
        LanePush::Centred
    );
}

#[test]
fn with_lane_keeping_off_a_tilt_steers_in_proportion() {
    let mut harness = a_tilt_drive("Tilt Proportion", "off");
    // Level is wherever the phone was: a phone held at 10 degrees.
    tilt(&mut harness, Some(10.0), 0.0);
    harness.with_drive(|drive, ctx| {
        drive.push_pause_menu(ctx);
    });
    frames(&mut harness, 0.1);
    assert_eq!(harness.app.ctx.tilt.state().key(), "waiting");
    harness.app.ctx.pop_state();
    frames(&mut harness, 0.4);
    assert_eq!(harness.app.ctx.tilt.state().level(), Some(10.0));
    // 17 degrees from that level is half of full.
    tilt(&mut harness, Some(27.0), 0.1);
    let steering = harness.read_drive(|drive| drive.lane.steering);
    assert!((steering - 0.5).abs() < 1e-6, "{steering}");
    tilt(&mut harness, Some(12.0), 0.1);
    let steering = harness.read_drive(|drive| drive.lane.steering);
    assert_eq!(steering, 0.0, "inside the dead zone");
}

#[test]
fn a_gap_in_the_readings_is_said_once_and_their_return_takes_level() {
    let mut harness = a_tilt_drive("Tilt Lost", "full");
    tilt(&mut harness, None, 2.0);
    assert_eq!(harness.app.ctx.tilt.state().key(), "lost");
    let lost = harness
        .transcript()
        .iter()
        .filter(|line| line.contains("No tilt reading. Swipe to steer."))
        .count();
    assert_eq!(lost, 1, "{:?}", harness.transcript());
    tilt(&mut harness, Some(8.0), 0.5);
    assert!(heard(&harness, "Tilt steering back."));
    assert_eq!(harness.app.ctx.tilt.state().level(), Some(8.0));
}

#[test]
fn set_tilt_level_takes_level_from_the_pose_now() {
    let mut harness = a_tilt_drive("Tilt Set Level", "full");
    tilt(&mut harness, Some(9.0), 0.2);
    harness.with_drive(|drive, ctx| drive.set_tilt_level(ctx));
    frames(&mut harness, 0.5);
    assert!(heard(&harness, "Level set."));
    assert_eq!(harness.app.ctx.tilt.state().level(), Some(9.0));
}

#[test]
fn with_tilt_steering_off_nothing_steers_and_nothing_is_said() {
    let mut harness = a_tilt_drive("Tilt Off", "off");
    harness.app.ctx.settings.touch_tilt_steering = false;
    tilt(&mut harness, Some(40.0), 1.0);
    assert_eq!(harness.app.ctx.tilt.state().key(), "off");
    assert_eq!(harness.read_drive(|drive| drive.lane.steering), 0.0);
    harness.with_drive(|drive, ctx| drive.set_tilt_level(ctx));
    frames(&mut harness, 0.5);
    assert!(
        harness.transcript().is_empty(),
        "{:?}",
        harness.transcript()
    );
}

#[test]
fn the_touch_gestures_screen_turns_tilt_steering_on_and_steps_its_angle() {
    let mut harness = PlaytestHarness::new();
    harness.app.push_state(TouchGesturesState::new());
    assert_eq!(
        harness.focused_label().as_deref(),
        Some("Tilt steering: Off")
    );
    assert!(!harness
        .menu_labels()
        .iter()
        .any(|l| l.starts_with("Tilt for full steering")));
    let mut input = TouchInput::new();
    let mut touch = |harness: &mut PlaytestHarness, gesture| {
        for event in input.handle(gesture).events {
            harness.app.handle_event(&event);
        }
    };
    touch(&mut harness, Gesture::SwipeUp);
    assert!(harness.app.ctx.settings.touch_tilt_steering);
    assert_eq!(
        harness.focused_label().as_deref(),
        Some("Tilt steering: On")
    );
    touch(&mut harness, Gesture::SwipeRight);
    assert_eq!(
        harness.focused_label().as_deref(),
        Some("Tilt for full steering: 30 degrees")
    );
    touch(&mut harness, Gesture::SwipeUp);
    assert_eq!(harness.app.ctx.settings.touch_tilt_full(), 35.0);
    for _ in 0..10 {
        touch(&mut harness, Gesture::SwipeDown);
    }
    assert_eq!(
        harness.focused_label().as_deref(),
        Some("Tilt for full steering: 15 degrees")
    );
    // Off again, and the angle row goes with it.
    touch(&mut harness, Gesture::SwipeLeft);
    touch(&mut harness, Gesture::SwipeDown);
    assert!(!harness.app.ctx.settings.touch_tilt_steering);
    assert!(!harness
        .menu_labels()
        .iter()
        .any(|l| l.starts_with("Tilt for full steering")));
}

#[test]
fn a_device_with_no_motion_sensor_keeps_the_row_off_and_says_so() {
    let mut harness = PlaytestHarness::new();
    harness.app.ctx.tilt.sensor_present = Some(false);
    harness.app.push_state(TouchGesturesState::new());
    harness.clear_speech();
    let mut input = TouchInput::new();
    for event in input.handle(Gesture::SwipeUp).events {
        harness.app.handle_event(&event);
    }
    assert!(!harness.app.ctx.settings.touch_tilt_steering);
    assert_eq!(
        harness.transcript(),
        vec!["This device has no motion sensor.".to_string()]
    );
    assert_eq!(
        harness.focused_label().as_deref(),
        Some("Tilt steering: Off")
    );
}

#[test]
fn a_sensor_found_missing_once_on_turns_the_switch_off() {
    let mut harness = PlaytestHarness::new();
    harness.app.ctx.settings.touch_tilt_steering = true;
    harness.app.ctx.tilt.set_agent_no_sensor();
    harness.app.push_state(TouchGesturesState::new());
    harness.clear_speech();
    frames(&mut harness, 0.2);
    assert!(!harness.app.ctx.settings.touch_tilt_steering);
    let saved = ff_core::settings::Settings::load();
    assert!(!saved.touch_tilt_steering);
    assert_eq!(
        harness.transcript(),
        vec!["This device has no motion sensor.".to_string()]
    );
    assert_eq!(harness.app.ctx.tilt.state().key(), "off");
    assert_eq!(harness.app.ctx.tilt.sensor_present, Some(false));
    assert!(!harness
        .menu_labels()
        .iter()
        .any(|l| l.starts_with("Tilt for full steering")));
}

#[test]
fn leaving_the_app_stops_tilt_and_the_return_takes_level() {
    use ff_core::app_lifecycle::AppEvent;
    use freight_fate::states::base::InputEvent;
    let mut harness = a_tilt_drive("Tilt Leave App", "full");
    assert_eq!(harness.app.ctx.tilt.state().level(), Some(0.0));
    harness
        .app
        .handle_event(&InputEvent::App(AppEvent::WillResignActive));
    tilt(&mut harness, Some(-25.0), 0.5);
    assert_eq!(harness.app.ctx.tilt.state().key(), "waiting");
    assert_eq!(harness.read_drive(|drive| drive.lane_change_target), None);
    harness
        .app
        .handle_event(&InputEvent::App(AppEvent::DidBecomeActive));
    tilt(&mut harness, Some(12.0), 0.1);
    // Leaving at the wheel paused the drive; resuming takes level.
    harness.app.ctx.pop_state();
    frames(&mut harness, 0.5);
    assert_eq!(harness.app.ctx.tilt.state().key(), "live");
    assert_eq!(harness.app.ctx.tilt.state().level(), Some(12.0));
}
