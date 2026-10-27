//! What the driver can ASK the truck: the Tab status screens and the driver
//! tablet (`states/driving_menu_states/status.rs`, `.../apps.rs`), the F1 air
//! help, and the two spoken readouts that answer a key
//! (`states/driving_controls/info.rs`, `.../status.rs`).
//!
//! Ported from `tests/test_driving_features.py`:
//! `test_air_brake_help_and_status_are_spoken`,
//! `test_driver_apps_screen_uses_keyboard_and_vague_road_chatter`,
//! `test_status_traffic_line_falls_back_to_legacy_npc_vehicles`,
//! `test_metric_status_lines_do_not_mix_mph_and_miles`,
//! `test_status_map_screen_describes_source_backed_poi_services` and
//! `test_the_speed_readout_says_what_the_keeper_is_holding_not_just_what_is_set`.
//!
//! Python's traffic case built the whole world out of `SimpleNamespace`
//! stand-ins -- a fake trip, a fake context, a fake lead vehicle -- and read a
//! private method. Here a real `TrafficVehicle` is put on a real trip ahead of
//! a real truck and the tablet's Traffic screen is built the way the player
//! opens it, so what is pinned is the row a driver hears.

use ff_core::sim::enforcement_posts::{method_by_kind, EnforcementPost, KIND_MEDIAN};
use ff_core::sim::traffic_manager::TrafficVehicle;
use ff_core::sim::trip_models::{NavigationCue, Zone};
use ff_core::sim::weather::WeatherKind;

use freight_fate::playtest::harness::{key_event, PlaytestHarness, StartDelivery};
use freight_fate::states::base::{Key, Menu, State};
use freight_fate::states::driving_menu_states::{
    DriveRef, DriverAppScreenState, DriverAppsState, DrivingStatusScreenState, DrivingStatusState,
};

const MPH_PER_MPS: f64 = 2.23694;

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
    });
    harness.clear_speech();
    harness
}

fn drive_ref(harness: &PlaytestHarness) -> DriveRef {
    DriveRef::of(&harness.shared_driving().expect("a drive on the stack"))
}

/// The rows of one Tab status screen, built the way the picker builds it.
fn screen_lines(harness: &mut PlaytestHarness, screen: &str) -> Vec<String> {
    let mut state = DrivingStatusScreenState::new(drive_ref(harness), screen);
    let items = state.build_items(&mut harness.app.ctx);
    items
        .iter()
        .map(|item| item.text(&state, &harness.app.ctx))
        .collect()
}

/// The rows of one driver-tablet app.
fn app_lines(harness: &mut PlaytestHarness, app_key: &str) -> Vec<String> {
    let handle = drive_ref(harness);
    let mut state = DriverAppScreenState::new(&mut harness.app.ctx, handle, app_key);
    let items = state.build_items(&mut harness.app.ctx);
    items
        .iter()
        .map(|item| item.text(&state, &harness.app.ctx))
        .collect()
}

fn press(harness: &mut PlaytestHarness, key: Key) {
    harness.with_drive(move |drive, ctx| drive.handle_key_event(ctx, &key_event(key, None)));
}

fn last_main(harness: &PlaytestHarness) -> String {
    harness.app.main_lines().last().cloned().unwrap_or_default()
}

// -- the air help and the status screens -------------------------------------------------

