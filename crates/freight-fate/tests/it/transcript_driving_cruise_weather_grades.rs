//! Holding the set speed on a grade, and predictive cruise (port of
//! `tests/test_driving_cruise_weather.py`, lines 2983-3458).
//!
//! Fifth of the split; see `transcript_driving_cruise_weather.rs` for why the
//! Python file is split and `transcript_cruise_support` for what replaced each
//! monkeypatch, including the two this half adds:
//!
//! * `trip.engine_brake_ban_at = lambda mile: None` -- the bench leg is 400
//!   miles long and every case starts the truck in its middle, which is clear
//!   of both city ends, so the ban really is absent.
//! * `_grade_hold`'s `trip.grade_at = lambda mile: grade` -- a baked
//!   `GradeSegment` over the whole leg; the loop still writes `t.grade` by
//!   hand each frame exactly as Python did.

use freight_fate::playtest::harness::PlaytestHarness;
use freight_fate::states::driving_core::{
    ACC_LIMIT_OFFSET_MPH, CRUISE_BRAKE_OVER_MPH, DESCENT_SAFE_MAX_MPH, MPH_PER_MPS,
    PCC_CREST_SAG_MPH,
};

use crate::transcript_cruise_support::*;

// -- holding the set speed on a grade -------------------------------------------

#[test]
fn test_cruise_does_not_run_away_down_a_grade() {
    // Cutting fuel is not speed control on a downgrade.
    //
    // Cruise had no authority over the truck from above unless a lead or a
    // lower posted limit was already pulling the target down, so gravity
    // simply carried it: a 2 percent descent settled nine mph past the set
    // speed and a 6 percent descent accelerated without limit (bench trace,
    // 2026-07-25: 62 set, 100 mph and still climbing). The retarder now stages
    // against the overspeed, and the drums snub when it is not enough.
    let set_mph = GradeHold::default().set_mph;
    // Two percent is the SHALLOW case and its bound is now the design deadband
    // itself, `CRUISE_BRAKE_OVER_MPH`: cruise is allowed to sit that far over
    // the number before it puts a foot on the drums, because dragging the
    // brakes down a hill is how a truck runs out of air. It used to be pinned
    // at 63.5 -- half a mile an hour tighter -- and that half was the mark of
    // a retarder trimming a grade the drums hold on their own all day, which
    // is precisely what `retarder_warranted` has since taken away from it. The
    // stage trace is asserted below so this cannot come back unnoticed.
    for (grade, ceiling) in [
        (-0.02, set_mph + CRUISE_BRAKE_OVER_MPH),
        (-0.04, 66.0),
        (-0.06, 66.0),
    ] {
        let (_harness, speeds, stages) = grade_hold("Runaway", grade, GradeHold::default());
        assert!(max_of(&speeds) <= ceiling, "{grade} {}", max_of(&speeds));
        // And it is holding a speed, not braking the truck to a stop: the jake
        // used to be pinned wide open the moment the grade passed 2.5 percent,
        // which dragged the truck well under its own target.
        let tail = &speeds[speeds.len().saturating_sub(600)..];
        assert!(min_of(tail) >= 58.0, "{grade} {}", min_of(tail));
        // It SETTLES rather than creeping: the tail is inside the same bound
        // the whole trace is, so the shallow grade is being held and not
        // slowly given away.
        assert!(max_of(tail) <= ceiling, "{grade} tail {}", max_of(tail));
        if grade == -0.02 {
            // And it is the DRUMS holding it. A two percent descent is not a
            // hill the service brakes cannot handle, so no retarder belongs on
            // it at all -- the owner's report of 2026-08-24, pinned.
            assert!(
                stages.iter().all(|stage| *stage == 0),
                "the retarder came up on a two percent descent: {:?}",
                stages.iter().filter(|s| **s > 0).count()
            );
        }
    }
}

#[test]
fn test_cruise_answers_a_climb_before_it_costs_twenty_mph() {
    // Feed-forward plus the pull downshift, instead of a slow integrator.
    //
    // Cruise used to walk the throttle up at 0.08 per mph-second with no idea
    // what the grade was asking for, and the automatic held top gear because
    // the revs were not lugging yet. A 2 percent climb bled six mph and never
    // got them back; a 4 percent climb lost thirty (bench trace, 2026-07-25).
    let (harness, speeds, _stages) = grade_hold("Climb Two", 0.02, GradeHold::default());
    // A 2 percent pull is well inside what the truck has: hold it.
    assert!(min_of(&speeds) >= 59.0, "{}", min_of(&speeds));
    assert!(
        *speeds.last().expect("a trace") >= 60.0,
        "{:?}",
        speeds.last()
    );
    let (gear, top, throttle) = harness.read_drive(|d| {
        (
            d.truck().transmission.gear,
            d.truck().transmission.num_gears(),
            d.truck().throttle,
        )
    });
    assert!(gear < top || throttle < 1.0);
    drop(harness);

    let (harness, speeds, _stages) = grade_hold("Climb Four", 0.04, GradeHold::default());
    // A 4 percent pull genuinely costs a loaded truck speed -- but it must
    // cost it in a lower gear making real torque, not at full throttle in
    // overdrive watching the hill win.
    assert!(
        *speeds.last().expect("a trace") >= 40.0,
        "{:?}",
        speeds.last()
    );
    let (gear, top) = harness.read_drive(|d| {
        (
            d.truck().transmission.gear,
            d.truck().transmission.num_gears(),
        )
    });
    assert!(gear < top);
}

