//! Transcript-backed rest-stop selection and stopping-assist regressions
//! (port of `tests/test_rest_stop_assist.py`).
//!
//! Python's fixture patched `ctx.say` AND `ctx.say_event` into one `spoken`
//! list. Here both channels land on `ctx.speech` in submission order, which
//! is the same list one rung lower: the driving verbosity ladder and the
//! event pacer now sit above the recording. Every line asserted below is
//! either a main-channel readout (the T and X answers, the menu rows) or a
//! navigation/safety event, so no rung silences one and none of the
//! expectations move.
//!
//! `start_drive(app)` -- new career, accept the assigned dispatch, depart --
//! is [`PlaytestHarness::start_delivery`], which walks the same menus.

use ff_core::sim::hos::parking_full_probability;
use ff_core::sim::trip_models::RoadStop;
use ff_core::sim::weather::WeatherKind;
use freight_fate::playtest::harness::{PlaytestHarness, StartDelivery};
use freight_fate::states::base::{Key, Menu};
use freight_fate::states::driving::DrivingState;
use freight_fate::states::driving_core::RAMP_OVERSHOOT_MI;
use freight_fate::states::driving_rest_states::RestStopState;

const DT: f64 = 1.0 / 60.0;
const MPS_PER_MPH: f64 = 1.0 / 2.23694;

fn approx(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-6 * b.abs().max(1.0)
}

/// The `driving_app` fixture: a real career at the wheel on a quiet road.
fn driving_app() -> PlaytestHarness {
    let mut harness = PlaytestHarness::new();
    harness.start_delivery(StartDelivery::named("Rest Assist"));
    // `quiet_trip(driving)`. `start_delivery` already pushes the random
    // hazard and inspection checks past the horizon and empties the rolling
    // bubble; the congestion zones it does not touch would re-inject slow NPC
    // traffic and steal the assist's attention.
    harness.with_drive(|d, _| {
        d.trip.zones.retain(|zone| zone.aadt.is_none());
        d.weather_mut().current = WeatherKind::Clear;
        // The origin yard may carry a turn-level street chain; these feature
        // tests exercise highway machinery, so skip the departure chain.
        d.departure_checked = true;
        d.truck_mut().set_air_ready(false);
        // Parking availability is deterministic in `(trip_seed, mile, clock,
        // spaces)` and only crunches after 8 PM. Python monkeypatched
        // `hos.parking_is_full` to False; parking the clock at midday is the
        // same answer through the real rule.
        d.trip.start_hour = 12.0;
    });
    harness.clear_speech();
    harness
}

/// `sleep_stop(driving, ahead=...)`.
fn sleep_stop(harness: &mut PlaytestHarness, ahead: f64) -> RoadStop {
    let at_mi = harness.read_drive(|d| d.trip.position_mi) + ahead;
    let mut stop = RoadStop::new("Prairie View Rest Area", at_mi, "public_rest_area");
    stop.actions = ["park", "save", "break", "sleep"]
        .iter()
        .map(|a| a.to_string())
        .collect();
    stop.parking = "confirmed".to_string();
    stop.exit_label = "exit 99".to_string();
    let staged = stop.clone();
    harness.with_drive(move |d, _| d.trip.stops = vec![staged]);
    stop
}

/// Every line said so far, both channels, in order (the fixture's `spoken`).
fn spoken(harness: &PlaytestHarness) -> Vec<String> {
    harness.app.speech().lines()
}

fn last(harness: &PlaytestHarness) -> String {
    spoken(harness).last().cloned().unwrap_or_default()
}

/// T while rolling plans a sleep stop, the purpose these drives are about.
fn press_t(harness: &mut PlaytestHarness) {
    harness.plan_stop("Sleep");
}

fn press_x(harness: &mut PlaytestHarness) {
    harness.press_key(Key::X, None);
}

fn rolling(harness: &mut PlaytestHarness, mph: f64) {
    harness.with_drive(move |d, _| d.truck_mut().velocity_mps = mph * MPS_PER_MPH);
}

/// One frame of the drive itself, with the pacer's clock kept honest.
fn drive_frame(harness: &mut PlaytestHarness) {
    harness.advance_frame_clock();
    harness.with_drive(|d, ctx| d.update_frame(ctx, DT));
}

