//! Taking (and missing) a route exit: the signal, the exit lane, the gore,
//! the destination exit's automation, and the exit speed assist.
//!
//! Ported from `tests/test_driving_exits.py`.
//!
//! # What replaced the Python rigging
//!
//! | Python | here |
//! |---|---|
//! | `start_drive(app)` + `quiet_trip` | [`crate::transcript_cruise_support::start_drive`] (the same new career, assigned dispatch and departure) plus that module's `quiet` |
//! | `HeldKeys(pygame.K_RIGHT)` handed to `_update_exit_preparation` | `hold` / `release_keys` writing the drive's real held-key set, which is where `update_exit_preparation` reads them |
//! | `monkeypatch.setattr(ctx, "say", ...)` | the harness capture at `ctx.speech`, one rung below the ladder and the pacer, so these assert what a player HEARS |
//! | `driving.trip.speed_limit_at = lambda m: (60, None)` | a `bench_road` baking that posted number onto the leg, which is the record `speed_limit_at` reads |
//! | `driving._begin_surface_chain = lambda: False` | the destination's own chain memo is answered False, which is the flag the chain consults |
//! | source-text `inspect.getsource` assertions | driven through the behaviour the source line produces -- see the two cases that say so |
//!
//! Two cases here run a real frame loop over a real dispatch, which is
//! deliberate and expensive: every earlier Python version of
//! `test_the_destination_approach_assist_actually_brings_the_truck_to_a_stop`
//! passed against a stand-in while the game drove straight past the market.

use ff_core::sim::trip_models::{
    RoadStop, TrafficPressure, TripEvent, TripEventData, TripEventKind,
};

use freight_fate::app::testing::AudioLog;
use freight_fate::playtest::harness::PlaytestHarness;
use freight_fate::states::base::Key;
use freight_fate::states::driving::DrivingState;
use freight_fate::states::driving_core::{
    DESTINATION_EXIT_SCAN_WINDOW_MI, DOCKING_MAX_MPH, EXIT_COMMIT_WINDOW_MI, EXIT_TAPER_MI,
    RAMP_ACCESS_MI,
};
use freight_fate::states::driving_rest_states::{ParkingFullState, RestStopState};

use crate::transcript_cruise_support::{
    bench_road, bench_road_with, frame, hold, quiet, release_keys, spoken, start_drive, DT,
    MPS_PER_MPH,
};

// -- rigging -------------------------------------------------------------------------

/// `start_drive(app)` + `quiet_trip(driving)`.
fn a_drive(name: &str) -> PlaytestHarness {
    let mut harness = a_drive_noisy(name);
    harness.with_drive(|d, _| quiet(&mut d.trip));
    harness.clear_speech();
    harness
}

/// The same drive with the road left as dispatch drew it, for the cases that
/// place their own traffic pressures (`quiet` clears them).
fn a_drive_noisy(name: &str) -> PlaytestHarness {
    let mut harness = start_drive(name);
    harness.with_drive(|d, _| {
        d.trip.set_npc_vehicles(Vec::new());
        d.trip.traffic_manager.rolling_bubble = false;
        d.trip.hazard_check_mi = 1e9;
        d.trip.inspection_check_mi = 1e9;
    });
    harness.clear_speech();
    harness
}

/// `sounds` in the Python cases: the `(key, volume)` pairs `ctx.audio.play`
/// was handed, in order.
fn played(log: &AudioLog) -> Vec<(String, f64)> {
    log.borrow()
        .played
        .iter()
        .map(|(key, volume, _pan)| (key.clone(), *volume))
        .collect()
}

fn said_any(harness: &PlaytestHarness, needle: &str) -> bool {
    spoken(harness).iter().any(|line| line.contains(needle))
}

/// Where a freshly taken ramp starts counting down from: this exit's own
/// gore-to-bar length, then the stretch to the driveway.
fn ramp_starts_at(harness: &PlaytestHarness, stop: &RoadStop) -> f64 {
    let stop = stop.clone();
    harness.read_drive(move |d| d.trip.ramp_length_mi(&stop)) + RAMP_ACCESS_MI
}

fn said_count(harness: &PlaytestHarness, needle: &str) -> usize {
    spoken(harness)
        .iter()
        .filter(|line| line.contains(needle))
        .count()
}

/// `driving.handle_event(key_event(pygame.K_x))`.
fn press_x(harness: &mut PlaytestHarness) {
    harness.press_key(Key::X, None);
}

/// The first en-route stop (`driving.trip.stops[0]`).
fn first_stop(harness: &PlaytestHarness) -> RoadStop {
    harness.read_drive(|d| {
        d.trip
            .stops
            .first()
            .cloned()
            .expect("the assigned route carries en-route stops")
    })
}

fn destination_exit(harness: &mut PlaytestHarness) -> RoadStop {
    harness.with_drive(|d, ctx| {
        d.destination_exit_stop(ctx)
            .expect("a delivery always has a destination exit")
    })
}

/// `_pressure_speech(driving, spoken)`: everything a pressure got to say --
/// spoken now or queued to speak. Both count as reaching the driver.
fn pressure_speech(harness: &PlaytestHarness) -> Vec<String> {
    let mut heard = spoken(harness);
    heard.extend(harness.read_drive(|d| {
        d.pending_ambient_events
            .iter()
            .map(|p| p.message.clone())
            .collect::<Vec<_>>()
    }));
    heard
}

/// `_pressure_event(driving, pressure, ahead)`: the GPS cue the trip emits
/// for a pressure, built by the trip itself and handed straight to the drive.
///
/// Handed over rather than waited for over frames: a live tick also carries
/// stop callouts and CB chatter that share the ambient slot, and which of them
/// lands first is not what these cases are about.
fn pressure_event(harness: &mut PlaytestHarness, pressure: &TrafficPressure) -> TripEvent {
    let pressure = pressure.clone();
    harness.read_drive(move |d| TripEvent {
        kind: TripEventKind::GpsCue,
        message: d.trip.traffic_pressure_message(&pressure, 1.0),
        data: TripEventData {
            traffic_pressure: Some(pressure),
            ..Default::default()
        },
    })
}

fn a_pressure(
    start_mi: f64,
    end_mi: f64,
    kind: &str,
    direction: &str,
    reason: &str,
) -> TrafficPressure {
    TrafficPressure {
        start_mi,
        end_mi,
        kind: kind.to_string(),
        direction: direction.to_string(),
        intensity: 0.75,
        target_speed_mph: 42.0,
        reason: reason.to_string(),
    }
}

