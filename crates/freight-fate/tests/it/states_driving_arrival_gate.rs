//! Arriving: the gate that will not open until the truck stops, the reminder
//! that keeps talking after an overshoot, what the trailer weighs on each leg
//! of a run, and the settlement a tolled route records.
//!
//! Ported from `tests/test_driving_features.py`:
//! `test_delivery_requires_parking_at_destination`,
//! `test_arrival_gate_repeats_after_overshoot`,
//! `test_cargo_mass_is_loaded_on_delivery_and_empty_on_pickup` and
//! `test_toll_route_delivery_settlement_records_expense`.

use ff_core::data::world::get_world;
use ff_core::models::business::{build_business_settlement, SettlementTerms};
use ff_core::models::jobs::{Job, CARGO_CATALOG};
use ff_core::models::profile::Profile;
use ff_core::sim::vehicle::KG_PER_TON;
use ff_core::sim::weather::WeatherKind;

use freight_fate::app::testing::TestApp;
use freight_fate::playtest::harness::{key_event, PlaytestHarness, StartDelivery};
use freight_fate::states::base::{Key, Menu, State};
use freight_fate::states::driving::DrivingState;
use freight_fate::states::driving_core::{DRIVE_PHASE_DELIVERY, DRIVE_PHASE_PICKUP};
use freight_fate::states::driving_menu_states::{
    settlement_hours, ArrivalState, FacilityArrivalState,
};

const DT: f64 = 1.0 / 60.0;
const DELIVERY_ACTIONS: [&str; 2] = [
    "Dock and deliver",
    "Drop the loaded trailer and hook an empty",
];

// -- rigging -------------------------------------------------------------------------

fn a_drive(name: &str) -> PlaytestHarness {
    let mut harness = PlaytestHarness::new();
    harness.start_delivery(StartDelivery::named(name));
    harness.with_drive(|drive, _| {
        drive.tutorial = None;
        drive.departure_checked = true;
        drive.trip.hazard_check_mi = 1e9;
        drive.trip.inspection_check_mi = 1e9;
        drive.trip.traffic_manager.rolling_bubble = false;
        drive.trip.set_npc_vehicles(Vec::new());
        drive.trip.traffic_pressures.clear();
        drive.trip.zones.retain(|z| z.aadt.is_none());
        drive.trip.weather.current = WeatherKind::Clear;
        // Somebody else's roadside log check is ambient colour landing
        // between the gate lines these cases read.
        drive.trip.set_patrols(Vec::new());
        drive.trip.posts.clear();
    });
    harness.clear_speech();
    harness
}

/// `driving_feature_helpers.mark_destination_exit_taken`.
fn mark_destination_exit_taken(drive: &mut DrivingState) {
    drive.destination_exit_taken = true;
    drive.trip.finished = true;
    drive.trip.position_mi = drive.trip.total_miles();
}

fn frame(harness: &mut PlaytestHarness) {
    harness.advance_clock(DT);
    harness.with_drive(|drive, ctx| drive.update_frame(ctx, DT));
}

/// The one event line containing `needle`, or a failure naming everything
/// that was said instead.
fn an_event_containing(harness: &PlaytestHarness, needle: &str) -> String {
    harness
        .app
        .event_lines()
        .into_iter()
        .find(|line| line.contains(needle))
        .unwrap_or_else(|| panic!("nothing said {needle:?}: {:#?}", harness.app.event_lines()))
}

fn last_main(harness: &PlaytestHarness) -> String {
    harness.app.main_lines().last().cloned().unwrap_or_default()
}

// -- the gate ---------------------------------------------------------------------------

