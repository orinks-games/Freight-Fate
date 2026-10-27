//! Resource abuse: fuel, hours-of-service, fatigue, and rest (port of
//! `tools/playtest_break_scenarios/resources.py`).
//!
//! Running dry for a free rescue, driving a 20+ hour marathon, cheesing HOS
//! with micro-rests, ignoring a microsleep, and taking a motel room with a
//! minute left on the delivery deadline.

use crate::playtest::breaker::{outcome, outcome_of, Outcome, Rig, RigOptions, Verdict, DT};
use crate::states::base::Key;
use crate::states::driving_core::{deadline_text, hos_of};
use crate::states::driving_menu_states::DriveRef;
use crate::states::driving_rest_states::RestStopState;

use ff_core::models::business::LEASED_OWNER_OPERATOR;
use ff_core::models::economy::MOTEL_COST;
use ff_core::sim::hos::HosClock;
use ff_core::sim::trip_models::RoadStop;

use super::text::leading_percent;

/// Run dry three times as a company driver, once as an owner-op; is the rescue
/// farmable?
///
/// Real minutes separate two run-dry events, so the pacer's repeat window has
/// to see real seconds pass too, or three byte-identical rescue lines collapse
/// into one purely because the loop runs faster than a real drive ever could.
/// Python swapped the pacer onto a hand-cranked clock for exactly this; the
/// rig already owns that clock, so this just winds it on between rescues.
pub fn fuel_rescue_farming() -> Outcome {
    let mut findings: Vec<String> = Vec::new();
    let company_summary;
    let mut company_problems: Vec<String> = Vec::new();
    {
        let mut rig = Rig::new(RigOptions::default());
        rig.drive.trip.position_mi = 12.0;
        rig.prepare(0.0, None);
        let (money_before, rep_before) = {
            let profile = rig.app.ctx.profile.as_ref().expect("a profile");
            (profile.money(), profile.career.reputation)
        };
        for _ in 0..3 {
            rig.drive.truck_mut().fuel_gal = 0.001;
            rig.drive.truck_mut().start_engine();
            rig.step(
                240,
                DT,
                Some(&|rig: &Rig| {
                    !rig.drive.truck().engine_on && rig.drive.truck().fuel_gal >= 25.0
                }),
            );
            rig.advance_clock(crate::app::FF_PACER.repeat_window_s + 1.0);
        }
        let rescues = rig.said("Roadside rescue");
        if rescues != 3 {
            findings.push(format!("expected 3 rescues, transcript has {rescues}"));
        }
        let (money, reputation) = {
            let profile = rig.app.ctx.profile.as_ref().expect("a profile");
            (profile.money(), profile.career.reputation)
        };
        if money != money_before {
            findings.push(format!(
                "company-driver rescue moved the driver's money ({:+.0})",
                money - money_before
            ));
        }
        if reputation == 0.0 && rep_before <= 6.0 {
            findings.push(
                "company driver: after reputation bottoms out at 0, roadside rescue is 30 free \
                 gallons with NO remaining cost -- fuel stops are optional forever"
                    .to_string(),
            );
        } else if rep_before - reputation < 5.9 {
            findings.push(format!(
                "reputation only fell {:.1} for three preventable service calls",
                rep_before - reputation
            ));
        }
        company_summary = format!(
            "company: 3 rescues, money {:+.0}, rep {rep_before:.0}->{reputation:.0}",
            money - money_before
        );
        company_problems.extend(rig.problems.iter().cloned());
        // A TestApp holds the environment lock until it is dropped; the
        // owner-op half below builds a second one.
        drop(rig);
    }

    let mut rig2 = Rig::new(RigOptions {
        business: Some(LEASED_OWNER_OPERATOR.to_string()),
        ..RigOptions::default()
    });
    if let Some(profile) = rig2.app.ctx.profile.as_mut() {
        profile.set_money(100.0);
    }
    rig2.drive.trip.position_mi = 12.0;
    rig2.prepare(0.0, None);
    for _ in 0..2 {
        rig2.drive.truck_mut().fuel_gal = 0.001;
        rig2.drive.truck_mut().start_engine();
        rig2.step(
            240,
            DT,
            Some(&|rig: &Rig| !rig.drive.truck().engine_on && rig.drive.truck().fuel_gal >= 25.0),
        );
        rig2.advance_clock(crate::app::FF_PACER.repeat_window_s + 1.0);
    }
    let money = rig2.app.ctx.profile.as_ref().map_or(0.0, |p| p.money());
    if (money - (100.0 - 1500.0)).abs() > 0.01 {
        findings.push(format!(
            "owner-op rescue billing off: expected -1,500 total, money is {money:.0}"
        ));
    }
    // Money below zero with no floor and no spoken balance is deliberate ("can
    // go negative: the rescue is not optional") -- only flag drift.
    rig2.check_invariants();
    for problem in company_problems.iter().chain(rig2.problems.iter()) {
        findings.push(format!("invariant: {problem}"));
    }
    let verdict = if findings.is_empty() {
        Verdict::Clean
    } else {
        Verdict::Odd
    };
    let note = findings
        .first()
        .cloned()
        .unwrap_or_else(|| format!("{company_summary}; owner-op billed to {money:.0}"));
    Outcome {
        name: "fuel_rescue_farming".to_string(),
        verdict,
        note,
        findings,
        transcript: rig2.transcript(),
    }
}

