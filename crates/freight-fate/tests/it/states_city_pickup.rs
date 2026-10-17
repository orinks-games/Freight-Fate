//! The pickup facility and route planning: the city-side ports of
//! `tests/test_pickup_loading.py`.
//!
//! The Python fixtures drove the deadhead to the shipper first; that drive
//! is the driving port's, so these build the pickup screen the arrival
//! would have built and pin what happens on it.

use crate::states_city_support::*;
use ff_core::models::business::LEASED_OWNER_OPERATOR;
use ff_core::models::jobs::{cargo_type, Job};
use ff_core::models::trailer_yard::{
    preloaded_trailer, DROP_HOOK_MIN, LIVE_LOAD_MIN, TRAILER_SWAP_MIN,
};
use freight_fate::app::testing::TestApp;
use freight_fate::app::GameContext;
use freight_fate::states::base::{Key, TimedMessageState};
use freight_fate::states::city::CityMenuState;
use freight_fate::states::city_pickup::{
    job_origin_exists, pickup_snapshot, PickupFacilityState, PickupOptions, PickupSnapshotOptions,
    RouteSelectState,
};
use freight_fate::states::driving::DrivingState;
use freight_fate::states::driving_pause_states::PauseMenuState;
use freight_fate::states::main_menu::MainMenuState;

/// A dispatch out of a facility that stages loaded trailers.
fn drop_yard_job(facility_id: &str, distance_mi: f64) -> Job {
    let mut job = Job::new(
        cargo_type("general").unwrap(),
        12.0,
        "Chicago",
        "Chicago Cross-Dock",
        "Milwaukee",
        distance_mi,
        1800.0,
        9.0,
    );
    job.origin_type = "cross_dock".to_string(); // a shipper that stages trailers
    job.origin_facility_id = facility_id.to_string();
    job
}

/// Search the deterministic yard for a job whose preloaded trailer does (or
/// does not) fail a walk-around. The condition is derived from the facility
/// and the run, so this picks a real one instead of patching the model.
fn job_with_trailer(defective: bool) -> Job {
    for i in 0..400 {
        let job = drop_yard_job(&format!("chicago-cross-dock-{i}"), 90.0 + i as f64);
        if let Some(trailer) = preloaded_trailer(&job) {
            if trailer.defect().is_some() == defective {
                return job;
            }
        }
    }
    panic!("no seeded drop yard produced a {defective} trailer");
}

fn push_pickup(app: &mut TestApp, job: Job, opts: PickupOptions) {
    let pickup = PickupFacilityState::new(&app.ctx, job, opts);
    app.push_state(pickup);
}

/// A checked-in, loaded pickup holding a trailer of a chosen condition
/// (`_pickup_with_trailer`).
fn pickup_with_trailer(app: &mut TestApp, defective: bool) -> Job {
    let job = job_with_trailer(defective);
    career(app, "Walker", "Chicago");
    push_pickup(
        app,
        job.clone(),
        PickupOptions {
            ..PickupOptions::default()
        },
    );
    key(app, Key::Return); // check in
    key(app, Key::Return); // drop and hook
    finish_timed_state(app);
    job
}

// -- the pickup screen itself ---------------------------------------------------------

#[test]
fn test_pickup_facility_walks_check_in_then_loading() {
    // The spine of the Python fixture, minus the drive that reaches it: the
    // first row is the check-in, the second the dock, and the clock and the
    // duty log move with each.
    let mut app = TestApp::new();
    career(&mut app, "Shipper Visit", "Chicago");
    let mut job = drop_yard_job("chicago-live-load", 92.0);
    job.origin_type = "mine_quarry".to_string(); // a shipper that loads at a dock
    push_pickup(&mut app, job, PickupOptions::default());

    assert_eq!(
        current_label::<PickupFacilityState>(&app),
        "Check in at shipping office"
    );
    key(&mut app, Key::Return);
    assert!(with_state::<PickupFacilityState, _>(&app, |p, _| p.checked_in));
    assert_eq!(
        current_label::<PickupFacilityState>(&app),
        "Load cargo at dock"
    );

    let hours_before = profile(&app).game_hours;
    let duty_before = profile(&app).hos.duty_min;
    key(&mut app, Key::Return);
    assert!(is::<TimedMessageState>(&app));
    finish_timed_state(&mut app);

    assert!(with_state::<PickupFacilityState, _>(&app, |p, _| p.loaded));
    assert!(profile(&app).game_hours > hours_before);
    assert!(profile(&app).hos.duty_min > duty_before);
    assert_eq!(
        current_label::<PickupFacilityState>(&app),
        "Depart for destination"
    );
}

#[test]
fn test_loading_at_pickup_uses_dock_sound() {
    let mut app = TestApp::new();
    career(&mut app, "Dock Sound", "Chicago");
    let mut job = drop_yard_job("chicago-live-load", 92.0);
    // This test is about the dock, so pin the pickup to a shipper that
    // loads at one: a drop yard would hand over a preloaded trailer and
    // never open a door (see tests/test_trailer_yard.py).
    job.origin_type = "mine_quarry".to_string();
    push_pickup(&mut app, job, PickupOptions::default());
    let played = app.record_audio();

    key(&mut app, Key::Return); // check in
    let plan = with_state::<PickupFacilityState, _>(&app, |p, ctx| p.pickup_plan(ctx));
    assert!(!plan.is_drop_hook());
    let hours_before = profile(&app).game_hours;
    let duty_before = profile(&app).hos.duty_min;
    key(&mut app, Key::Return); // load cargo
    assert_eq!(app.visible_lines()[0], "Loading cargo");
    finish_timed_state(&mut app);

    let keys: Vec<String> = played
        .borrow()
        .played
        .iter()
        .map(|(key, _, _)| key.clone())
        .collect();
    assert!(keys.iter().any(|k| k == "poi/dock_and_deliver"));
    assert!(keys.iter().any(|k| k == "ui/level_up"));
    assert_eq!(profile(&app).game_hours, hours_before + plan.minutes / 60.0);
    assert_eq!(profile(&app).hos.duty_min, duty_before + plan.minutes);
}

