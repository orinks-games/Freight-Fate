//! Low-fuel mid-drive cue (issue #272): once when the tank crosses about
//! 15 percent, latched until a refill climbs back above the line, naming the
//! next fuel stop ahead.

use ff_core::data::world::get_world;
use ff_core::models::jobs::make_reposition_job;
use ff_core::models::profile::Profile;
use ff_core::sim::trip_models::RoadStop;
use ff_core::sim::weather::WeatherKind;
use freight_fate::app::testing::TestApp;
use freight_fate::states::driving::DrivingState;
use freight_fate::states::driving_core::*;

fn a_drive(app: &mut TestApp) -> DrivingState {
    let world = get_world();
    let mut profile = Profile::named_in("Fuel Warn", "Denver");
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
    // A known road ahead: a truck-parking lot with no pumps, then a travel
    // center that sells fuel, so the warning's "next fuel" is predictable.
    drive.trip.position_mi = 0.0;
    drive.trip.stops = vec![
        a_stop(
            "The 115 Truck Stop",
            10.0,
            "truck_parking",
            &["park", "save", "break", "sleep"],
        ),
        a_stop(
            "Pilot Travel Center",
            23.0,
            "travel_center",
            &["park", "save", "fuel", "food", "break", "sleep"],
        ),
    ];
    drive
}

fn a_stop(name: &str, at_mi: f64, stop_type: &str, actions: &[&str]) -> RoadStop {
    let mut stop = RoadStop::new(name, at_mi, stop_type);
    stop.actions = actions.iter().map(|a| a.to_string()).collect();
    stop
}

fn set_fuel_fraction(drive: &mut DrivingState, fraction: f64) {
    let tank = drive.trip.truck.specs.fuel_tank_gal;
    drive.trip.truck.fuel_gal = tank * fraction;
}

fn low_fuel_lines(app: &TestApp) -> Vec<String> {
    app.event_lines()
        .into_iter()
        .filter(|line| {
            let lower = line.to_lowercase();
            lower.contains("fuel low") || lower.contains("low fuel")
        })
        .collect()
}

#[test]
fn test_crossing_fifteen_percent_speaks_once() {
    let mut app = TestApp::new();
    let log = app.record_audio();
    let mut drive = a_drive(&mut app);
    set_fuel_fraction(&mut drive, 0.20);
    drive.low_fuel_said = false;
    app.clear_speech();

    drive.check_low_fuel_warning(&mut app.ctx);
    assert!(
        low_fuel_lines(&app).is_empty(),
        "above the line must stay quiet: {:?}",
        app.event_lines()
    );
    assert!(!drive.low_fuel_said);

    set_fuel_fraction(&mut drive, 0.15);
    drive.check_low_fuel_warning(&mut app.ctx);

    let lines = low_fuel_lines(&app);
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert_eq!(
        lines[0],
        "Fuel low: 15 percent. Next fuel: Pilot Travel Center, 23 miles."
    );
    assert!(drive.low_fuel_said);
    assert!(
        log.borrow()
            .played
            .iter()
            .any(|(key, _, _)| key == "ui/warning"),
        "{:#?}",
        log.borrow().played
    );
}

#[test]
fn test_staying_below_fifteen_percent_does_not_re_speak() {
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app);
    set_fuel_fraction(&mut drive, 0.14);
    drive.low_fuel_said = false;
    app.clear_speech();

    drive.check_low_fuel_warning(&mut app.ctx);
    assert_eq!(low_fuel_lines(&app).len(), 1);

    set_fuel_fraction(&mut drive, 0.10);
    drive.check_low_fuel_warning(&mut app.ctx);
    set_fuel_fraction(&mut drive, 0.05);
    drive.check_low_fuel_warning(&mut app.ctx);

    assert_eq!(
        low_fuel_lines(&app).len(),
        1,
        "staying under the line must not spam: {:?}",
        low_fuel_lines(&app)
    );
}

#[test]
fn test_refilling_above_then_dropping_again_speaks_again() {
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app);
    set_fuel_fraction(&mut drive, 0.12);
    drive.low_fuel_said = false;
    app.clear_speech();

    drive.check_low_fuel_warning(&mut app.ctx);
    assert_eq!(low_fuel_lines(&app).len(), 1);

    set_fuel_fraction(&mut drive, 0.40);
    drive.check_low_fuel_warning(&mut app.ctx);
    assert!(!drive.low_fuel_said, "a refill above the line re-arms");

    // The event pacer may still hold the first cue; clear both it and the
    // capture so this assertion is only about the second crossing.
    app.ctx.event_pacer.reset();
    app.clear_speech();
    set_fuel_fraction(&mut drive, 0.14);
    drive.check_low_fuel_warning(&mut app.ctx);

    let lines = low_fuel_lines(&app);
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert_eq!(
        lines[0],
        "Fuel low: 14 percent. Next fuel: Pilot Travel Center, 23 miles."
    );
    assert!(drive.low_fuel_said);
}