/// 22-hour drive, micro-rest cheese, and the in-cab waiting clock vs the HOS
/// ledger.
pub fn hos_marathon_and_rest_cheese() -> Outcome {
    let mut findings: Vec<String> = Vec::new();
    // Pure-model probes first: no app needed, fully deterministic.
    let mut clock = HosClock::new();
    clock.drive(11.0 * 60.0 + 5.0);
    if !clock.in_violation("realistic") {
        findings.push("11h05m of driving is not a violation in realistic mode".to_string());
    }
    let mut cheese = HosClock::new();
    for _ in 0..12 {
        cheese.drive(48.0);
        cheese.off_duty(29.9); // 29.9-minute naps: never a qualifying break
    }
    if cheese.since_break_min < 8.0 * 60.0 {
        findings.push(
            "29.9-minute micro-rests reset the 30-minute-break clock (they must not)".to_string(),
        );
    }
    if cheese.driving_min < 9.0 * 60.0 {
        findings.push("micro-rests drained the 11-hour driving clock".to_string());
    }
    let mut legit = HosClock::new();
    legit.drive(60.0);
    for _ in 0..20 {
        legit.off_duty(30.0);
    }
    if legit.driving_min != 0.0 {
        findings.push(
            "10 continuous off-duty hours (in 30-min slices) did not reset the shift".to_string(),
        );
    }

    // In-cab: deliberate waiting burns the 14-hour window as ON DUTY and can
    // never become rest, while the alpha-book clock lever counts a 10-hour
    // wait as a full break. Two clocks, two answers for the same nap.
    let mut rig = Rig::new(RigOptions::default());
    rig.drive.trip.position_mi = 12.0;
    rig.prepare(0.0, None);
    rig.drive.toggle_parking_brake(&mut rig.app.ctx); // player-set brake arms deliberate waiting
    rig.app.ctx.run_deferred();
    if !rig.drive.trip.waiting {
        findings.push("player-set parking brake did not arm waiting fast-forward".to_string());
    }
    let scale = rig.drive.trip.effective_time_scale();
    if (scale - rig.drive.trip.time_scale * 2.0).abs() > 1e-6 {
        findings.push(format!(
            "waiting clock runs {scale:.0}x, expected double pacing"
        ));
    }
    let duty_before = hos_of(&rig.app.ctx).duty_min;
    for _ in 0..120 {
        // ten game-hours of parked waiting, direct-call idiom
        rig.drive.update_hours_and_fatigue(&mut rig.app.ctx, 5.0);
    }
    rig.app.ctx.run_deferred();
    let waited_min = hos_of(&rig.app.ctx).duty_min - duty_before;
    let status = hos_of(&rig.app.ctx).status.clone();
    if status != "on_duty_not_driving" {
        findings.push(format!("parked waiting logged as {status}"));
    }
    if hos_of(&rig.app.ctx).off_duty_min > 0.0 {
        findings.push("parked waiting accrued off-duty rest (it must stay on duty)".to_string());
    }
    // No else. Deliberate waiting staying on duty is the DESIGN -- the line
    // above asserts it -- so an else here reported a finding on the healthy
    // path and could never come back clean.
    if waited_min <= 0.0 {
        findings.push("ten game-hours of parked waiting burned no duty time at all".to_string());
    }
    outcome(
        "hos_marathon_and_rest_cheese",
        &rig,
        findings,
        "HOS ledger held against the marathon and the cheese",
    )
}

