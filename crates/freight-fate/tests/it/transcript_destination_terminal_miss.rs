//! Blowing the stop at the end of the destination ramp loops back
//! (port of `tests/test_destination_terminal_miss.py`).
//!
//! A destination terminal with no street chain used to simply wait when the
//! truck rolled past it at speed: the arrival line was spoken once, the ramp
//! counted down past it forever, and nothing else was ever said. A player
//! circled the ramp for minutes with the route status frozen and quit to the
//! menu (owner playtest, Buffalo to Albany, 2026-08-12). It now loops back
//! through the next safe turnaround like every other missed stop on the route.
//!
//! Two seams differ from the Python file, both deliberately:
//!
//! * Python patched `ctx.say_event` and read `(text, interrupt)` off the
//!   patch. Here the lines come off `ctx.speech`, below the driving verbosity
//!   ladder and the event pacer -- what a player actually hears. Every line
//!   asserted here is navigation-priority arrival speech that no rung
//!   silences, so the expectations are unchanged.
//! * Python monkeypatched `_open_facility_arrival` to a recorder. A Rust
//!   method cannot be swapped, so the drive is on the stack and the arrival
//!   is asserted by what it DID: the latch it sets and the pull-in beat it
//!   replaces the drive with.

use ff_core::message_log::MessageCategory;
use ff_core::models::jobs::{Job, CARGO_CATALOG};
use ff_core::models::profile::Profile;
use ff_core::sim::trip_models::RoadStop;
use freight_fate::app::testing::{FakeClock, TestApp};
use freight_fate::app::{share, GameContext, SharedState};
use freight_fate::states::base::TimedMessageState;
use freight_fate::states::driving::DrivingState;
use freight_fate::states::driving_core::{
    DRIVE_PHASE_DELIVERY, RAMP_OVERSHOOT_MI, RAMP_TERMINAL_MISS_LOOP_MIN, STOP_PULL_IN_MIN,
};

const MPS_PER_MPH: f64 = 1.0 / 2.23694;

fn approx(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-6 * b.abs().max(1.0)
}

/// `_driving(app)`: Buffalo to Rochester, delivering to the freight market.
///
/// The drive goes on the stack (without entering) so the arrival path can
/// really open its menu; Python left it off the stack and patched the one
/// case that would have needed it.
fn a_drive(app: &mut TestApp) -> (SharedState, FakeClock) {
    // The event pacer budgets in REAL seconds and refuses a repeat inside
    // `REPEAT_WINDOW_S`. Python never met that rule, because its patch sat
    // above the pacer. Back-to-back `update_exit` calls here would look to
    // the pacer like the same second, so the re-approach arrival -- minutes
    // of looping back later for a player -- would be silenced as an echo.
    // Give the pacer the simulated clock and advance it where the drive
    // really spends time.
    let clock = app.fake_pacer_clock();
    let mut profile = Profile::named_in("Terminal", "Buffalo");
    profile.tutorial_done = true;
    app.ctx.profile = Some(profile);
    let route = app
        .ctx
        .world
        .supported_route("Buffalo", "Rochester", None)
        .expect("the world routes")
        .expect("Buffalo to Rochester is supported");
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
    // The market itself HAS a baked facility street chain, which is why
    // Python monkeypatched `_surface_chain_route` away. An unmapped dock is
    // the honest way to get the same road here: no approach route is baked
    // for it, so `surface_chain_route` answers None the way the patch did.
    job.destination_location = "Rochester unmapped dock".to_string();
    let mut driving = DrivingState::new(
        &mut app.ctx,
        job,
        route,
        Some(0),
        DRIVE_PHASE_DELIVERY,
        None,
    );
    driving.trip.set_npc_vehicles(Vec::new());
    // `_surface_chain_route = lambda: None` in Python: this file is about the
    // facility that has no street chain, and the freight market is one.
    assert!(
        driving.surface_chain_route(&app.ctx).is_none(),
        "the Rochester freight market must have no facility street chain"
    );
    let shared = share(driving);
    app.ctx.push_shared_with(shared.clone(), false);
    app.ctx.run_deferred();
    (shared, clock)
}

