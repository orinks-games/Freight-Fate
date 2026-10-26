//! A facility's streets as the map has them: each street at its own posted
//! limit, the lights and signs OSM reads at its intersections played through
//! the ramp terminal's own machinery, the yard behind the driveway, the chain
//! that starts where the ramp lands, and the speed keeper between corners too
//! close to build up for (owner order, 2026-09-24).
//!
//! The lights and signs are off in 1.9 (`STREET_CONTROLS_IN_PLAY`) and come
//! back in 2.0; the drives here switch them on (`street_controls_on`), so
//! this file is the 2.0 suite for them.

use ff_core::data::world::get_world;
use ff_core::data::world_models::{Leg, Route, StreetControl, StreetLimit};
use ff_core::sim::trip::{Trip, TripOptions};
use ff_core::sim::trip_models::{FACILITY_GATE_LIMIT_MPH, YARD_LIMIT_MPH};
use ff_core::sim::weather::{WeatherKind, WeatherSystem};
use freight_fate::app::testing::TestApp;
use freight_fate::playtest::harness::PlaytestHarness;
use freight_fate::states::base::Key;
use freight_fate::states::driving::DrivingState;
use freight_fate::states::driving_core::*;
use freight_fate::states::driving_events::street_controls::STREET_BAR_BEFORE_NODE_MI;
use freight_fate::states::driving_turns::is_judged_turn;

use crate::transcript_cruise_support::{
    frame, hold, press, quiet, release_keys, start_drive, turn_cues, DT, MPS_PER_MPH,
};

// -- rigging -------------------------------------------------------------------------

fn limit(mph: f64, source: &str) -> Option<StreetLimit> {
    Some(StreetLimit {
        mph,
        source: source.to_string(),
        ..Default::default()
    })
}

fn control(at_mi: f64, kind: &str) -> StreetControl {
    StreetControl {
        at_mi,
        kind: kind.to_string(),
    }
}

/// A delivery's last streets as the bake gives them: a 30 mph statutory
/// street with `first_controls` along it, a left onto a 40 mph READ arterial
/// carrying `second_controls`, and a right onto the service road that is the
/// yard behind the driveway.
fn chain(
    d: &mut DrivingState,
    first_controls: Vec<StreetControl>,
    second_controls: Vec<StreetControl>,
) {
    let city = d.trip.route.cities[0].clone();
    let legs = vec![
        Leg::local(
            &city,
            0.8,
            "East Navarre Street",
            "Start on East Navarre Street.",
            30.0,
        )
        .with_street(limit(30.0, "statutory"), first_controls),
        Leg::local(
            &city,
            0.8,
            "North Michigan Street",
            "Turn left onto North Michigan Street.",
            40.0,
        )
        .with_turn_deg(90.0)
        .with_street(limit(40.0, "read"), second_controls),
        Leg::local(
            &city,
            0.1,
            "a service road",
            "Turn right onto a service road.",
            15.0,
        )
        .with_turn_deg(90.0)
        .with_street(limit(15.0, "assumed"), Vec::new())
        .with_yard(true),
    ];
    let route = Route::from_legs(vec![city; 4], legs);
    let truck = d.trip.truck.clone();
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
    quiet(&mut trip);
    d.trip = trip;
    d.reset_turn_state_for_trip();
    d.destination_exit_taken = true;
    d.trip_seed = 7;
    d.street_controls_on = true;
}

/// A drive on [`chain`], rolling at `mph` from `at_mi`, with the assists as
/// given: every assist on, or none.
fn on_the_streets(
    name: &str,
    first_controls: Vec<StreetControl>,
    second_controls: Vec<StreetControl>,
    assists: bool,
    at_mi: f64,
    mph: f64,
) -> PlaytestHarness {
    let mut harness = start_drive(name);
    release_keys(&mut harness);
    {
        let s = &mut harness.app.ctx.settings;
        s.time_scale = 1.0;
        s.automatic_transmission = true;
        s.automatic_emergency_braking = false;
        s.route_transition_assist = assists;
        s.destination_approach_assist = assists;
        s.speed_keeper = assists;
        s.curve_speed_assist = assists;
        s.lane_keeping = "full".to_string();
    }
    harness.with_drive(move |d, _| {
        chain(d, first_controls, second_controls);
        d.tutorial = None;
        d.truck_mut().start_engine();
        d.truck_mut().transmission.automatic = true;
        d.truck_mut().transmission.gear = 6;
        d.truck_mut().set_air_ready(false);
        d.truck_mut().velocity_mps = mph * MPS_PER_MPH;
        d.trip.position_mi = at_mi;
    });
    if assists {
        press(&mut harness, Key::K, None);
        assert!(
            harness.read_drive(|d| d.keeper_mph.is_some()),
            "the keeper holds the street"
        );
    }
    harness.clear_speech();
    harness
}

/// Frames until `until` holds, at most `limit` of them; the frames run.
fn run_until(
    harness: &mut PlaytestHarness,
    limit: usize,
    mut until: impl FnMut(&DrivingState) -> bool,
) -> usize {
    for n in 0..limit {
        if !harness.has_drive() || harness.read_drive(&mut until) {
            return n;
        }
        harness.with_drive(|d, _| {
            let cut_out = d.truck().specs.air_governor_cut_out_psi;
            d.truck_mut().set_air_pressure_psi(cut_out);
        });
        frame(harness, DT);
    }
    limit
}