/// `_exit_pressure_run(app)`: a drive with an exit-traffic pressure over the
/// next route exit.
fn exit_pressure_run(name: &str) -> (PlaytestHarness, RoadStop, TrafficPressure) {
    let mut harness = a_drive_noisy(name);
    let stop = first_stop(&harness);
    let pressure = a_pressure(
        stop.at_mi - 2.0,
        stop.at_mi + 0.4,
        "exit",
        "right",
        &format!("exit traffic for {}", stop.spoken_name()),
    );
    let armed = pressure.clone();
    let at = stop.at_mi;
    harness.with_drive(move |d, _| {
        d.trip.traffic_pressures = vec![armed];
        d.trip.announced_traffic_pressures.clear();
        d.trip.position_mi = at - 3.0;
        d.truck_mut().velocity_mps = 25.0;
        d.pending_ambient_events.clear();
        d.ambient_event_cooldown_s = 0.0;
    });
    harness.clear_speech();
    (harness, stop, pressure)
}

// -- the signal ------------------------------------------------------------------------

#[test]
fn test_can_back_up_to_a_missed_rest_stop_with_t_menu() {
    let mut harness = a_drive("Exits");
    let stop = first_stop(&harness);
    harness.with_drive(move |d, _| {
        d.trip.position_mi = stop.at_mi + 0.7;
        d.truck_mut().velocity_mps = -1.0;
        d.trip.update(60.0);
        d.truck_mut().velocity_mps = 0.0;
        assert!((d.trip.position_mi - stop.at_mi).abs() <= 1.5);
    });

    harness.press_key(Key::T, None);

    assert!(
        harness.state_is::<RestStopState>() || harness.state_is::<ParkingFullState>(),
        "T at a stop just behind the truck must open the stop"
    );
}

#[test]
fn test_x_signals_for_upcoming_route_exit_without_taking_it() {
    let mut harness = a_drive("Exits");
    harness.app.ctx.settings.lane_keeping = "partial".to_string();
    let stop = destination_exit(&mut harness);
    let at = stop.at_mi;
    harness.with_drive(move |d, _| d.trip.position_mi = at - 1.5);
    harness.clear_speech();

    press_x(&mut harness);

    harness.read_drive(|d| {
        let armed = d.exit_stop.as_ref().expect("X arms the destination exit");
        assert_eq!(armed.stop_type, "delivery_destination");
        assert!(d.exit_signal_on);
    });
    assert!(said_any(&harness, "Signal "), "{:?}", spoken(&harness));

    press_x(&mut harness);

    harness.read_drive(|d| {
        assert!(d.exit_stop.is_none());
        assert!(!d.exit_signal_on);
    });
    assert!(
        said_any(&harness, "Signal canceled"),
        "{:?}",
        spoken(&harness)
    );
}

#[test]
fn test_x_near_the_exit_keeps_the_signal_until_a_second_press() {
    let mut harness = a_drive("Exits");
    harness.app.ctx.settings.lane_keeping = "off".to_string();
    let stop = destination_exit(&mut harness);
    let at = stop.at_mi;
    harness.with_drive(move |d, _| d.trip.position_mi = at - 1.5);
    harness.clear_speech();
    press_x(&mut harness);
    assert!(harness.read_drive(|d| d.exit_signal_on));

    // Inside the guard mile a stray press keeps the signal and says so; a
    // playtested X meant as "confirm" must not throw the exit away.
    harness.with_drive(move |d, _| d.trip.position_mi = at - 0.5);
    press_x(&mut harness);
    assert!(harness.read_drive(|d| d.exit_signal_on));
    assert!(
        said_any(&harness, "Signal stays on"),
        "{:?}",
        spoken(&harness)
    );
    assert!(!said_any(&harness, "Signal canceled"));

    // A deliberate second press still cancels.
    press_x(&mut harness);
    assert!(!harness.read_drive(|d| d.exit_signal_on));
    assert!(
        said_any(&harness, "Signal canceled"),
        "{:?}",
        spoken(&harness)
    );
}

#[test]
fn test_right_taps_with_drift_on_earn_the_hold_hint_once() {
    let mut harness = a_drive("Exits");
    harness.app.ctx.settings.lane_keeping = "off".to_string();
    let stop = destination_exit(&mut harness);
    let at = stop.at_mi;
    harness.with_drive(move |d, _| d.trip.position_mi = at - 1.5);
    press_x(&mut harness);
    assert!(harness.read_drive(|d| d.exit_signal_on));
    // Inside the taper, where the exit lane is open and a steer is asked for.
    harness.with_drive(move |d, ctx| {
        d.trip.position_mi = at - EXIT_TAPER_MI / 2.0;
        d.update_exit_preparation(ctx, DT);
    });
    assert!(harness.read_drive(|d| d.lane.exit_lane_open));
    harness.clear_speech();

    for _ in 0..2 {
        hold(&mut harness, &[Key::Right]);
        harness.with_drive(|d, ctx| d.update_exit_preparation(ctx, DT));
        release_keys(&mut harness);
        harness.with_drive(|d, ctx| d.update_exit_preparation(ctx, DT));
    }
    assert_eq!(
        said_count(&harness, "Hold the Right arrow to steer"),
        1,
        "{:?}",
        spoken(&harness)
    );

    // Further taps stay quiet: the hint speaks once per approach.
    hold(&mut harness, &[Key::Right]);
    harness.with_drive(|d, ctx| d.update_exit_preparation(ctx, DT));
    release_keys(&mut harness);
    harness.with_drive(|d, ctx| d.update_exit_preparation(ctx, DT));
    assert_eq!(said_count(&harness, "Hold the Right arrow to steer"), 1);
}

#[test]
fn test_x_without_route_exit_reports_no_signal_target() {
    let mut harness = a_drive("Exits");
    harness.with_drive(|d, ctx| {
        d.trip.position_mi = 0.0;
        // The randomly assigned route may open with a truck stop inside the
        // signal window (a real Ubuntu CI draw had one at mile 1.0); clear
        // the en-route stops so "no exit target" is a property of the test,
        // not of the draw. The destination exit stays far beyond the window.
        d.trip.stops.clear();
        assert!(d.upcoming_exit_stop(ctx).is_none());
    });
    harness.clear_speech();

    press_x(&mut harness);

    assert!(!harness.read_drive(|d| d.exit_signal_on));
    assert!(
        said_any(&harness, "No route exit to signal for yet"),
        "{:?}",
        spoken(&harness)
    );
}

#[test]
fn test_canceled_exit_signal_does_not_prompt_lane_prep() {
    let mut harness = a_drive("Exits");
    harness.app.ctx.settings.lane_keeping = "partial".to_string();
    let stop = first_stop(&harness);
    let at = stop.at_mi;
    harness.with_drive(move |d, _| d.trip.position_mi = at - 1.5);

    press_x(&mut harness);
    press_x(&mut harness);
    harness.read_drive(|d| {
        assert!(d.exit_stop.is_none());
        assert!(!d.exit_signal_on);
    });

    harness.clear_speech();
    hold(&mut harness, &[Key::Right]);
    harness.with_drive(|d, ctx| d.update_exit_preparation(ctx, 1.5));

    assert!(
        !said_any(&harness, "Signal is on"),
        "{:?}",
        spoken(&harness)
    );
    assert!(
        !said_any(&harness, "Exit lane set"),
        "{:?}",
        spoken(&harness)
    );
}

