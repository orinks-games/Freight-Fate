//! `states/driving_menu_states.rs` and `states/driving_stop_detail.rs`: the
//! live status screens, the driver tablet, the destination facility and the
//! delivery settlement.
//!
//! Ported from the menu halves of `tests/test_stop_detail.py`,
//! `tests/test_settlement_readout_leaner.py`, `tests/test_facility_engine.py`
//! and `tests/test_settlement_accounting.py`. The cases that only reach these
//! screens by driving a whole pickup-to-delivery flow through the playtest
//! harness are listed here, ignored, with their bodies noted, so the two
//! suites diff by name.

use ff_core::models::business::COMPANY_DRIVER;
use ff_core::models::jobs::{Job, CARGO_CATALOG};
use ff_core::models::profile::Profile;
use ff_core::sim::trip_models::RoadStop;

use freight_fate::app::testing::TestApp;
use freight_fate::states::base::{Key, Menu};
use freight_fate::states::driving::DrivingState;
use freight_fate::states::driving_core::DRIVE_PHASE_DELIVERY;
use freight_fate::states::driving_menu_states::{
    ArrivalState, DriveRef, DriverAppScreenState, DriverAppsState, DrivingStatusScreenState,
    DrivingStatusState, FacilityArrivalState,
};
use freight_fate::states::driving_radio_app::RadioAppState;
use freight_fate::states::driving_stop_detail::{ConfirmMovePlanState, StopDetailState};

use crate::states_driving_menus_support::*;

/// `_stops(position_mi)` from `test_stop_detail.py`.
fn map_stops(position_mi: f64) -> Vec<RoadStop> {
    let mut first = RoadStop::new(
        "Willow Creek Travel Center",
        position_mi + 3.0,
        "travel_center",
    );
    first.actions = ["fuel", "break"].iter().map(|a| a.to_string()).collect();
    first.services = ["diesel", "food"].iter().map(|s| s.to_string()).collect();
    first.parking = "confirmed".to_string();
    first.exit_label = "exit 12".to_string();
    let mut second = RoadStop::new("Cedar Rapids Rest Area", position_mi + 20.0, "rest_area");
    second.actions = vec!["break".to_string()];
    second.parking = "limited".to_string();
    vec![first, second]
}

