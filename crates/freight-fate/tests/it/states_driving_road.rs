//! The small road mixins: `states/driving_lane_gap.rs`, `states/driving_wrong_way.rs`,
//! `states/driving_engine_brake.rs` and `states/driving_damage.rs`.
//!
//! Ported from `tests/test_lane_return_gap.py`,
//! `test_lane_discrete.py` (the lane-gap cases), `test_engine_brake_zones.py`
//! and `test_driving_damage_bands.py` -- everything in them a real
//! `DrivingState` answers without the per-frame loop or a menu state.
//!
//! Two Python seams have no Rust equivalent and are noted at each use:
//! `monkeypatch.setattr(d.trip, "grade_at", ...)` (there is no grade override
//! on `Trip`), and `monkeypatch.setattr(d, "_terse_speech", ...)` (the Rust
//! reading comes from the settings, so the settings are set instead).

use ff_core::achievements::int_stat;
use ff_core::models::business::{COMPANY_DRIVER, LEASED_OWNER_OPERATOR};
use ff_core::models::career::LEVEL_XP;
use ff_core::models::jobs::{Job, CARGO_CATALOG};
use ff_core::models::profile::Profile;
use ff_core::sim::traffic_manager::TrafficVehicle;
use ff_core::sim::trip_models::{RoadStop, Zone, FACILITY_GATE_ZONE_MI, URBAN_RADIUS_MI};
use ff_core::sim::vehicle::{
    DAMAGE_BAND_LAST_CALL, DAMAGE_BAND_LIMP, DAMAGE_BAND_NONE, DAMAGE_BAND_OUT_OF_SERVICE,
    DAMAGE_BAND_REDUCED, DAMAGE_LAST_CALL_PCT, DAMAGE_LIMP_CAP_MPH, DAMAGE_LIMP_PCT,
    DAMAGE_OUT_OF_SERVICE_PCT,
};

use freight_fate::app::testing::TestApp;
use freight_fate::states::driving::DrivingState;
use freight_fate::states::driving_core::*;
use freight_fate::states::driving_damage::{
    cargo_status_clause, damage_band_clause, damage_summary_line, preventable_damage_charge,
    CARGO_CUE_STEPS,
};
use freight_fate::states::driving_engine_brake::{
    JAKE_ZONE_FINES, JAKE_ZONE_GRACE_S, JAKE_ZONE_WARN_MI,
};
use freight_fate::states::driving_lane_gap::{
    LANE_GAP_ACT_REAL_S, LANE_GAP_CUE_MIN_GAP_S, LANE_GAP_MARGIN_MI,
};
use freight_fate::states::driving_wrong_way::{
    WRONG_WAY_REMIND_MI, WRONG_WAY_STOP_RADIUS_MI, WRONG_WAY_TRAFFIC_MI, WRONG_WAY_WARN_MI,
};

// -- rigging -------------------------------------------------------------------------

fn mph_to_mps(mph: f64) -> f64 {
    mph / 2.23694
}

/// `_driving(app)` in `test_engine_brake_zones.py`: Buffalo to Rochester, the
/// short two-city corridor every zone case runs on.
fn a_drive(app: &mut TestApp, name: &str) -> DrivingState {
    let world = app.ctx.world;
    let mut profile = Profile::named_in(name, "Buffalo");
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
    // The bubble is its own suite's business; an empty road keeps these
    // deterministic.
    drive.trip.set_npc_vehicles(Vec::new());
    drive
}

/// Every `traffic/` cue the audio backend was asked for, with its pan.
/// `_roll_with_jake(d, mile=...)`: the truck at `mile`, rolling at road speed
/// with the driver's own retarder on.
fn roll_with_jake(drive: &mut DrivingState, mile: f64) {
    drive.trip.position_mi = mile;
    drive.trip.truck.engine_on = true;
    drive.trip.truck.transmission.gear = 8;
    drive.trip.truck.throttle = 0.0;
    drive.trip.truck.velocity_mps = mph_to_mps(55.0);
    drive.trip.truck.set_engine_brake(true);
}

/// The Buffalo end of the route is inside the origin city's urban radius, so
/// mile 2 is inside a no-engine-brake zone. Asserted rather than assumed: a
/// world re-bake that moved the radius would otherwise silently gut this file.
fn a_zone_mile(drive: &DrivingState) -> f64 {
    let mile = 2.0;
    assert!(
        drive.trip.engine_brake_ban_at(mile).is_some(),
        "mile 2 of Buffalo to Rochester is inside the origin city's ban zone"
    );
    mile
}

/// Every line the drive SUBMITTED since `from`, read off the review log.
///
/// The Python suite monkeypatched `say_event` and so saw exactly what each
/// call site handed over. The Rust capture sees the channel instead, where an
/// interrupting line's purge can hand a cut line back to finish behind it --
/// real behaviour, but not what a "said once" assertion is about. The review
/// log records one entry per submission and no requeues.
fn logged_since(app: &TestApp, from: usize) -> Vec<String> {
    app.ctx.message_log.messages[from..]
        .iter()
        .map(|message| message.text.clone())
        .collect()
}

// -- driving_lane_gap.py --------------------------------------------------------------

/// `_npc(position_mi, lane, ...)` of `test_lane_return_gap.py`.
fn npc(
    position_mi: f64,
    lane: i64,
    speed_mph: f64,
    vehicle_class: &str,
    key: &str,
) -> TrafficVehicle {
    let key = if key.is_empty() {
        format!("npc:{lane}:{position_mi}")
    } else {
        key.to_string()
    };
    TrafficVehicle::new(
        &key,
        position_mi,
        speed_mph,
        speed_mph,
        -lane,
        "cruising",
        vehicle_class,
    )
    .with_lane(lane)
}

/// `_driving(app)` of `test_lane_return_gap.py`: a clean road, because every
/// vehicle and every closure in these cases is placed by the test.
fn a_gap_drive(app: &mut TestApp) -> DrivingState {
    let mut drive = a_drive(app, "Gaps");
    drive.trip.set_npc_vehicles(Vec::new());
    drive.trip.zones.clear();
    assert!(
        drive.lane.lane_count >= 2,
        "the passing cases need a second lane"
    );
    drive
}

/// `_rolling(driving, mph)`.
fn rolling(drive: &mut DrivingState, mph: f64) {
    drive.trip.truck.start_engine();
    drive.trip.truck.velocity_mps = mph / 2.2369362920544;
}

/// `_pass_a_box_truck`: move out around a box truck in the right lane and
/// pull level with it.
fn pass_a_box_truck(drive: &mut DrivingState, app: &mut TestApp) {
    drive.trip.traffic_manager.vehicles =
        vec![npc(drive.trip.position_mi + 0.1, 0, 45.0, "box truck", "")];
    drive.lane.lane = 1; // the change has landed
    for _ in 0..3 {
        drive.update_lane_gap(&mut app.ctx, 0.1);
    }
    assert!(app.event_lines().is_empty(), "still alongside it");
}

/// `_clear_the_box_truck`: the box truck is behind the drive tires.
fn clear_the_box_truck(drive: &mut DrivingState) {
    drive.trip.traffic_manager.vehicles =
        vec![npc(drive.trip.position_mi - 0.9, 0, 45.0, "box truck", "")];
}

/// `_openings(spoken)`.
fn openings(app: &TestApp) -> Vec<String> {
    app.event_lines()
        .into_iter()
        .filter(|line| line.contains("lane open"))
        .collect()
}

#[test]
fn test_the_lane_you_came_out_of_is_called_open_once_you_are_past() {
    let mut app = TestApp::new();
    let mut drive = a_gap_drive(&mut app);
    rolling(&mut drive, 60.0);
    app.clear_speech();
    pass_a_box_truck(&mut drive, &mut app);

    clear_the_box_truck(&mut drive);
    drive.update_lane_gap(&mut app.ctx, 0.1);

    assert_eq!(app.event_lines(), vec!["Right lane open.".to_string()]);
}

#[test]
fn test_the_cue_is_said_once_and_never_chants() {
    let mut app = TestApp::new();
    let mut drive = a_gap_drive(&mut app);
    rolling(&mut drive, 60.0);
    app.clear_speech();
    pass_a_box_truck(&mut drive, &mut app);
    clear_the_box_truck(&mut drive);

    for _ in 0..200 {
        // twenty seconds of open road behind the pass
        drive.update_lane_gap(&mut app.ctx, 0.1);
    }

    assert_eq!(openings(&app).len(), 1);
}

#[test]
fn test_nothing_is_said_while_the_vehicle_is_still_alongside() {
    let mut app = TestApp::new();
    let mut drive = a_gap_drive(&mut app);
    rolling(&mut drive, 60.0);
    app.clear_speech();
    drive.lane.lane = 1;
    drive.trip.traffic_manager.vehicles =
        vec![npc(drive.trip.position_mi + 0.05, 0, 45.0, "box truck", "")];

    for _ in 0..50 {
        drive.update_lane_gap(&mut app.ctx, 0.1);
    }

    assert!(openings(&app).is_empty());
}

