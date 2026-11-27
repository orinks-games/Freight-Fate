//! Which live weather service a drive asks: the game's data picks it by
//! the state the road is in, so a trip in Ontario asks Environment Canada,
//! one in Michigan the Weather Service, and a trip over the line switches
//! services without the sky dropping to simulated.

use std::sync::{Arc, Mutex};

use ff_core::data::world::get_world;
use ff_core::data::world_models::Route;
use ff_core::sim::trip::{Trip, TripOptions};
use ff_core::sim::vehicle::TruckState;
use ff_core::sim::weather::{WeatherKind, WeatherProvider, WeatherSystem};
use ff_core::sim::weather_service::WeatherService;

/// The requests a recorder saw: which service, and for which key.
type Calls = Arc<Mutex<Vec<(WeatherService, String)>>>;

/// Answers every key at once and records which service each request went
/// to.
struct Recorder(Calls);

impl WeatherProvider for Recorder {
    fn request(&mut self, city: &str, lat: f64, lon: f64) {
        self.request_from(WeatherService::UnitedStates, city, lat, lon);
    }

    fn request_from(&mut self, service: WeatherService, city: &str, _: f64, _: f64) {
        self.0.lock().unwrap().push((service, city.to_string()));
    }

    fn get(&mut self, _: &str) -> Option<WeatherKind> {
        Some(WeatherKind::Cloudy)
    }
}

fn route(from: &str, to: &str) -> Route {
    get_world()
        .route_options(from, to, 3, false)
        .unwrap_or_else(|e| panic!("no route {from} to {to}: {e:?}"))
        .into_iter()
        .next()
        .expect("a route")
}

fn trip_with_recorder(route: Route) -> (Trip, Calls) {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let weather = WeatherSystem::new(
        "great_lakes",
        Some(1),
        Some(Box::new(Recorder(Arc::clone(&calls)))),
        None,
        true,
    );
    let trip = Trip::new(
        route,
        TruckState::default(),
        weather,
        TripOptions::seeded(1),
    );
    (trip, calls)
}

fn services(calls: &Calls) -> Vec<WeatherService> {
    calls.lock().unwrap().iter().map(|(s, _)| *s).collect()
}

#[test]
fn test_a_trip_in_ontario_asks_only_environment_canada() {
    let (mut trip, calls) = trip_with_recorder(route("toronto_on_ca", "kingston_on_ca"));
    let total = trip.route.miles();
    let mut mile = 0.0;
    while mile < total {
        trip.position_mi = mile;
        trip.update(0.0);
        assert_eq!(trip.weather.source_status(), "live", "mile {mile}");
        mile += 10.0;
    }
    let asked = services(&calls);
    assert!(!asked.is_empty());
    assert!(
        asked.iter().all(|s| *s == WeatherService::Canada),
        "{asked:?}"
    );
    assert_eq!(trip.live_weather_service(0.0), Some(WeatherService::Canada));
}

#[test]
fn test_a_trip_in_michigan_asks_only_the_weather_service() {
    let (mut trip, calls) = trip_with_recorder(route("detroit_mi_us", "lansing_mi_us"));
    let total = trip.route.miles();
    let mut mile = 0.0;
    while mile < total {
        trip.position_mi = mile;
        trip.update(0.0);
        mile += 10.0;
    }
    let asked = services(&calls);
    assert!(!asked.is_empty());
    assert!(
        asked.iter().all(|s| *s == WeatherService::UnitedStates),
        "{asked:?}"
    );
}

#[test]
fn test_detroit_to_windsor_switches_service_at_the_line_and_stays_live() {
    let (mut trip, calls) = trip_with_recorder(route("detroit_mi_us", "windsor_on_ca"));
    let total = trip.route.miles();
    let mut mile = 0.0;
    let mut seen = Vec::new();
    while mile <= total {
        trip.position_mi = mile;
        trip.update(0.0);
        let service = trip
            .live_weather_service(mile)
            .expect("a service on both sides");
        if seen.last() != Some(&service) {
            seen.push(service);
        }
        assert_ne!(trip.weather.source_status(), "simulated", "mile {mile}");
        mile += 0.25;
    }
    assert_eq!(
        seen,
        [WeatherService::UnitedStates, WeatherService::Canada],
        "Detroit, then Windsor"
    );
    let calls = calls.lock().unwrap();
    // Every request in Canada was keyed to an Ontario cell, and every one
    // in Michigan to a Michigan cell: no point went to the other service.
    for (service, key) in calls.iter() {
        let ontario = key.ends_with(":Ontario") || key.ends_with(":ON");
        if key.starts_with("route:") {
            assert_eq!(
                *service == WeatherService::Canada,
                ontario,
                "{service:?} {key}"
            );
        }
    }
    assert!(calls.iter().any(|(s, _)| *s == WeatherService::Canada));
}

#[test]
fn test_a_facility_access_road_asks_the_service_of_its_city() {
    // The access road to a facility carries no state of its own, so the
    // city it serves picks the service: a Toronto pickup asks Environment
    // Canada, a Detroit one the Weather Service.
    let world = get_world();
    for (city, expected) in [
        ("toronto_on_ca", WeatherService::Canada),
        ("detroit_mi_us", WeatherService::UnitedStates),
    ] {
        let location = world.cities[city]
            .locations
            .first()
            .unwrap_or_else(|| panic!("{city} has a facility"))
            .name
            .clone();
        for streets in [false, true] {
            let route = world
                .facility_approach_route_with(city, &location, streets)
                .unwrap_or_else(|e| panic!("{city} {location}: {e:?}"));
            let (mut trip, calls) = trip_with_recorder(route);
            trip.update(0.0);
            assert_eq!(
                trip.live_weather_service(0.0),
                Some(expected),
                "{city} {location} streets {streets}"
            );
            assert!(
                services(&calls).iter().all(|s| *s == expected),
                "{city}: {:?}",
                calls.lock().unwrap()
            );
        }
    }
}
