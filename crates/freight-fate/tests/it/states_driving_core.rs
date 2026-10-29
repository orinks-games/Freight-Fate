//! The driving prelude and the `DrivingState` facade
//! (`states/driving_core.rs`, `states/driving.rs`).
//!
//! The pure helpers are the Python tests that imported `driving_core`
//! directly: `test_cruise_steps.py` (the +/- grid), `test_stop_detail.py`
//! (the spoken arrival grace), `test_driving_damage_bands.py` (the road
//! shop's severity curve), `test_ramp_controls.py` (the freeway `via`
//! pattern) and `test_amenities.py` (what a stop offers). The rest is the
//! facade itself: a real route builds, enters and ticks headless.

use ff_core::data::world::get_world;
use ff_core::models::economy::damage_severity_mult;
use ff_core::models::jobs::make_reposition_job;
use ff_core::models::profile::Profile;
use ff_core::sim::enforcement_posts::EnforcementPost;
use ff_core::sim::trip_models::RoadStop;

use freight_fate::app::testing::TestApp;
use freight_fate::states::base::State;
use freight_fate::states::driving::{DrivingState, ACTIVE_TRIP_DEADLINE_MODEL};
use freight_fate::states::driving_core::*;

// -- the pure helpers -------------------------------------------------------------

#[test]
fn test_off_grid_snaps_up_to_the_next_five() {
    assert_eq!(cruise_step_target(32.0, 1, false), 35.0);
}

#[test]
fn test_on_grid_steps_a_full_five_up() {
    assert_eq!(cruise_step_target(35.0, 1, false), 40.0);
}

#[test]
fn test_off_grid_snaps_down_to_the_previous_five() {
    assert_eq!(cruise_step_target(32.0, -1, false), 30.0);
}

#[test]
fn test_on_grid_steps_a_full_five_down() {
    assert_eq!(cruise_step_target(30.0, -1, false), 25.0);
}

#[test]
fn test_float_fuzz_on_the_grid_still_moves_a_full_step() {
    // A hair either side of a multiple is the same press, not a no-op snap.
    assert_eq!(cruise_step_target(35.0 - 1e-9, 1, false), 40.0);
    assert_eq!(cruise_step_target(35.0 + 1e-9, -1, false), 30.0);
}

#[test]
fn test_fine_steps_move_by_exactly_one() {
    assert_eq!(cruise_step_target(35.0, 1, true), 36.0);
    assert_eq!(cruise_step_target(35.0, -1, true), 34.0);
    assert_eq!(cruise_step_target(32.0, 1, true), 33.0);
}

#[test]
fn test_both_step_kinds_clamp_to_the_bounds() {
    assert_eq!(cruise_step_target(CRUISE_MAX_MPH, 1, false), CRUISE_MAX_MPH);
    assert_eq!(cruise_step_target(CRUISE_MAX_MPH, 1, true), CRUISE_MAX_MPH);
    assert_eq!(
        cruise_step_target(CRUISE_MIN_MPH, -1, false),
        CRUISE_MIN_MPH
    );
    assert_eq!(cruise_step_target(CRUISE_MIN_MPH, -1, true), CRUISE_MIN_MPH);
}

#[test]
fn test_rest_stop_arrival_grace_scales_for_slow_long_speech() {
    let short = ramp_arrival_grace_seconds("At Rest Area. Stop now.", 0.5);
    let long = ramp_arrival_grace_seconds(
        "You are at the Northbound Welcome Center and Scenic Travel Plaza. \
         Come to a complete stop.",
        0.5,
    );
    let slowest = ramp_arrival_grace_seconds("At Rest Area. Stop now.", 0.0);
    let fastest = ramp_arrival_grace_seconds("At Rest Area. Stop now.", 1.0);

    assert!(short >= RAMP_ARRIVAL_GRACE_MIN_S);
    assert!(long > short);
    assert!(slowest > fastest);
}

#[test]
fn test_road_shops_share_the_garage_severity_curve() {
    let cost = road_repair_cost(80.0, FIELD_REPAIR_DAMAGE_PCT, MECHANIC_CALLOUT_FEE);
    let flat = MECHANIC_CALLOUT_FEE + (80.0 - FIELD_REPAIR_DAMAGE_PCT) * MECHANIC_RATE_PER_PCT;
    assert!(cost > flat);
    assert!(damage_severity_mult(80.0) > damage_severity_mult(20.0));
    assert!(damage_severity_mult(20.0) > 1.0);
}

