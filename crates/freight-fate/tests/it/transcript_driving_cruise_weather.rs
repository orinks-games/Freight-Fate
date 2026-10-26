//! Cruise control and the speed keeper (port of the first section of
//! `tests/test_driving_cruise_weather.py`, lines 22-928).
//!
//! The Python file is 4,023 lines and 104 cases, well over the ~1,000-line
//! ceiling in `CLAUDE.md`, so it is split by section. Every Python test
//! function name survives the split, one file per section:
//!
//! | file | Python lines | section |
//! |---|---|---|
//! | `transcript_driving_cruise_weather.rs` | 22-928 | cruise control, the speed keeper |
//! | `..._acc.rs` | 928-1817 | hazard cancellation, adaptive cruise |
//! | `..._hazards.rs` | 1818-2426 | hazard reaction windows, descent control |
//! | `..._realweather.rs` | 2427-2982 | real weather, the tablet, the overspeed alert |
//! | `..._grades.rs` | 2983-3458 | holding a set speed on a grade, predictive cruise |
//! | `..._retarder.rs` | 3460-4023 | the retarder, the keeper's air, the gap, the weather cap |
//!
//! This file holds the cruise-control and speed-keeper section.
//!
//! `transcript_cruise_support` documents what replaced each monkeypatch.

use ff_core::data::corners::corner_speed_mph;
use ff_core::sim::trip_models::{NPCVehicle, Zone};
use ff_core::sim::vehicle::{HIGH_IDLE_DEFAULT_RPM, HIGH_IDLE_STEP_RPM};
use freight_fate::states::base::Key;
use freight_fate::states::driving_core::{CRUISE_MAX_MPH, CRUISE_STEP_MPH, MPH_PER_MPS};
use freight_fate::states::driving_events::pending::KEEPER_SNUB_OVER_MPH;
use freight_fate::states::driving_speed_control::{
    KEEPER_EASE_DECEL_MPS2, KEEPER_EASE_REAL_S, KEEPER_SETTLE_REAL_S,
};

use crate::transcript_cruise_support::*;

/// `driving.trip.speed_limit_at = lambda mile: (limit, reason)`.
///
/// A zone is how `speed_limit_at` returns a reason at all, so one covering
/// the whole road is the same road the patch described.
fn post_zone(
    harness: &mut freight_fate::playtest::harness::PlaytestHarness,
    limit: f64,
    reason: &str,
) {
    let reason = reason.to_string();
    harness.with_drive(move |d, _| {
        d.trip.zones = vec![Zone::new(0.0, 1e6, limit, &reason)];
    });
}

// -- cruise control -------------------------------------------------------------

#[test]
fn test_cruise_control_holds_the_set_speed() {
    let mut harness = bench_drive("Cruise Hold", 200.0, 0.0);
    release_keys(&mut harness);
    press(&mut harness, Key::E, None); // engine on
    harness.with_drive(|d, _| {
        d.truck_mut().cargo_kg = 0.0;
        d.truck_mut().grade = 0.0;
        d.truck_mut().transmission.gear = 10;
        d.truck_mut().velocity_mps = 26.8; // ~60 mph
        d.truck_mut().throttle = 0.35;
    });
    press(&mut harness, Key::K, None);
    assert!(approx_abs(
        harness
            .read_drive(|d| d.cruise_mph)
            .expect("cruise engaged"),
        60.0,
        1.0
    ));
    frames(&mut harness, 60 * 15, DT); // 15 seconds, no keys held
    assert!(harness.read_drive(|d| d.cruise_mph).is_some());
    assert!((harness.read_drive(|d| d.truck().speed_mph()) - 60.0).abs() < 5.0);
}

#[test]
fn test_shift_k_resumes_the_braked_away_cruise_speed() {
    let mut harness = bench_drive("Cruise Resume", 200.0, 0.0);
    release_keys(&mut harness);
    press(&mut harness, Key::E, None);
    harness.with_drive(|d, _| {
        d.truck_mut().transmission.gear = 10;
        d.truck_mut().velocity_mps = 26.8; // ~60 mph
    });
    press(&mut harness, Key::K, None);
    assert!(approx_abs(
        harness
            .read_drive(|d| d.cruise_mph)
            .expect("cruise engaged"),
        60.0,
        1.0
    ));
    let set_speed = harness
        .read_drive(|d| d.speed_control_target_mph)
        .expect("a target");

    // The player brakes: the session cancels but the speed is remembered.
    harness.with_drive(|d, ctx| d.cancel_cruise(ctx, false));
    assert!(harness.read_drive(|d| d.cruise_mph).is_none());
    assert!(approx_abs(
        harness
            .read_drive(|d| d.resume_target_mph)
            .expect("remembered"),
        set_speed,
        1.0
    ));

    // Shift+K re-arms at the remembered target; the per-frame helper engages
    // as soon as the truck is rolling and off the brakes.
    harness.with_drive(|d, _| d.truck_mut().velocity_mps = 22.0); // slowed, still rolling
    press_shift(&mut harness, Key::K);
    assert!(harness.read_drive(|d| d.speed_control_armed));
    assert!(approx_abs(
        harness
            .read_drive(|d| d.speed_control_target_mph)
            .expect("a target"),
        set_speed,
        1.0
    ));
    assert!(said_any(&harness, "Resuming automatic speed control"));
    frame(&mut harness, DT);
    assert!(approx_abs(
        harness
            .read_drive(|d| d.cruise_mph)
            .expect("cruise engaged"),
        set_speed,
        1.0
    ));
}

