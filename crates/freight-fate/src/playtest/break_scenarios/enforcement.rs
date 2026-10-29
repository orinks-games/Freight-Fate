//! Enforcement guidance under deliberate mis-play (port of
//! `tools/playtest_break_scenarios/enforcement.py`).
//!
//! Tester Jerry's weigh-station report (2026-08-12): the open-scale
//! announcement said "press T", T at speed planned a sleep stop past the
//! scale, X then armed that truck stop's exit, and following both spoken
//! instructions crossed the scale unarmed at speed straight into a bypass
//! pull-over -- with the exit assist still steering for the truck-stop ramp
//! under the trooper's lights. This family drives the fixed contract end to
//! end with the real driving state.

use crate::playtest::breaker::{outcome, Outcome, Rig, RigOptions, DT};
use crate::states::base::Key;
use crate::states::driving_border::BorderClearanceState;
use crate::states::driving_enforcement::SCALE_REMINDER_REAL_LEAD_S;
use crate::states::driving_rest_states::EnforcementStopState;

use ff_core::sim::enforcement_posts::{
    EnforcementPost, KIND_CMV, KIND_FIXED_SCALE, METHOD_SCALE_SCREEN, METHOD_VISUAL,
};
use ff_core::sim::roadside_inspection::TIRE_OUT_OF_SERVICE_PCT;
use ff_core::sim::trip::Trip;
use ff_core::sim::trip_models::RoadStop;

pub const SCALE_MI: f64 = 4.0;
pub const TRUCKSTOP_MI: f64 = 5.0;

/// One open scale with a sleep-capable travel center just past it.
pub fn inject_open_scale(trip: &mut Trip) -> (RoadStop, RoadStop) {
    let mut scale = RoadStop::new("Hamburg Scale", SCALE_MI, "weigh_station");
    scale.actions = vec!["inspect".to_string()];
    scale.parking = "none".to_string();
    let mut truckstop = RoadStop::new("Blue Beacon Travel Plaza", TRUCKSTOP_MI, "travel_center");
    truckstop.actions = ["park", "save", "fuel", "food", "break", "sleep"]
        .iter()
        .map(|action| action.to_string())
        .collect();
    truckstop.parking = "confirmed".to_string();
    truckstop.exit_label = "exit 55".to_string();
    trip.stops = vec![scale.clone(), truckstop.clone()];
    let mut post = EnforcementPost::new(SCALE_MI, KIND_FIXED_SCALE);
    post.method = METHOD_SCALE_SCREEN.to_string();
    post.reach_mi = 1.0;
    post.staffed = true;
    post.anchor = scale.key();
    trip.posts = vec![post];
    (scale, truckstop)
}

