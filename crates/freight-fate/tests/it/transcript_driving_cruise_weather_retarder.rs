//! The retarder answers grades not corners, the speed keeper's air budget,
//! the driver-selectable following gap and the weather cap (port of
//! `tests/test_driving_cruise_weather.py`, lines 3460-4023).
//!
//! Sixth of the split; see `transcript_driving_cruise_weather.rs` for why the
//! Python file is split and `transcript_cruise_support` for the bench road
//! these cases run on.

use ff_core::settings::ACC_GAP_CHOICES;
use ff_core::sim::enforcement_observe::TAILGATE_GAP_S;
use ff_core::sim::trip_models::Zone;
use ff_core::sim::trip_route_helpers::zone_key;
use ff_core::sim::weather::WeatherKind;
use freight_fate::playtest::harness::PlaytestHarness;
use freight_fate::states::base::Key;

use crate::transcript_cruise_support::*;

// -- the retarder answers grades, not corners -----------------------------------

#[test]
fn test_cruise_slows_for_a_level_bend_on_the_drums_not_the_retarder() {
    // A corner is a target speed, and target speeds belong to the drums.
    //
    // Adaptive cruise capped its working target to the bend's advisory and
    // then reached for the retarder against the resulting overspeed, at three
    // quarters of a mile per hour over, on flat road -- a tester heard the
    // engine brake in corners three times running. The CDL rule is to reach a
    // safe speed BEFORE a bend and pull through, because braking mid-corner is
    // what locks a wheel and jackknifes a trailer, and a retarder drives the
    // tractor's rear wheels alone. Jacobs say the same: sustained speed
    // control, "not a substitute for a service braking system".
    //
    // Bench, level road, 62 set against a 45 advisory: the retarder was up for
    // 350 of 1200 frames at stage three, once per bend over a route of five
    // (2026-08-11). It must be silent, and cruise must still arrive.
    let (_harness, speeds, stages) = grade_hold(
        "Level Bend",
        0.0,
        GradeHold {
            seconds: 60.0,
            advisory: Some(45.0),
            ..Default::default()
        },
    );
    let peak = stages.iter().cloned().max().unwrap_or(0);
    assert_eq!(peak, 0, "{peak}");
    // And it genuinely slows for the bend -- silence must not mean cruise
    // simply carried the set speed through the corner.
    let last = *speeds.last().expect("a trace");
    assert!(last <= 48.0, "{last}");
    assert!(last >= 38.0, "{last}");
}

#[test]
fn test_cruise_still_retards_for_a_bend_on_a_downgrade() {
    // A bend on a grade retards -- that is the grade's doing, not the bend's.
    //
    // Removing the retarder here would put a six percent descent on the drums
    // alone: past fade onset in four and a half minutes (bench, 2026-08-11).
    let (_harness, speeds, stages) = grade_hold(
        "Bend Downgrade",
        -0.06,
        GradeHold {
            seconds: 60.0,
            advisory: Some(45.0),
            ..Default::default()
        },
    );
    let peak = stages.iter().cloned().max().unwrap_or(0);
    assert!(peak >= 1, "{peak}");
    // Holding the advisory, not running away down the hill.
    let tail = &speeds[speeds.len().saturating_sub(600)..];
    assert!(max_of(tail) <= 55.0, "{}", max_of(tail));
}

#[test]
fn test_cruise_holds_a_sustained_grade_on_the_retarder() {
    // The descent case, pinned: a plain downgrade is the retarder's own job.
    let (harness, speeds, stages) = grade_hold(
        "Sustained Grade",
        -0.06,
        GradeHold {
            seconds: 60.0,
            ..Default::default()
        },
    );
    let peak = stages.iter().cloned().max().unwrap_or(0);
    assert!(peak >= 1, "{peak}");
    let (temp, onset) =
        harness.read_drive(|d| (d.truck().brake_temp_c, d.truck().brake_fade_onset_c()));
    assert!(temp < onset, "{temp} {onset}");
    assert!(max_of(&speeds) <= 66.0, "{}", max_of(&speeds));
}

