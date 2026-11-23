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
//! The South Central roads (2026-11-23) split Little Rock to Oklahoma City
//! at Henryetta and Dodge City to Salina at Great Bend; the Dodge City
//! chain is 15 miles shorter than the leg it replaced.

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
fn a_save_on_the_old_little_rock_to_oklahoma_city_leg_resumes_on_its_pieces() {
    // 200 miles in lands on I-40, short of Henryetta.
    let app = resume("Little Rock", "Oklahoma City", 339.0, 200.0);
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
            "little_rock_ar_us",
            "henryetta_ok_us",
            "oklahoma_city_ok_us"
        ]
    );
    assert_eq!(drive.trip.position_mi, 200.0);
}

#[test]
fn a_save_past_the_end_of_the_shorter_dodge_city_chain_comes_back_at_salina() {
    // The old Dodge City to Salina leg went north to I-70 (182 miles); the
    // chain over Great Bend is 167. A save at old mile 175 lies past the
    // chain's end and comes back at its last mile, at Salina.
    let app = resume("Dodge City", "Salina", 182.0, 175.0);
    let state = app.state().expect("a state");
    let state = state.borrow();
    let drive = state
        .as_any()
        .downcast_ref::<DrivingState>()
        .expect("the save resumes at the wheel");
    assert!(drive.resumed);
    assert_eq!(
        drive.route.cities,
        ["dodge_city_ks_us", "great_bend_ks_us", "salina_ks_us"]
    );
    assert_eq!(drive.trip.position_mi, drive.route.miles());
}
