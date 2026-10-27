use super::*;
use std::io::Write;
use std::sync::Mutex as StdMutex;

fn loc(id: &str, name: &str, location: &str) -> TruckParkingLocation {
    TruckParkingLocation::new(id, name, location)
}

fn with_counts(mut l: TruckParkingLocation, capacity: i64, available: i64) -> TruckParkingLocation {
    l.capacity = Some(capacity);
    l.available = Some(available);
    l
}

fn at(mut l: TruckParkingLocation, lat: f64, lon: f64) -> TruckParkingLocation {
    l.latitude = Some(lat);
    l.longitude = Some(lon);
    l
}

// --- test_truck_parking.py

#[test]
fn test_truck_parking_location_serialization() {
    let location = TruckParkingLocation {
        address: Some("1234 Highway 70".into()),
        open: true,
        ..at(
            with_counts(
                loc("parking-123", "I-70 Rest Area", "I-70 mile marker 45"),
                50,
                20,
            ),
            39.96,
            -82.99,
        )
    };
    let data = location.to_dict();
    assert_eq!(data["id"], "parking-123");
    assert_eq!(data["name"], "I-70 Rest Area");
    assert_eq!(data["capacity"], 50);
    assert_eq!(data["available"], 20);

    let restored = TruckParkingLocation::from_dict(&data).unwrap();
    assert_eq!(restored.id, "parking-123");
    assert_eq!(restored.capacity, Some(50));
    assert_eq!(restored.available, Some(20));
}

#[test]
fn test_truck_parking_location_from_invalid_dict() {
    assert!(TruckParkingLocation::from_dict(&json!({})).is_none());
    assert!(TruckParkingLocation::from_dict(&Value::Null).is_none());
    assert!(TruckParkingLocation::from_dict(&json!([])).is_none());
}

#[test]
fn test_occupancy_percentage_calculation() {
    let location = with_counts(loc("test-1", "Test Location", "Test Road"), 100, 25);
    assert_eq!(location.occupancy_percentage(), Some(75.0));
}

#[test]
fn test_occupancy_percentage_without_capacity() {
    let location = TruckParkingLocation {
        available: Some(10),
        ..loc("test-1", "Test Location", "Test Road")
    };
    assert_eq!(location.occupancy_percentage(), None);
}

#[test]
fn test_availability_status_full() {
    let location = with_counts(loc("test-1", "Test Location", "Test Road"), 50, 0);
    assert_eq!(location.availability_status(), "full");
}

#[test]
fn test_availability_status_almost_full() {
    let location = with_counts(loc("test-1", "Test Location", "Test Road"), 100, 5);
    assert_eq!(location.availability_status(), "almost_full");
}

#[test]
fn test_availability_status_mostly_full() {
    let location = with_counts(loc("test-1", "Test Location", "Test Road"), 100, 20);
    assert_eq!(location.availability_status(), "mostly_full");
}

#[test]
fn test_availability_status_available() {
    let location = with_counts(loc("test-1", "Test Location", "Test Road"), 100, 50);
    assert_eq!(location.availability_status(), "available");
}

#[test]
fn test_availability_status_closed() {
    let location = TruckParkingLocation {
        open: false,
        ..with_counts(loc("test-1", "Test Location", "Test Road"), 50, 25)
    };
    assert_eq!(location.availability_status(), "closed");
}

#[test]
fn test_availability_status_unknown() {
    let location = TruckParkingLocation {
        capacity: Some(50),
        ..loc("test-1", "Test Location", "Test Road")
    };
    assert_eq!(location.availability_status(), "unknown");
}

