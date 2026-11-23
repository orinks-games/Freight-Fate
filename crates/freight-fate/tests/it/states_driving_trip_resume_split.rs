//! A mid-trip save on a leg the map has since split at a city on its road
//! resumes on the chain that replaced it.
//!
//! The missing roads pass (2026-11-22) split Los Angeles to San Diego at
//! Santa Ana and Oceanside, and Sacramento to San Francisco at Fairfield.
//! A save names the old two cities; the resume path swaps the hop for its
//! chain and keeps the mile count. Texas and Louisiana (2026-11-23) retired
//! Dallas to St. Louis and Wichita Falls to McKinney the same way.

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

/// Texas and Louisiana (2026-11-23): Dallas to St. Louis retired for a chain
/// over Denton, Ardmore and the I-44 legs. A save past Tulsa resumes there.
#[test]
fn a_save_on_the_old_dallas_to_st_louis_leg_resumes_on_the_i44_chain() {
    let app = resume("Dallas", "St. Louis", 713.0, 360.0);
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
            "dallas_tx_us",
            "denton_tx_us",
            "ardmore_ok_us",
            "oklahoma_city_ok_us",
            "tulsa_ok_us",
            "joplin_mo_us",
            "springfield_mo_us",
            "rolla_mo_us",
            "st_louis_mo_us"
        ]
    );
    assert_eq!(drive.trip.position_mi, 360.0);
}

/// Wichita Falls to McKinney retired for a chain through Denton.
#[test]
fn a_save_on_the_old_wichita_falls_to_mckinney_leg_resumes_through_denton() {
    let app = resume("Wichita Falls", "McKinney", 133.0, 110.0);
    let state = app.state().expect("a state");
    let state = state.borrow();
    let drive = state
        .as_any()
        .downcast_ref::<DrivingState>()
        .expect("the save resumes at the wheel");
    assert!(drive.resumed);
    assert_eq!(
        drive.route.cities,
        ["wichita_falls_tx_us", "denton_tx_us", "mckinney_tx_us"]
    );
    assert_eq!(drive.trip.position_mi, 110.0);
}
