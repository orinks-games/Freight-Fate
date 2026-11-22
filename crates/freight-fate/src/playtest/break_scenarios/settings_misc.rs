//! Settings-flip and cross-cutting abuse: units, transmission, waiting,
//! hazards (port of `tools/playtest_break_scenarios/settings_misc.py`).
//!
//! Flipping transmission mode and units mid-drive, deliberate parked waiting's
//! double-speed clock, and ignoring hazards all the way to a totaled truck.

use crate::playtest::breaker::{outcome, Outcome, Rig, RigOptions, DT};
use crate::states::base::Key;
use crate::states::driving_core::hos_of;

use ff_core::sim::trip_models::{TripEvent, TripEventData, TripEventKind};
use ff_core::speech_text::SpokenMessage;

use super::text::leading_percent;

/// Flip transmission and units at 55 mph; the cab must announce and switch
/// everywhere.
pub fn settings_flips_mid_drive() -> Outcome {
    let mut rig = Rig::new(RigOptions::default());
    let mut findings: Vec<String> = Vec::new();
    rig.drive.trip.position_mi = 12.0;
    rig.prepare(55.0, None);
    rig.run_frames(30);
    let gear_before = rig.drive.truck().transmission.gear;
    let speed_before = rig.drive.truck().speed_mph();
    rig.app.ctx.settings.automatic_transmission = false;
    rig.run_frames(2);
    if rig.said("Transmission changed to manual") != 1 {
        findings.push("mid-drive flip to manual was not announced exactly once".to_string());
    }
    if rig.drive.truck().transmission.automatic {
        findings.push("settings flip did not reach the transmission".to_string());
    }
    let gear_now = rig.drive.truck().transmission.gear;
    if (gear_now - gear_before).abs() > 1 {
        findings.push(format!(
            "flipping to manual jumped the gear {gear_before} -> {gear_now}"
        ));
    }
    if (rig.drive.truck().speed_mph() - speed_before).abs() > 5.0 {
        findings.push("flipping the transmission changed the truck's speed".to_string());
    }
    rig.app.ctx.settings.automatic_transmission = true;
    rig.run_frames(2);

    rig.drive.speak_speed_limit(&mut rig.app.ctx);
    rig.app.ctx.run_deferred();
    let imperial_line = rig.transcript().last().cloned().unwrap_or_default();
    if !imperial_line.contains("miles per hour") && !imperial_line.contains("mile") {
        findings.push(format!(
            "imperial limit readout has no imperial units: {imperial_line}"
        ));
    }
    rig.app
        .ctx
        .settings
        .set_units_mode(ff_core::units::UnitsMode::Metric);
    rig.run_frames(2);
    rig.drive.speak_speed_limit(&mut rig.app.ctx);
    rig.app.ctx.run_deferred();
    let metric_line = rig.transcript().last().cloned().unwrap_or_default();
    if !metric_line.contains("kilometers per hour") {
        findings.push(format!("metric limit readout is not metric: {metric_line}"));
    }
    if metric_line.contains("miles per hour") {
        findings.push(format!("metric readout still speaks miles: {metric_line}"));
    }
    rig.drive.last_announced_mph = 0.0;
    rig.drive.speed_announce_timer = 1e9;
    rig.run_frames(2);
    let transcript = rig.transcript();
    let recent = &transcript[transcript.len().saturating_sub(3)..];
    if !recent
        .iter()
        .any(|line| line.contains("kilometers per hour"))
    {
        findings.push("routine speed announcements did not switch to metric".to_string());
    }
    outcome(
        "settings_flips_mid_drive",
        &rig,
        findings,
        "transmission and unit flips announced and applied everywhere checked",
    )
}

/// Deliberate parked waiting: the double-speed clock must bill time honestly.
pub fn waiting_time_warp() -> Outcome {
    let mut rig = Rig::new(RigOptions::default());
    let mut findings: Vec<String> = Vec::new();
    rig.drive.trip.position_mi = 12.0;
    rig.prepare(0.0, None);
    rig.drive.toggle_parking_brake(&mut rig.app.ctx);
    rig.app.ctx.run_deferred();
    if !rig.drive.trip.waiting {
        findings.push("player-set parking brake did not arm waiting".to_string());
    }
    let gm_before = rig.drive.trip.game_minutes;
    let duty_before = hos_of(&rig.app.ctx).duty_min;
    rig.run_frames(300); // 10 real seconds
    let gm = rig.drive.trip.game_minutes - gm_before;
    let expected = 300.0 * DT * rig.drive.trip.time_scale * 2.0 / 60.0;
    if (gm - expected).abs() > expected * 0.05 {
        findings.push(format!(
            "waiting advanced {gm:.2} game-min in 10s; double pacing predicts {expected:.2}"
        ));
    }
    let duty_gained = hos_of(&rig.app.ctx).duty_min - duty_before;
    if (duty_gained - gm).abs() > 0.2f64.max(gm * 0.05) {
        findings.push(format!(
            "trip clock moved {gm:.2} game-min while the HOS ledger logged {duty_gained:.2} -- \
             the two clocks disagree while waiting"
        ));
    }
    rig.drive.toggle_parking_brake(&mut rig.app.ctx); // release
    rig.app.ctx.run_deferred();
    if rig.drive.trip.waiting {
        findings.push("releasing the parking brake left waiting armed".to_string());
    }
    let note = format!("waiting billed {gm:.1} game-min in 10s, HOS ledger matched");
    outcome("waiting_time_warp", &rig, findings, &note)
}