/// Pin the live street light red (or green) from the start of its cycle.
fn pin_light(d: &mut DrivingState, red: bool) {
    d.ramp_light_timer = 0.0;
    d.ramp_light_offset_s = if red { 0.0 } else { d.ramp_light_red_s() + 1.0 };
}

// -- per-street limits ----------------------------------------------------------------

#[test]
fn test_each_street_posts_its_own_limit_and_the_yard_its_own() {
    let mut harness = start_drive("Street Limits");
    harness.with_drive(|d, _| chain(d, Vec::new(), Vec::new()));
    let zones = harness.read_drive(|d| {
        d.trip
            .zones
            .iter()
            .map(|z| (z.reason.clone(), z.limit_mph))
            .collect::<Vec<_>>()
    });
    assert_eq!(
        zones,
        vec![
            ("facility access road".to_string(), 30.0),
            ("facility access road".to_string(), 40.0),
            ("yard".to_string(), YARD_LIMIT_MPH),
        ]
    );
    // The corner onto the arterial is priced against the arterial's own
    // limit, and the driveway is a corner too, judged at its own speed.
    let (arterial, driveway) = harness.with_drive(|d, _| {
        let cues = turn_cues(d);
        assert!(cues.iter().all(is_judged_turn));
        (d.turn_speed_mph(&cues[0]), d.turn_speed_mph(&cues[1]))
    });
    assert!(arterial <= 40.0);
    assert!(driveway < FACILITY_GATE_LIMIT_MPH, "{driveway}");
}

#[test]
fn test_a_new_street_says_its_limit_and_the_yard_says_the_yard_limit() {
    let mut harness = on_the_streets("Street Lines", Vec::new(), Vec::new(), false, 0.7, 25.0);
    let heard = drive_to_the_gate(&mut harness);
    assert!(heard.contains("Speed limit raised to 40."), "{heard}");
    assert!(heard.contains("Into the yard. Yard limit 15."), "{heard}");
    assert!(!heard.contains("facility gate zone"), "{heard}");
    // The street change is said as a change, and a street is never called
    // a zone: "facility access road zone" named miles of city streets as one
    // (live drive into Abilene, 2026-09-24).
    assert!(!heard.contains("access road zone"), "{heard}");
    assert!(!heard.contains("Access Road zone"), "{heard}");
}

#[test]
fn test_cruise_handing_a_street_to_the_keeper_names_no_zone() {
    // "Facility Access Road zone. Speed keeper holding 30" called a city
    // street a zone when adaptive cruise handed it to the keeper.
    let mut harness = on_the_streets("Street Handoff", Vec::new(), Vec::new(), false, 0.2, 25.0);
    harness.app.ctx.settings.speed_keeper = true;
    harness.with_drive(|d, ctx| {
        d.cruise_mph = Some(30.0);
        d.speed_control_armed = true;
        d.update_cruise(ctx, DT, false, false, false);
    });
    let heard = harness.transcript_text();
    assert!(
        heard.contains("Speed keeper holding 30 miles per hour."),
        "{heard}"
    );
    assert!(!heard.contains("zone"), "{heard}");
}

/// A careful driver on their own: holds the street's number, takes each
/// corner at its advise speed, and stops at the gate.
fn drive_to_the_gate(harness: &mut PlaytestHarness) -> String {
    for _ in 0..(60 * 60 * 6) {
        if !harness.has_drive() || harness.read_drive(|d| d.trip.finished) {
            break;
        }
        let (brake, go) = harness.with_drive(|d, _| {
            let speed = d.truck().speed_mph();
            let posted = d.trip.speed_limit_at(d.trip.position_mi).0;
            let turn = d.turn_cue_in_play();
            let target = match turn.as_ref() {
                Some(cue) if cue.at_mi - d.trip.position_mi < 0.12 => {
                    posted.min(d.turn_speed_mph(cue) - 1.0)
                }
                _ => posted,
            };
            let left = d.trip.remaining_miles();
            let target = if left < 0.05 { 5.0 } else { target };
            (speed > target + 1.0, speed < target - 2.0)
        });
        let mut keys = Vec::new();
        if brake {
            keys.push(Key::Down);
        } else if go {
            keys.push(Key::Up);
        }
        hold(harness, &keys);
        frame(harness, DT);
    }
    release_keys(harness);
    harness.transcript_text()
}

// -- the lights and signs -----------------------------------------------------------