#[test]
fn test_delivery_requires_parking_at_destination() {
    let mut harness = a_drive("Park To Deliver");
    harness.with_drive(|drive, _| {
        // Pinned to a receiver that unloads live: this case is about having
        // to stop before the dock will take you, and it reads the unload back.
        drive.job.destination_type = "mine_quarry".to_string();
        mark_destination_exit_taken(drive);
        drive.truck_mut().velocity_mps = 26.8;
    });
    harness.clear_speech();

    frame(&mut harness);

    assert!(harness.state_is::<DrivingState>());
    // Looked up rather than assumed last: whatever the posted limit is on the
    // route dispatch drew, sixty may be over it, and the speeding warning
    // that earns lands in the same frame.
    let said = an_event_containing(&harness, "Destination ahead");
    assert!(said.to_lowercase().contains("stop at the gate"), "{said}");
    let hud = harness.with_drive(|d, ctx| d.lines(ctx));
    assert!(
        hud.last()
            .is_some_and(|line| line.to_lowercase().contains("stop at the gate")),
        "{hud:#?}"
    );

    harness.with_drive(|drive, _| drive.truck_mut().velocity_mps = 0.0);
    frame(&mut harness);
    harness.finish_timed_state();

    assert!(harness.state_is::<FacilityArrivalState>());
    // Either delivery is a valid arrival: a dock if this receiver unloads
    // live, dropping the loaded box if they have a yard for it.
    let focused = harness.focused_label().expect("a focused row");
    assert!(DELIVERY_ACTIONS.contains(&focused.as_str()), "{focused}");
    let lines = harness
        .app
        .ctx
        .state()
        .expect("a state")
        .borrow()
        .lines(&harness.app.ctx);
    assert!(
        lines
            .iter()
            .any(|l| l == "Stopping required before delivery settlement."),
        "{lines:#?}"
    );
    assert_eq!(
        harness
            .app
            .ctx
            .profile
            .as_ref()
            .expect("a career")
            .career
            .deliveries,
        0
    );

    harness.clear_speech();
    harness.key(key_event(Key::Return, None));
    harness.finish_timed_state();

    assert!(harness.state_is::<ArrivalState>());
    assert!(
        harness
            .app
            .main_lines()
            .iter()
            .any(|line| line.contains("Unloading")),
        "{:#?}",
        harness.app.main_lines()
    );
}

#[test]
fn test_arrival_gate_repeats_after_overshoot() {
    // Rolling past the destination gate keeps the stop instruction alive.
    //
    // Regression for the 2026-07-22 playtest: the gate warnings latched after
    // one announcement, so a driver who overshot the entrance at speed -- with
    // cruise re-armed -- heard silence for six minutes and lost the on-time
    // bonus, with S still answering speed limits for a route that had already
    // ended.
    let mut harness = a_drive("Gate Overshoot");
    harness.with_drive(|drive, _| {
        mark_destination_exit_taken(drive);
        drive.truck_mut().velocity_mps = 26.8; // ~60 mph, blowing past the gate
    });
    harness.clear_speech();

    frame(&mut harness);
    assert!(harness.state_is::<DrivingState>());
    an_event_containing(&harness, "Destination ahead");
    let announced = harness.app.event_lines().len();

    // Inside the reminder interval the gate stays quiet.
    for _ in 0..30 {
        frame(&mut harness);
    }
    assert_eq!(harness.app.event_lines().len(), announced);

    // Interval elapsed and cruise re-armed: the reminder re-speaks the
    // instruction and drops the cruise again.
    harness.with_drive(|drive, _| {
        drive.cruise_mph = Some(41.0);
        drive.gate_reminder_s = 0.0;
    });
    frame(&mut harness);
    let said = an_event_containing(&harness, "Still at");
    assert!(said.to_lowercase().contains("stop to dock"), "{said}");
    assert!(harness.read_drive(|d| d.cruise_mph).is_none());

    // S answers with the gate, not the posted limit of the ended route.
    harness.clear_speech();
    harness.with_drive(|drive, ctx| drive.speak_speed_limit(ctx));
    let said = last_main(&harness);
    assert!(said.contains("Stop to dock"), "{said}");
    assert!(!said.contains("miles per hour"), "{said}");

    // R answers with the arrival too, not the abandoned highway route with
    // its frozen "3 miles remaining".
    harness.clear_speech();
    harness.with_drive(|drive, ctx| drive.speak_route_status(ctx));
    let said = last_main(&harness);
    assert!(said.to_lowercase().contains("you have arrived"), "{said}");
    assert!(said.contains("Stop to dock"), "{said}");
    assert!(!said.contains("remaining"), "{said}");
}