#[test]
fn test_canceled_destination_exit_signal_stays_on_highway() {
    let mut harness = a_drive("Exits");
    harness.app.ctx.settings.lane_keeping = "full".to_string();
    let stop = destination_exit(&mut harness);
    let at = stop.at_mi;
    harness.with_drive(move |d, _| {
        d.trip.position_mi = at - 1.0;
        d.truck_mut().velocity_mps = 12.0;
    });
    harness.clear_speech();

    press_x(&mut harness);
    assert!(harness.read_drive(|d| d.exit_lane_ready()));
    // Inside the guard mile, canceling deliberately takes two presses; the
    // first keeps the signal so a stray X cannot throw the exit away.
    press_x(&mut harness);
    assert!(harness.read_drive(|d| d.exit_signal_on));
    press_x(&mut harness);
    let taken = stop.clone();
    assert!(said_any(&harness, "Signal canceled."));
    harness.clear_speech();
    harness.with_drive(move |d, ctx| {
        assert!(d.exit_stop.is_none());
        assert!(!d.exit_signal_on);
        assert!(!d.exit_intent_ready(ctx, &taken));
        assert!(!d.exit_lane_entered);
        assert!(d.cruise_exit_mph.is_none());
        d.check_destination_exit(ctx);
        assert!(
            d.exit_stop.is_none(),
            "the scanner must not recreate the watcher"
        );
        d.trip.position_mi = at - 0.5;
        d.check_destination_exit(ctx);
        d.update_exit(ctx, 0.0, DT);
        assert!(d.exit_stop.is_none());
        d.trip.position_mi = at;
    });
    assert!(
        spoken(&harness).is_empty(),
        "the canceled countdown must stay silent"
    );

    frame(&mut harness, DT);

    assert!(harness.read_drive(|d| d.ramp_mi.is_none()));
    assert!(
        !said_any(&harness, "Missed the exit"),
        "{:?}",
        spoken(&harness)
    );
}

#[test]
fn canceled_destination_exit_can_be_explicitly_signaled_again() {
    let mut harness = a_drive("Rearm canceled exit");
    let stop = destination_exit(&mut harness);
    harness.with_drive(|d, _| {
        d.trip.position_mi = stop.at_mi - 2.0;
        d.truck_mut().velocity_mps = 12.0;
    });
    press_x(&mut harness);
    press_x(&mut harness);
    assert!(harness.read_drive(|d| d.exit_stop.is_none()));
    harness.clear_speech();
    harness.with_drive(|d, ctx| {
        d.check_destination_exit(ctx);
        d.update_exit(ctx, 0.0, DT);
    });
    assert!(spoken(&harness).is_empty());
    press_x(&mut harness);
    assert!(harness.read_drive(|d| d.exit_signal_on));
    assert!(harness.read_drive(|d| d.canceled_exit_key.is_none()));
    assert_eq!(
        harness.read_drive(|d| d.exit_stop.as_ref().map(|s| s.at_mi)),
        Some(stop.at_mi)
    );
}

#[test]
fn test_a_canceled_destination_exit_is_taken_after_the_loop_back() {
    // Canceled, then missed: the loop-back says lane keeping will take the
    // exit this time. The cancel used to outlive the miss, the scanner skipped
    // the same exit again, and the truck looped past it for good.
    let mut harness = a_drive("Loop Back After Cancel");
    harness.app.ctx.settings.lane_keeping = "full".to_string();
    let stop = destination_exit(&mut harness);
    let at = stop.at_mi;
    harness.with_drive(move |d, _| {
        d.trip.position_mi = at - 1.0;
        d.truck_mut().velocity_mps = 12.0;
    });
    press_x(&mut harness);
    press_x(&mut harness);
    press_x(&mut harness);
    assert!(harness.read_drive(|d| d.canceled_exit_key.is_some()));

    harness.with_drive(move |d, ctx| {
        d.handle_missed_destination_exit(ctx);
        d.trip.position_mi = at - 0.5;
        d.check_destination_exit(ctx);
        assert_eq!(d.exit_stop.as_ref().map(|s| s.at_mi), Some(at));
        assert!(d.exit_lane_entered, "lane keeping takes the exit");
    });
}

// -- the destination exit's automation ------------------------------------------------

#[test]
fn test_destination_exit_auto_arms_and_takes_ramp_with_valid_setup() {
    let mut harness = a_drive("Exits");
    harness.app.ctx.settings.lane_keeping = "full".to_string();
    let stop = destination_exit(&mut harness);
    let at = stop.at_mi;
    harness.with_drive(move |d, _| {
        d.trip.position_mi = at - 1.0;
        d.truck_mut().velocity_mps = 15.0;
    });
    harness.clear_speech();

    frame(&mut harness, DT);
    harness.read_drive(|d| {
        let armed = d
            .exit_stop
            .as_ref()
            .expect("the destination exit auto-arms");
        assert_eq!(armed.stop_type, "delivery_destination");
    });

    harness.with_drive(move |d, _| d.trip.position_mi = at);
    frame(&mut harness, DT);

    let ramp = harness.read_drive(|d| d.ramp_mi);
    let start = ramp_starts_at(&harness, &stop);
    assert!(
        ramp.is_some_and(|mi| (mi - start).abs() < 1e-6),
        "the ramp starts at its own length, {start}: {ramp:?}"
    );
    assert!(harness.read_drive(|d| d.destination_exit_taken));
    assert!(
        spoken(&harness)
            .iter()
            .any(|line| line.contains("Exit 342. A quarter mile of ramp")),
        "{:?}",
        spoken(&harness)
    );
}

#[test]
fn test_full_lane_keeping_still_calls_the_destination_exit() {
    // The reported bug: exits took themselves with nothing said. The callout
    // names the exit; what lane keeping does with it is the manual's
    // (owner, 2026-10-14).
    let mut harness = a_drive("Exits");
    harness.app.ctx.settings.lane_keeping = "full".to_string();
    let stop = destination_exit(&mut harness);
    let at = stop.at_mi;
    harness.with_drive(move |d, _| {
        d.trip.position_mi = at - 1.0;
        d.truck_mut().velocity_mps = 15.0;
    });
    harness.clear_speech();

    frame(&mut harness, DT);

    assert!(
        said_any(&harness, "destination exit."),
        "{:?}",
        spoken(&harness)
    );
    // And it asks for no slowing on the mainline: the ramp is braked for
    // past the gore (realistic exit review, 2026-09-24).
    assert!(
        !said_any(&harness, "Slow down for the ramp"),
        "{:?}",
        spoken(&harness)
    );
}

