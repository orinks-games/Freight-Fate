//! Leaving the interstate the way a truck really does (owner-approved
//! redesign, 2026-09-24): road speed down the mainline, the braking done in
//! the deceleration lane past the gore, the exit speed named there, and a
//! ramp curve that costs something when it is taken hot.
//!
//! Every case runs on a bench road: one straight leg with one posted number
//! and one grade, and one invented truck stop on it, so the Green Book
//! numbers each case pins come from the road the case builds and not from
//! whichever corridor a fresh career draws.

use ff_core::sim::trip_models::RoadStop;
use freight_fate::playtest::harness::{PlaytestHarness, StartDelivery};
use freight_fate::states::base::Key;
use freight_fate::states::driving_core::EXIT_MAINLINE_EASE_MPH;

use crate::transcript_cruise_support::{bench_road, MPS_PER_MPH};

const DT: f64 = 1.0 / 30.0;
const STOP_MI: f64 = 40.0;

/// A loaded truck on a bench road posted `limit_mph` at `grade_pct`, rolling
/// at `speed_mph` `ahead_mi` short of a signalled truck stop exit, with only
/// the assists a case names switched on. Lane keeping is full, so the lane
/// work is never what a case turns on.
fn exit_rig(
    limit_mph: f64,
    grade_pct: f64,
    ahead_mi: f64,
    speed_mph: f64,
    cruise: bool,
    exit_speed_assist: bool,
) -> (PlaytestHarness, RoadStop) {
    let rig = Rig {
        exit_speed_assist,
        ..Rig::default()
    };
    exit_rig_with(limit_mph, grade_pct, ahead_mi, speed_mph, cruise, rig)
}

/// The settings a case can turn on beyond [`exit_rig`]'s.
#[derive(Clone, Copy)]
pub(crate) struct Rig {
    pub(crate) exit_speed_assist: bool,
    pub(crate) curve_speed_assist: bool,
    pub(crate) time_scale: f64,
}

impl Default for Rig {
    fn default() -> Self {
        Rig {
            exit_speed_assist: false,
            curve_speed_assist: false,
            time_scale: 1.0,
        }
    }
}

pub(crate) fn exit_rig_with(
    limit_mph: f64,
    grade_pct: f64,
    ahead_mi: f64,
    speed_mph: f64,
    cruise: bool,
    rig: Rig,
) -> (PlaytestHarness, RoadStop) {
    let mut harness = PlaytestHarness::new();
    harness.start_delivery(StartDelivery::named("Deceleration Lane"));
    harness.app.ctx.settings.time_scale = rig.time_scale;
    harness.app.ctx.settings.lane_keeping = "full".to_string();
    harness.app.ctx.settings.automatic_transmission = true;
    harness.app.ctx.settings.exit_speed_assist = rig.exit_speed_assist;
    harness.app.ctx.settings.route_transition_assist = false;
    harness.app.ctx.settings.curve_speed_assist = rig.curve_speed_assist;
    harness.with_drive(move |d, _| {
        d.departure_checked = true;
        bench_road(d, limit_mph, grade_pct, rig.time_scale);
        d.truck_mut().set_air_ready(false);
    });
    harness.press_key(Key::E, None); // engine on
    let stop = {
        let mut stop = RoadStop::new("Prairie Travel Center", STOP_MI, "truck_stop");
        stop.actions = ["park", "fuel", "food"]
            .iter()
            .map(|a| a.to_string())
            .collect();
        stop.parking = "confirmed".to_string();
        stop.exit_label = "exit 42".to_string();
        stop
    };
    let staged = stop.clone();
    harness.with_drive(move |d, _| {
        d.trip.stops = vec![staged.clone()];
        d.truck_mut().transmission.gear = 10;
        d.truck_mut().velocity_mps = speed_mph * MPS_PER_MPH;
        d.trip.position_mi = staged.at_mi - ahead_mi;
    });
    if cruise {
        harness.press_key(Key::K, None);
    }
    harness.press_key(Key::X, None);
    assert_eq!(
        harness.read_drive(|d| d.exit_stop.as_ref().map(|s| s.key())),
        Some(stop.key()),
        "{}",
        harness.transcript_text()
    );
    (harness, stop)
}

pub(crate) fn frame(harness: &mut PlaytestHarness) {
    harness.advance_clock(DT);
    harness.with_drive(|d, ctx| d.update_frame(ctx, DT));
}

