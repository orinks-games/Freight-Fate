//! Highway 401 exit 415 at Oshawa, the destination exit of a Toronto load,
//! taken with the speed assists off (agent drive, 2026-11-25: the truck rolled
//! over in the ramp curve).
//!
//! What went wrong there: cruise eased to 58 km/h for a 50 limit inside
//! Oshawa, on the highway past the exit, then let go of it 800 meters before
//! the gore and built from 59 to 87 into a 50 ramp, saying "Cruise down to 59
//! kilometers per hour on this grade" as it did. These cases drive the real
//! exit: cruise holds its exit glide, the truck reaches the gore no faster than
//! it was told, a driver who brakes on the curve warning keeps the load, and a
//! truck at road speed is warned before the gore.

use ff_core::data::world::get_world;
use ff_core::sim::vehicle::MPS_TO_MPH;
use freight_fate::playtest::harness::PlaytestHarness;
use freight_fate::playtest::road::{find_feature_seeded, RoadOptions};
use freight_fate::states::base::{Key, Mods};

use crate::states_driving_decel_lane::frame;

const WARNING: &str = "Ramp curve, too fast. Slow to";
const ROLLED: &str = "rolled over in the ramp curve";
const FRAMES_PER_S: usize = 30;

/// The exit 415 stage: a Toronto to Oshawa load of 14 tonnes, two miles short
/// of the destination exit, every speed assist off and Facility streets off,
/// the way the drive was set up.
fn oshawa(cruise_mph: f64) -> PlaytestHarness {
    let opts = RoadOptions {
        feature: "destination".to_string(),
        origin: Some("toronto_on_ca".to_string()),
        destination: Some("oshawa_on_ca".to_string()),
        seed: Some(7),
        trip_seed: Some(7),
        cargo: 14.0,
        cargo_type: "general".to_string(),
        lead: Some(3.0),
        cruise: cruise_mph,
        speed: 64.0,
        sandbox: false,
        ..Default::default()
    };
    let pairs = vec![("toronto_on_ca".to_string(), "oshawa_on_ca".to_string())];
    let hit = find_feature_seeded(get_world(), &pairs, "destination", &opts)
        .into_iter()
        .next()
        .expect("the Oshawa destination exit");
    assert!(hit.label.contains("exit 415"), "{}", hit.label);
    let mut harness = PlaytestHarness::new();
    {
        let s = &mut harness.app.ctx.settings;
        s.exit_speed_assist = false;
        s.route_transition_assist = false;
        s.curve_speed_assist = false;
        s.destination_approach_assist = false;
        s.descent_speed_control = "off".to_string();
        s.lane_keeping = "full".to_string();
        s.facility_streets = false;
    }
    harness.start_road_feature(&hit, &opts);
    harness.clear_speech();
    harness
}

/// What one run to the end of the ramp curve did.
#[derive(Debug, Default)]
struct Run {
    /// Slowest on the highway after the first mile, mph.
    slowest_mainline: f64,
    /// Fastest over the last half mile before the gore, mph.
    fastest_final: f64,
    gore_mph: f64,
    /// Speed when the warning was heard, and whether the truck was still
    /// short of the gore then.
    warned_at: Option<(f64, bool)>,
    rolled: bool,
    cargo_damage: f64,
    text: String,
}

