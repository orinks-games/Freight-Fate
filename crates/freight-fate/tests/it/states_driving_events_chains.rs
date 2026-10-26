//! The facility street chains (`tests/test_surface_chain.py`,
//! `tests/test_departure_chain.py`), plus the `driving_events` cases that
//! cannot run until another driving mixin lands -- those keep their ported
//! bodies behind `#[ignore]`.

use ff_core::data::world::{get_world, World};
use ff_core::models::jobs::make_reposition_job;
use ff_core::models::profile::Profile;
use ff_core::sim::trip_models::RoadStop;

use freight_fate::app::testing::TestApp;
use freight_fate::states::driving::DrivingState;
use freight_fate::states::driving_core::*;
use freight_fate::states::driving_rest_states::RestStopState;

/// `(city, location_name)` of a facility with a tier-1 street chain, or None
/// when the shipped data has none (`_turn_level_facility`).
fn a_turn_level_facility(world: &'static World) -> Option<(String, String)> {
    let mut keys: Vec<&String> = world.cities.keys().collect();
    keys.sort();
    for city in keys {
        let Some(entry) = world.cities.get(city) else {
            continue;
        };
        for location in &entry.locations {
            let Ok(route) = world.facility_approach_route(city, &location.name) else {
                continue;
            };
            if route.legs.len() >= 2 && route.legs.iter().any(|leg| leg.local_speed_mph > 0.0) {
                return Some((city.clone(), location.name.clone()));
            }
        }
    }
    None
}

/// A delivery whose destination is `city` / `location_name`.
fn a_drive_to(app: &mut TestApp, city: &str, location_name: &str) -> Option<DrivingState> {
    let world = get_world();
    let mut profile = Profile::named_in("Chain", "Denver");
    profile.tutorial_done = true;
    app.ctx.profile = Some(profile);
    let route = world.shortest_route("Denver", city, None, false).ok()??;
    let mut job = make_reposition_job(world, "Denver", city, false, None)?;
    job.destination_location = location_name.to_string();
    let mut drive = DrivingState::new(
        &mut app.ctx,
        job,
        route,
        Some(0),
        DRIVE_PHASE_DELIVERY,
        Some(12.0),
    );
    drive.trip.set_npc_vehicles(Vec::new());
    Some(drive)
}

/// The percent the board and the readouts publish covers the whole run.
///
/// On the street chain in to the dock the active trip is the streets alone,
/// and read on its own it had the drivers board saying "100% there" for the
/// whole last mile (Josh, 15 September). The highway is done but the streets
/// are not, so the figure sits just under 100 and climbs to it at the gate.
#[test]
fn test_progress_on_the_last_mile_streets_counts_the_whole_run() {
    let world = get_world();
    let Some((city, location)) = a_turn_level_facility(world) else {
        return; // no turn-level facility approaches in the shipped data
    };
    let mut app = TestApp::new();
    let Some(mut d) = a_drive_to(&mut app, &city, &location) else {
        return; // no corridor route from Denver to that city
    };
    d.trip.position_mi = d.trip.total_miles();
    d.destination_exit_taken = true;
    assert_eq!(d.trip.progress_percent(), 100);

    assert!(d.begin_surface_chain(&mut app.ctx, false));
    // A long run's last mile rounds to 100; the readout holds at 99 until
    // the truck is actually at the gate, and the board at 95.
    let at_start = d.journey_progress_percent();
    assert!((90..=99).contains(&at_start), "{at_start}");
    let detail = d
        .presence_state(&app.ctx)
        .expect("a drive has presence")
        .detail;
    assert!(detail.contains("95% there"), "{detail}");

    d.trip.position_mi = d.trip.total_miles() / 2.0;
    let halfway = d.journey_progress_percent();
    assert!(
        (at_start..=99).contains(&halfway),
        "{at_start} -> {halfway}"
    );

    d.trip.position_mi = d.trip.total_miles();
    assert_eq!(d.journey_progress_percent(), 100);
    let detail = d
        .presence_state(&app.ctx)
        .expect("a drive has presence")
        .detail;
    assert!(detail.contains("100% there"), "{detail}");
}