#[test]
fn test_parked_cruise_button_latches_high_idle() {
    let mut harness = bench_drive("High Idle", 200.0, 0.0);
    release_keys(&mut harness);
    harness.clear_speech();
    harness.with_drive(|d, _| {
        d.truck_mut().set_air_ready(true);
        d.truck_mut().start_engine();
        d.truck_mut().velocity_mps = 0.0;
    });

    press(&mut harness, Key::K, None); // parked: fast-idle switch
    assert_eq!(
        harness.read_drive(|d| d.truck().high_idle_rpm),
        Some(HIGH_IDLE_DEFAULT_RPM)
    );
    assert!(harness.read_drive(|d| d.cruise_mph).is_none()); // not a cruise session
    assert!(said_any(&harness, "High idle"));

    press(&mut harness, Key::KpPlus, None);
    assert_eq!(
        harness.read_drive(|d| d.truck().high_idle_rpm),
        Some(HIGH_IDLE_DEFAULT_RPM + HIGH_IDLE_STEP_RPM)
    );
    press(&mut harness, Key::KpMinus, None);
    assert_eq!(
        harness.read_drive(|d| d.truck().high_idle_rpm),
        Some(HIGH_IDLE_DEFAULT_RPM)
    );

    press(&mut harness, Key::K, None); // press again: off
    assert_eq!(harness.read_drive(|d| d.truck().high_idle_rpm), None);
    assert!(said_any(&harness, "High idle off"));

    // Latch it, then release the parking brake: the sim cancels it.
    press(&mut harness, Key::K, None);
    assert!(harness.read_drive(|d| d.truck().high_idle_rpm).is_some());
    harness.with_drive(|d, _| {
        d.truck_mut().release_parking_brake();
    });
    frame(&mut harness, DT);
    assert_eq!(harness.read_drive(|d| d.truck().high_idle_rpm), None);
}

#[test]
fn test_players_brake_press_cancels_cruise() {
    let mut harness = bench_drive("Brake Cancel", 200.0, 0.0);
    release_keys(&mut harness);
    press(&mut harness, Key::E, None);
    harness.with_drive(|d, _| {
        d.truck_mut().cargo_kg = 0.0;
        d.truck_mut().grade = 0.0;
        d.truck_mut().transmission.gear = 10;
        d.truck_mut().velocity_mps = 26.8; // ~60 mph
        d.truck_mut().throttle = 0.35;
    });
    press(&mut harness, Key::K, None);
    assert!(harness.read_drive(|d| d.cruise_mph).is_some());

    // The first tap of the service brake drops cruise, like a real truck.
    hold(&mut harness, &[Key::Down]);
    frame(&mut harness, DT);
    assert!(harness.read_drive(|d| d.cruise_mph).is_none());

    // Releasing the brake must not bring it back.
    release_keys(&mut harness);
    frames(&mut harness, 30, DT);
    assert!(harness.read_drive(|d| d.cruise_mph).is_none());
}

#[test]
fn test_cruise_does_not_rev_engine_when_clutch_is_depressed() {
    // A flat road for the whole run, not just the first frame: the trip
    // re-reads the grade every update, and this test needs cruise to be
    // genuinely holding throttle. On a downgrade it correctly holds none.
    let mut harness = bench_drive("Clutch Cruise", 200.0, 0.0);
    release_keys(&mut harness);
    press(&mut harness, Key::E, None);
    harness.app.ctx.settings.automatic_transmission = false;
    harness.with_drive(|d, _| {
        d.truck_mut().cargo_kg = 0.0;
        d.truck_mut().grade = 0.0;
        d.truck_mut().transmission.automatic = false; // the bug is manual-only
        d.truck_mut().transmission.gear = 10;
        d.truck_mut().velocity_mps = 26.8; // ~60 mph
        d.truck_mut().throttle = 0.35;
    });
    press(&mut harness, Key::K, None);
    // Let cruise settle to its holding throttle with the clutch out.
    frames(&mut harness, 30, DT);
    let held_throttle = harness.read_drive(|d| d.cruise_throttle);
    assert!(held_throttle > 0.05, "{held_throttle}");
    let (rpm, max_rpm) = harness.read_drive(|d| (d.truck().rpm, d.truck().specs.max_rpm));
    assert!(rpm < max_rpm * 0.9, "{rpm} {max_rpm}");

    // Depress the clutch to shift: throttle must cut to idle, not free-rev.
    hold(&mut harness, &[Key::LShift]);
    for _ in 0..30 {
        // ~0.5 s clutch in
        frame(&mut harness, DT);
        assert!(approx(harness.read_drive(|d| d.truck().throttle), 0.0));
    }
    assert!(harness.read_drive(|d| d.cruise_mph).is_some()); // cruise stays engaged
    let (rpm, max_rpm) = harness.read_drive(|d| (d.truck().rpm, d.truck().specs.max_rpm));
    assert!(rpm < max_rpm * 0.6, "{rpm} {max_rpm}"); // engine settled toward idle

    // Release the clutch: cruise ramps the throttle back up toward the hold.
    release_keys(&mut harness);
    frame(&mut harness, DT);
    assert!(harness.read_drive(|d| d.truck().throttle) > 0.0);
    frames(&mut harness, 30, DT);
    assert!(harness.read_drive(|d| d.truck().throttle) > held_throttle * 0.5);
}

