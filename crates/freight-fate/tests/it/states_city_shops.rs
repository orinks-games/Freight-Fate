//! The terminal's shops: business status, garage, trucks, upgrades, trailer
//! programs and endorsement courses. Ports of the app parts of
//! `tests/test_business_arc.py`, `tests/test_truck_dealer_menu.py`, the
//! garage cases of `tests/test_smoke.py`, and the truck-shop case of
//! `tests/test_save_migration.py`.

use crate::states_city_support::*;
use ff_core::models::business::{
    business_status_summary, has_authority_readiness, AUTHORITY_ACTIVATION_COST,
    AUTHORITY_ACTIVATION_DELIVERIES, AUTHORITY_ACTIVATION_LEVEL, AUTHORITY_ACTIVATION_REPUTATION,
    AUTHORITY_ACTIVATION_WORKING_CAPITAL, AUTHORITY_READY_DELIVERIES, AUTHORITY_READY_LEVEL,
    AUTHORITY_READY_REPUTATION, AUTHORITY_READY_RESERVE, AUTHORITY_READY_WORKING_CAPITAL,
    COMPANY_DRIVER, INDEPENDENT_AUTHORITY, LEASED_OWNER_OPERATOR, OWNER_OPERATOR_BUY_IN,
    OWNER_OPERATOR_DELIVERIES, OWNER_OPERATOR_LEVEL, OWNER_OPERATOR_REPUTATION,
    OWNER_OPERATOR_WORKING_CAPITAL, WEIGH_STATION_TRANSPONDER_SIGNUP_FEE,
};
use ff_core::models::career::LEVEL_XP;
use ff_core::models::carrier_fleet::assigned_truck_key;
use ff_core::models::profile::Profile;
use ff_core::models::trailers::{trailer_type, DEFAULT_TRAILER_PROGRAMS};
use ff_core::models::trucks::truck_model_or_panic;
use ff_core::sim::hos;
use freight_fate::app::testing::TestApp;
use freight_fate::states::base::Key;
use freight_fate::states::city::{
    BusinessStatusState, CityMenuState, EndorsementCourseState, GarageState, TrailerProgramState,
    TruckShopState, UpgradeShopState,
};
use freight_fate::states::city_garage::{
    CHAIN_SET_COST, TIRE_SERVICE_COST_PER_PCT, WINTER_TIRE_PREMIUM,
};

fn approx(a: f64, b: f64) {
    assert!((a - b).abs() < 0.01, "{a} != {b}");
}

// -- tests/test_business_arc.py: business status --------------------------------------

#[test]
fn test_business_status_menu_unlocks_owner_operator_when_qualified() {
    let mut app = TestApp::new();
    career(&mut app, "Owner Path", "Chicago");
    {
        let p = profile_mut(&mut app);
        p.career.xp = LEVEL_XP[(OWNER_OPERATOR_LEVEL - 1) as usize];
        p.career.deliveries = OWNER_OPERATOR_DELIVERIES;
        p.career.reputation = OWNER_OPERATOR_REPUTATION;
        p.set_money(OWNER_OPERATOR_BUY_IN + OWNER_OPERATOR_WORKING_CAPITAL + 500.0);
    }

    app.push_state(BusinessStatusState::new());
    let rows = labels::<BusinessStatusState>(&app);
    assert!(rows
        .iter()
        .any(|t| t.contains("Buy into leased-on owner-operator")));
    assert!(rows.iter().any(|t| t.contains("Carrier and rank")));
    assert!(rows.iter().any(|t| t.contains("Next business unlock")));
    select::<BusinessStatusState>(&mut app, "Buy into leased-on owner-operator");

    assert_eq!(profile(&app).business_status, LEASED_OWNER_OPERATOR);
    approx(
        profile(&app).money(),
        OWNER_OPERATOR_WORKING_CAPITAL + 500.0,
    );
    assert!(profile(&app).dispatch_board_cache.is_none());
}

#[test]
fn test_owner_operator_buy_in_records_first_owned_tractor() {
    let mut app = TestApp::new();
    career(&mut app, "Buy In Equipment", "Chicago");
    {
        let p = profile_mut(&mut app);
        p.career.xp = LEVEL_XP[(OWNER_OPERATOR_LEVEL - 1) as usize];
        p.career.deliveries = OWNER_OPERATOR_DELIVERIES;
        p.career.reputation = OWNER_OPERATOR_REPUTATION;
        p.set_money(OWNER_OPERATOR_BUY_IN + OWNER_OPERATOR_WORKING_CAPITAL);
    }

    app.push_state(BusinessStatusState::new());
    select::<BusinessStatusState>(&mut app, "Buy into leased-on owner-operator");

    assert_eq!(profile(&app).business_status, LEASED_OWNER_OPERATOR);
    // The buy-in takes over the tractor dispatch had you in: at the
    // level-18 gate that is a first-pick fleet unit, not the starter rig.
    let assigned =
        assigned_truck_key::<Profile, ff_core::models::carrier_fleet::NoJob>(profile(&app), None);
    assert_eq!(profile(&app).truck, assigned);
    assert_eq!(profile(&app).visible_owned_trucks(), vec![assigned]);
    assert_eq!(
        profile(&app).active_trailer_programs(),
        DEFAULT_TRAILER_PROGRAMS
            .iter()
            .map(|s| s.to_string())
            .collect::<Vec<_>>()
    );
}

