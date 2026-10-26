//! Business status: moving up from the training fleet to another carrier.

use crate::states_city_support::*;
use ff_core::models::business::LEASED_OWNER_OPERATOR;
use ff_core::models::career::LEVEL_XP;
use ff_core::models::carrier_move::MOVE_UP_LEVEL;
use ff_core::models::enforcement::{LAST_CHANCE_CARRIER_KEY, LAST_CHANCE_CARRIER_NAME};
use freight_fate::app::testing::TestApp;
use freight_fate::states::city::BusinessStatusState;

fn at_training_fleet(app: &mut TestApp, level: i64) {
    career(app, "Trainee", "Milwaukee");
    let p = profile_mut(app);
    p.carrier_key = LAST_CHANCE_CARRIER_KEY.to_string();
    p.carrier_name = LAST_CHANCE_CARRIER_NAME.to_string();
    p.career.xp = LEVEL_XP[(level - 1) as usize];
    p.career.reputation = 50.0;
}

#[test]
fn a_new_trainee_hears_what_the_move_still_needs() {
    let mut app = TestApp::new();
    at_training_fleet(&mut app, 1);
    app.push_state(BusinessStatusState::new());
    let rows = labels::<BusinessStatusState>(&app);
    assert!(
        !rows.iter().any(|t| t.starts_with("Move to Northstar")),
        "{rows:?}"
    );
    app.clear_speech();
    select::<BusinessStatusState>(&mut app, "Move to another carrier: locked");
    assert!(app
        .main_lines()
        .iter()
        .any(|l| l == "Move to another carrier. Reach level 4: Regional Company Driver."));
    assert_eq!(profile(&app).carrier_key, LAST_CHANCE_CARRIER_KEY);
}

#[test]
fn a_qualified_trainee_moves_on_the_second_press() {
    let mut app = TestApp::new();
    at_training_fleet(&mut app, MOVE_UP_LEVEL);
    profile_mut(&mut app).dispatch_board_cache = Some(serde_json::json!({"old": true}));
    app.push_state(BusinessStatusState::new());
    let rows = labels::<BusinessStatusState>(&app);
    for carrier in [
        "Northstar Freight Lines",
        "Prairie Link Regional",
        "Summit Value Logistics",
    ] {
        assert!(
            rows.iter()
                .any(|t| t.starts_with(&format!("Move to {carrier}"))),
            "{rows:?}"
        );
    }
    assert!(!rows.iter().any(|t| t.contains("Great Lakes")), "{rows:?}");

    select::<BusinessStatusState>(&mut app, "Move to Summit Value Logistics");
    assert_eq!(profile(&app).carrier_key, LAST_CHANCE_CARRIER_KEY);
    assert!(current_label::<BusinessStatusState>(&app).contains("press Enter again"));

    app.clear_speech();
    key(&mut app, freight_fate::states::base::Key::Return);
    let p = profile(&app);
    assert_eq!(p.carrier_key, "summit_value");
    assert_eq!(p.carrier_name, "Summit Value Logistics");
    assert!(p.dispatch_board_cache.is_none());
    assert!(app
        .main_lines()
        .iter()
        .any(|l| l.starts_with("Summit Value Logistics hires you away from Great Lakes")));
    let rows = labels::<BusinessStatusState>(&app);
    assert!(!rows.iter().any(|t| t.starts_with("Move to")), "{rows:?}");
}

#[test]
fn a_driver_at_another_carrier_sees_no_move_rows() {
    let mut app = TestApp::new();
    career(&mut app, "Northstar Driver", "Chicago");
    app.push_state(BusinessStatusState::new());
    let rows = labels::<BusinessStatusState>(&app);
    assert!(!rows.iter().any(|t| t.starts_with("Move to")), "{rows:?}");
}

#[test]
fn an_owner_operator_leased_to_the_training_fleet_sees_no_move_rows() {
    let mut app = TestApp::new();
    at_training_fleet(&mut app, MOVE_UP_LEVEL);
    {
        let p = profile_mut(&mut app);
        p.business_status = LEASED_OWNER_OPERATOR.to_string();
        p.owned_trucks = vec!["rig".to_string()];
    }
    app.push_state(BusinessStatusState::new());
    let rows = labels::<BusinessStatusState>(&app);
    assert!(!rows.iter().any(|t| t.starts_with("Move to")), "{rows:?}");
}