#[test]
fn test_drop_and_hook_gets_the_truck_out_in_a_fraction_of_the_time() {
    // A preloaded trailer means no dock and no hour standing at one.
    let mut app = TestApp::new();
    career(&mut app, "Drop Hook", "Chicago");
    let job = job_with_trailer(false);
    push_pickup(&mut app, job, PickupOptions::default());
    key(&mut app, Key::Return); // check in
    let plan = with_state::<PickupFacilityState, _>(&app, |p, ctx| p.pickup_plan(ctx));
    assert!(plan.is_drop_hook());
    assert_eq!(plan.minutes, DROP_HOOK_MIN);
    assert!(
        plan.minutes < LIVE_LOAD_MIN,
        "a preloaded trailer must leave sooner than a live load"
    );
    // Check-in already says there is no dock coming.
    let said = app.main_lines().last().cloned().unwrap();
    assert!(said.contains("drop yard"));
    assert!(said.contains(&plan.trailer.as_ref().unwrap().number));

    let hours_before = profile(&app).game_hours;
    key(&mut app, Key::Return); // drop and hook
    assert_eq!(app.visible_lines()[0], "Hooking the loaded trailer");
    finish_timed_state(&mut app);

    assert_eq!(
        profile(&app).game_hours,
        hours_before + DROP_HOOK_MIN / 60.0
    );
    assert!(profile(&app).hos.duty_min > 0.0);
    // The driver is told which trailer they are pulling and what shape it
    // is in -- the whole risk of hooking somebody else's box.
    let lines = app.main_lines();
    let readout = lines[lines.len().saturating_sub(3)..].join(" ");
    assert!(readout.contains(&plan.trailer.as_ref().unwrap().number));
    assert!(readout.to_lowercase().contains("hooked to"));
}

// -- the walk-around, and refusing a trailer ---------------------------------------

#[test]
fn test_a_clean_trailer_walks_around_clean() {
    let mut app = TestApp::new();
    let job = pickup_with_trailer(&mut app, false);
    assert!(preloaded_trailer(&job).unwrap().defect().is_none());
    select::<PickupFacilityState>(&mut app, "Walk around the trailer");
    assert!(app.main_lines().last().unwrap().contains("checks out"));
    // Nothing to refuse, so no refusal offered.
    assert!(!labels::<PickupFacilityState>(&app)
        .iter()
        .any(|t| t == "Refuse this trailer"));
}

#[test]
fn test_walking_a_bad_trailer_finds_it_and_offers_the_refusal() {
    // The defect is something the driver goes and finds, not something that
    // happens to them at a scale house.
    let mut app = TestApp::new();
    let job = pickup_with_trailer(&mut app, true);
    let unit = preloaded_trailer(&job).unwrap();
    let defect = unit.defect().expect("a defective trailer");
    assert!(!labels::<PickupFacilityState>(&app)
        .iter()
        .any(|t| t == "Refuse this trailer"));

    app.clear_speech();
    select::<PickupFacilityState>(&mut app, "Walk around the trailer");
    // What the walk-around found comes first; the badge for finding it after.
    let lines = app.main_lines();
    let found = lines
        .iter()
        .position(|line| line.contains("Take it and the write-up"))
        .unwrap_or_else(|| panic!("no walk-around line: {lines:?}"));
    assert!(lines[found].contains(defect), "{lines:?}");
    assert!(lines[found].contains(&unit.number));
    assert!(
        lines[found + 1..]
            .iter()
            .any(|line| line.starts_with("New achievement!")),
        "{lines:?}"
    );
    // Only once the driver has actually looked does refusing become an option.
    assert!(labels::<PickupFacilityState>(&app)
        .iter()
        .any(|t| t == "Refuse this trailer"));
}

#[test]
fn test_refusing_a_trailer_costs_time_and_gets_a_sound_one() {
    let mut app = TestApp::new();
    let job = pickup_with_trailer(&mut app, true);
    let unit = preloaded_trailer(&job).unwrap();
    select::<PickupFacilityState>(&mut app, "Walk around the trailer");
    let hours_before = profile(&app).game_hours;

    select::<PickupFacilityState>(&mut app, "Refuse this trailer");

    assert_eq!(
        profile(&app).game_hours,
        hours_before + TRAILER_SWAP_MIN / 60.0
    );
    let hooked = with_state::<PickupFacilityState, _>(&app, |p, ctx| p.hooked_trailer(ctx))
        .expect("a hooked trailer");
    assert!(hooked.defect().is_none());
    assert_ne!(hooked.number, unit.number);
    assert!(app
        .main_lines()
        .last()
        .unwrap()
        .contains("yard brings another"));
    // Walking it again reports the sound one, not the box that went back.
    select::<PickupFacilityState>(&mut app, "Walk around the trailer");
    assert!(app.main_lines().last().unwrap().contains("checks out"));
}

// -- saving and resuming the pickup objective ------------------------------------------

#[test]
fn test_pickup_arrival_state_and_loaded_planning_resume() {
    // The Python round-tripped through the main menu's Continue; the part
    // the pickup screen owns is the snapshot it writes and rebuilds from.
    let mut app = TestApp::new();
    career(&mut app, "Resume Pickup", "Chicago");
    let mut job = drop_yard_job("chicago-live-load", 92.0);
    job.origin_type = "mine_quarry".to_string();
    push_pickup(&mut app, job.clone(), PickupOptions::default());
    key(&mut app, Key::Return); // check in

    let snapshot = profile(&app)
        .active_trip
        .clone()
        .expect("a saved objective");
    assert_eq!(snapshot["kind"], "pickup");
    assert_eq!(snapshot["checked_in"], true);
    assert_eq!(snapshot["loaded"], false);
    let resumed = PickupFacilityState::from_snapshot(&app.ctx, &snapshot).expect("it rebuilds");
    assert!(resumed.checked_in);
    assert!(!resumed.loaded);
    app.pop_state();
    app.push_state(resumed);

    key(&mut app, Key::Return); // load
    finish_timed_state(&mut app);
    assert!(with_state::<PickupFacilityState, _>(&app, |p, _| p.loaded));
    assert_eq!(profile(&app).active_trip.as_ref().unwrap()["loaded"], true);

    let snapshot = profile(&app).active_trip.clone().unwrap();
    let resumed = PickupFacilityState::from_snapshot(&app.ctx, &snapshot).expect("it rebuilds");
    assert!(resumed.loaded);
    app.pop_state();
    app.push_state(resumed);
    assert_eq!(
        current_label::<PickupFacilityState>(&app),
        "Depart for destination"
    );
}

