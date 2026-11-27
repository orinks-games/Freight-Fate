use super::*;
use crate::sim::real_traffic::TransportError;
use crate::sim::real_weather::{RealWeatherProvider, RETRY_AFTER_S};
use crate::sim::weather_service::WeatherService;
use serde_json::json;
use std::sync::Mutex as StdMutex;

/// 2026-11-27 15:30 UTC: half an hour after the saved readings.
const NOW: f64 = 1_795_793_400.0;

fn fixture(name: &str) -> Value {
    let text = match name {
        "toronto" => include_str!("fixtures/site-toronto.json"),
        "burlington" => include_str!("fixtures/site-burlington.json"),
        "discovery" => include_str!("fixtures/discovery-toronto.json"),
        _ => unreachable!("{name}"),
    };
    serde_json::from_str(text).expect("fixture")
}

fn fixed(seconds: f64) -> Clock {
    Arc::new(move || seconds)
}

/// Answers each URL from `answer` (a null answer is a failed request) and
/// records every URL asked for.
struct Table {
    answer: Box<dyn Fn(&str) -> Value + Send + Sync>,
    calls: StdMutex<Vec<String>>,
}

impl Table {
    fn new(answer: impl Fn(&str) -> Value + Send + Sync + 'static) -> Arc<Self> {
        Arc::new(Self {
            answer: Box::new(answer),
            calls: StdMutex::new(Vec::new()),
        })
    }

    fn calls(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }
}

impl HttpTransport for Table {
    fn get(&self, url: &str, _: &[(&str, &str)], _: f64) -> Result<Vec<u8>, TransportError> {
        self.calls.lock().unwrap().push(url.to_string());
        let doc = (self.answer)(url);
        if doc.is_null() {
            return Err(TransportError::new("Recv failure"));
        }
        Ok(serde_json::to_vec(&doc).unwrap())
    }

    fn post(
        &self,
        _: &str,
        _: &[u8],
        _: &[(&str, &str)],
        _: f64,
    ) -> Result<Vec<u8>, TransportError> {
        Err(TransportError::new("no POST"))
    }
}

/// A clock the test moves, and the setter that moves it.
fn settable(start: f64) -> (Clock, impl Fn(f64)) {
    let now = Arc::new(StdMutex::new(start));
    let read = Arc::clone(&now);
    let clock: Clock = Arc::new(move || *read.lock().unwrap());
    (clock, move |t| *now.lock().unwrap() = t)
}

/// The city page documents asked for, in order.
fn page_calls(table: &Table) -> Vec<String> {
    table
        .calls()
        .into_iter()
        .filter(|c| c.contains("/citypageweather-realtime/items/"))
        .collect()
}

fn site_list(sites: &[(&str, f64, f64)]) -> Value {
    let features: Vec<Value> = sites
        .iter()
        .map(|(id, lat, lon)| {
            json!({"id": id, "type": "Feature",
                   "geometry": {"type": "Point", "coordinates": [lon, lat]},
                   "properties": {"identifier": id}})
        })
        .collect();
    json!({"type": "FeatureCollection", "features": features})
}

// -- parsing -----------------------------------------------------------------

#[test]
fn test_a_site_with_a_condition_parses() {
    let reading = parse_site(&fixture("toronto")).expect("Toronto");
    assert_eq!(reading.condition.as_deref(), Some("Light Rain"));
    assert_eq!(reading.temperature_c, Some(3.7));
    assert_eq!(reading.wind_kmh, 16.0);
    assert_eq!(reading.visibility_mi, None);
    assert_eq!(reading.observed_at, Some(NOW - 1800.0));
    assert_eq!(reading.hourly.len(), 4);
    assert_eq!(
        reading.hourly[0],
        (NOW + 1800.0, "Periods of rain".to_string())
    );
}

#[test]
fn test_a_site_with_no_present_weather_sensor_parses_without_a_condition() {
    let reading = parse_site(&fixture("burlington")).expect("Burlington");
    assert_eq!(reading.condition, None);
    assert_eq!(reading.temperature_c, Some(4.5));
    assert_eq!(reading.wind_kmh, 28.0);
    assert_eq!(reading.forecast_condition(NOW), Some("Periods of rain"));
    // Past the forecast's reach there is nothing to stand in.
    assert_eq!(reading.forecast_condition(NOW + 6.0 * 3600.0), None);
}