#[test]
fn test_cruise_gives_the_retarder_back_when_the_grade_runs_out_in_a_bend() {
    // The grade ends under the corner: the retarder goes with it.
    //
    // Handing the number to the drums used to leave whatever stage cruise had
    // raised on the hill still barking, all the way through the level bend --
    // 182 of 600 frames at stage three (bench, 2026-08-11). Only the stage
    // cruise raised itself is released; the driver's own switch is untouched.
    let mut harness = cruising("Grade Then Bend", 62.0, 200.0, &[(0.0, BENCH_MILES, -6.0)]);
    harness.app.ctx.settings.descent_speed_control = "realistic".to_string();
    for _ in 0..(12 * 60) {
        harness.advance_clock(DT);
        harness.with_drive(|d, ctx| {
            d.truck_mut().grade = -0.06;
            let ramp = DT * 2.2;
            let throttle = d.truck().throttle;
            let brake = d.truck().brake;
            d.truck_mut().throttle = 0.0f64.max(throttle - ramp * 2.0);
            d.truck_mut().brake = 0.0f64.max(brake - ramp * 3.0);
            d.update_cruise(ctx, DT, false, false, false);
            d.truck_mut().auto_shift();
            d.truck_mut().update(DT);
        });
    }
    assert!(
        harness.read_drive(|d| d.cruise_jake_stage) >= 1,
        "{}",
        harness.read_drive(|d| d.cruise_jake_stage)
    );

    // The bend arrives and the road levels out underneath it.
    harness.with_drive(|d, _| {
        d.cruise_curve_mph = Some(45.0);
        d.cruise_curve_end_mi = Some(d.trip.position_mi + 5.0);
        let at = d.trip.position_mi;
        bench_road_segments(d, &[(0.0, 200.0)], &[(0.0, BENCH_MILES, 0.0)], 1.0);
        d.trip.position_mi = at;
    });
    let mut stages = Vec::new();
    for _ in 0..(10 * 60) {
        harness.advance_clock(DT);
        let stage = harness.with_drive(|d, ctx| {
            d.truck_mut().grade = 0.0;
            let ramp = DT * 2.2;
            let throttle = d.truck().throttle;
            let brake = d.truck().brake;
            d.truck_mut().throttle = 0.0f64.max(throttle - ramp * 2.0);
            d.truck_mut().brake = 0.0f64.max(brake - ramp * 3.0);
            d.update_cruise(ctx, DT, false, false, false);
            d.truck_mut().auto_shift();
            d.truck_mut().update(DT);
            d.truck().engine_brake_stage
        });
        stages.push(stage);
    }
    let peak = stages.iter().cloned().max().unwrap_or(0);
    assert_eq!(peak, 0, "{peak}");
    assert_eq!(harness.read_drive(|d| d.cruise_jake_stage), 0);
}

/// Cruise's bench road with the AMT retarder manager armed by hand, exactly
/// as pressing J on an automatic box arms it.
///
/// Returns the stage trace, one entry per frame. The frame mirrors the
/// driving loop's order for the pieces the retarder rides on: the manager
/// runs, the automatic picks a gear, then the physics steps.
fn auto_jake_run(
    harness: &mut PlaytestHarness,
    armed_at: f64,
    grade: f64,
    seconds: f64,
) -> Vec<i32> {
    harness.with_drive(move |d, _| {
        d.truck_mut().grade = grade;
        d.truck_mut().throttle = 0.0;
        d.auto_jake = true;
        d.auto_jake_hold_mph = Some(5.0f64.max(armed_at));
        d.auto_jake_cooldown_s = 0.0;
        d.truck_mut().engine_brake_stage = 1; // the controller climbs from here
    });
    let mut stages = Vec::new();
    for _ in 0..((seconds * 60.0) as usize) {
        harness.advance_clock(DT);
        let stage = harness.with_drive(move |d, ctx| {
            d.truck_mut().grade = grade;
            d.truck_mut().throttle = 0.0;
            d.update_auto_jake(ctx, DT);
            d.truck_mut().auto_shift();
            d.truck_mut().update(DT);
            d.truck().engine_brake_stage
        });
        stages.push(stage);
    }
    stages
}

#[test]
fn test_auto_jake_does_not_chase_a_bend_advisory() {
    // The AMT retarder manager holds the driver's number, not the corner's.
    //
    // The third retarder path: the driver armed the stalk with J, so auto mode
    // owns the stage. It targets the speed it was armed at (or the number an
    // engaged speed authority is holding) and never reads a curve advisory, so
    // a bend cannot step it up. Pinned because it is the one path that would
    // otherwise reintroduce the corner bark by another route.
    //
    // Level road, at its number: there is nothing for a retarder to hold, so
    // it does not merely refuse to climb -- it comes all the way off. The
    // floor used to be stage one with no reason recorded for it anywhere, and
    // that floor is what put the jake on level road every time cruise lifted
    // off the fuel (jake sweep, 2026-08-24).
    let mut harness = cruising("Auto Jake", 62.0, 200.0, &[(0.0, BENCH_MILES, 0.0)]);
    harness.app.ctx.settings.descent_speed_control = "realistic".to_string();
    let armed_at = harness.read_drive(|d| d.truck().speed_mph());
    harness.with_drive(|d, ctx| {
        d.cancel_cruise(ctx, false);
        d.cruise_curve_mph = Some(45.0);
        d.cruise_curve_end_mi = Some(d.trip.position_mi + 5.0);
    });

    let stages = auto_jake_run(&mut harness, armed_at, 0.0, 20.0);

    let peak = stages.iter().cloned().max().unwrap_or(0);
    assert_eq!(peak, 0, "the bend bought the retarder a stage: {peak}");
    // And the manager is still armed -- it put the retarder down, it did not
    // switch itself off, so a real overspeed later still gets answered.
    assert!(harness.read_drive(|d| d.auto_jake));
}

