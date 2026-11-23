//! Integration tests for PR 3 driving assistance improvements:
//! 1. Grade-aware predictive cruise & smooth resume
//! 2. Directional blind-spot warning & assisted signal-wait-glide lane change
//! 3. Independent Steering vs. Speed assist toggles & save migration
//! 4. Non-verbal assist handoff earcons & snow lane-keep dropout

use ff_core::settings::Settings;
use ff_core::sim::lane_change_assist::{AssistedLaneChange, BlindSpotSide};
use ff_core::sim::lane_keep_status::{LaneKeepAvailability, LaneKeepDropoutReason};
use ff_core::sim::predictive_cruise::{PredictiveCruiseEvent, PredictiveCruiseState};
use ff_core::sim::traffic_manager::TrafficVehicle;
use ff_core::sim::weather::WeatherKind;
use freight_fate::playtest::harness::{PlaytestHarness, StartDelivery};
use freight_fate::states::base::Key;
use freight_fate::states::driving_core::assist_cues::{
    BLIND_SPOT_CHIRP_KEY, CURVE_SERVO_KEY, LANE_KEEP_DROPOUT_KEY,
};

use crate::transcript_cruise_support::{frame, DT};

fn open_highway_drive(name: &str, preset: &str) -> PlaytestHarness {
    let mut harness = PlaytestHarness::new();
    harness.start_delivery(StartDelivery::named(name));
    harness.prepare_for_driving(60.0);
    harness
        .app
        .ctx
        .settings
        .apply_driving_assistance_preset(preset);
    harness.with_drive(|d, _| {
        d.trip.truck.velocity_mps = 60.0 / 2.2369362920544;
        d.trip.position_mi = 20.0;
        d.lane.lane = 0;
        d.lane.lane_count = 2;
        d.trip.traffic_manager.vehicles.clear();
    });
    harness
}

#[test]
fn blind_spot_chirp_plays_directionally_on_balanced_without_waiting() {
    let mut harness = open_highway_drive("PR3 Blind Spot Balanced", "balanced");
    let audio = harness.app.record_audio();

    harness.with_drive(|d, _| {
        let pos = d.trip.position_mi;
        let beside =
            TrafficVehicle::new("bench:beside", pos, 60.0, 60.0, 1, "cruising", "car").with_lane(1);
        d.trip.traffic_manager.vehicles.push(beside);
    });

    harness
        .app
        .app
        .ctx
        .input
        .press(Key::Left, freight_fate::states::base::Mods::default());
    frame(&mut harness, DT);
    harness
        .app
        .app
        .ctx
        .input
        .release(Key::Left, freight_fate::states::base::Mods::default());

    let chirps: Vec<(f64, f64)> = audio
        .borrow()
        .played
        .iter()
        .filter(|(k, _, _)| k == BLIND_SPOT_CHIRP_KEY)
        .map(|(_, vol, pan)| (*vol, *pan))
        .collect();
    assert!(!chirps.is_empty(), "expected blind-spot chirp on Balanced");
    assert!(
        chirps.iter().any(|(_, pan)| *pan < -0.2),
        "expected left-panned chirp, got {chirps:?}"
    );
    assert_eq!(
        harness.read_drive(|d| d.assisted_lane_change.waiting_target()),
        None,
        "Balanced should warn without entering assisted waiting"
    );
    assert_eq!(harness.read_drive(|d| d.lane.lane), 0);
}

#[test]
fn assisted_lane_change_on_all_signals_waits_for_gap_and_glides() {
    let mut harness = open_highway_drive("PR3 Assisted Lane Change", "all");
    let audio = harness.app.record_audio();

    harness.with_drive(|d, _| {
        let pos = d.trip.position_mi;
        let beside =
            TrafficVehicle::new("bench:beside", pos, 60.0, 60.0, 1, "cruising", "car").with_lane(1);
        d.trip.traffic_manager.vehicles.push(beside);
    });

    harness.press_key(Key::Left, None);
    frame(&mut harness, DT);

    let chirps: Vec<f64> = audio
        .borrow()
        .played
        .iter()
        .filter(|(k, _, _)| k == BLIND_SPOT_CHIRP_KEY)
        .map(|(_, _, pan)| *pan)
        .collect();
    assert!(
        chirps.iter().any(|pan| *pan < -0.2),
        "expected left-panned chirp on All, got {chirps:?}"
    );
    assert_eq!(
        harness.read_drive(|d| d.assisted_lane_change.waiting_target()),
        Some((1, BlindSpotSide::Left))
    );
    assert_eq!(
        harness.read_drive(|d| d.trip.traffic_manager.player_signal_lane),
        Some(1),
        "waiting should feed player_signal_lane for courtesy yielding"
    );

    // Once the gap opens, the truck glides smoothly into lane 1 and returns to Idle.
    harness.with_drive(|d, _| {
        d.trip.traffic_manager.vehicles.clear();
    });
    for _ in 0..240 {
        frame(&mut harness, DT);
        if harness
            .read_drive(|d| d.lane.lane == 1 && d.assisted_lane_change == AssistedLaneChange::Idle)
        {
            break;
        }
    }
    assert_eq!(harness.read_drive(|d| d.lane.lane), 1);
    assert_eq!(
        harness.read_drive(|d| d.assisted_lane_change),
        AssistedLaneChange::Idle
    );
}

