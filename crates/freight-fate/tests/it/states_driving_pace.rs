//! The clock's pacing on the road (issue 293): the slow-merge handoff after
//! a pickup lets go of real time once the join is over, whatever the driver
//! does, and a pace change made while rolling is said once, waits for the
//! stop, and then applies.

use ff_core::data::world::get_world;
use ff_core::models::jobs::make_reposition_job;
use ff_core::models::profile::Profile;
use ff_core::models::trucks::truck_model_or_panic;
use ff_core::sim::trip_models::{
    merge_traffic_target_mph, MERGE_RECOVERY_MAX_MI, MERGE_RECOVERY_MAX_REAL_S,
};
use ff_core::sim::vehicle::KG_PER_TON;
use ff_core::sim::weather::WeatherKind;

use freight_fate::app::testing::TestApp;
use freight_fate::states::driving::DrivingState;
use freight_fate::states::driving_core::*;
use freight_fate::states::driving_updates::pacing::{
    PACE_CHANGE_DEFERRED_TERSE, PACE_CHANGE_DEFERRED_TEXT,
};

const DT: f64 = 1.0 / 30.0;
const RELAXED: f64 = 10.0;

fn mph_to_mps(mph: f64) -> f64 {
    mph / 2.2369362920544
}

/// Brandon's Carlisle departure on relaxed pace: a 25-ton flatbed behind a
/// yard mule at the 70-mph I-76 taper, in the acceleration lane's last feet.
fn a_relaxed_departure(app: &mut TestApp) -> DrivingState {
    app.ctx.settings.time_scale = RELAXED;
    let world = get_world();
    let mut profile = Profile::named_in("Relaxed Merge", "Carlisle");
    profile.tutorial_done = true;
    app.ctx.profile = Some(profile);
    let job = make_reposition_job(world, "Carlisle", "Pittsburgh", false, None)
        .expect("Carlisle to Pittsburgh is a supported reposition");
    let route = world
        .shortest_route("Carlisle", "Pittsburgh", None, false)
        .expect("the world routes")
        .expect("Carlisle to Pittsburgh has a route");
    let mut d = DrivingState::new(
        &mut app.ctx,
        job,
        route,
        Some(0),
        DRIVE_PHASE_DELIVERY,
        Some(12.0),
    );
    d.departure_checked = true;
    d.trip.set_npc_vehicles(Vec::new());
    d.trip.truck.specs = truck_model_or_panic("yard_mule").specs.clone();
    d.trip.truck.cargo_kg = 25.0 * KG_PER_TON;
    d.trip.truck.transmission.automatic = true;
    d.trip.truck.trailer_attached = true;
    d.weather_mut().current = WeatherKind::Clear;
    d.trip.truck.start_engine();
    // Staged at the end of the facility streets, on the on-ramp.
    d.trip.join_highway_after_streets();
    d.departure_ramp_mi = Some(0.05);
    assert_eq!(d.trip.time_scale, RELAXED);
    d
}

/// Reach the taper at 46 mph: the merge handoff takes the clock.
fn reach_the_taper_slow(app: &mut TestApp, d: &mut DrivingState) -> f64 {
    let (limit, _) = d.trip.speed_limit_at(d.trip.position_mi);
    assert_eq!(limit, 70.0);
    d.trip.truck.velocity_mps = mph_to_mps(46.0);
    d.update_departure_ramp(&mut app.ctx, 0.10);
    d.update_exit(&mut app.ctx, 0.0, 0.0);
    assert!(d.departure_merge_recovery);
    assert!(d.trip.controlled_ramp);
    assert_eq!(d.trip.effective_time_scale(), 1.0);
    assert_eq!(
        d.clock_override_reason().as_deref(),
        Some("controlled ramp: departure merge recovery")
    );
    limit
}

/// One frame of the merge watch, the way the frame loop runs it, with the
/// truck holding `mph` and covering `moved_mi` of road.
fn merge_frame(app: &mut TestApp, d: &mut DrivingState, mph: f64, moved_mi: f64, dt: f64) {
    d.trip.truck.velocity_mps = mph_to_mps(mph);
    d.update_exit(&mut app.ctx, 0.0, dt);
    d.update_departure_ramp(&mut app.ctx, moved_mi);
    d.bound_departure_merge_recovery(moved_mi, dt);
    d.trace_clock_override(dt);
}