#[test]
fn test_auto_jake_still_snubs_a_real_overspeed_on_level_road() {
    // The half that is NOT a bug, pinned so the question is not asked again.
    //
    // A retarder on level road has a legitimate reason to be there: the truck
    // is genuinely running over the number, which is what a driver arms the
    // manager for -- a crest that gave the truck speed, or the run-out of a
    // descent. Adaptive cruise reaches for the drums in that situation, on its
    // own doctrine; auto mode is the driver's own retarder and reaches for the
    // retarder.
    let mut harness = cruising("Auto Jake Over", 62.0, 200.0, &[(0.0, BENCH_MILES, 0.0)]);
    harness.app.ctx.settings.descent_speed_control = "realistic".to_string();
    harness.with_drive(|d, ctx| d.cancel_cruise(ctx, false));

    // Armed at 45, running 62: seventeen over its own number on level road.
    let stages = auto_jake_run(&mut harness, 45.0, 0.0, 60.0);

    let peak = stages.iter().cloned().max().unwrap_or(0);
    assert!(
        peak >= 2,
        "no retarder for a seventeen mph overspeed: {peak}"
    );
    // ...and it hands it back once the truck is down to the number rather
    // than riding it on down the road.
    assert_eq!(
        stages.last().copied().unwrap_or(-1),
        0,
        "the retarder never came off: {:?}",
        &stages[stages.len() - 5..]
    );
    let speed = harness.read_drive(|d| d.truck().speed_mph());
    assert!(speed <= 47.0, "never got down to its number: {speed:.1}");
}

#[test]
fn test_auto_jake_drops_the_retarder_the_moment_the_road_turns_up() {
    // The owner's report, on the driver's own retarder: "on uphill ascents the
    // truck should gain speed instead of using the engine brakes".
    //
    // Coming off a descent the truck is over its number, which on level road
    // is a legitimate snub (above). On a CLIMB it is not: the hill takes the
    // speed off by itself, and a real driver powers up a grade rather than
    // barking the retarder at it. Auto mode used to walk down one stage per
    // step and never below stage one, so a hill got seconds of retarder and
    // then a permanent two cylinders.
    let mut harness = cruising("Auto Jake Climb", 62.0, 200.0, &[(0.0, BENCH_MILES, 4.0)]);
    harness.app.ctx.settings.descent_speed_control = "realistic".to_string();
    harness.with_drive(|d, ctx| {
        d.cancel_cruise(ctx, false);
        d.truck_mut().velocity_mps = 70.0 * MPS_PER_MPH; // eight over, uphill
    });

    let stages = auto_jake_run(&mut harness, 62.0, 0.04, 10.0);

    let peak = stages.iter().cloned().max().unwrap_or(0);
    assert_eq!(peak, 0, "auto mode retarded a climb at stage {peak}");
}

#[test]
fn test_auto_jake_works_to_the_number_cruise_is_set_to() {
    // Two controllers with two answers is what put a full-stage retarder on
    // level road: armed at 45 and then cruise set to 62, auto mode spent the
    // drive trying to pull the truck back to a number cruise had never heard
    // of (jake sweep, 2026-08-24). An engaged speed authority owns the number.
    let mut harness = cruising("Auto Jake Cruise", 62.0, 200.0, &[(0.0, BENCH_MILES, 0.0)]);
    harness.app.ctx.settings.descent_speed_control = "realistic".to_string();
    assert_eq!(harness.read_drive(|d| d.cruise_mph), Some(62.0));

    // Armed at 45 while cruise holds 62: seventeen over the armed speed, and
    // on the number that actually governs.
    let stages = auto_jake_run(&mut harness, 45.0, 0.0, 20.0);

    let peak = stages.iter().cloned().max().unwrap_or(0);
    assert_eq!(peak, 0, "auto mode fought cruise at stage {peak}");
}

