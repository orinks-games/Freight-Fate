//! Buying a buff at a rest stop: the fatigue lift, the one-at-a-time rule,
//! the free shower, who pays for rig care, and the Big Buck's gate (the
//! rest-stop half of `tests/test_buffs.py`; the catalog is in
//! `crates/ff-core/src/data/buffs.rs`, the timed entry and the wear
//! multipliers in the profile and vehicle test modules).
//!
//! Python called `RestStopState._buy_buff(BUFF_CATALOG[id])` directly; the
//! rows are private here, so each purchase goes through the menu row a
//! player would land on -- which is the same code path plus its label.

use crate::states_driving_menus_support::*;
use ff_core::models::business::{COMPANY_DRIVER, LEASED_OWNER_OPERATOR};
use ff_core::sim::stimulant::Loan;
use ff_core::sim::trip_models::RoadStop;
use freight_fate::app::testing::TestApp;
use freight_fate::app::{GameContext, SharedState};
use freight_fate::states::base::Menu;
use freight_fate::states::driving::DrivingState;
use freight_fate::states::driving_menu_states::DriveRef;
use freight_fate::states::driving_rest_states::{
    LoyaltyRewardsState, ParkingFullState, RestStopState,
};

/// Activate the row whose HELP contains `needle`.
///
/// The fuel row is found this way rather than by its label, because the
/// label changes face with the tank ("Fuel: tank is full" against "Refuel
/// 110 gallons for ...") -- see
/// `test_rest_stop_fuel_row_reads_the_same_tank_as_the_lot_screen` below.
fn activate_by_help<M: Menu>(state: &mut M, ctx: &mut GameContext, needle: &str) {
    let items = state.build_items(ctx);
    let found = items
        .iter()
        .find(|item| item.help_text(state, ctx).contains(needle))
        .cloned();
    match found {
        Some(item) => (item.action)(state, ctx),
        None => panic!("no row whose help mentions {needle:?}"),
    }
}

/// `tests/test_buffs.py::_driving`: a Denver -> Salt Lake City run under a
/// chosen business status.
fn buff_drive(app: &mut TestApp, business_status: &str) -> SharedState {
    let drive = a_drive_between(app, "Denver", "Salt Lake City", "Buff Tester");
    let p = app.ctx.profile.as_mut().expect("a career");
    p.business_status = business_status.to_string();
    if business_status != COMPANY_DRIVER {
        p.owned_trucks = vec!["rig".to_string()];
    }
    drive
}

/// `tests/test_buffs.py::_stop`.
fn buff_stop(drive: &SharedState, name: &str, actions: &[&str]) -> RoadStop {
    let at_mi = with_drive(drive, |d| d.trip.position_mi);
    let mut stop = RoadStop::new(name, at_mi, "travel_center");
    stop.actions = actions.iter().map(|a| a.to_string()).collect();
    stop.parking = "limited".to_string();
    stop
}

fn rest_stop(drive: &SharedState, name: &str, actions: &[&str]) -> RestStopState {
    RestStopState::with_drive(DriveRef::of(drive), buff_stop(drive, name, actions), false)
}

fn money(app: &TestApp) -> f64 {
    app.ctx.profile.as_ref().expect("a career").money()
}

/// The timed buffs, without the stimulant loan an energy drink or coffee
/// keeps beside them.
fn timed_buffs(p: &ff_core::models::profile::Profile) -> Vec<&serde_json::Value> {
    p.active_buffs
        .iter()
        .filter(|entry| !Loan::is_entry(entry))
        .collect()
}

fn approx(a: f64, b: f64) {
    assert!((a - b).abs() < 0.01, "{a} != {b}");
}

#[test]
fn test_energy_drink_lifts_fatigue_and_starts_a_timed_buff() {
    let mut app = TestApp::new();
    let drive = buff_drive(&mut app, LEASED_OWNER_OPERATOR);
    {
        let p = app.ctx.profile.as_mut().expect("a career");
        p.set_money(1_000.0);
        p.fatigue = 50.0;
    }
    let mut state = rest_stop(&drive, "Cactus Flats Truck Stop", &["fuel", "break"]);
    let minutes_before = with_drive(&drive, |d| d.trip.game_minutes);
    app.clear_speech();

    activate(&mut state, &mut app.ctx, "Energy drink");

    let p = app.ctx.profile.as_ref().expect("a career");
    approx(p.money(), 994.0);
    approx(p.fatigue, 47.0);
    let buffs = timed_buffs(p);
    assert_eq!(buffs.len(), 1);
    let entry = buffs[0];
    assert_eq!(entry["id"], "energy_drink");
    approx(entry["rate"].as_f64().unwrap_or(0.0), 0.85);
    approx(
        with_drive(&drive, |d| d.trip.game_minutes),
        minutes_before + 5.0,
    );
    let said = app.main_lines();
    assert!(
        said.last().is_some_and(|line| line.contains("sharper")),
        "{said:?}"
    );
}