/// One frame of whatever screen is on top (`app.state.update(1/60)`).
fn top_frame(harness: &mut PlaytestHarness) {
    harness.advance_frame_clock();
    let Some(state) = harness.app.ctx.state() else {
        return;
    };
    state.borrow_mut().update(&mut harness.app.ctx, DT);
    harness.app.ctx.run_deferred();
}

#[test]
fn test_rolling_t_plans_exact_sleep_stop_without_silently_selecting_exit() {
    let mut harness = driving_app();
    let stop = sleep_stop(&mut harness, 2.0);
    rolling(&mut harness, 40.0);

    press_t(&mut harness);

    assert!(harness.state_is::<DrivingState>());
    assert_eq!(
        harness.read_drive(|d| d.trip.planned_stop_key.clone()),
        Some(stop.key())
    );
    assert_eq!(
        harness.read_drive(|d| d.selected_stop_key.clone()),
        Some(stop.key())
    );
    assert!(harness.read_drive(|d| d.exit_stop.is_none()));
    assert!(!harness.read_drive(|d| d.exit_signal_on));
    let said = last(&harness);
    assert!(
        said.contains("Sleep stop planned, public rest area: Prairie View Rest Area"),
        "{said}"
    );
    assert!(said.contains("Press X to signal."), "{said}");
    assert!(!said.contains("Come to a complete stop first"), "{said}");
    assert!(harness
        .read_drive(|d| d.status_text.clone())
        .contains("Press X to signal."));
}

#[test]
fn test_repeating_t_cancels_the_planned_sleep_stop() {
    let mut harness = driving_app();
    let stop = sleep_stop(&mut harness, 2.0);
    rolling(&mut harness, 40.0);

    press_t(&mut harness);
    assert_eq!(
        harness.read_drive(|d| d.trip.planned_stop_key.clone()),
        Some(stop.key())
    );

    press_t(&mut harness);

    assert!(harness.state_is::<DrivingState>());
    assert!(harness.read_drive(|d| d.trip.planned_stop_key.is_none()));
    assert!(harness.read_drive(|d| d.selected_stop_key.is_none()));
    assert!(!harness.read_drive(|d| d.selected_stop_assist_armed));
    assert_eq!(last(&harness), "Planned stop canceled.");
    assert_eq!(
        harness.read_drive(|d| d.status_text.clone()),
        "Planned stop canceled."
    );

    press_t(&mut harness);
    assert_eq!(
        harness.read_drive(|d| d.trip.planned_stop_key.clone()),
        Some(stop.key())
    );
}

#[test]
fn test_canceling_a_plan_clears_mismatched_selected_stop_intent() {
    let mut harness = driving_app();
    let planned = sleep_stop(&mut harness, 2.0);
    let mut alternate = planned.clone();
    alternate.name = "Alternate Rest Area".to_string();
    alternate.at_mi += 1.0;
    let staged = alternate.clone();
    harness.with_drive(move |d, _| d.trip.stops.push(staged));
    rolling(&mut harness, 40.0);

    press_t(&mut harness);
    let alternate_key = alternate.key();
    harness.with_drive(move |d, _| {
        d.selected_stop_key = Some(alternate_key);
        d.selected_stop_assist_armed = true;
        d.selected_stop_assist_said = true;
        d.selected_stop_assist_brake = 0.3;
        d.truck_mut().brake = 0.2;
    });

    press_t(&mut harness);

    assert!(harness.read_drive(|d| d.trip.planned_stop_key.is_none()));
    assert!(harness.read_drive(|d| d.selected_stop_key.is_none()));
    assert!(!harness.read_drive(|d| d.selected_stop_assist_armed));
    assert!(!harness.read_drive(|d| d.selected_stop_assist_said));
    assert!(approx(harness.read_drive(|d| d.truck().brake), 0.0));
    assert_eq!(last(&harness), "Planned stop canceled.");
}

