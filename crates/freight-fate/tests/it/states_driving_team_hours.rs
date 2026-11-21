//! The co-driver never drives past their hours, and every hand-back for
//! their hours or their break is made stopped (tester report, 2026-11-19).
//!
//! The tester drove six hours after a full rest, parked, and handed Dan the
//! wheel to sleep. Dan had 7.2 hours in the sleeper, short of his 10, so 85
//! minutes of driving left, and the next truck lot was 110 miles on. He
//! chose a lot his hours did not reach, ran out five miles short of it, a
//! trooper's log check fined the player 400 dollars and put the load out of
//! service for ten hours, and afterwards a stale "shift over" handed the
//! sleeping player the wheel at 43 miles per hour.

use ff_core::sim::hos::HosClock;
use ff_core::sim::team_shift::{TeamShift, TeamShiftEvent};
use ff_core::sim::weather::WeatherKind;
use freight_fate::playtest::harness::{PlaytestHarness, StartDelivery};
use freight_fate::states::base::{Key, Menu};
use freight_fate::states::driving_pause_states::PauseMenuState;
use freight_fate::states::driving_team::shift::CO_DRIVER_STOP_MARGIN_MIN;
use freight_fate::states::driving_team::TeamMode;

const DT: f64 = 1.0 / 60.0;
const MODE: &str = "realistic";
/// Dan's driving time left at the tester's hand-over.
const DAN_LEFT_MIN: f64 = 85.0;
/// Miles from the hand-over to the receiver: farther than Dan's hours
/// reach, near enough for the player's own hours once he hands back.
const TRIP_LEFT_MI: f64 = 230.0;

fn quiet_drive(name: &str) -> PlaytestHarness {
    let mut harness = PlaytestHarness::new();
    harness.start_delivery(StartDelivery::named(name));
    harness.app.ctx.settings.hos_mode = MODE.to_string();
    harness.app.ctx.settings.time_scale = 20.0;
    harness.with_drive(|d, _| {
        d.tutorial = None;
        d.departure_checked = true;
        d.trip.time_scale = 20.0;
        d.trip.hazard_check_mi = 1e9;
        d.trip.inspection_check_mi = 1e9;
        d.trip.traffic_manager.rolling_bubble = false;
        d.trip.set_npc_vehicles(Vec::new());
        d.trip.traffic_pressures.clear();
        d.trip.zones.retain(|z| z.aadt.is_none());
        d.trip.weather.current = WeatherKind::Clear;
        d.trip.set_patrols(Vec::new());
        d.trip.posts.clear();
    });
    harness
}

fn tail(harness: &PlaytestHarness) -> String {
    let lines = harness.transcript();
    let from = lines.len().saturating_sub(40);
    lines[from..].join("\n")
}

/// Dan's logbook at the tester's hand-over: 9.6 hours driven, a break on
/// the way, then 7.2 hours in the sleeper, short of his 10.
fn dans_logbook() -> HosClock {
    let mut hos = HosClock::new();
    hos.sleep();
    hos.drive(5.0 * 60.0);
    hos.take_break(30.0);
    hos.drive(11.0 * 60.0 - DAN_LEFT_MIN - 5.0 * 60.0);
    hos.sleeper(7.2 * 60.0);
    hos
}

/// The tester's hand-over, `TRIP_LEFT_MI` short of the receiver. The
/// player drove `player_min` since a full rest; a truck lot sits
/// `lot_ahead_mi` on, if any.
fn testers_handover(
    name: &str,
    mode: TeamMode,
    player_min: f64,
    lot_ahead_mi: Option<f64>,
) -> PlaytestHarness {
    let mut harness = quiet_drive(name);
    let hos = &mut harness.app.ctx.profile.as_mut().expect("a career").hos;
    hos.sleep();
    hos.on_duty(60.0);
    hos.drive(player_min);
    harness.with_drive(|d, ctx| {
        let lot = d
            .trip
            .stops
            .iter()
            .find(|stop| {
                stop.parking != "none"
                    && stop.stop_type != "weigh_station"
                    && stop.stop_type != "delivery_destination"
            })
            .cloned()
            .expect("a stop with parking");
        let here = (d.trip.total_miles() - TRIP_LEFT_MI).max(3.0);
        d.trip.position_mi = here;
        d.enforcement_prev_mi = here;
        d.trip
            .stops
            .retain(|stop| stop.stop_type == "delivery_destination");
        if let Some(ahead) = lot_ahead_mi {
            let mut stop = lot.clone();
            stop.at_mi = here + ahead;
            stop.name = format!("{} ahead", stop.name);
            d.trip.stops.push(stop);
        }
        d.trip.stops.sort_by(|a, b| a.at_mi.total_cmp(&b.at_mi));
        d.truck_mut().start_engine();
        d.truck_mut().set_air_ready(false);
        d.truck_mut().velocity_mps = 65.0 / 2.236_936;
        d.cruise_mph = Some(65.0);
        d.ensure_team(ctx);
        if let Some(team) = d.team.as_mut() {
            team.off_wheel_hos = dans_logbook();
        }
        d.hand_wheel_to_co_driver(ctx, mode);
        d.truck_mut().parking_brake = false;
    });
    harness
}