#[test]
fn test_new_fatigue_buff_replaces_the_old_one() {
    let mut app = TestApp::new();
    let drive = buff_drive(&mut app, LEASED_OWNER_OPERATOR);
    app.ctx
        .profile
        .as_mut()
        .expect("a career")
        .set_money(1_000.0);
    let mut state = rest_stop(&drive, "Cactus Flats", &["fuel", "food"]);

    activate(&mut state, &mut app.ctx, "Energy drink");
    activate(&mut state, &mut app.ctx, "Diner meal");

    let ids: Vec<&str> = app
        .ctx
        .profile
        .as_ref()
        .expect("a career")
        .active_buffs
        .iter()
        .map(|entry| entry["id"].as_str().unwrap_or(""))
        .collect();
    assert_eq!(ids, ["diner_meal"]);
}

#[test]
fn test_shower_is_free_after_fueling_at_pilot() {
    let mut app = TestApp::new();
    let drive = buff_drive(&mut app, LEASED_OWNER_OPERATOR);
    app.ctx
        .profile
        .as_mut()
        .expect("a career")
        .set_money(5_000.0);
    with_drive(&drive, |d| d.trip.truck.fuel_gal = 40.0);
    let mut state = rest_stop(&drive, "Pilot Travel Center", &["fuel", "break"]);

    let priced = row(&mut state, &mut app.ctx, "Shower").text(&state, &app.ctx);
    assert_eq!(priced, "Shower: 15 dollars", "{priced}");
    activate_by_help(&mut state, &mut app.ctx, "Fills the tank");
    let free = row(&mut state, &mut app.ctx, "Shower").text(&state, &app.ctx);
    assert!(free.contains("free with your fuel purchase"), "{free}");

    let money_after_fuel = money(&app);
    activate(&mut state, &mut app.ctx, "Shower");
    approx(money(&app), money_after_fuel);
    assert_eq!(
        app.ctx.profile.as_ref().expect("a career").active_buffs[0]["id"],
        "shower"
    );
}

/// Leaving the stop menu and opening it again with T is the same visit: the
/// fuel bought on the first screen still pays for the shower on the second
/// (tester report, 2026-09-28).
#[test]
fn test_free_shower_survives_reopening_the_stop_menu() {
    let mut app = TestApp::new();
    let drive = buff_drive(&mut app, LEASED_OWNER_OPERATOR);
    app.ctx
        .profile
        .as_mut()
        .expect("a career")
        .set_money(5_000.0);
    with_drive(&drive, |d| d.trip.truck.fuel_gal = 40.0);
    let stop = buff_stop(&drive, "Pilot Travel Center", &["fuel", "break"]);
    let mut first = RestStopState::with_drive(DriveRef::of(&drive), stop.clone(), false);
    activate_by_help(&mut first, &mut app.ctx, "Fills the tank");

    let mut reopened = RestStopState::with_drive(DriveRef::of(&drive), stop, false);
    let label = row(&mut reopened, &mut app.ctx, "Shower").text(&reopened, &app.ctx);
    assert!(label.contains("free with your fuel purchase"), "{label}");
    let money_before = money(&app);
    activate(&mut reopened, &mut app.ctx, "Shower");
    approx(money(&app), money_before);

    // The next stop down the road is a new visit and charges again.
    let later = buff_stop(&drive, "Pilot Travel Center", &["fuel", "break"]);
    let mut next = RestStopState::with_drive(
        DriveRef::of(&drive),
        RoadStop {
            at_mi: later.at_mi + 80.0,
            ..later
        },
        false,
    );
    let priced = row(&mut next, &mut app.ctx, "Shower").text(&next, &app.ctx);
    assert_eq!(priced, "Shower: 15 dollars", "{priced}");
}

/// Parked at the stop through a save and a reload, the fuel still pays for
/// the shower.
#[test]
fn test_the_stop_visit_survives_a_save_and_reload() {
    let mut app = TestApp::new();
    let drive = buff_drive(&mut app, LEASED_OWNER_OPERATOR);
    app.ctx
        .profile
        .as_mut()
        .expect("a career")
        .set_money(5_000.0);
    with_drive(&drive, |d| d.trip.truck.fuel_gal = 40.0);
    let stop = buff_stop(&drive, "Pilot Travel Center", &["fuel", "break"]);
    let mut state = RestStopState::with_drive(DriveRef::of(&drive), stop.clone(), false);
    activate_by_help(&mut state, &mut app.ctx, "Fills the tank");

    let snapshot = drive_and_ctx(&drive, &mut app, |d, ctx| d.snapshot(ctx));
    let mut reloaded =
        DrivingState::from_snapshot(&mut app.ctx, &snapshot).expect("the snapshot reloads");
    assert!(reloaded.stop_visit(&stop).fueled);
}

