//! Player-proof regressions for safe acceleration-lane assist handoff.

use ff_core::models::trucks::truck_model_or_panic;
use ff_core::sim::trip_models::merge_traffic_target_mph;
use ff_core::sim::vehicle::{KG_PER_TON, MPS_TO_MPH};
use ff_core::sim::weather::WeatherKind;

use freight_fate::playtest::harness::{PlaytestHarness, RouteSetup};
use freight_fate::states::driving::DrivingState;
use freight_fate::states::driving_turns::{TURN_COMMIT_TAIL_MI, TURN_CORNER_MAX_MPH};

use crate::transcript_cruise_support::{frame, quiet, release_keys, DT};

const RAMP_ENTRY_MPH: f64 = 18.0;
const RAMP_ENTRY_GEAR: i32 = 6;
const RAMP_ENTRY_RPM: f64 = 1412.0;

#[derive(Debug)]
struct RampRun {
    truck_key: &'static str,
    max_torque_nm: f64,
    gross_mass_kg: f64,
    cargo_kg: f64,
    trailer_attached: bool,
    automatic: bool,
    weather: WeatherKind,
    highway: String,
    highway_mph: f64,
    grade_pct: f64,
    lane_ft: f64,
    handoff_target_mph: Option<f64>,
    entry_gear: i32,
    entry_rpm: f64,
    cruise_handoff_mph: Option<f64>,
    cruise_handoff_on_lane: bool,
    peak_throttle: f64,
    peak_coupled_rpm: f64,
    ever_over_revving: bool,
    engine_wear_gained: f64,
    merge_mph: f64,
    spoken_merge_mph: Option<f64>,
    heard: Vec<String>,
}

impl RampRun {
    fn said(&self, needle: &str) -> bool {
        self.heard.iter().any(|line| line.contains(needle))
    }
}

fn lane_ending_mph(heard: &[String]) -> Option<f64> {
    let line = heard.iter().find(|line| line.contains("Lane ending at"))?;
    let tail = line.split("Lane ending at ").nth(1)?;
    tail.split_whitespace().next()?.parse::<f64>().ok()
}

/// Stage a real highway acceleration lane without spending several minutes
/// driving its unrelated surface-street departure chain first.
fn run_acceleration_lane_scenario(truck_key: &'static str, cargo_tons: f64) -> RampRun {
    run_acceleration_lane_scenario_with(truck_key, cargo_tons, |_| {})
}

