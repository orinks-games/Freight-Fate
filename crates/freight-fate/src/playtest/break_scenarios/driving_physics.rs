//! Driving/physics abuse: floor it, reverse it, coast it, dynamite it (port
//! of `tools/playtest_break_scenarios/driving_physics.py`).
//!
//! Every scenario here puts the truck in a position no sane driver would
//! choose and checks whether the physics, the ledgers, and the spoken cues
//! stay honest about the consequences.

use crate::playtest::breaker::{
    fabricated_curve, force_grade, outcome, Outcome, Rig, RigOptions, DT,
};
use crate::states::base::Key;
use crate::states::driving_core::{hos_of, FLAT_SPOT_WEAR_PCT_PER_MPH};

use super::text::spoken_damage_percent;

use ff_core::sim::transmission::{NEUTRAL, REVERSE};
use ff_core::sim::vehicle::RUNAWAY_SPEED_MPH;

/// Flooring it must produce a real pull-over or nothing at all.
///
/// This scenario used to check a "speeding strike" ledger: hold nine over for
/// six seconds anywhere on the route and the drive banked a charge, billed at
/// the dock. That was a fine from an officer who was never there, and it is
/// gone. What replaces it is the harder question -- when the road DOES charge
/// you, was there a trooper, and did the player hear them?
pub fn floor_it_through_town() -> Outcome {
    let mut rig = Rig::new(RigOptions::default());
    let mut findings: Vec<String> = Vec::new();
    let money_before = rig
        .app
        .ctx
        .profile
        .as_ref()
        .map(|p| p.money())
        .unwrap_or(0.0);
    rig.prepare(30.0, None);
    rig.hold(Key::Up);
    let target = 25.0f64.min(rig.drive.trip.total_miles() - 8.0);
    rig.step(
        9000,
        DT,
        Some(&move |rig: &Rig| rig.drive.trip.position_mi >= target),
    );

    // Nothing may bank a charge behind the player's back any more.
    for ghost in ["Speeding strike", "due at delivery", "dollar maximum"] {
        if !rig.lines_with(ghost).is_empty() {
            findings.push(format!(
                "the removed silent speeding charge still speaks: {ghost:?}"
            ));
        }
    }
    let money_now = rig
        .app
        .ctx
        .profile
        .as_ref()
        .map(|p| p.money())
        .unwrap_or(0.0);
    let money_delta = money_now - money_before;
    let tickets = rig.drive.speeding_tickets;
    let fines = rig.drive.ticket_fines_paid;
    if money_delta < 0.0 && tickets == 0 {
        findings.push(format!(
            "money fell {:.0} with no traffic stop on the record: speeding nobody saw is \
             supposed to cost nothing",
            -money_delta
        ));
    }
    // A ticket is real money, so it has to have had a real officer behind it
    // -- and the player has to have heard them coming.
    if tickets != 0 {
        if rig.lines_with("Lights and siren behind you").is_empty() {
            findings.push(format!(
                "{tickets} ticket(s) written with no lights-and-siren call: the player was \
                 charged by an officer they never heard"
            ));
        }
        if (money_delta + fines).abs() > 0.01 {
            findings.push(format!(
                "ticket ledger mismatch: money moved {money_delta:+.0} but tickets total {fines:.0}"
            ));
        }
    }
    let every_post_heard = rig
        .drive
        .trip
        .posts
        .iter()
        .filter(|post| post.declined || !post.staffed)
        .all(|post| post.announced);
    if !every_post_heard {
        findings.push("a post acted on the driver before it had made a sound".to_string());
    }
    let note =
        format!("{tickets} traffic stop(s), {fines:.0} dollars, every one of them audible first");
    outcome("floor_it_through_town", &rig, findings, &note)
}