#[test]
fn test_chain_swaps_to_streets_and_keeps_the_clock() {
    let world = get_world();
    let Some((city, location)) = a_turn_level_facility(world) else {
        return; // no turn-level facility approaches in the shipped data
    };
    let mut app = TestApp::new();
    let Some(mut d) = a_drive_to(&mut app, &city, &location) else {
        return; // no corridor route from Denver to that city
    };
    d.trip.game_minutes = 123.0;
    let start_hour = d.trip.start_hour;
    let career_hours = d.trip.career_hours;
    d.destination_exit_taken = true;

    assert!(d.begin_surface_chain(&mut app.ctx, true));
    assert!(d.surface_chain);
    assert!(d.highway_trip.is_some());
    // Clock, weekday, and settlement continuity.
    assert_eq!(d.trip.game_minutes, 123.0);
    assert_eq!(d.trip.start_hour, start_hour);
    assert_eq!(d.trip.career_hours, career_hours);
    // The streets are real tier-1 segments with per-street zones.
    assert!(d.trip.route.legs.len() >= 2);
    assert!(d
        .trip
        .route
        .legs
        .iter()
        .any(|leg| leg.local_speed_mph > 0.0));
    assert!(d
        .trip
        .zones
        .iter()
        .any(|zone| zone.reason == "facility access road"));
    // The yard behind a driveway, or no gate stretch at all on a chain that
    // ends on the public street -- never the old 15 on the street.
    // (A chain baked before the street detail keeps its old gate zone.)
    assert!(d.trip.has_street_detail());
    assert_eq!(
        d.trip.zones.iter().any(|zone| zone.reason == "yard"),
        d.trip.driveway_mi().is_some()
    );
    assert!(!d
        .trip
        .zones
        .iter()
        .any(|zone| zone.reason == "facility gate"));
    // No random hazards on the last city miles.
    assert_eq!(d.trip.hazard_scale, 0.0);
}

#[test]
fn test_each_corner_chimes_with_its_own_call_in_order() {
    // Live drive into Ardmore, 2026-09-24: the yard's streets open with three
    // corners 0.05 mile apart -- left onto West Broadway Street, right onto M
    // Street Southwest, left back onto West Broadway Street. The first is
    // called in the off-the-ramp line, and that line's grace (its words at
    // the slowest modelled voice, 71 seconds) held the corner in play long
    // after the truck had made it: the other two calls came late, and all
    // three turn tones sounded together 0.2 mile on.
    //
    // The chime goes with the call now (player report, 2026-10-13), so each
    // one has to sound before its own corner and after the one before it is
    // taken: never a right chime while a left is still ahead.
    use freight_fate::playtest::harness::{PlaytestHarness, RouteSetup};
    use freight_fate::states::driving_turns::is_judged_turn;
    let dt = 1.0 / 30.0;
    let mut harness = PlaytestHarness::new();
    {
        let settings = &mut harness.app.ctx.settings;
        settings.apply_driving_assistance_preset("all");
        settings.speed_keeper = true;
        settings.automatic_transmission = true;
    }
    harness.start_route(
        "oklahoma_city_ok_us",
        "ardmore_ok_us",
        RouteSetup::seeded(4242)
            .named("Ardmore Corners")
            .destination_location("Ardmore Company Yard"),
    );
    let log = harness.app.record_audio();
    let corners = harness.with_drive(|d, ctx| {
        if let Some(profile) = ctx.profile.as_mut() {
            profile.tutorial_done = true;
        }
        d.tutorial = None;
        d.departure_checked = true;
        d.truck_mut().start_engine();
        d.truck_mut().set_air_ready(false);
        d.truck_mut().transmission.automatic = true;
        d.truck_mut().transmission.gear = 4;
        d.truck_mut().velocity_mps = 15.0 / 2.23694;
        d.destination_exit_taken = true;
        d.speed_control_armed = true;
        assert!(d.begin_surface_chain(ctx, true));
        d.trip
            .navigation_cues
            .iter()
            .filter(|cue| is_judged_turn(cue))
            .take(3)
            .map(|cue| (cue.at_mi, cue.direction.clone()))
            .collect::<Vec<_>>()
    });
    let sides: Vec<&str> = corners.iter().map(|(_, side)| side.as_str()).collect();
    assert_eq!(sides, ["left", "right", "left"], "{corners:?}");
    assert!(corners[2].0 - corners[0].0 < 0.15, "{corners:?}");
    let mut tones = Vec::new();
    let mut played = 0;
    for _ in 0..(30 * 120) {
        harness.advance_clock(dt);
        harness.with_drive(|d, ctx| {
            let cut_out = d.truck().specs.air_governor_cut_out_psi;
            d.truck_mut().set_air_pressure_psi(cut_out);
            d.update_frame(ctx, dt);
        });
        let at = harness.read_drive(|d| d.trip.position_mi);
        let calls = log.borrow().played.clone();
        for (key, _, _) in &calls[played..] {
            if key.starts_with("events/turn_") {
                tones.push((key.clone(), at));
            }
        }
        played = calls.len();
        if at > corners[2].0 + 0.05 {
            break;
        }
    }
    assert_eq!(tones.len(), 3, "{tones:?}");
    let mut previous_mi = f64::NEG_INFINITY;
    for ((key, at), (corner_mi, side)) in tones.iter().zip(&corners) {
        assert_eq!(key, &format!("events/turn_{side}"), "{tones:?}");
        assert!(
            *at <= *corner_mi && *at > previous_mi,
            "the {side} turn at {corner_mi:.2} mile sounded at {at:.3}: {tones:?}"
        );
        previous_mi = *corner_mi;
    }
}