#[test]
fn test_pickup_save_and_departure_keep_speed_control_session() {
    let mut app = TestApp::new();
    career(&mut app, "Speed Session", "Chicago");
    let mut job = drop_yard_job("chicago-live-load", 92.0);
    job.origin_type = "mine_quarry".to_string();
    push_pickup(
        &mut app,
        job,
        PickupOptions {
            speed_control_armed: true,
            speed_control_target_mph: Some(47.0),
            announce_speed_control_status: true,
            ..PickupOptions::default()
        },
    );

    assert!(with_state::<PickupFacilityState, _>(&app, |p, _| p.speed_control_armed));
    assert!(app
        .main_lines()
        .iter()
        .any(|line| line.contains("Speed control paused, target 47 miles per hour.")));

    with_state_mut::<PickupFacilityState, _>(&mut app, |p, ctx| p.status(ctx));
    assert!(app
        .main_lines()
        .last()
        .unwrap()
        .contains("target 47 miles per hour"));

    key(&mut app, Key::Return); // check in
    let snapshot = profile(&app).active_trip.clone().unwrap();
    assert_eq!(snapshot["speed_control_armed"], true);
    assert_eq!(snapshot["speed_control_target_mph"], 47.0);
    let resumed = PickupFacilityState::from_snapshot(&app.ctx, &snapshot).expect("it rebuilds");
    assert!(resumed.speed_control_armed);
    assert_eq!(resumed.speed_control_target_mph, Some(47.0));
}

#[test]
fn test_cancelling_the_pickup_returns_to_the_terminal() {
    let mut app = TestApp::new();
    career(&mut app, "Cancel Pickup", "Chicago");
    let job = drop_yard_job("chicago-live-load", 92.0);
    push_pickup(&mut app, job, PickupOptions::default());

    select::<PickupFacilityState>(&mut app, "Cancel pickup and return to terminal");

    assert!(is::<CityMenuState>(&app));
    assert!(profile(&app).active_trip.is_none());
    assert!(profile(&app).dispatch_board_cache.is_none());
}

// -- route planning ---------------------------------------------------------------------

#[test]
fn test_owner_operator_route_menu_lists_routes_and_starts_one() {
    let mut app = TestApp::new();
    career(&mut app, "Route Picker", "Chicago");
    {
        let p = profile_mut(&mut app);
        p.business_status = LEASED_OWNER_OPERATOR.to_string();
        p.owned_trucks = vec!["rig".to_string()];
    }
    let job = Job::new(
        cargo_type("general").unwrap(),
        12.0,
        "Chicago",
        "Chicago yard",
        "Milwaukee",
        92.0,
        1800.0,
        9.0,
    );
    let pickup = loaded_pickup(&app, job);
    app.push_state(pickup);
    key(&mut app, Key::Return); // depart

    assert!(is::<RouteSelectState>(&app));
    let rows = labels::<RouteSelectState>(&app);
    assert!(rows[0].starts_with("Route 1:"));
    assert!(rows[0].contains("Fuel stops:"));
    assert_eq!(rows.last().unwrap(), "Back to pickup facility");
    // W speaks a forecast rather than moving the cursor.
    let index_before = index::<RouteSelectState>(&app);
    app.clear_speech();
    key(&mut app, Key::W);
    assert_eq!(index::<RouteSelectState>(&app), index_before);
    assert!(app
        .main_lines()
        .last()
        .unwrap()
        .starts_with("Forecast along the route."));

    key(&mut app, Key::Home);
    key(&mut app, Key::Return);
    assert!(is::<DrivingState>(&app));
    // The route screen named the destination; the drive does not repeat it.
    assert!(!app
        .main_lines()
        .iter()
        .any(|line| line.contains("Navigation set for")));
}

// -- the stale-facility guard ------------------------------------------------------------

#[test]
fn test_job_origin_exists_reads_the_current_world() {
    let app = TestApp::new();
    let world = app.ctx.world;
    let mut job = Job::new(
        cargo_type("general").unwrap(),
        12.0,
        "Chicago",
        "Chicago Retired Facility",
        "Milwaukee",
        92.0,
        1800.0,
        7.0,
    );
    assert!(!job_origin_exists(&job, world));
    job.bobtail = true; // a reposition's origin is a synthetic company yard
    assert!(job_origin_exists(&job, world));
}

#[test]
fn test_pickup_snapshot_carries_the_resume_fields() {
    let job = drop_yard_job("chicago-live-load", 92.0);
    let snapshot = pickup_snapshot(
        &job,
        &PickupSnapshotOptions {
            checked_in: true,
            loaded: true,
            speed_control_armed: true,
            speed_control_target_mph: Some(52.0),
            trailer_refused: true,
            ..PickupSnapshotOptions::default()
        },
    );
    assert_eq!(snapshot["kind"], "pickup");
    assert_eq!(snapshot["checked_in"], true);
    assert_eq!(snapshot["loaded"], true);
    assert_eq!(snapshot["trailer_refused"], true);
    assert_eq!(snapshot["speed_control_target_mph"], 52.0);
    assert!(snapshot.contains_key("job"));
}

// -- what still needs the drive ----------------------------------------------------------

#[test]
fn test_quit_during_pickup_drive_resumes_from_the_last_stop() {
    // Saving happens only at stops, so quitting mid-pickup-drive does not save
    // the in-progress position: the leg resumes from where it was last departed.
    let mut app = TestApp::new();
    accept_pickup_drive(&mut app);
    with_state_mut::<DrivingState, _>(&mut app, |d, _| d.trip.restore(1.5, 12.0));

    key(&mut app, Key::Escape);
    assert!(is::<PauseMenuState>(&app), "escape opens the pause menu");
    let rows = labels::<PauseMenuState>(&app);
    assert!(
        !rows.iter().any(|row| row == "Save and quit to main menu"),
        "{rows:?}"
    );
    select::<PauseMenuState>(&mut app, "Quit to main menu");

    select::<MainMenuState>(&mut app, "Continue latest career");

    assert!(is::<DrivingState>(&app));
    let (phase, position) =
        with_state::<DrivingState, _>(&app, |d, _| (d.phase.to_string(), d.trip.position_mi));
    assert_eq!(
        phase,
        freight_fate::states::driving_core::DRIVE_PHASE_PICKUP
    );
    // in-progress driving was not saved; the leg restarts from the terminal
    assert_eq!(position, 0.0);
}