pub fn hairpin_at_70_no_assists() -> Outcome {
    let mut rig = Rig::new(RigOptions::default());
    let mut findings: Vec<String> = Vec::new();
    rig.app.ctx.settings.curve_speed_assist = false;
    rig.app.ctx.settings.route_transition_assist = false;
    let start = 12.0;
    rig.drive.trip.position_mi = start;
    rig.drive.trip.curves = vec![fabricated_curve(start + 1.0, 25, 'R')];
    rig.prepare(70.0, None);
    let damage_before = rig.drive.truck().damage_pct;
    rig.hold(Key::Up);
    rig.step(
        6000,
        DT,
        Some(&move |rig: &Rig| rig.drive.trip.position_mi >= start + 2.0),
    );
    if rig.said(", too fast. Slow to") == 0 {
        findings.push("no too-fast warning through a hairpin taken 45 over".to_string());
    }
    let min_speed = rig.drive.truck().speed_mph();
    let damage_delta = rig.drive.truck().damage_pct - damage_before;
    // 70 into a 25 hairpin asks far more than a loaded truck stays upright
    // at, so it goes over and the run carries on through road service.
    let rolled = rig.said("rolled over") > 0;
    if !rolled && damage_delta == 0.0 {
        findings.push(
            "blew a 25-advisory hairpin at 70 (assists and lane drift off): zero damage, no \
             crash, no spoken consequence beyond the warning -- the bend cannot hurt you"
                .to_string(),
        );
    }
    let note = if rolled {
        "hairpin at 70 rolled the truck over, after the warning".to_string()
    } else {
        format!(
            "hairpin punished the overspeed (damage +{damage_delta:.0}, min {min_speed:.0} mph)"
        )
    };
    outcome("hairpin_at_70_no_assists", &rig, findings, &note)
}

pub fn reverse_down_the_route() -> Outcome {
    let mut rig = Rig::new(RigOptions::default());
    let mut findings: Vec<String> = Vec::new();
    rig.drive.trip.position_mi = 1.0;
    rig.prepare(0.0, None);
    // The deliberate direction gesture: a FRESH brake press at a standstill,
    // held through the confirmation beat.
    rig.run_frames(5); // settle with no keys so the press below is a rising edge
    rig.hold(Key::Down);
    rig.run_frames(60); // > DIRECTION_CHANGE_HOLD_S at DT
    if !rig.drive.truck().transmission.in_reverse() {
        findings.push("standstill reverse gesture never engaged reverse".to_string());
        return outcome("reverse_down_the_route", &rig, findings, "");
    }
    let mut max_reverse_mph = 0.0f64;
    let mut backed_mi = 0.0;
    for _ in 0..7200 {
        rig.advance_clock(DT);
        rig.drive.update_frame(&mut rig.app.ctx, DT);
        rig.app.ctx.run_deferred();
        max_reverse_mph = max_reverse_mph.max(rig.drive.truck().speed_mph());
        backed_mi = 1.0 - rig.drive.trip.position_mi;
        rig.check_invariants();
        if rig.drive.trip.position_mi <= 0.0 {
            break;
        }
    }
    rig.run_frames(300); // keep backing against the route origin
    if max_reverse_mph > 11.0 {
        findings.push(format!(
            "reverse reached {max_reverse_mph:.1} mph (cap should be ~10)"
        ));
    }
    if rig.drive.trip.position_mi < 0.0 {
        findings.push("backed to a negative route position".to_string());
    }
    let wrongway = rig.transcript().into_iter().any(|line| {
        let lower = line.to_lowercase();
        lower.contains("wrong") || lower.contains("backward") || lower.contains("back the way")
    });
    if backed_mi >= 0.5 && !wrongway {
        findings.push(format!(
            "backed {backed_mi:.1} miles down the interstate (to route mile {:.2}) with no \
             wrong-way or off-route feedback of any kind after the reverse beep started -- a \
             blind player has no way to know the trip is unwinding",
            rig.drive.trip.position_mi
        ));
    }
    if hos_of(&rig.app.ctx).driving_min <= 0.0 {
        findings.push("reversing at 9 mph never counted as HOS driving time".to_string());
    }
    let note = format!(
        "clamped at mile {:.2}, reverse capped {max_reverse_mph:.1} mph",
        rig.drive.trip.position_mi
    );
    outcome("reverse_down_the_route", &rig, findings, &note)
}

