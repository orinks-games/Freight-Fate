//! Backing down a traveled lane: the ladder, and where it must stay quiet
//! (port of `tests/test_wrong_way.py`).
//!
//! The adversarial harness backed a tractor-trailer a full mile down an
//! interstate and the game's only spoken line in all that time was a merge
//! instruction for the exit it was reversing away from. A sighted player
//! would see the world sliding the wrong way; a blind player had nothing,
//! which makes this an accessibility gap before it is a realism one.
//!
//! Python patched `ctx.say_event`; here the lines are read off `ctx.speech`,
//! below the ladder and the pacer. Nothing in this file changes as a result:
//! every rung is SAFETY, which no rung of the driving verbosity ladder
//! silences, and the four "must stay quiet" cases assert an empty transcript,
//! which is stricter at this seam than it was at the old one.

use ff_core::models::business::LEASED_OWNER_OPERATOR;
use ff_core::models::jobs::{cargo_type, Job};
use ff_core::models::profile::Profile;
use ff_core::sim::transmission::REVERSE;
use freight_fate::app::testing::TestApp;
use freight_fate::states::driving::DrivingState;
use freight_fate::states::driving_core::DRIVE_PHASE_DELIVERY;
use freight_fate::states::driving_wrong_way::{
    WRONG_WAY_REMIND_MI, WRONG_WAY_TRAFFIC_MI, WRONG_WAY_WARN_MI,
};

const MPS_PER_MPH: f64 = 1.0 / 2.23694;
const DT: f64 = 1.0 / 60.0;

/// `_driving(app)`: an owner-operator running Denver -> Salt Lake City.
fn a_drive(app: &mut TestApp) -> DrivingState {
    let mut profile = Profile::named_in("Backer", "Denver");
    profile.business_status = LEASED_OWNER_OPERATOR.to_string();
    profile.owned_trucks = vec!["rig".to_string()];
    app.ctx.profile = Some(profile);
    let route = app
        .ctx
        .world
        .route_from_cities(&["Denver".to_string(), "Salt Lake City".to_string()])
        .expect("Denver to Salt Lake City is a route");
    let job = Job::new(
        cargo_type("general").unwrap(),
        12.0,
        "Denver",
        "yard",
        "Salt Lake City",
        200.0,
        900.0,
        12.0,
    );
    let mut drive = DrivingState::new(
        &mut app.ctx,
        job,
        route,
        Some(99),
        DRIVE_PHASE_DELIVERY,
        Some(10.0),
    );
    drive.truck_mut().set_air_ready(false);
    drive
}

/// `_back_up`: roll the truck backwards along the route, a step at a time.
fn back_up(app: &mut TestApp, d: &mut DrivingState, miles: f64) {
    let step_mi = 0.005;
    d.truck_mut().transmission.gear = REVERSE;
    d.truck_mut().velocity_mps = 8.0 * MPS_PER_MPH;
    let mut travelled = 0.0;
    while travelled < miles {
        d.trip.last_moved_mi = -step_mi;
        d.trip.position_mi = (d.trip.position_mi - step_mi).max(0.0);
        d.update_wrong_way(&mut app.ctx, DT);
        travelled += step_mi;
    }
}

/// `_open_road`: park the truck mid-route, clear of any stop, yard or
/// receiver.
fn open_road(d: &mut DrivingState) {
    d.trip.position_mi = d.trip.total_miles() / 2.0;
    d.trip.stops.clear();
}

// -- the ladder --------------------------------------------------------------

#[test]
fn test_backing_a_truck_length_says_so() {
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    open_road(&mut d);
    app.clear_speech();

    back_up(&mut app, &mut d, WRONG_WAY_REMIND_MI * 1.5);

    assert!(app
        .event_lines()
        .iter()
        .any(|line| line.to_lowercase().contains("reverse")));
}

#[test]
fn test_the_warning_names_it_illegal_and_never_says_zero_miles() {
    // A driver told they are giving the route back needs a real number.
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    open_road(&mut d);
    app.clear_speech();

    back_up(&mut app, &mut d, WRONG_WAY_WARN_MI * 1.2);

    let warnings: Vec<String> = app
        .event_lines()
        .into_iter()
        .filter(|line| line.to_lowercase().contains("wrong way"))
        .collect();
    assert!(!warnings.is_empty());
    assert!(warnings[0].contains("illegal"), "{}", warnings[0]);
    assert!(!warnings[0].contains("0 miles"), "{}", warnings[0]);
}

#[test]
fn test_backing_far_enough_puts_you_into_traffic() {
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    open_road(&mut d);
    let damage_before = d.truck().damage_pct;
    app.clear_speech();

    back_up(&mut app, &mut d, WRONG_WAY_TRAFFIC_MI * 1.2);

    assert!(app
        .event_lines()
        .iter()
        .any(|line| line.to_lowercase().contains("traffic")));
    assert!(d.truck().damage_pct > damage_before);
}

// -- where it must stay quiet ------------------------------------------------

#[test]
fn test_backing_in_the_origin_yard_is_nobodys_business() {
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    d.trip.position_mi = 0.1;
    app.clear_speech();

    back_up(&mut app, &mut d, WRONG_WAY_WARN_MI * 2.0);

    assert_eq!(app.event_lines(), Vec::<String>::new());
}

#[test]
fn test_backing_onto_the_receivers_dock_is_the_job() {
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    d.trip.position_mi = d.trip.total_miles() - 0.1;
    app.clear_speech();

    back_up(&mut app, &mut d, WRONG_WAY_WARN_MI * 2.0);

    assert_eq!(app.event_lines(), Vec::<String>::new());
}

#[test]
fn test_rolling_forward_is_never_the_wrong_way() {
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    open_road(&mut d);
    d.truck_mut().velocity_mps = 60.0 * MPS_PER_MPH;
    app.clear_speech();

    for _ in 0..400 {
        d.trip.last_moved_mi = 0.005;
        d.trip.position_mi += 0.005;
        d.update_wrong_way(&mut app.ctx, DT);
    }

    assert_eq!(app.event_lines(), Vec::<String>::new());
}

#[test]
fn test_stopping_the_truck_forgets_the_stint() {
    // Coming to a stop resets the ladder; a later nudge starts over.
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    open_road(&mut d);

    back_up(&mut app, &mut d, WRONG_WAY_REMIND_MI * 1.5);
    assert!(d.wrong_way_mi > 0.0);

    d.truck_mut().velocity_mps = 0.0;
    d.update_wrong_way(&mut app.ctx, DT);

    assert_eq!(d.wrong_way_mi, 0.0);
}
