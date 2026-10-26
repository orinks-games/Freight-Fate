//! National Weather Service warnings, read keyless onto the route: dispatch
//! briefs them at the pickup departure and plans around the ones nobody
//! drives into, and the cab reads one out as the truck drives into it
//! (`sim/real_weather_alerts.rs`, `city_pickup.rs`, `live_sources.rs`).
//!
//! The alerts provider is the offline one with its cache seeded. Real
//! weather has to be on for the warnings to count, which also builds the
//! drive's live sky provider; the test network guard refuses its fetches,
//! so nothing here reaches api.weather.gov.

use std::sync::Arc;

use ff_core::data::world::get_world;
use ff_core::models::jobs::{cargo_type, Job};
use ff_core::sim::real_weather_alerts::{WeatherAlert, WeatherAlertsProvider};

use freight_fate::app::testing::TestApp;
use freight_fate::states::base::{Key, Menu};
use freight_fate::states::city_pickup::{PickupFacilityState, PickupOptions};
use freight_fate::states::driving::DrivingState;
use freight_fate::states::driving_menu_states::{DriveRef, DriverAppScreenState};

use crate::states_city_support::*;
use crate::transcript_cruise_support::{bench_drive, frame, frames, said_any};

fn warning(id: &str, event: &str, description: &str) -> WeatherAlert {
    WeatherAlert {
        id: id.to_string(),
        event: event.to_string(),
        severity: "Severe".to_string(),
        headline: format!("{event} issued"),
        area: "Milwaukee; Waukesha".to_string(),
        description: description.to_string(),
    }
}

#[test]
fn test_dispatch_departure_briefs_the_weather_alerts_on_the_way() {
    let mut app = TestApp::new();
    career(&mut app, "Alert Dispatch", "Chicago");
    let mut job = Job::new(
        cargo_type("general").unwrap(),
        12.0,
        "Chicago",
        "Chicago Cross-Dock",
        "Milwaukee",
        92.0,
        1800.0,
        9.0,
    );
    job.origin_type = "mine_quarry".to_string();
    job.origin_facility_id = "chicago-live-load".to_string();
    let pickup = PickupFacilityState::new(&app.ctx, job.clone(), PickupOptions::default());
    app.push_state(pickup);
    key(&mut app, Key::Return); // check in
    key(&mut app, Key::Return); // load cargo
    finish_timed_state(&mut app);
    assert_eq!(
        current_label::<PickupFacilityState>(&app),
        "Depart for destination"
    );

    // A High Wind Warning over the destination, on every route option.
    let world = get_world();
    let routes = world
        .supported_route_options(&job.origin, &job.destination, 3)
        .expect("routes to Milwaukee");
    let destination_key = routes[0].cities.last().expect("a destination").clone();
    let milwaukee = world.cities.get(&destination_key).expect("Milwaukee");
    let alerts = Arc::new(WeatherAlertsProvider::offline());
    alerts.seed(
        milwaukee.lat,
        milwaukee.lon,
        vec![warning(
            "wind",
            "High Wind Warning",
            "* WHAT...West winds 30 to 40 mph with gusts up to 60 mph.",
        )],
    );
    // The warnings ride the real weather toggle. The drive's own sky
    // provider is built too, and the test network guard refuses its
    // fetches; the alerts provider is the seeded offline one.
    app.ctx.settings.real_weather = true;
    app.ctx.set_weather_alerts_provider(Arc::clone(&alerts));

    app.clear_speech();
    key(&mut app, Key::Return); // depart for destination
    assert!(is::<DrivingState>(&app));
    let departure = app
        .main_lines()
        .into_iter()
        .rev()
        .find(|text| text.contains("Dispatch routed you to"))
        .expect("a departure line");
    assert!(
        departure.contains(&format!(
            "Weather alerts on the way: High Wind Warning near {}",
            milwaukee.name
        )),
        "{departure}"
    );
    // Every option ends under the same warning, so dispatch keeps the
    // shortest; a wind warning slows nothing in the plan, so there is no
    // "quicker way" to speak of.
    assert!(!departure.contains("No quicker way around."), "{departure}");
    let driven = with_state::<DrivingState, _>(&app, |d, _| d.trip.route.cities.clone());
    assert_eq!(driven, routes[0].cities);
}

