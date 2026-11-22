//! A mid-trip save on a leg the map has since split at a city resumes on
//! the halves.
//!
//! West Palm Beach -> Palm Bay (110 miles) passed Port Saint Lucie 1.2
//! miles off until the Florida and Georgia roads (2026-11-22) split it
//! there: 51 miles on Florida's Turnpike, then the existing 65-mile leg on
//! I-95. A save 80 miles in lands 29 miles past Port Saint Lucie.

use ff_core::models::profile::Profile;
use freight_fate::app::testing::TestApp;
use freight_fate::states::driving::DrivingState;
use freight_fate::states::main_menu::enter_world;
use serde_json::json;

#[test]
fn a_save_on_the_retired_leg_resumes_on_its_halves() {
    let mut app = TestApp::new();
    let mut p = Profile::named("Split Save");
    p.active_trip = Some(json!({
        "job": {
            "cargo": "general",
            "weight_tons": 14.0,
            "origin": "West Palm Beach",
            "origin_location": "West Palm Beach freight market",
            "destination": "Palm Bay",
            "distance_mi": 110.0,
            "pay": 700.0,
            "deadline_game_h": 8.0,
            "market_mult": 1.0,
        },
        "route_cities": ["West Palm Beach", "Palm Bay"],
        "trip_seed": 1234,
        "position_mi": 80.0,
        "game_minutes": 90.0,
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
            "west_palm_beach_fl_us",
            "port_saint_lucie_fl_us",
            "palm_bay_fl_us"
        ]
    );
    assert_eq!(drive.trip.position_mi, 80.0);
}