/// Drive frames until the co-driver hands back, asserting every frame that
/// the logbook at the wheel is legal while Dan holds it. Returns whether
/// he handed back.
fn until_handed_back_legal(harness: &mut PlaytestHarness, frames: usize) -> bool {
    for _ in 0..frames {
        harness.advance_frame_clock();
        harness.app.frame(DT);
        harness.app.ctx.run_deferred();
        if !harness.app.ctx.co_driver_away {
            return true;
        }
        let hos = &harness.app.ctx.profile.as_ref().expect("a career").hos;
        assert!(
            !hos.in_violation(MODE),
            "Dan drove past his hours: {:.1} minutes driven, {:.1} on duty\n{}",
            hos.driving_min,
            hos.duty_min,
            tail(harness)
        );
    }
    false
}

fn speed(harness: &PlaytestHarness) -> f64 {
    harness.read_drive(|d| d.truck().speed_mph())
}

#[test]
fn the_testers_hand_over_slept_through_stops_on_the_shoulder_and_wakes_the_player() {
    let mut harness = testers_handover(
        "Team Hours Sleep",
        TeamMode::SleepThrough,
        6.2 * 60.0,
        Some(110.0),
    );
    let back = until_handed_back_legal(&mut harness, 60 * 60 * 20);
    assert!(back, "{}", tail(&harness));
    let said = harness.transcript().join("\n");
    assert!(
        said.contains(
            "Dan stopped on the shoulder with no lot in reach and hands you the wheel. Their \
             hours are up, and yours reach the receiver."
        ),
        "{}",
        tail(&harness)
    );
    assert!(!said.contains("Hours of service violation"), "{said}");
    assert!(!said.contains("A trooper pulled Dan over"), "{said}");
    assert!(speed(&harness) < 1.0, "handed back stopped");
    // Woken, not slept out: the player's own clock was never reset.
    let hos = &harness.app.ctx.profile.as_ref().expect("a career").hos;
    assert!(hos.driving_min > 6.0 * 60.0, "{}", hos.driving_min);
}

#[test]
fn the_testers_hand_over_riding_along_stops_on_the_shoulder_too() {
    let mut harness = testers_handover(
        "Team Hours Ride",
        TeamMode::RideAlong,
        6.2 * 60.0,
        Some(110.0),
    );
    let back = until_handed_back_legal(&mut harness, 60 * 60 * 30);
    assert!(back, "{}", tail(&harness));
    let said = harness.transcript().join("\n");
    assert!(
        said.contains(
            "Dan stopped on the shoulder with no lot in reach and hands you the wheel. Their \
             hours are up, and yours reach the receiver."
        ),
        "{}",
        tail(&harness)
    );
    assert!(speed(&harness) < 1.0, "handed back stopped");
}

#[test]
fn with_a_lot_in_his_hours_dan_parks_there_and_wakes_the_player() {
    let mut harness = testers_handover(
        "Team Hours Lot",
        TeamMode::SleepThrough,
        6.2 * 60.0,
        Some(40.0),
    );
    let back = until_handed_back_legal(&mut harness, 60 * 60 * 20);
    assert!(back, "{}", tail(&harness));
    let said = harness.transcript().join("\n");
    assert!(
        said.contains(
            "Dan has parked and hands you the wheel. Their hours are up, and yours reach the \
             receiver."
        ),
        "{}",
        tail(&harness)
    );
    assert!(speed(&harness) < 1.0, "handed back parked");
}

