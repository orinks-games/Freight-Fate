//! Real-world weather via the U.S. National Weather Service API
//! (https://api.weather.gov).
//!
//! The NWS API is a free, public government service that needs no API key (it
//! only asks for an identifying `User-Agent`). The provider resolves a city's
//! nearest observation station once, then fetches the latest observation in a
//! background thread and caches it, so the game loop never blocks on the network.
//! When a fetch fails or hasn't landed yet, callers get `None` and the simulated
//! weather carries on -- the game works identically offline.
//!
//! The NWS only covers the United States and its territories, which is exactly the
//! game's map. Each observation's free-text condition (e.g. "Mostly Cloudy",
//! "Light Rain", "Fog") is mapped onto the game's [`WeatherKind`] conditions.
//! Because the NWS reports "Fog/Mist" or "Haze" for anything under about 7 miles
//! of visibility -- ordinary muggy summer air -- the fog mapping is gated on the
//! station's measured visibility, so only genuinely low visibility becomes the
//! game's fog.
//!
//! Port of `freight_fate/sim/real_weather.py`. The network rides the
//! [`HttpTransport`] seam from `real_traffic` (through [`NwsFetcher`], the
//! `_default_fetch` machinery); tests inject a `fetch` closure exactly as the
//! Python tests did, plus the monotonic and wall clocks.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::Instant;

use chrono::{DateTime, Local, NaiveDateTime, TimeZone};
use serde_json::Value;

use super::real_traffic::{lock_unpoisoned, wall_clock, Clock, HttpTransport, TransportError};
use super::real_traffic_parsers::pyval::{chain_str, py_str, to_f64, truthy};
use crate::pyfmt::{py_str_float, round_py_n};

pub use super::weather::{WeatherKind, WeatherProvider};

pub const API_ROOT: &str = "https://api.weather.gov";
/// NWS asks every client to identify itself; a contact URL is recommended.
pub const USER_AGENT: &str = "FreightFate/1.1 (accessible trucking game; https://orinks.net)";
pub const FETCH_TIMEOUT_S: f64 = 8.0;
/// Refresh every 5 min -- about as fast as api.weather.gov's own response cache
/// turns over, and quick enough to catch off-schedule SPECI observations.
pub const CACHE_TTL_S: f64 = 5.0 * 60.0;
/// keep serving cached data this long if refreshes fail
pub const STALE_AFTER_S: f64 = 30.0 * 60.0;
/// NWS stations file routine METAR observations once an hour (off-schedule
/// SPECIs are the exception), and dissemination adds minutes on top, so the
/// newest observation a healthy station offers is 30-60 minutes old for most
/// of every hour. That is what "current conditions" means; rejecting it as
/// stale pushed players onto simulated fallback weather for no reason. Only
/// an observation well past the hourly cycle -- a dead or parked station --
/// is unusable.
pub const OBSERVATION_MAX_AGE_S: f64 = 2.0 * 60.0 * 60.0;
/// An observation older than [`OBSERVATION_MAX_AGE_S`] but within this is
/// served as last-known live weather rather than dropped for simulated
/// weather. On 2026-10-19 every station along a tester's route in South
/// Carolina, Georgia and Alabama sat on the same reading for 40 minutes, its
/// age climbing from 123 to 162 minutes, then all of them updated at once:
/// an NWS observation outage. The game swapped clear skies for simulated
/// thunderstorms and fog while another NWS app kept showing the old
/// reading. A real sky a few hours old is closer to the truth than an
/// invented one, and the player hears it as last-known, with its age on the
/// Weather app. Past this, a reading is too old to stand for now.
pub const OUTAGE_HOLD_MAX_AGE_S: f64 = 6.0 * 60.0 * 60.0;
/// wait before retrying a failed city
pub const RETRY_AFTER_S: f64 = 60.0;
pub const STRONG_WIND_KMH: f64 = 38.0;
/// The game's fog is a sub-half-mile, 40-mph event with fog horns, but NWS
/// stations report "Fog/Mist" (METAR mist) or "Haze" for any visibility under
/// about 7 miles -- conditions that can blanket a whole region for hours on a
/// humid night. Only a measured visibility below this maps to the game's fog;
/// hazier-but-drivable air reads as cloudy instead.
pub const FOG_VISIBILITY_MI: f64 = 2.0;