#[test]
fn test_departing_loaded_trip_keeps_idling_engine() {
    let mut app = TestApp::new();
    accept_pickup_drive(&mut app);
    with_state_mut::<DrivingState, _>(&mut app, |d, _| {
        d.trip.truck.start_engine();
    });
    arrive_at_pickup(&mut app, 0.0);
    assert!(with_state::<PickupFacilityState, _>(&app, |p, _| p
        .truck
        .engine_on));

    key(&mut app, Key::Return); // check in
    key(&mut app, Key::Return); // load, or drop and hook
    finish_timed_state(&mut app);
    assert!(with_state::<PickupFacilityState, _>(&app, |p, _| p
        .truck
        .engine_on));
    select::<PickupFacilityState>(&mut app, "Depart for destination");

    assert!(is::<DrivingState>(&app));
    let (phase, engine_on) =
        with_state::<DrivingState, _>(&app, |d, _| (d.phase.to_string(), d.trip.truck.engine_on));
    assert_eq!(
        phase,
        freight_fate::states::driving_core::DRIVE_PHASE_DELIVERY
    );
    assert!(engine_on);
    assert_eq!(
        profile(&app).active_trip.as_ref().unwrap()["engine_on"],
        serde_json::Value::Bool(true)
    );
}

/// Otherwise the walk-around is theatre: the scale house has to find the box
/// actually under the truck.
#[test]
fn test_a_refused_trailer_does_not_follow_the_driver_onto_the_road() {
    let mut app = TestApp::new();
    pickup_with_trailer(&mut app, true);
    select::<PickupFacilityState>(&mut app, "Walk around the trailer");
    select::<PickupFacilityState>(&mut app, "Refuse this trailer");
    // The decision survives a save.
    assert_eq!(
        profile(&app).active_trip.as_ref().unwrap()["trailer_refused"],
        serde_json::Value::Bool(true)
    );

    select::<PickupFacilityState>(&mut app, "Depart for destination");
    assert!(is::<DrivingState>(&app));
    let refused = with_state::<DrivingState, _>(&app, |d, _| d.trailer_refused);
    assert!(refused);
    let defect = with_state::<DrivingState, _>(&app, |d, ctx| d.hooked_trailer_defect(ctx));
    assert_eq!(defect, None);
    let snapshot = with_state_mut::<DrivingState, _>(&mut app, |d, ctx| d.snapshot(ctx));
    assert_eq!(snapshot["trailer_refused"], serde_json::Value::Bool(true));
}

#[test]
fn test_an_unrefused_defect_is_what_the_inspector_finds() {
    let mut app = TestApp::new();
    let job = pickup_with_trailer(&mut app, true);
    let expected = preloaded_trailer(&job)
        .expect("the yard staged a trailer")
        .defect()
        .map(str::to_string);
    assert!(expected.is_some(), "the fixture wanted a defective trailer");

    select::<PickupFacilityState>(&mut app, "Depart for destination"); // rolled out without looking
    assert!(is::<DrivingState>(&app));
    let refused = with_state::<DrivingState, _>(&app, |d, _| d.trailer_refused);
    assert!(!refused);
    let defect = with_state::<DrivingState, _>(&app, |d, ctx| d.hooked_trailer_defect(ctx));
    assert_eq!(defect, expected);
}

/// The box waiting in the shipper's yard is not on the truck yet. A player
/// on the deadhead to a pickup was written up twice for its lamp (report,
/// 2026-10-17).
#[test]
fn test_the_deadhead_is_not_cited_for_the_trailer_still_in_the_yard() {
    use ff_core::sim::roadside_inspection::InspectionLevel;
    let mut app = TestApp::new();
    let job = job_with_trailer(true);
    career(&mut app, "Deadhead", "Chicago");
    profile_mut(&mut app)
        .achievements
        .push("first_dispatch".to_string());
    let board = freight_fate::states::city::JobBoardState::new(&app.ctx, vec![job]);
    app.push_state(board);
    key(&mut app, Key::Return); // accept assigned dispatch
    assert!(is::<DrivingState>(&app), "the deadhead starts");
    let (phase, defect, walk, findings) = with_state::<DrivingState, _>(&app, |d, ctx| {
        (
            d.phase.to_string(),
            d.hooked_trailer_defect(ctx),
            d.walk_around_lines(ctx),
            d.inspection_report(ctx, InspectionLevel::Full).findings,
        )
    });
    assert_eq!(
        phase,
        freight_fate::states::driving_core::DRIVE_PHASE_PICKUP
    );
    assert_eq!(defect, None);
    assert!(
        !walk.iter().any(|line| line.contains("trailer")),
        "{walk:?}"
    );
    assert!(
        !findings.iter().any(|f| f.what.contains("trailer")),
        "{findings:?}"
    );
}

/// It said it would wait for departure, so it must not re-engage at the gate.
#[test]
fn test_speed_control_stays_paused_until_departure() {
    let mut app = TestApp::new();
    accept_pickup_drive(&mut app);

    // An armed session, rolling toward the pickup gate.
    let armed = with_state_mut::<DrivingState, _>(&mut app, |d, ctx| {
        d.trip.truck.start_engine();
        d.trip.truck.set_air_ready(false);
        d.engage_cruise(ctx, 30.0, false);
        d.speed_control_armed
    });
    assert!(armed);

    let paused = with_state_mut::<DrivingState, _>(&mut app, |d, ctx| {
        d.trip.position_mi = d.trip.total_miles();
        d.trip.finished = true;
        d.trip.truck.velocity_mps = 8.0; // still rolling, above the gate stop speed
        d.update_frame(ctx, 1.0 / 60.0);
        d.speed_control_paused_at_stop
    });
    app.ctx.run_deferred();
    assert!(paused);
    let events = app.event_lines();
    assert!(
        events.iter().any(|text| text.contains("paused for pickup")),
        "{events:?}"
    );

    // Several frames of still rolling up to the gate.
    app.clear_speech();
    for _ in 0..30 {
        with_state_mut::<DrivingState, _>(&mut app, |d, ctx| {
            d.update_frame(ctx, 1.0 / 60.0);
        });
        app.ctx.run_deferred();
    }
    let events = app.event_lines();
    assert!(
        !events.iter().any(|text| text.contains("resuming")),
        "{events:?}"
    );
    let (cruise, keeper) = with_state::<DrivingState, _>(&app, |d, _| (d.cruise_mph, d.keeper_mph));
    assert_eq!(cruise, None);
    assert_eq!(keeper, None);
}

