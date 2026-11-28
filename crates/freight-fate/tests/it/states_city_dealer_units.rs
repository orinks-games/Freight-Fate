//! The dealer's and the buy-in's tractor rows read fuel in the player's
//! units: under kilometers the tank in liters and the economy in liters per
//! 100 kilometers, as fuel reads everywhere else (alpha test, 2026-11).

use crate::states_city_support::*;
use ff_core::models::business::{
    COMPANY_DRIVER, LEASED_OWNER_OPERATOR, OWNER_OPERATOR_DELIVERIES, OWNER_OPERATOR_LEVEL,
    OWNER_OPERATOR_REPUTATION,
};
use ff_core::models::career::LEVEL_XP;
use freight_fate::app::testing::TestApp;
use freight_fate::states::city::{BusinessStatusState, BuyInState, TruckShopState};

fn heavy_row(rows: &[String]) -> String {
    rows.iter()
        .find(|r| r.starts_with("Heavy hauler"))
        .cloned()
        .unwrap_or_else(|| panic!("heavy hauler row in {rows:?}"))
}

fn dealer_row(units: &str) -> String {
    let mut app = TestApp::new();
    career(&mut app, "Dealer Units", "Chicago");
    app.ctx.settings.units = units.to_string();
    profile_mut(&mut app).business_status = LEASED_OWNER_OPERATOR.to_string();
    app.push_state(TruckShopState::new(false));
    heavy_row(&labels::<TruckShopState>(&app))
}

#[test]
fn the_dealer_reads_liters_under_kilometers() {
    let row = dealer_row("metric");
    assert!(row.contains(" liter tank, "), "{row}");
    assert!(
        row.contains(" liters per 100 kilometers, buy for "),
        "{row}"
    );
    assert!(!row.contains("gallon"), "{row}");

    let row = dealer_row("imperial");
    assert!(row.contains(" gallon tank, "), "{row}");
    assert!(row.contains(" miles per gallon, buy for "), "{row}");
}

#[test]
fn the_buy_in_reads_liters_under_kilometers() {
    let mut app = TestApp::new();
    career(&mut app, "Buy In Units", "Chicago");
    app.ctx.settings.units = "metric".to_string();
    {
        let p = profile_mut(&mut app);
        assert_eq!(p.business_status, COMPANY_DRIVER);
        p.career.xp = LEVEL_XP[(OWNER_OPERATOR_LEVEL - 1) as usize];
        p.career.deliveries = OWNER_OPERATOR_DELIVERIES;
        p.career.reputation = OWNER_OPERATOR_REPUTATION;
        p.set_money(300_000.0);
    }
    app.push_state(BusinessStatusState::new());
    select::<BusinessStatusState>(&mut app, "Owner-operator buy-in");
    let row = heavy_row(&labels::<BuyInState>(&app));
    assert!(row.contains(" liter tank, "), "{row}");
    assert!(row.contains(" liters per 100 kilometers, "), "{row}");
    assert!(!row.contains("gallon"), "{row}");
}