#[test]
fn test_destination_exit_auto_grant_follows_full_lane_keeping() {
    // The auto-grant is keyed on the mode, not on the old string. Under
    // partial or off the destination exit needs the signal like any other.
    //
    // One app at a time: `TestApp` holds the process environment lock for its
    // whole life, so each mode's harness is dropped before the next is built.
    for (mode, expected) in [("full", true), ("partial", false), ("off", false)] {
        let mut harness = a_drive("Exits");
        harness.app.ctx.settings.lane_keeping = mode.to_string();
        let stop = destination_exit(&mut harness);
        let ready = harness.with_drive(move |d, ctx| d.exit_intent_ready(ctx, &stop));
        assert_eq!(ready, expected, "lane keeping {mode}");
        drop(harness);
    }
}

#[test]
fn test_destination_exit_no_longer_requires_x_to_take_ramp() {
    let mut harness = a_drive("Exits");
    harness.app.ctx.settings.lane_keeping = "full".to_string();
    let stop = destination_exit(&mut harness);
    let at = stop.at_mi;
    harness.with_drive(move |d, _| {
        d.trip.position_mi = at - 0.5;
        d.truck_mut().velocity_mps = 12.0;
    });
    harness.clear_speech();

    frame(&mut harness, DT);
    harness.with_drive(move |d, _| d.trip.position_mi = at);
    frame(&mut harness, DT);

    let ramp = harness.read_drive(|d| d.ramp_mi);
    let start = ramp_starts_at(&harness, &stop);
    assert!(ramp.is_some_and(|mi| (mi - start).abs() < 1e-6), "{ramp:?}");
    assert!(
        !said_any(&harness, "Press X to take"),
        "{:?}",
        spoken(&harness)
    );
}

#[test]
fn test_manual_lane_keeping_requires_signal_for_destination_exit() {
    for mode in ["partial", "off"] {
        let mut harness = a_drive("Exits");
        harness.app.ctx.settings.lane_keeping = mode.to_string();
        let stop = destination_exit(&mut harness);
        let at = stop.at_mi;
        harness.with_drive(move |d, _| {
            d.trip.position_mi = at - 1.0;
            d.truck_mut().velocity_mps = 12.0;
            d.exit_lane_entered = true;
        });
        harness.clear_speech();
        frame(&mut harness, DT);

        harness.with_drive(move |d, _| d.trip.position_mi = at);
        frame(&mut harness, DT);

        assert!(harness.read_drive(|d| d.ramp_mi.is_none()), "{mode}");
        assert!(
            said_any(&harness, ", no signal."),
            "{mode}: {:?}",
            spoken(&harness)
        );
        drop(harness);
    }
}

#[test]
fn test_relaxed_lane_drift_infers_destination_exit_intent() {
    let mut harness = a_drive("Exits");
    harness.app.ctx.settings.lane_keeping = "full".to_string();
    let stop = destination_exit(&mut harness);
    let at = stop.at_mi;
    harness.with_drive(move |d, _| {
        d.trip.position_mi = at - 1.0;
        d.truck_mut().velocity_mps = 12.0;
    });
    harness.clear_speech();
    frame(&mut harness, DT);

    harness.with_drive(move |d, _| d.trip.position_mi = at);
    frame(&mut harness, DT);

    let ramp = harness.read_drive(|d| d.ramp_mi);
    let start = ramp_starts_at(&harness, &stop);
    assert!(ramp.is_some_and(|mi| (mi - start).abs() < 1e-6), "{ramp:?}");
    assert!(said_any(&harness, " of ramp"), "{:?}", spoken(&harness));
}

#[test]
fn test_missed_destination_exit_reroutes_every_time() {
    let mut harness = a_drive("Exits");
    harness.app.ctx.settings.lane_keeping = "off".to_string();
    let stop = destination_exit(&mut harness);
    let at = stop.at_mi;
    harness.clear_speech();

    // Missing the destination exit twice must loop back both times; the
    // say-once latch used to swallow the second reposition and strand the
    // trip pinned at the end of the route with no exit left to signal for.
    for _ in 0..2 {
        // Real time between the two misses. The capture sits below the pacer,
        // which drops a line identical to one spoken moments ago -- and both
        // loop-backs happen inside a single simulated instant here, where on
        // the road they are a turnaround apart. Without this the second
        // announcement is swallowed by the repeat window and the say-once
        // latch this case exists to pin would look broken either way round.
        harness.advance_clock(30.0);
        harness.with_drive(|d, _| {
            d.trip.position_mi = d.trip.total_miles();
            d.trip.finished = true;
            d.truck_mut().velocity_mps = 20.0;
        });
        frame(&mut harness, DT);
        harness.read_drive(|d| {
            assert!(!d.trip.finished);
            assert!(d.trip.position_mi < at);
        });
    }
    assert_eq!(
        said_count(&harness, "Missed the destination exit"),
        2,
        "{:?}",
        spoken(&harness)
    );

    // The re-approach leaves the full exit window: a real exit to signal for,
    // far enough out to hear, arm, and brake under time compression.
    assert!(harness.read_drive(|d| at - d.trip.position_mi >= 5.0));
    harness.with_drive(move |d, _| d.trip.position_mi = at - 1.5);
    press_x(&mut harness);
    assert!(harness.read_drive(|d| d.exit_signal_on));
}

#[test]
fn test_a_blown_destination_exit_names_the_loop_back_not_a_later_exit() {
    // Tyler Rodick, Hattiesburg, 2026-08-26: "it never rerouted me". At the
    // gore the game told him to stay on the highway and recover at the next
    // safe exit. For an optional stop that is true. For the DESTINATION exit
    // there is no later exit to recover at: the route runs on to its end and
    // the scripted loop-back through the safe turnaround brings this same
    // exit back. A driver sent looking for another exit is off the route, and
    // the loop-back a mile later reads as a reroute nobody delivered.
    //
    // Both ways of blowing it, because a driver at 89 hits the second one:
    // no signal at all, and signalled and lined up but far too fast.
    for lane_keeping in ["off", "full"] {
        let mut harness = a_drive("Exits");
        harness.app.ctx.settings.lane_keeping = lane_keeping.to_string();
        // No enforcement posts on this road. 89 in a 65 is exactly what earns
        // a trooper, and being pulled over cancels the exit approach ("Exit
        // approach canceled; plan it again after the stop") -- so the run
        // never reaches the gore and the loop-back line never comes. That is
        // the game behaving correctly; it just answers a different question
        // than this case asks. Cleared for the same reason `a_drive_noisy`
        // pushes hazards and inspections out of reach.
        harness.with_drive(|d, _| d.trip.posts.clear());
        let stop = destination_exit(&mut harness);
        let at = stop.at_mi;
        harness.with_drive(move |d, _| {
            d.trip.position_mi = at - 1.0;
            d.truck_mut().velocity_mps = 89.0 * MPS_PER_MPH;
            d.exit_lane_entered = true;
        });
        harness.clear_speech();
        frame(&mut harness, DT);

        harness.with_drive(move |d, _| {
            d.trip.position_mi = at;
            d.truck_mut().velocity_mps = 89.0 * MPS_PER_MPH;
        });
        frame(&mut harness, DT);

        assert!(
            harness.read_drive(|d| d.ramp_mi.is_none()),
            "{lane_keeping}"
        );
        assert!(
            said_any(&harness, "exit comes around again"),
            "{lane_keeping}: {:?}",
            spoken(&harness)
        );
        assert!(
            !said_any(&harness, "Take a later exit"),
            "{lane_keeping}: {:?}",
            spoken(&harness)
        );
        drop(harness);
    }
}

