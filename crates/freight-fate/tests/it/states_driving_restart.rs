//! The cycle restart at a rest stop: the row after the full night or the
//! lot's night, the forecast on the first pick, the rest on the second, and
//! the "still used up" sentence after a night that left the cycle spent.

use ff_core::sim::hos::{CycleEntry, HosRules};
use ff_core::sim::trip_models::RoadStop;
use ff_core::sim::weather::WeatherKind;

use freight_fate::playtest::{PlaytestHarness, RouteSetup};
use freight_fate::states::base::Menu;
use freight_fate::states::driving_menu_states::DriveRef;
use freight_fate::states::driving_rest_states::{ParkingFullState, RestStopState};

/// A drive out of Austin with a worked shift and a used-up cycle under
/// `rules`.
fn a_spent_week(rules: HosRules) -> PlaytestHarness {
    let mut harness = PlaytestHarness::new();
    harness.start_route(
        "austin_tx_us",
        "dallas_tx_us",
        RouteSetup::default().cities(&["austin_tx_us", "dallas_tx_us"]),
    );
    harness.with_drive(|drive, ctx| {
        drive.tutorial = None;
        drive.departure_checked = true;
        drive.trip.hazard_check_mi = 1e9;
        drive.trip.inspection_check_mi = 1e9;
        drive.trip.weather.current = WeatherKind::Clear;
        drive.trip.position_mi = 30.0;
        // Plenty of deadline, so the forecast reads it ahead.
        drive.job.deadline_game_h = drive.trip.game_minutes / 60.0 + 100.0;
        let p = ctx.profile.as_mut().expect("a career");
        p.hos.set_rules(rules);
        p.hos.drive(300.0);
        p.hos.cycle_entries = vec![CycleEntry {
            end_min: p.hos.cycle_clock_min,
            on_duty_min: 70.0 * 60.0,
        }];
        p.fatigue = 60.0;
    });
    harness
}

fn a_stop(harness: &PlaytestHarness, actions: &[&str]) -> RestStopState {
    let at = harness.read_drive(|d| d.trip.position_mi);
    let mut stop = RoadStop::new("Example Truck Stop", at, "truck_stop");
    stop.actions = actions.iter().map(|a| a.to_string()).collect();
    stop.services = vec!["parking".to_string()];
    let handle = DriveRef::of(&harness.shared_driving().expect("a drive on the stack"));
    RestStopState::with_drive(handle, stop, false)
}

fn labels(state: &mut RestStopState, harness: &mut PlaytestHarness) -> Vec<String> {
    let items = state.build_items(&mut harness.app.ctx);
    items
        .iter()
        .map(|item| item.text(state, &harness.app.ctx))
        .collect()
}

/// Select the row with this exact label once.
fn pick(state: &mut RestStopState, harness: &mut PlaytestHarness, label: &str) {
    let items = state.build_items(&mut harness.app.ctx);
    let found = items
        .iter()
        .find(|item| item.text(state, &harness.app.ctx) == label)
        .cloned()
        .unwrap_or_else(|| panic!("no {label:?} row: {:?}", labels(state, harness)));
    (found.action)(state, &mut harness.app.ctx);
}

fn game_minutes(harness: &PlaytestHarness) -> f64 {
    harness.read_drive(|d| d.trip.game_minutes)
}

#[test]
fn a_sleeper_stop_forecasts_then_takes_the_restart() {
    let mut harness = a_spent_week(HosRules::Us);
    let mut state = a_stop(&harness, &["park", "sleep", "save"]);
    Menu::enter(&mut state, &mut harness.app.ctx);
    let rows = labels(&mut state, &mut harness);
    let night = rows
        .iter()
        .position(|r| r == "Sleep 10 hours")
        .expect("the full night");
    assert_eq!(rows[night + 1], "Take a 34-hour restart", "{rows:?}");

    let before = game_minutes(&harness);
    harness.clear_speech();
    pick(&mut state, &mut harness, "Take a 34-hour restart");
    let said = harness.transcript_text();
    assert!(
        said.contains(
            "Rest 34 hours at sleeper berth. 34 hours off in a row restart your cycle and reset \
             your driving hours and legal driving window."
        ),
        "{said}"
    );
    assert!(said.contains("The delivery deadline will be in "), "{said}");
    assert!(said.contains("Select again to rest."), "{said}");
    assert_eq!(game_minutes(&harness), before, "the forecast moves nothing");

    harness.clear_speech();
    pick(&mut state, &mut harness, "Take a 34-hour restart");
    let said = harness.transcript_text();
    assert!(said.contains("You rested 34 hours and woke "), "{said}");
    assert!(
        said.contains("Driving, duty and cycle clocks reset."),
        "{said}"
    );
    assert!(
        (game_minutes(&harness) - before - 34.0 * 60.0).abs() < 1e-6,
        "{}",
        game_minutes(&harness) - before
    );
    let p = harness.app.ctx.profile.as_ref().expect("a career");
    assert!(p.hos.restart_complete());
    assert_eq!(p.hos.cycle_used_min(), 0.0);
    assert!(p.achievements.iter().any(|a| a == "restart_34"));
    let rows = labels(&mut state, &mut harness);
    assert!(
        !rows.iter().any(|r| r.contains("restart")),
        "the row goes: {rows:?}"
    );
    // Focus steps back to the night's row, never onto a row that slid up.
    let focused = Menu::current_text(&state, &harness.app.ctx);
    assert!(focused.starts_with("Sleep 10 hours"), "{focused}");
}

