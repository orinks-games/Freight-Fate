//! When the co-driver hands back: once the player in the sleeper has had
//! their 10-hour rest, or sooner if the co-driver's own hours run out. And
//! the one-time introduction the first time a career is offered a
//! co-driver.

use ff_core::models::co_driver;
use ff_core::sim::weather::WeatherKind;
use freight_fate::playtest::harness::{PlaytestHarness, StartDelivery};
use freight_fate::states::base::{InputEvent, Key};
use freight_fate::states::driving_rest_states::{ParkingFullState, RestFocus, RestStopState};
use freight_fate::states::driving_team::overlay::{overlay_move, OverlayMove};
use freight_fate::states::driving_team::pilot::StopPurpose;
use freight_fate::states::driving_team::TeamMode;

const DT: f64 = 1.0 / 60.0;

const INTRO: &str = "This load is long enough to share with a co-driver.";

fn quiet_drive(name: &str) -> PlaytestHarness {
    let mut harness = PlaytestHarness::new();
    harness.start_delivery(StartDelivery::named(name));
    harness.app.ctx.settings.hos_mode = "realistic".to_string();
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

fn run_until(
    harness: &mut PlaytestHarness,
    max_frames: usize,
    mut done: impl FnMut(&PlaytestHarness) -> bool,
) -> bool {
    for _ in 0..max_frames {
        harness.advance_frame_clock();
        harness.app.frame(DT);
        if done(harness) {
            return true;
        }
    }
    false
}

fn tail(harness: &PlaytestHarness) -> String {
    let lines = harness.transcript();
    let from = lines.len().saturating_sub(30);
    lines[from..].join("\n")
}

/// The player has driven `driven_min` of a shift, then hands the wheel to a
/// rested co-driver.
fn hand_over_after(harness: &mut PlaytestHarness, driven_min: f64, mode: TeamMode) {
    harness
        .app
        .ctx
        .profile
        .as_mut()
        .expect("a career")
        .hos
        .drive(driven_min);
    harness.with_drive(|d, ctx| d.hand_wheel_to_co_driver(ctx, mode));
}

/// Add `minutes` to the player's own logbook, in the sleeper.
fn player_sleeper(harness: &mut PlaytestHarness, minutes: f64) {
    harness.with_drive(|d, _| {
        let team = d.team.as_mut().expect("a co-driver");
        team.off_wheel_hos.sleeper(minutes);
    });
}

#[test]
fn c_reads_the_rest_so_far_and_when_the_co_driver_hands_back() {
    let mut harness = quiet_drive("Team Rest Readout");
    hand_over_after(&mut harness, 300.0, TeamMode::RideAlong);
    player_sleeper(&mut harness, 210.0);
    let clause = harness.read_drive(|d| d.hos_nearest_clause(&harness.app.ctx));
    assert_eq!(
        clause, "Rest: 3.5 hours of 10 hours. Dan hands back in about 6.5 hours.",
        "the rest comes in before Dan's hours run out"
    );
    player_sleeper(&mut harness, 390.0);
    let clause = harness.read_drive(|d| d.hos_nearest_clause(&harness.app.ctx));
    assert_eq!(
        clause,
        "Your 10-hour rest is in. Dan hands back at the next place to park."
    );
}

#[test]
fn a_co_driver_short_of_hours_says_they_need_you_before_your_rest_is_in() {
    let mut harness = quiet_drive("Team Rest Short");
    hand_over_after(&mut harness, 300.0, TeamMode::RideAlong);
    // The co-driver's logbook, the profile's while they drive: two hours
    // of driving left before they hand back.
    let hos = &mut harness.app.ctx.profile.as_mut().expect("a career").hos;
    hos.drive(7.0 * 60.0);
    hos.take_break(30.0);
    hos.drive(2.0 * 60.0 - co_driver::CO_DRIVER_SUMMON_LEAD_MIN);
    let clause = harness.read_drive(|d| d.hos_nearest_clause(&harness.app.ctx));
    assert_eq!(
        clause,
        "Rest: 0 minutes of 10 hours. Dan needs you back in 2 hours, before your rest is in."
    );
}

#[test]
fn a_player_rested_at_the_handover_gets_the_co_drivers_whole_shift() {
    let mut harness = quiet_drive("Team Rest Whole");
    harness
        .app
        .ctx
        .profile
        .as_mut()
        .expect("a career")
        .hos
        .sleep();
    harness.with_drive(|d, ctx| d.hand_wheel_to_co_driver(ctx, TeamMode::RideAlong));
    player_sleeper(&mut harness, 700.0);
    let (clause, complete) = harness.read_drive(|d| {
        (
            d.hos_nearest_clause(&harness.app.ctx),
            d.player_rest_complete(&harness.app.ctx),
        )
    });
    assert!(!complete, "nothing was owed, so nothing ends the shift");
    assert!(clause.starts_with("Dan needs you back in "), "{clause}");
}

#[test]
fn the_pause_menu_reads_the_rest_in_the_take_the_wheel_back_help() {
    let mut harness = quiet_drive("Team Rest Pause");
    hand_over_after(&mut harness, 300.0, TeamMode::RideAlong);
    player_sleeper(&mut harness, 60.0);
    harness.press_key(Key::Escape, None);
    let help = harness.with_state::<freight_fate::states::driving_pause_states::PauseMenuState, _>(
        |pause, ctx| {
            use freight_fate::states::base::Menu;
            let items = pause.build_items(ctx);
            items
                .iter()
                .find(|item| item.label.text(pause, ctx) == "Take the wheel back")
                .map(|item| item.help.text(pause, ctx))
        },
    );
    assert_eq!(
        help.as_deref(),
        Some(
            "Rest: 1 hour of 10 hours. Dan hands back in about 9 hours. Wakes you and gives you the \
             wheel."
        )
    );
}

#[test]
fn a_slept_shift_ends_parked_once_the_players_rest_is_in() {
    let mut harness = quiet_drive("Team Rest Sleep");
    harness
        .app
        .ctx
        .profile
        .as_mut()
        .expect("a career")
        .hos
        .drive(300.0);
    harness.with_drive(|d, ctx| {
        d.truck_mut().start_engine();
        d.truck_mut().set_air_ready(false);
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
        d.truck_mut().velocity_mps = 60.0 / 2.236_936;
        d.cruise_mph = Some(60.0);
        d.hand_wheel_to_co_driver(ctx, TeamMode::SleepThrough);
        d.truck_mut().parking_brake = false;
        // Five minutes of the player's rest still to come; the co-driver
        // is fresh.
        let team = d.team.as_mut().expect("a co-driver");
        team.off_wheel_hos
            .sleeper(co_driver::rest_left_min(&team.off_wheel_hos) - 5.0);
    });
    let jump = harness.with_drive(|d, ctx| d.sleep_ahead(ctx));
    harness.app.ctx.run_deferred();
    assert!(jump.is_some(), "the shift passes as time");
    assert!(
        harness.state_is::<RestStopState>() || harness.state_is::<ParkingFullState>(),
        "the stop's menu is open: {}",
        tail(&harness)
    );
    let woken = run_until(&mut harness, 60 * 60, |h| {
        h.read_drive(|d| d.team.as_ref().is_some_and(|team| team.wheel.is_none()))
    });
    assert!(woken, "{}", tail(&harness));
    let said = harness.transcript().join("\n");
    assert!(
        said.contains("Dan has parked and hands you the wheel. Your 10-hour rest is in."),
        "{said}"
    );
    assert!(!said.contains("Their hours are up"), "{said}");
    let profile = harness.app.ctx.profile.as_ref().expect("a career");
    assert!(
        co_driver::fully_rested(&profile.hos),
        "the player takes the wheel rested"
    );
}

#[test]
fn woken_early_the_player_hears_how_much_rest_they_had() {
    let mut harness = quiet_drive("Team Rest Early");
    hand_over_after(&mut harness, 300.0, TeamMode::RideAlong);
    harness.with_drive(|d, _| {
        d.truck_mut().start_engine();
        d.trip
            .stops
            .retain(|stop| stop.stop_type == "delivery_destination");
    });
    player_sleeper(&mut harness, 120.0);
    let hos = &mut harness.app.ctx.profile.as_mut().expect("a career").hos;
    hos.drive(7.0 * 60.0);
    hos.take_break(30.0);
    hos.drive(4.0 * 60.0 - 5.0);
    // Damage wakes the player: the end of the co-driver's hours no longer
    // does while the rest is owed.
    harness.with_drive(|d, _| d.truck_mut().damage_pct += 5.0);
    let back = run_until(&mut harness, 60 * 60 * 2, |h| !h.app.ctx.co_driver_away);
    assert!(back, "the co-driver drove on past the damage");
    let said = harness.transcript().join("\n");
    assert!(
        said.contains(
            "Dan needs you. The truck took damage. You had 2 hours of your 10-hour rest."
        ),
        "{said}"
    );
}

#[test]
fn woken_from_a_slept_shift_the_player_hears_the_rest_still_to_go() {
    // "You slept" already gives the time, so the rest line says what is
    // left of it rather than repeating the same number. Damage is what
    // wakes the player here: the end of the co-driver's hours no longer
    // does while the rest is owed.
    let mut harness = slept_shift_with_no_lot("Team Rest Woken");
    harness.with_drive(|d, _| d.truck_mut().damage_pct += 5.0);
    let woken = run_until(&mut harness, 60 * 60, |h| {
        h.read_drive(|d| d.team.as_ref().is_some_and(|team| team.wheel.is_none()))
    });
    assert!(woken, "{}", tail(&harness));
    let said = harness.transcript().join("\n");
    assert!(said.contains("You slept"), "{said}");
    assert!(
        said.contains("Dan needs you. The truck took damage."),
        "{said}"
    );
    assert!(said.contains("of your 10-hour rest still to go."), "{said}");
    assert!(!said.contains("You had"), "{said}");
}

#[test]
fn with_no_lot_in_reach_the_rest_is_slept_out_on_the_shoulder() {
    // An agent drive on I-5, 2026-10-22: no truck lot between Corning and
    // Ripon, and Dan handed back on the road with 8.7 hours of the rest
    // still owed.
    let mut harness = slept_shift_with_no_lot("Team Rest Shoulder");
    let woken = run_until(&mut harness, 60 * 60 * 5, |h| {
        h.read_drive(|d| d.team.as_ref().is_some_and(|team| team.wheel.is_none()))
    });
    assert!(woken, "{}", tail(&harness));
    let said = harness.transcript().join("\n");
    assert!(
        said.contains(
            "Dan pulled onto the shoulder when their hours ran out, with no lot in reach, and \
             you slept on there. Your 10-hour rest is in."
        ),
        "{}",
        tail(&harness)
    );
    assert!(!said.contains("Their hours are up"), "{said}");
    assert!(!said.contains("of your 10-hour rest"), "{said}");
    let speed = harness.read_drive(|d| d.trip.truck.speed_mph());
    assert!(speed < 1.0, "stopped on the shoulder: {speed}");
    let hos = &harness.app.ctx.profile.as_ref().expect("a career").hos;
    assert_eq!(hos.driving_min, 0.0, "the player's clock is reset");
    assert!(
        hos.duty_min < 1.0,
        "the player's window is reset: {}",
        hos.duty_min
    );
}

/// The player drove 5 hours and handed Dan a sleeping shift with only an
/// hour of his own driving left, on a road with no lot to end it at; the
/// hour passes as time.
fn slept_shift_with_no_lot(name: &str) -> PlaytestHarness {
    let mut harness = quiet_drive(name);
    hand_over_after(&mut harness, 300.0, TeamMode::SleepThrough);
    harness.with_drive(|d, _| {
        d.trip
            .stops
            .retain(|stop| stop.stop_type == "delivery_destination");
        d.truck_mut().start_engine();
        d.truck_mut().set_air_ready(false);
        d.truck_mut().parking_brake = false;
        d.truck_mut().velocity_mps = 60.0 / 2.236_936;
        d.cruise_mph = Some(60.0);
    });
    let hos = &mut harness.app.ctx.profile.as_mut().expect("a career").hos;
    hos.drive(7.0 * 60.0);
    hos.take_break(30.0);
    hos.drive(3.0 * 60.0);
    let jump = harness.with_drive(|d, ctx| d.sleep_ahead(ctx));
    harness.app.ctx.run_deferred();
    assert!(jump.is_some(), "the shift passes as time");
    harness
}

#[test]
fn the_first_offer_of_a_co_driver_explains_it_once() {
    let mut harness = PlaytestHarness::new();
    harness.start_delivery(StartDelivery::named("Team Intro"));
    harness
        .app
        .ctx
        .profile
        .as_mut()
        .expect("a career")
        .hos
        .drive(600.0);
    harness.press_key(Key::Escape, None);
    let said = harness.transcript().join("\n");
    assert!(
        said.contains(&format!(
            "{INTRO} Dan can take the wheel from this menu while you rest in the sleeper. Dan \
             hands the wheel back once you have had your 10-hour rest."
        )),
        "{said}"
    );
    harness.key(InputEvent::key(Key::Escape));
    harness.press_key(Key::Escape, None);
    let heard = harness
        .transcript()
        .iter()
        .filter(|line| line.contains(INTRO))
        .count();
    assert_eq!(heard, 1, "said once per career");
}

#[test]
fn an_offer_that_turns_up_on_a_refresh_keeps_the_introduction_for_later() {
    use freight_fate::states::base::Menu;
    use freight_fate::states::driving_pause_states::PauseMenuState;
    let mut harness = PlaytestHarness::new();
    harness.start_delivery(StartDelivery::named("Team Intro Later"));
    // Paused with a load one shift covers: no co-driver yet.
    harness.press_key(Key::Escape, None);
    // Now the hours make it worth sharing, and the menu rebuilds its rows
    // without announcing itself again.
    harness
        .app
        .ctx
        .profile
        .as_mut()
        .expect("a career")
        .hos
        .drive(600.0);
    harness.with_state::<PauseMenuState, _>(|menu, ctx| menu.refresh(ctx, true));
    let stats = &harness
        .app
        .ctx
        .profile
        .as_ref()
        .expect("a career")
        .achievement_stats;
    assert!(
        !co_driver::introduced(stats),
        "built but never said, so not yet heard"
    );
    harness.key(InputEvent::key(Key::Escape));
    harness.press_key(Key::Escape, None);
    let said = harness.transcript().join("\n");
    assert!(said.contains(INTRO), "{said}");
}

#[test]
fn a_rested_co_driver_whose_hours_run_out_keeps_the_lot_they_chose() {
    // The rest came in and Dan chose a lot within his hours, farther than
    // the short look he takes when his hours run out first. Running out on
    // the way does not drop that lot and hand back on the road.
    let mut harness = quiet_drive("Team Rest Keep Lot");
    hand_over_after(&mut harness, 300.0, TeamMode::RideAlong);
    harness.with_drive(|d, _| {
        d.truck_mut().start_engine();
        d.truck_mut().set_air_ready(false);
        d.truck_mut().parking_brake = false;
        d.truck_mut().velocity_mps = 60.0 / 2.236_936;
        d.cruise_mph = Some(60.0);
        let here = d.trip.position_mi;
        let lot = d
            .trip
            .stops
            .iter()
            .find(|stop| {
                stop.parking != "none"
                    && stop.stop_type != "weigh_station"
                    && stop.stop_type != "delivery_destination"
                    && stop.at_mi > here + 45.0
            })
            .map(|stop| stop.key())
            .expect("this route has a lot well ahead");
        let wheel = d
            .team
            .as_mut()
            .and_then(|team| team.wheel.as_mut())
            .expect("Dan is driving");
        wheel.pilot.shift_over = true;
        wheel.pilot.rested = true;
        wheel.pilot.making_for = Some((lot, StopPurpose::ShiftEnd));
    });
    let planned = harness.read_drive(|d| {
        d.team
            .as_ref()
            .and_then(|team| team.wheel.as_ref())
            .and_then(|wheel| wheel.pilot.making_for.clone())
    });
    // Dan's own hours are now inside the lead he hands back on, though
    // not yet gone.
    let hos = &mut harness.app.ctx.profile.as_mut().expect("a career").hos;
    hos.drive(7.0 * 60.0);
    hos.take_break(30.0);
    hos.drive(4.0 * 60.0 - co_driver::CO_DRIVER_SUMMON_LEAD_MIN / 2.0);
    run_until(&mut harness, 120, |_| false);
    let (away, now) = harness.read_drive(|d| {
        (
            d.co_driver_driving(),
            d.team
                .as_ref()
                .and_then(|team| team.wheel.as_ref())
                .and_then(|wheel| wheel.pilot.making_for.clone()),
        )
    });
    assert!(away, "Dan handed back on the road: {}", tail(&harness));
    assert_eq!(now, planned);
}

#[test]
fn once_rested_the_co_driver_hands_back_at_a_lot_and_not_at_a_scale() {
    let mut harness = quiet_drive("Team Rest Scale");
    hand_over_after(&mut harness, 300.0, TeamMode::RideAlong);
    player_sleeper(&mut harness, 600.0);
    let lot = harness.read_drive(|d| {
        d.trip
            .stops
            .iter()
            .find(|stop| {
                stop.parking != "none"
                    && stop.stop_type != "weigh_station"
                    && stop.stop_type != "delivery_destination"
            })
            .cloned()
            .expect("a stop with parking")
    });
    let move_at = |harness: &mut PlaytestHarness, scale: bool, marked: bool| {
        let mut stop = lot.clone();
        harness.with_drive(|d, ctx| {
            // `marked`: the pilot has already seen the rest come in; without
            // it the rest came in while they were stopped here.
            if let Some(wheel) = d.team.as_mut().and_then(|team| team.wheel.as_mut()) {
                wheel.pilot.shift_over = marked;
                wheel.pilot.rested = marked;
            }
            if scale {
                stop.stop_type = "weigh_station".to_string();
                stop.parking = "none".to_string();
            }
            stop.at_mi = d.trip.position_mi;
            d.trip
                .stops
                .retain(|other| other.stop_type == "delivery_destination");
            d.trip.stops.push(stop.clone());
            let mut screen = RestStopState::new(ctx, stop, RestFocus::Default);
            screen.enter_over_drive(ctx, d);
            overlay_move(&screen, d, ctx)
        })
    };
    let at_scale = move_at(&mut harness, true, true);
    assert!(
        matches!(&at_scale, OverlayMove::Row(_)),
        "Dan works the scale and drives on: {at_scale:?}"
    );
    for marked in [true, false] {
        let at_lot = move_at(&mut harness, false, marked);
        assert_eq!(
            at_lot,
            OverlayMove::Summon("rested".into()),
            "marked {marked}"
        );
    }
}

#[test]
fn a_break_due_with_no_stop_in_reach_is_taken_on_the_shoulder() {
    // The same agent drive, 2026-10-22: Dan's break came due in the Shasta
    // mountains with no stop in reach, and he woke the player with 2.2
    // hours of the rest still owed.
    let mut harness = quiet_drive("Team Rest Shoulder Break");
    hand_over_after(&mut harness, 300.0, TeamMode::RideAlong);
    harness.with_drive(|d, _| {
        d.trip
            .stops
            .retain(|stop| stop.stop_type == "delivery_destination");
        d.truck_mut().start_engine();
        d.truck_mut().set_air_ready(false);
        d.truck_mut().parking_brake = false;
        d.truck_mut().velocity_mps = 60.0 / 2.236_936;
        d.cruise_mph = Some(60.0);
    });
    let hos = &mut harness.app.ctx.profile.as_mut().expect("a career").hos;
    hos.drive(7.0 * 60.0 + 50.0);
    let took = run_until(&mut harness, 60 * 60 * 2, |h| {
        h.transcript()
            .iter()
            .any(|line| line.contains("Dan took a 30-minute break on the shoulder."))
    });
    assert!(took, "{}", tail(&harness));
    assert!(harness.app.ctx.co_driver_away, "Dan drives on");
    let said = harness.transcript().join("\n");
    assert!(!said.contains("Their break is due"), "{said}");
    let since_break = harness
        .app
        .ctx
        .profile
        .as_ref()
        .expect("a career")
        .hos
        .since_break_min;
    assert!(since_break < 5.0, "the break counts: {since_break}");
}

#[test]
fn with_the_rest_in_a_break_due_short_of_a_stop_hands_back() {
    // The shoulder break is for a player still owed rest. Rested, the
    // player takes the wheel instead of the truck stopping in its lane.
    let mut harness = quiet_drive("Team Rested Break");
    hand_over_after(&mut harness, 300.0, TeamMode::RideAlong);
    player_sleeper(&mut harness, 600.0);
    harness.with_drive(|d, _| {
        d.trip
            .stops
            .retain(|stop| stop.stop_type == "delivery_destination");
        d.truck_mut().start_engine();
        d.truck_mut().set_air_ready(false);
        d.truck_mut().parking_brake = false;
        d.truck_mut().velocity_mps = 60.0 / 2.236_936;
        d.cruise_mph = Some(60.0);
    });
    let hos = &mut harness.app.ctx.profile.as_mut().expect("a career").hos;
    hos.drive(7.0 * 60.0 + 50.0);
    let woken = run_until(&mut harness, 60 * 60 * 2, |h| {
        h.read_drive(|d| d.team.as_ref().is_some_and(|team| team.wheel.is_none()))
    });
    assert!(woken, "{}", tail(&harness));
    let said = harness.transcript().join("\n");
    assert!(said.contains("Their break is due."), "{said}");
    assert!(!said.contains("on the shoulder"), "{said}");
}