// -- the exit lane ---------------------------------------------------------------------

#[test]
fn test_exit_requires_right_lane_alignment() {
    let mut harness = a_drive("Exits");
    harness.app.ctx.settings.lane_keeping = "partial".to_string();
    let stop = first_stop(&harness);
    let at = stop.at_mi;
    let key = stop.key();
    harness.with_drive(move |d, _| {
        d.trip.traffic_pressures.clear();
        d.trip.position_mi = at - 1.0;
        d.truck_mut().velocity_mps = 15.0;
    });
    harness.clear_speech();
    press_x(&mut harness);
    assert_eq!(
        harness.read_drive(|d| d.exit_stop.as_ref().map(|s| s.key())),
        Some(key)
    );

    // At the gore marker in the right lane, signal on, not yet steered in:
    // the exit lane runs on past the marker, so the exit is still there.
    harness.with_drive(move |d, _| d.trip.position_mi = at);
    frame(&mut harness, DT);
    harness.read_drive(|d| {
        assert!(d.ramp_mi.is_none());
        assert!(d.exit_stop.is_some());
    });

    // Past the end of the gore window without the steer: missed.
    harness.with_drive(move |d, _| d.trip.position_mi = at + EXIT_COMMIT_WINDOW_MI + 0.01);
    frame(&mut harness, DT);

    harness.read_drive(|d| {
        assert!(d.ramp_mi.is_none());
        assert!(d.exit_stop.is_none());
    });
    assert!(
        said_any(&harness, "not in the exit lane"),
        "{:?}",
        spoken(&harness)
    );
}

#[test]
fn test_exit_lane_is_steered_into_where_it_opens() {
    let mut harness = a_drive("Exits");
    harness.app.ctx.settings.lane_keeping = "partial".to_string();
    let stop = first_stop(&harness);
    let at = stop.at_mi;
    harness.with_drive(move |d, _| {
        d.trip.position_mi = at - EXIT_TAPER_MI / 2.0;
        d.truck_mut().velocity_mps = 25.0;
    });
    harness.clear_speech();
    let sounds = harness.app.record_audio();
    press_x(&mut harness);

    hold(&mut harness, &[Key::Right]);
    for _ in 0..(60 * 5) {
        harness.with_drive(|d, ctx| {
            d.update_lane(ctx, DT);
            d.update_exit_preparation(ctx, DT);
        });
        if harness.read_drive(|d| d.exit_lane_ready()) {
            break;
        }
    }

    assert!(harness.read_drive(|d| d.exit_lane_ready()));
    assert_eq!(
        said_count(&harness, "Exit lane opening. Steer right into it."),
        1,
        "{:?}",
        spoken(&harness)
    );
    assert!(
        !said_any(&harness, "Exit lane set"),
        "{:?}",
        spoken(&harness)
    );
    let heard = played(&sounds);
    assert!(heard.contains(&("ui/notify".to_string(), 0.6)), "{heard:?}");
    assert!(
        heard
            .iter()
            .any(|(key, _)| key == "vehicle/lane_line_cross"),
        "{heard:?}"
    );
    assert!(
        !heard.iter().any(|(key, _)| key == "vehicle/collision"),
        "{heard:?}"
    );
}

#[test]
fn test_lane_drift_off_sets_exit_lane_when_signaling() {
    let mut harness = a_drive("Exits");
    harness.app.ctx.settings.lane_keeping = "full".to_string();
    let stop = first_stop(&harness);
    let at = stop.at_mi;
    harness.with_drive(move |d, _| d.trip.position_mi = at - 1.5);
    harness.clear_speech();
    let sounds = harness.app.record_audio();

    press_x(&mut harness);

    assert!(harness.read_drive(|d| d.exit_lane_ready()));
    assert!(said_any(&harness, "Signal "), "{:?}", spoken(&harness));
    assert!(
        !said_any(&harness, "Lane keeping"),
        "{:?}",
        spoken(&harness)
    );
    assert!(!said_any(&harness, "Move"), "{:?}", spoken(&harness));
    assert!(
        played(&sounds).contains(&("ui/notify".to_string(), 0.6)),
        "{:?}",
        played(&sounds)
    );
}

#[test]
fn test_exit_missed_after_gore_window() {
    let mut harness = a_drive("Exits");
    let stop = first_stop(&harness);
    let at = stop.at_mi;
    harness.with_drive(move |d, _| {
        d.trip.position_mi = at - 1.0;
        d.truck_mut().velocity_mps = 10.0;
    });
    harness.clear_speech();
    press_x(&mut harness);
    harness.with_drive(move |d, _| {
        d.exit_lane_entered = true;
        d.trip.position_mi = at + 0.6;
    });

    frame(&mut harness, DT);

    harness.read_drive(|d| {
        assert!(d.ramp_mi.is_none());
        assert!(d.exit_stop.is_none());
    });
    assert!(
        said_any(&harness, "Missed the exit."),
        "{:?}",
        spoken(&harness)
    );
}

// -- exit traffic ------------------------------------------------------------------------

#[test]
fn test_exit_traffic_pressure_changes_missed_lane_recovery() {
    let mut harness = a_drive_noisy("Exits");
    harness.app.ctx.settings.lane_keeping = "partial".to_string();
    let stop = first_stop(&harness);
    let at = stop.at_mi;
    harness.with_drive(move |d, _| {
        d.trip.traffic_pressures = vec![a_pressure(
            at - 2.0,
            at + 0.4,
            "exit",
            "right",
            "exit traffic for test ramp",
        )];
        d.trip.position_mi = at - 1.0;
        d.truck_mut().velocity_mps = 15.0;
    });
    harness.clear_speech();
    press_x(&mut harness);
    harness.with_drive(move |d, _| d.trip.position_mi = at + EXIT_COMMIT_WINDOW_MI + 0.01);

    frame(&mut harness, DT);

    assert!(harness.read_drive(|d| d.ramp_mi.is_none()));
    assert!(
        said_any(&harness, "Boxed out of the exit lane"),
        "{:?}",
        spoken(&harness)
    );
    assert!(
        said_any(&harness, "Take a later exit."),
        "{:?}",
        spoken(&harness)
    );
}