#[test]
fn test_interactive_descent_control_caps_the_target_without_rewriting_it() {
    // The safe descent ceiling lasts as long as the grade, not the career.
    //
    // It used to assign straight into the cruise set speed, so one downgrade
    // on a 65 road knocked cruise to 55 permanently -- on the flat, uphill,
    // the rest of the run.
    let (mut harness, _speeds, _stages) = grade_hold(
        "Interactive Descent",
        -0.06,
        GradeHold {
            seconds: 25.0,
            descent: "interactive",
            ..Default::default()
        },
    );
    assert!(approx(
        harness.read_drive(|d| d.cruise_mph).expect("cruise"),
        62.0
    ));
    assert!(approx(
        harness
            .read_drive(|d| d.cruise_descent_mph)
            .expect("a ceiling"),
        55.0
    ));

    // Back on the level: the ceiling lifts and the driver's number returns.
    harness.with_drive(|d, _| {
        bench_road_segments(d, &[(0.0, 200.0)], &[(0.0, BENCH_MILES, 0.0)], 1.0);
        d.trip.position_mi = START_MI;
        d.truck_mut().grade = 0.0;
    });
    for _ in 0..120 {
        harness.advance_clock(DT);
        harness.with_drive(|d, ctx| {
            d.update_cruise(ctx, DT, false, false, false);
            d.truck_mut().update(DT);
        });
    }
    assert!(harness.read_drive(|d| d.cruise_descent_mph).is_none());
    assert!(approx(
        harness.read_drive(|d| d.cruise_mph).expect("cruise"),
        62.0
    ));
}

#[test]
fn test_cruise_snubs_the_drums_instead_of_dragging_them_down_a_grade() {
    // A held application empties the air tanks and fades the shoes.
    //
    // Cruise trimmed the service brake proportionally, which on a long grade
    // settled into a permanent light application: the compressor lost ground
    // until the spring brakes set and stopped the truck dead on a downhill
    // (bench trace, 2026-07-25: 125 psi to 74 in twenty-two seconds).
    let (harness, speeds, _stages) = grade_hold(
        "Snub Drums",
        -0.06,
        GradeHold {
            seconds: 80.0,
            ..Default::default()
        },
    );
    // The grade is held, and held without the drums paying for it: full tanks,
    // cool shoes, no spring brakes.
    assert!(max_of(&speeds) <= 66.0, "{}", max_of(&speeds));
    assert!(min_of(&speeds) > 30.0, "{}", min_of(&speeds));
    assert!(!harness.read_drive(|d| d.truck().air_brakes_holding()));
    let psi = harness.read_drive(|d| d.truck().air_pressure_psi());
    assert!(psi >= 100.0, "{psi}");
    let (temp, onset) =
        harness.read_drive(|d| (d.truck().brake_temp_c, d.truck().brake_fade_onset_c()));
    assert!(temp < onset, "{temp} {onset}");
}

#[test]
fn test_cruise_leaves_the_drivers_own_jake_alone() {
    // Cruise releases only the retarder stages it raised itself.
    let mut harness = cruising("Driver Jake", 62.0, 200.0, &[(0.0, BENCH_MILES, 0.0)]);
    harness.with_drive(|d, _| {
        d.truck_mut().grade = 0.0;
        d.truck_mut().engine_brake_stage = 2; // the driver's own selection
    });

    for _ in 0..60 {
        harness.advance_clock(DT);
        harness.with_drive(|d, ctx| d.update_cruise(ctx, DT, false, false, false));
    }

    assert_eq!(harness.read_drive(|d| d.truck().engine_brake_stage), 2);
    assert_eq!(harness.read_drive(|d| d.cruise_jake_stage), 0);
}

