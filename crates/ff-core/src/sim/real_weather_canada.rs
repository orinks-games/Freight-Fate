//! Real-world weather in Canada, from Environment and Climate Change
//! Canada's MSC GeoMet service (<https://api.weather.gc.ca>).
//!
//! Free, public, no key: the same `User-Agent` the National Weather Service
//! calls send is the only courtesy asked. Its licence asks for the credit
//! "Data Source: Environment and Climate Change Canada", which the README
//! carries.
//!
//! The service answers for the 844 city page sites of its
//! `citypageweather-realtime` collection. A point is resolved to the
//! nearest few sites once per session (a box search, widened until it finds
//! some), then each fetch walks them for a fresh reading the way
//! [`NwsFetcher::default_fetch`] walks NWS stations. Many sites have a
//! station that measures temperature and wind but has no present-weather
//! sensor, so no condition: the walk takes the next site that has one, and
//! when none does, the nearest site's own hourly forecast condition for this
//! hour, with that site's measured temperature and wind. The log says which
//! (read or derived); the player hears no difference.
//!
//! Conditions are mapped onto [`WeatherKind`] by [`map_condition_canada`]:
//! a few Environment Canada terms first, then the shared keyword table.
//!
//! [`NwsFetcher::default_fetch`]: super::real_weather::NwsFetcher::default_fetch

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use serde_json::Value;

use super::real_traffic::{haversine_distance, lock_unpoisoned, wall_clock, Clock, HttpTransport};
use super::real_weather::{
    iso_timestamp, map_condition, FetchFn, FetchSlot, Observation, FETCH_TIMEOUT_S,
    OBSERVATION_MAX_AGE_S, STATION_WALK_LIMIT, USER_AGENT,
};
use super::weather::WeatherKind;

pub const CANADA_API_ROOT: &str = "https://api.weather.gc.ca";
/// How far a city page site may be from the point and still stand for it.
/// ASSUMED, the same kind of number as `ALERT_REACH_MI`: a site's station is
/// usually at the town's airport, and past 60 miles the sky can differ. A
/// point with no site this close takes the single nearest one (the Dempster
/// and the Mackenzie have long gaps between towns).
pub const SITE_REACH_MI: f64 = 60.0;
/// The half-widths, in degrees, of the boxes searched for sites, tried in
/// turn until one holds a site.
pub const SITE_SEARCH_BOXES_DEG: [f64; 3] = [0.75, 2.0, 5.0];
/// An hourly forecast entry stands in for the hour only this close to now.
pub const FORECAST_REACH_S: f64 = 90.0 * 60.0;

/// Fog-family terms Environment Canada reports as fog proper. Without a
/// measured visibility the rest of the family (mist, haze, smoke, fog
/// patches, shallow fog, dust, ash, sand) reads as cloudy: in a station
/// report fog means visibility under five eighths of a mile and mist five
/// eighths or more, and the game's fog is a low-visibility event.
const FOG_PROPER: [&str; 2] = ["fog", "ice fog"];

/// Map an Environment Canada condition (plus wind, visibility and the
/// site's temperature) to a game condition. The terms the shared table
/// would get wrong come first; everything else is [`map_condition`].
pub fn map_condition_canada(
    text: &str,
    wind_kmh: f64,
    visibility_mi: Option<f64>,
    temperature_c: Option<f64>,
) -> WeatherKind {
    let lowered = text.trim().to_lowercase();
    let as_if = |phrase: &str| map_condition(phrase, wind_kmh, visibility_mi);
    match lowered.as_str() {
        // Diamond dust on a clear, very cold day; the "ice" keyword would
        // make it freezing rain.
        "ice crystals" => return as_if("Clear"),
        "ice fog" => return as_if("Fog"),
        // The sensor saw precipitation but not its type.
        "precipitation" | "light precipitation" | "heavy precipitation" => {
            let heavy = lowered.starts_with("heavy");
            return match temperature_c {
                Some(t) if t <= 0.0 => WeatherKind::Snow,
                _ if heavy => WeatherKind::HeavyRain,
                _ => WeatherKind::Rain,
            };
        }
        "funnel cloud" | "tornado" | "waterspout" => return WeatherKind::Thunderstorm,
        "hail" => return WeatherKind::Rain,
        _ => {}
    }
    let kind = map_condition(text, wind_kmh, visibility_mi);
    if kind == WeatherKind::Fog
        && visibility_mi.is_none()
        && !FOG_PROPER.contains(&lowered.as_str())
    {
        return as_if("Cloudy");
    }
    kind
}

