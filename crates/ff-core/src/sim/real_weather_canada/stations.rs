//! The Canadian reading from the real weather stations.
//!
//! The station list is read once a session, on the first Canadian fetch
//! (already off the loop, inside a fetch slot), and kept; a failed read is
//! retried after [`RETRY_AFTER_S`], and until it succeeds the city page
//! answers alone. With the list on hand a point reads its nearest stations,
//! nearest first and at most [`STATION_WALK_LIMIT`] of them: the first
//! with a fresh observation is the primary station, whose measurements the
//! report carries, and the first fresh one that reports a present weather
//! code gives the code, and its visibility when the primary measures none
//! (usually an airport near an automatic station). The sky is decided in [`sky::decide`]; the city
//! page the caller already read gives the condition text, the forecast and
//! whether Environment Canada says freezing rain.
//!
//! [`STATION_WALK_LIMIT`]: crate::sim::real_weather::STATION_WALK_LIMIT

use std::sync::{Arc, Mutex, OnceLock};

use super::{forecast_ahead, forecast_phrase, EcccFetcher, PageWalk, FORECAST_ONLY};
use crate::sim::real_traffic::lock_unpoisoned;
use crate::sim::real_weather::{Observation, OBSERVATION_MAX_AGE_S, RETRY_AFTER_S};
use crate::sim::real_weather_alerts_canada::{canada_alerts_url, parse_canada_alerts};
use crate::sim::real_weather_stations::sky::{self, SkyInputs};
use crate::sim::real_weather_stations::{
    nearest_stations, parse_station_list, parse_swob, station_list_url, station_observation_url,
    Station, SwobReading,
};
use crate::sim::weather_report::{clean_station_name, ObservationDetails};

const KM_PER_MILE: f64 = 1.609_344;

/// The session's station list: not read yet, read, or failed at a time.
#[derive(Debug, Default)]
pub enum StationList {
    #[default]
    NotRead,
    Read(Arc<Vec<Station>>),
    Failed(f64),
}

/// A station list several fetchers read through.
pub type SharedStationList = Arc<Mutex<StationList>>;

/// The session's one station list: every drive and the terminal make
/// their own provider, and the list is read once for all of them.
pub fn session_station_list() -> SharedStationList {
    static LIST: OnceLock<SharedStationList> = OnceLock::new();
    Arc::clone(LIST.get_or_init(|| Arc::new(Mutex::new(StationList::default()))))
}

impl EcccFetcher {
    /// The station list, read on first use and kept. None until it has
    /// read, and for [`RETRY_AFTER_S`] after a failed read.
    pub fn station_list(&self) -> Option<Arc<Vec<Station>>> {
        let now = (self.wall_clock)();
        {
            let state = lock_unpoisoned(&self.station_list);
            match &*state {
                StationList::Read(list) => return Some(Arc::clone(list)),
                StationList::Failed(at) if now - at < RETRY_AFTER_S => return None,
                _ => {}
            }
        }
        let read = self
            .get_json(&station_list_url())
            .map(|doc| parse_station_list(&doc));
        let mut state = lock_unpoisoned(&self.station_list);
        match read {
            Ok(list) if !list.is_empty() => {
                log::info!(
                    "Environment Canada station list read: {} stations",
                    list.len()
                );
                let list = Arc::new(list);
                *state = StationList::Read(Arc::clone(&list));
                Some(list)
            }
            Ok(_) => {
                log::warn!("Environment Canada station list was empty; the city pages answer");
                *state = StationList::Failed(now);
                None
            }
            Err(error) => {
                log::warn!(
                    "Environment Canada station list failed: {error}; the city pages answer"
                );
                *state = StationList::Failed(now);
                None
            }
        }
    }

    /// Whether an Environment Canada freezing rain or freezing drizzle
    /// alert is in effect at the point. A failed read is no.
    fn alert_says_freezing(&self, lat: f64, lon: f64) -> bool {
        let now = (self.wall_clock)();
        self.get_json(&canada_alerts_url(lat, lon))
            .map(|doc| {
                parse_canada_alerts(&doc, now)
                    .iter()
                    .any(|alert| sky::says_freezing(&alert.event))
            })
            .unwrap_or(false)
    }