/// Keyword groups for mapping NWS condition text onto game weather. Checked in
/// priority order: the first group whose keyword appears in the text wins, so
/// precipitation and storms beat plain cloud cover. NWS phrases are title-cased
/// (e.g. "Chance Light Rain", "Patchy Fog"); matching is case-insensitive.
const CONDITION_RULES: &[(WeatherKind, &[&str])] = &[
    (
        WeatherKind::Thunderstorm,
        &["thunder", "t-storm", "tstorm", "squall"],
    ),
    // Glaze conditions before snow and rain: "Freezing Rain" must land on ice,
    // not match the plain rain group below.
    (
        WeatherKind::Ice,
        &["freezing", "sleet", "ice", "icy", "glaze"],
    ),
    (
        WeatherKind::Snow,
        &["snow", "flurr", "blizzard", "wintry", "frost"],
    ),
    (WeatherKind::HeavyRain, &["heavy rain", "heavy shower"]),
    (WeatherKind::Rain, &["rain", "shower", "drizzle", "spray"]),
    (
        WeatherKind::Fog,
        &["fog", "mist", "haze", "smoke", "ash", "dust", "sand"],
    ),
    (WeatherKind::Wind, &["wind", "breez", "blust", "gale"]),
    (WeatherKind::Cloudy, &["cloud", "overcast"]),
    (WeatherKind::Clear, &["clear", "sunny", "fair", "sun"]),
];

/// Map an NWS condition phrase (plus wind and visibility) to a game condition.
///
/// Unrecognized or empty text falls back to cloudy -- a safe neutral that the
/// next fetch can refine. A fog-family phrase only becomes fog when the
/// station's measured visibility is genuinely low (below
/// [`FOG_VISIBILITY_MI`]); with no measurement the text is trusted as-is.
/// Strong wind promotes an otherwise clear or cloudy sky to high winds, but
/// never overrides precipitation.
pub fn map_condition(text: &str, wind_kmh: f64, visibility_mi: Option<f64>) -> WeatherKind {
    let lowered = text.to_lowercase();
    let mut kind = WeatherKind::Cloudy;
    for (candidate, keywords) in CONDITION_RULES {
        if keywords.iter().any(|word| lowered.contains(word)) {
            kind = *candidate;
            break;
        }
    }
    if kind == WeatherKind::Fog && visibility_mi.is_some_and(|v| v >= FOG_VISIBILITY_MI) {
        kind = WeatherKind::Cloudy;
    }
    if matches!(kind, WeatherKind::Clear | WeatherKind::Cloudy) && wind_kmh >= STRONG_WIND_KMH {
        return WeatherKind::Wind;
    }
    kind
}

/// What a fetch hands back: the Python `(text, wind_kmh, temp_c,
/// visibility_mi[, observed_at])` tuple. `observed_at` is `None` when the
/// fetch carried no timestamp (the worker then treats it as now).
#[derive(Debug, Clone, PartialEq)]
pub struct Observation {
    pub text: String,
    pub wind_kmh: f64,
    pub temperature_c: Option<f64>,
    pub visibility_mi: Option<f64>,
    pub observed_at: Option<f64>,
}

impl Observation {
    /// The four-tuple form (no observation time).
    pub fn new(
        text: &str,
        wind_kmh: f64,
        temperature_c: Option<f64>,
        visibility_mi: Option<f64>,
    ) -> Self {
        Self {
            text: text.to_string(),
            wind_kmh,
            temperature_c,
            visibility_mi,
            observed_at: None,
        }
    }

    /// The five-tuple form, with the station's observation time.
    pub fn observed_at(mut self, observed_at: f64) -> Self {
        self.observed_at = Some(observed_at);
        self
    }
}

/// `fetch(lat, lon)`: the injectable fetch; `Err` is any exception the
/// Python fetch raised (network failure, a bad document).
pub type FetchFn = Arc<dyn Fn(f64, f64) -> Result<Observation, String> + Send + Sync>;

/// `float(measurement["value"])` for an NWS `{value, unitCode}` object, or
/// `None` for a null/missing value. A value that will not convert is the
/// Python `ValueError`.
fn measurement(value: Option<&Value>) -> Result<Option<(f64, String)>, String> {
    let Some(obj) = value.filter(|v| truthy(v)).and_then(Value::as_object) else {
        return Ok(None);
    };
    let Some(raw) = obj.get("value").filter(|v| !v.is_null()) else {
        return Ok(None);
    };
    let number = to_f64(raw).ok_or_else(|| format!("could not convert {raw} to float"))?;
    Ok(Some((number, chain_str(obj, &["unitCode"], ""))))
}

/// Convert an NWS windSpeed measurement to km/h, tolerating null values.
pub fn wind_to_kmh(wind: Option<&Value>) -> Result<f64, String> {
    let Some((value, unit)) = measurement(wind)? else {
        return Ok(0.0);
    };
    if unit.contains("m_s") || unit.contains("m/s") {
        // metres per second
        return Ok(value * 3.6);
    }
    if unit.contains("mi_h") || unit.contains("mph") {
        // miles per hour
        return Ok(value * 1.609344);
    }
    Ok(value) // already km/h (wmoUnit:km_h-1)
}

