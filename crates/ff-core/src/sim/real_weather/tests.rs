use super::*;
use serde_json::json;
use std::sync::mpsc;
use std::sync::Mutex as StdMutex;

// -- NWS condition mapping -----------------------------------------------

#[test]
fn test_condition_mapping_basics() {
    assert_eq!(map_condition("Clear", 0.0, None), WeatherKind::Clear);
    assert_eq!(map_condition("Sunny", 0.0, None), WeatherKind::Clear);
    assert_eq!(
        map_condition("Mostly Cloudy", 0.0, None),
        WeatherKind::Cloudy
    );
    assert_eq!(map_condition("Overcast", 0.0, None), WeatherKind::Cloudy);
    assert_eq!(map_condition("Patchy Fog", 0.0, None), WeatherKind::Fog);
    assert_eq!(map_condition("Light Rain", 0.0, None), WeatherKind::Rain);
    assert_eq!(map_condition("Rain Showers", 0.0, None), WeatherKind::Rain);
    assert_eq!(
        map_condition("Heavy Rain", 0.0, None),
        WeatherKind::HeavyRain
    );
    assert_eq!(map_condition("Snow", 0.0, None), WeatherKind::Snow);
    assert_eq!(map_condition("Wintry Mix", 0.0, None), WeatherKind::Snow);
    assert_eq!(
        map_condition("Thunderstorm", 0.0, None),
        WeatherKind::Thunderstorm
    );
}

#[test]
fn test_glaze_conditions_map_to_freezing_rain() {
    // Freezing rain, sleet, and ice used to be lumped into snow; they now map
    // to the glare-ice condition, and must win over the plain rain keyword.
    assert_eq!(map_condition("Freezing Rain", 0.0, None), WeatherKind::Ice);
    assert_eq!(
        map_condition("Light Freezing Drizzle", 0.0, None),
        WeatherKind::Ice
    );
    assert_eq!(map_condition("Sleet", 0.0, None), WeatherKind::Ice);
    assert_eq!(map_condition("Ice Fog", 0.0, None), WeatherKind::Ice);
    // Plain snow phrasing still lands on snow.
    assert_eq!(map_condition("Light Snow", 0.0, None), WeatherKind::Snow);
}

#[test]
fn test_condition_unknown_or_empty_defaults_to_cloudy() {
    assert_eq!(map_condition("", 0.0, None), WeatherKind::Cloudy);
    assert_eq!(
        map_condition("Volcanic Eruption", 0.0, None),
        WeatherKind::Cloudy
    );
}

#[test]
fn test_condition_precipitation_beats_clouds_and_storms_beat_rain() {
    // cloud keyword present but rain wins
    assert_eq!(
        map_condition("Cloudy with Rain", 0.0, None),
        WeatherKind::Rain
    );
    // thunder wins over rain
    assert_eq!(
        map_condition("Thunderstorms and Rain", 0.0, None),
        WeatherKind::Thunderstorm
    );
    // snow wins over rain (e.g. wintry mix described with rain)
    assert_eq!(map_condition("Rain and Snow", 0.0, None), WeatherKind::Snow);
}

#[test]
fn test_strong_wind_promotes_clear_to_windy() {
    assert_eq!(map_condition("Clear", 45.0, None), WeatherKind::Wind);
    assert_eq!(map_condition("Clear", 10.0, None), WeatherKind::Clear);
    // wind never overrides precipitation
    assert_eq!(map_condition("Light Rain", 60.0, None), WeatherKind::Rain);
    // an explicit windy phrase maps to wind on its own
    assert_eq!(map_condition("Breezy", 0.0, None), WeatherKind::Wind);
}

#[test]
fn test_fog_family_gated_on_measured_visibility() {
    // NWS says "Fog/Mist" or "Haze" for anything under ~7 miles of visibility;
    // at 6 miles that's ordinary muggy air, not the game's quarter-mile fog.
    assert_eq!(
        map_condition("Fog/Mist", 0.0, Some(6.0)),
        WeatherKind::Cloudy
    );
    assert_eq!(map_condition("Haze", 0.0, Some(6.0)), WeatherKind::Cloudy);
    // Genuinely low visibility is still fog.
    assert_eq!(map_condition("Fog", 0.0, Some(0.25)), WeatherKind::Fog);
    assert_eq!(map_condition("Fog/Mist", 0.0, Some(1.0)), WeatherKind::Fog);
    // No measured visibility: trust the condition text.
    assert_eq!(map_condition("Fog", 0.0, None), WeatherKind::Fog);
    assert_eq!(map_condition("Patchy Fog", 0.0, None), WeatherKind::Fog);
    // The gate never touches non-fog conditions.
    assert_eq!(
        map_condition("Light Rain", 0.0, Some(6.0)),
        WeatherKind::Rain
    );
    // Hazy but windy air promotes to high winds like any cloudy sky.
    assert_eq!(map_condition("Haze", 45.0, Some(6.0)), WeatherKind::Wind);
}

// -- provider ----------------------------------------------------------------

/// The tests' `SyncProvider`: workers run inline so tests are deterministic.
fn sync_provider(fetch: FetchFn) -> RealWeatherProvider {
    RealWeatherProvider::new(fetch).with_threaded(false)
}

fn fixed(seconds: f64) -> Clock {
    Arc::new(move || seconds)
}

fn shared_clock(cell: &Arc<StdMutex<f64>>) -> Clock {
    let cell = Arc::clone(cell);
    Arc::new(move || *cell.lock().unwrap())
}

fn obs(text: &str, wind: f64, temp: Option<f64>, vis: Option<f64>) -> Observation {
    Observation::new(text, wind, temp, vis)
}

