//! Snow chains bought at a truck stop in a chain-control state, for a driver
//! whose set snapped or never came aboard.

use crate::states_driving_menus_support::*;
use ff_core::models::business::{COMPANY_DRIVER, LEASED_OWNER_OPERATOR};
use ff_core::models::profile::CHAIN_REPAIR_LINKS_PER_SET;
use ff_core::sim::trip_models::RoadStop;
use freight_fate::app::testing::TestApp;
use freight_fate::app::SharedState;
use freight_fate::states::driving_menu_states::DriveRef;
use freight_fate::states::driving_rest_states::RestStopState;

fn chainless_drive(app: &mut TestApp, from: &str, to: &str, business_status: &str) -> SharedState {
    let drive = a_drive_between(app, from, to, "Chain Buyer");
    let p = app.ctx.profile.as_mut().expect("a career");
    p.business_status = business_status.to_string();
    if business_status != COMPANY_DRIVER {
        p.owned_trucks = vec!["rig".to_string()];
    }
    p.set_money(1500.0);
    p.set_chains_owned(false);
    drive
}

fn truck_stop(drive: &SharedState) -> RestStopState {
    let at_mi = with_drive(drive, |d| d.trip.position_mi);
    let mut stop = RoadStop::new("Pilot Travel Center", at_mi, "travel_center");
    stop.actions = ["park", "fuel", "food", "break"]
        .iter()
        .map(|a| a.to_string())
        .collect();
    RestStopState::with_drive(DriveRef::of(drive), stop, false)
}

fn money(app: &TestApp) -> f64 {
    app.ctx.profile.as_ref().expect("a career").money()
}

#[test]
fn test_a_truck_stop_in_a_chain_state_sells_a_set() {
    let mut app = TestApp::new();
    let drive = chainless_drive(&mut app, "Denver", "Salt Lake City", LEASED_OWNER_OPERATOR);
    let mut state = truck_stop(&drive);
    let minutes = with_drive(&drive, |d| d.trip.game_minutes);
    app.clear_speech();

    activate(&mut state, &mut app.ctx, "Buy snow chains: 1,000 dollars");

    assert_eq!(money(&app), 500.0);
    assert_eq!(with_drive(&drive, |d| d.trip.game_minutes), minutes + 10.0);
    assert!(
        last(&app).starts_with("A new set of snow chains is in the side box. 1,000 dollars."),
        "{}",
        last(&app)
    );
    let p = app.ctx.profile.as_ref().expect("a career");
    assert!(p.chains_owned());
    assert_eq!(p.chain_wear_pct(), 0.0);
    assert_eq!(p.chain_repair_links(), CHAIN_REPAIR_LINKS_PER_SET);
    // With a set aboard the row is gone.
    let labels = build_labels(&mut state, &mut app.ctx);
    assert!(
        !labels.iter().any(|l| l.starts_with("Buy snow chains")),
        "{labels:?}"
    );
}

#[test]
fn test_a_company_driver_buys_chains_on_the_carrier() {
    let mut app = TestApp::new();
    let drive = chainless_drive(&mut app, "Denver", "Salt Lake City", COMPANY_DRIVER);
    let mut state = truck_stop(&drive);
    activate(&mut state, &mut app.ctx, "Buy snow chains, carrier billed");
    assert_eq!(money(&app), 1500.0);
    assert!(app.ctx.profile.as_ref().expect("a career").chains_owned());
}

#[test]
fn test_a_snapped_set_can_be_replaced_at_the_stop() {
    let mut app = TestApp::new();
    let drive = chainless_drive(&mut app, "Denver", "Salt Lake City", COMPANY_DRIVER);
    app.ctx
        .profile
        .as_mut()
        .expect("a career")
        .set_chains_owned(true);
    with_drive(&drive, |d| d.trip.truck.chain_wear_pct = 100.0);
    let mut state = truck_stop(&drive);
    let labels = build_labels(&mut state, &mut app.ctx);
    assert!(
        labels
            .iter()
            .any(|l| l == "Buy snow chains, carrier billed"),
        "{labels:?}"
    );
}

#[test]
fn test_no_chains_for_sale_outside_the_chain_states() {
    let mut app = TestApp::new();
    let drive = chainless_drive(&mut app, "New York", "Philadelphia", COMPANY_DRIVER);
    let mut state = truck_stop(&drive);
    let labels = build_labels(&mut state, &mut app.ctx);
    assert!(
        !labels.iter().any(|l| l.starts_with("Buy snow chains")),
        "{labels:?}"
    );
}

#[test]
fn test_an_owner_operator_short_of_cash_is_told_the_price() {
    let mut app = TestApp::new();
    let drive = chainless_drive(&mut app, "Denver", "Salt Lake City", LEASED_OWNER_OPERATOR);
    app.ctx.profile.as_mut().expect("a career").set_money(200.0);
    let mut state = truck_stop(&drive);
    app.clear_speech();
    activate(&mut state, &mut app.ctx, "Buy snow chains: 1,000 dollars");
    assert_eq!(
        last(&app),
        "A set of snow chains costs 1,000 dollars and you have 200 dollars."
    );
    assert!(!app.ctx.profile.as_ref().expect("a career").chains_owned());
}
