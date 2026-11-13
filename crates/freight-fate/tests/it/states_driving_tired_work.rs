//! Tired work is slow work (`docs/fatigue-design.md`, part 4): what the
//! driver does by hand takes longer in a higher fatigue band, and every row
//! and result that reads those minutes reads the longer figure. Time someone
//! else controls keeps its own.

use ff_core::sim::alertness::Alertness;
use freight_fate::app::testing::TestApp;
use freight_fate::app::SharedState;
use freight_fate::states::driving_core::{
    set_engine_running, CHAIN_INSTALL_MIN, FUEL_STOP_MIN, WALK_AROUND_MIN,
};
use freight_fate::states::driving_menu_states::DriveRef;
use freight_fate::states::driving_rest_states::RestStopState;

use crate::states_driving_menus_support::*;

/// Relaxed pacing, the driving-mode setting (the game's default).
const RELAXED: f64 = 10.0;
/// Standard pacing.
const STANDARD: f64 = 20.0;

fn set_fatigue(app: &mut TestApp, drive: &SharedState, fatigue: f64) {
    app.ctx.profile.as_mut().expect("a career").fatigue = fatigue;
    with_drive(drive, |d| {
        d.alertness = Alertness::from_fatigue(fatigue);
        d.trip.time_scale = STANDARD;
    });
}

/// A drive at `fatigue`, parked at a travel center with pumps, engine off
/// and the tank part empty.
fn at_the_pumps(app: &mut TestApp, fatigue: f64, time_scale: f64) -> (SharedState, RestStopState) {
    let drive = a_drive(app);
    set_fatigue(app, &drive, fatigue);
    let at = drive_and_ctx(&drive, app, |d, ctx| {
        d.trip.time_scale = time_scale;
        d.trip.truck.fuel_gal = 40.0;
        d.trip.truck.velocity_mps = 0.0;
        set_engine_running(ctx, &mut d.trip.truck, false);
        d.trip.position_mi
    });
    let state =
        RestStopState::with_drive(DriveRef::of(&drive), travel_center("Flying J", at), false);
    (drive, state)
}

/// Fuel and return (minutes it took, what the fuel row's help read first).
fn fuel(fatigue: f64, time_scale: f64) -> (f64, String, String) {
    let mut app = TestApp::new();
    let (drive, mut state) = at_the_pumps(&mut app, fatigue, time_scale);
    let item = row(&mut state, &mut app.ctx, "Refuel");
    let help = item.help_text(&state, &app.ctx);
    let before = with_drive(&drive, |d| d.trip.game_minutes);
    app.clear_speech();
    activate(&mut state, &mut app.ctx, "Refuel");
    let after = with_drive(&drive, |d| d.trip.game_minutes);
    (after - before, help, app.main_lines().join(" "))
}

#[test]
fn fueling_rested_takes_twenty_minutes_and_the_row_says_so() {
    let (minutes, help, said) = fuel(10.0, STANDARD);
    assert_eq!(minutes, FUEL_STOP_MIN);
    assert!(help.contains("20 minutes on duty."), "{help}");
    assert!(said.contains("Fueling took 20 minutes."), "{said}");
}

#[test]
fn fueling_at_severe_takes_thirty_minutes_and_the_row_reads_thirty() {
    let (minutes, help, said) = fuel(85.0, STANDARD);
    assert_eq!(minutes, 30.0);
    assert!(help.contains("30 minutes on duty."), "{help}");
    assert!(said.contains("Fueling took 30 minutes."), "{said}");
}

#[test]
fn relaxed_pacing_halves_the_extra_at_severe() {
    let (minutes, help, _) = fuel(85.0, RELAXED);
    assert_eq!(minutes, 25.0);
    assert!(help.contains("25 minutes on duty."), "{help}");
}

#[test]
fn the_walk_around_at_drowsy_reads_and_takes_the_longer_figure() {
    let mut app = TestApp::new();
    let drive = a_drive(&mut app);
    set_fatigue(&mut app, &drive, 65.0);
    let at = with_drive(&drive, |d| d.trip.position_mi);
    let mut state =
        RestStopState::with_drive(DriveRef::of(&drive), travel_center("Flying J", at), false);
    // 15 x 1.25 = 18.75, which rounds to 19.
    let help = row(&mut state, &mut app.ctx, "Walk around the truck").help_text(&state, &app.ctx);
    assert!(help.contains("19 minutes on duty."), "{help}");
    let before = with_drive(&drive, |d| d.trip.game_minutes);
    app.clear_speech();
    activate(&mut state, &mut app.ctx, "Walk around the truck");
    let after = with_drive(&drive, |d| d.trip.game_minutes);
    assert_eq!(after - before, 19.0);
    assert_eq!(WALK_AROUND_MIN * 1.25, 18.75);
    let said = app.main_lines().join(" ");
    assert!(said.contains("Walk-around done, 19 minutes."), "{said}");
}

