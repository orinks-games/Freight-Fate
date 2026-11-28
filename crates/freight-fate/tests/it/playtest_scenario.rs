//! The agent server's `scenario` tool (`playtest::scenario`): any situation
//! for the sandbox career, applied in place, checked before anything moves.

use serde_json::{json, Map, Value};

use ff_core::models::business_constants::{COMPANY_DRIVER, LEASED_OWNER_OPERATOR};
use ff_core::models::profile::Profile;

use freight_fate::app::testing::TestApp;
use freight_fate::playtest::road::{build_driving, find_feature, RoadOptions};
use freight_fate::playtest::scenario::{apply, Scenario};

fn scenario(json: Value) -> Scenario {
    let Value::Object(args) = json else {
        panic!("an object")
    };
    Scenario::from_json(&args).expect("a valid scenario")
}

/// `came_from` writes the lane memory as one delivery into the parked
/// city, so the board that opens next is the one a driver who just made
/// that run would see.
#[test]
fn came_from_writes_the_lane_the_truck_just_ran() {
    let mut app = TestApp::new();
    apply(
        &mut app.ctx,
        &scenario(json!({"city": "Milwaukee", "came_from": "Chicago"})),
    )
    .expect("the scenario applies");
    let p = app.ctx.profile.as_ref().expect("a career");
    assert_eq!(
        p.recent_lanes,
        vec!["chicago_il_us:milwaukee_wi_us".to_string()]
    );
    assert!(p.dispatch_board_cache.is_none());
    let refused = apply(
        &mut app.ctx,
        &scenario(json!({"came_from": "Nowhere Special"})),
    );
    assert_eq!(refused, Err("no city called Nowhere Special".to_string()));
}

#[test]
fn test_a_scenario_creates_a_bench_career_and_sets_every_part_asked_for() {
    let mut app = TestApp::new();
    assert!(app.ctx.profile.is_none());
    let notes = apply(
        &mut app.ctx,
        &scenario(json!({
            "city": "Tonopah",
            "level": 5,
            "deliveries": 12,
            "money": 42_000,
            "reputation": 70,
            "business": "leased",
            "endorsements": ["hazmat"],
            "fuel_pct": 25,
            "damage_pct": 10,
            "rested": true,
            "hos_driving_min": 300,
            "hos_duty_min": 340,
            "hos_since_break_min": 300,
            "market_seed": 99,
            "board_seed": 3,
            "settings": {"real_traffic": true, "time_scale": 1.0}
        })),
    )
    .expect("the scenario applies");
    let world = app.ctx.world;
    let p = app.ctx.profile.as_ref().expect("a career was created");
    assert_eq!(p.name, "Playtest");
    assert_eq!(p.current_city, world.resolve_city_key("Tonopah"));
    assert_eq!(p.career.level(), 5);
    assert_eq!(p.career.deliveries, 12);
    assert_eq!(p.money(), 42_000.0);
    assert_eq!(p.career.reputation, 70.0);
    assert_eq!(p.business_status, LEASED_OWNER_OPERATOR);
    assert!(p.career.endorsements().contains("hazmat"));
    assert!(p.tutorial_done);
    assert_eq!(p.market.seed, 99);
    assert_eq!(app.ctx.dispatch_board_seed, Some(3));
    assert!(app.ctx.settings.real_traffic);
    assert_eq!(app.ctx.settings.time_scale, 1.0);
    assert_eq!(p.hos.driving_min, 300.0);
    assert_eq!(p.hos.duty_min, 340.0);
    assert_eq!(p.hos.since_break_min, 300.0);
    let mut truck = ff_core::sim::vehicle::TruckState::new(p.truck_specs());
    p.load_truck_condition(&mut truck);
    assert!((truck.fuel_gal - truck.specs.fuel_tank_gal * 0.25).abs() < 0.01);
    assert_eq!(truck.damage_pct, 10.0);
    assert!(p.dispatch_board_cache.is_none());
    let said = notes.join(" ");
    assert!(said.contains("Level 5."), "{said}");
    assert!(said.contains("Fuel 25 percent."), "{said}");
    assert!(said.contains("real_traffic"), "{said}");
    assert!(
        said.contains("HOS since-break counter: 300 minutes"),
        "{said}"
    );
}

