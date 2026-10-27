//! A destination exit whose ramp ends at the gate (Facility streets off), on
//! every assist: one line for the ramp, one for the slowdown, and nothing
//! that says the assists let go while they are still driving the truck in.

use freight_fate::playtest::harness::PlaytestHarness;
use freight_fate::playtest::road::{find_feature, RoadOptions};
use freight_fate::states::driving::DrivingState;

const DT: f64 = 1.0 / 60.0;

/// Agent drive (2026-10-25), Chicago to Cleveland's exit 170A with streets
/// off: the ramp line repeated the exit speed the one-mile call had just
/// given, "Slowing for the ramp." and "Slowing for the entrance." went out in
/// the same breath, "Slowing for the ramp." came again 11 seconds later, and
/// then "Route-transition assistance released." with the arrival assist still
/// at the pedals.
#[test]
#[cfg_attr(
    ci_quick,
    ignore = "sweep: drives the last 12 miles of a 345-mile load"
)]
fn a_gate_ramp_on_every_assist_is_said_once() {
    let mut harness = PlaytestHarness::new();
    harness
        .app
        .ctx
        .settings
        .apply_driving_assistance_preset("all");
    harness.app.ctx.settings.facility_streets = false;
    harness.app.ctx.settings.driving_speech = "standard".into();
    harness.app.ctx.settings.time_scale = 20.0;
    let opts = RoadOptions {
        feature: "destination".to_string(),
        trip_seed: Some(11),
        ..Default::default()
    };
    let pairs = vec![("Chicago".to_string(), "Cleveland".to_string())];
    let hit = find_feature(
        harness.app.ctx.world,
        &pairs,
        "destination",
        &opts,
        Some(11),
    )
    .into_iter()
    .next()
    .expect("Cleveland's destination exit");
    harness.start_road_feature(&hit, &opts);
    harness.with_drive(|d, ctx| {
        d.tutorial = None;
        d.departure_checked = true;
        d.truck_mut().start_engine();
        d.truck_mut().set_air_ready(false);
        d.trip.truck.velocity_mps = 27.0;
        d.engage_cruise(ctx, 60.0, false);
    });
    for _ in 0..(60 * 60 * 30) {
        harness.advance_frame_clock();
        harness.app.frame(DT);
        if !harness.state_is::<DrivingState>() || harness.read_drive(|d| d.trip.finished) {
            break;
        }
    }
    let heard = harness.transcript();
    let gore = heard
        .iter()
        .position(|l| l.contains("of ramp, stop at the end."))
        .unwrap_or_else(|| panic!("no ramp line: {heard:#?}"));
    assert!(
        heard[..gore].iter().any(|l| l.contains("Exit speed ")),
        "{heard:#?}"
    );
    assert!(!heard[gore].contains("Exit speed"), "{heard:#?}");
    let after = &heard[gore..];
    let slowing: Vec<&String> = after.iter().filter(|l| l.contains("Slowing for")).collect();
    assert_eq!(slowing.len(), 1, "{heard:#?}");
    assert!(
        !after.iter().any(|l| l.contains("assistance released")),
        "{heard:#?}"
    );
    assert!(
        after.iter().any(|l| l.contains("Pulling into")),
        "{heard:#?}"
    );
}