#[test]
fn a_driver_who_never_reaches_merge_speed_gets_relaxed_pace_back() {
    // Issue 293: the handoff only let go at 75 percent of the road's speed,
    // so a driver who settled a mile an hour under it drove a relaxed haul
    // on the wall clock for three hours. Hold 51 mph against a 52.5 merge
    // speed: the old clear condition is never met.
    let mut app = TestApp::new();
    let mut d = a_relaxed_departure(&mut app);
    let limit = reach_the_taper_slow(&mut app, &mut d);
    let mph = 51.0;
    assert!(mph + 0.5 < merge_traffic_target_mph(limit));

    // The road the truck covers on the real clock while it is pinned. The
    // clock stays real while the truck is merging, and only that long.
    let moved = mph * DT / 3600.0;
    for _ in 0..90 {
        merge_frame(&mut app, &mut d, mph, moved, DT);
    }
    assert_eq!(
        d.clock_pacing.override_reason.as_deref(),
        Some("controlled ramp: departure merge recovery")
    );
    assert!(d.clock_pacing.override_logged, "logged once it has held");
    let mut held_s = 90.0 * DT;
    while d.departure_merge_recovery {
        assert_eq!(d.trip.effective_time_scale(), 1.0);
        merge_frame(&mut app, &mut d, mph, moved, DT);
        held_s += DT;
        assert!(
            held_s <= MERGE_RECOVERY_MAX_REAL_S + DT,
            "merge clock outlived its bound: {held_s} s"
        );
    }
    // At 51 mph the mile and a half of mainline is the bound that lands.
    assert!(held_s > 100.0, "{held_s}");
    // The session log's trace saw the pin by name, and saw it end.
    assert_eq!(d.clock_pacing.override_reason, None);
    assert!(!d.departure_merge_recovery, "the join is over");
    assert!(!d.trip.controlled_ramp);
    assert_eq!(d.trip.real_time_override(), None);
    assert_eq!(d.clock_override_reason(), None);
    // Above full-compression speed the clock is the Settings pacing again.
    assert_eq!(d.trip.effective_time_scale(), app.ctx.settings.time_scale);

    // And nothing re-arms it for the rest of the haul.
    for _ in 0..300 {
        merge_frame(&mut app, &mut d, mph, moved, DT);
    }
    assert!(!d.departure_merge_recovery);
    assert_eq!(d.trip.effective_time_scale(), RELAXED);
}

#[test]
fn reaching_merge_speed_still_ends_the_handoff_at_once() {
    let mut app = TestApp::new();
    let mut d = a_relaxed_departure(&mut app);
    let limit = reach_the_taper_slow(&mut app, &mut d);
    merge_frame(
        &mut app,
        &mut d,
        merge_traffic_target_mph(limit) + 1.0,
        0.001,
        DT,
    );
    assert!(!d.departure_merge_recovery);
    assert!(!d.trip.controlled_ramp);
    assert_eq!(d.clock_pacing.merge_recovery_s, 0.0);
}

#[test]
fn a_stopped_truck_cannot_hold_the_merge_clock_past_its_time_bound() {
    // Pulled onto the shoulder straight after the taper: no road covered,
    // no speed, so neither the merge speed nor the distance bound can ever
    // fire. The time bound still does.
    let mut app = TestApp::new();
    let mut d = a_relaxed_departure(&mut app);
    reach_the_taper_slow(&mut app, &mut d);
    let mut held_s = 0.0;
    while d.departure_merge_recovery {
        merge_frame(&mut app, &mut d, 0.0, 0.0, DT);
        held_s += DT;
        assert!(
            held_s <= MERGE_RECOVERY_MAX_REAL_S + DT,
            "merge clock outlived its bound: {held_s} s"
        );
    }
    assert!(held_s >= MERGE_RECOVERY_MAX_REAL_S - DT, "{held_s}");
    assert!(!d.trip.controlled_ramp);
    assert_eq!(d.trip.real_time_override(), None);
}