/// The clock lever rests a driver who waits ten hours or more, so the
/// fatigue asked for is applied after it and is what the scenario ends
/// with (agent playtest, 2026-11-13: 95 was announced, then the terminal
/// said "Already rested").
#[test]
fn fatigue_asked_for_with_an_hour_sticks() {
    let mut app = TestApp::new();
    apply(&mut app.ctx, &scenario(json!({"city": "Omaha"}))).expect("a career");
    let notes = apply(&mut app.ctx, &scenario(json!({"hour": 2.0, "fatigue": 95})))
        .expect("the scenario applies");
    assert_eq!(app.ctx.profile.as_ref().expect("a career").fatigue, 95.0);
    assert_eq!(
        notes.last().map(String::as_str),
        Some("Fatigue 95 percent, severe."),
        "{notes:?}"
    );
}

#[test]
fn test_a_move_with_a_load_in_progress_needs_clear_load() {
    let mut app = TestApp::new();
    let mut p = Profile::named_in("Loaded", "Chicago");
    p.active_trip = Some(Value::Object(Map::new()));
    app.ctx.profile = Some(p);
    let refused = apply(&mut app.ctx, &scenario(json!({"city": "Denver"})))
        .expect_err("a move over a live load is refused");
    assert!(refused.contains("clear_load"), "{refused}");
    let notes = apply(
        &mut app.ctx,
        &scenario(json!({"city": "Denver", "clear_load": true, "business": "company"})),
    )
    .expect("dropping the load first is allowed");
    let p = app.ctx.profile.as_ref().unwrap();
    assert!(p.active_trip.is_none());
    assert_eq!(p.current_city, app.ctx.world.resolve_city_key("Denver"));
    assert_eq!(p.business_status, COMPANY_DRIVER);
    assert!(notes.iter().any(|n| n.contains("dropped")), "{notes:?}");
}

#[test]
fn test_unknown_places_and_settings_are_refused_by_name() {
    let mut app = TestApp::new();
    let refused =
        apply(&mut app.ctx, &scenario(json!({"city": "Atlantis"}))).expect_err("no such city");
    assert!(refused.contains("Atlantis"), "{refused}");
    app.ctx.profile = Some(Profile::named_in("Settings", "Chicago"));
    let refused = apply(
        &mut app.ctx,
        &scenario(json!({"settings": {"warp_drive": true}})),
    )
    .expect_err("no such setting");
    assert!(refused.contains("warp_drive"), "{refused}");
}

#[test]
fn test_staging_leaves_the_title_screen_under_the_terminal_and_the_game_running() {
    // The first live scenario emptied the whole screen stack before pushing
    // the terminal, and an empty stack is how the loop knows the game is
    // over: the game quit on the frame it was staged (2026-09-12).
    let mut app = TestApp::new();
    let staged = scenario(json!({"city": "Tonopah", "level": 3, "business": "company"}));
    let mut depth: Option<(usize, bool)> = None;
    let mut text = String::new();
    app.run_with_player_input(Some(2), |input, _dt| {
        if depth.is_none() {
            text = input.stage_scenario(&staged).expect("the scenario stages");
            depth = Some(input.screen_depth());
        }
        true
    });
    assert_eq!(
        depth,
        Some((2, true)),
        "title screen, then the terminal, game running"
    );
    assert!(text.contains("Tonopah, Nevada terminal"), "{text}");
    assert!(text.contains("level 3"), "{text}");
}

#[test]
fn road_staging_keeps_the_sandbox_driver_hours_and_fatigue() {
    let mut app = TestApp::new();
    apply(
        &mut app.ctx,
        &scenario(json!({"city": "Buffalo", "rested": true})),
    )
    .unwrap();
    let p = app.ctx.profile.as_mut().unwrap();
    p.hos.on_duty(760.0);
    p.hos.drive(20.0);
    p.fatigue = 67.0;

    let opts = RoadOptions {
        feature: "stop".to_string(),
        trip_seed: Some(7),
        ..Default::default()
    };
    let pairs = vec![("Buffalo".to_string(), "Albany".to_string())];
    let hit = find_feature(app.ctx.world, &pairs, "stop", &opts, Some(7))
        .into_iter()
        .next()
        .expect("a stop on the test corridor");
    let _ = build_driving(&mut app.ctx, &hit, &opts);
    let staged = app.ctx.profile.as_ref().unwrap();
    assert_eq!(staged.hos.duty_min, 780.0);
    assert_eq!(staged.hos.driving_min, 20.0);
    assert_eq!(staged.fatigue, 67.0);
}