#[test]
fn test_cruise_set_point_adjusts_with_plus_and_minus() {
    let mut harness = bench_drive("Cruise Steps", 200.0, 0.0);
    press(&mut harness, Key::E, None);
    harness.with_drive(|d, _| {
        d.truck_mut().transmission.gear = 10;
        d.truck_mut().velocity_mps = 26.8; // ~60 mph
    });
    press(&mut harness, Key::K, None);
    let base = harness
        .read_drive(|d| d.cruise_mph)
        .expect("cruise engaged");
    // `engage_cruise` rounds the captured road speed (~59.95) to the whole mph
    // the player actually hears, so base lands exactly on the fives grid here
    // -- a plain tap steps a full CRUISE_STEP_MPH.
    assert!(approx(base, 60.0), "{base}");

    press(&mut harness, Key::Equals, None); // + raises by a step
    assert!(approx(
        harness.read_drive(|d| d.cruise_mph).expect("cruise"),
        base + CRUISE_STEP_MPH
    ));
    press(&mut harness, Key::Minus, None); // - lowers it back
    assert!(approx(
        harness.read_drive(|d| d.cruise_mph).expect("cruise"),
        base
    ));
    press(&mut harness, Key::Plus, Some('+'));
    assert!(approx(
        harness.read_drive(|d| d.cruise_mph).expect("cruise"),
        base + CRUISE_STEP_MPH
    ));
    press(&mut harness, Key::KpMinus, Some('-'));
    assert!(approx(
        harness.read_drive(|d| d.cruise_mph).expect("cruise"),
        base
    ));

    for _ in 0..20 {
        // clamps at the max
        press(&mut harness, Key::Equals, None);
    }
    assert!(approx(
        harness.read_drive(|d| d.cruise_mph).expect("cruise"),
        CRUISE_MAX_MPH
    ));
}

#[test]
fn test_cruise_refuses_to_engage_in_a_facility_zone() {
    let mut harness = bench_drive("Facility Cruise", 200.0, 0.0);
    // With the speed keeper turned off, the original explanation applies:
    // cruise must not engage on a low-speed facility access road.
    harness.app.ctx.settings.speed_keeper = false;
    post_zone(&mut harness, 25.0, "facility access road");
    harness.clear_speech();
    press(&mut harness, Key::E, None);
    harness.with_drive(|d, _| {
        d.truck_mut().transmission.gear = 4;
        d.truck_mut().velocity_mps = 10.0; // ~22 mph, above the floor
    });
    press(&mut harness, Key::K, None);

    assert!(harness.read_drive(|d| d.cruise_mph).is_none());
    assert!(harness.read_drive(|d| d.keeper_mph).is_none());
    // A street is not a zone: the refusal names city streets.
    assert!(
        spoken(&harness)
            .iter()
            .any(|s| s == "No cruise on city streets."),
        "{:?}",
        spoken(&harness)
    );
}

#[test]
fn test_speed_keeper_holds_through_a_facility_zone() {
    let mut harness = bench_drive("Keeper Zone", 200.0, 0.0);
    release_keys(&mut harness);
    post_zone(&mut harness, 15.0, "facility access road");
    harness.clear_speech();
    press(&mut harness, Key::E, None);
    harness.with_drive(|d, _| {
        d.truck_mut().cargo_kg = 0.0;
        d.truck_mut().grade = 0.0;
        d.truck_mut().transmission.gear = 3;
        d.truck_mut().velocity_mps = 4.5; // ~10 mph, no need to hold the accelerator
    });
    press(&mut harness, Key::K, None);

    assert!(harness.read_drive(|d| d.cruise_mph).is_none());
    assert!(approx_abs(
        harness
            .read_drive(|d| d.keeper_mph)
            .expect("keeper holding"),
        10.0,
        0.5
    ));
    assert!(said_any(&harness, "Speed keeper holding"));
    frames(&mut harness, 60 * 10, DT); // ten seconds, no keys held
    assert!(harness.read_drive(|d| d.keeper_mph).is_some());
    assert!((harness.read_drive(|d| d.truck().speed_mph()) - 10.0).abs() < 4.0);
}

#[test]
fn test_speed_keeper_cancels_on_braking() {
    let mut harness = bench_drive("Keeper Brake", 200.0, 0.0);
    release_keys(&mut harness);
    post_zone(&mut harness, 15.0, "facility access road");
    harness.clear_speech();
    press(&mut harness, Key::E, None);
    harness.with_drive(|d, _| {
        d.truck_mut().transmission.gear = 3;
        d.truck_mut().velocity_mps = 4.5;
    });
    press(&mut harness, Key::K, None);
    assert!(harness.read_drive(|d| d.keeper_mph).is_some());

    hold(&mut harness, &[Key::Down]); // brake
    frame(&mut harness, DT);
    assert!(harness.read_drive(|d| d.keeper_mph).is_none());
    assert!(!harness.read_drive(|d| d.speed_control_armed));
    assert!(said_any(&harness, "Speed keeper off."));

    // The access stretch ends.
    harness.with_drive(|d, _| d.trip.zones.clear());
    release_keys(&mut harness);
    frame(&mut harness, DT);
    // Braking disarmed it; no surprise restart.
    assert!(harness.read_drive(|d| d.cruise_mph).is_none());
}