/// An hourly forecast phrase as the condition it stands for now. A
/// forecast states possibilities an observation does not: a sentence that
/// starts "Risk of" is dropped ("Periods of rain mixed with snow. Risk of
/// freezing rain" is the rain and snow), and a phrase that starts "Chance
/// of" is a cloudy sky ("Chance of flurries" is not snow falling). The
/// result goes through [`map_condition_canada`] like an observation.
pub fn forecast_phrase(text: &str) -> String {
    let kept: Vec<&str> = text
        .split('.')
        .map(str::trim)
        .filter(|sentence| !sentence.is_empty() && !sentence.to_lowercase().starts_with("risk of"))
        .collect();
    let phrase = kept.join(". ");
    if phrase.to_lowercase().starts_with("chance of") {
        "Cloudy".to_string()
    } else {
        phrase
    }
}

/// One city page site: its id and where it is.
#[derive(Debug, Clone, PartialEq)]
pub struct Site {
    pub id: String,
    pub lat: f64,
    pub lon: f64,
}

/// What one site's document says now.
#[derive(Debug, Clone, PartialEq)]
pub struct SiteReading {
    /// The station's observed condition; None where it has no
    /// present-weather sensor.
    pub condition: Option<String>,
    pub temperature_c: Option<f64>,
    pub wind_kmh: f64,
    pub visibility_mi: Option<f64>,
    pub observed_at: Option<f64>,
    /// The hourly forecast: (Unix seconds, condition), soonest first.
    pub hourly: Vec<(f64, String)>,
}

impl SiteReading {
    /// The forecast condition for the hour nearest `now`, when one is
    /// within [`FORECAST_REACH_S`].
    pub fn forecast_condition(&self, now: f64) -> Option<&str> {
        self.hourly
            .iter()
            .filter(|(at, _)| (at - now).abs() <= FORECAST_REACH_S)
            .min_by(|a, b| (a.0 - now).abs().total_cmp(&(b.0 - now).abs()))
            .map(|(_, text)| text.as_str())
    }

    fn observation(&self, text: &str) -> Observation {
        Observation {
            text: text.to_string(),
            wind_kmh: self.wind_kmh,
            temperature_c: self.temperature_c,
            visibility_mi: self.visibility_mi,
            observed_at: self.observed_at,
        }
    }
}

/// The English value of a `{en, fr}` pair, or the value itself when it is
/// a plain string or number.
fn english(value: Option<&Value>) -> Option<&Value> {
    let value = value?;
    let english = value.get("en").unwrap_or(value);
    (!english.is_null()).then_some(english)
}