#[test]
fn test_canceling_a_planned_stop_resets_its_armed_exit_approach() {
    let mut harness = driving_app();
    let stop = sleep_stop(&mut harness, 2.0);
    rolling(&mut harness, 40.0);

    press_t(&mut harness);
    press_x(&mut harness);
    assert!(harness.read_drive(|d| d.exit_signal_on));
    assert_eq!(
        harness.read_drive(|d| d.exit_stop.as_ref().map(|active| active.key())),
        Some(stop.key())
    );
    harness.with_drive(|d, _| {
        d.selected_stop_assist_armed = true;
        d.selected_stop_assist_said = true;
        d.selected_stop_assist_brake = 0.3;
        d.truck_mut().brake = 0.2;
        d.cruise_exit_mph = Some(31.0);
        d.exit_signal_canceled = true;
        d.exit_lane_entered = true;
        d.exit_taper_said = true;
        d.exit_cancel_armed = true;
        d.exit_right_hold_s = 0.8;
        d.exit_right_taps = 3;
        d.exit_tap_hint_said = true;
        d.exit_countdown_said = vec![5.0, 2.0];
    });

    press_t(&mut harness);

    assert!(harness.read_drive(|d| d.trip.planned_stop_key.is_none()));
    assert!(harness.read_drive(|d| d.selected_stop_key.is_none()));
    assert!(!harness.read_drive(|d| d.selected_stop_assist_armed));
    assert!(!harness.read_drive(|d| d.selected_stop_assist_said));
    assert!(approx(
        harness.read_drive(|d| d.selected_stop_assist_brake),
        0.0
    ));
    assert!(approx(harness.read_drive(|d| d.truck().brake), 0.0));
    assert!(harness.read_drive(|d| d.exit_stop.is_none()));
    assert!(!harness.read_drive(|d| d.exit_signal_on));
    assert!(!harness.read_drive(|d| d.exit_signal_canceled));
    assert!(harness.read_drive(|d| d.cruise_exit_mph.is_none()));
    assert!(!harness.read_drive(|d| d.exit_lane_entered));
    assert!(!harness.read_drive(|d| d.exit_taper_said));
    assert!(!harness.read_drive(|d| d.exit_cancel_armed));
    assert!(approx(harness.read_drive(|d| d.exit_right_hold_s), 0.0));
    assert_eq!(harness.read_drive(|d| d.exit_right_taps), 0);
    assert!(!harness.read_drive(|d| d.exit_tap_hint_said));
    assert!(harness.read_drive(|d| d.exit_countdown_said.is_empty()));
    assert_eq!(
        last(&harness),
        "Planned stop canceled. Exit signal canceled."
    );
}

#[test]
fn test_canceling_a_plan_preserves_a_different_armed_exit_approach() {
    let mut harness = driving_app();
    let planned = sleep_stop(&mut harness, 2.0);
    let mut active_exit = RoadStop::new(
        "Northside Freight Terminal",
        planned.at_mi + 0.5,
        "delivery_destination",
    );
    active_exit.actions = vec!["deliver".to_string()];
    active_exit.parking = "confirmed".to_string();
    active_exit.exit_label = "exit 100".to_string();
    let staged = active_exit.clone();
    harness.with_drive(move |d, _| d.trip.stops.push(staged));
    rolling(&mut harness, 40.0);

    press_t(&mut harness);
    let armed_exit = active_exit.clone();
    harness.with_drive(move |d, _| {
        d.exit_stop = Some(armed_exit);
        d.exit_signal_on = true;
        d.exit_signal_canceled = true;
        d.selected_stop_assist_armed = true;
        d.selected_stop_assist_said = true;
        d.selected_stop_assist_brake = 0.3;
        d.truck_mut().brake = 0.2;
        d.cruise_exit_mph = Some(31.0);
        d.exit_lane_entered = true;
        d.exit_taper_said = true;
        d.exit_cancel_armed = true;
        d.exit_right_hold_s = 0.8;
        d.exit_right_taps = 3;
        d.exit_tap_hint_said = true;
        d.exit_countdown_said = vec![5.0, 2.0];
    });

    press_t(&mut harness);

    assert!(harness.read_drive(|d| d.trip.planned_stop_key.is_none()));
    assert!(harness.read_drive(|d| d.selected_stop_key.is_none()));
    assert!(!harness.read_drive(|d| d.selected_stop_assist_armed));
    assert!(!harness.read_drive(|d| d.selected_stop_assist_said));
    assert!(approx(
        harness.read_drive(|d| d.selected_stop_assist_brake),
        0.0
    ));
    assert!(approx(harness.read_drive(|d| d.truck().brake), 0.0));
    assert_eq!(
        harness.read_drive(|d| d.exit_stop.as_ref().map(|stop| stop.key())),
        Some(active_exit.key())
    );
    assert!(harness.read_drive(|d| d.exit_signal_on));
    assert!(harness.read_drive(|d| d.exit_signal_canceled));
    assert_eq!(harness.read_drive(|d| d.cruise_exit_mph), Some(31.0));
    assert!(harness.read_drive(|d| d.exit_lane_entered));
    assert!(harness.read_drive(|d| d.exit_taper_said));
    assert!(harness.read_drive(|d| d.exit_cancel_armed));
    assert!(approx(harness.read_drive(|d| d.exit_right_hold_s), 0.8));
    assert_eq!(harness.read_drive(|d| d.exit_right_taps), 3);
    assert!(harness.read_drive(|d| d.exit_tap_hint_said));
    assert_eq!(
        harness.read_drive(|d| d.exit_countdown_said.clone()),
        vec![5.0, 2.0]
    );
    let active_name = active_exit.spoken_name();
    let said = last(&harness);
    assert!(said.contains("Planned stop canceled."), "{said}");
    assert!(
        said.contains(&format!("Exit signal remains active for {active_name}.")),
        "{said}"
    );
}