/// A lowercased word joined by underscores, i.e. a world data key.
fn is_slug_key(word: &str) -> bool {
    let word = word.trim_matches(|c: char| !c.is_alphanumeric() && c != '_');
    word.contains('_')
        && word
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

// -- the status screens ----------------------------------------------------------------

#[test]
fn test_status_menu_lists_every_screen_and_a_way_back() {
    let mut app = TestApp::new();
    let drive = a_drive(&mut app);
    let mut state = DrivingStatusState::with_drive(drive_ref(&drive));
    let rows = build_labels(&mut state, &mut app.ctx);
    assert_eq!(
        rows,
        vec![
            "Route",
            "Driver",
            "Map",
            "Radio",
            "Driver apps",
            "Driving commands",
            "Back to driving",
        ]
    );
}

#[test]
fn test_driver_apps_row_opens_the_tablet_and_radio_opens_its_app() {
    let mut app = TestApp::new();
    let drive = a_drive(&mut app);
    let mut status = DrivingStatusState::with_drive(drive_ref(&drive));
    activate(&mut status, &mut app.ctx, "Driver apps");
    assert!(top_is::<DriverAppsState>(&app));

    app.handle_event(&key(Key::Return)); // the first row is Radio
    assert!(top_is::<RadioAppState>(&app));
}

#[test]
fn test_driver_app_screens_repeat_their_lines() {
    let mut app = TestApp::new();
    let drive = a_drive(&mut app);
    let mut apps = DriverAppsState::new(drive_ref(&drive));
    activate(&mut apps, &mut app.ctx, "ELD");
    assert!(top_is::<DriverAppScreenState>(&app));
    let rows = with_top_ctx::<DriverAppScreenState, _>(&mut app, build_labels);
    assert!(rows[0].starts_with("ELD: "), "{rows:?}");
    assert!(
        rows.iter().any(|line| line.starts_with("ELD route note:")),
        "{rows:?}"
    );
    assert_eq!(rows.last().map(String::as_str), Some("Back to Driver apps"));
}

#[test]
fn test_map_screen_speaks_city_names_never_slug_keys() {
    // The Map screen's route line reads city names, not world data keys.
    //
    // `route.cities` holds slugs (`new_york_ny_us`); every other screen wraps
    // them in `world.spoken_city`. This one did not, so the first line a
    // player heard on the Map screen was a string of underscored keys spelled
    // out.
    let mut app = TestApp::new();
    let drive = a_drive(&mut app);
    with_drive(&drive, |d| {
        d.trip.stops = map_stops(d.trip.position_mi);
    });
    let mut screen = DrivingStatusScreenState::new(drive_ref(&drive), "map");
    let texts = build_labels(&mut screen, &mut app.ctx);

    // Both names are shared with cities in other states, so the spoken layer
    // qualifies them; either way, no underscores.
    assert_eq!(texts[0], "Route: Buffalo, New York to Rochester, New York");
    for text in &texts {
        assert!(
            !text.split_whitespace().any(is_slug_key),
            "slug key leaked into spoken text: {text:?}"
        );
    }
}

#[test]
fn test_map_route_line_collapses_consecutive_deadhead_city_repeats() {
    // Facility approaches store one city key per local leg, so a deadhead
    // into Portland (or Rochester here) used to speak the destination many
    // times on the Map Route line (GitHub #205).
    let mut app = TestApp::new();
    let drive = a_drive(&mut app);
    with_drive(&drive, |d| {
        let end = d.route.cities.last().cloned().expect("destination");
        d.route.cities.extend(std::iter::repeat_n(end, 8));
    });
    let mut screen = DrivingStatusScreenState::new(drive_ref(&drive), "map");
    let texts = build_labels(&mut screen, &mut app.ctx);
    assert_eq!(
        texts[0], "Route: Buffalo, New York to Rochester, New York",
        "consecutive destination repeats must collapse on the spoken Route line"
    );
    let rochester_mentions = texts[0].matches("Rochester").count();
    assert_eq!(rochester_mentions, 1, "{}", texts[0]);
}

#[test]
fn test_enter_on_map_stop_opens_structured_detail_view() {
    let mut app = TestApp::new();
    let drive = a_drive(&mut app);
    let stop = with_drive(&drive, |d| {
        d.trip.stops = map_stops(d.trip.position_mi);
        d.trip.stops[0].clone()
    });
    let mut screen = DrivingStatusScreenState::new(drive_ref(&drive), "map");
    activate(&mut screen, &mut app.ctx, "Stop in");

    assert!(top_is::<StopDetailState>(&app));
    let texts = with_top_ctx::<StopDetailState, _>(&mut app, build_labels);
    let joined = texts.join(" ");
    assert_eq!(texts[0], format!("Stop: {}.", stop.spoken_name()));
    assert!(texts.contains(&"Exit: exit 12.".to_string()), "{texts:?}");
    assert!(joined.contains("Distance:"), "{joined}");
    assert!(
        texts.contains(&"Offers: fuel, and 30-minute rest break.".to_string()),
        "{texts:?}"
    );
    assert!(
        texts.contains(&"Listed services: diesel, and food.".to_string()),
        "{texts:?}"
    );
    assert!(
        texts.contains(&"Parking: confirmed truck parking.".to_string()),
        "{texts:?}"
    );
    // The ETA line sits below the services and distance lines.
    let eta_index = texts
        .iter()
        .position(|t| t.starts_with("Estimated time"))
        .expect("an ETA line");
    let services_index = texts
        .iter()
        .position(|t| t == "Listed services: diesel, and food.")
        .expect("a services line");
    assert!(eta_index > services_index);
    assert_eq!(
        texts[texts.len() - 2],
        format!("Plan to stop at {}", stop.name)
    );
    assert_eq!(texts[texts.len() - 1], "Back");
}

#[test]
fn test_eta_line_mirrors_eld_pace_rules() {
    let mut app = TestApp::new();
    let drive = a_drive(&mut app);
    let far = with_drive(&drive, |d| {
        let far = RoadStop::new(
            "Far Travel Center",
            d.trip.position_mi + 110.0,
            "travel_center",
        );
        d.trip.stops = vec![far.clone()];
        far
    });
    let mut state = StopDetailState::new(drive_ref(&drive), far);

    // parked -> typical highway pace at 55
    with_drive(&drive, |d| d.trip.truck.velocity_mps = 0.0);
    let line = build_labels(&mut state, &mut app.ctx)
        .into_iter()
        .find(|line| line.starts_with("Estimated time"))
        .expect("an ETA line");
    assert!(line.contains("at a typical highway pace"), "{line}");
    assert!(
        line.contains(&format!("{:.1} hours", 110.0 / 55.0)),
        "{line}"
    );

    // rolling -> your actual speed
    let speed = with_drive(&drive, |d| {
        d.trip.truck.velocity_mps = 60.0 / 2.23694;
        d.trip.truck.speed_mph()
    });
    let line = build_labels(&mut state, &mut app.ctx)
        .into_iter()
        .find(|line| line.starts_with("Estimated time"))
        .expect("an ETA line");
    assert!(line.contains("at your current speed"), "{line}");
    assert!(
        line.contains(&format!("{:.1} hours", 110.0 / speed)),
        "{line}"
    );
}

#[test]
fn test_plan_cancel_and_supersede() {
    let mut app = TestApp::new();
    let drive = a_drive(&mut app);
    let (first, second) = with_drive(&drive, |d| {
        d.trip.stops = map_stops(d.trip.position_mi);
        (d.trip.stops[0].clone(), d.trip.stops[1].clone())
    });

    let mut detail = StopDetailState::new(drive_ref(&drive), first.clone());
    activate(
        &mut detail,
        &mut app.ctx,
        &format!("Plan to stop at {}", first.name),
    );
    assert_eq!(
        with_drive(&drive, |d| d.trip.planned_stop_key.clone()),
        Some(first.key())
    );
    let texts = build_labels(&mut detail, &mut app.ctx);
    assert!(texts.contains(&format!("Cancel planned stop at {}", first.name)));
    assert!(!texts.contains(&format!("Plan to stop at {}", first.name)));

    // A different stop's details offer only plan-here, never cancel-the-other.
    let mut other = StopDetailState::new(drive_ref(&drive), second.clone());
    let texts = build_labels(&mut other, &mut app.ctx);
    assert!(texts.contains(&format!("Plan to stop at {}", second.name)));
    assert!(!texts.iter().any(|t| t.starts_with("Cancel planned stop")));

    // Planning the second stop while the first is planned confirms the move
    // rather than switching silently.
    activate(
        &mut other,
        &mut app.ctx,
        &format!("Plan to stop at {}", second.name),
    );
    assert!(top_is::<ConfirmMovePlanState>(&app));
    assert_eq!(
        with_drive(&drive, |d| d.trip.planned_stop_key.clone()),
        Some(first.key()),
        "unchanged until confirmed"
    );
    let confirm_labels = with_top_ctx::<ConfirmMovePlanState, _>(&mut app, build_labels);
    assert!(confirm_labels[0].starts_with("Yes,"), "lands on Yes");
    with_top_ctx::<ConfirmMovePlanState, _>(&mut app, |c, ctx| activate(c, ctx, "Yes,"));
    assert_eq!(
        with_drive(&drive, |d| d.trip.planned_stop_key.clone()),
        Some(second.key()),
        "moved after confirming"
    );

    // The Map screen carries a standalone cancel button while a plan exists.
    let mut screen = DrivingStatusScreenState::new(drive_ref(&drive), "map");
    activate(
        &mut screen,
        &mut app.ctx,
        &format!("Cancel planned stop at {}", second.name),
    );
    assert_eq!(
        with_drive(&drive, |d| d.trip.planned_stop_key.clone()),
        None
    );
    let texts = build_labels(&mut screen, &mut app.ctx);
    assert!(!texts.iter().any(|t| t.starts_with("Cancel planned stop")));
}

#[test]
fn test_cancel_button_only_on_the_planned_stops_details() {
    let mut app = TestApp::new();
    let drive = a_drive(&mut app);
    let (planned, other) = with_drive(&drive, |d| {
        d.trip.stops = map_stops(d.trip.position_mi);
        let planned = d.trip.stops[0].clone();
        d.trip.planned_stop_key = Some(planned.key());
        (planned, d.trip.stops[1].clone())
    });

    // The planned stop's own details offer Cancel and no Plan.
    let mut own = StopDetailState::new(drive_ref(&drive), planned.clone());
    let own_texts = build_labels(&mut own, &mut app.ctx);
    assert!(own_texts.contains(&format!("Cancel planned stop at {}", planned.name)));
    assert!(!own_texts.iter().any(|t| t.starts_with("Plan to stop")));

    // Every other stop offers Plan and never a cancel button.
    let mut elsewhere = StopDetailState::new(drive_ref(&drive), other.clone());
    let else_texts = build_labels(&mut elsewhere, &mut app.ctx);
    assert!(else_texts.contains(&format!("Plan to stop at {}", other.name)));
    assert!(!else_texts
        .iter()
        .any(|t| t.starts_with("Cancel planned stop")));

    // The move confirmation offers Yes (default) and a No that keeps the plan.
    let mut confirm = ConfirmMovePlanState::new(drive_ref(&drive), other);
    let confirm_labels = build_labels(&mut confirm, &mut app.ctx);
    assert!(confirm_labels[0].starts_with("Yes,"));
    assert!(confirm_labels.iter().any(|t| t.starts_with("No,")));
    activate(&mut confirm, &mut app.ctx, "No,");
    assert_eq!(
        with_drive(&drive, |d| d.trip.planned_stop_key.clone()),
        Some(planned.key()),
        "No leaves it unchanged"
    );
}

#[test]
fn test_plan_button_skips_the_menu_click_so_only_the_chime_plays() {
    let mut app = TestApp::new();
    let drive = a_drive(&mut app);
    let stop = with_drive(&drive, |d| {
        d.trip.stops = map_stops(d.trip.position_mi);
        d.trip.stops[0].clone()
    });
    let mut detail = StopDetailState::new(drive_ref(&drive), stop.clone());
    let items = detail.build_items(&mut app.ctx);
    let plan_label = format!("Plan to stop at {}", stop.name);
    for item in &items {
        let text = item.text(&detail, &app.ctx);
        if text == plan_label {
            // `plan` plays its own ui/notify chime.
            assert_eq!(item.select_sound, None);
        } else {
            assert_eq!(item.select_sound.as_deref(), Some("ui/menu_select"));
        }
    }
}

// -- the destination facility ------------------------------------------------------------

#[test]
fn test_destination_facility_offers_the_same_shutdown() {
    let mut app = TestApp::new();
    let drive = a_drive(&mut app);
    with_drive(&drive, |d| {
        d.trip.truck.start_engine();
    });
    let mut arrival = FacilityArrivalState::with_drive(drive_ref(&drive));
    let rows = build_labels(&mut arrival, &mut app.ctx);
    assert!(
        rows[0] == "Dock and deliver" || rows[0] == "Drop the loaded trailer and hook an empty",
        "{rows:?}"
    );
    assert!(
        rows.contains(&"Shut down the engine".to_string()),
        "{rows:?}"
    );

    app.clear_speech();
    activate(&mut arrival, &mut app.ctx, "Shut down the engine");
    assert!(!with_drive(&drive, |d| d.trip.truck.engine_on));
    assert!(last(&app).contains("Engine off."), "{}", last(&app));
    let rows = build_labels(&mut arrival, &mut app.ctx);
    assert!(rows.contains(&"Start the engine".to_string()), "{rows:?}");
}

#[test]
fn test_docking_needs_a_full_stop_first() {
    let mut app = TestApp::new();
    let drive = a_drive(&mut app);
    with_drive(&drive, |d| {
        d.trip.truck.start_engine();
        d.trip.truck.velocity_mps = 5.0;
    });
    let mut arrival = FacilityArrivalState::with_drive(drive_ref(&drive));
    app.clear_speech();
    let primary = build_labels(&mut arrival, &mut app.ctx)[0].clone();
    activate(&mut arrival, &mut app.ctx, &primary);
    assert_eq!(last(&app), "Stop before docking.");
}

#[test]
fn test_facility_go_back_names_the_finishing_action() {
    let mut app = TestApp::new();
    let drive = a_drive(&mut app);
    let mut arrival = FacilityArrivalState::with_drive(drive_ref(&drive));
    arrival.build_items(&mut app.ctx);
    app.clear_speech();
    arrival.go_back(&mut app.ctx);
    let said = last(&app);
    assert!(said.starts_with("At destination."), "{said}");
    assert!(said.ends_with("to finish."), "{said}");
}

// -- the delivery settlement ---------------------------------------------------------------

/// `_job()` from `test_settlement_readout_leaner.py`.
fn settlement_job() -> Job {
    let mut job = Job::new(
        &CARGO_CATALOG["electronics"],
        18.0,
        "New York",
        "New York pickup",
        "Philadelphia",
        78.0,
        2500.0,
        12.0,
    );
    job.origin_type = "air_cargo".to_string();
    job.destination_location = "Philadelphia receiver".to_string();
    job.destination_type = "dry_warehouse".to_string();
    job
}

/// `_settle(app, job, route_cities, damage=, fuel_fraction=)`.
fn settle(app: &mut TestApp, damage: f64, fuel_fraction: f64) -> ArrivalState {
    let world = ff_core::data::world::get_world();
    let mut profile = Profile::named_in("Readout Audit", "New York");
    profile.set_money(1000.0);
    profile.business_status = COMPANY_DRIVER.to_string();
    app.ctx.profile = Some(profile);
    let route = world
        .supported_route("New York", "Philadelphia", None)
        .expect("the world routes")
        .expect("the corridor is supported");
    let mut driving = DrivingState::new(
        &mut app.ctx,
        settlement_job(),
        route,
        Some(4),
        DRIVE_PHASE_DELIVERY,
        None,
    );
    if damage > 0.0 {
        driving.trip.truck.damage_pct += damage;
    }
    driving.trip.truck.fuel_gal = driving.trip.truck.specs.fuel_tank_gal * fuel_fraction;
    driving.trip.position_mi = driving.trip.total_miles();
    ArrivalState::new(&mut app.ctx, &mut driving)
}

#[test]
fn test_clean_run_drops_the_zero_information_rows() {
    let mut app = TestApp::new();
    let joined = settle(&mut app, 0.0, 1.0).summary_lines.join(" ");
    assert!(!joined.contains("No new damage recorded"), "{joined}");
    assert!(
        !joined.contains("Carrier charges are not deducted from driver pay"),
        "{joined}"
    );
    assert!(!joined.contains("Fuel remaining"), "{joined}");
    assert!(!joined.contains("Truck damage now"), "{joined}");
    assert!(!joined.contains("No new career messages"), "{joined}");
}

#[test]
fn test_damage_and_low_fuel_still_speak_when_they_matter() {
    let mut app = TestApp::new();
    let joined = settle(&mut app, 12.0, 0.1).summary_lines.join(" ");
    assert!(
        joined.contains("Truck damage added on this run"),
        "{joined}"
    );
    assert!(joined.contains("Fuel remaining"), "{joined}");
    assert!(joined.contains("Truck damage now"), "{joined}");
}

#[test]
fn test_settlement_pays_the_driver_and_parks_them_at_the_terminal() {
    let mut app = TestApp::new();
    let joined = settle(&mut app, 0.0, 1.0).summary_lines.join(" ");
    assert!(
        joined.contains("Delivered 18 tons of electronics"),
        "{joined}"
    );
    assert!(joined.contains("Net driver pay:"), "{joined}");
    assert!(joined.contains("Money after settlement:"), "{joined}");
    assert!(joined.contains("Parked at "), "{joined}");
    // The active trip is closed out and the driver has moved.
    let profile = app.ctx.profile.as_ref().expect("a career");
    assert!(profile.active_trip.is_none());
    // `p.current_city = job.destination`, exactly as the job named it.
    assert_eq!(profile.current_city, "Philadelphia");
    assert_eq!(profile.career.deliveries, 1);
}

#[test]
fn test_settlement_rows_are_reviewable_with_a_copy_and_a_continue() {
    let mut app = TestApp::new();
    let mut state = settle(&mut app, 0.0, 1.0);
    let rows = build_labels(&mut state, &mut app.ctx);
    assert_eq!(rows[rows.len() - 2], "Copy delivery summary to clipboard");
    assert!(rows[rows.len() - 1].starts_with("Continue to "));
    assert_eq!(rows.len(), state.summary_lines.len() + 2);
}

#[test]
fn test_copying_the_summary_reaches_the_clipboard() {
    let mut app = TestApp::new();
    let mut state = settle(&mut app, 0.0, 1.0);
    app.clear_speech();
    activate(&mut state, &mut app.ctx, "Copy delivery summary");
    assert_eq!(last(&app), "Delivery summary copied to clipboard.");
    let copied = app.ctx.clipboard.get_text().unwrap_or_default();
    assert!(
        copied.starts_with("Freight Fate: Delivery complete."),
        "{copied}"
    );
    assert!(copied.ends_with("\n\n#FreightFateRuns"), "{copied}");
}

#[test]
fn test_the_facility_keeps_its_rows_when_a_rebuild_misses_the_drive() {
    // A rebuild that misses the drive used to leave this screen with no rows,
    // which the menu speaks as "No options available." -- a driver parked at
    // the dock, told there is nothing to do and no way on. It keeps what it
    // was already showing now; at worst one label is an action out of date.
    // `unreachable_drive` explains why the miss here is not the nested borrow
    // itself.
    let mut app = TestApp::new();
    let drive = a_drive(&mut app);

    let mut arrival = FacilityArrivalState::with_drive(drive_ref(&drive));
    let showing = arrival.build_items(&mut app.ctx);
    assert!(!showing.is_empty());

    let mut stranded = FacilityArrivalState::with_drive(unreachable_drive());
    let rows = rows_with_the_drive_out_of_reach(&mut stranded, showing, &mut app.ctx);
    assert!(
        rows.iter()
            .any(|row| row == "Dock and deliver"
                || row == "Drop the loaded trailer and hook an empty"),
        "the facility lost the row that finishes the delivery: {rows:?}"
    );
}

// -- the drive handle ----------------------------------------------------------------------

#[test]
fn test_a_screen_without_a_drive_answers_nothing_quietly() {
    // The legitimate empty: a screen built without a drive, which is what a
    // test that never pushes one gets. It answers nothing, and says nothing
    // about it.
    let handle = DriveRef::empty();
    assert!(handle.is_empty());
    assert!(handle.read(|d| d.trip.position_mi).is_none());
}

#[test]
#[cfg(debug_assertions)]
#[should_panic(expected = "already borrowed")]
fn test_reaching_for_a_drive_that_is_already_held_fails_loudly() {
    // The other empty, which is never a state of the world: something further
    // up the stack already holds the drive, and this code reached for it again
    // instead of being handed the one that is already open. That used to
    // answer nothing, indistinguishable from having no drive at all, and every
    // caller turned it into a plausible default -- a full tank, an empty menu,
    // a trailer that was not there. It fails on a bench now.
    let mut app = TestApp::new();
    let drive = a_drive(&mut app);
    let handle = drive_ref(&drive);
    let _held = drive.borrow_mut();
    let _ = handle.read(|d| d.trip.position_mi);
}

// -- not portable without the harness ------------------------------------------------------

#[test]
fn test_unloading_burns_fuel_only_while_the_engine_runs() {
    // Idling through the unload costs fuel; shutting down first costs none.
    //
    // Python walked the whole pickup-to-delivery flow to reach the gate
    // (`arrive_running` -> `reach_destination_facility`). The facility screen
    // is what this case is about, and every other test in this section builds
    // it over a drive directly, so it is built the same way here.
    let mut app = TestApp::new();
    let drive = a_drive(&mut app);
    with_drive(&drive, |d| {
        d.trip.truck.start_engine();
        d.trip.truck.velocity_mps = 0.0;
    });
    let mut arrival = FacilityArrivalState::with_drive(drive_ref(&drive));
    let before = with_drive(&drive, |d| d.trip.truck.fuel_gal);

    activate(&mut arrival, &mut app.ctx, "Shut down the engine");
    assert!(!with_drive(&drive, |d| d.trip.truck.engine_on));
    // Dock and deliver, or drop the loaded trailer: whichever this facility
    // does, it is the first row.
    let primary = build_labels(&mut arrival, &mut app.ctx)[0].clone();
    activate(&mut arrival, &mut app.ctx, &primary);
    app.ctx.run_deferred();
    finish_timed_state(&mut app);

    assert!(
        top_is::<ArrivalState>(&app),
        "the unload settles at arrival"
    );
    assert_eq!(with_drive(&drive, |d| d.trip.truck.fuel_gal), before);
}

/// `finish_timed_state(app)`: run the timed message screen out.
fn finish_timed_state(app: &mut TestApp) {
    use freight_fate::states::base::TimedMessageState;
    let remaining = with_top::<TimedMessageState, _>(app, |s| s.remaining);
    let state = app.state().expect("a timed state");
    {
        let mut borrowed = state.borrow_mut();
        borrowed.update(&mut app.ctx, remaining + 0.01);
    }
    app.ctx.run_deferred();
}

// `test_the_primary_action_stays_the_first_item` is live in
// `crates/freight-fate/tests/states_city_pickup.rs`.
