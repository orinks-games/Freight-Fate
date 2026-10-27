//! The menus a stop pushes: the rest-stop screen
//! (`states/driving_rest_states/rest_stop.rs`), the dock arrival
//! (`states/driving_menu_states/facility_arrival.rs`), and the way pulling in
//! secures the truck.
//!
//! Ported from `tests/test_driving_features.py`:
//! `test_facility_menu_waits_for_full_stop`,
//! `test_exit_flow_reaches_the_rest_stop_menu`,
//! `test_rest_stop_menu_can_save_active_drive`,
//! `test_opening_a_route_stop_secures_the_truck`,
//! `test_poi_menu_uses_curated_roadside_assistance_label`,
//! `test_rest_stop_sleep_warns_before_redundant_double_sleep` and
//! `test_lot_sleep_warns_before_redundant_double_sleep`.
//!
//! Python read `state._confirm_sleep_rested` to tell a warning press from a
//! sleeping one. That flag is private here, and it does not need to be
//! reached: the player-facing halves of the same guard -- the clock that did
//! not move and the sentence that says why -- are what the two sleep cases
//! assert instead.

use ff_core::models::profile::Profile;
use ff_core::sim::hos;
use ff_core::sim::trip_models::RoadStop;
use ff_core::sim::weather::WeatherKind;

use freight_fate::controller::ControllerButton;
use freight_fate::playtest::harness::{key_event, PlaytestHarness, StartDelivery};
use freight_fate::states::base::{InputEvent, Key, Menu, State};
use freight_fate::states::career_stats::fully_rested;
use freight_fate::states::driving::DrivingState;
use freight_fate::states::driving_menu_states::{DriveRef, FacilityArrivalState};
use freight_fate::states::driving_rest_states::{ParkingFullState, RestStopState};

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
        // Roadside chatter about somebody else's log check is ambient colour
        // that lands between the lines these cases read.
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

fn press(harness: &mut PlaytestHarness, key: Key) {
    harness.with_drive(move |drive, ctx| drive.handle_key_event(ctx, &key_event(key, None)));
}

fn drive_ref(harness: &PlaytestHarness) -> DriveRef {
    DriveRef::of(&harness.shared_driving().expect("a drive on the stack"))
}

/// A rest-stop screen over the live drive, entered the way the drive enters it.
fn rest_stop_screen(harness: &mut PlaytestHarness, stop: RoadStop) -> RestStopState {
    let handle = drive_ref(harness);
    RestStopState::with_drive(handle, stop, false)
}

fn labels(state: &mut RestStopState, harness: &mut PlaytestHarness) -> Vec<String> {
    let items = state.build_items(&mut harness.app.ctx);
    items
        .iter()
        .map(|item| item.text(state, &harness.app.ctx))
        .collect()
}

/// Activate the row with this exact label.
fn activate(state: &mut RestStopState, harness: &mut PlaytestHarness, label: &str) {
    let items = state.build_items(&mut harness.app.ctx);
    let found = items
        .iter()
        .find(|item| item.text(state, &harness.app.ctx) == label)
        .cloned()
        .unwrap_or_else(|| {
            panic!(
                "no {label:?} item: {:?}",
                items
                    .iter()
                    .map(|item| item.text(state, &harness.app.ctx))
                    .collect::<Vec<_>>()
            )
        });
    (found.action)(state, &mut harness.app.ctx);
}

fn last_main(harness: &PlaytestHarness) -> String {
    harness.app.main_lines().last().cloned().unwrap_or_default()
}

// -- the dock ---------------------------------------------------------------------------

