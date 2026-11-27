//! Active National Weather Service warnings along a route, read keyless from
//! the same api.weather.gov the real weather already comes from, and their
//! Environment Canada counterparts for Canadian places
//! (`real_weather_alerts_canada`), the place's country picking the feed.
//!
//! The observations feed says what the sky is doing over a station right
//! now. The alerts feed says what the Weather Service has WARNED about for a
//! zone: a blizzard, an ice storm, a tornado, high wind, a flash flood, dense
//! fog. Dispatch reads it the way a dispatcher reads the morning briefing,
//! and the cab reads it the way a driver hears the weather radio:
//!
//! - a warning a dispatcher will not send a truck into while another way
//!   exists (`Avoid`) sorts that route last, the same as a closed road;
//! - a warning that slows a truck (`Slow`) costs the alerted stretch at the
//!   weather model's own safe speed for that condition, derived, not guessed;
//! - a warning worth knowing but not planning around (`Brief`) is spoken and
//!   nothing more. Thunderstorms move through in an hour; nobody reroutes a
//!   load for one.
//!
//! What counts as alerted road is the one assumption here: an alert found at
//! a route city is taken to cover up to [`ALERT_REACH_MI`] of road either
//! side of it, because NWS forecast zones are county-sized and the feed is
//! queried by point. That number is labelled assumed in its doc comment.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use regex::Regex;
use serde_json::Value;

use crate::data::world::World;
use crate::data::world_models::Route;
use crate::sim::real_traffic::{lock_unpoisoned, wall_clock, Clock, HttpTransport, NoTransport};
use crate::sim::real_weather::{monotonic_clock, API_ROOT, FETCH_TIMEOUT_S, USER_AGENT};
use crate::sim::real_weather_alerts_canada::{canada_alerts_url, parse_canada_alerts};
use crate::sim::weather::{effects, WeatherKind};
use crate::sim::weather_service::WeatherService;

/// Alerts change on the hour, not by the minute.
pub const ALERTS_CACHE_TTL_S: f64 = 10.0 * 60.0;
/// A failed point is left alone this long before another try.
pub const ALERTS_RETRY_AFTER_S: f64 = 60.0;
/// How much road either side of a route city an alert found there is taken
/// to cover. ASSUMED: NWS public forecast zones are roughly county-sized,
/// and a county in the interstate states runs 25 to 50 miles across.
pub const ALERT_REACH_MI: f64 = 40.0;
/// The cab asks for alerts at its own position this often.
pub const ALERT_POLL_MI: f64 = 8.0;

/// One active alert as the feed describes it.
#[derive(Debug, Clone, PartialEq)]
pub struct WeatherAlert {
    pub id: String,
    /// The product name: "High Wind Warning", "Winter Storm Warning".
    pub event: String,
    /// "Extreme", "Severe", "Moderate", "Minor", "Unknown".
    pub severity: String,
    pub headline: String,
    /// The counties the alert names.
    pub area: String,
    pub description: String,
}

impl WeatherAlert {
    /// Peak gust the description names, when it names one ("gusts up to 60
    /// mph").
    pub fn gusts_mph(&self) -> Option<i64> {
        let re = Regex::new(r"(?i)gusts?\s+(?:up\s+to|to|of|as high as)\s+(\d{2,3})\s*mph")
            .expect("static");
        re.captures(&self.description)
            .and_then(|caps| caps[1].parse().ok())
    }

    /// Peak gust an Environment Canada description names, in km/h as
    /// written ("gusts up to 90 km/h").
    pub fn gusts_kmh(&self) -> Option<i64> {
        let re = Regex::new(r"(?i)gusts?\s+(?:up\s+to|to|of|as high as)\s+(\d{2,3})\s*km/h")
            .expect("static");
        re.captures(&self.description)
            .and_then(|caps| caps[1].parse().ok())
    }

    /// The cab's line: the product name, plus the gust when the text has
    /// one, in the unit the text gave it.
    pub fn spoken(&self) -> String {
        if !self.event.to_lowercase().contains("wind") {
            return self.event.clone();
        }
        if let Some(gust) = self.gusts_mph() {
            return format!("{}, gusts to {gust} miles per hour", self.event);
        }
        match self.gusts_kmh() {
            Some(gust) => format!("{}, gusts to {gust} kilometers per hour", self.event),
            None => self.event.clone(),
        }
    }
}