/// Convert an NWS temperature measurement to Celsius, or None when absent.
///
/// NWS observations report Celsius (`wmoUnit:degC`); Fahrenheit is handled
/// defensively in case a station ever reports it. A null value (the station
/// has no current reading) yields None so callers fall back to the model.
pub fn temp_to_c(temp: Option<&Value>) -> Result<Option<f64>, String> {
    let Some((value, unit)) = measurement(temp)? else {
        return Ok(None);
    };
    if unit.contains("degF") {
        return Ok(Some((value - 32.0) * 5.0 / 9.0));
    }
    Ok(Some(value)) // degC (the NWS default)
}

/// Convert an NWS visibility measurement to statute miles, or None when
/// absent -- the fog gate then falls back to trusting the condition text.
pub fn visibility_to_mi(vis: Option<&Value>) -> Result<Option<f64>, String> {
    let Some((value, unit)) = measurement(vis)? else {
        return Ok(None);
    };
    if unit.contains("km") {
        return Ok(Some(value / 1.609344));
    }
    Ok(Some(value / 1609.344)) // metres (wmoUnit:m, the NWS default)
}

/// `datetime.fromisoformat(text.replace("Z", "+00:00")).timestamp()`: an
/// aware ISO stamp to Unix seconds; a naive one is read in local time, as
/// Python does.
fn iso_timestamp(text: &str) -> Option<f64> {
    let text = text.replace('Z', "+00:00");
    if let Ok(stamp) = DateTime::parse_from_rfc3339(&text) {
        return Some(stamp.timestamp() as f64 + f64::from(stamp.timestamp_subsec_nanos()) * 1e-9);
    }
    for layout in [
        "%Y-%m-%dT%H:%M:%S%.f",
        "%Y-%m-%d %H:%M:%S%.f",
        "%Y-%m-%dT%H:%M",
        "%Y-%m-%d",
    ] {
        if let Ok(naive) = NaiveDateTime::parse_from_str(&text, layout) {
            let local = Local.from_local_datetime(&naive).single()?;
            return Some(local.timestamp() as f64);
        }
    }
    None
}

/// Pull the fields the worker needs out of one `/observations/latest` document.
pub fn parse_observation(data: &Value) -> Result<Observation, String> {
    let props = data
        .get("properties")
        .and_then(Value::as_object)
        .ok_or("observation has no properties")?;
    let text = props
        .get("textDescription")
        .filter(|v| truthy(v))
        .map(py_str)
        .unwrap_or_default();
    let wind_kmh = wind_to_kmh(props.get("windSpeed"))?;
    let temp_c = temp_to_c(props.get("temperature"))?;
    let visibility_mi = visibility_to_mi(props.get("visibility"))?;
    let mut observed_at = None;
    if let Some(timestamp) = props.get("timestamp").filter(|v| truthy(v)) {
        observed_at = iso_timestamp(&py_str(timestamp));
        if observed_at.is_none() {
            log::warn!(
                "NWS observation carried an invalid timestamp: {:?}",
                py_str(timestamp)
            );
        }
    }
    Ok(Observation {
        text,
        wind_kmh,
        temperature_c: temp_c,
        visibility_mi,
        observed_at,
    })
}

/// Resolving a city's nearby observation stations is stable, so cache the list
/// across refreshes (keyed by coarse coordinates) to avoid repeating the two
/// lookups. The pick records which of them last answered with a FRESH
/// observation: the nearest station is not always a live one -- a dead or
/// parked station pinned one I-90 cell to simulated fallback for a whole
/// session (2026-08-12 manual playtest) -- so fetches walk past stale stations
/// instead of trusting index zero forever.
pub const STATION_WALK_LIMIT: usize = 3;

/// At most this many NWS fetches run at once, across every provider in
/// the process. Route planning asks for every city on every offered route
/// at once, one worker thread each, and each fetch is up to five requests:
/// dozens of simultaneous calls from one client, the burst api.weather.gov
/// throttles. The rest wait their turn; the game loop never waits.
pub const MAX_CONCURRENT_FETCHES: usize = 3;

static FETCH_SLOTS: (Mutex<usize>, Condvar) = (Mutex::new(0), Condvar::new());

/// One of the [`MAX_CONCURRENT_FETCHES`] slots, held for a whole fetch and
/// handed back on drop.
struct FetchSlot;

impl FetchSlot {
    fn acquire() -> Self {
        let (count, freed) = &FETCH_SLOTS;
        let mut busy = lock_unpoisoned(count);
        while *busy >= MAX_CONCURRENT_FETCHES {
            busy = freed.wait(busy).unwrap_or_else(|p| p.into_inner());
        }
        *busy += 1;
        FetchSlot
    }

