//! Terminal garage services, billing and fatigue-adjusted fueling.

use crate::states_city_support::*;
use ff_core::models::business::{COMPANY_DRIVER, LEASED_OWNER_OPERATOR};
use ff_core::models::economy::{cad_per_litre, canada_diesel_usd_per_gal};
use ff_core::pyfmt::{fmt_f, fmt_grouped};
use freight_fate::app::testing::TestApp;
use freight_fate::states::base::Key;
use freight_fate::states::city::GarageState;
use freight_fate::states::city_garage::{
    CHAIN_SET_COST, TIRE_SERVICE_COST_PER_PCT, WINTER_TIRE_PREMIUM,
};

fn approx(a: f64, b: f64) {
    assert!((a - b).abs() < 0.01, "{a} != {b}");
}

// -- tests/test_business_arc.py: the garage -------------------------------------------

#[test]
fn test_company_driver_garage_service_is_carrier_billed() {
    let mut app = TestApp::new();
    career(&mut app, "Company Service", "Chicago");
    {
        let p = profile_mut(&mut app);
        p.business_status = COMPANY_DRIVER.to_string();
        p.set_money(25.0);
        p.set_truck_fuel_gal(0.0);
        p.set_truck_damage_pct(12.0);
    }
    app.push_state(GarageState::new());

    assert!(profile(&app).visible_owned_trucks().is_empty());
    let rows = labels::<GarageState>(&app);
    // The carrier pays; the row says what, the help says who.
    assert!(rows[0].starts_with("Refuel: "));
    assert!(!rows[0].contains("dollars"));
    key(&mut app, Key::Return);
    approx(
        profile(&app).truck_fuel_gal(),
        profile(&app).truck_specs().fuel_tank_gal,
    );
    approx(profile(&app).money(), 25.0);

    with_state_mut::<GarageState, _>(&mut app, |g, _| {
        freight_fate::states::base::Menu::menu_mut(g).index = 1
    });
    key(&mut app, Key::Return);
    approx(profile(&app).truck_damage_pct(), 0.0);
    approx(profile(&app).money(), 25.0);
}

#[test]
fn test_garage_sells_the_traction_equipment_ladder() {
    let mut app = TestApp::new();
    career(&mut app, "Equipment", "Chicago");
    {
        let p = profile_mut(&mut app);
        p.business_status = LEASED_OWNER_OPERATOR.to_string();
        p.owned_trucks = vec!["rig".to_string()];
        p.set_money(20_000.0);
        p.set_tire_wear_pct(30.0);
    }
    app.push_state(GarageState::new());

    // Winter swap: a fresh set at the premium, compound on the record.
    with_state_mut::<GarageState, _>(&mut app, |g, ctx| g.swap_tire_compound(ctx));
    let winter_cost =
        ff_core::pyfmt::round_py_n(100.0 * TIRE_SERVICE_COST_PER_PCT * WINTER_TIRE_PREMIUM, 2);
    assert_eq!(profile(&app).tire_type(), "winter");
    assert_eq!(profile(&app).tire_wear_pct(), 0.0);
    approx(profile(&app).money(), 20_000.0 - winter_cost);

    // Chains go in the side box for a flat set price.
    let money_before = profile(&app).money();
    with_state_mut::<GarageState, _>(&mut app, |g, ctx| g.buy_chains(ctx));
    assert!(profile(&app).chains_owned());
    assert_eq!(profile(&app).chain_wear_pct(), 0.0);
    approx(profile(&app).money(), money_before - CHAIN_SET_COST);

    // A fresh set aboard is not sold twice.
    let money_before = profile(&app).money();
    with_state_mut::<GarageState, _>(&mut app, |g, ctx| g.buy_chains(ctx));
    approx(profile(&app).money(), money_before);
}