#[test]
fn test_an_empty_site_errs_and_calm_wind_and_a_missing_temperature_parse() {
    let empty = json!({"properties": {"currentConditions": {}}});
    assert!(parse_site(&empty).is_err());
    assert!(parse_site(&json!({"properties": {}})).is_err());
    let calm = json!({"properties": {"currentConditions": {
        "timestamp": {"en": "2026-11-27T15:00:00Z"},
        "condition": {"en": "Clear"},
        "wind": {"speed": {"value": {"en": "calm", "fr": "calme"}}},
        "visibility": {"value": {"en": 4.0}}
    }}});
    let reading = parse_site(&calm).expect("calm");
    assert_eq!(reading.wind_kmh, 0.0);
    assert_eq!(reading.temperature_c, None);
    assert!((reading.visibility_mi.unwrap() - 4.0 / 1.609_344).abs() < 1e-9);
    assert!(reading.hourly.is_empty());
}

// -- conditions --------------------------------------------------------------

/// What each of Environment Canada's own condition terms becomes, read at
/// 5 C, no wind and no measured visibility.
fn expected_kind(text: &str) -> Option<WeatherKind> {
    use WeatherKind::*;
    Some(match text {
        "Sunny" | "Mainly Sunny" | "Clear" | "Mainly Clear" | "Ice Crystals" => Clear,
        "Partly Cloudy" | "Mostly Cloudy" | "Cloudy" => Cloudy,
        "Haze" | "Fog Patches" | "Shallow Fog" | "Mist" | "Smoke" | "Dust Devils"
        | "Dust Storm" | "Volcanic Ash" | "Blowing Dust" | "Sandstorm" | "Drifting Dust" => Cloudy,
        "Fog" | "Ice Fog" => Fog,
        "Light Rain Shower"
        | "Precipitation"
        | "Light Precipitation"
        | "Rain Shower"
        | "Light Rain and Drizzle"
        | "Light Rain"
        | "Rain"
        | "Rain and Drizzle"
        | "Hail"
        | "Light Drizzle"
        | "Heavy Drizzle"
        | "Drizzle" => Rain,
        "Heavy Precipitation" | "Heavy Rain and Drizzle" | "Heavy Rain Shower" | "Heavy Rain" => {
            HeavyRain
        }
        "Light Freezing Drizzle"
        | "Light Freezing Rain"
        | "Heavy Freezing Drizzle"
        | "Heavy Freezing Rain"
        | "Freezing Drizzle"
        | "Freezing Rain"
        | "Ice Pellets" => Ice,
        "Light Rain Shower and Flurries"
        | "Light Flurries"
        | "Rain and Flurries"
        | "Rain and Snow"
        | "Light Rain and Snow"
        | "Heavy Rain Shower and Flurries"
        | "Heavy Rain and Snow"
        | "Light Snow"
        | "Snow"
        | "Flurries"
        | "Heavy Flurries"
        | "Heavy Snow"
        | "Drifting Snow"
        | "Snow Pellets"
        | "Snow Grains"
        | "Blowing Snow" => Snow,
        "Squalls"
        | "Thunderstorm with Rain"
        | "Thunderstorm with Heavy Rain"
        | "Thunderstorm with Light Rain"
        | "Thunderstorm"
        | "Heavy Thunderstorm with Rain"
        | "Heavy Thunderstorm"
        | "Funnel Cloud"
        | "Tornado"
        | "Waterspout"
        | "Thunderstorm with Hail"
        | "Heavy Thunderstorm with Hail"
        | "Thunderstorm with Blowing Dust"
        | "Thunderstorm with Dust Storm" => Thunderstorm,
        "Windy" => Wind,
        _ => return None,
    })
}

#[test]
fn test_every_environment_canada_condition_term_maps() {
    let csv = include_str!("fixtures/current-conditions-vocabulary.csv");
    let mut rows = 0;
    for line in csv.lines() {
        let mut cells = line.split(',');
        let (Some(code), Some(text)) = (cells.next(), cells.next()) else {
            continue;
        };
        if code.is_empty() || !code.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        rows += 1;
        let expected = expected_kind(text).unwrap_or_else(|| panic!("no row for {text:?}"));
        assert_eq!(
            map_condition_canada(text, 0.0, None, Some(5.0)),
            expected,
            "{text}"
        );
    }
    assert_eq!(rows, 89, "every row of the vocabulary was read");
}

