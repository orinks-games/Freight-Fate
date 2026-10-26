//! The grade advisory: `update_grade_advisory` in
//! `states/driving_updates/hazards.rs`, plus the terse gate over it.
//!
//! Ported from the advisory block at the bottom of
//! `tests/test_driving_features.py` (`_advisory_setup` and the six cases
//! under it).
//!
//! Python reaches these conditions by replacing `trip.grade_at` with a
//! closure. There is no such seam here, and faking one would test the fake:
//! every case below bakes REAL grade segments onto a real leg and lets
//! `Trip::grade_at` read them the way it reads the shipped corridor data, so
//! what runs is the same lookup the game runs. That costs one thing --
//! segments are fixed spans rather than a function, so a profile that Python
//! could bend mid-scan is laid out as a span here, and where a case needed a
//! second hill the truck is moved along the real profile to reach it instead
//! of the profile being rewritten under a stationary truck.

use ff_core::data::world::get_world;
use ff_core::data::world_models::{CorridorDetail, GradeSegment, Leg, Route};
use ff_core::models::jobs::{Job, CARGO_CATALOG};
use ff_core::models::profile::Profile;
use ff_core::sim::trip::{Trip, TripOptions};
use ff_core::sim::weather::{WeatherKind, WeatherSystem};

use freight_fate::app::testing::{AudioLog, TestApp};
use freight_fate::states::driving::DrivingState;
use freight_fate::states::driving_core::DRIVE_PHASE_DELIVERY;

const MPH_PER_MPS: f64 = 2.23694; // the constant the Python case divides by

// -- rigging -------------------------------------------------------------------------

/// `_advisory_setup(app, grade_at)`: a drive parked at mile 5 of a road whose
/// baked grade profile is `segments`, rolling at 60 mph.
///
/// The road is one 40-mile leg so every advisory's forward scan stays on it;
/// `Trip::grade_at` returns the first segment whose span contains the sample,
/// so the spans are written end to end in order.
fn advisory_setup(app: &mut TestApp, segments: Vec<GradeSegment>) -> DrivingState {
    let mut drive = a_drive(app);
    let city = drive.trip.route.cities[0].clone();
    let detail = CorridorDetail {
        grade_segments: segments,
        ..Default::default()
    };
    let leg = Leg::new(&city, &city, ROAD_MI, "I 90", "flat", Vec::new()).with_detail(detail);
    let route = Route::from_legs(vec![city.clone(), city], vec![leg]);
    let truck = drive.trip.truck.clone();
    let mut weather = WeatherSystem::new("heartland", Some(3), None, None, true);
    weather.current = WeatherKind::Clear;
    let mut trip = Trip::new(
        route,
        truck,
        weather,
        TripOptions {
            seed: Some(3),
            time_scale: 1.0,
            ..Default::default()
        },
    );
    // `quiet_trip`: nothing on the road but the grade under test.
    trip.set_npc_vehicles(Vec::new());
    trip.traffic_manager.rolling_bubble = false;
    trip.hazard_check_mi = 1e9;
    trip.inspection_check_mi = 1e9;
    trip.zones.clear();
    trip.curves.clear();
    trip.set_patrols(Vec::new());
    trip.position_mi = 5.0;
    drive.trip = trip;
    drive.reset_turn_state_for_trip();
    drive.destination_exit_taken = true;
    drive.trip.truck.start_engine();
    drive.trip.truck.velocity_mps = 60.0 / MPH_PER_MPS;
    app.clear_speech();
    drive
}

const ROAD_MI: f64 = 40.0;

/// A flat-to-`pct` profile: level up to `from_mi`, then `pct` to the end.
fn hill(from_mi: f64, pct: f64) -> Vec<GradeSegment> {
    vec![
        GradeSegment::new(0.0, from_mi, 0.0, "flat", "test bench"),
        GradeSegment::new(from_mi, ROAD_MI, pct, "hills", "test bench"),
    ]
}

