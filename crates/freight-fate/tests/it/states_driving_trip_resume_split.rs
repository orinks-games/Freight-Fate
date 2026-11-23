//! A mid-trip save on a leg the Great Lakes roads (2026-11-22) split at a
//! city it passes resumes on the pieces, at the same mile.
//!
//! Chicago -> St. Louis was one 298-mile I-55 leg; it is now Chicago,
//! Joliet, Bloomington, Springfield and St. Louis, about 302 miles of the
//! same road. The Northeast roads (2026-11-23) did the same to Buffalo ->
//! New York, a 468-mile Thruway leg that is now Buffalo, Newburgh and New
//! York, about 465 miles. The Upper Midwest roads (2026-11-23) split Des
//! Moines -> Chicago, a 334-mile I-80 and I-88 leg, at Aurora.

use ff_core::models::profile::Profile;
use freight_fate::app::testing::TestApp;
use freight_fate::states::driving::DrivingState;
use freight_fate::states::main_menu::enter_world;
use serde_json::json;

#[test]
fn a_save_on_the_old_chicago_to_st_louis_leg_resumes_on_its_pieces() {
    let mut app = TestApp::new();
    let mut p = Profile::named("Split Save");
    p.active_trip = Some(json!({
        "job": {
            "cargo": "general",
            "weight_tons": 14.0,
            "origin": "Chicago",
            "origin_location": "Chicago freight market",
            "destination": "St. Louis",
            "distance_mi": 298.0,
            "pay": 1200.0,
            "deadline_game_h": 12.0,
            "market_mult": 1.0,
        },
        "route_cities": ["Chicago", "St. Louis"],
        "trip_seed": 1234,
        "position_mi": 150.0,
        "game_minutes": 180.0,
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
fn a_save_on_the_old_buffalo_to_new_york_leg_resumes_on_its_pieces() {
    let mut app = TestApp::new();
    let mut p = Profile::named("Thruway Save");
    p.active_trip = Some(json!({
        "job": {
            "cargo": "general",
            "weight_tons": 14.0,
            "origin": "Buffalo",
            "origin_location": "Buffalo freight market",
            "destination": "New York",
            "distance_mi": 468.0,
            "pay": 1800.0,
            "deadline_game_h": 16.0,
            "market_mult": 1.0,
        },
        "route_cities": ["Buffalo", "New York"],
        "trip_seed": 4321,
        "position_mi": 300.0,
        "game_minutes": 300.0,
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