#[test]
fn test_the_chain_takes_the_truck_and_the_weather_with_it() {
    // Python aliased one truck object from both trips; the Rust `Trip` owns
    // its truck and weather, so the swap has to carry them across.
    let world = get_world();
    let Some((city, location)) = a_turn_level_facility(world) else {
        return;
    };
    let mut app = TestApp::new();
    let Some(mut d) = a_drive_to(&mut app, &city, &location) else {
        return;
    };
    d.trip.truck.start_engine();
    d.trip.truck.damage_pct = 11.0;
    let weather = d.trip.weather.current;
    d.destination_exit_taken = true;

    assert!(d.begin_surface_chain(&mut app.ctx, false));

    assert_eq!(d.trip.truck.damage_pct, 11.0);
    assert!(d.trip.truck.engine_on);
    assert_eq!(d.trip.weather.current, weather);
}

#[test]
fn test_chain_declines_without_turn_level_data() {
    let world = get_world();
    // A facility WITHOUT a turn-level approach.
    let mut plain: Option<(String, String)> = None;
    let mut keys: Vec<&String> = world.cities.keys().collect();
    keys.sort();
    'outer: for city in keys {
        let Some(entry) = world.cities.get(city) else {
            continue;
        };
        for location in &entry.locations {
            let short = match world.facility_approach_route(city, &location.name) {
                Ok(route) => {
                    route.legs.len() < 2 || !route.legs.iter().any(|leg| leg.local_speed_mph > 0.0)
                }
                Err(_) => false,
            };
            if short {
                plain = Some((city.clone(), location.name.clone()));
                break 'outer;
            }
        }
    }
    let Some((city, location)) = plain else {
        return;
    };
    let mut app = TestApp::new();
    let Some(mut d) = a_drive_to(&mut app, &city, &location) else {
        return;
    };
    let generation = d.trip_generation;

    assert!(!d.begin_surface_chain(&mut app.ctx, false));
    assert_eq!(d.trip_generation, generation);
    assert!(!d.surface_chain);
}

