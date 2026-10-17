//! `states/driving_rest_states.rs` and `states/driving_pause_states.rs`: the
//! route stop and its fuel island, the loyalty desk, a full lot, the
//! emergency shoulder, the three roadside enforcement outcomes, and the
//! pause menu.
//!
//! Ported from `tests/test_road_services.py`, the rest-stop half of
//! `tests/test_pay_advance.py`, the menu half of
//! `tests/test_rest_stop_assist.py`, and the pause/resume cases of
//! `tests/test_trip_resume.py`.

use ff_core::models::business::{COMPANY_DRIVER, LEASED_OWNER_OPERATOR};
use ff_core::models::economy::{PAY_ADVANCE_ELIGIBLE_BELOW, PAY_ADVANCE_LIMIT};
use ff_core::sim::enforcement_posts::{EnforcementPost, KIND_FIXED_SCALE};
use ff_core::sim::hos;
use ff_core::sim::trip_models::RoadStop;
use freight_fate::controller::ControllerButton;

use ff_core::sim::roadside_inspection::{InspectionLevel, DECAL_VALID_HOURS, OUT_OF_SERVICE_FINE};
use freight_fate::app::testing::TestApp;
use freight_fate::app::{share, SharedState};
use freight_fate::states::base::{InputEvent, Menu};
use freight_fate::states::driving::DrivingState;
use freight_fate::states::driving_core::{
    FIELD_REPAIR_DAMAGE_PCT, INSPECTION_MIN, MECHANIC_WAIT_MIN, ROAD_BRAKE_COST_PER_PCT,
    ROAD_TIRE_COST_PER_PCT, ROAD_TIRE_SPECIALIST_COST_PER_PCT, WALK_AROUND_MIN, WAVE_THROUGH_MIN,
};
use freight_fate::states::driving_menu_states::DriveRef;
use freight_fate::states::driving_pause_states::{
    AbandonJobConfirmationState, PauseMenuState, ASSIGNED_REPOSITION_ABANDON_REPUTATION_PENALTY,
};
use freight_fate::states::driving_rest_states::{
    LoyaltyRewardsState, ParkingFullState, RestStopState, ShoulderSleepConfirmationState,
};
use std::rc::Rc;

use crate::states_driving_menus_support::*;

/// `_driving(app, business_status)` from `test_road_services.py`.
fn a_wear_drive(app: &mut TestApp, business_status: &str) -> freight_fate::app::SharedState {
    let drive = a_drive_between(app, "Denver", "Salt Lake City", "Road Wear");
    let profile = app.ctx.profile.as_mut().expect("a career");
    profile.business_status = business_status.to_string();
    if business_status != COMPANY_DRIVER {
        profile.owned_trucks = vec!["rig".to_string()];
    }
    drive
}

fn rest_stop_at(
    app: &mut TestApp,
    drive: &freight_fate::app::SharedState,
    stop: RoadStop,
) -> RestStopState {
    let _ = app;
    RestStopState::with_drive(DriveRef::of(drive), stop, false)
}

// -- which shop offers what --------------------------------------------------------------

#[test]
fn test_tire_brand_offers_tires_but_not_brakes() {
    let mut app = TestApp::new();
    let drive = a_wear_drive(&mut app, LEASED_OWNER_OPERATOR);
    let at = with_drive(&drive, |d| {
        d.trip.truck.tire_wear_pct = 20.0;
        d.trip.truck.brake_wear_pct = 20.0;
        d.trip.position_mi
    });
    let mut state = rest_stop_at(&mut app, &drive, travel_center("Love's Travel Stop", at));
    let rows = build_labels(&mut state, &mut app.ctx);
    assert!(
        rows.iter().any(|l| l.starts_with("Replace tires")),
        "{rows:?}"
    );
    assert!(!rows.iter().any(|l| l.starts_with("Brake job")), "{rows:?}");
}

#[test]
fn test_full_service_brand_offers_brakes_and_marked_up_tires() {
    let mut app = TestApp::new();
    let drive = a_wear_drive(&mut app, LEASED_OWNER_OPERATOR);
    let at = with_drive(&drive, |d| {
        d.trip.truck.tire_wear_pct = 20.0;
        d.trip.truck.brake_wear_pct = 30.0;
        d.trip.position_mi
    });
    let mut state = rest_stop_at(
        &mut app,
        &drive,
        travel_center("TA Petro Travel Center", at),
    );
    let rows = build_labels(&mut state, &mut app.ctx);
    let tire_cost = 20.0 * ROAD_TIRE_COST_PER_PCT;
    let brake_cost = 30.0 * ROAD_BRAKE_COST_PER_PCT;
    assert!(
        rows.contains(&format!(
            "Replace tires: 20 percent wear for {} dollars",
            ff_core::pyfmt::fmt_grouped(tire_cost, 0)
        )),
        "{rows:?}"
    );
    assert!(
        rows.contains(&format!(
            "Brake job: 30 percent wear for {} dollars",
            ff_core::pyfmt::fmt_grouped(brake_cost, 0)
        )),
        "{rows:?}"
    );
}

#[test]
fn test_tire_specialist_beats_general_travel_center_price() {
    let mut app = TestApp::new();
    let drive = a_wear_drive(&mut app, LEASED_OWNER_OPERATOR);
    let at = with_drive(&drive, |d| {
        d.trip.truck.tire_wear_pct = 20.0;
        d.trip.position_mi
    });
    let mut specialist = rest_stop_at(&mut app, &drive, travel_center("Speedco Truck Service", at));
    let mut general = rest_stop_at(&mut app, &drive, travel_center("Pilot Travel Center", at));
    let cheap = 20.0 * ROAD_TIRE_SPECIALIST_COST_PER_PCT;
    let marked_up = 20.0 * ROAD_TIRE_COST_PER_PCT;
    assert!(
        build_labels(&mut specialist, &mut app.ctx).contains(&format!(
            "Replace tires: 20 percent wear for {} dollars",
            ff_core::pyfmt::fmt_grouped(cheap, 0)
        ))
    );
    assert!(build_labels(&mut general, &mut app.ctx).contains(&format!(
        "Replace tires: 20 percent wear for {} dollars",
        ff_core::pyfmt::fmt_grouped(marked_up, 0)
    )));
}

#[test]
fn test_generic_stop_and_big_bucks_offer_no_wear_service() {
    let mut app = TestApp::new();
    let drive = a_wear_drive(&mut app, LEASED_OWNER_OPERATOR);
    let at = with_drive(&drive, |d| {
        d.trip.truck.tire_wear_pct = 40.0;
        d.trip.truck.brake_wear_pct = 40.0;
        d.trip.position_mi
    });
    for name in ["Cactus Flats Truck Stop", "Big Buck's Travel Center"] {
        let mut state = rest_stop_at(&mut app, &drive, travel_center(name, at));
        let rows = build_labels(&mut state, &mut app.ctx);
        assert!(
            !rows.iter().any(|l| l.starts_with("Replace tires")),
            "{name}: {rows:?}"
        );
        assert!(
            !rows.iter().any(|l| l.starts_with("Brake job")),
            "{name}: {rows:?}"
        );
    }
}

// -- paying for the work -----------------------------------------------------------------

#[test]
fn test_road_tire_service_charges_and_clears_wear() {
    let mut app = TestApp::new();
    let drive = a_wear_drive(&mut app, LEASED_OWNER_OPERATOR);
    app.ctx
        .profile
        .as_mut()
        .expect("a career")
        .set_money(5_000.0);
    let (at, minutes_before) = with_drive(&drive, |d| {
        d.trip.truck.tire_wear_pct = 20.0;
        (d.trip.position_mi, d.trip.game_minutes)
    });
    let mut state = rest_stop_at(&mut app, &drive, travel_center("Love's Travel Stop", at));
    app.clear_speech();
    activate(&mut state, &mut app.ctx, "Replace tires");

    assert_eq!(with_drive(&drive, |d| d.trip.truck.tire_wear_pct), 0.0);
    let profile = app.ctx.profile.as_ref().expect("a career");
    // synced through store_truck_condition
    assert_eq!(profile.tire_wear_pct(), 0.0);
    assert!(
        (profile.money() - (5_000.0 - 20.0 * ROAD_TIRE_SPECIALIST_COST_PER_PCT)).abs() < 0.01,
        "{}",
        profile.money()
    );
    assert!(with_drive(&drive, |d| d.trip.game_minutes) > minutes_before);
    assert!(last(&app).contains("Tires replaced"), "{}", last(&app));
}

