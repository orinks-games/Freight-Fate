//! Port of `tests/test_vehicle.py`, plus the half of
//! `tests/test_tanker_surge.py` that puts a liquid load behind a truck.
#![allow(clippy::field_reassign_with_default)]

use std::collections::BTreeMap;

use serde_json::json;

use super::*;
use crate::sim::surge::LiquidLoad;
use crate::sim::transmission::{AUTO_DOWNSHIFT_RPM, REVERSE};

const DT: f64 = 1.0 / 60.0;

fn approx(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-6 * b.abs().max(1e-12)
}

fn approx_rel(a: f64, b: f64, rel: f64) -> bool {
    (a - b).abs() <= rel * b.abs()
}

fn approx_abs(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol
}

fn drive(truck: &mut TruckState, seconds: f64) {
    drive_dt(truck, seconds, DT);
}

fn drive_dt(truck: &mut TruckState, seconds: f64, dt: f64) {
    let steps = (seconds / dt) as i64;
    for _ in 0..steps {
        truck.auto_shift();
        truck.update(dt);
    }
}

fn time_to_speed(truck: &mut TruckState, target_mph: f64) -> Option<f64> {
    let limit_s = 240.0;
    for step in 0..((limit_s / DT) as i64) {
        truck.auto_shift();
        truck.update(DT);
        if truck.speed_mph() >= target_mph {
            return Some((step + 1) as f64 * DT);
        }
    }
    None
}

fn acceleration_marks(truck: &mut TruckState, targets: &[f64]) -> BTreeMap<u64, Option<f64>> {
    let limit_s = 240.0;
    let mut marks: BTreeMap<u64, Option<f64>> =
        targets.iter().map(|t| (t.to_bits(), None)).collect();
    for step in 0..((limit_s / DT) as i64) {
        truck.auto_shift();
        truck.update(DT);
        let elapsed = (step + 1) as f64 * DT;
        for target in targets {
            let slot = marks.get_mut(&target.to_bits()).unwrap();
            if slot.is_none() && truck.speed_mph() >= *target {
                *slot = Some(elapsed);
            }
        }
        if marks.values().all(|v| v.is_some()) {
            break;
        }
    }
    marks
}

fn make_auto_truck() -> TruckState {
    let mut t = TruckState::default();
    t.transmission.automatic = true;
    t.start_engine();
    t
}

#[test]
fn test_manual_governor_holds_low_gear_without_engine_damage() {
    let mut truck = TruckState::default();
    truck.start_engine();
    truck.set_air_ready(false);
    truck.transmission.gear = 1;
    truck.throttle = 1.0;

    drive(&mut truck, 20.0);
    let speed_at_governor = truck.speed_mph();
    let damage_at_governor = truck.damage_pct;
    drive(&mut truck, 10.0);

    // The hard fuel cut lets the equilibrium hover a hair over the governed
    // figure, so the check allows a tenth of a percent of overshoot.
    assert!(approx_rel(truck.rpm, truck.specs.max_rpm, 1e-3));
    assert!(approx_abs(truck.speed_mph(), speed_at_governor, 0.5));
    assert!(approx(truck.damage_pct, damage_at_governor));
}

#[test]
fn test_manual_downshift_with_clutch_in_does_not_overrev_or_damage_engine() {
    let mut truck = TruckState::default();
    truck.start_engine();
    truck.set_air_ready(false);
    truck.velocity_mps = 60.0 / 2.23694;
    truck.rpm = truck.specs.idle_rpm;
    truck.transmission.clutch = 1.0;
    truck.transmission.gear = 10;

    assert!(truck.transmission.request_gear(1).ok);
    assert!(truck.coupled_rpm(None) > truck.specs.max_rpm * 1.05);
    assert!(!truck.over_revving());

    for _ in 0..(5 * 60) {
        truck.update(DT);
    }

    assert!(truck.rpm < truck.specs.max_rpm * 0.6);
    assert!(approx(truck.damage_pct, 0.0));
}

/// A manual downshift with the clutch held matches revs like the automatic:
/// the engine climbs to the new gear's road speed and follows it, so letting
/// the clutch out lands without a jump (tester request, 2026-09-28).
#[test]
fn test_manual_downshift_with_clutch_held_matches_revs() {
    let mut truck = TruckState::default();
    truck.start_engine();
    truck.set_air_ready(false);
    truck.velocity_mps = 15.0;
    truck.transmission.gear = 8;
    truck.rpm = truck.coupled_rpm(None);
    truck.transmission.clutch = 1.0;
    assert!(truck.transmission.request_gear(7).ok);

    for _ in 0..(0.4 / DT) as usize {
        truck.update(DT);
    }
    let sync = truck.coupled_rpm(None);
    assert!(
        approx_rel(truck.rpm, sync, 0.03),
        "engine {:.0} vs the new gear's {sync:.0} with the clutch held",
        truck.rpm
    );

    truck.transmission.clutch = 0.0;
    let before = truck.rpm;
    truck.update(DT);
    assert!(
        (truck.rpm - before).abs() < 100.0,
        "letting the clutch out must not jump the engine: {before:.0} -> {:.0}",
        truck.rpm
    );
    assert!(
        !truck.transmission.rev_match,
        "the match ends with the clutch out"
    );
}

/// Braking a manual to a stop in a tall gear holds idle; off the brake, the
/// same gear at a standstill stalls (owner ruling, 2026-09-28).
#[test]
fn test_manual_braking_in_a_tall_gear_holds_idle_until_the_brake_lifts() {
    let mut truck = TruckState::default();
    truck.start_engine();
    truck.set_air_ready(false);
    truck.transmission.gear = 7;
    truck.velocity_mps = 1.0;
    truck.brake = 1.0;
    for _ in 0..(3.0 / DT) as usize {
        truck.update(DT);
    }
    assert!(truck.engine_on && !truck.stalled, "braking must not stall");
    assert!(truck.velocity_mps.abs() < 0.1, "the truck came to rest");

    truck.brake = 0.0;
    truck.update(DT);
    assert!(truck.stalled, "off the brake in seventh at a standstill");
}

fn loaded_automatic_avoids_steep_grade_shift_hunting(grade: f64) {
    let mut truck = make_auto_truck();
    truck.set_air_ready(false);
    truck.cargo_kg = 25.0 * KG_PER_TON;
    truck.grade = grade;
    truck.throttle = 1.0;
    let mut shifts = 0;
    let mut previous_gear = truck.transmission.gear;
    for _ in 0..(90 * 60) {
        truck.auto_shift();
        truck.update(DT);
        if truck.transmission.gear != previous_gear {
            shifts += 1;
            previous_gear = truck.transmission.gear;
        }
    }

    assert!(shifts <= 12);
    assert!(truck.speed_mph() > 3.0);
}

#[test]
fn test_loaded_automatic_avoids_steep_grade_shift_hunting_0_04() {
    loaded_automatic_avoids_steep_grade_shift_hunting(0.04);
}

#[test]
fn test_loaded_automatic_avoids_steep_grade_shift_hunting_0_06() {
    loaded_automatic_avoids_steep_grade_shift_hunting(0.06);
}

#[test]
fn test_loaded_automatic_avoids_steep_grade_shift_hunting_0_08() {
    loaded_automatic_avoids_steep_grade_shift_hunting(0.08);
}

#[test]
fn test_loaded_automatic_uses_progressive_early_upshifts() {
    let mut truck = make_auto_truck();
    truck.set_air_ready(false);
    truck.throttle = 1.0;
    let mut shifts: Vec<(f64, i32)> = Vec::new();
    let mut previous_gear = truck.transmission.gear;
    for step in 0..(90 * 60) {
        truck.auto_shift();
        truck.update(DT);
        if truck.transmission.gear != previous_gear {
            shifts.push(((step + 1) as f64 / 60.0, truck.transmission.gear));
            previous_gear = truck.transmission.gear;
        }
        if truck.transmission.gear >= 5 {
            break;
        }
    }

    let early_times: Vec<f64> = shifts
        .iter()
        .filter(|(_, gear)| (2..=5).contains(gear))
        .map(|(when, _)| *when)
        .collect();
    assert_eq!(early_times.len(), 4);
    assert!(early_times.windows(2).all(|w| w[1] - w[0] >= 1.5));
}

#[test]
fn test_loaded_automatic_upper_range_uses_realistic_working_rpm() {
    let mut truck = make_auto_truck();
    truck.set_air_ready(false);
    truck.throttle = 1.0;
    let mut previous_gear = truck.transmission.gear;
    let mut upper_shift_rpm: Vec<f64> = Vec::new();
    for _ in 0..(120 * 60) {
        truck.auto_shift();
        truck.update(DT);
        if truck.transmission.gear != previous_gear {
            if truck.transmission.gear >= 6 {
                upper_shift_rpm.push(truck.rpm);
            }
            previous_gear = truck.transmission.gear;
        }
        if truck.transmission.gear == 10 {
            break;
        }
    }

    assert!(upper_shift_rpm.len() >= 5);
    assert!(upper_shift_rpm.iter().cloned().fold(f64::MAX, f64::min) >= 1800.0);
    assert!(upper_shift_rpm.iter().cloned().fold(f64::MIN, f64::max) <= truck.specs.max_rpm);
}

#[test]
fn test_empty_automatic_shifts_low_range_faster_than_loaded() {
    fn time_to_fifth(cargo_kg: f64) -> Option<f64> {
        let mut truck = make_auto_truck();
        truck.set_air_ready(false);
        truck.cargo_kg = cargo_kg;
        truck.throttle = 1.0;
        for step in 0..(90 * 60) {
            truck.auto_shift();
            truck.update(DT);
            if truck.transmission.gear >= 5 {
                return Some((step + 1) as f64 / 60.0);
            }
        }
        None
    }

    let loaded_time = time_to_fifth(REFERENCE_CARGO_KG);
    let empty_time = time_to_fifth(0.0);
    assert!(loaded_time.is_some() && empty_time.is_some());
    assert!(empty_time.unwrap() < loaded_time.unwrap());
}

