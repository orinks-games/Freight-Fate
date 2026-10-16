//! Traffic in the open lane of a lane closure lets the truck in.
//!
//! Player report, 2026-10-16: with the right lane coned off, a stream of cars
//! in the left lane held it for as long as they kept coming, so the truck had
//! to stop dead in the closed lane; and an L key reading of "Left lane open"
//! taken from a stop went stale while the truck got up to lane-change speed,
//! so the merge it invited sideswiped a car that had come up from behind.

use ff_core::data::world_models::{CorridorDetail, LaneSegment, Leg, Route};
use ff_core::sim::traffic_manager::{
    TrafficVehicle, DODGE_CLEARANCE_AHEAD_MI, DODGE_CLEARANCE_BEHIND_MI,
};
use ff_core::sim::trip::{Trip, TripOptions};
use ff_core::sim::trip_models::{OpenSide, Zone};
use ff_core::sim::vehicle::TruckState;
use ff_core::sim::weather::WeatherSystem;

const MPH_PER_MPS: f64 = 2.2369362920544;

/// A straight two-lane one-way road, empty but for the vehicles a case
/// places, with the truck stopped at mile 100 in the right lane.
fn two_lane_road() -> Trip {
    let leg = Leg::new("a", "b", 300.0, "I-1", "flat", Vec::new()).with_detail(CorridorDetail {
        lane_segments: vec![LaneSegment {
            start_mi: 0.0,
            end_mi: 300.0,
            lanes: 2,
            oneway: true,
            ..Default::default()
        }],
        ..Default::default()
    });
    let route = Route::new(vec!["a".to_string(), "b".to_string()], vec![leg.into()]);
    let mut trip = Trip::new(
        route,
        TruckState::default(),
        WeatherSystem::new("heartland", Some(3), None, None, true),
        TripOptions {
            seed: Some(3),
            time_scale: 1.0,
            hazard_scale: 0.0,
            ..Default::default()
        },
    );
    trip.position_mi = 100.0;
    trip.zones.clear();
    trip.traffic_manager.rolling_bubble = false;
    trip.traffic_manager.player_lane = 0;
    trip
}

fn right_lane_closed(trip: &mut Trip) {
    trip.zones = vec![Zone::new(90.0, 120.0, 45.0, "construction").with_closed_side(Some("right"))];
}

/// A car every 0.3 miles in the left lane, the nearest `behind_mi` behind
/// the truck, all doing 50.
fn left_lane_stream(trip: &mut Trip, behind_mi: f64, count: usize) {
    let cars = (0..count)
        .map(|i| {
            let at = trip.position_mi - behind_mi - 0.3 * i as f64;
            TrafficVehicle::new(&format!("car:{i}"), at, 50.0, 50.0, -1, "passing", "car")
                .with_lane(1)
        })
        .collect();
    trip.set_npc_vehicles(cars);
}

/// What the arrival of a lane change into `lane` finds: the sideswipe test.
fn sideswipe_in(trip: &Trip, lane: i64) -> Option<String> {
    trip.traffic_manager
        .vehicle_in_lane(
            trip.position_mi,
            lane,
            DODGE_CLEARANCE_AHEAD_MI,
            DODGE_CLEARANCE_BEHIND_MI,
            0.0,
            0.0,
        )
        .map(|v| v.key.clone())
}

/// Stopped in the closed lane, the left lane stays open: the cars coming up
/// it wait behind the truck instead of streaming past it for good.
#[test]
fn traffic_in_the_open_lane_waits_for_a_truck_stopped_in_the_closed_one() {
    let mut trip = two_lane_road();
    right_lane_closed(&mut trip);
    left_lane_stream(&mut trip, 1.0, 8);
    assert_eq!(trip.closed_lane_at(None, None), Some(0));
    for frame in 0..1200 {
        trip.update(0.1);
        assert_eq!(
            trip.open_side_at(None),
            OpenSide::Left,
            "left lane held after {:.1} s",
            frame as f64 * 0.1
        );
    }
    assert!(trip
        .traffic_manager
        .vehicles
        .iter()
        .all(|v| v.position_mi < trip.position_mi));
}

/// "Left lane open" from a stop stays true through the seconds it takes to
/// get rolling and across: no car comes up into the gap meanwhile.
#[test]
fn an_open_lane_called_from_a_stop_is_still_open_on_arrival() {
    let mut trip = two_lane_road();
    right_lane_closed(&mut trip);
    left_lane_stream(&mut trip, 0.65, 8);
    trip.update(0.1);
    assert_eq!(trip.open_side_at(None), OpenSide::Left);
    // Twenty-five seconds getting up to fifteen, then the change itself.
    trip.truck.start_engine();
    for frame in 0..275 {
        let mph = (frame as f64 * 0.6 / 10.0).min(15.0);
        trip.truck.velocity_mps = mph / MPH_PER_MPS;
        trip.update(0.1);
    }
    assert_eq!(sideswipe_in(&trip, 1), None);
}

/// Once the truck is across, the closure leaves one lane, and traffic
/// behind it queues there the way it does on a one-lane road.
#[test]
fn past_the_merge_the_one_open_lane_queues_behind_the_truck() {
    let mut trip = two_lane_road();
    right_lane_closed(&mut trip);
    left_lane_stream(&mut trip, 1.0, 4);
    trip.traffic_manager.player_lane = 1;
    trip.truck.start_engine();
    trip.truck.velocity_mps = 30.0 / MPH_PER_MPS;
    for _ in 0..3000 {
        trip.update(0.1);
        let truck = trip.position_mi;
        assert!(trip
            .traffic_manager
            .vehicles
            .iter()
            .all(|v| v.position_mi < truck));
    }
}

/// With no closure, the left lane is a passing lane and stays one.
#[test]
fn without_a_closure_the_left_lane_still_passes() {
    let mut trip = two_lane_road();
    left_lane_stream(&mut trip, 1.0, 1);
    for _ in 0..1200 {
        trip.update(0.1);
    }
    assert!(trip.traffic_manager.vehicles[0].position_mi > trip.position_mi);
}