// -- what the trailer weighs ---------------------------------------------------------------

#[test]
fn test_cargo_mass_is_loaded_on_delivery_and_empty_on_pickup() {
    let mut app = TestApp::new();
    app.ctx.profile = Some(Profile::named_in("Load Mass", "Buffalo"));
    let world = get_world();
    let route = world
        .supported_route("Buffalo", "Rochester", None)
        .expect("the world routes")
        .expect("Buffalo to Rochester has a route");
    let mut job = Job::new(
        &CARGO_CATALOG["general"],
        18.0,
        "Buffalo",
        "company yard",
        "Rochester",
        route.miles(),
        1000.0,
        12.0,
    );
    job.destination_location = "Rochester freight market".to_string();

    let loaded = DrivingState::new(
        &mut app.ctx,
        job.clone(),
        route.clone(),
        Some(7),
        DRIVE_PHASE_DELIVERY,
        None,
    );
    assert!((loaded.truck().cargo_kg - 18.0 * KG_PER_TON).abs() < 1e-6);
    assert!(loaded.truck().gross_mass_kg() > loaded.truck().tare_kg());

    // The pickup deadhead runs empty: no payload aboard yet.
    let empty = DrivingState::new(&mut app.ctx, job, route, Some(7), DRIVE_PHASE_PICKUP, None);
    assert_eq!(empty.truck().cargo_kg, 0.0);
    assert!((empty.truck().gross_mass_kg() - empty.truck().tare_kg()).abs() < 1e-6);
}

// -- the settlement -------------------------------------------------------------------------

#[test]
fn test_toll_route_delivery_settlement_records_expense() {
    let mut app = TestApp::new();
    app.ctx.profile = Some(Profile::named_in("Toll Test", "New York"));
    let world = get_world();
    let mut job = Job::new(
        &CARGO_CATALOG["electronics"],
        18.0,
        "New York",
        "JFK Air Cargo",
        "Philadelphia",
        78.0,
        2500.0,
        12.0,
    );
    job.origin_type = "air_cargo".to_string();
    job.destination_location = "Philadelphia Distribution Center".to_string();
    job.destination_type = "retail_distribution".to_string();
    let route = world
        .route_from_cities(&["New York", "Philadelphia"])
        .expect("New York to Philadelphia is a route");
    let mut driving = DrivingState::new(
        &mut app.ctx,
        job.clone(),
        route,
        Some(5),
        DRIVE_PHASE_DELIVERY,
        None,
    );
    // Both I-95 tolls sit at mi 8.9 (NJ Turnpike) and 86.1 (Delaware River)
    // after the node moved to Hunts Point; drive past both.
    driving.trip.position_mi = 90.0;
    driving.trip.update(0.0);
    assert_eq!(driving.trip.toll_expense(), 30.0);

    app.ctx
        .profile
        .as_mut()
        .expect("a career")
        .set_money(1000.0);
    let hours = settlement_hours(&driving);
    let gross = job.payout_default(hours, 0.0);
    let status = app
        .ctx
        .profile
        .as_ref()
        .expect("a career")
        .business_status
        .clone();
    let carrier_key = app
        .ctx
        .profile
        .as_ref()
        .expect("a career")
        .carrier_key
        .clone();
    let owned: Vec<&str> = Vec::new();
    let expected = build_business_settlement(
        &status,
        &job,
        gross,
        true,
        0.0,
        &SettlementTerms {
            carrier_key: if carrier_key.is_empty() {
                None
            } else {
                Some(carrier_key.as_str())
            },
            owned_trailers: &owned,
            reputation: None,
            transponder: false,
            record_surcharge: 1.0,
        },
    );

    let arrival = ArrivalState::new(&mut app.ctx, &mut driving);
    let money = app.ctx.profile.as_ref().expect("a career").money();
    assert!(
        (money - (1000.0 + expected.net_before_advance)).abs() < 0.5,
        "{money}"
    );
    assert!(
        (app.ctx
            .profile
            .as_ref()
            .expect("a career")
            .career
            .total_earnings
            - expected.net_before_advance)
            .abs()
            < 0.5
    );
    let text = arrival.summary_parts.join(" ");
    for phrase in [
        "Charges 215: ",
        "tolls 30",
        "accessorials carrier-authorized unloading service 185 dollars",
        "not deducted from driver pay",
        "Fines 0.",
    ] {
        assert!(text.contains(phrase), "{text}");
    }
    assert!(text.contains("Carrier gross"), "{text}");
    assert!(text.contains("Net "), "{text}");

    assert_eq!(arrival.title(), "Delivery complete");
    let mut arrival = arrival;
    let summary_lines: Vec<String> = arrival
        .build_items(&mut app.ctx)
        .iter()
        .map(|item| item.text(&arrival, &app.ctx))
        .collect();
    assert!(
        summary_lines
            .iter()
            .any(|line| line.starts_with("Delivered 18 tons of electronics")),
        "{summary_lines:#?}"
    );
    assert!(
        summary_lines
            .iter()
            .any(|line| line.starts_with("Carrier gross:")),
        "{summary_lines:#?}"
    );
    assert!(
        summary_lines
            .iter()
            .any(|line| line.contains("Carrier-paid or reimbursed charges")),
        "{summary_lines:#?}"
    );
    assert!(
        summary_lines
            .iter()
            .any(|line| line.starts_with("Route: New York to Philadelphia")),
        "{summary_lines:#?}"
    );

    // Right does nothing here: the arrival summary has no screen carousel.
    let old_index = arrival.menu().index;
    State::handle_event(&mut arrival, &mut app.ctx, &key_event(Key::Right, None));
    assert_eq!(arrival.menu().index, old_index);
    let rebuilt: Vec<String> = arrival
        .build_items(&mut app.ctx)
        .iter()
        .map(|item| item.text(&arrival, &app.ctx))
        .collect();
    assert_eq!(rebuilt, summary_lines);
}