/// A shower credit pays for one shower at a stop that sells one; it used to
/// spend the credit and still charge full price (2026-09-28).
#[test]
fn test_a_shower_credit_makes_one_shower_free() {
    let mut app = TestApp::new();
    let drive = buff_drive(&mut app, LEASED_OWNER_OPERATOR);
    {
        let p = app.ctx.profile.as_mut().expect("a career");
        p.set_money(5_000.0);
        p.loyalty.shower_credits = 1;
    }
    // No shower sold at a Love's: the credit is not offered there, and a
    // driver holding one is not told they need more points.
    let loves = buff_stop(&drive, "Love's Travel Stop", &["fuel", "break"]);
    let mut desk = LoyaltyRewardsState::new(DriveRef::of(&drive), loves);
    let rows = build_labels(&mut desk, &mut app.ctx);
    assert_eq!(rows, vec!["Back to truck stop"]);

    let stop = buff_stop(&drive, "Pilot Travel Center", &["fuel", "break"]);
    let mut desk = LoyaltyRewardsState::new(DriveRef::of(&drive), stop.clone());
    activate(&mut desk, &mut app.ctx, "Use shower credit");
    assert_eq!(
        app.ctx
            .profile
            .as_ref()
            .expect("a career")
            .loyalty
            .shower_credits,
        0
    );

    let mut state = RestStopState::with_drive(DriveRef::of(&drive), stop, false);
    let label = row(&mut state, &mut app.ctx, "Shower").text(&state, &app.ctx);
    assert_eq!(label, "Shower: free with your loyalty reward");
    let before = money(&app);
    activate(&mut state, &mut app.ctx, "Shower");
    approx(money(&app), before);
    let label = row(&mut state, &mut app.ctx, "Shower").text(&state, &app.ctx);
    assert_eq!(label, "Shower: 15 dollars", "one credit, one shower");
}

#[test]
fn test_quick_lube_sets_a_trip_buff_and_carrier_pays_for_company_drivers() {
    let mut app = TestApp::new();
    let drive = buff_drive(&mut app, COMPANY_DRIVER);
    let money_before = money(&app);
    let mut state = rest_stop(&drive, "Speedco Truck Service", &["fuel", "break"]);
    app.clear_speech();

    activate(&mut state, &mut app.ctx, "Quick lube");

    approx(money(&app), money_before);
    let rate = with_drive(&drive, |d| {
        d.rig_buffs.get("engine").expect("an engine buff").rate
    });
    approx(rate, 0.75);
    let said = app.main_lines();
    assert!(
        said.last()
            .is_some_and(|line| line.to_lowercase().contains("carrier")),
        "{said:?}"
    );
    let snapshot = drive_and_ctx(&drive, &mut app, |d, ctx| d.snapshot(ctx));
    assert_eq!(snapshot["rig_buffs"]["engine"]["id"], "quick_lube");
}

#[test]
fn test_food_stays_personal_money_for_company_drivers() {
    let mut app = TestApp::new();
    let drive = buff_drive(&mut app, COMPANY_DRIVER);
    app.ctx.profile.as_mut().expect("a career").set_money(100.0);
    let mut state = rest_stop(&drive, "Cactus Flats", &["food"]);

    activate(&mut state, &mut app.ctx, "Diner meal");

    approx(money(&app), 100.0 - 18.0);
}

#[test]
fn test_big_bucks_buffs_require_running_bobtail() {
    let mut app = TestApp::new();
    let drive = buff_drive(&mut app, LEASED_OWNER_OPERATOR);
    let mut loaded = rest_stop(&drive, "Big Buck's Travel Center", &[]);
    let texts = build_labels(&mut loaded, &mut app.ctx);
    assert!(
        !texts.iter().any(|t| t.to_lowercase().contains("brisket")),
        "{texts:?}"
    );

    with_drive(&drive, |d| d.job.bobtail = true);
    let mut bobtail = rest_stop(&drive, "Big Buck's Travel Center", &[]);
    let texts = build_labels(&mut bobtail, &mut app.ctx);
    assert!(
        texts.iter().any(|t| t.to_lowercase().contains("brisket")),
        "{texts:?}"
    );
}