/// Roll into the pickup with adaptive cruise set to 55, stop at the gate the
/// way `stop` does, load, and depart: the loaded run starts with the same
/// session and the same 55 (tester report, 2026-09-28).
fn cruise_survives_the_pickup(
    assist: bool,
    stop: impl FnOnce(&mut DrivingState, &mut GameContext),
) {
    let mut app = TestApp::new();
    app.ctx.settings.destination_approach_assist = assist;
    accept_pickup_drive(&mut app);
    with_state_mut::<DrivingState, _>(&mut app, |d, ctx| {
        d.trip.truck.start_engine();
        d.trip.truck.set_air_ready(false);
        d.engage_cruise(ctx, 55.0, false);
        d.trip.position_mi = d.trip.total_miles();
        d.trip.finished = true;
        d.trip.truck.velocity_mps = 0.0;
        d.trip.truck.parking_brake = false;
        d.update_frame(ctx, 1.0 / 60.0);
        stop(d, ctx);
    });
    app.ctx.run_deferred();
    finish_timed_state(&mut app);
    let session = with_state::<PickupFacilityState, _>(&app, |p, _| {
        (p.speed_control_armed, p.speed_control_target_mph)
    });
    assert_eq!(session, (true, Some(55.0)));

    key(&mut app, Key::Return); // check in
    key(&mut app, Key::Return); // load, or drop and hook
    finish_timed_state(&mut app);
    select::<PickupFacilityState>(&mut app, "Depart for destination");
    let session = with_state::<DrivingState, _>(&app, |d, _| {
        (d.speed_control_armed, d.speed_control_target_mph)
    });
    assert_eq!(session, (true, Some(55.0)));
}

#[test]
fn test_cruise_setting_survives_the_pickup_with_the_stopping_assist() {
    cruise_survives_the_pickup(true, |d, ctx| {
        // The assist holds at the entrance; Enter continues in.
        d.update_frame(ctx, 1.0 / 60.0);
        d.open_ready_facility_arrival(ctx);
    });
}

#[test]
fn test_cruise_setting_survives_setting_the_parking_brake_at_the_pickup() {
    cruise_survives_the_pickup(false, |d, ctx| {
        // The gate asks for the parking brake, then T opens the facility.
        d.toggle_parking_brake(ctx);
        d.update_frame(ctx, 1.0 / 60.0);
    });
}

#[test]
fn test_arming_by_hand_at_the_gate_still_works() {
    let mut app = TestApp::new();
    accept_pickup_drive(&mut app);

    let paused = with_state_mut::<DrivingState, _>(&mut app, |d, ctx| {
        d.trip.truck.start_engine();
        d.trip.truck.set_air_ready(false);
        d.engage_cruise(ctx, 30.0, false);
        d.trip.position_mi = d.trip.total_miles();
        d.trip.finished = true;
        d.trip.truck.velocity_mps = 8.0;
        d.update_frame(ctx, 1.0 / 60.0);
        d.speed_control_paused_at_stop
    });
    app.ctx.run_deferred();
    assert!(paused);

    // The player overrides the hold themselves.
    let (paused, cruise) = with_state_mut::<DrivingState, _>(&mut app, |d, ctx| {
        d.engage_cruise(ctx, 20.0, false);
        (d.speed_control_paused_at_stop, d.cruise_mph)
    });
    assert!(!paused);
    assert!(cruise.is_some());
}

// -- the facility engine kill switch (tests/test_facility_engine.py) ------------------
//
// Shutting the engine down while parked at a pickup facility. The kill switch
// has always existed on the road, but a facility arrival takes the truck over
// at half a mile an hour and hands straight to a menu, so the one moment a
// driver actually reaches for it -- sitting at the shipper waiting to be
// loaded -- was the one moment the game did not offer it. Idling through that
// wait now costs fuel, which is what makes the switch worth reaching for.
//
// The Python fixtures drove the deadhead in with the engine running; here the
// pickup is built with `engine_on`, which is what that arrival hands over.
// The destination-facility half of the file lives with the arrival menus.

const SHUT_DOWN: &str = "Shut down the engine";
const START: &str = "Start the engine";

/// Reach the pickup facility with the engine idling, as a drive-in does.
fn pickup_running(app: &mut TestApp) -> Job {
    let mut job = drop_yard_job("chicago-live-load", 92.0);
    job.origin_type = "mine_quarry".to_string(); // a shipper that loads at a dock
    career(app, "Facility Engine", "Chicago");
    push_pickup(
        app,
        job.clone(),
        PickupOptions {
            engine_on: true,
            ..PickupOptions::default()
        },
    );
    assert!(with_state::<PickupFacilityState, _>(app, |p, _| p
        .truck
        .engine_on));
    job
}

/// Check in and get the freight on. `pickup_running` pins a dock shipper, so
/// the second primary row is the dock rather than a drop-and-hook yard.
fn load_out(app: &mut TestApp) {
    select::<PickupFacilityState>(app, "Check in at shipping office");
    select::<PickupFacilityState>(app, "Load cargo at dock");
    finish_timed_state(app);
    assert!(with_state::<PickupFacilityState, _>(app, |p, _| p.loaded));
}

#[test]
fn test_pickup_facility_offers_shutdown_and_then_restart() {
    let mut app = TestApp::new();
    pickup_running(&mut app);
    assert!(labels::<PickupFacilityState>(&app)
        .iter()
        .any(|l| l == SHUT_DOWN));

    select::<PickupFacilityState>(&mut app, SHUT_DOWN);
    assert!(!with_state::<PickupFacilityState, _>(&app, |p, _| p
        .truck
        .engine_on));
    assert!(app.main_lines().last().unwrap().contains("Engine off."));

    // One row that changes face, not two: a screen reader user arrows past a
    // single engine line either way.
    let rows = labels::<PickupFacilityState>(&app);
    assert!(!rows.iter().any(|l| l == SHUT_DOWN), "{rows:?}");
    assert!(rows.iter().any(|l| l == START), "{rows:?}");

    select::<PickupFacilityState>(&mut app, START);
    assert!(with_state::<PickupFacilityState, _>(&app, |p, _| p
        .truck
        .engine_on));
    assert!(app.main_lines().last().unwrap().contains("Engine running."));
    assert!(labels::<PickupFacilityState>(&app)
        .iter()
        .any(|l| l == SHUT_DOWN));
}