#[test]
fn test_chain_survives_save_and_resume() {
    let world = get_world();
    let Some((city, location)) = a_turn_level_facility(world) else {
        return;
    };
    let mut app = TestApp::new();
    let Some(mut d) = a_drive_to(&mut app, &city, &location) else {
        return;
    };
    d.destination_exit_taken = true;
    assert!(d.begin_surface_chain(&mut app.ctx, false));
    d.trip.position_mi = 1.0f64.min(d.trip.total_miles() / 2.0);
    let snap = d.snapshot(&app.ctx);
    assert_eq!(snap["surface_chain"], true);

    let resumed = DrivingState::from_snapshot(&mut app.ctx, &snap).expect("the snapshot resumes");
    assert!(resumed.surface_chain);
    assert!(resumed.destination_exit_taken);
    assert!(resumed
        .trip
        .route
        .legs
        .iter()
        .any(|leg| leg.local_speed_mph > 0.0));
    assert!((resumed.trip.position_mi - d.trip.position_mi).abs() < 1e-6);
    assert!((resumed.trip.game_minutes - d.trip.game_minutes).abs() < 1e-6);
}

#[test]
fn test_a_resumed_street_chain_calls_its_first_corner_fresh() {
    // Rebuilding a saved chain ran the live off-the-ramp handoff: the first
    // corner was marked as called in a line that is never spoken on a
    // resume, so the resumed drive heard no approach call for it, and an
    // armed session asked the speed keeper to take a parked, engine-off
    // truck ("needs the engine running") as the drive loaded. Ardmore's yard
    // streets open with a corner inside the first call's window.
    use freight_fate::playtest::harness::{PlaytestHarness, RouteSetup};
    let mut harness = PlaytestHarness::new();
    harness.app.ctx.settings.speed_keeper = true;
    harness.start_route(
        "oklahoma_city_ok_us",
        "ardmore_ok_us",
        RouteSetup::seeded(4242)
            .named("Ardmore Resume")
            .destination_location("Ardmore Company Yard"),
    );
    let key = harness.with_drive(|d, ctx| {
        d.destination_exit_taken = true;
        assert!(d.begin_surface_chain(ctx, true));
        let corner = d
            .turn_cue_in_play()
            .expect("the yard's streets open with a corner");
        assert!(
            d.turn_advised.contains(&corner.key),
            "the live handoff calls the first corner"
        );
        corner.key
    });
    harness.app.clear_speech();

    let resumed = harness.with_drive(|d, ctx| {
        d.speed_control_armed = true;
        let snap = d.snapshot(ctx);
        DrivingState::from_snapshot(ctx, &snap).expect("the snapshot resumes")
    });

    assert!(resumed.surface_chain);
    assert!(!resumed.turn_advised.contains(&key));
    assert!(!resumed.turn_announced.contains(&key));
    assert!(
        resumed.speed_control_armed,
        "the session waits for the truck to roll"
    );
    assert!(resumed.keeper_mph.is_none());
    let heard = [harness.app.main_lines(), harness.app.event_lines()].concat();
    assert!(
        !heard.iter().any(|line| line.contains("speed keeper needs")),
        "{heard:?}"
    );
}

#[test]
fn test_a_departure_chain_and_a_surface_chain_never_run_together() {
    let world = get_world();
    let Some((city, location)) = a_turn_level_facility(world) else {
        return;
    };
    let mut app = TestApp::new();
    let Some(mut d) = a_drive_to(&mut app, &city, &location) else {
        return;
    };
    d.surface_chain = true;
    assert!(!d.begin_departure_chain(&mut app.ctx, false));
}