#[test]
fn test_cruise_closing_on_a_downgrade_does_not_stack_past_the_cargo_line() {
    // Closing on a ramp, a lower limit, or a lead vehicle keeps whatever
    // retarder stage the descent already raised -- rightly, since dropping it
    // would hand the whole grade to the drums -- but the snub that does the
    // closing was sized blind to that retarder: a flat fraction of the
    // overspeed, with no idea the jake was already holding the hill. On a
    // steep loaded descent with the retarder up, the two stack, and the
    // combined deceleration reaches the freight the same way an emergency
    // stop does.
    let (mut harness, _speeds, stages) =
        grade_hold("Closing Downgrade", -0.06, GradeHold::default());
    assert!(
        stages.iter().any(|s| *s > 0),
        "the descent never raised the retarder: {stages:?}"
    );
    harness.with_drive(|d, _| d.truck_mut().cargo_damage_pct = 0.0);

    // A ramp cap well under the truck's speed, arriving mid-descent -- the
    // shape a lower posted limit or a slower lead makes too.
    let speed = harness.read_drive(|d| d.truck().speed_mph());
    harness.with_drive(move |d, _| d.cruise_exit_mph = Some(speed - 25.0));
    for _ in 0..(5 * 60) {
        harness.advance_clock(DT);
        harness.with_drive(|d, ctx| {
            d.truck_mut().grade = -0.06;
            d.update_cruise(ctx, DT, false, false, false);
            d.truck_mut().update(DT);
        });
    }
    assert_eq!(
        harness.read_drive(|d| d.truck().cargo_damage_pct),
        0.0,
        "closing on a downgrade stacked the snub on the retarder and damaged the freight"
    );
}

#[test]
fn test_a_held_clutch_lets_go_of_cruises_snub_under_the_number() {
    // A snub rides through a shift, not through a clutch held down or a box
    // left in neutral: on a manual those last as long as the driver does, and
    // the snub braked the truck toward a stop with nothing said (review,
    // 2026-09-28). Level road, so descent control stays out of it.
    let mut harness = cruising("Clutch Snub", 62.0, 200.0, &[(0.0, BENCH_MILES, 0.0)]);
    harness.with_drive(|d, _| {
        d.truck_mut().transmission.automatic = false;
        d.truck_mut().grade = 0.0;
        d.cruise_held_mph = Some(62.0);
        d.cruise_snubbing = true;
    });
    // Over the number, the snub holds through the clutch.
    harness.with_drive(|d, ctx| {
        d.truck_mut().velocity_mps = 66.0 * MPS_PER_MPH;
        d.truck_mut().brake = 0.0;
        d.update_cruise(ctx, DT, false, false, true);
    });
    assert!(harness.read_drive(|d| d.cruise_snubbing));
    assert!(harness.read_drive(|d| d.truck().brake) > 0.0);
    // Under it, clutch still in, it lets go.
    harness.with_drive(|d, ctx| {
        d.truck_mut().velocity_mps = 55.0 * MPS_PER_MPH;
        d.truck_mut().brake = 0.0;
        d.update_cruise(ctx, DT, false, false, true);
    });
    assert!(!harness.read_drive(|d| d.cruise_snubbing));
    assert_eq!(harness.read_drive(|d| d.truck().brake), 0.0);
}

// -- predictive cruise ------------------------------------------------------------

/// `_hill_road(driving, flat_mi=, grade=, climb_mi=)`: flat, then a sustained
/// climb, then flat, anchored where the truck is.
fn hill_road(harness: &mut PlaytestHarness, flat_mi: f64, grade: f64, climb_mi: f64) -> f64 {
    let start = harness.read_drive(|d| d.trip.position_mi);
    harness.with_drive(move |d, _| {
        bench_road_segments(
            d,
            &[(0.0, 200.0)],
            // A baked segment's range is CLOSED and the lookup takes the
            // first that contains the mile, so each boundary is nudged a hair
            // to keep the half-open shape Python's `offset < flat + climb`
            // had: the foot of the hill reads the climb, the crest reads the
            // flat beyond it. Without that the preview sees one extra
            // tenth-mile of hill and a 0.2-mile pull never reads as a crest.
            &[
                (0.0, (start + flat_mi - 1e-9).max(0.0), 0.0),
                (
                    start + flat_mi,
                    start + flat_mi + climb_mi - 1e-9,
                    grade * 100.0,
                ),
                (start + flat_mi + climb_mi - 1e-9, BENCH_MILES, 0.0),
            ],
            1.0,
        );
        d.trip.position_mi = start;
    });
    start
}

#[test]
fn test_predictive_cruise_banks_speed_before_a_climb() {
    // The preview reads the grade profile and enters the pull carrying more.
    //
    // Momentum banked on the flat is speed the truck keeps most of the way up,
    // which is the whole point of a predictive system reading a stored road
    // profile.
    let mut harness = cruising("Bank Climb", 62.0, 200.0, &[(0.0, BENCH_MILES, 0.0)]);
    hill_road(&mut harness, 0.5, 0.04, 1.0);
    harness.with_drive(|d, _| d.truck_mut().grade = 0.0);
    harness.app.ctx.settings.predictive_cruise = true;
    assert!(harness.with_drive(|d, ctx| d.predictive_cruise_bias(ctx, 62.0).0) > 1.0);

    // Turned off, cruise plans nothing and holds the number it was given.
    harness.app.ctx.settings.predictive_cruise = false;
    assert!(approx(
        harness.with_drive(|d, ctx| d.predictive_cruise_bias(ctx, 62.0).0),
        0.0
    ));
}