#[test]
fn test_the_primary_action_stays_the_first_item() {
    let mut app = TestApp::new();
    pickup_running(&mut app);
    // Enter on arrival must still check in. The engine row sits with the
    // other truck actions, never in front of the flow the facility is for.
    let first = labels::<PickupFacilityState>(&app)[0].clone();
    assert_eq!(first, "Check in at shipping office");
    select::<PickupFacilityState>(&mut app, SHUT_DOWN);
    assert_eq!(labels::<PickupFacilityState>(&app)[0], first);
}

#[test]
fn test_engine_off_at_the_pickup_survives_save_and_quit() {
    let mut app = TestApp::new();
    pickup_running(&mut app);
    select::<PickupFacilityState>(&mut app, SHUT_DOWN);
    assert_eq!(
        profile(&app).active_trip.as_ref().unwrap()["engine_on"],
        serde_json::Value::Bool(false)
    );

    select::<PickupFacilityState>(&mut app, "Save and quit to main menu");
    select::<MainMenuState>(&mut app, "Continue latest career");

    assert!(is::<PickupFacilityState>(&app));
    assert!(!with_state::<PickupFacilityState, _>(&app, |p, _| p
        .truck
        .engine_on));
    assert!(labels::<PickupFacilityState>(&app)
        .iter()
        .any(|l| l == START));
}

#[test]
fn test_loading_still_works_with_the_engine_shut_down() {
    let mut app = TestApp::new();
    pickup_running(&mut app);
    select::<PickupFacilityState>(&mut app, SHUT_DOWN);
    load_out(&mut app);
    assert!(!with_state::<PickupFacilityState, _>(&app, |p, _| p
        .truck
        .engine_on));
}

/// Jake's point: an hour on the dock has to cost something, or the switch is
/// decoration.
#[test]
fn test_idling_through_the_load_burns_fuel_and_shutting_down_does_not() {
    let mut idled = TestApp::new();
    pickup_running(&mut idled);
    let before = with_state::<PickupFacilityState, _>(&idled, |p, _| p.truck.fuel_gal);
    load_out(&mut idled);
    let burned_idling =
        before - with_state::<PickupFacilityState, _>(&idled, |p, _| p.truck.fuel_gal);
    // A TestApp holds the environment lock until it is dropped.
    drop(idled);

    let mut shut = TestApp::new();
    pickup_running(&mut shut);
    let before = with_state::<PickupFacilityState, _>(&shut, |p, _| p.truck.fuel_gal);
    select::<PickupFacilityState>(&mut shut, SHUT_DOWN);
    load_out(&mut shut);
    let burned_shut_down =
        before - with_state::<PickupFacilityState, _>(&shut, |p, _| p.truck.fuel_gal);

    // Check-in plus loading is over an hour of engine time at roughly
    // 0.8 gallons an hour.
    assert!(burned_idling > 0.3, "{burned_idling}");
    assert_eq!(burned_shut_down, 0.0);
}

#[test]
fn test_the_load_report_names_the_fuel_burned_idling() {
    let mut app = TestApp::new();
    pickup_running(&mut app);
    load_out(&mut app);
    let loaded_line = app.main_lines().last().unwrap().to_lowercase();
    assert!(loaded_line.contains("idling"), "{loaded_line}");
    assert!(loaded_line.contains("gallon"), "{loaded_line}");
}

#[test]
fn test_a_shut_down_load_says_nothing_about_fuel() {
    let mut app = TestApp::new();
    pickup_running(&mut app);
    select::<PickupFacilityState>(&mut app, SHUT_DOWN);
    load_out(&mut app);
    let loaded_line = app.main_lines().last().unwrap().to_lowercase();
    assert!(!loaded_line.contains("idling"), "{loaded_line}");
}

/// Checked in with the engine running, then shut down for the load: the dock
/// hour burns nothing, and the report says the idling came before the
/// shutdown -- a bare "burned idling" after loading read as the engine having
/// run through the load (tester report, 2026-09-28).
#[test]
fn test_shutting_down_after_check_in_stops_the_idle_before_the_load() {
    let mut app = TestApp::new();
    pickup_running(&mut app);
    select::<PickupFacilityState>(&mut app, "Check in at shipping office");
    select::<PickupFacilityState>(&mut app, SHUT_DOWN);
    let before_load = with_state::<PickupFacilityState, _>(&app, |p, _| p.truck.fuel_gal);
    select::<PickupFacilityState>(&mut app, "Load cargo at dock");
    finish_timed_state(&mut app);

    let (engine_on, after_load) =
        with_state::<PickupFacilityState, _>(&app, |p, _| (p.truck.engine_on, p.truck.fuel_gal));
    assert!(!engine_on);
    assert_eq!(after_load, before_load);
    let loaded_line = app.main_lines().last().unwrap().to_lowercase();
    assert!(
        loaded_line.contains("burned idling before you shut down"),
        "{loaded_line}"
    );
}

#[test]
fn test_departing_with_the_engine_off_names_the_start_control() {
    let mut app = TestApp::new();
    pickup_running(&mut app);
    load_out(&mut app);
    select::<PickupFacilityState>(&mut app, SHUT_DOWN);
    select::<PickupFacilityState>(&mut app, "Depart for destination");

    assert!(is::<DrivingState>(&app));
    assert!(!with_state::<DrivingState, _>(&app, |d, _| d
        .truck()
        .engine_on));
    // The first-run tutorial and any achievement speak after departure, so
    // find the departure line rather than trusting the last thing said.
    let lines = app.main_lines();
    let departure = lines
        .iter()
        .find(|line| line.contains("Loaded trip is"))
        .expect("the departure line");
    // Never "Departing now" over a dead engine, and the key named is the one
    // this driver's settings actually bind.
    assert!(!departure.contains("Departing now"), "{departure}");
    assert!(
        departure.contains(&app.ctx.control_hint("engine")),
        "{departure}"
    );
}