/// Drive to the curve's end. `braking` holds the brake from the reaction
/// time after the warning on; `throttle_mph` holds the truck at that speed
/// instead of cruise up to the gore.
fn run(mut harness: PlaytestHarness, braking: bool, throttle_mph: Option<f64>) -> Run {
    let mut out = Run {
        slowest_mainline: f64::INFINITY,
        ..Default::default()
    };
    let start = harness.read_drive(|d| d.trip.position_mi);
    let mut gore: Option<f64> = None;
    let mut brake_in: Option<usize> = None;
    for _ in 0..(FRAMES_PER_S * 240) {
        if let Some(mph) = throttle_mph {
            // Held there whatever eases the approach: the case is a truck
            // that reaches the gore at road speed.
            harness.with_drive(move |d, _| {
                if d.ramp_mi.is_none() && d.truck().speed_mph() < mph {
                    d.truck_mut().velocity_mps = mph / MPS_TO_MPH;
                }
            });
        }
        if let Some(left) = brake_in.as_mut() {
            if *left == 0 {
                harness.app.ctx.input.press(Key::Down, Mods::NONE);
            } else {
                *left -= 1;
            }
        }
        frame(&mut harness);
        let (speed, position, on_ramp, past_curve, cargo, exit_mi) = harness.read_drive(|d| {
            (
                d.truck().speed_mph(),
                d.trip.position_mi,
                d.ramp_mi.is_some(),
                d.ramp_mi.is_some() && !d.short_of_ramp_curve_end(),
                d.truck().cargo_damage_pct,
                d.exit_stop.as_ref().map(|s| s.at_mi),
            )
        });
        gore = gore.or(exit_mi);
        if !on_ramp {
            if position - start > 1.0 {
                out.slowest_mainline = out.slowest_mainline.min(speed);
            }
            if gore.is_some_and(|gore| gore - position < 0.5) {
                out.fastest_final = out.fastest_final.max(speed);
            }
            out.gore_mph = speed;
        }
        let text = harness.transcript_text();
        if out.warned_at.is_none() && text.contains(WARNING) {
            out.warned_at = Some((speed, !on_ramp));
            if braking {
                // The Green Book's 2.5 seconds before the foot moves.
                brake_in = Some((2.5 * FRAMES_PER_S as f64) as usize);
            }
        }
        out.rolled = text.contains(ROLLED);
        out.cargo_damage = cargo;
        if out.rolled || past_curve {
            break;
        }
    }
    harness.app.ctx.input.release(Key::Down, Mods::NONE);
    out.text = harness.transcript_text();
    out
}

#[test]
fn test_cruise_does_not_build_speed_into_the_oshawa_exit() {
    // Set at 65 (105 km/h, the 401's posting): cruise holds the road and
    // glides to its exit floor, ten under, at the gore. It used to drop to
    // 36 for Oshawa's town limit past the exit and climb back to 54 at the
    // gore.
    let run = run(oshawa(65.0), false, None);
    assert!(
        run.slowest_mainline > 50.0,
        "cruise slowed for a limit past the exit: {:.1} mph\n{}",
        run.slowest_mainline,
        run.text
    );
    assert!(
        run.gore_mph <= 57.0,
        "crossed the gore at {:.1} mph against an exit floor of 55\n{}",
        run.gore_mph,
        run.text
    );
    assert!(
        !run.text.contains("on this grade"),
        "a climb line while the truck was gaining\n{}",
        run.text
    );
}

#[test]
fn test_a_driver_who_brakes_on_the_ramp_curve_warning_keeps_the_load() {
    let run = run(oshawa(65.0), true, None);
    assert!(run.warned_at.is_some(), "no curve warning\n{}", run.text);
    assert!(
        !run.rolled,
        "rolled after braking on the warning\n{}",
        run.text
    );
    assert_eq!(run.cargo_damage, 0.0, "the load moved\n{}", run.text);
}

#[test]
fn test_a_driver_who_never_brakes_is_warned_and_then_rolls() {
    // Nothing brakes the ramp with the assists off: the warning is the
    // driver's whole notice, and ignoring it costs the load.
    let run = run(oshawa(65.0), false, None);
    assert!(run.warned_at.is_some(), "no curve warning\n{}", run.text);
    assert!(
        run.rolled,
        "the truck took a 50 ramp at the gore's speed\n{}",
        run.text
    );
    assert_eq!(run.text.matches(WARNING).count(), 1, "{}", run.text);
}

#[test]
fn test_a_truck_at_road_speed_hears_the_ramp_curve_warning_before_the_gore() {
    // Held at 65 to the gore, the truck needs more road than the
    // deceleration lane to come down to the curve: the warning comes on the
    // highway, and braking on it keeps the load.
    let run = run(oshawa(0.0), true, Some(65.0));
    let (speed, short_of_gore) = run.warned_at.expect("a curve warning");
    assert!(
        short_of_gore,
        "warned at {speed:.1} mph inside the lane, too late to slow\n{}",
        run.text
    );
    assert!(
        !run.rolled,
        "rolled after braking on the warning\n{}",
        run.text
    );
    assert!(
        run.fastest_final >= 63.0,
        "the stage never reached road speed ({:.1})\n{}",
        run.fastest_final,
        run.text
    );
}
