//! Spoken measurement wording, shared by the settings layer and the trip sim.
//!
//! A leaf module on purpose: in the Python game `settings` and `sim.trip`
//! could not import each other (settings reaches the models package, which
//! loads the sim package), so the one pluralization rule for spoken
//! distances lives here where both can reach it.
//!
//! Port of `freight_fate/units.py`.

use crate::pyfmt::{fmt_f, round_py_int};

pub const MILES_TO_KM: f64 = 1.609344;

/// A whole-number distance with the unit pluralized for speech, so a
/// screen reader never hears "in 1 miles".
pub fn spoken_distance(value: f64, unit: &str) -> String {
    // Python's `round()` returns an int, so the spoken number never carries
    // a "-0"; round to the integer here for the same reason.
    let rounded = round_py_int(value);
    if rounded == 1 {
        format!("{rounded} {unit}")
    } else {
        format!("{rounded} {unit}s")
    }
}

/// An internal mileage as the number the player's unit setting asks for.
///
/// The sim measures everything in miles; this is the single conversion the
/// readouts go through, so a metric player never hears one screen's
/// kilometers next to another screen's raw miles.
pub fn to_distance(miles: f64, imperial: bool) -> f64 {
    if imperial {
        miles
    } else {
        miles * MILES_TO_KM
    }
}

/// The spoken name of the player's distance unit.
pub fn distance_unit(imperial: bool, plural: bool) -> &'static str {
    match (imperial, plural) {
        (true, true) => "miles",
        (true, false) => "mile",
        (false, true) => "kilometers",
        (false, false) => "kilometer",
    }
}

/// A very short distance in round feet or meters, never decimals.
///
/// The last few hundred of them before a stop bar, a gate, or a turn, where
/// every miles-based wording in the game has already bottomed out: 50-foot
/// steps down to 50 feet, 20-meter steps down to 20 meters.
pub fn spoken_feet_or_meters(miles: f64, imperial: bool) -> String {
    if imperial {
        let feet = (round_py_int(miles * 5280.0 / 50.0) * 50).max(50);
        return format!("{feet} feet");
    }
    let meters = (round_py_int(miles * MILES_TO_KM * 1000.0 / 20.0) * 20).max(20);
    format!("{meters} meters")
}

/// A one-decimal distance, for cues close enough that rounding to whole
/// units would hide the gap being announced.
pub fn spoken_gap(miles: f64, imperial: bool) -> String {
    format!(
        "{} {}",
        fmt_f(to_distance(miles, imperial), 1),
        distance_unit(imperial, true)
    )
}

/// Speed for the visual HUD, in the short written form.
pub fn hud_speed(mph: f64, imperial: bool) -> String {
    if imperial {
        format!("{} mph", fmt_f(mph, 0))
    } else {
        format!("{} km/h", fmt_f(to_distance(mph, imperial), 0))
    }
}

/// The player's choice on the Units row.
///
/// Automatic follows the country the truck is in; Imperial and Metric hold
/// one unit everywhere. Stored in settings as its lowercase name.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum UnitsMode {
    #[default]
    Automatic,
    Imperial,
    Metric,
}

impl UnitsMode {
    /// The row's order, which is also the cycle order.
    pub const ALL: [UnitsMode; 3] = [UnitsMode::Automatic, UnitsMode::Imperial, UnitsMode::Metric];

    /// The settings-file value.
    pub fn key(self) -> &'static str {
        match self {
            UnitsMode::Automatic => "automatic",
            UnitsMode::Imperial => "imperial",
            UnitsMode::Metric => "metric",
        }
    }

    /// The settings-file value back to a mode; `None` for anything else.
    pub fn from_key(key: &str) -> Option<UnitsMode> {
        UnitsMode::ALL.into_iter().find(|mode| mode.key() == key)
    }
}

/// The unit a readout is spoken in right now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Units {
    Miles,
    Kilometers,
}

impl Units {
    pub fn is_imperial(self) -> bool {
        self == Units::Miles
    }
}

/// The country a truck is in when nothing says otherwise: the main menu,
/// a screen with no career open, and any place the map cannot place.
pub const HOME_COUNTRY: &str = "US";