#[test]
fn test_only_a_real_interstate_reads_as_a_freeway() {
    for (via, is_freeway) in [
        ("I 20 WEST;I 59 SOUTH", true),
        ("I-70", true),
        ("I 65 NORTH", true),
        ("US 31 SOUTH;US 280", false),
        ("AL 75 NORTH", false),
        ("STATE ROUTE 3", false),
        ("", false),
        // The boundary that a missing \b would swallow: these end in "I"
        // followed by a number and are not interstates.
        ("HAWAII 1", false),
        ("MISSISSIPPI 3", false),
    ] {
        assert_eq!(freeway_via_matches(via), is_freeway, "via {via:?}");
    }
}

#[test]
fn test_the_pattern_carries_a_word_boundary() {
    assert!(FREEWAY_VIA_PATTERN.starts_with("\\b"));
}

#[test]
fn test_poi_offers_text_surfaces_brand_specialty() {
    // Integration: the driving route-info menu speaks each stop through this.
    let mut stop = RoadStop::new("Love's Travel Stop #472", 42.0, "travel_center");
    stop.actions = ["fuel", "food", "park"].map(str::to_string).to_vec();
    stop.services = ["diesel", "food", "parking"].map(str::to_string).to_vec();
    stop.parking = "confirmed".to_string();
    let text = poi_offers_text(&stop);
    assert!(text.contains("listed services"), "{text}");
    assert!(
        text.contains("Love's specialty: tire care and quick lube"),
        "{text}"
    );

    let mut generic = RoadStop::new("Downtown Fuel Mart", 10.0, "travel_center");
    generic.services = vec!["diesel".to_string()];
    assert!(!poi_offers_text(&generic).contains("specialty"));
}

#[test]
fn the_closest_following_gap_still_clears_a_tailgating_ticket() {
    // The closest cushion the game offers must never be a setting that gets
    // the driver ticketed for choosing it.
    let closest = ACC_GAP_CHOICES
        .iter()
        .map(|(_, seconds)| *seconds)
        .fold(f64::INFINITY, f64::min);
    assert!(closest > ff_core::sim::enforcement_observe::TAILGATE_GAP_S);
    assert!(acc_gap_seconds(ACC_GAP_DEFAULT).is_some());
}

#[test]
fn the_overspeed_alert_arms_between_the_acc_pace_and_the_trooper() {
    // A driver must never become ticketable in silence, and the dash must
    // never chime at a speed the game's own automation picked.
    const { assert!(OVERSPEED_WARN_MPH > ACC_LIMIT_OFFSET_MPH) };
    const { assert!(OVERSPEED_WARN_MPH < SPEEDING_LEEWAY_MPH) };
}

#[test]
fn the_enforcement_signature_is_reproducible() {
    assert_eq!(enforcement_signature_wav(), enforcement_signature_wav());
    assert!(!enforcement_signature_wav().is_empty());
}

// -- the facade -------------------------------------------------------------------

/// A drive on a real corridor at `trip_seed = 0`, the transcript suite's seed.
fn a_real_drive(app: &mut TestApp) -> DrivingState {
    let world = get_world();
    let mut profile = Profile::named_in("Prelude", "Denver");
    // Past the walkthrough: the first-run Tutorial is its own test's business.
    profile.tutorial_done = true;
    app.ctx.profile = Some(profile);
    let job = make_reposition_job(world, "Denver", "Cheyenne", false, None)
        .expect("Denver to Cheyenne is a supported reposition");
    let route = world
        .shortest_route("Denver", "Cheyenne", None, false)
        .expect("the world routes")
        .expect("Denver to Cheyenne has a route");
    DrivingState::new(
        &mut app.ctx,
        job,
        route,
        Some(0),
        DRIVE_PHASE_DELIVERY,
        Some(12.0),
    )
}

