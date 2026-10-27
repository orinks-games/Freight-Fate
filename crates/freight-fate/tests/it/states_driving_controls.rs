//! `states/driving_controls.rs` and `states/driving_speed_control.rs`: the
//! discrete key and pad surface at the wheel, and the speed-control session
//! around adaptive cruise and the speed keeper.
//!
//! Ported from `tests/test_info_keys.py`, `test_cruise_steps.py` (its
//! App-driven half; the pure `cruise_step_target` grid is in
//! `states_driving_core.rs`), `test_driving_manual_controls.py`,
//! `test_pedal_latch_assists.py` (brake latch, and that the throttle key never
//! catches one), `test_driving_modes.py` (the keeper's ease window) and
//! `test_turn_commitment.py` (the keeper's corner planner) -- everything a real
//! `DrivingState` can answer without the per-frame loop or a menu state. The
//! rest are listed here, ignored, with their bodies noted, so the two suites
//! diff by name.
//!
//! `tests/test_controls_reference.py` is already ported in full as
//! `app_controls_reference.rs`; it is not repeated here.

use ff_core::data::world::get_world;
use ff_core::models::jobs::{Job, CARGO_CATALOG};
use ff_core::models::profile::Profile;
use ff_core::sim::enforcement_posts::{method_by_kind, EnforcementPost, KIND_MEDIAN};
use ff_core::sim::hos;
use ff_core::sim::transmission::REVERSE;
use ff_core::sim::trip_models::{TripEvent, TripEventData, TripEventKind, Zone};
use ff_core::sim::vehicle::{HIGH_IDLE_DEFAULT_RPM, HIGH_IDLE_STEP_RPM};
use ff_core::sim::weather::WeatherKind;
use ff_core::speech_text::SpokenMessage;

use freight_fate::app::testing::{FakeClock, TestApp};
use freight_fate::controller::ControllerButton;
use freight_fate::states::base::{InputEvent, Key, Mods};
use freight_fate::states::driving::DrivingState;
use freight_fate::states::driving_controls::UPCOMING_MAX_CLAUSES;
use freight_fate::states::driving_core::{
    hos_mut_of, profile_of, DRIVE_PHASE_DELIVERY, DRIVE_PHASE_PICKUP,
};
use freight_fate::states::driving_location::spoken_closing_distance;
use freight_fate::states::driving_menu_states::DrivingStatusState;
use freight_fate::states::driving_pause_states::PauseMenuState;
use freight_fate::states::driving_speed_control::KEEPER_EASE_MAX_MI;

// -- rigging -------------------------------------------------------------------------
//
// `_driving(app, origin, destination, origin_location)` from
// `test_info_keys.py`: a delivery drive on a real short corridor, built
// straight rather than driven up to.

fn a_drive(app: &mut TestApp) -> DrivingState {
    a_drive_between(app, "Buffalo", "Rochester", "company yard")
}

fn a_drive_between(
    app: &mut TestApp,
    origin: &str,
    destination: &str,
    origin_location: &str,
) -> DrivingState {
    let world = get_world();
    app.ctx.profile = Some(Profile::named_in("Info Keys", origin));
    let route = world
        .supported_route(origin, destination, None)
        .expect("the world routes")
        .expect("the corridor is supported");
    let mut job = Job::new(
        &CARGO_CATALOG["general"],
        12.0,
        origin,
        origin_location,
        destination,
        route.miles(),
        1000.0,
        12.0,
    );
    job.destination_location = format!("{destination} freight market");
    let mut drive = DrivingState::new(&mut app.ctx, job, route, None, DRIVE_PHASE_DELIVERY, None);
    // The bubble is its own suite's business; an empty road keeps these
    // deterministic (`driving_feature_helpers.quiet_trip`). The weather is
    // the other half of that helper: the trip seed is unseeded, so a drive
    // that does not pin the sky draws a real condition and an ice day caps
    // the safe speed under whatever the test is measuring.
    drive.trip.set_npc_vehicles(Vec::new());
    drive.trip.weather.current = WeatherKind::Clear;
    drive
}

/// `enforcement_helpers.always_observing_post(at_mi, reach_mi)`: a staffed
/// median post that has already announced itself and sees everything inside
/// its reach, so a readout has no excuse for missing it.
fn observing_post(at_mi: f64, reach_mi: f64) -> EnforcementPost {
    EnforcementPost {
        method: method_by_kind(KIND_MEDIAN).to_string(),
        reach_mi,
        facing: "both".to_string(),
        staffed: true,
        notice: 1.0,
        announced: true,
        leg_index: 0,
        ..EnforcementPost::new(at_mi, KIND_MEDIAN)
    }
}

fn key(k: Key) -> InputEvent {
    InputEvent::key(k)
}

fn alt(k: Key) -> InputEvent {
    InputEvent::key_mods(k, Mods::ALT)
}

fn pad(button: ControllerButton) -> InputEvent {
    InputEvent::button(button)
}

fn mph_to_mps(mph: f64) -> f64 {
    mph / 2.2369362920544
}

/// The last thing the main channel said.
fn last(app: &TestApp) -> String {
    app.main_lines().last().cloned().unwrap_or_default()
}

/// `_cruise_at(driving, mph)` from `test_cruise_steps.py`.
fn cruise_at(drive: &mut DrivingState, app: &mut TestApp, mph: f64) {
    drive.trip.truck.engine_on = true;
    drive.trip.truck.velocity_mps = mph_to_mps(mph);
    drive.engage_cruise(&mut app.ctx, mph, false);
}

include!("states_driving_controls/info.rs");
include!("states_driving_controls/speed_control.rs");
include!("states_driving_controls/route_and_cab.rs");
include!("states_driving_controls/window_and_climate.rs");