#[test]
fn test_the_gap_closing_again_holds_the_cue() {
    // Somebody else fills the lane behind the pass: it is not open any more,
    // and the truck must not say it is.
    let mut app = TestApp::new();
    let mut drive = a_gap_drive(&mut app);
    rolling(&mut drive, 60.0);
    app.clear_speech();
    pass_a_box_truck(&mut drive, &mut app);
    clear_the_box_truck(&mut drive);
    drive.update_lane_gap(&mut app.ctx, 0.1);
    assert_eq!(openings(&app).len(), 1);

    // A semi comes up the right lane and sits beside the cab.
    drive.trip.traffic_manager.vehicles = vec![npc(
        drive.trip.position_mi + 0.05,
        0,
        45.0,
        "semi",
        "npc:semi",
    )];
    for _ in 0..100 {
        drive.update_lane_gap(&mut app.ctx, 0.1);
    }

    assert_eq!(openings(&app).len(), 1);
}

#[test]
fn test_moving_back_over_first_leaves_the_cue_unsaid() {
    // The driver who did not wait has already answered the question.
    let mut app = TestApp::new();
    let mut drive = a_gap_drive(&mut app);
    rolling(&mut drive, 60.0);
    app.clear_speech();
    pass_a_box_truck(&mut drive, &mut app);

    drive.lane.lane = 0; // back in the right lane, still beside the box truck
    drive.update_lane_gap(&mut app.ctx, 0.1);
    clear_the_box_truck(&mut drive);
    for _ in 0..50 {
        drive.update_lane_gap(&mut app.ctx, 0.1);
    }

    assert!(openings(&app).is_empty());
}

#[test]
fn test_an_empty_lane_change_says_nothing() {
    // Nobody was ever passed, so there is nothing to be clear of.
    let mut app = TestApp::new();
    let mut drive = a_gap_drive(&mut app);
    rolling(&mut drive, 60.0);
    app.clear_speech();
    drive.lane.lane = 1;
    for _ in 0..100 {
        drive.update_lane_gap(&mut app.ctx, 0.1);
    }

    assert!(openings(&app).is_empty());
}

#[test]
fn test_passing_on_the_right_calls_the_left_lane_open() {
    // The same manoeuvre the other way round: out of the left lane, and back.
    let mut app = TestApp::new();
    let mut drive = a_gap_drive(&mut app);
    rolling(&mut drive, 60.0);
    app.clear_speech();
    drive.lane.lane = 1;
    drive.update_lane_gap(&mut app.ctx, 0.1); // settled in the left lane
    drive.trip.traffic_manager.vehicles =
        vec![npc(drive.trip.position_mi + 0.1, 1, 45.0, "car", "")];
    drive.lane.lane = 0; // moved right around it
    for _ in 0..3 {
        drive.update_lane_gap(&mut app.ctx, 0.1);
    }
    assert!(openings(&app).is_empty());

    drive.trip.traffic_manager.vehicles =
        vec![npc(drive.trip.position_mi - 0.9, 1, 45.0, "box truck", "")];
    drive.update_lane_gap(&mut app.ctx, 0.1);

    assert_eq!(
        app.event_lines().last().map(String::as_str),
        Some("Left lane open.")
    );
}

#[test]
fn test_a_coned_off_lane_is_never_called_open() {
    let mut app = TestApp::new();
    let mut drive = a_gap_drive(&mut app);
    rolling(&mut drive, 55.0);
    drive.trip.position_mi = 6.0;
    app.clear_speech();
    pass_a_box_truck(&mut drive, &mut app);
    // The right lane the pass came out of is closed for roadwork.
    let mut zone = Zone::new(5.0, 9.0, 45.0, "construction");
    zone.closed_lane = Some(0);
    drive.trip.zones.push(zone);
    clear_the_box_truck(&mut drive);

    for _ in 0..50 {
        drive.update_lane_gap(&mut app.ctx, 0.1);
    }

    assert!(openings(&app).is_empty());
}

#[test]
fn test_terse_speech_keeps_the_lane_and_drops_the_vehicle() {
    let mut app = TestApp::new();
    app.ctx.settings.driving_speech = "quiet".to_string();
    let mut drive = a_gap_drive(&mut app);
    rolling(&mut drive, 60.0);
    app.clear_speech();
    let logged_before = app.ctx.message_log.messages.len();
    pass_a_box_truck(&mut drive, &mut app);
    clear_the_box_truck(&mut drive);
    drive.update_lane_gap(&mut app.ctx, 0.1);

    // Quiet keeps the short lane opening in both speech and review.
    let logged: Vec<String> = app.ctx.message_log.messages[logged_before..]
        .iter()
        .map(|message| message.text.clone())
        .filter(|text| text.contains("lane open"))
        .collect();
    assert_eq!(logged, vec!["Right lane open.".to_string()]);
    assert_eq!(openings(&app), vec!["Right lane open.".to_string()]);
}

#[test]
fn test_the_cue_is_the_more_cautious_of_the_two_readings() {
    // Whatever this call misses, the collision check misses too: "open" can
    // never be the more optimistic of the two answers.
    const { assert!(LANE_GAP_MARGIN_MI > 0.0) };
    let mut app = TestApp::new();
    let mut drive = a_gap_drive(&mut app);
    rolling(&mut drive, 60.0);
    // A vehicle just outside the collision window but inside the cue's is
    // still a blocker.
    drive.trip.traffic_manager.vehicles = vec![npc(
        drive.trip.position_mi + DODGE_CLEARANCE_AHEAD_MI + LANE_GAP_MARGIN_MI / 2.0,
        1,
        60.0,
        "car",
        "",
    )];
    assert!(!drive.lane_gap_open(1));
}

#[test]
fn test_traffic_holding_your_own_speed_still_opens() {
    let mut app = TestApp::new();
    let mut drive = a_gap_drive(&mut app);
    rolling(&mut drive, 60.0);
    // Well behind, holding the truck's own speed: it never closes back up.
    drive.trip.traffic_manager.vehicles =
        vec![npc(drive.trip.position_mi - 0.9, 1, 60.0, "car", "")];
    assert!(drive.lane_gap_open(1));
}

#[test]
fn test_a_faster_vehicle_coming_up_behind_holds_the_lane() {
    let mut app = TestApp::new();
    let mut drive = a_gap_drive(&mut app);
    rolling(&mut drive, 55.0);
    // Behind but closing hard: by the time the driver is across it is there.
    drive.trip.traffic_manager.vehicles =
        vec![npc(drive.trip.position_mi - 0.5, 1, 95.0, "car", "")];
    assert!(!drive.lane_gap_open(1));
}

#[test]
fn test_the_watch_survives_the_vehicle_leaving_the_bubble() {
    let mut app = TestApp::new();
    let mut drive = a_gap_drive(&mut app);
    rolling(&mut drive, 60.0);
    app.clear_speech();
    pass_a_box_truck(&mut drive, &mut app);

    // The manager retires the cell entirely rather than moving it behind.
    drive.trip.set_npc_vehicles(Vec::new());
    drive.update_lane_gap(&mut app.ctx, 0.1);

    assert_eq!(openings(&app).len(), 1);
}

#[test]
fn test_the_cue_waits_for_the_truck_to_be_rolling() {
    let mut app = TestApp::new();
    let mut drive = a_gap_drive(&mut app);
    rolling(&mut drive, 60.0);
    app.clear_speech();
    pass_a_box_truck(&mut drive, &mut app);
    clear_the_box_truck(&mut drive);
    drive.trip.truck.velocity_mps = (LANE_MIN_MPH - 1.0) / 2.2369362920544;

    drive.update_lane_gap(&mut app.ctx, 0.1);

    assert!(openings(&app).is_empty());
    assert_eq!(drive.lane_gap_watch, Some(0)); // still watching
}

#[test]
fn test_a_change_still_underway_holds_the_cue() {
    let mut app = TestApp::new();
    let mut drive = a_gap_drive(&mut app);
    rolling(&mut drive, 60.0);
    app.clear_speech();
    pass_a_box_truck(&mut drive, &mut app);
    clear_the_box_truck(&mut drive);
    drive.lane_change_target = Some(0); // a change is already underway

    drive.update_lane_gap(&mut app.ctx, 0.1);

    assert!(openings(&app).is_empty());
}

#[test]
fn test_a_lane_the_road_took_away_is_never_called_open() {
    let mut app = TestApp::new();
    let mut drive = a_gap_drive(&mut app);
    rolling(&mut drive, 60.0);
    app.clear_speech();
    pass_a_box_truck(&mut drive, &mut app);
    clear_the_box_truck(&mut drive);

    drive.lane.lane_count = 1; // the road narrowed under the truck
    drive.lane.lane = 0;
    drive.update_lane_gap(&mut app.ctx, 0.1);

    assert_eq!(drive.lane_gap_watch, None);
    assert!(openings(&app).is_empty());
}

