//! What an officer and dispatch do about a tired driver
//! (`docs/fatigue-design.md`, parts 6 and 7): a stop at severe fatigue ends
//! in the fatigue out-of-service order after the stop's own result, and a
//! drowsy driver on a load can call dispatch to move the appointment.

use ff_core::models::business::COMPANY_DRIVER;
use ff_core::models::dispatch_policy::DECLINE_REPUTATION_PENALTY;
use ff_core::sim::alertness::Alertness;
use ff_core::sim::enforcement_posts::{EnforcementPost, KIND_FIXED_SCALE};
use ff_core::sim::fatigue_costs::{
    RestCall, OFFICER_FATIGUE_WARNING, REST_CALL_FALLBACK_H, REST_CALL_REST_H, STAT_REST_CALLS,
};
use ff_core::sim::hos;
use ff_core::sim::trip_models::RoadStop;
use serde_json::{json, Value};

use freight_fate::app::testing::TestApp;
use freight_fate::app::SharedState;
use freight_fate::states::base::Menu;
use freight_fate::states::driving::DrivingState;
use freight_fate::states::driving_dispatch_call::DispatchCallState;
use freight_fate::states::driving_menu_states::DriveRef;
use freight_fate::states::driving_rest_states::{
    EnforcementStopState, RestStopState, TrafficStopState,
};
use freight_fate::states::driving_updates::pending::EnforcementStopParams;

use crate::states_driving_menus_support::*;

const ORDER: &str = "Out of service for fatigue, 10 hours off duty.";
const REST_ROW: &str = "Too tired to drive safely";
const RELAXED: f64 = 10.0;

fn set_fatigue(app: &mut TestApp, drive: &SharedState, fatigue: f64) {
    app.ctx.profile.as_mut().expect("a career").fatigue = fatigue;
    with_drive(drive, |d| {
        d.alertness = Alertness::from_fatigue(fatigue);
        // Standard pacing; relaxed, the default, has its own test.
        d.trip.time_scale = 20.0;
    });
}

/// A first, marginal speeding stop: the trooper's own result is a warning.
fn traffic_stop(app: &mut TestApp, drive: &SharedState) -> String {
    drive_and_ctx(drive, app, |d, ctx| {
        TrafficStopState::new(ctx, d, true, 9.0, 65.0, false, false, false)
            .outcome_text()
            .to_string()
    })
}

fn stop_params(out_of_service: bool) -> EnforcementStopParams {
    EnforcementStopParams {
        title: "Hours violation".to_string(),
        summary: "The officer reads the logbook.".to_string(),
        fine: 100.0,
        reputation_hit: 2.0,
        signaled: true,
        return_message: "Back on the highway.".to_string(),
        out_of_service,
        warned: false,
        construction_zone: false,
        inspection_on_stop: false,
        inspection_level: None,
        fine_is_final: false,
        charged_to_carrier: false,
    }
}

fn index_of(text: &str, part: &str) -> usize {
    text.find(part)
        .unwrap_or_else(|| panic!("{part:?} not in {text:?}"))
}

// -- the officer -----------------------------------------------------------------

#[test]
fn a_traffic_stop_at_severe_gives_its_result_then_the_fatigue_order() {
    let mut app = TestApp::new();
    let drive = a_drive(&mut app);
    set_fatigue(&mut app, &drive, 88.0);
    let before = with_drive(&drive, |d| d.trip.game_minutes);
    let outcome = traffic_stop(&mut app, &drive);
    assert!(
        index_of(&outcome, "lets it go with a warning") < index_of(&outcome, ORDER),
        "{outcome}"
    );
    assert!(!outcome.contains("lets you off"), "{outcome}");
    assert!(
        outcome.contains("the delivery deadline kept counting."),
        "{outcome}"
    );
    assert_eq!(
        with_drive(&drive, |d| d.trip.game_minutes),
        before + hos::SLEEP_MIN
    );
    assert_eq!(with_drive(&drive, |d| d.out_of_service_count), 1);
    assert!(!with_drive(&drive, |d| d.trip.truck.engine_on));
}

#[test]
fn a_traffic_stop_at_drowsy_says_nothing_about_fatigue() {
    let mut app = TestApp::new();
    let drive = a_drive(&mut app);
    set_fatigue(&mut app, &drive, 75.0);
    let outcome = traffic_stop(&mut app, &drive);
    assert!(!outcome.contains("fatigue"), "{outcome}");
    assert!(!outcome.contains("tired"), "{outcome}");
    assert_eq!(with_drive(&drive, |d| d.out_of_service_count), 0);
}

