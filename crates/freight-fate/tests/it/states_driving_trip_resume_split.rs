//! A mid-trip save on a leg the map has since split at a city on its road
//! resumes on the chain that replaced it.
//!
//! The missing roads pass (2026-11-22) split Los Angeles to San Diego at
//! Santa Ana and Oceanside, and Sacramento to San Francisco at Fairfield.
//! The Great Lakes roads (2026-11-22) split Chicago to St. Louis, one
//! 298-mile I-55 leg, at Joliet, Bloomington and Springfield. A save names
//! the old two cities; the resume path swaps the hop for its chain and keeps
//! the mile count.
//!
//! The Florida and Georgia roads (2026-11-22) split West Palm Beach to Palm
//! Bay (110 miles), which passed Port Saint Lucie 1.2 miles off: 51 miles on
//! Florida's Turnpike, then the existing 65-mile leg on I-95.
//!
//! The Northeast roads (2026-11-23) split Buffalo to New York, a 468-mile
//! Thruway leg, at Newburgh: about 465 miles of the same road.
//!
//! The Upper Midwest roads (2026-11-23) split Des Moines to Chicago, a
//! 334-mile I-80 and I-88 leg, at Aurora.
//!
//! The Alabama, Mississippi and South Carolina roads (2026-11-23) split
//! Jackson to Baton Rouge (175 miles of I-55 and I-12) at Hammond: 133 miles
//! on I-55, then the existing 50-mile leg on I-12.

use ff_core::models::profile::Profile;
use freight_fate::app::testing::TestApp;
use freight_fate::states::driving::DrivingState;
use freight_fate::states::main_menu::enter_world;
use serde_json::json;

fn resume(origin: &str, destination: &str, miles: f64, position_mi: f64) -> TestApp {
    let mut app = TestApp::new();
    let mut p = Profile::named("Split Save");
    p.active_trip = Some(json!({
        "job": {
            "cargo": "general",
            "weight_tons": 14.0,
            "origin": origin,
            "origin_location": format!("{origin} freight market"),
            "destination": destination,
            "distance_mi": miles,
            "pay": 900.0,
            "deadline_game_h": 8.0,
            "market_mult": 1.0,
        },
        "route_cities": [origin, destination],
        "trip_seed": 4321,
        "position_mi": position_mi,
        "game_minutes": 90.0,
        "start_damage": 0.0,
    }));
    app.ctx.profile = Some(p);
    enter_world(&mut app.ctx, false);
    app
}

#[test]
fn a_save_on_the_old_i5_leg_resumes_on_the_santa_ana_and_oceanside_chain() {
    let app = resume("Los Angeles", "San Diego", 121.0, 70.0);
    let state = app.state().expect("a state");
    let state = state.borrow();
    let drive = state
        .as_any()
        .downcast_ref::<DrivingState>()
        .expect("the save resumes at the wheel");
    assert!(drive.resumed);
    assert_eq!(
        drive.route.cities,
        [
            "los_angeles_ca_us",
            "santa_ana_ca_us",
            "oceanside_ca_us",
            "san_diego_ca_us"
        ]
    );
    assert_eq!(drive.trip.position_mi, 70.0);
}

#[test]
fn a_save_past_the_bay_bridge_resumes_on_the_fairfield_chain_without_paying_it_again() {
    let app = resume("Sacramento", "San Francisco", 88.0, 86.0);
    let state = app.state().expect("a state");
    let state = state.borrow();
    let drive = state
        .as_any()
        .downcast_ref::<DrivingState>()
        .expect("the save resumes at the wheel");
    assert!(drive.resumed);
    assert_eq!(
        drive.route.cities,
        ["sacramento_ca_us", "fairfield_ca_us", "san_francisco_ca_us"]
    );
    assert_eq!(drive.trip.position_mi, 86.0);
    assert!(
        drive
            .trip
            .charged_tolls
            .iter()
            .any(|key| key.contains("Bay Bridge")),
        "the Bay Bridge booth sits before mile 86 on the chain: {:?}",
        drive.trip.charged_tolls
    );
}

