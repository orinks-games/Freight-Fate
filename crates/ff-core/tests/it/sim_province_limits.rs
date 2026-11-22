//! An unposted Canadian mile takes its province's statutory default
//! (`data::province_limits`), so it speaks a round number in km/h; the
//! United States keeps its road-class heuristic.

use crate::sim_support::*;
use ff_core::sim::limit_gaps::{in_canada_at, open_road_mph, unposted_limit};
use ff_core::sim::trip::{Trip, TripOptions};
use ff_core::sim::trip_models::{corridor_speed_limit, leg_state_at};
use ff_core::sim::vehicle::TruckState;
use ff_core::units::MILES_TO_KM;

/// Every half mile of every leg that has no posting in force.
fn unposted_offsets(leg: &ff_core::data::world_models::Leg) -> Vec<f64> {
    let mut out = Vec::new();
    let mut at = 0.0;
    while at <= leg.miles {
        if ff_core::sim::limit_gaps::is_unposted(leg, at) {
            out.push(at);
        }
        at += 0.5;
    }
    out
}

#[test]
fn an_unposted_canadian_mile_never_speaks_a_converted_number() {
    let mut checked = 0;
    for leg in &world().legs {
        for at in unposted_offsets(leg) {
            if !in_canada_at(leg, at) {
                continue;
            }
            let mph = unposted_limit(leg, at, open_road_mph(leg, at, ""));
            let kmh = (mph * MILES_TO_KM).round() as i64;
            assert_eq!(
                kmh % 10,
                0,
                "{}:{} at mile {at} ({}) would speak {kmh} km/h",
                leg.a,
                leg.b,
                leg_state_at(leg, at)
            );
            checked += 1;
        }
    }
    assert!(checked > 0, "no unposted Canadian mile on the map to check");
}

#[test]
fn the_lower_48_and_alaska_keep_the_road_class_heuristic() {
    for leg in &world().legs {
        for at in unposted_offsets(leg) {
            if in_canada_at(leg, at) {
                continue;
            }
            for region in ["", "great_basin", "upper_midwest"] {
                assert_eq!(
                    open_road_mph(leg, at, region),
                    corridor_speed_limit(&leg.highway, region),
                    "{}:{} at mile {at}",
                    leg.a,
                    leg.b
                );
            }
        }
    }
}

#[test]
fn a_stripped_ontario_leg_drives_at_the_statutory_80() {
    let route = first_route_option(world(), "Thunder Bay", "Nipigon");
    let stripped = with_corridor(&route.legs[0], |d| d.speed_limits.clear());
    let route = replace_leg(&route, 0, stripped);
    let trip = Trip::new(
        route,
        TruckState::default(),
        weather("upper_midwest", 1),
        TripOptions {
            seed: Some(2),
            world: Some(world()),
            ..Default::default()
        },
    );
    // Halfway along, away from both towns: the US urban cap (55 mph) would
    // have spoken 89 here.
    let mile = trip.total_miles() / 2.0;
    let kmh = trip.corridor_limit_at(mile) * MILES_TO_KM;
    assert!((kmh - 80.0).abs() < 1e-9, "spoke {kmh} km/h");
}