#[test]
fn test_company_driver_gets_carrier_chains_but_carrier_rubber() {
    let mut app = TestApp::new();
    career(&mut app, "Company Equip", "Chicago");
    {
        let p = profile_mut(&mut app);
        p.business_status = COMPANY_DRIVER.to_string();
        p.set_money(50.0);
    }
    app.push_state(GarageState::new());

    // The carrier specs the rubber: no compound swap on the assigned rig.
    with_state_mut::<GarageState, _>(&mut app, |g, ctx| g.swap_tire_compound(ctx));
    assert_eq!(profile(&app).tire_type(), "all_season");
    approx(profile(&app).money(), 50.0);

    // Chains are required equipment: carrier billed, never out of pocket.
    with_state_mut::<GarageState, _>(&mut app, |g, ctx| g.buy_chains(ctx));
    assert!(profile(&app).chains_owned());
    approx(profile(&app).money(), 50.0);
}

// -- tests/test_smoke.py: the garage ---------------------------------------------------

#[test]
fn terminal_fueling_refusals_do_not_advance_tired_work_time() {
    for full in [true, false] {
        let mut app = TestApp::new();
        career(&mut app, "No Fuel Work", "Chicago");
        {
            let p = profile_mut(&mut app);
            p.business_status = LEASED_OWNER_OPERATOR.to_string();
            p.fatigue = 85.0;
            p.set_money(0.0);
            let gallons = if full {
                p.truck_specs().fuel_tank_gal
            } else {
                0.0
            };
            p.set_truck_fuel_gal(gallons);
        }
        let start = profile(&app).game_hours;
        let duty = profile(&app).hos.duty_min;
        app.push_state(GarageState::new());
        with_state_mut::<GarageState, _>(&mut app, |g, ctx| g.refuel(ctx));
        approx(profile(&app).game_hours, start);
        approx(profile(&app).hos.duty_min, duty);
        approx(profile(&app).fatigue, 85.0);
    }
}

#[test]
fn terminal_fueling_uses_fatigue_minutes_for_carrier_full_and_partial_fuel() {
    for (business, money) in [
        (COMPANY_DRIVER, 5_000.0),
        (LEASED_OWNER_OPERATOR, 5_000.0),
        (LEASED_OWNER_OPERATOR, 100.0),
    ] {
        for (fatigue, scale, minutes) in [
            (0.0, 20.0, 20.0),
            (40.0, 20.0, 22.0),
            (64.0, 20.0, 25.0),
            (85.0, 20.0, 30.0),
            (40.0, 10.0, 21.0),
        ] {
            let mut app = TestApp::new();
            career(&mut app, "Tired Fuel", "Chicago");
            app.ctx.settings.time_scale = scale;
            {
                let p = profile_mut(&mut app);
                p.business_status = business.to_string();
                p.fatigue = fatigue;
                p.set_money(money);
                p.set_truck_fuel_gal(0.0);
            }
            let start = profile(&app).game_hours;
            let duty = profile(&app).hos.duty_min;
            app.push_state(GarageState::new());
            with_state_mut::<GarageState, _>(&mut app, |g, ctx| g.refuel(ctx));
            approx((profile(&app).game_hours - start) * 60.0, minutes);
            approx(profile(&app).hos.duty_min - duty, minutes);
            assert!(profile(&app).fatigue > fatigue);
            assert!(profile(&app).truck_fuel_gal() > 0.0);
        }
    }
}

#[test]
fn test_garage_offers_partial_fuel_and_repairs_when_cash_is_short() {
    let mut app = TestApp::new();
    career(&mut app, "Partial Garage", "Chicago");
    profile_mut(&mut app).business_status = LEASED_OWNER_OPERATOR.to_string();
    app.push_state(GarageState::new());

    {
        let p = profile_mut(&mut app);
        p.set_money(100.0);
        p.set_truck_fuel_gal(0.0);
    }
    select::<GarageState>(&mut app, "Refuel");
    let tank = profile(&app).truck_specs().fuel_tank_gal;
    assert!(profile(&app).truck_fuel_gal() >= 1.0 && profile(&app).truck_fuel_gal() < tank);
    approx(profile(&app).money(), 0.0);

    {
        let p = profile_mut(&mut app);
        p.set_money(170.0);
        p.set_truck_damage_pct(10.0);
    }
    with_state_mut::<GarageState, _>(&mut app, |g, ctx| {
        freight_fate::states::base::Menu::refresh(g, ctx, true)
    });
    select::<GarageState>(&mut app, "Repair");
    // Repairs price on the damage-severity curve, so a short wallet buys
    // a little under the two percent the old flat rate would have sold.
    let damage = profile(&app).truck_damage_pct();
    assert!((8.0..8.5).contains(&damage), "{damage}");
    approx(profile(&app).money(), 0.0);
}