#[test]
fn test_parking_data_freshness() {
    let now = wall_time();
    let data = ParkingData::new("ohio", vec![], now, now, "test");
    assert!(data.is_fresh());
    assert!(!data.is_stale());
    // Simulate old data
    let old_data = ParkingData::new(
        "ohio",
        vec![],
        now - CACHE_TTL_S - 1.0,
        now - CACHE_TTL_S - 1.0,
        "test",
    );
    assert!(!old_data.is_fresh());
    assert!(!old_data.is_stale()); // Not yet stale
                                   // Simulate stale data
    let stale_data = ParkingData::new(
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
fn test_parking_data_serialization() {
    let now = wall_time();
    let data = ParkingData::new(
        "ohio",
        vec![with_counts(loc("test-123", "Test Parking", "I-70"), 50, 20)],
        now,
        now,
        "test",
    );
    let serialized = data.to_dict();
    assert_eq!(serialized["state"], "ohio");
    assert_eq!(serialized["locations"].as_array().unwrap().len(), 1);
    assert_eq!(serialized["locations"][0]["id"], "test-123");
}

#[test]
fn test_provider_initialization() {
    let provider = TruckParkingProvider::offline();
    assert!(provider.cache().is_empty());
    assert!(provider.failed_until().is_empty());
}

#[test]
fn test_provider_unsupported_state() {
    let provider = TruckParkingProvider::offline();
    let data = provider.request("california");
    assert_eq!(data.state, "california");
    assert!(data.locations.is_empty());
    assert_eq!(data.source, "unsupported");
}

#[test]
fn test_provider_supported_state_returns_empty_initially() {
    let provider = TruckParkingProvider::offline();
    let data = provider.request("ohio");
    assert_eq!(data.state, "ohio");
    assert!(data.locations.is_empty());
    assert_eq!(data.source, "empty");
}

#[test]
fn test_get_locations_near_filters_by_distance() {
    let provider = TruckParkingProvider::offline();
    let location_near = at(
        with_counts(loc("near-1", "Nearby Parking", "I-70"), 50, 20),
        40.0,
        -83.0,
    );
    let location_far = at(
        with_counts(loc("far-1", "Far Parking", "I-90"), 30, 10),
        41.5,
        -81.7,
    );
    let now = wall_time();
    provider.seed_cache(
        "ohio",
        ParkingData::new("ohio", vec![location_near, location_far], now, now, "test"),
    );
    // Search for locations within 50 miles of Columbus
    let nearby = provider.get_locations_near("ohio", 39.96, -82.99, 50.0);
    assert_eq!(nearby.len(), 1);
    assert_eq!(nearby[0].id, "near-1");
}

#[test]
fn test_get_available_locations_near_filters_availability() {
    let provider = TruckParkingProvider::offline();
    let location_available = at(
        with_counts(loc("avail-1", "Available Parking", "I-70"), 50, 20),
        40.0,
        -83.0,
    );
    let location_full = at(
        with_counts(loc("full-1", "Full Parking", "I-70"), 30, 0),
        40.1,
        -83.0,
    );
    let location_closed = TruckParkingLocation {
        open: false,
        ..at(
            with_counts(loc("closed-1", "Closed Parking", "I-70"), 40, 10),
            40.2,
            -83.0,
        )
    };
    let now = wall_time();
    provider.seed_cache(
        "ohio",
        ParkingData::new(
            "ohio",
            vec![location_available, location_full, location_closed],
            now,
            now,
            "test",
        ),
    );
    let available = provider.get_available_locations_near("ohio", 39.96, -82.99, 50.0);
    assert_eq!(available.len(), 1);
    assert_eq!(available[0].id, "avail-1");
}

#[test]
fn test_get_locations_near_excludes_locations_without_location() {
    let provider = TruckParkingProvider::offline();
    let no_location = with_counts(
        loc("no-loc-1", "Parking without location", "Unknown"),
        50,
        20,
    );
    let now = wall_time();
    provider.seed_cache(
        "ohio",
        ParkingData::new("ohio", vec![no_location], now, now, "test"),
    );
    let nearby = provider.get_locations_near("ohio", 39.96, -82.99, 50.0);
    assert_eq!(nearby.len(), 0);
}

#[test]
fn test_haversine_distance() {
    // Test known distance (Columbus, OH to Cleveland, OH ≈ 126 miles)
    let distance = haversine_distance(39.96, -82.99, 41.50, -81.69);
    assert!(113.0 < distance && distance < 139.0);
}

#[test]
fn test_haversine_distance_same_point() {
    assert_eq!(haversine_distance(40.0, -83.0, 40.0, -83.0), 0.0);
}

#[test]
fn test_tpims_api_config() {
    let ohio_config = tpims_api("ohio").expect("ohio in TPIMS_APIS");
    assert_eq!(ohio_config.name, "Ohio OHGO TPIMS");
    assert_eq!(ohio_config.parking_endpoint, "/v1/truck-parking");
    // OHGO's keyless API is gone (checked 2026-08-09), so Ohio never fetches
    assert_eq!(ohio_config.parser, "no_api");
}

#[test]
fn test_wisconsin_tpims_config() {
    // Wisconsin joined TPIMS 2026-08-09 with the two-endpoint 511wi.gov join.
    let config = tpims_api("wisconsin").unwrap();
    assert_eq!(config.parser, "wi511");
    assert_eq!(config.parking_endpoint, "/List/GetData/truckparking");
    assert_eq!(config.icons_endpoint, Some("/map/mapIcons/TruckParking"));
}

#[test]
fn test_no_api_state_serves_fresh_cache_without_fetching() {
    let provider = TruckParkingProvider::offline();
    let now = wall_time();
    provider.seed_cache(
        "ohio",
        ParkingData::new(
            "ohio",
            vec![loc("seed-1", "Seeded", "I-70")],
            now,
            now,
            "test",
        ),
    );
    assert_eq!(provider.request("ohio").locations[0].id, "seed-1");

    let fresh_provider = TruckParkingProvider::offline();
    let data = fresh_provider.request("ohio");
    assert!(data.locations.is_empty());
    assert_eq!(data.source, "empty");
    assert!(fresh_provider.failed_until().is_empty());
}

// Trimmed real responses recorded 2026-08-09 from 511wi.gov:
// POST /List/GetData/truckparking (site names and live counts) and
// GET /map/mapIcons/TruckParking (coordinates keyed by the same site ids).
fn wi511_list_data() -> Value {
    json!({
        "draw": 1,
        "recordsTotal": 13,
        "recordsFiltered": 13,
        "data": [
            {
                "DT_RowId": "1",
                "tooltipUrl": "/tooltip/TruckParking/1?lang=%7Blang%7D&noCss=true",
                "lastUpdated": "8/9/26, 11:00 AM",
                "organization": "WI-TPIMS",
                "name": "Portage Rest Area #11",
                "pageName": "Columbia County, WI",
                "roadway": "I-39/90/94 EB",
                "exit": "",
                "availableParkingSpaces": 68,
                "totalParkingSpaces": 68,
                "trend": "CLEARING",
                "trustData": "Yes",
                "open": "Yes",
            },
            {
                "DT_RowId": "2",
                "lastUpdated": "8/9/26, 11:00 AM",
                "organization": "WI-TPIMS",
                "name": "Millston Rest Area #22",
                "roadway": "I-94 EB",
                "availableParkingSpaces": 0,
                "totalParkingSpaces": 35,
                "open": "No",
            },
        ],
    })
}

fn wi511_icons_data() -> Value {
    json!({
        "item1": {"size": [29, 35], "origin": [0, 0], "anchor": [14, 34]},
        "item2": [
            {
                "itemId": "1",
                "location": [43.428772, -89.483492],
                "icon": {"url": "/Generated/Content/Images/511/map_truckParking.svg"},
                "title": "",
            },
            {
                "itemId": "2",
                "location": [44.225922, -90.707871],
                "icon": {"url": "/Generated/Content/Images/511/map_truckParking.svg"},
                "title": "",
            },
        ],
    })
}

#[test]
fn test_parse_wi511_locations_joins_counts_and_coordinates() {
    let locations = parse_wi511_locations(&wi511_list_data(), &wi511_icons_data());
    assert_eq!(locations.len(), 2);

    let portage = &locations[0];
    assert_eq!(portage.id, "1");
    assert_eq!(portage.name, "Portage Rest Area #11");
    assert_eq!(portage.location, "I-39/90/94 EB");
    assert_eq!(portage.capacity, Some(68));
    assert_eq!(portage.available, Some(68));
    assert_eq!(portage.latitude, Some(43.428772));
    assert_eq!(portage.longitude, Some(-89.483492));
    assert!(portage.open);
    assert_eq!(portage.last_reported.as_deref(), Some("8/9/26, 11:00 AM"));
    assert_eq!(portage.availability_status(), "available");

    let millston = &locations[1];
    assert_eq!(millston.available, Some(0));
    assert!(!millston.open);
    assert_eq!(millston.availability_status(), "closed");
}

#[test]
fn test_parse_wi511_locations_without_matching_icon() {
    let icons = json!({"item1": {}, "item2": []});
    let locations = parse_wi511_locations(&wi511_list_data(), &icons);
    assert_eq!(locations.len(), 2);
    assert_eq!(locations[0].latitude, None);
    assert_eq!(locations[0].longitude, None);
}

#[test]
fn test_parse_wi511_locations_malformed_data() {
    assert!(parse_wi511_locations(&json!({}), &json!({})).is_empty());
    assert!(parse_wi511_locations(&json!({"data": null}), &json!({"item2": null})).is_empty());
    assert!(parse_wi511_locations(&json!({"data": [null, {}]}), &wi511_icons_data()).is_empty());
}

#[test]
fn test_retry_cooldown_after_failure() {
    let provider = TruckParkingProvider::offline();
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
    let provider = TruckParkingProvider::offline();
    let now = wall_time();
    provider.seed_cache(
        "ohio",
        ParkingData::new(
            "ohio",
            vec![with_counts(
                loc("cached-1", "Cached Parking", "I-70"),
                50,
                20,
            )],
            now,
            now,
            "test",
        ),
    );
    let data = provider.request("ohio");
    assert_eq!(data.locations.len(), 1);
    assert_eq!(data.locations[0].id, "cached-1");
    assert_eq!(data.source, "test");
}

#[test]
fn test_empty_data_creation() {
    let provider = TruckParkingProvider::offline();
    let empty = provider.empty_data("unsupported");
    assert_eq!(empty.state, "unsupported");
    assert!(empty.locations.is_empty());
    assert_eq!(empty.source, "empty");
}

#[test]
fn test_location_from_dict_with_id_only() {
    let location = TruckParkingLocation::from_dict(&json!({"id": "test-123"})).unwrap();
    assert_eq!(location.id, "test-123");
    assert_eq!(location.name, "");
    assert_eq!(location.capacity, None);
    assert_eq!(location.available, None);
}

// --- the fetch path (no Python equivalent: urllib was called directly)

struct Wi511Transport {
    gzip_icons: bool,
    calls: StdMutex<Vec<String>>,
}

impl HttpTransport for Wi511Transport {
    fn get(&self, url: &str, _: &[(&str, &str)], _: f64) -> Result<Vec<u8>, TransportError> {
        self.calls.lock().unwrap().push(url.to_string());
        let body = serde_json::to_vec(&wi511_icons_data()).unwrap();
        if self.gzip_icons {
            let mut encoder =
                flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
            encoder.write_all(&body).unwrap();
            Ok(encoder.finish().unwrap())
        } else {
            Ok(body)
        }
    }

    fn post(
        &self,
        url: &str,
        body: &[u8],
        _: &[(&str, &str)],
        _: f64,
    ) -> Result<Vec<u8>, TransportError> {
        self.calls.lock().unwrap().push(url.to_string());
        assert_eq!(body, b"draw=1&start=0&length=500&lang=en");
        Ok(serde_json::to_vec(&wi511_list_data()).unwrap())
    }
}

#[test]
fn the_wisconsin_fetch_joins_both_endpoints_and_gunzips_the_pins() {
    // 511wi.gov gzips the map icon layer even without Accept-Encoding.
    let transport = Arc::new(Wi511Transport {
        gzip_icons: true,
        calls: StdMutex::new(Vec::new()),
    });
    let provider = TruckParkingProvider::new(transport.clone()).with_threaded(false);
    assert_eq!(provider.request("wisconsin").source, "empty");
    let data = provider.request("wisconsin");
    assert_eq!(data.source, "Wisconsin 511WI TPIMS");
    assert_eq!(data.locations.len(), 2);
    assert_eq!(data.locations[0].latitude, Some(43.428772));
    assert_eq!(
        *transport.calls.lock().unwrap(),
        vec![
            "https://511wi.gov/List/GetData/truckparking".to_string(),
            "https://511wi.gov/map/mapIcons/TruckParking".to_string(),
        ]
    );
}

#[test]
fn read_json_body_sniffs_gzip_by_magic_bytes() {
    let plain = br#"{"a": 1}"#;
    assert_eq!(read_json_body(plain).unwrap(), json!({"a": 1}));
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(plain).unwrap();
    let zipped = encoder.finish().unwrap();
    assert_eq!(&zipped[..2], &[0x1f, 0x8b]);
    assert_eq!(read_json_body(&zipped).unwrap(), json!({"a": 1}));
}

#[test]
fn a_failed_fetch_enters_retry_cooldown() {
    let provider = TruckParkingProvider::offline().with_clock(Arc::new(|| 500.0));
    assert_eq!(provider.request("wisconsin").source, "empty");
    assert_eq!(provider.failed_until()["wisconsin"], 500.0 + RETRY_AFTER_S);
}

// --- test_truck_parking_capacity.py
//
// Every test in that file exercises other modules: stop parsing and
// restriction advisories (data::world_parsing, data::world_models),
// the capacity-aware overnight crunch (sim::hos), the road-stop
// spoken text (sim::trip_models), and the Jason's Law survey matcher
// in tools/curate_route_pois.py (a Python tool, since retired).
// They are kept here by name so the suite diff stays greppable.

fn stop_raw(spaces: Option<i64>) -> serde_json::Value {
    let mut raw = serde_json::json!({
        "name": "Kenosha Safety Rest Area",
        "type": "public_rest_area",
        "at_mi": 30.0,
        "parking": "confirmed",
        "source": "WisDOT rest-area page",
    });
    if let Some(spaces) = spaces {
        raw["parking_spaces"] = serde_json::json!(spaces);
    }
    raw
}
#[test]
fn test_parse_stop_reads_parking_spaces() {
    use crate::data::world_parsing::parse_stop;

    let stop = parse_stop(&stop_raw(Some(45)), 60.0, "a", "b").expect("parses");
    assert_eq!(stop.parking_spaces, 45);
}

#[test]
fn test_parse_stop_defaults_parking_spaces_to_zero() {
    use crate::data::world_parsing::parse_stop;

    let stop = parse_stop(&stop_raw(None), 60.0, "a", "b").expect("parses");
    assert_eq!(stop.parking_spaces, 0);
}

#[test]
fn test_parse_stop_rejects_implausible_parking_spaces() {
    use crate::data::world_parsing::parse_stop;

    for spaces in [5000, -3] {
        let err = parse_stop(&stop_raw(Some(spaces)), 60.0, "a", "b")
            .expect_err("implausible space counts are refused");
        assert!(
            err.to_string().contains("implausible"),
            "unexpected message: {err}"
        );
    }
}

#[test]
fn test_stop_parking_label_speaks_capacity_when_surveyed() {
    use crate::data::world_models::Stop;

    let stop = Stop {
        name: "Rest Area".to_string(),
        at_mi: 10.0,
        stop_type: "public_rest_area".to_string(),
        parking: "confirmed".to_string(),
        parking_spaces: 45,
        ..Stop::default()
    };
    assert_eq!(stop.parking_label(), "confirmed truck parking, 45 spaces");
    let unsurveyed = Stop {
        parking_spaces: 0,
        ..stop
    };
    assert_eq!(unsurveyed.parking_label(), "confirmed truck parking");
}

#[test]
fn test_road_stop_parking_text_speaks_capacity_when_surveyed() {
    use crate::sim::trip_models::RoadStop;

    let mut stop = RoadStop::new("Rest Area", 10.0, "public_rest_area");
    stop.parking = "confirmed".to_string();
    stop.parking_spaces = 22;
    assert_eq!(stop.parking_text(), "confirmed truck parking, 22 spaces");
    let mut silent = RoadStop::new("Rest Area", 10.0, "public_rest_area");
    silent.parking = "likely".to_string();
    silent.parking_spaces = 22;
    assert_eq!(silent.parking_text(), "");
}

#[test]
fn test_parking_crunch_unchanged_when_capacity_unknown() {
    use crate::sim::hos::parking_full_probability;

    // Python's default argument is `spaces=0`; the Rust port spells it out.
    assert_eq!(
        parking_full_probability(23.0, 0),
        parking_full_probability(23.0, 0)
    );
}

#[test]
fn test_small_lots_fill_earlier_and_big_lots_later() {
    use crate::sim::hos::parking_full_probability;

    let base = parking_full_probability(23.0, 0);
    assert!(base > 0.0);
    assert!(parking_full_probability(23.0, 8) > base);
    assert!(parking_full_probability(23.0, 150) < base);
    assert!(parking_full_probability(23.0, 60) < base);
}

#[test]
fn test_capacity_never_creates_daytime_crunch() {
    use crate::sim::hos::parking_full_probability;

    assert_eq!(parking_full_probability(12.0, 5), 0.0);
}

#[test]
fn test_parse_restrictions_orders_and_validates() {
    use crate::data::world_parsing::parse_restrictions;

    let raw = vec![
        serde_json::json!({"at_mi": 40.0, "kind": "weight_limit", "tons": 30.0}),
        serde_json::json!({"at_mi": 12.0, "kind": "low_clearance", "feet": 13.5}),
    ];
    let parsed = parse_restrictions(&raw, 60.0, "a", "b").expect("parses");
    let kinds: Vec<&str> = parsed.iter().map(|r| r.kind.as_str()).collect();
    assert_eq!(kinds, ["low_clearance", "weight_limit"]);
}

#[test]
fn test_parse_restriction_rejects_unknown_kind_and_bad_values() {
    use crate::data::world_parsing::parse_restrictions;

    let cases = [
        (
            serde_json::json!({"at_mi": 5.0, "kind": "toll"}),
            "unknown kind",
        ),
        (
            serde_json::json!({"at_mi": 5.0, "kind": "low_clearance", "feet": 3.0}),
            "implausible clearance",
        ),
        (
            serde_json::json!({"at_mi": 5.0, "kind": "weight_limit", "tons": 90.0}),
            "implausible weight",
        ),
    ];
    for (raw, needle) in cases {
        let err = parse_restrictions(std::slice::from_ref(&raw), 60.0, "a", "b")
            .expect_err("a bad restriction is refused");
        assert!(
            err.to_string().contains(needle),
            "expected {needle:?} in {err}"
        );
    }
}

/// Owner report 2026-08-13: "Posted restriction in 13 miles: low clearance
/// ahead: posted 13 feet 6 inches" read as word salad -- "posted" twice,
/// "ahead" fighting the distance, and no word on whether it matters. The cue
/// text now names the thing, quotes the sign, and answers the only question a
/// driver has: routing already avoided anything impassable.
#[test]
fn test_restriction_spoken_text_is_player_language() {
    use crate::data::world_models::RouteRestriction;

    let clearance = RouteRestriction {
        at_mi: 12.0,
        kind: "low_clearance".to_string(),
        feet: 13.5,
        ..RouteRestriction::default()
    };
    assert_eq!(
        clearance.spoken_ahead(),
        "a low bridge, signed 13 feet 6 inches. Your route clears it"
    );
    assert_eq!(
        clearance.spoken_near(),
        "Low bridge, signed 13 feet 6 inches."
    );
    let whole = RouteRestriction {
        feet: 14.0,
        ..clearance
    };
    assert_eq!(whole.value_text(), "14 feet");
    let weight = RouteRestriction {
        at_mi: 40.0,
        kind: "weight_limit".to_string(),
        tons: 30.0,
        ..RouteRestriction::default()
    };
    assert_eq!(
        weight.spoken_ahead(),
        "a weight limit, signed 30 tons. Your route clears it"
    );
    assert_eq!(weight.spoken_near(), "Weight limit, signed 30 tons.");
    let fractional = RouteRestriction {
        tons: 27.5,
        ..weight
    };
    assert_eq!(fractional.value_text(), "27.5 tons");
}

#[test]
fn test_restriction_rounding_never_speaks_twelve_inches() {
    use crate::data::world_models::RouteRestriction;

    // 13.999 ft rounds to inches == 12 and must carry into the next foot.
    let restriction = RouteRestriction {
        at_mi: 1.0,
        kind: "low_clearance".to_string(),
        feet: 13.999,
        ..RouteRestriction::default()
    };
    assert_eq!(restriction.value_text(), "14 feet");
}
