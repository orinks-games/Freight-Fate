//! The carried 1.9 career's "Choose a carrier" at the terminal hub.

use crate::states_city_support::*;
use ff_core::models::carryover::Carryover;
use freight_fate::app::testing::TestApp;
use freight_fate::states::base::Key;
use freight_fate::states::city::{ChooseCarrierState, CityMenuState};
use serde_json::{Map, Value};

/// A Prairie Link company driver copied from 1.9, living in `city`.
fn carried(app: &mut TestApp, city: &str) {
    career(app, "Carried", "Chicago");
    let p = profile_mut(app);
    p.achievements.push("first_day".to_string());
    p.carrier_key = "prairie_link".to_string();
    p.carrier_name = "Prairie Link Regional".to_string();
    p.home_city.clear();
    p.current_city = city.to_string();
    p.parked_facility.clear();
    p.carry_over_from_1_9();
    assert_eq!(p.carryover, Carryover::CarrierChoiceDue);
}

fn enter_terminal(app: &mut TestApp) {
    app.clear_speech();
    let terminal = CityMenuState::new(&app.ctx, false);
    app.push_state(terminal);
}

#[test]
fn the_notice_and_row_wait_for_the_load_in_hand() {
    let mut app = TestApp::new();
    carried(&mut app, "miami_fl_us");
    profile_mut(&mut app).active_trip = Some(Value::Object(Map::new()));
    enter_terminal(&mut app);
    assert!(!app.main_lines().join(" ").contains("now runs freight only"));
    assert!(!labels::<CityMenuState>(&app)
        .iter()
        .any(|t| t == "Choose a carrier"));

    // Delivered: the next visit says it, and the row takes the board's place.
    profile_mut(&mut app).active_trip = None;
    enter_terminal(&mut app);
    let spoken = app.main_lines().join(" ");
    assert!(
        spoken.contains(
            "Prairie Link Regional now runs freight only in the central plains around Kansas \
             City, Omaha, and Wichita. It no longer dispatches from Miami."
        ),
        "{spoken}"
    );
    let rows = labels::<CityMenuState>(&app);
    assert_eq!(rows[0], "Choose a carrier");
    assert!(!rows.iter().any(|t| t == "Dispatch board"));
}

#[test]
fn escape_leaves_the_choice_for_the_next_visit_and_choosing_opens_the_board() {
    let mut app = TestApp::new();
    carried(&mut app, "miami_fl_us");
    enter_terminal(&mut app);
    move_to::<CityMenuState>(&mut app, "Choose a carrier");
    key(&mut app, Key::Return);
    let rows = labels::<ChooseCarrierState>(&app);
    assert!(rows[0].starts_with("Stay with Prairie Link Regional: moves your home to"));
    assert_eq!(rows.last().map(String::as_str), Some("Back to terminal"));
    key(&mut app, Key::Escape);
    enter_terminal(&mut app);
    assert!(app.main_lines().join(" ").contains("now runs freight only"));
    assert_eq!(profile(&app).carryover, Carryover::CarrierChoiceDue);

    move_to::<CityMenuState>(&mut app, "Choose a carrier");
    key(&mut app, Key::Return);
    let second = labels::<ChooseCarrierState>(&app)[1].clone();
    move_to::<ChooseCarrierState>(&mut app, &second);
    app.clear_speech();
    key(&mut app, Key::Return);
    assert!(app
        .main_lines()
        .join(" ")
        .contains("Press Enter again to choose."));
    assert_eq!(profile(&app).carryover, Carryover::CarrierChoiceDue);
    key(&mut app, Key::Return);
    assert_eq!(profile(&app).carryover, Carryover::Settled);
    assert_ne!(profile(&app).carrier_key, "prairie_link");
    assert!(app.main_lines().join(" ").contains("has taken you on"));
    assert!(labels::<CityMenuState>(&app)
        .iter()
        .any(|t| t == "Dispatch board"));
}

#[test]
fn a_native_prairie_link_career_hears_nothing_new() {
    let mut app = TestApp::new();
    career(&mut app, "Native", "Topeka");
    {
        let p = profile_mut(&mut app);
        p.achievements.push("first_day".to_string());
        p.carrier_key = "prairie_link".to_string();
        p.carrier_name = "Prairie Link Regional".to_string();
    }
    enter_terminal(&mut app);
    assert!(!app.main_lines().join(" ").contains("now runs freight only"));
    assert!(labels::<CityMenuState>(&app)
        .iter()
        .any(|t| t == "Dispatch board"));
}