#[test]
fn short_of_his_hours_with_the_players_too_short_the_rest_is_slept_out() {
    // Ten hours driven: the player's own clock does not reach the receiver,
    // so the rest is slept out on the shoulder as before.
    let mut harness = testers_handover(
        "Team Hours Sleep Out",
        TeamMode::SleepThrough,
        10.0 * 60.0,
        Some(110.0),
    );
    let back = until_handed_back_legal(&mut harness, 60 * 60 * 20);
    assert!(back, "{}", tail(&harness));
    let said = harness.transcript().join("\n");
    assert!(
        said.contains(
            "Dan pulled onto the shoulder when their hours ran out, with no lot in reach, and you \
             slept on there. Your 10-hour rest is in."
        ),
        "{}",
        tail(&harness)
    );
    let hos = &harness.app.ctx.profile.as_ref().expect("a career").hos;
    assert_eq!(hos.driving_min, 0.0, "the player's clock is reset");
}

#[test]
fn a_clock_reset_under_the_shift_lets_dan_drive_on() {
    // The tester's last step: the ten-hour order reset Dan's clock, and the
    // stale shift handed the wheel back at 43 with "Their hours are up".
    let mut harness = testers_handover("Team Hours Reset", TeamMode::RideAlong, 0.0, Some(110.0));
    // Drive until his hours are short and he is braking for the shoulder.
    let mut braking = false;
    for _ in 0..60 * 60 * 30 {
        harness.advance_frame_clock();
        harness.app.frame(DT);
        harness.app.ctx.run_deferred();
        if harness.read_drive(|d| d.team_shift()) == TeamShift::Shoulder {
            braking = true;
            break;
        }
    }
    assert!(braking, "{}", tail(&harness));
    // Ten hours served: a fresh clock.
    harness
        .app
        .ctx
        .profile
        .as_mut()
        .expect("a career")
        .hos
        .sleep();
    for _ in 0..60 * 30 {
        harness.advance_frame_clock();
        harness.app.frame(DT);
        harness.app.ctx.run_deferred();
    }
    assert!(harness.app.ctx.co_driver_away, "{}", tail(&harness));
    assert_eq!(harness.read_drive(|d| d.team_shift()), TeamShift::Driving);
    let said = harness.transcript().join("\n");
    assert!(!said.contains("Their hours are up"), "{said}");
}

#[test]
fn the_shoulder_stop_from_highway_speed_fits_inside_the_backstop() {
    // The backstop's margin, measured: a loaded truck braking from 75 to a
    // stop on the Standard pacing, in minutes on the co-driver's clock.
    let mut harness = quiet_drive("Team Hours Stop");
    harness.with_drive(|d, ctx| {
        d.trip
            .stops
            .retain(|stop| stop.stop_type == "delivery_destination");
        d.trip.position_mi = 3.0;
        d.enforcement_prev_mi = 3.0;
        d.truck_mut().start_engine();
        d.truck_mut().set_air_ready(false);
        d.truck_mut().velocity_mps = 75.0 / 2.236_936;
        d.cruise_mph = Some(75.0);
        d.hand_wheel_to_co_driver(ctx, TeamMode::RideAlong);
        d.truck_mut().parking_brake = false;
    });
    let hos = &mut harness.app.ctx.profile.as_mut().expect("a career").hos;
    hos.drive(5.0 * 60.0);
    hos.take_break(30.0);
    hos.drive(11.0 * 60.0 - 5.0 * 60.0 - 14.0);
    let start = harness
        .app
        .ctx
        .profile
        .as_ref()
        .expect("a career")
        .hos
        .driving_min;
    let mut stopped = None;
    for _ in 0..60 * 60 {
        harness.advance_frame_clock();
        harness.app.frame(DT);
        harness.app.ctx.run_deferred();
        let mph = speed(&harness);
        if !harness.app.ctx.co_driver_away {
            assert!(mph < 1.0, "handed back at {mph:.0}: {}", tail(&harness));
            break;
        }
        if mph < 0.5 && stopped.is_none() {
            let now = harness
                .app
                .ctx
                .profile
                .as_ref()
                .expect("a career")
                .hos
                .driving_min;
            stopped = Some(now - start);
        }
    }
    let minutes = stopped.expect("the truck stopped");
    eprintln!("stop from 75 mph took {minutes:.2} minutes on the clock");
    assert!(
        minutes < CO_DRIVER_STOP_MARGIN_MIN,
        "the stop took {minutes:.2} minutes"
    );
    let said = harness.transcript().join("\n");
    assert!(
        said.contains(
            "Dan stopped on the shoulder with no lot in reach and hands you the wheel. Their \
             hours are up."
        ),
        "{}",
        tail(&harness)
    );
}

