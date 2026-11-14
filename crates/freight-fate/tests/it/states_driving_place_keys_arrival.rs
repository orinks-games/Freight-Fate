//! The place keys (Alt+1 to Alt+5) off the destination exit: they name where
//! the delivery is going, not where it started (issue 552).

use ff_core::data::world::get_world;
use ff_core::models::jobs::{Job, CARGO_CATALOG};
use ff_core::models::profile::Profile;
use ff_core::sim::weather::WeatherKind;

use freight_fate::app::testing::TestApp;
use freight_fate::states::driving::DrivingState;
use freight_fate::states::driving_core::DRIVE_PHASE_DELIVERY;

/// A Buffalo, New York to Erie, Pennsylvania delivery: two states, so the
/// state key can tell the ends apart as well as the town key.
fn a_drive(app: &mut TestApp) -> DrivingState {
    let world = get_world();
    let mut profile = Profile::named_in("Places", "Buffalo");
    profile.tutorial_done = true;
    app.ctx.profile = Some(profile);
    let route = world
        .supported_route("Buffalo", "Erie", None)
        .expect("the world routes")
        .expect("Buffalo to Erie has a route");
    let mut job = Job::new(
        &CARGO_CATALOG["general"],
        12.0,
        "Buffalo",
        "company yard",
        "Erie",
        route.miles(),
        1000.0,
        12.0,
    );
    job.destination_location = "Erie freight market".to_string();
    let mut drive = DrivingState::new(
        &mut app.ctx,
        job,
        route,
        None,
        DRIVE_PHASE_DELIVERY,
        Some(12.0),
    );
    drive.trip.set_npc_vehicles(Vec::new());
    drive.trip.weather.current = WeatherKind::Clear;
    drive
}

/// Off the destination exit, on the highway trip still: the ramp, or the
/// whole approach with facility streets off.
fn on_the_destination_ramp(d: &mut DrivingState) {
    d.facility_streets_on = false;
    d.trip.position_mi = d.trip.route.miles() - 0.1;
    d.destination_exit_taken = true;
}

#[test]
fn test_town_key_names_the_destination_off_the_exit() {
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    on_the_destination_ramp(&mut d);
    app.clear_speech();
    d.speak_current_town(&mut app.ctx);
    let line = app.main_lines().last().expect("a town line").clone();
    assert!(line.starts_with("In Erie"), "{line}");
    assert!(!line.contains("Buffalo"), "{line}");
}

#[test]
fn test_state_key_names_the_destination_state_off_the_exit() {
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    on_the_destination_ramp(&mut d);
    app.clear_speech();
    d.speak_current_state(&mut app.ctx);
    assert_eq!(
        app.main_lines().last().expect("a state line"),
        "In Pennsylvania."
    );
}

#[test]
fn test_place_record_names_the_destination_off_the_exit() {
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    on_the_destination_ramp(&mut d);
    let place = d.record_place(&app.ctx);
    assert!(place.contains("Erie"), "{place}");
    assert!(!place.contains("Buffalo"), "{place}");
}

#[test]
fn test_town_key_names_the_destination_on_its_streets() {
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    d.facility_streets_on = true;
    d.trip.position_mi = d.trip.route.miles() - 0.1;
    d.destination_exit_taken = true;
    assert!(
        d.begin_surface_chain(&mut app.ctx, false),
        "the Erie freight market has a street chain"
    );
    app.clear_speech();
    d.speak_current_town(&mut app.ctx);
    d.speak_current_state(&mut app.ctx);
    let lines = app.main_lines();
    assert!(lines[0].starts_with("In Erie"), "{lines:?}");
    assert_eq!(lines[1], "In Pennsylvania.");
}