#[test]
fn test_predictive_cruise_builds_for_a_grade_in_silence() {
    // "Building speed for a 2.0 percent uphill grade" was chatter about how
    // cruise works (player report and owner, 2026-10-14). The phase is still
    // tracked; nothing is said.
    let mut harness = cruising("Name The Grade", 62.0, 200.0, &[(0.0, BENCH_MILES, 0.0)]);
    hill_road(&mut harness, 0.5, 0.02, 1.0);
    harness.with_drive(|d, _| d.truck_mut().grade = 0.0);
    harness.app.ctx.settings.predictive_cruise = true;
    harness.clear_speech();
    let bias = harness.with_drive(|d, ctx| d.predictive_cruise_bias(ctx, 62.0));
    assert!(bias.0 > 0.5, "{:?}", bias);
    harness.with_drive(move |d, _| d.track_predictive_cruise(bias.0, bias.1));
    assert_eq!(harness.read_drive(|d| d.pcc_phase.clone()), "building");
    assert!(spoken(&harness).is_empty(), "{:#?}", spoken(&harness));
}

/// Whether the harness career holds the predictive-cruise badge.
fn holds_predictive_crest(harness: &PlaytestHarness) -> bool {
    harness
        .app
        .ctx
        .profile
        .as_ref()
        .expect("a career")
        .achievements
        .iter()
        .any(|id| id == "predictive_crest")
}

/// One frame of the preview naming its phase, then the badge tracker, in
/// the order the drive frame runs them.
fn preview_then_badges(harness: &mut PlaytestHarness) {
    harness.with_drive(|d, ctx| {
        let bias = d.predictive_cruise_bias(ctx, 62.0);
        d.track_predictive_cruise(bias.0, bias.1);
        d.track_driving_badges(ctx, 1.0 / 60.0);
    });
}

#[test]
fn test_the_predictive_crest_badge_lands_when_cruise_builds_for_a_real_climb() {
    // A two percent pull is cued, but it would not have taken the speed off
    // the truck, so building for it earns nothing.
    let mut harness = cruising("Crest Shallow", 62.0, 200.0, &[(0.0, BENCH_MILES, 0.0)]);
    hill_road(&mut harness, 0.5, 0.02, 1.0);
    harness.with_drive(|d, _| d.truck_mut().grade = 0.0);
    harness.app.ctx.settings.predictive_cruise = true;
    preview_then_badges(&mut harness);
    assert_eq!(harness.read_drive(|d| d.pcc_phase.clone()), "building");
    assert!(!holds_predictive_crest(&harness));
    drop(harness);

    // A five percent one does, on the frame the preview starts building.
    let mut harness = cruising("Crest Steep", 62.0, 200.0, &[(0.0, BENCH_MILES, 0.0)]);
    hill_road(&mut harness, 0.5, 0.05, 1.0);
    harness.with_drive(|d, _| d.truck_mut().grade = 0.0);
    harness.with_drive(|d, ctx| d.track_driving_badges(ctx, 1.0 / 60.0));
    assert!(
        !holds_predictive_crest(&harness),
        "before the preview built"
    );
    harness.app.ctx.settings.predictive_cruise = true;
    preview_then_badges(&mut harness);
    assert!(holds_predictive_crest(&harness));
}

#[test]
fn test_predictive_cruise_finds_a_short_hill() {
    // A half-mile hill must not average away inside the preview.
    //
    // Averaging the whole preview, a half-mile four percent pull came out at
    // 1.3 percent -- under the threshold -- so the hills that gain the most
    // from banked momentum were exactly the ones the preview skipped (bench,
    // 2026-07-25). The windowed reading finds them.
    let mut harness = cruising("Short Hill", 62.0, 200.0, &[(0.0, BENCH_MILES, 0.0)]);
    hill_road(&mut harness, 0.3, 0.04, 0.5);
    harness.with_drive(|d, _| d.truck_mut().grade = 0.0);
    harness.app.ctx.settings.predictive_cruise = true;
    let (climb_ahead, _descent) = harness.read_drive(|d| d.grade_extremes_ahead());
    assert!(climb_ahead >= 0.03, "{climb_ahead}");
    assert!(harness.with_drive(|d, ctx| d.predictive_cruise_bias(ctx, 62.0).0) > 1.0);
}