#[test]
fn test_the_cab_reads_a_warning_once_as_the_truck_drives_into_it() {
    let mut harness = bench_drive("Alert Cab", 65.0, 0.0);
    let alerts = Arc::new(WeatherAlertsProvider::offline());
    // Let the departure's own lines finish before the warnings arrive: a
    // safety line that cuts a line still speaking is requeued behind it by
    // the pacer, and the capture would then hold it twice for one saying.
    frames(&mut harness, 30, 0.1);
    harness.clear_speech();
    harness.app.ctx.settings.real_weather = true;
    harness
        .app
        .ctx
        .set_weather_alerts_provider(Arc::clone(&alerts));

    // The first poll asks about the truck's own point and gets no answer
    // yet, as a live fetch would; the cab keeps reading that same point on
    // the frames that follow instead of asking about a new one each time.
    frame(&mut harness, 0.1);
    let (lat, lon, _) = harness
        .read_drive(|d| d.alerts_pending)
        .expect("a point asked about and not yet answered");
    assert!(lat != 0.0 || lon != 0.0, "the delivery route has geometry");
    assert!(!said_any(&harness, "Weather alert"));
    alerts.seed(
        lat,
        lon,
        vec![
            warning(
                "wind",
                "High Wind Warning",
                "* WHAT...Gusts up to 60 mph expected.",
            ),
            warning(
                "snow",
                "Winter Storm Warning",
                "Heavy snow, 8 to 12 inches.",
            ),
        ],
    );
    frame(&mut harness, 0.1);
    assert!(harness.read_drive(|d| d.alerts_pending).is_none());
    // Both warnings found at once come as one line, so neither cuts the
    // other off.
    assert!(
        said_any(
            &harness,
            "Weather alerts: High Wind Warning, gusts to 60 miles per hour; Winter Storm Warning."
        ),
        "{}",
        harness.transcript_text()
    );
    // The warnings the truck is under now steer the chain law, before any
    // snow is on the road under the wheels.
    assert_eq!(harness.read_drive(|d| d.trip.live_alerts.len()), 2);
    assert_eq!(harness.read_drive(|d| d.trip.chain_law_level()), 1);

    // And the Weather app keeps them askable after the one spoken line.
    let handle = DriveRef::of(&harness.shared_driving().expect("a drive on the stack"));
    let mut app_screen = DriverAppScreenState::new(&mut harness.app.ctx, handle, "weather");
    let weather_lines: Vec<String> = app_screen
        .build_items(&mut harness.app.ctx)
        .iter()
        .map(|item| item.text(&app_screen, &harness.app.ctx))
        .collect();
    assert!(
        weather_lines.iter().any(|l| l
            == "Weather alerts: High Wind Warning, gusts to 60 miles per hour; Winter Storm Warning."),
        "{weather_lines:#?}"
    );

    // Said once: the same warnings, many frames on, are not repeated.
    frames(&mut harness, 40, 0.1);
    let mentions: Vec<String> = harness
        .transcript()
        .iter()
        .filter(|line| line.contains("Weather alert"))
        .cloned()
        .collect();
    assert_eq!(mentions.len(), 1, "{mentions:?}");

    // Switching real weather off clears what the trip holds.
    harness.app.ctx.settings.real_weather = false;
    frame(&mut harness, 0.1);
    assert!(harness.read_drive(|d| d.trip.live_alerts.is_empty()));
    assert_eq!(harness.read_drive(|d| d.trip.chain_law_level()), 0);
}