#[test]
fn test_observed_combinations_outside_the_vocabulary() {
    use WeatherKind::*;
    for (text, expected) in [
        ("Light Rain and Fog", Rain),
        ("Light Drizzle and Fog", Rain),
        ("Freezing Fog", Ice),
    ] {
        assert_eq!(
            map_condition_canada(text, 0.0, None, Some(-1.0)),
            expected,
            "{text}"
        );
    }
}

#[test]
fn test_every_forecast_phrase_seen_maps() {
    use WeatherKind::*;
    // Every first hourly forecast phrase in the 2026-11-27 dry run over the
    // map's Canadian cities.
    let table = [
        ("A few showers", Rain),
        ("A mix of sun and cloud", Cloudy),
        ("Chance of flurries", Cloudy),
        ("Chance of rain showers or flurries", Cloudy),
        ("Chance of showers", Cloudy),
        ("Chance of showers or drizzle", Cloudy),
        ("Chance of showers. Risk of freezing rain", Cloudy),
        ("Chance of wet flurries", Cloudy),
        ("Cloudy", Cloudy),
        ("Fog", Fog),
        ("Freezing rain", Ice),
        ("Freezing rain mixed with rain", Ice),
        ("Light snow", Snow),
        ("Mainly cloudy", Cloudy),
        ("Mainly sunny", Clear),
        ("Overcast", Cloudy),
        ("Periods of light snow", Snow),
        ("Periods of rain", Rain),
        ("Periods of rain mixed with snow", Snow),
        (
            "Periods of rain mixed with snow. Risk of freezing rain",
            Snow,
        ),
        ("Periods of rain or drizzle", Rain),
        ("Periods of rain. Risk of freezing rain", Rain),
        ("Periods of snow mixed with rain", Snow),
        (
            "Periods of snow mixed with rain. Risk of freezing rain",
            Snow,
        ),
        ("Rain", Rain),
        ("Rain mixed with freezing rain", Ice),
        ("Rain mixed with snow. Risk of freezing rain", Snow),
        ("Rain. Risk of freezing rain", Rain),
        ("Showers", Rain),
        ("Snow", Snow),
        ("Sunny", Clear),
    ];
    for (text, expected) in table {
        let phrase = forecast_phrase(text);
        assert_eq!(
            map_condition_canada(&phrase, 0.0, None, Some(1.0)),
            expected,
            "{text} read as {phrase}"
        );
    }
}

#[test]
fn test_fog_and_precipitation_rules() {
    use WeatherKind::*;
    // A measured visibility lets the shared fog gate decide.
    assert_eq!(map_condition_canada("Mist", 0.0, Some(0.3), None), Fog);
    assert_eq!(map_condition_canada("Fog", 0.0, Some(5.0), None), Cloudy);
    assert_eq!(
        map_condition_canada("Ice Fog", 0.0, Some(0.2), Some(-30.0)),
        Fog
    );
    // Precipitation of no known type follows the temperature.
    assert_eq!(
        map_condition_canada("Precipitation", 0.0, None, Some(0.0)),
        Snow
    );
    assert_eq!(
        map_condition_canada("Heavy Precipitation", 0.0, None, Some(-4.0)),
        Snow
    );
    assert_eq!(
        map_condition_canada("Light Precipitation", 0.0, None, Some(2.0)),
        Rain
    );
    assert_eq!(map_condition_canada("Precipitation", 0.0, None, None), Rain);
    assert_eq!(
        map_condition_canada("Heavy Precipitation", 0.0, None, None),
        HeavyRain
    );
    // Wind promotes the clear and cloudy readings, as it does for the NWS.
    assert_eq!(
        map_condition_canada("Ice Crystals", 45.0, None, Some(-35.0)),
        Wind
    );
    assert_eq!(map_condition_canada("Mist", 45.0, None, None), Wind);
    assert_eq!(map_condition_canada("Light Rain", 60.0, None, None), Rain);
}

// -- site search -------------------------------------------------------------

#[test]
fn test_sites_are_picked_nearest_first_within_sixty_miles() {
    let sites = parse_site_list(&fixture("discovery"));
    assert_eq!(sites.len(), 20);
    // Downtown Toronto: the city page site itself first.
    let picked = pick_sites(&sites, 43.65, -79.38);
    assert_eq!(picked.len(), STATION_WALK_LIMIT);
    assert_eq!(picked[0].id, "on-143");
    let far = [
        Site {
            id: "far".into(),
            lat: 46.0,
            lon: -79.38,
        },
        Site {
            id: "farther".into(),
            lat: 47.0,
            lon: -79.38,
        },
    ];
    let lone = pick_sites(&far, 43.65, -79.38);
    assert_eq!(
        lone.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(),
        ["far"]
    );
    assert!(pick_sites(&[], 43.65, -79.38).is_empty());
}

