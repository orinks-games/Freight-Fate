//! Full lane keeping and the open scale: on All assists the truck signals
//! for an open scale that wants it and stops itself at the scale bar, the
//! way it takes the destination exit. Signalling was the one lane job All
//! assists still left to the player, and a missed scale is a bypass stop.

use ff_core::sim::enforcement_posts::{EnforcementPost, KIND_FIXED_SCALE, METHOD_SCALE_SCREEN};
use ff_core::sim::trip_models::RoadStop;

use freight_fate::playtest::breaker::{Rig, RigOptions, DT};

const MPS_PER_MPH: f64 = 1.0 / 2.23694;
const SCALE_MI: f64 = 6.0;
const NAME: &str = "I-40 Weigh Station";

fn open_scale(rig: &mut Rig) -> RoadStop {
    let mut stop = RoadStop::new(NAME, SCALE_MI, "weigh_station");
    stop.actions = vec!["inspect".to_string()];
    stop.parking = "none".to_string();
    let mut post = EnforcementPost::new(stop.at_mi, KIND_FIXED_SCALE);
    post.method = METHOD_SCALE_SCREEN.to_string();
    post.reach_mi = 1.0;
    post.staffed = true;
    post.anchor = stop.key();
    rig.drive.trip.stops = vec![stop.clone()];
    rig.drive.trip.posts = vec![post];
    stop
}

fn rig_on(lane_keeping: &str) -> Rig {
    let mut rig = Rig::new(RigOptions {
        seed: 1,
        ..RigOptions::default()
    });
    rig.app.ctx.settings.lane_keeping = lane_keeping.to_string();
    rig.app.ctx.settings.destination_approach_assist = true;
    rig.app.ctx.settings.exit_speed_assist = true;
    rig
}

#[test]
fn full_lane_keeping_signals_for_an_open_scale_and_stops_at_it() {
    let mut rig = rig_on("full");
    let stop = open_scale(&mut rig);
    rig.prepare(55.0, None);
    rig.drive.trip.truck.velocity_mps = 55.0 * MPS_PER_MPH;
    rig.drive.engage_cruise(&mut rig.app.ctx, 55.0, false);
    let mut on_scale_ramp = false;
    let mut stopped_at_scale = false;
    for _ in 0..60 * 60 * 10 {
        rig.step(1, DT, None);
        if rig
            .drive
            .ramp_stop
            .as_ref()
            .is_some_and(|ramp| ramp.key() == stop.key())
        {
            on_scale_ramp = true;
        }
        if on_scale_ramp && rig.drive.ramp_stop.is_none() && rig.drive.trip.truck.speed_mph() < 1.0
        {
            stopped_at_scale = true;
            break;
        }
        if rig.drive.pull_over.is_some() || rig.drive.trip.position_mi > SCALE_MI + 1.0 {
            break;
        }
    }
    let transcript = rig.transcript().join("\n");
    assert!(
        rig.said("for the scale exit") > 0 || rig.said(&format!("for the {NAME} exit")) > 0,
        "lane keeping never signalled for the scale:\n{transcript}"
    );
    assert!(on_scale_ramp, "never took the scale ramp:\n{transcript}");
    assert!(
        stopped_at_scale,
        "never stopped at the scale:\n{transcript}"
    );
    assert_eq!(rig.said("Scale bypass enforcement"), 0, "{transcript}");
    assert_eq!(rig.said("Signal for the scale exit."), 0, "{transcript}");
    // No key to press and no stop to make: lane keeping and the assist do
    // both, and the scale's own screen announces the arrival.
    assert_eq!(rig.said("to signal for the exit"), 0, "{transcript}");
    assert_eq!(rig.said(&format!("At {NAME}.")), 0, "{transcript}");
}

#[test]
fn a_scale_signal_the_driver_cancelled_stays_cancelled() {
    let mut rig = rig_on("full");
    open_scale(&mut rig);
    rig.prepare(55.0, None);
    for _ in 0..60 * 60 * 5 {
        rig.drive.trip.truck.velocity_mps = 55.0 * MPS_PER_MPH;
        rig.step(1, DT, None);
        if rig.drive.exit_signal_on {
            break;
        }
    }
    assert!(rig.drive.exit_signal_on, "{:?}", rig.transcript());
    // Two presses: the first near the gore only confirms the signal.
    rig.drive.toggle_exit_signal(&mut rig.app.ctx);
    if rig.drive.exit_signal_on {
        rig.drive.toggle_exit_signal(&mut rig.app.ctx);
    }
    assert!(!rig.drive.exit_signal_on);
    rig.step(30, DT, None);
    assert!(
        !rig.drive.exit_signal_on,
        "lane keeping re-armed a signal the driver cancelled"
    );
}

#[test]
fn partial_lane_keeping_leaves_the_scale_signal_to_the_driver() {
    let mut rig = rig_on("partial");
    open_scale(&mut rig);
    rig.prepare(55.0, None);
    for _ in 0..60 * 60 * 5 {
        rig.drive.trip.truck.velocity_mps = 55.0 * MPS_PER_MPH;
        rig.step(1, DT, None);
        if rig.drive.exit_signal_on || rig.drive.trip.position_mi > SCALE_MI - 0.2 {
            break;
        }
    }
    assert!(!rig.drive.exit_signal_on, "{:?}", rig.transcript());
}
