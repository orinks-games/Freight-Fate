//! Live cattle through the real driving state: the load goes aboard at the
//! shipper's gate, the hours stop counting inside the 150-mile circle, a
//! hard stop throws the cattle, the walk round gets one back up, and a save
//! keeps the whole load.

use ff_core::data::world::get_world;
use ff_core::models::jobs::{Job, CARGO_CATALOG};
use ff_core::models::profile::Profile;
use ff_core::sim::livestock::{Exemption, Footing, LimitMark, LoadPhase};
use ff_core::sim::weather::WeatherKind;
use freight_fate::app::testing::TestApp;
use freight_fate::states::driving::DrivingState;
use freight_fate::states::driving_core::DRIVE_PHASE_DELIVERY;

const DT: f64 = 1.0 / 60.0;

/// Feeder calves from the Dodge City sale barn to a Garden City feed yard.
fn a_cattle_drive(app: &mut TestApp) -> DrivingState {
    let (origin, destination) = ("Dodge City", "Garden City");
    let world = get_world();
    app.ctx.profile = Some(Profile::named_in("Cattle", origin));
    let route = world
        .supported_route(origin, destination, None)
        .expect("the world routes")
        .expect("the corridor is supported");
    let mut job = Job::new(
        &CARGO_CATALOG["livestock"],
        22.2,
        origin,
        "Dodge City Sale Barn",
        destination,
        route.miles(),
        1500.0,
        8.0,
    );
    job.origin_type = "sale_barn".to_string();
    job.destination_type = "feed_yard".to_string();
    job.destination_location = "Garden City Feed Yard".to_string();
    let mut drive = DrivingState::new(
        &mut app.ctx,
        job,
        route,
        Some(7),
        DRIVE_PHASE_DELIVERY,
        None,
    );
    drive.trip.set_npc_vehicles(Vec::new());
    drive.trip.weather.current = WeatherKind::Clear;
    drive.trip.truck.engine_on = true;
    drive
}

#[test]
fn a_cattle_delivery_leaves_with_the_load_aboard_and_the_clock_running() {
    let mut app = TestApp::new();
    let d = a_cattle_drive(&mut app);
    let load = d.livestock().expect("live cattle ride with the truck");
    assert_eq!(load.phase, LoadPhase::Aboard);
    assert_eq!(load.limit, LimitMark::Running);
    assert_eq!(load.exemption, Exemption::NearPickup);
    assert_eq!(load.head, 89);
    assert_eq!(
        d.livestock_status_clause(),
        "89 head. 1 minute aboard, 28 hours left."
    );
    assert_eq!(
        d.livestock_condition_clause().as_deref(),
        Some("Cattle settled")
    );
}

#[test]
fn hours_do_not_count_inside_the_circle_and_the_break_never_comes_due() {
    let mut app = TestApp::new();
    let mut d = a_cattle_drive(&mut app);
    d.trip.truck.velocity_mps = 55.0 / 2.23694;
    let before = app.ctx.profile.as_ref().unwrap().hos.driving_min;
    for _ in 0..600 {
        d.update_hours_and_fatigue(&mut app.ctx, DT);
    }
    let hos = &app.ctx.profile.as_ref().unwrap().hos;
    assert_eq!(
        hos.driving_min, before,
        "inside 150 air miles nothing counts"
    );

    // Out of the circle the wheel counts again, and the break still does not.
    if let Some(load) = d.trip.truck.livestock.as_mut() {
        load.stage(None, None, Some(Exemption::Counting), None, None);
    }
    for _ in 0..600 {
        d.update_hours_and_fatigue(&mut app.ctx, DT);
    }
    let hos = &app.ctx.profile.as_ref().unwrap().hos;
    assert!(hos.driving_min > before);
    assert_eq!(hos.since_break_min, 0.0);
}

#[test]
fn a_hard_stop_throws_the_cattle_and_says_so_without_the_recordings() {
    let mut app = TestApp::new();
    let mut d = a_cattle_drive(&mut app);
    d.trip.truck.velocity_mps = 50.0 / 2.23694;
    // A panic stop: well past the scramble line, held for a second.
    d.trip.truck.last_decel_g = 0.45;
    for _ in 0..60 {
        d.update_livestock(&mut app.ctx, DT);
    }
    let load = d.livestock().unwrap();
    assert!(
        matches!(load.footing, Footing::Scrambling | Footing::OneDown),
        "{:?}",
        load.footing
    );
    assert!(d.trip.truck.cargo_damage_pct > 0.0);
    let spoken = app.event_lines().join(" | ");
    assert!(
        spoken.contains("Cattle losing their footing."),
        "spoken: {spoken}"
    );
}

#[test]
fn checking_the_cattle_gets_one_back_on_its_feet() {
    let mut app = TestApp::new();
    let mut d = a_cattle_drive(&mut app);
    if let Some(load) = d.trip.truck.livestock.as_mut() {
        load.stage(None, Some(Footing::OneDown), None, None, Some(6.0));
    }
    assert_eq!(
        d.check_the_cattle().as_deref(),
        Some("1 down in the rear compartment. You got it back on its feet.")
    );
    assert_eq!(d.livestock().unwrap().footing, Footing::Restless);
    assert_eq!(d.check_the_cattle().as_deref(), Some("All standing."));
    assert_eq!(
        d.livestock_limit_left_text(),
        "22 hours left on the 28-hour limit."
    );
}