#[test]
fn test_auto_jake_still_holds_a_downgrade() {
    // The control. Everything above is about giving the retarder back where
    // there is nothing to hold; a real grade is the one place there IS, and
    // dropping it there would put the whole hill onto the drums.
    let mut harness = cruising("Auto Jake Grade", 62.0, 200.0, &[(0.0, BENCH_MILES, -6.0)]);
    harness.app.ctx.settings.descent_speed_control = "realistic".to_string();
    harness.with_drive(|d, ctx| d.cancel_cruise(ctx, false));

    let stages = auto_jake_run(&mut harness, 62.0, -0.06, 30.0);

    let peak = stages.iter().cloned().max().unwrap_or(0);
    assert!(peak >= 2, "the grade went unretarded: {peak}");
    let tail = &stages[stages.len() - 60..];
    assert!(
        tail.iter().all(|s| *s >= 1),
        "the retarder was dropped on a six percent descent: {tail:?}"
    );
}

#[test]
fn test_the_jake_stalk_still_answers_an_auto_mode_that_has_put_it_down() {
    // Auto mode is allowed to sit at stage zero now, so the two controls that
    // used to read the STAGE to decide whether the jake was on have to read
    // the manager instead -- or J would arm a second one and the cylinder
    // keys would do nothing at all.
    let mut harness = cruising("Auto Jake Stalk", 62.0, 200.0, &[(0.0, BENCH_MILES, 0.0)]);
    harness.with_drive(|d, ctx| d.cancel_cruise(ctx, false));
    let armed_at = harness.read_drive(|d| d.truck().speed_mph());
    auto_jake_run(&mut harness, armed_at, 0.0, 2.0);
    assert_eq!(harness.read_drive(|d| d.truck().engine_brake_stage), 0);
    assert!(harness.read_drive(|d| d.auto_jake));

    // Two takes the retarder back by hand, at two cylinders.
    harness.clear_speech();
    harness.with_drive(|d, ctx| d.select_jake_stage(ctx, 2));
    assert_eq!(harness.read_drive(|d| d.truck().engine_brake_stage), 2);
    assert!(!harness.read_drive(|d| d.auto_jake));
    assert!(
        said_any(&harness, "Jake stage two selected, manual."),
        "{:#?}",
        spoken(&harness)
    );

    // And J switches an idle manager off, so the cab never says "Jake off"
    // over a manager that is still armed to bring it back.
    harness.with_drive(|d, _| {
        d.truck_mut().engine_brake_stage = 0;
        d.auto_jake = true;
    });
    harness.clear_speech();
    harness.with_drive(|d, ctx| d.toggle_engine_brake(ctx));
    assert!(!harness.read_drive(|d| d.auto_jake));
    assert!(said_any(&harness, "Jake off."), "{:#?}", spoken(&harness));
}

#[test]
fn test_descent_control_cue_does_not_chant_through_rolling_country() {
    // Every dip crosses the trigger; the announcement needs its own clock.
    //
    // Python's rolling country was a sine wave on `grade_at`. A baked profile
    // is a step function, so the same road is baked as alternating half-mile
    // segments of the same amplitude: every dip still crosses the trigger.
    let mut segments = Vec::new();
    let mut mile = 0.0;
    let mut up = true;
    while mile < BENCH_MILES {
        segments.push((mile, mile + 0.5, if up { 5.0 } else { -5.0 }));
        mile += 0.5;
        up = !up;
    }
    let mut harness = cruising("Rolling Country", 62.0, 200.0, &segments);
    harness.clear_speech();
    for _ in 0..(6 * 60 * 60) {
        harness.advance_clock(DT);
        harness.with_drive(|d, ctx| {
            let grade = d.trip.grade_at(d.trip.position_mi);
            d.truck_mut().grade = grade;
            let ramp = DT * 2.2;
            let throttle = d.truck().throttle;
            let brake = d.truck().brake;
            d.truck_mut().throttle = 0.0f64.max(throttle - ramp * 2.0);
            d.truck_mut().brake = 0.0f64.max(brake - ramp * 3.0);
            d.update_cruise(ctx, DT, false, false, false);
            if d.truck().transmission.automatic {
                d.truck_mut().auto_shift();
            }
            d.truck_mut().update(DT);
            d.trip.position_mi += d.truck().speed_mph() / 60.0 / 3600.0;
        });
    }
    let holding = spoken(&harness)
        .iter()
        .filter(|e| e.contains("Descent control holding"))
        .count();
    assert!(holding <= 3, "{holding}");
}

// -- the speed keeper's air budget ----------------------------------------------