#[test]
fn test_provider_fetches_and_caches() {
    let calls = Arc::new(StdMutex::new(Vec::new()));
    let recorder = Arc::clone(&calls);
    let p = sync_provider(Arc::new(move |lat, lon| {
        recorder.lock().unwrap().push((lat, lon));
        Ok(obs("Light Rain", 12.0, Some(8.0), Some(4.0)))
    }));
    assert_eq!(p.get("Chicago"), None);
    p.request("Chicago", 41.88, -87.63);
    assert_eq!(p.get("Chicago"), Some(WeatherKind::Rain));
    p.request("Chicago", 41.88, -87.63); // cached: no second call
    assert_eq!(calls.lock().unwrap().len(), 1);
}

#[test]
fn test_provider_failure_is_silent_and_rate_limited() {
    let calls = Arc::new(StdMutex::new(0));
    let counter = Arc::clone(&calls);
    let p = sync_provider(Arc::new(move |_, _| {
        *counter.lock().unwrap() += 1;
        Err("no network".to_string())
    }));
    p.request("Denver", 39.7, -105.0);
    assert_eq!(p.get("Denver"), None);
    p.request("Denver", 39.7, -105.0); // within retry window: no new attempt
    assert_eq!(*calls.lock().unwrap(), 1);
}

#[test]
fn test_provider_refetches_after_ttl() {
    let now = Arc::new(StdMutex::new(0.0));
    let conditions = Arc::new(StdMutex::new(vec!["Thunderstorm", "Clear"]));
    let p = sync_provider(Arc::new(move |_, _| {
        let next = conditions.lock().unwrap().pop().unwrap();
        Ok(obs(next, 0.0, None, None))
    }))
    .with_clock(shared_clock(&now));
    p.request("Dallas", 32.8, -96.8);
    assert_eq!(p.get("Dallas"), Some(WeatherKind::Clear));
    *now.lock().unwrap() = CACHE_TTL_S + 1.0;
    p.request("Dallas", 32.8, -96.8);
    assert_eq!(p.get("Dallas"), Some(WeatherKind::Thunderstorm));
}

#[test]
fn test_newly_fetched_old_observation_stays_live_without_claiming_update() {
    // An old station reading can arrive in a fresh response. The five-minute
    // fetch throttle means that response is not itself evidence of another
    // active update, so speech must separate observation age from request
    // activity. (The spoken-report half of this test lives with
    // sim::weather's WeatherSystem; this is the provider half.)
    let wall = 2_000_000.0;
    let observed_at = wall - 12.0 * 60.0;
    let calls = Arc::new(StdMutex::new(0));
    let counter = Arc::clone(&calls);
    let provider = sync_provider(Arc::new(move |_, _| {
        *counter.lock().unwrap() += 1;
        Ok(obs("Light Rain", 5.0, Some(14.0), Some(6.0)).observed_at(observed_at))
    }))
    .with_clock(fixed(0.0))
    .with_wall_clock(fixed(wall));
    provider.request("route-cell", 40.0, -80.0);
    assert_eq!(provider.get("route-cell"), Some(WeatherKind::Rain));
    assert_eq!(provider.observation_age_s("route-cell"), Some(12.0 * 60.0));
    assert!(!provider.refreshing("route-cell"));
    provider.request("route-cell", 40.0, -80.0);
    assert_eq!(*calls.lock().unwrap(), 1); // fresh fetch timestamp still enforces the throttle
}

#[test]
fn test_last_known_report_says_updating_only_during_true_inflight_refresh() {
    // Provider half: `refreshing` is true only while a worker really is
    // in flight. The report wording lives with WeatherSystem.
    let monotonic = Arc::new(StdMutex::new(0.0));
    let wall = Arc::new(StdMutex::new(2_000_000.0));
    let (started_tx, started_rx) = mpsc::channel::<()>();
    let (release_tx, release_rx) = mpsc::channel::<()>();
    let release_rx = StdMutex::new(release_rx);
    let calls = Arc::new(StdMutex::new(0));
    let counter = Arc::clone(&calls);
    let wall_for_fetch = Arc::clone(&wall);
    let provider = RealWeatherProvider::new(Arc::new(move |_, _| {
        let n = {
            let mut c = counter.lock().unwrap();
            *c += 1;
            *c
        };
        let now = *wall_for_fetch.lock().unwrap();
        if n == 1 {
            return Ok(obs("Light Rain", 5.0, Some(14.0), Some(6.0)).observed_at(now - 12.0 * 60.0));
        }
        started_tx.send(()).unwrap();
        release_rx
            .lock()
            .unwrap()
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("released");
        Ok(obs("Clear", 0.0, Some(15.0), Some(10.0)).observed_at(now))
    }))
    .with_clock(shared_clock(&monotonic))
    .with_wall_clock(shared_clock(&wall));
    provider.request("route-cell", 40.0, -80.0);
    provider.join_background();
    assert_eq!(provider.get("route-cell"), Some(WeatherKind::Rain));

    *monotonic.lock().unwrap() = CACHE_TTL_S + 1.0;
    *wall.lock().unwrap() += CACHE_TTL_S + 1.0;
    provider.request("route-cell", 40.0, -80.0);
    started_rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .expect("refresh started");
    assert!(provider.refreshing("route-cell"));
    assert!(provider.stale("route-cell"));

    release_tx.send(()).unwrap();
    provider.join_background();
    assert!(!provider.refreshing("route-cell"));
    assert_eq!(provider.get("route-cell"), Some(WeatherKind::Clear));
}