#[test]
fn test_business_status_menu_sets_authority_readiness_reserve() {
    let mut app = TestApp::new();
    career(&mut app, "Authority Ready", "Chicago");
    {
        let p = profile_mut(&mut app);
        p.business_status = LEASED_OWNER_OPERATOR.to_string();
        p.owned_trucks = vec!["rig".to_string()];
        p.career.xp = LEVEL_XP[(AUTHORITY_READY_LEVEL - 1) as usize];
        p.career.deliveries = AUTHORITY_READY_DELIVERIES;
        p.career.reputation = AUTHORITY_READY_REPUTATION;
        p.set_money(AUTHORITY_READY_RESERVE + AUTHORITY_READY_WORKING_CAPITAL + 500.0);
        p.dispatch_board_cache = Some(serde_json::json!({"old": true}));
    }

    app.push_state(BusinessStatusState::new());
    select::<BusinessStatusState>(&mut app, "Commit 12,500 dollars to authority prep");

    assert!(has_authority_readiness(profile(&app)));
    approx(
        profile(&app).money(),
        AUTHORITY_READY_WORKING_CAPITAL + 500.0,
    );
    assert!(profile(&app).dispatch_board_cache.is_none());
    assert!(business_status_summary(profile(&app)).contains("Authority prep reserve is set"));
    assert!(labels::<BusinessStatusState>(&app)
        .iter()
        .any(|t| t.contains("Authority prep reserve: set")));
}

#[test]
fn test_business_status_menu_activates_own_authority() {
    let mut app = TestApp::new();
    career(&mut app, "Own Authority", "Chicago");
    {
        let p = profile_mut(&mut app);
        p.business_status = LEASED_OWNER_OPERATOR.to_string();
        p.owned_trucks = vec!["rig".to_string()];
        p.trailer_programs = vec!["dry_van".to_string(), "reefer".to_string()];
        p.authority_readiness = true;
        p.career.xp = LEVEL_XP[(AUTHORITY_ACTIVATION_LEVEL - 1) as usize];
        p.career.deliveries = AUTHORITY_ACTIVATION_DELIVERIES;
        p.career.reputation = AUTHORITY_ACTIVATION_REPUTATION;
        p.set_money(AUTHORITY_ACTIVATION_COST + AUTHORITY_ACTIVATION_WORKING_CAPITAL + 750.0);
        p.dispatch_board_cache = Some(serde_json::json!({"old": true}));
    }

    app.push_state(BusinessStatusState::new());
    select::<BusinessStatusState>(&mut app, "Activate own authority");

    assert_eq!(profile(&app).business_status, INDEPENDENT_AUTHORITY);
    approx(
        profile(&app).money(),
        AUTHORITY_ACTIVATION_WORKING_CAPITAL + 750.0,
    );
    assert!(profile(&app).dispatch_board_cache.is_none());
    assert!(business_status_summary(profile(&app)).contains("Direct freight"));
    assert!(labels::<BusinessStatusState>(&app)
        .iter()
        .any(|t| t.contains("Own authority active")));
}

// --- the transponder row an owner-operator cannot afford yet -----------------
//
// It used to appear only once the fee was already in the bank, so the driver
// with most to gain from knowing the transponder exists was the one told
// nothing about it -- and the eligibility reasons were computed at that call
// site and discarded (owner, 2026-08-21).

fn owner_operator_in_business_menu(app: &mut TestApp, money: f64) {
    career(app, "Lease Op", "Chicago");
    {
        let p = profile_mut(app);
        p.business_status = LEASED_OWNER_OPERATOR.to_string();
        p.career.xp = LEVEL_XP[(OWNER_OPERATOR_LEVEL - 1) as usize];
        p.set_money(money);
    }
    app.push_state(BusinessStatusState::new());
}

#[test]
fn test_transponder_shows_as_locked_when_the_fee_is_out_of_reach() {
    let mut app = TestApp::new();
    owner_operator_in_business_menu(&mut app, WEIGH_STATION_TRANSPONDER_SIGNUP_FEE - 1.0);
    let rows = labels::<BusinessStatusState>(&app);
    assert!(
        rows.iter()
            .any(|t| t.contains("Weigh station transponder locked")),
        "no locked transponder row: {rows:?}"
    );
    assert!(!rows
        .iter()
        .any(|t| t.contains("Subscribe to weigh station transponder")));
}

#[test]
fn test_the_locked_transponder_row_says_what_it_is_waiting_on() {
    let mut app = TestApp::new();
    owner_operator_in_business_menu(&mut app, WEIGH_STATION_TRANSPONDER_SIGNUP_FEE - 1.0);
    app.clear_speech();
    activate::<BusinessStatusState>(&mut app, "Weigh station transponder locked");

    let spoken = app.main_lines();
    assert!(!spoken.is_empty(), "the locked row answered with silence");
    let said = spoken.last().unwrap();
    // The money reason, not the generic next-business-unlock answer: a
    // locked row that answers a different question teaches nothing.
    assert!(
        said.contains(&ff_core::pyfmt::fmt_grouped(
            WEIGH_STATION_TRANSPONDER_SIGNUP_FEE,
            0
        )),
        "{said}"
    );
}

