//! Where a delivery leaves the road. A signed exit counts only inside the last
//! eight miles of the trip; past that the road after the exit is still the
//! highway, and the trip runs on to town. Casa Grande to Globe once ended at
//! US-60 exit 227 at Superior, 24 miles short of Globe (missing roads pass,
//! 2026-11-22).

use std::sync::Arc;

use ff_core::data::world_models::Route;
use ff_core::sim::trip::destination_exit::{destination_arrival, DestinationArrival};
use ff_core::sim::trip::{Trip, TripOptions};
use ff_core::sim::trip_models::{DESTINATION_EXIT_SCAN_WINDOW_MI, DESTINATION_LOCAL_APPROACH_MI};
use ff_core::sim::vehicle::TruckState;

use crate::sim_support::{weather, world};

/// A trip over the one stored leg between `from` and `to`, driven from `from`.
fn single_leg_trip(from: &str, to: &str) -> Trip {
    let leg = world()
        .legs
        .iter()
        .find(|leg| (leg.a == from && leg.b == to) || (leg.a == to && leg.b == from))
        .unwrap_or_else(|| panic!("a leg between {from} and {to}"));
    trip_over(from, to, Arc::clone(leg))
}

fn trip_over(from: &str, to: &str, leg: Arc<ff_core::data::world_models::Leg>) -> Trip {
    let route = Route::new(vec![from.to_string(), to.to_string()], vec![leg]);
    Trip::new(
        route,
        TruckState::default(),
        weather("heartland", 7),
        TripOptions {
            seed: Some(7),
            world: Some(world()),
            ..Default::default()
        },
    )
}

/// The trips that ended furthest short at a freeway exit before the rule, and
/// one that keeps its exit.
#[test]
fn each_listed_trip_leaves_the_road_where_the_table_says() {
    // (from, to, the exit it keeps, or None to end in town)
    let table: &[(&str, &str, Option<&str>)] = &[
        ("casa_grande_az_us", "globe_az_us", None),
        ("salinas_ca_us", "merced_ca_us", None),
        ("barstow_ca_us", "bullhead_city_az_us", None),
        ("blythe_ca_us", "bullhead_city_az_us", None),
        ("yuma_az_us", "buckeye_az_us", None),
        ("chico_ca_us", "willits_ca_us", None),
        ("alexandria_la_us", "natchitoches_la_us", None),
        ("lumberton_nc_us", "florence_sc_us", Some("exit 164")),
    ];
    for &(from, to, keeps) in table {
        let trip = single_leg_trip(from, to);
        let total = trip.total_miles();
        let arrival = destination_arrival(world(), &trip, true);
        match (keeps, &arrival) {
            (Some(expected), DestinationArrival::Exit { at_mi, label, .. }) => {
                assert_eq!(label, expected, "{from} to {to}");
                assert!(
                    total - at_mi <= DESTINATION_EXIT_SCAN_WINDOW_MI,
                    "{from} to {to}"
                );
            }
            (None, DestinationArrival::InTown { at_mi }) => {
                assert!(
                    (at_mi - (total - DESTINATION_LOCAL_APPROACH_MI)).abs() < 1e-9,
                    "{from} to {to}: in town at {at_mi} of {total}"
                );
            }
            _ => panic!("{from} to {to}: expected {keeps:?}, got {arrival:?} of {total} mi"),
        }
    }
}

/// Every stored leg, both ways: the delivery leaves the road at a signed exit
/// in the last eight miles, or one mile from the end in town. Never further
/// out.
#[test]
fn no_single_leg_trip_ends_more_than_eight_miles_short() {
    let mut exits = 0;
    let mut in_town = 0;
    for leg in &world().legs {
        if leg.a == leg.b || leg.interchanges().is_empty() {
            continue;
        }
        for (from, to) in [(&leg.a, &leg.b), (&leg.b, &leg.a)] {
            let trip = trip_over(from, to, Arc::clone(leg));
            let short = trip.total_miles() - destination_arrival(world(), &trip, true).at_mi();
            match destination_arrival(world(), &trip, true) {
                DestinationArrival::Exit { label, .. } => {
                    exits += 1;
                    assert!(
                        short <= DESTINATION_EXIT_SCAN_WINDOW_MI,
                        "{from} to {to}: {label} is {short:.1} mi short"
                    );
                }
                DestinationArrival::InTown { .. } => {
                    in_town += 1;
                    assert!(
                        (short - DESTINATION_LOCAL_APPROACH_MI).abs() < 1e-9
                            || trip.total_miles() < DESTINATION_LOCAL_APPROACH_MI,
                        "{from} to {to}: in town {short:.1} mi short"
                    );
                }
            }
        }
    }
    assert!(
        exits > 1000 && in_town > 100,
        "{exits} exits, {in_town} in town"
    );
}

/// Once the truck is past the named exit, the plan is the next exit inside
/// the window, or town.
#[test]
fn a_passed_exit_hands_the_arrival_on() {
    let mut trip = single_leg_trip("lumberton_nc_us", "florence_sc_us");
    let DestinationArrival::Exit { at_mi, .. } = destination_arrival(world(), &trip, false) else {
        panic!("Lumberton to Florence ends at a signed exit");
    };
    trip.position_mi = at_mi + 0.1;
    let next = destination_arrival(world(), &trip, false);
    assert!(next.at_mi() > trip.position_mi, "{next:?}");
    assert!(
        trip.total_miles() - next.at_mi() <= DESTINATION_EXIT_SCAN_WINDOW_MI,
        "{next:?}"
    );
    // As planned, the passed exit still answers.
    assert_eq!(destination_arrival(world(), &trip, true).at_mi(), at_mi);
}