/// Follow the scale announcement literally -- T mid-warning, then X: both must
/// serve the scale.
pub fn scale_check_in_guidance() -> Outcome {
    let mut rig = Rig::new(RigOptions::default());
    let mut findings: Vec<String> = Vec::new();
    let (scale, _truckstop) = inject_open_scale(&mut rig.drive.trip);
    // Partial until the X press: on full, lane keeping signals for the scale
    // itself, and this scenario is about the driver's own press.
    rig.app.ctx.settings.lane_keeping = "partial".to_string();
    rig.prepare(54.0, None);
    rig.press(Key::K); // cruise at the limit: the scale is the only demand

    let ran = rig.step(
        20000,
        DT,
        Some(&|rig: &Rig| rig.said("Open weigh station") > 0),
    );
    if rig.said("Open weigh station") == 0 {
        findings.push(format!("no open-scale announcement in {ran} frames"));
        return outcome("scale_check_in_guidance", &rig, findings, "");
    }
    let notice = rig.lines_with("Open weigh station")[0].clone();
    if notice.contains("press T for inspection check-in") {
        findings.push("the announcement still teaches the rest key at speed".to_string());
    }
    // The notice stopped naming keys on 2026-10-17; the scale's own stop
    // callout and the reminder teach the exit key.

    // Jerry's press: T immediately, mid-announcement in real audio.
    rig.press(Key::T);
    if rig.said("Sleep stop planned") > 0 {
        findings.push(
            "T mid scale warning planned a sleep stop past the scale instead of deferring"
                .to_string(),
        );
    }
    if rig.said("Weigh station first") == 0 {
        findings.push("T mid scale warning never said the scale comes first".to_string());
    }

    // Then X, following the announcement's exit instruction. Full lane keeping
    // pins the exit-lane mechanics out of the way: this scenario is about
    // WHICH exit the press serves, not lane work.
    rig.app.ctx.settings.lane_keeping = "full".to_string();
    rig.press(Key::X);
    match rig.drive.exit_stop.as_ref() {
        None => findings.push("X after the scale warning armed no exit at all".to_string()),
        Some(_) if !rig.drive.exit_signal_on => {
            findings.push("X after the scale warning armed no exit at all".to_string())
        }
        Some(stop) if stop.key() != scale.key() => findings.push(format!(
            "X armed the exit for {}, not the scale's inspection lane",
            stop.name
        )),
        Some(_) => {}
    }

    let lines_after_x = rig.transcript().len();
    // Ride the capped cruise down to the gore. Compliance must end on the
    // scale's own ramp with no enforcement lights anywhere in the run.
    rig.step(
        20000,
        DT,
        Some(&|rig: &Rig| {
            rig.drive.ramp_mi.is_some()
                || rig.drive.pull_over.is_some()
                || rig.drive.trip.position_mi > SCALE_MI + 0.3
        }),
    );
    if rig.transcript()[lines_after_x..]
        .iter()
        .any(|line| line.contains("to signal for the exit"))
    {
        findings.push("with the exit signaled, a callout still asked for the signal".to_string());
    }
    if rig.drive.pull_over.is_some() || rig.said("Scale bypass enforcement") > 0 {
        findings.push(
            "following both spoken instructions still ended in a bypass pull-over".to_string(),
        );
    }
    let on_scale_ramp = rig
        .drive
        .ramp_stop
        .as_ref()
        .is_some_and(|stop| stop.key() == scale.key());
    if !on_scale_ramp {
        findings.push(format!(
            "the truck never ended up on the scale's own ramp (position {:.2}, {:.0} mph)",
            rig.drive.trip.position_mi,
            rig.drive.truck().speed_mph()
        ));
    }
    outcome(
        "scale_check_in_guidance",
        &rig,
        findings,
        "T deferred to the scale, X armed the inspection lane, no bypass charge",
    )
}

