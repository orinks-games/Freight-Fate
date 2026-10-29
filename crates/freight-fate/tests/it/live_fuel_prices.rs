//! This week's diesel price at the pumps (`sim/real_fuel_price.rs`,
//! `GameContext::sync_fuel_prices`): the setting turns it on, the provider
//! is the offline one with the week seeded, and the economy prices every
//! region off it until the setting goes back off.

use std::sync::Arc;

use ff_core::models::economy::Economy;
use ff_core::pyfmt::round_py_n;
use ff_core::sim::real_fuel_price::{FuelPriceProvider, LiveDieselPrice};

use freight_fate::app::testing::TestApp;

#[test]
fn test_real_fuel_prices_put_the_weeks_survey_figure_at_every_pump() {
    let mut app = TestApp::new();
    let seeded_price = app.ctx.economy.fuel_price("great_lakes");
    assert!(!app.ctx.settings.real_fuel_prices);

    let provider = Arc::new(FuelPriceProvider::offline());
    app.ctx.set_fuel_price_provider(Arc::clone(&provider));
    app.ctx.settings.real_fuel_prices = true;

    // Nothing has answered yet: the seeded market stands, and nothing waits.
    app.ctx.sync_fuel_prices();
    assert_eq!(app.ctx.economy.live_national_price(), None);
    assert_eq!(app.ctx.economy.fuel_price("great_lakes"), seeded_price);

    provider.seed(LiveDieselPrice {
        per_gal: 5.967,
        week: "2026-09-07".to_string(),
    });
    // The per-frame hook every state's update runs is what applies it.
    app.ctx.update_music_rotation(0.016);
    assert_eq!(app.ctx.economy.live_national_price(), Some(5.967));
    let mean = Economy::table_national_price();
    assert_eq!(
        app.ctx.economy.fuel_price("great_lakes"),
        round_py_n(5.967 + (3.75 - mean), 2)
    );
    assert_eq!(
        app.ctx.economy.fuel_price("california"),
        round_py_n(5.967 + (5.10 - mean), 2)
    );

    // Off again: the session's own market comes back.
    app.ctx.settings.real_fuel_prices = false;
    app.ctx.sync_fuel_prices();
    assert_eq!(app.ctx.economy.live_national_price(), None);
    assert_eq!(app.ctx.economy.fuel_price("great_lakes"), seeded_price);
}
