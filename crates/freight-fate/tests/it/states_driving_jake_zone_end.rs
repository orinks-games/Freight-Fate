//! The end of a no engine brake zone: said once, for a zone the driver was
//! told about on this pass, and never for one they were not (issue 520).

use ff_core::models::jobs::{Job, CARGO_CATALOG};
use ff_core::models::profile::Profile;
use ff_core::sim::jake_zone::ZoneNotice;

use freight_fate::app::testing::TestApp;
use freight_fate::states::driving::DrivingState;
use freight_fate::states::driving_core::*;

const END: &str = "End of no engine brake zone.";

/// Buffalo to Rochester: the Buffalo end of the route is inside the origin
/// city's zone, and the open road follows it.
fn a_drive(app: &mut TestApp) -> DrivingState {
    let world = app.ctx.world;
    let mut profile = Profile::named_in("Zone End", "Buffalo");
    profile.tutorial_done = true;
    app.ctx.profile = Some(profile);
    let route = world
        .supported_route("Buffalo", "Rochester", None)
        .expect("the world routes")
        .expect("Buffalo to Rochester is supported");
    let mut job = Job::new(
        CARGO_CATALOG
            .get("general")
            .expect("the general cargo type"),
        12.0,
        "Buffalo",
        "company yard",
        "Rochester",
        route.miles(),
        1000.0,
        12.0,
    );
    job.destination_location = "Rochester freight market".to_string();
    let mut drive = DrivingState::new(
        &mut app.ctx,
        job,
        route,
        Some(0),
        DRIVE_PHASE_DELIVERY,
        Some(12.0),
    );
    drive.trip.set_npc_vehicles(Vec::new());
    drive
}

/// The truck at `mile`, rolling at road speed, retarder on or off.
fn roll(drive: &mut DrivingState, mile: f64, jake: bool) {
    drive.trip.position_mi = mile;
    drive.trip.truck.engine_on = true;
    drive.trip.truck.transmission.gear = 8;
    drive.trip.truck.throttle = 0.0;
    drive.trip.truck.velocity_mps = 55.0 / 2.23694;
    drive.trip.truck.set_engine_brake(jake);
}

/// A mile inside the Buffalo zone, and the first tenth past its end.
fn zone_and_exit(drive: &DrivingState) -> (f64, f64) {
    let inside = 2.0;
    assert!(drive.trip.engine_brake_ban_at(inside).is_some());
    let mut mile = inside;
    while drive.trip.engine_brake_ban_at(mile).is_some() {
        mile += 0.1;
    }
    (inside, mile)
}

fn ends_said(app: &TestApp) -> usize {
    app.event_lines()
        .iter()
        .filter(|line| line.contains(END))
        .count()
}

#[test]
fn a_zone_the_driver_was_warned_in_says_once_when_it_ends() {
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app);
    let (inside, exit) = zone_and_exit(&drive);
    roll(&mut drive, inside, true);
    drive.update_engine_brake_zone(&mut app.ctx, 0.1); // "Switch it off."
    drive.trip.truck.set_engine_brake(false);
    drive.update_engine_brake_zone(&mut app.ctx, 0.1);
    app.clear_speech();

    roll(&mut drive, exit, false);
    drive.update_engine_brake_zone(&mut app.ctx, 0.1);
    drive.update_engine_brake_zone(&mut app.ctx, 0.1);

    assert_eq!(app.event_lines(), vec![END.to_string()]);
    assert_eq!(drive.jake_zone_notice, ZoneNotice::Untold);
}

#[test]
fn a_zone_an_assist_named_says_when_it_ends() {
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app);
    let (inside, exit) = zone_and_exit(&drive);
    roll(&mut drive, inside, true);
    drive.cruise_jake_stage = 3; // cruise raised it, so cruise lets go
    drive.update_engine_brake_zone(&mut app.ctx, 0.1);
    let spoken = app.event_lines().join(" ");
    assert!(
        spoken.contains("No engine brake zone, Buffalo."),
        "{spoken}"
    );
    app.clear_speech();

    roll(&mut drive, exit, false);
    drive.update_engine_brake_zone(&mut app.ctx, 0.1);

    assert_eq!(ends_said(&app), 1);
}

#[test]
fn a_zone_heard_about_before_it_says_when_it_ends() {
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app);
    let (inside, exit) = zone_and_exit(&drive);
    // The heads-up's own line is covered with the approach callout; the
    // driver then switched the retarder off before the town line.
    drive.jake_zone_notice = ZoneNotice::HeadsUp { start_tenths: 0 };
    roll(&mut drive, inside, false);
    drive.update_engine_brake_zone(&mut app.ctx, 0.1);
    assert!(app.event_lines().is_empty(), "the zone itself stays quiet");

    roll(&mut drive, exit, false);
    drive.update_engine_brake_zone(&mut app.ctx, 0.1);

    assert_eq!(ends_said(&app), 1);
}

#[test]
fn a_zone_the_driver_never_heard_about_ends_quietly() {
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app);
    let (inside, exit) = zone_and_exit(&drive);
    roll(&mut drive, inside, false);
    drive.update_engine_brake_zone(&mut app.ctx, 0.1);
    roll(&mut drive, exit, false);
    drive.update_engine_brake_zone(&mut app.ctx, 0.1);

    assert!(app.event_lines().is_empty(), "{:?}", app.event_lines());
}

#[test]
fn terse_speech_skips_the_end_line() {
    let mut app = TestApp::new();
    app.ctx.settings.driving_speech = "quiet".to_string();
    let mut drive = a_drive(&mut app);
    let (inside, exit) = zone_and_exit(&drive);
    roll(&mut drive, inside, true);
    drive.update_engine_brake_zone(&mut app.ctx, 0.1); // the warning still speaks
    drive.trip.truck.set_engine_brake(false);
    app.clear_speech();

    roll(&mut drive, exit, false);
    drive.update_engine_brake_zone(&mut app.ctx, 0.1);

    assert_eq!(ends_said(&app), 0);
}