#[test]
fn test_the_search_box_widens_until_it_finds_a_site_and_is_cached() {
    // A point on the Dempster: nothing within 2 degrees.
    let table = Table::new(|url| {
        if url.contains("bbox=") {
            if url.contains("bbox=-142.") {
                site_list(&[("yt-11", 64.06, -139.43)])
            } else {
                site_list(&[])
            }
        } else {
            Value::Null
        }
    });
    let fetcher = EcccFetcher::new(table.clone());
    let sites = fetcher.resolve_sites(65.5, -137.5).expect("found");
    assert_eq!(sites.len(), 1);
    assert_eq!(sites[0].id, "yt-11");
    let calls = table.calls();
    assert_eq!(calls.len(), 3, "{calls:?}");
    assert!(
        calls[0].contains("bbox=-138.2500,64.7500,-136.7500,66.2500"),
        "{}",
        calls[0]
    );
    assert!(
        calls[2].contains("bbox=-142.5000,60.5000,-132.5000,70.5000"),
        "{}",
        calls[2]
    );
    assert!(calls.iter().all(|c| c.starts_with(CANADA_API_ROOT)));
    fetcher.resolve_sites(65.5, -137.5).expect("cached");
    assert_eq!(table.calls().len(), 3, "the list is cached for the place");
}

// -- the walk ----------------------------------------------------------------

fn walk_table(answer: impl Fn(&str) -> Value + Send + Sync + 'static) -> Arc<Table> {
    Table::new(move |url| {
        if url.contains("bbox=") {
            // Burlington nearest, then downtown Toronto.
            site_list(&[("on-95", 43.37, -79.81), ("on-143", 43.65, -79.38)])
        } else {
            answer(url)
        }
    })
}

#[test]
fn test_the_walk_passes_a_site_with_no_condition_for_one_with_one() {
    let table = walk_table(|url| {
        if url.contains("/on-95?") {
            fixture("burlington")
        } else {
            fixture("toronto")
        }
    });
    let (clock, set_clock) = settable(NOW);
    let fetcher = EcccFetcher::new(table.clone()).with_wall_clock(clock);
    let reading = fetcher.default_fetch(43.37, -79.81).expect("read");
    assert_eq!(reading.text, "Light Rain");
    assert_eq!(reading.temperature_c, Some(3.7));
    // Within ten minutes the pages read are not asked for again.
    let before = page_calls(&table).len();
    fetcher.default_fetch(43.37, -79.81).expect("cached");
    assert_eq!(page_calls(&table).len(), before);
    // After that, the site that answered is asked first.
    set_clock(NOW + PAGE_CACHE_S + 1.0);
    fetcher.default_fetch(43.37, -79.81).expect("again");
    let calls = page_calls(&table);
    assert_eq!(calls.len(), before + 1, "{calls:?}");
    assert!(calls.last().unwrap().contains("/on-143?"));
}

#[test]
fn test_with_no_condition_anywhere_the_forecast_stands_in() {
    let table = walk_table(|_| fixture("burlington"));
    let fetcher = EcccFetcher::new(table).with_wall_clock(fixed(NOW));
    let reading = fetcher.default_fetch(43.37, -79.81).expect("derived");
    assert_eq!(reading.text, "Periods of rain");
    assert_eq!(
        map_condition_canada(&reading.text, 28.0, None, Some(4.5)),
        WeatherKind::Rain
    );
    // The nearest site's own measured temperature and wind.
    assert_eq!(reading.temperature_c, Some(4.5));
    assert_eq!(reading.wind_kmh, 28.0);
    assert_eq!(reading.observed_at, Some(NOW - 1800.0));
}

#[test]
fn test_every_site_failing_is_an_error() {
    let table = walk_table(|_| Value::Null);
    let fetcher = EcccFetcher::new(table).with_wall_clock(fixed(NOW));
    let err = fetcher.default_fetch(43.37, -79.81).unwrap_err();
    assert!(err.starts_with("every site failed: on-95:"), "{err}");
    let empty = walk_table(|_| json!({"properties": {"currentConditions": {}}}));
    let fetcher = EcccFetcher::new(empty).with_wall_clock(fixed(NOW));
    assert!(fetcher.default_fetch(43.37, -79.81).is_err());
}