#[test]
fn test_air_brake_help_and_status_are_spoken() {
    let mut harness = a_drive("Air Status");
    harness.with_drive(|drive, _| drive.truck_mut().set_cold_air_start());
    harness.clear_speech();

    press(&mut harness, Key::F1);
    let help = last_main(&harness);
    assert!(help.contains("Air pressure must build"), "{help}");
    assert!(
        help.contains("P sets or releases the parking brake"),
        "{help}"
    );

    let status_lines = screen_lines(&mut harness, "route");
    let air_status = status_lines
        .iter()
        .find(|line| line.starts_with("Air brakes:"))
        .unwrap_or_else(|| panic!("no air line on the Route screen: {status_lines:#?}"));
    for phrase in [
        "primary 55 psi",
        "secondary 55 psi",
        "trailer 55 psi",
        "parking brake set",
        "compressor idle",
        "brakes cool",
    ] {
        assert!(air_status.contains(phrase), "{air_status}");
    }
    assert!(
        status_lines.iter().any(|l| l.starts_with("Weather:")),
        "{status_lines:#?}"
    );
    assert!(
        status_lines.iter().any(|l| l.starts_with("Traffic:")),
        "{status_lines:#?}"
    );

    let driver_lines = screen_lines(&mut harness, "driver");
    assert!(
        driver_lines.iter().any(|l| l.starts_with("Driver:")),
        "{driver_lines:#?}"
    );
    assert!(
        driver_lines.iter().any(|l| l.starts_with("Hours:")),
        "{driver_lines:#?}"
    );

    let radio_lines = screen_lines(&mut harness, "radio");
    assert!(
        radio_lines.iter().any(|l| l.starts_with("Radio on.")),
        "{radio_lines:#?}"
    );
    assert!(
        radio_lines
            .iter()
            .any(|l| l.starts_with("Receivable stations:")),
        "{radio_lines:#?}"
    );

    let map_lines = screen_lines(&mut harness, "map");
    assert!(
        map_lines.iter().any(|l| l.starts_with("Route:")),
        "{map_lines:#?}"
    );
    assert!(
        map_lines.iter().any(|l| l.contains("offers")),
        "{map_lines:#?}"
    );

    // The tablet, and the apps on it.
    let handle = drive_ref(&harness);
    let mut tablet = DriverAppsState::new(handle);
    let tablet_apps: Vec<String> = tablet
        .build_items(&mut harness.app.ctx)
        .iter()
        .map(|item| item.text(&tablet, &harness.app.ctx))
        .collect();
    for app in [
        "Navigation",
        "Weather",
        "Traffic",
        "Truck stops",
        "Road chatter",
        "ELD",
    ] {
        assert!(
            tablet_apps.iter().any(|row| row == app),
            "{app} missing: {tablet_apps:#?}"
        );
    }

    let navigation_lines = app_lines(&mut harness, "navigation");
    assert!(
        navigation_lines
            .iter()
            .any(|l| l.starts_with("Navigation:")),
        "{navigation_lines:#?}"
    );
    assert!(
        navigation_lines
            .iter()
            .any(|l| l.starts_with("Route progress:")),
        "{navigation_lines:#?}"
    );

    let weather_lines = app_lines(&mut harness, "weather");
    assert!(
        weather_lines
            .iter()
            .any(|l| l.starts_with("Weather source:")),
        "{weather_lines:#?}"
    );
    assert!(
        weather_lines
            .iter()
            .any(|l| l.starts_with("Safe speed guidance:")),
        "{weather_lines:#?}"
    );

    let truck_stop_lines = app_lines(&mut harness, "truck_stops");
    assert!(
        truck_stop_lines
            .iter()
            .any(|l| l.starts_with("Truck stops:")),
        "{truck_stop_lines:#?}"
    );

    let eld_lines = app_lines(&mut harness, "eld");
    assert!(
        eld_lines.iter().any(|l| l.starts_with("ELD:")),
        "{eld_lines:#?}"
    );

    // Tab in, Escape out: the screen picker hands the drive back and says so.
    harness.clear_speech();
    press(&mut harness, Key::Tab);
    assert!(harness.state_is::<DrivingStatusState>());
    harness.key(key_event(Key::Escape, None));
    assert!(harness.state_is::<freight_fate::states::driving::DrivingState>());
    assert!(
        harness
            .app
            .main_calls()
            .contains(&("Back to driving.".to_string(), false)),
        "{:#?}",
        harness.app.main_calls()
    );

    // And the space-bar readout carries the air pressure while it is low.
    harness.clear_speech();
    press(&mut harness, Key::Space);
    let said = last_main(&harness);
    assert!(said.contains("Low air, 55 psi"), "{said}");
    // `driving.lines()`: the on-screen readout, not the Tab status list.
    let lines = harness.with_drive(|d, ctx| d.lines(ctx));
    assert!(
        lines.iter().any(|l| l.starts_with("Air: 55 psi")),
        "{lines:#?}"
    );
}

// -- the tablet --------------------------------------------------------------------------

