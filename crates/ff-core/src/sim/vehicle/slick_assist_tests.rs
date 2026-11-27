//! Brakes and throttle on slick roads: when ABS engages, and the pedals an
//! assist holds (2.0 alpha freezing-rain report, 2026-11-27).

use super::traction::ABS_SLIP_RATIO;
use super::*;

const ICE: f64 = 0.15;
const SNOW: f64 = 0.45;
const RAIN: f64 = 0.80;

fn rolling_truck(grip: f64, mph: f64) -> TruckState {
    let mut truck = TruckState::default();
    truck.transmission.automatic = true;
    truck.start_engine();
    truck.set_air_ready(false);
    truck.grip = grip;
    truck.velocity_mps = mph / 2.236_936;
    truck
}

/// One ABS step for this pedal on this road, from rest.
fn abs_after(grip: f64, pedal: f64) -> AbsState {
    let mut truck = rolling_truck(grip, 45.0);
    truck.brake = pedal;
    let (demand, limit) = truck.service_brake_demand_and_grip_limit();
    AbsState::Inactive.next(AbsEvent {
        braking_demand_n: demand,
        grip_limit_n: limit,
        effective_grip: truck.effective_grip(),
        speed_mph: truck.speed_mph(),
        dt: 0.02,
    })
}

#[test]
fn abs_engages_only_on_a_full_application() {
    let cases: &[(&str, f64, f64, bool)] = &[
        ("light pedal on ice", ICE, 0.25, false),
        ("half pedal on ice", ICE, 0.5, false),
        (
            "an assist's capped pedal on ice",
            ICE,
            ASSIST_BRAKE_PEDAL_CAP,
            false,
        ),
        ("full pedal on ice", ICE, 1.0, true),
        ("half pedal on snow", SNOW, 0.5, false),
        ("full pedal on snow", SNOW, 1.0, true),
        ("half pedal in rain", RAIN, 0.5, false),
        ("full pedal in rain", RAIN, 1.0, true),
        ("full pedal on dry road", 1.0, 1.0, false),
    ];
    for (label, grip, pedal, modulating) in cases {
        assert_eq!(abs_after(*grip, *pedal).is_active(), *modulating, "{label}");
    }
}

#[test]
fn the_emergency_brake_and_held_air_brakes_engage_abs_on_ice() {
    let mut truck = rolling_truck(ICE, 45.0);
    truck.emergency_brake = true;
    let (demand, limit) = truck.service_brake_demand_and_grip_limit();
    assert!(demand > limit * ABS_SLIP_RATIO);
}

#[test]
fn an_assist_pedal_is_unchanged_on_a_dry_road() {
    let truck = rolling_truck(1.0, 45.0);
    for pedal in [0.0, 0.12, 0.3, 0.6, 0.95, 1.0] {
        assert_eq!(truck.assist_brake_pedal(pedal), pedal);
    }
}

#[test]
fn an_assist_pedal_delivers_its_dry_deceleration_on_slick_roads() {
    let dry = rolling_truck(1.0, 45.0);
    for grip in [RAIN, SNOW, ICE] {
        let slick = rolling_truck(grip, 45.0);
        for pedal in [0.05, 0.1, 0.12, 0.3] {
            let compensated = slick.assist_brake_pedal(pedal);
            assert!(compensated <= ASSIST_BRAKE_PEDAL_CAP);
            if compensated < ASSIST_BRAKE_PEDAL_CAP {
                let want = dry.foundation_brake_force(BrakeApplication::Service(pedal));
                let got = slick.foundation_brake_force(BrakeApplication::Service(compensated));
                assert!(
                    (got - want).abs() < 1e-6 * want.max(1.0),
                    "grip {grip}, pedal {pedal}: {got} against {want}"
                );
            }
        }
    }
}

#[test]
fn an_assist_pedal_never_passes_the_cap() {
    for grip in [RAIN, SNOW, ICE, 0.05] {
        let truck = rolling_truck(grip, 45.0);
        for step in 0..=20 {
            let pedal = f64::from(step) / 20.0;
            assert!(truck.assist_brake_pedal(pedal) <= ASSIST_BRAKE_PEDAL_CAP);
        }
    }
}

#[test]
fn a_full_pedal_is_the_whole_of_the_roads_grip() {
    // The force model is kept: the pedal is a share of what the tires hold,
    // and a full application on ice is exactly the ice's whole grip.
    let truck = rolling_truck(ICE, 40.0);
    let s = &truck.specs;
    let expected = truck.gross_mass_kg()
        * G
        * s.max_brake_decel_g
        * truck.brake_fade_factor()
        * truck.brake_wear_factor()
        * truck.effective_grip();
    let got = truck.foundation_brake_force(BrakeApplication::Service(1.0));
    assert!((got - expected).abs() < 1e-6 * expected);
}

#[test]
fn a_full_pedal_stop_from_40_on_ice_keeps_its_distance() {
    // The ice bench anchor: about 880 feet from 40 mph with the ABS pinned
    // inactive, so the stop measures the force model alone.
    let mut truck = rolling_truck(ICE, 40.0);
    truck.throttle = 0.0;
    let dt = 0.02;
    let mut feet = 0.0;
    for _ in 0..20_000 {
        if truck.velocity_mps <= 0.05 {
            break;
        }
        truck.brake = 1.0;
        truck.abs_state = AbsState::Inactive;
        let before = truck.velocity_mps;
        truck.update(dt);
        truck.abs_state = AbsState::Inactive;
        feet += 0.5 * (before + truck.velocity_mps.max(0.0)) * dt * 3.280_84;
    }
    assert!(truck.velocity_mps <= 0.05, "the truck stopped");
    assert!((800.0..960.0).contains(&feet), "{feet:.0} feet");
}

#[test]
fn an_assist_throttle_keeps_the_drive_wheels_hooked_up_on_ice() {
    let mut truck = rolling_truck(ICE, 30.0);
    truck.transmission.gear = 6;
    truck.rpm = 1500.0;
    let cap = truck.assist_throttle_cap();
    assert!(cap < 1.0, "full throttle would spin them on ice");
    truck.throttle = cap;
    assert!(truck.drive_force_demand() < truck.drive_traction_limit());
    let held = DriveTractionState::Grip.next(DriveTractionEvent {
        drive_demand_n: truck.drive_force_demand(),
        traction_limit_n: truck.drive_traction_limit(),
        effective_grip: truck.effective_grip(),
        throttle: truck.throttle,
    });
    assert_eq!(
        held,
        DriveTractionState::Grip,
        "hooked-up wheels stay hooked up"
    );
    // Wheels the driver spun: the assist backs further off until they grip.
    truck.drive_traction_state = DriveTractionState::Spinning;
    let recover = truck.assist_throttle_cap();
    assert!(recover < cap);
    truck.throttle = recover;
    let mut state = DriveTractionState::Spinning;
    state = state.next(DriveTractionEvent {
        drive_demand_n: truck.drive_force_demand(),
        traction_limit_n: truck.drive_traction_limit(),
        effective_grip: truck.effective_grip(),
        throttle: truck.throttle,
    });
    assert_eq!(state, DriveTractionState::Grip, "spun wheels hook back up");
    assert_eq!(rolling_truck(1.0, 30.0).assist_throttle_cap(), 1.0);
}