// -- the provider ------------------------------------------------------------

#[test]
fn test_the_provider_asks_each_service_only_for_its_own_places() {
    let table = Table::new(|url| {
        if url.contains("api.weather.gc.ca") {
            if url.contains("bbox=") {
                site_list(&[("on-143", 43.65, -79.38)])
            } else {
                fixture("toronto")
            }
        } else {
            Value::Null
        }
    });
    let provider = RealWeatherProvider::live(table.clone())
        .with_threaded(false)
        .with_wall_clock(fixed(NOW));
    provider.request_from(WeatherService::Canada, "city:toronto_on_ca", 43.65, -79.38);
    assert_eq!(provider.get("city:toronto_on_ca"), Some(WeatherKind::Rain));
    assert_eq!(provider.get_temperature("city:toronto_on_ca"), Some(3.7));
    assert!(table.calls().iter().all(|c| c.starts_with(CANADA_API_ROOT)));
    // The same point asked of the Weather Service is its own entry, and
    // goes to api.weather.gov alone.
    provider.request_from(WeatherService::UnitedStates, "city:us", 43.65, -79.38);
    assert_eq!(provider.get("city:us"), None);
    let us: Vec<String> = table
        .calls()
        .into_iter()
        .filter(|c| !c.starts_with(CANADA_API_ROOT))
        .collect();
    assert_eq!(us.len(), 1, "{us:?}");
    assert!(us[0].starts_with("https://api.weather.gov/points/"));
}

#[test]
fn test_the_provider_maps_canadian_terms_its_own_way() {
    let crystals: FetchFn = Arc::new(|_, _| {
        Ok(Observation::new("Ice Crystals", 5.0, Some(-31.0), None).observed_at(NOW))
    });
    let provider = RealWeatherProvider::new(crystals.clone())
        .with_canada_fetch(crystals)
        .with_threaded(false)
        .with_wall_clock(fixed(NOW));
    provider.request_from(WeatherService::Canada, "ca", 62.45, -114.37);
    assert_eq!(provider.get("ca"), Some(WeatherKind::Clear));
    provider.request_from(WeatherService::UnitedStates, "us", 64.84, -147.72);
    assert_eq!(
        provider.get("us"),
        Some(WeatherKind::Ice),
        "the NWS table is unchanged"
    );
}

// -- the real stations -------------------------------------------------------

/// An ISO time `ago` seconds before [`NOW`].
fn stamp(ago: f64) -> String {
    let at = (NOW - ago) as i64;
    let (h, m) = ((at % 86_400) / 3_600, (at % 3_600) / 60);
    format!("2026-11-27T{h:02}:{m:02}:00.000Z")
}

fn station_doc(stations: &[(&str, &str, f64, f64)]) -> Value {
    let features: Vec<Value> = stations
        .iter()
        .map(|(id, name, lat, lon)| {
            json!({"id": id, "geometry": {"coordinates": [lon, lat]},
                   "properties": {"msc_id": id, "name": name}})
        })
        .collect();
    json!({"features": features})
}

fn swob(props: Value) -> Value {
    json!({"features": [{"properties": props}]})
}

/// Downtown Toronto's city page, and three stations: a stale one in town,
/// the airport with a fresh code, and a fresh one with no code beyond it.
fn toronto_stations(answer: impl Fn(&str) -> Option<Value> + Send + Sync + 'static) -> Arc<Table> {
    Table::new(move |url| {
        if let Some(doc) = answer(url) {
            return doc;
        }
        if url.contains("bbox=") {
            site_list(&[("on-143", 43.65, -79.38)])
        } else if url.contains("/on-143?") {
            fixture("toronto")
        } else if url.contains("swob-stations") {
            station_doc(&[
                ("6158355", "TORONTO CITY", 43.665, -79.395),
                (
                    "6158731",
                    "Toronto/Pearson International",
                    43.6767,
                    -79.6306,
                ),
                ("6158443", "OSHAWA", 43.92, -78.88),
            ])
        } else if url.contains("msc_id-value=6158355") {
            swob(json!({"date_tm-value": stamp(4.0 * 3600.0), "air_temp": 9.0, "prsnt_wx_1": 80}))
        } else if url.contains("msc_id-value=6158731") {
            swob(json!({"date_tm-value": stamp(600.0), "air_temp": 3.9,
                        "avg_wnd_spd_10m_pst2mts": 22.0, "avg_wnd_dir_10m_pst2mts": 270,
                        "max_wnd_gst_spd_10m_pst10mts": 41.0, "vis": 4.0, "prsnt_wx_1": 65}))
        } else if url.contains("msc_id-value=6158443") {
            swob(json!({"date_tm-value": stamp(600.0), "air_temp": 3.0}))
        } else {
            Value::Null
        }
    })
}

