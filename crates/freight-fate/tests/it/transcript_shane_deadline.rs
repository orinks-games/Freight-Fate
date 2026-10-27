//! Shane P's Tyler-to-Payson deadline report, reproduced against the Rust
//! clock key and settlement.

use ff_core::models::business::COMPANY_DRIVER;
use ff_core::models::career::LEVEL_XP;
use ff_core::models::carrier_fleet::assigned_truck_key;
use ff_core::models::jobs::{Job, CARGO_CATALOG};
use ff_core::models::profile::Profile;
use ff_core::models::trailer_yard::{delivery_plan, LIVE_UNLOAD_MIN};
use ff_core::models::trucks::truck_model;
use freight_fate::app::testing::TestApp;
use freight_fate::playtest::harness::{key_event, PlaytestHarness, StartDelivery};
use freight_fate::states::base::{InputEvent, Key};
use freight_fate::states::driving::DrivingState;
use freight_fate::states::driving_core::{advance_rest_clock, DRIVE_PHASE_DELIVERY};
use freight_fate::states::driving_menu_states::{ArrivalState, FacilityArrivalState};

const DEADLINE_H: f64 = 41.0;
// The game clock uses Eastern Time as its absolute reference: this is
// 1:31 AM Central at Tyler and 12:31 AM Mountain at Payson.
const START_EASTERN_H: f64 = 2.0 + 31.0 / 60.0;

fn shane_job() -> Job {
    let mut job = Job::new(
        &CARGO_CATALOG["bulk"],
        19.0,
        "Tyler",
        "Tyler Energy Terminal",
        "Payson",
        1163.0,
        5862.0,
        DEADLINE_H,
    );
    job.origin_type = "chemical_petroleum_terminal".to_string();
    job.destination_location = "Payson Materials Yard".to_string();
    job.destination_type = "construction_materials_yard".to_string();
    job
}

fn shane_drive(app: &mut TestApp) -> DrivingState {
    app.ctx.settings.automatic_transmission = true;
    let job = shane_job();
    let mut profile = Profile::named_in("munchkinbear", "Tyler");
    profile.business_status = COMPANY_DRIVER.to_string();
    profile.carrier_key = "northstar".to_string();
    profile.career.xp = LEVEL_XP[9] + 7395.0;
    profile.career.reputation = 68.0;
    profile.career.deliveries = 49;
    profile.trailer_programs = vec!["bulk".to_string()];
    profile.game_hours = START_EASTERN_H;
    assert_eq!(profile.career.level(), 10);
    assert_eq!(assigned_truck_key(&profile, Some(&job)), "long_run_midroof");
    app.ctx.profile = Some(profile);

    let route = app
        .ctx
        .world
        .supported_route("Tyler", "Payson", None)
        .expect("the world loads")
        .expect("Tyler to Payson is supported");
    let drive = DrivingState::new(
        &mut app.ctx,
        job,
        route,
        Some(0x5A4E),
        DRIVE_PHASE_DELIVERY,
        Some(START_EASTERN_H),
    );
    let assigned = truck_model("long_run_midroof").expect("assigned tractor model");
    assert_eq!(drive.trip.truck.specs.max_torque_nm, 2550.0);
    assert_eq!(drive.trip.truck.specs.fuel_tank_gal, 185.0);
    assert_eq!(drive.trip.truck.specs.drag_coefficient, 0.62);
    assert_eq!(drive.trip.truck.specs.fuel_burn_factor, 0.96);
    assert_eq!(drive.trip.truck.specs.mass_kg, 36_100.0);
    assert_eq!(assigned.cab, "sleeper");
    assert_eq!(assigned.spec, "standard");
    assert!(drive.trip.truck.transmission.automatic);
    assert_eq!(drive.trip.truck.cargo_kg, 19_000.0);
    assert!(drive.trip.truck.trailer_attached);
    drive
}

fn press_clock(app: &mut TestApp, drive: &mut DrivingState) -> String {
    app.clear_speech();
    drive.handle_key_event(&mut app.ctx, &InputEvent::key(Key::C));
    app.main_lines().last().cloned().expect("clock speech")
}

fn place_eight_miles_before_destination_exit(app: &mut TestApp, drive: &mut DrivingState) {
    let exit = drive
        .destination_exit_stop(&mut app.ctx)
        .expect("Payson destination exit");
    drive.trip.position_mi = exit.at_mi - 8.0;
    drive.trip.truck.velocity_mps = 53.0 / 2.23694;
}