/// `_keeper_on_a_grade(app, grade_pct=, limit_mph=)`: speed keeper holding a
/// zone limit down a steady grade.
fn keeper_on_a_grade(name: &str, grade_pct: f64, limit_mph: f64) -> PlaytestHarness {
    let mut harness = start_drive(name);
    harness.app.ctx.settings.time_scale = 1.0;
    release_keys(&mut harness);
    harness.with_drive(move |d, _| {
        bench_road_segments(d, &[(0.0, 200.0)], &[(0.0, BENCH_MILES, grade_pct)], 1.0);
        d.trip.position_mi = START_MI;
        d.trip.zones = vec![Zone::new(0.0, 1e6, limit_mph, "facility access road")];
    });
    press(&mut harness, Key::E, None);
    harness.with_drive(|d, _| {
        d.truck_mut().cargo_kg = 15_000.0;
        d.truck_mut().velocity_mps = 25.0 * MPS_PER_MPH;
    });
    press(&mut harness, Key::K, None);
    assert!(harness.read_drive(|d| d.keeper_mph).is_some());
    harness
}

#[test]
fn test_speed_keeper_holds_a_zone_speed_without_emptying_the_tanks() {
    // The report this exists for: the assist ran the truck out of air.
    //
    // A mild downgrade put the old proportional trim right on its own braking
    // deadband, so it made and released a brake application several times a
    // second. The air system charges a whole application on every rise, so the
    // tanks went 125 psi to 41 in eighteen seconds, the spring brakes set, and
    // the truck stopped dead in a 15 mph zone.
    //
    // Python counted applications by patching `TruckState._consume_brake_air`.
    // Here the same rising edges are read from outside, off
    // `last_service_air_application`, which is the very number that patch
    // compared against.
    let mut harness = keeper_on_a_grade("Keeper Air", -2.0, 15.0);
    let mut applications = 0;
    let mut previous = harness.read_drive(|d| d.truck().last_service_air_application);
    for _ in 0..(60 * 60) {
        frame(&mut harness, DT);
        let application = harness.read_drive(|d| d.truck().last_service_air_application);
        if application - previous > 1e-9 {
            applications += 1;
        }
        previous = application;
    }
    assert!(!harness.read_drive(|d| d.truck().spring_brakes_active()));
    let psi = harness.read_drive(|d| d.truck().air_pressure_psi());
    assert!(
        !harness.read_drive(|d| d.truck().air_low_warning()),
        "{psi}"
    );
    assert!(psi > 100.0, "{psi}");
    // Still driving the zone a minute later, not parked in it.
    assert!(harness.read_drive(|d| d.keeper_mph).is_some());
    assert!(harness.read_drive(|d| d.truck().speed_mph()) > 5.0);
    // A snub is one application held to the number, so a minute of holding
    // costs a handful of them rather than one per frame.
    assert!(applications <= 20, "{applications}");
}

#[test]
fn test_speed_keeper_says_when_it_cannot_hold_the_speed() {
    // Hot brakes on a real grade: the keeper is out of pedal and says so.
    //
    // An assist that quietly holds the wrong speed is the one failure a driver
    // who cannot see the speedometer has no way to catch.
    let mut harness = keeper_on_a_grade("Keeper Beaten", -6.0, 15.0);
    harness.clear_speech();
    for _ in 0..(60 * 30) {
        harness.with_drive(|d, _| {
            let temp = d.truck().brake_temp_c;
            d.truck_mut().brake_temp_c = temp.max(750.0); // faded past any authority
        });
        frame(&mut harness, DT);
    }
    let said = spoken(&harness)
        .iter()
        .filter(|e| *e == "Speed keeper can't hold 15 miles per hour on this grade. Brake.")
        .count();
    assert_eq!(said, 1, "{:#?}", spoken(&harness));
}

// -- driver-selectable following gap ----------------------------------------

#[test]
fn test_the_closest_gap_offered_is_still_well_clear_of_a_citation() {
    // The floor under the whole setting.
    //
    // Tester Darren was fined 1,200 dollars for a following gap adaptive
    // cruise was managing and he had no say in (I-75, 2026-08-18). Giving him
    // a say is worth nothing if the closest choice is itself ticketable, so
    // every offered gap has to sit clear of TAILGATE_GAP_S -- and clear by
    // enough that closing on a slower vehicle does not walk straight into it.
    assert!(!ACC_GAP_CHOICES.is_empty());
    for (name, seconds) in ACC_GAP_CHOICES {
        assert!(
            seconds >= TAILGATE_GAP_S + 1.0,
            "{name} leaves only {seconds}s"
        );
    }
}