#[test]
fn test_selected_stop_assist_does_nothing_without_t_and_x() {
    let mut harness = driving_app();
    let stop = sleep_stop(&mut harness, 0.2);
    harness.app.ctx.settings.destination_approach_assist = true;
    rolling(&mut harness, 35.0);

    for _ in 0..20 {
        drive_frame(&mut harness);
    }

    assert!(harness.state_is::<DrivingState>());
    assert!(harness.read_drive(|d| d.trip.planned_stop_key.is_none()));
    assert!(harness.read_drive(|d| d.selected_stop_key.is_none()));
    assert!(harness.read_drive(|d| d.exit_stop.is_none()));
    assert!(harness.read_drive(|d| d.ramp_stop.is_none()));
    assert!(!harness.read_drive(|d| d.selected_stop_assist_armed));
    assert!(approx(harness.read_drive(|d| d.truck().brake), 0.0));
    assert!(!spoken(&harness)
        .iter()
        .any(|line| line.contains("Slowing for the entrance")));
    assert!(stop.at_mi > harness.read_drive(|d| d.trip.position_mi));
}

#[test]
fn test_x_cancel_clears_explicit_assist_but_keeps_route_plan() {
    let mut harness = driving_app();
    let stop = sleep_stop(&mut harness, 2.0);
    harness.app.ctx.settings.destination_approach_assist = true;
    rolling(&mut harness, 35.0);

    press_t(&mut harness);
    press_x(&mut harness);
    assert!(harness.read_drive(|d| d.selected_stop_assist_armed));

    press_x(&mut harness);

    assert!(!harness.read_drive(|d| d.exit_signal_on));
    assert!(harness.read_drive(|d| d.selected_stop_key.is_none()));
    assert!(!harness.read_drive(|d| d.selected_stop_assist_armed));
    assert_eq!(
        harness.read_drive(|d| d.trip.planned_stop_key.clone()),
        Some(stop.key())
    );
    let said = last(&harness);
    assert_eq!(said, "Signal canceled.");
    assert!(harness.read_drive(|d| d.exit_stop.is_none()));

    press_t(&mut harness);
    assert_eq!(
        harness.read_drive(|d| d.selected_stop_key.clone()),
        Some(stop.key())
    );
    let said = last(&harness);
    assert!(said.contains("Press X to signal"), "{said}");
    assert!(!said.contains("Press X to cancel"), "{said}");
    press_x(&mut harness);
    assert!(harness.read_drive(|d| d.exit_signal_on));
    assert!(harness.read_drive(|d| d.selected_stop_assist_armed));
}