/// Drive to the gore. Returns the slowest the truck went on the mainline
/// and the speed it crossed the gore at.
pub(crate) fn drive_to_the_gore(harness: &mut PlaytestHarness, stop: &RoadStop) -> (f64, f64) {
    let mut slowest = f64::INFINITY;
    for _ in 0..(30 * 60 * 10) {
        let (speed, on_ramp, position) = harness.read_drive(|d| {
            (
                d.truck().speed_mph(),
                d.ramp_mi.is_some(),
                d.trip.position_mi,
            )
        });
        if on_ramp {
            return (slowest, speed);
        }
        assert!(
            position < stop.at_mi + 0.5,
            "missed the exit\n{}",
            harness.transcript_text()
        );
        slowest = slowest.min(speed);
        frame(harness);
    }
    panic!("never reached the gore\n{}", harness.transcript_text());
}

#[test]
fn test_the_mainline_never_sheds_more_than_ten_under_road_speed() {
    // The tester report the redesign answers: cruise and exit speed
    // assistance shed the truck to ramp speed ON THE MAINLINE, about 44 mph
    // in a 70 lane for the last half mile. Real drivers diverge at 58 to 70
    // mph at 70 mph sites (NCHRP Research Report 1081), and TxDOT allows up
    // to ten of slowing in the through lanes. With cruise on, and with nobody
    // on the pedals and the exit speed assist holding the approach.
    // Without cruise the case starts inside the assist's own window: a
    // driver coasting the miles before it is shedding by their own choice.
    for (cruise, ahead_mi) in [(true, 3.0), (false, 1.4)] {
        let (mut harness, stop) = exit_rig(70.0, 0.0, ahead_mi, 70.0, cruise, true);
        let (slowest, entry) = drive_to_the_gore(&mut harness, &stop);
        let floor = 70.0 - EXIT_MAINLINE_EASE_MPH;
        assert!(
            slowest >= floor - 2.5,
            "cruise={cruise}: shed to {slowest:.1} on the mainline\n{}",
            harness.transcript_text()
        );
        let accepted = harness.with_drive(move |d, _| d.gore_acceptance_mph(Some(&stop)));
        assert!(
            entry <= accepted,
            "cruise={cruise}: {entry:.1} over {accepted}"
        );
        // Nothing on the approach asked for the ramp's number.
        let text = harness.transcript_text();
        assert!(!text.contains("slow to"), "{text}");
        assert!(!text.contains("Stay under"), "{text}");
    }
}

#[test]
fn test_the_gore_opens_a_green_book_deceleration_lane_and_pauses_speed_control() {
    // Green Book 2018 Table 10-6 for a 70 mph highway and this bench ramp's
    // 49 mph curve (70 percent of the road, the surface-ramp share) is 350
    // feet on the flat, between the 45 and 50 columns. Table 10-5's
    // deceleration factors: 1.2 on a 3 to 5 percent downgrade, 0.9 on the
    // same upgrade, 1.35 and 0.8 at 5 percent and over.
    for (grade_pct, feet) in [
        (0.0, 350.0),
        (-3.5, 420.0),
        (3.5, 315.0),
        (-5.5, 472.5),
        (5.5, 280.0),
    ] {
        let (mut harness, stop) = exit_rig(70.0, grade_pct, 1.0, 62.0, true, false);
        drive_to_the_gore(&mut harness, &stop);
        let (layout, ramp_mph) =
            harness.read_drive(|d| (d.ramp_layout.expect("laid out"), d.armed_ramp_mph(None)));
        assert_eq!(ramp_mph, 49.0);
        assert!(
            (layout.decel_mi * 5280.0 - feet).abs() < 0.01,
            "{grade_pct}: {} feet, not {feet}",
            layout.decel_mi * 5280.0
        );
        assert!(harness.read_drive(|d| d.in_deceleration_lane()));
        // Paused, never disarmed: the session rides along and returns past
        // the bar (owner ruling, speed control survives hazards).
        assert!(harness.read_drive(|d| d.speed_control_armed), "{grade_pct}");
        assert!(harness.read_drive(|d| d.cruise_mph.is_none() && d.keeper_mph.is_none()));
        // And the ramp past the lane is level: ASSUMED, no grade is baked.
        for _ in 0..(30 * 30) {
            if !harness.read_drive(|d| d.in_deceleration_lane()) {
                break;
            }
            frame(&mut harness);
        }
        frame(&mut harness);
        assert_eq!(harness.read_drive(|d| d.trip.ramp_grade), Some(0.0));
        assert_eq!(harness.read_drive(|d| d.truck().grade), 0.0);
    }
}