#[test]
fn test_speed_keeper_switches_to_cruise_on_the_open_road() {
    // Python's mutable `zone` dict: the same road, then the access stretch
    // ends and the posted 55 takes over.
    let mut harness = bench_drive("Keeper To Cruise", 55.0, 0.0);
    release_keys(&mut harness);
    post_zone(&mut harness, 15.0, "facility access road");
    harness.clear_speech();
    press(&mut harness, Key::E, None);
    harness.with_drive(|d, _| {
        d.truck_mut().transmission.gear = 3;
        d.truck_mut().velocity_mps = 4.5;
    });
    press(&mut harness, Key::K, None);
    assert!(harness.read_drive(|d| d.keeper_mph).is_some());

    harness.with_drive(|d, _| d.trip.zones.clear()); // the access stretch ends
    frame(&mut harness, DT);
    assert!(harness.read_drive(|d| d.keeper_mph).is_none());
    assert!(approx(
        harness.read_drive(|d| d.cruise_mph).expect("cruise"),
        55.0
    ));
    assert!(harness.read_drive(|d| d.speed_control_armed));
    assert!(said_any(&harness, "Cruise resuming, 55"));
}

/// `_keeper_on_a_street_chain(app, start_before_mi=...)`: a keeper session
/// holding the street limit, rolling up to the corner.
fn keeper_on_a_street_chain(
    name: &str,
    start_before_mi: f64,
) -> (
    freight_fate::playtest::harness::PlaytestHarness,
    ff_core::sim::trip_models::NavigationCue,
) {
    let mut harness = start_drive(name);
    release_keys(&mut harness);
    harness.app.ctx.settings.time_scale = 1.0;
    let cue = harness.with_drive(|d, _| {
        facility_street_chain(d, 1.0);
        turn_cues(d)
            .first()
            .cloned()
            .expect("a corner on the chain")
    });
    press(&mut harness, Key::E, None);
    let at_mi = cue.at_mi - start_before_mi;
    harness.with_drive(move |d, _| {
        d.truck_mut().cargo_kg = 0.0;
        d.truck_mut().grade = 0.0;
        d.truck_mut().transmission.gear = 5;
        d.truck_mut().velocity_mps = 25.0 * MPS_PER_MPH;
        d.trip.position_mi = at_mi;
        d.truck_mut().set_air_ready(false);
    });
    press(&mut harness, Key::K, None);
    assert!(approx_abs(
        harness
            .read_drive(|d| d.keeper_mph)
            .expect("keeper holding"),
        25.0,
        0.5
    ));
    (harness, cue)
}

#[test]
fn test_speed_keeper_is_under_the_turn_speed_before_the_corner() {
    // The tester report: the keeper held the street's 25 into a corner it
    // could not take, so the corner was taken over its speed and the safe
    // turnaround was charged. It now sheds the speed on the approach.
    let (mut harness, cue) = keeper_on_a_street_chain("Corner Keeper", 0.25);
    let advise = harness.read_drive(|d| d.turn_speed_mph(&cue));
    // An unmeasured corner on this fixture, so the square-corner price.
    let load = harness.read_drive(|d| d.trip.truck.roll_load_fraction());
    assert_eq!(advise, corner_speed_mph(None, load));

    let trace = roll_to(&mut harness, cue.at_mi, 60 * 300);
    // Under the number BEFORE the corner, not arriving at it on the spot: the
    // settling tail is what the tester was missing.
    let under: Vec<f64> = trace
        .iter()
        .filter(|(_, mph)| *mph <= advise)
        .map(|(mile, _)| *mile)
        .collect();
    assert!(
        !under.is_empty(),
        "the keeper never reached the corner speed"
    );
    let first_under = under.iter().cloned().fold(f64::INFINITY, f64::min);
    assert!(cue.at_mi - first_under >= 0.01);
    assert!(harness.read_drive(|d| d.truck().speed_mph()) <= advise);
    // And the corner is made without the loop-back, with the session intact.
    frame(&mut harness, DT);
    assert_eq!(harness.read_drive(|d| d.turn_miss_count), 0);
    assert!(harness.read_drive(|d| d.keeper_mph).is_some());
}

#[test]
fn test_speed_keeper_holds_the_street_limit_until_the_corner_is_close() {
    // The other half of the fix: easing early enough must not mean crawling a
    // whole block. Well outside the ease window there is nothing to slow for.
    let (mut harness, _cue) = keeper_on_a_street_chain("Corner Hold", 0.4);
    assert!(harness
        .with_drive(|d, ctx| d.keeper_speed_ahead(ctx))
        .is_none());
    for _ in 0..(60 * 5) {
        frame(&mut harness, DT);
        assert!(harness.read_drive(|d| d.truck().brake) < 0.02); // below where the brake even sounds
    }
    assert!(approx_abs(
        harness.read_drive(|d| d.truck().speed_mph()),
        25.0,
        1.0
    ));
}

