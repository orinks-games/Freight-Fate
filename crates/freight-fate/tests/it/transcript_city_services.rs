//! City service POIs, and the retired local-service-drive snapshot
//! (port of `tests/test_city_services.py`).
//!
//! Python read `service.spoken_name`; the Rust `CityService` carries the
//! spoken form as `name` (the model note: "spoken text uses `name`"), so
//! that is what the raw-map-data screens read here.

use ff_core::data::world::{get_world, World};
use ff_core::models::profile::Profile;
use freight_fate::app::testing::TestApp;
use freight_fate::states::city::CityMenuState;
use freight_fate::states::main_menu::enter_world;
use serde_json::json;

const SERVICE_ORDER: [&str; 3] = ["freight_market", "garage", "truck_dealer"];

fn keys(services: &[ff_core::data::world_models::CityService]) -> Vec<&str> {
    services.iter().map(|s| s.key.as_str()).collect()
}

#[test]
fn test_city_services_are_source_backed() {
    let world: &World = get_world();
    let services = world.city_services("Indianapolis").unwrap();

    assert_eq!(keys(&services), SERVICE_ORDER);
    assert!(services.iter().all(|s| !s.source_note.is_empty()));
    assert!(services.iter().all(|s| !s.name.is_empty()));
    for service in &services {
        assert!(!service.fallback);
        assert_eq!(service.source_type, "osm");
        assert!(service.lat != 0.0);
        assert!(service.lon != 0.0);
        assert!(service.source_note.contains("OpenStreetMap"));
        let spoken = service.name.to_lowercase();
        assert!(!spoken.contains("node/"));
        assert!(!spoken.contains("way/"));
    }
}

#[test]
fn test_city_services_fallback_when_no_source_data() {
    let world: &World = get_world();
    let services = world.city_services("Erie").unwrap();

    assert_eq!(keys(&services), SERVICE_ORDER);
    assert!(!services[0].fallback);
    assert!(!services[1].fallback);
    assert!(services[2].fallback);
    assert_eq!(services[2].source_type, "fallback");
    assert!(!services[2].fallback_reason.is_empty());
    assert!(services.iter().all(|s| !s.source_note.is_empty()));
}

#[test]
fn test_city_service_data_covers_every_supported_city() {
    let raw_markers = [
        "osm_id",
        "amenity=",
        "highway=",
        "operator=",
        "node/",
        "way/",
    ];
    let world: &World = get_world();

    let mut source_backed = 0usize;
    let mut fallback = 0usize;
    let cities = world.city_names();
    for city in &cities {
        let services = world.city_services(city).unwrap();
        assert_eq!(keys(&services), SERVICE_ORDER, "{city}");
        for service in &services {
            assert!(!service.name.is_empty(), "{city}");
            let spoken = service.name.to_lowercase();
            for marker in raw_markers {
                assert!(!spoken.contains(marker), "{city}: {spoken}");
            }
            assert!(!service.source_note.is_empty(), "{city}");
            if service.fallback {
                fallback += 1;
                assert_eq!(service.source_type, "fallback");
                assert!(!service.fallback_reason.is_empty());
            } else {
                source_backed += 1;
                assert_eq!(service.source_type, "osm");
                assert!(service.lat != 0.0);
                assert!(service.lon != 0.0);
                assert!(service.approach_miles > 0.0);
                assert!(!service.approach_road.is_empty());
            }
        }
    }

    // The sweep now covers every city on the map. Each city's three services
    // are source-backed where a real POI sits within the city-errand cap, and
    // fall back to a synthesized errand where none does.
    assert_eq!(source_backed, 1174);
    assert_eq!(fallback, cities.len() * 3 - source_backed);
}

#[test]
fn test_city_service_snapshot_drops_to_terminal() {
    // A save from before local city-service drives were retired can still
    // carry one mid-trip. There is no route or phase left to resume it with,
    // so loading it should park the driver at the terminal instead of
    // crashing.
    let mut app = TestApp::new();
    let mut profile = Profile::named_in("Retired Drive", "Chicago");
    profile.active_trip = Some(json!({"kind": "city_service_drive", "job": {}, "trip_seed": 1}));
    profile.set_money(4_321.0);
    profile.game_hours = 88.0;
    let path = profile.path().to_path_buf();
    app.ctx.profile = Some(profile);
    app.clear_speech();

    enter_world(&mut app.ctx, false);
    app.ctx.run_deferred();

    let state = app.state().expect("a state on the stack");
    assert!(state
        .borrow()
        .as_any()
        .downcast_ref::<CityMenuState>()
        .is_some());
    let profile = app.ctx.profile.as_ref().unwrap();
    assert!(profile.active_trip.is_none());
    assert_eq!(profile.money(), 4_321.0);
    assert_eq!(profile.game_hours, 88.0);
    assert!(app.main_lines().iter().any(|line| line
        == "Local service drives were retired in this update. You are parked at the terminal."));

    // The clear must reach disk, not just the in-memory profile, so the
    // notice does not replay on every future load of this save.
    let reloaded = Profile::load(&path).unwrap();
    assert!(reloaded.active_trip.is_none());
}