#[test]
fn a_drive_builds_on_a_real_route_at_seed_zero() {
    let mut app = TestApp::new();
    let drive = a_real_drive(&mut app);

    assert_eq!(drive.trip_seed, 0);
    assert_eq!(drive.phase, DRIVE_PHASE_DELIVERY);
    assert!(!drive.resumed);
    assert!(drive.trip.total_miles() > 0.0);
    assert!(!drive.route.cities.is_empty());
    // The truck is parked, cold and braked, exactly as the cab is handed over.
    assert!(!drive.truck().engine_on);
    assert!(drive.truck().parking_brake);
    assert!(!drive.truck().air_ready());
    // A drive paces the main channel; menus do not.
    assert!(drive.paces_main_speech());
    // The delivery run drops the trip's legacy arrival zones: the destination
    // exit and the gate own those speeds now.
    assert!(!drive
        .trip
        .zones
        .iter()
        .any(|zone| zone.reason == "destination approach" || zone.reason == "facility gate"));
    // The drive is its own radio's owner, and the status line is the first
    // thing the window shows.
    assert!(drive.radio().is_some());
    assert!(drive.status_text.contains("start the engine"));
}

#[test]
fn entering_the_drive_speaks_the_objective() {
    let mut app = TestApp::new();
    let mut drive = a_real_drive(&mut app);
    app.clear_speech();

    drive.enter(&mut app.ctx);

    let lines = app.main_lines();
    assert!(!lines.is_empty(), "entering a drive says something");
    let spoken = lines.join(" ");
    assert!(spoken.contains("At the wheel"), "{spoken}");
    assert!(spoken.contains("It is "), "{spoken}");
    // Entering again is silent: a menu popping back to the road must not
    // re-read the whole objective.
    app.clear_speech();
    drive.enter(&mut app.ctx);
    assert!(app.main_lines().is_empty());
}

#[test]
fn sixty_frames_of_the_drive_run_without_panicking() {
    let mut app = TestApp::new();
    let mut drive = a_real_drive(&mut app);
    drive.enter(&mut app.ctx);

    for _ in 0..60 {
        drive.update(&mut app.ctx, 1.0 / 60.0);
    }

    // The facade's own reads keep answering mid-drive.
    assert!(drive.absolute_game_hour(&app.ctx, None) >= 0.0);
    assert!(!drive.logbook_location(&app.ctx).is_empty());
    assert!(!drive.parked_entry_status().is_empty());
}

/// flight, 2026-09-22: descend in Real time to gain speed, switch back to
/// Standard, and coast for miles on no engine. The pause menu's pacing
/// change now waits until the truck is stopped, so a hill's speed is spent
/// on the clock that gave it.
#[test]
fn a_pacing_change_while_rolling_waits_for_the_truck_to_stop() {
    let mut app = TestApp::new();
    app.ctx.settings.time_scale = 1.0;
    let mut drive = a_real_drive(&mut app);
    drive.enter(&mut app.ctx);
    drive.trip.truck.velocity_mps = 25.0;

    app.ctx.settings.time_scale = 20.0;
    for _ in 0..30 {
        drive.update(&mut app.ctx, 1.0 / 60.0);
    }
    assert!(drive.trip.truck.speed_mph() > 1.0);
    assert_eq!(drive.trip.time_scale, 1.0, "rolling keeps the old pacing");

    drive.trip.truck.velocity_mps = 0.0;
    drive.update(&mut app.ctx, 1.0 / 60.0);
    assert_eq!(drive.trip.time_scale, 20.0, "stopped takes the new pacing");
}

#[test]
fn a_snapshot_round_trips_the_drive() {
    let mut app = TestApp::new();
    let mut drive = a_real_drive(&mut app);
    drive.trip.game_minutes = 42.0;
    drive.gate_miss_count = 2;
    drive.record_events.push("Reputation up.".to_string());

    let data = drive.snapshot(&app.ctx);
    assert_eq!(data["trip_seed"], 0);
    assert_eq!(data["kind"], "delivery");
    assert_eq!(data["deadline_model"], ACTIVE_TRIP_DEADLINE_MODEL);

    let resumed = DrivingState::from_snapshot(&mut app.ctx, &data).expect("the snapshot resumes");
    assert!(resumed.resumed);
    assert_eq!(resumed.trip_seed, 0);
    assert_eq!(resumed.gate_miss_count, 2);
    assert_eq!(resumed.record_events, vec!["Reputation up.".to_string()]);
    assert_eq!(resumed.phase, DRIVE_PHASE_DELIVERY);
}