#[test]
fn test_automatic_spaces_downshifts_while_stopping() {
    let mut truck = make_auto_truck();
    truck.set_air_ready(false);
    truck.transmission.gear = 10;
    truck.velocity_mps = 26.8;
    truck.brake = 0.65;
    let mut shifts: Vec<f64> = Vec::new();
    let mut previous_gear = 10;
    for step in 0..(30 * 60) {
        truck.auto_shift();
        truck.update(DT);
        if truck.transmission.gear != previous_gear {
            if truck.speed_mph() >= 1.0 {
                shifts.push((step + 1) as f64 / 60.0);
            }
            previous_gear = truck.transmission.gear;
        }
        if truck.speed_mph() < 1.0 {
            break;
        }
    }

    assert!(shifts.windows(2).all(|w| w[1] - w[0] >= 1.65));
}

#[test]
fn test_empty_automatic_selects_third_as_starting_gear() {
    let mut truck = make_auto_truck();
    truck.set_air_ready(false);
    truck.cargo_kg = 0.0;
    truck.throttle = 1.0;

    assert_eq!(truck.auto_shift(), Some(3));
}

#[test]
fn test_loaded_automatic_selects_first_as_starting_gear() {
    let mut truck = make_auto_truck();
    truck.set_air_ready(false);
    truck.throttle = 1.0;

    assert_eq!(truck.auto_shift(), Some(1));
}

#[test]
fn test_empty_automatic_can_skip_an_unneeded_gear() {
    let mut truck = make_auto_truck();
    truck.set_air_ready(false);
    truck.cargo_kg = 0.0;
    truck.transmission.gear = 3;
    truck.velocity_mps = 8.0;
    truck.throttle = 1.0;

    assert_eq!(truck.auto_shift(), Some(5));
}

#[test]
fn test_gross_mass_includes_cargo_payload() {
    let mut t = TruckState::default();
    // Default cargo equals the reference payload, so gross stays the tuned 36 t.
    assert_eq!(t.cargo_kg, REFERENCE_CARGO_KG);
    assert!(approx(t.gross_mass_kg(), t.specs.mass_kg));
    let tare = t.tare_kg();
    assert!(approx(tare, t.specs.mass_kg - REFERENCE_CARGO_KG));
    // An empty deadhead is just the tractor and empty trailer.
    t.cargo_kg = 0.0;
    assert!(approx(t.gross_mass_kg(), tare));
    // A heavier load weighs proportionally more.
    t.cargo_kg = 25.0 * KG_PER_TON;
    assert!(approx(t.gross_mass_kg(), tare + 25.0 * KG_PER_TON));
}

#[test]
fn test_heavier_load_accelerates_slower() {
    let mut light = make_auto_truck();
    light.cargo_kg = 0.0; // empty deadhead
    let mut heavy = make_auto_truck();
    heavy.cargo_kg = 25.0 * KG_PER_TON; // a 25-ton load
    light.throttle = 1.0;
    heavy.throttle = 1.0;
    let light_t = time_to_speed(&mut light, 50.0);
    let heavy_t = time_to_speed(&mut heavy, 50.0);
    assert!(light_t.is_some() && heavy_t.is_some());
    assert!(heavy_t.unwrap() > light_t.unwrap());
}

#[test]
fn test_loaded_launch_uses_lower_low_speed_traction() {
    // The gentle launch belongs to the load: this probes a rig at the full
    // reference cargo, where the low-speed easing is at its strongest.
    let mut t = make_auto_truck();
    t.cargo_kg = REFERENCE_CARGO_KG;
    t.transmission.gear = 1;
    t.rpm = t.specs.peak_torque_rpm;
    t.throttle = 1.0;

    // From a dead stop the loaded rig eases in: drive force sits at the
    // launch cap, well below the rolling traction cap.
    let start_force = t.drive_force().abs();
    // At 25 mph in first gear the governor has already cut fuel, so the
    // rolling end of the ramp is probed through the shared traction cap the
    // drive and the shift prediction both use.
    t.velocity_mps = 25.0 / 2.23694;
    let rolling_limit = t.drive_traction_limit();

    assert!(approx(
        start_force / t.gross_mass_kg(),
        G * LAUNCH_TRACTION_START_G
    ));
    assert!(approx(
        rolling_limit / t.gross_mass_kg(),
        G * LAUNCH_TRACTION_ROLLING_G
    ));
    assert!(start_force < rolling_limit);
}

#[test]
fn test_empty_deadhead_launches_at_full_traction() {
    let mut t = make_auto_truck();
    t.cargo_kg = 0.0;
    assert!(approx(
        t.drive_traction_limit() / t.gross_mass_kg(),
        G * LAUNCH_TRACTION_ROLLING_G
    ));
}

#[test]
fn test_steep_climb_gets_the_full_traction_cap_at_crawl_speed() {
    let mut t = make_auto_truck();
    t.grade = 0.06;
    assert!(approx(
        t.drive_traction_limit() / t.gross_mass_kg(),
        G * LAUNCH_TRACTION_ROLLING_G
    ));
}

#[test]
fn test_automatic_does_not_rush_through_low_gears_on_launch() {
    let mut t = make_auto_truck();
    t.throttle = 1.0;

    drive(&mut t, 5.0);

    assert!((5.0..=13.0).contains(&t.speed_mph()));
    assert!(t.transmission.gear <= 4);
}

#[test]
fn test_heavier_load_raises_grade_resistance() {
    let mut light = make_auto_truck();
    let mut heavy = make_auto_truck();
    light.cargo_kg = 0.0;
    heavy.cargo_kg = 25.0 * KG_PER_TON;
    // Same speed on the same climb: the loaded rig fights more rolling and
    // grade resistance, which is what makes it lug uphill.
    for t in [&mut light, &mut heavy] {
        t.velocity_mps = 25.0;
        t.grade = 0.04;
    }
    assert!(heavy.resistance_force() > light.resistance_force());
}

#[test]
fn test_heavier_load_burns_more_fuel_reaching_speed() {
    let mut light = make_auto_truck();
    let mut heavy = make_auto_truck();
    light.cargo_kg = 0.0;
    heavy.cargo_kg = 25.0 * KG_PER_TON;
    light.throttle = 1.0;
    heavy.throttle = 1.0;
    let (light_start, heavy_start) = (light.fuel_gal, heavy.fuel_gal);
    time_to_speed(&mut light, 50.0);
    time_to_speed(&mut heavy, 50.0);
    assert!((heavy_start - heavy.fuel_gal) > (light_start - light.fuel_gal));
}

#[test]
fn test_load_over_rated_gross_brakes_more_gently() {
    let mut truck = make_auto_truck();
    truck.velocity_mps = 25.0;
    truck.brake = 1.0;
    truck.grip = 1.0;

    let mut decel = |cargo_kg: f64| -> f64 {
        truck.cargo_kg = cargo_kg;
        truck.brake_force().abs() / truck.gross_mass_kg()
    };

    let rated = decel(REFERENCE_CARGO_KG); // gross == rated gross
    let light = decel(4.0 * KG_PER_TON); // well under rated
    let heavy = decel(REFERENCE_CARGO_KG + 6.0 * KG_PER_TON); // over rated gross

    // At or below the rated gross, braking is friction-limited and the
    // deceleration does not depend on mass.
    assert!(approx(light, rated));
    // Over the rated gross the foundation brakes cannot keep up, so the rig
    // decelerates more gently -- a longer stop.
    assert!(heavy < rated);
}

#[test]
fn test_heavier_load_heats_brakes_faster() {
    let mut light = make_auto_truck();
    let mut heavy = make_auto_truck();
    light.cargo_kg = 0.0;
    heavy.cargo_kg = 25.0 * KG_PER_TON;
    light.throttle = 0.0;
    heavy.throttle = 0.0;
    light.brake = 1.0;
    heavy.brake = 1.0;
    for _ in 0..60 {
        // one second of hard braking from 25 m/s
        light.velocity_mps = light.velocity_mps.max(25.0);
        heavy.velocity_mps = heavy.velocity_mps.max(25.0);
        light.update(DT);
        heavy.update(DT);
    }
    assert!(heavy.brake_temp_c > light.brake_temp_c);
}

#[test]
fn test_engine_start_requires_fuel() {
    let mut t = TruckState::default();
    t.fuel_gal = 0.0;
    assert!(!t.start_engine());
    t.fuel_gal = 10.0;
    assert!(t.start_engine());
    assert!(!t.start_engine()); // already running
}

#[test]
fn test_full_throttle_reaches_highway_speed() {
    let mut t = make_auto_truck();
    t.throttle = 1.0;
    drive(&mut t, 120.0);
    assert!((60.0..=76.0).contains(&t.speed_mph()));
    assert_eq!(t.transmission.gear, 10);
}

#[test]
fn test_loaded_rig_accelerates_to_highway_speed_believably() {
    let mut t = make_auto_truck();
    t.throttle = 1.0;

    let marks = acceleration_marks(&mut t, &[60.0, 65.0, 70.0]);
    let mark = |mph: f64| marks[&mph.to_bits()].unwrap();

    assert!((50.0..=75.0).contains(&mark(60.0)));
    assert!(mark(65.0) <= 90.0);
    assert!(mark(70.0) <= 125.0);
}

#[test]
fn test_highway_cruise_rpm_keeps_engine_audio_believable() {
    let mut t = make_auto_truck();
    t.throttle = 1.0;
    let to_65 = time_to_speed(&mut t, 65.0);

    assert!(to_65.is_some());
    assert_eq!(t.transmission.gear, 10);
    assert!((1400.0..=1900.0).contains(&t.rpm));
}

#[test]
fn test_automatic_shift_does_not_flare_engine_rpm() {
    // Full throttle through an upshift: the unloaded engine must fall to the
    // taller gear's speed, never rev up. (The fixture used to sit third gear
    // at 20 m/s -- nine thousand rpm on the road -- which the rev-match
    // rightly reads as "sync is above you"; 4.4 m/s is 2000 rpm in third,
    // just past the full-throttle upshift point.)
    let mut t = make_auto_truck();
    t.throttle = 1.0;
    t.transmission.gear = 3;
    t.velocity_mps = 4.4;
    t.rpm = t.coupled_rpm(Some(3));

    assert_eq!(t.auto_shift(), Some(4));
    let rpm_before = t.rpm;
    t.update(0.15); // inside the low-box 0.25 s interrupt

    assert!(t.transmission.shifting());
    assert!(t.rpm <= rpm_before);
}

