//! "Descent control cannot hold this grade" -- said only where it is true.
//!
//! Owner, live playtest, I-70 west of Vail, 2026-08-24. Three of these inside
//! one minute, and the G key answered in between:
//!
//! ```text
//!   00:05:23  Level road. Nothing steep in the next 15 miles, but a 2.2
//!             percent downgrade starts in a quarter mile.
//!   00:05:25  [event] Descent control cannot hold this grade. Apply service
//!             brakes.
//!   00:05:44  Level road. Nothing steep in the next 15 miles.
//! ```
//!
//! Two statements about the same piece of road that cannot both be true, and
//! the one that costs the drive is the warning: it tells a blind driver to get
//! on the brakes on a road the truck's own instrument calls level.
//!
//! What was actually happening, measured on his own route in
//! [`the_warning_stays_off_the_owners_i70_run`] before the fix: eight
//! warnings between Silverthorne and Glenwood Springs, EVERY one of them while
//! the drums were applied between 0.44 and 0.70 and the truck was losing three
//! to five miles an hour a second, on dips a quarter of a mile long. The
//! sentence fired on a single frame's arithmetic -- speed more than ten over
//! the ceiling interactive descent control had just imposed
//! ([`DESCENT_SAFE_MAX_MPH`], 55) -- which on a 75 mph road with cruise set at
//! 80 is true on the FIRST frame of every dip, before the control has done
//! anything at all, and while it is about to do it successfully.
//!
//! So the cases below pin the three things that make the sentence honest, and
//! the fourth that keeps it from being merely quiet:
//!
//! * a hill that genuinely beats the control still says so, in these exact
//!   words;
//! * a dip the drums hold says nothing;
//! * the G readout and the descent control never disagree about one moment of
//!   road, because they now ask the same physics the same question
//!   (`TruckState::net_accel_mph_per_s` against `GRADE_HOLDING_MPH_PER_S`);
//! * and the retarder still stays down on shallow road, which is the rule
//!   landed earlier the same day.

use ff_core::sim::weather::WeatherKind;

use freight_fate::playtest::harness::{PlaytestHarness, RouteSetup};
use freight_fate::states::driving::DrivingState;
use freight_fate::states::driving_core::{DESCENT_BEATEN_MPH, GRADE_HOLDING_MPH_PER_S};

use crate::transcript_cruise_support::{
    bench_road_segments, frame, quiet, release_keys, start_drive_scaled, DT, MPS_PER_MPH, START_MI,
};

/// The line itself, verbatim, because a driver acts on it.
const WARNING: &str = "Descent control can't hold this grade. Brake.";

/// The owner drives compressed time and every assist on; so does this file.
const TIME_SCALE: f64 = 10.0;

// -- rigging -------------------------------------------------------------------------

/// Everything worth knowing about one frame of a descent.
#[derive(Clone, Copy, Debug)]
struct Frame {
    mile: f64,
    grade_pct: f64,
    speed_mph: f64,
    /// What descent control is working to, which is the driver's set speed
    /// under interactive mode's own ceiling.
    hold_mph: f64,
    stage: i32,
    brake: f64,
    /// Gaining or losing, in mph per second: the G key's own verdict.
    accel_mph_s: f64,
}

fn read_frame(drive: &DrivingState) -> Frame {
    let truck = &drive.trip.truck;
    Frame {
        mile: drive.trip.position_mi,
        grade_pct: drive.trip.grade_at(drive.trip.position_mi) * 100.0,
        speed_mph: truck.speed_mph(),
        hold_mph: drive.descent_hold_mph(),
        stage: truck.engine_brake_stage,
        brake: truck.brake,
        accel_mph_s: truck.net_accel_mph_per_s(),
    }
}

impl Frame {
    fn line(&self) -> String {
        format!(
            "mile {:.2}, grade {:+.2}%, {:.1} mph against a hold of {:.1}, jake {}, \
             brake {:.2}, {:+.2} mph/s",
            self.mile,
            self.grade_pct,
            self.speed_mph,
            self.hold_mph,
            self.stage,
            self.brake,
            self.accel_mph_s
        )
    }
}