#[test]
fn test_exit_traffic_stays_quiet_for_an_exit_you_are_not_taking() {
    // Owner, 2026-08-15: the game announced the traffic at every exit coming
    // up, none of them the driver's. Every route stop grows an exit-traffic
    // pressure, so a corridor thick with truck stops narrated one after
    // another. Un-signalled, the advisory says nothing at all.
    let (mut harness, _stop, pressure) = exit_pressure_run("Exits");
    assert!(!harness.read_drive(|d| d.exit_signal_on));

    let event = pressure_event(&mut harness, &pressure);
    harness.with_drive(move |d, ctx| d.handle_trip_event(ctx, &event));

    let heard = pressure_speech(&harness);
    assert!(
        !heard.iter().any(|line| line.contains("Exit traffic")),
        "{heard:?}"
    );

    // Marked announced by the trip all the same, so arming the exit late
    // cannot dump a stale advisory afterwards.
    harness.with_drive(|d, _| {
        d.trip.check_traffic_pressures();
        assert!(!d.trip.announced_traffic_pressures.is_empty());
    });
}

#[test]
fn test_exit_traffic_still_speaks_once_you_signal_for_that_exit() {
    // Signal first and the full advisory arrives in time to be useful.
    let (mut harness, stop, pressure) = exit_pressure_run("Exits");
    harness.app.ctx.settings.lane_keeping = "partial".to_string();
    press_x(&mut harness);
    assert_eq!(
        harness.read_drive(|d| d.exit_stop.as_ref().map(|s| s.key())),
        Some(stop.key())
    );
    assert!(harness.read_drive(|d| d.exit_signal_on));
    harness.clear_speech();
    harness.with_drive(|d, _| {
        d.pending_ambient_events.clear();
        d.ambient_event_cooldown_s = 0.0;
    });

    let event = pressure_event(&mut harness, &pressure);
    harness.with_drive(move |d, ctx| d.handle_trip_event(ctx, &event));

    let heard = pressure_speech(&harness);
    assert!(
        heard.iter().any(|line| line.contains("Exit traffic,")),
        "{heard:?}"
    );
    assert!(
        heard.iter().any(|line| line.contains("Hold right")),
        "{heard:?}"
    );
}

#[test]
fn test_merging_and_construction_pressures_still_speak_unsignalled() {
    // Only the exit ones are gated. A merge warns about the road the truck is
    // already on, not a turn-off it is free to ignore.
    let mut harness = a_drive("Exits");
    let at = harness.read_drive(|d| d.trip.position_mi) + 1.6;
    for (kind, direction, phrase) in [
        ("route_merge", "right", "Merging traffic,"),
        ("construction_merge", "left", "Taper squeezing,"),
        ("traffic_pack", "right", "Traffic pack,"),
    ] {
        // Real time between the three cues: these are AMBIENT lines, and
        // AMBIENT is the one priority the pacer's stale rule throws away. Fire
        // all three into a single simulated instant and the third one is
        // dropped for arriving behind a channel that never fell silent, which
        // is a property of a bench that freezes time, not of the road.
        harness.advance_clock(30.0);
        harness.clear_speech();
        harness.with_drive(|d, _| {
            d.pending_ambient_events.clear();
            d.ambient_event_cooldown_s = 0.0;
        });
        let pressure = a_pressure(at, at + 0.6, kind, direction, "test pressure");
        let event = pressure_event(&mut harness, &pressure);
        harness.with_drive(move |d, ctx| d.handle_trip_event(ctx, &event));
        let heard = pressure_speech(&harness);
        assert!(
            heard.iter().any(|line| line.contains(phrase)),
            "{kind}: {heard:?}"
        );
    }
}

// -- the exit speed assist ---------------------------------------------------------------

#[test]
fn test_exit_speed_assist_slows_with_full_lane_keeping() {
    // Regression: the assist sat below the lane-work early return, so it
    // never ran with lane keeping on full -- and the All assists preset
    // selects full, silently disabling an assist it had just turned on.
    let mut harness = a_drive("Exits");
    harness
        .app
        .ctx
        .settings
        .apply_driving_assistance_preset("all");
    assert_eq!(harness.app.ctx.settings.lane_keeping, "full");
    assert!(harness.app.ctx.settings.exit_speed_assist);
    let stop = first_stop(&harness);
    let at = stop.at_mi;
    harness.with_drive(move |d, _| {
        d.trip.position_mi = at - 1.0;
        d.truck_mut().velocity_mps = 29.0; // ~65 mph, well over ramp speed
                                           // Out of the right lane.
        d.lane.lane_count = 2;
        d.lane.lane = 1;
    });
    harness.clear_speech();
    press_x(&mut harness);

    release_keys(&mut harness);
    harness.with_drive(|d, ctx| d.update_exit_preparation(ctx, DT));

    assert!(harness.read_drive(|d| d.truck().brake >= 0.35));
    let slowing: Vec<String> = spoken(&harness)
        .into_iter()
        .filter(|line| line.starts_with("Slowing."))
        .collect();
    assert!(!slowing.is_empty(), "{:?}", spoken(&harness));
    // No lane request at all: lane keeping on full moves right itself, and a
    // tap into a lane it is waiting on is a sideswipe.
    let last = slowing.last().expect("a slowing line");
    assert!(!last.contains("Right arrow"), "{last}");
    assert!(!last.contains("right lane"), "{last}");
}

#[test]
fn test_the_exit_speed_assist_runs_when_lane_keeping_takes_the_exit() {
    // Owner playtest, Denver->Silverthorne, 2026-08-19: "why did all assists
    // not stop at my destination exit?"
    //
    // Because the assist was gated on the exit signal, and the signal is how
    // a DRIVER commits to an exit. With lane keeping automated they never
    // press it -- the game itself says "lane keeping will take this exit" --
    // so the gate switched the assist off for precisely the preset that
    // promises the most help.
    //
    // Python asserted this by reading `_update_exit_preparation`'s source for
    // `lane_is_automated()`. Here it is driven instead: no signal is ever
    // pressed, and the assist has to act anyway.
    let mut harness = a_drive("Exits");
    harness
        .app
        .ctx
        .settings
        .apply_driving_assistance_preset("all");
    assert_eq!(harness.app.ctx.settings.lane_keeping, "full");
    let stop = first_stop(&harness);
    let at = stop.at_mi;
    harness.with_drive(move |d, _| {
        d.exit_stop = Some(stop.clone());
        d.exit_signal_on = false; // automated lane keeping IS the commitment
        d.trip.position_mi = at - 1.0;
        d.truck_mut().velocity_mps = 29.0; // ~65 mph, well over ramp speed
    });
    harness.clear_speech();

    release_keys(&mut harness);
    harness.with_drive(|d, ctx| d.update_exit_preparation(ctx, DT));

    assert!(!harness.read_drive(|d| d.exit_signal_on));
    assert!(
        harness.read_drive(|d| d.truck().brake >= 0.35),
        "the assist never touched the brakes without a signal"
    );
    assert!(said_any(&harness, "Slowing."), "{:?}", spoken(&harness));
}

