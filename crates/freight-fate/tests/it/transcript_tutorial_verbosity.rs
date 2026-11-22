//! First-run guidance ignores verbosity until the walkthrough is done (R15)
//! -- port of `tests/test_tutorial_verbosity.py`.
//!
//! Terse speech is a filter on running commentary, and first-run teaching is
//! not commentary. Terse mode used to silence the first-drive walkthrough
//! outright: a brand-new player who picked terse before their first drive --
//! exactly the player who hates chatty games -- was never told the status,
//! help, or hazard keys exist, and could not pull information they were never
//! told about. The gate is `tutorial_done` itself, not verbosity history: the
//! walkthrough speaks in full whatever the speech mode, and finishing it then
//! flipping terse on resurrects nothing.
//!
//! # Where this differs from the Python
//!
//! Python drove a `_Ctx` stub whose `say` appended to a list and whose
//! `control_hint` returned `action.upper()`, so the key-name assertions read
//! `"SPEED"`, `"STATUS_MENU"` and friends. Here the real `GameContext`
//! answers, so the same assertions ask `ctx.control_hint(...)` for the names
//! this driver's controls actually give -- the point of the case (the keys
//! are named at all) is unchanged, and pinning the stub's shouty
//! placeholders would have pinned nothing.
//!
//! The stub also bypassed the driving speech ladder. It does not need
//! bypassing: `ladder_applies()` is false while `tutorial_done` is false, so
//! an unfinished walkthrough reaches the voice whatever the rung is set to.
//! That IS the feature, and recording at `ctx.speech` is what proves it.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use ff_core::models::jobs::{cargo_type, Job};
use ff_core::models::profile::{self, Profile};
use ff_core::sim::vehicle::TruckState;
use freight_fate::app::testing::TestApp;
use freight_fate::app::GameContext;
use freight_fate::states::driving::DrivingState;
use freight_fate::states::driving_core::{Instructor, Tutorial, DRIVE_PHASE_DELIVERY};

const MPS_PER_MPH: f64 = 1.0 / 2.23694;

/// `_Ctx(terse=...)`: a fresh career whose walkthrough is unfinished, at the
/// requested rung, with the automatic box the Python stub forced.
fn prepare(app: &mut TestApp, terse: bool) {
    app.ctx.settings.driving_speech = if terse { "quiet" } else { "standard" }.to_string();
    app.ctx.settings.automatic_transmission = true;
    let mut profile = Profile::named_in("Tutorial", "Buffalo");
    profile.tutorial_done = false;
    app.ctx.profile = Some(profile);
}

/// A truck rolling at `speed_mph` with the brake off (the Python
/// `SimpleNamespace(speed_mph=..., parking_brake=...)`).
fn truck_at(speed_mph: f64, parking_brake: bool) -> TruckState {
    TruckState {
        velocity_mps: speed_mph * MPS_PER_MPH,
        parking_brake,
        ..Default::default()
    }
}

/// `_walk_through(ctx)`: engine, brake, rolling -- the whole walkthrough.
fn walk_through(ctx: &mut GameContext) -> Tutorial {
    let mut tutorial = Tutorial::new();
    tutorial.begin(ctx);
    tutorial.on_engine_started(ctx);
    tutorial.on_parking_brake_released(ctx);
    tutorial.update(ctx, 1.0 / 60.0, &truck_at(25.0, false));
    tutorial
}

#[test]
fn test_a_terse_player_hears_the_whole_walkthrough() {
    // One app, two runs: a second `TestApp` in the same scope deadlocks on
    // the environment lock, so the rung is flipped and the gate reopened in
    // place instead of building a second context.
    let mut app = TestApp::new();

    prepare(&mut app, false);
    app.clear_speech();
    walk_through(&mut app.ctx);
    let normal = app.main_lines();

    prepare(&mut app, true);
    app.clear_speech();
    walk_through(&mut app.ctx);
    let terse = app.main_lines();

    assert_eq!(terse, normal);
    assert_eq!(terse.len(), 4);
}

#[test]
fn test_the_walkthrough_still_teaches_the_pull_keys_in_terse() {
    let mut app = TestApp::new();
    prepare(&mut app, true);
    app.clear_speech();
    walk_through(&mut app.ctx);
    let spoken = app.main_lines();
    let keys_line = spoken.last().expect("the walkthrough spoke").clone();
    // The lines every push-message retirement leans on: the player can pull
    // speed, status, and the full controls on demand -- but only if they
    // were told the keys exist.
    for action in ["speed", "status_menu", "help", "emergency_brake"] {
        let hint = app.ctx.control_hint(action);
        assert!(
            keys_line.contains(&hint),
            "{action} ({hint:?}) missing from {keys_line:?}"
        );
    }
    assert!(keys_line.contains("hazard warning"));
}