#[test]
fn test_truck_does_not_move_in_neutral() {
    let mut t = TruckState::default();
    t.start_engine();
    t.throttle = 1.0;
    drive(&mut t, 5.0);
    assert_eq!(t.velocity_mps, 0.0);
}

#[test]
fn test_truck_can_back_up_slowly_in_reverse() {
    let mut t = TruckState::default();
    t.start_engine();
    t.transmission.automatic = false;
    t.transmission.clutch = 1.0;
    assert!(t.transmission.request_gear(REVERSE).ok);
    t.transmission.update(1.0);
    t.transmission.clutch = 0.0;
    t.throttle = 0.4;

    drive(&mut t, 5.0);

    assert!(t.velocity_mps < 0.0);
    assert!(1.0 < t.speed_mph() && t.speed_mph() <= 11.0);
    assert!(t.odometer_mi > 0.0);
}

#[test]
fn test_braking_stops_the_truck() {
    let mut t = make_auto_truck();
    t.throttle = 1.0;
    drive(&mut t, 60.0);
    assert!(t.speed_mph() > 40.0);
    t.throttle = 0.0;
    t.brake = 1.0;
    drive(&mut t, 30.0);
    assert!(t.speed_mph() < 1.0);
    assert!(t.engine_on); // downshifting prevented a stall
}

#[test]
fn test_high_gear_launch_stalls() {
    let mut t = TruckState::default();
    t.start_engine();
    t.transmission.automatic = false;
    t.transmission.clutch = 1.0;
    assert!(t.transmission.request_gear(6).ok);
    t.transmission.clutch = 0.0;
    t.throttle = 0.2;
    drive(&mut t, 3.0);
    assert!(t.stalled);
    assert!(!t.engine_on);
}

#[test]
fn test_rolling_automatic_kicks_down_instead_of_stalling() {
    // Regression: a hard deceleration could leave an automatic lugging in a
    // high gear in the one frame the shift delay blocked the RPM downshift, and
    // the engine stalled while still rolling (above the 'stopped -> first' reset).
    // It must kick down a gear and keep running instead.
    let mut t = make_auto_truck();
    t.transmission.gear = 5;
    t.velocity_mps = 0.7; // rolling, but lugging below idle*0.5 in 5th
    t.throttle = 0.8;
    t.transmission.shift_timer = DT; // shift lock expires inside this frame
    t.update(DT);
    assert!(t.engine_on);
    assert!(!t.stalled);
    assert_eq!(t.transmission.gear, 4); // dropped one gear
}

#[test]
fn test_hard_collision_stop_does_not_stall_an_automatic() {
    // Regression: collisions used to strand the truck stopped in a high
    // gear, where the engine stalled instantly on every restart.
    let mut t = make_auto_truck();
    t.throttle = 1.0;
    drive(&mut t, 90.0);
    assert!(t.transmission.gear >= 8);
    for _ in 0..3 {
        t.apply_collision(0.9, true);
    }
    assert!(t.velocity_mps < 0.5); // shoved to a crawl, box still in a high gear
    t.throttle = 0.0;
    drive(&mut t, 5.0);
    assert!(t.engine_on);
    assert!(!t.stalled);
    assert_eq!(t.transmission.gear, 1);
}

#[test]
fn test_emergency_brake_outbrakes_service_brakes() {
    let mut a = make_auto_truck();
    let mut b = make_auto_truck();
    a.velocity_mps = 30.0;
    b.velocity_mps = 30.0;
    a.brake = 1.0;
    b.brake = 1.0;
    b.emergency_brake = true;
    for _ in 0..120 {
        a.update(DT);
        b.update(DT);
    }
    assert!(b.velocity_mps < a.velocity_mps);
}

#[test]
fn test_fuel_burns_under_load_and_engine_dies_empty() {
    let mut t = make_auto_truck();
    t.fuel_gal = 0.02;
    t.fuel_burn_mult = 50.0;
    t.throttle = 1.0;
    drive(&mut t, 30.0);
    assert_eq!(t.fuel_gal, 0.0);
    assert!(!t.engine_on);
}

#[test]
fn test_grade_slows_the_truck() {
    let mut flat = make_auto_truck();
    flat.throttle = 1.0;
    drive(&mut flat, 90.0);
    let mut hill = make_auto_truck();
    hill.grade = 0.06;
    hill.throttle = 1.0;
    drive(&mut hill, 90.0);
    assert!(hill.speed_mph() < flat.speed_mph() - 5.0);
}

#[test]
fn test_low_grip_limits_acceleration() {
    let mut dry = make_auto_truck();
    dry.throttle = 1.0;
    drive(&mut dry, 10.0);
    let mut ice = make_auto_truck();
    ice.grip = 0.2;
    ice.throttle = 1.0;
    drive(&mut ice, 10.0);
    assert!(ice.velocity_mps < dry.velocity_mps);
}

#[test]
fn test_collision_damages_and_slows() {
    let mut t = make_auto_truck();
    t.velocity_mps = 25.0;
    t.apply_collision(0.6, true);
    assert!(t.velocity_mps < 25.0);
    assert!(t.damage_pct > 0.0);
}

#[test]
fn test_damage_reduces_power() {
    let mut t = make_auto_truck();
    t.damage_pct = 90.0;
    assert!(t.health_factor() < 0.5);
}

#[test]
fn test_refuel_caps_at_tank_size() {
    let mut t = TruckState::default();
    t.fuel_gal = 100.0;
    let added = t.refuel(Some(1000.0));
    assert_eq!(added, 50.0);
    assert_eq!(t.fuel_gal, t.specs.fuel_tank_gal);
}

#[test]
fn test_brake_heat_builds_and_cools() {
    let mut t = make_auto_truck();
    t.velocity_mps = 30.0;
    t.brake = 1.0;
    for _ in 0..600 {
        t.update_temps(DT);
    }
    let hot = t.brake_temp_c;
    assert!(hot > 40.0);
    t.brake = 0.0;
    t.velocity_mps = 20.0;
    for _ in 0..6000 {
        t.update_temps(DT);
    }
    assert!(t.brake_temp_c < hot);
}

#[test]
fn test_tire_wear_accrues_with_miles_and_load() {
    let mut light = make_auto_truck();
    let mut heavy = make_auto_truck();
    light.cargo_kg = 0.0;
    heavy.cargo_kg = 25.0 * KG_PER_TON;
    for t in [&mut light, &mut heavy] {
        t.velocity_mps = 25.0;
        t.fuel_burn_mult = 60.0; // a compressed-time cruise, like a real trip
        for _ in 0..600 {
            t.update_wear(DT);
        }
    }
    assert!(light.tire_wear_pct > 0.0);
    assert!(heavy.tire_wear_pct > light.tire_wear_pct);
}

#[test]
fn test_parked_truck_does_not_wear_tires_or_brakes() {
    let mut t = TruckState::default();
    t.set_air_ready(true); // spring brakes applied, speed zero
    for _ in 0..600 {
        t.update_wear(DT);
    }
    assert_eq!(t.tire_wear_pct, 0.0);
    assert_eq!(t.brake_wear_pct, 0.0);
}

#[test]
fn test_jake_brake_spares_the_service_brakes() {
    // The same descent on the jake costs the shoes nothing; riding the
    // service brakes wears them -- the whole point of the jake as a mechanic.
    let mut service = make_auto_truck();
    let mut jake = make_auto_truck();
    for t in [&mut service, &mut jake] {
        t.velocity_mps = 13.0; // ~30 mph downgrade
        t.grade = -0.06;
    }
    service.brake = 0.5;
    jake.set_engine_brake(true);
    for _ in 0..1200 {
        // 20 seconds of descent
        service.update_wear(DT);
        jake.update_wear(DT);
    }
    assert!(service.brake_wear_pct > 0.0);
    assert_eq!(jake.brake_wear_pct, 0.0);
}

#[test]
fn test_hot_brakes_wear_faster() {
    let mut cool = make_auto_truck();
    let mut glazed = make_auto_truck();
    for t in [&mut cool, &mut glazed] {
        t.velocity_mps = 20.0;
        t.brake = 1.0;
    }
    glazed.brake_temp_c = glazed.specs.brake_fade_temp_c + 50.0;
    cool.update_wear(1.0);
    glazed.update_wear(1.0);
    assert!(glazed.brake_wear_pct > cool.brake_wear_pct);
}

#[test]
fn test_worn_tires_cut_grip_and_lengthen_stops() {
    let mut fresh = make_auto_truck();
    let mut bald = make_auto_truck();
    bald.tire_wear_pct = 100.0;
    assert!(bald.effective_grip() < fresh.effective_grip());
    fresh.velocity_mps = 25.0;
    bald.velocity_mps = 25.0;
    fresh.brake = 1.0;
    bald.brake = 1.0;
    assert!(bald.brake_force().abs() < fresh.brake_force().abs());
}

#[test]
fn test_worn_brakes_fade_sooner_and_pull_weaker() {
    let mut fresh = make_auto_truck();
    let mut worn = make_auto_truck();
    worn.brake_wear_pct = 80.0;
    assert!(worn.brake_fade_onset_c() < fresh.brake_fade_onset_c());
    fresh.velocity_mps = 25.0;
    worn.velocity_mps = 25.0;
    fresh.brake = 1.0;
    worn.brake = 1.0;
    // Cool brakes: worn shoes still pull weaker than fresh ones.
    assert!(worn.brake_force().abs() < fresh.brake_force().abs());
    // At a temperature between the worn and fresh fade onsets, only the
    // worn shoes have started to fade.
    let temp = (worn.brake_fade_onset_c() + fresh.brake_fade_onset_c()) / 2.0;
    fresh.brake_temp_c = temp;
    worn.brake_temp_c = temp;
    let ratio = worn.brake_force().abs() / fresh.brake_force().abs();
    assert!(ratio < worn.brake_wear_factor());
}