#[test]
fn test_a_fresh_cruise_session_inherits_an_armed_exit_s_ramp_cap() {
    // The other half of the same miss, and either alone was enough.
    //
    // Cancelling cruise clears the exit cap. On the Denver run the descent
    // cancelled it about a mile from the ramp, the driver re-engaged at 53,
    // and the new session had forgotten the exit -- its own line said
    // "adaptive cruise set at 53 miles per hour" with no "for the ramp" note,
    // which is the tell. The cap belongs to the road ahead, not to whichever
    // cruise session happened to be running when the exit was announced.
    //
    // Python read `_engage_cruise`'s source for `_cruise_exit_mph` and
    // `_armed_ramp_cruise_mph`. Here a fresh session is engaged with an exit
    // armed and the cap is read back off the drive -- which also pins that the
    // number is the EXIT's, not one constant for every ramp in the country.
    let mut harness = a_drive("Exits");
    let stop = first_stop(&harness);
    let armed = stop.clone();
    harness.with_drive(move |d, _| {
        d.trip.position_mi = armed.at_mi - 1.0;
        d.exit_stop = Some(armed.clone());
        d.exit_signal_on = true;
        d.cruise_exit_mph = None; // the descent cancelled the old session
        d.truck_mut().start_engine();
        d.truck_mut().set_air_ready(false);
        d.truck_mut().velocity_mps = 53.0 * MPS_PER_MPH;
    });

    // The exit's floor for the mainline, ten under road speed at most, or the
    // 53 the driver set if that is lower (realistic exit, 2026-09-24).
    let expected =
        harness.with_drive(move |d, _| 53.0f64.min(d.exit_approach_floor_mph(Some(&stop))));
    harness.with_drive(|d, ctx| d.engage_cruise(ctx, 53.0, false));

    assert_eq!(
        harness.read_drive(|d| d.cruise_exit_mph),
        Some(expected),
        "a fresh session forgot the exit it is driving toward"
    );
}

#[test]
fn test_the_ramp_cap_holds_road_speed_until_the_exit() {
    // Owner playtest, 2026-08-21: heard "adaptive cruise will ease to 40 for
    // the ramp" five miles from the exit and reported the truck slowing early.
    // It was not slowing: the cap holds road speed until about half a mile
    // out and only then sheds. The line is gone (owner, 2026-10-14); the
    // behaviour it described is pinned here.
    let mut harness = a_drive("Exits");
    harness.with_drive(|d, _| {
        d.truck_mut().velocity_mps = 65.0 * MPS_PER_MPH;
        d.cruise_mph = Some(65.0);
    });
    harness.with_drive(|d, _| {
        let stop = RoadStop::new("Test Plaza", 100.0, "travel_center");
        let ramp = 65.0f64.min(d.exit_approach_floor_mph(Some(&stop)));
        d.exit_stop = Some(stop);
        d.cruise_exit_mph = Some(ramp);
        d.trip.position_mi = 95.0;
        assert!(
            d.ramp_approach_cap_mph().is_none_or(|cap| cap > 65.0),
            "cruise is untouched five miles out"
        );
        d.trip.position_mi = 99.9;
        assert_eq!(d.ramp_approach_cap_mph(), Some(ramp));
    });
}

#[test]
fn test_the_exit_assist_leaves_cruise_alone_while_it_has_nothing_to_shed() {
    // Owner, Spokane, twice (2026-08-21 and -22): on the run-in to the
    // destination exit the status said "automatic speed control paused" and
    // the truck ran from 60 to 69 down a 3.7 percent grade.
    //
    // The exit speed assist paused speed control the moment the exit came
    // inside its reach, whether or not it had anything to brake for. That was
    // right for the old flat 45, where every truck at road speed was over it.
    // Now that the gore accepts road speed a truck at the limit is UNDER the
    // gate, so the assist paused cruise and then did nothing -- and with
    // cruise gone, nothing held the grade. It must leave a holding controller
    // alone until it has work, and only then take the pedals.
    let mut harness = a_drive("Exits");
    harness.app.ctx.settings.exit_speed_assist = true;
    // `trip.speed_limit_at = lambda _: (60.0, None)` done for real: a bench
    // leg posted at 60 with no grade, which is the record `speed_limit_at`
    // reads.
    harness.with_drive(|d, _| {
        bench_road(d, 60.0, 0.0, 1.0);
        d.trip.position_mi = 200.0;
        d.truck_mut().set_air_ready(false);
        d.truck_mut().start_engine();
        d.truck_mut().transmission.automatic = true;
        d.truck_mut().transmission.gear = 10;
        d.truck_mut().velocity_mps = 60.0 * MPS_PER_MPH;
    });
    let stop = harness.read_drive(|d| {
        RoadStop::new(
            "Downgrade Travel Plaza",
            d.trip.position_mi + 1.0,
            "truck_stop",
        )
    });
    let armed = stop.clone();
    harness.with_drive(move |d, _| d.exit_stop = Some(armed));
    harness.with_drive(|d, ctx| d.engage_cruise(ctx, 60.0, false));
    assert_eq!(harness.read_drive(|d| d.cruise_mph), Some(60.0));

    // At road speed, under what the gore accepts: nothing to shed.
    let under = stop.clone();
    harness.with_drive(move |d, ctx| d.update_exit_speed_assist(ctx, &under));
    harness.read_drive(|d| {
        assert_eq!(
            d.cruise_mph,
            Some(60.0),
            "cruise was paused with nothing to brake for"
        );
        assert_eq!(d.truck().brake, 0.0);
    });

    // Over the gate: now it has work, and it takes the pedals.
    let over = stop.clone();
    harness.with_drive(move |d, ctx| {
        let gate = d.gore_acceptance_mph(Some(&over));
        d.truck_mut().velocity_mps = (gate + 5.0) * MPS_PER_MPH;
        d.update_exit_speed_assist(ctx, &over);
    });
    harness.read_drive(|d| {
        assert!(d.cruise_mph.is_none());
        assert!(d.truck().brake > 0.0);
    });
}