#[test]
fn test_predictive_cruise_holds_at_a_crest_but_never_slows_the_truck() {
    // Near the top it stops reaching for speed; it does not give speed away.
    //
    // An earlier cut returned a flat four mph giveaway and cost a 2 percent
    // pull three mph it had been holding comfortably (bench, 2026-07-25).
    let mut harness = cruising("Crest", 62.0, 200.0, &[(0.0, BENCH_MILES, 0.0)]);
    let start = hill_road(&mut harness, 0.0, 0.04, 0.2);
    harness.app.ctx.settings.predictive_cruise = true;
    harness.with_drive(move |d, _| {
        d.trip.position_mi = start;
        d.truck_mut().grade = 0.04;
        d.truck_mut().velocity_mps = 55.0 * MPS_PER_MPH;
    });
    let bias = harness.with_drive(|d, ctx| d.predictive_cruise_bias(ctx, 62.0));
    assert!(bias.0 < 0.0, "{:?}", bias);
    // It brings the target down to the speed on the clock, no further.
    assert!(62.0 + bias.0 >= harness.read_drive(|d| d.truck().speed_mph()) - 0.01);
    assert!(bias.0 >= -PCC_CREST_SAG_MPH, "{:?}", bias);

    // A truck still holding its number at a crest is left alone.
    harness.with_drive(|d, _| d.truck_mut().velocity_mps = 62.0 * MPS_PER_MPH);
    assert!(approx(
        harness.with_drive(|d, ctx| d.predictive_cruise_bias(ctx, 62.0).0),
        0.0
    ));
}

#[test]
fn test_the_crest_hold_stays_silent() {
    // The crest hold eases nothing -- it only stops chasing the number -- so
    // "easing" was a lie (silence over redundant speech). It still tracks the
    // phase.
    let mut harness = cruising("Crest Quiet", 62.0, 200.0, &[(0.0, BENCH_MILES, 0.0)]);
    let start = hill_road(&mut harness, 0.0, 0.04, 0.2);
    harness.app.ctx.settings.predictive_cruise = true;
    harness.with_drive(move |d, _| {
        d.trip.position_mi = start;
        d.truck_mut().grade = 0.04;
        d.truck_mut().velocity_mps = 55.0 * MPS_PER_MPH;
    });
    harness.clear_speech();
    harness.with_drive(|d, ctx| {
        let (bias, reason) = d.predictive_cruise_bias(ctx, 62.0);
        d.track_predictive_cruise(bias, reason);
    });
    assert!(!said_any(&harness, "easing"), "{:#?}", spoken(&harness));
    assert_eq!(harness.read_drive(|d| d.pcc_phase.clone()), "holding");
}

#[test]
fn test_predictive_cruise_shaves_before_a_descent() {
    // Speed added just before a downgrade comes back out through the brakes.
    let mut harness = cruising("Shave Descent", 62.0, 200.0, &[(0.0, BENCH_MILES, 0.0)]);
    hill_road(&mut harness, 0.4, -0.05, 1.0);
    harness.with_drive(|d, _| d.truck_mut().grade = 0.0);
    harness.app.ctx.settings.predictive_cruise = true;
    assert!(harness.with_drive(|d, ctx| d.predictive_cruise_bias(ctx, 62.0).0) < 0.0);
    // It eases off in silence.
    harness.clear_speech();
    harness.with_drive(|d, ctx| {
        let (bias, reason) = d.predictive_cruise_bias(ctx, 62.0);
        d.track_predictive_cruise(bias, reason);
    });
    assert_eq!(harness.read_drive(|d| d.pcc_phase.clone()), "easing");
    assert!(spoken(&harness).is_empty(), "{:#?}", spoken(&harness));
}

#[test]
fn test_predictive_cruise_never_banks_past_the_posted_limit() {
    // Momentum for a hill is not a license to speed.
    let mut harness = cruising("Bank Limit", 55.0, 55.0, &[(0.0, BENCH_MILES, 0.0)]);
    harness.app.ctx.settings.predictive_cruise = true;
    let start = harness.read_drive(|d| d.trip.position_mi);
    harness.with_drive(move |d, _| {
        bench_road_segments(
            d,
            &[(0.0, 55.0)],
            &[
                (0.0, start + 0.5, 0.0),
                (start + 0.5, start + 1.5, 6.0),
                (start + 1.5, BENCH_MILES, 0.0),
            ],
            1.0,
        );
        d.trip.position_mi = start;
        d.truck_mut().grade = 0.0;
    });
    for _ in 0..240 {
        harness.advance_clock(DT);
        harness.with_drive(|d, ctx| {
            d.update_cruise(ctx, DT, false, false, false);
            d.truck_mut().update(DT);
        });
    }
    assert!(
        harness.read_drive(|d| d.truck().speed_mph()) <= 55.0 + ACC_LIMIT_OFFSET_MPH + 0.5,
        "{}",
        harness.read_drive(|d| d.truck().speed_mph())
    );
}