#[test]
fn test_selected_stop_assist_reaches_full_stop_and_sleep_menu() {
    let mut harness = driving_app();
    let stop = sleep_stop(&mut harness, 1.0);
    harness.app.ctx.settings.destination_approach_assist = true;
    harness.app.ctx.settings.lane_keeping = "full".to_string();
    // Python patched `hos.parking_is_full` to False. The rule is deterministic
    // and quiet outside the evening crunch, so the midday clock the fixture
    // sets answers the same way -- asserted, not assumed.
    let local_hour = harness.read_drive(|d| d.trip.local_hour());
    assert!(
        approx(
            parking_full_probability(local_hour, stop.parking_spaces),
            0.0
        ),
        "the lot must not be full for this case"
    );
    harness.with_drive(|d, _| {
        d.truck_mut().start_engine();
        d.truck_mut().set_air_ready(false);
        d.truck_mut().transmission.automatic = true;
        d.truck_mut().transmission.gear = 8;
        d.truck_mut().velocity_mps = 40.0 * MPS_PER_MPH;
    });

    press_t(&mut harness);
    press_x(&mut harness);
    assert!(harness.read_drive(|d| d.selected_stop_assist_armed));
    assert!(harness.read_drive(|d| d.exit_signal_on));
    // The assist is armed in silence: the driver chose it (owner,
    // 2026-10-14).
    assert!(spoken(&harness).iter().any(|line| line.contains("Signal ")));
    assert!(!spoken(&harness)
        .iter()
        .any(|line| line.contains("stopping assistance armed")));

    let at_mi = stop.at_mi;
    harness.with_drive(move |d, _| d.trip.position_mi = at_mi);
    drive_frame(&mut harness);
    assert_eq!(
        harness.read_drive(|d| d.ramp_stop.as_ref().map(|s| s.key())),
        Some(stop.key())
    );
    // Isolate the entrance stop from the independently tested light/sign flow.
    harness.with_drive(|d, _| {
        d.ramp_control = "none".to_string();
        d.ramp_terminal_done = true;
    });

    let mut opened = false;
    for _ in 0..4_000 {
        top_frame(&mut harness);
        if harness.state_is::<RestStopState>() {
            opened = true;
            break;
        }
    }
    assert!(
        opened,
        "selected-stop assistance did not open the rest menu\n{}",
        harness.transcript_text()
    );

    assert!(harness.read_drive(|d| d.truck().speed_mph()) <= 0.5);
    assert!(harness.read_drive(|d| d.truck().parking_brake));
    assert!(harness.read_drive(|d| d.selected_stop_key.is_none()));
    assert!(harness.read_drive(|d| d.trip.planned_stop_key.is_none()));
    assert!(spoken(&harness)
        .iter()
        .any(|line| line.contains("Slowing for the entrance.")));
    assert!(spoken(&harness)
        .iter()
        .any(|line| line.contains("Stopped at public rest area: Prairie View Rest Area")));
    assert_eq!(
        harness.with_state::<RestStopState, _>(|state, _| state.menu().title.clone()),
        "public rest area: Prairie View Rest Area"
    );
    assert_eq!(
        harness.focused_label().unwrap_or_default(),
        "Sleep 10 hours"
    );
    assert!(spoken(&harness)
        .iter()
        .any(|line| line.contains("Sleep 10 hours")));
    let labels = harness.menu_labels();
    for hours in [2, 3, 7, 8] {
        assert!(
            labels
                .iter()
                .any(|l| l == &format!("Sleep {hours} hours in sleeper berth")),
            "{labels:?}"
        );
    }
    assert!(labels.iter().any(|l| l == "Sleep 10 hours"), "{labels:?}");

    let lines = spoken(&harness);
    let index_of = |needle: &str| {
        lines
            .iter()
            .position(|line| line.contains(needle))
            .unwrap_or_else(|| panic!("no line contains {needle:?}\n{lines:#?}"))
    };
    let selected_i = index_of("Sleep stop planned");
    let armed_i = index_of("Signal ");
    let braking_i = index_of("Slowing for the entrance.");
    let stopped_i = index_of("Stopped at public rest area");
    let menu_i = index_of("Sleep 10 hours");
    assert!(
        selected_i < armed_i && armed_i < braking_i && braking_i < stopped_i && stopped_i < menu_i,
        "{selected_i} {armed_i} {braking_i} {stopped_i} {menu_i}"
    );

    for _ in 0..3 {
        harness.key(freight_fate::playtest::harness::key_event(Key::Up, None));
    }
    assert_eq!(
        harness.focused_label().unwrap_or_default(),
        "Sleep 3 hours in sleeper berth"
    );
    assert!(
        last(&harness).contains("Sleep 3 hours in sleeper berth"),
        "{}",
        last(&harness)
    );

    harness.key(freight_fate::playtest::harness::key_event(Key::Home, None));
    assert!(harness
        .focused_label()
        .unwrap_or_default()
        .starts_with("Loyalty program"));
    harness.key(freight_fate::playtest::harness::key_event(
        Key::Return,
        None,
    ));
    assert!(!harness.state_is::<DrivingState>());
    harness.key(freight_fate::playtest::harness::key_event(
        Key::Escape,
        None,
    ));
    assert!(harness.state_is::<RestStopState>());
    assert!(harness
        .focused_label()
        .unwrap_or_default()
        .starts_with("Loyalty program"));
    assert!(
        last(&harness).contains("Loyalty program"),
        "{}",
        last(&harness)
    );

    harness.key(freight_fate::playtest::harness::key_event(
        Key::Escape,
        None,
    ));
    assert!(harness.state_is::<DrivingState>());
    assert!(harness.read_drive(|d| d.truck().parking_brake));
}