/// What a warning means for planning a truck's route.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlertEffect {
    /// Sort a route through it last while another way exists.
    Avoid,
    /// Price the alerted stretch at this condition's safe speed.
    Slow(WeatherKind),
    /// Say it; do not plan around it.
    Brief,
}

/// The planning meaning of an NWS product name, or None when it does not
/// concern a truck on a highway (heat, fire weather, air quality, the
/// marine and beach products).
pub fn alert_effect(event: &str) -> Option<AlertEffect> {
    let e = event.to_lowercase();
    let has = |needle: &str| e.contains(needle);
    // Environment Canada's flash freeze is ice on the road, not the frost
    // and freeze products below.
    if has("flash freeze") {
        return Some(AlertEffect::Slow(WeatherKind::Ice));
    }
    // Not a road product.
    if has("marine")
        || has("small craft")
        || has("surf")
        || has("rip current")
        || has("beach")
        || has("gale")
        || has("heat")
        || has("red flag")
        || has("fire weather")
        || has("air quality")
        || has("frost")
        || has("freeze warning")
        || has("freeze watch")
        || has("hard freeze")
        || has("coastal flood")
        || has("lakeshore")
    {
        return None;
    }
    if has("tornado warning")
        || has("hurricane")
        || has("blizzard")
        || has("ice storm")
        || has("extreme wind")
        || has("typhoon")
    {
        return Some(AlertEffect::Avoid);
    }
    if has("freezing rain") || has("freezing fog") || has("freezing drizzle") {
        return Some(AlertEffect::Slow(WeatherKind::Ice));
    }
    if has("winter storm warning")
        || has("winter weather")
        || has("snow")
        || has("lake effect")
        || has("winter storm")
    {
        return Some(AlertEffect::Slow(WeatherKind::Snow));
    }
    if has("flash flood warning")
        || has("flood warning")
        || has("flood advisory")
        || has("rainfall warning")
    {
        return Some(AlertEffect::Slow(WeatherKind::HeavyRain));
    }
    if has("high wind") || has("wind advisory") || has("wind warning") {
        return Some(AlertEffect::Slow(WeatherKind::Wind));
    }
    if has("dense fog")
        || has("fog advisory")
        || has("dust storm")
        || has("blowing dust")
        || has("dense smoke")
    {
        return Some(AlertEffect::Slow(WeatherKind::Fog));
    }
    if has("thunderstorm")
        || has("tornado watch")
        || has("flood watch")
        || has("tropical storm")
        || has("special weather")
        || has("wind chill")
        || has("extreme cold")
        || has("arctic outflow")
        || has("weather advisory")
    {
        return Some(AlertEffect::Brief);
    }
    None
}

/// The alerts in an api.weather.gov `alerts/active` document that concern
/// a truck, in the feed's order.
pub fn parse_alerts(data: &Value) -> Vec<WeatherAlert> {
    let Some(features) = data.get("features").and_then(Value::as_array) else {
        return Vec::new();
    };
    let text = |props: &Value, key: &str| {
        props
            .get(key)
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string()
    };
    let mut alerts = Vec::new();
    for feature in features {
        let Some(props) = feature.get("properties") else {
            continue;
        };
        let event = text(props, "event");
        if event.is_empty() || alert_effect(&event).is_none() {
            continue;
        }
        let id = {
            let id = text(props, "id");
            if id.is_empty() {
                text(feature, "id")
            } else {
                id
            }
        };
        if id.is_empty() {
            continue;
        }
        alerts.push(WeatherAlert {
            id,
            event,
            severity: text(props, "severity"),
            headline: text(props, "headline"),
            area: text(props, "areaDesc"),
            description: text(props, "description"),
        });
    }
    alerts
}

/// Whether any of the active warnings calls for chains on every truck: a
/// blizzard, ice storm or freezing rain event, never a watch. It only raises
/// a chain law the road already posted (`chain_control::posted_level`); on
/// a bare road no warning posts one.
pub fn alerts_raise_chain_law(alerts: &[WeatherAlert]) -> bool {
    alerts.iter().any(|alert| {
        let e = alert.event.to_lowercase();
        !e.contains("watch")
            && (e.contains("blizzard") || e.contains("ice storm") || e.contains("freezing rain"))
    })
}