#[test]
fn test_urgent_only_still_speaks_the_low_fuel_warning() {
    let mut app = TestApp::new();
    app.ctx.settings.driving_speech = "urgent_only".to_string();
    let mut drive = a_drive(&mut app);
    set_fuel_fraction(&mut drive, 0.12);
    drive.low_fuel_said = false;
    app.clear_speech();

    drive.check_low_fuel_warning(&mut app.ctx);

    let lines = low_fuel_lines(&app);
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert_eq!(
        lines[0],
        "Fuel low: 12 percent. Next fuel: Pilot Travel Center, 23 miles."
    );
}

#[test]
fn test_streamer_safe_mode_does_not_mute_the_low_fuel_warning() {
    // Streamer-safe gates licensed radio streams, not Safety speech.
    let mut app = TestApp::new();
    app.ctx.settings.radio_streamer_safe = true;
    let mut drive = a_drive(&mut app);
    set_fuel_fraction(&mut drive, 0.12);
    drive.low_fuel_said = false;
    app.clear_speech();

    drive.check_low_fuel_warning(&mut app.ctx);

    assert_eq!(low_fuel_lines(&app).len(), 1, "{:?}", app.event_lines());
}

#[test]
fn test_empty_tank_rescue_still_runs_and_skips_the_low_fuel_cue() {
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app);
    drive.trip.truck.fuel_gal = 0.0;
    drive.low_fuel_said = false;
    app.clear_speech();

    drive.check_low_fuel_warning(&mut app.ctx);
    assert!(
        low_fuel_lines(&app).is_empty(),
        "dry tank is rescue, not the 15 percent cue: {:?}",
        app.event_lines()
    );

    drive.handle_out_of_fuel(&mut app.ctx);

    let spoken = app.event_lines().join(" ");
    assert!(spoken.contains("Out of fuel"), "{spoken}");
    assert!(spoken.contains("to restart the engine"), "{spoken}");
    assert!(drive.trip.truck.fuel_gal > 0.0);
    // Rescue gallons sit above the warn line on stock tanks, so the latch
    // clears on the next check and a later drop can warn again.
    drive.check_low_fuel_warning(&mut app.ctx);
    assert!(!drive.low_fuel_said);
}

#[test]
fn test_the_warning_skips_stops_behind_and_stops_without_pumps() {
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app);
    drive.trip.stops.insert(
        0,
        a_stop(
            "Behind Travel Center",
            2.0,
            "travel_center",
            &["park", "fuel", "food"],
        ),
    );
    drive.trip.position_mi = 5.0;
    set_fuel_fraction(&mut drive, 0.12);
    drive.low_fuel_said = false;
    app.clear_speech();

    drive.check_low_fuel_warning(&mut app.ctx);

    assert_eq!(
        low_fuel_lines(&app),
        vec!["Fuel low: 12 percent. Next fuel: Pilot Travel Center, 18 miles.".to_string()]
    );
}

#[test]
fn test_the_warning_says_when_no_fuel_stop_is_listed_ahead() {
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app);
    drive.trip.stops.retain(|stop| !stop.sells_fuel());
    set_fuel_fraction(&mut drive, 0.12);
    drive.low_fuel_said = false;
    app.clear_speech();

    drive.check_low_fuel_warning(&mut app.ctx);

    assert_eq!(
        low_fuel_lines(&app),
        vec!["Fuel low: 12 percent. No fuel stop before the destination.".to_string()]
    );
}

// -- The fuel range against the next fuel stop -----------------------------------

fn fuel_stop(name: &str, mile: f64) -> RoadStop {
    let mut stop = RoadStop::new(name, mile, "travel_center");
    stop.actions = vec!["fuel".into()];
    stop
}

fn range_lines(app: &TestApp) -> Vec<String> {
    app.event_lines()
        .into_iter()
        .filter(|line| line.starts_with("Fuel range"))
        .collect()
}

/// Ten gallons at the assumed six miles per gallon: a 60 mile range.
fn a_short_drive(app: &mut TestApp) -> DrivingState {
    let mut drive = a_drive(app);
    drive.trip.position_mi = 0.0;
    drive.trip.fuel_used_gal = 0.0;
    drive.trip.truck.fuel_gal = 10.0;
    drive.fuel_range_short_said = false;
    app.clear_speech();
    drive
}

