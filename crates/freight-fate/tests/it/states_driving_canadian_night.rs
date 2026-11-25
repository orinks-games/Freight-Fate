//! The full night at a rest stop in Canada (brief 2 review, 2026-11-25): 8
//! hours only under Canadian rules on a road that stays in Canada, and the
//! board's plan after it starting from the counts its route holds the driver
//! to.

use ff_core::models::jobs::plan_hos;
use ff_core::sim::hos::{FullNight, HosRules};
use ff_core::sim::trip_models::RoadStop;
use ff_core::sim::weather::WeatherKind;

use freight_fate::playtest::{PlaytestHarness, RouteSetup};
use freight_fate::states::base::Menu;
use freight_fate::states::driving_menu_states::DriveRef;
use freight_fate::states::driving_rest_states::RestStopState;

fn a_drive(cities: &[&str], carrier: Option<&str>) -> PlaytestHarness {
    let mut harness = PlaytestHarness::new();
    let origin = cities[0];
    let destination = cities[cities.len() - 1];
    harness.start_route(origin, destination, RouteSetup::default().cities(cities));
    harness.with_drive(|drive, ctx| {
        drive.tutorial = None;
        drive.departure_checked = true;
        drive.trip.hazard_check_mi = 1e9;
        drive.trip.inspection_check_mi = 1e9;
        drive.trip.weather.current = WeatherKind::Clear;
        let p = ctx.profile.as_mut().expect("a career");
        if let Some(carrier) = carrier {
            p.carrier_key = carrier.to_string();
        }
        p.fatigue = 70.0;
    });
    harness
}

/// Put the truck at `at_mi` under `rules`, `hours` into its shift.
fn spend_a_shift(harness: &mut PlaytestHarness, at_mi: f64, rules: HosRules, hours: f64) {
    harness.with_drive(|drive, ctx| {
        drive.trip.position_mi = at_mi;
        let p = ctx.profile.as_mut().expect("a career");
        p.hos.set_rules(rules);
        p.hos.drive(hours * 60.0);
    });
}