#[test]
fn test_garage_services_tires_and_wash() {
    let mut app = TestApp::new();
    career(&mut app, "Maintenance", "Chicago");
    {
        let p = profile_mut(&mut app);
        p.business_status = LEASED_OWNER_OPERATOR.to_string();
        p.owned_trucks = vec!["rig".to_string()];
        p.set_money(1_000.0);
        p.set_tire_wear_pct(10.0);
        p.set_road_grime_pct(25.0);
    }
    app.push_state(GarageState::new());

    let rows = labels::<GarageState>(&app);
    assert!(rows.iter().any(|t| t.contains("Replace all-season tires")));
    assert!(rows.iter().any(|t| t.contains("Wash truck")));

    with_state_mut::<GarageState, _>(&mut app, |g, ctx| g.service_tires(ctx));
    assert_eq!(profile(&app).tire_wear_pct(), 0.0);
    assert_eq!(profile(&app).money(), 550.0);

    with_state_mut::<GarageState, _>(&mut app, |g, ctx| g.wash_truck(ctx));
    assert_eq!(profile(&app).road_grime_pct(), 0.0);
    assert_eq!(profile(&app).money(), 515.0);
}

#[test]
fn test_garage_services_brakes_and_engine() {
    let mut app = TestApp::new();
    career(&mut app, "Maintenance", "Chicago");
    {
        let p = profile_mut(&mut app);
        p.business_status = LEASED_OWNER_OPERATOR.to_string();
        p.owned_trucks = vec!["rig".to_string()];
        p.set_money(10_000.0);
        p.set_brake_wear_pct(20.0);
        p.set_engine_wear_pct(30.0);
    }
    app.push_state(GarageState::new());

    let rows = labels::<GarageState>(&app);
    assert!(rows.iter().any(|t| t.contains("Brake job")));
    assert!(rows.iter().any(|t| t.contains("Engine overhaul")));

    with_state_mut::<GarageState, _>(&mut app, |g, ctx| g.service_brakes(ctx));
    assert_eq!(profile(&app).brake_wear_pct(), 0.0);
    assert_eq!(profile(&app).money(), 10_000.0 - 20.0 * 40.0);

    with_state_mut::<GarageState, _>(&mut app, |g, ctx| g.service_engine(ctx));
    assert_eq!(profile(&app).engine_wear_pct(), 0.0);
    assert_eq!(profile(&app).money(), 10_000.0 - 20.0 * 40.0 - 30.0 * 120.0);
}

#[test]
fn test_garage_partial_brake_service_when_broke() {
    let mut app = TestApp::new();
    career(&mut app, "Broke", "Chicago");
    {
        let p = profile_mut(&mut app);
        p.business_status = LEASED_OWNER_OPERATOR.to_string();
        p.owned_trucks = vec!["rig".to_string()];
        p.set_money(200.0); // 5 percent of brake service at 40 dollars per percent
        p.set_brake_wear_pct(50.0);
    }
    app.push_state(GarageState::new());

    with_state_mut::<GarageState, _>(&mut app, |g, ctx| g.service_brakes(ctx));
    approx(profile(&app).brake_wear_pct(), 45.0);
    approx(profile(&app).money(), 0.0);
}

