//! Fuel-inclusive operating weight. No extra state is stored in saves.

use super::{TruckState, KG_PER_LB, LEGAL_GVW_KG, REFERENCE_CARGO_KG, TRAILER_TARE_KG};

/// Assumed diesel density: 7.1 pounds per US gallon. This fixed calibration
/// omits changes with fuel blend and temperature; it is not a measured value.
pub const DIESEL_KG_PER_GAL: f64 = 7.1 * KG_PER_LB;

impl TruckState {
    /// Tractor and attached empty trailer, excluding diesel.
    /// Rated mass is calibrated at reference cargo and a full tank, so its
    /// fuel allowance is removed before the remaining fuel is added back.
    pub fn dry_tare_kg(&self) -> f64 {
        let combination = (self.specs.mass_kg
            - REFERENCE_CARGO_KG
            - self.specs.fuel_tank_gal.max(0.0) * DIESEL_KG_PER_GAL)
            .max(0.0);
        if self.trailer_attached {
            combination
        } else {
            (combination - TRAILER_TARE_KG).max(0.0)
        }
    }

    /// Diesel mass for a tank reading, capped at this truck's tank capacity.
    pub fn fuel_mass_for_gallons_kg(&self, fuel_gal: f64) -> f64 {
        fuel_gal.clamp(0.0, self.specs.fuel_tank_gal.max(0.0)) * DIESEL_KG_PER_GAL
    }

    pub fn fuel_mass_kg(&self) -> f64 {
        self.fuel_mass_for_gallons_kg(self.fuel_gal)
    }

    /// How full the trailer is, 0 to 1, for the models that care about the
    /// load rather than the gross weight: launch traction, shifting, and the
    /// corner speed a street turn is taken at.
    pub fn load_fraction(&self) -> f64 {
        (self.cargo_kg / REFERENCE_CARGO_KG).clamp(0.0, 1.0)
    }

    /// The load fraction the roll models use.
    ///
    /// A tank with anything at all in it is priced as full. In a steady bend
    /// a part-filled tank is no steadier than a full one: the liquid runs to
    /// the outside of the turn and gives back what its lower weight would
    /// have bought (NTSB HAR-11/01 2.3.4, READ for 80 percent against 100).
    /// What makes it worse is the wave, which `vehicle/roll.rs` prices on top.
    pub fn roll_load_fraction(&self) -> f64 {
        if self.liquid.is_some() && self.cargo_kg > 0.0 {
            1.0
        } else {
            self.load_fraction()
        }
    }

    /// Remaining legal gross-weight capacity; negative means overweight.
    pub fn gross_weight_margin_kg(&self) -> f64 {
        LEGAL_GVW_KG - self.gross_mass_kg()
    }

    /// Legal capacity after replacing the current payload with `cargo_kg`.
    pub fn gross_weight_margin_with_cargo_kg(&self, cargo_kg: f64) -> f64 {
        LEGAL_GVW_KG - (self.tare_kg() + cargo_kg.max(0.0))
    }

    /// Gross weight after changing only the fuel quantity.
    pub fn gross_mass_after_fuel_kg(&self, fuel_gal: f64) -> f64 {
        self.dry_tare_kg() + self.fuel_mass_for_gallons_kg(fuel_gal) + self.cargo_kg.max(0.0)
    }

    /// Legal capacity after changing only the fuel quantity.
    pub fn gross_weight_margin_after_fuel_kg(&self, fuel_gal: f64) -> f64 {
        LEGAL_GVW_KG - self.gross_mass_after_fuel_kg(fuel_gal)
    }
}