/// The same lane, with one last touch on the street trip before the ramp
/// swap -- the frame Brandon's stale corner hold came from.
fn run_acceleration_lane_scenario_with(
    truck_key: &'static str,
    cargo_tons: f64,
    before_swap: impl FnOnce(&mut DrivingState),
) -> RampRun {
    let mut harness = PlaytestHarness::new();
    assert!(harness
        .app
        .ctx
        .settings
        .apply_driving_assistance_preset("all"));
    harness.app.ctx.settings.speed_keeper = true;
    harness.app.ctx.settings.automatic_transmission = true;

    let mut setup = RouteSetup::seeded(4242)
        .named("Loaded Acceleration Lane Regression")
        .origin_location("Carlisle Dry Warehouse");
    setup.cargo = "machinery".to_string();
    setup.tons = cargo_tons;
    harness.start_route("Carlisle", "Pittsburgh", setup);
    harness.with_drive(move |d, ctx| {
        quiet(&mut d.trip);
        d.weather_mut().current = WeatherKind::Clear;
        d.truck_mut().specs = truck_model_or_panic(truck_key).specs.clone();
        d.truck_mut().cargo_kg = cargo_tons * KG_PER_TON;
        d.truck_mut().trailer_attached = true;
        d.truck_mut().transmission.automatic = true;
        d.truck_mut().start_engine();
        d.truck_mut().set_air_ready(false);
        d.speed_control_armed = true;
        if let Some(profile) = ctx.profile.as_mut() {
            profile.tutorial_done = true;
        }
        d.tutorial = None;
        // This test stages the departure chain itself. Mark the normal
        // first-frame lifecycle check consumed so update_frame cannot start a
        // second copy of the Carlisle street chain after the manual I-76
        // handoff below.
        d.departure_checked = true;
    });
    let began = harness.with_drive(|d, ctx| d.begin_departure_chain(ctx, false));
    assert!(
        began,
        "Carlisle Dry Warehouse must keep its real street chain"
    );
    harness.with_drive(|d, _| {
        quiet(&mut d.trip);
        if let Some(highway) = d.highway_trip.as_mut() {
            quiet(highway);
        }
        d.truck_mut().velocity_mps = RAMP_ENTRY_MPH / MPS_TO_MPH;
        // Brandon's log records the rolling ramp approach in sixth at 1,412
        // RPM. Leaving this staged truck in neutral made the heavy-load
        // automatic choose its legitimate standing-start gear (first) at 18
        // mph, mechanically over-revving an impossible reproduction.
        d.truck_mut().transmission.gear = RAMP_ENTRY_GEAR;
        d.truck_mut().transmission.shift_timer = 0.0;
        d.truck_mut().transmission.gear_hold_timer = 999.0;
        d.truck_mut().rpm = RAMP_ENTRY_RPM;
    });
    harness.with_drive(|d, _| before_swap(d));
    harness.clear_speech();
    harness.with_drive(|d, ctx| d.finish_departure_chain(ctx));

    let (
        trailer_attached,
        automatic,
        max_torque_nm,
        gross_mass_kg,
        cargo_kg,
        weather,
        highway,
        highway_mph,
        grade_pct,
        lane_ft,
        handoff_target_mph,
        entry_gear,
        entry_rpm,
    ) = harness.with_drive(|d, _| {
        (
            d.truck().trailer_attached,
            d.truck().transmission.automatic,
            d.truck().specs.max_torque_nm,
            d.truck().gross_mass_kg(),
            d.truck().cargo_kg,
            d.weather().current,
            d.trip.route.legs[0].highway.clone(),
            d.trip.speed_limit_at(0.0).0,
            d.trip.grade_at(0.0) * 100.0,
            d.departure_ramp_mi.expect("acceleration lane") * 5280.0,
            d.departure_cruise_handoff_mph,
            d.truck().transmission.gear,
            d.truck().rpm,
        )
    });

    release_keys(&mut harness);
    let engine_wear_before = harness.read_drive(|d| d.truck().engine_wear_pct);
    let mut peak_throttle = 0.0f64;
    let mut peak_coupled_rpm = 0.0f64;
    let mut ever_over_revving = false;
    let mut cruise_handoff_mph = None;
    let mut cruise_handoff_on_lane = false;
    let mut merge_mph = None;
    let mut ramp_was_active = harness.read_drive(|d| {
        let leg = &d.trip.route.legs[d.trip.current_leg_index()];
        !d.departure_chain && leg.highway.contains("I-76") && d.departure_ramp_mi.is_some()
    });
    assert!(
        ramp_was_active,
        "the staged lane must be the I-76 acceleration lane"
    );
    // The route-start merge instruction can interrupt this route sentence
    // and make the speech pacer hand it back for completion. CaptureSpeech
    // records both full submissions, so transcript cardinality is not ramp
    // cardinality. Count the live I-76 state cycle instead.
    let mut i76_ramp_cycles = 1;
    for _ in 0..(60 * 180) {
        frame(&mut harness, DT);
        let (throttle, coupled_rpm, over_revving, speed_mph, cruise_on, on_i76, ramp_is_active) =
            harness.read_drive(|d| {
                let leg = &d.trip.route.legs[d.trip.current_leg_index()];
                let on_i76 = !d.departure_chain && leg.highway.contains("I-76");
                (
                    d.truck().throttle,
                    d.truck().coupled_rpm(None),
                    d.truck().over_revving(),
                    d.truck().speed_mph(),
                    d.cruise_mph.is_some(),
                    on_i76,
                    on_i76 && d.departure_ramp_mi.is_some(),
                )
            });
        peak_throttle = peak_throttle.max(throttle);
        peak_coupled_rpm = peak_coupled_rpm.max(coupled_rpm);
        ever_over_revving |= over_revving;
        if !ramp_was_active && ramp_is_active {
            i76_ramp_cycles += 1;
        }
        if merge_mph.is_none() && ramp_was_active && on_i76 && !ramp_is_active {
            merge_mph = Some(speed_mph);
        }
        if cruise_handoff_mph.is_none()
            && on_i76
            && (ramp_is_active || merge_mph.is_some())
            && cruise_on
        {
            cruise_handoff_mph = Some(speed_mph);
            cruise_handoff_on_lane = ramp_is_active;
        }
        ramp_was_active = ramp_is_active;
        if merge_mph.is_some() && cruise_handoff_mph.is_some() {
            break;
        }
    }
    for _ in 0..300 {
        frame(&mut harness, DT);
        let (throttle, coupled_rpm, over_revving) = harness.read_drive(|d| {
            (
                d.truck().throttle,
                d.truck().coupled_rpm(None),
                d.truck().over_revving(),
            )
        });
        peak_throttle = peak_throttle.max(throttle);
        peak_coupled_rpm = peak_coupled_rpm.max(coupled_rpm);
        ever_over_revving |= over_revving;
    }
    let engine_wear_gained = harness.read_drive(|d| d.truck().engine_wear_pct) - engine_wear_before;
    let heard = harness.transcript();
    assert_eq!(
        i76_ramp_cycles, 1,
        "the harness must stage exactly one live I-76 acceleration lane: {heard:#?}"
    );
    let spoken_merge_mph = lane_ending_mph(&heard);

    RampRun {
        truck_key,
        max_torque_nm,
        gross_mass_kg,
        cargo_kg,
        trailer_attached,
        automatic,
        weather,
        highway,
        highway_mph,
        grade_pct,
        lane_ft,
        handoff_target_mph,
        entry_gear,
        entry_rpm,
        cruise_handoff_mph,
        cruise_handoff_on_lane,
        peak_throttle,
        peak_coupled_rpm,
        ever_over_revving,
        engine_wear_gained,
        merge_mph: merge_mph.expect("the truck must consume the acceleration lane"),
        spoken_merge_mph,
        heard,
    }
}