#[test]
fn test_a_range_short_of_the_next_fuel_stop_says_so_once() {
    let mut app = TestApp::new();
    let mut drive = a_short_drive(&mut app);
    assert!(drive.trip.remaining_miles() > 80.0);
    // A bobtail-only island cannot take a rig with a trailer, so it is not
    // the next fuel.
    drive.trip.bobtail = false;
    let mut blocked = fuel_stop("Bobtail only", 30.0);
    blocked.vehicle_access = "bobtail_only".into();
    drive.trip.stops = vec![blocked, fuel_stop("Pumps", 80.0)];

    drive.check_fuel_range_warning(&mut app.ctx);
    drive.check_fuel_range_warning(&mut app.ctx);

    assert_eq!(
        range_lines(&app),
        vec!["Fuel range about 60 miles, short of the next fuel stop, 80 miles ahead.".to_string()]
    );
    assert!(drive.fuel_range_short_said);

    // A refill clears the latch; the next shortfall speaks again.
    drive.trip.truck.fuel_gal = 50.0;
    drive.check_fuel_range_warning(&mut app.ctx);
    assert!(!drive.fuel_range_short_said);
}

#[test]
fn test_a_range_that_reaches_the_next_fuel_stop_stays_quiet() {
    let mut app = TestApp::new();
    let mut drive = a_short_drive(&mut app);
    drive.trip.stops = vec![fuel_stop("Pumps", 50.0)];
    drive.check_fuel_range_warning(&mut app.ctx);
    assert!(range_lines(&app).is_empty(), "{:?}", app.event_lines());
}

#[test]
fn test_with_no_fuel_stop_ahead_the_range_is_held_to_the_destination() {
    let mut app = TestApp::new();
    let mut drive = a_short_drive(&mut app);
    drive.trip.stops = Vec::new();
    drive.check_fuel_range_warning(&mut app.ctx);
    assert_eq!(
        range_lines(&app),
        vec![
            "Fuel range about 60 miles, short of the destination, with no fuel stop ahead \
             on this route."
                .to_string()
        ]
    );

    // Terse speech keeps the numbers.
    drop(drive);
    drop(app);
    let mut app = TestApp::new();
    app.ctx.settings.driving_speech = "urgent_only".to_string();
    let mut drive = a_short_drive(&mut app);
    drive.trip.stops = vec![fuel_stop("Pumps", 80.0)];
    drive.check_fuel_range_warning(&mut app.ctx);
    assert_eq!(
        range_lines(&app),
        vec!["Fuel range 60 miles, next fuel 80 miles.".to_string()]
    );
}

#[test]
fn test_the_range_uses_the_runs_own_miles_per_gallon_once_it_has_one() {
    let mut app = TestApp::new();
    let mut drive = a_short_drive(&mut app);
    assert_eq!(drive.range_mpg(), ASSUMED_RANGE_MPG);
    // Forty miles on ten gallons: a heavy load at four miles per gallon.
    drive.trip.position_mi = 40.0;
    drive.trip.fuel_used_gal = 10.0;
    assert!((drive.range_mpg() - 4.0).abs() < 1e-9);
    assert!((drive.fuel_range_mi() - 40.0).abs() < 1e-9);
    drive.trip.stops = vec![fuel_stop("Pumps", 90.0)];
    drive.check_fuel_range_warning(&mut app.ctx);
    assert_eq!(
        range_lines(&app),
        vec!["Fuel range about 40 miles, short of the next fuel stop, 50 miles ahead.".to_string()]
    );
}

#[test]
fn test_the_low_fuel_warning_names_only_fuel_this_rig_can_reach_on_the_route() {
    // A bobtail-only island and pumps past the destination are not the next
    // fuel for a loaded rig on this run.
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app);
    drive.trip.bobtail = false;
    let mut island = fuel_stop("Bobtail Island", 5.0);
    island.vehicle_access = "bobtail_only".into();
    let past = drive.trip.remaining_miles() + 5.0;
    drive.trip.stops = vec![island, fuel_stop("Past The Gate", past)];
    set_fuel_fraction(&mut drive, 0.12);
    drive.low_fuel_said = false;
    app.clear_speech();

    drive.check_low_fuel_warning(&mut app.ctx);

    assert_eq!(
        low_fuel_lines(&app),
        vec!["Fuel low: 12 percent. No fuel stop before the destination.".to_string()]
    );
}