pub fn dynamite_parking_brake_at_60() -> Outcome {
    let mut rig = Rig::new(RigOptions::default());
    let mut findings: Vec<String> = Vec::new();
    rig.drive.trip.position_mi = 12.0;
    rig.prepare(60.0, None);
    rig.run_frames(30);
    let wear_before = rig.drive.truck().tire_wear_pct;
    let speed = rig.drive.truck().speed_mph();
    rig.drive.toggle_parking_brake(&mut rig.app.ctx);
    rig.app.ctx.run_deferred();
    if rig.said("pulled the parking brake") == 0 {
        findings
            .push("no spoken dynamiting consequence for a parking brake pull at 60".to_string());
    }
    let expected = speed * FLAT_SPOT_WEAR_PCT_PER_MPH;
    let delta = rig.drive.truck().tire_wear_pct - wear_before;
    if (delta - expected).abs() > 0.3 {
        findings.push(format!(
            "flat-spot wear {delta:.2}% does not match the model ({expected:.2}%)"
        ));
    }
    if rig.drive.trip.waiting {
        findings.push("waiting fast-forward armed by a parking brake pulled at speed".to_string());
    }
    rig.step(
        1200,
        DT,
        Some(&|rig: &Rig| rig.drive.truck().speed_mph() < 0.5),
    );
    if rig.drive.truck().speed_mph() >= 0.5 {
        findings.push("spring brakes never brought the truck to a stop".to_string());
    }
    if rig.drive.trip.waiting {
        findings.push("waiting fast-forward armed itself after the dynamited stop".to_string());
    }
    let scale = rig.drive.trip.effective_time_scale();
    if scale > rig.drive.trip.time_scale {
        findings.push(format!(
            "parked clock is running at {scale:.0}x without deliberate waiting"
        ));
    }
    rig.hold(Key::Up);
    rig.run_frames(90);
    rig.release(Key::Up);
    if rig.said("Parking brake set") == 0 {
        findings
            .push("throttle against the set parking brake drew no spoken lockout cue".to_string());
    }
    let note =
        format!("flat-spotted {delta:.1}% tread, stopped, no fast-forward, lockout cue spoken");
    outcome("dynamite_parking_brake_at_60", &rig, findings, &note)
}

/// Manual box: grab reverse at 60 mph; a real driveline would grenade.
pub fn slam_reverse_at_speed() -> Outcome {
    let mut rig = Rig::new(RigOptions {
        automatic: false,
        ..RigOptions::default()
    });
    let mut findings: Vec<String> = Vec::new();
    rig.drive.trip.position_mi = 12.0;
    rig.prepare(60.0, Some(10));
    rig.hold(Key::LShift); // clutch in
    rig.run_frames(8);
    // Through TruckState, which is the path the gear keys take
    // (states/driving_controls). Calling Transmission::request_gear directly
    // reaches past the road-speed guard that lives one layer up, and reported
    // a bug no player can reach for the harness's first run.
    let damage_before = rig.drive.truck().damage_pct;
    let result = rig.drive.truck_mut().request_gear(REVERSE);
    rig.release(Key::LShift);
    if result.ok {
        findings.push(
            "reverse engaged at 60 mph forward with only the clutch pressed: no speed guard, \
             no grind, no driveline damage model"
                .to_string(),
        );
    } else if rig.drive.truck().damage_pct <= damage_before {
        findings.push(
            "reverse at 60 mph was refused but cost the driveline nothing: a real box would \
             be short some teeth for the attempt"
                .to_string(),
        );
    }
    rig.run_frames(900);
    if rig.drive.truck().transmission.in_reverse() && rig.drive.truck().velocity_mps > 1.0 {
        findings.push(format!(
            "rolling forward at {:.0} mph in reverse gear; over-rev wear is the only \
             consequence (engine wear {:.1}%)",
            rig.drive.truck().speed_mph(),
            rig.drive.truck().engine_wear_pct
        ));
    }
    let redline = rig.said("Redline") + rig.said("taking damage");
    if redline > 0 && rig.drive.truck().damage_pct == 0.0 {
        findings.push(format!(
            "redline warning says 'taking damage, now {:.0} percent' but over-rev only raises \
             engine WEAR -- the spoken damage number never moves",
            rig.drive.truck().damage_pct
        ));
    }
    outcome(
        "slam_reverse_at_speed",
        &rig,
        findings,
        "reverse at speed was refused or punished",
    )
}

