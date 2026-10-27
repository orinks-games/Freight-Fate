//! Speaking speeds and distances in the player's units.

use super::{distance_text_for, short_distance_text_for, Settings};
use crate::pyfmt::{fmt_f, fmt_grouped, round_py_int};
use crate::units::{
    distance_unit, hud_speed, spoken_distance, spoken_gap, to_distance, MILES_TO_KM,
};

impl Settings {
    pub fn speed_text(&self, mph: f64) -> String {
        if self.imperial_units {
            format!("{} per hour", spoken_distance(mph, "mile"))
        } else {
            format!(
                "{} per hour",
                spoken_distance(mph * MILES_TO_KM, "kilometer")
            )
        }
    }

    /// `speed_text`'s bare number, for the terse slot grammar where the
    /// frame carries the unit ("Limit 65.").
    pub fn speed_value(&self, mph: f64) -> String {
        let value = if self.imperial_units {
            mph
        } else {
            mph * MILES_TO_KM
        };
        round_py_int(value).to_string()
    }

    /// Spoken distance in the player's unit. `precise` keeps one decimal for
    /// short spans ("1.2 miles ahead") where whole numbers would read as
    /// zero or lie by half a mile.
    pub fn distance_text(&self, miles: f64, precise: bool) -> String {
        distance_text_for(miles, self.imperial_units, precise)
    }

    /// Colloquial short range for pacenote-style calls: quarter-mile steps
    /// under a mile ("half a mile"), 100-meter steps under a kilometer ("400
    /// meters"), the normal precise form beyond.
    pub fn short_distance_text(&self, miles: f64) -> String {
        short_distance_text_for(miles, self.imperial_units)
    }

    /// A spoken distance kept to one decimal, for close-range cues.
    pub fn gap_text(&self, miles: f64) -> String {
        spoken_gap(miles, self.imperial_units)
    }

    /// Speed for the visual HUD, in the short written form.
    pub fn hud_speed_text(&self, mph: f64) -> String {
        hud_speed(mph, self.imperial_units)
    }

    /// A bare converted distance, for readouts that name the unit once after
    /// two numbers ("12 of 400 miles").
    pub fn distance_value(&self, miles: f64, decimals: usize, grouped: bool) -> String {
        let value = to_distance(miles, self.imperial_units);
        if grouped {
            fmt_grouped(value, decimals)
        } else {
            fmt_f(value, decimals)
        }
    }

    /// The player's distance unit, to pair with `distance_value`.
    pub fn distance_unit_text(&self, plural: bool) -> &'static str {
        distance_unit(self.imperial_units, plural)
    }

    /// A per-mile rate as a rate in the player's own distance unit.
    pub fn per_distance(&self, per_mile: f64) -> f64 {
        if self.imperial_units {
            per_mile
        } else {
            per_mile / MILES_TO_KM
        }
    }
}