#[test]
fn test_the_assists_stop_for_a_red_street_light_hold_it_and_drive_on_at_green() {
    let mut harness = on_the_streets(
        "Street Red",
        vec![control(0.6, "signal")],
        Vec::new(),
        true,
        0.1,
        30.0,
    );
    let bar_mi = 0.6 - STREET_BAR_BEFORE_NODE_MI;
    // Red from the moment it is named until the truck is stopped at it.
    let mut stopped = false;
    for _ in 0..(60 * 120) {
        harness.with_drive(|d, _| {
            if d.street_bar_mi.is_some() && !d.ramp_waiting_at_light && !d.ramp_terminal_done {
                pin_light(d, true);
            }
        });
        frame(&mut harness, DT);
        if harness.read_drive(|d| d.ramp_waiting_at_light) {
            stopped = true;
            break;
        }
    }
    let heard = harness.transcript_text();
    assert!(stopped, "never held at the red\n{heard}");
    // Named in whatever phase it was in; pinned red from the next frame on.
    assert!(heard.contains("Traffic light ahead. Light"), "{heard}");
    assert!(heard.contains("Braking for the light."), "{heard}");
    assert!(heard.contains("Red light."), "{heard}");
    let at = harness.read_drive(|d| d.trip.position_mi);
    assert!(
        at <= bar_mi + 0.005 && bar_mi - at < 0.03,
        "stopped at {at}, bar {bar_mi}"
    );
    // Green: the keeper drives on from the bar with nobody on the pedals.
    harness.with_drive(|d, _| pin_light(d, false));
    run_until(&mut harness, 60 * 60, |d| d.trip.position_mi > 0.7);
    let heard = harness.transcript_text();
    assert!(
        harness.read_drive(|d| d.trip.position_mi) > 0.7,
        "never drove on\n{heard}"
    );
    assert!(heard.contains("Light green."), "{heard}");
    assert!(harness.read_drive(|d| d.keeper_mph.is_some()), "{heard}");
    for bad in [
        "ran the red",
        "far too fast",
        "at the ramp end",
        "Stop at the entrance",
    ] {
        assert!(!heard.contains(bad), "heard {bad:?}\n{heard}");
    }
}

#[test]
fn test_a_green_street_light_is_driven_at_the_streets_own_speed() {
    // Mid-block, well clear of the corner the keeper eases for.
    let mut harness = on_the_streets(
        "Street Green",
        vec![control(0.3, "signal")],
        Vec::new(),
        true,
        0.02,
        30.0,
    );
    let mut slowest = f64::MAX;
    for _ in 0..(60 * 60) {
        harness.with_drive(|d, _| {
            if d.street_bar_mi.is_some() && !d.ramp_terminal_done {
                pin_light(d, false);
            }
        });
        frame(&mut harness, DT);
        let (at, speed) = harness.read_drive(|d| (d.trip.position_mi, d.truck().speed_mph()));
        if at > 0.15 && at < 0.32 {
            slowest = slowest.min(speed);
        }
        if at > 0.35 {
            break;
        }
    }
    let heard = harness.transcript_text();
    assert!(heard.contains("Traffic light ahead."), "{heard}");
    assert!(
        slowest > 25.0,
        "slowed to {slowest:.1} for a green\n{heard}"
    );
    for bad in ["Slowing for", "Green light. Through", "far too fast"] {
        assert!(!heard.contains(bad), "heard {bad:?}\n{heard}");
    }
}

#[test]
fn test_a_red_street_light_run_is_not_placed_at_a_ramp() {
    let mut harness = on_the_streets(
        "Street Run",
        vec![control(0.6, "signal")],
        Vec::new(),
        false,
        0.45,
        30.0,
    );
    hold(&mut harness, &[Key::Up]);
    for _ in 0..(60 * 30) {
        harness.with_drive(|d, _| {
            if d.street_bar_mi.is_some() && !d.ramp_terminal_done {
                pin_light(d, true);
            }
        });
        frame(&mut harness, DT);
        if harness.read_drive(|d| d.trip.position_mi > 0.65) {
            break;
        }
    }
    release_keys(&mut harness);
    let heard = harness.transcript_text();
    assert!(heard.contains("You ran the red light"), "{heard}");
    assert!(!heard.contains("at the ramp end"), "{heard}");
}

#[test]
fn test_the_assists_stop_at_a_street_stop_sign_and_drive_on_from_it() {
    let mut harness = on_the_streets(
        "Street Stop",
        Vec::new(),
        vec![control(0.4, "stop")],
        true,
        0.9,
        30.0,
    );
    let node = 0.8 + 0.4;
    // An empty crossroad, so the gap is there as soon as the stop is made.
    let mut stopped_at_sign = false;
    for _ in 0..(60 * 180) {
        harness.with_drive(|d, _| {
            if let Some(bubble) = d.cross_bubble.as_mut() {
                bubble.vehicles.clear();
            }
        });
        frame(&mut harness, DT);
        if harness.read_drive(|d| {
            (node - d.trip.position_mi).abs() < 0.03 && d.truck().speed_mph() <= RED_STOP_MPH
        }) {
            stopped_at_sign = true;
        }
        if harness.read_drive(|d| d.trip.position_mi > node + 0.05) {
            break;
        }
    }
    let heard = harness.transcript_text();
    assert!(stopped_at_sign, "never stopped at the sign\n{heard}");
    assert!(heard.contains("Stop sign ahead."), "{heard}");
    assert!(heard.contains("Stopped at the sign."), "{heard}");
    assert!(!heard.contains("Speed keeper pulling ahead."), "{heard}");
    assert!(
        harness.read_drive(|d| d.trip.position_mi) > node + 0.05,
        "never drove on\n{heard}"
    );
    for bad in [
        "blew the stop sign",
        "rolled the stop sign",
        "pull ahead onto the streets",
    ] {
        assert!(!heard.contains(bad), "heard {bad:?}\n{heard}");
    }
}