#[test]
fn test_road_brake_job_is_all_or_nothing_when_broke() {
    let mut app = TestApp::new();
    let drive = a_wear_drive(&mut app, LEASED_OWNER_OPERATOR);
    app.ctx.profile.as_mut().expect("a career").set_money(100.0);
    let at = with_drive(&drive, |d| {
        d.trip.truck.brake_wear_pct = 30.0;
        d.trip.position_mi
    });
    let mut state = rest_stop_at(&mut app, &drive, travel_center("Petro Stopping Center", at));
    app.clear_speech();
    activate(&mut state, &mut app.ctx, "Brake job");

    assert_eq!(with_drive(&drive, |d| d.trip.truck.brake_wear_pct), 30.0);
    assert_eq!(app.ctx.profile.as_ref().expect("a career").money(), 100.0);
    assert!(last(&app).contains("cannot afford"), "{}", last(&app));
}

#[test]
fn test_company_driver_road_wear_service_is_carrier_billed() {
    let mut app = TestApp::new();
    let drive = a_wear_drive(&mut app, COMPANY_DRIVER);
    let money_before = app.ctx.profile.as_ref().expect("a career").money();
    let at = with_drive(&drive, |d| {
        d.trip.truck.brake_wear_pct = 30.0;
        d.trip.position_mi
    });
    let mut state = rest_stop_at(&mut app, &drive, travel_center("TA Travel Center", at));
    app.clear_speech();
    activate(&mut state, &mut app.ctx, "Brake job");

    assert_eq!(with_drive(&drive, |d| d.trip.truck.brake_wear_pct), 0.0);
    assert_eq!(
        app.ctx.profile.as_ref().expect("a career").money(),
        money_before
    );
    assert!(last(&app).contains("carrier account"), "{}", last(&app));
}

#[test]
fn test_a_weigh_station_offers_no_bed_and_no_motel() {
    // Nobody sleeps in an active inspection facility, and there is no motel
    // on the far side of the platform. The scale menu was the generic
    // truck-stop template -- lot sleep, a 95 dollar motel room, a loyalty
    // readout -- at an open scale (owner playtest, 2026-08-20).
    let mut app = TestApp::new();
    let drive = a_wear_drive(&mut app, LEASED_OWNER_OPERATOR);
    let at = with_drive(&drive, |d| d.trip.position_mi);
    let mut scale = travel_center("I-24 Weigh Station", at);
    scale.stop_type = "weigh_station".to_string();
    scale.actions = Vec::new();
    let mut state = rest_stop_at(&mut app, &drive, scale);
    let rows = build_labels(&mut state, &mut app.ctx);
    assert!(!rows.iter().any(|l| l.contains("Sleep")), "{rows:?}");
    assert!(!rows.iter().any(|l| l.contains("Motel")), "{rows:?}");
    assert!(!rows.iter().any(|l| l.contains("Loyalty")), "{rows:?}");
}

#[test]
fn test_only_hospitality_and_parking_stops_earn_the_truck_stop_achievement() {
    for (stop_type, should_earn) in [
        ("truck_stop", true),
        ("travel_center", true),
        ("fuel_station", true),
        ("service_plaza", true),
        ("public_rest_area", true),
        ("truck_parking", true),
        ("weigh_station", false),
        ("repair_shop", false),
    ] {
        let mut app = TestApp::new();
        let drive = a_wear_drive(&mut app, LEASED_OWNER_OPERATOR);
        let at = with_drive(&drive, |d| {
            d.trip.truck.velocity_mps = 0.0;
            d.trip.position_mi
        });
        let mut stop = travel_center("Roadside facility", at);
        stop.stop_type = stop_type.to_string();
        drive_and_ctx(&drive, &mut app, |driving, ctx| {
            driving.open_poi_stop(ctx, &stop, false, None);
        });

        let earned = app
            .ctx
            .profile
            .as_ref()
            .expect("a career")
            .achievements
            .iter()
            .any(|id| id == "first_rest_stop");
        assert_eq!(earned, should_earn, "unexpected result for {stop_type}");
    }
}

#[test]
fn test_a_motel_bed_is_not_five_by_two() {
    // The badge is ten hours in the bunk; a motel room is the night you
    // specifically did not spend in it. Every sleep path used to award it
    // (owner report, 2026-08-20). The cramped-lot sleep keeps it -- the stop
    // has no beds, so a lot night is a bunk night.
    let mut app = TestApp::new();
    let drive = a_wear_drive(&mut app, LEASED_OWNER_OPERATOR);
    let at = with_drive(&drive, |d| d.trip.position_mi);
    let mut stop = travel_center("Roadside Stop", at);
    // no sleeper facility: motel and lot offered
    stop.actions = vec!["break".to_string()];
    let mut state = rest_stop_at(&mut app, &drive, stop);
    app.ctx
        .profile
        .as_mut()
        .expect("a career")
        .set_money(10_000.0);

    // A tired driver beds down in the truck's own bunk in the lot: that
    // night counts (the guard refuses a fresh driver an emergency sleep).
    {
        let profile = app.ctx.profile.as_mut().expect("a career");
        profile.hos.drive(600.0);
        profile.fatigue = 80.0;
        profile.achievements.clear();
    }
    activate(&mut state, &mut app.ctx, "Sleep 10 hours in the lot");
    activate(&mut state, &mut app.ctx, "Sleep 10 hours in the lot");
    assert!(
        app.ctx
            .profile
            .as_ref()
            .expect("a career")
            .achievements
            .iter()
            .any(|id| id == "slept_on_route"),
        "the lot night is a bunk night"
    );

    // A day later, the motel night must not award it.
    {
        let profile = app.ctx.profile.as_mut().expect("a career");
        profile.hos.drive(600.0);
        profile.fatigue = 80.0;
        profile.achievements.retain(|id| id != "slept_on_route");
    }
    activate(&mut state, &mut app.ctx, "Motel room");
    assert!(
        !app.ctx
            .profile
            .as_ref()
            .expect("a career")
            .achievements
            .iter()
            .any(|id| id == "slept_on_route"),
        "a motel bed is the night you did not spend in the bunk"
    );

    // The alternate motel offered when truck parking is full follows the
    // same rule. It must not turn a paid room into sleeper-berth sleep.
    {
        let profile = app.ctx.profile.as_mut().expect("a career");
        profile.hos.drive(600.0);
        profile.fatigue = 80.0;
        profile.achievements.retain(|id| id != "slept_on_route");
    }
    let mut full_lot =
        ParkingFullState::with_drive(DriveRef::of(&drive), travel_center("Prairie Plaza", at));
    activate(&mut full_lot, &mut app.ctx, "Motel room");
    assert!(
        !app.ctx
            .profile
            .as_ref()
            .expect("a career")
            .achievements
            .iter()
            .any(|id| id == "slept_on_route"),
        "a full-lot motel bed is not the truck's bunk"
    );
}

// -- the sleep guard ----------------------------------------------------------------------

#[test]
fn test_a_rested_driver_is_warned_once_before_a_pointless_sleep() {
    let mut app = TestApp::new();
    let drive = a_wear_drive(&mut app, LEASED_OWNER_OPERATOR);
    let at = with_drive(&drive, |d| d.trip.position_mi);
    let stop = sleep_stop(at);
    let mut state = rest_stop_at(&mut app, &drive, stop);
    {
        let profile = app.ctx.profile.as_mut().expect("a career");
        profile.fatigue = 0.0;
    }
    let before = with_drive(&drive, |d| d.trip.game_minutes);
    app.clear_speech();
    activate(&mut state, &mut app.ctx, "Sleep 10 hours");
    assert!(
        last(&app).starts_with("You are already rested"),
        "{}",
        last(&app)
    );
    assert_eq!(with_drive(&drive, |d| d.trip.game_minutes), before);

    // Pressing Enter again goes through. The sleep line is not necessarily
    // last: waking rested can earn a badge, which announces after it.
    app.clear_speech();
    activate(&mut state, &mut app.ctx, "Sleep 10 hours");
    assert!(with_drive(&drive, |d| d.trip.game_minutes) > before);
    let lines = app.main_lines();
    assert!(
        lines.iter().any(|line| line.contains("You slept 10 hours")),
        "{lines:?}"
    );
}