    /// Fetches in flight right now (a test aid).
    #[cfg(test)]
    fn in_use() -> usize {
        *lock_unpoisoned(&FETCH_SLOTS.0)
    }
}

impl Drop for FetchSlot {
    fn drop(&mut self) {
        let (count, freed) = &FETCH_SLOTS;
        let mut busy = lock_unpoisoned(count);
        *busy = busy.saturating_sub(1);
        freed.notify_one();
    }
}

/// The `_default_fetch` machinery: NWS discovery chain, station cache and
/// the fresh-station pick, over an injected transport. (The Python kept the
/// two caches module-global; here they live on the fetcher, which the game
/// holds once.)
pub struct NwsFetcher {
    transport: Arc<dyn HttpTransport>,
    wall_clock: Clock,
    station_cache: Mutex<HashMap<String, Vec<String>>>,
    station_pick: Mutex<HashMap<String, usize>>,
}

impl NwsFetcher {
    pub fn new(transport: Arc<dyn HttpTransport>) -> Self {
        Self {
            transport,
            wall_clock: wall_clock(),
            station_cache: Mutex::new(HashMap::new()),
            station_pick: Mutex::new(HashMap::new()),
        }
    }

    /// Replace the wall clock the freshness walk reads (`time.time()`).
    pub fn with_wall_clock(mut self, clock: Clock) -> Self {
        self.wall_clock = clock;
        self
    }

    /// Wrap the fetcher as the provider's `fetch` callable.
    pub fn fetch_fn(self: Arc<Self>) -> FetchFn {
        Arc::new(move |lat, lon| self.default_fetch(lat, lon))
    }

    /// Fetch and decode a JSON document from the NWS API. Errors on failure.
    pub fn get_json(&self, url: &str) -> Result<Value, TransportError> {
        self.transport.get_json(
            url,
            &[
                ("User-Agent", USER_AGENT),
                ("Accept", "application/geo+json"),
            ],
            FETCH_TIMEOUT_S,
        )
    }

    /// `(round(lat, 2), round(lon, 2))` as the cache key.
    fn coarse_key(lat: f64, lon: f64) -> String {
        format!(
            "{},{}",
            py_str_float(round_py_n(lat, 2)),
            py_str_float(round_py_n(lon, 2))
        )
    }

    /// The `/points/{lat},{lon}` path NWS serves without a redirect: four
    /// decimals at most and no trailing zeros. `41.8800,-87.6300` answers
    /// 301 to `41.88,-87.63`, an extra round trip inside the same request
    /// deadline on every first lookup.
    pub fn point_path(lat: f64, lon: f64) -> String {
        format!(
            "{},{}",
            py_str_float(round_py_n(lat, 4)),
            py_str_float(round_py_n(lon, 4))
        )
    }

    /// Return 'latest observation' URLs for the stations nearest a point,
    /// nearest first, capped at [`STATION_WALK_LIMIT`].
    ///
    /// Walks the NWS discovery chain: `/points` yields the station list URL, and
    /// that list yields the stations. The result is cached per location.
    pub fn resolve_station_urls(&self, lat: f64, lon: f64) -> Result<Vec<String>, String> {
        let key = Self::coarse_key(lat, lon);
        if let Some(cached) = lock_unpoisoned(&self.station_cache).get(&key) {
            return Ok(cached.clone());
        }
        let point = self
            .get_json(&format!("{API_ROOT}/points/{}", Self::point_path(lat, lon)))
            .map_err(|e| format!("points lookup: {}", e.message))?;
        let stations_url = point
            .pointer("/properties/observationStations")
            .filter(|v| !v.is_null())
            .map(py_str)
            .ok_or("point has no observationStations")?;
        let stations = self
            .get_json(&stations_url)
            .map_err(|e| format!("station list: {}", e.message))?;
        let station_urls: Vec<String> = stations
            .get("observationStations")
            .and_then(Value::as_array)
            .map(|urls| urls.iter().map(py_str).collect())
            .unwrap_or_default();
        if station_urls.is_empty() {
            return Err(format!("no observation stations near {lat:.4},{lon:.4}"));
        }
        let urls: Vec<String> = station_urls
            .iter()
            .take(STATION_WALK_LIMIT)
            .map(|u| format!("{u}/observations/latest"))
            .collect();
        lock_unpoisoned(&self.station_cache).insert(key, urls.clone());
        Ok(urls)
    }