#[test]
fn test_an_all_way_stop_waits_for_no_cross_traffic() {
    let mut harness = on_the_streets(
        "All Way",
        vec![control(0.6, "all_way_stop")],
        Vec::new(),
        true,
        0.3,
        30.0,
    );
    run_until(&mut harness, 60 * 60, |d| d.street_bar_mi.is_some());
    assert!(harness.read_drive(|d| d.street_control_kind == "all_way_stop"));
    assert!(harness.read_drive(|d| d.ramp_control == "stop"));
    assert!(harness.read_drive(|d| d.cross_bubble.is_none()));
    run_until(&mut harness, 60 * 120, |d| d.trip.position_mi > 0.65);
    let heard = harness.transcript_text();
    assert!(heard.contains("All-way stop ahead."), "{heard}");
    assert!(heard.contains("Stopped at the sign."), "{heard}");
    assert!(!heard.contains("wait for your gap"), "{heard}");
}

#[test]
fn test_an_intersection_with_no_mapped_control_has_none() {
    // Unknown is not free, and it is not a stop sign either: the dice never
    // stand in for OSM on a street.
    let mut harness = on_the_streets("No Control", Vec::new(), Vec::new(), true, 0.1, 30.0);
    run_until(&mut harness, 60 * 60 * 4, |d| d.trip.finished);
    let heard = harness.transcript_text();
    for bad in [
        "Traffic light",
        "Stop sign",
        "Yield sign",
        "All-way stop",
        "Stopped at",
    ] {
        assert!(!heard.contains(bad), "heard {bad:?}\n{heard}");
    }
    assert!(harness.read_drive(|d| d.street_bar_mi.is_none()));
}

#[test]
fn test_a_1_9_drive_plays_no_street_controls() {
    // Owner decision, 2026-09-24: the streets' lights and signs are off for
    // 1.9 and the streets drive as they did before them. A drive starts with
    // them off, and mapped ones then say and stop for nothing.
    use freight_fate::states::driving_events::street_controls::STREET_CONTROLS_IN_PLAY;
    assert!(!start_drive("Lights Off").read_drive(|d| d.street_controls_on));
    let mut harness = on_the_streets(
        "Lights Off",
        vec![control(0.3, "signal"), control(0.6, "all_way_stop")],
        vec![control(0.3, "stop"), control(0.5, "give_way")],
        true,
        0.1,
        30.0,
    );
    harness.with_drive(|d, _| d.street_controls_on = STREET_CONTROLS_IN_PLAY);
    let mut bar_seen = false;
    run_until(&mut harness, 60 * 60 * 4, |d| {
        bar_seen |= d.street_bar_mi.is_some();
        d.trip.finished
    });
    assert!(!bar_seen, "a street control went on the bar");
    let heard = harness.transcript_text();
    for bad in [
        "Traffic light",
        "Stop sign",
        "Yield sign",
        "All-way stop",
        "Stopped at",
    ] {
        assert!(!heard.contains(bad), "heard {bad:?}\n{heard}");
    }
}

#[test]
fn test_the_ramp_terminals_own_corner_is_not_played_again() {
    // A chain starts at the node its ramp ends on; a control baked there is
    // the terminal the ramp already played.
    let mut harness = on_the_streets(
        "Terminal Corner",
        vec![control(0.0, "signal")],
        Vec::new(),
        true,
        0.0,
        15.0,
    );
    for _ in 0..(60 * 20) {
        frame(&mut harness, DT);
    }
    assert!(harness.read_drive(|d| d.street_bar_mi.is_none()));
    assert!(!harness.transcript_text().contains("Traffic light"));
}

// -- coordinated street signals -------------------------------------------------------

/// A 35 mph arterial three miles long with ten signals a quarter mile apart,
/// driven at `mph` with route-transition assistance making the stops, from
/// trip seed `seed`. Returns how many reds the truck stopped at.
fn reds_on_the_arterial(seed: i64, mph: f64) -> usize {
    let mut harness = start_drive("Arterial");
    release_keys(&mut harness);
    {
        let s = &mut harness.app.ctx.settings;
        s.time_scale = 1.0;
        s.automatic_transmission = true;
        s.automatic_emergency_braking = false;
        s.route_transition_assist = true;
        s.destination_approach_assist = false;
        s.speed_keeper = false;
        s.lane_keeping = "full".to_string();
    }
    harness.with_drive(move |d, _| {
        let city = d.trip.route.cities[0].clone();
        let signals = (0..10)
            .map(|i| control(0.3 + 0.25 * i as f64, "signal"))
            .collect();
        let legs = vec![Leg::local(
            &city,
            3.0,
            "South 1st Street",
            "Start on South 1st Street.",
            35.0,
        )
        .with_street(limit(35.0, "read"), signals)];
        let route = Route::from_legs(vec![city.clone(), city], legs);
        let truck = d.trip.truck.clone();
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
        quiet(&mut trip);
        d.trip = trip;
        d.reset_turn_state_for_trip();
        d.destination_exit_taken = true;
        d.trip_seed = seed;
        d.street_controls_on = true;
        d.tutorial = None;
        d.truck_mut().start_engine();
        d.truck_mut().transmission.automatic = true;
        d.truck_mut().transmission.gear = 7;
        d.truck_mut().set_air_ready(false);
        d.truck_mut().velocity_mps = mph * MPS_PER_MPH;
        d.trip.position_mi = 0.02;
    });
    let mut reds = 0;
    let mut was_waiting = false;
    for _ in 0..(60 * 60 * 15) {
        let (at, waiting, owned, speed, braking) = harness.read_drive(|d| {
            (
                d.trip.position_mi,
                d.ramp_waiting_at_light,
                d.ramp_assist_said && !d.ramp_terminal_done,
                d.truck().speed_mph(),
                d.truck().brake > 0.01,
            )
        });
        if at > 2.9 {
            break;
        }
        if waiting && !was_waiting {
            reds += 1;
        }
        was_waiting = waiting;
        // The driver holds `mph` and leaves a stop to the assist.
        let go = !waiting && !owned && !braking && speed < mph - 1.0;
        hold(&mut harness, if go { &[Key::Up] } else { &[] });
        harness.with_drive(|d, _| {
            let cut_out = d.truck().specs.air_governor_cut_out_psi;
            d.truck_mut().set_air_pressure_psi(cut_out);
        });
        frame(&mut harness, DT);
    }
    release_keys(&mut harness);
    assert!(
        harness.read_drive(|d| d.trip.position_mi) > 2.9,
        "never drove the arterial\n{}",
        harness.transcript_text()
    );
    reds
}