/// With Facility streets off, the 1.8 arrival and departure: no street chain
/// either end of a delivery, even to and from a facility that has one, and a
/// pickup in town is the single access-road leg.
#[test]
fn test_facility_streets_off_skips_both_chains() {
    let world = get_world();
    let Some((city, location)) = a_turn_level_facility(world) else {
        return;
    };
    let classic = world
        .facility_approach_route_with(&city, &location, false)
        .expect("the facility resolves");
    assert_eq!(classic.legs.len(), 1);
    assert!(classic.legs.iter().all(|leg| leg.local_speed_mph == 0.0));

    let mut app = TestApp::new();
    app.ctx.settings.facility_streets = false;
    let Some(mut d) = a_drive_to(&mut app, &city, &location) else {
        return;
    };
    assert!(!d.facility_streets_on);
    d.destination_exit_taken = true;
    assert!(d.surface_chain_route(&app.ctx).is_none());
    assert!(!d.destination_street_chain_ahead(&app.ctx));
    assert!(!d.begin_surface_chain(&mut app.ctx, false));
    assert!(!d.surface_chain);

    // Out of the same facility, loaded.
    let mut job = make_reposition_job(world, &city, "Denver", false, None).expect("a job");
    job.origin_location = location.clone();
    let route = world
        .shortest_route(&city, "Denver", None, false)
        .ok()
        .flatten()
        .expect("a corridor to Denver");
    let mut out = DrivingState::new(
        &mut app.ctx,
        job.clone(),
        route.clone(),
        Some(0),
        DRIVE_PHASE_DELIVERY,
        Some(12.0),
    );
    assert!(out.departure_chain_route(&app.ctx).is_none());
    assert!(!out.begin_departure_chain(&mut app.ctx, false));

    // The streets setting still drives that departure.
    app.ctx.settings.facility_streets = true;
    let mut streets = DrivingState::new(
        &mut app.ctx,
        job,
        route,
        Some(0),
        DRIVE_PHASE_DELIVERY,
        Some(12.0),
    );
    assert!(streets.begin_departure_chain(&mut app.ctx, false));
}

/// With Facility streets off no exit ramp ends at a traffic light or stop
/// sign, as in 1.8: the facility's own ramp runs into the facility, and a
/// truck stop's runs into its lot.
#[test]
fn test_facility_streets_off_has_no_light_or_sign_at_any_ramp() {
    let world = get_world();
    let Some((city, location)) = a_turn_level_facility(world) else {
        return;
    };
    let mut app = TestApp::new();
    let Some(mut d) = a_drive_to(&mut app, &city, &location) else {
        return;
    };
    let total = d.trip.total_miles();
    let exits: Vec<f64> = (1..40).map(|i| total * f64::from(i) / 40.0).collect();
    let controls = |d: &DrivingState, ctx: &freight_fate::app::GameContext, kind: &str| {
        exits
            .iter()
            .map(|&at| d.ramp_control_for(ctx, &RoadStop::new("Exit", at, kind), None))
            .collect::<Vec<_>>()
    };
    let with_streets = controls(&d, &app.ctx, "delivery_destination");
    assert!(
        with_streets.iter().any(|control| control != "none"),
        "{with_streets:?}"
    );
    let truck_stop = controls(&d, &app.ctx, "truck_stop");
    assert!(
        truck_stop.iter().any(|control| control != "none"),
        "{truck_stop:?}"
    );

    d.facility_streets_on = false;
    for kind in ["delivery_destination", "truck_stop", "public_rest_area"] {
        let classic = controls(&d, &app.ctx, kind);
        assert!(
            classic.iter().all(|control| control == "none"),
            "{kind}: {classic:?}"
        );
    }
}

/// A drive keeps the Facility streets value it started with, across a save
/// and a change of the setting before the resume.
#[test]
fn test_a_resumed_drive_keeps_its_facility_streets() {
    let world = get_world();
    let Some((city, location)) = a_turn_level_facility(world) else {
        return;
    };
    let mut app = TestApp::new();
    app.ctx.settings.facility_streets = false;
    let Some(d) = a_drive_to(&mut app, &city, &location) else {
        return;
    };
    let snap = d.snapshot(&app.ctx);
    assert_eq!(snap["facility_streets"], false);
    app.ctx.settings.facility_streets = true;
    let mut resumed =
        DrivingState::from_snapshot(&mut app.ctx, &snap).expect("the snapshot resumes");
    assert!(!resumed.facility_streets_on);
    resumed.destination_exit_taken = true;
    assert!(!resumed.begin_surface_chain(&mut app.ctx, false));

    // A save from before the setting was made on the streets.
    let mut old = snap.clone();
    old.as_object_mut().unwrap().remove("facility_streets");
    app.ctx.settings.facility_streets = false;
    let resumed = DrivingState::from_snapshot(&mut app.ctx, &old).expect("the snapshot resumes");
    assert!(resumed.facility_streets_on);
}