#[test]
fn test_departing_with_the_engine_running_still_just_departs() {
    let mut app = TestApp::new();
    pickup_running(&mut app);
    load_out(&mut app);
    select::<PickupFacilityState>(&mut app, "Depart for destination");

    assert!(with_state::<DrivingState, _>(&app, |d, _| d
        .truck()
        .engine_on));
    let lines = app.main_lines();
    let departure = lines
        .iter()
        .find(|line| line.contains("Loaded trip is"))
        .expect("the departure line");
    assert!(departure.contains("Departing now"), "{departure}");
}

#[test]
fn test_pickup_status_and_screen_report_the_engine() {
    let mut app = TestApp::new();
    pickup_running(&mut app);
    select::<PickupFacilityState>(&mut app, "Pickup status");
    assert!(app
        .main_lines()
        .last()
        .unwrap()
        .to_lowercase()
        .contains("engine running"));
    assert!(app
        .visible_lines()
        .iter()
        .any(|line| line.contains("Engine: running")));

    select::<PickupFacilityState>(&mut app, SHUT_DOWN);
    select::<PickupFacilityState>(&mut app, "Pickup status");
    assert!(app
        .main_lines()
        .last()
        .unwrap()
        .to_lowercase()
        .contains("engine off"));
    assert!(app
        .visible_lines()
        .iter()
        .any(|line| line.contains("Engine: off")));
}

// -- the drivable pickup leg (tests/test_pickup_loading.py, drive half) ---------------
//
// The Python fixtures walked the whole new-career flow to reach the deadhead;
// here the assigned board is pushed directly and the assignment accepted,
// which is the same hand-off (`states::city::board` -> `launch_driving`).

/// `accept_pickup_drive`: take the assigned dispatch and end up on the
/// deadhead to the shipper.
fn accept_pickup_drive(app: &mut TestApp) {
    career(app, "Pickup Drive", "Chicago");
    profile_mut(app)
        .achievements
        .push("first_dispatch".to_string());
    let board = freight_fate::states::city::JobBoardState::new(
        &app.ctx,
        vec![drop_yard_job("chicago-live-load", 92.0)],
    );
    app.push_state(board);
    key(app, Key::Return); // accept assigned dispatch
    assert!(is::<DrivingState>(app), "the deadhead starts");
    assert_eq!(
        with_state::<DrivingState, _>(app, |d, _| d.phase.to_string()),
        freight_fate::states::driving_core::DRIVE_PHASE_PICKUP
    );
}

/// `arrive_at_pickup`: put the deadhead on the shipper's doorstep at
/// `speed_mps` and run one frame.
fn arrive_at_pickup(app: &mut TestApp, speed_mps: f64) {
    with_state_mut::<DrivingState, _>(app, |d, ctx| {
        d.trip.position_mi = d.trip.total_miles();
        d.trip.finished = true;
        d.trip.truck.velocity_mps = speed_mps;
        d.update_frame(ctx, 1.0 / 60.0);
    });
    app.ctx.run_deferred();
    if speed_mps <= 0.45 {
        finish_timed_state(app);
        assert!(is::<PickupFacilityState>(app));
    }
}

#[test]
fn test_accepting_job_starts_drivable_pickup_leg() {
    let mut app = TestApp::new();
    accept_pickup_drive(&mut app);

    assert!(is::<DrivingState>(&app));
    assert!(!is::<PickupFacilityState>(&app));
    let trip = profile(&app).active_trip.clone().expect("a saved trip");
    assert_eq!(trip["kind"], "pickup_drive");
    assert!(!trip["job"]["origin_facility_id"]
        .as_str()
        .unwrap()
        .is_empty());
    let (legs, miles, total, remaining) = with_state::<DrivingState, _>(&app, |d, _| {
        (
            d.trip.route.legs.len(),
            d.trip.route.miles(),
            d.trip.total_miles(),
            d.trip.remaining_miles(),
        )
    });
    let first_line = app.visible_lines()[0].clone();
    // A chain-capable yard deadheads on its real street chain (short but
    // multi-leg); facilities without one keep the 2-mile-plus fallback.
    if legs >= 2 {
        assert!(miles > 0.5);
    } else {
        assert!(miles > 2.0);
    }
    assert_eq!(total, miles);
    assert_eq!(remaining, miles);
    assert!(first_line.contains("Deadheading to pickup"), "{first_line}");
    let dispatch: Vec<String> = app
        .main_lines()
        .into_iter()
        .filter(|text| text.starts_with("Accepted."))
        .collect();
    assert!(!dispatch.is_empty());
    assert!(dispatch.last().unwrap().contains("Deadhead"));
}

#[test]
fn test_pickup_facility_waits_for_full_stop() {
    let mut app = TestApp::new();
    accept_pickup_drive(&mut app);

    // Python rolled in at 26.8 m/s; this facility's approach is a 15 mph
    // street, so that speed puts an overspeed warning after the cue. Any
    // speed above the gate's stop threshold exercises the same branch.
    arrive_at_pickup(&mut app, 6.0);
    assert!(is::<DrivingState>(&app));
    // Found by its own text rather than taken as the last line: 6 m/s is over
    // the corner's advise speed now that corners are priced from their angle,
    // so the corner call legitimately lands after this one. What this case is
    // about is what the pickup cue SAYS.
    let lines = app.event_lines();
    let last = lines
        .iter()
        .rev()
        .find(|line| line.contains("Pickup ahead"))
        .cloned()
        .unwrap_or_default();
    assert!(last.contains("Pickup ahead"), "{lines:?}");
    assert!(last.to_lowercase().contains("stop at the gate"), "{last}");

    // Inside the creep band (DELIVERY_PARK_MPH) but not stopped. The creep
    // cue is queued rather than spoken over the one before it, so give the
    // event channel a few frames to reach it.
    for _ in 0..30 {
        with_state_mut::<DrivingState, _>(&mut app, |d, ctx| {
            d.trip.truck.velocity_mps = 1.1;
            d.update_frame(ctx, 1.0 / 60.0);
        });
        app.ctx.run_deferred();
    }
    assert!(is::<DrivingState>(&app));
    let events = app.event_lines();
    assert!(
        events
            .iter()
            .any(|line| line.contains("Stop and set the parking brake.")),
        "{events:?}"
    );

    with_state_mut::<DrivingState, _>(&mut app, |d, ctx| {
        d.trip.truck.velocity_mps = 0.0;
        d.trip.truck.set_parking_brake();
        d.update_frame(ctx, 1.0 / 60.0);
    });
    app.ctx.run_deferred();
    assert!(
        app.visible_lines()[0].contains("Pulling into pickup"),
        "{:?}",
        app.visible_lines()[0]
    );
}