#[test]
fn the_hand_over_rows_say_how_long_dan_can_drive() {
    let mut harness = quiet_drive("Team Hours Rows");
    let hos = &mut harness.app.ctx.profile.as_mut().expect("a career").hos;
    hos.sleep();
    hos.on_duty(60.0);
    hos.drive(6.2 * 60.0);
    harness.with_drive(|d, ctx| {
        let here = (d.trip.total_miles() - TRIP_LEFT_MI).max(3.0);
        d.trip.position_mi = here;
        d.enforcement_prev_mi = here;
        d.trip
            .stops
            .retain(|stop| stop.stop_type == "delivery_destination");
        d.ensure_team(ctx);
        if let Some(team) = d.team.as_mut() {
            team.off_wheel_hos = dans_logbook();
        }
    });
    let screen = harness.read_drive(|d| d.team_status_lines(&harness.app.ctx));
    assert_eq!(
        screen.first().map(String::as_str),
        Some("Co-driver: Dan, in the sleeper. Rest: 7.2 hours of 10 hours. 1.4 hours of driving left.")
    );
    harness.press_key(Key::Escape, None);
    let rows = harness.with_state::<PauseMenuState, _>(|pause, ctx| {
        pause
            .build_items(ctx)
            .iter()
            .map(|item| (item.label.text(pause, ctx), item.help.text(pause, ctx)))
            .collect::<Vec<_>>()
    });
    let sleep = rows
        .iter()
        .find(|(label, _)| label == "Hand the wheel to Dan for 1.2 hours and sleep")
        .unwrap_or_else(|| panic!("{rows:?}"));
    assert!(
        rows.iter()
            .any(|(label, _)| label == "Hand the wheel to Dan for 1.2 hours and ride along"),
        "{rows:?}"
    );
    assert!(
        sleep.1.contains(
            "Dan has 1.2 hours of driving left and no truck lot in reach, so they stop on the \
             shoulder and hand you the wheel. Your own hours reach the receiver from there."
        ),
        "{}",
        sleep.1
    );
    // The hand-back is awake: the help does not also promise the rest.
    assert!(
        !sleep.1.contains("once you have had your 10-hour rest"),
        "{}",
        sleep.1
    );
}

#[test]
fn a_rested_co_drivers_driver_screen_row_says_so() {
    let mut harness = quiet_drive("Team Hours Fresh");
    harness.with_drive(|d, ctx| d.ensure_team(ctx));
    let screen = harness.read_drive(|d| d.team_status_lines(&harness.app.ctx));
    assert_eq!(
        screen.first().map(String::as_str),
        Some("Co-driver: Dan, in the sleeper, rested.")
    );
    harness.app.ctx.settings.hos_mode = "off".to_string();
    let screen = harness.read_drive(|d| d.team_status_lines(&harness.app.ctx));
    assert_eq!(
        screen.first().map(String::as_str),
        Some("Co-driver: Dan, in the sleeper")
    );
}

#[test]
fn the_shift_machine_is_moved_by_the_drive() {
    let mut harness = quiet_drive("Team Hours Machine");
    assert_eq!(harness.read_drive(|d| d.team_shift()), TeamShift::Bunk);
    harness.with_drive(|d, ctx| d.hand_wheel_to_co_driver(ctx, TeamMode::RideAlong));
    assert_eq!(harness.read_drive(|d| d.team_shift()), TeamShift::Driving);
    harness.with_drive(|d, ctx| {
        d.team_shift_event(TeamShiftEvent::LastLotInReach);
        d.take_wheel_back(ctx);
    });
    assert_eq!(harness.read_drive(|d| d.team_shift()), TeamShift::Bunk);
}