    /// Fetch (condition, wind, temperature, visibility, observation time)
    /// from NWS, from the nearest station with a FRESH observation.
    ///
    /// Stations die and park: the nearest one can sit on a reading days old
    /// while the next one over reports on the hour. The walk tries the station
    /// that last answered fresh first, then the others in distance order, and
    /// returns the first fresh observation. If every station within the walk
    /// limit is stale, the freshest of them is returned and the caller's stale
    /// handling takes over. Temperature and visibility are None when the
    /// station reports no current value. A station that fails outright is
    /// walked past too: NWS answers 404 for a station with no current
    /// observation (Casa Grande's KCGZ, 2026-09-24, while KA39 and KCHD next
    /// door reported), and one such station used to end the whole walk.
    /// Errors only when no station in the walk answered.
    pub fn default_fetch(&self, lat: f64, lon: f64) -> Result<Observation, String> {
        let _slot = FetchSlot::acquire();
        let key = Self::coarse_key(lat, lon);
        let urls = self.resolve_station_urls(lat, lon)?;
        let preferred = lock_unpoisoned(&self.station_pick)
            .get(&key)
            .copied()
            .unwrap_or(0);
        let order: Vec<usize> = std::iter::once(preferred)
            .chain((0..urls.len()).filter(|i| *i != preferred))
            .collect();
        let now = (self.wall_clock)();
        let mut freshest: Option<(f64, Observation)> = None;
        let mut errors: Vec<String> = Vec::new();
        for i in order {
            let Some(url) = urls.get(i) else {
                continue;
            };
            let fetched = self.get_json(url).map_err(|e| e.message);
            let parsed = match fetched.and_then(|doc| parse_observation(&doc)) {
                Ok(parsed) => parsed,
                Err(error) => {
                    errors.push(format!("{}: {error}", station_name(url)));
                    continue;
                }
            };
            // No timestamp reads as current: the worker treats it as now.
            let age_s = parsed.observed_at.map(|observed| now - observed);
            if age_s.is_none_or(|age| age <= OBSERVATION_MAX_AGE_S) {
                lock_unpoisoned(&self.station_pick).insert(key, i);
                return Ok(parsed);
            }
            let age = age_s.unwrap_or(0.0);
            if freshest.as_ref().is_none_or(|(best, _)| age < *best) {
                freshest = Some((age, parsed));
            }
        }
        freshest.map(|(_, parsed)| parsed).ok_or_else(|| {
            if errors.is_empty() {
                "no station answered".to_string()
            } else {
                format!("every station failed: {}", errors.join("; "))
            }
        })
    }
}

/// The station id out of an `/observations/latest` URL, for the log
/// (`.../stations/KDEN/observations/latest` is `KDEN`).
fn station_name(url: &str) -> &str {
    url.trim_end_matches("/observations/latest")
        .rsplit('/')
        .next()
        .unwrap_or(url)
}

#[derive(Debug, Clone, PartialEq)]
struct CachedObservation {
    kind: WeatherKind,
    temperature_c: Option<f64>,
    fetched_at: f64,
    observed_at: f64,
}

#[derive(Default)]
struct Inner {
    // Observations belong to stations, not to request keys: the city menu
    // warms "city:denver", the trip then asks for "route:denver:x:0", and
    // both are the same place. Observations are cached per station
    // identity (the same coordinate rounding the station-URL cache uses)
    // with request keys as aliases, so same-place keys share one fetch.
    obs_by_station: HashMap<String, CachedObservation>,
    station_for_key: HashMap<String, String>,
    failed_at: HashMap<String, f64>,
    inflight: HashSet<String>,
    // Route segments currently in a stale-observation stretch, so the
    // miss is logged once when the stretch starts rather than on every
    // RETRY_AFTER_S retry until the station catches up.
    stale_logged: HashSet<String>,
    // The last failure logged per request key, so a stretch of identical
    // failures writes one warning and a changed reason writes another,
    // instead of one every RETRY_AFTER_S.
    error_logged: HashMap<String, String>,
}

/// How one worker run ended. The Python `_worker` returns nothing; this
/// lets tests count the "logged once per stretch" behaviour without a log
/// capture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkerOutcome {
    /// A fresh observation landed in the cache.
    Cached,
    /// NWS answered with an observation past [`OBSERVATION_MAX_AGE_S`] but
    /// within [`OUTAGE_HOLD_MAX_AGE_S`]; it serves as last-known live
    /// weather.
    Held,
    /// NWS answered with an observation older than the cutoff; this run
    /// wrote the once-per-stretch note.
    StaleLogged,
    /// Same, but the stretch was already noted.
    StaleSilent,
    /// The fetch failed (network, bad document); this run logged why.
    Failed,
    /// Failed again for the reason already logged this stretch.
    FailedSilent,
}

/// Everything a worker needs, cloneable onto a thread.
#[derive(Clone)]
struct WorkerContext {
    inner: Arc<Mutex<Inner>>,
    fetch: FetchFn,
    clock: Clock,
    wall_clock: Clock,
}