/// How wintry the active warnings are: 0 = not winter, 1 = winter storm,
/// winter weather or snow, 2 = blizzard, ice storm or freezing rain. Read
/// as the "is this a winter warning" filter; it posts no chain law, which
/// follows the road (`chain_control`).
pub fn alerts_chain_law_level(alerts: &[WeatherAlert]) -> i64 {
    alerts
        .iter()
        .map(|alert| {
            let e = alert.event.to_lowercase();
            if e.contains("blizzard") || e.contains("ice storm") || e.contains("freezing rain") {
                2
            } else if e.contains("winter storm")
                || e.contains("winter weather")
                || e.contains("snow")
            {
                1
            } else {
                0
            }
        })
        .max()
        .unwrap_or(0)
}

struct CachedAlerts {
    fetched_at: f64,
    alerts: Vec<WeatherAlert>,
}

#[derive(Default)]
struct Inner {
    cache: HashMap<String, CachedAlerts>,
    inflight: HashSet<String>,
    failed_at: HashMap<String, f64>,
}

#[derive(Clone)]
struct Shared {
    inner: Arc<Mutex<Inner>>,
    transport: Arc<dyn HttpTransport>,
    clock: Clock,
    /// Unix seconds, for the end times Environment Canada alerts carry.
    wall_clock: Clock,
}

/// Cached, non-blocking source of active NWS alerts by point.
///
/// `request(lat, lon)` starts a background fetch when the point is not
/// cached or is stale; `get(lat, lon)` answers from the cache and never
/// waits. Points are keyed to a hundredth of a degree, so a city asked for
/// twice is fetched once.
pub struct WeatherAlertsProvider {
    shared: Shared,
    threaded: bool,
    workers: Mutex<Vec<JoinHandle<()>>>,
}

impl WeatherAlertsProvider {
    pub fn new(transport: Arc<dyn HttpTransport>) -> Self {
        Self {
            shared: Shared {
                inner: Arc::new(Mutex::new(Inner::default())),
                transport,
                clock: monotonic_clock(),
                wall_clock: wall_clock(),
            },
            threaded: true,
            workers: Mutex::new(Vec::new()),
        }
    }

    /// No network: every fetch fails and only seeded points answer.
    pub fn offline() -> Self {
        Self::new(Arc::new(NoTransport)).with_threaded(false)
    }

    pub fn with_threaded(mut self, threaded: bool) -> Self {
        self.threaded = threaded;
        self
    }

    pub fn with_clock(mut self, clock: Clock) -> Self {
        self.shared.clock = clock;
        self
    }

    pub fn with_wall_clock(mut self, clock: Clock) -> Self {
        self.shared.wall_clock = clock;
        self
    }

    /// The cache key for a point.
    pub fn point_key(lat: f64, lon: f64) -> String {
        format!("{lat:.2},{lon:.2}")
    }

    /// The cache key for a point and the service that answers there; the
    /// Weather Service keeps the bare key.
    pub fn service_key(service: WeatherService, lat: f64, lon: f64) -> String {
        match service {
            WeatherService::UnitedStates => Self::point_key(lat, lon),
            WeatherService::Canada => format!("ca:{}", Self::point_key(lat, lon)),
        }
    }

    /// The feed URL for a point.
    pub fn point_url(lat: f64, lon: f64) -> String {
        format!("{API_ROOT}/alerts/active?point={lat:.4},{lon:.4}")
    }

    /// Put alerts in the cache for a point, as a fresh fetch would (tests).
    pub fn seed(&self, lat: f64, lon: f64, alerts: Vec<WeatherAlert>) {
        self.seed_from(WeatherService::UnitedStates, lat, lon, alerts);
    }

    /// [`Self::seed`] for a service.
    pub fn seed_from(
        &self,
        service: WeatherService,
        lat: f64,
        lon: f64,
        alerts: Vec<WeatherAlert>,
    ) {
        let now = (self.shared.clock)();
        lock_unpoisoned(&self.shared.inner).cache.insert(
            Self::service_key(service, lat, lon),
            CachedAlerts {
                fetched_at: now,
                alerts,
            },
        );
    }