#[test]
fn test_weather_opens_the_chosen_gap_and_never_shortens_it() {
    // Close means close on a clear day, not on ice.
    //
    // The driver's choice is the floor and weather only ever adds to it, so
    // picking the shortest cushion cannot cancel the wet-road opening, and
    // picking the longest one cannot be quietly pulled back to the middle.
    let mut harness = start_drive("Gap Weather");
    // Effects are derived from the condition, so drive the condition --
    // forcing a grip number would test a state the weather cannot be in.
    for (name, seconds) in ACC_GAP_CHOICES {
        harness.app.ctx.settings.acc_following_gap = name.to_string();
        harness.with_drive(|d, _| d.weather_mut().current = WeatherKind::Clear);
        assert!(
            approx(harness.with_drive(|d, ctx| d.acc_gap_seconds(ctx)), seconds),
            "{name}"
        );

        harness.with_drive(|d, _| d.weather_mut().current = WeatherKind::Snow);
        let wet = harness.with_drive(|d, ctx| d.acc_gap_seconds(ctx));
        assert!(wet > seconds, "{name} did not open up for snow");
    }
}

#[test]
fn test_the_gap_row_speaks_the_seconds_not_just_the_word() {
    // A word alone tells a player working by ear nothing about how much road
    // "close" actually buys. The number is in the label, not buried in the
    // help text.
    //
    // Python read `SettingsCategoryState._acc_gap_label()`. That helper is
    // `pub(super)` here, so the row is read the way a player hears it: off the
    // assistance settings screen itself.
    use freight_fate::states::main_menu::SettingsCategoryState;

    let mut harness = PlaytestHarness::new();
    harness.app.ctx.settings.acc_following_gap = "close".to_string();
    harness
        .app
        .ctx
        .push_state(SettingsCategoryState::new("assistance"));
    harness.app.ctx.run_deferred();
    let row = harness
        .menu_labels()
        .into_iter()
        .find(|row| row.to_lowercase().contains("following gap"))
        .expect("a following-gap row");
    assert!(row.contains("close, 2 and a half seconds"), "{row}");

    harness.app.ctx.settings.acc_following_gap = "normal".to_string();
    harness.app.ctx.pop_state();
    harness
        .app
        .ctx
        .push_state(SettingsCategoryState::new("assistance"));
    harness.app.ctx.run_deferred();
    let row = harness
        .menu_labels()
        .into_iter()
        .find(|row| row.to_lowercase().contains("following gap"))
        .expect("a following-gap row");
    assert!(row.contains("normal, 3 seconds"), "{row}");
}

// -- the retarder in weather, and the weather cap -------------------------------

#[test]
fn test_cruise_leaves_the_jake_alone_on_a_slick_flat_road() {
    // A storm ease is drums-only: the jake retards the drive axle, which on
    // soaked pavement is how a tractor swaps ends, so a driver shuts it off in
    // rain and cruise must hold itself to the same rule. The thunderstorm
    // safe-speed ease from 65 to 40 used to play the jake on flat, soaked I-24
    // (owner playtest, 2026-08-20).
    let mut harness = cruising("Slick Flat", 62.0, 200.0, &[(0.0, BENCH_MILES, 0.0)]);
    harness.app.ctx.settings.descent_speed_control = "realistic".to_string();
    harness.with_drive(|d, _| d.weather_mut().current = WeatherKind::Thunderstorm);
    assert!(
        harness.read_drive(|d| d.weather().effects().grip) < 0.7,
        "the premise: a storm is slick"
    );
    // The storm ease: cruise target far below the truck's speed, level road.
    harness.with_drive(|d, _| d.cruise_mph = Some(40.0));
    let mut stages = Vec::new();
    for _ in 0..(12 * 60) {
        harness.advance_clock(DT);
        let stage = harness.with_drive(|d, ctx| {
            d.truck_mut().grade = 0.0;
            d.truck_mut().throttle = 0.0;
            d.update_cruise(ctx, DT, false, false, false);
            d.truck_mut().auto_shift();
            d.truck_mut().update(DT);
            d.truck().engine_brake_stage
        });
        stages.push(stage);
    }
    let peak = stages.iter().cloned().max().unwrap_or(0);
    assert_eq!(
        peak, 0,
        "cruise raised jake stage {peak} on slick flat road"
    );
    assert!(
        harness.read_drive(|d| d.truck().speed_mph()) < 55.0,
        "the drums still have to do the slowing"
    );
}