#[test]
fn no_fatigue_order_with_enforcement_bypassed() {
    let mut app = TestApp::new();
    let drive = a_drive(&mut app);
    set_fatigue(&mut app, &drive, 95.0);
    app.ctx.settings.hos_mode = "off".to_string();
    let outcome = traffic_stop(&mut app, &drive);
    assert!(!outcome.contains("fatigue"), "{outcome}");
    assert_eq!(with_drive(&drive, |d| d.out_of_service_count), 0);
}

#[test]
fn relaxed_pacing_warns_the_first_time_then_orders() {
    let mut app = TestApp::new();
    let drive = a_drive(&mut app);
    set_fatigue(&mut app, &drive, 90.0);
    with_drive(&drive, |d| d.trip.time_scale = RELAXED);
    let first = traffic_stop(&mut app, &drive);
    assert!(first.ends_with(OFFICER_FATIGUE_WARNING), "{first}");
    assert!(first.contains("lets you off with a warning"), "{first}");
    assert!(!first.contains(ORDER), "{first}");
    assert_eq!(with_drive(&drive, |d| d.out_of_service_count), 0);

    let second = traffic_stop(&mut app, &drive);
    assert!(second.contains(ORDER), "{second}");
    assert!(!second.contains(OFFICER_FATIGUE_WARNING), "{second}");
    assert_eq!(with_drive(&drive, |d| d.out_of_service_count), 1);
}

#[test]
fn a_stop_that_already_ordered_a_hold_adds_no_second_order() {
    let mut app = TestApp::new();
    let drive = a_drive(&mut app);
    set_fatigue(&mut app, &drive, 95.0);
    app.ctx
        .profile
        .as_mut()
        .expect("a career")
        .hos
        .drive(11.0 * 60.0 + 1.0);
    let before = with_drive(&drive, |d| d.trip.game_minutes);
    let outcome = drive_and_ctx(&drive, &mut app, |d, ctx| {
        EnforcementStopState::new(ctx, d, stop_params(true))
            .outcome_text()
            .to_string()
    });
    assert!(outcome.contains("The order is served"), "{outcome}");
    assert!(!outcome.contains(ORDER), "{outcome}");
    assert_eq!(with_drive(&drive, |d| d.out_of_service_count), 1);
    assert_eq!(
        with_drive(&drive, |d| d.trip.game_minutes),
        before + hos::SLEEP_MIN
    );
}

#[test]
fn a_routine_roadside_inspection_at_severe_ends_in_the_order() {
    let mut app = TestApp::new();
    let drive = a_drive(&mut app);
    set_fatigue(&mut app, &drive, 85.0);
    let mut params = stop_params(false);
    params.inspection_level = Some(ff_core::sim::roadside_inspection::InspectionLevel::DriverOnly);
    let outcome = drive_and_ctx(&drive, &mut app, |d, ctx| {
        EnforcementStopState::new(ctx, d, params)
            .outcome_text()
            .to_string()
    });
    assert!(
        index_of(&outcome, "minutes") < index_of(&outcome, ORDER),
        "{outcome}"
    );
}

/// The scale's inspection lane, with a record the lane always pulls in.
fn scale_check_in(fatigue: f64) -> String {
    let mut app = TestApp::new();
    let drive = a_drive_between(&mut app, "Denver", "Salt Lake City", "Lane Tired");
    {
        let p = app.ctx.profile.as_mut().expect("a career");
        p.business_status = COMPANY_DRIVER.to_string();
        p.career.reputation = 10.0;
        p.driving_record.citations = 6;
        p.driving_record.citation_times = vec![p.game_hours; 6];
        p.out_of_service_events = 3;
        p.driving_record.out_of_service_times = vec![p.game_hours; 3];
    }
    set_fatigue(&mut app, &drive, fatigue);
    with_drive(&drive, |d| d.trip.truck.damage_pct = 70.0);
    let at = with_drive(&drive, |d| d.trip.position_mi);
    let mut stop = RoadStop::new("I-70 West Scale", at, "weigh_station");
    stop.actions = vec!["inspect".to_string()];
    stop.parking = "none".to_string();
    with_drive(&drive, |d| {
        let mut post = EnforcementPost::new(at, KIND_FIXED_SCALE);
        post.anchor = stop.key();
        d.trip.posts = vec![post];
    });
    let mut state = RestStopState::with_drive(DriveRef::of(&drive), stop, false);
    app.clear_speech();
    activate(&mut state, &mut app.ctx, "Check in at inspection station");
    app.main_lines().join(" ")
}