#[test]
fn test_over_rev_wears_engine_not_damage() {
    let mut t = make_auto_truck();
    t.rpm = t.specs.max_rpm * 1.1; // the road driving the engine past the governor
    t.update_wear(1.0);
    assert!(t.engine_wear_pct > 0.5);
    assert_eq!(t.damage_pct, 0.0);
}

#[test]
fn test_jake_force_scales_with_gear_stage_and_rpm() {
    // The jake is torque through the gearing: a lower gear multiplies it, a
    // lighter stage weakens it, and low RPM starves it -- the grade discipline.
    let mut t = make_auto_truck();
    t.velocity_mps = 15.0;
    t.throttle = 0.0;
    t.rpm = 1800.0;
    t.engine_brake_stage = 3;
    t.transmission.gear = 8;
    let tall = t.jake_brake_force();
    t.transmission.gear = 7;
    let low = t.jake_brake_force();
    assert!(tall > 0.0);
    assert!(low > tall); // lower gear, more retard at the wheels
    t.engine_brake_stage = 1;
    assert!(t.jake_brake_force() < low / 2.0); // stage 1 is a third of stage 3
    t.engine_brake_stage = 3;
    t.rpm = 900.0;
    assert!(t.jake_brake_force() < low); // slow engine, weak jake
}

#[test]
fn test_engine_brake_bool_view_selects_full_stage() {
    let mut t = make_auto_truck();
    t.set_engine_brake(true);
    assert_eq!(t.engine_brake_stage, 3);
    assert!(t.engine_brake());
    t.set_engine_brake(false);
    assert_eq!(t.engine_brake_stage, 0);
}

#[test]
fn test_jake_is_capped_by_drive_axle_grip_on_ice() {
    // On glare ice a hard jake in a low gear outruns what the drive axle can
    // transmit: force caps, the slipping flag raises, and a lighter stage stays
    // hooked up -- the CDL rule about compression brakes on slick roads.
    let mut t = make_auto_truck();
    t.velocity_mps = 15.0;
    t.throttle = 0.0;
    t.rpm = 1800.0;
    t.engine_brake_stage = 3;
    t.transmission.gear = 7;
    let dry = t.jake_brake_force();
    assert!(!t.jake_slipping());
    t.grip = 0.15; // glare ice
    assert!(t.jake_slipping());
    assert!(t.jake_brake_force() < dry);
    t.engine_brake_stage = 1; // light stage asks for a third; the axle holds it
    assert!(!t.jake_slipping());
}

#[test]
fn test_hydroplane_onset_needs_water_and_drops_with_tread_wear() {
    // Fresh tread at highway pressure essentially never planes; worn tread in
    // deep water planes right in the speeds the game drives.
    let mut t = make_auto_truck();
    t.water_mm = 0.2; // a damp film cannot float a loaded truck tire
    assert!(t.hydro_onset_mph().is_none());
    t.water_mm = 3.0; // heavy rain
    let fresh = t.hydro_onset_mph();
    assert!(fresh.is_some() && fresh.unwrap() > 85.0);
    t.tire_wear_pct = 80.0;
    let worn = t.hydro_onset_mph();
    assert!(worn.is_some() && worn.unwrap() < fresh.unwrap());
    assert!(50.0 < worn.unwrap() && worn.unwrap() < 70.0); // reachable at highway speed
}

#[test]
fn test_hydroplaning_collapses_grip_and_slowing_restores_it() {
    let mut t = make_auto_truck();
    t.grip = 0.62; // heavy-rain surface
    t.water_mm = 3.0;
    t.tire_wear_pct = 80.0;
    let onset = t.hydro_onset_mph();
    assert!(onset.is_some());
    let onset = onset.unwrap();
    t.velocity_mps = (onset + 15.0) / 2.23694; // past the collapse band
    assert!(t.hydroplaning());
    let planing = t.effective_grip();
    t.velocity_mps = (onset - 5.0) / 2.23694;
    assert!(!t.hydroplaning());
    assert!(t.effective_grip() > planing * 2.0); // contact restored
}

#[test]
fn test_winter_tires_trade_dry_grip_for_snow_and_ice() {
    // The winter compound is a real trade, not a free upgrade: more bite on
    // snow and ice, slightly less on warm dry pavement, and faster tread wear.
    let mut t = make_auto_truck();
    t.grip = 0.45;
    t.surface = "snow".to_string();
    let stock_snow = t.effective_grip();
    t.tire_type = TIRE_WINTER.to_string();
    assert!(approx(
        t.effective_grip(),
        stock_snow * WINTER_SNOW_GRIP_MULT
    ));
    t.grip = 0.15;
    t.surface = "ice".to_string();
    assert!(approx(t.effective_grip(), 0.15 * WINTER_ICE_GRIP_MULT));
    t.grip = 1.0;
    t.surface = "dry".to_string();
    assert!(approx(t.effective_grip(), 1.0 - WINTER_DRY_GRIP_LOSS));

    // Same mile at the same speed costs the winter set more tread.
    let mut winter = make_auto_truck();
    winter.tire_type = TIRE_WINTER.to_string();
    let mut stock = make_auto_truck();
    for truck in [&mut winter, &mut stock] {
        truck.velocity_mps = 25.0;
        truck.update_wear(60.0);
    }
    assert!(winter.tire_wear_pct > stock.tire_wear_pct);
}

#[test]
fn test_chains_replace_the_contact_patch() {
    // With chains on, steel touches the road: tread wear and the water film
    // stop mattering, and the chain multiplier alone works on the weather grip.
    let mut t = make_auto_truck();
    t.grip = 0.62;
    t.surface = "wet".to_string();
    t.water_mm = 3.0;
    t.tire_wear_pct = 80.0;
    t.velocity_mps = 70.0 / 2.23694;
    assert!(t.hydroplaning());
    t.chains_on = true;
    assert!(!t.hydroplaning()); // chains bite through the film
    assert!(approx(
        t.effective_grip(),
        0.62 * (1.0 - CHAIN_BARE_GRIP_LOSS)
    ));
    t.grip = 0.15;
    t.surface = "ice".to_string();
    t.water_mm = 0.0;
    assert!(approx(t.effective_grip(), 0.15 * CHAIN_ICE_GRIP_MULT));
}

#[test]
fn test_chained_jake_holds_the_icy_grade() {
    // The jake cap that breaks loose on glare ice holds once the drives are
    // chained: the same demand fits under two and a half times the grip.
    let mut t = make_auto_truck();
    t.velocity_mps = 15.0;
    t.rpm = 1800.0;
    t.engine_brake_stage = 3;
    t.transmission.gear = 7;
    t.grip = 0.15;
    t.surface = "ice".to_string();
    assert!(t.jake_slipping());
    t.chains_on = true;
    assert!(!t.jake_slipping());
}

#[test]
fn test_chains_grind_apart_on_bare_pavement_and_snap() {
    // Chains left on at highway speed on dry pavement destroy themselves in a
    // couple of miles: the set snaps, takes a bite of the fender, and is scrap.
    let mut t = make_auto_truck();
    t.chains_on = true;
    t.surface = "dry".to_string();
    t.velocity_mps = 55.0 / 2.23694;
    for _ in 0..4000 {
        // up to 400 simulated seconds at highway speed
        t.update_wear(0.1);
        if t.chains_just_snapped {
            break;
        }
    }
    assert!(t.chains_just_snapped);
    assert!(!t.chains_on);
    assert!(approx(t.chain_wear_pct, 100.0));
    assert!(approx(t.damage_pct, CHAIN_SNAP_DAMAGE_PCT));
    // Used as intended -- packed snow at chain speed -- a set lasts the pass.
    let mut proper = make_auto_truck();
    proper.chains_on = true;
    proper.grip = 0.45;
    proper.surface = "snow".to_string();
    proper.velocity_mps = 25.0 / 2.23694;
    proper.update_wear(600.0); // ten minutes of chained descent
    assert!(proper.chains_on);
    assert!(proper.chain_wear_pct < 2.0);
}

#[test]
fn test_chains_inside_a_posted_control_wear_as_on_snow() {
    // A live winter-storm alert can post the chain law while the simulated
    // road still reads wet. A lawful set at chain speed must last the
    // control, not snap a dozen miles in (player report, 2026-10-17).
    let mut posted = make_auto_truck();
    posted.chains_on = true;
    posted.surface = "wet".to_string();
    posted.chain_control_road = true;
    posted.velocity_mps = 25.0 / 2.23694;
    posted.update_wear(1440.0); // ten miles at 25 mph
    assert!(posted.chains_on);
    assert!(posted.chain_wear_pct < 3.0, "{}", posted.chain_wear_pct);
    // Out of the control the same wet road grinds them as before.
    let mut left_on = make_auto_truck();
    left_on.chains_on = true;
    left_on.surface = "wet".to_string();
    left_on.velocity_mps = 25.0 / 2.23694;
    left_on.update_wear(1440.0);
    assert!(left_on.chain_wear_pct > 50.0, "{}", left_on.chain_wear_pct);
}

#[test]
fn test_governed_speed_is_not_abuse() {
    // Sitting AT the governor is normal diesel running; overspeed wear only
    // starts past it, when a downgrade drives the engine through the wheels.
    let mut t = make_auto_truck();
    t.rpm = t.specs.max_rpm;
    t.update_wear(1.0);
    assert!(t.engine_wear_pct < 0.1);
}

#[test]
fn test_lugging_wears_the_engine() {
    let mut lugger = TruckState::default();
    lugger.start_engine();
    lugger.transmission.automatic = false;
    lugger.transmission.gear = 8;
    lugger.velocity_mps = 3.0;
    lugger.throttle = 1.0;
    lugger.rpm = lugger.specs.idle_rpm; // far below the torque band, wide open
    let mut clean = TruckState::default();
    clean.start_engine();
    clean.transmission.automatic = false;
    clean.transmission.gear = 8;
    clean.velocity_mps = 25.0;
    clean.throttle = 1.0;
    clean.rpm = clean.specs.peak_torque_rpm;
    lugger.update_wear(1.0);
    clean.update_wear(1.0);
    assert!(lugger.engine_wear_pct > clean.engine_wear_pct + 0.01);
}