#[test]
fn test_moving_off_a_sleep_row_withdraws_the_pending_confirmation() {
    let mut app = TestApp::new();
    let drive = a_wear_drive(&mut app, LEASED_OWNER_OPERATOR);
    let at = with_drive(&drive, |d| d.trip.position_mi);
    let mut state = rest_stop_at(&mut app, &drive, sleep_stop(at));
    app.ctx.profile.as_mut().expect("a career").fatigue = 0.0;
    Menu::enter(&mut state, &mut app.ctx);
    activate(&mut state, &mut app.ctx, "Sleep 10 hours");
    let before = with_drive(&drive, |d| d.trip.game_minutes);
    // Arrowing away and back re-arms the warning rather than sleeping.
    state.move_by(&mut app.ctx, 1);
    app.clear_speech();
    activate(&mut state, &mut app.ctx, "Sleep 10 hours");
    assert!(
        last(&app).starts_with("You are already rested"),
        "{}",
        last(&app)
    );
    assert_eq!(with_drive(&drive, |d| d.trip.game_minutes), before);
}

#[test]
fn first_letter_navigation_withdraws_sleep_confirmation_before_selecting_again() {
    let mut app = TestApp::new();
    let drive = a_wear_drive(&mut app, LEASED_OWNER_OPERATOR);
    let at = with_drive(&drive, |d| d.trip.position_mi);
    let mut state = rest_stop_at(&mut app, &drive, sleep_stop(at));
    Menu::enter(&mut state, &mut app.ctx);
    activate(&mut state, &mut app.ctx, "Sleep 10 hours");
    let before_time = with_drive(&drive, |d| d.trip.game_minutes);
    let profile = app.ctx.profile.as_ref().unwrap();
    let before_hos = profile.hos.clone();
    let before_fatigue = profile.fatigue;
    let before_money = profile.money();

    state.first_letter_jump(&mut app.ctx, "t");
    assert_eq!(
        labels(&state, &app.ctx)[state.menu().index],
        "Take a 30-minute break"
    );
    for _ in 0..state.menu().items.len() {
        state.first_letter_jump(&mut app.ctx, "s");
        if labels(&state, &app.ctx)[state.menu().index] == "Sleep 10 hours" {
            break;
        }
    }
    assert_eq!(
        labels(&state, &app.ctx)[state.menu().index],
        "Sleep 10 hours"
    );
    app.clear_speech();
    Menu::activate(&mut state, &mut app.ctx);
    let said = app.main_lines().join(" ");
    assert!(said.contains("Sleep 10 hours at"), "{said}");
    assert_eq!(with_drive(&drive, |d| d.trip.game_minutes), before_time);
    let profile = app.ctx.profile.as_ref().unwrap();
    assert_eq!(profile.hos, before_hos);
    assert_eq!(profile.fatigue, before_fatigue);
    assert_eq!(profile.money(), before_money);
}

#[test]
fn controller_navigation_cancels_sleep_preview_and_a_second_select_confirms() {
    let mut app = TestApp::new();
    let drive = a_wear_drive(&mut app, LEASED_OWNER_OPERATOR);
    let at = with_drive(&drive, |d| d.trip.position_mi);
    let mut state = rest_stop_at(&mut app, &drive, sleep_stop(at));
    app.ctx.profile.as_mut().unwrap().fatigue = 40.0;
    Menu::enter(&mut state, &mut app.ctx);
    let sleep_index = labels(&state, &app.ctx)
        .iter()
        .position(|row| row == "Sleep 10 hours")
        .unwrap();
    state.jump(&mut app.ctx, sleep_index);
    let before = with_drive(&drive, |d| d.trip.game_minutes);
    state.handle_controller(&mut app.ctx, &InputEvent::button(ControllerButton::A));
    assert_eq!(with_drive(&drive, |d| d.trip.game_minutes), before);
    state.handle_controller(
        &mut app.ctx,
        &InputEvent::button(ControllerButton::DPadDown),
    );
    state.handle_controller(&mut app.ctx, &InputEvent::button(ControllerButton::DPadUp));
    app.clear_speech();
    state.handle_controller(&mut app.ctx, &InputEvent::button(ControllerButton::A));
    assert_eq!(with_drive(&drive, |d| d.trip.game_minutes), before);
    let said = app.main_lines().join(" ");
    assert!(said.contains("Select again to sleep"), "{said}");
    state.handle_controller(&mut app.ctx, &InputEvent::button(ControllerButton::A));
    assert!(with_drive(&drive, |d| d.trip.game_minutes) > before);
}

#[test]
fn test_prefer_sleep_lands_the_cursor_on_the_first_sleep_row() {
    let mut app = TestApp::new();
    let drive = a_wear_drive(&mut app, LEASED_OWNER_OPERATOR);
    let at = with_drive(&drive, |d| d.trip.position_mi);
    let mut state = RestStopState::with_drive(DriveRef::of(&drive), sleep_stop(at), true);
    Menu::enter(&mut state, &mut app.ctx);
    let rows = labels(&state, &app.ctx);
    assert_eq!(rows[state.menu().index], "Sleep 10 hours");
    for hours in [2, 3, 7, 8] {
        assert!(
            rows.contains(&format!("Sleep {hours} hours in sleeper berth")),
            "{rows:?}"
        );
    }
    assert!(rows.contains(&"Sleep 10 hours".to_string()), "{rows:?}");
}

// -- the loyalty desk ----------------------------------------------------------------------

#[test]
fn test_the_loyalty_row_opens_the_rewards_desk() {
    let mut app = TestApp::new();
    let drive = a_wear_drive(&mut app, LEASED_OWNER_OPERATOR);
    let at = with_drive(&drive, |d| d.trip.position_mi);
    let mut state = rest_stop_at(&mut app, &drive, travel_center("Love's Travel Stop", at));
    let rows = build_labels(&mut state, &mut app.ctx);
    assert!(rows[0].starts_with("Loyalty program"), "{rows:?}");
    activate(&mut state, &mut app.ctx, "Loyalty program");
    assert!(top_is::<LoyaltyRewardsState>(&app));
    let desk_rows = with_top_ctx::<LoyaltyRewardsState, _>(&mut app, build_labels);
    // A Love's sells no shower, so there is nothing here to need points for.
    assert_eq!(desk_rows, vec!["Back to truck stop"]);
}

// -- the pay advance ------------------------------------------------------------------------

#[test]
fn test_rest_stop_pay_advance_option_only_appears_when_available() {
    let mut app = TestApp::new();
    let drive = a_drive_between(&mut app, "New York", "Philadelphia", "Advance Test");
    let at = with_drive(&drive, |d| d.trip.position_mi);
    let mut stop = RoadStop::new("Example Service Plaza", at + 10.0, "service_plaza");
    stop.actions = ["park", "save"].iter().map(|a| a.to_string()).collect();
    stop.services = vec!["parking".to_string()];
    let mut state = rest_stop_at(&mut app, &drive, stop);

    app.ctx
        .profile
        .as_mut()
        .expect("a career")
        .set_money(PAY_ADVANCE_ELIGIBLE_BELOW);
    assert!(!build_labels(&mut state, &mut app.ctx)
        .iter()
        .any(|t| t.starts_with("Request pay advance")));

    app.ctx
        .profile
        .as_mut()
        .expect("a career")
        .set_money(PAY_ADVANCE_ELIGIBLE_BELOW - 1.0);
    assert!(build_labels(&mut state, &mut app.ctx)
        .iter()
        .any(|t| t.starts_with("Request pay advance")));

    activate(&mut state, &mut app.ctx, "Request pay advance");
    assert!(
        app.ctx
            .profile
            .as_ref()
            .expect("a career")
            .pay_advance_used_for_load
    );
    assert!(!build_labels(&mut state, &mut app.ctx)
        .iter()
        .any(|t| t.starts_with("Request pay advance")));

    {
        let profile = app.ctx.profile.as_mut().expect("a career");
        profile.pay_advance_used_for_load = false;
        profile.pay_advance = PAY_ADVANCE_LIMIT;
    }
    assert!(!build_labels(&mut state, &mut app.ctx)
        .iter()
        .any(|t| t.starts_with("Request pay advance")));
}

// -- the full lot -----------------------------------------------------------------------------

#[test]
fn test_a_full_lot_still_offers_the_pumps_first() {
    let mut app = TestApp::new();
    let drive = a_wear_drive(&mut app, LEASED_OWNER_OPERATOR);
    let at = with_drive(&drive, |d| d.trip.position_mi);
    let mut state =
        ParkingFullState::with_drive(DriveRef::of(&drive), travel_center("Prairie Plaza", at));
    let rows = build_labels(&mut state, &mut app.ctx);
    // Engine kill switch sits with the pumps: the island refuses a running
    // tractor, and the road's engine key is out of reach under this menu.
    assert!(
        rows[0] == "Shut down the engine" || rows[0] == "Start the engine",
        "{rows:?}"
    );
    assert!(
        rows[1].starts_with("Refuel ") || rows[1].starts_with("Fuel:"),
        "{rows:?}"
    );
    assert_eq!(rows[2], "Drive on to the next stop");
    assert!(rows[3].starts_with("Motel room:"), "{rows:?}");
    assert_eq!(rows[4], "Park on the shoulder and sleep");
}