#[test]
fn test_speed_keeper_makes_the_second_corner_of_a_short_block() {
    // The rest of the tester report: turns coming up really quickly. The
    // keeper held the corner it was already easing for through that corner's
    // whole tail, and a city block is shorter than the tail -- so the 15 mph
    // service way one block on was invisible until the truck was on top of it,
    // and the keeper drove into the second corner at the first corner's speed.
    let mut harness = start_drive("Short Block");
    release_keys(&mut harness);
    harness.app.ctx.settings.time_scale = 1.0;
    let (first, second) = harness.with_drive(|d, _| short_block_street_chain(d, 0.08, 1.0));
    press(&mut harness, Key::E, None);
    let at_mi = first.at_mi - 0.25;
    harness.with_drive(move |d, _| {
        d.truck_mut().cargo_kg = 0.0;
        d.truck_mut().grade = 0.0;
        d.truck_mut().transmission.gear = 5;
        d.truck_mut().velocity_mps = 25.0 * MPS_PER_MPH;
        d.trip.position_mi = at_mi;
        d.truck_mut().set_air_ready(false);
    });
    press(&mut harness, Key::K, None);
    assert!(approx_abs(
        harness.read_drive(|d| d.keeper_mph).expect("keeper"),
        25.0,
        0.5
    ));
    // Unmeasured corners on this fixture, so both price as square ones.
    let square = corner_speed_mph(
        None,
        harness.read_drive(|d| d.trip.truck.roll_load_fraction()),
    );
    assert_eq!(harness.read_drive(|d| d.turn_speed_mph(&first)), square);
    assert_eq!(harness.read_drive(|d| d.turn_speed_mph(&second)), square);
    assert!(second.at_mi - first.at_mi < 0.15); // inside the first corner's tail

    roll_to(&mut harness, first.at_mi, 60 * 300);
    assert!(harness.read_drive(|d| d.truck().speed_mph()) <= 20.0); // the first corner, as before
                                                                    // The second corner is the one the old planner could not see: while there
                                                                    // is still road to shed on, the keeper has to be aiming at ITS number
                                                                    // rather than still holding the first corner's.
    roll_to(&mut harness, second.at_mi - 0.03, 60 * 300);
    // The label depends on which source latched first -- the corner's own
    // advise or the service way's posted 15 -- and both are the truth. The
    // number is the behavior under test.
    let ahead = harness.with_drive(|d, ctx| d.keeper_speed_ahead(ctx));
    assert_eq!(ahead.map(|(mph, _)| mph), Some(square));
    let trace = roll_to(&mut harness, second.at_mi, 60 * 300);
    assert!(
        trace.iter().any(|(_, mph)| *mph <= square),
        "the keeper never reached the service road's corner speed"
    );
    assert!(harness.read_drive(|d| d.truck().speed_mph()) <= square);

    frame(&mut harness, DT);
    assert_eq!(harness.read_drive(|d| d.turn_miss_count), 0);
    assert!(harness.read_drive(|d| d.keeper_mph).is_some());
}

#[test]
fn test_speed_keeper_eases_for_a_lower_posted_limit_and_says_so() {
    let mut harness = bench_drive("Keeper Ease", 200.0, 0.0);
    release_keys(&mut harness);
    press(&mut harness, Key::E, None);
    harness.with_drive(|d, _| {
        d.truck_mut().cargo_kg = 0.0;
        d.truck_mut().grade = 0.0;
        d.truck_mut().transmission.gear = 5;
        d.truck_mut().velocity_mps = 25.0 * MPS_PER_MPH;
    });
    // Exactly the window the keeper says it needs for this drop -- that is
    // what the window is a promise about. Placing the sign at 0.9x demanded
    // arrival with 10 percent less road than the physics the window prices,
    // which put the assertion on a knife edge that the drawn route's time
    // scale decided (the 1-in-4 flake).
    let drop_mi = harness
        .read_drive(|d| d.trip.position_mi + d.keeper_ease_mi(15.0, d.trip.effective_time_scale()));
    harness.with_drive(move |d, _| {
        d.trip.zones = vec![
            Zone::new(0.0, drop_mi, 25.0, "facility access road"),
            Zone::new(drop_mi, 1e6, 15.0, "facility access road"),
        ];
    });
    harness.clear_speech();
    press(&mut harness, Key::K, None);
    assert!(approx_abs(
        harness.read_drive(|d| d.keeper_mph).expect("keeper"),
        25.0,
        0.5
    ));

    let mut recent: Vec<f64> = Vec::new();
    for _ in 0..(60 * 30) {
        frame(&mut harness, DT);
        recent.push(harness.read_drive(|d| d.truck().speed_mph()));
        if harness.read_drive(|d| d.trip.position_mi) >= drop_mi {
            break;
        }
    }
    // Down into the eased band by the time the sign is under the wheels, said
    // once for that number rather than once a frame. The keeper holds a number
    // by SNUBBING -- one brake application, held, released a mile under,
    // throttle back, repeat -- so its speed at any single instant rides a
    // designed ripple around the eased target. The promise is the band, not a
    // knife-edge instant: never above the number by more than the snub
    // threshold that polices it, and the ripple's floor at or under the
    // number, which is what proves the shed actually happened. Asserting a
    // bare <= 15.0 at one milepost made the sign a phase detector on that
    // ripple: pass or fail by which half of the snub cycle the sign happened
    // to land on, one run in four (ROADMAP 2026-08-19).
    let lines = spoken(&harness);
    assert!(
        lines
            .iter()
            .any(|e| e == "Posted limit lower; speed keeper easing to 15 miles per hour."),
        "{lines:#?}"
    );
    assert!(harness.read_drive(|d| d.truck().speed_mph()) <= 15.0 + KEEPER_SNUB_OVER_MPH);
    let tail = &recent[recent.len().saturating_sub(90)..];
    assert!(
        tail.iter().cloned().fold(f64::INFINITY, f64::min) <= 15.0,
        "the ripple never dipped to the eased number"
    );
    assert_eq!(
        lines
            .iter()
            .filter(|e| e.contains("speed keeper easing to 15"))
            .count(),
        1
    );
    // The keeper's own line already named the number; it must feed the trip's
    // pre-announce set so the plain arrival "Speed limit reduced to 15" does
    // not repeat it a moment later (owner's live playtest, 2026-08-12, on the
    // plain posted-drop case this hook covers).
    assert!(harness.read_drive(|d| d.trip.limit_drop_preannounced.contains(&15.0)));
}