#[test]
fn chain_install_at_drowsy_rounds_thirty_seven_and_a_half_up_to_thirty_eight() {
    let mut app = TestApp::new();
    let drive = a_drive(&mut app);
    set_fatigue(&mut app, &drive, 65.0);
    let (minutes, before, after, text) = drive_and_ctx(&drive, &mut app, |d, ctx| {
        // Daylight, so the dark's own multiplier is not in it.
        let start = d.trip.start_hour;
        d.trip.game_minutes = ((12.0 - start).rem_euclid(24.0)) * 60.0;
        d.trip.truck.velocity_mps = 0.0;
        let minutes = d.chain_install_min(ctx);
        let before = d.trip.game_minutes;
        let text = d
            .do_install_chains(ctx)
            .expect("stopped, so the chains go on");
        (minutes, before, d.trip.game_minutes, text)
    });
    assert_eq!(CHAIN_INSTALL_MIN * 1.25, 37.5);
    assert_eq!(minutes, 38.0);
    assert_eq!(after - before, 38.0);
    assert!(text.starts_with("Chains on in 38 minutes."), "{text}");
}

#[test]
fn work_someone_else_does_keeps_its_time_at_severe() {
    // A scale's inspection lane, the officer's time: the report's minutes
    // whatever the driver's band.
    let mut app = TestApp::new();
    let drive = a_drive(&mut app);
    set_fatigue(&mut app, &drive, 85.0);
    let (report_min, burned) = drive_and_ctx(&drive, &mut app, |d, ctx| {
        let report = d.inspection_report(
            ctx,
            ff_core::sim::roadside_inspection::InspectionLevel::DriverOnly,
        );
        let before = d.trip.game_minutes;
        // Hours off, so the officer's fatigue order does not run here.
        ctx.settings.hos_mode = "off".to_string();
        d.settle_inspection(ctx, &report);
        (report.minutes(), d.trip.game_minutes - before)
    });
    assert_eq!(burned, report_min);
}

/// The minutes the receiver's dock work takes, from the dock menu's row to
/// the settlement, at `fatigue`.
fn dock_work_min(fatigue: f64) -> f64 {
    use freight_fate::playtest::harness::{PlaytestHarness, StartDelivery};
    use freight_fate::states::driving_menu_states::{ArrivalState, FacilityArrivalState};
    let mut harness = PlaytestHarness::new();
    harness.start_delivery(StartDelivery::named("Unload Tired"));
    harness.app.ctx.profile.as_mut().expect("a career").fatigue = fatigue;
    harness.prepare_for_driving(30.0);
    harness.with_drive(|d, ctx| {
        d.trip.time_scale = STANDARD;
        d.trip.position_mi = d.trip.total_miles();
        d.trip.finished = true;
        d.truck_mut().velocity_mps = 0.0;
        d.truck_mut().set_parking_brake();
        d.handle_arrival_gate(ctx);
    });
    harness.finish_timed_state();
    assert!(harness.state_is::<FacilityArrivalState>());
    let before = harness.read_drive(|d| d.trip.game_minutes);
    let rows = harness.menu_labels();
    let action = [
        "Drop the loaded trailer and hook an empty",
        "Dock and deliver",
    ]
    .into_iter()
    .find(|label| rows.iter().any(|row| row == label))
    .unwrap_or_else(|| panic!("no way to deliver: {rows:?}"));
    harness.select_menu_item(action);
    harness.finish_timed_state();
    assert!(harness.state_is::<ArrivalState>());
    harness.read_drive(|d| d.trip.game_minutes) - before
}

#[test]
fn unloading_at_severe_takes_the_receivers_time_not_longer() {
    let rested = dock_work_min(10.0);
    assert!(rested > 0.0);
    assert_eq!(dock_work_min(85.0), rested);
}
