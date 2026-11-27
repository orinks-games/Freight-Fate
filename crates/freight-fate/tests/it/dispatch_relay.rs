//! Accepting a load dispatch relayed from a nearby city (`jobs::relay`,
//! `JobBoardState::accept_relay`): the deadhead corridor and the shipper's
//! approach are one pickup drive, the offer says the deadhead comes first,
//! and a save on the way rebuilds the same road.

use ff_core::models::jobs::{route_drive_hours, Job, CARGO_CATALOG};
use ff_core::models::market::Market;
use ff_core::sim::timezones::{appointment_text, city_zone, EASTERN};

use freight_fate::app::testing::TestApp;
use freight_fate::states::base::{Key, State};
use freight_fate::states::city::{
    describe_job, dispatch_cache_key, relay_load_for_board, JobBoardState, JobDetailState,
    DRIVE_PHASE_PICKUP,
};
use freight_fate::states::city_pickup::{PICKUP_CHECK_IN_MIN, PICKUP_LOADING_MIN};
use freight_fate::states::driving::DrivingState;

use crate::states_city_support::*;

/// The freight market both tests run on. A new career's market seed is
/// random, and about one seed in two thousand leaves none of these towns a
/// relay that fits the rookie deadhead rule; the scheduled snapshot of
/// 2026-11-09 drew one. Pinned, the board is the same on every run.
const MARKET_SEED: i64 = 12345;

/// A company driver one delivery in, parked in `city`, and the load
/// dispatch relays onto an empty board there.
fn relayed_load(app: &mut TestApp, city: &str) -> Option<Job> {
    career(app, "Relay Driver", city);
    let p = app.ctx.profile.as_mut().expect("a career");
    p.career.deliveries = 1;
    p.market = Market::with_seed(MARKET_SEED);
    let key = dispatch_cache_key(profile(app));
    relay_load_for_board(&app.ctx, &key, &[])
}

#[test]
fn test_accepting_a_relayed_load_drives_the_deadhead_and_the_approach_as_one_pickup() {
    let mut app = TestApp::new();
    let job = relayed_load(&mut app, "Sherman").expect("Sherman relays a load off an empty board");
    let world = app.ctx.world;
    let here = world.resolve_city_key(&profile(&app).current_city);
    let origin = world.resolve_city_key(&job.origin);
    assert_ne!(origin, here);

    // The offer says the deadhead comes first.
    let text = describe_job(&app.ctx, 1, &job, Some(1));
    assert!(text.contains(" empty to the pickup first. "), "{text}");

    let mut board = JobBoardState::new(&app.ctx, vec![job.clone()]);
    app.clear_speech();
    board.accept(&mut app.ctx, 0);
    app.ctx.run_deferred();
    assert!(is::<DrivingState>(&app), "the accept launched a drive");
    let announced = app.main_lines().join(" ");
    assert!(announced.starts_with("Accepted."), "{announced}");
    assert!(announced.contains(" empty to "), "{announced}");

    let (phase, cities) = with_state::<DrivingState, _>(&app, |d, _| {
        (d.phase.to_string(), d.trip.route.cities.clone())
    });
    assert_eq!(phase, DRIVE_PHASE_PICKUP);
    // Corridor from here to the shipper's city, then that city's approach:
    // the city appears at the join and again at the very end.
    assert_eq!(cities.first(), Some(&here), "{cities:?}");
    assert_eq!(cities.last(), Some(&origin), "{cities:?}");
    assert!(cities.len() > 2, "{cities:?}");
    if world
        .facility_approach_route(&origin, &job.origin_location)
        .is_ok()
    {
        assert_eq!(cities[cities.len() - 2], origin, "{cities:?}");
    }

    // A save mid-deadhead rebuilds the same road.
    let snapshot = with_state::<DrivingState, _>(&app, |d, ctx| d.snapshot(ctx));
    assert_eq!(snapshot["route_kind"], "deadhead_approach");
    let resumed = DrivingState::from_snapshot(&mut app.ctx, &snapshot).expect("resumes");
    assert_eq!(resumed.trip.route.cities, cities);
    assert_eq!(resumed.phase.to_string(), DRIVE_PHASE_PICKUP);
}