/// Agent drive A (2026-09-24): "Posted limit lower; speed keeper easing to
/// 15" said again while the truck was building speed from 9. A truck under
/// the number is not being eased, so nothing is said.
#[test]
fn test_speed_keeper_says_nothing_of_easing_when_already_under_the_number() {
    let mut harness = bench_drive("Keeper Under", 200.0, 0.0);
    release_keys(&mut harness);
    press(&mut harness, Key::E, None);
    harness.with_drive(|d, _| {
        d.truck_mut().cargo_kg = 0.0;
        d.truck_mut().grade = 0.0;
        d.truck_mut().transmission.gear = 3;
        d.truck_mut().velocity_mps = 9.0 * MPS_PER_MPH;
    });
    let drop_mi = harness.read_drive(|d| d.trip.position_mi + 0.3);
    harness.with_drive(move |d, _| {
        d.trip.zones = vec![
            Zone::new(0.0, drop_mi, 25.0, "facility access road"),
            Zone::new(drop_mi, 1e6, 15.0, "facility access road"),
        ];
    });
    harness.clear_speech();
    press(&mut harness, Key::K, None);
    // Holding the road's 25 and already aiming at the 15 -- the keeper's held
    // ease target, the state a truck is in when it pulls away again short of
    // the drop.
    harness.with_drive(move |d, _| {
        d.keeper_mph = Some(25.0);
        d.keeper_ease_target = Some((drop_mi, 15.0, "posted limit".to_string()));
    });
    for _ in 0..(60 * 30) {
        frame(&mut harness, DT);
        if harness.read_drive(|d| d.trip.position_mi) >= drop_mi {
            break;
        }
    }
    let lines = spoken(&harness);
    assert!(
        harness.read_drive(|d| d.truck().speed_mph()) <= 15.0 + KEEPER_SNUB_OVER_MPH,
        "{lines:#?}"
    );
    assert!(
        !lines.iter().any(|e| e.contains("speed keeper easing")),
        "{lines:#?}"
    );
}

#[test]
fn test_speed_keeper_takes_the_next_street_up_to_its_posted_number() {
    // The tester report: the keeper "sometimes doesn't hold speeds on access
    // roads". A facility approach zones every street at its own baked number,
    // and the keeper's number was frozen at whatever it engaged with, capped
    // by the limit under the wheels -- so a session started on a 15 mph
    // service way carried that crawl over every 25 mph street after it, for
    // the rest of the chain, while the zone entry announced 25 and nothing on
    // the wheel could raise it.
    let mut harness = bench_drive("Next Street", 200.0, 0.0);
    release_keys(&mut harness);
    post_zone(&mut harness, 15.0, "facility access road");
    press(&mut harness, Key::E, None);
    harness.with_drive(|d, _| {
        d.truck_mut().cargo_kg = 0.0;
        d.truck_mut().grade = 0.0;
        d.truck_mut().transmission.gear = 4;
        d.truck_mut().velocity_mps = 15.0 * MPS_PER_MPH;
        d.truck_mut().set_air_ready(false);
    });
    harness.clear_speech();
    press(&mut harness, Key::K, None);
    assert!(approx_abs(
        harness.read_drive(|d| d.keeper_mph).expect("keeper"),
        15.0,
        0.5
    ));

    // The service way ends and a named street begins.
    post_zone(&mut harness, 25.0, "facility access road");
    frame(&mut harness, DT);
    assert!(approx(
        harness.read_drive(|d| d.keeper_mph).expect("keeper"),
        25.0
    ));
    // An assist that speeds the truck up on its own says the new number: the
    // zone entry announced the law, not what the truck will do.
    // A street is not a zone: the line says the number alone.
    assert!(
        spoken(&harness)
            .iter()
            .any(|e| e == "Speed keeper holding 25 miles per hour."),
        "{:#?}",
        spoken(&harness)
    );
    // Raised once for the street. The zone-entry line lands in the same
    // frame; it queues behind the keeper line rather than purging it and
    // handing it back, so the voice is sent it once. What must not happen is
    // the assist raising it again as the frames run on.
    let said_at_the_street = spoken(&harness)
        .iter()
        .filter(|e| e.contains("Speed keeper holding 25"))
        .count();
    assert_eq!(said_at_the_street, 1, "{:#?}", spoken(&harness));
    frames(&mut harness, 60 * 40, DT);
    let speed = harness.read_drive(|d| d.truck().speed_mph());
    assert!(speed > 21.0, "{speed}");
    // Said once for the street, not once a frame.
    assert_eq!(
        spoken(&harness)
            .iter()
            .filter(|e| e.contains("Speed keeper holding 25"))
            .count(),
        said_at_the_street,
        "{:#?}",
        spoken(&harness)
    );

    // A lower street is still simply obeyed, without re-arming the number or
    // announcing anything: coming down was never the broken direction.
    let before = spoken(&harness).len();
    post_zone(&mut harness, 15.0, "facility access road");
    frame(&mut harness, DT);
    assert!(approx(
        harness.read_drive(|d| d.keeper_mph).expect("keeper"),
        25.0
    )); // the number it was handed
    assert!(!spoken(&harness)[before..]
        .iter()
        .any(|e| e.contains("Speed keeper holding")));
}