/// No AEB, never brake for hazards; collision math, spoken damage, and what
/// 100% allows.
pub fn hazard_ignored_to_100_damage() -> Outcome {
    let mut rig = Rig::new(RigOptions::default());
    let mut findings: Vec<String> = Vec::new();
    rig.app.ctx.settings.automatic_emergency_braking = false;
    rig.drive.trip.position_mi = 12.0;
    rig.drive.trip.curves.clear();
    rig.prepare(70.0, None);
    rig.hold(Key::Up);
    let mut collisions = 0;
    // Each spoken number is checked against the damage the truck carried at
    // the moment it was said. Comparing the LAST line to the truck's state at
    // the END of the run is what made the harness's first pass report a lie
    // that was not one: this scenario drives the truck to out of service, and
    // the automatic roadside rescue then patches it down to
    // BREAKDOWN_REPAIR_DAMAGE_PCT, long after the line was honest.
    let mut spoken_vs_actual: Vec<(f64, f64)> = Vec::new();
    // And whether "Brake!" ever came back AFTER the truck hit the thing. The
    // warning is a ROUTE-and-up line, so the collision's own interrupt offers
    // it the rescue slot; its validity gate is what has to refuse, because by
    // then there is no hazard left to answer. Counted per pass, after the
    // warning for that pass has already been spoken, so anything the crash
    // step adds is a replay.
    let mut replays_after_impact = 0;
    for _ in 0..16 {
        if rig.drive.truck().damage_pct >= 100.0 {
            break;
        }
        let event = TripEvent {
            kind: TripEventKind::Hazard,
            message: SpokenMessage::new("Debris on the road ahead. Brake!"),
            data: TripEventData::default(),
        };
        rig.drive.handle_trip_event(&mut rig.app.ctx, &event);
        rig.app.ctx.run_deferred();
        let before = collisions;
        let seen_lines = rig.lines_with("Total damage").len();
        let warnings_before = rig.lines_with("Debris on the road ahead").len();
        rig.step(
            1200,
            DT,
            Some(&move |rig: &Rig| rig.said("Collision!") > before),
        );
        collisions = rig.said("Collision!");
        replays_after_impact += rig.lines_with("Debris on the road ahead").len() - warnings_before;
        if collisions == before {
            break;
        }
        let damage_now = rig.drive.truck().damage_pct;
        for line in rig.lines_with("Total damage").into_iter().skip(seen_lines) {
            if let Some((_, rest)) = line.split_once("Total damage ") {
                if let Some(said) = leading_percent(rest) {
                    spoken_vs_actual.push((said, damage_now));
                }
            }
        }
        rig.step(
            600,
            DT,
            Some(&|rig: &Rig| rig.drive.truck().speed_mph() >= 55.0),
        ); // power back up
    }
    for (said_pct, actual_pct) in &spoken_vs_actual {
        if (said_pct - actual_pct.round()).abs() > 1.0 {
            findings.push(format!(
                "a collision spoke {said_pct:.0}% total damage while the truck was at \
                 {actual_pct:.0}%"
            ));
            break;
        }
    }
    if collisions == 0 {
        findings.push("ignored hazards never produced a collision".to_string());
    }
    if replays_after_impact > 0 {
        findings.push(format!(
            "{replays_after_impact} hazard warnings were spoken again after the truck had \
             already hit the hazard, telling the driver to brake for something that had \
             already happened"
        ));
    }
    let damage = rig.drive.truck().damage_pct;
    if damage > 100.0 {
        findings.push(format!("damage exceeded its cap: {damage}"));
    }
    if damage >= 100.0 {
        rig.run_frames(300);
        if rig.drive.truck().speed_mph() > 30.0 && rig.drive.pull_over.is_none() {
            findings.push(
                "at 100% damage the wreck still cruises at highway speed; the unsafe-equipment \
                 stop only exists inside patrol windows, so on an empty road a totaled truck is \
                 street-legal forever"
                    .to_string(),
            );
        }
    }
    let note = format!(
        "{collisions} collisions to {:.0}% damage, all spoken honestly",
        rig.drive.truck().damage_pct
    );
    outcome("hazard_ignored_to_100_damage", &rig, findings, &note)
}