#[test]
fn shane_payson_on_schedule_check_in_stays_on_time_after_live_unload() {
    let mut app = TestApp::new();
    let mut drive = shane_drive(&mut app);
    place_eight_miles_before_destination_exit(&mut app, &mut drive);

    // The logged required reset: 4:46 AM Mountain, then 2:46 PM Mountain.
    drive.trip.game_minutes = 28.25 * 60.0;
    assert!(
        (drive.trip.local_hour() - (4.0 + 46.0 / 60.0)).abs() < 0.02,
        "local hour was {} in {}",
        drive.trip.local_hour(),
        drive.trip.current_timezone().name
    );
    advance_rest_clock(&mut drive, &mut app.ctx, 600.0, Some("off_duty"), "TA");
    assert!((drive.trip.local_hour() - (14.0 + 46.0 / 60.0)).abs() < 0.02);
    assert!((DEADLINE_H - drive.trip.game_minutes / 60.0 - 2.75).abs() < 1e-9);

    // At the report's 5:01 PM boundary, the clock truthfully says the truck
    // can reach the receiver before its 5:31 PM appointment.
    drive.trip.game_minutes = 40.5 * 60.0;
    let clock = press_clock(&mut app, &mut drive);
    assert!(clock.starts_with("5:01 PM Mountain Time"), "{clock}");
    assert!(clock.contains("On schedule. Arrival 0.2 hours"), "{clock}");
    assert!(clock.contains("Deadline 0.5"), "{clock}");

    // Shane checked in at 5:19 PM, then the construction yard's receiver
    // spent 45 minutes unloading. The visible clock advances to 6:04 PM,
    // but the appointment, payout, and bonus belong to the check-in instant.
    let check_in_hours = 40.8;
    drive.trip.game_minutes = check_in_hours * 60.0;
    let plan = delivery_plan(&drive.job, app.ctx.profile.as_ref().expect("career"));
    assert_eq!(plan.mode, "live_load");
    assert_eq!(plan.minutes, LIVE_UNLOAD_MIN);
    advance_rest_clock(
        &mut drive,
        &mut app.ctx,
        plan.minutes,
        None,
        "receiver unloading",
    );
    assert!((drive.trip.game_minutes / 60.0 - 41.55).abs() < 1e-9);
    let arrival = ArrivalState::new_at(&mut app.ctx, &mut drive, check_in_hours);
    let transcript = arrival.summary_lines.join("\n");
    assert!(
        transcript.contains("Trip time: 40.8 hours, on time."),
        "{transcript}"
    );
    assert!(
        transcript.contains("Receiver service after check-in: 0.8 hours."),
        "{transcript}"
    );
    assert!(transcript.contains("It is 6:04 PM."), "{transcript}");
    assert!(
        transcript.contains("On-time delivery bonus:"),
        "{transcript}"
    );
    assert!(!transcript.contains("late."), "{transcript}");

    println!("CLOCK: {clock}");
    println!("SETTLEMENT:\n{transcript}");
}

#[test]
fn shane_payson_check_in_after_the_appointment_remains_late() {
    let mut app = TestApp::new();
    let mut drive = shane_drive(&mut app);
    let check_in_hours = 41.1;
    drive.trip.game_minutes = (check_in_hours + LIVE_UNLOAD_MIN / 60.0) * 60.0;

    let arrival = ArrivalState::new_at(&mut app.ctx, &mut drive, check_in_hours);
    let transcript = arrival.summary_lines.join("\n");
    assert!(
        transcript.contains("Trip time: 41.1 hours, late."),
        "{transcript}"
    );
    assert!(
        !transcript.contains("On-time delivery bonus:"),
        "{transcript}"
    );
}

#[test]
fn receiver_menu_captures_check_in_before_advancing_the_unload_clock() {
    let mut harness = PlaytestHarness::new();
    harness.start_delivery(StartDelivery::named("Receiver Check In"));
    harness.with_drive(|drive, ctx| {
        drive.tutorial = None;
        drive.job.destination_type = "construction_materials_yard".to_string();
        drive.job.deadline_game_h = DEADLINE_H;
        drive.trip.game_minutes = 40.8 * 60.0;
        drive.trip.position_mi = drive.trip.total_miles();
        drive.trip.finished = true;
        drive.destination_exit_taken = true;
        drive.trip.truck.velocity_mps = 0.0;
        drive.handle_arrival_gate(ctx);
    });
    harness.finish_timed_state();
    assert!(harness.state_is::<FacilityArrivalState>());

    harness.key(key_event(Key::Return, None));
    harness.finish_timed_state();
    assert!(harness.state_is::<ArrivalState>());

    let transcript = {
        let state = harness.app.ctx.state().expect("arrival state");
        let borrowed = state.borrow();
        borrowed
            .as_any()
            .downcast_ref::<ArrivalState>()
            .expect("delivery settlement")
            .summary_lines
            .join("\n")
    };
    assert!(
        transcript.contains("Trip time: 40.9 hours, on time."),
        "{transcript}"
    );
    assert!(
        transcript.contains("Receiver service after check-in: 0.8 hours."),
        "{transcript}"
    );
    assert!(
        transcript.contains("On-time delivery bonus:"),
        "{transcript}"
    );
}