#[test]
fn test_a_stale_station_is_walked_past_for_a_fresh_one_with_a_code() {
    let table = toronto_stations(|_| None);
    let fetcher = EcccFetcher::new(table.clone()).with_wall_clock(fixed(NOW));
    let reading = fetcher.default_fetch(43.65, -79.38).expect("read");
    assert_eq!(reading.text, "Light Rain");
    assert_eq!(reading.temperature_c, Some(3.9));
    assert_eq!(reading.wind_kmh, 22.0);
    let d = &reading.details;
    assert_eq!(d.station.as_deref(), Some("Toronto Pearson International"));
    assert!(d
        .station_distance_mi
        .is_some_and(|mi| mi > 10.0 && mi < 15.0));
    assert_eq!(d.wind_direction_deg, Some(270.0));
    assert_eq!(d.gust_kmh, Some(41.0));
    assert!(d
        .visibility_mi
        .is_some_and(|mi| (mi - 4.0 / 1.609_344).abs() < 1e-9));
    assert_eq!(d.basis.as_deref(), Some("code, station 6158731"));
    assert!(d.has_forecast());
    // The airport gave a code, so the walk stopped there.
    assert!(!table.calls().iter().any(|c| c.contains("6158443")));
}

#[test]
fn test_the_station_list_is_read_once_a_session() {
    let table = toronto_stations(|_| None);
    let fetcher = EcccFetcher::new(table.clone()).with_wall_clock(fixed(NOW));
    fetcher.default_fetch(43.65, -79.38).expect("first");
    fetcher.default_fetch(43.65, -79.38).expect("second");
    let lists = table
        .calls()
        .iter()
        .filter(|c| c.contains("swob-stations"))
        .count();
    assert_eq!(lists, 1);
}

#[test]
fn test_without_the_station_list_the_city_page_answers_and_the_list_is_retried() {
    let (clock, set_clock) = settable(NOW);
    let table = toronto_stations(|url| url.contains("swob-stations").then_some(Value::Null));
    let fetcher = EcccFetcher::new(table.clone()).with_wall_clock(clock);
    let reading = fetcher.default_fetch(43.65, -79.38).expect("city page");
    assert_eq!(reading.text, "Light Rain");
    assert_eq!(reading.details.station, None);
    assert_eq!(
        reading.details.basis.as_deref(),
        Some("city page read, site on-143")
    );
    let lists = |table: &Table| {
        table
            .calls()
            .iter()
            .filter(|c| c.contains("swob-stations"))
            .count()
    };
    fetcher.default_fetch(43.65, -79.38).expect("again");
    assert_eq!(lists(&table), 1, "not retried inside a minute");
    set_clock(NOW + RETRY_AFTER_S + 1.0);
    fetcher.default_fetch(43.65, -79.38).expect("later");
    assert_eq!(lists(&table), 2);
}

#[test]
fn test_with_no_fresh_station_the_city_page_answers() {
    let table = toronto_stations(|url| url.contains("msc_id-value=").then_some(Value::Null));
    let fetcher = EcccFetcher::new(table).with_wall_clock(fixed(NOW));
    let reading = fetcher.default_fetch(43.65, -79.38).expect("city page");
    assert_eq!(reading.text, "Light Rain");
    assert_eq!(
        reading.details.basis.as_deref(),
        Some("city page read, site on-143")
    );
}