#[test]
fn test_a_truck_at_the_limit_rides_the_green_band_down_an_arterial() {
    // Arterial signals are coordinated: offset so a truck at the posted limit
    // arrives on green. The band is not a promise -- the first light is met
    // wherever its cycle is, and a stop drops the truck out of step -- but it
    // is at most a stop or two in ten signals.
    let seeds = [11, 12, 13, 14, 15];
    let at_limit: Vec<usize> = seeds
        .iter()
        .map(|s| reds_on_the_arterial(*s, 35.0))
        .collect();
    // At half the limit the truck falls out of the band and meets several.
    let crawling: Vec<usize> = seeds
        .iter()
        .map(|s| reds_on_the_arterial(*s, 17.5))
        .collect();
    eprintln!("reds at the limit {at_limit:?}, at half of it {crawling:?}");
    assert!(at_limit.iter().all(|reds| *reds <= 2), "{at_limit:?}");
    let total: usize = crawling.iter().sum();
    assert!(
        total >= 3 * seeds.len() && total > at_limit.iter().sum::<usize>() * 2,
        "at the limit {at_limit:?}, crawling {crawling:?}"
    );
}

#[test]
fn test_a_turn_at_a_signal_gets_the_side_streets_split() {
    use freight_fate::states::driving_events::street_controls::{
        STREET_MAJOR_GREEN_S, STREET_MINOR_GREEN_S,
    };
    // The left onto North Michigan Street is made at a signal: there the
    // truck is the side street's movement. The light further down North
    // Michigan is the through movement's.
    let mut harness = on_the_streets(
        "Turn Split",
        Vec::new(),
        vec![control(0.0, "signal"), control(0.5, "signal")],
        false,
        0.6,
        20.0,
    );
    run_until(&mut harness, 60 * 60, |d| d.street_bar_mi.is_some());
    assert_eq!(
        harness.read_drive(|d| d.street_light_split.map(|(_, green)| green)),
        Some(STREET_MINOR_GREEN_S)
    );
    harness.with_drive(|d, _| {
        d.trip.position_mi = 1.1;
        d.street_bar_mi = None;
        d.ramp_terminal_done = true;
    });
    run_until(&mut harness, 60 * 60, |d| d.street_bar_mi.is_some());
    assert_eq!(
        harness.read_drive(|d| d.street_light_split.map(|(_, green)| green)),
        Some(STREET_MAJOR_GREEN_S)
    );
}

// -- the speed keeper between close corners ------------------------------------------