/// The pickup check-in gate must not freeze engine audio at whatever rev the
/// truck was carrying on the approach -- same defect class as the
/// already-fixed police-stop freeze.
#[test]
fn test_pickup_arrival_settles_the_engine_to_idle() {
    let mut app = TestApp::new();
    accept_pickup_drive(&mut app);
    arrive_at_pickup(&mut app, 6.0);

    let (rpm, idle, throttle) = with_state_mut::<DrivingState, _>(&mut app, |d, ctx| {
        d.trip.truck.velocity_mps = 0.0;
        // revs still up from the approach
        d.trip.truck.rpm = d.trip.truck.specs.max_rpm;
        d.update_frame(ctx, 1.0 / 60.0);
        (
            d.trip.truck.rpm,
            d.trip.truck.specs.idle_rpm,
            d.trip.truck.throttle,
        )
    });
    app.ctx.run_deferred();

    assert!(
        app.visible_lines()[0].contains("Pulling into pickup"),
        "{:?}",
        app.visible_lines()[0]
    );
    assert!((rpm - idle).abs() < 1e-6, "{rpm} vs idle {idle}");
    assert_eq!(throttle, 0.0);
}

// -- dispatch reads 511 before naming the lane ----------------------------------------

#[test]
fn test_dispatch_departs_by_the_route_511_construction_recommends() {
    // A company driver leaves the pickup on the route dispatch names. With
    // real traffic on, dispatch checks the state 511 construction reports
    // on each option first, says what it found, and drives the route its
    // ranking put first -- here the shortest way is reported closed.
    use std::sync::Arc;

    use ff_core::data::world::get_world;
    use ff_core::sim::real_traffic::{wall_time, RealTrafficProvider, TrafficData};
    use ff_core::sim::real_traffic_parsers::TrafficEvent;
    use ff_core::sim::route_roadwork::{
        choose_dispatch_route, dispatch_route_line, route_state_keys,
    };
    use ff_core::sim::trip_traffic::TrafficProvider;

    let mut app = TestApp::new();
    career(&mut app, "Roadwork Dispatch", "Chicago");
    let mut job = drop_yard_job("chicago-live-load", 92.0);
    job.origin_type = "mine_quarry".to_string();
    push_pickup(&mut app, job.clone(), PickupOptions::default());
    key(&mut app, Key::Return); // check in
    key(&mut app, Key::Return); // load cargo
    finish_timed_state(&mut app);
    assert_eq!(
        current_label::<PickupFacilityState>(&app),
        "Depart for destination"
    );

    let world = get_world();
    let routes = world
        .supported_route_options(&job.origin, &job.destination, 3)
        .expect("routes to the destination");
    let leg = &routes[0].legs[0];
    let points = leg.route_points();
    let point = &points[points.len() / 2];
    let state = route_state_keys(&routes[0])
        .first()
        .cloned()
        .expect("the first leg names its state");
    let mut event = TrafficEvent::new("closed", "construction", "high", "Roadwork", "Test");
    event.latitude = Some(point.lat);
    event.longitude = Some(point.lon);
    event.closure = "full closure".to_string();
    event.work_type = "construction".to_string();
    let provider = Arc::new(RealTrafficProvider::offline());
    let now = wall_time();
    provider.seed_cache(
        &format!("{state}:construction"),
        TrafficData::new(&state, vec![event], now, now, "test"),
    );
    app.ctx.settings.real_traffic = true;
    app.ctx.set_real_traffic_provider(Arc::clone(&provider));
    let routing = choose_dispatch_route(
        &routes,
        Some(&*provider as &dyn TrafficProvider),
        None,
        world,
    );
    let expected = dispatch_route_line(&routes, &routing, world, &app.ctx.settings);
    assert!(
        expected.contains("the road closed on ") || expected.starts_with("The road is closed on "),
        "{expected}"
    );

    app.clear_speech();
    key(&mut app, Key::Return); // depart for destination
    assert!(is::<DrivingState>(&app));
    let departure = app
        .main_lines()
        .into_iter()
        .rev()
        .find(|text| text.contains("Dispatch routed you to"))
        .expect("a departure line");
    assert!(departure.contains(&expected), "{departure}");
    let driven = with_state::<DrivingState, _>(&app, |d, _| d.trip.route.cities.clone());
    assert_eq!(driven, routes[routing.pick()].cities);
}

#[test]
fn test_dispatch_departure_is_unchanged_with_real_traffic_off() {
    // No feeds, no construction talk, the shortest route: the day-one path.
    let mut app = TestApp::new();
    career(&mut app, "Quiet Dispatch", "Chicago");
    let mut job = drop_yard_job("chicago-live-load", 92.0);
    job.origin_type = "mine_quarry".to_string();
    push_pickup(&mut app, job.clone(), PickupOptions::default());
    key(&mut app, Key::Return);
    key(&mut app, Key::Return);
    finish_timed_state(&mut app);
    assert!(!app.ctx.settings.real_traffic);
    app.clear_speech();
    key(&mut app, Key::Return);
    assert!(is::<DrivingState>(&app));
    let departure = app
        .main_lines()
        .into_iter()
        .rev()
        .find(|text| text.contains("Dispatch routed you to"))
        .expect("a departure line");
    assert!(!departure.contains("Construction"), "{departure}");
    assert!(!departure.contains("road is closed"), "{departure}");
    let routes = ff_core::data::world::get_world()
        .supported_route_options(&job.origin, &job.destination, 3)
        .expect("routes");
    let driven = with_state::<DrivingState, _>(&app, |d, _| d.trip.route.cities.clone());
    assert_eq!(driven, routes[0].cities);
}