#[test]
fn loaded_low_power_tractor_uses_the_full_acceleration_lane_safely() {
    // game.log, 2026-08-26: automatic yard mule, flatbed 2607, 25 tons of
    // heavy machinery, clear weather, Carlisle Dry Warehouse, I-76 West
    // toward Pittsburgh, 1.1 percent downhill, and a spoken 1,600-foot lane.
    // It was 18 mph before the ramp, 31 shortly after the call, and only 46
    // at the taper while the keeper's open-road target was 70.
    let run = run_acceleration_lane_scenario("yard_mule", 25.0);
    let yard_mule = &truck_model_or_panic("yard_mule").specs;

    assert_eq!(run.truck_key, "yard_mule");
    assert_eq!(run.max_torque_nm, yard_mule.max_torque_nm);
    assert_eq!(run.cargo_kg, 25.0 * KG_PER_TON);
    assert!(run.gross_mass_kg > run.cargo_kg);
    assert!(run.trailer_attached);
    assert!(run.automatic);
    assert_eq!(run.weather, WeatherKind::Clear);
    assert!(run.highway.contains("I-76"), "{run:#?}");
    assert_eq!(run.highway_mph, 70.0);
    assert!((run.grade_pct + 1.1).abs() <= 0.05, "{run:#?}");
    assert!((run.lane_ft - 1620.0).abs() <= 1.0, "{run:#?}");
    assert_eq!(run.entry_gear, RAMP_ENTRY_GEAR, "{run:#?}");
    assert!((run.entry_rpm - RAMP_ENTRY_RPM).abs() <= 0.1, "{run:#?}");

    assert!(run.peak_throttle >= 0.99, "{run:#?}");
    assert!(!run.ever_over_revving, "{run:#?}");
    assert!(run.peak_coupled_rpm <= yard_mule.max_rpm * 1.05, "{run:#?}");
    assert!(run.engine_wear_gained < 0.05, "{run:#?}");
    assert!(
        !run.heard
            .iter()
            .any(|line| line.to_ascii_lowercase().contains("redline")),
        "{:#?}",
        run.heard
    );
    assert!(run.merge_mph > 46.5, "{run:#?}");
    let handoff = run
        .cruise_handoff_mph
        .expect("cruise must take the truck no later than the mainline");
    match run.handoff_target_mph {
        Some(target) => {
            assert!(run.cruise_handoff_on_lane, "{run:#?}");
            assert!(handoff + 0.6 >= target, "{run:#?}");
            assert!((target - merge_traffic_target_mph(run.highway_mph)).abs() <= 0.01);
            assert!(run.said("Cruise resuming, 70"), "{:#?}", run.heard);
        }
        None => {
            assert!(!run.cruise_handoff_on_lane, "{run:#?}");
            assert!(
                run.merge_mph < merge_traffic_target_mph(run.highway_mph),
                "{run:#?}"
            );
            let spoken_merge = run
                .spoken_merge_mph
                .expect("the slow-merge warning must state the measured taper speed");
            assert!((spoken_merge - run.merge_mph).abs() <= 1.0, "{run:#?}");
            assert!(run.said("Take a big gap"), "{:#?}", run.heard);
        }
    }
    assert!(run.said("of acceleration lane."), "{:#?}", run.heard);
    assert!(run.said("Lane ending"), "{:#?}", run.heard);
}