#[test]
fn test_overshoot_clears_assist_then_stopped_t_recovers() {
    let mut harness = driving_app();
    let stop = sleep_stop(&mut harness, 1.0);
    harness.app.ctx.settings.destination_approach_assist = true;
    rolling(&mut harness, 35.0);
    press_t(&mut harness);
    press_x(&mut harness);
    assert!(harness.read_drive(|d| d.selected_stop_assist_armed));

    let staged = stop.clone();
    harness.with_drive(move |d, ctx| {
        d.ramp_stop = Some(staged);
        d.ramp_mi = Some(-RAMP_OVERSHOOT_MI - 0.1);
        d.ramp_terminal_done = true;
        d.ramp_end_said = true;
        d.ramp_arrival_grace_s = 0.0;
        d.truck_mut().velocity_mps = 10.0 * MPS_PER_MPH;
        d.update_exit(ctx, 0.0, 0.0);
    });

    assert!(harness.read_drive(|d| d.ramp_stop.is_none()));
    assert!(harness.read_drive(|d| d.selected_stop_key.is_none()));
    assert!(!harness.read_drive(|d| d.selected_stop_assist_armed));
    assert!(harness.read_drive(|d| d.trip.planned_stop_key.is_none()));
    assert!(approx(harness.read_drive(|d| d.truck().brake), 0.0));
    assert!(spoken(&harness).iter().any(|line| {
        let lower = line.to_lowercase();
        lower.contains("drove past")
            && (lower.contains("without stopping") || lower.contains("never stopped"))
    }));

    let at_mi = stop.at_mi;
    harness.with_drive(move |d, _| {
        d.trip.position_mi = at_mi + 0.7;
        d.truck_mut().velocity_mps = 0.0;
    });
    press_t(&mut harness);
    assert!(harness.state_is::<RestStopState>());
}

#[test]
fn test_rolling_t_without_sleep_stop_gives_recovery_guidance() {
    let mut harness = driving_app();
    harness.with_drive(|d, _| d.trip.stops.clear());
    rolling(&mut harness, 30.0);

    press_t(&mut harness);

    assert!(harness.state_is::<DrivingState>());
    assert!(harness.read_drive(|d| d.selected_stop_key.is_none()));
    let said = last(&harness);
    assert_eq!(said, "No sleep stop ahead. Shoulder sleep available.");
}

#[test]
fn test_unselected_stop_passes_without_braking_or_menu() {
    let mut harness = driving_app();
    let stop = sleep_stop(&mut harness, 0.01);
    harness.app.ctx.settings.destination_approach_assist = true;
    rolling(&mut harness, 35.0);

    let at_mi = stop.at_mi;
    harness.with_drive(move |d, _| d.trip.position_mi = at_mi + 0.2);
    drive_frame(&mut harness);

    assert!(harness.state_is::<DrivingState>());
    assert!(approx(harness.read_drive(|d| d.truck().brake), 0.0));
    assert!(harness.read_drive(|d| d.ramp_stop.is_none()));
    assert!(!spoken(&harness)
        .iter()
        .any(|line| line.contains("Slowing for the entrance")));
}

