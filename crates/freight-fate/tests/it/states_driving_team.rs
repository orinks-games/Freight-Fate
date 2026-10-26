//! The co-driver driving whole loads through the real frame: the app's
//! own tick, the player's controls, and the screens the road opens.

use ff_core::models::co_driver::{self, PairingReason};
use ff_core::sim::weather::WeatherKind;
use freight_fate::playtest::harness::{PlaytestHarness, StartDelivery};
use freight_fate::states::base::{InputEvent, Key};
use freight_fate::states::driving::DrivingState;
use freight_fate::states::driving_menu_states::{ArrivalState, FacilityArrivalState};
use freight_fate::states::driving_team::{TeamDrive, TeamMode};

const DT: f64 = 1.0 / 60.0;

/// A quiet road: no traffic, hazards, inspections, patrols or weather, so
/// the case reads the co-driver and nothing else.
fn quiet_drive(name: &str) -> PlaytestHarness {
    let mut harness = PlaytestHarness::new();
    harness.start_delivery(StartDelivery::named(name));
    harness.with_drive(|d, _| {
        d.tutorial = None;
        d.departure_checked = true;
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

/// Tick the app until `done` or `max_frames` pass. Returns whether `done`
/// came true; on a timeout, prints where the drive got to.
fn run_until(
    harness: &mut PlaytestHarness,
    max_frames: usize,
    mut done: impl FnMut(&PlaytestHarness) -> bool,
) -> bool {
    for frame in 0..max_frames {
        harness.advance_frame_clock();
        harness.app.frame(DT);
        if done(harness) {
            return true;
        }
        if frame % (60 * 60) == 0 && harness.state_is::<DrivingState>() {
            let (at, total, mph) = harness.read_drive(|d| {
                (
                    d.trip.position_mi,
                    d.trip.total_miles(),
                    d.truck().speed_mph(),
                )
            });
            eprintln!("frame {frame}: {at:.2}/{total:.2} mi at {mph:.0} mph");
        }
    }
    false
}

fn transcript_tail(harness: &PlaytestHarness) -> String {
    let lines = harness.transcript();
    let from = lines.len().saturating_sub(40);
    lines[from..].join("\n")
}

#[test]
#[cfg_attr(ci_quick, ignore = "sweep: drives the last miles of a load asleep")]
fn a_slept_shift_runs_ahead_to_the_receivers_gate_and_wakes_the_driver() {
    let mut harness = quiet_drive("Team Run");
    harness.with_drive(|d, ctx| {
        d.truck_mut().start_engine();
        d.truck_mut().set_air_ready(false);
        let destination = d
            .destination_exit_stop(ctx)
            .expect("this route has a destination exit");
        d.trip.position_mi = (destination.at_mi - d.exit_window_mi() - 1.0).max(0.0);
        // Lou, the classmate who comes back from Meridian.
        let lou = co_driver::co_driver("lou").expect("Lou is in the cast");
        let mut team = TeamDrive::new(lou, PairingReason::Meridian);
        team.off_wheel_hos.sleep();
        d.team = Some(team);
        d.hand_wheel_to_co_driver(ctx, TeamMode::SleepThrough);
    });
    assert!(
        harness.app.ctx.co_driver_fast_forward,
        "a slept shift runs ahead"
    );
    let at_gate = run_until(&mut harness, 60 * 60 * 12, |h| {
        h.state_is::<FacilityArrivalState>()
    });
    assert!(
        at_gate,
        "the co-driver never reached the gate:\n{}",
        transcript_tail(&harness)
    );
    assert!(
        !harness.app.ctx.co_driver_away,
        "the wheel came back at the gate"
    );
    let said = harness.transcript().join("\n");
    assert!(said.contains("You slept"), "{said}");
    assert!(
        said.contains("Lou has the truck at the receiver's gate"),
        "{said}"
    );
    assert!(!harness.app.ctx.co_driver_fast_forward, "awake at the gate");
    harness.key(InputEvent::key(Key::Return));
    harness.finish_timed_state();
    assert!(harness.state_is::<ArrivalState>());
    let summary =
        harness.with_state::<ArrivalState, _>(|arrival, _| arrival.summary_parts.join(" "));
    assert!(summary.contains("Lou drove"), "{summary}");
    // The share is a row of its own on the settlement menu, read just
    // before the net pay it explains.
    let rows = harness.with_state::<ArrivalState, _>(|arrival, _| arrival.summary_lines.clone());
    let share = rows.iter().position(|l| l.starts_with("Lou drove"));
    let net = rows.iter().position(|l| l.starts_with("Net driver pay"));
    assert!(share.is_some() && share.map(|i| i + 1) == net, "{rows:?}");
    let note = harness.with_state::<ArrivalState, _>(|arrival, _| arrival.team_note.clone());
    assert!(
        note.as_deref()
            .is_some_and(|n| n.starts_with("Team load: Lou drove") && n.len() <= 60),
        "{note:?}"
    );
    let earned = &harness
        .app
        .ctx
        .profile
        .as_ref()
        .expect("a career")
        .achievements;
    assert!(
        earned.iter().any(|id| id == "meridian_classmate"),
        "{earned:?}"
    );
}

#[test]
#[cfg_attr(ci_quick, ignore = "sweep: drives through a fuel stop")]
fn the_co_driver_fuels_at_a_stop_and_gets_back_on_the_road() {
    let mut harness = quiet_drive("Team Fuel");
    let found = harness.with_drive(|d, ctx| {
        let window = d.exit_window_mi();
        let stop = d
            .trip
            .stops
            .iter()
            .filter(|stop| {
                stop.actions.iter().any(|a| a == "fuel")
                    && stop.stop_type != "weigh_station"
                    && stop.stop_type != "delivery_destination"
                    && stop.at_mi > window + 2.0
            })
            .map(|stop| stop.at_mi)
            .next();
        let Some(at_mi) = stop else {
            return false;
        };
        d.truck_mut().start_engine();
        d.truck_mut().set_air_ready(false);
        let tank = d.truck().specs.fuel_tank_gal;
        d.truck_mut().fuel_gal = tank * 0.3;
        d.trip.position_mi = at_mi - window - 1.0;
        d.hand_wheel_to_co_driver(ctx, TeamMode::RideAlong);
        true
    });
    assert!(found, "this route has a fuel stop");
    let mut menu_opened = false;
    let back_on_road = run_until(&mut harness, 60 * 60 * 20, |h| {
        menu_opened |= !h.state_is::<DrivingState>();
        h.state_is::<DrivingState>()
            && h.read_drive(|d| d.truck().fuel_fraction() > 0.9 && d.truck().speed_mph() > 30.0)
    });
    assert!(
        !menu_opened,
        "no stop menu to walk: {}",
        transcript_tail(&harness)
    );
    let logged = harness
        .app
        .ctx
        .message_log
        .filtered_messages()
        .iter()
        .any(|message| message.text.contains("fueled"));
    assert!(logged, "the fuel stop is in the message log");
    assert!(
        back_on_road,
        "the co-driver never fuelled and rejoined the road:\n{}",
        transcript_tail(&harness)
    );
    assert!(
        harness.app.ctx.co_driver_away,
        "the co-driver still has the wheel"
    );
}

fn team_ready() -> PlaytestHarness {
    let mut harness = PlaytestHarness::new();
    harness.start_delivery(StartDelivery::named("Team Keys"));
    harness
}

#[test]
fn the_pause_menu_offers_a_co_driver_only_when_the_load_needs_a_sleep() {
    let mut harness = team_ready();
    harness.press_key(Key::Escape, None);
    let fresh = harness.menu_labels();
    assert!(
        !fresh.iter().any(|row| row.starts_with("Hand the wheel")),
        "a load one shift covers offers no co-driver: {fresh:?}"
    );
    harness.key(InputEvent::key(Key::Escape));
    // Most of a shift already driven: the rest of the load needs a sleep.
    harness
        .app
        .ctx
        .profile
        .as_mut()
        .expect("a career")
        .hos
        .drive(600.0);
    harness.press_key(Key::Escape, None);
    let rows = harness.menu_labels();
    assert!(
        rows.contains(&"Hand the wheel to Dan and ride along".to_string()),
        "{rows:?}"
    );
    assert!(
        rows.contains(&"Hand the wheel to Dan and sleep".to_string()),
        "{rows:?}"
    );
}

#[test]
fn handing_over_swaps_the_logbooks_and_the_assists_and_taking_back_restores_them() {
    let mut harness = team_ready();
    harness
        .app
        .ctx
        .settings
        .apply_driving_assistance_preset("realistic");
    harness
        .app
        .ctx
        .profile
        .as_mut()
        .expect("a career")
        .hos
        .drive(300.0);
    harness.with_drive(|d, ctx| d.hand_wheel_to_co_driver(ctx, TeamMode::RideAlong));
    {
        let ctx = &harness.app.ctx;
        assert!(ctx.co_driver_away);
        assert_eq!(
            ctx.profile.as_ref().unwrap().hos.driving_min,
            0.0,
            "the co-driver's logbook"
        );
        assert!(ctx.settings.destination_approach_assist);
        assert!(ctx.settings.automatic_transmission);
        assert!(
            !ctx.settings.player_owned().destination_approach_assist,
            "the file keeps the player's own assists"
        );
    }
    let back = harness.with_drive(|d, ctx| d.take_wheel_back(ctx));
    assert!(back.is_some());
    let ctx = &harness.app.ctx;
    assert!(!ctx.co_driver_away);
    assert_eq!(ctx.profile.as_ref().unwrap().hos.driving_min, 300.0);
    assert!(!ctx.settings.destination_approach_assist);
}

#[test]
fn the_players_driving_keys_do_nothing_while_the_co_driver_drives() {
    let mut harness = team_ready();
    harness.with_drive(|d, ctx| d.hand_wheel_to_co_driver(ctx, TeamMode::RideAlong));
    let engine_before = harness.read_drive(|d| d.truck().engine_on);
    harness.press_key(Key::E, Some('e'));
    assert_eq!(harness.read_drive(|d| d.truck().engine_on), engine_before);
    let parked_before = harness.read_drive(|d| d.truck().parking_brake);
    harness.press_key(Key::P, Some('p'));
    assert_eq!(
        harness.read_drive(|d| d.truck().parking_brake),
        parked_before
    );
}

#[test]
fn a_radio_left_off_stays_off_for_the_whole_shift() {
    let mut harness = team_ready();
    harness.with_drive(|d, ctx| {
        d.truck_mut().start_engine();
        if d.radio.enabled {
            d.toggle_radio(ctx);
        }
        d.hand_wheel_to_co_driver(ctx, TeamMode::RideAlong);
    });
    for _ in 0..60 * 60 * 5 {
        harness.advance_frame_clock();
        harness.app.tick(DT);
        if !harness.state_is::<DrivingState>() {
            break;
        }
    }
    assert!(harness.read_drive(|d| !d.radio.enabled));
}

#[test]
fn a_manual_driver_keeps_the_manual_bonus_through_a_co_driver_shift() {
    let mut harness = team_ready();
    harness.app.ctx.settings.automatic_transmission = false;
    harness.with_drive(|d, ctx| {
        d.truck_mut().start_engine();
        d.trip.truck.transmission.automatic = false;
        d.hand_wheel_to_co_driver(ctx, TeamMode::RideAlong);
    });
    assert!(harness.read_drive(|d| d.truck().transmission.automatic));
    let moving = run_until(&mut harness, 60 * 90, |h| {
        h.read_drive(|d| d.truck().speed_mph() > 10.0)
    });
    assert!(moving, "the co-driver drives off");
    harness.with_drive(|d, ctx| d.take_wheel_back(ctx));
    for _ in 0..60 {
        harness.advance_frame_clock();
        harness.app.frame(DT);
    }
    let said = harness.transcript().join("\n");
    assert!(!said.contains("Transmission changed"), "{said}");
    assert!(harness.read_drive(|d| !d.truck().transmission.automatic));
    assert!(
        harness.read_drive(|d| !d.drove_automatic),
        "the co-driver's automatic miles are not the player's"
    );
}

#[test]
fn the_co_drivers_miles_are_the_miles_the_truck_covered() {
    let mut harness = quiet_drive("Team Miles");
    let start = harness.with_drive(|d, ctx| {
        d.trip.time_scale = 4.0;
        d.truck_mut().start_engine();
        d.hand_wheel_to_co_driver(ctx, TeamMode::RideAlong);
        d.trip.position_mi
    });
    let moved = run_until(&mut harness, 60 * 60 * 3, |h| {
        h.state_is::<DrivingState>() && h.read_drive(|d| d.trip.position_mi - start > 2.0)
    });
    assert!(moved, "the co-driver drove two miles");
    let (covered, logged) = harness.read_drive(|d| {
        (
            d.trip.position_mi - start,
            d.team.as_ref().map_or(0.0, |team| team.co_driver_mi),
        )
    });
    assert!(
        (logged - covered).abs() < covered * 0.1,
        "logged {logged:.2} of {covered:.2} miles"
    );
}

#[test]
fn a_stop_already_behind_the_truck_is_dropped_from_the_plan() {
    use freight_fate::states::driving_team::pilot::StopPurpose;
    let mut harness = quiet_drive("Team Plan");
    harness.with_drive(|d, ctx| {
        d.truck_mut().start_engine();
        d.truck_mut().set_air_ready(false);
        d.hand_wheel_to_co_driver(ctx, TeamMode::RideAlong);
        let wheel = d.team.as_mut().and_then(|t| t.wheel.as_mut()).unwrap();
        wheel.pilot.making_for = Some(("a stop not on this road".into(), StopPurpose::Fuel));
    });
    run_until(&mut harness, 60 * 60, |h| {
        h.read_drive(|d| d.truck().speed_mph() > 5.0)
    });
    let planned = harness.read_drive(|d| {
        d.team
            .as_ref()
            .and_then(|t| t.wheel.as_ref())
            .map(|w| w.pilot.making_for.clone())
    });
    assert!(
        planned.as_ref().is_some_and(|stop| stop
            .as_ref()
            .is_none_or(|(key, _)| key != "a stop not on this road")),
        "{planned:?}"
    );
}

#[test]
fn a_co_driver_out_of_hours_with_no_stop_in_reach_hands_back() {
    let mut harness = quiet_drive("Team Hours");
    harness.app.ctx.settings.hos_mode = "realistic".to_string();
    harness.with_drive(|d, ctx| {
        d.truck_mut().start_engine();
        d.hand_wheel_to_co_driver(ctx, TeamMode::RideAlong);
        d.trip
            .stops
            .retain(|stop| stop.stop_type == "delivery_destination");
    });
    // The co-driver's logbook, the profile's while they drive: five
    // minutes of driving left, the break already taken.
    let hos = &mut harness.app.ctx.profile.as_mut().expect("a career").hos;
    hos.drive(7.0 * 60.0);
    hos.take_break(30.0);
    hos.drive(4.0 * 60.0 - 5.0);
    let back = run_until(&mut harness, 60 * 60 * 2, |h| !h.app.ctx.co_driver_away);
    assert!(back, "the co-driver drove on past their hours");
    let said = harness.transcript().join("\n");
    assert!(said.contains("Their hours are up."), "{said}");
}

/// The fuel stops ahead of the truck that the co-driver could plan for.
fn fuel_stops_ahead(d: &DrivingState) -> Vec<String> {
    let past = d.trip.position_mi + d.exit_window_mi();
    d.trip
        .stops
        .iter()
        .filter(|stop| {
            stop.actions.iter().any(|a| a == "fuel")
                && stop.stop_type != "weigh_station"
                && stop.stop_type != "delivery_destination"
                && stop.at_mi > past
        })
        .map(|stop| stop.key())
        .collect()
}

#[test]
fn the_co_driver_keeps_a_planned_stop_that_serves_the_purpose() {
    use freight_fate::states::driving_team::pilot::StopPurpose;
    let mut harness = quiet_drive("Team Kept Plan");
    let planned = harness.with_drive(|d, ctx| {
        // A second fuel stop further on, planned ahead of the nearer one.
        let first = fuel_stops_ahead(d).first()?.clone();
        let mut second = d.trip.stops.iter().find(|s| s.key() == first)?.clone();
        second.at_mi += 20.0;
        let later = second.key();
        d.trip.stops.push(second);
        d.truck_mut().start_engine();
        d.truck_mut().set_air_ready(false);
        let tank = d.truck().specs.fuel_tank_gal;
        d.truck_mut().fuel_gal = tank * 0.3;
        d.trip.planned_stop_key = Some(later.clone());
        d.selected_stop_key = Some(later.clone());
        d.hand_wheel_to_co_driver(ctx, TeamMode::RideAlong);
        Some(later)
    });
    let planned = planned.expect("this route has two fuel stops");
    run_until(&mut harness, 60 * 30, |h| {
        h.read_drive(|d| {
            d.team
                .as_ref()
                .and_then(|t| t.wheel.as_ref())
                .is_some_and(|w| w.pilot.making_for.is_some())
        })
    });
    let making_for = harness.read_drive(|d| {
        d.team
            .as_ref()
            .and_then(|t| t.wheel.as_ref())
            .and_then(|w| w.pilot.making_for.clone())
    });
    assert_eq!(making_for, Some((planned, StopPurpose::Fuel)));
}

#[test]
fn leaving_a_stop_drops_the_co_drivers_plan() {
    use freight_fate::states::driving_team::pilot::StopPurpose;
    let mut harness = quiet_drive("Team Done Plan");
    let left = harness.with_drive(|d, ctx| {
        let later = fuel_stops_ahead(d).first()?.clone();
        d.truck_mut().start_engine();
        d.hand_wheel_to_co_driver(ctx, TeamMode::RideAlong);
        d.trip.planned_stop_key = Some(later.clone());
        d.selected_stop_key = Some(later.clone());
        let wheel = d.team.as_mut().and_then(|t| t.wheel.as_mut())?;
        wheel.pilot.making_for = Some((later, StopPurpose::Fuel));
        d.note_co_driver_choice(ctx, "Back to the road");
        Some((d.trip.planned_stop_key.clone(), d.selected_stop_key.clone()))
    });
    assert_eq!(left, Some((None, None)));
}

#[test]
fn a_slept_shift_is_simulated_rather_than_played() {
    let mut harness = quiet_drive("Team Sim");
    harness.with_drive(|d, ctx| {
        d.truck_mut().start_engine();
        d.truck_mut().set_air_ready(false);
        d.trip.position_mi = 3.0;
        d.truck_mut().velocity_mps = 60.0 / 2.236_936;
        d.cruise_mph = Some(60.0);
        d.hand_wheel_to_co_driver(ctx, TeamMode::SleepThrough);
        d.truck_mut().parking_brake = false;
    });
    let driven_before = harness
        .app
        .ctx
        .profile
        .as_ref()
        .expect("a career")
        .hos
        .driving_min;
    let (jump, start_min, end_min, miles, at) = harness.with_drive(|d, ctx| {
        let start_min = d.trip.game_minutes;
        let jump = d.sleep_ahead(ctx);
        let miles = d.team.as_ref().expect("a team").co_driver_mi;
        (
            jump,
            start_min,
            d.trip.game_minutes,
            miles,
            d.trip.position_mi,
        )
    });
    let jump = jump.expect("the open road ahead is simulated at once");
    assert!(jump.miles > 10.0, "{jump:?}");
    assert!(
        (at - 3.0 - jump.miles).abs() < 1e-6,
        "the truck is where the jump ended"
    );
    assert!(
        (miles - jump.miles).abs() < 1e-6,
        "the co-driver's miles: {miles}"
    );
    assert!((end_min - start_min - jump.minutes).abs() < 1e-6);
    let driven = harness
        .app
        .ctx
        .profile
        .as_ref()
        .expect("a career")
        .hos
        .driving_min;
    assert!(
        (driven - driven_before - jump.minutes).abs() < 1e-6,
        "the co-driver's logbook carries the drive"
    );
    // Short of the receiver's exit: the last stretch is driven live.
    let exit = harness.with_drive(|d, ctx| d.destination_exit_stop(ctx).map(|s| s.at_mi));
    assert!(exit.is_some_and(|exit| at < exit), "{at} past {exit:?}");
}

/// The truck on the open road at 60 with the cruise set, the co-driver at
/// the wheel and the player asleep.
fn asleep_at_speed(name: &str) -> PlaytestHarness {
    let mut harness = quiet_drive(name);
    harness.with_drive(|d, ctx| {
        d.truck_mut().start_engine();
        d.truck_mut().set_air_ready(false);
        d.trip.position_mi = 3.0;
        d.truck_mut().velocity_mps = 60.0 / 2.236_936;
        d.cruise_mph = Some(60.0);
        d.hand_wheel_to_co_driver(ctx, TeamMode::SleepThrough);
        d.truck_mut().parking_brake = false;
    });
    harness
}

#[test]
fn a_slept_shift_fuels_without_leaving_the_road() {
    let mut harness = asleep_at_speed("Team Sim Fuel");
    let (first_fuel, exit) = harness.with_drive(|d, ctx| {
        let tank = d.truck().specs.fuel_tank_gal;
        d.truck_mut().fuel_gal = tank * 0.2;
        let exit = d.destination_exit_stop(ctx).map(|s| s.at_mi);
        (fuel_stops_ahead(d).first().cloned(), exit)
    });
    let first_fuel = first_fuel.expect("this route sells diesel");
    let (jump, fuel, fueled_at) = harness.with_drive(|d, ctx| {
        let jump = d.sleep_ahead(ctx);
        let fueled = d
            .team
            .as_ref()
            .and_then(|team| team.wheel.as_ref())
            .map(|wheel| wheel.log.fuel_stops.clone())
            .unwrap_or_default();
        (jump, d.truck().fuel_fraction(), fueled)
    });
    let jump = jump.expect("the shift passes as time");
    assert!(
        !fueled_at.is_empty(),
        "fuelled on the way ({first_fuel}, exit at {exit:?}): {jump:?}"
    );
    assert!(fuel > 0.5, "the tank was filled: {fuel}");
    assert!(
        harness.state_is::<DrivingState>(),
        "no stop menu opened for the fuel"
    );
}

#[test]
fn a_slept_shift_ends_parked_at_a_stop_with_the_driver_woken() {
    use freight_fate::states::driving_rest_states::{ParkingFullState, RestStopState};
    let mut harness = asleep_at_speed("Team Sim Shift");
    harness.with_drive(|d, ctx| {
        // A lot fifteen miles on, where the last of the hours run out.
        let lot = d
            .trip
            .stops
            .iter()
            .find(|stop| {
                stop.parking != "none"
                    && stop.stop_type != "weigh_station"
                    && stop.stop_type != "delivery_destination"
                    && stop.at_mi > 20.0
            })
            .map(|stop| stop.at_mi)
            .expect("this route has somewhere to park");
        d.trip.position_mi = lot - 15.0;
        d.enforcement_prev_mi = d.trip.position_mi;
        // The co-driver's logbook: ten minutes of driving left before
        // they plan the end of the shift.
        let hos = &mut ctx.profile.as_mut().expect("a career").hos;
        hos.driving_min = 11.0 * 60.0 - co_driver::CO_DRIVER_SUMMON_LEAD_MIN - 10.0;
        hos.duty_min = hos.driving_min;
        hos.since_break_min = 0.0;
    });
    let (jump, at, speed) = harness.with_drive(|d, ctx| {
        let jump = d.sleep_ahead(ctx);
        (jump, d.trip.position_mi, d.truck().speed_mph())
    });
    harness.app.ctx.run_deferred();
    assert!(jump.is_some(), "the shift passes as time");
    assert!(speed < 0.5, "parked at {at}");
    assert!(
        harness.state_is::<RestStopState>() || harness.state_is::<ParkingFullState>(),
        "the stop's menu is open: {}",
        transcript_tail(&harness)
    );
    // The co-driver hands back from the stop's menu.
    let woken = run_until(&mut harness, 600, |h| {
        h.read_drive(|d| d.team.as_ref().is_some_and(|team| team.wheel.is_none()))
    });
    assert!(woken, "{}", transcript_tail(&harness));
    let said = harness.transcript().join("\n");
    assert!(said.contains("Their hours are up"), "{said}");
}
