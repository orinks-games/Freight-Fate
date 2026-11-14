//! Live road reports: only the truck's own road, its side of it, and the
//! next few miles are spoken (Orinks, 2026-11-05, after thirteen reports
//! for other roads in ninety seconds near Gary).

use std::sync::Arc;

use crate::sim_support::*;
use ff_core::data::world_models::{CorridorDetail, Leg, Route, RoutePoint, StateMileage};
use ff_core::sim::real_traffic_parsers::{cars_title_direction, TrafficEvent};
use ff_core::sim::trip::{Trip, TripOptions};
use ff_core::sim::trip_traffic::{report_on_highway, TrafficProvider};
use ff_core::sim::vehicle::{TruckSpecs, TruckState};

/// Degrees of latitude per mile, for placing reports on the leg below.
const DEG_PER_MI: f64 = 1.0 / 69.09;

/// A due-south I-71 leg of 69 miles from 40N along 83W.
fn south_leg() -> Leg {
    Leg::new(
        "columbus_oh_us",
        "cincinnati_oh_us",
        69.09,
        "I-71",
        "flat",
        Vec::new(),
    )
    .with_detail(CorridorDetail {
        route_points: vec![
            RoutePoint {
                at_mi: 0.0,
                lat: 40.0,
                lon: -83.0,
            },
            RoutePoint {
                at_mi: 69.09,
                lat: 39.0,
                lon: -83.0,
            },
        ],
        state_miles: vec![StateMileage::new("Ohio", 69.09)],
        ..Default::default()
    })
}

struct FakeProvider {
    events: Vec<TrafficEvent>,
}

impl TrafficProvider for FakeProvider {
    fn get_events_near(&self, _: &str, _: f64, _: f64, _: f64) -> Vec<TrafficEvent> {
        self.events.clone()
    }

    fn get_construction_near_route(
        &self,
        _: &str,
        _: &[(f64, f64)],
        _: Option<&str>,
        _: f64,
    ) -> Vec<TrafficEvent> {
        Vec::new()
    }
}

fn trip_with(events: Vec<TrafficEvent>) -> Trip {
    let route = Route::from_legs(
        vec!["columbus_oh_us".to_string(), "cincinnati_oh_us".to_string()],
        vec![south_leg()],
    );
    let provider: Arc<dyn TrafficProvider> = Arc::new(FakeProvider { events });
    Trip::new(
        route,
        TruckState::new(TruckSpecs::default()),
        default_weather(),
        TripOptions {
            time_scale: 1.0,
            seed: Some(42),
            traffic_provider: Some(provider),
            world: Some(world()),
            ..Default::default()
        },
    )
}

/// An incident at `mile` down the leg, `east_mi` off the road.
fn report(id: &str, road: &str, mile: f64, east_mi: f64, description: &str) -> TrafficEvent {
    let lat = 40.0 - mile * DEG_PER_MI;
    TrafficEvent {
        id: id.to_string(),
        event_type: "incident".to_string(),
        severity: "high".to_string(),
        description: description.to_string(),
        latitude: Some(lat),
        longitude: Some(-83.0 + east_mi * DEG_PER_MI / lat.to_radians().cos()),
        road_name: road.to_string(),
        ..Default::default()
    }
}

fn reports_at(trip: &mut Trip, mile: f64) -> Vec<String> {
    trip.position_mi = mile;
    trip.next_real_traffic_check_mi = 0.0;
    let before = trip.events.len();
    trip.check_real_traffic_events();
    trip.events[before..]
        .iter()
        .map(|e| e.text().to_string())
        .filter(|t| t.starts_with("Live road report"))
        .collect()
}

#[test]
fn a_report_on_the_trucks_road_ahead_is_spoken_with_its_distance() {
    let mut trip = trip_with(vec![report("a", "I-71", 3.0, 0.0, "Crash.")]);
    assert_eq!(
        reports_at(&mut trip, 0.0),
        vec!["Live road report, 3 miles ahead: Crash."]
    );
    // Once only.
    assert!(reports_at(&mut trip, 1.0).is_empty());
}

#[test]
fn a_report_on_a_crossing_road_stays_quiet() {
    // A ramp closure on US 12 where it crosses the truck's road.
    let mut trip = trip_with(vec![report("a", "US 12", 3.0, 0.0, "Ramp closed.")]);
    assert!(reports_at(&mut trip, 0.0).is_empty());
}

#[test]
fn a_report_for_the_other_direction_stays_quiet() {
    let mut north = report("a", "I-71", 3.0, 0.0, "Crash.");
    north.direction = "North".to_string();
    let mut south = report("b", "I-71", 4.0, 0.0, "Stalled truck.");
    south.direction = "South".to_string();
    let mut trip = trip_with(vec![north, south]);
    assert_eq!(
        reports_at(&mut trip, 0.0),
        vec!["Live road report, 4 miles ahead: Stalled truck."]
    );
}

#[test]
fn a_report_with_no_road_name_must_sit_on_the_road() {
    let off = report("a", "", 3.0, 0.6, "Crash.");
    let on = report("b", "", 5.0, 0.05, "Debris.");
    let mut trip = trip_with(vec![off, on]);
    assert_eq!(
        reports_at(&mut trip, 0.0),
        vec!["Live road report, 5 miles ahead: Debris."]
    );
}

#[test]
fn a_far_report_waits_until_it_is_close_and_one_behind_is_dropped() {
    let mut trip = trip_with(vec![
        report("far", "I-71", 30.0, 0.0, "Crash."),
        report("behind", "I-71", 15.0, 0.0, "Disabled vehicle."),
    ]);
    assert!(reports_at(&mut trip, 0.0)
        .iter()
        .all(|r| !r.contains("Crash")));
    assert_eq!(
        reports_at(&mut trip, 22.0),
        vec!["Live road report, 8 miles ahead: Crash."]
    );
}

#[test]
fn a_cars_title_on_the_trucks_road_drops_its_road_and_mile_points() {
    let mut trip = trip_with(vec![report(
        "a",
        "I-71",
        3.0,
        0.0,
        "I-71 southbound (Mile Point 3 - 4.2): Right lane closed.",
    )]);
    assert_eq!(
        reports_at(&mut trip, 0.0),
        vec!["Live road report, 3 miles ahead: Right lane closed."]
    );
}

#[test]
fn feed_road_names_match_the_maps_highway() {
    assert_eq!(report_on_highway("I-80/I-94", "I-94"), Some(true));
    assert_eq!(report_on_highway("IN 912", "SR-912"), Some(true));
    assert_eq!(report_on_highway("I-90 - NYS Thruway", "I-90"), Some(true));
    assert_eq!(report_on_highway("US 12", "I-94"), Some(false));
    assert_eq!(report_on_highway("US 12", "SR-12"), Some(false));
    assert_eq!(report_on_highway("I-80 Toll", "I-80"), Some(true));
    assert_eq!(report_on_highway("", "I-94"), None);
}

#[test]
fn a_cars_title_gives_the_direction_it_holds() {
    assert_eq!(
        cars_title_direction("I-80 eastbound: Exit ramp closed."),
        "East"
    );
    assert_eq!(
        cars_title_direction("I-80 westbound (Mile Point 15): Entrance ramp closed."),
        "West"
    );
    assert_eq!(
        cars_title_direction("IN 51 in both directions (Mile Point 3.24 - 3.53): Building fire."),
        ""
    );
    assert_eq!(
        cars_title_direction("US 231 (Mile Point 297.29 - 291.74): Road closed."),
        ""
    );
}