    /// The truck-relevant alerts cached for a point, or None when the point
    /// has not answered yet.
    pub fn get(&self, lat: f64, lon: f64) -> Option<Vec<WeatherAlert>> {
        self.get_from(WeatherService::UnitedStates, lat, lon)
    }

    /// [`Self::get`] for a service.
    pub fn get_from(
        &self,
        service: WeatherService,
        lat: f64,
        lon: f64,
    ) -> Option<Vec<WeatherAlert>> {
        let inner = lock_unpoisoned(&self.shared.inner);
        inner
            .cache
            .get(&Self::service_key(service, lat, lon))
            .map(|entry| entry.alerts.clone())
    }

    /// Make sure a point's alerts are cached or on their way. Never blocks.
    pub fn request(&self, lat: f64, lon: f64) {
        self.request_from(WeatherService::UnitedStates, lat, lon);
    }

    /// [`Self::request`] from the service the game's data picked.
    pub fn request_from(&self, service: WeatherService, lat: f64, lon: f64) {
        let key = Self::service_key(service, lat, lon);
        let now = (self.shared.clock)();
        {
            let mut inner = lock_unpoisoned(&self.shared.inner);
            if inner.inflight.contains(&key) {
                return;
            }
            if let Some(entry) = inner.cache.get(&key) {
                if now - entry.fetched_at < ALERTS_CACHE_TTL_S {
                    return;
                }
            }
            if let Some(failed) = inner.failed_at.get(&key) {
                if now - failed < ALERTS_RETRY_AFTER_S {
                    return;
                }
            }
            inner.inflight.insert(key.clone());
        }
        let shared = self.shared.clone();
        let name = format!("alerts-{key}");
        let job = move || fetch_point(&shared, service, &key, lat, lon);
        if self.threaded {
            let handle = std::thread::Builder::new()
                .name(name)
                .spawn(job)
                .expect("spawn alerts worker");
            lock_unpoisoned(&self.workers).push(handle);
        } else {
            job();
        }
    }

    /// Wait for every worker spawned so far (a test aid).
    pub fn join_background(&self) {
        let handles: Vec<JoinHandle<()>> = lock_unpoisoned(&self.workers).drain(..).collect();
        for handle in handles {
            let _ = handle.join();
        }
    }
}

fn fetch_point(shared: &Shared, service: WeatherService, key: &str, lat: f64, lon: f64) {
    let (url, label) = match service {
        WeatherService::UnitedStates => (WeatherAlertsProvider::point_url(lat, lon), "NWS"),
        WeatherService::Canada => (canada_alerts_url(lat, lon), "Environment Canada"),
    };
    let result = shared.transport.get_json(
        &url,
        &[
            ("User-Agent", USER_AGENT),
            ("Accept", "application/geo+json"),
        ],
        FETCH_TIMEOUT_S,
    );
    let now = (shared.clock)();
    let mut inner = lock_unpoisoned(&shared.inner);
    inner.inflight.remove(key);
    match result {
        Ok(data) => {
            let alerts = match service {
                WeatherService::UnitedStates => parse_alerts(&data),
                WeatherService::Canada => parse_canada_alerts(&data, (shared.wall_clock)()),
            };
            log::debug!(
                "{label} alerts at {key}: {} that concern a truck",
                alerts.len()
            );
            inner.failed_at.remove(key);
            inner.cache.insert(
                key.to_string(),
                CachedAlerts {
                    fetched_at: now,
                    alerts,
                },
            );
        }
        Err(err) => {
            log::debug!("{label} alerts at {key} failed: {err}");
            inner.failed_at.insert(key.to_string(), now);
        }
    }
}

/// One alert placed on a route: found at a route city, taken to cover up to
/// [`ALERT_REACH_MI`] of road either side of it.
#[derive(Debug, Clone, PartialEq)]
pub struct RouteAlert {
    pub alert: WeatherAlert,
    /// The route city the alert was found at.
    pub near_city: String,
    pub effect: AlertEffect,
    /// Hours the alerted stretch costs against the route's planned pace;
    /// zero for `Brief` and `Avoid`.
    pub delay_h: f64,
    /// Miles of this route taken as alerted.
    pub miles: f64,
}