#[test]
fn test_cruise_says_when_a_climb_has_beaten_it() {
    // The climb side owes the driver the same honesty the descent side gives.
    //
    // Terse speech keeps it: the engine note and the downshifts already say
    // the truck is working, and a terse player asked for less.
    for (driving_speech, expected) in [("standard", true), ("quiet", false)] {
        let mut harness = cruising("Beaten Climb", 62.0, 200.0, &[(0.0, BENCH_MILES, 7.0)]);
        harness.app.ctx.settings.driving_speech = driving_speech.to_string();
        harness.clear_speech();
        for _ in 0..(90 * 60) {
            harness.advance_clock(DT);
            harness.with_drive(|d, ctx| {
                d.truck_mut().grade = 0.07;
                d.update_cruise(ctx, DT, false, false, false);
                if d.truck().transmission.automatic {
                    d.truck_mut().auto_shift();
                }
                d.truck_mut().update(DT);
            });
        }
        let said = spoken(&harness)
            .iter()
            .filter(|e| e.contains("Cruise down to"))
            .count();
        assert_eq!(said > 0, expected, "{driving_speech}: {said}");
        if expected {
            assert_eq!(said, 1); // once a hill, not once a second
        }
    }
}

#[test]
fn test_climb_cue_stays_quiet_when_cruise_is_winning() {
    // The ported dev guards (f23a97ec): a limit rise that jumps the target
    // well above current speed floors the throttle on near-level road -- that
    // is acceleration toward the number, not defeat, and it must stay silent
    // (71-and-climbing-to-77 was announced as losing the grade; playtest
    // transcript 2026-07-27).
    let mut harness = cruising("Winning Climb", 62.0, 200.0, &[(0.0, BENCH_MILES, 0.5)]);
    harness.app.ctx.settings.driving_speech = "standard".to_string();
    // The target sits well above the truck -- the limit-rise shape -- so
    // cruise floors the pedal while genuinely accelerating.
    harness.with_drive(|d, _| d.truck_mut().velocity_mps = 50.0 * MPS_PER_MPH);
    harness.clear_speech();
    for _ in 0..(30 * 60) {
        harness.advance_clock(DT);
        harness.with_drive(|d, ctx| {
            // Road the G key calls level: below the beaten-grade floor.
            d.truck_mut().grade = 0.005;
            d.update_cruise(ctx, DT, false, false, false);
            if d.truck().transmission.automatic {
                d.truck_mut().auto_shift();
            }
            d.truck_mut().update(DT);
        });
    }
    assert!(
        !said_any(&harness, "Cruise down to"),
        "{:#?}",
        spoken(&harness)
    );
}

#[test]
fn test_cruise_leaves_the_retarder_alone_when_descent_control_is_off() {
    // The stalk decides. The drums still hold the speed either way.
    //
    // Turning descent control off is the driver saying they manage grades
    // themselves, and a real truck's cruise does not flip the engine brake on
    // for you. It must cost the quiet retarder, never the ability to hold the
    // set speed -- that was the runaway this whole area started with.
    let mut harness = cruising("Descent Off", 62.0, 200.0, &[(0.0, BENCH_MILES, -6.0)]);
    harness.app.ctx.settings.descent_speed_control = "off".to_string();
    let mut speeds = Vec::new();
    for _ in 0..(60 * 60) {
        harness.advance_clock(DT);
        let speed = harness.with_drive(|d, ctx| {
            d.truck_mut().grade = -0.06;
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
            d.truck().speed_mph()
        });
        speeds.push(speed);
    }
    assert_eq!(harness.read_drive(|d| d.cruise_jake_stage), 0);
    assert_eq!(harness.read_drive(|d| d.truck().engine_brake_stage), 0);
    assert!(max_of(&speeds) <= 68.0, "{}", max_of(&speeds));
    assert!(!harness.read_drive(|d| d.truck().air_brakes_holding()));
}

// -- the driver's own brake on a grade -----------------------------------------

