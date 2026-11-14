//! Rollover and the curve warning in the cab (`states/driving_rollover.rs`).
//!
//! The ramp cases run on the deceleration-lane bench: a 45 mph road whose
//! exit curve is posted 32 (243 feet of radius), taken with the load, the
//! lane keeping and the assists each case names.

use ff_core::sim::lane::OFF_ROAD;
use ff_core::sim::trip_models::NavigationCue;
use ff_core::sim::vehicle::{REFERENCE_CARGO_KG, ROLL_WARN_SHARE};
use freight_fate::playtest::harness::{PlaytestHarness, StartDelivery};
use freight_fate::states::base::Key;
use freight_fate::states::driving_core::DAMAGE_BAND_OUT_OF_SERVICE;

use crate::states_driving_decel_lane::{drive_to_the_gore, exit_rig_with, frame, Rig};
use crate::transcript_cruise_support::bench_road;

const WARNING: &str = "Ramp curve, too fast. Slow to";
const ROLLED: &str = "rolled over in the ramp curve";

/// What a run through the ramp curve did.
#[derive(Debug, Default)]
struct CurveRun {
    rolled: bool,
    warned: bool,
    /// Whether anything had cost the truck or the load yet when the warning
    /// was first heard.
    cost_before_warning: bool,
    /// Anything cost at all: freight, the shoulder, or going over.
    cost: bool,
    max_roll_share: f64,
    /// Frames in the curve running wide with the engine leaning the wrong
    /// way (or not at all), and frames running wide in all.
    lean_wrong: usize,
    wide: usize,
    /// At the end: the load's condition, the speed, the deepest damage band.
    after: (f64, f64, i32),
    /// Crashes on the career's driving record, and the newest entry's kind.
    crashes: (i64, String),
    text: String,
}

/// How a case takes the curve.
#[derive(Clone, Copy)]
struct Take {
    speed_mph: f64,
    /// Share of a full trailer aboard.
    load: f64,
    lane_keeping: &'static str,
    rig: Rig,
    /// Keep the throttle on to the speed; otherwise the driver's feet are off
    /// the pedals and the assists have the truck.
    hold: bool,
    /// A street turn just past the ramp's end, the way a delivery that leaves
    /// the ramp onto city streets has one.
    corner: bool,
    lane_departure_warning: bool,
    /// `None` leaves the harness's own setting.
    curve_assistance: Option<bool>,
}

impl Take {
    fn at(speed_mph: f64, load: f64) -> Self {
        Take {
            speed_mph,
            load,
            lane_keeping: "full",
            rig: Rig::default(),
            hold: true,
            corner: false,
            lane_departure_warning: true,
            curve_assistance: None,
        }
    }
}

/// Take the 32 mph ramp curve off a 45 mph road the way `take` says.
fn through_the_curve(take: Take) -> CurveRun {
    let Take {
        speed_mph,
        load,
        hold,
        ..
    } = take;
    let (mut harness, stop) = exit_rig_with(45.0, 0.0, 0.05, speed_mph, false, take.rig);
    harness.app.ctx.settings.lane_keeping = take.lane_keeping.to_string();
    harness.app.ctx.settings.lane_departure_warning = take.lane_departure_warning;
    if let Some(on) = take.curve_assistance {
        harness.app.ctx.settings.curve_speed_assist = on;
    }
    harness.app.ctx.settings.steering_guide_inverted = false;
    harness.app.ctx.settings.lane_guide_tone = false;
    harness.with_drive(move |d, _| d.truck_mut().cargo_kg = REFERENCE_CARGO_KG * load);
    drive_to_the_gore(&mut harness, &stop);
    if take.corner {
        harness.with_drive(|d, _| {
            let mut cue = NavigationCue::new(
                "local:turn:1",
                "local_turn",
                d.trip.position_mi + 0.15,
                "Turn left onto Main Street.",
                "",
            );
            cue.direction = "left".to_string();
            d.trip.navigation_cues.push(cue);
        });
    }
    let damage_before = harness.read_drive(|d| d.truck().damage_pct);
    let mut run = CurveRun::default();
    for _ in 0..(30 * 60) {
        if hold {
            harness.with_drive(move |d, _| {
                if d.truck().speed_mph() < speed_mph {
                    d.truck_mut().throttle = 0.5;
                }
            });
        }
        frame(&mut harness);
        let (share, cargo, damage, off_road, in_curve, offset, pan, past) =
            harness.read_drive(|d| {
                (
                    d.truck().roll_share(),
                    d.truck().cargo_damage_pct,
                    d.truck().damage_pct,
                    d.lane.edge_excursion() >= OFF_ROAD,
                    // How much of the curve is still to come.
                    d.ramp_curve_radius_ft().is_some()
                        && d.ramp_layout.zip(d.ramp_travelled_mi()).is_some_and(
                            |(layout, travelled)| {
                                travelled - layout.decel_mi < 0.9 * layout.curve_mi
                            },
                        ),
                    d.lane.offset,
                    d.engine_guide_pan_applied.unwrap_or(0.0),
                    d.ramp_curve_radius_ft().is_none() && !d.in_deceleration_lane(),
                )
            });
        let text = harness.transcript_text();
        run.rolled = text.contains(ROLLED);
        run.max_roll_share = run.max_roll_share.max(share);
        let costing = cargo > 0.0 || off_road || damage > damage_before || run.rolled;
        if !run.warned && text.contains(WARNING) {
            run.warned = true;
            run.cost_before_warning = run.cost;
        }
        run.cost |= costing;
        // The ramp bends right, so running wide is the truck's offset going
        // left; the inside of the curve is a lean to the right. Judged while
        // the curve is still being taken: at its very end the turn's own lean
        // has closed, and with the warning off nothing speaks for the drift
        // (the owner's "turns yes, drift no").
        if in_curve && offset < -0.3 {
            run.wide += 1;
            if pan <= 0.0 {
                run.lean_wrong += 1;
            }
        }
        if run.rolled || past {
            break;
        }
    }
    run.after = harness.read_drive(|d| {
        (
            d.truck().cargo_damage_pct,
            d.truck().speed_mph(),
            d.worst_damage_band,
        )
    });
    run.crashes = harness
        .app
        .ctx
        .profile
        .as_ref()
        .map_or((0, String::new()), |p| {
            let record = &p.driving_record;
            let newest = record
                .entries
                .last()
                .map_or(String::new(), |e| e.kind.clone());
            (record.crashes, newest)
        });
    run.text = harness.transcript_text();
    run
}