#[test]
fn independent_steering_and_speed_assist_pairings_work_and_migrate() {
    let mut settings = Settings::default();

    // Manual speed/jake paired with full lane-centering.
    settings.apply_steering_assist_mode("full");
    settings.apply_speed_assist_mode("manual");
    assert_eq!(settings.lane_keeping, "full");
    assert_eq!(settings.descent_speed_control, "off");
    assert!(!settings.automatic_emergency_braking);
    assert_eq!(settings.steering_assist_mode, "full");
    assert_eq!(settings.speed_assist_mode, "manual");
    assert_eq!(settings.driving_assistance_preset, "custom");

    // Strong adaptive speed paired with manual steering keeps curve braking
    // active while suppressing bend steering.
    settings.apply_steering_assist_mode("manual");
    settings.apply_speed_assist_mode("strong");
    assert_eq!(settings.lane_keeping, "off");
    assert_eq!(settings.descent_speed_control, "interactive");
    assert!(settings.automatic_emergency_braking);
    assert!(settings.curve_speed_assist);
    assert!(!settings.road_steers_the_bend());
    assert_eq!(settings.steering_assist_mode, "manual");
    assert_eq!(settings.speed_assist_mode, "strong");
    assert_eq!(settings.driving_assistance_preset, "custom");

    // Existing presets stay intact and sync both independent axes.
    settings.apply_driving_assistance_preset("balanced");
    assert_eq!(settings.driving_assistance_preset, "balanced");
    assert_eq!(settings.steering_assist_mode, "partial");
    assert_eq!(settings.speed_assist_mode, "standard");
    assert!(settings.road_steers_the_bend());

    // Legacy settings migration infers independent modes cleanly.
    let mut legacy = Settings::default();
    legacy.apply_driving_assistance_preset("realistic");
    legacy.lane_keeping = "full".to_string();
    legacy.steering_assist_mode.clear();
    legacy.speed_assist_mode.clear();
    legacy.refresh_assist_modes();
    assert_eq!(legacy.steering_assist_mode, "full");
    assert_eq!(legacy.speed_assist_mode, "light");
}

#[test]
fn snow_covered_road_drops_out_lane_keep_with_earcon_and_recovers() {
    let mut harness = open_highway_drive("PR3 Snow Dropout", "all");
    let audio = harness.app.record_audio();

    harness.with_drive(|d, _| {
        d.trip.weather.current = WeatherKind::Snow;
        d.trip.truck.grip = 0.55;
    });
    frame(&mut harness, DT);

    assert_eq!(
        harness.read_drive(|d| d.lane_keep_availability),
        LaneKeepAvailability::DroppedOut {
            reason: LaneKeepDropoutReason::SnowCovered,
        }
    );
    let dropout_plays = audio
        .borrow()
        .played
        .iter()
        .filter(|(k, _, _)| k == LANE_KEEP_DROPOUT_KEY)
        .count();
    assert!(
        dropout_plays >= 1,
        "expected lane-keep dropout earcon on snow-covered low-grip road"
    );

    let driving = harness
        .app
        .driving_observation()
        .expect("driving observation");
    assert_eq!(driving.lane_keep_availability.key(), "dropped_out_snow");

    // Recovery when snow clears and grip returns.
    harness.with_drive(|d, _| {
        d.trip.weather.current = WeatherKind::Clear;
        d.trip.truck.grip = 1.0;
    });
    frame(&mut harness, DT);
    assert_eq!(
        harness.read_drive(|d| d.lane_keep_availability),
        LaneKeepAvailability::Active
    );
}

#[test]
fn predictive_cruise_smooth_resume_and_curve_servo_earcon() {
    let mut harness = open_highway_drive("PR3 Predictive And Curve Earcon", "all");
    let audio = harness.app.record_audio();

    // Trigger smooth resume state directly as green-light / work-zone exit does.
    harness.with_drive(|d, _| {
        d.predictive_cruise = d
            .predictive_cruise
            .next(PredictiveCruiseEvent::TriggerSmoothResume { target_mph: 65.0 });
    });
    assert!(matches!(
        harness.read_drive(|d| d.predictive_cruise),
        PredictiveCruiseState::ResumingSmooth { .. }
    ));
    let driving = harness
        .app
        .driving_observation()
        .expect("driving observation");
    assert_eq!(driving.predictive_cruise.key(), "resuming_smooth");

    // Curve speed servo plays a short earcon when transitioning from 0 to positive braking.
    harness.with_drive(|d, ctx| {
        let here = d.trip.position_mi;
        d.trip.truck.velocity_mps = 65.0 / 2.2369362920544;
        d.trip.truck.brake = 0.0;
        d.arm_curve_servo(35.0, here - 0.1, here + 2.0, false);
        d.update_curve_speed_servo(ctx);
    });
    let servo_plays = audio
        .borrow()
        .played
        .iter()
        .filter(|(k, _, _)| k == CURVE_SERVO_KEY)
        .count();
    assert!(
        servo_plays >= 1,
        "expected curve servo earcon when curve braking engages"
    );
}