#[test]
fn test_speed_keeper_ease_window_buys_only_the_road_the_shed_costs() {
    // The other half of the same report. The window is a budget of real
    // seconds, but it was priced at the speed the truck STARTS from for every
    // one of them -- and the truck is slowing through most of them. On a
    // 25-to-15 drop that bought about 40 percent more road than the shed
    // costs, and since 7ff22b6e the eased number is a held floor, so the
    // surplus is crawled at the low number rather than re-planned.
    let mut harness = start_drive("Ease Window");
    harness.with_drive(|d, _| d.truck_mut().velocity_mps = 25.0 * MPS_PER_MPH);
    let speed = harness.read_drive(|d| d.truck().speed_mph());

    let reaction_mi = (KEEPER_EASE_REAL_S + KEEPER_SETTLE_REAL_S) * speed / 3600.0;
    let window = harness.read_drive(|d| d.keeper_ease_mi(15.0, 1.0));
    let shed_s = (speed - 15.0) / MPH_PER_MPS / KEEPER_EASE_DECEL_MPS2;
    // Exactly what the shed costs: its seconds at the mean of the two speeds,
    // plus the settling tail down at the new number.
    let shed_mi = (shed_s * (speed + 15.0) / 2.0 + KEEPER_SETTLE_REAL_S * 15.0) / 3600.0;
    assert!(shed_mi > reaction_mi); // a drop big enough to be shed-bound
    assert!(approx(window, shed_mi), "{window} {shed_mi}");
    // And strictly less than charging every budgeted second at the speed the
    // truck came in at, which is what it used to claim.
    let entry_sized_mi = (shed_s + KEEPER_SETTLE_REAL_S) * speed / 3600.0;
    assert!(window < entry_sized_mi * 0.9);

    // The reaction budget underneath is untouched. Those seconds are spent
    // before any slowing starts, so they still cost road at today's speed -- a
    // corner-sized drop, and no drop at all, are both as they were.
    assert!(approx(
        harness.read_drive(|d| d.keeper_ease_mi(20.0, 1.0)),
        reaction_mi
    ));
    assert!(approx(
        harness.read_drive(|d| d.keeper_ease_mi(speed + 5.0, 1.0)),
        reaction_mi
    ));
}

#[test]
fn test_speed_keeper_ignores_a_slower_vehicle_miles_up_the_road() {
    // The keeper matched any slower vehicle the traffic bubble could see --
    // two and a half miles of it -- with no test on the gap at all, so a car
    // doing 35 in a 45 work zone put the truck at 35 from the far end of the
    // zone, silently. It now waits until there is a reason to shed for it, and
    // still creeps behind a queue that is genuinely there.
    //
    // Python patched `trip.traffic_context` with a synthetic lead. Here the
    // lead is a real NPC on the road, which is what `traffic_context` reads;
    // it is re-placed each frame so its gap behaves the way the patch's did.
    let mut harness = bench_drive("Far Lead", 200.0, 0.0);
    release_keys(&mut harness);
    post_zone(&mut harness, 45.0, "construction");
    let gap_mi = 2.2;
    let lead_mph = 35.0;
    press(&mut harness, Key::E, None);
    harness.with_drive(|d, _| {
        d.truck_mut().cargo_kg = 0.0;
        d.truck_mut().grade = 0.0;
        d.truck_mut().transmission.gear = 8;
        d.truck_mut().velocity_mps = 45.0 * MPS_PER_MPH;
        d.truck_mut().set_air_ready(false);
    });
    press(&mut harness, Key::K, None);
    assert!(approx_abs(
        harness.read_drive(|d| d.keeper_mph).expect("keeper"),
        45.0,
        0.5
    ));
    // Well outside anything the keeper has a reason to shed for.
    assert!(
        gap_mi > harness.read_drive(|d| d.keeper_ease_mi(lead_mph, d.trip.effective_time_scale()))
    );

    for _ in 0..(60 * 20) {
        harness.with_drive(move |d, _| {
            let at = d.trip.position_mi + gap_mi;
            d.trip.set_npc_vehicles(vec![NPCVehicle::new(
                "lead", at, lead_mph, lead_mph, 0, "slow_car",
            )
            .into()]);
        });
        frame(&mut harness, DT);
        // Matching the lead outright would have parked the truck at 35 within
        // a few seconds and held it there for the whole zone.
        let speed = harness.read_drive(|d| d.truck().speed_mph());
        assert!(speed > 37.0, "{speed}");
    }

    // Right behind it, the queue rule still applies all the way to a stop.
    let stop_at_mi = harness.read_drive(|d| d.trip.position_mi) + 0.02;
    for _ in 0..(60 * 60) {
        harness.with_drive(move |d, _| {
            d.trip.set_npc_vehicles(vec![NPCVehicle::new(
                "lead", stop_at_mi, 0.0, 0.0, 0, "slow_car",
            )
            .into()]);
        });
        frame(&mut harness, DT);
        if harness.read_drive(|d| d.truck().speed_mph()) < 1.0 {
            break;
        }
    }
    let speed = harness.read_drive(|d| d.truck().speed_mph());
    assert!(speed < 1.0, "{speed}");
}