/// What each country posts its speeds and distances in. A country missing
/// from the table reads as miles, the game's home unit. Adding a country
/// that posts in kilometres is one row here, never a new branch.
pub const COUNTRY_UNITS: &[(&str, Units)] = &[
    ("US", Units::Miles),
    ("CA", Units::Kilometers),
    ("MX", Units::Kilometers),
];

/// The unit a country posts in, by two-letter code, any case.
pub fn country_units(country: &str) -> Units {
    COUNTRY_UNITS
        .iter()
        .find(|(code, _)| code.eq_ignore_ascii_case(country))
        .map_or(Units::Miles, |(_, units)| *units)
}

/// The one answer to "which units now": Imperial and Metric ignore the
/// country, Automatic takes the country's own.
pub fn effective(mode: UnitsMode, country: &str) -> Units {
    match mode {
        UnitsMode::Imperial => Units::Miles,
        UnitsMode::Metric => Units::Kilometers,
        UnitsMode::Automatic => country_units(country),
    }
}

/// Where the units question is being asked from. Each one moves
/// [`UnitsPlace`]; only a drive crossing into a country with other units
/// owes the driver a spoken line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UnitsEvent {
    /// The truck is on the road in this country (a drive, every frame).
    TruckIn(String),
    /// A screen at a place in this country: a city, a dock, a garage.
    ScreenAt(String),
    /// No career is open: the main menu and its screens.
    NoCareer,
}

/// Spoken once when a drive crosses into a country with other units.
pub const INTO_KILOMETERS_LINE: &str = "Speeds and distances are in kilometers now.";
pub const INTO_MILES_LINE: &str = "Speeds and distances are in miles now.";

/// The country the units are being judged in, and the transition into the
/// next one. The mode is the player's and lives in settings; this holds
/// only where the truck is. `next` is the only place it moves.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnitsPlace {
    country: String,
}

impl Default for UnitsPlace {
    fn default() -> Self {
        Self {
            country: HOME_COUNTRY.to_string(),
        }
    }
}

impl UnitsPlace {
    pub fn country(&self) -> &str {
        &self.country
    }

    /// Move to the place `event` names. Returns the line to speak when a
    /// drive just changed the units the player hears: only Automatic can
    /// do that, only on the road, and only when the unit itself changed.
    pub fn next(&mut self, mode: UnitsMode, event: UnitsEvent) -> Option<&'static str> {
        let before = effective(mode, &self.country);
        let (country, spoken) = match event {
            UnitsEvent::TruckIn(country) => (country, true),
            UnitsEvent::ScreenAt(country) => (country, false),
            UnitsEvent::NoCareer => (HOME_COUNTRY.to_string(), false),
        };
        if country.is_empty() {
            // The map is silent here: keep the last country rather than
            // flip the units on a gap in the data.
            return None;
        }
        self.country = country.to_ascii_uppercase();
        let after = effective(mode, &self.country);
        if !spoken || before == after {
            return None;
        }
        Some(match after {
            Units::Kilometers => INTO_KILOMETERS_LINE,
            Units::Miles => INTO_MILES_LINE,
        })
    }
}

#[cfg(test)]
mod tests {
    //! Ported from the pure parts of `tests/test_metric_readouts.py` (the
    //! `Settings`-backed cases stay with the settings port) and the
    //! `_spoken_distance` check in `tests/test_driving_features.py`.
    use super::*;

    #[test]
    fn test_to_distance_leaves_imperial_alone_and_converts_metric() {
        assert_eq!(to_distance(100.0, true), 100.0);
        assert_eq!(to_distance(100.0, false), 100.0 * MILES_TO_KM);
    }

    #[test]
    fn test_distance_unit_names_both_settings_singular_and_plural() {
        assert_eq!(distance_unit(true, true), "miles");
        assert_eq!(distance_unit(true, false), "mile");
        assert_eq!(distance_unit(false, true), "kilometers");
        assert_eq!(distance_unit(false, false), "kilometer");
    }