#[test]
fn test_a_hot_ramp_curve_with_a_full_load_rolls_the_truck() {
    // 50 into the 32 curve with a full trailer: 0.69 g against a 0.35 g
    // threshold, with the ramp's bank credited. The truck goes over, the load
    // is scrap, and the run carries on through road service.
    let run = through_the_curve(Take::at(50.0, 1.0));
    assert!(run.rolled, "{}", run.text);
    assert!(run.warned, "went over with no warning\n{}", run.text);
    assert_eq!(run.after.0, 100.0, "the load after going over");
    assert!(run.after.1 < 0.5, "still rolling at {:.1}", run.after.1);
    assert_eq!(
        run.after.2, DAMAGE_BAND_OUT_OF_SERVICE,
        "settlement must see the wall"
    );
    let text = &run.text;
    assert!(
        text.contains("Road service")
            || text.contains("Roadside repair")
            || text.contains("Dispatch has taken"),
        "no recovery after the rollover\n{text}"
    );
    // The rollover line names the load; the condition cue must not repeat it.
    assert!(!text.contains("The load has shifted hard"), "{text}");
    // And it is a crash on the driving record (owner ruling, 2026-09-24).
    assert_eq!(run.crashes, (1, "crash".to_string()), "{text}");
    assert!(
        text.contains("goes on your driving record as a crash"),
        "{text}"
    );
}

#[test]
fn test_a_rollover_with_no_load_aboard_names_no_load_or_receiver() {
    // A pickup deadhead hauls an empty box and a reposition runs bobtail:
    // neither has a load to call secure or a receiver to refuse it.
    for quiet in [false, true] {
        for trailer_attached in [true, false] {
            let mut harness = PlaytestHarness::new();
            harness.start_delivery(StartDelivery::named("Empty Rollover"));
            if quiet {
                harness.app.ctx.settings.driving_speech = "quiet".to_string();
            }
            harness.with_drive(move |d, ctx| {
                d.truck_mut().cargo_kg = 0.0;
                d.truck_mut().trailer_attached = trailer_attached;
                d.truck_mut().liquid = None;
                d.roll_over(ctx);
            });
            let case = format!("quiet {quiet}, trailer {trailer_attached}");
            let lines = harness.app.event_lines();
            let line = lines
                .iter()
                .find(|line| line.contains("olled over in the"))
                .unwrap_or_else(|| panic!("{case}: no rollover line in {lines:?}"));
            let lower = line.to_lowercase();
            assert!(
                !lower.contains("load") && !lower.contains("receiver"),
                "{case}: {line}"
            );
        }
    }
}