/// Floor it through the Alcan port, take the penalty, clear customs, then
/// merge back onto the highway.
pub fn run_the_border() -> Outcome {
    let mut rig = Rig::new_for_route(RigOptions::default(), "Whitehorse", "Tok");
    let mut findings = Vec::new();
    rig.prepare(60.0, None);
    let booth = rig
        .drive
        .trip
        .border_booths
        .iter()
        .find(|booth| booth.name == "Alcan Port of Entry")
        .cloned()
        .expect("the Whitehorse-to-Tok trip has the Alcan booth");
    rig.drive.trip.position_mi = booth.at_mi - 0.0001;
    let money_before = rig
        .app
        .ctx
        .profile
        .as_ref()
        .map(|profile| profile.money())
        .unwrap_or(0.0);
    let (stop_opened, clearance_opened, continued) = rig.with_drive_on_stack(|rig, drive| {
        let _ = drive.with(&mut rig.app.ctx, |drive, ctx| {
            drive.trip.truck.velocity_mps = 60.0 / 2.23694;
            drive.update_frame(ctx, DT);
        });
        rig.app.ctx.run_deferred();
        for _ in 0..5 {
            let _ = drive.with(&mut rig.app.ctx, |drive, ctx| {
                if drive.pull_over.is_some() {
                    drive.trip.truck.velocity_mps = 0.0;
                    drive.update_pull_over(ctx, 1.0, false);
                }
            });
            rig.app.ctx.run_deferred();
            if rig.app.ctx.state().is_some_and(|state| {
                state
                    .borrow()
                    .as_any()
                    .downcast_ref::<EnforcementStopState>()
                    .is_some()
            }) {
                break;
            }
        }
        let stop_opened = rig.app.ctx.state().is_some_and(|state| {
            state
                .borrow()
                .as_any()
                .downcast_ref::<EnforcementStopState>()
                .is_some()
        });
        if !stop_opened {
            return (false, false, false);
        }

        rig.select_menu_containing("Pull back onto the highway");
        let clearance_opened = rig.app.ctx.state().is_some_and(|state| {
            state
                .borrow()
                .as_any()
                .downcast_ref::<BorderClearanceState>()
                .is_some()
        });
        if !clearance_opened || !rig.select_menu_containing("Answer the officer's questions") {
            return (true, clearance_opened, false);
        }

        let continued_from = drive
            .read(|drive| drive.trip.position_mi)
            .unwrap_or(booth.at_mi);
        let _ = drive.with(&mut rig.app.ctx, |drive, _| {
            drive.trip.truck.release_parking_brake();
            drive.trip.truck.parking_brake = false;
            drive.trip.truck.throttle = 0.5;
            drive.trip.truck.velocity_mps = 30.0 / 2.23694;
        });
        for _ in 0..10 {
            rig.advance_clock(DT);
            let _ = drive.with(&mut rig.app.ctx, |drive, ctx| {
                drive.update_frame(ctx, DT);
            });
            rig.app.ctx.run_deferred();
        }
        let continued = drive
            .read(|drive| drive.trip.position_mi > continued_from)
            .unwrap_or(false);
        (true, true, continued)
    });

    if !stop_opened {
        findings.push("the port-running pull-over did not open".to_string());
    }
    if stop_opened && rig.drive.pull_over.is_some() {
        findings.push("the port-running pull-over remained unresolved".to_string());
    }
    if stop_opened && !clearance_opened {
        findings.push("the pull-over returned without opening customs clearance".to_string());
    }
    if clearance_opened && !continued {
        findings.push("the truck did not continue after customs cleared it".to_string());
    }
    let fine = money_before
        - rig
            .app
            .ctx
            .profile
            .as_ref()
            .map(|profile| profile.money())
            .unwrap_or(money_before);
    if (fine - 5_000.0).abs() > 1e-6 {
        findings.push(format!(
            "the Alcan stop charged {fine:.0} dollars, not 5000"
        ));
    }
    let port_summary = rig.lines_with("stopped the truck for running the customs booth");
    if !port_summary.iter().any(|line| {
        line.contains("Alcan Port of Entry")
            && line.contains("U.S. Customs and Border Protection")
            && line.contains("5,000 US dollar civil penalty")
    }) {
        findings.push("the port-running summary omitted its booth, agency or penalty".to_string());
    }
    if rig.said("Officers walk you back to the customs booth.") == 0 {
        findings.push("the return-to-booth message was not spoken".to_string());
    }
    let note = format!(
        "Alcan first-offence fine ${fine:.0}; stop opened {stop_opened}; pull-over resolved {}; driving continued {continued}",
        rig.drive.pull_over.is_none()
    );
    outcome("run_the_border", &rig, findings, &note)
}

/// Blow past the scale with a truck-stop exit armed: the pull-over must own
/// the road.
pub fn scale_pull_over_stands_down_exit() -> Outcome {
    let mut rig = Rig::new(RigOptions::default());
    let mut findings: Vec<String> = Vec::new();
    let (scale, truckstop) = inject_open_scale(&mut rig.drive.trip);
    rig.prepare(54.0, None);
    // Recreate the pre-fix wreckage by force: exit armed for the truck stop
    // (not the scale), then blow the scale at speed.
    rig.drive.trip.position_mi = SCALE_MI - 0.1;
    // A truck that drove the approach heard the notice and, half a mile
    // out, the reminder more than its real seconds ago: the crossing is
    // the driver's to answer for.
    let key = rig.drive.weigh_station_key(&scale);
    rig.drive.weigh_station_noticed.insert(key.clone());
    rig.drive.weigh_station_reminder_key = key;
    rig.drive.weigh_station_reminder_age_s = SCALE_REMINDER_REAL_LEAD_S;
    rig.drive.enforcement_prev_mi = rig.drive.trip.position_mi;
    rig.drive.exit_stop = Some(truckstop);
    rig.drive.exit_signal_on = true;
    rig.drive.cruise_exit_mph = Some(40.0);
    // The armed exit puts this approach on the real clock. A tenth of a
    // mile at 54 mph takes nearly seven seconds, so sixty frames (two
    // seconds) cannot establish that the truck actually crossed the scale.
    rig.step(
        600,
        DT,
        Some(&|rig: &Rig| {
            rig.drive.pull_over.is_some() || rig.drive.trip.position_mi > SCALE_MI + 0.2
        }),
    );
    if rig.drive.pull_over.is_none() {
        findings.push("an unarmed 54 mph scale crossing was never charged".to_string());
    }
    if rig.drive.exit_signal_on || rig.drive.exit_stop.is_some() {
        findings.push(
            "the armed truck-stop exit kept announcing and steering during the pull-over"
                .to_string(),
        );
    }
    if rig.drive.cruise_exit_mph.is_some() {
        findings.push("the ramp cruise cap survived into the trooper stop".to_string());
    }
    outcome(
        "scale_pull_over_stands_down_exit",
        &rig,
        findings,
        "the pull-over stood the armed exit down; one demand on the driver",
    )
}

