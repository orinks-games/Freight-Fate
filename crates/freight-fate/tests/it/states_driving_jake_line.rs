//! WHERE the retarder's line is, measured against the drums rather than argued.
//!
//! Owner report, 2026-08-24: "The jake activates on every single descent it
//! seems, even shallow descent like 1-3 percent. It should only activate on
//! descents that are steep where service brakes can't handle, like 6%+. That's
//! just a number I threw out there."
//!
//! Six percent was flagged as a guess and is treated as one here. So was the
//! two percent that shipped -- see `JAKE_ZONE_EXEMPT_GRADE_PCT`, which is the
//! release edge of a SPEECH hysteresis pair and never had a brake in it.
//! `DrivingState::retarder_warranted` derives the answer instead, from FHWA's
//! Grade Severity Rating System criterion (grade, length and gross weight
//! against a drum-temperature ceiling) evaluated on the truck's own brake heat
//! model:
//!
//! ```text
//!   T_settle = T_ambient + P_brake / (C * k(v))     >=  fade onset ?
//! ```
//!
//! This file is what stops that being a nice argument. It drives real frames
//! down real sustained grades with NO retarder available at all, records how
//! hot the drums actually get, and checks two things:
//!
//! 1. the closed form predicts the temperature the simulated drums reach, and
//! 2. `retarder_warranted` says yes for exactly the grades that cook them.
//!
//! Run `cargo test -p freight-fate --test it jake_line -- --nocapture` to read
//! the table.

use ff_core::data::world_models::{CorridorDetail, GradeSegment, Leg, Route, SpeedLimitSample};
use ff_core::sim::trip::{Trip, TripOptions};
use ff_core::sim::vehicle::{
    AIR_DENSITY, AMBIENT_C, BRAKE_COOL_BASE_PER_S, BRAKE_COOL_SPEED_PER_S, REFERENCE_CARGO_KG,
};
use ff_core::sim::weather::{WeatherKind, WeatherSystem};
use freight_fate::playtest::harness::PlaytestHarness;
use freight_fate::states::driving::DrivingState;

use crate::transcript_cruise_support::{
    frame, quiet, release_keys, spoken, start_drive, BENCH_MILES, DT, MPS_PER_MPH, START_MI,
};

/// One straight road with one sustained grade on it, seeded and weather-pinned
/// the way the jake sweep pins its roads: an unseeded delivery draws its own
/// route and its own sky, and a calibration that let the draw decide what it
/// measured would be measuring nothing.
fn one_grade_road(drive: &mut DrivingState, limit_mph: f64, grade_pct: f64, run_mi: f64) {
    let city = drive.trip.route.cities[0].clone();
    let detail = CorridorDetail {
        speed_limits: vec![SpeedLimitSample {
            at_mi: 0.0,
            mph: Some(limit_mph),
            source: "jake line".to_string(),
            hgv: false,
        }],
        grade_segments: vec![
            GradeSegment::new(0.0, START_MI + 1.0, 0.0, "flat", "jake line"),
            GradeSegment::new(
                START_MI + 1.0,
                START_MI + 1.0 + run_mi,
                grade_pct,
                if grade_pct.abs() >= 3.0 {
                    "mountain"
                } else {
                    "flat"
                },
                "jake line",
            ),
            GradeSegment::new(
                START_MI + 1.0 + run_mi,
                BENCH_MILES,
                0.0,
                "flat",
                "jake line",
            ),
        ],
        ..Default::default()
    };
    let leg = Leg::new(&city, &city, BENCH_MILES, "I 90", "flat", Vec::new()).with_detail(detail);
    let route = Route::from_legs(vec![city.clone(), city], vec![leg]);
    let truck = drive.trip.truck.clone();
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
    // A dispatched trip arrives with real zones and real bends on it. Left
    // alone they hold the truck at a construction limit and the run measures
    // that instead of the grade.
    trip.zones.clear();
    trip.curves.clear();
    trip.set_patrols(Vec::new());
    drive.trip = trip;
    drive.reset_turn_state_for_trip();
    drive.destination_exit_taken = true;
    drive.trip.position_mi = START_MI;
}