#[test]
fn test_facility_menu_waits_for_full_stop() {
    let mut harness = a_drive("Dock Stop");
    let log = harness.app.record_audio();
    harness.with_drive(|drive, _| {
        // This case walks the dock ending in detail, so pin the receiver to
        // one that unloads live. A receiver with a drop yard takes the whole
        // trailer instead, and that ending has its own test.
        drive.job.destination_type = "mine_quarry".to_string();
        mark_destination_exit_taken(drive);
        drive.truck_mut().velocity_mps = 1.1; // about 2.5 mph: parked, not docked
    });
    harness.clear_speech();
    log.borrow_mut().played.clear();

    frame(&mut harness);
    assert!(harness.state_is::<DrivingState>());
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
    // Looked up rather than assumed last: the frame can also carry a
    // speeding warning, depending on the posted limit of the route dispatch
    // drew.
    assert!(
        harness
            .app
            .event_lines()
            .iter()
            .any(|line| { line.contains("Stop and set the parking brake.") }),
        "{:#?}",
        harness.app.event_lines()
    );
    let hud = harness.with_drive(|d, ctx| d.lines(ctx));
    assert!(
        hud.last()
            .is_some_and(|line| line.to_lowercase().contains("stop to dock")),
        "{hud:#?}"
    );
    assert_eq!(
        log.borrow().played.last().map(|(key, ..)| key.clone()),
        Some("ui/notify".to_string())
    );

    harness.with_drive(|drive, _| {
        drive.truck_mut().velocity_mps = 0.0;
        drive.truck_mut().set_parking_brake();
    });
    frame(&mut harness);
    let arriving = harness
        .app
        .ctx
        .state()
        .expect("a state")
        .borrow()
        .lines(&harness.app.ctx);
    assert!(
        arriving
            .first()
            .is_some_and(|line| line.contains("Pulling into destination")),
        "{arriving:#?}"
    );
    harness.key(key_event(Key::Down, None));
    harness.finish_timed_state();

    assert!(harness.state_is::<FacilityArrivalState>());
    assert_eq!(
        log.borrow().played.last().map(|(key, ..)| key.clone()),
        Some("facility/dock_gate".to_string())
    );
    assert!(
        log.borrow()
            .played
            .iter()
            .all(|(key, ..)| key != "ui/menu_open"),
        "{:#?}",
        log.borrow().played
    );
    let rows = harness.menu_labels();
    assert!(DELIVERY_ACTIONS.contains(&rows[0].as_str()), "{rows:#?}");
    // The engine row sits with the actions, ahead of the two questions.
    assert!(
        rows[1] == "Shut down the engine" || rows[1] == "Start the engine",
        "{rows:#?}"
    );
    assert_eq!(&rows[2..], ["Check paperwork", "Check arrival status"]);

    harness.clear_speech();
    harness.select_menu_item("Check paperwork");
    assert!(harness.state_is::<FacilityArrivalState>());
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
    let paperwork = last_main(&harness);
    for phrase in [
        "Paperwork, ",
        "gross ",
        "Charges ",
        "Those charges do not reduce driver pay",
        "Net about ",
        "hours to the deadline",
        "Cargo condition",
        "Dock and deliver to settle",
    ] {
        assert!(paperwork.contains(phrase), "{paperwork}");
    }
    // A company driver's estimate is wages, not the carrier's gross: the
    // board quoted 224 dollars for a load this line then called 330 of
    // "net driver pay" (agent playtest, 2026-09-02).
    let gross = dollars_after(&paperwork, ", gross ");
    let net = dollars_after(&paperwork, "Net about ");
    assert!(net > 0.0 && net < gross, "{paperwork}");

    let minutes_before_unloading = harness.read_drive(|d| d.trip.game_minutes);
    log.borrow_mut().played.clear();
    harness.select_menu_item(&rows[0]);
    let unloading = harness
        .app
        .ctx
        .state()
        .expect("a state")
        .borrow()
        .lines(&harness.app.ctx);
    assert!(
        unloading
            .first()
            .is_some_and(|line| line.contains("Unloading cargo")),
        "{unloading:#?}"
    );
    harness.finish_timed_state();
    assert!(!harness.state_is::<FacilityArrivalState>());
    assert_eq!(
        harness
            .app
            .ctx
            .profile
            .as_ref()
            .expect("a career")
            .career
            .deliveries,
        1
    );
    assert!(
        (harness.read_drive(|d| d.trip.game_minutes)
            - (minutes_before_unloading + freight_fate::states::driving_core::UNLOADING_MIN))
            .abs()
            < 1e-6
    );
    let played_keys: Vec<String> = log
        .borrow()
        .played
        .iter()
        .map(|(key, ..)| key.clone())
        .collect();
    for key in ["poi/dock_and_deliver", "ui/job_complete", "ui/cash"] {
        assert!(played_keys.iter().any(|k| k == key), "{played_keys:#?}");
    }
    assert!(
        !played_keys.iter().any(|k| k == "ui/menu_open"),
        "{played_keys:#?}"
    );
}