/// Run `f` on the drive behind `shared`, then drain the deferred stack work.
fn with_drive<R>(
    app: &mut TestApp,
    shared: &SharedState,
    f: impl FnOnce(&mut DrivingState, &mut GameContext) -> R,
) -> R {
    let mut borrowed = shared.borrow_mut();
    let drive = borrowed
        .as_any_mut()
        .downcast_mut::<DrivingState>()
        .expect("the handle is a DrivingState");
    let out = f(drive, &mut app.ctx);
    drop(borrowed);
    app.ctx.run_deferred();
    out
}

fn read_drive<R>(shared: &SharedState, f: impl FnOnce(&DrivingState) -> R) -> R {
    let borrowed = shared.borrow();
    f(borrowed
        .as_any()
        .downcast_ref::<DrivingState>()
        .expect("the handle is a DrivingState"))
}

/// Every line the drive SUBMITTED to the event channel, with the interrupt
/// flag it carried -- the view Python's `say_event` patch had.
///
/// The capture sits one rung BELOW `say_event`, on the channel itself, and
/// there a line an interrupt purged mid-sentence is handed back to finish
/// behind the interrupter. That requeue is real -- a player hears the arrival
/// line resume after the loop-back cuts in -- but it is not a second
/// submission, and "the loop-back speaks once, not every frame" is about
/// submissions. The review log records exactly one entry per submission and
/// no requeues, so the sequence is read there and the flag off the channel.
fn submissions(app: &TestApp) -> Vec<(String, bool)> {
    let calls = app.event_calls();
    app.ctx
        .message_log
        .messages
        .iter()
        .filter(|message| message.category == MessageCategory::Event)
        .map(|message| {
            let interrupt = calls
                .iter()
                .find(|(text, _)| *text == message.text)
                .map(|(_, interrupt)| *interrupt)
                .unwrap_or(false);
            (message.text.clone(), interrupt)
        })
        .collect()
}

/// `_at_the_terminal`: end of a no-chain destination ramp, terminal cleared,
/// still rolling.
fn at_the_terminal(app: &mut TestApp, shared: &SharedState, mph: f64) -> RoadStop {
    let mut stop = RoadStop::new("the Rochester freight market", 10.0, "delivery_destination");
    stop.actions = vec!["deliver".to_string()];
    let staged = stop.clone();
    with_drive(app, shared, move |d, _ctx| {
        d.destination_exit_taken = true;
        d.ramp_stop = Some(staged);
        d.ramp_mi = Some(0.0);
        d.ramp_control = "none".to_string();
        d.ramp_terminal_done = true;
        d.ramp_light_announced = true;
        d.truck_mut().set_air_ready(false);
        d.truck_mut().start_engine();
        d.truck_mut().velocity_mps = mph * MPS_PER_MPH;
    });
    stop
}

/// `_arrival_line(stop)`.
fn arrival_line(stop: &RoadStop) -> (String, bool) {
    (format!("At {}. Come to a stop.", stop.name), true)
}