#[test]
fn test_retry_after_a_failure_stays_unavailable_while_it_runs() {
    // Reading a retry as "loading" flipped the source back and forth every
    // minute and re-announced simulated weather after each failed retry.
    let monotonic = Arc::new(StdMutex::new(0.0));
    let (started_tx, started_rx) = mpsc::channel::<()>();
    let (release_tx, release_rx) = mpsc::channel::<()>();
    let release_rx = StdMutex::new(release_rx);
    let calls = Arc::new(StdMutex::new(0));
    let counter = Arc::clone(&calls);
    let provider = RealWeatherProvider::new(Arc::new(move |_, _| {
        let n = {
            let mut c = counter.lock().unwrap();
            *c += 1;
            *c
        };
        if n > 1 {
            started_tx.send(()).unwrap();
            release_rx
                .lock()
                .unwrap()
                .recv_timeout(std::time::Duration::from_secs(5))
                .expect("released");
        }
        Err("The server answered with error 404.".to_string())
    }))
    .with_clock(shared_clock(&monotonic));
    provider.request("route-cell", 40.0, -80.0);
    provider.join_background();
    assert!(provider.unavailable("route-cell"));

    *monotonic.lock().unwrap() = RETRY_AFTER_S + 1.0;
    provider.request("route-cell", 40.0, -80.0);
    started_rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .expect("retry started");
    assert!(provider.refreshing("route-cell"));
    assert!(provider.unavailable("route-cell"));

    release_tx.send(()).unwrap();
    provider.join_background();
    assert!(provider.unavailable("route-cell"));
}

#[test]
fn test_failed_refresh_expires_last_known_observation_instead_of_loading_forever() {
    let monotonic = Arc::new(StdMutex::new(0.0));
    let wall = Arc::new(StdMutex::new(1_000_000.0));
    let calls = Arc::new(StdMutex::new(0));
    let counter = Arc::clone(&calls);
    let wall_for_fetch = Arc::clone(&wall);
    let p = sync_provider(Arc::new(move |_, _| {
        let mut c = counter.lock().unwrap();
        *c += 1;
        if *c == 1 {
            let now = *wall_for_fetch.lock().unwrap();
            return Ok(obs("Heavy Rain", 5.0, Some(12.0), Some(1.0)).observed_at(now));
        }
        Err("offline".to_string())
    }))
    .with_clock(shared_clock(&monotonic))
    .with_wall_clock(shared_clock(&wall));
    p.request("route-cell", 40.0, -80.0);
    assert_eq!(p.get("route-cell"), Some(WeatherKind::HeavyRain));

    *monotonic.lock().unwrap() = CACHE_TTL_S + 1.0;
    *wall.lock().unwrap() = CACHE_TTL_S + 1.0 + 1_000_000.0;
    p.request("route-cell", 40.0, -80.0);
    assert!(p.stale("route-cell"));
    assert!(!p.unavailable("route-cell"));

    *monotonic.lock().unwrap() = STALE_AFTER_S + 1.0;
    *wall.lock().unwrap() = 1_000_000.0 + STALE_AFTER_S + 1.0;
    p.request("route-cell", 40.0, -80.0);
    assert_eq!(p.get("route-cell"), None);
    assert!(p.unavailable("route-cell"));
    assert!(p.unavailable("route-cell"));
}

#[test]
fn test_same_place_keys_share_one_observation() {
    // Observations belong to stations, not to request-key strings: the city
    // menu's warm-up and the trip's first route cell are the same place, so
    // the second key must ride the first key's fetch instead of refetching.
    let calls = Arc::new(StdMutex::new(0));
    let counter = Arc::clone(&calls);
    let p = sync_provider(Arc::new(move |_, _| {
        *counter.lock().unwrap() += 1;
        Ok(obs("Light Rain", 5.0, Some(14.0), Some(6.0)).observed_at(2_000_000.0))
    }))
    .with_clock(fixed(0.0))
    .with_wall_clock(fixed(2_000_000.0));
    p.request("city:newark", 40.7357, -74.1724);
    p.request("route:newark:philadelphia:0", 40.7357, -74.1724);
    assert_eq!(
        p.get("route:newark:philadelphia:0"),
        Some(WeatherKind::Rain)
    );
    assert_eq!(p.get_temperature("route:newark:philadelphia:0"), Some(14.0));
    assert_eq!(*calls.lock().unwrap(), 1);
}

#[test]
fn test_provider_reports_session_observation_history() {
    let p = sync_provider(Arc::new(|_, _| {
        Ok(obs("Clear", 0.0, Some(20.0), Some(10.0)).observed_at(2_000_000.0))
    }))
    .with_clock(fixed(0.0))
    .with_wall_clock(fixed(2_000_000.0));
    assert!(!p.has_any_observation());
    p.request("route-cell", 40.0, -80.0);
    assert!(p.has_any_observation());
}

#[test]
fn test_hourly_metar_cadence_is_still_live() {
    // NWS stations file routine observations once an hour, so the newest
    // available observation is 30-60 minutes old for most of every hour. That
    // is what "current conditions" means; it must never read as an NWS failure
    // and push the player onto simulated fallback weather.
    let now = 2_000_000.0;
    let p = sync_provider(Arc::new(move |_, _| {
        Ok(obs("Heavy Rain", 5.0, Some(12.0), Some(1.0)).observed_at(now - 45.0 * 60.0))
    }))
    .with_clock(fixed(0.0))
    .with_wall_clock(fixed(now));
    p.request("route-cell", 40.0, -80.0);
    assert_eq!(p.get("route-cell"), Some(WeatherKind::HeavyRain));
    assert!(!p.unavailable("route-cell"));
}

#[test]
fn test_old_nws_observation_timestamp_is_never_treated_as_live() {
    let now = 2_000_000.0;
    let old = now - OUTAGE_HOLD_MAX_AGE_S - 1.0;
    let p = sync_provider(Arc::new(move |_, _| {
        Ok(obs("Heavy Rain", 5.0, Some(12.0), Some(1.0)).observed_at(old))
    }))
    .with_clock(fixed(0.0))
    .with_wall_clock(fixed(now));
    p.request("route-cell", 40.0, -80.0);
    assert_eq!(p.get("route-cell"), None);
}