#[test]
fn test_braking_on_a_grade_caps_it_without_rewriting_the_set_speed() {
    // The same correction the interactive path already carries.
    //
    // Brandon drove a whole run pinned at "forty nine mph or lower and losing
    // speed instead of getting back up to highway speed" (2026-08-23).
    // Braking on a downgrade assigned straight into the cruise set speed, so
    // it was permanent AND cumulative: 65 becomes 55 on one hill, 49 on the
    // next, and cruise never climbs back on the flat because 49 IS the set
    // speed by then. A ratchet that only ever turns down.
    //
    // `test_interactive_descent_control_caps_the_target_without_rewriting_it`
    // above pins the identical rule one branch over. This one was missed out
    // of it.
    let mut harness = bench_drive("Grade Brake Ratchet", 200.0, 0.0);
    harness.app.ctx.settings.descent_speed_control = "balanced".to_string();
    harness.with_drive(|d, _| {
        quiet(&mut d.trip);
        d.truck_mut().start_engine();
        d.truck_mut().velocity_mps = 65.0 / MPH_PER_MPS;
        d.cruise_mph = Some(65.0);
        d.cruise_working_mph = Some(65.0);
    });

    // Two hills, braking lower on the second one.
    for slowed_to in [55.0_f64, 49.0] {
        harness.with_drive(move |d, ctx| {
            d.truck_mut().grade = -0.05;
            d.truck_mut().velocity_mps = slowed_to / MPH_PER_MPS;
            d.update_cruise(ctx, DT, true, false, false);
        });
        // The driver's number is untouched, and the grade carries the cap.
        assert!(approx(
            harness.read_drive(|d| d.cruise_mph).expect("cruise"),
            65.0
        ));
        assert!(approx_abs(
            harness
                .read_drive(|d| d.cruise_descent_mph)
                .expect("a grade cap"),
            slowed_to,
            0.01
        ));
    }

    // Back on the level: the cap lifts and highway speed comes back.
    harness.with_drive(|d, _| {
        bench_road_segments(d, &[(0.0, 200.0)], &[(0.0, BENCH_MILES, 0.0)], 1.0);
        d.truck_mut().grade = 0.0;
    });
    for _ in 0..600 {
        harness.with_drive(|d, ctx| {
            d.truck_mut().grade = 0.0;
            d.update_cruise(ctx, DT, false, false, false);
        });
    }
    assert!(harness.read_drive(|d| d.cruise_descent_mph).is_none());
    assert!(approx(
        harness.read_drive(|d| d.cruise_mph).expect("cruise"),
        65.0
    ));
    assert!(approx(
        harness
            .read_drive(|d| d.cruise_working_mph)
            .expect("a working target"),
        65.0
    ));
}

#[test]
fn test_the_automatic_grade_cap_never_undoes_the_drivers_own_brake() {
    // Capture is an instruction; the automatic ceiling must not raise it.
    //
    // Both write `cruise_descent_mph`, and on the frame after a deliberate
    // brake the interactive ceiling would otherwise hand the speed straight
    // back.
    let mut harness = bench_drive("Grade Brake Instruction", 200.0, 0.0);
    harness.app.ctx.settings.descent_speed_control = "interactive".to_string();
    harness.with_drive(|d, _| {
        quiet(&mut d.trip);
        d.truck_mut().start_engine();
        d.cruise_mph = Some(65.0);
        d.cruise_working_mph = Some(65.0);
    });

    let slowed_to = DESCENT_SAFE_MAX_MPH - 10.0;
    harness.with_drive(move |d, ctx| {
        d.truck_mut().grade = -0.05;
        d.truck_mut().velocity_mps = slowed_to / MPH_PER_MPS;
        d.update_cruise(ctx, DT, true, false, false);
    });
    assert!(approx_abs(
        harness
            .read_drive(|d| d.cruise_descent_mph)
            .expect("a grade cap"),
        slowed_to,
        0.01
    ));

    // Still on the grade, no brake: the automatic ceiling runs and must
    // leave the lower, deliberate cap alone.
    for _ in 0..60 {
        harness.with_drive(|d, ctx| {
            d.truck_mut().grade = -0.05;
            d.update_cruise(ctx, DT, false, false, false);
        });
    }
    assert!(harness.read_drive(|d| d.cruise_descent_mph).expect("a cap") <= slowed_to + 0.01);
    assert!(approx(
        harness.read_drive(|d| d.cruise_mph).expect("cruise"),
        65.0
    ));
}

#[test]
fn test_the_status_readout_names_the_grade_cap_and_leaves_the_set_speed_alone() {
    // The two halves of the grade fix have to compose.
    //
    // The cap is only honest if the driver can hear it as a cap: Space and Tab
    // read what the loop published after every ceiling, so a grade brake must
    // come back as the HELD number with the driver's own set speed still
    // beside it. Before the fix the brake rewrote `cruise_mph`, so the two
    // numbers were the same one and the readout fell through to "adaptive
    // cruise set at 55" -- the truck changing its own target and saying
    // nothing about it.
    let mut harness = bench_drive("Grade Cap Readout", 200.0, 0.0);
    harness.app.ctx.settings.descent_speed_control = "balanced".to_string();
    harness.with_drive(|d, _| {
        quiet(&mut d.trip);
        d.truck_mut().start_engine();
        d.cruise_mph = Some(65.0);
        d.cruise_working_mph = Some(65.0);
    });

    // The driver brakes on the grade, then comes off the pedal still on it.
    harness.with_drive(|d, ctx| {
        d.truck_mut().grade = -0.05;
        d.truck_mut().velocity_mps = 55.0 / MPH_PER_MPS;
        d.update_cruise(ctx, DT, true, false, false);
    });
    harness.with_drive(|d, ctx| {
        d.truck_mut().grade = -0.05;
        d.update_cruise(ctx, DT, false, false, false);
    });

    let readout = harness.with_drive(|d, ctx| d.cruise_holding_text(ctx));
    assert_eq!(
        readout, "adaptive cruise holding 55 miles per hour for the grade, set 65 miles per hour",
        "{readout}"
    );
    // And the driver's own number is still the set speed, not the cap.
    assert!(approx(
        harness.read_drive(|d| d.cruise_mph).expect("cruise"),
        65.0
    ));
}