#[test]
fn test_the_keeper_holds_the_next_corners_number_between_close_corners() {
    // September 23, into Abilene: between corners a quarter mile apart the
    // keeper built back to the street's limit and then braked for the second
    // corner at about 0.3 g in its last 0.07 mile. With no room to build and
    // shed, it holds the next corner's number instead.
    let mut harness = start_drive("Close Corners");
    release_keys(&mut harness);
    harness.app.ctx.settings.time_scale = 1.0;
    harness.app.ctx.settings.speed_keeper = true;
    harness.app.ctx.settings.automatic_transmission = true;
    let (first, second) = harness.with_drive(|d, _| {
        let city = d.trip.route.cities[0].clone();
        let legs = vec![
            Leg::local(
                &city,
                0.6,
                "East Navarre Street",
                "Start on East Navarre Street.",
                30.0,
            )
            .with_street(limit(30.0, "statutory"), Vec::new()),
            Leg::local(
                &city,
                0.25,
                "North Michigan Street",
                "Turn left onto North Michigan Street.",
                30.0,
            )
            .with_turn_deg(90.0)
            .with_street(limit(30.0, "statutory"), Vec::new()),
            Leg::local(
                &city,
                0.6,
                "West Sample Street",
                "Turn right onto West Sample Street.",
                30.0,
            )
            .with_turn_deg(90.0)
            .with_street(limit(30.0, "statutory"), Vec::new()),
        ];
        let route = Route::from_legs(vec![city; 4], legs);
        let truck = d.trip.truck.clone();
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
        quiet(&mut trip);
        d.trip = trip;
        d.reset_turn_state_for_trip();
        d.destination_exit_taken = true;
        d.truck_mut().start_engine();
        d.truck_mut().transmission.automatic = true;
        d.truck_mut().transmission.gear = 6;
        d.truck_mut().set_air_ready(false);
        d.truck_mut().velocity_mps = 30.0 * MPS_PER_MPH;
        d.trip.position_mi = 0.3;
        let cues = turn_cues(d);
        (cues[0].clone(), cues[1].clone())
    });
    press(&mut harness, Key::K, None);
    let next = harness.read_drive(|d| d.turn_speed_mph(&second));
    let mut peak: f64 = 0.0;
    let mut worst_decel: f64 = 0.0;
    let mut last_speed = None;
    for _ in 0..(60 * 60 * 3) {
        frame(&mut harness, DT);
        let (at, speed, mps) = harness.read_drive(|d| {
            (
                d.trip.position_mi,
                d.truck().speed_mph(),
                d.truck().velocity_mps,
            )
        });
        if at > first.at_mi && at < second.at_mi {
            peak = peak.max(speed);
        }
        if at > second.at_mi - 0.07 && at < second.at_mi {
            if let Some(previous) = last_speed {
                worst_decel = worst_decel.max((previous - mps) / DT);
            }
        }
        last_speed = Some(mps);
        if at > second.at_mi {
            break;
        }
    }
    // Never built up past the slower of the corner it just took and the
    // next one: a left taken at its own number may be faster than the right
    // after it, and that speed is shed, not added.
    let first_speed = harness.read_drive(|d| d.turn_speed_mph(&first));
    let allowed = next.max(first_speed);
    assert!(
        peak <= allowed + 0.5,
        "built to {peak:.1} between corners; the next one advises {next:.1}"
    );
    assert!(
        worst_decel < 0.2 * 9.81,
        "braked at {:.2} g into the second corner",
        worst_decel / 9.81
    );
    assert_eq!(harness.read_drive(|d| d.turn_miss_count), 0);
}

#[test]
fn test_the_keepers_build_rate_is_the_one_its_planner_assumes() {
    // `KEEPER_BUILD_MPS2` prices the build half of the close-corner rule; it
    // is the keeper's own measured pace from a corner back up to a 30 mph
    // street, loaded and empty, never faster than the truck really builds.
    use freight_fate::states::driving_speed_control::KEEPER_BUILD_MPS2;
    for cargo_kg in [0.0, 18_000.0] {
        let mut harness = on_the_streets("Build Rate", Vec::new(), Vec::new(), true, 0.0, 10.0);
        harness.with_drive(move |d, _| {
            // One long straight street: nothing ahead to ease for.
            let city = d.trip.route.cities[0].clone();
            let street = Leg::local(
                &city,
                3.0,
                "East Navarre Street",
                "Start on East Navarre Street.",
                30.0,
            )
            .with_street(limit(30.0, "statutory"), Vec::new());
            let route = Route::from_legs(vec![city.clone(), city], vec![street]);
            let truck = d.trip.truck.clone();
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
            quiet(&mut trip);
            d.trip = trip;
            d.reset_turn_state_for_trip();
            d.truck_mut().cargo_kg = cargo_kg;
        });
        // Holding the street's number, the way it takes a street off a corner.
        harness.with_drive(|d, ctx| {
            d.engage_keeper(ctx, 30.0, "facility access road", Some(30.0), false)
        });
        let mut seconds = 0.0;
        while seconds < 120.0 && harness.read_drive(|d| d.truck().speed_mph()) < 28.0 {
            frame(&mut harness, DT);
            seconds += DT;
        }
        let rate = (28.0 - 10.0) * MPS_PER_MPH / seconds;
        eprintln!("cargo {cargo_kg} kg: 10 to 28 mph in {seconds:.1} s, {rate:.2} m/s2");
        assert!(
            rate >= KEEPER_BUILD_MPS2,
            "the keeper built at {rate:.2} m/s2 with {cargo_kg} kg aboard, slower than the {KEEPER_BUILD_MPS2} planned"
        );
    }
}

// -- the chain starts where the ramp lands --------------------------------------------

#[test]
fn test_the_streets_start_on_the_road_the_destination_ramp_meets() {
    use ff_core::models::jobs::make_reposition_job;
    use ff_core::models::profile::Profile;
    let world = get_world();
    let mut app = TestApp::new();
    let mut profile = Profile::named_in("Terminal", "Dallas");
    profile.tutorial_done = true;
    app.ctx.profile = Some(profile);
    let route = world
        .shortest_route("dallas_tx_us", "abilene_tx_us", None, false)
        .expect("the world routes")
        .expect("Dallas has a route to Abilene");
    let mut job = make_reposition_job(world, "dallas_tx_us", "abilene_tx_us", false, None)
        .expect("a reposition job exists");
    job.destination_location = "Abilene Company Yard".to_string();
    let mut d = DrivingState::new(
        &mut app.ctx,
        job,
        route,
        Some(0),
        DRIVE_PHASE_DELIVERY,
        Some(12.0),
    );
    d.trip.set_npc_vehicles(Vec::new());
    let node = d
        .destination_terminal_node(&app.ctx)
        .expect("the destination exit's ramp ends at a mapped node");
    let exit_chain = world
        .facility_exit_route("abilene_tx_us", "Abilene Company Yard", node)
        .expect("the world answers")
        .expect("a chain was baked from that terminal");
    let default_chain = world
        .facility_approach_route("abilene_tx_us", "Abilene Company Yard")
        .expect("the default chain");
    assert_ne!(
        exit_chain.legs[0].highway, default_chain.legs[0].highway,
        "the case needs an exit whose ramp does not meet the city-centre chain's first street"
    );
    d.destination_exit_taken = true;
    app.clear_speech();
    assert!(d.begin_surface_chain(&mut app.ctx, true));
    let first = d.trip.route.legs[0].highway.clone();
    assert_eq!(first, exit_chain.legs[0].highway);
    let heard = app.event_lines().join("\n");
    assert!(heard.contains(&format!("Start on {first}")), "{heard}");
}