#[test]
fn test_whitehorse_garage_quotes_and_charges_the_yukon_diesel_price() {
    let mut app = TestApp::new();
    career(&mut app, "Whitehorse Fuel", "Whitehorse");
    {
        let p = profile_mut(&mut app);
        p.business_status = LEASED_OWNER_OPERATOR.to_string();
        p.owned_trucks = vec!["rig".to_string()];
        p.set_money(50_000.0);
        p.set_truck_fuel_gal(0.0);
    }
    let price = app.ctx.economy.fuel_price_at("pacific_northwest", "YT");
    let cost = app.ctx.economy.fuel_cost_at(
        "pacific_northwest",
        "YT",
        profile(&app).truck_specs().fuel_tank_gal,
    );
    // In town the terminal moves the units to Canada; Automatic is liters.
    app.ctx
        .settings
        .move_units(ff_core::units::UnitsEvent::ScreenAt("CA".into()));
    let expected_label = format!(
        "Refuel {} for {} dollars. Diesel here is {} Canadian dollars a liter.",
        app.ctx
            .settings
            .volume_text(profile(&app).truck_specs().fuel_tank_gal),
        fmt_grouped(cost, 0),
        fmt_f(cad_per_litre(price), 2)
    );
    app.push_state(GarageState::new());
    assert!(labels::<GarageState>(&app)
        .iter()
        .any(|label| label == &expected_label));

    let before = profile(&app).money();
    select::<GarageState>(&mut app, "Refuel");
    approx(profile(&app).money(), before - cost);
    assert!(canada_diesel_usd_per_gal("YT").is_some());
}

#[test]
fn test_garage_refuel_requires_engine_off() {
    let mut app = TestApp::new();
    career(&mut app, "Garage Engine Gate", "Chicago");
    {
        let p = profile_mut(&mut app);
        p.business_status = LEASED_OWNER_OPERATOR.to_string();
        p.set_money(5_000.0);
        p.set_truck_fuel_gal(20.0);
        p.set_truck_engine_on(true);
    }
    app.push_state(GarageState::new());

    app.clear_speech();
    select::<GarageState>(&mut app, "Refuel");
    let said = app.main_lines().last().cloned().unwrap_or_default();
    assert!(
        said.contains("Shut the engine off before you fuel."),
        "{said}"
    );
    assert!((profile(&app).truck_fuel_gal() - 20.0).abs() < 1e-9);

    app.clear_speech();
    select::<GarageState>(&mut app, "Shut down the engine");
    let said = app.main_lines().last().cloned().unwrap_or_default();
    assert!(said.contains("Engine off."), "{said}");
    assert!(!profile(&app).truck_engine_on());

    with_state_mut::<GarageState, _>(&mut app, |g, ctx| {
        freight_fate::states::base::Menu::refresh(g, ctx, true)
    });

    app.clear_speech();
    select::<GarageState>(&mut app, "Refuel");
    let said = app.main_lines().join(" ");
    assert!(
        !said.contains("Shut the engine off before you fuel."),
        "{said}"
    );
    assert!(profile(&app).truck_fuel_gal() > 20.0);
}

#[test]
fn test_garage_full_tank_with_engine_on_speaks_the_engine_refusal() {
    // Engine first, then the tank, as at the fuel island.
    let mut app = TestApp::new();
    career(&mut app, "Garage Engine Order", "Chicago");
    {
        let p = profile_mut(&mut app);
        p.business_status = LEASED_OWNER_OPERATOR.to_string();
        p.set_money(5_000.0);
        p.set_truck_fuel_gal(10_000.0);
        p.set_truck_engine_on(true);
    }
    app.push_state(GarageState::new());

    app.clear_speech();
    select::<GarageState>(&mut app, "Fuel: tank is full");
    let said = app.main_lines().last().cloned().unwrap_or_default();
    assert!(
        said.contains("Shut the engine off before you fuel."),
        "{said}"
    );
    assert!(profile(&app).truck_engine_on());
}

#[test]
fn test_garage_help_strings_have_no_double_spaces() {
    let mut app = TestApp::new();
    career(&mut app, "Garage Help", "Chicago");
    profile_mut(&mut app).set_truck_engine_on(true);
    app.push_state(GarageState::new());
    let helps = labels_and_help::<GarageState>(&app);
    assert!(!helps.is_empty());
    for (label, help) in helps {
        assert!(
            !help.contains("  "),
            "garage help for {label:?} has a double space: {help:?}"
        );
    }
}