#[test]
fn test_driver_apps_screen_uses_keyboard_and_vague_road_chatter() {
    let mut harness = a_drive("Road Chatter");
    harness.with_drive(|drive, _| {
        let at = drive.trip.position_mi + 4.0;
        drive.trip.posts = vec![EnforcementPost {
            method: method_by_kind(KIND_MEDIAN).to_string(),
            reach_mi: 1.0,
            facing: "both".to_string(),
            staffed: true,
            notice: 1.0,
            announced: true,
            ..EnforcementPost::new(at, KIND_MEDIAN)
        }];
    });

    press(&mut harness, Key::Tab);
    assert!(harness.state_is::<DrivingStatusState>());
    let picker_labels = harness.menu_labels();
    assert!(
        picker_labels.iter().any(|row| row == "Driver apps"),
        "{picker_labels:#?}"
    );
    harness.select_menu_item("Driver apps");
    assert!(harness.state_is::<DriverAppsState>());
    let tablet_apps = harness.menu_labels();
    assert!(tablet_apps.iter().any(|row| row == "Road chatter"));
    assert!(tablet_apps.iter().any(|row| row == "Navigation"));
    harness.select_menu_item("Road chatter");
    assert!(harness.state_is::<DriverAppScreenState>());

    let lines = harness.menu_labels();
    let road_chatter = lines
        .iter()
        .find(|line| line.starts_with("Road chatter:"))
        .unwrap_or_else(|| panic!("no chatter row: {lines:#?}"));
    assert!(
        road_chatter.contains("enforcement reported somewhere ahead"),
        "{road_chatter}"
    );
    let lower = road_chatter.to_lowercase();
    for banned in ["radar", "scanner", "patrol", "speed trap", "3 miles"] {
        assert!(!lower.contains(banned), "{road_chatter} says {banned:?}");
    }

    harness.clear_speech();
    harness.key(key_event(Key::Return, None));
    assert_eq!(last_main(&harness), lines[0]);
}

#[test]
fn test_status_traffic_line_falls_back_to_legacy_npc_vehicles() {
    let mut harness = a_drive("Traffic Line");
    harness.with_drive(|drive, _| {
        drive.trip.position_mi = 10.0;
        // Python's stand-in carried `reason = "slow merge"` as a plain
        // attribute. A real vehicle derives its reason from its INTENT, so
        // the merging intent is what puts a merge in the row -- and the
        // distance is the game's own spoken rendering of two and a half
        // miles rather than Python's raw `%g`.
        let lead = TrafficVehicle::new("lead", 12.5, 42.0, 42.0, 1, "merging", "car");
        drive.trip.set_npc_vehicles(vec![lead]);
    });

    let lines = app_lines(&mut harness, "traffic");

    assert!(
        lines
            .iter()
            .any(|line| line == "Traffic: Merging car, 2.5 miles ahead, 42 miles per hour."),
        "{lines:#?}"
    );
}

#[test]
fn test_traffic_app_names_a_yielding_vehicle_as_being_on_the_right_ramp() {
    let mut harness = a_drive("Yielding Ramp Traffic");
    harness.with_drive(|drive, _| {
        drive.trip.position_mi = 10.0;
        let merger =
            TrafficVehicle::new("ramp", 10.4, 42.0, 42.0, 1, "merging", "car").with_lane(-1);
        drive.trip.set_npc_vehicles(vec![merger]);
    });

    let lines = app_lines(&mut harness, "traffic");

    assert!(
        lines.iter().any(|line| line
            == "Traffic: Merging car on the right ramp, 0.4 miles ahead, 42 miles per hour."),
        "{lines:#?}"
    );
}