/// Cached, non-blocking source of real current weather per city.
///
/// `request(city, lat, lon)` kicks off a background fetch if needed;
/// `get(city)` returns the last known [`WeatherKind`] or `None`.
/// A custom `fetch` callable is injected for tests (and the NWS one for the
/// game, see [`NwsFetcher::fetch_fn`]).
pub struct RealWeatherProvider {
    ctx: WorkerContext,
    threaded: bool,
    workers: Mutex<Vec<JoinHandle<()>>>,
}

/// `time.monotonic()`: seconds since an arbitrary origin, never going back.
pub fn monotonic_clock() -> Clock {
    let start = Instant::now();
    Arc::new(move || start.elapsed().as_secs_f64())
}

impl RealWeatherProvider {
    /// A provider over `fetch`, with the monotonic and wall clocks and
    /// background threads (the Python defaults).
    pub fn new(fetch: FetchFn) -> Self {
        Self {
            ctx: WorkerContext {
                inner: Arc::new(Mutex::new(Inner::default())),
                fetch,
                clock: monotonic_clock(),
                wall_clock: wall_clock(),
            },
            threaded: true,
            workers: Mutex::new(Vec::new()),
        }
    }

    /// The game's provider: the NWS fetch over the given transport.
    pub fn with_nws(transport: Arc<dyn HttpTransport>) -> Self {
        Self::new(Arc::new(NwsFetcher::new(transport)).fetch_fn())
    }

    /// Replace the refresh-cadence clock (`time.monotonic()`).
    pub fn with_clock(mut self, clock: Clock) -> Self {
        self.ctx.clock = clock;
        self
    }

    /// Replace the observation-age clock (`time.time()`).
    pub fn with_wall_clock(mut self, clock: Clock) -> Self {
        self.ctx.wall_clock = clock;
        self
    }

    /// Run workers on a `std::thread` (true, the Python shape) or inline on
    /// the calling thread (false -- the tests' `SyncProvider`).
    pub fn with_threaded(mut self, threaded: bool) -> Self {
        self.threaded = threaded;
        self
    }

    /// Wait for every worker spawned so far (a test aid).
    pub fn join_background(&self) {
        let handles: Vec<JoinHandle<()>> = lock_unpoisoned(&self.workers).drain(..).collect();
        for handle in handles {
            let _ = handle.join();
        }
    }

    pub fn station_identity(lat: f64, lon: f64) -> String {
        format!(
            "{},{}",
            py_str_float(round_py_n(lat, 2)),
            py_str_float(round_py_n(lon, 2))
        )
    }

    fn usable(ctx: &WorkerContext, entry: &CachedObservation) -> bool {
        (ctx.clock)() - entry.fetched_at <= STALE_AFTER_S
            && (ctx.wall_clock)() - entry.observed_at <= OUTAGE_HOLD_MAX_AGE_S
    }