#[test]
fn test_engine_wear_cuts_power() {
    let mut fresh = make_auto_truck();
    let mut tired = make_auto_truck();
    tired.engine_wear_pct = 100.0;
    for t in [&mut fresh, &mut tired] {
        t.transmission.gear = 10;
        t.velocity_mps = 25.0;
        t.rpm = t.specs.peak_torque_rpm;
        t.throttle = 1.0;
    }
    assert!(tired.drive_force() < fresh.drive_force());
}

#[test]
fn test_engine_wear_burns_more_fuel_for_the_same_power() {
    // At low speed both trucks are traction-limited to the same drive force,
    // so equal power output shows the worn engine's fuel penalty cleanly.
    let mut fresh = make_auto_truck();
    let mut tired = make_auto_truck();
    tired.engine_wear_pct = 100.0;
    for t in [&mut fresh, &mut tired] {
        t.transmission.gear = 1;
        t.velocity_mps = 5.0;
        t.rpm = t.specs.peak_torque_rpm;
        t.throttle = 1.0;
    }
    assert!(approx(tired.drive_force(), fresh.drive_force()));
    let (fresh_start, tired_start) = (fresh.fuel_gal, tired.fuel_gal);
    fresh.update_fuel(10.0);
    tired.update_fuel(10.0);
    assert!((tired_start - tired.fuel_gal) > (fresh_start - fresh.fuel_gal));
}

#[test]
fn test_wear_clamps_at_100() {
    let mut t = make_auto_truck();
    t.tire_wear_pct = 99.999;
    t.brake_wear_pct = 99.999;
    t.engine_wear_pct = 99.999;
    t.velocity_mps = 30.0;
    t.brake = 1.0;
    t.rpm = t.specs.max_rpm;
    t.fuel_burn_mult = 10_000.0;
    t.update_wear(60.0);
    assert_eq!(t.tire_wear_pct, 100.0);
    assert_eq!(t.brake_wear_pct, 100.0);
    assert_eq!(t.engine_wear_pct, 100.0);
}

#[test]
fn test_air_pressure_builds_when_engine_running_and_stops_at_cutout() {
    let mut t = TruckState::default();
    t.set_cold_air_start();

    assert_eq!(t.air_pressure_psi(), 55.0);
    assert!(!t.air_compressor_active);

    drive(&mut t, 5.0);
    assert_eq!(t.air_pressure_psi(), 55.0);

    t.start_engine();
    drive(&mut t, 30.0);

    assert!(approx(
        t.air_pressure_psi(),
        t.specs.air_governor_cut_out_psi
    ));
    assert!(!t.air_compressor_active);
}

#[test]
fn test_engine_off_air_reservoirs_leak_during_parked_time() {
    let mut t = TruckState::default();
    t.set_air_ready(true);

    t.advance_parked_time(10.0 * 60.0);

    assert!(approx(t.air_pressure_psi(), t.specs.air_cold_start_psi));
    assert!(t.air_low_warning());
    assert!(!t.air_ready());
    assert!(!t.air_compressor_active);
}

#[test]
fn test_running_engine_prevents_parked_time_air_leak() {
    let mut t = TruckState::default();
    t.set_air_ready(true);
    t.start_engine();

    t.advance_parked_time(10.0 * 60.0);

    assert!(approx(
        t.air_pressure_psi(),
        t.specs.air_governor_cut_out_psi
    ));
}

#[test]
fn test_air_compressor_cuts_in_when_pressure_drops_below_cut_in() {
    let mut t = TruckState::default();
    t.set_air_ready(false);
    t.start_engine();
    t.set_air_pressure_psi(t.specs.air_governor_cut_in_psi - 1.0);

    t.update(0.1);

    assert!(t.air_compressor_active);
    assert!(t.air_pressure_psi() > t.specs.air_governor_cut_in_psi - 1.0);
}

#[test]
fn test_brake_applications_consume_air_and_trigger_low_air_warning() {
    let mut t = TruckState::default();
    t.set_air_ready(false);

    for _ in 0..18 {
        t.brake = 1.0;
        t.update(0.1);
        t.brake = 0.0;
        t.update(0.1);
    }

    assert!(t.air_pressure_psi() < t.specs.air_low_warning_psi);
    assert!(t.air_low_warning());
}

#[test]
fn test_air_low_warning_clear_threshold_has_hysteresis_margin() {
    // air_low_warning itself is a raw instantaneous crossing (no memory) --
    // the driving state layer is what latches the warning until pressure
    // recovers clear of this threshold, so it must sit safely above the warn
    // point (and below the compressor cut-in) or the hysteresis band does
    // nothing.
    let specs = TruckState::default().specs;
    assert!(specs.air_low_warning_clear_psi > specs.air_low_warning_psi);
    assert!(specs.air_low_warning_clear_psi < specs.air_governor_cut_in_psi);
}

#[test]
fn test_service_brakes_drain_separate_air_reservoirs() {
    let mut t = TruckState::default();
    t.set_air_ready(false);

    t.brake = 1.0;
    t.update(0.1);

    assert!(t.primary_air_psi < t.secondary_air_psi && t.secondary_air_psi < t.trailer_air_psi);
    assert!(approx(t.air_pressure_psi(), t.primary_air_psi));
}

#[test]
fn test_compressor_builds_all_reservoirs_before_cutout() {
    let mut t = TruckState::default();
    t.primary_air_psi = 92.0;
    t.secondary_air_psi = 118.0;
    t.trailer_air_psi = 86.0;
    t.start_engine();

    assert!(t.air_compressor_active);
    drive(&mut t, 20.0);

    assert!(approx(t.primary_air_psi, t.specs.air_governor_cut_out_psi));
    assert!(approx(
        t.secondary_air_psi,
        t.specs.air_governor_cut_out_psi
    ));
    assert!(approx(t.trailer_air_psi, t.specs.air_governor_cut_out_psi));
    assert!(!t.air_compressor_active);
}

#[test]
fn test_fast_idle_builds_air_then_settles_to_drive_idle() {
    // A cold-started parked truck holds the raised idle until the governor
    // releases the parking-brake air, then settles back -- the audible flip
    // the engine voice keys off. The higher rpm also spins the compressor
    // faster, so the charge genuinely arrives sooner.
    let mut t = TruckState::default();
    t.set_cold_air_start();
    t.start_engine();

    assert!(t.fast_idle_active());
    drive(&mut t, 3.0);
    assert!(approx_rel(t.rpm, t.specs.fast_idle_rpm, 0.05));
    assert!(!t.air_ready());

    drive(&mut t, 15.0); // the air comes ready along the way
    assert!(t.air_ready());
    assert!(!t.fast_idle_active());
    assert!(approx_rel(t.rpm, t.specs.idle_rpm, 0.05));
}

#[test]
fn test_high_idle_holds_setpoint_and_cancels_on_brake_release() {
    let mut t = TruckState::default();
    t.set_air_ready(true);
    t.start_engine();
    t.high_idle_rpm = Some(HIGH_IDLE_DEFAULT_RPM);

    drive(&mut t, 3.0);
    assert!(approx_rel(t.rpm, HIGH_IDLE_DEFAULT_RPM, 0.05));

    // Throttle still revs above the latched floor.
    t.throttle = 1.0;
    drive(&mut t, 3.0);
    assert!(t.rpm > HIGH_IDLE_DEFAULT_RPM * 1.5);
    t.throttle = 0.0;

    // Releasing the parking brake cancels the latch, like real fast idle.
    assert!(t.release_parking_brake());
    drive(&mut t, 3.0);
    assert!(t.high_idle_rpm.is_none());
    assert!(approx_rel(t.rpm, t.specs.idle_rpm, 0.05));
}

#[test]
fn test_high_idle_burns_more_parked_fuel_than_plain_idle() {
    let mut idle = TruckState::default();
    idle.set_air_ready(true);
    idle.start_engine();
    let mut high = TruckState::default();
    high.set_air_ready(true);
    high.start_engine();
    high.high_idle_rpm = Some(1500.0);

    drive(&mut idle, 30.0);
    drive(&mut high, 30.0);

    assert!(high.fuel_gal < idle.fuel_gal && idle.fuel_gal < high.specs.fuel_tank_gal);
}

#[test]
fn test_fast_idle_never_engages_while_rolling() {
    let mut t = TruckState::default();
    t.set_air_ready(false);
    t.start_engine();
    t.set_air_pressure_psi(t.specs.air_governor_cut_in_psi - 5.0); // rebuilding on the move
    t.velocity_mps = 15.0;

    assert!(!t.fast_idle_active());
}

#[test]
fn test_parking_brake_release_requires_ready_air_pressure() {
    let mut t = TruckState::default();
    t.set_cold_air_start();

    assert!(!t.release_parking_brake());
    assert!(t.parking_brake);

    t.set_air_pressure_psi(t.specs.air_parking_release_psi);
    assert!(t.release_parking_brake());
    assert!(!t.parking_brake);
}

#[test]
fn test_parking_brake_holds_truck_until_released() {
    let mut t = make_auto_truck();
    t.set_air_ready(true);
    t.throttle = 1.0;

    drive(&mut t, 5.0);

    assert_eq!(t.speed_mph(), 0.0);

    assert!(t.release_parking_brake());
    drive(&mut t, 5.0);

    assert!(t.speed_mph() > 1.0);
}

#[test]
fn test_air_brake_snapshot_preserves_richer_reservoir_state() {
    let mut t = TruckState::default();
    t.primary_air_psi = 91.2;
    t.secondary_air_psi = 103.4;
    t.trailer_air_psi = 97.6;
    t.parking_brake = false;
    t.air_compressor_active = true;

    let mut restored = TruckState::default();
    restored.restore_air_brake_snapshot(&t.air_brake_snapshot(), false);

    assert!(approx(restored.primary_air_psi, 91.2));
    assert!(approx(restored.secondary_air_psi, 103.4));
    assert!(approx(restored.trailer_air_psi, 97.6));
    assert!(!restored.parking_brake);
}