fn number(value: Option<&Value>) -> Option<f64> {
    match english(value)? {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

/// The sites in a `citypageweather-realtime` box search, in the order the
/// service listed them.
pub fn parse_site_list(doc: &Value) -> Vec<Site> {
    let Some(features) = doc.get("features").and_then(Value::as_array) else {
        return Vec::new();
    };
    features
        .iter()
        .filter_map(|feature| {
            let id = feature
                .pointer("/properties/identifier")
                .or_else(|| feature.get("id"))
                .and_then(Value::as_str)?
                .to_string();
            let coords = feature.pointer("/geometry/coordinates")?.as_array()?;
            let lon = coords.first()?.as_f64()?;
            let lat = coords.get(1)?.as_f64()?;
            Some(Site { id, lat, lon })
        })
        .collect()
}

/// Read one site's document. Errs when the site has no station reading at
/// all (an empty `currentConditions`).
pub fn parse_site(doc: &Value) -> Result<SiteReading, String> {
    let props = doc
        .get("properties")
        .and_then(Value::as_object)
        .ok_or("site has no properties")?;
    let current = props
        .get("currentConditions")
        .and_then(Value::as_object)
        .filter(|c| !c.is_empty())
        .ok_or("site has no current conditions")?;
    let condition = english(current.get("condition"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_string);
    let temperature_c = number(current.get("temperature").and_then(|t| t.get("value")));
    let wind_speed = current
        .get("wind")
        .and_then(|w| w.get("speed"))
        .and_then(|s| s.get("value"));
    // "calm" (or anything else that is not a number) is no wind.
    let wind_kmh = number(wind_speed).unwrap_or(0.0);
    let visibility_mi =
        number(current.get("visibility").and_then(|v| v.get("value"))).map(|km| km / 1.609_344);
    let observed_at = english(current.get("timestamp"))
        .and_then(Value::as_str)
        .and_then(iso_timestamp);
    if temperature_c.is_none() && condition.is_none() && observed_at.is_none() {
        return Err("site reports no observation".to_string());
    }
    let hourly = props
        .get("hourlyForecastGroup")
        .and_then(|g| g.get("hourlyForecasts"))
        .and_then(Value::as_array)
        .map(|hours| {
            hours
                .iter()
                .filter_map(|hour| {
                    let at = english(hour.get("timestamp"))
                        .and_then(Value::as_str)
                        .and_then(iso_timestamp)?;
                    let text = english(hour.get("condition"))?.as_str()?.trim();
                    (!text.is_empty()).then(|| (at, text.to_string()))
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(SiteReading {
        condition,
        temperature_c,
        wind_kmh,
        visibility_mi,
        observed_at,
        hourly,
    })
}

/// The sites that stand for a point, nearest first: up to
/// [`STATION_WALK_LIMIT`] within [`SITE_REACH_MI`], or the single nearest
/// when none is that close.
pub fn pick_sites(sites: &[Site], lat: f64, lon: f64) -> Vec<Site> {
    let mut by_distance: Vec<(f64, &Site)> = sites
        .iter()
        .map(|site| (haversine_distance(lat, lon, site.lat, site.lon), site))
        .collect();
    by_distance.sort_by(|a, b| a.0.total_cmp(&b.0));
    let near: Vec<Site> = by_distance
        .iter()
        .filter(|(miles, _)| *miles <= SITE_REACH_MI)
        .take(STATION_WALK_LIMIT)
        .map(|(_, site)| (*site).clone())
        .collect();
    if near.is_empty() {
        by_distance
            .first()
            .map(|(_, site)| vec![(*site).clone()])
            .unwrap_or_default()
    } else {
        near
    }
}

/// The Canadian counterpart of [`NwsFetcher`](super::real_weather::NwsFetcher):
/// site search, the per-place site cache and the walk, over an injected
/// transport.
pub struct EcccFetcher {
    transport: Arc<dyn HttpTransport>,
    wall_clock: Clock,
    site_cache: Mutex<HashMap<String, Vec<Site>>>,
    site_pick: Mutex<HashMap<String, usize>>,
}

impl EcccFetcher {
    pub fn new(transport: Arc<dyn HttpTransport>) -> Self {
        Self {
            transport,
            wall_clock: wall_clock(),
            site_cache: Mutex::new(HashMap::new()),
            site_pick: Mutex::new(HashMap::new()),
        }
    }

    pub fn with_wall_clock(mut self, clock: Clock) -> Self {
        self.wall_clock = clock;
        self
    }

    /// Wrap the fetcher as the provider's Canadian `fetch` callable.
    pub fn fetch_fn(self: Arc<Self>) -> FetchFn {
        Arc::new(move |lat, lon| self.default_fetch(lat, lon))
    }

    fn get_json(&self, url: &str) -> Result<Value, String> {
        self.transport
            .get_json(
                url,
                &[("User-Agent", USER_AGENT), ("Accept", "application/json")],
                FETCH_TIMEOUT_S,
            )
            .map_err(|e| e.message)
    }

    /// The box search URL around a point.
    pub fn search_url(lat: f64, lon: f64, half_deg: f64) -> String {
        format!(
            "{CANADA_API_ROOT}/collections/citypageweather-realtime/items?f=json&bbox={:.4},{:.4},{:.4},{:.4}&properties=identifier&limit=100",
            lon - half_deg,
            lat - half_deg,
            lon + half_deg,
            lat + half_deg
        )
    }

    /// One site's document URL.
    pub fn site_url(id: &str) -> String {
        format!("{CANADA_API_ROOT}/collections/citypageweather-realtime/items/{id}?f=json")
    }

    fn place_key(lat: f64, lon: f64) -> String {
        format!("{lat:.2},{lon:.2}")
    }

    /// The sites that stand for a point, nearest first, cached per place
    /// for the session. Each box is searched only when the one before it
    /// held no site.
    pub fn resolve_sites(&self, lat: f64, lon: f64) -> Result<Vec<Site>, String> {
        let key = Self::place_key(lat, lon);
        if let Some(cached) = lock_unpoisoned(&self.site_cache).get(&key) {
            return Ok(cached.clone());
        }
        for half_deg in SITE_SEARCH_BOXES_DEG {
            let doc = self
                .get_json(&Self::search_url(lat, lon, half_deg))
                .map_err(|e| format!("site search: {e}"))?;
            let picked = pick_sites(&parse_site_list(&doc), lat, lon);
            if !picked.is_empty() {
                lock_unpoisoned(&self.site_cache).insert(key, picked.clone());
                return Ok(picked);
            }
        }
        Err(format!("no Environment Canada site near {lat:.4},{lon:.4}"))
    }

    /// The reading for a point: the first fresh site with an observed
    /// condition, the one that last answered tried first. With none, the
    /// nearest site that answered, its condition taken from its hourly
    /// forecast for this hour and its temperature and wind measured. With
    /// no forecast near enough either, the freshest stale site that has a
    /// condition, for the caller's last-known rules. Errs only when no site
    /// gave anything usable.
    pub fn default_fetch(&self, lat: f64, lon: f64) -> Result<Observation, String> {
        let _slot = FetchSlot::acquire();
        let key = Self::place_key(lat, lon);
        let sites = self.resolve_sites(lat, lon)?;
        let preferred = lock_unpoisoned(&self.site_pick)
            .get(&key)
            .copied()
            .filter(|i| *i < sites.len())
            .unwrap_or(0);
        let order = std::iter::once(preferred).chain((0..sites.len()).filter(|i| *i != preferred));
        let now = (self.wall_clock)();
        let mut answered: Vec<(usize, SiteReading)> = Vec::new();
        let mut errors: Vec<String> = Vec::new();
        for i in order {
            let Some(site) = sites.get(i) else {
                continue;
            };
            let reading = match self
                .get_json(&Self::site_url(&site.id))
                .and_then(|doc| parse_site(&doc))
            {
                Ok(reading) => reading,
                Err(error) => {
                    errors.push(format!("{}: {error}", site.id));
                    continue;
                }
            };
            let fresh = reading
                .observed_at
                .is_none_or(|at| now - at <= OBSERVATION_MAX_AGE_S);
            if let (true, Some(text)) = (fresh, reading.condition.as_deref()) {
                lock_unpoisoned(&self.site_pick).insert(key, i);
                log::info!(
                    "Environment Canada weather at {lat:.2},{lon:.2}: read, site {} condition {text:?}",
                    site.id
                );
                return Ok(reading.observation(text));
            }
            answered.push((i, reading));
        }
        answered.sort_by_key(|(i, _)| *i);
        if let Some((i, reading, text)) = answered.iter().find_map(|(i, reading)| {
            reading
                .forecast_condition(now)
                .map(|text| (*i, reading, text))
        }) {
            let id = &sites[i].id;
            let phrase = forecast_phrase(text);
            log::info!(
                "Environment Canada weather at {lat:.2},{lon:.2}: derived, site {id} hourly \
                 forecast {text:?} (read as {phrase:?}) with its measured temperature and wind"
            );
            return Ok(reading.observation(&phrase));
        }
        let stale = answered
            .iter()
            .filter_map(|(i, reading)| {
                let text = reading.condition.as_deref()?;
                Some((*i, reading, text, reading.observed_at.unwrap_or(now)))
            })
            .max_by(|a, b| a.3.total_cmp(&b.3));
        if let Some((i, reading, text, _)) = stale {
            log::info!(
                "Environment Canada weather at {lat:.2},{lon:.2}: read, site {} condition {text:?} \
                 (no fresher site)",
                sites[i].id
            );
            return Ok(reading.observation(text));
        }
        Err(if errors.is_empty() {
            "no site reported a condition".to_string()
        } else {
            format!("every site failed: {}", errors.join("; "))
        })
    }
}

#[cfg(test)]
mod tests;