fn press_pad(harness: &mut PlaytestHarness, button: ControllerButton) {
    harness.with_drive(move |drive, ctx| {
        drive.handle_controller_event(ctx, &InputEvent::button(button))
    });
}

#[test]
fn test_assist_off_facility_waits_for_the_players_parking_brake() {
    let mut harness = a_drive("Manual Facility Stop");
    harness.app.ctx.settings.destination_approach_assist = false;
    harness.with_drive(|drive, _| {
        mark_destination_exit_taken(drive);
        drive.truck_mut().velocity_mps = 0.0;
        drive.truck_mut().parking_brake = false;
    });

    frame(&mut harness);

    assert!(harness.state_is::<DrivingState>());
    assert!(!harness.read_drive(|d| d.arrival_menu_open));
    assert!(!harness.read_drive(|d| d.truck().parking_brake));

    harness.with_drive(|drive, _| drive.truck_mut().parking_brake = true);
    frame(&mut harness);

    assert!(harness.read_drive(|d| d.arrival_menu_open));
}

#[test]
fn test_t_opens_an_assist_off_facility_only_when_stopped_and_parked() {
    let mut harness = a_drive("Manual Facility T");
    harness.app.ctx.settings.destination_approach_assist = false;
    harness.with_drive(|drive, _| {
        mark_destination_exit_taken(drive);
        drive.truck_mut().velocity_mps = 0.0;
        drive.truck_mut().parking_brake = true;
    });

    press(&mut harness, Key::T);

    assert!(harness.read_drive(|d| d.arrival_menu_open));
}

#[test]
fn test_t_does_not_open_an_assist_off_facility_while_rolling() {
    let mut harness = a_drive("Rolling Facility T");
    harness.app.ctx.settings.destination_approach_assist = false;
    harness.with_drive(|drive, _| {
        mark_destination_exit_taken(drive);
        drive.truck_mut().velocity_mps = 2.0;
        drive.truck_mut().parking_brake = true;
    });

    press(&mut harness, Key::T);

    assert!(!harness.read_drive(|d| d.arrival_menu_open));
}

#[test]
fn test_enter_and_controller_a_cannot_bypass_manual_facility_parking() {
    for controller in [false, true] {
        let mut harness = a_drive("Manual Facility Confirm");
        harness.app.ctx.settings.destination_approach_assist = false;
        harness.with_drive(|drive, _| {
            mark_destination_exit_taken(drive);
            drive.arrival_full_stop_said = true;
            drive.truck_mut().velocity_mps = 0.0;
            drive.truck_mut().parking_brake = false;
        });

        if controller {
            press_pad(&mut harness, ControllerButton::A);
        } else {
            press(&mut harness, Key::Return);
        }

        assert!(!harness.read_drive(|d| d.arrival_menu_open));
        assert!(!harness.read_drive(|d| d.truck().parking_brake));
    }
}

#[test]
fn test_controller_rest_control_opens_a_stopped_parked_manual_pickup() {
    let mut harness = a_drive("Manual Pickup Controller");
    harness.app.ctx.settings.destination_approach_assist = false;
    harness.app.ctx.controller.modifier = true;
    harness.with_drive(|drive, _| {
        drive.phase = freight_fate::states::driving_core::DRIVE_PHASE_PICKUP;
        mark_destination_exit_taken(drive);
        drive.truck_mut().velocity_mps = 0.0;
        drive.truck_mut().parking_brake = true;
    });

    press_pad(&mut harness, ControllerButton::DPadDown);

    assert!(harness.read_drive(|d| d.arrival_menu_open));
}