/// A loaded rig at highway speed on a sustained grade, every assist in its
/// shipped state unless `retarder` turns the stalk off.
fn loaded_run(
    name: &str,
    grade_pct: f64,
    set_mph: f64,
    run_mi: f64,
    retarder: bool,
) -> PlaytestHarness {
    let mut harness = start_drive(name);
    harness.app.ctx.settings.time_scale = 1.0;
    harness.app.ctx.settings.automatic_transmission = true;
    harness.app.ctx.settings.curve_speed_assist = true;
    harness.app.ctx.settings.descent_speed_control = if retarder {
        "realistic".to_string()
    } else {
        "off".to_string()
    };
    harness.app.ctx.settings.speed_keeper = true;
    // Nobody is at the wheel for fifteen minutes, and lane wander runs off a
    // seed the drive draws fresh when nobody hands it one -- so it was the one
    // thing here that differed every run. Left in, the truck drifted onto the
    // shoulder, took damage for every second it stayed there, went into limp
    // mode and then out of service, and the "hold" being measured was a wreck
    // coasting to a stop: that is what put 20 mph rows in the table and
    // flipped the verdict on about two runs in five. Full lane keeping is how
    // the break scenarios take steering out of an experiment that is not about
    // steering, and it is what makes this table repeat exactly.
    harness.app.ctx.settings.lane_keeping = "full".to_string();
    release_keys(&mut harness);
    harness.with_drive(move |d, _| {
        one_grade_road(d, 65.0, grade_pct, run_mi);
        // The hill has to be the ONLY thing acting on the truck. Traffic and
        // patrols are drawn per run, and a truck that catches a slower vehicle
        // holds ITS speed down the grade instead of the set speed -- which is
        // a different experiment, and an unrepeatable one. Left in, this read
        // as 20 mph held on a one percent downgrade, moved which grades
        // "settled" from run to run, and failed roughly two runs in five on
        // unchanged code.
        d.trip.set_npc_vehicles(Vec::new());
        d.trip.set_patrols(Vec::new());
        d.weather_mut().forced = Some(WeatherKind::Clear);
        d.weather_mut().current = WeatherKind::Clear;
        d.truck_mut().transmission.automatic = true;
        // Grossed out: `specs.mass_kg` IS the rated gross, so a reference load
        // puts the truck at the eighty thousand pounds the derivation quotes.
        d.truck_mut().cargo_kg = REFERENCE_CARGO_KG;
        d.truck_mut().start_engine();
        d.truck_mut().set_air_ready(false);
        d.truck_mut().velocity_mps = set_mph * MPS_PER_MPH;
        d.truck_mut().transmission.gear = d.truck().transmission.num_gears();
    });
    harness.with_drive(move |d, ctx| d.engage_cruise(ctx, set_mph, false));
    harness
}

/// Standard gravity, the same number the vehicle model uses.
const G_MPS2: f64 = 9.81;

/// The closed form the predicate is built on, written out a second time HERE
/// so the test is checking the rule and not just re-running the code under it.
fn predicted_settle_c(hold_force_n: f64, speed_mps: f64, thermal_mass: f64) -> f64 {
    let k = BRAKE_COOL_BASE_PER_S + BRAKE_COOL_SPEED_PER_S * speed_mps.sqrt();
    AMBIENT_C + hold_force_n * speed_mps / (thermal_mass * k)
}

/// Drive until the wheels are actually on the grade, which starts a mile
/// ahead of where the truck is put down. Asking the predicate on the flat
/// approach answers about the flat.
fn roll_onto_the_grade(harness: &mut PlaytestHarness) {
    for _ in 0..(150.0 / DT) as usize {
        frame(harness, DT);
        if harness.with_drive(|d, _| d.on_downgrade()) {
            // `Trip::update` writes `truck.grade` from the mile the truck was
            // at when the frame STARTED, so on the crossing frame the geometry
            // already says descent and the truck's own forces still say flat.
            // A second of road settles it; nothing on a grade that has to run
            // three quarters of a mile turns on that one frame.
            for _ in 0..60 {
                frame(harness, DT);
            }
            return;
        }
    }
    panic!("the truck never reached the grade");
}

