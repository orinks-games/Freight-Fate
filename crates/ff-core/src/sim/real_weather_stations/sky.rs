//! The sky condition for a point in Canada, from the real stations first.
//!
//! In this order, each step only when the one before it gave nothing:
//!
//! 1. A present weather code: the primary station's, or the nearest fresh
//!    station within reach that reports one (usually the airport).
//! 2. The city page's observed condition text.
//! 3. The primary station's precipitation gauge can then ADD precipitation
//!    to a dry result from steps 1 and 2, never remove it: gauges under-read
//!    light snow and drizzle, so a zero does not overrule an observer.
//!    Snow at or below 0 C, else rain; heavy rain at [`HEAVY_RAIN_MM_H`].
//!    Freezing rain never comes from a gauge alone: measured precipitation
//!    at or below 0 C is freezing rain only when Environment Canada also
//!    says so for the place (a freezing rain or drizzle alert in effect, or
//!    the city page's observed or forecast condition).
//! 4. With no precipitation and no code or text, the station's cloud cover.
//! 5. Else the city page's hourly forecast condition.
//!
//! Every text step keeps the city page rules of
//! [`map_condition_canada`]. The result says which step decided, for the
//! log line that records where the condition came from.

use crate::sim::real_weather_canada::map_condition_canada;
use crate::sim::weather::WeatherKind;

use super::present_weather;
use super::CloudCover;

/// Rain falling at this many millimeters an hour or more is heavy rain.
/// Read from the American Meteorological Society's Glossary of Meteorology
/// ("Rain": heavy rain falls at more than 7.6 mm an hour, 0.30 inch), the
/// intensity scale North American observers use. The figure in MANOBS,
/// Environment Canada's own observing manual, was not checked.
pub const HEAVY_RAIN_MM_H: f64 = 7.6;

/// Which step decided the sky.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkyBasis {
    /// A present weather code at a station.
    Code,
    /// The city page's observed condition.
    Text,
    /// The precipitation gauge added precipitation.
    Gauge,
    /// The station's cloud cover.
    CloudAmount,
    /// The city page's hourly forecast.
    Forecast,
}

impl SkyBasis {
    /// The word the log and the agent server use.
    pub fn key(self) -> &'static str {
        match self {
            Self::Code => "code",
            Self::Text => "text",
            Self::Gauge => "gauge",
            Self::CloudAmount => "cloud amount",
            Self::Forecast => "forecast",
        }
    }
}

/// What the sky decision has to go on.
#[derive(Debug, Clone, Default)]
pub struct SkyInputs {
    /// The present weather codes of the station that reported some.
    pub codes: Vec<u16>,
    /// The city page's fresh observed condition.
    pub page_text: Option<String>,
    /// The primary station's gauge: precipitating now, and its rate.
    pub precipitating: bool,
    pub rate_mm_h: Option<f64>,
    pub temperature_c: Option<f64>,
    pub cloud: Option<CloudCover>,
    /// The city page's hourly forecast condition for now, already read
    /// through `forecast_phrase`.
    pub forecast_text: Option<String>,
    pub wind_kmh: f64,
    pub visibility_mi: Option<f64>,
}

/// The decided sky: condition words in Environment Canada's vocabulary
/// (mapped by the caller with [`map_condition_canada`]) and the step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkyCall {
    pub phrase: String,
    pub basis: SkyBasis,
}

/// How much a condition says, most first, for choosing between two codes.
fn weight(kind: WeatherKind) -> u8 {
    match kind {
        WeatherKind::Thunderstorm => 8,
        WeatherKind::Ice => 7,
        WeatherKind::Snow => 6,
        WeatherKind::HeavyRain => 5,
        WeatherKind::Rain => 4,
        WeatherKind::Fog => 3,
        WeatherKind::Wind => 2,
        WeatherKind::Cloudy => 1,
        WeatherKind::Clear => 0,
    }
}

/// Whether a condition has no precipitation in it.
fn dry(kind: WeatherKind) -> bool {
    matches!(
        kind,
        WeatherKind::Clear | WeatherKind::Cloudy | WeatherKind::Fog | WeatherKind::Wind
    )
}

/// The phrase of the most telling code among `codes` (a station reports up
/// to two), None when none is weather now.
pub fn code_phrase(codes: &[u16], inputs: &SkyInputs) -> Option<&'static str> {
    codes
        .iter()
        .filter_map(|code| present_weather::phrase(*code))
        .max_by_key(|phrase| weight(kind_of(phrase, inputs)))
}

fn kind_of(phrase: &str, inputs: &SkyInputs) -> WeatherKind {
    map_condition_canada(
        phrase,
        inputs.wind_kmh,
        inputs.visibility_mi,
        inputs.temperature_c,
    )
}

/// Decide the sky. `freezing_said` is asked only when the gauge measured
/// precipitation at or below 0 C, and answers whether Environment Canada
/// says freezing rain or drizzle for the place.
pub fn decide(inputs: &SkyInputs, freezing_said: &mut dyn FnMut() -> bool) -> Option<SkyCall> {
    let observed = code_phrase(&inputs.codes, inputs)
        .map(|phrase| SkyCall {
            phrase: phrase.to_string(),
            basis: SkyBasis::Code,
        })
        .or_else(|| {
            inputs
                .page_text
                .as_deref()
                .map(str::trim)
                .filter(|text| !text.is_empty())
                .map(|text| SkyCall {
                    phrase: text.to_string(),
                    basis: SkyBasis::Text,
                })
        });
    if inputs.precipitating {
        let already_wet = observed
            .as_ref()
            .is_some_and(|call| !dry(kind_of(&call.phrase, inputs)));
        if !already_wet {
            let phrase = match inputs.temperature_c {
                Some(t) if t <= 0.0 => {
                    if freezing_said() {
                        "Freezing Rain"
                    } else {
                        "Snow"
                    }
                }
                _ if inputs.rate_mm_h.is_some_and(|rate| rate >= HEAVY_RAIN_MM_H) => "Heavy Rain",
                _ => "Rain",
            };
            return Some(SkyCall {
                phrase: phrase.to_string(),
                basis: SkyBasis::Gauge,
            });
        }
    }
    if observed.is_some() {
        return observed;
    }
    if let Some(cloud) = inputs.cloud {
        return Some(SkyCall {
            phrase: match cloud {
                CloudCover::Clear => "Clear",
                CloudCover::Cloudy => "Cloudy",
            }
            .to_string(),
            basis: SkyBasis::CloudAmount,
        });
    }
    inputs
        .forecast_text
        .as_deref()
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(|text| SkyCall {
            phrase: text.to_string(),
            basis: SkyBasis::Forecast,
        })
}

/// Whether a condition text says freezing rain or freezing drizzle.
pub fn says_freezing(text: &str) -> bool {
    let lowered = text.to_lowercase();
    lowered.contains("freezing rain") || lowered.contains("freezing drizzle")
}

#[cfg(test)]
mod tests;
