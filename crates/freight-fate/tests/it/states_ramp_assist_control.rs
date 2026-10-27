//! Rust regressions for TowerAlphaTheta15's ramp-assist report (#185).

use ff_core::data::world::get_world;
use ff_core::models::jobs::make_reposition_job;
use ff_core::models::profile::Profile;
use ff_core::sim::trip_models::RoadStop;
use ff_core::sim::weather::WeatherKind;
use freight_fate::app::testing::TestApp;
use freight_fate::states::driving::DrivingState;
use freight_fate::states::driving_core::*;

fn a_real_drive(app: &mut TestApp) -> DrivingState {
    let world = get_world();
    let mut profile = Profile::named_in("Ramps", "Denver");
    profile.tutorial_done = true;
    app.ctx.profile = Some(profile);
    let job = make_reposition_job(world, "Denver", "Cheyenne", false, None)
        .expect("Denver to Cheyenne is a supported reposition");
    let route = world
        .shortest_route("Denver", "Cheyenne", None, false)
        .expect("the world routes")
        .expect("Denver to Cheyenne has a route");
    let mut drive = DrivingState::new(
        &mut app.ctx,
        job,
        route,
        Some(0),
        DRIVE_PHASE_DELIVERY,
        Some(12.0),
    );
    // The bubble is its own suite's business; an empty road keeps these
    // deterministic (`driving_feature_helpers.quiet_trip`). The weather is
    // the other half of that helper: the trip seed is unseeded, so a drive
    // that does not pin the sky draws a real condition and an ice day caps
    // the safe speed under whatever the test is measuring.
    drive.trip.set_npc_vehicles(Vec::new());
    drive.trip.weather.current = WeatherKind::Clear;
    drive
}

fn mph_to_mps(mph: f64) -> f64 {
    mph / 2.2369362920544
}

/// `_FakeStop`: a bare route point at a milepost.
fn a_stop(at_mi: f64) -> RoadStop {
    RoadStop::new("Test Plaza", at_mi, "travel_center")
}

/// `_on_ramp`: the truck mid-ramp at the terminal bar with a known light.
fn on_ramp(drive: &mut DrivingState, control: &str, red: bool, mph: f64) {
    drive.trip.truck.start_engine();
    drive.trip.truck.velocity_mps = mph_to_mps(mph);
    drive.ramp_mi = Some(RAMP_ACCESS_MI); // right at the terminal bar
    drive.ramp_control = control.to_string();
    drive.ramp_light_offset_s = if red { 0.0 } else { RAMP_LIGHT_RED_S }; // phase start
    drive.ramp_light_timer = 0.0;
    drive.ramp_light_announced = true;
    drive.ramp_light_last_phase = if red { "red" } else { "green" }.to_string();
    drive.ramp_terminal_done = false;
    drive.ramp_waiting_at_light = false;
    drive.ramp_stop = Some(a_stop(drive.trip.position_mi + 0.5));
}

#[test]
fn ramp_assist_releases_a_completed_snub() {
    let mut app = TestApp::new();
    let mut d = a_real_drive(&mut app);
    app.ctx.settings.route_transition_assist = true;
    on_ramp(&mut d, "stop", false, 15.0);
    d.ramp_mi = Some(RAMP_ACCESS_MI + 1000.0 / 5280.0);
    d.ramp_assist_brake = 0.25;
    d.trip.truck.brake = 0.0;
    d.trip.truck.throttle = 0.5;
    d.update_ramp_terminal_assist(&mut app.ctx);
    assert_eq!(d.ramp_assist_brake, 0.0);
    assert_eq!(d.trip.truck.brake, 0.0);
    assert_eq!(d.trip.truck.throttle, 0.5);
}

#[test]
fn ramp_assist_accelerator_wins_through_the_frame_loop() {
    use freight_fate::states::base::{Key, Mods};
    let mut app = TestApp::new();
    let mut d = a_real_drive(&mut app);
    app.ctx.settings.route_transition_assist = true;
    app.ctx.settings.destination_approach_assist = false;
    // A straight, empty highway keeps unrelated curve/traffic assists from
    // owning the pedals while this test exercises the terminal override.
    crate::transcript_cruise_support::bench_road(&mut d, 65.0, 0.0, 1.0);
    on_ramp(&mut d, "stop", false, 35.0);
    d.ramp_mi = Some(RAMP_ACCESS_MI + 0.08);
    d.ramp_assist_brake = 0.25;
    d.trip.truck.brake = 0.0;
    d.trip.truck.throttle = 0.5;
    d.trip.truck.set_air_ready(false);
    d.trip.truck.transmission.gear = 10;
    d.departure_checked = true;
    app.ctx.input.press(Key::Up, Mods::NONE);
    d.update_frame(&mut app.ctx, 1.0 / 60.0);
    assert_eq!(d.ramp_assist_brake, 0.0);
    assert!(d.trip.truck.throttle > 0.0);
    app.ctx.input.release(Key::Up, Mods::NONE);
    d.update_frame(&mut app.ctx, 1.0 / 60.0);
    assert!(d.ramp_assist_brake > 0.0);
}