struct Held {
    grade_pct: f64,
    /// Damage taken during the hold. A hill does not damage a truck, so
    /// anything above zero means something else got into the experiment.
    damage_pct: f64,
    peak_temp_c: f64,
    settled_temp_c: f64,
    predicted_c: f64,
    fade_onset_c: f64,
    mean_mph: f64,
    /// Spread of the held speed over the settled window. The closed form is a
    /// STEADY-state formula, so it is only fair to check it against a run that
    /// was actually steady -- and a run that was not is worth seeing.
    spread_mph: f64,
    set_mph: f64,
    warranted: bool,
    retarder_frames: usize,
}

impl Held {
    /// Did the truck really hold one speed down this hill? Cruise snubs in
    /// cycles and a hill it cannot hold walks the speed away entirely.
    fn steady(&self) -> bool {
        (self.mean_mph - self.set_mph).abs() <= 3.0 && self.spread_mph <= 6.0
    }
}

/// Hold a grade on the DRUMS alone for `seconds` and watch the drums.
fn hold_on_the_drums(grade_pct: f64, set_mph: f64, seconds: f64) -> Held {
    // The road has to be longer than the truck can drive in the time, or the
    // grade runs out mid-measurement and the drums start cooling.
    let run_mi = set_mph * seconds / 3600.0 + 2.0;
    let mut harness = loaded_run("Drums", grade_pct, set_mph, run_mi, false);
    let mut warranted = false;
    let mut peak = 0.0f64;
    let mut speeds = Vec::new();
    let mut retarder_frames = 0usize;
    let total = (seconds / DT) as usize;
    for index in 0..total {
        frame(&mut harness, DT);
        let (temp, mph, stage, on_grade, wants) = harness.with_drive(|d, _| {
            (
                d.truck().brake_temp_c,
                d.truck().speed_mph(),
                d.truck().engine_brake_stage,
                d.on_downgrade(),
                d.retarder_warranted(),
            )
        });
        peak = peak.max(temp);
        // Asked ON the grade, never on the flat mile the truck starts on, and
        // never once: the answer is what the assists would have seen frame by
        // frame all the way down.
        if on_grade {
            warranted |= wants;
        }
        // The settled window: the last two thirds, once the drums and the
        // controller have both stopped chasing the top of the hill.
        if index > total / 3 {
            speeds.push(mph);
        }
        if stage > 0 {
            retarder_frames += 1;
        }
    }
    let damage_pct = harness.read_drive(|d| d.truck().damage_pct);
    let mean_mph = speeds.iter().sum::<f64>() / speeds.len().max(1) as f64;
    let spread_mph = speeds.iter().cloned().fold(f64::NEG_INFINITY, f64::max)
        - speeds.iter().cloned().fold(f64::INFINITY, f64::min);
    let (settled, predicted, onset) = harness.read_drive(|d| {
        let t = d.truck();
        // Resistance at the MEAN held speed, written out here rather than read
        // off `resistance_force()`: the truck snubs in cycles, so one frame's
        // instantaneous velocity is a poor stand-in for the speed the hill was
        // actually held at.
        let v = mean_mph * MPS_PER_MPH;
        let gross = t.gross_mass_kg();
        let roll = gross * G_MPS2 * t.specs.rolling_resistance;
        let drag = 0.5 * AIR_DENSITY * t.specs.drag_coefficient * t.specs.frontal_area_m2 * v * v;
        let gravity = gross * G_MPS2 * (grade_pct / 100.0).atan().sin().abs();
        (
            t.brake_temp_c,
            predicted_settle_c(
                (gravity - roll - drag).max(0.0),
                v,
                t.specs.brake_thermal_mass_j_per_c,
            ),
            t.brake_fade_onset_c(),
        )
    });
    Held {
        grade_pct,
        damage_pct,
        peak_temp_c: peak,
        settled_temp_c: settled,
        predicted_c: predicted,
        fade_onset_c: onset,
        mean_mph,
        spread_mph,
        set_mph,
        warranted,
        retarder_frames,
    }
}