#[test]
fn test_cruise_keeps_the_retarder_on_a_slick_downgrade() {
    // The exception is a real grade: dropping a retarder that is holding the
    // hill puts the whole descent onto the drums, the greater evil the release
    // branch's own comment records. Slick flat road gets the drums; a slick
    // grade keeps the retarder.
    let mut harness = cruising("Slick Grade", 62.0, 200.0, &[(0.0, BENCH_MILES, -6.0)]);
    harness.app.ctx.settings.descent_speed_control = "realistic".to_string();
    harness.with_drive(|d, _| d.weather_mut().current = WeatherKind::Thunderstorm);
    for _ in 0..(12 * 60) {
        harness.advance_clock(DT);
        harness.with_drive(|d, ctx| {
            d.truck_mut().grade = -0.06;
            let ramp = DT * 2.2;
            let throttle = d.truck().throttle;
            let brake = d.truck().brake;
            d.truck_mut().throttle = 0.0f64.max(throttle - ramp * 2.0);
            d.truck_mut().brake = 0.0f64.max(brake - ramp * 3.0);
            d.update_cruise(ctx, DT, false, false, false);
            d.truck_mut().auto_shift();
            d.truck_mut().update(DT);
        });
    }
    assert!(
        harness.read_drive(|d| d.cruise_jake_stage) >= 1,
        "a held grade keeps its retarder, storm or not"
    );
}

#[test]
fn test_the_resume_line_names_the_zone_cap_it_will_actually_hold() {
    // Clear of the queue inside a heavy-traffic zone posting 20, the resume
    // line used to say the SET speed while the zone cap silently held the
    // working target at 23 -- minutes of open-looking road with the words
    // contradicting the truck (Brandon, 2026-08-20). The line now names the
    // capped number and why.
    //
    // Python patched `_acc_posted_limit_ahead` to answer (20, "heavy
    // traffic"). Here a real heavy-traffic zone is put on the road ahead, the
    // way `test_cruise_pre_brakes_for_heavy_traffic_like_a_work_zone` does, so
    // the same answer comes out of the real lookahead.
    let mut harness = cruising("Zone Resume", 70.0, 200.0, &[(0.0, BENCH_MILES, 0.0)]);
    let start = harness.read_drive(|d| d.trip.position_mi) + 0.5;
    let zone = Zone::new(start, start + 3.0, 20.0, "heavy traffic");
    harness.with_drive(move |d, _| {
        d.trip.zones.push(zone.clone());
        d.trip.announced_zone_warnings.insert(zone_key(&zone));
    });
    assert_eq!(
        harness.with_drive(|d, ctx| d.acc_posted_limit_ahead(ctx)),
        (20.0, Some("heavy traffic".to_string()))
    );
    harness.clear_speech();
    harness.with_drive(|d, ctx| d.engage_cruise(ctx, 70.0, true));
    let line = spoken(&harness)
        .into_iter()
        .find(|s| s.starts_with("Cruise resuming"))
        .unwrap_or_else(|| panic!("no resume line\n{:#?}", spoken(&harness)));
    assert!(
        line.contains("resuming, 20 through the heavy traffic"),
        "{line}"
    );
    assert!(!line.contains("70"), "{line}");
}

#[test]
fn test_cruise_never_raises_the_jake_on_a_climb() {
    // Overspeed carried into an upgrade is the hill's to eat: a real driver
    // powers up a grade, never barks the retarder at it (Brandon,
    // 2026-08-20). The raise path now honors `on_downgrade`'s own doctrine.
    let mut harness = cruising("Jake Climb", 55.0, 200.0, &[(0.0, BENCH_MILES, 3.0)]);
    harness.app.ctx.settings.descent_speed_control = "realistic".to_string();
    harness.with_drive(|d, _| {
        d.truck_mut().velocity_mps = 70.0 * MPS_PER_MPH; // fifteen over, uphill
    });
    let mut stages = Vec::new();
    for _ in 0..(10 * 60) {
        harness.advance_clock(DT);
        let stage = harness.with_drive(|d, ctx| {
            d.truck_mut().grade = 0.03;
            d.truck_mut().throttle = 0.0;
            d.update_cruise(ctx, DT, false, false, false);
            d.truck_mut().auto_shift();
            d.truck_mut().update(DT);
            d.truck().engine_brake_stage
        });
        stages.push(stage);
    }
    let peak = stages.iter().cloned().max().unwrap_or(0);
    assert_eq!(peak, 0, "cruise raised jake stage {peak} on an upgrade");
}