// -- the manual-spec differential ---------------------------------------------------------

/// A New York to Philadelphia delivery held by a manual-trained driver,
/// parked at the receiver in manual.
fn manual_run(drove_automatic: bool) -> String {
    let mut app = TestApp::new();
    let mut profile = Profile::named_in("Manual Test", "New York");
    profile
        .career
        .purchased_endorsements
        .push("manual_transmission".to_string());
    app.ctx.profile = Some(profile);
    let job = Job::new(
        &CARGO_CATALOG["electronics"],
        18.0,
        "New York",
        "JFK Air Cargo",
        "Philadelphia",
        78.0,
        2500.0,
        12.0,
    );
    let route = get_world()
        .route_from_cities(&["New York", "Philadelphia"])
        .expect("New York to Philadelphia is a route");
    let mut driving = DrivingState::new(
        &mut app.ctx,
        job,
        route,
        Some(5),
        DRIVE_PHASE_DELIVERY,
        None,
    );
    driving.trip.truck.transmission.automatic = false;
    driving.drove_automatic = drove_automatic;
    let arrival = ArrivalState::new(&mut app.ctx, &mut driving);
    arrival.summary_parts.join(" ")
}

#[test]
fn test_the_manual_differential_pays_only_a_run_driven_in_manual() {
    assert!(manual_run(false).contains("Manual-spec differential"));
    // Switched to manual for the last stretch: the gate sees manual, the
    // run was automatic.
    assert!(!manual_run(true).contains("Manual-spec differential"));
}

#[test]
fn test_moving_in_automatic_marks_the_run_and_a_save_keeps_it() {
    let mut harness = a_drive("Automatic Run");
    harness.with_drive(|drive, ctx| {
        ctx.settings.automatic_transmission = true;
        drive.trip.truck.velocity_mps = 20.0;
    });
    frame(&mut harness);
    let resumed = harness.with_drive(|drive, ctx| {
        assert!(drive.drove_automatic);
        let snapshot = drive.snapshot(ctx);
        DrivingState::from_snapshot(ctx, &snapshot)
            .expect("the snapshot resumes")
            .drove_automatic
    });
    assert!(resumed);
}