#[test]
fn test_route_and_driver_traffic_name_box_truck_with_cruise_set() {
    let mut harness = a_drive("Named Cruise Traffic");
    harness.with_drive(|drive, _| {
        drive.trip.position_mi = 10.0;
        drive.cruise_mph = Some(65.0);
        let lead = TrafficVehicle::new("lead-box", 12.2, 60.0, 60.0, 0, "following", "box truck");
        drive.trip.set_npc_vehicles(vec![lead]);
    });

    let route_lines = screen_lines(&mut harness, "route");
    let route_traffic: Vec<_> = route_lines
        .iter()
        .filter(|line| line.starts_with("Traffic:"))
        .collect();
    assert_eq!(route_traffic.len(), 1, "{route_lines:#?}");
    assert_eq!(
        route_traffic[0],
        "Traffic: Slow box truck, 2.2 miles ahead, 60 miles per hour."
    );

    let traffic_lines = app_lines(&mut harness, "traffic");
    let app_traffic: Vec<_> = traffic_lines
        .iter()
        .filter(|line| line.starts_with("Traffic:"))
        .cloned()
        .collect();
    assert_eq!(
        app_traffic,
        vec!["Traffic: Slow box truck, 2.2 miles ahead, 60 miles per hour.".to_string()],
        "{traffic_lines:#?}"
    );
    assert_eq!(
        traffic_lines.last().map(String::as_str),
        Some("Back to Driver apps"),
        "{traffic_lines:#?}"
    );
    let spoken = route_lines.join(" ").to_lowercase();
    assert!(!spoken.contains("lead vehicle"), "{route_lines:#?}");
    assert!(!spoken.contains("slow car"), "{route_lines:#?}");
}

#[test]
fn test_metric_status_lines_do_not_mix_mph_and_miles() {
    let mut harness = PlaytestHarness::new();
    harness.app.ctx.settings.imperial_units = false;
    harness.start_delivery(StartDelivery::named("Metric Status"));
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
        drive.truck_mut().velocity_mps = 26.8;
        drive.cruise_mph = Some(60.0);
        // Force a known traffic cue ahead so the route line always renders the
        // traffic speed. The speed used to be baked into the cue text as mph
        // at build time, so it leaked imperial units in metric mode -- but
        // only when a traffic lead randomly landed in range, which made the
        // Python case flaky.
        let at = drive.trip.position_mi + 5.0;
        drive.trip.navigation_cues =
            vec![
                NavigationCue::new("traffic:test", "traffic", at, "traffic queue ahead", "")
                    .with_speed(Some(50.0)),
            ];
    });

    let lines = harness.with_drive(|d, ctx| d.status_lines(ctx));

    assert!(
        lines.iter().any(|l| l.contains("kilometers per hour")),
        "{lines:#?}"
    );
    // 50 mph rendered in metric, not "miles per hour".
    assert!(
        lines
            .iter()
            .any(|l| l.contains("traffic queue ahead at 80 kilometers per hour")),
        "{lines:#?}"
    );
    assert!(lines.iter().all(|l| !l.contains(" mph")), "{lines:#?}");
    assert!(lines.iter().all(|l| !l.contains(" miles")), "{lines:#?}");
}

#[test]
fn test_status_map_screen_describes_source_backed_poi_services() {
    let mut harness = a_drive("Map Services");

    let text = screen_lines(&mut harness, "map").join(" ");

    assert!(text.contains("offers"), "{text}");
    assert!(text.contains("fuel"), "{text}");
    assert!(text.contains("food"), "{text}");
    assert!(
        text.contains("sleep or long rest") || text.contains("30-minute rest break"),
        "{text}"
    );
    assert!(text.contains("listed services"), "{text}");
}

// -- the speed readout ---------------------------------------------------------------------