/// One grade the whole way.
fn all_the_way(pct: f64) -> Vec<GradeSegment> {
    vec![GradeSegment::new(
        0.0,
        ROAD_MI,
        pct,
        if pct.abs() >= 3.0 { "hills" } else { "flat" },
        "test bench",
    )]
}

/// A drive on a real corridor, before its road is swapped for the bench one.
fn a_drive(app: &mut TestApp) -> DrivingState {
    let world = get_world();
    let mut profile = Profile::named_in("Grades", "Buffalo");
    profile.tutorial_done = true;
    app.ctx.profile = Some(profile);
    let route = world
        .supported_route("Buffalo", "Rochester", None)
        .expect("the world routes")
        .expect("Buffalo to Rochester has a route");
    let mut job = Job::new(
        &CARGO_CATALOG["general"],
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
        Some(99),
        DRIVE_PHASE_DELIVERY,
        Some(12.0),
    );
    drive.trip.set_npc_vehicles(Vec::new());
    drive.trip.weather.current = WeatherKind::Clear;
    drive
}

fn advisories(app: &TestApp) -> Vec<String> {
    app.event_lines()
        .into_iter()
        .filter(|line| line.contains("grade ahead"))
        .collect()
}

fn downgrades(app: &TestApp) -> Vec<String> {
    app.event_lines()
        .into_iter()
        .filter(|line| line.contains("downhill grade"))
        .collect()
}

// -- the advisory ---------------------------------------------------------------------

#[test]
fn test_a_steep_downgrade_is_called_out_before_the_truck_is_on_it() {
    // The player had no warning at all: the first news of a hill was the
    // speeding chime after cruise had already run away down it.
    let mut app = TestApp::new();
    let mut drive = advisory_setup(&mut app, hill(5.5, -6.0));

    drive.update_grade_advisory(&mut app.ctx);

    let spoken = app.event_lines();
    assert!(
        spoken
            .iter()
            .any(|l| l.contains("6.0 percent downhill grade ahead")),
        "{spoken:#?}"
    );
    assert!(spoken.iter().any(|l| l.contains("at least")), "{spoken:#?}");
    assert!(
        spoken.iter().any(|l| l.contains("Set the jake")),
        "{spoken:#?}"
    );

    // Once per grade, not once per scan, all the way down the hill.
    let said = spoken.len();
    for _ in 0..10 {
        drive.trip.position_mi += 0.2;
        drive.update_grade_advisory(&mut app.ctx);
    }
    assert_eq!(app.event_lines().len(), said, "{:#?}", app.event_lines());
}

#[test]
fn test_a_gentle_grade_gets_no_advisory() {
    let mut app = TestApp::new();
    let mut drive = advisory_setup(&mut app, all_the_way(-2.0));

    drive.update_grade_advisory(&mut app.ctx);

    assert!(downgrades(&app).is_empty(), "{:#?}", app.event_lines());
}

#[test]
fn test_a_short_dip_is_not_announced_as_a_grade() {
    // The baked profile is full of third-of-a-mile blips; they are not hills,
    // and warning about each one buried the grades that matter.
    let mut app = TestApp::new();
    // A 5 percent dip a third of a mile long, then a real 4 percent hill.
    let mut drive = advisory_setup(
        &mut app,
        vec![
            GradeSegment::new(0.0, 5.6, 0.0, "flat", "test bench"),
            GradeSegment::new(5.6, 5.9, -5.0, "hills", "test bench"),
            GradeSegment::new(5.9, 7.0, 0.0, "flat", "test bench"),
            GradeSegment::new(7.0, ROAD_MI, -4.0, "hills", "test bench"),
        ],
    );

    drive.update_grade_advisory(&mut app.ctx);
    assert!(advisories(&app).is_empty(), "{:#?}", app.event_lines());

    // And the dip did not latch away the hill behind it.
    drive.trip.position_mi = 6.5;
    drive.update_grade_advisory(&mut app.ctx);
    let spoken = app.event_lines();
    assert!(
        spoken
            .iter()
            .any(|l| l.contains("4.0 percent downhill grade ahead")),
        "{spoken:#?}"
    );
}