#[test]
fn test_the_exit_speed_is_spoken_once_in_the_deceleration_lane_and_braked_for_there() {
    let (mut harness, stop) = exit_rig(70.0, 0.0, 1.0, 62.0, true, true);
    drive_to_the_gore(&mut harness, &stop);
    // Counted as the game announced it: a line another event cuts is handed
    // back and finished by the pacer, which the voice hears as one line and
    // the capture records twice.
    let said = |harness: &PlaytestHarness| {
        harness
            .app
            .event_calls()
            .iter()
            .filter(|(line, _)| line.contains("Exit speed 49."))
            .count()
            .saturating_sub(harness.app.ctx.handed_back_count("Exit speed 49."))
    };
    // Said once, on the driving channel: at the mile, and not again in the
    // gore line while the exit speed assist does the braking (owner,
    // 2026-10-25). The gore line leads with the exit.
    assert!(harness.read_drive(|d| d.in_deceleration_lane()));
    assert_eq!(said(&harness), 1, "{}", harness.transcript_text());
    assert!(
        harness
            .app
            .event_lines()
            .iter()
            .any(|line| line.starts_with("Exit 42")),
        "{}",
        harness.transcript_text()
    );
    assert!(
        !harness
            .app
            .main_lines()
            .iter()
            .any(|line| line.contains("Exit speed 49")),
        "the exit speed went out on the menu channel"
    );
    // The exit speed assist brakes in the lane, and the truck reaches the
    // curve at its number.
    let mut curve_entry = None;
    for _ in 0..(30 * 60) {
        frame(&mut harness);
        if harness.read_drive(|d| d.ramp_curve_radius_ft().is_some()) {
            curve_entry = Some(harness.read_drive(|d| d.truck().speed_mph()));
            break;
        }
    }
    let entry = curve_entry.expect("the ramp curve");
    assert!(entry <= 49.0 + 2.0, "reached the curve at {entry:.1}");
    assert_eq!(said(&harness), 1, "{}", harness.transcript_text());
    assert!(
        harness.transcript_text().contains("Slowing for the ramp."),
        "{}",
        harness.transcript_text()
    );
}

/// Drive from the gore to the ramp curve. Returns the speed the truck
/// entered the curve at.
fn to_the_ramp_curve(harness: &mut PlaytestHarness) -> f64 {
    for _ in 0..(30 * 60) {
        frame(harness);
        if harness.read_drive(|d| d.ramp_curve_radius_ft().is_some()) {
            return harness.read_drive(|d| d.truck().speed_mph());
        }
    }
    panic!(
        "never reached the ramp curve\n{}",
        harness.transcript_text()
    );
}

#[test]
fn test_a_free_flowing_ramp_runs_the_lane_and_curve_on_the_real_clock() {
    // Review of the realistic exit: the lane is priced in real metres, and a
    // ramp with no light or sign ran on the compressed clock. At five times
    // the lane went by in about two real seconds and the truck met its
    // curve far over the exit speed. Every exit is real time from the gore
    // through the curve now.
    let rig = Rig {
        exit_speed_assist: true,
        time_scale: 5.0,
        ..Rig::default()
    };
    let (mut harness, stop) = exit_rig_with(70.0, 0.0, 1.0, 62.0, true, rig);
    drive_to_the_gore(&mut harness, &stop);
    // A free-flowing ramp: nothing at its end to hold the clock.
    harness.with_drive(|d, _| {
        d.ramp_control = "none".to_string();
        d.ramp_terminal_done = true;
        d.cross_bubble = None;
    });
    let mut entry = None;
    for _ in 0..(30 * 60) {
        frame(&mut harness);
        let (short, scale, in_curve, speed) = harness.read_drive(|d| {
            (
                d.short_of_ramp_curve_end(),
                d.trip.effective_time_scale(),
                d.ramp_curve_radius_ft().is_some(),
                d.truck().speed_mph(),
            )
        });
        if short {
            assert_eq!(scale, 1.0, "the lane or curve ran on a compressed clock");
        }
        if in_curve && entry.is_none() {
            entry = Some(speed);
        }
        if !short {
            break;
        }
    }
    let entry = entry.expect("reached the ramp curve");
    assert!(
        entry <= 49.0 + 2.0,
        "met the curve at {entry:.1} at five times"
    );
}