#[test]
fn test_t_opens_an_assist_off_pickup_only_when_stopped_and_parked() {
    let mut harness = a_drive("Manual Pickup T");
    harness.app.ctx.settings.destination_approach_assist = false;
    harness.with_drive(|drive, _| {
        drive.phase = freight_fate::states::driving_core::DRIVE_PHASE_PICKUP;
        mark_destination_exit_taken(drive);
        drive.truck_mut().velocity_mps = 0.0;
        drive.truck_mut().parking_brake = true;
    });

    press(&mut harness, Key::T);

    assert!(harness.read_drive(|d| d.arrival_menu_open));
}

#[test]
fn test_assist_off_pickup_waits_for_the_players_parking_brake() {
    let mut harness = a_drive("Manual Pickup Stop");
    harness.app.ctx.settings.destination_approach_assist = false;
    harness.with_drive(|drive, _| {
        drive.phase = freight_fate::states::driving_core::DRIVE_PHASE_PICKUP;
        mark_destination_exit_taken(drive);
        drive.truck_mut().velocity_mps = 0.0;
        drive.truck_mut().parking_brake = false;
    });

    frame(&mut harness);

    assert!(harness.state_is::<DrivingState>());
    assert!(!harness.read_drive(|d| d.arrival_menu_open));
    assert!(!harness.read_drive(|d| d.truck().parking_brake));

    harness.with_drive(|drive, _| drive.truck_mut().parking_brake = true);
    frame(&mut harness);

    assert!(harness.read_drive(|d| d.arrival_menu_open));
}

// -- the rest stop -------------------------------------------------------------------------

#[test]
fn test_exit_flow_reaches_the_rest_stop_menu() {
    let mut harness = a_drive("Exit Flow");
    let stop_mi = harness.read_drive(|d| d.trip.stops[0].at_mi);
    harness.with_drive(|drive, _| {
        drive.trip.position_mi = stop_mi - 2.0;
        drive.truck_mut().velocity_mps = 15.0; // ~34 mph: slow enough for the ramp
    });
    press(&mut harness, Key::X);
    assert_eq!(
        harness.read_drive(|d| d.exit_stop.as_ref().map(|s| s.at_mi)),
        Some(stop_mi)
    );
    // Where the exit lane opens, steer right into it.
    harness.with_drive(move |drive, _| {
        drive.trip.position_mi = stop_mi - freight_fate::states::driving_core::EXIT_TAPER_MI / 2.0;
    });
    harness
        .app
        .ctx
        .input
        .press(Key::Right, freight_fate::states::base::Mods::NONE);
    for _ in 0..(60 * 5) {
        harness.with_drive(|drive, ctx| {
            drive.update_lane(ctx, DT);
            drive.update_exit_preparation(ctx, DT);
        });
        if harness.read_drive(|d| d.exit_lane_ready()) {
            break;
        }
    }
    assert!(harness.read_drive(|d| d.exit_lane_ready()));

    harness.with_drive(move |drive, _| drive.trip.position_mi = stop_mi); // reach the exit
    frame(&mut harness);
    assert!(harness.read_drive(|d| d.ramp_mi.is_some()), "on the ramp");
    assert!(harness.read_drive(|d| d.exit_stop.is_none()));

    harness.with_drive(|drive, _| {
        drive.ramp_mi = Some(0.0); // end of the ramp...
        drive.truck_mut().velocity_mps = 0.0; // ...braked to a stop
    });
    frame(&mut harness);
    let arriving = harness
        .app
        .ctx
        .state()
        .expect("a state")
        .borrow()
        .lines(&harness.app.ctx);
    assert!(
        arriving
            .first()
            .is_some_and(|line| line.contains("Pulling into stop")),
        "{arriving:#?}"
    );
    harness.key(key_event(Key::Down, None));
    harness.finish_timed_state();
    assert!(
        harness.state_is::<RestStopState>() || harness.state_is::<ParkingFullState>(),
        "the ramp reached no stop menu"
    );
    let rows = harness.menu_labels();
    assert_eq!(
        harness.focused_label().as_deref(),
        rows.first().map(|s| s.as_str()),
        "the stop menu opens on its first row"
    );
}