/// A bench road of alternating level stretches and short downgrade dips --
/// the shape of I-70 through the Colorado River valley, which is what the
/// owner was driving.
fn dips(dip_pct: f64, dip_mi: f64, level_mi: f64, count: usize) -> Vec<(f64, f64, f64)> {
    let mut out = vec![(0.0, START_MI + 0.5, 0.0)];
    let mut at = START_MI + 0.5;
    for _ in 0..count {
        out.push((at, at + dip_mi, dip_pct));
        at += dip_mi;
        out.push((at, at + level_mi, 0.0));
        at += level_mi;
    }
    out.push((at, 400.0, 0.0));
    out
}

/// A drive on a bench road with cruise engaged and the assists the owner runs.
fn bench(
    name: &str,
    limit_mph: f64,
    grades: Vec<(f64, f64, f64)>,
    set_mph: f64,
    start_mph: f64,
    descent_level: &str,
) -> PlaytestHarness {
    let mut harness = start_drive_scaled(name, Some(TIME_SCALE));
    harness.app.ctx.settings.automatic_transmission = true;
    harness.app.ctx.settings.curve_speed_assist = true;
    harness.app.ctx.settings.speed_keeper = true;
    harness.app.ctx.settings.descent_speed_control = descent_level.to_string();
    release_keys(&mut harness);
    harness.with_drive(move |d, ctx| {
        bench_road_segments(d, &[(0.0, limit_mph)], &grades, TIME_SCALE);
        d.trip.position_mi = START_MI;
        d.weather_mut().forced = Some(WeatherKind::Clear);
        d.weather_mut().current = WeatherKind::Clear;
        d.truck_mut().transmission.automatic = true;
        d.truck_mut().cargo_kg = 18_000.0;
        d.truck_mut().start_engine();
        d.truck_mut().set_air_ready(false);
        d.truck_mut().transmission.gear = d.truck().transmission.num_gears();
        d.truck_mut().velocity_mps = start_mph * MPS_PER_MPH;
        d.engage_cruise(ctx, set_mph, false);
    });
    harness
}

/// Drive `seconds` and collect every frame the warning was spoken on, with
/// the state of the truck at that instant and what the G key would have
/// answered had the driver pressed it right then.
fn drive_watching(harness: &mut PlaytestHarness, seconds: f64) -> Vec<(Frame, String)> {
    let mut warnings = Vec::new();
    for _ in 0..(seconds / DT) as usize {
        let before = harness.app.speech().lines().len();
        frame(harness, DT);
        let said = harness.app.speech().lines()[before..]
            .iter()
            .any(|line| line.contains(WARNING));
        if !said {
            continue;
        }
        let at = harness.read_drive(read_frame);
        // The G key, pressed on the same frame, against the same truck.
        let before_g = harness.app.speech().lines().len();
        harness.with_drive(|d, ctx| d.speak_grade(ctx));
        let g = harness.app.speech().lines()[before_g].clone();
        warnings.push((at, g));
    }
    warnings
}

// -- the owner's own road --------------------------------------------------------------

