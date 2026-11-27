//! What a live report measured beyond the sky, and the Weather app lines
//! that say it.
//!
//! A station reports more than its condition: where it is, the wind's
//! direction and gusts, visibility and snow on the ground, and in Canada an
//! hourly forecast. [`ObservationDetails`] carries what one report had; a
//! line whose value was not measured is left out, never spoken as
//! unavailable. Humidity, dew point and pressure are not carried: a driver
//! does nothing with them.

use crate::pyfmt::{fmt_f, round_py_int};
use crate::sim::weather::WeatherKind;
use crate::units::MILES_TO_KM;

const KM_PER_MILE: f64 = MILES_TO_KM;
const CM_PER_INCH: f64 = 2.54;
/// Under this sustained wind, in km/h, the report reads "calm".
pub const CALM_KMH: f64 = 2.0;
/// Snow on the ground under this many centimeters is not reported.
pub const SNOW_DEPTH_MIN_CM: f64 = 1.0;

/// What a live report measured besides its condition, temperature and
/// sustained wind. Every field is optional: a station without the sensor
/// leaves it out.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ObservationDetails {
    /// The station's name as it is spoken ("Barrie-Oro").
    pub station: Option<String>,
    /// How far the station is from the point asked about, in miles.
    pub station_distance_mi: Option<f64>,
    /// Where the wind comes from, in degrees.
    pub wind_direction_deg: Option<f64>,
    /// The strongest gust in the last ten minutes, in km/h.
    pub gust_kmh: Option<f64>,
    /// Measured visibility, in miles.
    pub visibility_mi: Option<f64>,
    /// Snow on the ground, in centimeters.
    pub snow_depth_cm: Option<f64>,
    /// The next changes of condition the forecast gives, soonest first:
    /// None where no forecast is on hand, empty where it holds no change.
    pub forecast: Option<Vec<WeatherKind>>,
    /// How the condition was decided and from which station, for the log
    /// and the agent server ("code 65, station 6158731").
    pub basis: Option<String>,
}

impl ObservationDetails {
    /// Whether a forecast came with the report.
    pub fn has_forecast(&self) -> bool {
        self.forecast.is_some()
    }
}

/// A compass direction, written out, for a wind from `degrees`.
pub fn compass_point(degrees: f64) -> &'static str {
    const POINTS: [&str; 8] = [
        "north",
        "northeast",
        "east",
        "southeast",
        "south",
        "southwest",
        "west",
        "northwest",
    ];
    let index = ((degrees.rem_euclid(360.0) + 22.5) / 45.0).floor() as usize % 8;
    POINTS[index]
}

/// A count with its unit, singular for one ("1 mile", "6 miles").
fn counted(value: i64, unit: &str) -> String {
    if value == 1 {
        format!("{value} {unit}")
    } else {
        format!("{value} {unit}s")
    }
}

/// "Station: Barrie-Oro, 6 miles away." Under one unit, "nearby".
pub fn station_line(details: &ObservationDetails, imperial: bool) -> Option<String> {
    let station = details.station.as_deref()?.trim();
    if station.is_empty() {
        return None;
    }
    let Some(miles) = details.station_distance_mi else {
        return Some(format!("Station: {station}."));
    };
    let (value, unit) = if imperial {
        (miles, "mile")
    } else {
        (miles * KM_PER_MILE, "kilometer")
    };
    if value < 1.0 {
        return Some(format!("Station: {station}, nearby."));
    }
    let distance = counted(round_py_int(value), unit);
    Some(format!("Station: {station}, {distance} away."))
}