#[test]
fn test_rest_stop_menu_can_save_active_drive() {
    let mut harness = a_drive("Save At Stop");
    let stop_mi = harness.read_drive(|d| d.trip.stops[0].at_mi);
    harness.with_drive(move |drive, _| {
        drive.trip.position_mi = stop_mi;
        drive.truck_mut().velocity_mps = 0.0;
    });
    press(&mut harness, Key::T);
    assert!(
        harness.state_is::<RestStopState>() || harness.state_is::<ParkingFullState>(),
        "T opened no stop menu"
    );
    if harness.state_is::<ParkingFullState>() {
        return;
    }

    harness.select_menu_item("Save at this stop");

    let profile = harness.app.ctx.profile.as_ref().expect("a career");
    let saved = profile.active_trip.clone().expect("the drive was saved");
    assert_eq!(saved["kind"], "delivery");
    assert_eq!(saved["route_kind"], "corridor_itinerary");
    assert_eq!(saved["position_mi"].as_f64(), Some(stop_mi));
    let loaded = Profile::load(&profile.path()).expect("the saved profile reloads");
    assert_eq!(loaded.active_trip, Some(saved));
}

#[test]
fn test_opening_a_route_stop_secures_the_truck() {
    // A truck that rolled in just under the docking threshold must be parked
    // when the stop menu opens, so it cannot creep while the driver rests.
    let mut harness = a_drive("Secure At Stop");
    let stop_mi = harness.read_drive(|d| d.trip.stops[0].at_mi);
    harness.with_drive(move |drive, _| {
        drive.trip.position_mi = stop_mi;
        drive.truck_mut().velocity_mps = 0.0;
        drive.truck_mut().parking_brake = false; // rolled in still un-parked
        drive.truck_mut().throttle = 0.4; // idling in gear, creeping
    });

    press(&mut harness, Key::T);

    assert!(harness.state_is::<RestStopState>() || harness.state_is::<ParkingFullState>());
    assert!(harness.read_drive(|d| d.truck().parking_brake));
    assert_eq!(harness.read_drive(|d| d.truck().throttle), 0.0);
}

#[test]
fn test_poi_menu_uses_curated_roadside_assistance_label() {
    let mut harness = a_drive("Roadside Label");
    let at = harness.read_drive(|d| d.trip.position_mi);
    let mut stop = RoadStop::new("Example Turnpike Service Plaza", at, "service_plaza");
    stop.actions = ["park", "save", "roadside_assistance"]
        .iter()
        .map(|a| a.to_string())
        .collect();
    stop.services = ["parking", "roadside_assistance"]
        .iter()
        .map(|a| a.to_string())
        .collect();
    let mut state = rest_stop_screen(&mut harness, stop);

    let texts = labels(&mut state, &mut harness);

    assert!(
        texts.iter().any(|t| t == "Call roadside assistance"),
        "{texts:#?}"
    );
    assert!(
        texts.iter().all(|t| !t.to_lowercase().contains("osm")),
        "{texts:#?}"
    );
}