#[test]
fn test_the_cue_spacing_is_real_seconds() {
    // A queue of vehicles clearing one after another is a real sequence of
    // facts, but read out back to back it is a chant.
    const { assert!(LANE_GAP_CUE_MIN_GAP_S > 0.0) };
    const { assert!(LANE_GAP_ACT_REAL_S > LANE_GAP_CUE_MIN_GAP_S) };
}

// -- the L key readout ----------------------------------------------------------------

#[test]
fn test_the_lane_readout_says_whether_the_next_lane_over_is_open() {
    let mut app = TestApp::new();
    let mut drive = a_gap_drive(&mut app);
    rolling(&mut drive, 60.0);
    drive.lane.lane = 0;

    assert!(drive.lane_status_text().contains("Left lane open."));

    drive.trip.traffic_manager.vehicles =
        vec![npc(drive.trip.position_mi + 0.05, 1, 60.0, "car", "")];
    assert!(drive
        .lane_status_text()
        .contains("Left lane blocked by a car."));
}

#[test]
fn test_the_lane_readout_reports_a_coned_off_lane_as_closed() {
    let mut app = TestApp::new();
    let mut drive = a_gap_drive(&mut app);
    rolling(&mut drive, 60.0);
    drive.trip.position_mi = 6.0;
    drive.lane.lane = 0;
    let mut zone = Zone::new(5.0, 9.0, 45.0, "construction");
    zone.closed_lane = Some(1);
    drive.trip.zones.push(zone);

    assert!(drive
        .lane_status_text()
        .contains("Left lane closed for construction."));
}

#[test]
fn test_a_one_lane_road_has_no_neighbour_to_report() {
    let mut app = TestApp::new();
    let mut drive = a_gap_drive(&mut app);
    rolling(&mut drive, 60.0);
    drive.lane.lane_count = 1;
    drive.lane.lane = 0;

    let text = drive.lane_status_text();
    assert!(!text.contains("lane open."));
    assert!(!text.contains("blocked by"));
}

#[test]
fn test_the_readout_and_the_cue_read_the_same_traffic() {
    let mut app = TestApp::new();
    let mut drive = a_gap_drive(&mut app);
    rolling(&mut drive, 60.0);
    drive.lane.lane = 0;
    drive.trip.traffic_manager.vehicles =
        vec![npc(drive.trip.position_mi + 0.05, 1, 60.0, "car", "")];

    assert_eq!(
        drive.lane_gap_open(1),
        !drive.lane_status_text().contains("blocked by")
    );
}

// -- driving_wrong_way.py -------------------------------------------------------------

/// Back the truck up `moved` miles of route, once.
fn back_up(drive: &mut DrivingState, app: &mut TestApp, moved: f64) {
    drive.trip.truck.engine_on = true;
    drive.trip.truck.transmission.gear = REVERSE;
    drive.trip.truck.velocity_mps = mph_to_mps(5.0);
    drive.trip.last_moved_mi = -moved;
    drive.update_wrong_way(&mut app.ctx, 1.0 / 60.0);
}

#[test]
fn test_backing_in_the_yard_is_never_scolded() {
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, "Backer");
    drive.trip.position_mi = WRONG_WAY_STOP_RADIUS_MI / 2.0;
    app.clear_speech();

    back_up(&mut drive, &mut app, 0.05);

    assert!(app.event_lines().is_empty());
    assert_eq!(drive.wrong_way_mi, 0.0);
}

#[test]
fn test_backing_on_a_live_lane_reminds_first() {
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, "Backer");
    drive.trip.position_mi = drive.trip.total_miles() / 2.0;
    drive.trip.stops.clear();
    app.clear_speech();

    back_up(&mut drive, &mut app, WRONG_WAY_REMIND_MI);

    let spoken = app.event_lines().join(" ");
    assert!(
        spoken.contains("You are still in reverse, backing away from your destination."),
        "{spoken}"
    );
}

#[test]
fn test_a_tenth_of_a_mile_back_names_it_as_illegal() {
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, "Backer");
    drive.trip.position_mi = drive.trip.total_miles() / 2.0;
    drive.trip.stops.clear();
    back_up(&mut drive, &mut app, WRONG_WAY_REMIND_MI);
    app.clear_speech();

    back_up(&mut drive, &mut app, WRONG_WAY_WARN_MI);

    let spoken = app.event_lines().join(" ");
    assert!(
        spoken.contains("You are driving the wrong way."),
        "{spoken}"
    );
    assert!(
        spoken.contains("Backing on a traveled lane is illegal"),
        "{spoken}"
    );
}

#[test]
fn test_a_quarter_mile_back_meets_traffic() {
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, "Backer");
    drive.trip.position_mi = drive.trip.total_miles() / 2.0;
    drive.trip.stops.clear();
    let damage_before = drive.trip.truck.damage_pct;
    app.clear_speech();

    back_up(&mut drive, &mut app, WRONG_WAY_TRAFFIC_MI);

    let spoken = app.event_lines().join(" ");
    assert!(spoken.contains("You are backing into traffic."), "{spoken}");
    assert!(spoken.contains("Something hit the trailer."), "{spoken}");
    assert!(drive.trip.truck.damage_pct > damage_before);
}

#[test]
fn test_a_forward_gear_resets_the_watch() {
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, "Backer");
    drive.trip.position_mi = drive.trip.total_miles() / 2.0;
    drive.trip.stops.clear();
    back_up(&mut drive, &mut app, WRONG_WAY_WARN_MI);
    assert!(drive.wrong_way_mi > 0.0);

    drive.trip.truck.transmission.gear = 8;
    drive.update_wrong_way(&mut app.ctx, 1.0 / 60.0);

    assert_eq!(drive.wrong_way_mi, 0.0);
    assert_eq!(drive.wrong_way_said_at, 0.0);
}

// -- driving_engine_brake.py ----------------------------------------------------------

#[test]
fn test_zone_violation_warns_before_any_fine() {
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, "Jake");
    let mile = a_zone_mile(&drive);
    let money = profile_of(&app.ctx).money();
    roll_with_jake(&mut drive, mile);
    app.clear_speech();

    drive.update_engine_brake_zone(&mut app.ctx, 0.1);

    assert_eq!(drive.jake_zone_fines, 0);
    assert_eq!(profile_of(&app.ctx).money(), money);
    let spoken = app.event_lines().join(" ");
    assert!(
        spoken.contains("No engine brake zone. Switch it off."),
        "{spoken}"
    );
}

#[test]
fn test_keeping_the_jake_on_past_the_grace_draws_a_fine() {
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, "Jake");
    app.seasoned_driver();
    let mile = a_zone_mile(&drive);
    let money = profile_of(&app.ctx).money();
    roll_with_jake(&mut drive, mile);
    app.clear_speech();

    drive.update_engine_brake_zone(&mut app.ctx, 0.1); // warning
    drive.update_engine_brake_zone(&mut app.ctx, JAKE_ZONE_GRACE_S + 1.0); // grace expires

    assert_eq!(drive.jake_zone_fines, 1);
    assert_eq!(profile_of(&app.ctx).money(), money - JAKE_ZONE_FINES[0]);
    let spoken = app.event_lines().join(" ");
    assert!(spoken.contains("150 dollar"), "{spoken}");
    assert!(spoken.contains("Engine brake citation"), "{spoken}");
}

#[test]
fn test_switching_off_within_the_grace_avoids_the_fine() {
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, "Jake");
    let mile = a_zone_mile(&drive);
    roll_with_jake(&mut drive, mile);

    drive.update_engine_brake_zone(&mut app.ctx, 0.1); // warning
    drive.trip.truck.set_engine_brake(false); // driver complies
    drive.update_engine_brake_zone(&mut app.ctx, JAKE_ZONE_GRACE_S + 5.0);

    assert_eq!(drive.jake_zone_fines, 0);
}

#[test]
fn test_open_road_jake_use_stays_free() {
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, "Jake");
    let open_road = drive.trip.total_miles() / 2.0;
    assert!(drive.trip.engine_brake_ban_at(open_road).is_none());
    roll_with_jake(&mut drive, open_road);
    app.clear_speech();

    drive.update_engine_brake_zone(&mut app.ctx, JAKE_ZONE_GRACE_S + 5.0);

    assert_eq!(drive.jake_zone_fines, 0);
    assert!(app.event_lines().is_empty());
}

#[test]
fn test_hazard_emergency_is_exempt() {
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, "Jake");
    let mile = a_zone_mile(&drive);
    roll_with_jake(&mut drive, mile);
    drive.hazard_deadline = Some(6.0); // braking for a live hazard warning
    app.clear_speech();

    drive.update_engine_brake_zone(&mut app.ctx, 0.1);
    drive.update_engine_brake_zone(&mut app.ctx, JAKE_ZONE_GRACE_S + 5.0);

    assert_eq!(drive.jake_zone_fines, 0);
    assert!(app.event_lines().is_empty());
}