/// One dropped request at a route-cell boundary must never simulate.
///
/// The truck crosses into a fresh 20-mile cell, that cell's first fetch
/// fails, and the previous cell's live conditions are seconds old: the
/// weather holds them as last-known and retries -- it does not flip to
/// simulated fallback while NWS is fine (owner ruling, 2026-08-08).
#[test]
fn test_new_cell_fetch_failure_holds_last_known_not_fallback() {
    use crate::sim::weather::WeatherSystem;

    let boom = Arc::new(StdMutex::new(false));
    let armed = Arc::clone(&boom);
    let provider = sync_provider(Arc::new(move |_, _| {
        if *armed.lock().unwrap() {
            return Err("transient".to_string());
        }
        Ok(obs("Heavy Rain", 5.0, Some(12.0), Some(1.0)).observed_at(2_000_000.0))
    }))
    .with_clock(fixed(0.0))
    .with_wall_clock(fixed(2_000_000.0));

    let mut weather =
        WeatherSystem::new("great_lakes", Some(1), Some(Box::new(provider)), None, true);
    weather.set_city("cell-0", 40.0, -80.0);
    weather.update(0.0);
    assert_eq!(weather.source_status(), "live");
    assert_eq!(weather.current, WeatherKind::HeavyRain);

    *boom.lock().unwrap() = true;
    weather.set_city("cell-1", 41.0, -81.0);
    weather.update(0.0);
    assert_eq!(weather.source_status(), "last_known");
    assert_eq!(weather.current, WeatherKind::HeavyRain); // held, not resimulated
                                                         // And it stays held on later ticks rather than drifting to fallback.
    weather.update(1.0);
    assert_eq!(weather.source_status(), "last_known");
    assert_eq!(weather.current, WeatherKind::HeavyRain);
}

#[test]
fn test_cold_session_with_failing_provider_still_reaches_fallback() {
    use crate::sim::weather::WeatherSystem;

    let provider = sync_provider(Arc::new(|_, _| Err("offline".to_string())))
        .with_clock(fixed(0.0))
        .with_wall_clock(fixed(2_000_000.0));
    let mut weather =
        WeatherSystem::new("great_lakes", Some(1), Some(Box::new(provider)), None, true);
    weather.set_city("cell-0", 40.0, -80.0);
    weather.update(0.0);
    assert_eq!(weather.source_status(), "fallback");
}

#[test]
fn test_expired_observation_retries_then_recovers_to_live_weather() {
    // Provider half: WeatherSystem.update drives request() every tick; here
    // the requests are issued directly at the same clock readings.
    let monotonic = Arc::new(StdMutex::new(0.0));
    let wall = Arc::new(StdMutex::new(2_000_000.0));
    let calls = Arc::new(StdMutex::new(0));
    let counter = Arc::clone(&calls);
    let wall_for_fetch = Arc::clone(&wall);
    let provider = sync_provider(Arc::new(move |_, _| {
        let mut c = counter.lock().unwrap();
        *c += 1;
        let mut observed_at = *wall_for_fetch.lock().unwrap();
        if *c == 1 {
            observed_at -= OUTAGE_HOLD_MAX_AGE_S + 1.0;
        }
        Ok(obs("Clear", 0.0, Some(10.0), Some(10.0)).observed_at(observed_at))
    }))
    .with_clock(shared_clock(&monotonic))
    .with_wall_clock(shared_clock(&wall));

    provider.request("route-cell", 40.0, -80.0);
    assert_eq!(*calls.lock().unwrap(), 1);
    assert_eq!(provider.get("route-cell"), None);
    assert!(provider.unavailable("route-cell"));

    *monotonic.lock().unwrap() = RETRY_AFTER_S - 0.1;
    *wall.lock().unwrap() += RETRY_AFTER_S - 0.1;
    provider.request("route-cell", 40.0, -80.0);
    assert_eq!(*calls.lock().unwrap(), 1);
    assert!(provider.unavailable("route-cell"));

    *monotonic.lock().unwrap() = RETRY_AFTER_S + 0.1;
    *wall.lock().unwrap() += 0.2;
    provider.request("route-cell", 40.0, -80.0);
    assert_eq!(*calls.lock().unwrap(), 2);
    assert_eq!(provider.get("route-cell"), Some(WeatherKind::Clear));
    assert!(!provider.unavailable("route-cell"));
}

// -- stale-observation robustness ----------------------------------------------
//
// Regression coverage for a tester report (2026-08-11): a dead/parked station
// made every fetch for a route segment raise "NWS observation is too old to
// use", which escaped through the worker's generic failure handler as a
// WARNING-with-traceback every RETRY_AFTER_S (about once a minute) for
// stretches of a drive. The fix keeps the staleness gate well clear of a
// normal hourly METAR cadence, handles a too-old reading as a routine miss
// instead of an error, and logs it at most once per stretch.

#[test]
fn test_59_minute_old_observation_is_accepted() {
    // A METAR up to just under an hour old is completely normal and must
    // read as live, not trip the staleness gate.
    let now = 2_000_000.0;
    let p = sync_provider(Arc::new(move |_, _| {
        Ok(obs("Clear", 0.0, Some(10.0), Some(10.0)).observed_at(now - 59.0 * 60.0))
    }))
    .with_clock(fixed(0.0))
    .with_wall_clock(fixed(now));
    p.request("route-cell", 40.0, -80.0);
    assert_eq!(p.get("route-cell"), Some(WeatherKind::Clear));
    assert!(!p.unavailable("route-cell"));
}