    /// The aliased station observation for a request key. Caller holds
    /// the lock.
    fn entry_for<'a>(inner: &'a Inner, city: &str) -> Option<&'a CachedObservation> {
        let station = inner.station_for_key.get(city)?;
        inner.obs_by_station.get(station)
    }

    pub fn get(&self, city: &str) -> Option<WeatherKind> {
        let inner = lock_unpoisoned(&self.ctx.inner);
        let entry = Self::entry_for(&inner, city)?;
        if !Self::usable(&self.ctx, entry) {
            return None;
        }
        Some(entry.kind)
    }

    /// The last real observed temperature in Celsius, or None when there is
    /// no fresh reading -- still loading, offline, too stale, or the station
    /// omitted it. Callers fall back to the seasonal model on None.
    pub fn get_temperature(&self, city: &str) -> Option<f64> {
        let inner = lock_unpoisoned(&self.ctx.inner);
        let entry = Self::entry_for(&inner, city)?;
        if !Self::usable(&self.ctx, entry) {
            return None;
        }
        entry.temperature_c
    }

    /// Whether usable cached data reads as last-known: due for a network
    /// refresh, or an observation held through an outage.
    ///
    /// Refresh cadence follows when the response was fetched. An ordinary
    /// observation's age (up to [`OBSERVATION_MAX_AGE_S`]) does not make a
    /// freshly fetched, still-usable response last-known; one past it,
    /// served under [`OUTAGE_HOLD_MAX_AGE_S`], always does.
    pub fn stale(&self, city: &str) -> bool {
        let inner = lock_unpoisoned(&self.ctx.inner);
        Self::entry_for(&inner, city).is_some_and(|entry| {
            Self::usable(&self.ctx, entry)
                && ((self.ctx.clock)() - entry.fetched_at >= CACHE_TTL_S
                    || (self.ctx.wall_clock)() - entry.observed_at > OBSERVATION_MAX_AGE_S)
        })
    }

    /// Whether any live observation has been seen this session.
    ///
    /// The weather layer uses this to tell "the network is having a moment"
    /// (hold last-known conditions) apart from "this session has never been
    /// online" (the only case where simulated fallback is honest).
    pub fn has_any_observation(&self) -> bool {
        !lock_unpoisoned(&self.ctx.inner).obs_by_station.is_empty()
    }

    /// Whether a network request for this location is actually in flight.
    pub fn refreshing(&self, city: &str) -> bool {
        lock_unpoisoned(&self.ctx.inner).inflight.contains(city)
    }

    /// Age of the cached station observation, separate from fetch activity.
    pub fn observation_age_s(&self, city: &str) -> Option<f64> {
        let inner = lock_unpoisoned(&self.ctx.inner);
        let entry = Self::entry_for(&inner, city)?;
        Some(((self.ctx.wall_clock)() - entry.observed_at).max(0.0))
    }

    /// Whether the most recent completed refresh attempt failed.
    pub fn refresh_failed(&self, city: &str) -> bool {
        let inner = lock_unpoisoned(&self.ctx.inner);
        !inner.inflight.contains(city) && inner.failed_at.contains_key(city)
    }

    /// True when live data is not usable *and* a fetch has failed.
    ///
    /// Lets callers tell a still-loading first fetch (hold steady, no warm-up
    /// flicker) apart from a genuine offline state (fall back to simulated
    /// weather). False while data is cached or the first request is in
    /// flight. A retry after a failure stays unavailable while it runs: the
    /// player is still on simulated weather, and reading the retry as
    /// "loading" flipped the source every minute, re-announcing the fallback
    /// after each failed retry (agent drive, 2026-09-24).
    pub fn unavailable(&self, city: &str) -> bool {
        let inner = lock_unpoisoned(&self.ctx.inner);
        if Self::entry_for(&inner, city).is_some_and(|entry| Self::usable(&self.ctx, entry)) {
            return false;
        }
        inner.failed_at.contains_key(city)
    }

    /// Ensure fresh data for `city` is available or being fetched.
    pub fn request(&self, city: &str, lat: f64, lon: f64) {
        let now = (self.ctx.clock)();
        let station = Self::station_identity(lat, lon);
        {
            let mut inner = lock_unpoisoned(&self.ctx.inner);
            if inner.inflight.contains(city) {
                return;
            }
            if let Some(entry) = inner.obs_by_station.get(&station) {
                if Self::usable(&self.ctx, entry) && now - entry.fetched_at < CACHE_TTL_S {
                    // Another key already fetched this place: alias and serve it.
                    inner
                        .station_for_key
                        .insert(city.to_string(), station.clone());
                    inner.failed_at.remove(city);
                    return;
                }
            }
            if let Some(failed) = inner.failed_at.get(city) {
                if now - failed < RETRY_AFTER_S {
                    return;
                }
            }
            inner.inflight.insert(city.to_string());
        }
        let ctx = self.ctx.clone();
        let city = city.to_string();
        if self.threaded {
            let handle = std::thread::Builder::new()
                .name(format!("weather-{city}"))
                .spawn(move || {
                    run_worker(&ctx, &city, lat, lon);
                })
                .expect("spawn weather worker");
            lock_unpoisoned(&self.workers).push(handle);
        } else {
            run_worker(&ctx, &city, lat, lon);
        }
    }

    /// The worker body, runnable synchronously (the Python `_worker`).
    pub fn worker(&self, city: &str, lat: f64, lon: f64) -> WorkerOutcome {
        run_worker(&self.ctx, city, lat, lon)
    }
}

/// The duck-typed surface `WeatherSystem` probes, over the shared state.
impl WeatherProvider for RealWeatherProvider {
    fn request(&mut self, city: &str, lat: f64, lon: f64) {
        RealWeatherProvider::request(self, city, lat, lon)
    }

    fn get(&mut self, city: &str) -> Option<WeatherKind> {
        RealWeatherProvider::get(self, city)
    }

    fn get_temperature(&mut self, city: &str) -> Option<f64> {
        RealWeatherProvider::get_temperature(self, city)
    }

    fn unavailable(&mut self, city: &str) -> bool {
        RealWeatherProvider::unavailable(self, city)
    }

    fn stale(&mut self, city: &str) -> bool {
        RealWeatherProvider::stale(self, city)
    }

    fn refreshing(&mut self, city: &str) -> bool {
        RealWeatherProvider::refreshing(self, city)
    }

    fn refresh_failed(&mut self, city: &str) -> bool {
        RealWeatherProvider::refresh_failed(self, city)
    }

    fn observation_age_s(&mut self, city: &str) -> Option<f64> {
        RealWeatherProvider::observation_age_s(self, city)
    }
}