/// "Wind: northeast at 12 miles per hour, gusting to 21." Calm under
/// [`CALM_KMH`]. `wind_kmh` is the sustained wind the report gave.
pub fn wind_line(details: &ObservationDetails, wind_kmh: f64, imperial: bool) -> String {
    if wind_kmh < CALM_KMH {
        return "Wind: calm.".to_string();
    }
    let speed = |kmh: f64| {
        if imperial {
            round_py_int(kmh / KM_PER_MILE)
        } else {
            round_py_int(kmh)
        }
    };
    let unit = if imperial { "mile" } else { "kilometer" };
    let sustained = format!("{} per hour", counted(speed(wind_kmh), unit));
    let from = details.wind_direction_deg.map(compass_point);
    let gust = details.gust_kmh.filter(|gust| *gust > wind_kmh).map(speed);
    match (from, gust) {
        (Some(from), Some(gust)) => format!("Wind: {from} at {sustained}, gusting to {gust}."),
        (Some(from), None) => format!("Wind: {from} at {sustained}."),
        (None, Some(gust)) => format!("Wind: {sustained}, gusting to {gust}."),
        (None, None) => format!("Wind: {sustained}."),
    }
}

/// One decimal below ten, whole numbers from ten up, no trailing ".0".
fn short_number(value: f64) -> String {
    let text = if value < 10.0 {
        fmt_f(value, 1)
    } else {
        round_py_int(value).to_string()
    };
    text.strip_suffix(".0").map(str::to_string).unwrap_or(text)
}

/// "Visibility: 1.2 miles." Only when the station measures it.
pub fn visibility_line(details: &ObservationDetails, imperial: bool) -> Option<String> {
    let miles = details.visibility_mi?;
    let (value, unit) = if imperial {
        (miles, "mile")
    } else {
        (miles * KM_PER_MILE, "kilometer")
    };
    let number = short_number(value);
    let distance = if number == "1" {
        format!("1 {unit}")
    } else {
        format!("{number} {unit}s")
    };
    Some(format!("Visibility: {distance}."))
}

/// "Snow on the ground: 14 centimeters." Only from [`SNOW_DEPTH_MIN_CM`].
pub fn snow_line(details: &ObservationDetails, imperial: bool) -> Option<String> {
    let cm = details
        .snow_depth_cm
        .filter(|cm| *cm >= SNOW_DEPTH_MIN_CM)?;
    let depth = if imperial {
        let inches = round_py_int(cm / CM_PER_INCH).max(1);
        if inches == 1 {
            "1 inch".to_string()
        } else {
            format!("{inches} inches")
        }
    } else {
        counted(round_py_int(cm), "centimeter")
    };
    Some(format!("Snow on the ground: {depth}."))
}

/// "Forecast ahead: rain, then snow." from a report's forecast; the current
/// condition alone when the forecast holds no change. None without a
/// forecast.
pub fn forecast_line(details: &ObservationDetails, current: WeatherKind) -> Option<String> {
    let changes: Vec<WeatherKind> = details
        .forecast
        .as_ref()?
        .iter()
        .copied()
        .filter(|kind| *kind != current)
        .take(2)
        .collect();
    if changes.is_empty() {
        return Some(format!("Forecast ahead: {}.", current.value()));
    }
    let names: Vec<&str> = changes.iter().map(|kind| kind.value()).collect();
    Some(format!("Forecast ahead: {}.", names.join(", then ")))
}

/// The next changes of condition in a run of forecast conditions, starting
/// from `current`: each entry that differs from the one before it, at most
/// two.
pub fn forecast_changes(current: WeatherKind, hours: &[WeatherKind]) -> Vec<WeatherKind> {
    let mut changes = Vec::new();
    let mut last = current;
    for kind in hours {
        if *kind != last {
            changes.push(*kind);
            last = *kind;
            if changes.len() == 2 {
                break;
            }
        }
    }
    changes
}

/// The Weather app's station lines, in order: Station, Wind, Visibility,
/// Snow on the ground. Lines with nothing measured are left out.
pub fn station_lines(details: &ObservationDetails, wind_kmh: f64, imperial: bool) -> Vec<String> {
    let mut lines = Vec::new();
    lines.extend(station_line(details, imperial));
    lines.push(wind_line(details, wind_kmh, imperial));
    lines.extend(visibility_line(details, imperial));
    lines.extend(snow_line(details, imperial));
    lines
}

