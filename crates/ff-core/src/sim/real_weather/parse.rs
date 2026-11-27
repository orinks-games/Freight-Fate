//! Reading one NWS `/observations/latest` document: the measurement
//! units, the timestamp and the fields the worker needs.

use chrono::{DateTime, Local, NaiveDateTime, TimeZone};
use serde_json::Value;

use super::Observation;
use crate::sim::real_traffic_parsers::pyval::{chain_str, py_str, to_f64, truthy};

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
pub(crate) fn iso_timestamp(text: &str) -> Option<f64> {
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
