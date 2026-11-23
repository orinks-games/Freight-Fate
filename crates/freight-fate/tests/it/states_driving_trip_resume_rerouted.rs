//! A mid-trip save on a leg the map has since put back on its real road
//! resumes where it was.
//!
//! Newark -> Trenton was a straight line across the New Jersey Turnpike
//! until the South Jersey roads (2026-11-22) re-routed it, with the same 58
//! miles and the Turnpike ticket now charged at the 7A booth (mile 47.1).

use ff_core::models::profile::Profile;
use freight_fate::app::testing::TestApp;
use freight_fate::states::driving::DrivingState;
use freight_fate::states::main_menu::enter_world;
use serde_json::json;

#[test]
fn a_turnpike_save_resumes_past_the_booth_without_paying_it_again() {
    let mut app = TestApp::new();
    let mut p = Profile::named("Turnpike Save");
    p.active_trip = Some(json!({
        "job": {
            "cargo": "general",
            "weight_tons": 14.0,
            "origin": "Newark",
            "origin_location": "Newark freight market",
            "destination": "Trenton",
            "distance_mi": 58.0,
            "pay": 400.0,
            "deadline_game_h": 6.0,
            "market_mult": 1.0,
        },
        "route_cities": ["Newark", "Trenton"],
        "trip_seed": 1234,
        "position_mi": 50.0,
        "game_minutes": 60.0,
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
    assert_eq!(drive.route.cities, ["newark_nj_us", "trenton_nj_us"]);
    assert_eq!(drive.trip.position_mi, 50.0);
    assert!(
        drive
            .trip
            .charged_tolls
            .iter()
            .any(|key| key.contains("New Jersey Turnpike")),
        "the booth at mile 47.1 counts as paid: {:?}",
        drive.trip.charged_tolls
    );
}