#[test]
fn test_blown_destination_terminal_loops_back() {
    let mut app = TestApp::new();
    let (shared, clock) = a_drive(&mut app);
    let stop = at_the_terminal(&mut app, &shared, 36.0);
    let minutes = read_drive(&shared, |d| d.trip.game_minutes);

    with_drive(&mut app, &shared, |d, ctx| d.update_exit(ctx, 0.0, 0.0));
    assert_eq!(
        submissions(&app).last().cloned(),
        Some(arrival_line(&stop)),
        "{:?}",
        submissions(&app)
    );
    let spoken = submissions(&app).len();
    let grace = read_drive(&shared, |d| d.ramp_arrival_grace_s);
    assert!(
        grace > 0.0,
        "the destination arrival must open a reaction window"
    );

    // Distance alone cannot consume the spoken reaction window.
    with_drive(&mut app, &shared, |d, ctx| {
        d.update_exit(ctx, RAMP_OVERSHOOT_MI, grace - 0.1)
    });
    assert!(read_drive(&shared, |d| d.ramp_mi).expect("still on the ramp") <= -RAMP_OVERSHOOT_MI);
    assert_eq!(submissions(&app).len(), spoken);
    assert!(approx(
        read_drive(&shared, |d| d.trip.game_minutes),
        minutes
    ));

    with_drive(&mut app, &shared, |d, ctx| d.update_exit(ctx, 0.0, 0.2));
    assert_eq!(
        submissions(&app).len(),
        spoken + 1,
        "the loop-back speaks once, not every frame"
    );
    let (line, interrupt) = submissions(&app).last().cloned().expect("a line");
    assert!(interrupt);
    assert!(line.contains("It is ahead again."), "{line}");
    assert!(line.contains(&stop.name), "{line}");
    assert!(approx(
        read_drive(&shared, |d| d.trip.game_minutes),
        minutes + RAMP_TERMINAL_MISS_LOOP_MIN
    ));

    // The entrance is ahead again, and it announces fresh on the re-approach.
    assert_eq!(
        read_drive(&shared, |d| d.ramp_stop.as_ref().map(|s| s.key())),
        Some(stop.key())
    );
    let ramp_mi = read_drive(&shared, |d| d.ramp_mi).expect("the ramp is ahead again");
    assert!(ramp_mi > 0.0);
    assert!(!read_drive(&shared, |d| d.ramp_end_said));
    assert!(!read_drive(&shared, |d| d.speed_control_armed));
    // The loop-back is a real maneuver through the next safe turnaround --
    // `RAMP_TERMINAL_MISS_LOOP_MIN` of game time -- so the re-approach is
    // nowhere near the pacer's repeat window.
    clock.advance(60.0);
    with_drive(&mut app, &shared, |d, ctx| d.update_exit(ctx, ramp_mi, 0.0));
    assert_eq!(submissions(&app).last().cloned(), Some(arrival_line(&stop)));
}

#[test]
fn test_second_blown_terminal_loops_again_and_names_the_brake() {
    let mut app = TestApp::new();
    let (shared, clock) = a_drive(&mut app);
    let stop = at_the_terminal(&mut app, &shared, 36.0);
    let minutes = read_drive(&shared, |d| d.trip.game_minutes);

    with_drive(&mut app, &shared, |d, ctx| d.update_exit(ctx, 0.0, 0.0)); // the arrival line
    let grace = read_drive(&shared, |d| d.ramp_arrival_grace_s);
    with_drive(&mut app, &shared, |d, ctx| {
        d.update_exit(ctx, RAMP_OVERSHOOT_MI, grace + 1.0)
    });
    let first = submissions(&app).last().expect("a line").0.clone();
    assert!(first.contains("It is ahead again."), "{first}");
    assert!(!first.contains("Brake with"), "{first}");

    let ramp_mi = read_drive(&shared, |d| d.ramp_mi).expect("the ramp is ahead again");
    clock.advance(60.0); // a whole loop back around, not an echo
    with_drive(&mut app, &shared, |d, ctx| d.update_exit(ctx, ramp_mi, 0.0)); // fresh arrival
    assert_eq!(submissions(&app).last().cloned(), Some(arrival_line(&stop)));
    let grace = read_drive(&shared, |d| d.ramp_arrival_grace_s);
    with_drive(&mut app, &shared, |d, ctx| {
        d.update_exit(ctx, RAMP_OVERSHOOT_MI, grace + 1.0)
    });

    let second = submissions(&app).last().expect("a line").0.clone();
    assert!(second.contains("It is ahead again."), "{second}");
    assert!(
        second.contains("Brake with"),
        "a repeat miss earns help, not silence: {second}"
    );
    assert_eq!(read_drive(&shared, |d| d.ramp_terminal_miss_count), 2);
    assert!(read_drive(&shared, |d| d.ramp_mi).expect("on the ramp") > 0.0);
    assert!(approx(
        read_drive(&shared, |d| d.trip.game_minutes),
        minutes + 2.0 * RAMP_TERMINAL_MISS_LOOP_MIN
    ));
}