#[test]
fn test_stale_observation_does_not_raise_out_of_worker() {
    // A too-old observation used to raise a bare ValueError from inside the
    // fetch path; the worker must absorb it internally instead.
    let now = 2_000_000.0;
    let p =
        RealWeatherProvider::new(Arc::new(move |_, _| {
            Ok(obs("Clear", 0.0, Some(10.0), Some(10.0))
                .observed_at(now - OUTAGE_HOLD_MAX_AGE_S - 1.0))
        }))
        .with_clock(fixed(0.0))
        .with_wall_clock(fixed(now));
    // Calling the worker body directly (synchronously, no thread boundary to
    // hide behind) must not panic.
    p.worker("route:charleston_wv_us:roanoke_va_us:6", 40.0, -80.0);
    assert_eq!(p.get("route:charleston_wv_us:roanoke_va_us:6"), None);
    assert!(p.unavailable("route:charleston_wv_us:roanoke_va_us:6"));
}

#[test]
fn test_stale_observation_keeps_previous_conditions_smooth() {
    // Once real conditions are known, one stale refresh must not yank the
    // player back to simulated fallback weather -- the previous observation
    // keeps serving (per `usable`/`STALE_AFTER_S`) until it genuinely
    // expires or a fresh reading arrives.
    let monotonic = Arc::new(StdMutex::new(0.0));
    let wall = Arc::new(StdMutex::new(2_000_000.0));
    let calls = Arc::new(StdMutex::new(0));
    let counter = Arc::clone(&calls);
    let wall_for_fetch = Arc::clone(&wall);
    let p =
        sync_provider(Arc::new(move |_, _| {
            let mut c = counter.lock().unwrap();
            *c += 1;
            let now = *wall_for_fetch.lock().unwrap();
            if *c == 1 {
                return Ok(obs("Heavy Rain", 5.0, Some(12.0), Some(1.0)).observed_at(now));
            }
            Ok(obs("Clear", 0.0, Some(10.0), Some(10.0))
                .observed_at(now - OBSERVATION_MAX_AGE_S - 1.0))
        }))
        .with_clock(shared_clock(&monotonic))
        .with_wall_clock(shared_clock(&wall));
    p.request("route-cell", 40.0, -80.0);
    assert_eq!(p.get("route-cell"), Some(WeatherKind::HeavyRain));

    *monotonic.lock().unwrap() = CACHE_TTL_S + 1.0;
    *wall.lock().unwrap() += CACHE_TTL_S + 1.0;
    p.request("route-cell", 40.0, -80.0);
    assert_eq!(*calls.lock().unwrap(), 2); // the stale refresh really was attempted
    assert_eq!(p.get("route-cell"), Some(WeatherKind::HeavyRain)); // but did not overwrite the cache
    assert!(!p.unavailable("route-cell"));
}

#[test]
fn test_repeated_stale_fetches_do_not_multiply_warnings() {
    // A station stuck past the staleness cutoff keeps getting retried
    // (RETRY_AFTER_S is under a minute), but the log must not repeat a full
    // warning-with-traceback on every attempt -- at most one note per stretch,
    // and never at WARNING. The worker outcome stands in for the log capture.
    let now = 2_000_000.0;
    let segment = "route:charleston_wv_us:roanoke_va_us:6";
    let p =
        RealWeatherProvider::new(Arc::new(move |_, _| {
            Ok(obs("Clear", 0.0, Some(10.0), Some(10.0))
                .observed_at(now - OUTAGE_HOLD_MAX_AGE_S - 1.0))
        }))
        .with_clock(fixed(0.0))
        .with_wall_clock(fixed(now));
    let outcomes: Vec<WorkerOutcome> = (0..5).map(|_| p.worker(segment, 40.0, -80.0)).collect();
    assert!(p.unavailable(segment));
    assert!(!outcomes.contains(&WorkerOutcome::Failed));
    let notes = outcomes
        .iter()
        .filter(|o| **o == WorkerOutcome::StaleLogged)
        .count();
    assert_eq!(notes, 1);
    assert_eq!(outcomes[1..], [WorkerOutcome::StaleSilent; 4]);
}

// -- temperature ---------------------------------------------------------------

#[test]
fn test_temp_to_c_handles_units_and_nulls() {
    assert_eq!(
        temp_to_c(Some(&json!({"value": 20.0, "unitCode": "wmoUnit:degC"}))).unwrap(),
        Some(20.0)
    );
    assert_eq!(
        temp_to_c(Some(&json!({"value": 68.0, "unitCode": "wmoUnit:degF"}))).unwrap(),
        Some(20.0)
    );
    assert_eq!(
        temp_to_c(Some(&json!({"value": null, "unitCode": "wmoUnit:degC"}))).unwrap(),
        None
    );
    assert_eq!(temp_to_c(None).unwrap(), None);
}

#[test]
fn test_visibility_to_mi_handles_units_and_nulls() {
    let mi = visibility_to_mi(Some(&json!({"value": 16093.44, "unitCode": "wmoUnit:m"})))
        .unwrap()
        .unwrap();
    assert!((mi - 10.0).abs() < 0.01);
    let km = visibility_to_mi(Some(&json!({"value": 1.609344, "unitCode": "wmoUnit:km"})))
        .unwrap()
        .unwrap();
    assert!((km - 1.0).abs() < 0.01);
    assert_eq!(
        visibility_to_mi(Some(&json!({"value": null, "unitCode": "wmoUnit:m"}))).unwrap(),
        None
    );
    assert_eq!(visibility_to_mi(None).unwrap(), None);
}