    #[test]
    fn test_spoken_gap_keeps_one_decimal_in_both_units() {
        assert_eq!(spoken_gap(2.0, true), "2.0 miles");
        assert_eq!(spoken_gap(2.0, false), "3.2 kilometers");
    }

    #[test]
    fn test_hud_speed_uses_the_short_written_form() {
        assert_eq!(hud_speed(55.0, true), "55 mph");
        assert_eq!(hud_speed(55.0, false), "89 km/h");
    }

    #[test]
    fn test_spoken_distances_pluralize() {
        assert_eq!(spoken_distance(1.4, "mile"), "1 mile");
        assert_eq!(spoken_distance(0.6, "mile"), "1 mile");
        assert_eq!(spoken_distance(2.6, "kilometer"), "3 kilometers");
        assert_eq!(spoken_distance(0.2, "mile"), "0 miles");
        // Python's round() is half to even: 2.5 miles is "2 miles".
        assert_eq!(spoken_distance(2.5, "mile"), "2 miles");
    }

    #[test]
    fn test_spoken_feet_or_meters_steps_and_floors() {
        // 0.05 mi = 264 ft -> nearest 50 is 250.
        assert_eq!(spoken_feet_or_meters(0.05, true), "250 feet");
        assert_eq!(spoken_feet_or_meters(0.001, true), "50 feet");
        // 0.05 mi = 80.47 m -> nearest 20 is 80.
        assert_eq!(spoken_feet_or_meters(0.05, false), "80 meters");
        assert_eq!(spoken_feet_or_meters(0.001, false), "20 meters");
    }

    #[test]
    fn effective_units_transition_table() {
        use UnitsMode::*;
        let table = [
            (Automatic, "US", Units::Miles),
            (Automatic, "CA", Units::Kilometers),
            (Automatic, "ca", Units::Kilometers),
            (Automatic, "MX", Units::Kilometers),
            (Automatic, "ZZ", Units::Miles),
            (Imperial, "US", Units::Miles),
            (Imperial, "CA", Units::Miles),
            (Metric, "US", Units::Kilometers),
            (Metric, "CA", Units::Kilometers),
        ];
        for (mode, country, want) in table {
            assert_eq!(effective(mode, country), want, "{mode:?} in {country}");
        }
    }

    #[test]
    fn units_place_speaks_only_an_automatic_drive_changing_units() {
        use UnitsMode::*;
        let into = |c: &str| UnitsEvent::TruckIn(c.to_string());
        let at = |c: &str| UnitsEvent::ScreenAt(c.to_string());
        // (mode, start, event, line)
        let table: [(UnitsMode, &str, UnitsEvent, Option<&str>); 10] = [
            (Automatic, "US", into("CA"), Some(INTO_KILOMETERS_LINE)),
            (Automatic, "CA", into("US"), Some(INTO_MILES_LINE)),
            (Automatic, "US", into("US"), None),
            (Automatic, "CA", into("CA"), None),
            (Automatic, "US", at("CA"), None),
            (Automatic, "CA", UnitsEvent::NoCareer, None),
            (Automatic, "US", into(""), None),
            (Imperial, "US", into("CA"), None),
            (Metric, "US", into("CA"), None),
            (Metric, "CA", into("US"), None),
        ];
        for (mode, start, event, line) in table {
            let mut place = UnitsPlace::default();
            place.next(mode, UnitsEvent::ScreenAt(start.to_string()));
            assert_eq!(
                place.next(mode, event.clone()),
                line,
                "{mode:?} {start} {event:?}"
            );
        }
    }

    #[test]
    fn a_gap_in_the_map_keeps_the_last_country() {
        let mut place = UnitsPlace::default();
        place.next(UnitsMode::Automatic, UnitsEvent::TruckIn("CA".into()));
        place.next(UnitsMode::Automatic, UnitsEvent::TruckIn(String::new()));
        assert_eq!(place.country(), "CA");
    }

    #[test]
    fn units_mode_keys_round_trip() {
        for mode in UnitsMode::ALL {
            assert_eq!(UnitsMode::from_key(mode.key()), Some(mode));
        }
        assert_eq!(UnitsMode::from_key("furlongs"), None);
    }
}
