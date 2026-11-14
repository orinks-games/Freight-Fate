use super::*;
use std::sync::Mutex as StdMutex;

fn event(id: &str, severity: &str, description: &str, county: &str) -> TrafficEvent {
    TrafficEvent::new(id, "incident", severity, description, county)
}

fn located(mut e: TrafficEvent, lat: f64, lon: f64) -> TrafficEvent {
    e.latitude = Some(lat);
    e.longitude = Some(lon);
    e
}

/// A transport answering from a URL -> JSON table, recording every URL
/// it was asked for.
struct TableTransport {
    answers: Vec<(&'static str, Value)>,
    calls: StdMutex<Vec<String>>,
}

impl TableTransport {
    fn new(answers: Vec<(&'static str, Value)>) -> Self {
        Self {
            answers,
            calls: StdMutex::new(Vec::new()),
        }
    }

    fn answer(&self, url: &str) -> Result<Vec<u8>, TransportError> {
        self.calls.lock().unwrap().push(url.to_string());
        self.answers
            .iter()
            .find(|(needle, _)| url.contains(needle))
            .map(|(_, v)| serde_json::to_vec(v).unwrap())
            .ok_or_else(|| TransportError::new(format!("404 {url}")))
    }
}

impl HttpTransport for TableTransport {
    fn get(&self, url: &str, _: &[(&str, &str)], _: f64) -> Result<Vec<u8>, TransportError> {
        self.answer(url)
    }