/// Fifteen minutes: the drums' time constant at highway speed is a shade under
/// five, so this is three of them -- past 95 percent of wherever the grade is
/// taking them, which is enough to tell "settles below fade" from "on its way
/// through it".
const HOLD_SECONDS: f64 = 900.0;

#[test]
#[cfg_attr(
    not(feature = "slow-tests"),
    ignore = "sweep: a grid of grades and speeds held on the drums"
)]
fn test_the_retarder_line_is_where_the_drums_stop_holding() {
    let held: Vec<Held> = [1.0, 2.0, 3.0, 4.0, 4.5, 5.0, 6.0]
        .iter()
        .map(|pct| hold_on_the_drums(-pct, 55.0, HOLD_SECONDS))
        .collect();

    println!(
        "
== eighty thousand pounds, 55 mph set, fifteen minutes of grade, NO retarder =="
    );
    println!(
        "{:>7} {:>10} {:>9} {:>10} {:>11} {:>10} {:>9} {:>11}",
        "grade",
        "held mph",
        "spread",
        "peak degC",
        "settled C",
        "predicted",
        "fade at",
        "warranted?"
    );
    for h in &held {
        println!(
            "{:>6.1}% {:>10.1} {:>9.1} {:>10.0} {:>11.0} {:>10.0} {:>9.0} {:>11}{}",
            h.grade_pct,
            h.mean_mph,
            h.spread_mph,
            h.peak_temp_c,
            h.settled_temp_c,
            h.predicted_c,
            h.fade_onset_c,
            if h.warranted { "yes" } else { "no" },
            if h.steady() {
                ""
            } else {
                "   (speed never settled)"
            },
        );
    }
    println!();

    // The rig has to be measuring drums and not a retarder that crept in.
    for h in &held {
        assert_eq!(
            h.retarder_frames, 0,
            "{:.1}% ran with the retarder up, so the drum heat is not the drums'",
            h.grade_pct
        );
    }

    // And it has to be measuring a truck, not a wreck. A grade does not
    // damage a rig; damage here means something outside the experiment --
    // the shoulder, a barrier, traffic -- got hold of the truck partway
    // down, and every number in its row is about that instead of the hill.
    for h in &held {
        assert_eq!(
            h.damage_pct, 0.0,
            "{:.1}% took {:.0} percent damage on the way down, so the hold is not a hold",
            h.grade_pct, h.damage_pct
        );
    }

    // 1. THE CLOSED FORM PREDICTS THE SIMULATION. Below fade, on a hill the
    //    truck genuinely held at one speed, the drums settle where the
    //    arithmetic says they will. That is what licenses using the arithmetic
    //    as a gate instead of waiting for the heat to arrive.
    //
    //    Restricted to the steady runs BELOW fade, on purpose and for two
    //    reasons. It is a steady-state formula, so a run where the speed walked
    //    away (a loaded automatic downshifting itself into the governor on the
    //    steepest hills) is not a question it answers. And past fade the shoes
    //    stop making the force the formula assumes, so the prediction there is
    //    an extrapolation whose only job is to be ABOVE the onset -- which is
    //    what the separation check below tests, by measured heat.
    let checked: Vec<&Held> = held
        .iter()
        .filter(|h| h.steady() && h.settled_temp_c < h.fade_onset_c)
        .collect();
    assert!(
        checked.len() >= 3,
        "only {} steady runs below fade to check the closed form against",
        checked.len()
    );
    for h in checked {
        assert!(
            (h.settled_temp_c - h.predicted_c).abs() <= 25.0,
            "{:.1}%: the drums settled at {:.0} C and the rule predicted {:.0} C",
            h.grade_pct,
            h.settled_temp_c,
            h.predicted_c
        );
    }

    // 2. THE PREDICATE SEPARATES. Warranted exactly where the drums reach the
    //    temperature at which they stop answering, and nowhere else. This is
    //    the whole claim, and it is checked against measured heat rather than
    //    against the number the predicate itself used.
    for h in &held {
        let cooked = h.peak_temp_c >= h.fade_onset_c;
        assert_eq!(
            h.warranted,
            cooked,
            "{:.1}%: retarder_warranted said {} and the drums {} (peak {:.0} C, fade at {:.0} C)",
            h.grade_pct,
            h.warranted,
            if cooked { "faded" } else { "held" },
            h.peak_temp_c,
            h.fade_onset_c
        );
    }

    // 3. The line is a CURVE IN WEIGHT, not the constant it replaced, and it
    //    lands between the two percent that shipped and the six percent the
    //    report guessed -- so neither guess was the answer.
    let shallowest_warranted = held
        .iter()
        .filter(|h| h.warranted)
        .map(|h| h.grade_pct.abs())
        .fold(f64::INFINITY, f64::min);
    assert!(
        shallowest_warranted > 2.0 && shallowest_warranted < 6.0,
        "the line landed at {shallowest_warranted:.1}%, outside the two-to-six window the \
         measurement puts it in"
    );
}