#[test]
fn test_the_exit_hold_never_pushes_past_a_lower_limit_before_the_gore() {
    // The hold aims for the gore's road speed less ten. A 45 that ends just
    // short of a 70 gore used to be throttled toward 60 while the truck was
    // still inside it, with nobody on the pedals.
    let mut harness = a_drive("Exits");
    harness.app.ctx.settings.exit_speed_assist = true;
    harness.with_drive(|d, _| {
        bench_road_with(d, &[(0.0, 45.0), (199.7, 70.0)], 0.0, 1.0);
        d.trip.position_mi = 199.0;
        d.truck_mut().set_air_ready(false);
        d.truck_mut().start_engine();
        d.truck_mut().transmission.automatic = true;
        d.truck_mut().transmission.gear = 10;
        d.truck_mut().velocity_mps = 45.5 * MPS_PER_MPH;
        d.truck_mut().throttle = 0.0;
        d.truck_mut().brake = 0.0;
        d.exit_stop = Some(RoadStop::new("Gore Travel Plaza", 200.0, "truck_stop"));
        d.hold_exit_approach_speed();
    });
    assert_eq!(
        harness.read_drive(|d| d.truck().throttle),
        0.0,
        "the hold pushed the truck over the 45 in force here"
    );

    // Past the 45, the hold brings the truck up toward the gore's floor.
    harness.with_drive(|d, _| {
        d.trip.position_mi = 199.8;
        d.hold_exit_approach_speed();
    });
    assert!(harness.read_drive(|d| d.truck().throttle) > 0.0);
}

// -- the destination approach ------------------------------------------------------------

#[test]
fn test_destination_exit_scan_stays_on_the_final_approach() {
    // Routes that finish on rural highways carry no baked interchanges, and
    // the scan used to crown the last labeled exit anywhere on the route as
    // the destination exit: player transcripts (2026-07-16) show a Lampasas
    // run settled from Wichita Falls, 224 miles out, and a Havre, Montana run
    // settled from I-39 in Wisconsin, 1,158 miles out. The scan must find an
    // exit on the final approach or report none, so the synthetic end-of-route
    // exit takes over.
    //
    // Python duck-typed a stand-in around the scan. Rust needs the real thing,
    // so each corridor is built as a real delivery on that route.
    for (start, end) in [
        ("springfield_il_us", "lampasas_tx_us"),
        ("jamestown_ny_us", "havre_mt_us"),
    ] {
        let mut app = freight_fate::app::testing::TestApp::new();
        let world = ff_core::data::world::get_world();
        let route = world
            .shortest_route(start, end, None, true)
            .expect("the world routes")
            .unwrap_or_else(|| panic!("{start} to {end} has a route"));
        app.ctx.profile = Some(ff_core::models::profile::Profile::named_in("Scan", start));
        let mut job = ff_core::models::jobs::Job::new(
            &ff_core::models::jobs::CARGO_CATALOG["general"],
            12.0,
            start,
            "company yard",
            end,
            route.miles(),
            1000.0,
            12.0,
        );
        job.destination_location = format!("{end} freight market");
        let d = DrivingState::new(
            &mut app.ctx,
            job,
            route,
            Some(99),
            freight_fate::states::driving_core::DRIVE_PHASE_DELIVERY,
            Some(12.0),
        );
        let total = d.trip.total_miles();

        if let Some((at_mi, _label, _phrase)) = d.scan_destination_exit_details(&app.ctx, false) {
            assert!(
                at_mi >= total - DESTINATION_EXIT_SCAN_WINDOW_MI,
                "{start} to {end}: the scan crowned an exit {:.0} miles out",
                total - at_mi
            );
        }
        drop(d);
        drop(app);
    }
}

#[test]
fn test_the_destination_approach_assist_actually_brings_the_truck_to_a_stop() {
    // Owner, Odessa, 2026-08-19: "I did, and it's wrong. Never stopped."
    //
    // Driven on the REAL harness -- a real App, a real dispatch, the real ramp
    // and the real clock -- because a stand-in is what let this through three
    // times. Every earlier Python version of this test built fake trip and
    // truck objects, and every one of them passed while the game drove
    // straight past the market.
    let mut harness = a_drive("Exits");
    harness.app.ctx.settings.destination_approach_assist = true;
    // `driving._begin_surface_chain = lambda: False`, arranged in the data
    // rather than patched. A chain-capable destination flows off the ramp onto
    // city streets at whatever legal speed the ramp let through -- which is the
    // street chain's business and has its own suite -- and dispatch draws a
    // different facility every run, so without this the case measures the ramp
    // end on some runs and a street handoff on others (it failed exactly that
    // way at 1.8 mph). A destination location the world has no approach route
    // for makes the ramp itself the arrival, whichever facility was drawn.
    harness.with_drive(|d, ctx| {
        d.job.destination_location = "no facility approach route".to_string();
        d.destination_chain_ahead = Some(false);
        assert!(
            d.surface_chain_route(ctx).is_none(),
            "the chain has to be out of the way for this case to mean anything"
        );
    });
    let destination = destination_exit(&mut harness);

    // Onto the destination ramp at ramp speed, hands off from there.
    let at = destination.at_mi;
    harness.with_drive(move |d, ctx| {
        d.exit_stop = Some(destination.clone());
        d.exit_lane_entered = true;
        d.exit_signal_on = true; // signalled for it, like a driver
        d.trip.position_mi = at;
        d.truck_mut().velocity_mps = 40.0 * MPS_PER_MPH;
        d.update_exit(ctx, 0.0, 0.0);
    });
    assert!(
        harness.read_drive(|d| d.ramp_mi.is_some()),
        "never got onto the ramp: {:?}",
        spoken(&harness).last()
    );
    harness.with_drive(|d, _| {
        // This ramp ends in a stop sign, and the ramp-terminal assist stops
        // the truck for THAT -- which passed this test even with the broken
        // distance underneath it. Clear the terminal so the destination
        // approach assist is the only thing that can bring the truck up.
        d.ramp_control = String::new();
        d.ramp_terminal_done = true;
        d.truck_mut().start_engine();
    });

    for _ in 0..(60 * 600) {
        if !harness.has_drive() || harness.read_drive(|d| d.arrival_menu_open) {
            break;
        }
        harness.with_drive(|d, _| d.truck_mut().throttle = 0.0); // the assist is the only input
        frame(&mut harness, DT);
        if harness.read_drive(|d| d.ramp_mi.is_none()) {
            break;
        }
    }

    let past: Vec<String> = spoken(&harness)
        .into_iter()
        .filter(|line| line.contains("Drove past"))
        .collect();
    assert!(
        past.is_empty(),
        "the assist let the truck run the gate: {past:?}"
    );
    let speed = harness.read_drive(|d| d.truck().speed_mph());
    assert!(
        speed <= DOCKING_MAX_MPH,
        "stopped nowhere: still doing {speed:.1} mph"
    );
    // Stopped is not arrived. The first fix passed with the truck parked 32
    // metres short of the gate and the dock never opening (Jerry, Hobbs,
    // 2026-08-22); the facility overshoot suite drives that end to end, and
    // this one at least insists the gate was reached.
    let left = harness.read_drive(|d| d.ramp_mi);
    assert!(
        left.is_none(),
        "stopped {:.1} m short of the gate",
        left.unwrap_or(0.0) * 1609.344
    );
}