// -- each corner at its own number ---------------------------------------------------

/// A 30 mph street, a left 90 onto a 30 mph street `gap_mi` long, then a right
/// of `second_deg` onto a third. Returns the two corners.
fn left_then_right(
    harness: &mut PlaytestHarness,
    gap_mi: f64,
    second_deg: f64,
) -> (
    ff_core::sim::trip_models::NavigationCue,
    ff_core::sim::trip_models::NavigationCue,
) {
    two_corners(harness, "left", gap_mi, "right", second_deg)
}

/// [`left_then_right`] with the sides given.
fn two_corners(
    harness: &mut PlaytestHarness,
    first_side: &str,
    gap_mi: f64,
    second_side: &str,
    second_deg: f64,
) -> (
    ff_core::sim::trip_models::NavigationCue,
    ff_core::sim::trip_models::NavigationCue,
) {
    harness.with_drive(|d, _| {
        let city = d.trip.route.cities[0].clone();
        let legs = vec![
            Leg::local(
                &city,
                0.6,
                "East Navarre Street",
                "Start on East Navarre Street.",
                30.0,
            )
            .with_street(limit(30.0, "statutory"), Vec::new()),
            Leg::local(
                &city,
                gap_mi,
                "North Michigan Street",
                &format!("Turn {first_side} onto North Michigan Street."),
                30.0,
            )
            .with_turn_deg(90.0)
            .with_street(limit(30.0, "statutory"), Vec::new()),
            Leg::local(
                &city,
                0.6,
                "West Sample Street",
                &format!("Turn {second_side} onto West Sample Street."),
                30.0,
            )
            .with_turn_deg(second_deg)
            .with_street(limit(30.0, "statutory"), Vec::new()),
        ];
        let route = Route::from_legs(vec![city; 4], legs);
        let truck = d.trip.truck.clone();
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
        quiet(&mut trip);
        d.trip = trip;
        d.reset_turn_state_for_trip();
        d.destination_exit_taken = true;
        d.truck_mut().start_engine();
        d.truck_mut().transmission.automatic = true;
        d.truck_mut().transmission.gear = 6;
        d.truck_mut().set_air_ready(false);
        d.truck_mut().velocity_mps = 30.0 * MPS_PER_MPH;
        d.trip.position_mi = 0.3;
        let cues = turn_cues(d);
        (cues[0].clone(), cues[1].clone())
    })
}

/// Drive on the keeper alone until past `last_mi`; the speed as the truck
/// crossed each of `marks`.
fn speeds_at(harness: &mut PlaytestHarness, marks: &[f64], last_mi: f64) -> Vec<f64> {
    let mut out = vec![f64::NAN; marks.len()];
    let mut prev = harness.read_drive(|d| d.trip.position_mi);
    for _ in 0..(60 * 60 * 4) {
        harness.with_drive(|d, _| {
            let cut_out = d.truck().specs.air_governor_cut_out_psi;
            d.truck_mut().set_air_pressure_psi(cut_out);
        });
        frame(harness, DT);
        let (at, speed) = harness.read_drive(|d| (d.trip.position_mi, d.truck().speed_mph()));
        for (i, mark) in marks.iter().enumerate() {
            if prev < *mark && at >= *mark {
                out[i] = speed;
            }
        }
        prev = at;
        if at > last_mi {
            break;
        }
    }
    out
}

fn keeper_drive(name: &str) -> PlaytestHarness {
    let mut harness = start_drive(name);
    release_keys(&mut harness);
    harness.app.ctx.settings.time_scale = 1.0;
    harness.app.ctx.settings.speed_keeper = true;
    harness.app.ctx.settings.automatic_transmission = true;
    harness
}

#[test]
fn test_a_left_90_runs_its_own_number_with_a_far_right_90_after_it() {
    let mut harness = keeper_drive("Own Number");
    let (left, right) = left_then_right(&mut harness, 0.6, 90.0);
    press(&mut harness, Key::K, None);
    let (left_target, right_target) =
        harness.read_drive(|d| (d.turn_speed_mph(&left), d.turn_speed_mph(&right)));
    assert!(
        left_target >= 13.0 && right_target <= 11.0,
        "{left_target} {right_target}"
    );
    let speeds = speeds_at(&mut harness, &[left.at_mi, right.at_mi], right.at_mi + 0.01);
    println!(
        "left target {left_target:.1} actual {:.1}; right target {right_target:.1} actual {:.1}",
        speeds[0], speeds[1]
    );
    assert!(
        speeds[0] > right_target + 1.5 && speeds[0] <= left_target + 1.0,
        "took the left at {:.1}; its own number is {left_target:.1}, the far right's {right_target:.1}",
        speeds[0]
    );
    assert!(
        speeds[1] <= right_target + 1.0,
        "took the right at {:.1}",
        speeds[1]
    );
    assert_eq!(harness.read_drive(|d| d.turn_miss_count), 0);
}

