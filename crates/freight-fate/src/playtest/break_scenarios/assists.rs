//! Driving assists fighting each other, or fighting the driver (port of
//! `tools/playtest_break_scenarios/assists.py`).
//!
//! Cruise, curve-speed assist, and descent control stacked through a mountain;
//! rapid engine-brake toggling to dodge a town no-jake ordinance; the facility
//! gate overshoot loop with destination approach assist on and off; and the
//! ramp handback both of Shane's 2026-08-15 reports live in.

use crate::playtest::breaker::{
    fabricated_curve, force_grade, outcome, Outcome, Rig, RigOptions, DT,
};
use crate::playtest::MPH_PER_MPS;
use crate::states::base::Key;
use crate::states::driving_core::hos_of;
use crate::states::driving_engine_brake::JAKE_ZONE_GRACE_S;
use crate::states::driving_facility_gate::GATE_MISS_LOOP_MIN;

use ff_core::models::profile::STARTING_MONEY;

/// Cruise + curve assist + descent control on a 6% grade with bends; count
/// cue spam.
pub fn assists_fight_descent() -> Outcome {
    let mut rig = Rig::new(RigOptions::default());
    let mut findings: Vec<String> = Vec::new();
    let start = 15.0;
    rig.drive.trip.position_mi = start;
    rig.drive.trip.curves = [1.5, 3.5, 5.5, 7.5]
        .iter()
        .enumerate()
        .map(|(i, off)| fabricated_curve(start + off, 35, if i % 2 == 0 { 'L' } else { 'R' }))
        .collect();
    force_grade(&mut rig.drive.trip, -0.06);
    rig.prepare(55.0, None);
    rig.press(Key::K); // arm automatic speed control at 55
    let mut stage_flips = 0;
    let mut last_stage = rig.drive.truck().engine_brake_stage;
    let mut seconds = 0.0;
    let mut frames = 0;
    while frames < 5400 && rig.drive.trip.position_mi < start + 9.0 {
        rig.advance_clock(DT);
        rig.drive.update_frame(&mut rig.app.ctx, DT);
        rig.app.ctx.run_deferred();
        frames += 1;
        seconds += DT;
        if rig.drive.truck().engine_brake_stage != last_stage {
            stage_flips += 1;
            last_stage = rig.drive.truck().engine_brake_stage;
        }
        if frames % 10 == 0 {
            rig.check_invariants();
        }
    }
    rig.check_invariants();
    let cue_count =
        rig.said("Curve assistance") + rig.said("Slowing for the bend") + rig.said("Descent");
    if seconds > 0.0 && cue_count as f64 / seconds > 0.2 {
        findings.push(format!(
            "assist cue spam: {cue_count} assist cues in {seconds:.0}s of descent"
        ));
    }
    if seconds > 0.0 && stage_flips as f64 / seconds > 0.5 {
        findings.push(format!(
            "retarder chatter: engine brake stage changed {stage_flips} times in {seconds:.0}s"
        ));
    }
    let speed = rig.drive.truck().speed_mph();
    if speed > 80.0 {
        findings.push(format!(
            "assists lost the mountain: {speed:.0} mph with cruise, curve assist, and descent \
             control all engaged"
        ));
    }
    let note = format!(
        "{cue_count} cues, {stage_flips} stage steps over {seconds:.0}s; held {speed:.0} mph"
    );
    outcome("assists_fight_descent", &rig, findings, &note)
}

/// Toggle the jake off at each warning in a ban zone: fines dodged forever,
/// warnings spam.
pub fn jake_toggle_fine_dodge() -> Outcome {
    let mut rig = Rig::new(RigOptions::default());
    let mut findings: Vec<String> = Vec::new();
    rig.drive.trip.position_mi = 2.0; // inside the Buffalo zone
    rig.drive.truck_mut().engine_on = true;
    rig.drive.truck_mut().transmission.gear = 8;
    rig.drive.truck_mut().velocity_mps = 55.0 / MPH_PER_MPS;
    force_grade(&mut rig.drive.trip, 0.0);
    let cycles = 12;
    for _ in 0..cycles {
        rig.drive.truck_mut().set_engine_brake(true);
        rig.drive.update_engine_brake_zone(&mut rig.app.ctx, 0.1); // warning fires
        rig.drive
            .update_engine_brake_zone(&mut rig.app.ctx, JAKE_ZONE_GRACE_S - 1.0); // bark through the grace
        rig.drive.truck_mut().set_engine_brake(false);
        rig.drive.update_engine_brake_zone(&mut rig.app.ctx, 0.1); // engagement ends: clean slate
        rig.app.ctx.run_deferred();
    }
    let warnings = rig.said("No engine brake");
    let fines = rig.drive.jake_zone_fines;
    if fines == 0 && warnings >= cycles {
        findings.push(format!(
            "jake toggled off just inside the grace {cycles} times: retarder barking ~90% of \
             the time in town, {warnings} warnings spoken, zero dollars fined -- the ordinance \
             is a rhythm game, and the warning repeats forever"
        ));
    } else if fines > 0 {
        let money_delta = STARTING_MONEY - rig.app.ctx.profile.as_ref().map_or(0.0, |p| p.money());
        if (money_delta - rig.drive.jake_fines_paid).abs() > 0.01 {
            findings.push(format!(
                "jake fines paid {:.0} but money moved {money_delta:.0}",
                rig.drive.jake_fines_paid
            ));
        }
    }
    let note = format!("{fines} fines, {warnings} warnings over {cycles} toggle cycles");
    outcome("jake_toggle_fine_dodge", &rig, findings, &note)
}

