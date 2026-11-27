//! Environment Canada's real weather stations: where they are, and what
//! each one measured last.
//!
//! The `swob-stations` collection lists every station (938 on 2026-11-27)
//! at its real position, and `swob-realtime` holds each station's
//! observations, the newest asked for by station id. Stations are where
//! Environment Canada says they are: the list is read once a session, off
//! the loop, and nothing about them is baked into the game's data.
//!
//! A measurement the station has no sensor for is an absent key, a null
//! or the string `MSNG`; each value has a `-qa` sibling, and a `-qa` of -1
//! marks a value not to use. [`SwobReading`] keeps only what passed.
//!
//! The sky condition is decided in [`sky`], present weather codes are
//! decoded in [`present_weather`].

pub mod present_weather;
pub mod sky;

use serde_json::{Map, Value};

use super::real_traffic::haversine_distance;
use super::real_weather::{iso_timestamp, STATION_WALK_LIMIT};
use super::real_weather_canada::{CANADA_API_ROOT, SITE_REACH_MI};

/// One weather station: its Environment Canada id, its name as listed, and
/// where it stands.
#[derive(Debug, Clone, PartialEq)]
pub struct Station {
    pub id: String,
    pub name: String,
    pub lat: f64,
    pub lon: f64,
}

/// The URL that lists every station in one answer.
pub fn station_list_url() -> String {
    format!("{CANADA_API_ROOT}/collections/swob-stations/items?f=json&limit=5000")
}

/// The URL of one station's newest observation.
pub fn station_observation_url(id: &str) -> String {
    format!(
        "{CANADA_API_ROOT}/collections/swob-realtime/items?f=json&msc_id-value={id}&sortby=-date_tm-value&limit=1"
    )
}

/// The stations in a `swob-stations` answer. A station with no id or no
/// position is left out.
pub fn parse_station_list(doc: &Value) -> Vec<Station> {
    let Some(features) = doc.get("features").and_then(Value::as_array) else {
        return Vec::new();
    };
    features
        .iter()
        .filter_map(|feature| {
            let props = feature.get("properties")?;
            let id = props
                .get("msc_id")
                .or_else(|| feature.get("id"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|id| !id.is_empty())?
                .to_string();
            let name = props
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_string();
            let coords = feature.pointer("/geometry/coordinates")?.as_array()?;
            let lon = coords.first()?.as_f64()?;
            let lat = coords.get(1)?.as_f64()?;
            Some(Station { id, name, lat, lon })
        })
        .collect()
}

/// The stations to read for a point, nearest first with their distance in
/// miles: up to [`STATION_WALK_LIMIT`] within [`SITE_REACH_MI`], or the
/// single nearest when none is that close (McBride, British Columbia, is
/// 75 miles from its nearest).
pub fn nearest_stations(stations: &[Station], lat: f64, lon: f64) -> Vec<(Station, f64)> {
    let mut by_distance: Vec<(f64, &Station)> = stations
        .iter()
        .map(|station| {
            (
                haversine_distance(lat, lon, station.lat, station.lon),
                station,
            )
        })
        .collect();
    by_distance.sort_by(|a, b| a.0.total_cmp(&b.0));
    let near: Vec<(Station, f64)> = by_distance
        .iter()
        .filter(|(miles, _)| *miles <= SITE_REACH_MI)
        .take(STATION_WALK_LIMIT)
        .map(|(miles, station)| ((*station).clone(), *miles))
        .collect();
    if near.is_empty() {
        by_distance
            .first()
            .map(|(miles, station)| vec![((*station).clone(), *miles)])
            .unwrap_or_default()
    } else {
        near
    }
}

/// The sky's cloud cover as a station reports it: clear (clear, few or
/// scattered) or cloudy (broken or overcast).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloudCover {
    Clear,
    Cloudy,
}

/// What one station measured in its newest observation.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SwobReading {
    pub station_name: Option<String>,
    pub observed_at: Option<f64>,
    pub temperature_c: Option<f64>,
    /// Sustained wind in km/h: the 10-minute mean, else the 2-minute, else
    /// the 1-minute.
    pub wind_kmh: Option<f64>,
    /// Where that wind comes from, in degrees.
    pub wind_direction_deg: Option<f64>,
    pub gust_kmh: Option<f64>,
    pub visibility_km: Option<f64>,
    /// Precipitation in the last 10 minutes, in mm.
    pub precipitation_10min_mm: Option<f64>,
    /// Precipitation in the last hour, in mm (total, else rain).
    pub precipitation_1hr_mm: Option<f64>,
    pub snow_depth_cm: Option<f64>,
    /// The present weather codes, in the order reported.
    pub present_weather: Vec<u16>,
    pub cloud: Option<CloudCover>,
}