impl RouteAlert {
    /// "Winter Storm Warning near Laramie", with the cost when it has one.
    pub fn text(&self) -> String {
        let mut text = format!("{} near {}", self.alert.event, self.near_city);
        let minutes = (self.delay_h * 60.0).round();
        if minutes >= 1.0 {
            text.push_str(&format!(", about {} minutes slower", minutes as i64));
        }
        text
    }
}

/// The active warnings along one route option.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RouteWeather {
    pub alerts: Vec<RouteAlert>,
    /// The sum of the `Slow` alerts' delays, in hours.
    pub delay_h: f64,
    /// An `Avoid` warning somewhere on the route.
    pub avoid: bool,
}

impl RouteWeather {
    /// The first warning dispatch would route around, if any.
    pub fn avoided(&self) -> Option<&RouteAlert> {
        self.alerts
            .iter()
            .find(|alert| alert.effect == AlertEffect::Avoid)
    }
}

/// Ask for the alerts at every city on a route, so they are cached when
/// dispatch needs them. Never blocks.
pub fn warm_route_alerts(route: &Route, provider: &WeatherAlertsProvider, world: &World) {
    for key in &route.cities {
        if let Some(city) = world.cities.get(key) {
            if let Some(service) = WeatherService::for_country(&city.country) {
                provider.request_from(service, city.lat, city.lon);
            }
        }
    }
}

/// Read the cached alerts onto a route. `pace_mph` is the pace the deadline
/// model plans the route at; the alerted miles are priced at each
/// condition's safe speed against it.
pub fn scan_route_weather(
    route: &Route,
    provider: &WeatherAlertsProvider,
    world: &World,
    pace_mph: f64,
) -> RouteWeather {
    let mut report = RouteWeather::default();
    let mut seen: HashSet<String> = HashSet::new();
    for (i, key) in route.cities.iter().enumerate() {
        let Some(city) = world.cities.get(key) else {
            continue;
        };
        let Some(service) = WeatherService::for_country(&city.country) else {
            continue;
        };
        provider.request_from(service, city.lat, city.lon);
        let Some(alerts) = provider.get_from(service, city.lat, city.lon) else {
            continue;
        };
        // The road this city's alert is taken to cover: up to the reach on
        // each leg that touches it.
        let mut miles = 0.0;
        if i > 0 {
            miles += route.legs[i - 1].miles.min(ALERT_REACH_MI);
        }
        if i < route.legs.len() {
            miles += route.legs[i].miles.min(ALERT_REACH_MI);
        }
        for alert in alerts {
            if !seen.insert(alert.id.clone()) {
                continue;
            }
            let Some(effect) = alert_effect(&alert.event) else {
                continue;
            };
            let delay_h = match effect {
                AlertEffect::Slow(kind) if pace_mph > 0.0 => {
                    let safe = effects(kind).safe_speed_mph.max(1.0);
                    (miles / safe - miles / pace_mph).max(0.0)
                }
                _ => 0.0,
            };
            report.avoid |= effect == AlertEffect::Avoid;
            report.delay_h += delay_h;
            report.alerts.push(RouteAlert {
                alert,
                near_city: city.name.clone(),
                effect,
                delay_h,
                miles,
            });
        }
    }
    report
}

/// Dispatch's weather sentence for the route it picked: every warning on
/// it, in road order. Empty when there are none.
pub fn weather_brief(weather: &RouteWeather) -> String {
    if weather.alerts.is_empty() {
        return String::new();
    }
    let listed: Vec<String> = weather.alerts.iter().map(RouteAlert::text).collect();
    format!("Weather alerts on the way: {}.", listed.join("; "))
}