#[test]
fn test_a_slower_corner_too_close_to_brake_for_is_honored_at_the_first() {
    let mut harness = keeper_drive("Close Slower");
    let (left, right) = left_then_right(&mut harness, 0.01, 130.0);
    press(&mut harness, Key::K, None);
    let (left_own, left_eff, right_target) = harness.with_drive(|d, _| {
        let targets = d.corner_targets_ahead();
        (
            d.turn_speed_mph(&left),
            targets[0].1,
            d.turn_speed_mph(&right),
        )
    });
    assert!(left_eff < left_own - 1.0, "{left_eff} vs {left_own}");
    let speeds = speeds_at(&mut harness, &[left.at_mi, right.at_mi], right.at_mi + 0.01);
    println!("left own {left_own:.1} held {left_eff:.1} actual {:.1}; right target {right_target:.1} actual {:.1}", speeds[0], speeds[1]);
    assert!(
        speeds[0] <= left_eff + 1.0,
        "took the left at {:.1}",
        speeds[0]
    );
    assert!(
        speeds[1] <= right_target + 1.0,
        "took the right at {:.1}",
        speeds[1]
    );
    assert_eq!(harness.read_drive(|d| d.turn_miss_count), 0);
}

#[test]
fn test_a_same_road_bend_keeps_the_posted_limit_and_a_slight_turn_is_capped() {
    for (name, expect_posted) in [("Old US-20", true), ("McKinley Highway", false)] {
        let mut harness = keeper_drive("Bend");
        let cue = harness.with_drive(|d, _| {
            let city = d.trip.route.cities[0].clone();
            let legs = vec![
                Leg::local(&city, 0.6, "Old US-20", "Start on Old US-20.", 45.0)
                    .with_street(limit(45.0, "read"), Vec::new()),
                Leg::local(&city, 0.6, name, &format!("Turn left onto {name}."), 45.0)
                    .with_turn_deg(45.0)
                    .with_street(limit(45.0, "read"), Vec::new()),
                Leg::local(
                    &city,
                    0.6,
                    "West Sample Street",
                    "Turn right onto West Sample Street.",
                    30.0,
                )
                .with_turn_deg(90.0)
                .with_street(limit(30.0, "statutory"), Vec::new()),
            ];
            let route = Route::from_legs(vec![city; 4], legs);
            let truck = d.trip.truck.clone();
            let weather = WeatherSystem::new("heartland", Some(3), None, None, true);
            let mut trip = Trip::new(
                route,
                truck,
                weather,
                TripOptions {
                    seed: Some(3),
                    ..Default::default()
                },
            );
            quiet(&mut trip);
            d.trip = trip;
            d.reset_turn_state_for_trip();
            turn_cues(d).into_iter().find(|c| c.key == "local:turn:1")
        });
        if expect_posted {
            // No corner at all: the same road carries on, so nothing slows
            // the truck under the street's 45 for it.
            assert!(
                cue.is_none(),
                "a bend in the same road is not a turn: {cue:?}"
            );
            let planned = harness.with_drive(|d, ctx| {
                d.trip.position_mi = 0.55;
                d.trip.truck.velocity_mps = 45.0 * MPS_PER_MPH;
                d.keeper_speed_ahead(ctx)
            });
            assert!(
                planned
                    .as_ref()
                    .is_none_or(|p| p.1 != "turn" || p.0 >= 45.0),
                "slowed for the bend: {planned:?}"
            );
        } else {
            let cue = cue.expect("a slight turn onto another road is a corner");
            let speed = harness.read_drive(|d| d.turn_speed_mph(&cue));
            assert_eq!(speed, ff_core::data::corners::BEND_CAP_MPH);
        }
    }
}

#[test]
fn test_a_left_600_ft_after_a_right_runs_its_own_number() {
    // The right's hold runs 792 ft past it; a left 600 ft on must still get
    // its own, higher number rather than coast under the right's.
    let mut harness = keeper_drive("Right Then Left");
    let (right, left) = two_corners(&mut harness, "right", 600.0 / 5280.0, "left", 90.0);
    press(&mut harness, Key::K, None);
    let (right_target, left_target) =
        harness.read_drive(|d| (d.turn_speed_mph(&right), d.turn_speed_mph(&left)));
    let speeds = speeds_at(&mut harness, &[right.at_mi, left.at_mi], left.at_mi + 0.01);
    println!(
        "right target {right_target:.1} actual {:.1}; left target {left_target:.1} actual {:.1}",
        speeds[0], speeds[1]
    );
    assert!(
        speeds[0] <= right_target + 1.0,
        "took the right at {:.1}",
        speeds[0]
    );
    assert!(
        (speeds[1] - left_target).abs() <= 1.0,
        "took the left at {:.1} against its {left_target:.1}",
        speeds[1]
    );
    assert_eq!(harness.read_drive(|d| d.turn_miss_count), 0);
}