/// Reported by a player: resuming a saved trip played police sirens four
/// times over, at once. The enforcement watch measured its first frame from
/// mile zero, so every trooper already behind the truck counted as just
/// passed and each one played its siren, pass and marker together.
#[test]
fn a_resumed_trip_does_not_replay_the_troopers_it_already_passed() {
    let mut app = TestApp::new();
    let world = get_world();
    let mut profile = Profile::named_in("Resume Troopers", "Denver");
    profile.tutorial_done = true;
    app.ctx.profile = Some(profile);
    let job = make_reposition_job(world, "Denver", "Salt Lake City", false, None)
        .expect("Denver to Salt Lake City is supported");
    let route = world
        .shortest_route("Denver", "Salt Lake City", None, false)
        .expect("the world routes")
        .expect("Denver to Salt Lake City has a route");
    let mut drive = DrivingState::new(
        &mut app.ctx,
        job,
        route,
        Some(7),
        DRIVE_PHASE_DELIVERY,
        Some(7.0),
    );
    drive.trip.position_mi = 348.0;
    let data = drive.snapshot(&app.ctx);

    let mut resumed =
        DrivingState::from_snapshot(&mut app.ctx, &data).expect("the snapshot resumes");
    let behind = |post: &EnforcementPost| post.staffed && post.end_mi() < 348.0;
    assert!(
        resumed
            .trip
            .posts
            .iter()
            .filter(|post| behind(post))
            .count()
            >= 2
            && resumed
                .trip
                .posts
                .iter()
                .any(|post| behind(post) && post.tableau),
        "this road needs staffed troopers, one working a stop, behind the resume point"
    );
    let log = app.record_audio();
    resumed.update_enforcement_watch(&mut app.ctx, 1.0 / 60.0);
    resumed.service_pending_sounds(&mut app.ctx, 1.0);

    let played: Vec<String> = log
        .borrow()
        .played
        .iter()
        .map(|(key, _, _)| key.clone())
        .collect();
    for key in [
        "events/police_siren",
        "traffic/trooper_pass",
        "enforcement/signature",
    ] {
        assert!(
            !played.iter().any(|played| played == key),
            "{key} played for troopers behind the resume point: {played:?}"
        );
    }
}

#[test]
fn a_real_time_snapshot_restores_the_spoken_calendar_hour() {
    let mut app = TestApp::new();
    app.ctx.settings.time_scale = 1.0;
    let world = get_world();
    app.ctx.profile = Some(Profile::named_in("Resume Clock", "Atlanta"));
    let job = make_reposition_job(world, "Atlanta", "Dallas", false, None)
        .expect("Atlanta to Dallas is supported");
    let route = world
        .shortest_route("Atlanta", "Dallas", None, false)
        .expect("the world routes")
        .expect("Atlanta to Dallas has a route");
    let mut drive = DrivingState::new(
        &mut app.ctx,
        job,
        route,
        Some(0),
        DRIVE_PHASE_DELIVERY,
        Some(12.0),
    );
    let departure_calendar = 100.0 * 24.0 + 22.5;
    app.ctx
        .profile
        .as_mut()
        .unwrap()
        .sync_calendar_to(departure_calendar);
    let crossing = drive
        .trip
        .timezone_crossings
        .first()
        .expect("the route crosses a timezone")
        .at_mi;
    drive.trip.restore(crossing + 5.0, 180.0);
    assert_ne!(drive.trip.current_timezone(), drive.trip.start_timezone);
    drive.trip.start_hour = (22.5 - drive.trip.current_timezone().offset_h).rem_euclid(24.0);
    let data = drive.snapshot(&app.ctx);

    let saved = app.ctx.profile.as_ref().unwrap().to_dict();
    app.ctx.profile = Some(Profile::from_dict(&saved));
    let resumed = DrivingState::from_snapshot(&mut app.ctx, &data).expect("the snapshot resumes");

    let spoken_calendar =
        app.ctx.profile.as_ref().unwrap().calendar_game_hours() + resumed.trip.game_minutes / 60.0;
    assert!((spoken_calendar - (departure_calendar + 3.0)).abs() < 1e-9);
}