#[test]
fn the_inspection_lane_at_severe_reads_the_lane_then_the_order() {
    let said = scale_check_in(85.0);
    assert!(
        index_of(&said, "Officers pull you into the inspection lane.") < index_of(&said, ORDER),
        "{said}"
    );
    assert!(
        index_of(&said, "Written up for") < index_of(&said, ORDER),
        "{said}"
    );
}

#[test]
fn the_inspection_lane_at_drowsy_has_no_order() {
    let said = scale_check_in(70.0);
    assert!(
        said.contains("Officers pull you into the inspection lane."),
        "{said}"
    );
    assert!(!said.contains(ORDER), "{said}");
}

// -- the rest call ---------------------------------------------------------------

fn call_rows(app: &mut TestApp, drive: &SharedState) -> Vec<String> {
    let mut call = DispatchCallState::new(DriveRef::of(drive));
    build_labels(&mut call, &mut app.ctx)
}

fn rest_call(app: &mut TestApp, drive: &SharedState) -> String {
    let mut call = DispatchCallState::new(DriveRef::of(drive));
    Menu::enter(&mut call, &mut app.ctx);
    app.clear_speech();
    activate(&mut call, &mut app.ctx, REST_ROW);
    let said = last(app);
    let rows = labels(&call, &app.ctx);
    assert!(!rows.iter().any(|r| r == REST_ROW), "{rows:?}");
    said
}

#[test]
fn the_rest_call_is_offered_drowsy_on_a_load_and_not_rested() {
    let mut app = TestApp::new();
    let drive = a_drive(&mut app);
    set_fatigue(&mut app, &drive, 20.0);
    assert!(!call_rows(&mut app, &drive).iter().any(|r| r == REST_ROW));
    set_fatigue(&mut app, &drive, 50.0);
    assert!(!call_rows(&mut app, &drive).iter().any(|r| r == REST_ROW));
    set_fatigue(&mut app, &drive, 65.0);
    assert_eq!(
        call_rows(&mut app, &drive).last().map(String::as_str),
        Some(REST_ROW)
    );
    set_fatigue(&mut app, &drive, 90.0);
    assert!(call_rows(&mut app, &drive).iter().any(|r| r == REST_ROW));
}

#[test]
fn the_rest_call_is_absent_once_used_on_this_load() {
    let mut app = TestApp::new();
    let drive = a_drive(&mut app);
    set_fatigue(&mut app, &drive, 70.0);
    rest_call(&mut app, &drive);
    assert!(!call_rows(&mut app, &drive).iter().any(|r| r == REST_ROW));
    assert_eq!(with_drive(&drive, |d| d.rest_call), RestCall::Used);
}

#[test]
fn the_rest_call_is_absent_on_a_team_load_whose_co_driver_has_hours() {
    let mut app = TestApp::new();
    let drive = a_drive(&mut app);
    set_fatigue(&mut app, &drive, 70.0);
    let offered = drive_and_ctx(&drive, &mut app, |d, ctx| {
        d.job.team = true;
        (d.handover_offered(ctx), d.rest_call_offered(ctx))
    });
    // A co-driver with hours takes the wheel instead of the rest call.
    assert_eq!(offered, (true, false), "handover, rest call");
    assert!(!call_rows(&mut app, &drive).iter().any(|r| r == REST_ROW));
}

#[test]
fn the_rest_call_moves_the_appointment_by_the_rest_and_the_drive_to_parking() {
    let mut app = TestApp::new();
    let drive = a_drive(&mut app);
    set_fatigue(&mut app, &drive, 70.0);
    let (deadline, travel) = drive_and_ctx(&drive, &mut app, |d, ctx| {
        let at = d.trip.position_mi;
        d.trip.stops = vec![sleep_stop(at + 20.0)];
        (d.job.deadline_game_h, d.nearest_sleep_stop_travel_min(ctx))
    });
    let travel = travel.expect("the rest area is ahead");
    assert!(travel > 0.0);
    let said = rest_call(&mut app, &drive);
    let moved = with_drive(&drive, |d| d.job.deadline_game_h);
    assert!(
        (moved - deadline - (REST_CALL_REST_H + travel / 60.0)).abs() < 1e-9,
        "{deadline} to {moved}, travel {travel}"
    );
    assert!(
        said.starts_with("Dispatch: Understood. Your appointment moves to "),
        "{said}"
    );
    assert!(
        said.ends_with("Park and sleep. This load pays base rate, with no on-time bonus."),
        "{said}"
    );
}