#[test]
fn wind_to_kmh_converts_each_unit() {
    assert_eq!(
        wind_to_kmh(Some(&json!({"value": 10.0, "unitCode": "wmoUnit:km_h-1"}))).unwrap(),
        10.0
    );
    assert_eq!(
        wind_to_kmh(Some(&json!({"value": 10.0, "unitCode": "wmoUnit:m_s-1"}))).unwrap(),
        36.0
    );
    assert_eq!(
        wind_to_kmh(Some(&json!({"value": 10.0, "unitCode": "mph"}))).unwrap(),
        16.09344
    );
    assert_eq!(wind_to_kmh(Some(&json!({"value": null}))).unwrap(), 0.0);
    assert_eq!(wind_to_kmh(None).unwrap(), 0.0);
}

#[test]
fn test_provider_reports_haze_with_good_visibility_as_cloudy() {
    // The regression that shipped fog horns over a 6-mile-visibility summer
    // haze: the provider itself must apply the visibility gate.
    let p = sync_provider(Arc::new(|_, _| Ok(obs("Haze", 9.0, Some(27.0), Some(6.0)))));
    p.request("Wilmington", 39.74, -75.54);
    assert_eq!(p.get("Wilmington"), Some(WeatherKind::Cloudy));
}

#[test]
fn test_provider_caches_observed_temperature() {
    let p = sync_provider(Arc::new(|_, _| {
        Ok(obs("Clear", 0.0, Some(-3.5), Some(10.0)))
    }));
    assert_eq!(p.get_temperature("Fargo"), None); // nothing fetched yet
    p.request("Fargo", 46.88, -96.79);
    assert_eq!(p.get("Fargo"), Some(WeatherKind::Clear));
    assert_eq!(p.get_temperature("Fargo"), Some(-3.5));
}

#[test]
fn test_provider_temperature_none_when_station_omits_it() {
    let p = sync_provider(Arc::new(|_, _| Ok(obs("Clear", 0.0, None, None))));
    p.request("Reno", 39.5, -119.8);
    assert_eq!(p.get("Reno"), Some(WeatherKind::Clear));
    assert_eq!(p.get_temperature("Reno"), None);
}

#[test]
fn test_weather_system_reports_real_observed_temperature() {
    use crate::sim::weather::WeatherSystem;

    // A live provider with a real reading: the system reports the station's
    // temperature, not the seasonal climate model.
    let provider = sync_provider(Arc::new(|_, _| {
        Ok(obs("Clear", 0.0, Some(2.0), Some(10.0))) // 2 C real
    }));
    let mut ws = WeatherSystem::new("great_lakes", Some(1), Some(Box::new(provider)), None, true);
    ws.set_city("Chicago", 41.88, -87.63);
    ws.update(1.0);
    assert!(ws.live);
    assert_eq!(ws.temperature_c(), Some(2.0));
    // 2 C -> 35.6 F -> "36"
    assert!(ws.describe(true, false).contains("36 degrees"));
}

#[test]
fn test_live_report_omits_modeled_temperature_when_observation_has_none() {
    use crate::sim::weather::{WeatherProvider, WeatherSystem};

    struct ConditionsOnly;
    impl WeatherProvider for ConditionsOnly {
        fn request(&mut self, _city: &str, _lat: f64, _lon: f64) {}
        fn get(&mut self, _city: &str) -> Option<WeatherKind> {
            Some(WeatherKind::HeavyRain)
        }
    }

    let mut ws = WeatherSystem::new(
        "desert_southwest",
        Some(1),
        Some(Box::new(ConditionsOnly)),
        None,
        true,
    );
    ws.set_city("route-cell", 33.45, -112.07);
    ws.update(1.0);

    assert_eq!(ws.source_status(), "live");
    // The seasonal model remains available to mechanics.
    assert!(ws.temperature_c().is_some());
    assert!(!ws.report_lead(true).contains("degrees"));
    assert!(!ws.source_conditions(true).contains("degrees"));
    assert!(!ws.source_conditions(true).contains("visibility"));
    assert!(!ws.source_conditions(true).contains("slick roads"));
}

// -- weather system integration ------------------------------------------------

#[test]
fn test_weather_system_applies_live_conditions() {
    use crate::sim::weather::WeatherSystem;

    let provider = sync_provider(Arc::new(|_, _| {
        Ok(obs("Heavy Rain", 5.0, Some(18.0), Some(1.5)))
    }));
    let mut ws = WeatherSystem::new(
        "desert_southwest",
        Some(1),
        Some(Box::new(provider)),
        None,
        true,
    );
    ws.set_city("Phoenix", 33.45, -112.07);
    let changed = ws.update(1.0);
    assert!(ws.live);
    assert_eq!(ws.current, WeatherKind::HeavyRain);
    assert_eq!(changed, Some(WeatherKind::HeavyRain));
    // Stable live data: no further changes, simulation stays paused.
    for _ in 0..100 {
        assert_eq!(ws.update(30.0), None);
    }
    assert_eq!(ws.current, WeatherKind::HeavyRain);
}