/// The station of a report for the agent server's status: its name, its
/// distance and how the condition was decided ("Barrie-Oro, 6.1 mi, code,
/// station 6151064"); "none" without a station or a basis.
pub fn describe_station(details: &ObservationDetails) -> String {
    let parts: Vec<String> = [
        details.station.clone(),
        details.station_distance_mi.map(|mi| format!("{mi:.1} mi")),
        details.basis.clone(),
    ]
    .into_iter()
    .flatten()
    .filter(|part| !part.is_empty())
    .collect();
    if parts.is_empty() {
        "none".to_string()
    } else {
        parts.join(", ")
    }
}

/// Network and siting suffixes station names carry that are not part of
/// the place.
const NAME_SUFFIXES: [&str; 8] = [
    " CS", " RCS", " AUT", " AWOS", " (AUT)", " AUTO", " CLIMATE", " CDA",
];

/// A station's name as it is spoken, never a code: "/" as a space, the
/// network suffixes ([`NAME_SUFFIXES`]) dropped, an all-capitals name in
/// title case ("WASAGA PROVINCIAL PARK" is "Wasaga Provincial Park"), a
/// trailing "A" or "'A'" as "Airport", "INTL" as "International", and a
/// name the feed cut off inside a parenthesis ends before it.
pub fn clean_station_name(raw: &str) -> String {
    let mut name = raw.replace('/', " ");
    name = name.split_whitespace().collect::<Vec<_>>().join(" ");
    if let Some(open) = name.rfind('(') {
        if !name[open..].contains(')') {
            name = name[..open].trim_end().to_string();
        }
    }
    loop {
        let before = name.len();
        for suffix in NAME_SUFFIXES {
            if let Some(stripped) = name.strip_suffix(suffix) {
                name = stripped.trim_end().to_string();
            }
        }
        if name.len() == before {
            break;
        }
    }
    if !name.chars().any(char::is_lowercase) {
        name = name
            .split(' ')
            .map(title_word)
            .collect::<Vec<_>>()
            .join(" ");
    }
    let mut words: Vec<String> = name.split(' ').map(str::to_string).collect();
    if words.len() > 1 && matches!(words.last().map(String::as_str), Some("A" | "'A'")) {
        words.pop();
        words.push("Airport".to_string());
    }
    for word in &mut words {
        if matches!(word.as_str(), "Intl" | "INTL" | "Int'l") {
            *word = "International".to_string();
        }
    }
    // Environment Canada cuts names at 32 characters: a last word cut
    // short of a common ending is that ending ("Toronto City Airpor").
    if raw.chars().count() >= NAME_CUT_AT && words.len() > 1 {
        if let Some(last) = words.last_mut() {
            let cut = |whole: &&&str| {
                last.len() >= 3 && last.len() < whole.len() && whole.starts_with(last.as_str())
            };
            if let Some(whole) = CUT_ENDINGS.iter().find(cut) {
                *last = (*whole).to_string();
            }
        }
    }
    words.join(" ")
}

/// The length Environment Canada cuts station names at.
const NAME_CUT_AT: usize = 32;
/// Last words a cut name most often loses the end of.
const CUT_ENDINGS: [&str; 4] = ["Airport", "International", "Regional", "Municipal"];

/// One upper-case word in title case: a capital after a hyphen, a
/// parenthesis or a full stop, and after the apostrophe of a French
/// elision ("L'ACADIE" is "L'Acadie", "VAL-D'OR" is "Val-D'Or"), but not of
/// a possessive ("ST ALBAN'S" is "St Alban's"). "'A'" is left as it is.
fn title_word(word: &str) -> String {
    if word == "'A'" {
        return word.to_string();
    }
    let mut out = String::with_capacity(word.len());
    let mut start = true;
    let mut run = 0usize;
    for ch in word.chars() {
        if start {
            out.extend(ch.to_uppercase());
        } else {
            out.extend(ch.to_lowercase());
        }
        let elision = ch == '\'' && run == 1;
        start = matches!(ch, '-' | '(' | '.') || elision;
        run = if ch.is_alphabetic() { run + 1 } else { 0 };
    }
    out
}

#[cfg(test)]
mod tests;