#[test]
fn a_saved_cattle_drive_keeps_its_load() {
    let mut app = TestApp::new();
    let mut d = a_cattle_drive(&mut app);
    if let Some(load) = d.trip.truck.livestock.as_mut() {
        load.stage(
            None,
            Some(Footing::Restless),
            Some(Exemption::Counting),
            None,
            Some(9.5),
        );
    }
    let saved = d.snapshot(&app.ctx);
    let restored = DrivingState::from_snapshot(&mut app.ctx, &saved).expect("the save restores");
    let load = restored.livestock().expect("the cattle are still aboard");
    assert_eq!(load.footing, Footing::Restless);
    assert_eq!(load.exemption, Exemption::Counting);
    assert!((load.hours_aboard() - 9.5).abs() < 1e-9);
    assert!(restored.livestock_departure_said);
}

/// The forecast of a sleep that would carry the cattle past their 28-hour
/// limit says so before the driver commits to it; one that wakes in time,
/// or a load already past the limit, adds nothing.
#[test]
fn a_sleep_forecast_warns_when_the_cattle_would_pass_the_limit() {
    use ff_core::sim::livestock::{CattleKind, LivestockLoad};
    use ff_core::sim::trip_models::RoadStop;
    use freight_fate::playtest::harness::{PlaytestHarness, StartDelivery};
    use freight_fate::states::base::Menu;
    use freight_fate::states::driving_menu_states::DriveRef;
    use freight_fate::states::driving_rest_states::RestStopState;

    const WARNING: &str = "The cattle pass the 28-hour limit while you sleep.";
    let mut harness = PlaytestHarness::new();
    harness.start_delivery(StartDelivery::named("Cattle Sleep"));
    let stage = |harness: &mut PlaytestHarness, hours: f64| {
        harness.with_drive(|drive, _| {
            let mut load = LivestockLoad::booked(
                CattleKind::FedCattle,
                19.0,
                (37.7, -100.0),
                (37.0, -100.9),
                7,
            );
            load.stage(Some(LoadPhase::Aboard), None, None, None, Some(hours));
            drive.trip.truck.livestock = Some(load);
        });
    };
    let forecast = |harness: &mut PlaytestHarness, label: &str| -> String {
        let at = harness.read_drive(|d| d.trip.position_mi);
        let mut stop = RoadStop::new("Test Travel Center", at, "travel_center");
        stop.actions = vec!["park".to_string(), "sleep".to_string()];
        stop.services = vec!["parking".to_string()];
        stop.parking = "confirmed".to_string();
        let handle = DriveRef::of(&harness.shared_driving().expect("a drive on the stack"));
        let mut state = RestStopState::with_drive(handle, stop, false);
        harness.clear_speech();
        let items = state.build_items(&mut harness.app.ctx);
        let row = items
            .iter()
            .find(|item| item.text(&state, &harness.app.ctx).starts_with(label))
            .cloned()
            .expect("the sleep row");
        (row.action)(&mut state, &mut harness.app.ctx);
        harness.app.main_lines().join(" | ")
    };

    // 25.5 hours aboard: a 3-hour sleep passes the limit, a 2-hour one wakes
    // with half an hour to spare.
    stage(&mut harness, 25.5);
    let three = forecast(&mut harness, "Sleep 3 hours");
    assert!(three.contains("Select again to sleep"), "{three}");
    assert!(three.contains(WARNING), "{three}");
    let two = forecast(&mut harness, "Sleep 2 hours");
    assert!(!two.contains(WARNING), "{two}");

    // Already past it: the status says so; the forecast does not repeat it.
    stage(&mut harness, 29.0);
    let past = forecast(&mut harness, "Sleep 10 hours");
    assert!(!past.contains(WARNING), "{past}");
}

/// The receiver's count and the claim reach the settlement screen itself,
/// the rows the player arrows through, not only the composed summary.
#[test]
fn the_settlement_screen_reads_the_receivers_count_and_the_claim() {
    use freight_fate::states::driving_menu_states::ArrivalState;

    let mut app = TestApp::new();
    let mut d = a_cattle_drive(&mut app);
    if let Some(load) = d.trip.truck.livestock.as_mut() {
        load.stage(None, None, None, None, Some(2.0));
    }
    // Past the claim line: the receiver counts the cattle that could not
    // walk off, and the carrier pays a claim.
    d.trip.truck.cargo_damage_pct = 52.0;
    d.trip.position_mi = d.trip.total_miles();
    d.trip.update(0.0);
    let arrival = ArrivalState::new(&mut app.ctx, &mut d);
    let rows = arrival.summary_lines.join(" | ");
    assert!(
        rows.contains("could not walk off. The carrier pays a claim."),
        "{rows}"
    );
    assert!(
        rows.contains("Receiver took the load but noted damage"),
        "{rows}"
    );
    // The count follows the load row it is about.
    let load_row = arrival
        .summary_lines
        .iter()
        .position(|l| l.starts_with("Load: "))
        .expect("a load row");
    assert!(arrival.summary_lines[load_row + 1].contains("could not walk off"));
}