#[test]
fn a_lot_stop_offers_the_restart_in_the_lot() {
    let mut harness = a_spent_week(HosRules::Us);
    let mut state = a_stop(&harness, &["park", "break"]);
    Menu::enter(&mut state, &mut harness.app.ctx);
    let rows = labels(&mut state, &mut harness);
    let lot = rows
        .iter()
        .position(|r| r == "Sleep 10 hours in the lot")
        .expect("the lot's night");
    assert_eq!(
        rows[lot + 1],
        "Take a 34-hour restart in the lot",
        "{rows:?}"
    );

    let before = game_minutes(&harness);
    harness.clear_speech();
    pick(
        &mut state,
        &mut harness,
        "Take a 34-hour restart in the lot",
    );
    let said = harness.transcript_text();
    assert!(said.contains("Rest 34 hours at the lot. "), "{said}");
    assert!(said.contains("Select again to rest."), "{said}");
    harness.clear_speech();
    pick(
        &mut state,
        &mut harness,
        "Take a 34-hour restart in the lot",
    );
    let said = harness.transcript_text();
    assert!(said.contains("You rested 34 hours and woke "), "{said}");
    assert!(
        said.contains("Driving, duty and cycle clocks reset."),
        "{said}"
    );
    assert!((game_minutes(&harness) - before - 34.0 * 60.0).abs() < 1e-6);
    let p = harness.app.ctx.profile.as_ref().expect("a career");
    assert!(p.hos.restart_complete());
    // Poor rest in the lot: never below 30.
    assert!(p.fatigue >= 30.0, "{}", p.fatigue);
}

#[test]
fn a_night_that_leaves_the_cycle_spent_says_so_and_the_row_counts_it() {
    let mut harness = a_spent_week(HosRules::Us);
    let mut state = a_stop(&harness, &["park", "sleep", "save"]);
    Menu::enter(&mut state, &mut harness.app.ctx);
    pick(&mut state, &mut harness, "Sleep 10 hours");
    harness.clear_speech();
    pick(&mut state, &mut harness, "Sleep 10 hours");
    let said = harness.transcript_text();
    assert!(
        said.contains(
            "Driving and duty clocks reset. Your cycle is still used up. Rest 24 hours more to \
             restart it."
        ),
        "{said}"
    );
    let label = "Rest 24 hours more to finish a 34-hour restart";
    let before = game_minutes(&harness);
    pick(&mut state, &mut harness, label);
    harness.clear_speech();
    pick(&mut state, &mut harness, label);
    let said = harness.transcript_text();
    assert!(
        said.contains("You rested 24 hours more, 34 hours off in a row, and woke "),
        "{said}"
    );
    assert!((game_minutes(&harness) - before - 24.0 * 60.0).abs() < 1e-6);
    assert!(harness
        .app
        .ctx
        .profile
        .as_ref()
        .expect("a career")
        .hos
        .restart_complete());
}

#[test]
fn canadian_rules_at_a_stop_read_36_hours_off() {
    let mut harness = a_spent_week(HosRules::CanadaSouth60);
    let mut state = a_stop(&harness, &["park", "sleep", "save"]);
    Menu::enter(&mut state, &mut harness.app.ctx);
    let rows = labels(&mut state, &mut harness);
    assert!(rows.iter().any(|r| r == "Take 36 hours off"), "{rows:?}");
}