#[test]
fn test_the_speed_readout_says_what_the_keeper_is_holding_not_just_what_is_set() {
    // Owner, Spokane, 2026-08-22: "the truck slows to 15 while the speed stays
    // 25." The keeper was easing for the corners and the gate zone, and S kept
    // saying "holding 25" -- the SET speed -- while the truck held 15. The
    // readout says the live number, and the set one only when it differs.
    let mut harness = a_drive("Keeper Readout");
    harness.with_drive(|drive, _| {
        drive.truck_mut().start_engine();
        drive.truck_mut().velocity_mps = 15.0 / MPH_PER_MPS;
        drive.speed_control_armed = true;
        drive.keeper_mph = Some(25.0);
        drive.speed_control_target_mph = Some(25.0);
        // Easing for a corner a block ahead: the truck is at 15 for it.
        let at = drive.trip.position_mi + 0.1;
        drive.keeper_ease_target = Some((at, 15.0, "turn".to_string()));
    });
    harness.clear_speech();

    harness.with_drive(|drive, ctx| drive.speak_speed(ctx));
    let said = last_main(&harness);
    assert!(
        said.contains(
            "Speed keeper holding 15 miles per hour for the corner, set 25 miles per hour"
        ),
        "{said}"
    );
    assert!(!said.contains("holding 25 miles per hour,"), "{said}");
    let status = harness.with_drive(|d, ctx| d.status_lines(ctx)).join("\n");
    assert!(
        status.contains(
            "Speed control: speed keeper holding 15 miles per hour for the corner, set 25"
        ),
        "{status}"
    );

    // Nothing to ease for: one number, as before.
    harness.with_drive(|drive, _| drive.keeper_ease_target = None);
    harness.clear_speech();
    harness.with_drive(|drive, ctx| drive.speak_speed(ctx));
    let said = last_main(&harness);
    assert!(
        said.contains("Speed keeper holding 25 miles per hour"),
        "{said}"
    );
    assert!(!said.contains("set 25"), "{said}");

    // The eased point already behind the truck is no longer what it holds.
    harness.with_drive(|drive, _| {
        let behind = drive.trip.position_mi - 0.01;
        drive.keeper_ease_target = Some((behind, 15.0, "turn".to_string()));
    });
    harness.clear_speech();
    harness.with_drive(|drive, ctx| drive.speak_speed(ctx));
    let said = last_main(&harness);
    assert!(
        said.contains("Speed keeper holding 25 miles per hour"),
        "{said}"
    );
    assert!(!said.contains("for the corner"), "{said}");
}

#[test]
fn test_the_speed_readout_names_the_slow_car_setting_the_keepers_number() {
    // Agent drive, I-35, 2026-09-11: a car doing 51 held the truck at 51
    // through twenty miles of a 58 zone, and Space answered "speed keeper
    // holding 58" every time -- the SET number, with nothing naming the car.
    // The loop publishes what it held and why, as cruise does.
    let mut harness = a_drive("Keeper Follows");
    harness.app.ctx.settings.speed_keeper = true;
    harness.with_drive(|drive, _| {
        let miles = drive.trip.total_miles();
        drive.trip.zones = vec![Zone::new(0.0, miles, 58.0, "heavy traffic")];
        drive.truck_mut().transmission.automatic = true;
        drive.truck_mut().start_engine();
        drive.truck_mut().set_air_ready(true);
        drive.truck_mut().parking_brake = false;
        drive.truck_mut().velocity_mps = 58.0 / MPH_PER_MPS;
        drive.truck_mut().transmission.gear = drive.truck().transmission.num_gears() / 2;
        let ahead = drive.trip.position_mi + 0.03;
        drive.trip.set_npc_vehicles(vec![TrafficVehicle::new(
            "bench:slow-car",
            ahead,
            51.0,
            51.0,
            0,
            "cruising",
            "car",
        )]);
    });
    harness.with_drive(|drive, ctx| {
        drive.engage_keeper(ctx, 58.0, "heavy traffic", Some(58.0), false);
        drive.update_keeper(ctx, 0.1, false, false, false);
    });
    assert!(
        harness.read_drive(|d| d.keeper_mph.is_some()),
        "the keeper refused to engage"
    );
    harness.clear_speech();

    harness.with_drive(|drive, ctx| drive.speak_speed(ctx));
    let said = last_main(&harness);
    assert!(
        said.contains("Speed keeper holding 51 miles per hour for the traffic ahead, set 58"),
        "{said}"
    );
    assert!(!said.contains("holding 58 miles per hour,"), "{said}");

    // The car gone: one number again, and no stale reason.
    harness.with_drive(|drive, ctx| {
        drive.trip.set_npc_vehicles(Vec::new());
        drive.update_keeper(ctx, 0.1, false, false, false);
    });
    harness.clear_speech();
    harness.with_drive(|drive, ctx| drive.speak_speed(ctx));
    let said = last_main(&harness);
    assert!(
        said.contains("Speed keeper holding 58 miles per hour"),
        "{said}"
    );
    assert!(!said.contains("for the traffic ahead"), "{said}");
}
