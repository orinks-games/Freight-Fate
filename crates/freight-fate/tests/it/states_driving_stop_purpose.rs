//! T while rolling asks what the stop is for, then plans a stop that serves
//! it (owner, 2026-10-22: T planned a sleep stop for a driver who needed
//! fuel).

use ff_core::sim::trip_models::RoadStop;
use freight_fate::app::testing::TestApp;
use freight_fate::app::SharedState;
use freight_fate::playtest::menu::menu_rows;
use freight_fate::states::base::{InputEvent, Key};
use freight_fate::states::driving::DrivingState;
use freight_fate::states::driving_events::stop_plan::StopPlanPurpose;
use freight_fate::states::driving_menu_states::StopPurposeState;
use freight_fate::states::driving_rest_states::RestStopState;

use super::states_driving_menus_support::{a_drive_between, drive_and_ctx};

fn stop(name: &str, mile: f64, actions: &[&str]) -> RoadStop {
    let mut stop = RoadStop::new(name, mile, "travel_center");
    stop.actions = actions.iter().map(|a| a.to_string()).collect();
    stop.parking = "confirmed".into();
    stop
}

/// A rolling drive with a sleep stop first, then fuel, then fuel and sleep,
/// then food.
fn setup() -> (TestApp, SharedState) {
    let mut app = TestApp::new();
    let drive = a_drive_between(&mut app, "Buffalo", "Albany", "Purpose Driver");
    drive_and_ctx(&drive, &mut app, |d, ctx| {
        d.departure_checked = true;
        d.trip.position_mi = 10.0;
        d.trip.truck.velocity_mps = 25.0;
        d.trip.truck.fuel_gal = d.trip.truck.specs.fuel_tank_gal;
        d.trip.stops = vec![
            stop("Sleeper Lot", 14.0, &["sleep", "break"]),
            stop("Pumps Only", 20.0, &["fuel"]),
            stop("Full Service", 30.0, &["fuel", "sleep", "break", "food"]),
        ];
        ctx.profile.as_mut().unwrap().hos.duty_min = 0.0;
    });
    app.clear_speech();
    (app, drive)
}

fn press_t(app: &mut TestApp, drive: &SharedState) {
    drive_and_ctx(drive, app, |d, ctx| {
        d.handle_key_event(ctx, &InputEvent::key(Key::T));
    });
    app.ctx.run_deferred();
}

fn purpose_rows(app: &TestApp) -> (Vec<String>, usize) {
    let state = app.ctx.state().unwrap();
    let borrowed = state.borrow();
    assert!(
        borrowed.as_any().is::<StopPurposeState>(),
        "T did not open the purpose list"
    );
    menu_rows(&*borrowed, &app.ctx).unwrap()
}

/// Arrow to the row that starts with `prefix` and press Enter.
fn choose(app: &mut TestApp, prefix: &str) {
    let (rows, _) = purpose_rows(app);
    for _ in 0..=rows.len() {
        let (rows, focus) = purpose_rows(app);
        let key = if rows[focus].starts_with(prefix) {
            Key::Return
        } else {
            Key::Down
        };
        let state = app.ctx.state().unwrap();
        state
            .borrow_mut()
            .handle_event(&mut app.ctx, &InputEvent::key(key));
        app.ctx.run_deferred();
        if key == Key::Return {
            return;
        }
    }
    panic!("no row starts with {prefix:?}: {rows:?}");
}

fn planned(drive: &SharedState, app: &mut TestApp) -> Option<String> {
    drive_and_ctx(drive, app, |d: &mut DrivingState, _| {
        d.trip.planned_stop().map(|stop| stop.name.clone())
    })
}

#[test]
fn t_lists_each_purpose_with_its_distance() {
    let (mut app, drive) = setup();
    press_t(&mut app, &drive);
    let (rows, focus) = purpose_rows(&app);
    assert_eq!(
        rows,
        vec![
            "Fuel: 10 miles",
            "Break: 4.0 miles",
            "Sleep: 4.0 miles",
            "Fuel and sleep: 20 miles",
            "Food and coffee: 20 miles",
        ]
    );
    // A full tank and no hours advice: the list opens on its first row.
    assert_eq!(focus, 0);
    assert!(
        planned(&drive, &mut app).is_none(),
        "the list planned nothing yet"
    );
}

#[test]
fn choosing_fuel_plans_the_next_pumps_not_the_sleep_lot() {
    let (mut app, drive) = setup();
    press_t(&mut app, &drive);
    choose(&mut app, "Fuel:");
    assert_eq!(planned(&drive, &mut app).as_deref(), Some("Pumps Only"));
    let said = app.main_lines().join(" ");
    assert!(
        said.contains("Fuel stop planned, travel center: Pumps Only"),
        "{said}"
    );
    assert!(!said.contains("Sleeper Lot"), "{said}");
}