#[test]
fn test_an_empty_truck_reaches_for_the_retarder_far_later_than_a_loaded_one() {
    // The derivation says the line is a function of WEIGHT above all -- 4.2
    // percent grossed out, past nine percent empty -- and an empty rig
    // genuinely does not need the retarder. A rule that is really a hidden
    // constant would answer the same for both.
    // One TestApp to a thread -- it pins the save directory -- so these run in
    // sequence and the first is dropped before the second is built.
    let loaded = {
        let mut harness = loaded_run("Loaded", -5.0, 55.0, 30.0, true);
        roll_onto_the_grade(&mut harness);
        harness.with_drive(|d, _| d.retarder_warranted())
    };
    let empty = {
        let mut harness = loaded_run("Empty", -5.0, 55.0, 30.0, true);
        harness.with_drive(|d, _| d.truck_mut().cargo_kg = 0.0);
        roll_onto_the_grade(&mut harness);
        harness.with_drive(|d, _| d.retarder_warranted())
    };
    assert!(
        loaded,
        "a grossed-out truck on a five percent descent wants the retarder"
    );
    assert!(!empty, "an empty truck on a five percent descent does not");
}

#[test]
fn test_a_quarter_mile_pitch_is_not_a_sustained_descent() {
    // Steep enough that the drums could never hold it forever, short enough
    // that they never get anywhere near fade. The retarder is for SUSTAINED
    // speed control (Jacobs), so a pitch is not its road.
    let brief = {
        let mut harness = loaded_run("Pitch", -7.0, 55.0, 0.25, true);
        roll_onto_the_grade(&mut harness);
        harness.with_drive(|d, _| d.retarder_warranted())
    };
    let hill = {
        let mut harness = loaded_run("Hill", -7.0, 55.0, 6.0, true);
        roll_onto_the_grade(&mut harness);
        harness.with_drive(|d, _| d.retarder_warranted())
    };
    assert!(
        !brief,
        "a quarter mile of seven percent is a dip, not a grade to set up for"
    );
    assert!(
        hill,
        "six miles of seven percent is exactly the grade the retarder is for"
    );
}