#[test]
fn test_stopping_before_the_overshoot_still_opens_the_arrival() {
    let mut app = TestApp::new();
    let (shared, _clock) = a_drive(&mut app);
    let stop = at_the_terminal(&mut app, &shared, 36.0);
    let minutes = read_drive(&shared, |d| d.trip.game_minutes);

    with_drive(&mut app, &shared, |d, ctx| d.update_exit(ctx, 0.0, 0.0));
    assert_eq!(submissions(&app).last().cloned(), Some(arrival_line(&stop)));

    // Rolled a little past the end of the ramp, but stopped inside the
    // overshoot distance: that is an arrival, not a miss.
    with_drive(&mut app, &shared, |d, ctx| {
        d.truck_mut().velocity_mps = 0.0;
        d.update_exit(ctx, 0.2, 1.0);
    });

    // Python's `opened == [True]`. `_open_facility_arrival` cannot be
    // replaced here, so the assertion is what it does: it latches the menu
    // open (a second call returns at that latch) and replaces the drive on
    // the stack with the spoken pull-in beat.
    assert!(read_drive(&shared, |d| d.arrival_menu_open));
    assert!(app
        .ctx
        .state()
        .is_some_and(|state| state.borrow().as_any().is::<TimedMessageState>()));
    assert!(read_drive(&shared, |d| d.ramp_mi).is_none());
    assert!(read_drive(&shared, |d| d.ramp_stop.is_none()));
    assert!(read_drive(&shared, |d| d.trip.finished));
    // Python asserted the clock had not moved at all, because it had patched
    // `_open_facility_arrival` out. The real arrival charges its own pull-in
    // beat; what this case is about is that NO miss loop was charged.
    assert!(approx(
        read_drive(&shared, |d| d.trip.game_minutes),
        minutes + STOP_PULL_IN_MIN
    ));
    assert_eq!(read_drive(&shared, |d| d.ramp_terminal_miss_count), 0);
}

#[test]
fn test_facility_arrival_settles_the_engine_to_idle() {
    // The destination dock gate must not freeze engine audio at whatever rev
    // the truck was carrying on the approach -- same defect class as the
    // already-fixed police-stop freeze.
    let mut app = TestApp::new();
    let (shared, _clock) = a_drive(&mut app);
    at_the_terminal(&mut app, &shared, 36.0);
    with_drive(&mut app, &shared, |d, ctx| {
        // Simulate revs still up from the highway.
        d.truck_mut().rpm = d.truck().specs.max_rpm;
        d.open_facility_arrival(ctx);
    });

    assert!(approx(
        read_drive(&shared, |d| d.truck().rpm),
        read_drive(&shared, |d| d.truck().specs.idle_rpm)
    ));
    assert!(approx(read_drive(&shared, |d| d.truck().throttle), 0.0));
}

#[test]
fn test_speed_control_waits_out_a_pending_ramp_stop() {
    // Automatic speed control must not resume onto a ramp with a stop on it.
    // Taking the exit cancels cruise outright; the resume helper re-engaged it
    // a frame later and drove the playtest straight past the entrance.
    //
    // Python pinned the road with `trip.speed_limit_at = lambda mile: (65.0,
    // None)`. There is no such seam here, so the truck is put on a stretch of
    // the real corridor instead; the case turns on the ramp, not the number.
    let mut app = TestApp::new();
    let (shared, _clock) = a_drive(&mut app);
    with_drive(&mut app, &shared, |d, ctx| {
        d.truck_mut().set_air_ready(false);
        d.truck_mut().start_engine();
        d.truck_mut().velocity_mps = 25.0;
        d.restore_speed_control_session(ctx, true, Some(55.0));

        d.ramp_mi = Some(0.3); // rolling down the ramp toward the stop
        d.resume_speed_control_if_ready(ctx, false);
    });
    assert!(read_drive(&shared, |d| d.cruise_mph).is_none());
    assert!(read_drive(&shared, |d| d.keeper_mph).is_none());
    // Still armed, just not engaging here.
    assert!(read_drive(&shared, |d| d.speed_control_armed));

    with_drive(&mut app, &shared, |d, ctx| {
        d.ramp_mi = None; // the arrival settled and the ramp is behind the truck
        d.resume_speed_control_if_ready(ctx, false);
    });
    assert!(approx(
        read_drive(&shared, |d| d.cruise_mph).expect("cruise re-engaged"),
        55.0
    ));
}