#[test]
fn test_the_subscribe_row_returns_once_the_fee_is_affordable() {
    let mut app = TestApp::new();
    owner_operator_in_business_menu(&mut app, WEIGH_STATION_TRANSPONDER_SIGNUP_FEE + 1.0);
    let rows = labels::<BusinessStatusState>(&app);
    assert!(rows
        .iter()
        .any(|t| t.contains("Subscribe to weigh station transponder")));
    assert!(!rows
        .iter()
        .any(|t| t.contains("Weigh station transponder locked")));
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

// -- tests/test_business_arc.py and test_smoke.py: trucks and upgrades ------------------

#[test]
fn test_company_driver_shops_hide_owned_truck_language() {
    let mut app = TestApp::new();
    career(&mut app, "No Ownership", "Chicago");
    {
        let p = profile_mut(&mut app);
        p.owned_trucks = vec!["rig".to_string(), "heavy_hauler".to_string()]; // old save values stay hidden
        p.set_money(200_000.0);
    }

    app.push_state(TruckShopState::new(false));
    assert!(labels::<TruckShopState>(&app)[0].contains("carrier-assigned tractor"));
    assert!(!labels::<TruckShopState>(&app)
        .iter()
        .any(|t| t.to_lowercase().contains("owned")));
    key(&mut app, Key::Return);
    assert_eq!(profile(&app).truck, "rig");
    approx(profile(&app).money(), 200_000.0);
    assert!(app
        .main_lines()
        .last()
        .unwrap()
        .contains("owner-operator buy-in"));

    app.pop_state();
    app.push_state(UpgradeShopState::new());
    assert!(labels::<UpgradeShopState>(&app)[0].contains("carrier-assigned tractor"));
    assert!(!labels::<UpgradeShopState>(&app)
        .iter()
        .any(|t| t.to_lowercase().contains("owned")));
    key(&mut app, Key::Return);
    assert!(profile(&app).upgrades.is_empty());
    approx(profile(&app).money(), 200_000.0);
    assert!(app
        .main_lines()
        .last()
        .unwrap()
        .contains("owner-operator buy-in"));
}

#[test]
fn test_owner_operator_can_buy_switch_and_upgrade_owned_equipment() {
    let mut app = TestApp::new();
    career(&mut app, "Owned Equipment", "Chicago");
    {
        let p = profile_mut(&mut app);
        p.business_status = LEASED_OWNER_OPERATOR.to_string();
        p.owned_trucks = vec!["rig".to_string()];
        p.set_money(200_000.0);
    }

    app.push_state(UpgradeShopState::new());
    key(&mut app, Key::Return);
    assert!(!profile(&app).upgrades.is_empty());
    assert!(profile(&app).money() < 200_000.0);

    app.pop_state();
    let money_after_upgrade = profile(&app).money();
    app.push_state(TruckShopState::new(false));
    select::<TruckShopState>(&mut app, "Heavy hauler");
    assert_eq!(profile(&app).truck, "heavy_hauler");
    assert!(profile(&app)
        .visible_owned_trucks()
        .iter()
        .any(|k| k == "heavy_hauler"));
    approx(profile(&app).money(), money_after_upgrade - 52_000.0);

    let money_before_switch = profile(&app).money();
    select::<TruckShopState>(&mut app, "Standard rig");
    assert_eq!(profile(&app).truck, "rig");
    approx(profile(&app).money(), money_before_switch);
}

#[test]
fn test_upgrades_are_money_gated() {
    let mut app = TestApp::new();
    career(&mut app, "Broke", "Chicago");
    {
        let p = profile_mut(&mut app);
        p.business_status = LEASED_OWNER_OPERATOR.to_string();
        p.set_money(10.0);
    }
    app.push_state(UpgradeShopState::new());
    key(&mut app, Key::Return);
    assert!(profile(&app).upgrades.is_empty());
    assert_eq!(profile(&app).money(), 10.0);
}

#[test]
fn test_upgrade_f1_help_explains_player_benefits() {
    let mut app = TestApp::new();
    career(&mut app, "Helper", "Chicago");
    {
        let p = profile_mut(&mut app);
        p.business_status = LEASED_OWNER_OPERATOR.to_string();
        p.owned_trucks = vec!["rig".to_string()];
    }
    app.push_state(UpgradeShopState::new());
    let mut help_by_label = std::collections::HashMap::new();
    for (label, help) in labels_and_help::<UpgradeShopState>(&app) {
        let key = label
            .split(':')
            .next()
            .unwrap()
            .split(',')
            .next()
            .unwrap()
            .to_lowercase();
        help_by_label.insert(key, help.to_lowercase());
    }

    let engine = &help_by_label["engine tune"];
    assert!(engine.contains("more pulling power"));
    assert!(engine.contains("heavy freight"));
    let aero = &help_by_label["aerodynamic kit"];
    assert!(aero.contains("burn less fuel at highway speed"));
    assert!(aero.contains("same tank last longer"));
    let tank = &help_by_label["long-range tank"];
    assert!(tank.contains("fifty gallons"));
    assert!(tank.contains("carry more fuel"));
    assert!(tank.contains("more distance between fuel stops"));
    let brakes = &help_by_label["reinforced brakes"];
    assert!(brakes.contains("emergency stops"));
    assert!(brakes.contains("downhill control"));
}

#[test]
fn test_bought_truck_starts_fresh_and_each_keeps_its_own_condition() {
    let mut app = TestApp::new();
    career(&mut app, "Fleet Condition", "Chicago");
    {
        // Only an owner-operator buys or switches tractors; a company driver
        // sees a locked shop with nothing to pick.
        let p = profile_mut(&mut app);
        p.business_status = LEASED_OWNER_OPERATOR.to_string();
        p.owned_trucks = vec!["rig".to_string()];
        p.set_money(60_000.0);
        p.set_truck_fuel_gal(40.0);
        p.set_truck_damage_pct(30.0);
        p.set_tire_wear_pct(12.0);
        p.set_road_grime_pct(55.0);
    }
    app.push_state(TruckShopState::new(false));
    select::<TruckShopState>(&mut app, "Heavy hauler");

    assert_eq!(profile(&app).truck, "heavy_hauler");
    assert_eq!(
        profile(&app).truck_fuel_gal(),
        truck_model_or_panic("heavy_hauler").specs.fuel_tank_gal
    );
    assert_eq!(profile(&app).truck_damage_pct(), 0.0);
    assert_eq!(profile(&app).tire_wear_pct(), 0.0);
    // Grime belongs to the truck that earned it, so a tractor off the lot
    // is clean no matter how filthy the one it replaces was.
    assert_eq!(profile(&app).road_grime_pct(), 0.0);

    // The rig kept its own condition and gets it back on switch.
    select::<TruckShopState>(&mut app, "Standard rig");
    assert_eq!(profile(&app).truck, "rig");
    assert_eq!(profile(&app).truck_fuel_gal(), 40.0);
    assert_eq!(profile(&app).truck_damage_pct(), 30.0);
    assert_eq!(profile(&app).tire_wear_pct(), 12.0);
    // ...including the grime it was parked with.
    assert_eq!(profile(&app).road_grime_pct(), 55.0);

    let path = profile(&app).path();
    let loaded = Profile::load(&path).expect("the save reloads");
    assert_eq!(loaded.truck_conditions["rig"]["fuel_gal"], 40.0);
    assert_eq!(loaded.truck_conditions["rig"]["grime_pct"], 55.0);
    assert_eq!(loaded.truck_conditions["heavy_hauler"]["damage_pct"], 0.0);
    assert_eq!(loaded.truck_conditions["heavy_hauler"]["grime_pct"], 0.0);
}

// -- tests/test_truck_dealer_menu.py ---------------------------------------------------

/// An owner-operator in a company driver's seat.
fn owner_operator(app: &mut TestApp) {
    let p = profile_mut(app);
    p.business_status = LEASED_OWNER_OPERATOR.to_string();
    p.owned_trucks = vec!["rig".to_string()];
}

#[test]
fn test_the_terminal_menu_offers_truck_dealer_directly() {
    let mut app = TestApp::new();
    career(&mut app, "Dale", "Buffalo");
    owner_operator(&mut app);
    let mut menu = CityMenuState::new(&app.ctx, false);
    let rows = built_labels(&mut app, &mut menu);

    assert!(rows.iter().any(|t| t == "Truck dealer"));
    assert!(!rows.iter().any(|t| t == "Drive to city services"));
}

/// The carrier assigns a company driver's tractor, tires and trailer, so the
/// owner's shopping stays off their menus until the buy-in (owner,
/// 2026-09-28).
#[test]
fn test_a_company_driver_is_not_shown_owner_operator_shops() {
    let mut app = TestApp::new();
    career(&mut app, "Dale", "Buffalo");
    profile_mut(&mut app).business_status = COMPANY_DRIVER.to_string();
    let mut menu = CityMenuState::new(&app.ctx, false);
    let rows = built_labels(&mut app, &mut menu);
    assert!(!rows.iter().any(|t| t == "Truck dealer"), "{rows:?}");
    assert!(rows.iter().any(|t| t == "Business status"), "{rows:?}");

    app.push_state(GarageState::new());
    let rows = labels::<GarageState>(&app);
    for hidden in ["Upgrades", "Trucks", "Trailer programs"] {
        assert!(!rows.iter().any(|t| t == hidden), "{hidden}: {rows:?}");
    }
    assert!(
        !rows.iter().any(|t| t.starts_with("Tire compound")),
        "{rows:?}"
    );
}

#[test]
fn test_the_truck_dealer_item_pushes_truck_shop_state() {
    let mut app = TestApp::new();
    career(&mut app, "Dale", "Buffalo");
    owner_operator(&mut app);
    let city = CityMenuState::new(&app.ctx, false);
    app.push_state(city);
    activate::<CityMenuState>(&mut app, "Truck dealer");

    assert!(is::<TruckShopState>(&app));
}

#[test]
fn test_truck_shop_entry_names_the_source_backed_dealer() {
    // Opened from the terminal's "Truck dealer" row (at_dealer=True), the
    // entry names the source-backed dealer the player is actually standing in.
    let mut app = TestApp::new();
    career(&mut app, "Dale", "Indianapolis");
    let dealer = app
        .ctx
        .world
        .city_service("Indianapolis", "truck_dealer")
        .expect("Indianapolis has a dealer");
    assert!(!dealer.fallback);
    app.clear_speech();

    app.push_state(TruckShopState::new(true));

    let spoken = app.main_lines();
    assert!(spoken.iter().any(|line| line.contains(&dealer.name)));
    assert!(spoken
        .iter()
        .any(|line| line.starts_with(&format!("Inside {}. Trucks.", dealer.name))));
}

#[test]
fn test_truck_shop_entry_stays_plain_for_a_fallback_city() {
    let mut app = TestApp::new();
    career(&mut app, "Dale", "Erie");
    let dealer = app
        .ctx
        .world
        .city_service("Erie", "truck_dealer")
        .expect("Erie has a service row");
    assert!(dealer.fallback);
    app.clear_speech();

    app.push_state(TruckShopState::new(true));

    let spoken = app.main_lines();
    assert!(spoken
        .iter()
        .any(|line| line.starts_with("Trucks. You have")));
    assert!(!spoken.iter().any(|line| line.contains("Inside")));
}

#[test]
fn test_truck_shop_entry_stays_plain_from_the_garage() {
    // Opened from the terminal garage's "Trucks" row (no at_dealer flag),
    // the entry never names the dealer -- even in a city with a real
    // source-backed one -- because the player is standing in the garage, not
    // the dealership, and naming it would contradict where they actually are.
    let mut app = TestApp::new();
    career(&mut app, "Dale", "Indianapolis");
    let dealer = app
        .ctx
        .world
        .city_service("Indianapolis", "truck_dealer")
        .expect("Indianapolis has a dealer");
    assert!(!dealer.fallback);
    owner_operator(&mut app);

    app.push_state(GarageState::new());
    app.clear_speech();
    activate::<GarageState>(&mut app, "Trucks");

    assert!(is::<TruckShopState>(&app));
    assert!(!with_state::<TruckShopState, _>(&app, |s, _| s.at_dealer));
    let spoken = app.main_lines();
    assert!(!spoken.iter().any(|line| line.contains("Inside")));
    assert!(!spoken.iter().any(|line| line.contains(&dealer.name)));
}

// -- tests/test_business_arc.py: trailer programs --------------------------------------

#[test]
fn test_owner_operator_can_add_specialty_trailer_program() {
    let mut app = TestApp::new();
    career(&mut app, "Trailer Lease", "Chicago");
    {
        let p = profile_mut(&mut app);
        p.business_status = LEASED_OWNER_OPERATOR.to_string();
        p.owned_trucks = vec!["rig".to_string()];
        p.set_money(20_000.0);
    }

    app.push_state(TrailerProgramState::new());
    assert!(labels::<TrailerProgramState>(&app)[0]
        .contains("Dry van: included carrier trailer program"));
    select::<TrailerProgramState>(&mut app, "Reefer");

    assert!(profile(&app)
        .active_trailer_programs()
        .iter()
        .any(|k| k == "reefer"));
    approx(profile(&app).money(), 12_000.0);
    assert!(profile(&app).dispatch_board_cache.is_none());
}

#[test]
fn test_own_authority_can_buy_owned_trailer() {
    let mut app = TestApp::new();
    career(&mut app, "Trailer Owner", "Chicago");
    let reefer = trailer_type("reefer").unwrap();
    {
        let p = profile_mut(&mut app);
        p.business_status = INDEPENDENT_AUTHORITY.to_string();
        p.owned_trucks = vec!["rig".to_string()];
        p.trailer_programs = vec!["dry_van".to_string(), "reefer".to_string()];
        p.set_money(reefer.purchase_price + 2_000.0);
        p.dispatch_board_cache = Some(serde_json::json!({"old": true}));
    }

    app.push_state(TrailerProgramState::new());
    move_to::<TrailerProgramState>(&mut app, "Reefer");
    assert!(current_label::<TrailerProgramState>(&app).contains("buy trailer"));
    key(&mut app, Key::Return);

    assert_eq!(profile(&app).visible_owned_trailers(), vec!["reefer"]);
    assert!(profile(&app)
        .active_trailer_programs()
        .iter()
        .any(|k| k == "reefer"));
    approx(profile(&app).money(), 2_000.0);
    assert!(profile(&app).dispatch_board_cache.is_none());
    assert!(current_label::<TrailerProgramState>(&app).contains("owned trailer"));
}

#[test]
fn test_leased_on_owner_operator_does_not_see_trailer_purchase() {
    let mut app = TestApp::new();
    career(&mut app, "Leased Trailer", "Chicago");
    {
        let p = profile_mut(&mut app);
        p.business_status = LEASED_OWNER_OPERATOR.to_string();
        p.owned_trucks = vec!["rig".to_string()];
        p.set_money(200_000.0);
    }

    app.push_state(TrailerProgramState::new());

    let rows = labels::<TrailerProgramState>(&app);
    assert!(rows.iter().any(|t| t.contains("lease program")));
    assert!(!rows.iter().any(|t| t.contains("buy trailer")));
    assert!(profile(&app).visible_owned_trailers().is_empty());
}

#[test]
fn test_company_driver_trailer_program_menu_stays_carrier_provided() {
    let mut app = TestApp::new();
    career(&mut app, "Company Trailers", "Chicago");
    app.push_state(TrailerProgramState::new());

    let rows = labels::<TrailerProgramState>(&app);
    assert!(rows[0].contains("carrier-provided trailers"));
    assert!(!rows.iter().any(|t| t.contains("lease program for")));
}

// -- the endorsement course screen ------------------------------------------------------

#[test]
fn endorsement_courses_price_each_unearned_endorsement() {
    // No Python test of its own: the screen is exercised through the
    // terminal, and this pins the rows it offers a level-one driver.
    let mut app = TestApp::new();
    career(&mut app, "Course Buyer", "Chicago");
    profile_mut(&mut app).set_money(50_000.0);
    app.push_state(EndorsementCourseState::new());

    let rows = labels::<EndorsementCourseState>(&app);
    assert!(rows
        .iter()
        .any(|t| t.starts_with("Refrigerated certificate course:")
            && t.contains("carrier-sponsored free at level")));
    // A background-checked course says so on its row; a course-only
    // endorsement never claims a sponsor level.
    assert!(rows
        .iter()
        .any(|t| t.starts_with("Hazmat endorsement course:")
            && t.contains("background check")
            && !t.contains("carrier-sponsored")));
    // F1 on a course says what it opens and both roads to it: the sponsor
    // level, and the level a driver can pay for it early (owner, 2026-09-18).
    move_to::<EndorsementCourseState>(&mut app, "Refrigerated certificate course:");
    let help = current_help::<EndorsementCourseState>(&app);
    assert!(
        help.starts_with("Unlocks fresh food and refrigerated goods."),
        "{help}"
    );
    assert!(help.contains("sponsors it free at level 2"), "{help}");
    assert!(
        help.contains("900 dollars for it yourself from level 1"),
        "{help}"
    );
    move_to::<EndorsementCourseState>(&mut app, "Hazmat endorsement course:");
    let help = current_help::<EndorsementCourseState>(&app);
    assert!(
        help.starts_with("Unlocks placarded hazardous materials"),
        "{help}"
    );
    assert!(
        help.contains("Course only: 185 dollars from level 10."),
        "{help}"
    );
    let before = profile(&app).money();
    select::<EndorsementCourseState>(&mut app, "Refrigerated certificate course:");
    assert!(profile(&app).money() < before);
    assert!(profile(&app)
        .career
        .purchased_endorsements
        .iter()
        .any(|k| k == "refrigerated"));
    assert!(labels::<EndorsementCourseState>(&app)
        .iter()
        .any(|t| t.contains("earned, self-paid course")));
    move_to::<EndorsementCourseState>(&mut app, "earned, self-paid course");
    let help = current_help::<EndorsementCourseState>(&app);
    assert!(
        help.contains("unlocks fresh food and refrigerated goods"),
        "{help}"
    );
}

/// Book the flatbed course (8 h) in `city` at absolute `game_hours` with
/// `fatigue` on arrival. Returns the fatigue after and the completion line.
fn take_flatbed_course(
    city: &str,
    game_hours: f64,
    fatigue: f64,
    setup: impl FnOnce(&mut TestApp),
) -> (f64, String) {
    let mut app = TestApp::new();
    career(&mut app, "Course Fatigue", city);
    app.ctx.settings.time_scale = 20.0; // Standard pressure unless setup says otherwise
    {
        let p = profile_mut(&mut app);
        p.set_money(50_000.0);
        p.fatigue = fatigue;
        p.game_hours = game_hours;
    }
    setup(&mut app);
    app.push_state(EndorsementCourseState::new());
    select::<EndorsementCourseState>(&mut app, "Flatbed securement certificate course:");
    let line = app
        .speech()
        .lines()
        .into_iter()
        .rfind(|l| l.starts_with("Course complete"))
        .expect("a completion line");
    (profile(&app).fatigue, line)
}

#[test]
fn course_fatigue_runs_at_the_classroom_rate_day_and_night() {
    // GitHub #314: a course is awake time, not sleep, but a classroom has no
    // driving vigilance load, so it runs at half the driving rate. Chicago
    // is Central: 8.0 Eastern is 7 AM local, a clean 8 h of day.
    let (day, line) = take_flatbed_course("Chicago", 8.0, 10.0, |_| {});
    approx(day, 10.0 + 0.0575 * 480.0); // 37.6
    approx(
        day,
        10.0 + hos::CLASSROOM_FATIGUE_FACTOR * hos::awake_fatigue_gain(7.0, 8.0 * 60.0),
    );
    assert!(!line.contains("drowsy"), "{line}");

    // 22.0 Eastern is 9 PM Central: 9 PM to 5 AM is all night.
    let (night, line) = take_flatbed_course("Chicago", 22.0, 10.0, |_| {});
    approx(night, 10.0 + 0.085 * 480.0); // 50.8
    assert!(!line.contains("drowsy"), "{line}");
}

fn take_lcv_course(game_hours: f64, fatigue: f64) -> f64 {
    let mut app = TestApp::new();
    career(&mut app, "Course Fatigue", "Chicago");
    app.ctx.settings.time_scale = 20.0;
    {
        let p = profile_mut(&mut app);
        p.fatigue = fatigue;
        p.game_hours = game_hours;
        p.career.xp = LEVEL_XP[19]; // level 20
        p.career
            .purchased_endorsements
            .push("doubles_triples".to_string());
        p.set_money(50_000.0);
    }
    app.push_state(EndorsementCourseState::new());
    select::<EndorsementCourseState>(&mut app, "Lcv certificate course:");
    profile(&app).fatigue
}

#[test]
fn multi_day_course_counts_only_the_last_class_day() {
    // LCV is the 24 h course: class days with sleep between, so the driver
    // leaves with the last class day's fatigue, whatever they walked in
    // with. HOS still books it off duty.
    let after_24 = take_lcv_course(8.0, 95.0); // 7 AM Central
                                               // The last class day is 8 AM to 4 PM local, all day rate: 0.0575 x 480.
    approx(after_24, 27.6);
    approx(
        after_24,
        hos::course_fatigue(0.0, 7.0, 24.0 * 60.0, |_| 1.0),
    );
    assert_ne!(after_24, 0.0, "a course must not wipe fatigue to zero");
}

#[test]
fn multi_day_course_scores_a_daytime_class_day_from_any_start_hour() {
    // Schools teach in the daytime: an LCV course started at 3 to 7 AM
    // used to run its last class day overnight (37.5 to 40.8). Chicago is
    // Central, an hour behind the Eastern game clock.
    for local in [3.0, 5.0, 7.0, 12.0, 21.0] {
        for arrival in [0.0, 95.0] {
            let after = take_lcv_course(local + 1.0, arrival);
            assert!(
                (after - 27.6).abs() < 0.01,
                "{local} local, arrival {arrival}: {after}"
            );
        }
    }
}

#[test]
fn course_day_and_night_follow_the_western_city_clock() {
    // 5.0 Eastern is dawn (day rate) on the Eastern clock, but 2 AM in Los
    // Angeles: three night hours before the Pacific dawn.
    let (west, _) = take_flatbed_course("Los Angeles", 5.0, 10.0, |_| {});
    approx(west, 10.0 + 0.085 * 180.0 + 0.0575 * 300.0); // 42.55
    let eastern_clock = 10.0 + 0.0575 * 480.0; // what the Eastern clock gave: 37.6
    assert!(west > eastern_clock + 4.0, "{west}");
}

#[test]
fn course_fatigue_honors_food_buffs_and_relaxed_mode() {
    // A fatigue buff at half rate that wears off four hours into class, and
    // Relaxed pressure (0.8 of the fatigue rate) on top.
    let (after, _) = take_flatbed_course("Chicago", 8.0, 10.0, |app| {
        app.ctx.settings.time_scale = 10.0;
        profile_mut(app).add_timed_buff(serde_json::json!({
            "group": "fatigue", "rate": 0.5, "expires_h": 12.0
        }));
    });
    approx(after, 10.0 + 0.8 * (0.0575 * 240.0 * 0.5 + 0.0575 * 240.0)); // 26.56
}

#[test]
fn course_completion_warns_when_it_leaves_you_drowsy() {
    // 25 + 40.8 night = 65.8, past the drowsy threshold.
    let (drowsy, line) = take_flatbed_course("Chicago", 22.0, 25.0, |_| {});
    assert!((hos::FATIGUE_DROWSY..hos::FATIGUE_SEVERE).contains(&drowsy));
    assert!(
        line.ends_with("You're drowsy after the course. Sleep before you drive."),
        "{line}"
    );
    // 50 + 40.8 = 90.8, past severe.
    let (severe, line) = take_flatbed_course("Chicago", 22.0, 50.0, |_| {});
    assert!(severe >= hos::FATIGUE_SEVERE);
    assert!(
        line.ends_with("You're dangerously drowsy after the course. Sleep before you drive."),
        "{line}"
    );
    // Just under: 19 + 40.8 = 59.8 stays quiet.
    let (under, line) = take_flatbed_course("Chicago", 22.0, 19.0, |_| {});
    assert!(under < hos::FATIGUE_DROWSY);
    assert!(!line.contains("drowsy"), "{line}");
}

#[test]
fn test_business_status_lets_a_qualified_driver_stay_a_company_driver() {
    // Owner, 2026-09-03: a player who does not want the lease needs a way
    // to say so, and a way back if they change their mind.
    let mut app = TestApp::new();
    career(&mut app, "Stay Company", "Chicago");
    {
        let p = profile_mut(&mut app);
        p.career.xp = LEVEL_XP[(OWNER_OPERATOR_LEVEL - 1) as usize];
        p.career.deliveries = OWNER_OPERATOR_DELIVERIES;
        p.career.reputation = OWNER_OPERATOR_REPUTATION;
        p.set_money(OWNER_OPERATOR_BUY_IN + OWNER_OPERATOR_WORKING_CAPITAL + 500.0);
    }

    app.push_state(BusinessStatusState::new());
    let rows = labels::<BusinessStatusState>(&app);
    assert!(
        rows.iter().any(|t| t == "Stay a company driver"),
        "{rows:?}"
    );
    app.clear_speech();
    select::<BusinessStatusState>(&mut app, "Stay a company driver");

    assert!(profile(&app).owner_operator_declined);
    assert_eq!(profile(&app).business_status, COMPANY_DRIVER);
    assert_eq!(
        ff_core::models::business::display_rank_for(profile(&app)).title,
        "Company Fleet Captain"
    );
    let said = app.main_lines().join(" ");
    assert!(said.contains("Staying a company driver"), "{said}");
    app.clear_speech();
    select::<BusinessStatusState>(&mut app, "Carrier and rank");
    let rank_said = app.main_lines().join(" ");
    assert!(rank_said.contains("Company Fleet Captain"), "{rank_said}");
    assert!(
        !rank_said.contains("Leased-On Owner-Operator"),
        "{rank_said}"
    );
    let rows = labels::<BusinessStatusState>(&app);
    assert!(
        rows.iter()
            .any(|t| t.contains("Buy into leased-on owner-operator")),
        "the buy-in row must stay: {rows:?}"
    );
    assert!(
        rows.iter().any(|t| t == "Reopen the owner-operator plan"),
        "{rows:?}"
    );
    assert!(
        !rows.iter().any(|t| t == "Stay a company driver"),
        "{rows:?}"
    );

    select::<BusinessStatusState>(&mut app, "Reopen the owner-operator plan");
    assert!(!profile(&app).owner_operator_declined);
    let rows = labels::<BusinessStatusState>(&app);
    assert!(
        rows.iter().any(|t| t == "Stay a company driver"),
        "{rows:?}"
    );

    // And buying in after all clears the choice with the status.
    select::<BusinessStatusState>(&mut app, "Stay a company driver");
    select::<BusinessStatusState>(&mut app, "Buy into leased-on owner-operator");
    assert_eq!(profile(&app).business_status, LEASED_OWNER_OPERATOR);
    assert!(!profile(&app).owner_operator_declined);
}

#[test]
fn test_business_status_lets_an_owner_operator_go_back_to_company_driving() {
    let mut app = TestApp::new();
    career(&mut app, "Back To Company", "Chicago");
    {
        let p = profile_mut(&mut app);
        p.career.xp = LEVEL_XP[(OWNER_OPERATOR_LEVEL - 1) as usize];
        p.career.deliveries = OWNER_OPERATOR_DELIVERIES;
        p.career.reputation = OWNER_OPERATOR_REPUTATION;
        p.set_money(OWNER_OPERATOR_BUY_IN + OWNER_OPERATOR_WORKING_CAPITAL);
    }
    app.push_state(BusinessStatusState::new());
    select::<BusinessStatusState>(&mut app, "Buy into leased-on owner-operator");
    assert_eq!(profile(&app).business_status, LEASED_OWNER_OPERATOR);
    let owned = profile(&app).owned_trucks.clone();
    assert_eq!(owned.len(), 1);
    let money_after_buy_in = profile(&app).money();
    let expected_back = (truck_model_or_panic(&owned[0]).price
        * ff_core::models::solvency::REPOSSESSION_EQUITY_SHARE)
        .min(OWNER_OPERATOR_BUY_IN);

    // One press explains and asks again; nothing moves.
    app.clear_speech();
    select::<BusinessStatusState>(&mut app, "Go back to company driving");
    assert_eq!(profile(&app).business_status, LEASED_OWNER_OPERATOR);
    let said = app.main_lines().join(" ");
    assert!(said.contains("Press Enter again"), "{said}");
    let rows = labels::<BusinessStatusState>(&app);
    assert!(
        rows.iter()
            .any(|t| t == "Go back to company driving: press Enter again to confirm"),
        "{rows:?}"
    );

    app.clear_speech();
    select::<BusinessStatusState>(&mut app, "Go back to company driving");

    assert_eq!(profile(&app).business_status, COMPANY_DRIVER);
    assert!(profile(&app).owned_trucks.is_empty());
    assert!(profile(&app).visible_owned_trucks().is_empty());
    approx(profile(&app).money(), money_after_buy_in + expected_back);
    assert_eq!(profile(&app).driving_record.repossessions, 0);
    assert!(
        profile(&app).owner_operator_declined,
        "returned company drivers use company rank titles and goals"
    );
    let said = app.main_lines().join(" ");
    assert!(said.contains("company driver again"), "{said}");
    let rows = labels::<BusinessStatusState>(&app);
    assert!(
        !rows
            .iter()
            .any(|t| t.contains("Go back to company driving")),
        "{rows:?}"
    );
    assert!(
        rows.iter()
            .any(|t| t.contains("Buy into leased-on owner-operator")),
        "{rows:?}"
    );
}