#[test]
fn a_lot_stop_offers_the_restart_in_a_motel_room_too() {
    let mut harness = a_spent_week(HosRules::Us);
    let mut state = a_stop(&harness, &["park", "break"]);
    Menu::enter(&mut state, &mut harness.app.ctx);
    let label = "Take a 34-hour restart in a motel room for 190 dollars";
    let rows = labels(&mut state, &mut harness);
    let motel = rows
        .iter()
        .position(|r| r == "Motel room: sleep 10 hours for 95 dollars")
        .expect("the motel's night");
    assert_eq!(rows[motel + 1], label, "{rows:?}");

    let money_before = harness.app.ctx.profile.as_ref().expect("a career").money();
    let before = game_minutes(&harness);
    harness.clear_speech();
    pick(&mut state, &mut harness, label);
    let said = harness.transcript_text();
    assert!(said.contains("Rest 34 hours at a motel room. "), "{said}");
    assert!(
        said.contains("The room costs 190 dollars. Select again to rest."),
        "{said}"
    );
    assert_eq!(game_minutes(&harness), before, "the forecast moves nothing");
    harness.clear_speech();
    pick(&mut state, &mut harness, label);
    let said = harness.transcript_text();
    assert!(
        said.contains("Motel, 190 dollars. You rested 34 hours and woke rested."),
        "{said}"
    );
    assert!(
        said.contains("Driving, duty and cycle clocks reset."),
        "{said}"
    );
    assert!((game_minutes(&harness) - before - 34.0 * 60.0).abs() < 1e-6);
    let p = harness.app.ctx.profile.as_ref().expect("a career");
    assert!(p.hos.restart_complete());
    assert!((money_before - p.money() - 190.0).abs() < 1e-6);
    assert!(p.fatigue < 30.0, "a real bed: {}", p.fatigue);
}

#[test]
fn a_motel_restart_the_driver_cannot_pay_for_says_the_price() {
    let mut harness = a_spent_week(HosRules::Us);
    harness.with_drive(|_, ctx| {
        let p = ctx.profile.as_mut().expect("a career");
        let cash = p.money();
        p.spend(cash - 150.0);
    });
    let mut state = a_stop(&harness, &["park", "break"]);
    Menu::enter(&mut state, &mut harness.app.ctx);
    let before = game_minutes(&harness);
    harness.clear_speech();
    pick(
        &mut state,
        &mut harness,
        "Take a 34-hour restart in a motel room for 190 dollars",
    );
    let said = harness.transcript_text();
    assert!(
        said.contains("A motel room costs 190 dollars and you have 150."),
        "{said}"
    );
    assert_eq!(game_minutes(&harness), before);
}

#[test]
fn a_full_lot_offers_the_restart_in_a_motel_room() {
    let mut harness = a_spent_week(HosRules::Us);
    let at = harness.read_drive(|d| d.trip.position_mi);
    let mut stop = RoadStop::new("Example Rest Area", at, "public_rest_area");
    stop.actions = ["park", "save", "break", "sleep"]
        .iter()
        .map(|a| a.to_string())
        .collect();
    let handle = DriveRef::of(&harness.shared_driving().expect("a drive on the stack"));
    let mut state = ParkingFullState::with_drive(handle, stop);
    let items = state.build_items(&mut harness.app.ctx);
    let rows: Vec<String> = items
        .iter()
        .map(|item| item.text(&state, &harness.app.ctx))
        .collect();
    let label = "Take a 34-hour restart in a motel room for 190 dollars";
    let motel = rows
        .iter()
        .position(|r| r == "Motel room: sleep 10 hours for 95 dollars")
        .expect("the motel's night");
    assert_eq!(rows[motel + 1], label, "{rows:?}");

    let before = game_minutes(&harness);
    harness.clear_speech();
    let row = items
        .iter()
        .find(|item| item.text(&state, &harness.app.ctx) == label)
        .cloned()
        .expect("the restart row");
    (row.action)(&mut state, &mut harness.app.ctx);
    let said = harness.transcript_text();
    assert!(
        said.contains("Motel, 190 dollars. You rested 34 hours and woke rested."),
        "{said}"
    );
    assert!((game_minutes(&harness) - before - 34.0 * 60.0).abs() < 1e-6);
    assert!(harness
        .app
        .ctx
        .profile
        .as_ref()
        .expect("a career")
        .hos
        .restart_complete());
}