fn sleep_stop(harness: &PlaytestHarness) -> RestStopState {
    let at = harness.read_drive(|d| d.trip.position_mi);
    let mut stop = RoadStop::new("Example Truck Stop", at, "truck_stop");
    stop.actions = ["park", "sleep", "save"]
        .iter()
        .map(|a| a.to_string())
        .collect();
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

/// Select the row with this exact label twice: the forecast, then the sleep.
fn sleep_on(state: &mut RestStopState, harness: &mut PlaytestHarness, label: &str) {
    for _ in 0..2 {
        let items = state.build_items(&mut harness.app.ctx);
        let found = items
            .iter()
            .find(|item| item.text(state, &harness.app.ctx) == label)
            .cloned()
            .unwrap_or_else(|| panic!("no {label:?} row: {:?}", labels(state, harness)));
        (found.action)(state, &mut harness.app.ctx);
    }
}

fn full_night(harness: &PlaytestHarness) -> FullNight {
    let stays = harness.read_drive(|d| d.trip.road_ahead_stays_in_canada());
    harness
        .app
        .ctx
        .profile
        .as_ref()
        .expect("a career")
        .hos
        .full_night(stays)
}

fn first_booth_mi(harness: &PlaytestHarness) -> f64 {
    harness.read_drive(|d| {
        d.trip
            .border_booths
            .first()
            .map(|booth| booth.at_mi)
            .expect("the route crosses at a booth")
    })
}

#[test]
fn a_canadian_night_on_a_road_in_canada_plans_the_next_canadian_board_fresh() {
    let mut harness = a_drive(
        &["winnipeg_mb_ca", "portage_la_prairie_mb_ca"],
        Some("longspur_transport"),
    );
    spend_a_shift(&mut harness, 30.0, HosRules::CanadaSouth60, 13.0);
    assert_eq!(full_night(&harness), FullNight::EightHours);
    let mut state = sleep_stop(&harness);
    Menu::enter(&mut state, &mut harness.app.ctx);
    let rows = labels(&mut state, &mut harness);
    assert!(rows.iter().any(|r| r == "Sleep 8 hours"), "{rows:?}");
    sleep_on(&mut state, &mut harness, "Sleep 8 hours");

    let world = harness.app.ctx.world;
    let hos = harness.app.ctx.profile.as_ref().unwrap().hos.clone();
    assert_eq!(hos.ca_driving_min, 0.0);
    assert_eq!(hos.ca_on_duty_min, 0.0);
    // The US counts still hold the 13 hours: 8 hours off is not their reset.
    assert_eq!(hos.driving_min, 13.0 * 60.0);

    // The next board in Canada plans from the fresh Canadian clock.
    let home = world
        .route_from_cities(&[
            "portage_la_prairie_mb_ca".to_string(),
            "winnipeg_mb_ca".to_string(),
        ])
        .expect("Portage la Prairie to Winnipeg");
    let planned = plan_hos(home.miles(), Some(&home), Some(world), Some(&hos));
    let fresh = plan_hos(home.miles(), Some(&home), Some(world), None);
    assert_eq!(planned, fresh);
    assert_eq!(planned.sleeps, 0);
}

#[test]
fn a_canadian_load_to_fargo_sleeps_ten_hours_in_manitoba_and_crosses_on_a_fresh_us_clock() {
    let mut harness = a_drive(
        &["winnipeg_mb_ca", "grand_forks_nd_us", "fargo_nd_us"],
        Some("longspur_transport"),
    );
    let booth = first_booth_mi(&harness);
    spend_a_shift(&mut harness, 20.0, HosRules::CanadaSouth60, 12.0);
    assert!(booth > 20.0, "the booth at {booth} is past the stop");
    assert_eq!(full_night(&harness), FullNight::TenHours);
    let mut state = sleep_stop(&harness);
    Menu::enter(&mut state, &mut harness.app.ctx);
    let rows = labels(&mut state, &mut harness);
    assert!(rows.iter().any(|r| r == "Sleep 10 hours"), "{rows:?}");
    assert!(!rows.iter().any(|r| r == "Sleep 8 hours"), "{rows:?}");
    sleep_on(&mut state, &mut harness, "Sleep 10 hours");

    let p = harness.app.ctx.profile.as_mut().unwrap();
    assert_eq!(p.hos.driving_min, 0.0);
    assert_eq!(p.hos.duty_min, 0.0);
    assert_eq!(p.hos.ca_driving_min, 0.0);
    // Over the line, the US rules find no count running.
    p.hos.set_rules(HosRules::Us);
    assert!(!p.hos.in_violation("realistic"));
    assert!(p.hos.shift_over_kind("realistic").is_none());
}

#[test]
fn a_us_driver_who_slept_eight_hours_in_canada_is_planned_the_rest_before_a_load_home() {
    let mut harness = a_drive(&["grand_forks_nd_us", "winnipeg_mb_ca"], None);
    let booth = first_booth_mi(&harness);
    spend_a_shift(&mut harness, booth + 5.0, HosRules::CanadaSouth60, 10.0);
    // Past the booth the road stays in Canada: the 8-hour night is offered.
    assert_eq!(full_night(&harness), FullNight::EightHours);
    let mut state = sleep_stop(&harness);
    Menu::enter(&mut state, &mut harness.app.ctx);
    sleep_on(&mut state, &mut harness, "Sleep 8 hours");

    let world = harness.app.ctx.world;
    let hos = harness.app.ctx.profile.as_ref().unwrap().hos.clone();
    assert_eq!(hos.ca_driving_min, 0.0);
    assert_eq!(hos.driving_min, 10.0 * 60.0);

    // The load home starts on the US counts the 8 hours left running, so
    // its plan books the 10-hour rest before the US rules let it run.
    let home = world
        .route_from_cities(&[
            "winnipeg_mb_ca".to_string(),
            "grand_forks_nd_us".to_string(),
            "fargo_nd_us".to_string(),
        ])
        .expect("Winnipeg to Fargo");
    let planned = plan_hos(home.miles(), Some(&home), Some(world), Some(&hos));
    let fresh = plan_hos(home.miles(), Some(&home), Some(world), None);
    assert_eq!(fresh.sleeps, 0, "{fresh:?}");
    assert_eq!(planned.sleeps, 1, "{planned:?}");
    assert!(
        planned.summary().contains("1 10-hour sleep"),
        "{}",
        planned.summary()
    );
}

#[test]
fn the_lot_and_the_motel_follow_the_road_ahead() {
    for (cities, night) in [
        (&["winnipeg_mb_ca", "portage_la_prairie_mb_ca"][..], 8),
        (&["winnipeg_mb_ca", "grand_forks_nd_us"][..], 10),
    ] {
        let mut harness = a_drive(cities, Some("longspur_transport"));
        spend_a_shift(&mut harness, 10.0, HosRules::CanadaSouth60, 12.0);
        let at = harness.read_drive(|d| d.trip.position_mi);
        let mut stop = RoadStop::new("Example Rest Area", at, "rest_area");
        stop.actions = ["park", "break"].iter().map(|a| a.to_string()).collect();
        let handle = DriveRef::of(&harness.shared_driving().expect("a drive"));
        let mut state = RestStopState::with_drive(handle, stop, false);
        let rows = labels(&mut state, &mut harness);
        let lot = format!("Sleep {night} hours in the lot");
        let motel = format!("Motel room: sleep {night} hours for 95 dollars");
        assert!(rows.contains(&lot), "{cities:?}: {rows:?}");
        assert!(rows.contains(&motel), "{cities:?}: {rows:?}");
    }
}