#[test]
fn test_selected_stop_outranks_unsignaled_destination_exit() {
    let mut harness = driving_app();
    let stop = sleep_stop(&mut harness, 1.0);
    let mut destination = RoadStop::new("Warehouse", stop.at_mi + 0.1, "delivery_destination");
    destination.actions = vec!["deliver".to_string()];
    destination.parking = "confirmed".to_string();
    destination.exit_label = "exit 100".to_string();
    let staged = destination.clone();
    harness.with_drive(move |d, _| {
        d.trip.stops.push(staged.clone());
        d.exit_stop = Some(staged);
        d.truck_mut().velocity_mps = 35.0 * MPS_PER_MPH;
    });

    press_t(&mut harness);
    press_x(&mut harness);

    assert_eq!(
        harness.read_drive(|d| d.exit_stop.as_ref().map(|s| s.key())),
        Some(stop.key())
    );
    assert!(harness.read_drive(|d| d.exit_signal_on));
    let said = last(&harness);
    assert!(said.contains("Prairie View Rest Area"), "{said}");
    assert!(!said.contains("Warehouse"), "{said}");
}

#[test]
fn test_t_during_police_stop_names_the_trooper_action() {
    let mut harness = driving_app();
    let stop = sleep_stop(&mut harness, 2.0);
    let planned_key = stop.key();
    harness.with_drive(|d, _| {
        d.pull_over = Some("lights".to_string());
        d.truck_mut().velocity_mps = 35.0 * MPS_PER_MPH;
    });
    harness.with_drive(move |d, _| {
        d.trip.planned_stop_key = Some(planned_key.clone());
        d.selected_stop_key = Some(planned_key);
        d.selected_stop_assist_armed = true;
    });

    press_t(&mut harness);

    assert_eq!(
        harness.read_drive(|d| d.trip.planned_stop_key.clone()),
        Some(stop.key())
    );
    assert_eq!(
        harness.read_drive(|d| d.selected_stop_key.clone()),
        Some(stop.key())
    );
    let said = last(&harness);
    assert!(said.contains("Resolve the police stop"), "{said}");
    assert!(
        said.contains("Press X to signal the trooper stop"),
        "{said}"
    );
}

#[test]
fn test_t_on_selected_ramp_reports_live_assist_state() {
    let mut harness = driving_app();
    let stop = sleep_stop(&mut harness, 2.0);
    harness.app.ctx.settings.destination_approach_assist = true;
    let staged = stop.clone();
    harness.with_drive(move |d, _| {
        d.trip.planned_stop_key = Some(staged.key());
        d.selected_stop_key = Some(staged.key());
        d.selected_stop_assist_armed = true;
        d.ramp_stop = Some(staged);
        d.ramp_mi = Some(0.4);
        d.truck_mut().velocity_mps = 20.0 * MPS_PER_MPH;
    });

    press_t(&mut harness);

    let said = last(&harness);
    assert!(said.contains("On the selected ramp"), "{said}");
    // The assist is on, so the driver has nothing to do: nothing to say.
    assert!(!said.contains("assistance"), "{said}");
    assert!(!said.contains("Stop at the entrance"), "{said}");
    assert!(!said.contains("behind you"), "{said}");
    assert_eq!(
        harness.read_drive(|d| d.trip.planned_stop_key.clone()),
        Some(stop.key())
    );

    harness.app.ctx.settings.destination_approach_assist = false;
    press_t(&mut harness);
    let said = last(&harness);
    assert!(said.contains("Stop at the entrance."), "{said}");
}

