//! The terminal truck-status reader keeps every fact independently reachable.

use crate::states_city_support::*;
use ff_core::models::career::LEVEL_XP;
use freight_fate::app::testing::TestApp;
use freight_fate::states::base::Key;
use freight_fate::states::city::{CityMenuState, TruckStatusState};

#[test]
fn truck_status_is_a_reviewable_menu_with_every_fact_on_its_own_row() {
    let mut app = TestApp::new();
    career(&mut app, "Truck Reader", "Chicago");
    {
        let p = profile_mut(&mut app);
        p.set_truck_fuel_gal(75.0);
        p.set_truck_damage_pct(32.0);
        p.set_tire_wear_pct(12.0);
        p.set_brake_wear_pct(23.0);
        p.set_engine_wear_pct(34.0);
        p.set_road_grime_pct(45.0);
        p.set_chains_owned(true);
        p.set_chain_wear_pct(56.0);
    }
    let city = CityMenuState::new(&app.ctx, false);
    app.push_state(city);
    app.clear_speech();

    select::<CityMenuState>(&mut app, "Truck status");

    assert!(is::<TruckStatusState>(&app));
    let rows = labels::<TruckStatusState>(&app);
    assert!(rows[0].starts_with("Assignment: assigned Northstar Freight Lines tractor."));
    assert!(rows.iter().any(|line| line.starts_with("Eligibility: ")));
    assert!(rows
        .iter()
        .any(|line| line == "Fuel: 50 percent, 75 gallons of 150."));
    assert!(rows
        .iter()
        .any(|line| line == "Tractor condition: worn, 32 percent damage."));
    assert!(rows
        .iter()
        .any(|line| line == "Tire wear: 12 percent, all-season tires."));
    assert!(rows.iter().any(|line| line == "Brake wear: 23 percent."));
    assert!(rows.iter().any(|line| line == "Engine wear: 34 percent."));
    assert!(rows.iter().any(|line| line == "Road grime: 45 percent."));
    assert!(rows
        .iter()
        .any(|line| line == "Snow chains: aboard, 56 percent worn."));
    assert_eq!(rows.last().map(String::as_str), Some("Back"));

    // Moving reaches fuel without replaying the rest of the truck's status;
    // Enter repeats that one fact and keeps the player on this screen.
    move_to::<TruckStatusState>(&mut app, "Fuel:");
    key(&mut app, Key::Return);
    assert!(is::<TruckStatusState>(&app));
    assert_eq!(
        app.main_lines().last().map(String::as_str),
        Some("Fuel: 50 percent, 75 gallons of 150.")
    );

    key(&mut app, Key::Escape);
    assert!(is::<CityMenuState>(&app));
}

#[test]
fn held_back_driver_can_review_and_repeat_why_the_next_truck_is_unavailable() {
    let mut app = TestApp::new();
    career(&mut app, "Held Back Driver", "Chicago");
    {
        let profile = profile_mut(&mut app);
        profile.career.xp = LEVEL_XP[10];
        profile.career.reputation = 0.0;
    }
    let city = CityMenuState::new(&app.ctx, false);
    app.push_state(city);
    app.clear_speech();

    select::<CityMenuState>(&mut app, "Truck status");

    let eligibility = labels::<TruckStatusState>(&app)
        .into_iter()
        .find(|line| line.starts_with("Eligibility: Your level earns"))
        .expect("held-back drivers must get an eligibility reason");
    assert!(eligibility.contains("dispatch trust is down"));
    assert!(eligibility.contains("clean on-time runs"));
    assert!(eligibility.contains("comes back to you"));

    move_to::<TruckStatusState>(&mut app, "Eligibility: Your level earns");
    key(&mut app, Key::Return);
    assert_eq!(app.main_lines().last(), Some(&eligibility));
}