#[test]
fn the_merge_clock_ends_at_its_distance_bound() {
    let mut app = TestApp::new();
    let mut d = a_relaxed_departure(&mut app);
    reach_the_taper_slow(&mut app, &mut d);
    // A tenth of a mile a frame: the distance bound lands long before the
    // time bound could.
    let mut frames = 0;
    while d.departure_merge_recovery {
        merge_frame(&mut app, &mut d, 45.0, 0.1, DT);
        frames += 1;
        assert!(frames <= 20, "merge clock outlived its distance bound");
    }
    // Fifteen tenths, give or take the float sum.
    let expected = (MERGE_RECOVERY_MAX_MI / 0.1).round() as usize;
    assert!((expected..=expected + 1).contains(&frames), "{frames}");
    assert!(!d.trip.controlled_ramp);
}

#[test]
fn taking_an_exit_ends_the_merge_clock() {
    // The truck left the road it was joining: the merge is not the reason
    // for the clock any more, whatever the ramp itself then decides.
    let mut app = TestApp::new();
    let mut d = a_relaxed_departure(&mut app);
    reach_the_taper_slow(&mut app, &mut d);
    d.ramp_mi = Some(0.3);
    d.bound_departure_merge_recovery(0.0, DT);
    assert!(!d.departure_merge_recovery);
    assert_ne!(
        d.controlled_ramp_reason(),
        Some("departure merge recovery"),
        "the ramp may still pin the clock, but not as a merge"
    );
}

// -- a pace change made while rolling ---------------------------------------------

fn spoken_count(app: &TestApp, text: &str) -> usize {
    app.event_lines()
        .iter()
        .filter(|line| line.contains(text))
        .count()
}

#[test]
fn a_pace_change_while_rolling_is_said_once_and_applies_at_the_stop() {
    let mut app = TestApp::new();
    let mut d = a_relaxed_departure(&mut app);
    d.departure_ramp_mi = None;
    d.trip.truck.velocity_mps = mph_to_mps(40.0);
    app.clear_speech();

    // Standard chosen from the pause menu at 40 mph: saved, said, waiting.
    // The pacer's clock jumps ten seconds a call, far past its own repeat
    // window, so "once" here is the request's latch and not the pacer.
    let clock = app.fake_pacer_clock();
    app.ctx.settings.time_scale = 20.0;
    for _ in 0..90 {
        d.update_pace_change(&mut app.ctx, DT);
        clock.advance(10.0);
    }
    assert_eq!(d.trip.time_scale, RELAXED, "deferred while rolling");
    assert_eq!(spoken_count(&app, PACE_CHANGE_DEFERRED_TEXT), 1);
    let (pending, waited) = d.clock_pacing.deferred_pace.expect("a waiting change");
    assert_eq!(pending, 20.0);
    assert!((waited - 89.0 * DT).abs() < 1e-9, "{waited}");

    // Slowing but still over the threshold changes nothing and says nothing.
    d.trip.truck.velocity_mps = mph_to_mps(0.6);
    d.update_pace_change(&mut app.ctx, DT);
    assert_eq!(d.trip.time_scale, RELAXED);

    // Stopped: it applies, silently, and the request is spent.
    d.trip.truck.velocity_mps = 0.0;
    d.update_pace_change(&mut app.ctx, DT);
    assert_eq!(d.trip.time_scale, 20.0);
    assert_eq!(d.clock_pacing.deferred_pace, None);
    assert_eq!(spoken_count(&app, PACE_CHANGE_DEFERRED_TEXT), 1);

    // Rolling again on the new pacing: nothing is pending, nothing is said.
    d.trip.truck.velocity_mps = mph_to_mps(40.0);
    for _ in 0..30 {
        d.update_pace_change(&mut app.ctx, DT);
        clock.advance(10.0);
    }
    assert_eq!(spoken_count(&app, PACE_CHANGE_DEFERRED_TEXT), 1);
}