#[test]
fn a_light_standard_rig_hands_off_at_the_traffic_relative_target() {
    let loaded = run_acceleration_lane_scenario("yard_mule", 25.0);
    let light = run_acceleration_lane_scenario("rig", 0.0);
    let traffic_target = merge_traffic_target_mph(light.highway_mph);

    assert_eq!(light.handoff_target_mph, Some(traffic_target));
    assert!(light.cruise_handoff_mph.is_some(), "{light:#?}");
    assert!(light.cruise_handoff_on_lane, "{light:#?}");
    assert!(
        light.merge_mph > loaded.merge_mph,
        "loaded={loaded:#?}\nlight={light:#?}"
    );
    assert!(light.peak_throttle >= 0.99, "{light:#?}");
    assert!(!light.ever_over_revving, "{light:#?}");
    assert!(light.engine_wear_gained < 0.05, "{light:#?}");
}

/// Brandon, Waco onto TX-31, 2026-09-01: "speed keeper didn't build up to
/// traffic speed to hand off to cruise control". His log has the keeper
/// "holding 20 miles per hour for the corner" from the top of the ramp to
/// the end of the 350-foot lane, and the truck merging at 19 into 40 mph
/// traffic. The corner was the street chain's last one: the keeper's memory
/// of it is a milepost on the SURFACE trip, and it survived the swap to the
/// highway trip, whose own mileposts start again from zero.
#[test]
fn the_last_street_corner_does_not_hold_the_keeper_through_the_acceleration_lane() {
    let clean = run_acceleration_lane_scenario("rig", 0.0);
    let held = run_acceleration_lane_scenario_with("rig", 0.0, |d| {
        // The corner just taken onto the ramp, still inside its own tail.
        let corner_mi = d.trip.position_mi - 0.02;
        d.keeper_ease_target = Some((
            corner_mi + TURN_COMMIT_TAIL_MI,
            TURN_CORNER_MAX_MPH,
            "turn".to_string(),
        ));
    });
    assert!(
        held.merge_mph > TURN_CORNER_MAX_MPH + 5.0,
        "the keeper held the street corner's number down the lane: {held:#?}"
    );
    assert!(
        (held.merge_mph - clean.merge_mph).abs() < 1.0,
        "a corner behind the ramp changed the merge: held {:.1} vs clean {:.1}",
        held.merge_mph,
        clean.merge_mph
    );
    assert!(
        !held.said("for the corner"),
        "the lane was narrated as a corner hold: {:#?}",
        held.heard
    );
}