#[test]
fn test_g_announces_upcoming_grade_without_nothing_steep() {
    for grade in [0.015, -0.015, 0.037, -0.037] {
        let mut harness = cruising("G Upcoming Grade", 62.0, 200.0, &[(0.0, BENCH_MILES, 0.0)]);
        hill_road(&mut harness, 1.0, grade, 0.5);
        harness.with_drive(|d, _| d.truck_mut().grade = -0.014);
        harness.clear_speech();
        harness.with_drive(|d, ctx| d.speak_grade(ctx));
        let direction = if grade > 0.0 {
            "uphill grade"
        } else {
            "downhill grade"
        };
        assert!(said_any(&harness, direction), "{:?}", spoken(&harness));
        assert!(said_any(&harness, "Grade 1.4 percent downhill"));
        assert!(!said_any(&harness, "Nothing"), "{:?}", spoken(&harness));
    }
}

#[test]
fn test_interactive_descent_brake_does_not_pump_on_its_edge() {
    // Interactive descent control's brake used to switch straight to a third
    // of the pedal at eight over its safe ceiling. With the automatic
    // retarder manager in charge (no cruise snub), a grade the jake cannot
    // hold parked the truck on that edge and the pedal pumped: every new
    // application costs air (the pedal-fanning class, 2026-09-24).
    //
    // Since 2026-09-24 cruise also snubs under the retarder manager (it used
    // to leave the drums alone, and a loaded truck ran from 45 to 55 down a
    // seven percent grade), so the brake now comes in real applications: a
    // snub, a release, another snub. What must never happen is the flutter
    // -- an application that lets go a frame or two after it started.
    let mut harness = cruising("Descent Edge", 62.0, 200.0, &[(0.0, BENCH_MILES, -8.0)]);
    harness.app.ctx.settings.descent_speed_control = "interactive".to_string();
    harness.with_drive(|d, _| d.auto_jake = true);
    let mut top = 0.0f64;
    let mut hold = f64::INFINITY;
    let mut applying = false;
    let mut on_frames = 0usize;
    let mut shortest = usize::MAX;
    let mut lowest_air = f64::INFINITY;
    for _ in 0..(60 * 90) {
        harness.advance_clock(DT);
        let (brake, speed, held, air) = harness.with_drive(|d, ctx| {
            d.truck_mut().grade = -0.08;
            d.truck_mut().brake = 0.0; // the pedal is the controller's alone
            d.update_cruise(ctx, DT, false, false, false);
            d.update_auto_jake(ctx, DT);
            d.truck_mut().auto_shift();
            d.truck_mut().update(DT);
            (
                d.truck().brake,
                d.truck().speed_mph(),
                d.descent_hold_mph(),
                d.truck().air_pressure_psi(),
            )
        });
        let on = brake > 0.01;
        if on {
            on_frames += 1;
        } else if applying {
            shortest = shortest.min(on_frames);
            on_frames = 0;
        }
        applying = on;
        top = top.max(speed);
        hold = hold.min(held);
        lowest_air = lowest_air.min(air);
    }
    assert!(
        top > hold + 7.0,
        "the grade never beat the retarder ({top:.1} mph against {hold:.1}), so the edge was \
         never reached"
    );
    assert!(
        shortest >= (0.25 / DT) as usize,
        "an application let go after {shortest} frames: the pedal pumped"
    );
    assert!(
        lowest_air >= 90.0,
        "the air ran down to {lowest_air:.0} psi"
    );
}

#[test]
fn test_g_keeps_clear_road_report_when_no_grade_is_ahead() {
    let mut harness = cruising("G Clear Road", 62.0, 200.0, &[(0.0, BENCH_MILES, 0.0)]);
    harness.clear_speech();
    harness.with_drive(|d, ctx| d.speak_grade(ctx));
    assert!(
        said_any(&harness, "Nothing steep in the next"),
        "{:?}",
        spoken(&harness)
    );
}

#[test]
fn test_g_under_a_mile_from_the_end_says_nothing_steep_ahead() {
    // The streets to a gate are under a mile of road: whole miles said
    // "Nothing steep in the next 0 miles" (live drive into Abilene,
    // 2026-09-24).
    let mut harness = cruising("G Last Streets", 20.0, 30.0, &[(0.0, BENCH_MILES, 0.0)]);
    harness.clear_speech();
    harness.with_drive(|d, ctx| {
        d.trip.position_mi = d.trip.total_miles() - 0.4;
        d.speak_grade(ctx);
    });
    assert!(
        said_any(&harness, "Nothing steep ahead."),
        "{:?}",
        spoken(&harness)
    );
    assert!(!said_any(&harness, "0 miles"), "{:?}", spoken(&harness));
}