#[test]
fn test_a_lot_with_no_pumps_leads_with_driving_on() {
    let mut app = TestApp::new();
    let drive = a_wear_drive(&mut app, LEASED_OWNER_OPERATOR);
    let at = with_drive(&drive, |d| d.trip.position_mi);
    let mut stop = travel_center("Prairie Plaza", at);
    stop.actions = vec!["park".to_string()];
    let mut state = ParkingFullState::with_drive(DriveRef::of(&drive), stop);
    let rows = build_labels(&mut state, &mut app.ctx);
    assert_eq!(rows[0], "Drive on to the next stop");
}

#[test]
fn test_the_shoulder_row_asks_before_it_sleeps() {
    let mut app = TestApp::new();
    let drive = a_wear_drive(&mut app, LEASED_OWNER_OPERATOR);
    let at = with_drive(&drive, |d| d.trip.position_mi);
    let mut state =
        ParkingFullState::with_drive(DriveRef::of(&drive), travel_center("Prairie Plaza", at));
    activate(&mut state, &mut app.ctx, "Park on the shoulder");
    assert!(top_is::<ShoulderSleepConfirmationState>(&app));
    let rows = with_top_ctx::<ShoulderSleepConfirmationState, _>(&mut app, |confirm, ctx| {
        build_labels(confirm, ctx)
    });
    assert_eq!(rows[0], "Cancel and keep looking for a safe stop");
    assert_eq!(rows[1], "Sleep on the shoulder anyway");
}

#[test]
fn test_shoulder_sleep_advances_ten_hours_and_resets_the_clock() {
    let mut app = TestApp::new();
    let drive = a_wear_drive(&mut app, LEASED_OWNER_OPERATOR);
    let at = with_drive(&drive, |d| {
        d.trip.truck.velocity_mps = 0.0;
        d.trip.position_mi
    });
    {
        let profile = app.ctx.profile.as_mut().expect("a career");
        profile.hos.drive(600.0);
        profile.fatigue = 90.0;
    }
    let before = with_drive(&drive, |d| d.trip.game_minutes);
    let mut state = ShoulderSleepConfirmationState::from_menu(
        DriveRef::of(&drive),
        "Nowhere to park.",
        Some(at),
    );
    app.clear_speech();
    activate(&mut state, &mut app.ctx, "Sleep on the shoulder anyway");

    assert_eq!(
        with_drive(&drive, |d| d.trip.game_minutes),
        before + hos::SLEEP_MIN
    );
    let profile = app.ctx.profile.as_ref().expect("a career");
    assert_eq!(profile.hos.driving_min, 0.0);
    // Poor rest: the shoulder floor, never fully fresh.
    assert_eq!(profile.fatigue, hos::FATIGUE_SHOULDER_FLOOR);
    assert!(
        last(&app).contains("You sleep poorly on the shoulder"),
        "{}",
        last(&app)
    );
}

// -- the pause menu -----------------------------------------------------------------------------

#[test]
fn test_pause_menu_lists_the_drive_controls() {
    let mut app = TestApp::new();
    let drive = a_drive(&mut app);
    let mut state = PauseMenuState::with_drive(DriveRef::of(&drive));
    let rows = build_labels(&mut state, &mut app.ctx);
    assert_eq!(rows[0], "Resume driving");
    for expected in [
        "Trip status",
        "Controls and help",
        "Learn game sounds",
        "Settings",
        "Drivers on duty",
        "Abandon job",
        "Quit to main menu",
    ] {
        assert!(
            rows.iter().any(|row| row == expected),
            "{expected:?} missing from {rows:?}"
        );
    }
}

#[test]
fn test_pausing_clears_the_queued_road_lines() {
    let mut app = TestApp::new();
    let drive = a_drive(&mut app);
    with_drive(&drive, |d| {
        d.pending_ambient_events.push_back(
            freight_fate::states::driving_core::PendingAmbient::new("a line from a mile back"),
        );
        d.reverse_cue_active = true;
        d.air_cue_active = true;
        d.jake_cue_key = Some("engine/jake_1".to_string());
    });
    let mut state = PauseMenuState::with_drive(DriveRef::of(&drive));
    Menu::enter(&mut state, &mut app.ctx);
    with_drive(&drive, |d| {
        assert!(d.pending_ambient_events.is_empty());
        assert!(!d.reverse_cue_active);
        assert!(!d.air_cue_active);
        assert_eq!(d.jake_cue_key, None);
    });
}

#[test]
fn test_resuming_says_so_and_brings_the_facility_names_back() {
    let mut app = TestApp::new();
    let drive = a_drive(&mut app);
    let mut state = PauseMenuState::with_drive(DriveRef::of(&drive));
    Menu::enter(&mut state, &mut app.ctx);
    app.ctx
        .push_shared_with(freight_fate::app::share(state), false);
    app.clear_speech();
    with_top_ctx::<PauseMenuState, _>(&mut app, |pause, ctx| {
        activate(pause, ctx, "Resume driving")
    });
    assert_eq!(last(&app), "Resumed.");
    // The pause menu is off the stack; the drive is back on top.
    assert!(top_is::<freight_fate::states::driving::DrivingState>(&app));
}

#[test]
fn test_abandon_confirmation_lands_on_no() {
    let mut app = TestApp::new();
    let drive = a_drive(&mut app);
    let mut state = AbandonJobConfirmationState::new(DriveRef::of(&drive));
    let rows = build_labels(&mut state, &mut app.ctx);
    assert_eq!(rows[0], "No, keep driving");
    assert_eq!(rows[1], "Yes, abandon the job");
}

#[test]
fn test_abandoning_a_load_costs_five_hundred_and_reputation() {
    let mut app = TestApp::new();
    let drive = a_drive(&mut app);
    let (money_before, rep_before) = {
        let profile = app.ctx.profile.as_mut().expect("a career");
        profile.set_money(4_000.0);
        (profile.money(), profile.career.reputation)
    };
    let mut state = AbandonJobConfirmationState::new(DriveRef::of(&drive));
    app.clear_speech();
    activate(&mut state, &mut app.ctx, "Yes, abandon the job");
    let profile = app.ctx.profile.as_ref().expect("a career");
    assert_eq!(profile.money(), money_before - 500.0);
    assert_eq!(profile.career.reputation, (rep_before - 5.0).max(0.0));
    assert!(profile.active_trip.is_none());
    assert!(last(&app).starts_with("Job abandoned."), "{}", last(&app));
}

#[test]
fn test_abandoning_an_assigned_reposition_costs_standing_not_money() {
    let mut app = TestApp::new();
    let drive = a_drive(&mut app);
    with_drive(&drive, |d| {
        d.job.bobtail = true;
        d.job.assigned = true;
    });
    let (money_before, rep_before) = {
        let profile = app.ctx.profile.as_mut().expect("a career");
        profile.set_money(4_000.0);
        (profile.money(), profile.career.reputation)
    };
    let mut state = AbandonJobConfirmationState::new(DriveRef::of(&drive));
    app.clear_speech();
    activate(&mut state, &mut app.ctx, "Yes, abandon the job");
    let profile = app.ctx.profile.as_ref().expect("a career");
    assert_eq!(profile.money(), money_before, "no fine on an empty run");
    assert_eq!(
        profile.career.reputation,
        (rep_before - ASSIGNED_REPOSITION_ABANDON_REPUTATION_PENALTY).max(0.0)
    );
    assert!(
        last(&app).starts_with("Dispatch assignment abandoned."),
        "{}",
        last(&app)
    );
}

// -- not portable without the harness --------------------------------------------------------

// `test_selected_stop_assist_reaches_full_stop_and_sleep_menu` and
// `test_overshoot_clears_assist_then_stopped_t_recovers` are live on the
// harness in `crates/freight-fate/tests/transcript_rest_stop_assist.rs`.

// `test_three_missed_microsleeps_force_a_stop` is live in
// `crates/freight-fate/tests/states_driving_updates.rs`.