// -- cases that wait on another driving mixin -----------------------------------------

#[test]
fn test_exit_missed_when_too_fast() {
    // `tests/test_driving_exits.py`: the gore refuses a truck carrying more
    // than road speed, and the exit is settled either way.
    let mut app = TestApp::new();
    let world = get_world();
    let mut profile = Profile::named_in("Exits", "Denver");
    profile.tutorial_done = true;
    app.ctx.profile = Some(profile);
    let job = make_reposition_job(world, "Denver", "Cheyenne", false, None).expect("a reposition");
    let route = world
        .shortest_route("Denver", "Cheyenne", None, false)
        .expect("the world routes")
        .expect("a route");
    let mut d = DrivingState::new(
        &mut app.ctx,
        job,
        route,
        Some(0),
        DRIVE_PHASE_DELIVERY,
        Some(12.0),
    );
    let stop: RoadStop = d.trip.stops[0].clone();
    d.trip.position_mi = stop.at_mi - 1.0;
    // Python used a flat 29 m/s (~65 mph) on the corridor a default new career
    // starts on. The gore's acceptance is corridor-aware (posted limit plus
    // the enforcement leeway), and this corridor is a fast one, so the speed
    // is taken from the gate itself: comfortably over whatever it will accept.
    let too_fast_mph = d.gore_acceptance_mph(Some(&stop)) + 10.0;
    d.trip.truck.velocity_mps = too_fast_mph / 2.2369362920544;
    d.toggle_exit_signal(&mut app.ctx);
    assert_eq!(d.exit_stop.as_ref().map(|s| s.key()), Some(stop.key()));
    d.exit_lane_entered = true;
    d.trip.position_mi = stop.at_mi;

    d.update_frame(&mut app.ctx, 1.0 / 60.0);

    assert!(d.ramp_mi.is_none(), "blew past it");
    assert!(d.exit_stop.is_none());
}

#[test]
fn test_taking_the_exit_puts_the_truck_on_the_ramp() {
    let mut app = TestApp::new();
    let world = get_world();
    let mut profile = Profile::named_in("Exits", "Denver");
    profile.tutorial_done = true;
    app.ctx.profile = Some(profile);
    let job = make_reposition_job(world, "Denver", "Cheyenne", false, None).expect("a reposition");
    let route = world
        .shortest_route("Denver", "Cheyenne", None, false)
        .expect("the world routes")
        .expect("a route");
    let mut d = DrivingState::new(
        &mut app.ctx,
        job,
        route,
        Some(0),
        DRIVE_PHASE_DELIVERY,
        Some(12.0),
    );
    let stop: RoadStop = d.trip.stops[0].clone();
    d.trip.position_mi = stop.at_mi - 1.0;
    d.trip.truck.velocity_mps = 13.0; // ~29 mph: inside gore acceptance
    d.toggle_exit_signal(&mut app.ctx);
    d.exit_lane_entered = true;
    d.trip.position_mi = stop.at_mi;

    d.update_frame(&mut app.ctx, 1.0 / 60.0);

    // Gore to bar is this exit's own ramp, then the stretch to the driveway.
    let expected = d.trip.ramp_length_mi(&stop) + RAMP_ACCESS_MI;
    let ramp_mi = d.ramp_mi.expect("on the ramp");
    assert!((ramp_mi - expected).abs() < 0.01, "{ramp_mi} vs {expected}");
    assert!(d.ramp_stop.is_some());
}