#[test]
fn ramp_assist_release_has_hysteresis_and_keeps_the_close_stop() {
    let mut app = TestApp::new();
    let mut d = a_real_drive(&mut app);
    app.ctx.settings.route_transition_assist = true;
    on_ramp(&mut d, "stop", false, 15.0);
    let gap_m = 100.0;
    d.ramp_mi = Some(RAMP_ACCESS_MI + gap_m / 1609.344);
    // Start a snub, keep it through the dead band, finish it, then stay
    // released through that same band until braking is needed again.
    for (demand, braking) in [
        (0.7_f64, true),
        (0.45, true),
        (0.3, false),
        (0.45, false),
        (0.7, true),
    ] {
        d.trip.truck.velocity_mps = (2.0 * gap_m * demand).sqrt();
        d.trip.truck.brake = 0.0;
        d.update_ramp_terminal_assist(&mut app.ctx);
        assert_eq!(d.ramp_assist_brake > 0.0, braking, "demand {demand}");
    }
    // A low demand must not release the final approach to the stop line.
    d.ramp_mi = Some(RAMP_ACCESS_MI + 20.0 / 1609.344);
    d.trip.truck.velocity_mps = mph_to_mps(5.0);
    d.ramp_assist_brake = 0.0;
    d.update_ramp_terminal_assist(&mut app.ctx);
    assert!(d.ramp_assist_brake > 0.0);
}

#[test]
fn ramp_assist_release_does_not_erase_a_manual_brake() {
    let mut app = TestApp::new();
    let mut d = a_real_drive(&mut app);
    on_ramp(&mut d, "stop", false, 15.0);
    d.ramp_mi = Some(RAMP_ACCESS_MI + 1000.0 / 5280.0);
    d.ramp_assist_brake = 0.25;
    d.trip.truck.brake = 0.7;
    d.update_ramp_terminal_assist(&mut app.ctx);
    assert_eq!(d.ramp_assist_brake, 0.0);
    assert_eq!(d.trip.truck.brake, 0.7);
}

#[test]
fn ramp_assist_full_lane_destination_already_works_without_a_signal() {
    let mut app = TestApp::new();
    let mut d = a_real_drive(&mut app);
    app.ctx.settings.apply_driving_assistance_preset("all");
    let stop = RoadStop::new(
        "Destination",
        d.trip.position_mi + 1.0,
        "delivery_destination",
    );
    d.exit_stop = Some(stop.clone());
    d.exit_signal_on = false;
    d.trip.truck.velocity_mps = mph_to_mps(90.0);
    d.trip.truck.brake = 0.0;
    assert!(d.exit_intent_ready(&app.ctx, &stop));
    d.update_exit_preparation(&mut app.ctx, 1.0 / 60.0);
    assert!(d.trip.truck.brake >= 0.35);
}

/// Stopped 400 feet short of a red light: who drives the truck up to it.
fn stopped_short_of_a_red(app: &mut TestApp, armed: bool, brake: f64) -> f64 {
    let mut d = a_real_drive(app);
    app.ctx.settings.route_transition_assist = true;
    on_ramp(&mut d, "signal", true, 0.0);
    d.ramp_mi = Some(RAMP_ACCESS_MI + 400.0 / 5280.0);
    d.speed_control_armed = armed;
    d.trip.truck.brake = brake;
    d.trip.truck.throttle = 0.0;
    d.update_ramp_terminal_assist(&mut app.ctx);
    d.trip.truck.throttle
}

#[test]
fn with_speed_control_on_the_assist_drives_up_to_a_light_it_stopped_short_of() {
    // Agent drive (2026-10-25): every assist on, the truck coasted to a stand
    // 400 feet short of a red and sat there through every green after it.
    let mut app = TestApp::new();
    assert!(stopped_short_of_a_red(&mut app, true, 0.0) > 0.0);
}

#[test]
fn without_speed_control_closing_up_to_the_light_is_the_drivers() {
    let mut app = TestApp::new();
    assert_eq!(stopped_short_of_a_red(&mut app, false, 0.0), 0.0);
}

#[test]
fn a_foot_on_the_brake_short_of_the_light_is_not_driven_through() {
    let mut app = TestApp::new();
    assert_eq!(stopped_short_of_a_red(&mut app, true, 0.5), 0.0);
}