#[test]
fn test_calling_the_mechanic_leaves_the_pause_menu_with_its_rows() {
    let mut app = TestApp::new();
    let drive = a_drive(&mut app);
    with_drive(&drive, |d| d.trip.truck.damage_pct = 60.0);
    let mut state = PauseMenuState::with_drive(DriveRef::of(&drive));
    Menu::enter(&mut state, &mut app.ctx);
    assert!(!labels(&state, &app.ctx).is_empty());

    activate(&mut state, &mut app.ctx, "Call a roadside mechanic");

    let rows = labels(&state, &app.ctx);
    assert!(
        rows.iter().any(|row| row == "Resume driving"),
        "the pause menu lost its rows: {rows:?}"
    );
    assert!(
        rows.iter()
            .any(|row| row == "Call a roadside mechanic: not needed yet"),
        "the mechanic row did not re-read the repaired truck: {rows:?}"
    );
}

#[test]
fn test_chains_are_not_offered_on_a_bare_road_with_no_chain_law() {
    // Owner, 2026-10-01: an October run into Chicago offered "Install snow
    // chains" at every pause. They ride in the side box until the road or the
    // law calls for them.
    let mut app = TestApp::new();
    let drive = a_drive(&mut app);
    {
        let profile = app.ctx.profile.as_mut().expect("a career");
        profile.set_chains_owned(true);
    }
    assert_eq!(with_drive(&drive, |d| d.trip.chain_law_level()), 0);
    let mut state = PauseMenuState::with_drive(DriveRef::of(&drive));
    Menu::enter(&mut state, &mut app.ctx);
    let rows = labels(&state, &app.ctx);
    assert!(
        !rows.iter().any(|row| row.contains("snow chains")),
        "{rows:?}"
    );
}

#[test]
fn test_hanging_chains_leaves_the_pause_menu_with_its_rows() {
    let mut app = TestApp::new();
    let drive = a_drive(&mut app);
    // Snow under the truck: the only place the row is offered.
    with_drive(&drive, |d| d.trip.truck.surface = "snow".to_string());
    {
        let profile = app.ctx.profile.as_mut().expect("a career");
        profile.set_chains_owned(true);
    }
    let mut state = PauseMenuState::with_drive(DriveRef::of(&drive));
    Menu::enter(&mut state, &mut app.ctx);
    assert!(labels(&state, &app.ctx)
        .iter()
        .any(|row| row.starts_with("Install snow chains")));

    activate(&mut state, &mut app.ctx, "Install snow chains");

    let rows = labels(&state, &app.ctx);
    assert!(
        rows.iter().any(|row| row == "Resume driving"),
        "the pause menu lost its rows: {rows:?}"
    );
    assert!(
        rows.iter().any(|row| row.starts_with("Remove snow chains")),
        "the chain row did not turn around: {rows:?}"
    );

    activate(&mut state, &mut app.ctx, "Remove snow chains");

    let rows = labels(&state, &app.ctx);
    assert!(
        rows.iter().any(|row| row == "Resume driving"),
        "the pause menu lost its rows: {rows:?}"
    );
    assert!(
        rows.iter()
            .any(|row| row.starts_with("Install snow chains")),
        "the chain row did not turn back: {rows:?}"
    );
}

#[test]
fn test_a_snapped_set_of_chains_is_not_offered_again_on_the_same_drive() {
    // Player report, 2026-10-14: in a Colorado chain zone the set snapped,
    // the pause menu offered it again, it snapped as soon as the truck
    // rolled, and round it went until the chain-law ticket. The profile's
    // wear meter only catches up at a stop, so it still read the set as whole.
    let mut app = TestApp::new();
    let drive = a_drive(&mut app);
    with_drive(&drive, |d| {
        d.trip.truck.surface = "snow".to_string();
        d.trip.truck.chain_wear_pct = 100.0;
    });
    {
        let profile = app.ctx.profile.as_mut().expect("a career");
        profile.set_chains_owned(true);
        profile.set_chain_wear_pct(0.0);
    }
    let mut state = PauseMenuState::with_drive(DriveRef::of(&drive));
    Menu::enter(&mut state, &mut app.ctx);
    let rows = labels(&state, &app.ctx);
    assert!(
        !rows.iter().any(|row| row.contains("snow chains")),
        "{rows:?}"
    );
}

/// A stopped drive with a chain set aboard at `wear`, on or off.
fn chain_drive(
    app: &mut TestApp,
    wear: f64,
    on: bool,
    links: i64,
) -> freight_fate::app::SharedState {
    let drive = a_drive(app);
    {
        let profile = app.ctx.profile.as_mut().expect("a career");
        profile.set_chains_owned(true);
        profile.set_chain_wear_pct(wear);
        profile.set_chain_repair_links(links);
    }
    with_drive(&drive, |d| {
        d.trip.truck.surface = "snow".to_string();
        d.trip.truck.chain_wear_pct = wear;
        d.trip.truck.chains_on = on;
        d.trip.truck.velocity_mps = 0.0;
    });
    drive
}

#[test]
fn test_a_broken_chain_is_repaired_with_a_spare_link() {
    let mut app = TestApp::new();
    let drive = chain_drive(&mut app, 100.0, false, 2);
    let before = with_drive(&drive, |d| d.trip.game_minutes);
    let mut state = PauseMenuState::with_drive(DriveRef::of(&drive));
    Menu::enter(&mut state, &mut app.ctx);
    let rows = labels(&state, &app.ctx);
    assert!(
        rows.iter()
            .any(|row| row == "Repair the broken chain: about 15 minutes"),
        "{rows:?}"
    );
    app.clear_speech();
    activate(&mut state, &mut app.ctx, "Repair the broken chain");
    assert_eq!(with_drive(&drive, |d| d.trip.truck.chain_wear_pct), 80.0);
    assert!(with_drive(&drive, |d| d.trip.truck.chains_on));
    // Under the slap threshold: no slap line straight after the repair.
    drive_and_ctx(&drive, &mut app, |d, ctx| d.update_traction_cues(ctx));
    app.ctx.run_deferred();
    assert!(
        !app.event_lines().iter().any(|s| s.contains("slapping")),
        "{:?}",
        app.event_lines()
    );
    assert_eq!(app.ctx.profile.as_ref().unwrap().chain_repair_links(), 1);
    let minutes = with_drive(&drive, |d| d.trip.game_minutes) - before;
    assert!((minutes - 15.0).abs() < 0.5, "{minutes}");
    assert!(
        app.main_lines()
            .iter()
            .any(|s| s.starts_with("Chain repaired with a spare link.")),
        "{:?}",
        app.main_lines()
    );
}

#[test]
fn test_no_repair_links_leaves_the_chain_off() {
    let mut app = TestApp::new();
    let drive = chain_drive(&mut app, 100.0, false, 0);
    let mut state = PauseMenuState::with_drive(DriveRef::of(&drive));
    Menu::enter(&mut state, &mut app.ctx);
    app.clear_speech();
    activate(&mut state, &mut app.ctx, "Repair the broken chain");
    assert_eq!(with_drive(&drive, |d| d.trip.truck.chain_wear_pct), 100.0);
    assert!(!with_drive(&drive, |d| d.trip.truck.chains_on));
    assert!(
        app.main_lines()
            .iter()
            .any(|s| s == "No repair links left. The chain stays off."),
        "{:?}",
        app.main_lines()
    );
}

#[test]
fn test_a_slapping_chain_warns_once_before_it_breaks() {
    let mut app = TestApp::new();
    let drive = chain_drive(&mut app, 84.0, true, 2);
    app.clear_speech();
    drive_and_ctx(&drive, &mut app, |d, ctx| d.update_traction_cues(ctx));
    assert!(app.event_lines().is_empty(), "{:?}", app.event_lines());
    with_drive(&drive, |d| d.trip.truck.chain_wear_pct = 86.0);
    drive_and_ctx(&drive, &mut app, |d, ctx| {
        d.update_traction_cues(ctx);
        d.update_traction_cues(ctx);
    });
    app.ctx.run_deferred();
    let slaps = app
        .event_lines()
        .into_iter()
        .filter(|s| s == "A chain is slapping the fender. Stop soon and tighten it.")
        .count();
    assert_eq!(slaps, 1);
    assert!(with_drive(&drive, |d| d.trip.truck.chains_on));
}

#[test]
fn test_tightening_a_slapping_chain_buys_back_wear_once() {
    let mut app = TestApp::new();
    let drive = chain_drive(&mut app, 90.0, true, 2);
    let mut state = PauseMenuState::with_drive(DriveRef::of(&drive));
    Menu::enter(&mut state, &mut app.ctx);
    activate(&mut state, &mut app.ctx, "Tighten chains");
    assert_eq!(with_drive(&drive, |d| d.trip.truck.chain_wear_pct), 85.0);
    let rows = labels(&state, &app.ctx);
    assert!(
        !rows.iter().any(|row| row.starts_with("Tighten chains")),
        "{rows:?}"
    );
}

