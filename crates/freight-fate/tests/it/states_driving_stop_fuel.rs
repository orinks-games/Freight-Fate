//! Stop callouts and arrivals say whether a stop sells fuel (issue #272
//! follow-up): a driver exited at "The 115 Truck Stop", a truck-parking lot,
//! and found no pumps.

use ff_core::sim::trip_models::RoadStop;
use ff_core::sim::weather::WeatherKind;

use freight_fate::app::testing::{FakeClock, TestApp};
use freight_fate::playtest::harness::{PlaytestHarness, StartDelivery};
use freight_fate::states::driving::DrivingState;
use freight_fate::states::driving_core::*;
use freight_fate::states::driving_menu_states::DriveRef;
use freight_fate::states::driving_rest_states::RestStopState;

use ff_core::data::world::get_world;
use ff_core::models::jobs::make_reposition_job;
use ff_core::models::profile::Profile;

fn a_drive(app: &mut TestApp) -> DrivingState {
    let world = get_world();
    let mut profile = Profile::named_in("Stop Fuel", "Denver");
    profile.tutorial_done = true;
    app.ctx.profile = Some(profile);
    let job = make_reposition_job(world, "Denver", "Cheyenne", false, None)
        .expect("Denver to Cheyenne is a supported reposition");
    let route = world
        .shortest_route("Denver", "Cheyenne", None, false)
        .expect("the world routes")
        .expect("Denver to Cheyenne has a route");
    let mut drive = DrivingState::new(
        &mut app.ctx,
        job,
        route,
        Some(0),
        DRIVE_PHASE_DELIVERY,
        Some(12.0),
    );
    drive.trip.set_npc_vehicles(Vec::new());
    drive.trip.weather.current = WeatherKind::Clear;
    drive.tutorial = None;
    drive.departure_checked = true;
    drive
}

fn truck_parking(at_mi: f64) -> RoadStop {
    let mut stop = RoadStop::new("The 115 Truck Stop", at_mi, "truck_parking");
    stop.actions = ["park", "save", "break", "sleep"]
        .iter()
        .map(|a| a.to_string())
        .collect();
    stop.parking = "confirmed".to_string();
    stop
}

fn travel_center(at_mi: f64) -> RoadStop {
    let mut stop = RoadStop::new("Pilot Travel Center", at_mi, "travel_center");
    stop.actions = ["park", "save", "fuel", "food", "break", "sleep"]
        .iter()
        .map(|a| a.to_string())
        .collect();
    stop.parking = "confirmed".to_string();
    stop
}

/// Put the truck at `mile`, let the trip notice what is ahead, and hand
/// every event to the driving layer; then let the ambient queue speak.
fn roll_to(app: &mut TestApp, d: &mut DrivingState, clock: &FakeClock, mile: f64) {
    d.trip.position_mi = mile;
    d.trip.check_stops();
    let events = std::mem::take(&mut d.trip.events);
    for event in &events {
        d.handle_trip_event(&mut app.ctx, event);
    }
    for _ in 0..4 {
        clock.advance(AMBIENT_EVENT_SPACING_S);
        d.update_ambient_events(&mut app.ctx, AMBIENT_EVENT_SPACING_S);
    }
}

fn callouts(app: &TestApp) -> Vec<String> {
    app.event_lines()
        .into_iter()
        .filter(|line| line.contains("115 Truck Stop") || line.contains("Pilot Travel Center"))
        .collect()
}

#[test]
fn test_driving_past_a_parking_lot_and_a_travel_center_says_which_sells_fuel() {
    let mut app = TestApp::new();
    let clock = app.fake_pacer_clock();
    let mut d = a_drive(&mut app);
    d.trip.stops = vec![truck_parking(3.0), travel_center(14.0)];
    d.trip.announced_stops.clear();
    app.clear_speech();

    roll_to(&mut app, &mut d, &clock, 0.0);
    roll_to(&mut app, &mut d, &clock, 10.0);

    let said = callouts(&app);
    assert_eq!(said.len(), 2, "{:?}", app.event_lines());
    assert!(said[0].contains("The 115 Truck Stop"), "{said:?}");
    assert!(said[0].contains("No fuel."), "{said:?}");
    assert!(said[1].contains("Pilot Travel Center"), "{said:?}");
    assert!(said[1].contains("Fuel."), "{said:?}");
    assert!(!said[1].contains("No fuel."), "{said:?}");
}