#[test]
fn test_t_on_a_different_active_ramp_keeps_the_future_sleep_plan_selected() {
    let mut harness = driving_app();
    let planned = sleep_stop(&mut harness, 2.0);
    harness.app.ctx.settings.destination_approach_assist = true;
    let mut active = planned.clone();
    active.name = "Riverbend Travel Center".to_string();
    active.stop_type = "travel_center".to_string();
    active.at_mi -= 1.0;
    let staged = active.clone();
    let planned_key = planned.key();
    let staged_planned_key = planned_key.clone();
    harness.with_drive(move |d, _| {
        d.trip.planned_stop_key = Some(staged_planned_key.clone());
        d.selected_stop_key = Some(staged_planned_key);
        d.selected_stop_assist_armed = true;
        d.selected_stop_assist_brake = 0.4;
        d.selected_stop_assist_said = true;
        d.ramp_stop = Some(staged);
        d.ramp_mi = Some(0.4);
        d.truck_mut().velocity_mps = 20.0 * MPS_PER_MPH;
        d.truck_mut().brake = 0.25;
    });

    press_t(&mut harness);

    let said = last(&harness);
    assert!(
        said.contains("On the ramp for Riverbend Travel Center"),
        "{said}"
    );
    assert!(!said.contains("selected ramp"), "{said}");
    assert!(!said.contains("assistance"), "{said}");
    assert!(!said.contains("Planned stop canceled"), "{said}");
    assert_eq!(
        harness.read_drive(|d| d.trip.planned_stop_key.clone()),
        Some(planned_key.clone())
    );
    assert_eq!(
        harness.read_drive(|d| d.selected_stop_key.clone()),
        Some(planned_key)
    );
    assert!(harness.read_drive(|d| d.selected_stop_assist_armed));
    assert!(approx(
        harness.read_drive(|d| d.selected_stop_assist_brake),
        0.4
    ));
    assert!(harness.read_drive(|d| d.selected_stop_assist_said));
    assert!(approx(harness.read_drive(|d| d.trip.truck.brake), 0.25));
    assert_eq!(
        harness.read_drive(|d| d.ramp_stop.as_ref().map(|stop| stop.key())),
        Some(active.key())
    );
}

#[test]
#[ignore = "needs a controller seam: Python patched `ctx.control_hint` to return \
            gamepad labels. `GameContext::control_hint` forwards to \
            `Controller::hint(action)`, which reads the bound SDL device, and a \
            headless test has no pad to bind."]
fn test_controller_rest_planning_names_controller_exit_control() {
    // With a pad bound, planning a rest stop must name the pad's own exit
    // control -- "Press D-pad down to signal" -- and never the keyboard X.
    //
    //   driving.try_rest_stop(ctx);
    //   assert last(&harness).contains("Press D-pad down to signal");
    //   assert !last(&harness).contains("Press X");
}

#[test]
fn test_t_plans_a_sleep_stop_well_beyond_the_exit_window() {
    // Owner, Hanging Lake, 2026-08-22: T seven miles out answered "no
    // sleep-capable route stop is close enough ahead to plan", and worked a
    // minute later with nothing changed but the odometer. Planning was bounded
    // by the exit window -- the five-odd miles inside which an exit can be
    // SIGNALLED -- which has nothing to do with deciding where to sleep.
    //
    // T plans the next sleep-capable stop however far ahead it is. Inside the
    // window it says press X; beyond it, it says you will be told when the
    // exit comes up, because X does nothing out there yet.
    let mut harness = driving_app();
    let stop = sleep_stop(&mut harness, 20.0);
    assert!(harness.read_drive(|d| d.exit_window_mi()) < 20.0);
    rolling(&mut harness, 60.0);

    press_t(&mut harness);

    assert_eq!(
        harness.read_drive(|d| d.trip.planned_stop_key.clone()),
        Some(stop.key())
    );
    assert_eq!(
        harness.read_drive(|d| d.selected_stop_key.clone()),
        Some(stop.key())
    );
    let said = last(&harness);
    assert!(
        said.contains("Sleep stop planned, public rest area: Prairie View Rest Area"),
        "{said}"
    );
    assert!(
        said.contains("20 miles ahead") || said.contains("20.0 miles ahead"),
        "{said}"
    );
    // Twenty miles out the exit cannot be signalled yet; it is called when
    // it comes.
    assert!(!said.contains("Press X"), "{said}");
    // Nothing is armed or signalled by the plan itself.
    assert!(harness.read_drive(|d| d.exit_stop.is_none()));
    assert!(!harness.read_drive(|d| d.exit_signal_on));
}