#[test]
fn test_the_emergency_brake_is_exempt_too() {
    // `_jake_zone_exempt`'s other carve-out, which the Python suite reached
    // through the hazard deadline alone.
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, "Jake");
    let mile = a_zone_mile(&drive);
    roll_with_jake(&mut drive, mile);
    drive.trip.truck.emergency_brake = true;

    assert!(drive.jake_zone_exempt());
    assert!(drive.assist_jake_allowed(&mut app.ctx));
}

#[test]
fn test_cruise_jake_releases_entering_a_zone_with_a_spoken_reason() {
    // A real driver flips engine-brake mode off coming into town; cruise now
    // does the same, telling the player once why the retarder note stopped.
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, "Jake");
    let mile = a_zone_mile(&drive);
    roll_with_jake(&mut drive, mile);
    drive.cruise_jake_stage = 3; // automation raised it, not the driver's stalk
    app.clear_speech();

    drive.update_engine_brake_zone(&mut app.ctx, 0.1);

    assert_eq!(drive.trip.truck.engine_brake_stage, 0);
    assert_eq!(drive.cruise_jake_stage, 0);
    let spoken = app.event_lines().join(" ");
    assert!(
        spoken.contains("No engine brake zone, Buffalo."),
        "{spoken}"
    );
    drive.update_engine_brake_zone(&mut app.ctx, JAKE_ZONE_GRACE_S + 5.0);
    assert_eq!(drive.jake_zone_fines, 0);
}

#[test]
fn test_curve_assist_jake_releases_in_zone_with_its_own_reason() {
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, "Jake");
    let mile = a_zone_mile(&drive);
    roll_with_jake(&mut drive, mile);
    drive.curve_assist_jake = true; // the assist engaged it for a bend
    app.clear_speech();

    drive.update_engine_brake_zone(&mut app.ctx, 0.1);

    assert_eq!(drive.trip.truck.engine_brake_stage, 0);
    assert!(!drive.curve_assist_jake);
    let spoken = app.event_lines().join(" ");
    assert!(
        spoken.contains("No engine brake zone, Buffalo."),
        "{spoken}"
    );
    drive.update_engine_brake_zone(&mut app.ctx, JAKE_ZONE_GRACE_S + 5.0);
    assert_eq!(drive.jake_zone_fines, 0);
}

#[test]
fn test_assist_zone_cue_speaks_once_per_zone_and_never_in_terse() {
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, "Jake");
    let mile = a_zone_mile(&drive);
    roll_with_jake(&mut drive, mile);
    drive.cruise_jake_stage = 3;
    app.clear_speech();

    drive.update_engine_brake_zone(&mut app.ctx, 0.1);
    assert_eq!(app.event_lines().len(), 1);
    // Cruise tries the retarder again further into the same zone.
    drive.trip.truck.set_engine_brake(true);
    drive.cruise_jake_stage = 2;
    drive.update_engine_brake_zone(&mut app.ctx, 0.1);
    assert_eq!(drive.trip.truck.engine_brake_stage, 0);
    assert_eq!(app.event_lines().len(), 1); // released again, said nothing new

    // Python monkeypatched `_terse_speech`; the Rust reading comes from the
    // settings, so the settings say it instead.
    app.ctx.settings.driving_speech = "urgent_only".to_string();
    let mut terse = a_drive(&mut app, "Terse");
    roll_with_jake(&mut terse, mile);
    terse.cruise_jake_stage = 3;
    app.clear_speech();
    terse.update_engine_brake_zone(&mut app.ctx, 0.1);
    assert_eq!(terse.trip.truck.engine_brake_stage, 0); // still released
    assert!(app.event_lines().is_empty()); // advisory-class: terse stays quiet
}

#[test]
fn test_assists_may_not_raise_the_jake_inside_a_zone() {
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, "Jake");
    let mile = a_zone_mile(&drive);
    roll_with_jake(&mut drive, mile);
    drive.trip.truck.set_engine_brake(false);

    assert!(!drive.assist_jake_allowed(&mut app.ctx)); // flat ground in town

    drive.trip.position_mi = drive.trip.total_miles() / 2.0;
    assert!(drive.assist_jake_allowed(&mut app.ctx)); // open road
}

#[test]
fn test_one_citation_per_continuous_engagement() {
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, "Jake");
    app.seasoned_driver();
    let mile = a_zone_mile(&drive);
    roll_with_jake(&mut drive, mile);

    drive.update_engine_brake_zone(&mut app.ctx, 0.1);
    drive.update_engine_brake_zone(&mut app.ctx, JAKE_ZONE_GRACE_S + 1.0);
    assert_eq!(drive.jake_zone_fines, 1);
    // Still on, still in the zone: the citation is written, not repeated.
    for _ in 0..10 {
        drive.update_engine_brake_zone(&mut app.ctx, JAKE_ZONE_GRACE_S + 1.0);
    }
    assert_eq!(drive.jake_zone_fines, 1);
}

#[test]
fn test_fines_escalate_and_cap() {
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, "Jake");
    app.seasoned_driver();
    let mile = a_zone_mile(&drive);
    let money = profile_of(&app.ctx).money();
    roll_with_jake(&mut drive, mile);

    for _ in 0..4 {
        drive.trip.truck.set_engine_brake(true);
        drive.update_engine_brake_zone(&mut app.ctx, 0.1); // fresh warning
        drive.update_engine_brake_zone(&mut app.ctx, JAKE_ZONE_GRACE_S + 1.0); // fine
        drive.trip.truck.set_engine_brake(false);
        drive.update_engine_brake_zone(&mut app.ctx, 0.1); // engagement ends
    }

    assert_eq!(drive.jake_zone_fines, 4);
    let expected: f64 = JAKE_ZONE_FINES.iter().sum::<f64>() + JAKE_ZONE_FINES[2];
    assert_eq!(profile_of(&app.ctx).money(), money - expected);
    assert_eq!(drive.jake_fines_paid, expected);
}

#[test]
fn test_approach_callout_when_the_jake_is_on() {
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, "Jake");
    // Just short of the Rochester zone, retarder on from the open road.
    let start_mi = drive.trip.total_miles() - URBAN_RADIUS_MI;
    roll_with_jake(&mut drive, start_mi - JAKE_ZONE_WARN_MI / 2.0);
    app.clear_speech();

    drive.update_engine_brake_zone(&mut app.ctx, 0.1);

    let lines = app.event_lines();
    assert_eq!(lines.len(), 1);
    assert!(lines[0].contains("No engine brake zone"), "{:?}", lines[0]);
    assert!(lines[0].contains("Rochester"), "{:?}", lines[0]);
    drive.update_engine_brake_zone(&mut app.ctx, 0.1); // said once, not every frame
    assert_eq!(app.event_lines().len(), 1);
}

#[test]
fn test_no_approach_callout_with_the_jake_off() {
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, "Jake");
    let start_mi = drive.trip.total_miles() - URBAN_RADIUS_MI;
    roll_with_jake(&mut drive, start_mi - JAKE_ZONE_WARN_MI / 2.0);
    drive.trip.truck.set_engine_brake(false);
    app.clear_speech();

    drive.update_engine_brake_zone(&mut app.ctx, 0.1);

    assert!(app.event_lines().is_empty());
}

#[test]
fn test_terse_speech_still_hears_the_violation_warning() {
    let mut app = TestApp::new();
    app.ctx.settings.driving_speech = "quiet".to_string();
    let mut drive = a_drive(&mut app, "Jake");
    let mile = a_zone_mile(&drive);
    roll_with_jake(&mut drive, mile);
    app.clear_speech();

    drive.update_engine_brake_zone(&mut app.ctx, 0.1);

    let lines = app.event_lines();
    assert!(
        !lines.is_empty(),
        "terse mode must still hear the warning that gates the fine"
    );
    assert!(
        lines
            .last()
            .expect("a line")
            .contains("No engine brake zone"),
        "{lines:?}"
    );
}

#[test]
fn test_snapshot_round_trips_the_citations() {
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, "Jake");
    drive.jake_zone_fines = 2;
    drive.jake_fines_paid = 450.0;

    let data = drive.snapshot(&app.ctx);
    let resumed = DrivingState::from_snapshot(&mut app.ctx, &data).expect("the snapshot resumes");

    assert_eq!(resumed.jake_zone_fines, 2);
    assert_eq!(resumed.jake_fines_paid, 450.0);
}

#[test]
#[ignore = "needs a Trip grade seam: Python monkeypatched trip.grade_at"]
fn test_descent_grade_is_exempt() {
    // `_roll_with_jake(d, mile=2.0, grade=-0.04)` then two frames past the
    // grace: no fine and nothing spoken. `Trip::grade_at` reads the baked
    // elevation profile and has no override, so the -4 percent town grade
    // this needs cannot be staged. `on_downgrade` itself is covered below.
}