#[test]
fn test_reminders_speak_in_terse_too() {
    let mut app = TestApp::new();
    prepare(&mut app, true);
    let mut tutorial = Tutorial::new();
    tutorial.begin(&mut app.ctx);
    app.clear_speech();

    tutorial.update(&mut app.ctx, 26.0, &truck_at(0.0, true));

    let engine = app.ctx.control_hint("engine");
    assert_eq!(
        app.main_lines(),
        vec![format!("Reminder: press {engine} to start the engine.")]
    );
}

#[test]
fn test_the_air_reminder_matches_the_gauge() {
    // The reminder is on a timer, and the compressor does not wait for it:
    // a first drive heard "Air ready: 100 psi" and then "wait for air
    // pressure to reach 100 psi" (agent drive, 2026-09-01). Air still
    // building keeps the old line; air already up says the one step left.
    let mut app = TestApp::new();
    prepare(&mut app, true);
    let brake = app.ctx.control_hint("parking_brake");

    let mut tutorial = Tutorial::new();
    tutorial.begin(&mut app.ctx);
    tutorial.on_engine_started(&mut app.ctx);
    app.clear_speech();
    let mut building = truck_at(0.0, true);
    building.set_cold_air_start();
    tutorial.update(&mut app.ctx, 26.0, &building);
    assert_eq!(
        app.main_lines(),
        vec![format!(
            "Reminder: wait for air pressure to reach 100 psi, then press {brake} to release \
             the parking brake."
        )]
    );

    let mut tutorial = Tutorial::new();
    tutorial.begin(&mut app.ctx);
    tutorial.on_engine_started(&mut app.ctx);
    app.clear_speech();
    let mut ready = truck_at(0.0, true);
    ready.set_air_ready(true);
    tutorial.update(&mut app.ctx, 26.0, &ready);
    assert_eq!(
        app.main_lines(),
        vec![format!(
            "Reminder: air is ready. Press {brake} to release the parking brake."
        )]
    );
}

#[test]
fn test_finishing_the_walkthrough_persists_the_gate() {
    let mut app = TestApp::new();
    prepare(&mut app, true);
    let saves = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&saves);
    profile::set_save_listener(Some(Arc::new(move |_p: &Profile| {
        counter.fetch_add(1, Ordering::SeqCst);
    })));
    walk_through(&mut app.ctx);
    profile::set_save_listener(None);

    assert!(app.ctx.profile.as_ref().unwrap().tutorial_done);
    assert_eq!(saves.load(Ordering::SeqCst), 1);
}

#[test]
fn test_the_gate_is_tutorial_done_itself_not_verbosity_history() {
    // A finished walkthrough plus a later flip to terse resurrects nothing,
    // and a fresh terse career still gets the walkthrough: the driving state
    // builds a Tutorial on tutorial_done alone, never on the speech mode.
    let mut app = TestApp::new();
    app.ctx.settings.driving_speech = "quiet".to_string();
    app.ctx.profile = Some(Profile::named_in("Vet", "Buffalo"));
    let route = app
        .ctx
        .world
        .supported_route("Buffalo", "Rochester", None)
        .unwrap()
        .expect("Buffalo to Rochester is a supported route");
    let miles = route.miles();
    let mut job = Job::new(
        cargo_type("general").unwrap(),
        12.0,
        "Buffalo",
        "company yard",
        "Rochester",
        miles,
        1000.0,
        12.0,
    );
    job.destination_location = "Rochester freight market".to_string();

    app.ctx.profile.as_mut().unwrap().tutorial_done = true;
    let veteran = DrivingState::new(
        &mut app.ctx,
        job.clone(),
        route.clone(),
        None,
        DRIVE_PHASE_DELIVERY,
        None,
    );
    assert!(veteran.tutorial.is_none());

    app.ctx.profile.as_mut().unwrap().tutorial_done = false;
    let first_timer = DrivingState::new(&mut app.ctx, job, route, None, DRIVE_PHASE_DELIVERY, None);
    assert!(first_timer.tutorial.is_some());
}

/// Phone walk W2: "Air ready." and "Reminder: air is ready. ..." landed a
/// second apart, the reminder timed from the engine start. Air coming up is
/// its own line, so the reminder waits from it.
#[test]
fn test_air_ready_is_not_followed_by_its_own_reminder() {
    let mut app = TestApp::new();
    prepare(&mut app, false);
    let mut tutorial = Tutorial::new();
    tutorial.begin(&mut app.ctx);
    tutorial.on_engine_started(&mut app.ctx);
    let parked = TruckState {
        primary_air_psi: 120.0,
        secondary_air_psi: 120.0,
        ..truck_at(0.0, true)
    };
    tutorial.update(&mut app.ctx, 24.5, &parked);
    app.clear_speech();
    tutorial.on_air_ready();
    tutorial.update(&mut app.ctx, 1.0, &parked);
    assert!(
        !app.main_lines()
            .iter()
            .any(|line| line.starts_with("Reminder")),
        "{:?}",
        app.main_lines()
    );
    tutorial.update(&mut app.ctx, 25.0, &parked);
    let lines = app.main_lines();
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert!(lines[0].starts_with("Reminder: air is ready."), "{lines:?}");
}