#[test]
fn test_a_blown_destination_terminal_loops_back() {
    // `tests/test_destination_terminal_miss.py`: the terminal used to be the
    // one blown stop with no consequence at all (owner playtest, Buffalo to
    // Albany, 2026-08-12).
    let mut app = TestApp::new();
    let world = get_world();
    let mut profile = Profile::named_in("Miss", "Denver");
    profile.tutorial_done = true;
    app.ctx.profile = Some(profile);
    let job = make_reposition_job(world, "Denver", "Cheyenne", false, None).expect("a reposition");
    let route = world
        .shortest_route("Denver", "Cheyenne", None, false)
        .expect("the world routes")
        .expect("a route");
    let mut d = DrivingState::new(
        &mut app.ctx,
        job,
        route,
        Some(0),
        DRIVE_PHASE_DELIVERY,
        Some(12.0),
    );
    let stop = RoadStop::new("Acme Freight", d.trip.total_miles(), "delivery_destination");
    d.ramp_stop = Some(stop.clone());
    d.ramp_mi = Some(-RAMP_OVERSHOOT_MI - 0.1);
    d.ramp_end_said = true;
    d.ramp_arrival_grace_s = 0.0;
    let minutes = d.trip.game_minutes;

    d.loop_back_to_destination_terminal(&mut app.ctx, &stop);

    assert_eq!(d.ramp_terminal_miss_count, 1);
    assert!(d.trip.game_minutes >= minutes + RAMP_TERMINAL_MISS_LOOP_MIN);
    assert!(d.ramp_mi.unwrap() >= RAMP_ACCESS_MI);
    assert!(!d.ramp_end_said, "the arrival line speaks fresh");
    assert!(d.status_text.contains("Drove past"));
}

#[test]
fn test_the_missed_destination_exit_loops_back_a_whole_window() {
    // `tests/test_exit_recovery.py`: under time compression one mile passes
    // in a few real seconds, making the re-approach unwinnable before it was
    // heard.
    let mut app = TestApp::new();
    let world = get_world();
    let mut profile = Profile::named_in("Recovery", "Denver");
    profile.tutorial_done = true;
    app.ctx.profile = Some(profile);
    let job = make_reposition_job(world, "Denver", "Cheyenne", false, None).expect("a reposition");
    let route = world
        .shortest_route("Denver", "Cheyenne", None, false)
        .expect("the world routes")
        .expect("a route");
    let mut d = DrivingState::new(
        &mut app.ctx,
        job,
        route,
        Some(0),
        DRIVE_PHASE_DELIVERY,
        Some(12.0),
    );
    d.trip.position_mi = d.trip.total_miles();
    d.trip.finished = true;
    let minutes = d.trip.game_minutes;

    d.handle_missed_destination_exit(&mut app.ctx);

    assert!(!d.trip.finished);
    assert!(d.missed_destination_exit_said);
    assert!(d.trip.game_minutes >= minutes + EXIT_MISS_LOOP_MIN);
    assert!(d.trip.position_mi < d.trip.total_miles() - 1.0);
    assert_eq!(d.destination_exit_announced_key, "");
    assert!(d.destination_exit_cache.is_none());
}

#[test]
fn test_the_rest_key_opens_a_route_points_menu() {
    // `tests/test_driving_exits.py`: T at a standstill beside a stop opens
    // that stop's own menu.
    let mut app = TestApp::new();
    let world = get_world();
    let mut profile = Profile::named_in("Rest", "Denver");
    profile.tutorial_done = true;
    app.ctx.profile = Some(profile);
    let job = make_reposition_job(world, "Denver", "Cheyenne", false, None).expect("a reposition");
    let route = world
        .shortest_route("Denver", "Cheyenne", None, false)
        .expect("the world routes")
        .expect("a route");
    let mut d = DrivingState::new(
        &mut app.ctx,
        job,
        route,
        Some(0),
        DRIVE_PHASE_DELIVERY,
        Some(12.0),
    );
    let stop = d.trip.stops[0].clone();
    d.trip.position_mi = stop.at_mi;
    d.trip.truck.velocity_mps = 0.0;

    d.try_rest_stop(&mut app.ctx);
    app.ctx.run_deferred();

    assert!(
        app.ctx
            .state()
            .is_some_and(|s| s.borrow().as_any().is::<RestStopState>()),
        "T beside a stop opens that stop's own menu"
    );
}