#[test]
fn a_second_different_request_is_a_new_request() {
    let mut app = TestApp::new();
    let mut d = a_relaxed_departure(&mut app);
    d.departure_ramp_mi = None;
    d.trip.truck.velocity_mps = mph_to_mps(40.0);
    let clock = app.fake_pacer_clock();
    app.clear_speech();
    app.ctx.settings.time_scale = 20.0;
    d.update_pace_change(&mut app.ctx, DT);
    assert_eq!(d.clock_pacing.deferred_pace.map(|(p, _)| p), Some(20.0));
    // The next visit to the pause menu is a while later.
    clock.advance(30.0);
    // Changed back to the pacing already running: the request is withdrawn.
    app.ctx.settings.time_scale = RELAXED;
    d.update_pace_change(&mut app.ctx, DT);
    assert_eq!(d.clock_pacing.deferred_pace, None);
    assert_eq!(d.trip.time_scale, RELAXED);
    // Real time asked for next is its own request, and the cue says so.
    app.clear_speech();
    app.ctx.settings.time_scale = 1.0;
    d.update_pace_change(&mut app.ctx, DT);
    assert_eq!(d.clock_pacing.deferred_pace.map(|(p, _)| p), Some(1.0));
    assert_eq!(spoken_count(&app, PACE_CHANGE_DEFERRED_TEXT), 1);
    assert_eq!(d.trip.time_scale, RELAXED);
}

#[test]
fn a_pace_change_while_stopped_applies_at_once_without_the_cue() {
    let mut app = TestApp::new();
    let mut d = a_relaxed_departure(&mut app);
    d.departure_ramp_mi = None;
    d.trip.truck.velocity_mps = 0.0;
    app.clear_speech();
    app.ctx.settings.time_scale = 20.0;
    d.update_pace_change(&mut app.ctx, DT);
    assert_eq!(d.trip.time_scale, 20.0);
    assert_eq!(spoken_count(&app, PACE_CHANGE_DEFERRED_TEXT), 0);
}

#[test]
fn the_deferred_pace_cue_follows_the_speech_rungs() {
    // Quiet takes the terse rendering; urgent only keeps confirmations out.
    for (rung, expected) in [
        ("standard", Some(PACE_CHANGE_DEFERRED_TEXT)),
        ("quiet", Some(PACE_CHANGE_DEFERRED_TERSE)),
        ("urgent_only", None),
    ] {
        let mut app = TestApp::new();
        let mut d = a_relaxed_departure(&mut app);
        d.departure_ramp_mi = None;
        app.ctx.settings.driving_speech = rung.to_string();
        d.trip.truck.velocity_mps = mph_to_mps(40.0);
        app.clear_speech();
        app.ctx.settings.time_scale = 20.0;
        d.update_pace_change(&mut app.ctx, DT);
        let lines = app.event_lines();
        let said: Vec<&String> = lines
            .iter()
            .filter(|line| {
                line.contains(PACE_CHANGE_DEFERRED_TEXT)
                    || line.contains(PACE_CHANGE_DEFERRED_TERSE)
            })
            .collect();
        match expected {
            Some(text) => assert_eq!(said, vec![text], "{rung}"),
            None => assert!(said.is_empty(), "{rung}: {said:?}"),
        }
        // Silent or not, the change still waits for the stop and applies.
        assert_eq!(d.trip.time_scale, RELAXED, "{rung}");
        d.trip.truck.velocity_mps = 0.0;
        d.update_pace_change(&mut app.ctx, DT);
        assert_eq!(d.trip.time_scale, 20.0, "{rung}");
    }
}

#[test]
fn the_frame_loop_defers_and_announces_a_rolling_pace_change() {
    // Wired through `update_frame`, the way the pause menu's change reaches
    // the drive when it closes.
    let mut app = TestApp::new();
    let mut d = a_relaxed_departure(&mut app);
    d.departure_ramp_mi = None;
    d.trip.truck.velocity_mps = mph_to_mps(40.0);
    app.clear_speech();
    app.ctx.settings.time_scale = 20.0;
    for _ in 0..10 {
        d.update_frame(&mut app.ctx, DT);
    }
    assert!(d.trip.truck.speed_mph() > 1.0);
    assert_eq!(d.trip.time_scale, RELAXED);
    assert_eq!(spoken_count(&app, PACE_CHANGE_DEFERRED_TEXT), 1);
}