#[test]
fn test_terse_callouts_keep_only_the_no_fuel_verdict() {
    let mut app = TestApp::new();
    app.ctx.settings.driving_speech = "quiet".to_string();
    let clock = app.fake_pacer_clock();
    let mut d = a_drive(&mut app);
    d.trip.stops = vec![truck_parking(3.0), travel_center(14.0)];
    d.trip.announced_stops.clear();
    app.clear_speech();

    roll_to(&mut app, &mut d, &clock, 0.0);
    roll_to(&mut app, &mut d, &clock, 10.0);

    let said = callouts(&app);
    let parking = said
        .iter()
        .find(|line| line.contains("115 Truck Stop"))
        .unwrap_or_else(|| panic!("the parking lot was not called: {:?}", app.event_lines()));
    assert!(parking.ends_with("No fuel."), "{parking}");
    if let Some(center) = said.iter().find(|line| line.contains("Pilot")) {
        assert!(!center.contains("fuel"), "{center}");
    }
}

fn arrive_at(stop: RoadStop) -> Vec<String> {
    let mut harness = PlaytestHarness::new();
    harness.start_delivery(StartDelivery::named("Stop Fuel Arrival"));
    harness.with_drive(|drive, _| {
        drive.tutorial = None;
        drive.departure_checked = true;
    });
    let handle = DriveRef::of(&harness.shared_driving().expect("a drive on the stack"));
    let mut state = RestStopState::with_drive(handle, stop, false);
    harness.clear_speech();
    harness.with_drive(|drive, ctx| state.enter_over_drive(ctx, drive));
    harness.app.main_lines()
}

#[test]
fn test_arriving_at_a_stop_without_pumps_says_so() {
    let lines = arrive_at(truck_parking(0.0));
    let joined = lines.join(" ");
    assert!(joined.contains("No fuel here."), "{joined}");
}

#[test]
fn test_arriving_at_a_travel_center_says_nothing_about_missing_fuel() {
    let lines = arrive_at(travel_center(0.0));
    let joined = lines.join(" ");
    assert!(!joined.contains("No fuel"), "{joined}");
}

#[test]
fn test_arriving_at_a_weigh_station_is_left_alone() {
    let mut scale = RoadStop::new("Test Scale", 0.0, "weigh_station");
    scale.actions = vec!["inspect".to_string()];
    let joined = arrive_at(scale).join(" ");
    assert!(joined.contains("Inspection station."), "{joined}");
    assert!(!joined.contains("fuel"), "{joined}");
}

fn public_rest_area(at_mi: f64) -> RoadStop {
    let mut stop = RoadStop::new("Lone Tree Rest Area", at_mi, "public_rest_area");
    stop.actions = ["park", "save", "break", "sleep"]
        .iter()
        .map(|a| a.to_string())
        .collect();
    stop.parking = "confirmed".to_string();
    stop
}

#[test]
fn test_stops_nobody_fuels_at_say_nothing_about_fuel() {
    // A rest area or a repair shop never had pumps to miss: "No fuel." at
    // every one would be noise the driver sits through.
    let mut app = TestApp::new();
    let clock = app.fake_pacer_clock();
    let mut d = a_drive(&mut app);
    d.trip.stops = vec![public_rest_area(3.0)];
    d.trip.announced_stops.clear();
    app.clear_speech();

    roll_to(&mut app, &mut d, &clock, 0.0);

    let said: Vec<String> = app
        .event_lines()
        .into_iter()
        .filter(|line| line.contains("Lone Tree"))
        .collect();
    assert_eq!(said.len(), 1, "{:?}", app.event_lines());
    assert!(!said[0].to_lowercase().contains("fuel"), "{said:?}");
    drop(d);
    drop(app);

    let joined = arrive_at(public_rest_area(0.0)).join(" ");
    assert!(!joined.contains("No fuel"), "{joined}");

    let mut shop = RoadStop::new("Front Range Truck Repair", 0.0, "repair_shop");
    shop.actions = vec!["park".into(), "save".into(), "repair".into()];
    assert_eq!(shop.fuel_callout(), None);
    let joined = arrive_at(shop).join(" ");
    assert!(!joined.contains("No fuel"), "{joined}");
}