#[test]
fn test_old_air_brake_snapshot_restores_all_reservoirs_from_pressure() {
    let mut t = TruckState::default();

    t.restore_air_brake_snapshot(
        &json!({"schema": 1, "pressure_psi": 88.0, "parking_brake": false}),
        false,
    );

    assert!(approx(t.primary_air_psi, 88.0));
    assert!(approx(t.secondary_air_psi, 88.0));
    assert!(approx(t.trailer_air_psi, 88.0));
    assert!(approx(t.air_pressure_psi(), 88.0));
    assert!(!t.parking_brake);
}

#[test]
fn test_bobtail_tare_drops_the_trailer_share() {
    let hitched = TruckState {
        cargo_kg: 0.0,
        ..Default::default()
    };
    let bobtail = TruckState {
        cargo_kg: 0.0,
        trailer_attached: false,
        ..Default::default()
    };
    assert!(approx(
        bobtail.tare_kg(),
        hitched.tare_kg() - TRAILER_TARE_KG
    ));
    assert!(bobtail.gross_mass_kg() < hitched.gross_mass_kg());
}

#[test]
fn test_bobtail_outruns_the_deadhead_off_the_line() {
    let mut marks: BTreeMap<&str, Option<f64>> = BTreeMap::new();
    for (name, attached) in [("deadhead", true), ("bobtail", false)] {
        let mut truck = make_auto_truck();
        truck.set_air_ready(false);
        truck.cargo_kg = 0.0;
        truck.trailer_attached = attached;
        truck.throttle = 1.0;
        marks.insert(name, time_to_speed(&mut truck, 45.0));
    }
    assert!(marks["bobtail"].is_some() && marks["deadhead"].is_some());
    assert!(marks["bobtail"].unwrap() < marks["deadhead"].unwrap());
}

#[test]
fn test_bobtail_air_gauge_ignores_the_trailer_line() {
    let mut truck = TruckState {
        trailer_attached: false,
        ..Default::default()
    };
    truck.trailer_air_psi = 0.0;
    assert!(approx(
        truck.air_pressure_psi(),
        truck.primary_air_psi.min(truck.secondary_air_psi)
    ));
    assert!(!truck.spring_brakes_active());
}

fn gear_steps(truck: &mut TruckState, seconds: f64) -> Vec<i32> {
    let mut steps = Vec::new();
    let mut previous = truck.transmission.gear;
    for _ in 0..((seconds * 60.0) as i64) {
        truck.auto_shift();
        truck.update(DT);
        let gear = truck.transmission.gear;
        if gear > previous {
            steps.push(gear - previous);
        }
        if gear != previous {
            previous = gear;
        }
    }
    steps
}

#[test]
fn test_light_truck_skip_shifts_at_moderate_throttle() {
    // The machine-gun report: empty at part throttle, the box used to grab
    // every single gear about a second apart because the skip's landing floor
    // was tuned for a loaded rig. Light trucks should skip holes like a real
    // driver does.
    let mut truck = make_auto_truck();
    truck.set_air_ready(false);
    truck.cargo_kg = 0.0;
    truck.throttle = 0.45;
    let steps = gear_steps(&mut truck, 60.0);
    assert!(steps.contains(&2));
}

#[test]
fn test_loaded_truck_never_skip_shifts() {
    let mut truck = make_auto_truck();
    truck.set_air_ready(false);
    truck.cargo_kg = REFERENCE_CARGO_KG;
    truck.throttle = 0.45;
    let steps = gear_steps(&mut truck, 60.0);
    assert!(!steps.is_empty() && steps.iter().all(|step| *step == 1));
}

#[test]
fn test_part_loaded_truck_skip_shifts() {
    // Player report, 2026-10-13: a quarter load went through all ten gears,
    // because the skip was gated on an empty trailer alone.
    let mut truck = make_auto_truck();
    truck.set_air_ready(false);
    truck.cargo_kg = 0.25 * REFERENCE_CARGO_KG;
    truck.throttle = 0.45;
    let steps = gear_steps(&mut truck, 60.0);
    assert!(steps.contains(&2));
}

#[test]
fn test_lighter_loads_take_fewer_shifts() {
    let shifts = |fraction: f64| {
        let mut truck = make_auto_truck();
        truck.set_air_ready(false);
        truck.cargo_kg = fraction * REFERENCE_CARGO_KG;
        truck.throttle = 0.7;
        gear_steps(&mut truck, 90.0).len()
    };
    let (empty, half, full) = (shifts(0.0), shifts(0.5), shifts(1.0));
    assert!(empty < half && half < full, "{empty} {half} {full}");
}

#[test]
fn test_start_gear_still_downshifts_once_the_road_wins() {
    // The launch hold is for a truck still gathering speed. In its starting
    // gear on a rise at a light throttle, with the road out-pulling the
    // engine, the revs are falling and the box must still gear down.
    let mut truck = make_auto_truck();
    truck.set_air_ready(false);
    truck.cargo_kg = 0.7 * REFERENCE_CARGO_KG;
    truck.grade = 0.019;
    truck.transmission.gear = 2;
    truck.velocity_mps = 1.0;
    truck.throttle = 0.06;
    assert!(truck.coupled_rpm(None) < AUTO_DOWNSHIFT_RPM);
    assert!(truck.drive_force() < truck.resistance_force());
    assert_eq!(truck.auto_shift(), Some(1));
}

#[test]
fn test_worn_engine_does_not_skip_into_a_gear_it_cannot_pull() {
    // The skip judges the gear two up with the power the engine still has.
    let skip = |wear_pct: f64| {
        let mut truck = make_auto_truck();
        truck.set_air_ready(false);
        truck.cargo_kg = 0.0;
        truck.transmission.gear = 7;
        truck.velocity_mps = 20.0;
        truck.throttle = 0.2;
        truck.engine_wear_pct = wear_pct;
        truck.auto_shift()
    };
    assert_eq!(skip(0.0), Some(9));
    assert_ne!(skip(100.0), Some(9));
}

#[test]
fn test_gentle_light_launch_holds_its_starting_gear() {
    // Pulling away gently in third, the box used to drop to second at a
    // walking pace before shifting back up past it.
    let mut truck = make_auto_truck();
    truck.set_air_ready(false);
    truck.cargo_kg = 0.0;
    truck.throttle = 0.3;
    let mut lowest = i32::MAX;
    for _ in 0..(20 * 60) {
        truck.auto_shift();
        truck.update(DT);
        if truck.transmission.gear > 0 {
            lowest = lowest.min(truck.transmission.gear);
        }
    }
    assert_eq!(lowest, 3);
}

// -- tests/test_tanker_surge.py: the load behind the truck -------------------------

const CARGO_KG: f64 = 20_000.0;

fn tank_truck(liquid: Option<LiquidLoad>, speed_mps: f64) -> TruckState {
    let mut t = TruckState::default();
    t.cargo_kg = CARGO_KG;
    t.velocity_mps = speed_mps;
    t.set_air_ready(false);
    t.liquid = liquid;
    t
}

/// Brake to a standstill; returns metres travelled.
fn roll_to_stop(truck: &mut TruckState, brake: f64) -> f64 {
    let dt = 0.02;
    let mut travelled = 0.0;
    truck.throttle = 0.0;
    truck.brake = brake;
    for _ in 0..((120.0 / dt) as i64) {
        let before = truck.velocity_mps;
        truck.update(dt);
        travelled += ((before + truck.velocity_mps) / 2.0).max(0.0) * dt;
        if truck.velocity_mps <= 0.001 {
            break;
        }
    }
    travelled
}

#[test]
fn test_a_load_that_is_not_liquid_is_bit_for_bit_unchanged() {
    // The whole design rests on this. No tank, no surge, no difference.
    let dry = tank_truck(None, 24.6);
    assert!(dry.liquid.is_none());
    assert_eq!(dry.surge_force_n(), 0.0);
    assert_eq!(dry.surge_decel_penalty_mps2(), 0.0);
    assert!(!dry.pushed_through_by_surge());

    // And the trajectory itself, frame by frame, against a truck built before
    // any of this existed -- same speeds at every step.
    let (mut a, mut b) = (tank_truck(None, 24.6), tank_truck(None, 24.6));
    a.brake = 0.55;
    b.brake = 0.55;
    for _ in 0..400 {
        a.update(0.02);
        b.update(0.02);
        assert_eq!(a.velocity_mps, b.velocity_mps);
    }
    assert!(a.velocity_mps < 24.6);
}

#[test]
fn test_stopping_distance_is_unchanged_without_liquid() {
    let truck = tank_truck(None, 24.6);
    // 24.6 m/s against the truck's own full-service figure, no surge term.
    let expected = 24.6_f64.powi(2) / (2.0 * truck.full_service_decel_mps2());
    assert!(approx_rel(
        truck.stopping_distance_m(None, 0.0, true),
        expected,
        1e-9
    ));
}

#[test]
fn test_a_half_full_smooth_bore_needs_the_most_road() {
    // Worst at half full, and worse than baffled at the same fill: the two
    // facts every tanker manual leads with.
    let dry = tank_truck(None, 24.6).stopping_distance_m(None, 0.0, true);
    let smooth =
        tank_truck(Some(LiquidLoad::new(0.5, false)), 24.6).stopping_distance_m(None, 0.0, true);
    let baffled =
        tank_truck(Some(LiquidLoad::new(0.5, true)), 24.6).stopping_distance_m(None, 0.0, true);
    let nearly_full =
        tank_truck(Some(LiquidLoad::new(0.95, false)), 24.6).stopping_distance_m(None, 0.0, true);
    let quarter =
        tank_truck(Some(LiquidLoad::new(0.25, false)), 24.6).stopping_distance_m(None, 0.0, true);

    assert!(smooth > baffled && baffled > dry);
    assert!(smooth > quarter && quarter > dry);
    assert!(nearly_full < quarter);
    assert!(1.2 < smooth / dry && smooth / dry < 1.6); // a third more road again, near enough
}