#[test]
fn a_save_on_the_old_chicago_to_st_louis_leg_resumes_on_its_pieces() {
    let app = resume("Chicago", "St. Louis", 298.0, 150.0);
    let state = app.state().expect("a state");
    let state = state.borrow();
    let drive = state
        .as_any()
        .downcast_ref::<DrivingState>()
        .expect("the save resumes at the wheel");
    assert!(drive.resumed);
    assert_eq!(
        drive.route.cities,
        [
            "chicago_il_us",
            "joliet_il_us",
            "bloomington_il_us",
            "springfield_il_us",
            "st_louis_mo_us",
        ]
    );
    assert_eq!(drive.trip.position_mi, 150.0);
}

#[test]
fn a_save_on_the_old_west_palm_beach_leg_resumes_on_the_port_saint_lucie_chain() {
    // 80 miles in lands 29 miles past Port Saint Lucie.
    let app = resume("West Palm Beach", "Palm Bay", 110.0, 80.0);
    let state = app.state().expect("a state");
    let state = state.borrow();
    let drive = state
        .as_any()
        .downcast_ref::<DrivingState>()
        .expect("the save resumes at the wheel");
    assert!(drive.resumed);
    assert_eq!(
        drive.route.cities,
        [
            "west_palm_beach_fl_us",
            "port_saint_lucie_fl_us",
            "palm_bay_fl_us"
        ]
    );
    assert_eq!(drive.trip.position_mi, 80.0);
}

#[test]
fn a_save_on_the_old_buffalo_to_new_york_leg_resumes_on_its_pieces() {
    // 300 miles in lands on the Thruway, short of Newburgh.
    let app = resume("Buffalo", "New York", 468.0, 300.0);
    let state = app.state().expect("a state");
    let state = state.borrow();
    let drive = state
        .as_any()
        .downcast_ref::<DrivingState>()
        .expect("the save resumes at the wheel");
    assert!(drive.resumed);
    assert_eq!(
        drive.route.cities,
        ["buffalo_ny_us", "newburgh_ny_us", "new_york_ny_us"]
    );
    assert_eq!(drive.trip.position_mi, 300.0);
}

#[test]
fn a_save_on_the_old_des_moines_to_chicago_leg_resumes_on_its_pieces() {
    let mut app = TestApp::new();
    let mut p = Profile::named("Interstate Save");
    p.active_trip = Some(json!({
        "job": {
            "cargo": "general",
            "weight_tons": 14.0,
            "origin": "Des Moines",
            "origin_location": "Des Moines freight market",
            "destination": "Chicago",
            "distance_mi": 334.0,
            "pay": 1300.0,
            "deadline_game_h": 12.0,
            "market_mult": 1.0,
        },
        "route_cities": ["Des Moines", "Chicago"],
        "trip_seed": 2468,
        "position_mi": 200.0,
        "game_minutes": 240.0,
        "start_damage": 0.0,
    }));
    app.ctx.profile = Some(p);

    enter_world(&mut app.ctx, false);

    let state = app.state().expect("a state");
    let state = state.borrow();
    let drive = state
        .as_any()
        .downcast_ref::<DrivingState>()
        .expect("the save resumes at the wheel");
    assert!(drive.resumed);
    assert_eq!(
        drive.route.cities,
        ["des_moines_ia_us", "aurora_il_us", "chicago_il_us"]
    );
    assert_eq!(drive.trip.position_mi, 200.0);
}

#[test]
fn a_save_on_the_old_jackson_to_baton_rouge_leg_resumes_on_the_hammond_chain() {
    // 90 miles in is on I-55, short of Hammond.
    let app = resume("jackson_ms_us", "baton_rouge_la_us", 175.0, 90.0);
    let state = app.state().expect("a state");
    let state = state.borrow();
    let drive = state
        .as_any()
        .downcast_ref::<DrivingState>()
        .expect("the save resumes at the wheel");
    assert!(drive.resumed);
    assert_eq!(
        drive.route.cities,
        ["jackson_ms_us", "hammond_la_us", "baton_rouge_la_us"]
    );
    assert_eq!(drive.trip.position_mi, 90.0);
}