impl SwobReading {
    /// Whether the gauge says it is precipitating now: the last 10 minutes
    /// measured more than zero, or, where only the hourly figure exists,
    /// the last hour did.
    pub fn precipitating(&self) -> bool {
        match self.precipitation_10min_mm {
            Some(mm) => mm > 0.0,
            None => self.precipitation_1hr_mm.is_some_and(|mm| mm > 0.0),
        }
    }

    /// The measured rate, in mm an hour: the 10-minute amount times six,
    /// else the last hour's amount.
    pub fn precipitation_rate_mm_h(&self) -> Option<f64> {
        self.precipitation_10min_mm
            .map(|mm| mm * 6.0)
            .or(self.precipitation_1hr_mm)
    }
}

/// A measurement that passed: a number, not null, not `MSNG`, and not
/// marked with a `-qa` of -1.
fn measured(props: &Map<String, Value>, key: &str) -> Option<f64> {
    let qa = props.get(&format!("{key}-qa")).and_then(|qa| match qa {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    });
    if qa.is_some_and(|qa| qa < 0.0) {
        return None;
    }
    match props.get(key)? {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

/// The first of `keys` that was measured.
fn first_measured(props: &Map<String, Value>, keys: &[&str]) -> Option<f64> {
    keys.iter().find_map(|key| measured(props, key))
}

/// The cloud cover a station reports: its total cloud amount in percent
/// (broken starts at 50, as the SWOB guide's table 6.5.15 defines it for
/// automatic stations), else its layer amount codes from that table (0
/// clear, 1 few, 2 scattered, 3 broken, 4 overcast), the most covered
/// layer deciding. Codes outside 0 to 4 are not cloud amounts and are
/// passed over.
fn cloud_cover(props: &Map<String, Value>) -> Option<CloudCover> {
    if let Some(percent) = measured(props, "tot_cld_amt") {
        return Some(if percent >= 50.0 {
            CloudCover::Cloudy
        } else {
            CloudCover::Clear
        });
    }
    let layers = (1..=5)
        .filter_map(|n| measured(props, &format!("cld_amt_code_{n}")))
        .filter(|code| (0.0..=4.0).contains(code))
        .fold(None::<f64>, |most, code| {
            Some(most.map_or(code, |m| m.max(code)))
        })?;
    Some(if layers >= 3.0 {
        CloudCover::Cloudy
    } else {
        CloudCover::Clear
    })
}

/// Read the newest observation in a `swob-realtime` answer. Errs when the
/// answer holds no observation.
pub fn parse_swob(doc: &Value) -> Result<SwobReading, String> {
    let feature = doc
        .get("features")
        .and_then(Value::as_array)
        .and_then(|features| features.first())
        .ok_or("no observation")?;
    let props = feature
        .get("properties")
        .and_then(Value::as_object)
        .ok_or("observation has no properties")?;
    let text = |key: &str| {
        props
            .get(key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty() && *s != "MSNG")
            .map(str::to_string)
    };
    let observed_at = text("date_tm-value")
        .or_else(|| text("obs_date_tm"))
        .and_then(|stamp| iso_timestamp(&stamp));
    let wind_kmh = first_measured(
        props,
        &[
            "avg_wnd_spd_10m_pst10mts",
            "avg_wnd_spd_10m_pst2mts",
            "avg_wnd_spd_10m_pst1mt",
        ],
    );
    let wind_direction_deg = first_measured(
        props,
        &[
            "avg_wnd_dir_10m_pst10mts",
            "avg_wnd_dir_10m_pst2mts",
            "avg_wnd_dir_10m_pst1mt",
        ],
    );
    let present_weather = ["prsnt_wx_1", "prsnt_wx_2"]
        .iter()
        .filter_map(|key| measured(props, key))
        .filter(|code| *code >= 0.0 && code.fract() == 0.0 && *code <= f64::from(u16::MAX))
        .map(|code| code as u16)
        .collect();
    Ok(SwobReading {
        station_name: text("stn_nam-value"),
        observed_at,
        temperature_c: measured(props, "air_temp"),
        wind_kmh,
        wind_direction_deg,
        gust_kmh: measured(props, "max_wnd_gst_spd_10m_pst10mts"),
        visibility_km: measured(props, "vis"),
        precipitation_10min_mm: measured(props, "pcpn_amt_pst10mts"),
        precipitation_1hr_mm: first_measured(props, &["pcpn_amt_pst1hr", "rnfl_amt_pst1hr"]),
        // Negative depths occur and mean nothing.
        snow_depth_cm: measured(props, "snw_dpth").filter(|cm| *cm >= 0.0),
        present_weather,
        cloud: cloud_cover(props),
    })
}

#[cfg(test)]
mod tests;