#[test]
fn test_a_real_downgrade_is_what_on_downgrade_answers() {
    // The half of `test_descent_grade_is_exempt` and
    // `test_cruise_keeps_its_jake_in_zone_on_a_real_downgrade` that a real
    // corridor can still answer: find a mile the bake really does call steep
    // enough, and check the exemption reads it.
    let mut app = TestApp::new();
    let world = app.ctx.world;
    let mut profile = Profile::named_in("Grades", "Denver");
    profile.tutorial_done = true;
    app.ctx.profile = Some(profile);
    let route = world
        .supported_route("Denver", "Grand Junction", None)
        .expect("the world routes")
        .expect("Denver to Grand Junction is supported");
    let job = Job::new(
        CARGO_CATALOG
            .get("general")
            .expect("the general cargo type"),
        12.0,
        "Denver",
        "yard",
        "Grand Junction",
        route.miles(),
        1000.0,
        24.0,
    );
    let mut drive = DrivingState::new(
        &mut app.ctx,
        job,
        route,
        Some(0),
        DRIVE_PHASE_DELIVERY,
        Some(12.0),
    );
    let total = drive.trip.total_miles();
    let mut steep = None;
    let mut mile = 0.0;
    while mile < total {
        if drive.trip.grade_at(mile) * 100.0 <= -2.0 {
            steep = Some(mile);
            break;
        }
        mile += 0.25;
    }
    let steep = steep.expect("I-70 west of Denver carries a real downgrade");
    drive.trip.position_mi = steep;
    assert!(drive.on_downgrade());
    assert!(drive.jake_zone_exempt());
    // And a level stretch is not a grade.
    let mut level = None;
    let mut mile = 0.0;
    while mile < total {
        if drive.trip.grade_at(mile).abs() * 100.0 < 0.5 {
            level = Some(mile);
            break;
        }
        mile += 0.25;
    }
    if let Some(level) = level {
        drive.trip.position_mi = level;
        assert!(!drive.on_downgrade());
    }
}

// -- driving_damage.py ----------------------------------------------------------------

#[test]
fn test_the_band_clause_never_leaves_a_number_alone() {
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, "Wrench");

    drive.trip.truck.damage_pct = 10.0;
    assert_eq!(damage_band_clause(&app.ctx.settings, &drive.trip.truck), "");

    drive.trip.truck.damage_pct = DAMAGE_LIMP_PCT + 1.0;
    let clause = damage_band_clause(&app.ctx.settings, &drive.trip.truck);
    assert!(clause.starts_with("limp mode, capped at"), "{clause}");

    drive.trip.truck.damage_pct = DAMAGE_OUT_OF_SERVICE_PCT;
    assert_eq!(
        damage_band_clause(&app.ctx.settings, &drive.trip.truck),
        "out of service"
    );
}

#[test]
fn test_the_delivery_summary_carries_the_band_with_the_number() {
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, "Wrench");

    assert!(damage_summary_line(&app.ctx.settings, &drive.trip.truck, 0.5).is_none());

    drive.trip.truck.damage_pct = 5.0;
    let clean = damage_summary_line(&app.ctx.settings, &drive.trip.truck, 4.0)
        .expect("a run that added damage says so");
    assert!(clean.contains("Visit the garage when you can."), "{clean}");

    drive.trip.truck.damage_pct = DAMAGE_LIMP_PCT + 1.0;
    let banded = damage_summary_line(&app.ctx.settings, &drive.trip.truck, 40.0)
        .expect("a run that added damage says so");
    assert!(banded.contains("limp mode"), "{banded}");
    assert!(
        banded.contains("Repair it before the next run."),
        "{banded}"
    );
}

#[test]
fn test_crossing_a_band_announces_it_and_opens_the_cap_at_road_speed() {
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, "Wrench");
    drive.trip.truck.velocity_mps = mph_to_mps(62.0);
    app.clear_speech();

    drive.trip.truck.damage_pct = DAMAGE_LIMP_PCT + 1.0;
    drive.update_damage_bands(&mut app.ctx, 1.0 / 60.0);

    assert_eq!(drive.damage_band, DAMAGE_BAND_LIMP);
    assert_eq!(drive.worst_damage_band, DAMAGE_BAND_LIMP);
    let spoken = app.event_lines().join(" ");
    assert!(spoken.contains("Limp mode."), "{spoken}");
    // Opened at the speed the truck already had, never below the target.
    let cap = drive.limp_cap_mph.expect("a cap is winding in");
    assert!(cap > DAMAGE_LIMP_CAP_MPH);
    assert_eq!(drive.trip.truck.speed_cap_mph, Some(cap));
}

#[test]
fn test_the_cap_winds_down_and_never_below_the_band() {
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, "Wrench");
    drive.trip.truck.velocity_mps = mph_to_mps(62.0);
    drive.trip.truck.damage_pct = DAMAGE_LIMP_PCT + 1.0;
    drive.update_damage_bands(&mut app.ctx, 1.0 / 60.0);
    let opened = drive.limp_cap_mph.expect("a cap is winding in");

    for _ in 0..600 {
        drive.update_damage_cap(1.0 / 60.0);
    }

    let settled = drive.limp_cap_mph.expect("still capped");
    assert!(settled < opened);
    assert_eq!(settled, DAMAGE_LIMP_CAP_MPH);
}

#[test]
fn test_the_last_call_names_the_number_that_stops_the_truck() {
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, "Wrench");
    drive.damage_band = DAMAGE_BAND_LIMP;
    drive.trip.truck.damage_pct = DAMAGE_LAST_CALL_PCT + 1.0;
    app.clear_speech();

    drive.update_damage_bands(&mut app.ctx, 1.0 / 60.0);

    assert_eq!(drive.damage_band, DAMAGE_BAND_LAST_CALL);
    let spoken = app.event_lines().join(" ");
    assert!(
        spoken.contains(&format!(
            "{DAMAGE_OUT_OF_SERVICE_PCT:.0} percent the truck goes out of"
        )),
        "{spoken}"
    );
}

#[test]
fn test_a_repair_speaks_the_downward_edge_too() {
    // Without it a player cannot tell whether a repair cleared limp mode.
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, "Wrench");
    drive.damage_band = DAMAGE_BAND_LIMP;
    drive.trip.truck.damage_pct = 5.0;
    app.clear_speech();

    drive.update_damage_bands(&mut app.ctx, 1.0 / 60.0);

    assert_eq!(drive.damage_band, DAMAGE_BAND_NONE);
    let spoken = app.event_lines().join(" ");
    assert!(spoken.contains("full power restored"), "{spoken}");
    assert!(drive.limp_cap_mph.is_none());
    assert!(drive.trip.truck.speed_cap_mph.is_none());
}

#[test]
fn test_the_limp_cap_stands_down_for_a_pull_over() {
    // A stop that is already braking owns the speed.
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, "Wrench");
    drive.trip.truck.damage_pct = DAMAGE_LIMP_PCT + 1.0;
    drive.update_damage_cap(1.0 / 60.0);
    assert!(drive.limp_cap_mph.is_some());

    drive.pull_over = Some(PULL_OVER_LIGHTS.to_string());
    drive.update_damage_cap(1.0 / 60.0);

    assert!(drive.limp_cap_suspended());
    assert!(drive.limp_cap_mph.is_none());
    assert!(drive.trip.truck.speed_cap_mph.is_none());
}

#[test]
fn test_cruise_owns_up_to_a_limp_cap_once_per_engagement() {
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, "Wrench");
    drive.trip.truck.damage_pct = DAMAGE_LIMP_PCT + 1.0;
    drive.trip.truck.speed_cap_mph = Some(DAMAGE_LIMP_CAP_MPH);
    drive.trip.truck.velocity_mps = mph_to_mps(DAMAGE_LIMP_CAP_MPH);
    drive.cruise_mph = Some(DAMAGE_LIMP_CAP_MPH + 15.0);
    app.clear_speech();

    drive.announce_limp_cruise_cap(&mut app.ctx);
    let spoken = app.event_lines().join(" ");
    assert!(spoken.contains("Limp mode. Holding"), "{spoken}");
    assert!(drive.limp_cruise_said);

    app.clear_speech();
    drive.announce_limp_cruise_cap(&mut app.ctx);
    assert!(app.event_lines().is_empty());
}

#[test]
fn test_cruise_says_nothing_when_the_set_speed_is_already_under_the_cap() {
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, "Wrench");
    drive.trip.truck.speed_cap_mph = Some(DAMAGE_LIMP_CAP_MPH);
    drive.trip.truck.velocity_mps = mph_to_mps(DAMAGE_LIMP_CAP_MPH);
    drive.cruise_mph = Some(DAMAGE_LIMP_CAP_MPH);
    app.clear_speech();

    drive.announce_limp_cruise_cap(&mut app.ctx);

    assert!(app.event_lines().is_empty());
    assert!(!drive.limp_cruise_said);
}