/// Save the drive mid-run and resume it onto the stack, the way quitting and
/// continuing does, with the profile's meter still at its last-stop value.
fn resume_with_chain_wear(app: &mut TestApp, truck_wear: f64, profile_wear: f64) -> SharedState {
    let drive = a_drive(app);
    {
        let profile = app.ctx.profile.as_mut().expect("a career");
        profile.set_chains_owned(true);
        profile.set_chain_wear_pct(profile_wear);
    }
    let snap = drive_and_ctx(&drive, app, |d, ctx| {
        d.trip.truck.surface = "snow".to_string();
        d.trip.truck.chain_wear_pct = truck_wear;
        d.trip.truck.chains_on = false;
        d.snapshot(ctx)
    });
    assert_eq!(snap["chain_wear_pct"], truck_wear);
    let mut resumed =
        DrivingState::from_snapshot(&mut app.ctx, &snap).expect("the snapshot resumes");
    resumed.trip.truck.surface = "snow".to_string();
    let shared = share(resumed);
    app.ctx.push_shared_with(Rc::clone(&shared), false);
    shared
}

#[test]
fn test_a_snapped_set_stays_snapped_after_quit_and_resume() {
    let mut app = TestApp::new();
    let drive = resume_with_chain_wear(&mut app, 100.0, 40.0);
    assert_eq!(with_drive(&drive, |d| d.trip.truck.chain_wear_pct), 100.0);
    assert!(!with_drive(&drive, |d| d.trip.truck.chains_on));
    let mut state = PauseMenuState::with_drive(DriveRef::of(&drive));
    Menu::enter(&mut state, &mut app.ctx);
    let rows = labels(&state, &app.ctx);
    assert!(
        !rows
            .iter()
            .any(|row| row.starts_with("Install snow chains")),
        "{rows:?}"
    );
}

#[test]
fn test_chain_wear_since_the_last_stop_survives_quit_and_resume() {
    let mut app = TestApp::new();
    let drive = resume_with_chain_wear(&mut app, 40.0, 10.0);
    assert_eq!(with_drive(&drive, |d| d.trip.truck.chain_wear_pct), 40.0);
}

#[test]
fn test_an_older_save_without_chain_wear_keeps_the_profile_meter() {
    let mut app = TestApp::new();
    let drive = a_drive(&mut app);
    app.ctx
        .profile
        .as_mut()
        .expect("a career")
        .set_chain_wear_pct(40.0);
    let mut snap = drive_and_ctx(&drive, &mut app, |d, ctx| d.snapshot(ctx));
    snap.as_object_mut()
        .expect("a snapshot object")
        .remove("chain_wear_pct");
    let resumed = DrivingState::from_snapshot(&mut app.ctx, &snap).expect("the snapshot resumes");
    assert_eq!(resumed.trip.truck.chain_wear_pct, 40.0);
}

// -- a rebuild that cannot reach the drive ----------------------------------------------------
//
// When a rebuild misses the drive, these screens used to show nothing at all,
// which the menu speaks as "No options available." A player who has only the
// speech is then standing in a screen that says it has no rows and no way off
// it, mid-drive. They keep what they were already showing instead: at worst
// one label is an action out of date, and any keypress recovers from that.
//
// `unreachable_drive` explains why the miss here is not the nested borrow
// itself.

#[test]
fn test_the_route_stop_keeps_its_rows_when_a_rebuild_misses_the_drive() {
    let mut app = TestApp::new();
    let drive = a_wear_drive(&mut app, LEASED_OWNER_OPERATOR);
    let at = with_drive(&drive, |d| d.trip.position_mi);
    let stop = travel_center("Pilot Travel Center", at);

    let mut state = rest_stop_at(&mut app, &drive, stop.clone());
    let showing = state.build_items(&mut app.ctx);
    assert!(!showing.is_empty());

    let mut stranded = RestStopState::with_drive(unreachable_drive(), stop, false);
    let rows = rows_with_the_drive_out_of_reach(&mut stranded, showing, &mut app.ctx);
    assert!(
        rows.iter().any(|row| row == "Back to the road"),
        "the route stop lost the row that leaves it: {rows:?}"
    );
}

#[test]
fn test_a_full_lot_keeps_its_rows_when_a_rebuild_misses_the_drive() {
    let mut app = TestApp::new();
    let drive = a_wear_drive(&mut app, LEASED_OWNER_OPERATOR);
    let at = with_drive(&drive, |d| d.trip.position_mi);
    let stop = travel_center("Prairie Plaza", at);

    let mut state = ParkingFullState::with_drive(DriveRef::of(&drive), stop.clone());
    let showing = state.build_items(&mut app.ctx);
    assert!(!showing.is_empty());

    let mut stranded = ParkingFullState::with_drive(unreachable_drive(), stop);
    let rows = rows_with_the_drive_out_of_reach(&mut stranded, showing, &mut app.ctx);
    assert!(
        rows.iter().any(|row| row == "Drive on to the next stop"),
        "the full lot lost the row that leaves it: {rows:?}"
    );
}

#[test]
fn test_the_pause_menu_keeps_its_rows_when_a_rebuild_misses_the_drive() {
    let mut app = TestApp::new();
    let drive = a_drive(&mut app);

    let mut state = PauseMenuState::with_drive(DriveRef::of(&drive));
    let showing = state.build_items(&mut app.ctx);
    assert!(!showing.is_empty());

    let mut stranded = PauseMenuState::with_drive(unreachable_drive());
    let rows = rows_with_the_drive_out_of_reach(&mut stranded, showing, &mut app.ctx);
    assert!(
        rows.iter().any(|row| row == "Resume driving"),
        "the pause menu lost the row that returns to the wheel: {rows:?}"
    );
}

fn a_scale_stop(at_mi: f64) -> RoadStop {
    let mut stop = RoadStop::new("I-90 West Scale", at_mi, "weigh_station");
    stop.actions = vec!["inspect".to_string()];
    stop.parking = "none".to_string();
    stop
}

/// Open today: a check-in at a closed scale only says it is closed.
fn open_the_scale(drive: &freight_fate::app::SharedState, stop: &RoadStop) {
    with_drive(drive, |d| {
        let mut post = EnforcementPost::new(stop.at_mi, KIND_FIXED_SCALE);
        post.anchor = stop.key();
        d.trip.posts = vec![post];
    });
}

/// Back to the road at an open scale runs the check-in first (owner ruling,
/// 2026-09-28): leaving used to skip the inspection.
#[test]
fn test_leaving_an_open_scale_runs_the_check_in_first() {
    let mut app = TestApp::new();
    let drive = a_wear_drive(&mut app, COMPANY_DRIVER);
    let at = with_drive(&drive, |d| d.trip.position_mi);
    let stop = a_scale_stop(at);
    with_drive(&drive, |d| {
        let mut post = EnforcementPost::new(at, KIND_FIXED_SCALE);
        post.anchor = stop.key();
        d.trip.posts = vec![post];
    });
    let mut state = rest_stop_at(&mut app, &drive, stop.clone());
    build_labels(&mut state, &mut app.ctx);
    app.clear_speech();
    state.go_back(&mut app.ctx);
    let said = app.main_lines().join(" ");
    assert!(said.contains("Inspection check-in complete"), "{said}");
    assert!(said.to_lowercase().contains("parking brake set."), "{said}");
    assert!(with_drive(&drive, |d| d.stop_visit(&stop).inspected));
}

/// A closed scale has no lane to pull into: Back only leaves.
#[test]
fn test_leaving_a_closed_scale_only_leaves() {
    let mut app = TestApp::new();
    let drive = a_wear_drive(&mut app, COMPANY_DRIVER);
    let at = with_drive(&drive, |d| d.trip.position_mi);
    let stop = a_scale_stop(at);
    let before = with_drive(&drive, |d| d.trip.game_minutes);
    let mut state = rest_stop_at(&mut app, &drive, stop.clone());
    build_labels(&mut state, &mut app.ctx);
    app.clear_speech();
    state.go_back(&mut app.ctx);
    let said = app.main_lines().join(" ");
    assert!(!said.contains("Inspection check-in complete"), "{said}");
    assert!(said.to_lowercase().contains("parking brake set."), "{said}");
    assert!(!with_drive(&drive, |d| d.stop_visit(&stop).inspected));
    assert_eq!(with_drive(&drive, |d| d.trip.game_minutes), before);
}