#[test]
fn each_purpose_plans_a_stop_that_serves_it() {
    for (prefix, expected, line) in [
        ("Sleep:", "Sleeper Lot", "Sleep stop planned"),
        ("Break:", "Sleeper Lot", "Break stop planned"),
        (
            "Fuel and sleep:",
            "Full Service",
            "Fuel and sleep stop planned",
        ),
        ("Food and coffee:", "Full Service", "Food stop planned"),
    ] {
        let (mut app, drive) = setup();
        press_t(&mut app, &drive);
        choose(&mut app, prefix);
        assert_eq!(
            planned(&drive, &mut app).as_deref(),
            Some(expected),
            "{prefix}"
        );
        let said = app.main_lines().join(" ");
        assert!(said.contains(line), "{prefix}: {said}");
    }
}

#[test]
fn a_low_tank_opens_the_list_on_fuel_and_the_stop_menu_on_the_pump() {
    let (mut app, drive) = setup();
    drive_and_ctx(&drive, &mut app, |d, ctx| {
        d.trip.truck.fuel_gal = d.trip.truck.specs.fuel_tank_gal * 0.1;
        // Hours that would otherwise put the cursor on the break.
        ctx.settings.hos_planning_hints = true;
        let hos = &mut ctx.profile.as_mut().unwrap().hos;
        hos.driving_min = 360.0;
        hos.since_break_min = 360.0;
    });
    press_t(&mut app, &drive);
    let (rows, focus) = purpose_rows(&app);
    assert!(rows[focus].starts_with("Fuel:"), "{rows:?} at {focus}");
    choose(&mut app, "Fuel:");
    let pumps = drive_and_ctx(&drive, &mut app, |d, ctx| {
        let pumps = d.trip.planned_stop().unwrap().clone();
        d.trip.position_mi = pumps.at_mi;
        d.trip.truck.velocity_mps = 0.0;
        d.open_poi_stop(ctx, &pumps, false, None);
        pumps
    });
    assert_eq!(pumps.name, "Pumps Only");
    app.ctx.run_deferred();
    let state = app.ctx.state().unwrap();
    let borrowed = state.borrow();
    assert!(borrowed.as_any().is::<RestStopState>());
    let (rows, focus) = menu_rows(&*borrowed, &app.ctx).unwrap();
    assert!(rows[focus].starts_with("Refuel "), "{rows:?} at {focus}");
}

#[test]
fn a_fuel_plan_past_the_range_says_so() {
    let (mut app, drive) = setup();
    drive_and_ctx(&drive, &mut app, |d, _| {
        d.trip.truck.fuel_gal = 0.5;
    });
    press_t(&mut app, &drive);
    choose(&mut app, "Fuel:");
    let said = app.main_lines().join(" ");
    assert!(said.contains("Fuel range only"), "{said}");
}

#[test]
fn a_purpose_nothing_ahead_serves_says_so_and_plans_nothing() {
    let (mut app, drive) = setup();
    drive_and_ctx(&drive, &mut app, |d, _| {
        d.trip.stops = vec![stop("Sleeper Lot", 14.0, &["sleep"])];
    });
    press_t(&mut app, &drive);
    let (rows, _) = purpose_rows(&app);
    assert_eq!(rows[0], "Fuel: none ahead");
    choose(&mut app, "Fuel:");
    assert!(planned(&drive, &mut app).is_none());
    assert!(app.main_lines().join(" ").contains("No fuel stop ahead."));
}

#[test]
fn t_with_a_plan_cancels_it_without_the_list() {
    let (mut app, drive) = setup();
    press_t(&mut app, &drive);
    choose(&mut app, "Fuel:");
    app.clear_speech();
    press_t(&mut app, &drive);
    assert!(planned(&drive, &mut app).is_none());
    assert!(!app
        .ctx
        .state()
        .unwrap()
        .borrow()
        .as_any()
        .is::<StopPurposeState>());
    assert!(app.main_lines().join(" ").contains("Planned stop canceled"));
}

#[test]
fn a_fuel_plan_survives_a_save_and_resume() {
    let (mut app, drive) = setup();
    press_t(&mut app, &drive);
    choose(&mut app, "Fuel:");
    let snapshot = drive_and_ctx(&drive, &mut app, |d, ctx| d.snapshot(ctx));
    assert_eq!(snapshot["selected_stop_purpose"], "fuel");
    let resumed = DrivingState::from_snapshot(&mut app.ctx, &snapshot).unwrap();
    assert_eq!(resumed.selected_stop_purpose, Some(StopPlanPurpose::Fuel));
}