#[test]
fn test_the_cargo_rungs_each_warn_once_and_the_highest_wins() {
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, "Wrench");
    app.clear_speech();

    // A collision can put a load through every rung at once; the driver needs
    // the state they are actually in, said once.
    drive.trip.truck.cargo_damage_pct = 70.0;
    drive.update_cargo_condition(&mut app.ctx, 1.0 / 60.0);

    let lines = app.event_lines();
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert!(lines[0].contains("The dock will refuse"), "{:?}", lines[0]);
    assert!(
        lines[0].contains("Brake and corner gently from here."),
        "{:?}",
        lines[0]
    );
    assert!(drive.cargo_coaching_said);

    // Nothing has moved: nothing more is said.
    app.clear_speech();
    drive.update_cargo_condition(&mut app.ctx, 1.0 / 60.0);
    assert!(app.event_lines().is_empty());
}

#[test]
fn test_the_coaching_tail_only_rides_the_first_report() {
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, "Wrench");
    app.clear_speech();

    drive.trip.truck.cargo_damage_pct = 15.0; // exception
    drive.update_cargo_condition(&mut app.ctx, 1.0 / 60.0);
    let first = app.event_lines().join(" ");
    assert!(
        first.contains("Brake and corner gently from here."),
        "{first}"
    );

    app.clear_speech();
    drive.trip.truck.cargo_damage_pct = 70.0; // straight to refused
    drive.update_cargo_condition(&mut app.ctx, 1.0 / 60.0);
    // The escalation itself, picked out of the channel: the interrupting
    // report cut the first one mid-sentence, and the pacer hands a cut line
    // back to finish rather than dropping it -- so the exception line (tail
    // and all) is legitimately in the channel again behind this one.
    let lines = app.event_lines();
    let escalation = lines
        .iter()
        .find(|line| line.contains("The dock will refuse"))
        .unwrap_or_else(|| panic!("the refused-load report speaks: {lines:?}"));
    assert!(
        !escalation.contains("Brake and corner gently"),
        "{escalation}"
    );
}

#[test]
fn test_a_tank_load_gets_the_tank_vocabulary() {
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, "Wrench");
    assert_eq!(cargo_status_clause(&drive.trip.truck), "secure");
    drive.trip.truck.cargo_damage_pct = 20.0;
    let clause = cargo_status_clause(&drive.trip.truck);
    assert!(clause.ends_with("20 percent"), "{clause}");
}

#[test]
fn test_the_preventable_bill_scales_with_the_deepest_band_reached() {
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, "Wrench");

    // Nothing preventable, nothing billed.
    assert_eq!(preventable_damage_charge(&drive).0, 0.0);

    drive.trip.truck.preventable_damage_pct = 30.0;
    drive.worst_damage_band = DAMAGE_BAND_REDUCED;
    let (shallow, shallow_rep, reason) = preventable_damage_charge(&drive);
    assert!(shallow > 0.0);
    assert_eq!(reason, "the truck came back in reduced power");

    // Patching the truck on the shoulder does not launder the run.
    drive.worst_damage_band = DAMAGE_BAND_OUT_OF_SERVICE;
    let (deep, deep_rep, reason) = preventable_damage_charge(&drive);
    assert!(deep > shallow);
    assert!(deep_rep > shallow_rep);
    assert_eq!(reason, "the truck went out of service on the road");
}

#[test]
fn test_the_wall_says_what_it_costs_before_it_charges_it() {
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, "Wrench");
    drive.trip.truck.damage_pct = DAMAGE_OUT_OF_SERVICE_PCT;

    // An owner-operator is told the bill and the hours.
    app.ctx.profile.as_mut().expect("a profile").business_status =
        LEASED_OWNER_OPERATOR.to_string();
    let owner = drive.out_of_service_message(&app.ctx);
    assert!(owner.contains("The repair will cost about"), "{owner}");

    // A company driver is told who pays and how long they wait.
    app.ctx.profile.as_mut().expect("a profile").business_status = COMPANY_DRIVER.to_string();
    let company = drive.out_of_service_message(&app.ctx);
    assert!(company.contains("The carrier covers the bill"), "{company}");
    assert!(company.contains("grounds the tractor"), "{company}");
}

#[test]
fn test_an_owner_operator_pays_the_roadside_bill_out_of_pocket() {
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, "Wrench");
    app.ctx.profile.as_mut().expect("a profile").business_status =
        LEASED_OWNER_OPERATOR.to_string();
    drive.trip.truck.damage_pct = DAMAGE_OUT_OF_SERVICE_PCT + 5.0;
    let money = profile_of(&app.ctx).money();
    let cost = drive.roadside_repair_cost();
    app.clear_speech();

    drive.recover_out_of_service(&mut app.ctx);

    assert!(!drive.trip.truck.out_of_service());
    assert_eq!(profile_of(&app.ctx).money(), money - cost);
    assert!(drive.limp_cap_mph.is_none());
    // The recovery line IS the announcement for the band it lands in.
    assert_eq!(drive.damage_band, drive.trip.truck.damage_band());
    let spoken = app.event_lines().join(" ");
    assert!(spoken.contains("Roadside repair"), "{spoken}");
}

#[test]
fn test_a_company_driver_is_grounded_and_pays_nothing() {
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, "Wrench");
    app.ctx.profile.as_mut().expect("a profile").business_status = COMPANY_DRIVER.to_string();
    drive.trip.truck.damage_pct = DAMAGE_OUT_OF_SERVICE_PCT + 5.0;
    let money = profile_of(&app.ctx).money();
    let reputation = profile_of(&app.ctx).career.reputation;
    app.clear_speech();

    drive.recover_out_of_service(&mut app.ctx);

    assert_eq!(profile_of(&app.ctx).money(), money);
    assert!(profile_of(&app.ctx).career.reputation < reputation);
    let spoken = app.event_lines().join(" ");
    assert!(
        spoken.contains("Dispatch logged preventable equipment damage"),
        "{spoken}"
    );
}

#[test]
fn test_the_out_of_service_creep_calls_road_service_at_a_stop() {
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, "Wrench");
    app.ctx.profile.as_mut().expect("a profile").business_status =
        LEASED_OWNER_OPERATOR.to_string();
    drive.trip.truck.damage_pct = DAMAGE_OUT_OF_SERVICE_PCT + 5.0;
    drive.trip.truck.velocity_mps = 0.0; // pulled over on the shoulder

    drive.update_damage_bands(&mut app.ctx, 1.0 / 60.0);

    // Coming to a stop ends the wait at once.
    assert!(!drive.trip.truck.out_of_service());
    assert_eq!(drive.out_of_service_creep_s, 0.0);
}

#[test]
fn test_a_stop_that_belongs_to_a_route_point_is_a_legitimate_reverse() {
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, "Backer");
    let mile = drive.trip.total_miles() / 2.0;
    drive.trip.position_mi = mile;
    drive.trip.stops = vec![RoadStop::new("Plaza", mile + 0.1, "travel_center")];

    assert!(drive.reverse_is_legitimate());
}

// -- driving_damage.py: the drive built the way test_driving_damage_bands.py
// builds it (Denver to Salt Lake City, a chosen business status and level).