    fn post(
        &self,
        url: &str,
        _: &[u8],
        _: &[(&str, &str)],
        _: f64,
    ) -> Result<Vec<u8>, TransportError> {
        self.answer(url)
    }
}

// --- test_real_traffic.py

#[test]
fn test_traffic_data_freshness() {
    let now = wall_time();
    let data = TrafficData::new("ohio", vec![], now, now, "test");
    assert!(data.is_fresh());
    assert!(!data.is_stale());

    // Simulate old data
    let old_data = TrafficData::new(
        "ohio",
        vec![],
        now - CACHE_TTL_S - 1.0,
        now - CACHE_TTL_S - 1.0,
        "test",
    );
    assert!(!old_data.is_fresh());
    assert!(!old_data.is_stale()); // Not yet stale

    // Simulate stale data
    let stale_data = TrafficData::new(
        "ohio",
        vec![],
        now - STALE_AFTER_S - 1.0,
        now - STALE_AFTER_S - 1.0,
        "test",
    );
    assert!(!stale_data.is_fresh());
    assert!(stale_data.is_stale());
}

#[test]
fn test_traffic_data_serialization() {
    let now = wall_time();
    let data = TrafficData::new(
        "ohio",
        vec![event("test-123", "medium", "Construction", "Hamilton")],
        now,
        now,
        "test",
    );
    let serialized = data.to_dict();
    assert_eq!(serialized["state"], "ohio");
    assert_eq!(serialized["events"].as_array().unwrap().len(), 1);
    assert_eq!(serialized["events"][0]["id"], "test-123");
}

#[test]
fn test_provider_initialization() {
    let provider = RealTrafficProvider::offline();
    assert!(provider.cache().is_empty());
    assert!(provider.failed_until().is_empty());
}

#[test]
fn test_provider_unsupported_state() {
    let provider = RealTrafficProvider::offline();
    let data = provider.request("atlantis");
    assert_eq!(data.state, "atlantis");
    assert!(data.events.is_empty());
    assert_eq!(data.source, "empty");
}

#[test]
fn test_provider_supported_state_returns_empty_initially() {
    let provider = RealTrafficProvider::offline();
    let data = provider.request("ohio");
    assert_eq!(data.state, "ohio");
    assert!(data.events.is_empty());
    assert_eq!(data.source, "empty");
}

#[test]
fn test_haversine_distance() {
    // Test known distance (Columbus, OH to Cleveland, OH ≈ 126 miles)
    let distance = haversine_distance(39.96, -82.99, 41.50, -81.69);
    // Should be approximately 126 miles (within 10% tolerance)
    assert!(113.0 < distance && distance < 139.0);
}

#[test]
fn test_haversine_distance_same_point() {
    assert_eq!(haversine_distance(40.0, -83.0, 40.0, -83.0), 0.0);
}

// TestHaversineDistance (test_real_construction_zones.py) — the
// trip_route_helpers `_haversine_distance_mi` shares this formula.
#[test]
fn test_known_distance() {
    // Columbus to Cincinnati is about 100 miles.
    let dist = haversine_distance(39.9612, -82.9988, 39.1031, -84.5120);
    assert!((90.0..=110.0).contains(&dist));
}

#[test]
fn test_small_distance() {
    let dist = haversine_distance(39.96, -83.0, 39.97, -83.0);
    assert!((0.5..=1.5).contains(&dist));
}

#[test]
fn test_get_events_near_filters_by_distance() {
    let provider = RealTrafficProvider::offline();
    let event_near = located(
        event("near-1", "high", "Nearby accident", "Franklin"),
        40.0,
        -83.0,
    );
    let event_far = located(
        event("far-1", "medium", "Far accident", "Cuyahoga"),
        41.5,
        -81.7,
    );
    // Mock cache with test data
    let now = wall_time();
    provider.seed_cache(
        "ohio",
        TrafficData::new("ohio", vec![event_near, event_far], now, now, "test"),
    );
    // Search for events within 50 miles of Columbus
    let nearby = provider.get_events_near("ohio", 39.96, -82.99, 50.0);
    // Should only include the nearby event
    assert_eq!(nearby.len(), 1);
    assert_eq!(nearby[0].id, "near-1");
}

#[test]
fn test_get_events_near_includes_events_within_radius() {
    let provider = RealTrafficProvider::offline();
    let boundary = located(
        event("boundary-1", "low", "Boundary event", "Franklin"),
        40.0,
        -83.0,
    );
    let now = wall_time();
    provider.seed_cache(
        "ohio",
        TrafficData::new("ohio", vec![boundary], now, now, "test"),
    );
    let nearby = provider.get_events_near("ohio", 39.96, -82.99, 50.0);
    assert_eq!(nearby.len(), 1);
}

#[test]
fn test_get_events_near_excludes_events_without_location() {
    let provider = RealTrafficProvider::offline();
    let no_location = event("no-loc-1", "medium", "Event without location", "Unknown");
    let now = wall_time();
    provider.seed_cache(
        "ohio",
        TrafficData::new("ohio", vec![no_location], now, now, "test"),
    );
    let nearby = provider.get_events_near("ohio", 39.96, -82.99, 50.0);
    assert_eq!(nearby.len(), 0);
}

#[test]
fn test_state_api_config() {
    let ohio_config = state_api("ohio").expect("ohio in STATE_APIS");
    assert!(ohio_config.base_url.is_some());
    assert!(ohio_config.events_endpoint.is_some());
    assert_eq!(ohio_config.name, "Ohio OHGO");
}

#[test]
fn test_retry_cooldown_after_failure() {
    let provider = RealTrafficProvider::offline();
    // Simulate a failed fetch by setting the cooldown directly
    provider.set_failed_until("ohio", wall_time() + RETRY_AFTER_S);
    // Request should return cached data and not trigger new fetch
    let data = provider.request("ohio");
    assert_eq!(data.source, "empty"); // No cache, so empty data
                                      // Verify cooldown is still active
    let failed = provider.failed_until();
    assert!(failed.contains_key("ohio"));
    assert!(failed["ohio"] > wall_time());
}

#[test]
fn test_cache_returned_when_fresh() {
    let provider = RealTrafficProvider::offline();
    let now = wall_time();
    provider.seed_cache(
        "ohio",
        TrafficData::new(
            "ohio",
            vec![event("cached-1", "low", "Cached event", "Franklin")],
            now,
            now,
            "test",
        ),
    );
    let data = provider.request("ohio");
    assert_eq!(data.events.len(), 1);
    assert_eq!(data.events[0].id, "cached-1");
    assert_eq!(data.source, "test");
}

#[test]
fn test_empty_data_creation() {
    let provider = RealTrafficProvider::offline();
    let empty = provider.empty_data("unsupported");
    assert_eq!(empty.state, "unsupported");
    assert!(empty.events.is_empty());
    assert_eq!(empty.source, "empty");
}

#[test]
fn test_no_api_state_serves_fresh_cache() {
    // no_api states never fetch but still serve a fresh cache entry.
    // Ohio moved to no_api 2026-08-09 (OHGO now requires an API key), and
    // the trip tests seed its cache directly, so the cache check must run
    // before the no_api short-circuit.
    assert_eq!(state_api("ohio").unwrap().parser, "no_api");
    let provider = RealTrafficProvider::offline();
    let now = wall_time();
    let seeded = event("seeded-1", "low", "Seeded event", "Franklin");
    provider.seed_cache(
        "ohio",
        TrafficData::new("ohio", vec![seeded.clone()], now, now, "test"),
    );
    provider.seed_cache(
        "ohio:construction",
        TrafficData::new("ohio", vec![seeded], now, now, "test"),
    );
    assert_eq!(provider.request("ohio").events[0].id, "seeded-1");
    assert_eq!(provider.fetch_construction("ohio").events[0].id, "seeded-1");
    // And still no fetch machinery engaged
    assert!(provider.failed_until().is_empty());
}

#[test]
fn test_no_api_state_returns_empty_without_cache() {
    let provider = RealTrafficProvider::offline();
    let data = provider.request("ohio");
    assert!(data.events.is_empty());
    assert_eq!(data.source, "empty");
    let data = provider.fetch_construction("ohio");
    assert!(data.events.is_empty());
    assert_eq!(data.source, "empty");
}

#[test]
fn test_a_live_report_never_claims_the_road_ahead_is_shut() {
    // The game does not act on these, so it must not speak as if it did.
    // A state DOT feed describes the real road today. Called a "traffic
    // alert", a reported closure read as the state of the road in front of
    // the truck -- a driver was told a toll road was closed in both
    // directions, took it, and nothing stopped him, in three states
    // (Brandon, 2026-08-21). Enforcing them was declined: a live feed must
    // not make an accepted route impassable mid-run. So the frame has to
    // carry the provenance instead.
    let closure = TrafficEvent {
        lanes_affected: Some("all lanes closed".into()),
        ..event("x1", "high", "I-88 in both directions: Closed.", "")
    };
    let mut message = format!("Live road report: {}", closure.description);
    if let Some(lanes) = closure.lanes_affected.as_deref().filter(|l| !l.is_empty()) {
        message.push_str(&format!(". {lanes} affected."));
    }
    assert!(message.starts_with("Live road report:"));
    // The word that made it read as the game's own road is gone.
    assert!(!message.to_lowercase().contains("alert"));
    // The feed's own words survive: the report is still reported.
    assert!(message.contains("Closed"));
}

// --- Trip-level incident announcements ----------------------------------------

fn incident_leg() -> crate::data::world_models::Leg {
    use crate::data::world_models::{CorridorDetail, Leg, RoutePoint, StateMileage};

    Leg::new(
        "columbus_oh_us",
        "cincinnati_oh_us",
        100.0,
        "I-71",
        "flat",
        Vec::new(),
    )
    .with_detail(CorridorDetail {
        route_points: vec![
            RoutePoint {
                at_mi: 0.0,
                lat: 39.9612,
                lon: -82.9988,
            }, // Columbus
            RoutePoint {
                at_mi: 50.0,
                lat: 39.53,
                lon: -83.65,
            },
            RoutePoint {
                at_mi: 100.0,
                lat: 39.1031,
                lon: -84.5120,
            }, // Cincinnati
        ],
        state_miles: vec![StateMileage::new("Ohio", 100.0)],
        ..Default::default()
    })
}

fn incident_trip(provider: Arc<RealTrafficProvider>) -> crate::sim::trip::Trip {
    use crate::data::world_models::Route;
    use crate::sim::trip::{Trip, TripOptions};
    use crate::sim::trip_traffic::TrafficProvider;
    use crate::sim::vehicle::{TruckSpecs, TruckState};
    use crate::sim::weather::test_support::new_system;

    let route = Route::from_legs(
        vec!["columbus_oh_us".to_string(), "cincinnati_oh_us".to_string()],
        vec![incident_leg()],
    );
    let provider: Arc<dyn TrafficProvider> = provider;
    Trip::new(
        route,
        TruckState::new(TruckSpecs::default()),
        new_system("heartland", None, None, None, true),
        TripOptions {
            time_scale: 1.0,
            seed: Some(42),
            traffic_provider: Some(provider),
            ..Default::default()
        },
    )
}

/// `provider._cache["ohio"] = ...`; a fresh empty construction entry keeps
/// trip construction off the network.
fn seed_ohio_cache(provider: &RealTrafficProvider, events: Vec<TrafficEvent>) {
    let now = wall_time();
    provider.seed_cache("ohio", TrafficData::new("ohio", events, now, now, "test"));
    provider.seed_cache(
        "ohio:construction",
        TrafficData::new("ohio", Vec::new(), now, now, "test"),
    );
}

fn live_reports(trip: &crate::sim::trip::Trip) -> Vec<String> {
    trip.events.iter().map(|e| e.text().to_string()).collect()
}

// Trip-level incident announcements (test_trip_announces_nearby_real_incident,
// test_trip_does_not_announce_construction_as_traffic_alert,
// test_trip_skips_incident_beyond_radius) drive Trip._check_real_traffic_events
// and belong with sim::trip.
#[test]
fn test_trip_announces_nearby_real_incident() {
    // An incident near the truck's actual position is spoken once.
    // Regression for the 1.9 crash: the checker called a nonexistent
    // Trip._leg_at, so any drive with Traffic source set to real time died
    // on the first simulation tick.
    let provider = Arc::new(RealTrafficProvider::offline());
    let mut incident = located(
        event(
            "inc-1",
            "high",
            "Jackknifed truck on I-71 southbound",
            "Franklin",
        ),
        // On the leg's line, about 3 miles out of Columbus.
        39.9396,
        -83.0314,
    );
    incident.road_name = "I-71".to_string();
    incident.lanes_affected = Some("2 right lanes".to_string());
    seed_ohio_cache(&provider, vec![incident]);
    let mut trip = incident_trip(provider);

    trip.check_real_traffic_events();

    let messages = live_reports(&trip);
    assert!(messages
        .iter()
        .any(|m| m.contains("Live road report") && m.contains("Jackknifed truck")));
    assert!(messages
        .iter()
        .any(|m| m.contains("2 right lanes affected")));

    // A later check must not repeat the same incident.
    trip.next_real_traffic_check_mi = 0.0;
    let before = trip.events.len();
    trip.check_real_traffic_events();
    assert_eq!(trip.events.len(), before);
}

#[test]
fn test_trip_does_not_announce_construction_as_traffic_alert() {
    // Construction-typed events near the truck stay out of traffic alerts:
    // the WZDx states return their whole work-zone feed from request().
    let provider = Arc::new(RealTrafficProvider::offline());
    let work_zone = located(
        TrafficEvent::new(
            "wz-1",
            "construction",
            "medium",
            "Lane closed on I-71 northbound",
            "Franklin",
        ),
        39.95,
        -83.0,
    );
    seed_ohio_cache(&provider, vec![work_zone]);
    let mut trip = incident_trip(provider);

    trip.check_real_traffic_events();

    assert!(!live_reports(&trip)
        .iter()
        .any(|m| m.contains("Live road report")));
}

#[test]
fn test_trip_skips_incident_beyond_radius() {
    // The truck sits at Columbus (mile 0); an incident near Cincinnati is
    // about 100 miles away and must stay silent.
    let provider = Arc::new(RealTrafficProvider::offline());
    seed_ohio_cache(
        &provider,
        vec![located(
            event(
                "inc-2",
                "high",
                "Bridge closure near Cincinnati",
                "Hamilton",
            ),
            39.11,
            -84.50,
        )],
    );
    let mut trip = incident_trip(provider);

    trip.check_real_traffic_events();

    assert!(!live_reports(&trip)
        .iter()
        .any(|m| m.contains("Live road report")));
}

// --- test_real_construction_zones.py: TestRealTrafficProviderConstruction

#[test]
fn test_state_apis_has_construction_endpoint() {
    assert_eq!(
        state_api("ohio").unwrap().construction_endpoint,
        Some("/v1/construction")
    );
}

#[test]
fn test_fetch_construction_for_unsupported_state() {
    let provider = RealTrafficProvider::offline();
    // Texas is now in STATE_APIS with wzdx parser, so use a code not
    // in the list (e.g., "puerto rico") for the unsupported test.
    let data = provider.fetch_construction("puerto rico");
    assert!(data.events.is_empty());
    assert_eq!(data.source, "empty");
}

#[test]
fn test_fetch_construction_for_no_api_state() {
    let provider = RealTrafficProvider::offline();
    let data = provider.fetch_construction("alabama");
    assert!(data.events.is_empty());
    assert_eq!(data.source, "empty");
}

#[test]
fn test_all_states_have_parser() {
    let valid_parsers = [
        "ohgo",
        "iteris",
        "wzdx",
        "cars",
        "list511",
        "caltrans_lcs",
        "no_api",
    ];
    for (key, config) in STATE_APIS {
        assert!(
            valid_parsers.contains(&config.parser),
            "{key} has unknown parser {}",
            config.parser
        );
        if let Some(construction_parser) = config.construction_parser {
            assert!(valid_parsers.contains(&construction_parser), "{key}");
        }
    }
    // Every state plus DC is listed.
    assert_eq!(STATE_APIS.len(), 51);
}

#[test]
fn test_cars_states_have_bounds_and_layer_slugs() {
    let cars_keys: Vec<&str> = STATE_APIS
        .iter()
        .filter(|(_, c)| c.parser == "cars")
        .map(|(k, _)| *k)
        .collect();
    let mut sorted = cars_keys.clone();
    sorted.sort();
    assert_eq!(sorted, vec!["indiana", "minnesota"]);
    for key in cars_keys {
        let config = state_api(key).unwrap();
        let bounds: Vec<f64> = config
            .bounds
            .unwrap()
            .split(',')
            .map(|v| v.parse().unwrap())
            .collect();
        let [south, west, north, east] = bounds[..] else {
            panic!("{key} bounds")
        };
        assert!(south < north, "{key} bounds south/north swapped");
        assert!(west < east, "{key} bounds west/east swapped");
        // Layer slugs are bare words, not URL paths
        assert!(!config.events_endpoint.unwrap().starts_with('/'), "{key}");
        assert!(
            !config.construction_endpoint.unwrap().starts_with('/'),
            "{key}"
        );
    }
}

#[test]
fn test_road_name_matching_variants() {
    assert!(road_name_matches("I-71", "I-71"));
    assert!(road_name_matches("I 71", "I-71"));
    assert!(road_name_matches("Interstate 71", "I-71"));
    assert!(!road_name_matches("71", "I-71")); // No I prefix
    assert!(!road_name_matches("I-90", "I-71"));
}

#[test]
fn test_no_state_rides_the_iteris_rest_api() {
    // The 2026-08-09 live sweep found every Iteris-platform /api/events
    // REST endpoint gone (404); those sites now publish WZDx v4 feeds at
    // /api/wzdx instead.  The parser stays because the CARS parser reuses
    // its closure and location helpers.
    for (key, config) in STATE_APIS {
        assert_ne!(
            config.parser, "iteris",
            "{key} still rides the dead Iteris REST API"
        );
    }
}

#[test]
fn test_fetch_construction_recognises_live_state() {
    // Without network, falls through to empty data; the live state is
    // recognised as supported.
    let provider = RealTrafficProvider::offline();
    let data = provider.fetch_construction("georgia");
    assert_eq!(data.state, "georgia");
}

#[test]
fn test_request_recognises_live_state() {
    let provider = RealTrafficProvider::offline();
    let data = provider.request("new york");
    assert_eq!(data.state, "new york");
}

#[test]
fn test_wzdx_states_in_state_apis() {
    // This is the live roster from the 2026-08-09 sweep.
    for key in [
        "arizona",
        "connecticut",
        "georgia",
        "idaho",
        "nevada",
        "north carolina",
        "pennsylvania",
        "utah",
        "wisconsin",
    ] {
        let config = state_api(key).unwrap_or_else(|| panic!("Missing {key} in STATE_APIS"));
        assert_eq!(config.parser, "wzdx", "{key} parser not wzdx");
        assert_eq!(config.events_endpoint, Some("/api/wzdx"), "{key}");
        assert_eq!(config.construction_endpoint, Some("/api/wzdx"), "{key}");
    }
}

/// COtrip's CARS GraphQL is retired and its WZDx feed wants a key: every
/// refresh of the owner's I-70 drive logged "Failed to fetch traffic data for
/// colorado: invalid JSON" (2026-09-24). Benched, it is never asked.
#[test]
fn test_colorado_is_benched_and_never_fetched() {
    let config = state_api("colorado").unwrap();
    assert_eq!(config.parser, "no_api");
    assert!(!fetches(config, "colorado", false));
    assert!(!fetches(config, "colorado", true));
}

#[test]
fn test_no_api_states_return_empty() {
    let provider = RealTrafficProvider::offline();
    for key in ["alabama", "montana", "wyoming"] {
        assert_eq!(state_api(key).unwrap().parser, "no_api");
        assert!(provider.request(key).events.is_empty());
        assert!(provider.fetch_construction(key).events.is_empty());
    }
}

#[test]
fn test_registry_wzdx_feeds_read_one_full_url() {
    // The FHWA registry feeds live at a URL of their own: the base IS the
    // feed and both endpoints are empty, so the fetch concatenates to the
    // registry URL exactly. Kansas was on the no_api bench until 2026-09-12.
    for key in [
        "iowa",
        "kansas",
        "kentucky",
        "maryland",
        "missouri",
        "washington",
        "new jersey",
        "mississippi",
        "north dakota",
        "delaware",
        "louisiana",
        "new hampshire",
        "vermont",
        "maine",
        "massachusetts",
    ] {
        let config = state_api(key).unwrap_or_else(|| panic!("{key} in STATE_APIS"));
        assert_eq!(config.parser, "wzdx", "{key}");
        assert_eq!(config.construction_parser, None, "{key}");
        assert_eq!(config.events_endpoint, Some(""), "{key}");
        assert_eq!(config.construction_endpoint, Some(""), "{key}");
        let url = config.base_url.unwrap_or_default();
        assert!(url.starts_with("https://"), "{key}: {url}");
        assert!(
            !url.contains("key") && !url.contains("token"),
            "{key}: {url}"
        );
    }
    // The three New England states share one Compass document.
    let compass = state_api("vermont").unwrap().base_url;
    assert_eq!(state_api("new hampshire").unwrap().base_url, compass);
    assert_eq!(state_api("maine").unwrap().base_url, compass);
}

#[test]
fn test_list511_states_in_state_apis() {
    // Florida and New York ride list511 incidents plus WZDx zones.
    for key in ["florida", "new york"] {
        let config = state_api(key).unwrap();
        assert_eq!(config.parser, "list511", "{key}");
        // The events endpoint is a list layer name, not a URL path
        assert_eq!(config.events_endpoint, Some("Incidents"), "{key}");
        // Work zones stay on the WZDx feed
        assert_eq!(config.construction_parser, Some("wzdx"), "{key}");
        assert_eq!(config.construction_endpoint, Some("/api/wzdx"), "{key}");
    }
}

// --- the transport seam (no Python equivalent: urllib was called directly)

#[test]
fn a_failed_fetch_enters_retry_cooldown_and_keeps_serving_empty() {
    let clock_now = Arc::new(StdMutex::new(1_000.0));
    let clock_ref = Arc::clone(&clock_now);
    let provider =
        RealTrafficProvider::offline().with_clock(Arc::new(move || *clock_ref.lock().unwrap()));
    let data = provider.request("georgia");
    assert_eq!(data.source, "empty");
    let failed = provider.failed_until();
    assert_eq!(failed["georgia"], 1_000.0 + RETRY_AFTER_S);
    // Inside the cooldown no fetch happens; after it the next request tries again.
    *clock_now.lock().unwrap() = 1_000.0 + RETRY_AFTER_S + 1.0;
    provider.request("georgia");
    assert_eq!(
        provider.failed_until()["georgia"],
        1_000.0 + RETRY_AFTER_S + 1.0 + RETRY_AFTER_S
    );
}

#[test]
fn a_wzdx_fetch_lands_in_the_cache_under_the_site_name() {
    let feed = serde_json::json!({
        "features": [{
            "id": "wz-1",
            "geometry": {"type": "Point", "coordinates": [-84.0, 33.0]},
            "properties": {
                "core_details": {"event_type": "work-zone", "road_names": ["I-75"]},
                "vehicle_impact": "some-lanes-closed",
            },
        }]
    });
    let transport = Arc::new(TableTransport::new(vec![("511ga.org/api/wzdx", feed)]));
    let provider = RealTrafficProvider::new(transport.clone())
        .with_threaded(false)
        .with_clock(Arc::new(|| 5_000.0));
    // First call returns empty and fetches inline; the cache is then fresh.
    assert_eq!(provider.request("georgia").source, "empty");
    let data = provider.request("georgia");
    assert_eq!(data.source, "Georgia 511GA");
    assert_eq!(data.events.len(), 1);
    assert_eq!(data.events[0].road_name, "I-75");
    assert_eq!(data.cache_time, 5_000.0);
    // Construction reads the same feed and filters to work zones.
    provider.fetch_construction("georgia");
    assert_eq!(provider.fetch_construction("georgia").events.len(), 1);
    assert!(provider.failed_until().is_empty());
    assert_eq!(transport.calls.lock().unwrap().len(), 2);
}

#[test]
fn a_threaded_fetch_delivers_through_the_shared_cache() {
    let feed = serde_json::json!({"features": []});
    let transport = Arc::new(TableTransport::new(vec![("nvroads.com/api/wzdx", feed)]));
    let provider = RealTrafficProvider::new(transport);
    assert_eq!(provider.request("nevada").source, "empty");
    provider.join_background();
    assert_eq!(provider.request("nevada").source, "Nevada NVRoads");
}

#[test]
fn list511_pages_join_pins_and_stop_at_the_record_total() {
    let page = serde_json::json!({
        "recordsTotal": 1,
        "data": [{
            "id": 815973,
            "roadwayName": "SR-70",
            "description": "Crash. Last updated at 03:54 PM.",
            "severity": "Intermediate",
            "isFullClosure": false,
        }],
    });
    let icons = serde_json::json!({"item2": [
        {"itemId": "815973", "location": [27.431793, -82.396087]}
    ]});
    let transport = Arc::new(TableTransport::new(vec![
        ("fl511.com/List/GetData/Incidents", page),
        ("fl511.com/map/mapIcons/Incidents", icons),
    ]));
    let provider = RealTrafficProvider::new(transport.clone()).with_threaded(false);
    provider.request("florida");
    let data = provider.request("florida");
    assert_eq!(data.source, "Florida FL511");
    assert_eq!(data.events[0].latitude, Some(27.431793));
    assert_eq!(data.events[0].description, "Crash.");
    // One list page (the total was reached) plus the pin layer.
    assert_eq!(transport.calls.lock().unwrap().len(), 2);
}

#[test]
fn cars_posts_the_map_features_query_to_graphql() {
    let reply =
        serde_json::json!({"data": {"mapFeaturesQuery": {"mapFeatures": [], "error": null}}});
    let transport = Arc::new(TableTransport::new(vec![("511in.org/api/graphql", reply)]));
    let provider = RealTrafficProvider::new(transport.clone()).with_threaded(false);
    provider.fetch_construction("indiana");
    assert_eq!(
        provider.fetch_construction("indiana").source,
        "Indiana 511IN"
    );
    assert_eq!(
        transport.calls.lock().unwrap()[0],
        "https://511in.org/api/graphql"
    );
}

#[test]
fn urlencode_is_quote_plus() {
    assert_eq!(
        urlencode(&[
            ("columns[0][data]", "description"),
            ("search[value]", "a b&c")
        ]),
        "columns%5B0%5D%5Bdata%5D=description&search%5Bvalue%5D=a+b%26c"
    );
}

#[test]
fn a_feed_being_fetched_is_not_fetched_again_alongside() {
    /// Holds every GET until the test lets it go.
    struct Gate {
        release: StdMutex<std::sync::mpsc::Receiver<()>>,
        calls: StdMutex<usize>,
    }
    impl HttpTransport for Gate {
        fn get(&self, url: &str, _: &[(&str, &str)], _: f64) -> Result<Vec<u8>, TransportError> {
            *self.calls.lock().unwrap() += 1;
            let _ = self.release.lock().unwrap().recv();
            Err(TransportError::new(url))
        }
        fn post(
            &self,
            url: &str,
            _: &[u8],
            _: &[(&str, &str)],
            _: f64,
        ) -> Result<Vec<u8>, TransportError> {
            Err(TransportError::new(url))
        }
    }
    let (release, gate_rx) = std::sync::mpsc::channel();
    let gate = Arc::new(Gate {
        release: StdMutex::new(gate_rx),
        calls: StdMutex::new(0),
    });
    let provider = RealTrafficProvider::new(gate.clone());
    provider.fetch_construction("georgia");
    provider.fetch_construction("georgia");
    release.send(()).unwrap();
    drop(release);
    provider.join_background();
    assert_eq!(*gate.calls.lock().unwrap(), 1);

    // The failed fetch cleared its mark: once the cooldown lapses, it
    // fetches again.
    provider.set_failed_until("georgia", 0.0);
    provider.fetch_construction("georgia");
    provider.join_background();
    assert_eq!(*gate.calls.lock().unwrap(), 2);
}