#[test]
fn test_the_same_curve_with_exit_speed_assistance_stays_far_from_the_threshold() {
    // Same truck, same load, same 50 at the gore: exit speed assistance
    // brakes the deceleration lane down and the curve never asks the load for
    // more than a posted advisory asks of a full trailer.
    let run = through_the_curve(Take {
        rig: Rig {
            exit_speed_assist: true,
            ..Rig::default()
        },
        hold: false,
        ..Take::at(50.0, 1.0)
    });
    assert!(!run.rolled, "{}", run.text);
    assert!(!run.warned, "{}", run.text);
    assert!(!run.cost, "{}", run.text);
    assert!(
        run.max_roll_share <= ROLL_WARN_SHARE,
        "asked {:.3} of the threshold",
        run.max_roll_share
    );
}

#[test]
fn test_the_curve_warning_comes_before_anything_costs() {
    // Agent drive, 2026-09-24: 47 into a 35 ramp curve with partial lane
    // keeping put the truck on the shoulder and moved the load with nothing
    // said, because the warning waited for 15 over the sign. It is priced
    // from the roll model and the lane's own hold now, and heard first.
    for lane_keeping in ["partial", "full"] {
        for load in [0.3, 1.0] {
            for speed in [36.0, 40.0, 44.0, 48.0] {
                let run = through_the_curve(Take {
                    lane_keeping,
                    ..Take::at(speed, load)
                });
                if run.cost {
                    assert!(
                        run.warned && !run.cost_before_warning,
                        "{lane_keeping}, load {load}, {speed} mph: cost before the warning\n{}",
                        run.text
                    );
                }
                assert_eq!(
                    run.text.matches(WARNING).count(),
                    usize::from(run.warned),
                    "said more than once\n{}",
                    run.text
                );
            }
        }
    }
}

#[test]
fn test_the_engine_leans_to_the_inside_of_a_ramp_curve_the_truck_runs_wide_on() {
    // Same drive: the engine stayed centred while the truck ran wide. With
    // the turn the driver's -- lane keeping off and curve assistance off,
    // since the lean asks for the wheel only where nothing else is steering
    // (2026-09-30) -- the lean points where the wheel should go, into the
    // curve. The curve is a turn, so its lean speaks with the lane-departure
    // warning off as well ("turns yes, drift no", 2026-09-19), and a street
    // turn waiting past the ramp's end does not take the engine from it
    // while the curve is being taken.
    for lane_departure_warning in [true, false] {
        for corner in [false, true] {
            let run = through_the_curve(Take {
                lane_keeping: "off",
                curve_assistance: Some(false),
                corner,
                lane_departure_warning,
                ..Take::at(40.0, 0.3)
            });
            let case = format!("warning {lane_departure_warning}, corner {corner}");
            assert!(run.wide > 0, "{case}: never ran wide, which proves nothing");
            assert_eq!(
                run.lean_wrong, 0,
                "{case}: {} of {} wide frames leaned away from the inside",
                run.lean_wrong, run.wide
            );
        }
    }
}

#[test]
fn test_leaving_the_pavement_costs_the_truck_and_says_so_with_the_warning_off() {
    // The lane-departure warning governs the drift cues before the edge,
    // never the edge itself: with it off the shoulder still costs the truck,
    // and the charge is still heard with the damage it left (issue 533, an
    // All assists run that ended 1 percent worse with nothing said).
    let mut harness = PlaytestHarness::new();
    harness.start_delivery(StartDelivery::named("Shoulder"));
    harness.app.ctx.settings.lane_keeping = "off".to_string();
    harness.app.ctx.settings.lane_departure_warning = false;
    harness.with_drive(|d, _| {
        d.departure_checked = true;
        bench_road(d, 55.0, 0.0, 1.0);
        d.truck_mut().set_air_ready(false);
    });
    harness.press_key(Key::E, None);
    harness.with_drive(|d, _| {
        d.truck_mut().transmission.gear = 10;
        d.truck_mut().velocity_mps = 45.0 / 2.23694;
        d.trip.position_mi = 10.0;
    });
    let damage_before = harness.read_drive(|d| d.truck().damage_pct);
    let mut off_seen = false;
    for _ in 0..(30 * 6) {
        harness.with_drive(|d, _| {
            // Held out past the right-hand edge, pointing along the road.
            d.lane.lane = 0;
            d.lane.offset = 1.6;
            d.lane.yaw_rad = 0.0;
        });
        frame(&mut harness);
        off_seen |= harness.read_drive(|d| d.lane.edge_excursion() >= OFF_ROAD);
    }
    assert!(off_seen, "the case never put the truck off the pavement");
    let damage = harness.read_drive(|d| d.truck().damage_pct);
    assert!(damage > damage_before, "the shoulder cost nothing");
    let text = harness.transcript_text();
    // One line at the edge, then the rumble alone for the charges after it.
    assert_eq!(
        text.matches("Off the road on the right! Damage ").count(),
        1,
        "{text}"
    );
}