#[test]
fn test_wall_drug_park_only_sells_five_cent_coffee_and_free_ice_water() {
    let mut app = TestApp::new();
    let drive = buff_drive(&mut app, LEASED_OWNER_OPERATOR);
    {
        let p = app.ctx.profile.as_mut().expect("a career");
        p.set_money(10.0);
        p.fatigue = 40.0;
    }
    let mut state = rest_stop(&drive, "Wall Drug", &["park", "save", "break", "sleep"]);
    let texts = build_labels(&mut state, &mut app.ctx);
    assert!(
        texts
            .iter()
            .any(|t| t.contains("Five-cent coffee") && t.contains("5 cents")),
        "{texts:?}"
    );
    assert!(
        texts.iter().any(|t| t.starts_with("Free ice water: free")),
        "{texts:?}"
    );
    assert!(
        !texts.iter().any(|t| {
            let low = t.to_lowercase();
            low.contains("diesel") || low.contains("energy drink")
        }),
        "{texts:?}"
    );

    app.clear_speech();
    activate(&mut state, &mut app.ctx, "Free ice water");
    let p = app.ctx.profile.as_ref().expect("a career");
    approx(p.money(), 10.0);
    approx(p.fatigue, 39.0);
    assert_eq!(timed_buffs(p)[0]["id"], "wall_drug_free_ice_water");
    // Water is real relief, not a stimulant.
    assert!(!p.stimulant_loan().is_open());
    let said = app.main_lines();
    assert!(
        said.last().is_some_and(|line| {
            line.to_lowercase().contains("free ice water") && line.contains("Free.")
        }),
        "{said:?}"
    );

    app.clear_speech();
    activate(&mut state, &mut app.ctx, "Five-cent coffee");
    let p = app.ctx.profile.as_ref().expect("a career");
    approx(p.money(), 9.95);
    approx(p.fatigue, 37.0); // 39 - 2
    assert_eq!(timed_buffs(p)[0]["id"], "wall_drug_five_cent_coffee");
    assert!(p.stimulant_loan().is_open());
    let said = app.main_lines();
    assert!(
        said.last().is_some_and(|line| {
            line.to_lowercase().contains("five-cent coffee") && line.contains("5 cents")
        }),
        "{said:?}"
    );
}

// -- the fuel row reads the tank it is standing at -------------------------

/// The rest stop's fuel row used to read "Fuel: tank is full" on any tank.
///
/// `RestStopState::build_items` runs `rows()` inside
/// `self.driving.clone().call(...)`, which holds the drive's `RefCell`
/// borrow. `rows()` called `FuelPump::fuel_label(ctx)`, which reached the
/// drive a SECOND time through `DriveRef::with` -- a nested
/// `try_borrow_mut` on the same cell, which can only fail. `with` returning
/// `None` fell through to the "tank is full" string, so a nearly empty tank
/// was announced as a full one and the gallons and the price were never
/// spoken.
///
/// `fuel_label` now takes the already-borrowed drive, the shape the sibling
/// `tire_label(ctx, d)` two rows below always had. Python has no borrow to
/// lose -- `self.driving` is a plain reference -- so this was a port
/// regression, not shipped behaviour.
///
/// Same truck (40 of 150 gallons) at the same stop, on two screens that
/// build their rows differently: they have to agree.
#[test]
fn test_rest_stop_fuel_row_reads_the_same_tank_as_the_lot_screen() {
    let mut app = TestApp::new();
    let drive = buff_drive(&mut app, LEASED_OWNER_OPERATOR);
    with_drive(&drive, |d| d.trip.truck.fuel_gal = 40.0);

    let mut rest = rest_stop(&drive, "Pilot Travel Center", &["fuel", "break"]);
    let rest_rows = build_labels(&mut rest, &mut app.ctx);
    let rest_fuel = rest_rows
        .iter()
        .find(|row| row.starts_with("Fuel") || row.starts_with("Refuel"))
        .expect("a fuel row");

    let mut lot = ParkingFullState::with_drive(
        DriveRef::of(&drive),
        buff_stop(&drive, "Pilot Travel Center", &["fuel", "break"]),
    );
    let lot_rows = build_labels(&mut lot, &mut app.ctx);
    let lot_fuel = lot_rows
        .iter()
        .find(|row| row.starts_with("Fuel") || row.starts_with("Refuel"))
        .expect("a fuel row");

    // Same truck, same stop, same tank: the two screens must agree, and
    // both must name the gallons rather than claim a full tank.
    assert_eq!(rest_fuel, lot_fuel);
    // The dollar figure rides this session's fuel market, so pin the part
    // the bug erased: the gallons, the offer, and the weight after filling.
    assert!(
        rest_fuel.starts_with("Refuel 110 gallons for ")
            && rest_fuel.contains(" dollars. Full tank:")
            && rest_fuel.ends_with("the gross-weight limit"),
        "{rest_fuel:?}"
    );
}