/// The note appended to one route option on the route planning screen.
pub fn route_option_weather_note(weather: &RouteWeather) -> String {
    if weather.alerts.is_empty() {
        return String::new();
    }
    let listed: Vec<String> = weather.alerts.iter().map(RouteAlert::text).collect();
    format!("Weather: {}.", listed.join("; "))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::world::get_world;
    use crate::models::jobs::route_drive_hours;

    fn alert(id: &str, event: &str, description: &str) -> WeatherAlert {
        WeatherAlert {
            id: id.to_string(),
            event: event.to_string(),
            severity: "Severe".to_string(),
            headline: format!("{event} issued"),
            area: "Laramie; Albany".to_string(),
            description: description.to_string(),
        }
    }

    #[test]
    fn test_products_map_to_avoid_slow_or_brief_and_the_rest_are_ignored() {
        assert_eq!(alert_effect("Blizzard Warning"), Some(AlertEffect::Avoid));
        assert_eq!(alert_effect("Tornado Warning"), Some(AlertEffect::Avoid));
        assert_eq!(alert_effect("Ice Storm Warning"), Some(AlertEffect::Avoid));
        assert_eq!(alert_effect("Hurricane Warning"), Some(AlertEffect::Avoid));
        assert_eq!(
            alert_effect("Winter Storm Warning"),
            Some(AlertEffect::Slow(WeatherKind::Snow))
        );
        assert_eq!(
            alert_effect("High Wind Warning"),
            Some(AlertEffect::Slow(WeatherKind::Wind))
        );
        assert_eq!(
            alert_effect("Flash Flood Warning"),
            Some(AlertEffect::Slow(WeatherKind::HeavyRain))
        );
        assert_eq!(
            alert_effect("Dense Fog Advisory"),
            Some(AlertEffect::Slow(WeatherKind::Fog))
        );
        assert_eq!(
            alert_effect("Freezing Rain Advisory"),
            Some(AlertEffect::Slow(WeatherKind::Ice))
        );
        assert_eq!(
            alert_effect("Severe Thunderstorm Warning"),
            Some(AlertEffect::Brief)
        );
        assert_eq!(alert_effect("Tornado Watch"), Some(AlertEffect::Brief));
        assert_eq!(alert_effect("Heat Advisory"), None);
        assert_eq!(alert_effect("Red Flag Warning"), None);
        assert_eq!(alert_effect("Small Craft Advisory"), None);
        assert_eq!(alert_effect("Coastal Flood Warning"), None);
        assert_eq!(alert_effect("Air Quality Alert"), None);
    }

    #[test]
    fn test_parse_keeps_truck_alerts_and_drops_the_rest() {
        let doc = serde_json::json!({
            "features": [
                {"id": "urn:1", "properties": {
                    "id": "urn:1", "event": "High Wind Warning", "severity": "Severe",
                    "headline": "High Wind Warning issued", "areaDesc": "Albany; Laramie",
                    "description": "* WHAT...West winds 35 to 45 mph with gusts up to 65 mph."
                }},
                {"id": "urn:2", "properties": {
                    "id": "urn:2", "event": "Heat Advisory", "severity": "Moderate",
                    "headline": "Heat Advisory", "areaDesc": "Sevier", "description": "hot"
                }},
                {"id": "urn:3", "properties": {"event": "", "headline": "blank"}}
            ]
        });
        let alerts = parse_alerts(&doc);
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].event, "High Wind Warning");
        assert_eq!(alerts[0].gusts_mph(), Some(65));
        assert_eq!(
            alerts[0].spoken(),
            "High Wind Warning, gusts to 65 miles per hour"
        );
        assert!(parse_alerts(&serde_json::json!({"features": []})).is_empty());
        assert!(parse_alerts(&Value::Null).is_empty());
    }

    #[test]
    fn test_gusts_are_only_read_for_wind_products() {
        let storm = alert(
            "s",
            "Severe Thunderstorm Warning",
            "gusts up to 60 mph and hail",
        );
        assert_eq!(storm.gusts_mph(), Some(60));
        assert_eq!(storm.spoken(), "Severe Thunderstorm Warning");
        let calm = alert("w", "High Wind Warning", "no numbers here");
        assert_eq!(calm.spoken(), "High Wind Warning");
    }

    #[test]
    fn test_chain_law_level_follows_the_winter_warnings() {
        assert_eq!(alerts_chain_law_level(&[]), 0);
        assert_eq!(
            alerts_chain_law_level(&[alert("a", "Winter Storm Warning", "")]),
            1
        );
        assert_eq!(
            alerts_chain_law_level(&[
                alert("a", "Winter Weather Advisory", ""),
                alert("b", "Blizzard Warning", "")
            ]),
            2
        );
        assert_eq!(
            alerts_chain_law_level(&[alert("a", "High Wind Warning", "")]),
            0
        );
    }

    #[test]
    fn test_only_blizzard_ice_and_freezing_rain_warnings_raise_a_chain_law() {
        assert!(!alerts_raise_chain_law(&[]));
        assert!(!alerts_raise_chain_law(&[alert(
            "a",
            "Winter Storm Warning",
            ""
        )]));
        assert!(!alerts_raise_chain_law(&[alert(
            "a",
            "Winter Storm Watch",
            ""
        )]));
        assert!(!alerts_raise_chain_law(&[alert("a", "Blizzard Watch", "")]));
        assert!(alerts_raise_chain_law(&[alert(
            "a",
            "Blizzard Warning",
            ""
        )]));
        assert!(alerts_raise_chain_law(&[alert(
            "a",
            "Ice Storm Warning",
            ""
        )]));
        assert!(alerts_raise_chain_law(&[alert(
            "a",
            "Freezing Rain Advisory",
            ""
        )]));
    }

    #[test]
    fn test_offline_provider_answers_only_what_was_seeded() {
        let provider = WeatherAlertsProvider::offline();
        assert_eq!(provider.get(41.14, -104.82), None);
        provider.request(41.14, -104.82);
        // The fetch failed inline; nothing is cached and nothing blocks.
        assert_eq!(provider.get(41.14, -104.82), None);
        provider.seed(41.14, -104.82, vec![alert("w", "High Wind Warning", "")]);
        let cached = provider
            .get(41.1401, -104.8199)
            .expect("keyed to a hundredth");
        assert_eq!(cached[0].event, "High Wind Warning");
        assert!(WeatherAlertsProvider::point_url(41.14, -104.82)
            .ends_with("/alerts/active?point=41.1400,-104.8200"));
    }

    #[test]
    fn test_route_weather_prices_a_slow_warning_and_flags_an_avoid_one() {
        // Columbus to Cincinnati on I-71: 106 miles, two cities.
        let world = get_world();
        let routes = world
            .supported_route_options("Columbus", "Cincinnati", 3)
            .expect("routes");
        let route = &routes[0];
        let pace = route.miles() / route_drive_hours(Some(route), 0.0, Some(world));
        let provider = WeatherAlertsProvider::offline();
        let columbus = world.cities.get(&route.cities[0]).expect("Columbus");
        provider.seed(
            columbus.lat,
            columbus.lon,
            vec![
                alert("snow", "Winter Storm Warning", "Heavy snow."),
                alert("storm", "Severe Thunderstorm Warning", ""),
            ],
        );
        let weather = scan_route_weather(route, &provider, world, pace);
        assert_eq!(weather.alerts.len(), 2);
        assert!(!weather.avoid);
        let snow = &weather.alerts[0];
        assert_eq!(snow.effect, AlertEffect::Slow(WeatherKind::Snow));
        // Forty miles of the one leg at the snow safe speed instead of pace.
        assert!((snow.miles - ALERT_REACH_MI).abs() < 1e-9, "{}", snow.miles);
        assert!(snow.delay_h > 0.0, "{}", snow.delay_h);
        assert_eq!(weather.alerts[1].effect, AlertEffect::Brief);
        assert_eq!(weather.alerts[1].delay_h, 0.0);
        let brief = weather_brief(&weather);
        assert!(
            brief.starts_with(
                "Weather alerts on the way: Winter Storm Warning near Columbus, about "
            ),
            "{brief}"
        );
        assert!(
            brief.ends_with("Severe Thunderstorm Warning near Columbus."),
            "{brief}"
        );
        assert!(route_option_weather_note(&weather)
            .starts_with("Weather: Winter Storm Warning near Columbus"));

        let cincinnati = world.cities.get(&route.cities[1]).expect("Cincinnati");
        provider.seed(
            cincinnati.lat,
            cincinnati.lon,
            vec![alert("blizzard", "Blizzard Warning", "")],
        );
        let weather = scan_route_weather(route, &provider, world, pace);
        assert!(weather.avoid);
        assert_eq!(
            weather.avoided().map(|a| a.near_city.as_str()),
            Some("Cincinnati")
        );
        assert_eq!(weather_brief(&RouteWeather::default()), "");
    }
}