#[test]
fn test_a_gauge_below_freezing_is_freezing_rain_only_with_an_alert() {
    let cold_gauge = |url: &str| {
        if url.contains("msc_id-value=6158731") {
            Some(swob(
                json!({"date_tm-value": stamp(600.0), "air_temp": -1.5,
                             "pcpn_amt_pst10mts": 0.2, "tot_cld_amt": 100}),
            ))
        } else if url.contains("msc_id-value=") {
            Some(Value::Null)
        } else if url.contains("/on-143?") {
            Some(json!({"properties": {"currentConditions": {
                "timestamp": {"en": stamp(600.0)},
                "temperature": {"value": {"en": -1.5}},
                "wind": {"speed": {"value": {"en": 10}}}}}}))
        } else {
            None
        }
    };
    let quiet = toronto_stations(move |url| {
        cold_gauge(url).or_else(|| url.contains("alerts").then(|| json!({"features": []})))
    });
    let fetcher = EcccFetcher::new(quiet).with_wall_clock(fixed(NOW));
    let reading = fetcher.default_fetch(43.65, -79.38).expect("snow");
    assert_eq!(reading.text, "Snow");
    assert_eq!(
        reading.details.basis.as_deref(),
        Some("gauge, station 6158731")
    );
    let warned = toronto_stations(move |url| {
        cold_gauge(url).or_else(|| {
            url.contains("weather-alerts").then(|| {
                json!({"features": [{"id": "1_fea1", "properties": {
                    "alert_name_en": "freezing rain warning", "status_en": "active",
                    "event_end_datetime": stamp(-3600.0)}}]})
            })
        })
    });
    let fetcher = EcccFetcher::new(warned).with_wall_clock(fixed(NOW));
    let reading = fetcher.default_fetch(43.65, -79.38).expect("freezing rain");
    assert_eq!(reading.text, "Freezing Rain");
}

/// Valemount's page on 2026-11-27: no current conditions, only a forecast.
fn forecast_only_page(text: &str) -> Value {
    json!({"properties": {"currentConditions": {}, "hourlyForecastGroup": {"hourlyForecasts": [
        {"timestamp": stamp(-600.0), "condition": {"en": text}},
        {"timestamp": stamp(-4200.0), "condition": {"en": "Snow"}},
    ]}}})
}

#[test]
fn test_a_site_that_measured_nothing_is_kept_for_its_forecast() {
    let reading = parse_site(&forecast_only_page("Showers")).expect("kept");
    assert!(reading.forecast_only);
    assert_eq!(reading.temperature_c, None);
    assert_eq!(reading.hourly.len(), 2);
    let bare = json!({"properties": {"currentConditions": {}}});
    assert!(parse_site(&bare).is_err());
}

#[test]
fn test_a_far_station_with_a_forecast_only_page_reads_the_forecast() {
    // McBride: its nearest station measures temperature and wind only, and
    // the nearby pages carry a forecast and no reading.
    let table = Table::new(|url| {
        if url.contains("bbox=") && url.contains("citypageweather") {
            site_list(&[("bc-13", 52.83, -119.26)])
        } else if url.contains("/bc-13?") {
            forecast_only_page("Showers")
        } else if url.contains("swob-stations") {
            station_doc(&[("1096468", "MCBRIDE UPPER FRASER (", 54.2, -121.5)])
        } else if url.contains("msc_id-value=1096468") {
            swob(
                json!({"date_tm-value": stamp(600.0), "stn_nam-value": "MCBRIDE UPPER FRASER",
                        "air_temp": 2.5, "avg_wnd_spd_10m_pst10mts": 12.0}),
            )
        } else {
            Value::Null
        }
    });
    let fetcher = EcccFetcher::new(table).with_wall_clock(fixed(NOW));
    let reading = fetcher.default_fetch(53.30, -120.17).expect("forecast");
    assert_eq!(reading.text, "Showers");
    assert_eq!(reading.temperature_c, Some(2.5));
    assert_eq!(reading.wind_kmh, 12.0);
    assert_eq!(
        reading.details.station.as_deref(),
        Some("Mcbride Upper Fraser")
    );
    assert!(reading
        .details
        .station_distance_mi
        .is_some_and(|mi| mi > SITE_REACH_MI));
    assert_eq!(
        reading.details.basis.as_deref(),
        Some("forecast, site bc-13")
    );
}

#[test]
fn test_a_forecast_only_page_is_no_reading_without_stations() {
    let table = Table::new(|url| {
        if url.contains("bbox=") {
            site_list(&[("bc-13", 52.83, -119.26)])
        } else if url.contains("/bc-13?") {
            forecast_only_page("Showers")
        } else {
            Value::Null
        }
    });
    let fetcher = EcccFetcher::new(table).with_wall_clock(fixed(NOW));
    assert!(fetcher.default_fetch(53.30, -120.17).is_err());
}