#[test]
fn test_a_shallow_descent_is_still_held_and_not_restated() {
    // The fix must not trade a barking retarder for a truck that runs away
    // quietly. On a three percent grade -- squarely inside what the owner
    // reported as over-served -- the retarder stays out of it and the DRUMS
    // keep the number.
    //
    // What descent control does NOT do any more is announce the driver's own
    // set speed back to them. This line used to require "Descent control
    // holding 60" here; since 2026-09-25 descent control names only a number
    // of its own -- the hill's safe descent speed, interactive's ceiling, a
    // brake's capture -- because the set speed, or the limit plus five, was
    // exactly what it said on I-70's seven percent while the hill needed 45
    // (owner's drive into Denver, 2026-09-24), and a three percent grade
    // needs no number of its own (silence over redundant speech, 2026-09-21).
    let set_mph = 60.0;
    let mut harness = loaded_run("Shallow", -3.0, set_mph, 30.0, true);
    let mut retarder_frames = 0usize;
    let mut peak_mph = 0.0f64;
    let mut settled_mph = Vec::new();
    let total = (180.0 / DT) as usize;
    for index in 0..total {
        frame(&mut harness, DT);
        let (stage, mph) =
            harness.read_drive(|d| (d.truck().engine_brake_stage, d.truck().speed_mph()));
        if stage > 0 {
            retarder_frames += 1;
        }
        peak_mph = peak_mph.max(mph);
        if index > total / 3 {
            settled_mph.push(mph);
        }
    }

    assert_eq!(
        retarder_frames, 0,
        "the retarder came up on a three percent descent, which is the report"
    );
    // Held, not run away: a real downgrade the drums own still gets held to
    // the driver's number. The tolerance is cruise's own snub deadband, not a
    // number chosen to pass.
    assert!(
        peak_mph <= set_mph + 5.0,
        "the truck ran to {peak_mph:.1} mph on a three percent descent with the retarder out of it"
    );
    let low = settled_mph.iter().cloned().fold(f64::INFINITY, f64::min);
    assert!(
        low >= set_mph - 6.0,
        "the truck sagged to {low:.1} mph holding a three percent descent on the drums"
    );

    // Held at the driver's own number, so nothing to say about it.
    assert_eq!(
        harness.read_drive(|d| (d.descent_hold_mph(), d.descent_safe_mph)),
        (set_mph, None),
        "a three percent grade has no safe descent speed of its own"
    );
    let lines = spoken(&harness);
    assert!(
        !lines
            .iter()
            .any(|line| line.contains("Descent control holding")),
        "descent control restated the set speed; heard: {:#?}",
        lines
            .iter()
            .filter(|l| l.contains("escent"))
            .collect::<Vec<_>>()
    );
}

#[test]
fn test_a_jake_the_driver_armed_is_never_taken_away() {
    // The rule that kept auto mode's overspeed snub: an explicit choice is not
    // an assist's to overrule. A driver who reached for the stalk on a shallow
    // grade has asked for the retarder, and the new gate decides only what the
    // ASSISTS may raise.
    let mut harness = loaded_run("Stalk", -3.0, 60.0, 30.0, true);
    harness.with_drive(|d, _| d.truck_mut().engine_brake_stage = 3);
    let mut lowest = 3;
    let mut barking_frames = 0usize;
    for _ in 0..(120.0 / DT) as usize {
        frame(&mut harness, DT);
        let (stage, barking) = harness.read_drive(|d| {
            (
                d.truck().engine_brake_stage,
                d.truck().jake_retard_torque_nm() > 0.0,
            )
        });
        lowest = lowest.min(stage);
        if barking {
            barking_frames += 1;
        }
    }
    assert_eq!(
        lowest, 3,
        "an assist dropped the driver's own engine brake to stage {lowest} on a shallow grade"
    );
    assert!(
        barking_frames > 0,
        "the driver's own engine brake never actually retarded"
    );
}

#[test]
fn test_auto_mode_still_manages_the_stage_on_a_shallow_grade() {
    // Auto mode is the driver's own retarder manager, armed with J. It is NOT
    // gated on the new line -- pressing J is the explicit choice -- so it must
    // still step the stage against an overspeed on a grade too shallow for any
    // assist to reach for.
    let mut harness = loaded_run("AutoJ", -3.0, 55.0, 30.0, true);
    harness.with_drive(|d, _| {
        d.auto_jake = true;
        d.auto_jake_hold_mph = Some(55.0);
        d.auto_jake_cooldown_s = 0.0;
        d.truck_mut().engine_brake_stage = 1;
    });
    let mut top_stage = 0;
    for _ in 0..(90.0 / DT) as usize {
        frame(&mut harness, DT);
        top_stage = top_stage.max(harness.read_drive(|d| d.truck().engine_brake_stage));
    }
    assert!(
        top_stage >= 1,
        "auto mode stopped managing the retarder on a shallow grade; top stage {top_stage}"
    );
}