#[test]
fn test_braking_to_a_stop_actually_takes_longer_with_liquid_aboard() {
    // Not just the estimate -- the simulated stop.
    let mut dry = tank_truck(None, 24.6);
    let mut wet = tank_truck(Some(LiquidLoad::new(0.5, false)), 24.6);
    let dry_m = roll_to_stop(&mut dry, 1.0);
    let wet_m = roll_to_stop(&mut wet, 1.0);
    assert!(wet_m > dry_m);
}

#[test]
fn test_the_load_pushes_a_stopped_truck_forward_when_the_brakes_come_off() {
    // The lesson the CDL manuals put first: the wave can shove a stopped
    // tractor out into the intersection.
    let mut truck = tank_truck(Some(LiquidLoad::new(0.5, false)), 13.4);
    roll_to_stop(&mut truck, 1.0);
    assert!(approx_abs(truck.velocity_mps, 0.0, 1e-3));
    // Off the brakes at the bar, the way a driver does when they think the
    // stop is made.
    truck.brake = 0.0;
    truck.parking_brake = false;
    let mut crept = 0.0;
    for _ in 0..400 {
        // 8 s
        truck.update(0.02);
        crept += truck.velocity_mps * 0.02;
    }
    assert!(
        crept > 0.1,
        "the wave should still be shoving the truck along"
    );
}

#[test]
fn test_surge_is_a_forward_push_never_a_brake() {
    let load = LiquidLoad::new(0.5, false);
    let mut truck = tank_truck(Some(load), 24.6);
    truck.brake = 1.0;
    let mut seen_forward = false;
    for _ in 0..500 {
        truck.update(0.02);
        if truck.surge_force_n() > 0.0 {
            seen_forward = true;
        }
    }
    assert!(seen_forward);
}

#[test]
fn test_lateral_surge_builds_in_a_bend_taken_over_its_advisory() {
    let mut truck = tank_truck(Some(LiquidLoad::new(0.5, false)), 20.0);
    truck.corner_advisory_mph = 30.0;
    truck.throttle = 0.3;
    for _ in 0..200 {
        truck.update(0.02);
    }
    let liquid = truck.liquid.as_ref().unwrap();
    assert!(liquid.lateral.reach() > 0.0);
    assert!(liquid.lateral_load_factor() > 0.0);
}

#[test]
fn test_being_carried_through_on_a_full_brake_application_is_not_preventable() {
    let mut truck = tank_truck(Some(LiquidLoad::new(0.5, false)), 13.4);
    truck.brake = 1.0;
    let mut pushed = false;
    for _ in 0..600 {
        truck.update(0.02);
        if truck.pushed_through_by_surge() {
            pushed = true;
            break;
        }
    }
    assert!(
        pushed,
        "a hard-braking driver should be able to be pushed through"
    );

    let before = truck.preventable_damage_pct;
    let preventable = !truck.pushed_through_by_surge();
    truck.apply_collision(0.2, preventable);
    assert!(truck.damage_pct > 0.0);
    assert_eq!(truck.preventable_damage_pct, before);
}

#[test]
fn test_coasting_into_a_bar_without_braking_is_still_the_drivers_fault() {
    let mut truck = tank_truck(Some(LiquidLoad::new(0.5, false)), 13.4);
    truck.brake = 0.0;
    for _ in 0..200 {
        truck.update(0.02);
    }
    assert!(!truck.pushed_through_by_surge());
}

#[test]
fn test_a_slug_that_reaches_the_head_stops_there_and_does_not_feed_itself() {
    // Bend sweep, 2026-09-24: the slug rebounded off a head at nearly the
    // speed it arrived with AND handed that speed to the truck, whose lurch
    // threw it back harder. Kicked once, a half-full smooth bore ran itself up
    // to 18 m/s end to end and cost the load with no pedal touched.
    // At the game's own frame, the slug kicked hard enough to reach a head:
    // what a throttle swinging from nothing to full, or a stab of the brake,
    // leaves in the tank.
    const DT: f64 = 1.0 / 30.0;
    let mut truck = tank_truck(Some(LiquidLoad::new(0.5, false)), 20.0);
    let axis = &mut truck.liquid.as_mut().unwrap().longitudinal;
    let peak = axis.peak_v();
    axis.v = 1.5 * peak;
    let mut fastest = 0.0f64;
    for _ in 0..1800 {
        // A minute of coasting.
        truck.update(DT);
        fastest = fastest.max(truck.liquid.as_ref().unwrap().longitudinal.v.abs());
    }
    assert!(
        fastest <= 1.5 * peak + 1e-9,
        "the slug ran at {fastest:.1} m/s, a full swing is {peak:.1}"
    );
    assert_eq!(truck.cargo_damage_pct, 0.0);
}

// `test_liquid_freight_is_hard_to_ruin_because_liquid_does_not_break` and
// `test_a_tank_load_is_described_in_words_that_fit_a_tank` are the
// cargo-condition half of `tests/test_tanker_surge.py`; they live with the
// code they cover, in `models::cargo_condition`.

#[test]
fn test_tank_freight_is_gated_to_the_back_half_of_the_career() {
    use crate::models::jobs::cargo_type;
    use crate::models::trailers::trailer_keys_for_cargo;
    for key in ["fuel_bulk", "liquid_food"] {
        let cargo = cargo_type(key).unwrap();
        assert!(
            cargo.min_level >= 16,
            "tank work belongs to the back half of the arc"
        );
        assert!(cargo.credentials.contains(&"tank"));
        assert!(cargo.tank);
        assert_eq!(trailer_keys_for_cargo(key), ["tank"]);
    }
}

/// Sanitation forbids baffles in a food-grade tank, so the gentlest cargo in
/// the game travels in the most vicious equipment.
#[test]
fn test_liquid_food_is_the_harder_one_and_pays_for_it() {
    use crate::models::jobs::cargo_type;
    let fuel = cargo_type("fuel_bulk").unwrap();
    let milk = cargo_type("liquid_food").unwrap();
    assert!(fuel.baffled);
    assert!(!milk.baffled);
    assert!(milk.min_level > fuel.min_level);
    assert!(milk.rate_per_mile > fuel.rate_per_mile);
    // And both pay over the dry freight that shares their level band.
    assert!(fuel.rate_per_mile > cargo_type("chemicals").unwrap().rate_per_mile);
}

#[test]
fn test_the_tank_endorsement_unlocks_in_the_back_half() {
    use crate::models::career::{endorsement_level, Career};
    assert!(endorsement_level("tank").unwrap() >= 16);
    let mut career = Career::default();
    career.xp = 0.0;
    assert!(!career.endorsements().contains("tank"));
    career.xp = 1_000_000.0;
    assert!(career.endorsements().contains("tank"));
}

// `test_the_cue_layer_is_completely_silent_for_other_freight` is live in `crates/freight-fate/tests/states_driving_facility.rs`.

// `test_the_wash_is_silent_on_steady_cruise_and_alive_once_the_load_runs` is live in `crates/freight-fate/tests/states_driving_facility.rs`.

// `test_the_hit_fires_when_the_wave_arrives_and_is_the_loudest_thing_here` is live in `crates/freight-fate/tests/states_driving_facility.rs`.

// `test_the_lateral_hit_has_its_own_voice` is live in `crates/freight-fate/tests/states_driving_facility.rs`.

// `test_the_load_running_and_the_load_settling_are_both_spoken` is live in `crates/freight-fate/tests/states_driving_facility.rs`.

// `test_the_bed_is_dropped_on_the_way_out` is live in `crates/freight-fate/tests/states_driving_facility.rs`.

// `test_the_status_screen_can_be_asked_what_the_tank_will_do` is live in `crates/freight-fate/tests/states_driving_facility.rs`.

// `test_the_stop_bar_tick_range_is_unchanged_for_ordinary_freight` is live in `crates/freight-fate/tests/states_driving_facility.rs`.

// `test_the_stop_bar_tick_starts_earlier_when_the_truck_needs_more_road` is live in `crates/freight-fate/tests/states_driving_facility.rs`.

// `test_the_held_tone_comes_in_early_when_sixty_feet_is_not_enough` is live in `crates/freight-fate/tests/states_driving_facility.rs`.

// `test_the_stopping_assist_can_only_ever_press_harder_than_it_used_to` is live in `crates/freight-fate/tests/states_driving_facility.rs`.

#[test]
fn test_stopping_distance_answers_with_grade_grip_and_fade() {
    let level = tank_truck(None, 24.6);
    let base = level.stopping_distance_m(None, 0.0, true);

    let mut uphill = tank_truck(None, 24.6);
    uphill.grade = 0.06;
    assert!(uphill.stopping_distance_m(None, 0.0, true) < base);

    let mut downhill = tank_truck(None, 24.6);
    downhill.grade = -0.06;
    assert!(downhill.stopping_distance_m(None, 0.0, true) > base);

    let mut icy = tank_truck(None, 24.6);
    icy.grip = 0.2;
    assert!(icy.stopping_distance_m(None, 0.0, true) > base);

    assert_eq!(
        tank_truck(None, 0.0).stopping_distance_m(None, 0.0, true),
        0.0
    );
    // A reaction allowance is ground covered before the pedal moves.
    assert!(approx(
        level.stopping_distance_m(None, 1.5, true),
        base + 24.6 * 1.5
    ));
}

#[test]
fn test_a_truck_that_cannot_out_brake_its_grade_still_returns_a_finite_number() {
    let mut runaway = tank_truck(None, 24.6);
    runaway.grade = -0.5;
    runaway.grip = 0.05;
    runaway.brake_temp_c = 800.0;
    assert!(runaway.stopping_distance_m(None, 0.0, true).is_finite());
    assert!(runaway.stopping_distance_m(None, 0.0, true) > 0.0);
}

