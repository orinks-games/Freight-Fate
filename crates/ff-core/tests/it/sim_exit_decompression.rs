//! How much of a signaled exit's approach runs on the real clock.
//!
//! The clock goes real so the truck can shed for the ramp and the driver
//! has real seconds for the half-mile call and the exit lane. A truck
//! already under what the gore accepts has nothing to shed on the mainline,
//! so only that last stretch is real: holding the whole mile and a half
//! real crawled a truck at 35 to a 30 mph exit for two and a half minutes
//! (owner's drive into Jamestown, 2026-11-04).

use ff_core::sim::trip::{Trip, TripOptions, EXIT_FINAL_REAL_MI};
use ff_core::sim::trip_models::{EXIT_SPEED_ASSIST_START_MI, RAMP_MAX_MPH};
use ff_core::sim::vehicle::TruckState;

use crate::sim_support::{route_from_cities, weather, world};

const GORE_MI: f64 = 40.0;

fn trip_at(speed_mph: f64, ahead_mi: f64) -> Trip {
    let route = route_from_cities(world(), &["buffalo_ny_us", "jamestown_ny_us"]);
    let mut trip = Trip::new(
        route,
        TruckState::default(),
        weather("great_lakes", 1),
        TripOptions {
            world: Some(world()),
            time_scale: 20.0,
            ..TripOptions::seeded(1)
        },
    );
    trip.zones.clear();
    trip.position_mi = GORE_MI - ahead_mi;
    trip.exit_approach_mi = Some(ahead_mi);
    trip.truck.velocity_mps = speed_mph / 2.23694;
    trip
}

#[test]
fn under_the_gore_only_the_last_stretch_is_real() {
    let probe = trip_at(40.0, 1.0);
    let ramp = probe.ramp_speed_at(GORE_MI);
    let accepts = probe.gore_accepts_mph(GORE_MI);
    assert!(accepts >= RAMP_MAX_MPH, "{accepts}");
    // A little over the ramp's own number, well under what the gore takes.
    let speed = (ramp + 5.0).min(accepts - 1.0);
    assert!(speed > ramp, "ramp {ramp}, accepts {accepts}");
    let mid = (EXIT_FINAL_REAL_MI + EXIT_SPEED_ASSIST_START_MI) / 2.0;
    assert!(
        !trip_at(speed, mid).armed_exit_decompression(),
        "{speed} mph, {mid} miles out, ramp {ramp}: still real"
    );
    assert!(trip_at(speed, EXIT_FINAL_REAL_MI - 0.05).armed_exit_decompression());
}

#[test]
fn over_the_gore_the_whole_assist_window_stays_real() {
    let probe = trip_at(40.0, 1.0);
    let accepts = probe.gore_accepts_mph(GORE_MI);
    let speed = accepts + 5.0;
    assert!(trip_at(speed, EXIT_SPEED_ASSIST_START_MI - 0.05).armed_exit_decompression());
}

#[test]
fn at_the_ramp_speed_nothing_is_real() {
    let probe = trip_at(40.0, 1.0);
    let ramp = probe.ramp_speed_at(GORE_MI);
    assert!(!trip_at(ramp, 0.2).armed_exit_decompression());
}