#[test]
fn road_staging_keeps_the_sandbox_driver_cash() {
    let mut app = TestApp::new();
    apply(
        &mut app.ctx,
        &scenario(json!({"city": "Buffalo", "money": 150})),
    )
    .unwrap();
    let opts = RoadOptions {
        feature: "stop".to_string(),
        trip_seed: Some(7),
        ..Default::default()
    };
    let pairs = vec![("Buffalo".to_string(), "Albany".to_string())];
    let hit = find_feature(app.ctx.world, &pairs, "stop", &opts, Some(7))
        .into_iter()
        .next()
        .expect("a stop on the test corridor");
    let _ = build_driving(&mut app.ctx, &hit, &opts);
    assert_eq!(app.ctx.profile.as_ref().unwrap().money(), 150.0);
}

#[test]
fn road_staging_moves_the_career_clock_to_the_staged_hour() {
    // A staged drive's clock was its own hour while the career clock stayed
    // where the scenario left it, so what the drive read from the calendar
    // could be a day off (2026-11-20). The career clock now moves to the
    // staged hour on its own day.
    let mut app = TestApp::new();
    apply(
        &mut app.ctx,
        &scenario(json!({"city": "Buffalo", "rested": true, "hour": 6})),
    )
    .unwrap();
    let career = app.ctx.profile.as_ref().unwrap().game_hours;
    let pairs = vec![("Buffalo".to_string(), "Albany".to_string())];
    let base = RoadOptions {
        feature: "stop".to_string(),
        trip_seed: Some(7),
        ..Default::default()
    };
    let hit = find_feature(app.ctx.world, &pairs, "stop", &base, Some(7))
        .into_iter()
        .next()
        .expect("a stop on the test corridor");
    for hour in [2.0, 13.0, 23.0] {
        let opts = RoadOptions {
            feature: "stop".to_string(),
            trip_seed: Some(7),
            hour: Some(hour),
            ..Default::default()
        };
        let (drive, _) = build_driving(&mut app.ctx, &hit, &opts);
        let staged = drive.trip.career_hours.expect("a career clock");
        assert_eq!(staged.rem_euclid(24.0), drive.trip.start_hour, "{hour}");
        assert_eq!(
            (staged / 24.0).floor(),
            (career / 24.0).floor(),
            "the career's own day at {hour}"
        );
    }
}

/// `board_load_to` puts a load for a named facility on the board: here the
/// southbound trailer for a Laredo transfer yard, its Mexican consignee
/// named on the board row.
#[test]
fn board_load_to_adds_a_southbound_yard_load_to_the_board() {
    use ff_core::models::border_freight::southbound_consignee;
    use freight_fate::states::city::open_freight_market;

    let mut app = TestApp::new();
    apply(
        &mut app.ctx,
        &scenario(json!({
            "city": "San Antonio",
            "level": 4,
            "board_seed": 1,
            "board_load_to": "SCI Cross Dock",
        })),
    )
    .expect("the scenario applies");
    let jobs = open_freight_market(&mut app.ctx);
    let last = jobs.last().expect("a board");
    assert_eq!(
        last.destination_facility_id,
        "laredo-tx-us:cross_dock:sci-cross-dock"
    );
    assert!(southbound_consignee(last).is_some(), "{last:?}");
}

/// `hos_cycle_min` puts a working week on the cycle ledger, so a drive can
/// start near the 70-hour limit or out of it.
#[test]
fn hos_cycle_min_stages_the_working_week() {
    let mut app = TestApp::new();
    let notes = apply(&mut app.ctx, &scenario(json!({"hos_cycle_min": 4140})))
        .expect("the scenario applies");
    assert!(notes
        .iter()
        .any(|n| n == "HOS cycle: 4140 minutes on duty."));
    let p = app.ctx.profile.as_ref().expect("a career");
    assert_eq!(p.hos.cycle_used_min(), 4140.0);
    assert_eq!(p.hos.cycle_remaining_min("realistic"), Some(60.0));
    assert_eq!(p.hos.restart_remaining_min(), 34.0 * 60.0);
}

/// `hos_restart_off_min` stages rest already taken toward the cycle restart,
/// on top of the week `hos_cycle_min` puts on the ledger.
#[test]
fn hos_restart_off_min_stages_rest_toward_the_restart() {
    let mut app = TestApp::new();
    let notes = apply(
        &mut app.ctx,
        &scenario(json!({"hos_cycle_min": 4140, "hos_restart_off_min": 1500})),
    )
    .expect("the scenario applies");
    assert!(notes
        .iter()
        .any(|n| n == "HOS restart: 1500 minutes off in a row."));
    let p = app.ctx.profile.as_ref().expect("a career");
    assert_eq!(p.hos.restart_remaining_min(), 34.0 * 60.0 - 1500.0);
}