/// Slam neutral on a 6% descent and ride it; how fast does it get, and does
/// anything object?
///
/// Python patched `Trip.grade_at` with a constant-slope lambda; here the slope
/// is BAKED onto the leg the rig already drives (see
/// [`force_grade`][crate::playtest::breaker::force_grade]), which is stricter
/// -- the whole road answers minus six percent rather than one method.
pub fn neutral_coast_mountain() -> Outcome {
    let mut rig = Rig::new(RigOptions {
        automatic: false,
        ..RigOptions::default()
    });
    let mut findings: Vec<String> = Vec::new();
    rig.drive.trip.position_mi = 12.0;
    rig.drive.trip.curves.clear();
    force_grade(&mut rig.drive.trip, -0.06);
    rig.prepare(55.0, Some(10));
    let result = rig.drive.truck_mut().transmission.request_gear(NEUTRAL); // neutral needs no clutch
    if !result.ok {
        findings.push(format!(
            "could not select neutral while rolling: {}",
            result.message
        ));
    }
    // Reaching three figures in neutral down a long grade is not the bug --
    // that IS a runaway, and the sim is right to allow it. What would be a bug
    // is a truck that survives one. So check for the consequence rather than
    // asserting its absence.
    let mut max_mph = 0.0f64;
    let damage_before = rig.drive.truck().damage_pct;
    for _ in 0..5400 {
        rig.advance_clock(DT);
        rig.drive.update_frame(&mut rig.app.ctx, DT);
        rig.app.ctx.run_deferred();
        max_mph = max_mph.max(rig.drive.truck().speed_mph());
        rig.check_invariants();
        if rig.drive.trip.position_mi >= rig.drive.trip.total_miles() - 8.0 {
            break;
        }
    }
    let runaway = max_mph > RUNAWAY_SPEED_MPH;
    let took_damage = rig.drive.truck().damage_pct > damage_before
        || !rig.lines_with("Out of service").is_empty()
        || !rig.lines_with("Limp mode").is_empty();
    if runaway && !took_damage {
        findings.push(format!(
            "neutral coast reached {max_mph:.0} mph, past the {RUNAWAY_SPEED_MPH:.0} mph runaway \
             threshold, and the truck took no damage at all: tires past their rated speed and \
             an unloaded driveline cost nothing"
        ));
    }
    let strike_count = rig.said("Speeding strike");
    // Try to stop it on the service brakes alone from whatever speed remains.
    rig.hold(Key::Down);
    let stopped = rig.step(
        2700,
        DT,
        Some(&|rig: &Rig| rig.drive.truck().speed_mph() < 5.0),
    );
    if rig.drive.truck().speed_mph() >= 5.0 {
        findings.push(format!(
            "service brakes could not stop the neutral runaway ({:.0} mph after {:.0}s of full \
             brake, drums {:.0}C)",
            rig.drive.truck().speed_mph(),
            stopped as f64 * DT,
            rig.drive.truck().brake_temp_c
        ));
    }
    let note = format!("peaked {max_mph:.0} mph, {strike_count} strikes, brakes recovered it");
    outcome("neutral_coast_mountain", &rig, findings, &note)
}

/// Force a road-driven over-rev; the warning quotes a damage number that must
/// be honest.
pub fn redline_damage_readout() -> Outcome {
    let mut rig = Rig::new(RigOptions {
        automatic: false,
        ..RigOptions::default()
    });
    let mut findings: Vec<String> = Vec::new();
    rig.drive.trip.position_mi = 12.0;
    rig.drive.trip.curves.clear();
    force_grade(&mut rig.drive.trip, -0.10);
    rig.prepare(50.0, None);
    // Pick the tallest gear that is already past damaging revs at this speed.
    let max_rpm = rig.drive.truck().specs.max_rpm;
    let gear = (1..=10)
        .rev()
        .find(|g| rig.drive.truck().coupled_rpm(Some(*g)) > max_rpm * 1.06)
        .unwrap_or(3);
    rig.drive.truck_mut().transmission.gear = gear;
    let wear_before = rig.drive.truck().engine_wear_pct;
    rig.step(
        600,
        DT,
        Some(&|rig: &Rig| rig.said("Redline") + rig.said("redline") > 0),
    );
    let mut redline_lines = rig.lines_with("redline");
    redline_lines.extend(rig.lines_with("Redline"));
    if redline_lines.is_empty() {
        findings.push("sustained over-rev never drew a spoken redline warning".to_string());
        return outcome("redline_damage_readout", &rig, findings, "");
    }
    let spoken_damage = spoken_damage_percent(&redline_lines[0]);
    let wear_gained = rig.drive.truck().engine_wear_pct - wear_before;
    let damage_pct = rig.drive.truck().damage_pct;
    if let Some(spoken) = spoken_damage {
        if (spoken - damage_pct).abs() > 1.0 {
            findings.push(format!(
                "redline warning spoke {spoken:.0}% damage but damage is {damage_pct:.0}%"
            ));
        }
    }
    if wear_gained > 0.1 && damage_pct == 0.0 && redline_lines[0].to_lowercase().contains("damage")
    {
        findings.push(format!(
            "redline warning says the engine is taking damage, now 0 percent, while the harm \
             actually lands on engine WEAR (+{wear_gained:.1}%) -- the spoken number will read 0 \
             forever, telling a blind player the abuse is free"
        ));
    }
    outcome(
        "redline_damage_readout",
        &rig,
        findings,
        "redline warning quotes a number that tracks the real harm",
    )
}