/// Fatigue 100, never react, keep the floor down; does the forced stop
/// actually stop you?
pub fn microsleep_throttle_through() -> Outcome {
    let mut rig = Rig::new(RigOptions::default());
    let mut findings: Vec<String> = Vec::new();
    rig.drive.trip.position_mi = 12.0;
    rig.drive.trip.curves.clear();
    if let Some(profile) = rig.app.ctx.profile.as_mut() {
        profile.fatigue = 100.0;
    }
    rig.prepare(65.0, None);
    rig.hold(Key::Up); // throttle is not a microsleep reaction key
    let mut drift_damage: Vec<f64> = Vec::new();
    let mut forced_stop_frame: Option<usize> = None;
    let mut frames = 0;
    while frames < 14000 {
        rig.advance_clock(DT);
        rig.drive.update_frame(&mut rig.app.ctx, DT);
        rig.app.ctx.run_deferred();
        frames += 1;
        if frames % 10 == 0 {
            rig.check_invariants();
        }
        if forced_stop_frame.is_none() && rig.said("cannot stay awake") > 0 {
            forced_stop_frame = Some(frames);
        }
        if forced_stop_frame.is_some_and(|at| frames >= at + 150) {
            break;
        }
    }
    let drift_lines = rig.lines_with("drifted onto the rumble strip");
    for line in &drift_lines {
        if let Some((_, rest)) = line.split_once("now ") {
            if let Some(percent) = leading_percent(rest) {
                drift_damage.push(percent);
            }
        }
    }
    if drift_lines.is_empty() && forced_stop_frame.is_none() {
        findings.push("severe fatigue never produced a microsleep at all".to_string());
    }
    if let Some(first) = drift_damage.first() {
        if (first - 6.0).abs() > 6.5 {
            findings.push(format!(
                "first drift damage spoke {first:.0}%, model adds 6%"
            ));
        }
    }
    let speed = rig.drive.truck().speed_mph();
    if forced_stop_frame.is_some() && speed > 40.0 {
        findings.push(format!(
            "\"You cannot stay awake ... jolt awake on the brakes. Stop and sleep before you \
             wreck\" -- but the forced stop is a one-frame brake tap: with the throttle held the \
             truck is doing {speed:.0} mph five seconds later and the exhausted driver just keeps \
             going"
        ));
    }
    let fatigue = rig.app.ctx.profile.as_ref().map_or(0.0, |p| p.fatigue);
    if fatigue > 100.0 {
        findings.push(format!("fatigue exceeded its cap: {fatigue}"));
    }
    let note = format!("{} drifts, forced stop held the truck", drift_lines.len());
    outcome("microsleep_throttle_through", &rig, findings, &note)
}

/// In violation, rest one minute thirty times; split-sleeper credit must not
/// double-dip.
pub fn hos_rest_minute_cheese() -> Outcome {
    let mut findings: Vec<String> = Vec::new();
    let mut clock = HosClock::new();
    clock.drive(11.0 * 60.0 + 5.0);
    for _ in 0..29 {
        clock.off_duty(1.0);
    }
    if !clock.in_violation("realistic") {
        findings
            .push("29 one-minute naps talked the ledger out of an 11-hour violation".to_string());
    }
    if clock.driving_min < 11.0 * 60.0 {
        findings.push("one-minute rests drained the driving clock".to_string());
    }
    clock.off_duty(1.0); // the 30th consecutive minute is a legitimate break
    if clock.since_break_min != 0.0 {
        findings.push("30 consecutive off-duty minutes did not clear the break clock".to_string());
    }
    if !clock.in_violation("realistic") {
        findings
            .push("a 30-minute break cleared an 11-hour DRIVING violation (it cannot)".to_string());
    }

    let mut split = HosClock::new();
    split.drive(5.0 * 60.0);
    split.sleeper(7.0 * 60.0);
    split.drive(2.0 * 60.0);
    let drive_before_credit = split.driving_min;
    split.off_duty(3.0 * 60.0); // completes a 7/3 split pair
    let after_first = split.driving_min;
    split.off_duty(3.0 * 60.0); // the same long rest must not pair twice
    let after_second = split.driving_min;
    if after_second < after_first {
        findings.push(format!(
            "split-sleeper credit double-dipped: driving {drive_before_credit:.0} -> \
             {after_first:.0} -> {after_second:.0} minutes"
        ));
    }
    for (label, value) in [
        ("driving_min", split.driving_min),
        ("duty_min", split.duty_min),
        ("since_break_min", split.since_break_min),
    ] {
        if value < 0.0 {
            findings.push(format!("split credit drove {label} negative: {value}"));
        }
    }
    outcome_of(
        "hos_rest_minute_cheese",
        findings,
        "minute-rest cheese and split double-dip both held",
    )
}