/// The engine is whatever it already was: a running one is not "started".
#[test]
fn test_leaving_a_stop_with_the_engine_running_does_not_ask_to_start_it() {
    let mut app = TestApp::new();
    let drive = a_wear_drive(&mut app, COMPANY_DRIVER);
    with_drive(&drive, |d| {
        d.trip.truck.start_engine();
    });
    let at = with_drive(&drive, |d| d.trip.position_mi);
    let mut state = rest_stop_at(&mut app, &drive, travel_center("Love's Travel Stop", at));
    app.clear_speech();
    state.go_back(&mut app.ctx);
    let said = app.main_lines().join(" ");
    assert!(said.contains("Parking brake set."), "{said}");
    assert!(!said.contains("Engine off"), "{said}");
}

#[test]
fn test_scale_wave_through_is_two_minutes_not_fifteen() {
    assert_eq!(WAVE_THROUGH_MIN, 2.0);
    assert_eq!(INSPECTION_MIN, 15.0);
    let mut app = TestApp::new();
    let drive = a_wear_drive(&mut app, COMPANY_DRIVER);
    let at = with_drive(&drive, |d| d.trip.position_mi);
    let stop = a_scale_stop(at);
    open_the_scale(&drive, &stop);
    let selected = drive_and_ctx(&drive, &mut app, |d, ctx| {
        d.scale_selects_driver(ctx, &stop)
    });
    // A sound truck: the lane is the Level 1's own minutes and nothing more.
    with_drive(&drive, |d| {
        d.trip.truck.tire_wear_pct = 0.0;
        d.trip.truck.brake_wear_pct = 0.0;
        d.trip.truck.damage_pct = 0.0;
    });
    let before = with_drive(&drive, |d| d.trip.game_minutes);
    let mut state = rest_stop_at(&mut app, &drive, stop);
    activate(&mut state, &mut app.ctx, "Check in at inspection station");
    let after = with_drive(&drive, |d| d.trip.game_minutes);
    let expected = if selected {
        InspectionLevel::Full.minutes()
    } else {
        WAVE_THROUGH_MIN
    };
    assert!(
        (after - before - expected).abs() < 1e-6,
        "selected={selected} burned {} minutes, expected {expected}",
        after - before
    );
}

#[test]
fn test_scale_check_in_is_removed_after_one_completed_inspection() {
    let mut app = TestApp::new();
    let drive = a_wear_drive(&mut app, COMPANY_DRIVER);
    let at = with_drive(&drive, |d| d.trip.position_mi);
    let stop = a_scale_stop(at);
    open_the_scale(&drive, &stop);
    let mut state = rest_stop_at(&mut app, &drive, stop);

    activate(&mut state, &mut app.ctx, "Check in at inspection station");

    assert_eq!(
        labels(&state, &app.ctx),
        vec!["Walk around the truck", "Back to the road"]
    );
}

#[test]
fn test_a_targeted_record_takes_the_inspection_lane() {
    let mut app = TestApp::new();
    let drive = a_wear_drive(&mut app, COMPANY_DRIVER);
    {
        let p = app.ctx.profile.as_mut().expect("a career");
        p.career.reputation = 10.0;
        p.driving_record.citations = 6;
        p.driving_record.citation_times = vec![p.game_hours; 6];
        p.out_of_service_events = 3;
        p.driving_record.out_of_service_times = vec![p.game_hours; 3];
    }
    with_drive(&drive, |d| d.trip.truck.damage_pct = 70.0);
    let at = with_drive(&drive, |d| d.trip.position_mi);
    let stop = a_scale_stop(at);
    open_the_scale(&drive, &stop);
    let before = with_drive(&drive, |d| d.trip.game_minutes);
    let mut state = rest_stop_at(&mut app, &drive, stop);
    app.clear_speech();
    activate(&mut state, &mut app.ctx, "Check in at inspection station");
    let after = with_drive(&drive, |d| d.trip.game_minutes);
    // Seventy percent damage is past the safe limit: the Level 1 parks the
    // truck until the roadside mechanic patches it.
    let expected = InspectionLevel::Full.minutes() + MECHANIC_WAIT_MIN;
    assert!(
        (after - before - expected).abs() < 1e-6,
        "targeted record burned {} minutes, expected {expected}",
        after - before
    );
    let said = app.main_lines().join(" ");
    assert!(said.contains("inspection lane"), "{said}");
    assert!(said.contains("body damage past the safe limit"), "{said}");
    assert!(said.contains("Out of service until repaired"), "{said}");
    assert!(with_drive(&drive, |d| d.trip.truck.damage_pct) <= FIELD_REPAIR_DAMAGE_PCT);
    assert_eq!(
        app.ctx.profile.as_ref().unwrap().driving_record.citations,
        7
    );
}

#[test]
fn test_bald_tires_in_the_lane_are_out_of_service_until_replaced() {
    let mut app = TestApp::new();
    let drive = a_wear_drive(&mut app, COMPANY_DRIVER);
    {
        let p = app.ctx.profile.as_mut().expect("a career");
        p.career.reputation = 10.0;
        p.driving_record.citations = 6;
        p.driving_record.citation_times = vec![p.game_hours; 6];
        p.out_of_service_events = 3;
        p.driving_record.out_of_service_times = vec![p.game_hours; 3];
    }
    with_drive(&drive, |d| {
        d.trip.truck.tire_wear_pct = 95.0;
        d.trip.truck.brake_wear_pct = 0.0;
        d.trip.truck.damage_pct = 0.0;
    });
    let money_before = app.ctx.profile.as_ref().unwrap().money();
    let at = with_drive(&drive, |d| d.trip.position_mi);
    let stop = a_scale_stop(at);
    open_the_scale(&drive, &stop);
    let mut state = rest_stop_at(&mut app, &drive, stop);
    app.clear_speech();
    activate(&mut state, &mut app.ctx, "Check in at inspection station");
    let said = app.main_lines().join(" ");
    assert!(
        said.contains("a tire below the minimum tread depth"),
        "{said}"
    );
    assert!(said.contains("new tires"), "{said}");
    assert_eq!(with_drive(&drive, |d| d.trip.truck.tire_wear_pct), 0.0);
    // A company driver pays the fine; the carrier's breakdown account pays
    // the tires.
    let money_after = app.ctx.profile.as_ref().unwrap().money();
    assert!(
        (money_before - money_after - OUT_OF_SERVICE_FINE).abs() < 1e-6,
        "{money_before} -> {money_after}"
    );
}

#[test]
fn test_a_clean_level_one_earns_a_decal_that_waves_the_next_scale_through() {
    let mut app = TestApp::new();
    let drive = a_wear_drive(&mut app, COMPANY_DRIVER);
    with_drive(&drive, |d| {
        d.trip.truck.tire_wear_pct = 0.0;
        d.trip.truck.brake_wear_pct = 0.0;
        d.trip.truck.damage_pct = 0.0;
    });
    let outcome = drive_and_ctx(&drive, &mut app, |d, ctx| {
        let report = d.inspection_report(ctx, InspectionLevel::Full);
        assert!(report.clean(), "{:?}", report.findings);
        d.settle_inspection(ctx, &report)
    });
    assert!(
        outcome.contains("Clean Level 1 full inspection"),
        "{outcome}"
    );
    assert!(outcome.contains("inspection decal"), "{outcome}");
    let until = app
        .ctx
        .profile
        .as_ref()
        .unwrap()
        .driving_record
        .decal_until_h;
    assert!(until > DECAL_VALID_HOURS - 1.0, "{until}");

    let at = with_drive(&drive, |d| d.trip.position_mi);
    let stop = a_scale_stop(at);
    open_the_scale(&drive, &stop);
    let before = with_drive(&drive, |d| d.trip.game_minutes);
    let mut state = rest_stop_at(&mut app, &drive, stop);
    app.clear_speech();
    activate(&mut state, &mut app.ctx, "Check in at inspection station");
    let after = with_drive(&drive, |d| d.trip.game_minutes);
    assert!(
        (after - before - WAVE_THROUGH_MIN).abs() < 1e-6,
        "{}",
        after - before
    );
    let said = app.main_lines().join(" ");
    assert!(said.contains("decal on the windshield"), "{said}");
}