/// Carry past the facility gate at 70 with the approach assist off, then on;
/// verify both.
///
/// The missed-facility-gate loop-back, stressed under two assist
/// configurations: none (the miss should latch and charge time) and
/// `destination_approach_assist` (which should brake for the player and
/// prevent the miss outright). Also checks that HOS/fuel keep ticking honestly
/// through a loop, since the clock is the only consequence.
pub fn gate_overshoot_with_assists() -> Outcome {
    let mut rig = Rig::new(RigOptions::default());
    let mut findings: Vec<String> = Vec::new();
    rig.app.ctx.settings.destination_approach_assist = false;
    rig.drive.destination_exit_taken = true;
    rig.drive.trip.position_mi = rig.drive.trip.total_miles();
    rig.drive.trip.finished = true;
    rig.drive.truck_mut().engine_on = true;
    rig.drive.truck_mut().velocity_mps = 70.0 / MPH_PER_MPS;
    rig.drive.gate_speed_warned = true;
    rig.drive.gate_grace_s = 0.0;
    let minutes_before = rig.drive.trip.game_minutes;
    let driving_min_before = hos_of(&rig.app.ctx).driving_min;
    let fuel_before = rig.drive.truck().fuel_gal;
    rig.drive.handle_arrival_gate(&mut rig.app.ctx);
    rig.app.ctx.run_deferred();
    if rig.drive.trip.finished {
        findings.push("70 mph across the gate with no assist did not miss it at all".to_string());
    }
    if rig.drive.gate_miss_count != 1 {
        findings.push(format!(
            "expected exactly one miss recorded, got {}",
            rig.drive.gate_miss_count
        ));
    }
    let charged = rig.drive.trip.game_minutes - minutes_before;
    if (charged - GATE_MISS_LOOP_MIN).abs() > 1e-6 {
        findings.push(format!(
            "gate miss charged {charged:.1} min, expected {GATE_MISS_LOOP_MIN}"
        ));
    }
    if hos_of(&rig.app.ctx).driving_min == driving_min_before {
        findings.push(
            "the 20 lost minutes of the loop-back never touched the HOS driving clock: a \
             scripted reposition is free of hours-of-service cost"
                .to_string(),
        );
    }
    if rig.drive.truck().fuel_gal >= fuel_before {
        findings.push(
            "looping back through the safe turnaround burned zero fuel -- a scripted \
             reposition with no fuel cost while the player's odometer clearly moved"
                .to_string(),
        );
    }
    match rig.transcript().last() {
        Some(last) if last.to_lowercase().contains("slow to") => {}
        Some(last) => findings.push(format!(
            "miss message does not restate a target speed: {last}"
        )),
        None => findings.push("the missed gate said nothing at all".to_string()),
    }

    // Now the assist should own it and never miss.
    rig.app.ctx.settings.destination_approach_assist = true;
    rig.drive.destination_exit_taken = true;
    rig.drive.trip.position_mi = rig.drive.trip.total_miles();
    rig.drive.trip.finished = true;
    rig.drive.truck_mut().velocity_mps = 70.0 / MPH_PER_MPS;
    rig.drive.gate_speed_warned = true;
    rig.drive.gate_grace_s = 0.0;
    let miss_count_before = rig.drive.gate_miss_count;
    rig.drive.handle_arrival_gate(&mut rig.app.ctx);
    rig.app.ctx.run_deferred();
    if !rig.drive.trip.finished {
        findings.push(
            "destination_approach_assist enabled did not prevent a 70 mph gate miss".to_string(),
        );
    }
    if rig.drive.gate_miss_count != miss_count_before {
        findings.push("assist-owned approach still incremented the gate-miss counter".to_string());
    }
    if rig.drive.truck().brake != 1.0 {
        findings.push(format!(
            "assist claims to be braking the truck but truck.brake is {}",
            rig.drive.truck().brake
        ));
    }
    outcome(
        "gate_overshoot_with_assists",
        &rig,
        findings,
        "unassisted miss looped back honestly; the assist then owned the approach",
    )
}