/// The run he drove, seeded, with the preset he had on: not one warning.
///
/// Silverthorne to Glenwood Springs on I-70 -- Vail Pass, the Vail valley and
/// Glenwood Canyon, ninety-two miles of the most broken grade profile in the
/// game. The "All assists" preset is what the log recorded him on, and it is
/// the one that matters: it selects interactive descent control, whose 55 mph
/// ceiling on a 75 mph road was the whole of the false arithmetic.
///
/// The bar is not merely silence. The descent control has to have ENGAGED on
/// this road, and the drums have to have done real work, or the case would
/// pass just as well on a bug that switched the assist off.
#[test]
fn the_warning_stays_off_the_owners_i70_run() {
    let mut harness = PlaytestHarness::new();
    harness.app.ctx.settings.time_scale = TIME_SCALE;
    harness
        .app
        .ctx
        .settings
        .apply_driving_assistance_preset("all");
    assert_eq!(
        harness.app.ctx.settings.descent_speed_control, "interactive",
        "the preset under test has to be the one he drove"
    );
    harness.start_route(
        "Silverthorne",
        "Glenwood Springs",
        RouteSetup::seeded(4242).named("Descent Truth").cities(&[
            "Silverthorne",
            "Edwards",
            "Glenwood Springs",
        ]),
    );
    harness.with_drive(|d, ctx| {
        d.departure_checked = true;
        d.destination_exit_taken = true;
        quiet(&mut d.trip);
        d.weather_mut().forced = Some(WeatherKind::Clear);
        d.weather_mut().current = WeatherKind::Clear;
        d.trip.position_mi = 0.0;
        d.truck_mut().start_engine();
        d.truck_mut().set_air_ready(false);
        d.truck_mut().transmission.automatic = true;
        d.truck_mut().transmission.gear = d.truck().transmission.num_gears();
        d.truck_mut().velocity_mps = 65.0 * MPS_PER_MPH;
        d.engage_cruise(ctx, 80.0, false);
    });
    release_keys(&mut harness);

    let mut warnings: Vec<String> = Vec::new();
    let mut descents = 0usize;
    let mut braked_frames = 0usize;
    let mut was_active = false;
    for _ in 0..200_000 {
        let before = harness.app.speech().lines().len();
        frame(&mut harness, DT);
        let lines = harness.app.speech().lines();
        for line in lines[before..].iter() {
            if line.contains(WARNING) {
                warnings.push(harness.read_drive(|d| read_frame(d).line()));
            }
        }
        let (active, braking, done) = harness.read_drive(|d| {
            (
                d.descent_control_active,
                d.trip.truck.brake > 0.05 && d.trip.truck.grade < 0.0,
                d.trip.position_mi >= d.trip.total_miles() - 1.0,
            )
        });
        if active && !was_active {
            descents += 1;
        }
        was_active = active;
        if braking {
            braked_frames += 1;
        }
        if done {
            break;
        }
    }

    assert!(
        descents >= 5,
        "the road has to work the descent control to be worth measuring; engaged {descents} times"
    );
    assert!(
        braked_frames >= 60,
        "the drums have to have held those grades; braked on {braked_frames} frames"
    );
    assert!(
        warnings.is_empty(),
        "the truck was told to get on the brakes on road the assist was holding:\n  {}",
        warnings.join("\n  ")
    );
}

// -- a hill that really does beat it ---------------------------------------------------

/// Hot, worn drums on a ten percent grade: it still says it, in these words.
///
/// The fix must never be a mute. This is a real runaway -- twenty-five tonnes
/// on a grade steeper than anything signed, with the shoes already past their
/// fade temperature, so the retarder is at full stage, the drums are applied
/// and the truck is STILL gaining speed. That is what the sentence means, and
/// it is the one road in this file where a driver has to hear it.
#[test]
fn a_hill_that_beats_the_assist_still_says_so() {
    let mut harness = bench(
        "Runaway",
        65.0,
        vec![(0.0, 400.0, -10.0)],
        60.0,
        60.0,
        "realistic",
    );
    harness.with_drive(|d, _| {
        d.truck_mut().cargo_kg = 25_000.0;
        // Drums that have already been asked for too much: this is the
        // condition a runaway ramp exists for.
        d.truck_mut().brake_temp_c = 520.0;
        d.truck_mut().brake_wear_pct = 60.0;
    });
    let warnings = drive_watching(&mut harness, 30.0);
    assert!(
        !warnings.is_empty(),
        "a genuine runaway has to be spoken; the truck ended at {}",
        harness.read_drive(|d| read_frame(d).line())
    );
    let (at, g) = &warnings[0];
    assert!(
        at.speed_mph > at.hold_mph + DESCENT_BEATEN_MPH,
        "it must only be said when the truck is genuinely away: {}",
        at.line()
    );
    assert!(
        at.accel_mph_s > GRADE_HOLDING_MPH_PER_S,
        "it must only be said while the truck is still GAINING: {}",
        at.line()
    );
    // And the G key, pressed on that frame, has to agree that it is getting
    // away rather than being held.
    assert!(
        g.contains("not holding it") || g.contains("Speed is building"),
        "the readout disagreed with the warning: {g}"
    );
}

// -- the dips he was actually driving --------------------------------------------------

