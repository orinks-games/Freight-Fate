//! Speaking speeds and distances in the player's units.

use super::{distance_text_for, short_distance_text_for, Settings};
use crate::pyfmt::{fmt_f, fmt_grouped, round_py_int};
use crate::units::{
    distance_unit, effective, hud_speed, spoken_distance, spoken_gap, to_distance, Units,
    UnitsEvent, UnitsMode, MILES_TO_KM,
};

const LITRES_PER_US_GALLON: f64 = crate::models::economy::LITRES_PER_US_GALLON;
const KG_PER_LB: f64 = 0.45359237;
const TONNES_PER_SHORT_TON: f64 = 0.90718474;

impl Settings {
    /// The player's choice on the Units row.
    pub fn units_mode(&self) -> UnitsMode {
        UnitsMode::from_key(&self.units).unwrap_or_default()
    }

    pub fn set_units_mode(&mut self, mode: UnitsMode) {
        self.units = mode.key().to_string();
    }

    /// An unreadable value on the row falls to Automatic, the default.
    pub(crate) fn normalize_units(&mut self) {
        self.set_units_mode(self.units_mode());
    }

    /// The units every readout uses right now: the row's choice, and for
    /// Automatic the country the truck or the screen is in.
    pub fn units(&self) -> Units {
        effective(self.units_mode(), self.units_place.country())
    }

    /// `units()` as the miles-or-not flag the readouts take.
    pub fn imperial(&self) -> bool {
        self.units().is_imperial()
    }

    /// Move the units to a new place. Returns the line a drive owes the
    /// driver when Automatic changed the units under them.
    pub fn move_units(&mut self, event: UnitsEvent) -> Option<&'static str> {
        let mode = self.units_mode();
        self.units_place.next(mode, event)
    }

    pub fn speed_text(&self, mph: f64) -> String {
        if self.imperial() {
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
        let value = if self.imperial() {
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
        distance_text_for(miles, self.imperial(), precise)
    }

    /// Colloquial short range for pacenote-style calls: quarter-mile steps
    /// under a mile ("half a mile"), 100-meter steps under a kilometer ("400
    /// meters"), the normal precise form beyond.
    pub fn short_distance_text(&self, miles: f64) -> String {
        short_distance_text_for(miles, self.imperial())
    }

    /// A spoken distance kept to one decimal, for close-range cues.
    pub fn gap_text(&self, miles: f64) -> String {
        spoken_gap(miles, self.imperial())
    }

    /// Speed for the visual HUD, in the short written form.
    pub fn hud_speed_text(&self, mph: f64) -> String {
        hud_speed(mph, self.imperial())
    }

    /// A bare converted distance, for readouts that name the unit once after
    /// two numbers ("12 of 400 miles").
    pub fn distance_value(&self, miles: f64, decimals: usize, grouped: bool) -> String {
        let value = to_distance(miles, self.imperial());
        if grouped {
            fmt_grouped(value, decimals)
        } else {
            fmt_f(value, decimals)
        }
    }

    /// The player's distance unit, to pair with `distance_value`.
    pub fn distance_unit_text(&self, plural: bool) -> &'static str {
        distance_unit(self.imperial(), plural)
    }

    /// A per-mile rate as a rate in the player's own distance unit.
    pub fn per_distance(&self, per_mile: f64) -> f64 {
        if self.imperial() {
            per_mile
        } else {
            per_mile / MILES_TO_KM
        }
    }

    /// A fuel volume in the player's units: whole gallons, or whole liters
    /// when the readouts are in kilometers. The tank is measured in gallons
    /// inside.
    pub fn volume_text(&self, gallons: f64) -> String {
        if self.imperial() {
            spoken_distance(gallons, "gallon")
        } else {
            spoken_distance(gallons * LITRES_PER_US_GALLON, "liter")
        }
    }

    /// The bare whole number of `volume_text`, for "75 of 150 gallons".
    pub fn volume_value(&self, gallons: f64) -> String {
        if self.imperial() {
            fmt_f(gallons, 0)
        } else {
            fmt_f(gallons * LITRES_PER_US_GALLON, 0)
        }
    }

    /// The volume unit's name, for "per gallon" and "a liter".
    pub fn volume_unit_text(&self) -> &'static str {
        if self.imperial() {
            "gallon"
        } else {
            "liter"
        }
    }

    /// A per-gallon price as a price for the player's volume unit.
    pub fn per_volume(&self, per_gallon: f64) -> f64 {
        if self.imperial() {
            per_gallon
        } else {
            per_gallon / LITRES_PER_US_GALLON
        }
    }

    /// A weight in the player's units, grouped: pounds, or kilograms when
    /// the readouts are in kilometers.
    pub fn weight_text(&self, kg: f64) -> String {
        if self.imperial() {
            format!("{} pounds", fmt_grouped((kg / KG_PER_LB).round(), 0))
        } else {
            format!("{} kilograms", fmt_grouped(kg.round(), 0))
        }
    }

    /// A load weight in US short tons, as tons or (in kilometers) metric
    /// tonnes, both whole.
    pub fn tons_text(&self, short_tons: f64) -> String {
        if self.imperial() {
            format!("{} tons", fmt_f(short_tons, 0))
        } else {
            format!("{} tonnes", fmt_f(short_tons * TONNES_PER_SHORT_TON, 0))
        }
    }

    /// A diesel price per volume unit, in the pump's currency: Canadian
    /// dollars at a Canadian pump, a liter or a gallon by the player's units.
    pub fn diesel_price_text(&self, usd_per_gal: f64, canadian: bool) -> String {
        let usd = self.per_volume(usd_per_gal);
        let (price, currency) = if canadian {
            (
                usd * crate::models::enforcement::CAD_PER_USD,
                "Canadian dollars",
            )
        } else {
            (usd, "dollars")
        };
        format!(
            "{} {currency} a {}",
            fmt_f(price, 2),
            self.volume_unit_text()
        )
    }
}