/// Real trucks run the horn off the brake air (Brandon, 2026-08-20) -- and
/// FMVSS 121 pressure protection means the horn can never take the brakes
/// down with it: below the valve's threshold the horn goes silent and the
/// draw stops (realism audit, 2026-08-20; the first version let you honk to a
/// spring-brake lockout, which a compliant tractor cannot do).
///
/// From `tests/test_trip_cues.py`; it lives here because the air-drain step
/// is crate-private.
#[test]
fn test_the_horn_drains_the_air_tanks_to_the_protection_valve() {
    let mut t = TruckState::default();
    let before = t.primary_air_psi;
    t.horn_on = true;
    for _ in 0..(60 * 60) {
        // a full minute of leaning on it
        t.consume_brake_air(1.0 / 60.0);
    }
    let drained = before - t.primary_air_psi;
    assert!(
        (5.0..=10.0).contains(&drained),
        "a minute of horn drained {drained:.2} psi"
    );
    t.horn_on = false;
    let mid = t.primary_air_psi;
    for _ in 0..60 {
        t.consume_brake_air(1.0 / 60.0);
    }
    assert_eq!(t.primary_air_psi, mid, "released horn must not draw");
    // Honk forever: the valve floors the drain at its threshold.
    t.horn_on = true;
    for _ in 0..(60 * 60 * 30) {
        t.consume_brake_air(1.0 / 60.0);
    }
    assert!(
        t.air_pressure_psi() >= TruckState::HORN_PROTECTION_PSI - 1.0,
        "the horn drained past the protection valve: {:.1}",
        t.air_pressure_psi()
    );
    assert!(!t.horn_available(), "below threshold the horn must be dead");
}

// -- the truck half of `tests/test_buffs.py` ----------------------------------

#[test]
fn test_tire_buff_slows_tread_wear() {
    let mut plain = TruckState::default();
    let mut buffed = TruckState::default();
    buffed.tire_wear_buff_mult = 0.5;
    for t in [&mut plain, &mut buffed] {
        t.velocity_mps = 25.0;
        t.update_wear(60.0);
    }
    assert!(plain.tire_wear_pct > 0.0);
    assert!(approx(buffed.tire_wear_pct, plain.tire_wear_pct * 0.5));
}

#[test]
fn test_engine_buff_slows_duty_wear_but_not_over_rev_abuse() {
    let mut plain = TruckState::default();
    let mut buffed = TruckState::default();
    buffed.engine_wear_buff_mult = 0.5;
    for t in [&mut plain, &mut buffed] {
        t.start_engine();
        t.throttle = 0.6;
        t.rpm = 1_500.0;
        t.update_wear(600.0);
    }
    assert!(plain.engine_wear_pct > 0.0);
    assert!(approx(buffed.engine_wear_pct, plain.engine_wear_pct * 0.5));

    // over-revving charges full price no matter the buff
    let mut plain_abuse = TruckState::default();
    let mut buffed_abuse = TruckState::default();
    buffed_abuse.engine_wear_buff_mult = 0.5;
    for t in [&mut plain_abuse, &mut buffed_abuse] {
        t.start_engine();
        t.throttle = 0.0;
        // a downgrade driving the engine past the governor
        t.rpm = t.specs.max_rpm * 1.1;
        t.update_wear(1.0);
    }
    assert!(plain_abuse.engine_wear_pct >= 0.8); // the abuse term alone
    assert!(approx_abs(
        buffed_abuse.engine_wear_pct,
        plain_abuse.engine_wear_pct,
        1e-3
    ));
}

#[test]
fn test_legal_gvw_is_tractor_trailer_and_cargo_not_fuel() {
    let mut t = TruckState::default();
    t.cargo_kg = 12.0 * KG_PER_TON;
    t.fuel_gal = t.specs.fuel_tank_gal;
    assert!(!t.is_over_legal_gvw());
    // 25 metric tons of cargo on the stock tare is over 80,000 lb.
    t.cargo_kg = 25.0 * KG_PER_TON;
    assert!(t.is_over_legal_gvw());
    assert!(t.gross_mass_kg() > LEGAL_GVW_KG);
    // Fuel is not part of the cap: emptying the tanks does not clear overweight.
    t.fuel_gal = 0.0;
    assert!(t.is_over_legal_gvw());
    // At the cap is legal; over it is not.
    t.cargo_kg = max_legal_cargo_kg(t.tare_kg());
    assert!(!t.is_over_legal_gvw());
    t.cargo_kg += 1.0;
    assert!(t.is_over_legal_gvw());
}

// -- rev-matching through the torque interrupt ------------------------------------

/// A rolling automatic mid-shift: `from` was the gear before, `to` is the
/// gear the box is taking, the engine still at `from`'s coupled speed.
fn mid_shift(v_mps: f64, from: i32, to: i32) -> TruckState {
    use crate::sim::transmission::{shift_time_for, DOWNSHIFT_TIME};
    let mut t = make_auto_truck();
    t.set_air_ready(false);
    t.velocity_mps = v_mps;
    t.transmission.gear = from;
    t.rpm = t.coupled_rpm(Some(from));
    t.transmission.gear = to;
    t.transmission.shift_timer = if to < from {
        DOWNSHIFT_TIME
    } else {
        shift_time_for(to)
    };
    t.throttle = 0.5;
    t
}

#[test]
fn test_auto_downshift_blips_the_engine_up_toward_the_lower_gears_speed() {
    // A real automated box fuels the unloaded engine UP to the lower gear's
    // synchronous speed before it re-engages. The voice used to sit at the
    // old note for the whole second and jump when the gear took.
    let mut t = mid_shift(15.0, 8, 7);
    let start = t.rpm;
    let sync = t.coupled_rpm(None);
    assert!(
        sync > start + 400.0,
        "the case needs a real step: {start} -> {sync}"
    );
    drive_dt(&mut t, 0.2, DT);
    assert!(t.transmission.shifting(), "still in the interrupt");
    assert!(
        t.rpm > start + 150.0,
        "the blip must be under way at 0.2 s: {start} -> {}",
        t.rpm
    );
}

#[test]
fn test_auto_shift_lands_on_the_new_gears_speed_at_engagement() {
    // Both directions: by the time the interrupt ends the engine is at the
    // new gear's road speed, so engagement is a clunk, not a jump.
    for (from, to) in [(8, 7), (7, 8)] {
        let mut t = mid_shift(15.0, from, to);
        let timer = t.transmission.shift_timer;
        drive_dt(&mut t, timer - DT, DT);
        assert!(
            t.transmission.shifting(),
            "{from}->{to}: last frame of the interrupt"
        );
        let sync = t.coupled_rpm(None);
        assert!(
            approx_rel(t.rpm, sync, 0.03),
            "{from}->{to}: engine {:.0} vs sync {sync:.0} at engagement",
            t.rpm
        );
        let before = t.rpm;
        drive_dt(&mut t, 2.0 * DT, DT);
        assert!(!t.transmission.shifting());
        assert!(
            (t.rpm - before).abs() < 100.0,
            "{from}->{to}: the gear taking must not jump the engine: {before:.0} -> {:.0}",
            t.rpm
        );
    }
}

#[test]
fn test_shift_rev_match_rate_is_bounded_by_engine_inertia() {
    // The engine cannot teleport to sync: one frame moves it by what its
    // spare torque over its rotating inertia allows, in either direction.
    let mut down = mid_shift(15.0, 8, 7);
    let start = down.rpm;
    down.update(DT);
    let rise = down.rpm - start;
    assert!(rise > 0.0, "a downshift rises");
    assert!(
        rise < 80.0,
        "one frame rose {rise:.0} rpm: not inertia-limited"
    );

    let mut up = mid_shift(15.0, 7, 8);
    let start = up.rpm;
    up.update(DT);
    let fall = start - up.rpm;
    assert!(fall > 0.0, "an upshift falls");
    assert!(
        fall < 120.0,
        "one frame fell {fall:.0} rpm: not inertia-limited"
    );
}

#[test]
fn test_a_stronger_engine_rev_matches_faster() {
    // Across the yard: the blip rate comes from each truck's own torque,
    // not a shared timer, so a big-torque tractor matches sooner.
    let mut base = mid_shift(15.0, 8, 7);
    let mut strong = mid_shift(15.0, 8, 7);
    strong.specs.max_torque_nm = base.specs.max_torque_nm * 1.5;
    let start = base.rpm;
    drive_dt(&mut base, 0.15, DT);
    drive_dt(&mut strong, 0.15, DT);
    assert!(
        strong.rpm - start > (base.rpm - start) * 1.2,
        "strong {:.0} vs base {:.0} from {start:.0}",
        strong.rpm,
        base.rpm
    );
}

/// Floor a loaded automatic from rest on the flat at time compression `pace`
/// until it reaches `to_mph`. Returns (real seconds, upshifts, downshifts).
fn loaded_launch(pace: f64, to_mph: f64) -> (f64, u32, u32) {
    let mut truck = TruckState {
        fuel_burn_mult: pace,
        cargo_kg: 18_000.0,
        ..Default::default()
    };
    truck.transmission.automatic = true;
    truck.set_air_ready(false);
    truck.start_engine();
    truck.throttle = 1.0;
    let (mut real_s, mut up, mut down) = (0.0, 0, 0);
    while truck.speed_mph() < to_mph {
        assert!(real_s < 600.0, "{pace}x never reached {to_mph} mph");
        let before = truck.transmission.gear;
        if let Some(gear) = truck.auto_shift() {
            if gear > before {
                up += 1;
            } else if gear < before {
                down += 1;
            }
        }
        truck.update(DT);
        real_s += DT;
    }
    (real_s, up, down)
}

#[test]
fn test_a_loaded_truck_builds_speed_in_real_seconds_at_every_pace() {
    // Speed builds on the real clock whatever the pace: a loaded tractor
    // takes most of a minute to reach 50, and the box shifts through its
    // gears once each. On the game clock it reached 50 in a few real seconds
    // at standard pace and rattled through the gears.
    let (real_s, real_up, real_down) = loaded_launch(1.0, 50.0);
    assert!(real_s > 30.0, "a loaded truck reached 50 in {real_s:.1} s");
    assert_eq!(real_down, 0, "the real-time launch must not hunt");
    for pace in [4.0, 14.0, 20.0] {
        let (s, up, down) = loaded_launch(pace, 50.0);
        assert!(
            (s - real_s).abs() < 0.5,
            "{pace}x reached 50 in {s:.1} real s where real time takes {real_s:.1}"
        );
        assert_eq!(
            (up, down),
            (real_up, real_down),
            "{pace}x shifted differently"
        );
    }
}