/// Signal early at 20x, honor the ramp's stop bar, drive on: no early shed,
/// and speed control returns unaided.
///
/// Both of Shane's 2026-08-15 reports, driven end to end in real frames.
/// Signalling nine miles out used to start the shed immediately under time
/// compression, and taking the ramp used to kill adaptive cruise and the speed
/// keeper for the rest of the run -- only the resume key brought them back.
pub fn ramp_speed_control_handback() -> Outcome {
    const NAME: &str = "ramp_speed_control_handback";
    let mut rig = Rig::new(RigOptions::default());
    let mut findings: Vec<String> = Vec::new();
    // High pacing is the case the early shed showed up in; full lane keeping
    // pins the exit-lane mechanics that are not under test here.
    rig.app.ctx.settings.time_scale = 20.0;
    rig.drive.trip.time_scale = 20.0;
    rig.app.ctx.settings.route_transition_assist = true;
    rig.app.ctx.settings.speed_keeper = true;
    rig.app.ctx.settings.lane_keeping = "full".to_string();
    rig.drive.trip.curves.clear();
    force_grade(&mut rig.drive.trip, 0.0);
    let position = rig.drive.trip.position_mi;
    let (limit, _) = rig.drive.trip.speed_limit_at(position);
    rig.prepare(limit - 1.0, None);
    rig.press(Key::K); // automatic speed control on, at corridor speed
    let Some(cruise) = rig.drive.cruise_mph else {
        findings.push("cruise never armed at corridor speed".to_string());
        return outcome(NAME, &rig, findings, "");
    };

    // Signal as early as the game lets a driver signal, the way the report
    // did: X every half second until an exit takes it.
    let mut armed_mi: Option<f64> = None;
    // This scenario never touches the pedals, so anything that stops the truck
    // on the way -- automatic braking for a merging vehicle, say -- leaves it
    // parked for the rest of the frame budget and the run reports that no exit
    // ever arrived. The subject is the ramp handback, not surviving traffic,
    // so a stall gets the truck rolling again.
    let mut stalls = 0;
    for frame in 0..20000 {
        if let Some(stop) = rig.drive.exit_stop.as_ref() {
            armed_mi = Some(stop.at_mi - rig.drive.trip.position_mi);
            break;
        }
        if frame % 15 == 0 {
            rig.press(Key::X);
        }
        rig.advance_clock(DT);
        rig.drive.update_frame(&mut rig.app.ctx, DT);
        rig.app.ctx.run_deferred();
        if rig.drive.truck().speed_mph() < 3.0 && stalls < 20 {
            stalls += 1;
            rig.prepare(limit - 1.0, None);
            rig.press(Key::K);
        }
        if frame % 10 == 0 {
            rig.check_invariants();
        }
    }
    let Some(armed_mi) = armed_mi else {
        findings.push("no exit ever came within signalling range".to_string());
        return outcome(NAME, &rig, findings, "");
    };
    let exit_stop = rig.drive.exit_stop.clone().expect("the exit just armed");
    // The gore accepts road speed -- the deceleration lane exists so a driver
    // leaves at it and sheds inside it -- and the ramp's own number governs
    // from there (owner, 2026-08-21). Read it now, while the exit is still
    // armed: the helper falls back to the flat 45 without a stop.
    let gore_mph = rig.drive.gore_acceptance_mph(Some(&exit_stop));

    // 1. Far from the gore the signal itself must not slow the truck: the
    //    approach cap has to stay clear of the set speed.
    let mut early_shed_mi: Option<f64> = None;
    let mut stopped_short_mi: Option<f64> = None;
    let mut entry_mph: Option<f64> = None;
    for _ in 0..60000 {
        let ahead = exit_stop.at_mi - rig.drive.trip.position_mi;
        let cap = rig.drive.ramp_approach_cap_mph();
        if early_shed_mi.is_none() && ahead > 1.0 && cap.is_some_and(|cap| cap < cruise - 0.01) {
            early_shed_mi = Some(ahead);
        }
        if stopped_short_mi.is_none() && ahead > 0.0 && rig.drive.truck().speed_mph() <= 0.05 {
            stopped_short_mi = Some(ahead);
        }
        if rig.drive.ramp_mi.is_some() {
            entry_mph = Some(rig.drive.truck().speed_mph());
            break;
        }
        if ahead < -0.5 {
            break;
        }
        rig.advance_clock(DT);
        rig.drive.update_frame(&mut rig.app.ctx, DT);
        rig.app.ctx.run_deferred();
        rig.check_invariants();
    }
    if let Some(short) = stopped_short_mi {
        findings.push(format!(
            "came to a dead stop {short:.2} miles short of the gore, in the through lane: the \
             approach slowed the truck and then nothing drove it"
        ));
    }
    if let Some(shed) = early_shed_mi {
        findings.push(format!(
            "the exit cap fell under the {cruise:.0} mph set speed {shed:.1} miles from the \
             gore: signalling early is itself what slows the truck"
        ));
    }
    let Some(entry_mph) = entry_mph else {
        findings.push("the signaled exit was never taken at all".to_string());
        return outcome(NAME, &rig, findings, "");
    };
    if entry_mph > gore_mph {
        findings.push(format!(
            "entered the gore at {entry_mph:.1} mph, over the {gore_mph:.0} the gore accepts"
        ));
    }

    // 2. The ramp takes the pedals, never the session.
    if !rig.drive.speed_control_armed {
        findings.push(
            "taking the exit disarmed automatic speed control outright, so nothing can bring \
             it back but the resume key"
                .to_string(),
        );
    }
    // 3. Route-transition assistance brings the truck to the bar. Nothing may
    //    re-engage on the creep toward it.
    //
    //    The bar is pinned to a stop sign, which is what this run is about: a
    //    sign always stops the truck, and the handback is measured from that
    //    stop. Left to the map, the ramp's control is whatever this exit
    //    carries that day -- a seeded stop sign until 2026-09-17, then exit
    //    48A's own traffic light once stops read their interchange -- and a
    //    light that turns green releases a truck nobody is braking, which
    //    rolls past the entrance and off down the road. Lights have their own
    //    suite (`states_driving_ramps`).
    //    The crossroad goes with it: it was seeded for the control the ramp
    //    began with, and a sign waits for a gap in it.
    rig.drive.ramp_control = "stop".to_string();
    rig.drive.cross_bubble = None;
    rig.drive.ramp_waiting_at_light = false;
    rig.step(
        60000,
        DT,
        Some(&|rig: &Rig| rig.drive.ramp_terminal_done && rig.drive.truck().speed_mph() < 1.0),
    );
    if !rig.drive.ramp_terminal_done {
        findings.push("the ramp terminal never resolved".to_string());
    }
    if rig.drive.cruise_mph.is_some() || rig.drive.keeper_mph.is_some() {
        findings.push("automatic speed control re-engaged while still on the ramp".to_string());
    }

    // 4. Drive on from the bar -- past the plaza rather than into it, which is
    //    the "honor the bar and drive on" the report is about. No key but the
    //    throttle: speed control has to be live again once the ramp is behind
    //    the truck.
    rig.hold(Key::Up);
    rig.step(
        60000,
        DT,
        Some(&|rig: &Rig| rig.drive.ramp_mi.is_none() && rig.drive.truck().speed_mph() > 25.0),
    );
    rig.run_frames(120); // a couple of seconds of open road to hand it back on
    rig.release(Key::Up);
    if rig.drive.cruise_mph.is_none() && rig.drive.keeper_mph.is_none() {
        findings.push(
            "past the stop bar and back up to road speed with automatic speed control still \
             dead: the driver has to switch it on by hand"
                .to_string(),
        );
    }

    // 5. And the drive stayed sane by ear: no resume announced on the ramp,
    //    and the pause never announced itself twice.
    //    Counted as what the game announced, not what the voice received:
    //    where a ramp ends at a street speed zone, the zone line lands in the
    //    same frame as the return and cuts it, and the pacer finishes the cut
    //    line. That is one announcement, heard once, and the rig's transcript
    //    shows it twice.
    if rig.announced("Automatic speed control paused") > 1 {
        findings.push("the ramp pause announced itself more than once".to_string());
    }
    if rig.announced("resuming") > 1 {
        findings.push("automatic speed control announced its return more than once".to_string());
    }
    // A line handed back again and again is still a chant at the ear.
    for phrase in ["Automatic speed control paused", "resuming"] {
        let heard = rig.said(phrase);
        if heard > 2 * rig.announced(phrase).max(1) {
            findings.push(format!(
                "\"{phrase}\" reached the voice {heard} times: the pacer kept handing it back"
            ));
        }
    }

    let note = format!(
        "signalled {armed_mi:.1} mi out with no early shed, entered the gore at {entry_mph:.0} \
         mph, and speed control came back past the bar unaided"
    );
    outcome(NAME, &rig, findings, &note)
}