#[test]
fn test_rest_stop_sleep_warns_before_redundant_double_sleep() {
    let mut harness = a_drive("Double Sleep");
    let at = harness.read_drive(|d| d.trip.position_mi);
    let mut stop = RoadStop::new("Example Turnpike Service Plaza", at, "service_plaza");
    stop.actions = ["park", "sleep", "save"]
        .iter()
        .map(|a| a.to_string())
        .collect();
    stop.services = vec!["parking".to_string()];
    let mut state = rest_stop_screen(&mut harness, stop);

    // Force the driver fully rested: sleeping would gain nothing but time.
    rest_the_driver(&mut harness, 0.0);
    assert!(fully_rested(
        harness.app.ctx.profile.as_ref().expect("a career")
    ));

    let before = harness.read_drive(|d| d.trip.game_minutes);

    // First press only warns; the clock must not move.
    harness.clear_speech();
    activate(&mut state, &mut harness, "Sleep 10 hours");
    assert_eq!(harness.read_drive(|d| d.trip.game_minutes), before);
    assert!(
        last_main(&harness).starts_with("You are already rested"),
        "{}",
        last_main(&harness)
    );

    // Second consecutive press sleeps the full 10 hours.
    activate(&mut state, &mut harness, "Sleep 10 hours");
    assert!(
        (harness.read_drive(|d| d.trip.game_minutes) - (before + 600.0)).abs() < 1e-6,
        "{}",
        harness.read_drive(|d| d.trip.game_minutes)
    );

    // The guard covers sleeper-split rests too: a fresh visit warns first.
    let mut state = rest_stop_screen(&mut harness, state.stop.clone());
    rest_the_driver(&mut harness, 0.0);
    let split_before = harness.read_drive(|d| d.trip.game_minutes);
    harness.clear_speech();
    activate(&mut state, &mut harness, "Sleep 2 hours in sleeper berth");
    assert_eq!(harness.read_drive(|d| d.trip.game_minutes), split_before);
    assert!(
        last_main(&harness).starts_with("You are already rested"),
        "{}",
        last_main(&harness)
    );
}

#[test]
fn test_lot_sleep_warns_before_redundant_double_sleep() {
    // A non-sleeper stop only offers the poor-rest lot sleep, which floors
    // fatigue at the shoulder value -- so the guard must key off "hours fresh
    // and no more rest to gain", not full restedness, or it never fires.
    let mut harness = a_drive("Lot Sleep");
    let at = harness.read_drive(|d| d.trip.position_mi);
    let mut stop = RoadStop::new("Test Fuel Stop", at, "fuel_stop");
    stop.actions = ["park", "fuel", "break"]
        .iter()
        .map(|a| a.to_string())
        .collect();
    stop.services = vec!["parking".to_string()];
    let mut state = rest_stop_screen(&mut harness, stop);

    // Simulate a drive, then a first lot sleep so hours are fresh but fatigue
    // is stuck at the shoulder floor -- the state a driver is in right after
    // bedding down once.
    rest_the_driver(&mut harness, hos::FATIGUE_SHOULDER_FLOOR);

    let before = harness.read_drive(|d| d.trip.game_minutes);
    harness.clear_speech();
    activate(&mut state, &mut harness, "Sleep 10 hours in the lot"); // first press warns
    assert_eq!(harness.read_drive(|d| d.trip.game_minutes), before);
    assert!(
        last_main(&harness).starts_with("You are already rested"),
        "{}",
        last_main(&harness)
    );

    activate(&mut state, &mut harness, "Sleep 10 hours in the lot"); // second sleeps anyway
    assert!(
        (harness.read_drive(|d| d.trip.game_minutes) - (before + 600.0)).abs() < 1e-6,
        "{}",
        harness.read_drive(|d| d.trip.game_minutes)
    );
}

fn rest_the_driver(harness: &mut PlaytestHarness, fatigue: f64) {
    let profile = harness.app.ctx.profile.as_mut().expect("a career");
    profile.hos.driving_min = 0.0;
    profile.hos.duty_min = 0.0;
    profile.fatigue = fatigue;
}

/// The number (commas allowed) that follows `marker` in `text`.
fn dollars_after(text: &str, marker: &str) -> f64 {
    let start = text
        .find(marker)
        .unwrap_or_else(|| panic!("{marker:?} in {text:?}"))
        + marker.len();
    let digits: String = text[start..]
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == ',' || *c == '.')
        .filter(|c| *c != ',')
        .collect();
    digits
        .parse()
        .unwrap_or_else(|_| panic!("a number after {marker:?}: {digits:?}"))
}