/// The delivery clock starts at the shipper, so the detail screen's "deliver
/// by" time counts the deadhead, check-in and loading ahead of the deadline.
/// Counted from the board, a long deadhead put it before the truck could
/// even arrive.
#[test]
fn test_a_relayed_loads_appointment_counts_from_the_loaded_departure() {
    let mut app = TestApp::new();
    let job = relayed_load(&mut app, "Sherman").expect("Sherman relays a load off an empty board");
    let world = app.ctx.world;
    let here = world.resolve_city_key(&profile(&app).current_city);
    let origin = world.resolve_city_key(&job.origin);
    let corridor = world
        .supported_route(&here, &origin, None)
        .unwrap()
        .unwrap();
    let approach_h = world
        .facility_approach_route(&job.origin, &job.origin_location)
        .map(|approach| route_drive_hours(Some(&approach), 0.0, Some(world)))
        .unwrap_or(0.0);
    let departure_h = route_drive_hours(Some(&corridor), 0.0, Some(world))
        + approach_h
        + (PICKUP_CHECK_IN_MIN + PICKUP_LOADING_MIN) / 60.0;
    let zone = world
        .city(&job.destination)
        .map(|city| city_zone(city))
        .unwrap_or(EASTERN);
    let now = profile(&app).game_hours;

    let board = JobBoardState::new(&app.ctx, vec![job.clone()]);
    app.push_state(board);
    key(&mut app, Key::F1);
    let deadline = labels::<JobDetailState>(&app)
        .into_iter()
        .find(|line| line.starts_with("Deadline "))
        .expect("a deadline line");
    let due = appointment_text(now, departure_h + job.deadline_game_h, zone);
    assert!(
        deadline.ends_with(&format!("after loading, about {due}.")),
        "{deadline}"
    );
    assert_ne!(due, appointment_text(now, job.deadline_game_h, zone));
}

#[test]
fn test_relayed_pickup_waits_for_streets_then_hands_off_cruise_to_keeper_for_corners() {
    // Player report (2026-11-26): Point Comfort -> Victoria Dry Warehouse
    // relayed pickup on US-87. While still on US-87, street turns must stay
    // quiet and out of street-corner pacing; once on the shipper's streets,
    // adaptive cruise must hand off to the speed keeper and slow for the
    // first corner without missing it.
    let mut app = TestApp::new();
    career(&mut app, "Relay Corner Driver", "Point Comfort");
    let world = app.ctx.world;
    let corridor = world
        .supported_route("point_comfort_tx_us", "victoria_tx_us", None)
        .unwrap()
        .expect("Point Comfort to Victoria is on the network");
    let approach = world
        .facility_approach_route("victoria_tx_us", "Victoria Dry Warehouse")
        .unwrap();
    let route = corridor.then(&approach);
    let join = corridor.legs.len();

    let cargo = CARGO_CATALOG.values().next().expect("cargo catalog");
    let mut job = Job::new(
        cargo,
        15.0,
        "victoria_tx_us",
        "Victoria Dry Warehouse",
        "houston_tx_us",
        120.0,
        500.0,
        12.0,
    );
    job.destination_location = "Houston Dry Warehouse".to_string();
    job.bobtail = true;

    let mut drive = DrivingState::new(&mut app.ctx, job, route, Some(2), DRIVE_PHASE_PICKUP, None);
    let join_mi = drive.trip.leg_starts[join];

    // 1.5 miles before the join, still on US-87 at 60 mph on cruise:
    // not on local streets, no street turns in play, no street-corner clock override.
    app.ctx.settings.speed_keeper = true;
    drive.trip.truck.engine_on = true;
    drive.trip.truck.parking_brake = false;
    drive.trip.position_mi = join_mi - 1.5;
    drive.trip.truck.velocity_mps = 60.0 * 0.44704;
    drive.toggle_cruise(&mut app.ctx);
    assert!(drive.cruise_mph.is_some(), "cruise engaged on US-87");
    drive.update_turn_commitment(&mut app.ctx, 0.05);
    assert!(!drive.on_local_streets());
    assert!(drive.turn_cues_in_play().is_empty());
    assert!(!drive.trip.controlled_turn);

    // Step across the join onto the shipper's streets and through the first
    // judged corner. Cruise must hand off to the speed keeper and slow down
    // cleanly without missing the turn.
    let first_turn_mi = drive
        .trip
        .navigation_cues
        .iter()
        .find(|cue| cue.kind == "local_turn" && cue.key.starts_with("local:turn:"))
        .expect("Victoria Dry Warehouse has street turns")
        .at_mi;
    app.clear_speech();
    let dt = 0.05;
    let mut handed_off = false;
    for _ in 0..4000 {
        drive.update(&mut app.ctx, dt);
        if drive.trip.position_mi >= join_mi && drive.keeper_mph.is_some() {
            handed_off = true;
            assert!(
                drive.cruise_mph.is_none(),
                "cruise should cancel once keeper takes over on shipper streets"
            );
        }
        if drive.trip.position_mi > first_turn_mi + 0.06 {
            break;
        }
    }
    let spoken = app.event_lines().join(" ");
    assert!(
        handed_off,
        "expected cruise to hand off to speed keeper on shipper streets (pos={:.2}, join={join_mi:.2}, cruise={:?}, keeper={:?})",
        drive.trip.position_mi,
        drive.cruise_mph,
        drive.keeper_mph
    );
    assert!(
        drive.on_local_streets(),
        "expected on_local_streets() past join"
    );
    assert!(
        !spoken.contains("Missed the turn"),
        "truck should slow for the first street corner instead of missing it: {spoken}"
    );
}