/// Take a 10-hour motel room with 1 minute left to deliver; the game must not
/// paper over it.
///
/// A player one minute from a deadline who takes a full 10-hour motel rest
/// should be told, honestly, that the load is now hours overdue -- never a
/// stretched deadline, never silence.
///
/// Python called `RestStopState._motel_sleep()` directly; that method is
/// private here, so this drives the row that calls it, which is the path a
/// player takes anyway.
pub fn motel_rest_deadline_crunch() -> Outcome {
    let mut rig = Rig::new(RigOptions::default());
    let mut findings: Vec<String> = Vec::new();
    if let Some(profile) = rig.app.ctx.profile.as_mut() {
        profile.set_money(500.0);
        profile.fatigue = 90.0;
    }
    rig.drive.trip.position_mi = 12.0;
    rig.prepare(0.0, None);
    // One minute of game-time left before the deadline.
    let deadline_h = rig.drive.job.deadline_game_h;
    rig.drive.trip.game_minutes = deadline_h * 60.0 - 1.0;
    let remaining_before = deadline_h - rig.drive.trip.game_minutes / 60.0;
    if !(remaining_before > 0.0 && remaining_before <= 1.0 / 60.0 + 1e-6) {
        findings.push(format!(
            "setup did not land ~1 minute out: {:.2} min",
            remaining_before * 60.0
        ));
    }

    let mut stop = RoadStop::new(
        "Test Travel Center",
        rig.drive.trip.position_mi,
        "travel_center",
    );
    stop.actions = vec!["break".to_string(), "fuel".to_string()];
    stop.parking = "limited".to_string();

    let deadline_before_h = rig.drive.job.deadline_game_h;
    let lines_before_sleep = rig.transcript().len();
    let took_the_room = rig.with_drive_on_stack(|rig, drive: DriveRef| {
        let mut state = RestStopState::with_drive(drive, stop.clone(), false);
        // `enter_over_drive` is how a drive-owned screen opens with the drive
        // already in hand; the rig has to hand it over the same way.
        let shared = rig.app.ctx.state().expect("the drive is on the stack");
        {
            let mut borrowed = shared.borrow_mut();
            let driving = borrowed
                .as_any_mut()
                .downcast_mut::<crate::states::driving::DrivingState>()
                .expect("the rig's drive");
            state.enter_over_drive(&mut rig.app.ctx, driving);
        }
        rig.app.ctx.push_state_with(state, false);
        rig.app.ctx.run_deferred();
        let selected = rig.select_menu_containing("Motel room");
        if selected {
            // The first press previews the cost and deadline; the second
            // commits while the menu is still on the drive's state stack.
            if rig.app.ctx.profile.as_ref().map_or(0.0, |p| p.money()) != 500.0
                || !rig.transcript()[lines_before_sleep..]
                    .iter()
                    .any(|line| line.contains("Select again to sleep"))
            {
                findings.push("motel preview changed state or did not speak".to_string());
            }
            rig.key_screen(Key::Return);
        }
        selected
    });
    if !took_the_room {
        findings.push("no motel row on the rest-stop menu to take at all".to_string());
        return outcome("motel_rest_deadline_crunch", &rig, findings, "");
    }
    let money = rig.app.ctx.profile.as_ref().map_or(0.0, |p| p.money());
    if money != 500.0 - MOTEL_COST {
        findings.push(format!(
            "motel did not charge {MOTEL_COST:.0} exactly: money is {money}"
        ));
    }
    if rig.drive.job.deadline_game_h != deadline_before_h {
        findings.push(format!(
            "deadline moved during a rest ({deadline_before_h} -> {}); a motel room must never \
             buy back deadline time",
            rig.drive.job.deadline_game_h
        ));
    }
    let remaining_after = deadline_text(&rig.drive, &rig.app.ctx);
    if !remaining_after.contains("past the deadline") {
        findings.push(format!(
            "took a 10-hour rest 1 minute from the deadline and the honesty line reads \
             {remaining_after:?} instead of admitting the load is now overdue"
        ));
    }
    // The motel confirmation is followed by an achievement announcement
    // (slept_on_route), so check every line this call actually spoke.
    let transcript = rig.transcript();
    let sleep_lines = &transcript[lines_before_sleep.min(transcript.len())..];
    let confirmation = sleep_lines
        .iter()
        .find(|line| line.contains("Motel, "))
        .cloned()
        .unwrap_or_default();
    if !confirmation.contains(&remaining_after) {
        findings.push(format!(
            "the motel confirmation line does not carry the blown-deadline warning \
             ({remaining_after:?} missing): {sleep_lines:?}"
        ));
    }
    // The shorter rest lines (2026-10-17) say what the sleep reset rather
    // than "you wake fresh".
    if !confirmation.contains("clocks reset") {
        findings.push("motel confirmation line dropped what the sleep reset".to_string());
    }
    outcome(
        "motel_rest_deadline_crunch",
        &rig,
        findings,
        "motel charged honestly and admitted the blown deadline",
    )
}