/// Quarter-mile dips at highway speed: the assist holds them and says nothing.
///
/// The shape from the log -- a 3 percent dip a quarter of a mile long, level
/// road either side, the truck at 69 on a 75 road with cruise set at 80. Every
/// one of these used to draw the warning on its first frame. The drums take
/// them; nothing is said; and the retarder stays out of it, which is the
/// separate rule that landed hours earlier the same day (a shallow grade is
/// not a retarder's job -- the drums settle cooler than fade and hold it).
#[test]
fn a_dip_the_drums_hold_says_nothing() {
    let mut harness = bench(
        "Valley Dips",
        75.0,
        dips(-3.0, 0.25, 0.75, 12),
        80.0,
        69.0,
        "interactive",
    );
    let mut engaged = 0usize;
    let mut was_active = false;
    let mut max_stage_on_dip = 0;
    let mut warnings: Vec<String> = Vec::new();
    for _ in 0..(60.0 / DT) as usize {
        let before = harness.app.speech().lines().len();
        frame(&mut harness, DT);
        let lines = harness.app.speech().lines();
        for line in lines[before..].iter() {
            if line.contains(WARNING) {
                warnings.push(harness.read_drive(|d| read_frame(d).line()));
            }
        }
        let (active, stage) =
            harness.read_drive(|d| (d.descent_control_active, d.trip.truck.engine_brake_stage));
        if active && !was_active {
            engaged += 1;
        }
        was_active = active;
        max_stage_on_dip = max_stage_on_dip.max(stage);
    }
    assert!(
        engaged >= 3,
        "the dips have to reach the descent control at all; engaged {engaged} times"
    );
    assert!(
        warnings.is_empty(),
        "a quarter-mile dip the drums hold must not tell the driver to brake:\n  {}",
        warnings.join("\n  ")
    );
    assert_eq!(
        max_stage_on_dip, 0,
        "the retarder must stay down on a shallow dip"
    );
}

// -- the two of them, on the same instant, across a sweep ------------------------------

/// The G key and the descent control, asked about the same frame, on grades
/// from a gentle roll to a runaway.
///
/// This is the guard that makes the owner's report impossible rather than
/// merely unlikely. On every grade in the sweep, in both descent modes, any
/// frame carrying the warning is also a frame on which the readout says the
/// speed is getting away -- never "Level road", never "Speed in hand", never
/// "has it". They read one grade and one net-force verdict between them.
#[test]
#[cfg_attr(
    ci_quick,
    ignore = "sweep: the readout against the descent control over every bench dip -- left to the nightly by --cfg ci_quick"
)]
fn the_readout_and_the_descent_control_agree_about_the_road() {
    let mut checked = 0usize;
    for (pct, fade) in [
        (-1.0, false),
        (-2.0, false),
        (-3.0, false),
        (-4.0, false),
        (-6.0, false),
        (-8.0, true),
        (-10.0, true),
        (-12.0, true),
    ] {
        for level in ["realistic", "balanced", "interactive"] {
            let mut harness = bench(
                "Agreement Sweep",
                65.0,
                vec![(0.0, 400.0, pct)],
                60.0,
                60.0,
                level,
            );
            if fade {
                harness.with_drive(|d, _| {
                    d.truck_mut().cargo_kg = 25_000.0;
                    d.truck_mut().brake_temp_c = 520.0;
                    d.truck_mut().brake_wear_pct = 60.0;
                });
            }
            for (at, g) in drive_watching(&mut harness, 25.0) {
                checked += 1;
                assert!(
                    !g.contains("Level road"),
                    "{pct}% {level}: warned about a grade the readout calls level: {g} | {}",
                    at.line()
                );
                assert!(
                    g.contains("not holding it") || g.contains("Speed is building"),
                    "{pct}% {level}: warned while the readout says it is held: {g} | {}",
                    at.line()
                );
                assert!(
                    at.accel_mph_s > GRADE_HOLDING_MPH_PER_S,
                    "{pct}% {level}: warned while the truck was slowing: {}",
                    at.line()
                );
            }
        }
    }
    assert!(
        checked > 0,
        "the sweep has to reach a real runaway somewhere or it proves nothing"
    );
}