#[test]
fn test_cruise_eases_to_the_weather_safe_speed_and_says_so_once() {
    // A real adaptive system treats the weather like any other road fact: the
    // safe speed was computed and spoken as guidance since live weather
    // shipped, and enforced by nothing -- cruise held a set seventy through a
    // thunderstorm until the driver tapped it down (Brandon's suggestion,
    // 2026-08-20; owner-approved same day).
    let mut harness = cruising("Storm Ease", 70.0, 200.0, &[(0.0, BENCH_MILES, 0.0)]);
    harness.clear_speech();
    harness.with_drive(|d, _| d.weather_mut().current = WeatherKind::Thunderstorm);
    for _ in 0..(20 * 60) {
        harness.advance_clock(DT);
        harness.with_drive(|d, ctx| {
            d.truck_mut().grade = 0.0;
            d.update_cruise(ctx, DT, false, false, false);
            d.truck_mut().auto_shift();
            d.truck_mut().update(DT);
        });
    }
    let speed = harness.read_drive(|d| d.truck().speed_mph());
    assert!(speed <= 40.0 + 2.0, "held {speed:.1} in a thunderstorm");
    let eased: Vec<String> = spoken(&harness)
        .into_iter()
        .filter(|e| e.contains("cruise easing to 40"))
        .collect();
    assert_eq!(
        eased,
        vec!["Thunderstorm, cruise easing to 40.".to_string()],
        "{:#?}",
        spoken(&harness)
    );
}

#[test]
fn test_the_weather_cap_lifts_with_the_weather() {
    let mut harness = cruising("Cap Lifts", 70.0, 200.0, &[(0.0, BENCH_MILES, 0.0)]);
    harness.clear_speech();
    harness.with_drive(|d, _| d.weather_mut().current = WeatherKind::Thunderstorm);
    for _ in 0..(20 * 60) {
        harness.advance_clock(DT);
        harness.with_drive(|d, ctx| {
            d.truck_mut().grade = 0.0;
            d.update_cruise(ctx, DT, false, false, false);
            d.truck_mut().auto_shift();
            d.truck_mut().update(DT);
        });
    }
    assert!(harness.read_drive(|d| d.truck().speed_mph()) <= 42.0);

    harness.with_drive(|d, _| d.weather_mut().current = WeatherKind::Clear);
    for _ in 0..(40 * 60) {
        harness.advance_clock(DT);
        harness.with_drive(|d, ctx| {
            d.truck_mut().grade = 0.0;
            let ramp = DT * 2.2;
            let brake = d.truck().brake;
            d.truck_mut().brake = 0.0f64.max(brake - ramp * 3.0);
            d.update_cruise(ctx, DT, false, false, false);
            d.truck_mut().auto_shift();
            d.truck_mut().update(DT);
        });
    }
    let speed = harness.read_drive(|d| d.truck().speed_mph());
    assert!(
        speed > 55.0,
        "never climbed back after the storm: {speed:.1}"
    );

    // The next front is news again.
    harness.with_drive(|d, _| d.weather_mut().current = WeatherKind::HeavyRain);
    for _ in 0..(5 * 60) {
        harness.advance_clock(DT);
        harness.with_drive(|d, ctx| {
            d.truck_mut().grade = 0.0;
            let brake = d.truck().brake;
            d.truck_mut().brake = 0.0f64.max(brake - DT * 2.2 * 3.0);
            d.update_cruise(ctx, DT, false, false, false);
            d.truck_mut().auto_shift();
            d.truck_mut().update(DT);
        });
    }
    assert!(
        said_any(&harness, "Heavy rain, cruise easing to 45."),
        "{:#?}",
        spoken(&harness)
    );
}

#[test]
fn test_the_resume_line_names_the_weather_cap() {
    // Python patched `_acc_posted_limit_ahead` to (75, None); here the bench
    // road really posts 75, which is the same answer through the real
    // lookahead.
    let mut harness = cruising("Weather Resume", 70.0, 75.0, &[(0.0, BENCH_MILES, 0.0)]);
    assert_eq!(
        harness.with_drive(|d, ctx| d.acc_posted_limit_ahead(ctx)),
        (75.0, None)
    );
    harness.with_drive(|d, _| d.weather_mut().current = WeatherKind::Snow);
    harness.clear_speech();
    harness.with_drive(|d, ctx| d.engage_cruise(ctx, 70.0, true));
    let line = spoken(&harness)
        .into_iter()
        .find(|s| s.starts_with("Cruise resuming"))
        .unwrap_or_else(|| panic!("no resume line\n{:#?}", spoken(&harness)));
    assert!(line.contains("resuming, 35 in the snow"), "{line}");
    // The resume line named the cap, so the cruise loop must not repeat it:
    // "Snow, cruise easing to 35." used to follow at once (agent drive,
    // 2026-10-25).
    for _ in 0..(5 * 60) {
        harness.advance_clock(DT);
        harness.with_drive(|d, ctx| {
            d.truck_mut().grade = 0.0;
            d.update_cruise(ctx, DT, false, false, false);
            d.truck_mut().auto_shift();
            d.truck_mut().update(DT);
        });
    }
    let eased: Vec<String> = spoken(&harness)
        .into_iter()
        .filter(|s| s.contains("cruise easing to"))
        .collect();
    assert!(eased.is_empty(), "{eased:#?}");
}
