//! The road tally a delivery reports: what the drive met, noted where the
//! drive already knows it, and kept through a saved and resumed trip.

use ff_core::sim::roadside_inspection::InspectionLevel;
use ff_core::sim::trip_models::{RoadStop, Zone};
use ff_core::sim::weather::WeatherKind;
use freight_fate::states::driving::DrivingState;
use serde_json::json;

use crate::badge_moments_support::*;

const CHICAGO: &str = "chicago_il_us";
const CLEVELAND: &str = "cleveland_oh_us";

#[test]
fn a_drive_tallies_its_weather_work_zones_scales_and_inspections() {
    let mut app = career_in(CHICAGO);
    let mut d = run(&mut app, CHICAGO, CLEVELAND);
    d.trip.position_mi = 10.0;
    d.trip.weather.current = WeatherKind::HeavyRain;
    d.trip.truck.velocity_mps = 25.0;
    d.trip
        .zones
        .push(Zone::new(10.5, 12.0, 45.0, "construction"));
    d.trip
        .zones
        .push(Zone::new(10.6, 11.0, 50.0, "heavy traffic"));
    d.trip.position_mi = 11.0;
    d.note_road_tally(10.0);
    // The same zone a frame later is the same zone.
    d.trip.position_mi = 11.5;
    d.note_road_tally(11.0);
    let scale = RoadStop::new("Test Scale", 30.0, "weigh_station");
    d.note_road_tally_scale(&scale);
    d.note_road_tally_scale(&scale);
    let report = d.inspection_report(&app.ctx, InspectionLevel::DriverOnly);
    d.settle_inspection(&mut app.ctx, &report);
    let inspections =
        freight_fate::states::driving_updates::road_tally::career_inspections(&app.ctx);
    assert_eq!(
        d.road_tally.conditions(inspections),
        Some(json!({"weather": ["heavy_rain"], "workZones": 1, "scales": 1, "inspections": 1}))
    );
}

#[test]
fn a_parked_truck_tallies_no_weather() {
    let mut app = career_in(CHICAGO);
    let mut d = run(&mut app, CHICAGO, CLEVELAND);
    d.trip.truck.velocity_mps = 0.0;
    d.trip.weather.current = WeatherKind::Snow;
    d.note_road_tally(d.trip.position_mi);
    assert_eq!(d.road_tally.conditions(0), None);
}

#[test]
fn the_tally_survives_a_saved_and_resumed_drive() {
    let mut app = career_in(CHICAGO);
    let mut d = run(&mut app, CHICAGO, CLEVELAND);
    d.road_tally.note_weather("fog");
    d.road_tally.note_chain_law();
    let snapshot = d.snapshot(&app.ctx);
    let resumed = DrivingState::from_snapshot(&mut app.ctx, &snapshot).expect("the drive resumes");
    assert_eq!(resumed.road_tally, d.road_tally);
}