#[test]
fn with_no_stop_known_the_appointment_moves_ten_and_a_half_hours() {
    let mut app = TestApp::new();
    let drive = a_drive(&mut app);
    set_fatigue(&mut app, &drive, 70.0);
    let deadline = with_drive(&drive, |d| {
        d.trip.stops.clear();
        d.job.deadline_game_h
    });
    rest_call(&mut app, &drive);
    let moved = with_drive(&drive, |d| d.job.deadline_game_h);
    assert!((moved - deadline - REST_CALL_FALLBACK_H).abs() < 1e-9);
}

#[test]
fn the_third_rest_call_in_a_week_costs_trust_and_says_so() {
    let mut app = TestApp::new();
    let drive = a_drive(&mut app);
    set_fatigue(&mut app, &drive, 70.0);
    let now = drive_and_ctx(&drive, &mut app, |d, ctx| d.absolute_game_hour(ctx, None));
    {
        let p = app.ctx.profile.as_mut().expect("a career");
        p.achievement_stats.insert(
            STAT_REST_CALLS.to_string(),
            json!([now - 100.0, now - 30.0]),
        );
        p.career.reputation = 60.0;
    }
    let said = rest_call(&mut app, &drive);
    assert!(
        said.ends_with(
            "That is three rest calls this week. Plan your sleep before you take the load."
        ),
        "{said}"
    );
    let p = app.ctx.profile.as_ref().expect("a career");
    assert_eq!(p.career.reputation, 60.0 - DECLINE_REPUTATION_PENALTY);
    assert_eq!(
        p.achievement_stats
            .get(STAT_REST_CALLS)
            .and_then(Value::as_array)
            .map(Vec::len),
        Some(3)
    );
}

#[test]
fn a_rest_call_with_an_old_one_out_of_the_week_costs_nothing() {
    let mut app = TestApp::new();
    let drive = a_drive(&mut app);
    set_fatigue(&mut app, &drive, 70.0);
    let now = drive_and_ctx(&drive, &mut app, |d, ctx| d.absolute_game_hour(ctx, None));
    {
        let p = app.ctx.profile.as_mut().expect("a career");
        p.achievement_stats.insert(
            STAT_REST_CALLS.to_string(),
            json!([now - 200.0, now - 30.0]),
        );
        p.career.reputation = 60.0;
    }
    let said = rest_call(&mut app, &drive);
    assert!(!said.contains("rest calls this week"), "{said}");
    assert_eq!(
        app.ctx
            .profile
            .as_ref()
            .expect("a career")
            .career
            .reputation,
        60.0
    );
}

#[test]
fn the_spent_rest_call_and_the_moved_appointment_survive_a_save() {
    let mut app = TestApp::new();
    let drive = a_drive(&mut app);
    set_fatigue(&mut app, &drive, 70.0);
    rest_call(&mut app, &drive);
    let (snapshot, deadline) = drive_and_ctx(&drive, &mut app, |d, ctx| {
        (d.snapshot(ctx), d.job.deadline_game_h)
    });
    assert_eq!(snapshot.get("rest_call"), Some(&json!("used")));
    let resumed = DrivingState::from_snapshot(&mut app.ctx, &snapshot).expect("it resumes");
    assert_eq!(resumed.rest_call, RestCall::Used);
    assert!((resumed.job.deadline_game_h - deadline).abs() < 1e-9);
    assert_eq!(resumed.rest_call_state(), Some(RestCall::Used));
}

/// Settle a delivery on time, with or without a spent rest call: the
/// settlement's text.
fn settle(rest_call: RestCall) -> String {
    use freight_fate::playtest::harness::{PlaytestHarness, StartDelivery};
    use freight_fate::states::driving_menu_states::ArrivalState;
    let mut harness = PlaytestHarness::new();
    harness.start_delivery(StartDelivery::named("Base Rate"));
    harness.with_drive(move |d, _| {
        d.rest_call = rest_call;
        // Inside the appointment, wherever the rest call moved it.
        d.job.deadline_game_h = 500.0;
    });
    harness.settle_current_delivery();
    harness.with_state::<ArrivalState, _>(|s, _| {
        format!(
            "{} | {}",
            s.summary_parts.join(" "),
            s.summary_lines.join(" ")
        )
    })
}

#[test]
fn a_load_with_a_spent_rest_call_pays_base_with_no_bonus_and_no_penalty() {
    let with_bonus = settle(RestCall::Available);
    assert!(
        with_bonus.contains("On-time delivery bonus:"),
        "{with_bonus}"
    );
    let base = settle(RestCall::Used);
    assert!(!base.contains("On-time delivery bonus:"), "{base}");
    assert!(!base.contains("Early delivery bonus:"), "{base}");
    assert!(base.contains("on time"), "{base}");
    assert!(!base.contains(", late."), "{base}");
}