#[test]
fn test_the_walk_around_says_what_an_inspector_would_find_first() {
    let mut app = TestApp::new();
    let drive = a_wear_drive(&mut app, COMPANY_DRIVER);
    with_drive(&drive, |d| {
        d.trip.truck.tire_wear_pct = 92.0;
        d.trip.truck.brake_wear_pct = 80.0;
        d.trip.truck.damage_pct = 0.0;
    });
    let at = with_drive(&drive, |d| d.trip.position_mi);
    let stop = travel_center("Flying J", at);
    let before = with_drive(&drive, |d| d.trip.game_minutes);
    let mut state = rest_stop_at(&mut app, &drive, stop);
    app.clear_speech();
    activate(&mut state, &mut app.ctx, "Walk around the truck");
    let after = with_drive(&drive, |d| d.trip.game_minutes);
    assert!(
        (after - before - WALK_AROUND_MIN).abs() < 1e-6,
        "{}",
        after - before
    );
    let said = app.main_lines().join(" ");
    assert!(
        said.contains(
            "A tire below the minimum tread depth: an inspector would park you for this."
        ),
        "{said}"
    );
    assert!(
        said.contains("Brakes close to the adjustment limit: an inspector would write this up."),
        "{said}"
    );
}

// -- fuel island needs the tractor off --------------------------------------------------------

#[test]
fn test_refuel_refuses_while_the_tractor_engine_is_running() {
    let mut app = TestApp::new();
    let drive = a_wear_drive(&mut app, LEASED_OWNER_OPERATOR);
    with_drive(&drive, |d| {
        d.trip.truck.fuel_gal = 10.0;
        d.trip.truck.engine_on = true;
    });
    let at = with_drive(&drive, |d| d.trip.position_mi);
    let mut state = rest_stop_at(&mut app, &drive, travel_center("Pilot Travel Center", at));
    let before = with_drive(&drive, |d| d.trip.truck.fuel_gal);
    app.clear_speech();
    activate(&mut state, &mut app.ctx, "Refuel");
    let after = with_drive(&drive, |d| d.trip.truck.fuel_gal);
    assert_eq!(after, before, "tank must not fill with the engine running");
    assert!(
        with_drive(&drive, |d| d.trip.truck.engine_on),
        "refuse must not silently kill the engine"
    );
    assert_eq!(last(&app), "Shut the engine off before you fuel.");
}

#[test]
fn test_refuel_is_allowed_once_the_tractor_engine_is_off() {
    let mut app = TestApp::new();
    let drive = a_wear_drive(&mut app, LEASED_OWNER_OPERATOR);
    app.ctx
        .profile
        .as_mut()
        .expect("a career")
        .set_money(50_000.0);
    with_drive(&drive, |d| {
        d.trip.truck.fuel_gal = 10.0;
        d.trip.truck.engine_on = false;
    });
    let at = with_drive(&drive, |d| d.trip.position_mi);
    let mut state = rest_stop_at(&mut app, &drive, travel_center("Pilot Travel Center", at));
    app.clear_speech();
    activate(&mut state, &mut app.ctx, "Refuel");
    with_drive(&drive, |d| {
        assert_eq!(d.trip.truck.fuel_gal, d.trip.truck.specs.fuel_tank_gal);
    });
    assert!(
        app.main_lines()
            .iter()
            .any(|line| line.starts_with("Refueled ")),
        "{:?}",
        app.main_lines()
    );
}

#[test]
fn test_rest_stop_engine_row_shuts_down_so_fuel_can_proceed() {
    let mut app = TestApp::new();
    let drive = a_wear_drive(&mut app, LEASED_OWNER_OPERATOR);
    app.ctx
        .profile
        .as_mut()
        .expect("a career")
        .set_money(50_000.0);
    with_drive(&drive, |d| {
        d.trip.truck.fuel_gal = 10.0;
        d.trip.truck.engine_on = true;
        d.trip.truck.velocity_mps = 0.0;
    });
    let at = with_drive(&drive, |d| d.trip.position_mi);
    let mut state = rest_stop_at(&mut app, &drive, travel_center("Love's Travel Stop", at));
    let rows = build_labels(&mut state, &mut app.ctx);
    assert!(rows.iter().any(|r| r == "Shut down the engine"), "{rows:?}");
    app.clear_speech();
    activate(&mut state, &mut app.ctx, "Shut down the engine");
    assert!(!with_drive(&drive, |d| d.trip.truck.engine_on));
    assert_eq!(last(&app), "Engine off.");
    app.clear_speech();
    activate(&mut state, &mut app.ctx, "Refuel");
    with_drive(&drive, |d| {
        assert_eq!(d.trip.truck.fuel_gal, d.trip.truck.specs.fuel_tank_gal);
    });
}

#[test]
fn test_a_full_lot_refuses_fuel_while_the_engine_is_running() {
    let mut app = TestApp::new();
    let drive = a_wear_drive(&mut app, LEASED_OWNER_OPERATOR);
    with_drive(&drive, |d| {
        d.trip.truck.fuel_gal = 10.0;
        d.trip.truck.engine_on = true;
    });
    let at = with_drive(&drive, |d| d.trip.position_mi);
    let mut state =
        ParkingFullState::with_drive(DriveRef::of(&drive), travel_center("Prairie Plaza", at));
    let before = with_drive(&drive, |d| d.trip.truck.fuel_gal);
    app.clear_speech();
    activate(&mut state, &mut app.ctx, "Refuel");
    assert_eq!(with_drive(&drive, |d| d.trip.truck.fuel_gal), before);
    assert_eq!(last(&app), "Shut the engine off before you fuel.");
}

#[test]
fn test_a_break_burns_idle_fuel_only_with_the_engine_running() {
    for engine_on in [true, false] {
        let mut app = TestApp::new();
        let drive = a_wear_drive(&mut app, LEASED_OWNER_OPERATOR);
        let at = with_drive(&drive, |d| {
            d.trip.truck.engine_on = engine_on;
            d.trip.truck.fuel_gal = 100.0;
            d.trip.position_mi
        });
        let mut state = rest_stop_at(&mut app, &drive, travel_center("Love's Travel Stop", at));
        activate(&mut state, &mut app.ctx, "Take a 30-minute break");
        let fuel = with_drive(&drive, |d| d.trip.truck.fuel_gal);
        if engine_on {
            // About 0.8 gallons an hour at idle.
            assert!(fuel < 99.8 && fuel > 99.0, "{fuel}");
        } else {
            assert_eq!(fuel, 100.0);
        }
    }
}

/// One lamp, one citation. A player was written up for the same light at
/// post after post with no way to fix it before the next shop (report,
/// 2026-10-17).
#[test]
fn test_a_fault_already_written_up_is_not_cited_again_this_drive() {
    use ff_core::sim::enforcement_posts::{build_post, KIND_CMV};
    let mut app = TestApp::new();
    let (drive, defect) = a_drive_with_a_lamp_out(&mut app);
    with_drive(&drive, |d| {
        d.trip.truck.tire_wear_pct = 80.0; // past the citation line, short of parked
        d.trip.truck.brake_wear_pct = 0.0;
        d.trip.truck.damage_pct = 0.0;
    });
    let first = drive_and_ctx(&drive, &mut app, |d, ctx| {
        let report = d.inspection_report(ctx, InspectionLevel::WalkAround);
        assert!(
            report.findings.iter().any(|f| f.what == defect),
            "{:?}",
            report.findings
        );
        d.settle_inspection(ctx, &report)
    });
    assert!(first.contains("Fined"), "{first}");
    let money = app.ctx.profile.as_ref().unwrap().money();

    let (second, sample) = drive_and_ctx(&drive, &mut app, |d, ctx| {
        let report = d.inspection_report(ctx, InspectionLevel::Full);
        assert!(report.clean(), "{:?}", report.findings);
        let said = d.settle_inspection(ctx, &report);
        d.refresh_visible_trailer_defect(ctx);
        let post = build_post(
            d.trip.position_mi,
            KIND_CMV,
            0,
            None,
            1.0,
            "",
            Some(true),
            1.0,
        );
        (said, d.road_sample(&post))
    });
    assert!(second.contains("Nothing new to write up"), "{second}");
    assert!(!second.contains("decal"), "{second}");
    assert_eq!(app.ctx.profile.as_ref().unwrap().money(), money);
    assert!(
        sample.trailer_defect.is_empty(),
        "{:?}",
        sample.trailer_defect
    );
    assert_eq!(sample.tire_wear_pct, 0.0);

    // A fault that parks the truck still counts.
    let parked = drive_and_ctx(&drive, &mut app, |d, ctx| {
        d.trip.truck.tire_wear_pct = 95.0;
        d.inspection_report(ctx, InspectionLevel::WalkAround)
    });
    assert!(parked.out_of_service(), "{:?}", parked.findings);
}