#[test]
fn test_late_observation_for_previous_route_cell_cannot_replace_current_cell() {
    use crate::sim::weather::{WeatherProvider, WeatherSystem};

    type CellConditions = Arc<StdMutex<Vec<(&'static str, Option<WeatherKind>)>>>;
    struct LocationProvider {
        data: CellConditions,
    }
    impl WeatherProvider for LocationProvider {
        fn request(&mut self, _city: &str, _lat: f64, _lon: f64) {}
        fn get(&mut self, city: &str) -> Option<WeatherKind> {
            self.data
                .lock()
                .unwrap()
                .iter()
                .find(|(k, _)| *k == city)
                .and_then(|(_, v)| *v)
        }
        // A mild station reading, so the real-calendar season model (read
        // off the wall clock) never turns this rain to ice on a cold night.
        fn get_temperature(&mut self, _city: &str) -> Option<f64> {
            Some(15.0)
        }
    }

    let data = Arc::new(StdMutex::new(vec![
        ("cell-a", None),
        ("cell-b", Some(WeatherKind::Rain)),
    ]));
    let mut ws = WeatherSystem::new(
        "great_lakes",
        Some(1),
        Some(Box::new(LocationProvider {
            data: Arc::clone(&data),
        })),
        None,
        true,
    );
    ws.set_city("cell-a", 41.0, -87.0);
    ws.update(0.0);
    ws.set_city("cell-b", 40.0, -86.0);
    ws.update(0.0);
    assert_eq!(ws.current, WeatherKind::Rain);

    data.lock().unwrap()[0].1 = Some(WeatherKind::HeavyRain);
    ws.update(0.0);
    assert_eq!(ws.city.as_deref(), Some("cell-b"));
    assert_eq!(ws.current, WeatherKind::Rain);
}

/// The calendar toggle must not restart the simulated transition timer.
///
/// Live weather may change when the provider's observation or target city
/// changes, but it must not wander from rain to heavy rain or fog on its own.
#[test]
fn test_live_conditions_do_not_evolve_simulated_weather_with_independent_calendar() {
    use crate::sim::weather::WeatherSystem;

    let provider = sync_provider(Arc::new(|_, _| Ok(obs("Rain", 5.0, Some(18.0), Some(5.0)))));
    let mut ws = WeatherSystem::new(
        "great_lakes",
        Some(2),
        Some(Box::new(provider)),
        Some(100.0),
        false,
    );
    ws.set_city("Chicago", 41.88, -87.63);
    ws.update(1.0);
    assert!(ws.live);
    // With the career calendar independent of the live feed, precipitation is
    // reconciled to the career season: live rain in a freezing Great Lakes
    // window lands as freezing rain. What matters here is that it settles once
    // and then holds -- it must not wander on its own.
    assert_eq!(ws.current, WeatherKind::Ice);
    assert_eq!(
        ws.source_conditions(true),
        "observation rain, 64 degrees; treated as freezing rain for driving"
    );
    assert!(ws.report_lead(true).starts_with(
        "Live weather: observation rain, 64 degrees; treated as freezing rain for driving"
    ));

    for _ in 0..200 {
        assert_eq!(ws.update(30.0), None);
    }
    assert_eq!(ws.current, WeatherKind::Ice);
}

/// With a provider attached, weather starts clear and holds -- no simulated
/// warm-up -- until live data (or a confirmed offline state) arrives.
#[test]
fn test_weather_system_holds_clear_while_live_data_pending() {
    use crate::sim::weather::{WeatherProvider, WeatherSystem};

    struct Pending;
    impl WeatherProvider for Pending {
        fn request(&mut self, _city: &str, _lat: f64, _lon: f64) {}
        fn get(&mut self, _city: &str) -> Option<WeatherKind> {
            None
        }
        fn unavailable(&mut self, _city: &str) -> bool {
            false // still fetching, not offline
        }
    }

    let mut ws = WeatherSystem::new(
        "pacific_northwest",
        Some(1),
        Some(Box::new(Pending)),
        None,
        true,
    );
    ws.set_city("Seattle", 47.61, -122.33);
    assert_eq!(ws.current, WeatherKind::Clear);
    for _ in 0..200 {
        // no simulated transitions while pending
        assert_eq!(ws.update(30.0), None);
    }
    assert_eq!(ws.current, WeatherKind::Clear);
    assert!(!ws.live);
}

#[test]
fn test_weather_system_falls_back_when_offline() {
    use crate::sim::weather::WeatherSystem;

    let provider = sync_provider(Arc::new(|_, _| Err("offline".to_string())));
    let mut ws = WeatherSystem::new("great_lakes", Some(2), Some(Box::new(provider)), None, true);
    ws.set_city("Chicago", 41.88, -87.63);
    let changes: Vec<Option<WeatherKind>> = (0..200).map(|_| ws.update(15.0)).collect();
    assert!(!ws.live);
    // simulated weather still evolves
    assert!(changes.iter().any(|c| c.is_some()));
}

#[test]
fn test_weather_system_without_provider_unchanged() {
    use crate::sim::weather::WeatherSystem;

    let mut ws = WeatherSystem::new("great_lakes", Some(3), None, None, true);
    ws.update(1.0);
    assert!(!ws.live);
}

#[test]
fn test_world_cities_have_coordinates() {
    let world = crate::data::world::World::load().expect("the shipped world loads");
    for city in world.cities.values() {
        assert!(city.lat != 0.0, "{} missing latitude", city.name);
        assert!(city.lon != 0.0, "{} missing longitude", city.name);
        assert!(city.lat > 24.0 && city.lat < 50.0);
        assert!(city.lon > -125.0 && city.lon < -66.0);
    }
}

// -- the station walk (monkeypatched _get_json in Python; a table transport here)

fn nws_obs(text: &str, age_s: f64, now: f64) -> Value {
    let stamp = chrono::DateTime::from_timestamp((now - age_s) as i64, 0)
        .unwrap()
        .to_rfc3339();
    json!({
        "properties": {
            "textDescription": text,
            "windSpeed": {"value": 10.0, "unitCode": "wmoUnit:km_h-1"},
            "temperature": {"value": 20.0, "unitCode": "wmoUnit:degC"},
            "visibility": {"value": 16093.44, "unitCode": "wmoUnit:m"},
            "timestamp": stamp,
        }
    })
}

/// Answers the discovery chain for any point with two stations, then
/// each station's latest observation from `answer`; a null answer is the
/// 404 NWS gives a station with no current observation.
struct StationTransport {
    answer: Box<dyn Fn(&str) -> Value + Send + Sync>,
    calls: StdMutex<Vec<String>>,
}

impl HttpTransport for StationTransport {
    fn get(&self, url: &str, _: &[(&str, &str)], _: f64) -> Result<Vec<u8>, TransportError> {
        self.calls.lock().unwrap().push(url.to_string());
        let doc = if url.contains("/points/") {
            json!({"properties": {"observationStations": "https://x/stations"}})
        } else if url.ends_with("/stations") {
            json!({"observationStations": ["https://x/st0", "https://x/st1"]})
        } else {
            (self.answer)(url)
        };
        if doc.is_null() {
            return Err(TransportError::new("The server answered with error 404."));
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
        Err(TransportError::new("no POST on the NWS API"))
    }
}

#[test]
fn test_station_walk_skips_a_dead_nearest_station() {
    // The nearest NWS station is not always a live one. A station sitting on
    // a days-old observation pinned a route cell to simulated fallback for a
    // whole session (2026-08-12 manual playtest): the resolver trusted index
    // zero forever, and first contact had no previous conditions to hold. The
    // fetch now walks to the next-nearest fresh station and remembers it.
    let now = 1_700_000_000.0;
    let transport = Arc::new(StationTransport {
        answer: Box::new(move |url| {
            if url.contains("st0") {
                nws_obs("Mostly Cloudy", 3.0 * 24.0 * 3600.0, now) // parked station
            } else {
                nws_obs("Rain", 20.0 * 60.0, now) // fresh, 20 minutes old
            }
        }),
        calls: StdMutex::new(Vec::new()),
    });
    let fetcher = NwsFetcher::new(transport.clone()).with_wall_clock(fixed(now));
    let observation = fetcher.default_fetch(41.88, -87.63).unwrap();
    assert_eq!(observation.text, "Rain");
    let observed_at = observation.observed_at.unwrap();
    assert!(now - observed_at < OBSERVATION_MAX_AGE_S);

    // The fresh station is remembered and asked first next time.
    transport.calls.lock().unwrap().clear();
    fetcher.default_fetch(41.88, -87.63).unwrap();
    assert!(transport.calls.lock().unwrap()[0].contains("st1"));
}

#[test]
fn test_station_walk_skips_a_nearest_station_that_answers_404() {
    // Casa Grande, 2026-09-24: the nearest station (KCGZ) answered 404 for
    // its latest observation and the first error ended the walk, so the
    // whole leg drove on simulated weather while the next station reported.
    let now = 1_700_000_000.0;
    let transport = Arc::new(StationTransport {
        answer: Box::new(move |url| {
            if url.contains("st0") {
                Value::Null
            } else {
                nws_obs("Clear", 20.0 * 60.0, now)
            }
        }),
        calls: StdMutex::new(Vec::new()),
    });
    let fetcher = NwsFetcher::new(transport).with_wall_clock(fixed(now));
    assert_eq!(fetcher.default_fetch(32.88, -111.76).unwrap().text, "Clear");

    // Every station failing is still an error, carrying what the last said.
    let dead = Arc::new(StationTransport {
        answer: Box::new(|_| Value::Null),
        calls: StdMutex::new(Vec::new()),
    });
    let error = NwsFetcher::new(dead)
        .with_wall_clock(fixed(now))
        .default_fetch(32.88, -111.76)
        .unwrap_err();
    assert!(error.contains("404"), "{error}");
}

#[test]
fn test_station_walk_returns_freshest_when_all_are_stale() {
    let now = 1_700_000_000.0;
    let transport = Arc::new(StationTransport {
        answer: Box::new(move |url| {
            if url.contains("st0") {
                nws_obs("Fog", 5.0 * 24.0 * 3600.0, now)
            } else {
                nws_obs("Snow", 4.0 * 3600.0, now) // stale too, but fresher
            }
        }),
        calls: StdMutex::new(Vec::new()),
    });
    let fetcher = NwsFetcher::new(transport).with_wall_clock(fixed(now));
    let observation = fetcher.default_fetch(41.88, -87.63).unwrap();
    assert_eq!(observation.text, "Snow"); // the freshest stale one, for the caller's hold logic
}

#[test]
fn the_discovery_chain_is_cached_per_coarse_location() {
    let now = 1_700_000_000.0;
    let transport = Arc::new(StationTransport {
        answer: Box::new(move |_| nws_obs("Clear", 60.0, now)),
        calls: StdMutex::new(Vec::new()),
    });
    let fetcher = NwsFetcher::new(transport.clone()).with_wall_clock(fixed(now));
    fetcher.default_fetch(41.88, -87.63).unwrap();
    fetcher.default_fetch(41.881, -87.632).unwrap(); // rounds to the same key
    let calls = transport.calls.lock().unwrap();
    // points + stations once, then one observation per fetch.
    assert_eq!(calls.iter().filter(|u| u.contains("/points/")).count(), 1);
    assert_eq!(calls.len(), 4);
    assert!(calls[0].ends_with("/points/41.88,-87.63"));
}

#[test]
fn parse_observation_reads_an_nws_document() {
    let doc = nws_obs("Light Rain", 0.0, 1_700_000_000.0);
    let parsed = parse_observation(&doc).unwrap();
    assert_eq!(parsed.text, "Light Rain");
    assert_eq!(parsed.wind_kmh, 10.0);
    assert_eq!(parsed.temperature_c, Some(20.0));
    assert!((parsed.visibility_mi.unwrap() - 10.0).abs() < 0.01);
    assert_eq!(parsed.observed_at, Some(1_700_000_000.0));
    // A Z suffix parses like +00:00.
    assert_eq!(iso_timestamp("2023-11-14T22:13:20Z"), Some(1_700_000_000.0));
    assert!(parse_observation(&json!({})).is_err());
}