/// A row of staffed commercial-vehicle units on the shoulder, each able to
/// look the truck over as it passes. Several, because whether one looks is
/// a seeded roll and one post would report nothing on most seeds.
fn staffed_cmv_units(trip: &mut Trip, miles: &[f64]) {
    trip.posts = miles
        .iter()
        .map(|mi| {
            let mut post = EnforcementPost::new(*mi, KIND_CMV);
            post.method = METHOD_VISUAL.to_string();
            post.reach_mi = 0.6;
            post.staffed = true;
            post
        })
        .collect();
}

/// Roll past a row of commercial-vehicle units on bald tires at the limit:
/// one of them must see the tread and pull the truck in for a walk-around
/// that names the tire, replaces it, and lets the truck go.
pub fn bald_tires_get_a_walk_around() -> Outcome {
    let mut rig = Rig::new(RigOptions {
        keep_patrols: true,
        ..RigOptions::default()
    });
    let mut findings: Vec<String> = Vec::new();
    staffed_cmv_units(&mut rig.drive.trip, &[3.0, 4.5, 6.0, 7.5, 9.0, 10.5]);
    rig.drive.truck_mut().tire_wear_pct = TIRE_OUT_OF_SERVICE_PCT + 5.0;
    rig.prepare(60.0, None);
    rig.press(Key::K);

    rig.step(
        30000,
        DT,
        Some(&|rig: &Rig| rig.drive.pull_over.is_some() || rig.drive.trip.position_mi > 12.0),
    );
    if rig.drive.pull_over.is_none() {
        findings.push(
            "six staffed commercial-vehicle units let a truck on bald tires roll past".to_string(),
        );
        return outcome("bald_tires_get_a_walk_around", &rig, findings, "");
    }
    if rig.drive.pull_over_kind != "roadside_walkaround" {
        findings.push(format!(
            "the stop was {:?}, not the walk-around the tread should have drawn",
            rig.drive.pull_over_kind
        ));
    }
    if rig.said("tires worn") == 0 {
        findings.push("the lights line never said what the trooper saw".to_string());
    }
    // Do as told: signal, stop, and take the inspection.
    rig.press(Key::X);
    rig.drive.truck_mut().velocity_mps = 0.0;
    rig.step(600, DT, Some(&|rig: &Rig| rig.drive.pull_over.is_none()));
    if rig.said("Level 2 walk-around inspection") == 0 {
        findings.push("the stop never ran the Level 2".to_string());
    }
    if rig.said("a tire below the minimum tread depth") == 0 {
        findings.push("the walk-around did not write up the tire it was pulled in for".to_string());
    }
    if rig.said("fitted new tires") == 0 {
        findings.push("the out-of-service tire was never replaced".to_string());
    }
    let wear = rig.drive.truck().tire_wear_pct;
    if wear > 1.0 {
        findings.push(format!(
            "tire wear is still {wear:.0} percent after the repair"
        ));
    }
    let note = format!(
        "stop kind {:?}; tire wear after {wear:.0} percent",
        rig.drive.pull_over_kind
    );
    outcome("bald_tires_get_a_walk_around", &rig, findings, &note)
}