#[test]
fn test_speed_keeper_needs_the_truck_rolling() {
    let mut harness = bench_drive("Keeper Stopped", 200.0, 0.0);
    post_zone(&mut harness, 15.0, "facility access road");
    harness.clear_speech();
    press(&mut harness, Key::E, None);
    harness.with_drive(|d, _| d.truck_mut().velocity_mps = 0.0);
    press(&mut harness, Key::K, None);

    assert!(harness.read_drive(|d| d.keeper_mph).is_none());
    assert!(said_any(
        &harness,
        "needs the engine running and the truck rolling"
    ));
}

#[test]
fn test_cruise_adjust_is_inert_when_cruise_is_off() {
    let mut harness = bench_drive("Inert Adjust", 200.0, 0.0);
    harness.clear_speech();
    press(&mut harness, Key::E, None);
    assert!(harness.read_drive(|d| d.cruise_mph).is_none());
    press(&mut harness, Key::Equals, None);
    assert!(harness.read_drive(|d| d.cruise_mph).is_none()); // nothing to adjust
    assert!(spoken(&harness)
        .iter()
        .any(|s| s.to_lowercase().contains("off")));
}

#[test]
fn test_air_ready_cue_does_not_repeat_on_compressor_cycling() {
    let mut harness = bench_drive("Air Ready", 200.0, 0.0);
    harness.clear_speech();
    harness.with_drive(|d, _| {
        d.truck_mut().parking_brake = true; // cue only fires while set
        let cut_out = d.truck().specs.air_governor_cut_out_psi;
        d.truck_mut().set_air_pressure_psi(cut_out); // charged
        d.air_ready_said = true; // already announced
    });

    // Routine compressor cycling dips below the release threshold (which sits
    // at the cut-in pressure) but stays well above low air. Must not
    // re-announce.
    // Python's three-argument call `(a, b, c)` runs through the compat shim in
    // `_update_air_brake_announcements`, which reads it as
    // `(was_engine_on=t.engine_on, was_ready=a, was_low=b, was_spring=c)`.
    // Rust has no shim, so the four readings are spelled out.
    for _ in 0..3 {
        harness.with_drive(|d, ctx| {
            let engine_on = d.truck().engine_on;
            let cut_in = d.truck().specs.air_governor_cut_in_psi;
            d.truck_mut().set_air_pressure_psi(cut_in - 5.0);
            d.update_air_brake_announcements(ctx, engine_on, true, false, false);
            let cut_out = d.truck().specs.air_governor_cut_out_psi;
            d.truck_mut().set_air_pressure_psi(cut_out);
            d.update_air_brake_announcements(ctx, engine_on, false, false, false);
        });
    }
    assert_eq!(
        spoken(&harness)
            .iter()
            .filter(|e| e.contains("Air ready."))
            .count(),
        0
    );

    // A genuine depletion to low air, then recovery, re-announces exactly once.
    harness.with_drive(|d, ctx| {
        let engine_on = d.truck().engine_on;
        let low = d.truck().specs.air_low_warning_psi;
        d.truck_mut().set_air_pressure_psi(low - 5.0);
        d.update_air_brake_announcements(ctx, engine_on, false, false, false);
        let cut_out = d.truck().specs.air_governor_cut_out_psi;
        d.truck_mut().set_air_pressure_psi(cut_out);
        d.update_air_brake_announcements(ctx, engine_on, false, true, false);
    });
    assert_eq!(
        spoken(&harness)
            .iter()
            .filter(|e| e.contains("Air ready."))
            .count(),
        1
    );
}

#[test]
fn test_automatic_shift_uses_shift_cue_not_brake_air() {
    let mut harness = bench_drive("Auto Shift", 200.0, 0.0);
    release_keys(&mut harness);
    let log = harness.app.record_audio();
    harness.with_drive(|d, _| {
        d.truck_mut().start_engine();
        d.truck_mut().transmission.gear = 3;
        d.truck_mut().velocity_mps = 5.0;
    });

    frame(&mut harness, 0.0);

    // The shift cue is the auto-shift bank when the licensed cuts are
    // installed (volume carries a small per-trigger jitter around 0.65), the
    // classic gear_shift on a clean clone.
    let played = log.borrow().played.clone();
    let shifts: Vec<(String, f64)> = played
        .iter()
        .filter(|(key, _, _)| key == "vehicle/gear_shift" || key.starts_with("vehicle/shift_auto"))
        .map(|(key, volume, _)| (key.clone(), *volume))
        .collect();
    assert!(!shifts.is_empty(), "{played:?}");
    assert!(
        shifts.iter().all(|(_, vol)| (0.5..=0.8).contains(vol)),
        "{shifts:?}"
    );
    assert!(played.iter().all(|(key, _, _)| key != "vehicle/brake_air"));
}