fn run_worker(ctx: &WorkerContext, city: &str, lat: f64, lon: f64) -> WorkerOutcome {
    let station = RealWeatherProvider::station_identity(lat, lon);
    let outcome = match (ctx.fetch)(lat, lon) {
        Ok(fetched) => {
            let observed_at = fetched.observed_at.unwrap_or_else(|| (ctx.wall_clock)());
            let age_s = (ctx.wall_clock)() - observed_at;
            if age_s > OUTAGE_HOLD_MAX_AGE_S {
                // NWS answered fine, but the newest observation the station
                // has on offer is older than OUTAGE_HOLD_MAX_AGE_S. This is
                // an expected, routine condition for a dead or parked
                // station -- not a fetch failure -- so it is handled in its
                // own clause, ahead of the generic error handler, and never
                // surfaces as a warning-with-traceback. Fall back like any
                // other miss (any previously cached conditions keep serving
                // for up to STALE_AFTER_S) but treat this as routine, not an
                // error: no traceback, and only one log line per stretch of
                // staleness -- not one every RETRY_AFTER_S until the station
                // catches up.
                let already_logged = {
                    let mut inner = lock_unpoisoned(&ctx.inner);
                    inner.failed_at.insert(city.to_string(), (ctx.clock)());
                    inner.error_logged.remove(city);
                    let already = inner.stale_logged.contains(city);
                    inner.stale_logged.insert(city.to_string());
                    already
                };
                if already_logged {
                    WorkerOutcome::StaleSilent
                } else {
                    let age_min = age_s / 60.0;
                    let limit_min = OUTAGE_HOLD_MAX_AGE_S / 60.0;
                    log::info!(
                        "Real weather for {city}: newest station observation is {age_min:.0} min \
                         old (limit {limit_min:.0} min) -- holding previous conditions until a \
                         newer reading arrives"
                    );
                    WorkerOutcome::StaleLogged
                }
            } else {
                // Past OBSERVATION_MAX_AGE_S (and within the outage hold):
                // served as last-known, noted once per stretch.
                let held = age_s > OBSERVATION_MAX_AGE_S;
                let kind = map_condition(&fetched.text, fetched.wind_kmh, fetched.visibility_mi);
                let already_noted = {
                    let mut inner = lock_unpoisoned(&ctx.inner);
                    // A held reading never replaces a newer one already
                    // cached for the same place.
                    let newer_cached = inner.obs_by_station.get(&station).is_some_and(|entry| {
                        entry.observed_at >= observed_at && RealWeatherProvider::usable(ctx, entry)
                    });
                    if !(held && newer_cached) {
                        inner.obs_by_station.insert(
                            station.clone(),
                            CachedObservation {
                                kind,
                                temperature_c: fetched.temperature_c,
                                fetched_at: (ctx.clock)(),
                                observed_at,
                            },
                        );
                    }
                    inner.station_for_key.insert(city.to_string(), station);
                    inner.failed_at.remove(city);
                    inner.error_logged.remove(city);
                    if held {
                        !inner.stale_logged.insert(city.to_string())
                    } else {
                        inner.stale_logged.remove(city);
                        false
                    }
                };
                if held {
                    if !already_noted {
                        let age_min = age_s / 60.0;
                        log::info!(
                            "Real weather for {city}: newest station observation is {age_min:.0} \
                             min old -- serving it as last-known live weather (NWS {:?})",
                            fetched.text
                        );
                    }
                    return finish(ctx, city, WorkerOutcome::Held);
                }
                log::info!(
                    "Real weather for {city}: {} (NWS {:?}, wind {:.0} km/h, temp {}, vis {})",
                    kind.value(),
                    fetched.text,
                    fetched.wind_kmh,
                    fetched
                        .temperature_c
                        .map(|t| format!("{t:.0}C"))
                        .unwrap_or_else(|| "n/a".to_string()),
                    fetched
                        .visibility_mi
                        .map(|v| format!("{v:.1}mi"))
                        .unwrap_or_else(|| "n/a".to_string()),
                );
                WorkerOutcome::Cached
            }
        }
        Err(err) => {
            let repeat = {
                let mut inner = lock_unpoisoned(&ctx.inner);
                inner.failed_at.insert(city.to_string(), (ctx.clock)());
                let repeat = inner.error_logged.get(city) == Some(&err);
                inner.error_logged.insert(city.to_string(), err.clone());
                repeat
            };
            if repeat {
                WorkerOutcome::FailedSilent
            } else {
                log::warn!("Real weather fetch failed for {city}: {err}");
                WorkerOutcome::Failed
            }
        }
    };
    finish(ctx, city, outcome)
}

/// End a worker run: the key is no longer in flight.
fn finish(ctx: &WorkerContext, city: &str, outcome: WorkerOutcome) -> WorkerOutcome {
    lock_unpoisoned(&ctx.inner).inflight.remove(city);
    outcome
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_diagnostics;