#[test]
fn test_nothing_on_the_approach_asks_for_the_ramp_speed() {
    // Review of the realistic exit: the signal-on line still said "then 49
    // or less for the ramp" a mile out, which had drivers braking to it on
    // the mainline. The approach names the number once, at the mile, as a
    // fact to plan by (`test_the_mile_call_names_the_exit_speed_the_ramp_holds`);
    // nothing on it asks the truck to slow.
    let (mut harness, stop) = exit_rig(70.0, 0.0, 3.0, 70.0, true, true);
    let armed = harness
        .transcript()
        .into_iter()
        .find(|line| line.contains("Signal set for exit 42"))
        .expect("the signal-on line");
    assert!(!armed.contains("for the ramp"), "{armed}");
    assert!(!armed.contains("49"), "{armed}");
    harness.clear_speech();
    drive_to_the_gore(&mut harness, &stop);
    let approach: Vec<String> = harness
        .transcript()
        .into_iter()
        .filter(|line| !line.contains("Exit speed"))
        .collect();
    let approach = approach.join("\n");
    assert!(!approach.contains("49"), "{approach}");
    assert!(!approach.to_lowercase().contains("slow"), "{approach}");
}

#[test]
fn test_curve_assistance_alone_brakes_before_the_ramp_curve() {
    // Review of the realistic exit: with the ramp bending only inside its
    // curve, curve assistance alone met it hot and braked mid-corner. It
    // sees the ramp curve ahead from the lane, as it sees a mapped bend.
    let rig = Rig {
        curve_speed_assist: true,
        ..Rig::default()
    };
    let (mut harness, stop) = exit_rig_with(70.0, 0.0, 1.0, 62.0, true, rig);
    drive_to_the_gore(&mut harness, &stop);
    let entry = to_the_ramp_curve(&mut harness);
    assert!(entry <= 49.0 + 2.0, "met the curve at {entry:.1}");
    assert!(
        harness.transcript_text().contains("Slowing for the ramp."),
        "{}",
        harness.transcript_text()
    );
}

/// Take the exit off a 45 mph road (a 32 mph ramp curve, 214 feet of
/// radius) at `speed_mph` with every exit assist off, and roll it through
/// the curve. Returns the load's damage after, and what was said.
fn through_the_ramp_curve(speed_mph: f64) -> (f64, String) {
    let (mut harness, stop) = exit_rig(45.0, 0.0, 0.05, speed_mph, false, false);
    harness.with_drive(|d, _| assert!(d.truck().cargo_kg > 0.0, "the rig is loaded"));
    drive_to_the_gore(&mut harness, &stop);
    for _ in 0..(30 * 60) {
        // Holding the speed the driver chose: no pedal but a light throttle.
        harness.with_drive(move |d, _| {
            if d.truck().speed_mph() < speed_mph {
                d.truck_mut().throttle = 0.5;
            }
        });
        frame(&mut harness);
        let past_curve =
            harness.read_drive(|d| d.ramp_curve_radius_ft().is_none() && !d.in_deceleration_lane());
        if past_curve {
            break;
        }
    }
    (
        harness.read_drive(|d| d.truck().cargo_damage_pct),
        harness.transcript_text(),
    )
}

#[test]
fn test_a_ramp_curve_taken_hot_moves_the_load_and_says_so() {
    // The exit speed is the ramp curve's advisory, and the curve is a corner
    // to the freight like any mapped bend: taken at 50 against its 32 it
    // pulls about 0.8 g, far past what the truck stays upright at. At the
    // advisory it costs nothing. And the mapped bend's too-fast warning
    // covers it.
    let warning = "Ramp curve, too fast. Slow to";
    let (at_advisory, calm) = through_the_ramp_curve(32.0);
    let (hot, text) = through_the_ramp_curve(50.0);
    assert_eq!(at_advisory, 0.0);
    assert!(!calm.contains(warning), "{calm}");
    assert!(hot > 0.0, "a hot ramp curve cost the load nothing");
    assert_eq!(text.matches(warning).count(), 1, "{text}");
}