fn a_damage_drive(app: &mut TestApp, business_status: &str, level: usize) -> DrivingState {
    let world = app.ctx.world;
    let mut profile = Profile::named_in("Damage Tester", "Denver");
    profile.tutorial_done = true;
    profile.business_status = business_status.to_string();
    profile.career.xp = LEVEL_XP[level - 1];
    if business_status != COMPANY_DRIVER {
        profile.owned_trucks = vec!["rig".to_string()];
    }
    app.ctx.profile = Some(profile);
    let route = world
        .route_from_cities(&["Denver", "Salt Lake City"])
        .expect("Denver to Salt Lake City is a route");
    let job = Job::new(
        CARGO_CATALOG
            .get("general")
            .expect("the general cargo type"),
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
    drive.trip.truck.set_air_ready(false);
    drive.trip.set_npc_vehicles(Vec::new());
    drive
}

/// `_rolling(driving, mph)`.
fn damage_rolling(drive: &mut DrivingState, mph: f64) {
    drive.trip.truck.engine_on = true;
    drive.trip.truck.velocity_mps = mph_to_mps(mph);
}

#[test]
fn test_each_band_announces_once_when_it_begins() {
    let mut app = TestApp::new();
    let mut drive = a_damage_drive(&mut app, LEASED_OWNER_OPERATOR, 1);
    damage_rolling(&mut drive, 60.0);
    let logged_before = app.ctx.message_log.messages.len();
    app.clear_speech();

    for damage in [
        DAMAGE_DERATE_PCT + 1.0,
        DAMAGE_LIMP_PCT + 1.0,
        DAMAGE_LAST_CALL_PCT + 1.0,
    ] {
        drive.trip.truck.damage_pct = damage;
        drive.update_damage_bands(&mut app.ctx, 1.0 / 60.0);
    }

    // Read as SUBMITTED lines rather than raw channel traffic: each band
    // interrupts, and the pacer hands a cut line back to finish behind the
    // one that cut it, so the same words can legitimately reach the voice
    // twice. The review log records each submission once.
    let lines = logged_since(&app, logged_before);
    assert_eq!(lines.len(), 3, "{lines:?}");
    assert!(lines[0].contains("Reduced power"), "{:?}", lines[0]);
    assert!(lines[1].contains("Limp mode"), "{:?}", lines[1]);
    assert!(
        lines[2].to_lowercase().contains("out of service"),
        "{:?}",
        lines[2]
    );
}

#[test]
fn test_a_second_excursion_into_a_band_warns_again() {
    let mut app = TestApp::new();
    let mut drive = a_damage_drive(&mut app, LEASED_OWNER_OPERATOR, 1);
    damage_rolling(&mut drive, 60.0);
    app.clear_speech();

    drive.trip.truck.damage_pct = DAMAGE_DERATE_PCT + 1.0;
    drive.update_damage_bands(&mut app.ctx, 1.0 / 60.0);
    drive.trip.truck.damage_pct = 0.0;
    drive.update_damage_bands(&mut app.ctx, 1.0 / 60.0);
    drive.trip.truck.damage_pct = DAMAGE_DERATE_PCT + 1.0;
    drive.update_damage_bands(&mut app.ctx, 1.0 / 60.0);

    let said = app
        .event_lines()
        .into_iter()
        .filter(|line| line.contains("Reduced power"))
        .count();
    assert_eq!(said, 2);
}

#[test]
fn test_terse_speech_keeps_a_short_form_of_every_band() {
    let mut app = TestApp::new();
    app.ctx.settings.driving_speech = "quiet".to_string();
    let mut drive = a_damage_drive(&mut app, LEASED_OWNER_OPERATOR, 1);
    damage_rolling(&mut drive, 60.0);
    let logged_before = app.ctx.message_log.messages.len();
    app.clear_speech();

    for damage in [
        DAMAGE_DERATE_PCT + 1.0,
        DAMAGE_LIMP_PCT + 1.0,
        DAMAGE_LAST_CALL_PCT + 1.0,
    ] {
        drive.trip.truck.damage_pct = damage;
        drive.update_damage_bands(&mut app.ctx, 1.0 / 60.0);
    }

    // Quiet speaks concise status and safety bands and keeps them in review.
    let logged: Vec<String> = app.ctx.message_log.messages[logged_before..]
        .iter()
        .map(|message| message.text.clone())
        .collect();
    assert_eq!(logged.len(), 3, "{logged:?}");
    assert_eq!(logged[0], "Reduced power. Damage 51 percent.");
    assert!(
        logged[1].starts_with("Limp mode. Capped at "),
        "{:?}",
        logged[1]
    );
    assert!(logged[2].contains("Out of service at"), "{:?}", logged[2]);
}

#[test]
fn test_repair_announces_the_band_on_the_way_back_down() {
    let mut app = TestApp::new();
    let mut drive = a_damage_drive(&mut app, LEASED_OWNER_OPERATOR, 1);
    damage_rolling(&mut drive, 60.0);
    drive.trip.truck.damage_pct = DAMAGE_LIMP_PCT + 5.0;
    drive.update_damage_bands(&mut app.ctx, 1.0 / 60.0);
    let logged_before = app.ctx.message_log.messages.len();
    app.clear_speech();

    drive.trip.truck.damage_pct = 30.0;
    drive.update_damage_bands(&mut app.ctx, 1.0 / 60.0);

    let lines = logged_since(&app, logged_before);
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert!(lines[0].contains("30 percent"), "{:?}", lines[0]);
    assert!(
        lines[0].to_lowercase().contains("full power"),
        "{:?}",
        lines[0]
    );
}

#[test]
fn test_limp_cap_speaks_before_the_physics_bites() {
    let mut app = TestApp::new();
    let mut drive = a_damage_drive(&mut app, LEASED_OWNER_OPERATOR, 1);
    damage_rolling(&mut drive, 68.0);
    app.clear_speech();

    drive.trip.truck.damage_pct = DAMAGE_LIMP_PCT + 1.0;
    drive.update_damage_bands(&mut app.ctx, 1.0 / 60.0);

    assert!(app
        .event_lines()
        .iter()
        .any(|line| line.contains("Limp mode")));
    // The cap opens at the speed the truck already has: nothing snapped away.
    let cap = drive.trip.truck.speed_cap_mph.expect("a cap");
    assert!((cap - 68.0).abs() < 0.5, "{cap}");
}

#[test]
fn test_limp_cap_ramps_down_at_comfortable_braking() {
    let mut app = TestApp::new();
    let mut drive = a_damage_drive(&mut app, LEASED_OWNER_OPERATOR, 1);
    damage_rolling(&mut drive, 65.0);
    drive.trip.truck.damage_pct = DAMAGE_LIMP_PCT + 1.0;
    drive.update_damage_bands(&mut app.ctx, 1.0 / 60.0);

    for _ in 0..60 {
        // one second of ramp
        drive.update_damage_bands(&mut app.ctx, 1.0 / 60.0);
    }
    let cap = drive.trip.truck.speed_cap_mph.expect("a cap");
    assert!(
        (cap - (65.0 - LIMP_CAP_RAMP_MPH_PER_S)).abs() < 0.2,
        "{cap}"
    );

    for _ in 0..(60 * 60) {
        drive.update_damage_bands(&mut app.ctx, 1.0 / 60.0);
    }
    assert_eq!(drive.trip.truck.speed_cap_mph, Some(DAMAGE_LIMP_CAP_MPH));
}

#[test]
fn test_the_wall_cap_also_ramps_instead_of_snapping() {
    let mut app = TestApp::new();
    let mut drive = a_damage_drive(&mut app, LEASED_OWNER_OPERATOR, 1);
    damage_rolling(&mut drive, 62.0);
    drive.trip.truck.damage_pct = DAMAGE_OUT_OF_SERVICE_PCT;

    drive.update_damage_cap(1.0 / 60.0);

    let cap = drive.trip.truck.speed_cap_mph.expect("a cap");
    assert!((cap - 62.0).abs() < 0.5, "{cap}");
    for _ in 0..60 {
        drive.update_damage_cap(1.0 / 60.0);
    }
    let cap = drive.trip.truck.speed_cap_mph.expect("a cap");
    assert!(
        (cap - (62.0 - LIMP_CAP_RAMP_MPH_PER_S)).abs() < 0.2,
        "{cap}"
    );
}

#[test]
fn test_limp_cap_never_engages_inside_the_gate_zone() {
    let mut app = TestApp::new();
    let mut drive = a_damage_drive(&mut app, LEASED_OWNER_OPERATOR, 1);
    damage_rolling(&mut drive, 60.0);
    drive.trip.truck.damage_pct = DAMAGE_LIMP_PCT + 5.0;
    drive.destination_exit_taken = true;
    drive.trip.position_mi = drive.trip.total_miles() - FACILITY_GATE_ZONE_MI / 2.0;

    drive.update_damage_cap(1.0 / 60.0);

    assert!(drive.trip.truck.speed_cap_mph.is_none());
}

#[test]
fn test_company_driver_pays_no_money_but_hours_and_standing() {
    let mut app = TestApp::new();
    let mut drive = a_damage_drive(&mut app, COMPANY_DRIVER, 1);
    damage_rolling(&mut drive, 0.0);
    let money = profile_of(&app.ctx).money();
    let reputation = profile_of(&app.ctx).career.reputation;
    let minutes_before = drive.trip.game_minutes;
    drive.trip.truck.damage_pct = DAMAGE_OUT_OF_SERVICE_PCT;
    app.clear_speech();

    drive.update_damage_bands(&mut app.ctx, 1.0 / 60.0);

    assert_eq!(profile_of(&app.ctx).money(), money);
    assert_eq!(
        profile_of(&app.ctx).career.reputation,
        reputation - BREAKDOWN_REPUTATION_HIT
    );
    assert_eq!(drive.trip.game_minutes, minutes_before + GROUNDED_SWAP_MIN);
    assert!(!drive.trip.truck.out_of_service());
    let spoken = app.event_lines().join(" ").to_lowercase();
    assert!(spoken.contains("carrier"), "{spoken}");
    assert!(spoken.contains("out of service"), "{spoken}");
}

#[test]
fn test_company_grounding_costs_more_hours_than_paying_for_it() {
    // The asymmetry in one line: the company driver trades money for time.
    const { assert!(GROUNDED_SWAP_MIN > BREAKDOWN_REPAIR_MIN) };
}

#[test]
fn test_company_driver_grounding_is_recorded_for_the_career_layer() {
    let mut app = TestApp::new();
    let mut drive = a_damage_drive(&mut app, COMPANY_DRIVER, 1);
    damage_rolling(&mut drive, 0.0);
    drive.trip.truck.damage_pct = DAMAGE_OUT_OF_SERVICE_PCT;

    drive.update_damage_bands(&mut app.ctx, 1.0 / 60.0);

    let profile = app.ctx.profile.as_mut().expect("a profile");
    assert!(int_stat(profile, "preventable_equipment_damage") >= 1);
}

#[test]
fn test_slip_seating_driver_is_moved_into_a_different_yard_spare() {
    let mut app = TestApp::new();
    let mut drive = a_damage_drive(&mut app, COMPANY_DRIVER, 5);
    let grounded = profile_of(&app.ctx).active_truck_key();
    damage_rolling(&mut drive, 0.0);
    let cargo_kg = drive.trip.truck.cargo_kg;
    drive.trip.truck.damage_pct = DAMAGE_OUT_OF_SERVICE_PCT;

    drive.update_damage_bands(&mut app.ctx, 1.0 / 60.0);

    assert_ne!(profile_of(&app.ctx).active_truck_key(), grounded);
    // The grounded tractor keeps its damage: it is in the shop, not fixed.
    let kept = profile_of(&app.ctx)
        .truck_conditions
        .get(&grounded)
        .and_then(|record| record.get("damage_pct"))
        .and_then(|value| value.as_f64())
        .expect("the grounded tractor keeps a record");
    assert!(kept >= DAMAGE_OUT_OF_SERVICE_PCT, "{kept}");
    // And the driver is in something they can actually drive.
    assert!(!drive.trip.truck.out_of_service());
    assert_eq!(drive.trip.truck.cargo_kg, cargo_kg);
}

#[test]
fn test_a_yard_spare_takes_over_the_wrecked_load_as_it_is() {
    // A rollover scraps the load and grounds the tractor; the spare used to
    // start the load over clean, so the receiver the driver was told would
    // refuse it signed for it in full, and a tank load stopped surging.
    let mut app = TestApp::new();
    let mut drive = a_damage_drive(&mut app, COMPANY_DRIVER, 5);
    let grounded = profile_of(&app.ctx).active_truck_key();
    damage_rolling(&mut drive, 0.0);
    drive.trip.truck.cargo_damage_pct = 100.0;
    drive.trip.truck.cargo_fragility = 1.6;
    drive.trip.truck.preventable_damage_pct = 40.0;
    drive.trip.truck.liquid = Some(ff_core::sim::surge::LiquidLoad::new(0.5, false));
    drive.trip.truck.damage_pct = DAMAGE_OUT_OF_SERVICE_PCT;

    drive.update_damage_bands(&mut app.ctx, 1.0 / 60.0);

    assert_ne!(profile_of(&app.ctx).active_truck_key(), grounded);
    let truck = &drive.trip.truck;
    assert_eq!(truck.cargo_damage_pct, 100.0);
    assert_eq!(truck.cargo_fragility, 1.6);
    assert!(truck.liquid.is_some(), "the tank load became dry freight");
    assert!(
        preventable_damage_charge(&drive).0 > 0.0,
        "the deductible went"
    );
}

#[test]
fn test_a_driver_with_no_spare_gets_the_road_crew_instead() {
    // A level-one yard has one tractor. Grounding must still leave a way on.
    let mut app = TestApp::new();
    let mut drive = a_damage_drive(&mut app, COMPANY_DRIVER, 1);
    let before = profile_of(&app.ctx).active_truck_key();
    damage_rolling(&mut drive, 0.0);
    drive.trip.truck.damage_pct = DAMAGE_OUT_OF_SERVICE_PCT;

    drive.update_damage_bands(&mut app.ctx, 1.0 / 60.0);

    assert_eq!(profile_of(&app.ctx).active_truck_key(), before);
    assert!(!drive.trip.truck.out_of_service());
    assert_eq!(drive.trip.truck.damage_pct, BREAKDOWN_REPAIR_DAMAGE_PCT);
}

#[test]
fn test_recovery_runs_once_however_many_frames_pass() {
    let mut app = TestApp::new();
    let mut drive = a_damage_drive(&mut app, LEASED_OWNER_OPERATOR, 1);
    app.ctx
        .profile
        .as_mut()
        .expect("a profile")
        .set_money(50_000.0);
    damage_rolling(&mut drive, 0.0);
    drive.trip.truck.damage_pct = DAMAGE_OUT_OF_SERVICE_PCT;
    let logged_before = app.ctx.message_log.messages.len();
    app.clear_speech();

    drive.update_damage_bands(&mut app.ctx, 1.0 / 60.0);
    let charged = 50_000.0 - profile_of(&app.ctx).money();
    for _ in 0..30 {
        drive.update_damage_bands(&mut app.ctx, 1.0 / 60.0);
    }

    assert_eq!(50_000.0 - profile_of(&app.ctx).money(), charged);
    let walls = logged_since(&app, logged_before)
        .into_iter()
        .filter(|line| line.contains("Out of service"))
        .count();
    assert_eq!(walls, 1);
}

#[test]
fn test_creeping_past_the_grace_window_summons_service_anyway() {
    // A driver who never stops must not be left crawling forever.
    let mut app = TestApp::new();
    let mut drive = a_damage_drive(&mut app, LEASED_OWNER_OPERATOR, 1);
    damage_rolling(&mut drive, 60.0);
    drive.trip.truck.damage_pct = DAMAGE_OUT_OF_SERVICE_PCT;

    for _ in 0..((OUT_OF_SERVICE_RECOVERY_GRACE_S * 60.0) as i32 + 120) {
        drive.update_damage_bands(&mut app.ctx, 1.0 / 60.0);
        drive.trip.truck.velocity_mps = drive.trip.truck.velocity_mps.max(5.0);
    }

    assert!(!drive.trip.truck.out_of_service());
}

#[test]
fn test_band_state_round_trips_through_a_snapshot() {
    let mut app = TestApp::new();
    let mut drive = a_damage_drive(&mut app, LEASED_OWNER_OPERATOR, 1);
    damage_rolling(&mut drive, 60.0);
    drive.trip.truck.damage_pct = DAMAGE_LIMP_PCT + 5.0;
    drive.update_damage_bands(&mut app.ctx, 1.0 / 60.0);
    drive.cargo_cue_at = CARGO_CUE_STEPS[0];
    drive.cargo_coaching_said = true;

    let data = drive.snapshot(&app.ctx);
    let resumed = DrivingState::from_snapshot(&mut app.ctx, &data).expect("the snapshot resumes");

    // A resume that re-announced limp mode, or snapped the cap back to full
    // speed, would be lying about the truck.
    assert_eq!(resumed.damage_band, DAMAGE_BAND_LIMP);
    assert_eq!(resumed.worst_damage_band, DAMAGE_BAND_LIMP);
    assert_eq!(resumed.cargo_cue_at, CARGO_CUE_STEPS[0]);
    assert!(resumed.cargo_coaching_said);
}

#[test]
fn test_the_worst_band_reached_survives_a_shoulder_repair() {
    // Settlement grades the run, not the moment it ended.
    let mut app = TestApp::new();
    let mut drive = a_damage_drive(&mut app, LEASED_OWNER_OPERATOR, 1);
    damage_rolling(&mut drive, 60.0);
    drive.trip.truck.damage_pct = DAMAGE_LAST_CALL_PCT + 1.0;
    drive.update_damage_bands(&mut app.ctx, 1.0 / 60.0);
    assert_eq!(drive.worst_damage_band, DAMAGE_BAND_LAST_CALL);

    drive.trip.truck.damage_pct = 5.0;
    drive.update_damage_bands(&mut app.ctx, 1.0 / 60.0);

    assert_eq!(drive.damage_band, DAMAGE_BAND_NONE);
    assert_eq!(drive.worst_damage_band, DAMAGE_BAND_LAST_CALL);
}

#[path = "states_driving_road/quiet_speech.rs"]
mod quiet_speech;

#[test]
fn test_a_car_passing_the_truck_is_not_a_lane_opening() {
    // Passers come up the lane beside now. One going by the truck is not
    // one the truck overtook, and "Left lane open" for every one of them
    // was four lines in three minutes (owner drive, 2026-10-09).
    let mut app = TestApp::new();
    let mut drive = a_gap_drive(&mut app);
    rolling(&mut drive, 60.0);
    app.clear_speech();
    drive.lane.lane = 1;
    drive.update_lane_gap(&mut app.ctx, 0.1); // settled in the left lane
    drive.lane.lane = 0; // and back right: the left lane is watched
    let alongside = npc(drive.trip.position_mi, 1, 70.0, "car", "passer");
    drive.trip.traffic_manager.vehicles = vec![alongside];
    drive.update_lane_gap(&mut app.ctx, 0.1);
    drive.trip.traffic_manager.vehicles =
        vec![npc(drive.trip.position_mi + 0.9, 1, 70.0, "car", "passer")];
    for _ in 0..20 {
        drive.update_lane_gap(&mut app.ctx, 0.1);
    }
    assert!(openings(&app).is_empty(), "{:?}", openings(&app));
}