    /// The reading for a point from its real stations. Errs when no station
    /// near enough has a fresh observation, or nothing decides the sky.
    pub fn from_stations(
        &self,
        list: &[Station],
        lat: f64,
        lon: f64,
        page: Option<&PageWalk>,
    ) -> Result<Observation, String> {
        let now = (self.wall_clock)();
        let mut primary: Option<(Station, f64, SwobReading)> = None;
        let mut coded: Option<(Station, SwobReading)> = None;
        for (station, miles) in nearest_stations(list, lat, lon) {
            let reading = match self
                .get_json(&station_observation_url(&station.id))
                .and_then(|doc| parse_swob(&doc))
            {
                Ok(reading) => reading,
                Err(error) => {
                    log::debug!("Environment Canada station {}: {error}", station.id);
                    continue;
                }
            };
            let fresh = reading
                .observed_at
                .is_some_and(|at| now - at <= OBSERVATION_MAX_AGE_S);
            if !fresh {
                continue;
            }
            let has_code =
                sky::code_phrase(&reading.present_weather, &SkyInputs::default()).is_some();
            if coded.is_none() && has_code {
                coded = Some((station.clone(), reading.clone()));
            }
            if primary.is_none() {
                primary = Some((station, miles, reading));
            }
            if coded.is_some() {
                break;
            }
        }
        let (station, miles, reading) =
            primary.ok_or("no station near enough has a fresh observation")?;
        // Visibility from the station that gave the code when the primary
        // measures none: the code and the figure then share an observer.
        let visibility_mi = reading
            .visibility_km
            .or_else(|| coded.as_ref().and_then(|(_, r)| r.visibility_km))
            .map(|km| km / KM_PER_MILE);
        // A site that measured nothing lends its forecast, not numbers.
        let page_observation = page
            .filter(|walk| walk.basis != FORECAST_ONLY)
            .map(|walk| &walk.observation);
        let temperature_c = reading
            .temperature_c
            .or_else(|| page_observation.and_then(|o| o.temperature_c));
        let wind_kmh = reading
            .wind_kmh
            .or_else(|| page_observation.map(|o| o.wind_kmh))
            .unwrap_or(0.0);
        let inputs = SkyInputs {
            codes: coded
                .as_ref()
                .map(|(_, r)| r.present_weather.clone())
                .unwrap_or_default(),
            page_text: page.and_then(|walk| walk.observed_condition.clone()),
            precipitating: reading.precipitating(),
            rate_mm_h: reading.precipitation_rate_mm_h(),
            temperature_c,
            cloud: reading.cloud,
            forecast_text: page
                .and_then(|walk| walk.forecast_now.as_deref())
                .map(forecast_phrase),
            wind_kmh,
            visibility_mi,
        };
        let mut freezing_said = || {
            page.is_some_and(|walk| {
                walk.observed_condition
                    .as_deref()
                    .is_some_and(sky::says_freezing)
                    || walk.forecast_now.as_deref().is_some_and(sky::says_freezing)
            }) || self.alert_says_freezing(lat, lon)
        };
        let call = sky::decide(&inputs, &mut freezing_said).ok_or("nothing decides the sky")?;
        let decided_by = match (call.basis, coded.as_ref()) {
            (sky::SkyBasis::Code, Some((code_station, _))) => {
                format!("station {}", code_station.id)
            }
            (sky::SkyBasis::Text | sky::SkyBasis::Forecast, _) => page
                .map(|walk| format!("site {}", walk.site))
                .unwrap_or_default(),
            _ => format!("station {}", station.id),
        };
        // The observation's own name first: the list cuts names at 32
        // characters ("Billy Bishop Toronto City Airpor").
        let name = reading
            .station_name
            .clone()
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| station.name.clone());
        let spoken_name = clean_station_name(&name);
        log::info!(
            "Environment Canada weather at {lat:.2},{lon:.2}: {basis} from {decided_by}, \
             condition {phrase:?}; station {id} {spoken_name:?}, {miles:.1} mi",
            basis = call.basis.key(),
            phrase = call.phrase,
            id = station.id,
        );
        let mut observation = Observation {
            text: call.phrase.clone(),
            wind_kmh,
            temperature_c,
            visibility_mi,
            observed_at: reading.observed_at,
            details: ObservationDetails {
                station: Some(spoken_name).filter(|n| !n.is_empty()),
                station_distance_mi: Some(miles),
                wind_direction_deg: reading.wind_direction_deg,
                gust_kmh: reading.gust_kmh,
                visibility_mi,
                snow_depth_cm: reading.snow_depth_cm,
                forecast: None,
                basis: Some(format!("{}, {decided_by}", call.basis.key())),
            },
        };
        if let Some(walk) = page {
            observation.details.forecast = Some(forecast_ahead(&walk.hourly, now, &observation));
        }
        Ok(observation)
    }
}