#[test]
fn test_the_steering_lean_asks_for_the_wheel_only_for_the_ramp_curve() {
    // Every-assist audit, 2026-09-24: the engine leaned for the whole ramp,
    // gore to driveway, so a driver steering for themselves was asked to
    // turn down a straight deceleration lane and a straight run to the bar.
    // It leans for the curve now, leading into it like a street turn.
    use freight_fate::states::driving_turns::{RAMP_GUIDE_DEMAND, TURN_GUIDE_LEAD_MI};
    let (mut harness, stop) = exit_rig(70.0, 0.0, 1.0, 62.0, true, true);
    drive_to_the_gore(&mut harness, &stop);
    let mut seen = (false, false, false); // lane far, curve, past the curve
    for _ in 0..(30 * 120) {
        frame(&mut harness);
        let (lane_left, in_curve, past, lean) = harness.read_drive(|d| {
            (
                d.deceleration_lane_left_mi(),
                d.ramp_curve_radius_ft().is_some(),
                d.ramp_mi.is_some() && !d.short_of_ramp_curve_end(),
                d.maneuver_steer_demand(None),
            )
        });
        if lane_left.is_some_and(|left| left > TURN_GUIDE_LEAD_MI) {
            assert_eq!(lean, 0.0, "leaned {lean} down the deceleration lane");
            seen.0 = true;
        } else if in_curve {
            assert_eq!(lean, RAMP_GUIDE_DEMAND);
            seen.1 = true;
        } else if past {
            assert_eq!(lean, 0.0, "leaned {lean} on the run to the bar");
            seen.2 = true;
            break;
        }
    }
    assert!(seen.1 && seen.2, "{seen:?}");
}

/// The countdown lines an armed exit spoke, as the game announced them.
fn mile_calls(harness: &PlaytestHarness) -> Vec<String> {
    harness
        .app
        .event_lines()
        .into_iter()
        .filter(|line| line.starts_with("Exit for "))
        .collect()
}

#[test]
fn test_the_mile_call_names_the_exit_speed_the_ramp_holds() {
    // Player report (2026-10-20): the exit speed was named only on the ramp,
    // too late to plan for, so a driver slowed to 20 for every exit and still
    // rolled the truck on the one ramp posted 20. The mile call names it,
    // and it is the number the ramp line speaks and the ramp curve is judged
    // against -- never a second figure for the same ramp.
    let (mut harness, stop) = exit_rig(70.0, 0.0, 2.4, 62.0, true, true);
    drive_to_the_gore(&mut harness, &stop);
    let calls = mile_calls(&harness);
    assert_eq!(calls.len(), 3, "{calls:#?}");
    assert!(!calls[0].contains("Exit speed"), "two miles: {}", calls[0]);
    assert!(
        calls[1].contains("in 1 mile. Exit speed 49."),
        "one mile: {}",
        calls[1]
    );
    assert!(
        !calls[2].contains("Exit speed"),
        "half a mile: {}",
        calls[2]
    );
    // The ramp line does not say it again while the assist brakes for it.
    let ramp_line = harness
        .app
        .event_lines()
        .into_iter()
        .find(|line| line.contains(" of ramp"))
        .expect("the ramp line");
    assert!(!ramp_line.contains("Exit speed"), "{ramp_line}");
    let curve_mph = harness.read_drive(|d| d.ramp_layout.expect("laid out").curve_mph);
    assert_eq!(curve_mph, 49.0);
}

#[test]
fn test_a_frame_that_skips_the_mile_still_names_the_exit_speed() {
    // Time compression can carry the truck from past two miles to inside
    // half a mile in one frame; the countdown speaks only the nearest anchor,
    // and that one carries the exit speed the skipped mile would have.
    let (mut harness, stop) = exit_rig(70.0, 0.0, 2.4, 62.0, true, true);
    harness.clear_speech();
    let at_mi = stop.at_mi;
    harness.with_drive(move |d, _| d.trip.position_mi = at_mi - 0.4);
    frame(&mut harness);
    let calls = mile_calls(&harness);
    assert_eq!(calls.len(), 1, "{calls:#?}");
    assert!(
        calls[0].contains("in half a mile. Exit speed 49."),
        "{}",
        calls[0]
    );
}