#[test]
fn test_the_next_grade_is_announced_after_the_road_levels_out() {
    // The latch clears on the flat, so a rolling route keeps warning.
    //
    // Python rewrites `grade_at` under a truck that barely moves. A baked
    // profile cannot be rewritten, so the truck drives the rolling road it
    // is describing: a long 5 percent descent, four level miles, then a
    // 4 percent one. The second hill is a different steepness on purpose --
    // the two advisories then differ in wording, so what the second
    // assertion measures is the advisory's own latch rather than the speech
    // pacer's identical-line window.
    //
    // The clock moves with the truck. Without that the three advisories all
    // land in the same instant, and the pacer -- correctly -- reads the last
    // one as still mid-sentence, flushes the backlog and resubmits it behind
    // the new line, so the first advisory is heard twice. Six real minutes
    // pass between these mileposts at 60 mph.
    let mut app = TestApp::new();
    let clock = app.fake_pacer_clock();
    let mut drive = advisory_setup(
        &mut app,
        vec![
            GradeSegment::new(0.0, 8.0, -5.0, "hills", "test bench"),
            GradeSegment::new(8.0, 12.0, 0.0, "flat", "test bench"),
            GradeSegment::new(12.0, ROAD_MI, -4.0, "hills", "test bench"),
        ],
    );

    drive.update_grade_advisory(&mut app.ctx);
    assert_eq!(downgrades(&app).len(), 1, "{:#?}", app.event_lines());

    drive.trip.position_mi = 9.5; // level here and level ahead: the latch lifts
    clock.advance(4.5 / 60.0 * 3600.0);
    drive.update_grade_advisory(&mut app.ctx);
    assert_eq!(downgrades(&app).len(), 1, "{:#?}", app.event_lines());

    drive.trip.position_mi = 11.5; // the next hill is in the lookahead
    clock.advance(2.0 / 60.0 * 3600.0);
    drive.update_grade_advisory(&mut app.ctx);
    assert_eq!(downgrades(&app).len(), 2, "{:#?}", app.event_lines());
}

#[test]
fn test_an_upgrade_is_called_out_too() {
    let mut app = TestApp::new();
    let mut drive = advisory_setup(&mut app, all_the_way(4.5));

    drive.update_grade_advisory(&mut app.ctx);

    let spoken = app.event_lines();
    assert!(
        spoken
            .iter()
            .any(|l| l.contains("4.5 percent uphill grade ahead")),
        "{spoken:#?}"
    );
    // Losing speed on a climb is expected; the line no longer says so.
    assert!(
        spoken.iter().all(|l| !l.contains("lose speed")),
        "{spoken:#?}"
    );
}

#[test]
fn test_terse_speech_hears_no_grade_advisories() {
    // Terse asked for the road to stay quiet; G is there on demand. The
    // advisory is unrequested commentary, which is exactly what the setting
    // exists to remove -- and it costs nothing, because terse skips the road
    // profile scan entirely rather than scanning and then staying silent.
    let mut app = TestApp::new();
    let log: AudioLog = app.record_audio();
    app.ctx.settings.driving_speech = "quiet".to_string();
    let mut drive = advisory_setup(&mut app, all_the_way(-6.0));
    app.clear_speech();
    log.borrow_mut().played.clear(); // the drive's own start-up sounds are not ours

    for _ in 0..10 {
        drive.trip.position_mi += 0.5;
        drive.update_grade_advisory(&mut app.ctx);
    }

    assert!(app.event_lines().is_empty(), "{:#?}", app.event_lines());
    // Silent means silent: no cue sound either.
    assert!(log.borrow().played.is_empty(), "{:#?}", log.borrow().played);
    assert_eq!(drive.grade_scan_mi, -1e9); // never even scanned

    // Normal speech still gets it.
    app.ctx.settings.driving_speech = "standard".to_string();
    drive.update_grade_advisory(&mut app.ctx);
    assert!(!downgrades(&app).is_empty(), "{:#?}", app.event_lines());
}
