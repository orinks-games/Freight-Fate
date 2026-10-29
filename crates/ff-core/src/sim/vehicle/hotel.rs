//! Reefer TRU and APU hotel power (2.0 v1).
//!
//! Coarse cargo temperature and low diesel burns from the tractor tank.
//! Not a thermal model, not a separate TRU tank, and not a noise/HOS sim.

use super::TruckState;

/// Gallons per game-second while the reefer TRU runs (~0.5× normal idle).
pub const REEFER_BURN_GAL_PER_S: f64 = 0.00011;
/// Gallons per game-second while the APU runs (~0.25× normal idle).
pub const APU_BURN_GAL_PER_S: f64 = 0.000055;

/// Coarse cold-setpoint for reefer freight (°C). Honesty: not cargo-specific.
pub const REEFER_SETPOINT_C: f64 = 2.0;
/// Acceptable band around the setpoint (°C).
pub const REEFER_BAND_C: f64 = 4.0;

/// Approach rate toward setpoint while the TRU is on (fraction of gap per game-second).
/// Slowed with drift so a warm box cools over hours, not minutes.
pub const REEFER_HOLD_RATE: f64 = 3.3e-5;
/// Approach rate toward ambient while the TRU is off.
/// Tuned so 2 C -> 6 C in ~30 C ambient takes about 3.5 game hours.
pub const REEFER_DRIFT_RATE: f64 = 1.2e-5;

/// Game minutes cargo may sit outside the band before spoilage starts.
pub const REEFER_SPOIL_GRACE_MIN: f64 = 45.0;
/// Cargo-damage percent per game-minute once past the grace window (× fragility).
const REEFER_SPOIL_PCT_PER_MIN: f64 = 0.08;

/// One-shot cues the driving/speech layer reads after a hotel advance.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct HotelEvents {
    /// Reefer was running and the tractor tank ran dry.
    pub reefer_starved: bool,
    /// APU was running and the tractor tank ran dry.
    pub apu_starved: bool,
    /// Spoilage damage was applied this step.
    pub spoil_tick: bool,
}

impl TruckState {
    /// Whether this truck currently carries freight that needs the reefer.
    pub fn cargo_needs_reefer(&self) -> bool {
        self.cargo_needs_reefer
    }

    pub fn set_cargo_needs_reefer(&mut self, needs: bool) {
        let was = self.cargo_needs_reefer;
        self.cargo_needs_reefer = needs;
        if needs && !was {
            // Fresh reefer load: always tendered at the coarse setpoint, and
            // the TRU auto-starts. Alt+R is manual after that.
            self.cargo_temp_c = REEFER_SETPOINT_C;
            self.reefer_out_of_range_min = 0.0;
            let _ = self.start_reefer();
        }
        if !needs {
            self.reefer_on = false;
            self.reefer_out_of_range_min = 0.0;
        }
    }

    /// Turn the reefer TRU on. Returns false when there is no reefer load.
    pub fn start_reefer(&mut self) -> bool {
        if !self.cargo_needs_reefer || !self.trailer_attached || self.cargo_kg <= 0.0 {
            return false;
        }
        if self.fuel_gal <= 0.0 {
            return false;
        }
        self.reefer_on = true;
        true
    }

    pub fn stop_reefer(&mut self) {
        self.reefer_on = false;
    }

    pub fn start_apu(&mut self) -> bool {
        if self.fuel_gal <= 0.0 {
            return false;
        }
        self.apu_on = true;
        true
    }

    pub fn stop_apu(&mut self) {
        self.apu_on = false;
    }

    /// Burn hotel diesel and advance coarse cargo temp.
    ///
    /// `burn_seconds` follows the fuel clock (`dt * fuel_burn_mult` on the
    /// frame loop). `thermo_seconds` is the cargo/spoil clock and must not
    /// use `fuel_burn_mult` -- rest advances pass the same game-seconds for
    /// both.
    pub fn advance_hotel_power(&mut self, burn_seconds: f64, thermo_seconds: f64) -> HotelEvents {
        let mut events = HotelEvents::default();
        if burn_seconds <= 0.0 && thermo_seconds <= 0.0 {
            return events;
        }

        let mut burn = 0.0;
        if burn_seconds > 0.0 {
            if self.reefer_on {
                burn += REEFER_BURN_GAL_PER_S * burn_seconds;
            }
            if self.apu_on {
                burn += APU_BURN_GAL_PER_S * burn_seconds;
            }
        }
        if burn > 0.0 {
            let had_reefer = self.reefer_on;
            let had_apu = self.apu_on;
            let took = self.fuel_gal.min(burn);
            self.fuel_gal -= took;
            if self.fuel_gal <= 0.0 {
                self.fuel_gal = 0.0;
                if had_reefer {
                    self.reefer_on = false;
                    events.reefer_starved = true;
                    self.reefer_just_starved = true;
                }
                if had_apu {
                    self.apu_on = false;
                    events.apu_starved = true;
                }
                // Tractor engine also dies when the tank is empty.
                if self.engine_on {
                    self.stop_engine();
                }
            }
        }

        if thermo_seconds > 0.0
            && self.cargo_needs_reefer
            && self.trailer_attached
            && self.cargo_kg > 0.0
        {
            let target = if self.reefer_on {
                REEFER_SETPOINT_C
            } else {
                self.ambient_temp_c
            };
            let rate = if self.reefer_on {
                REEFER_HOLD_RATE
            } else {
                REEFER_DRIFT_RATE
            };
            let gap = target - self.cargo_temp_c;
            self.cargo_temp_c += gap * (1.0 - (-rate * thermo_seconds).exp());

            let in_band = (self.cargo_temp_c - REEFER_SETPOINT_C).abs() <= REEFER_BAND_C;
            let minutes = thermo_seconds / 60.0;
            if in_band {
                self.reefer_out_of_range_min = 0.0;
            } else {
                self.reefer_out_of_range_min += minutes;
                if self.reefer_out_of_range_min > REEFER_SPOIL_GRACE_MIN {
                    let over = self.reefer_out_of_range_min - REEFER_SPOIL_GRACE_MIN;
                    // Only the minutes in this step past grace, not the whole overage.
                    let spoil_minutes = minutes.min(over);
                    let dmg = REEFER_SPOIL_PCT_PER_MIN * spoil_minutes * self.cargo_fragility;
                    if self.add_cargo_damage(dmg) > 0.0 {
                        events.spoil_tick = true;
                    }
                }
            }
        }

        events
    }

    /// True when cargo temp is outside the coarse reefer band.
    pub fn reefer_temp_out_of_range(&self) -> bool {
        self.cargo_needs_reefer && (self.cargo_temp_c - REEFER_SETPOINT_C).abs() > REEFER_BAND_C
    }

    /// Spoken cargo-temp readout for reefer loads.
    pub fn reefer_temp_status_text(&self, imperial: bool) -> String {
        if !self.cargo_needs_reefer {
            return String::new();
        }
        format!(
            "Cargo temperature {}",
            spoken_cargo_temp_degrees(self.cargo_temp_c, imperial)
        )
    }

    /// Cold-pickup / departure line while the TRU is already running.
    pub fn reefer_running_announcement(&self, imperial: bool) -> String {
        reefer_running_announcement(self.cargo_temp_c, imperial)
    }
}

/// Spoken magnitude for a cargo temperature in the player's unit setting.
///
/// Imperial: `"36 degrees"` (Fahrenheit, no unit word — same shape as weather).
/// Metric: `"2 degrees Celsius"`.
pub fn spoken_cargo_temp_degrees(temp_c: f64, imperial: bool) -> String {
    if imperial {
        let f = (temp_c * 9.0 / 5.0 + 32.0).round();
        format!("{f:.0} degrees")
    } else {
        format!("{:.0} degrees Celsius", temp_c.round())
    }
}

/// `"Reefer running at 36 degrees."` / `"Reefer running at 2 degrees Celsius."`
pub fn reefer_running_announcement(temp_c: f64, imperial: bool) -> String {
    format!(
        "Reefer running at {}.",
        spoken_cargo_temp_degrees(temp_c, imperial)
    )
}

/// Whether a cargo class needs the reefer TRU.
pub fn cargo_needs_reefer_key(key: &str) -> bool {
    key == "food" || key == "refrigerated"
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::vehicle::{TruckSpecs, AMBIENT_C};

    fn truck() -> TruckState {
        let mut t = TruckState::new(TruckSpecs::default());
        t.fuel_gal = 100.0;
        t.ambient_temp_c = 25.0;
        t.cargo_temp_c = 25.0;
        t.cargo_kg = 10_000.0;
        t.trailer_attached = true;
        t.cargo_fragility = 2.4;
        t.set_cargo_needs_reefer(true);
        t
    }

    #[test]
    fn reefer_burns_only_when_on() {
        let mut t = truck();
        t.stop_reefer();
        let before = t.fuel_gal;
        t.advance_hotel_power(3600.0, 3600.0);
        assert!((t.fuel_gal - before).abs() < 1e-9);

        t.reefer_on = true;
        t.advance_hotel_power(3600.0, 3600.0);
        let burned = before - t.fuel_gal;
        assert!(burned > 0.3 && burned < 0.6, "burned {burned}");
    }

    #[test]
    fn temp_holds_when_on_and_drifts_when_off() {
        let mut t = truck();
        // Auto-start left it on at setpoint; warm the box then hold.
        t.cargo_temp_c = 25.0;
        t.reefer_on = true;
        t.advance_hotel_power(0.0, 3600.0 * 24.0);
        assert!(
            (t.cargo_temp_c - REEFER_SETPOINT_C).abs() < 1.5,
            "held at {}",
            t.cargo_temp_c
        );

        t.stop_reefer();
        t.cargo_temp_c = REEFER_SETPOINT_C;
        t.ambient_temp_c = 30.0;
        t.advance_hotel_power(0.0, 3600.0 * 3.5);
        assert!(
            t.cargo_temp_c > 5.0 && t.cargo_temp_c < 8.0,
            "should drift toward ambient over hours, got {}",
            t.cargo_temp_c
        );
    }

    #[test]
    fn drift_from_setpoint_to_band_edge_takes_about_three_to_four_hours() {
        // 2 C -> 6 C in 30 C ambient with the unit off.
        let mut t = truck();
        t.stop_reefer();
        t.cargo_temp_c = REEFER_SETPOINT_C;
        t.ambient_temp_c = 30.0;
        t.reefer_out_of_range_min = 0.0;

        let step = 60.0; // one game minute
        let mut elapsed = 0.0;
        let limit = 3600.0 * 5.0;
        while t.cargo_temp_c < 6.0 && elapsed < limit {
            t.advance_hotel_power(0.0, step);
            elapsed += step;
        }
        let hours = elapsed / 3600.0;
        assert!(
            (3.0..4.0).contains(&hours),
            "2 C to 6 C took {hours:.2} game hours (want 3-4); temp={}",
            t.cargo_temp_c
        );
    }

    #[test]
    fn spoilage_only_for_reefer_freight_past_threshold() {
        let mut reefer = truck();
        reefer.stop_reefer();
        reefer.ambient_temp_c = 30.0;
        reefer.cargo_temp_c = 30.0;
        reefer.advance_hotel_power(0.0, 60.0 * 90.0);
        assert!(
            reefer.cargo_damage_pct > 0.0,
            "reefer should spoil, got {}",
            reefer.cargo_damage_pct
        );

        let mut dry = truck();
        dry.set_cargo_needs_reefer(false);
        dry.cargo_damage_pct = 0.0;
        dry.reefer_on = false;
        dry.ambient_temp_c = 30.0;
        dry.cargo_temp_c = 30.0;
        dry.advance_hotel_power(0.0, 60.0 * 90.0);
        assert_eq!(dry.cargo_damage_pct, 0.0);
    }

    #[test]
    fn spoil_timing_ignores_fuel_burn_mult() {
        let mut low = truck();
        low.stop_reefer();
        low.ambient_temp_c = 30.0;
        low.cargo_temp_c = 30.0;
        low.fuel_burn_mult = 1.0;
        low.engine_on = false;

        let mut high = truck();
        high.stop_reefer();
        high.ambient_temp_c = 30.0;
        high.cargo_temp_c = 30.0;
        high.fuel_burn_mult = 100.0;
        high.engine_on = false;

        // Same wall-clock updates: thermo must match even when burn mult differs.
        for _ in 0..3_600 {
            low.update(1.0);
            high.update(1.0);
        }
        assert!(
            (low.reefer_out_of_range_min - high.reefer_out_of_range_min).abs() < 1e-6,
            "low={} high={}",
            low.reefer_out_of_range_min,
            high.reefer_out_of_range_min
        );
        assert_eq!(low.cargo_damage_pct > 0.0, high.cargo_damage_pct > 0.0);
    }

    #[test]
    fn apu_burns_low_and_only_when_on() {
        let mut t = truck();
        t.set_cargo_needs_reefer(false);
        t.apu_on = false;
        let before = t.fuel_gal;
        t.advance_hotel_power(3600.0, 3600.0);
        assert!((t.fuel_gal - before).abs() < 1e-9);

        t.apu_on = true;
        t.advance_hotel_power(3600.0, 3600.0);
        let burned = before - t.fuel_gal;
        assert!(burned > 0.15 && burned < 0.3, "APU burned {burned}");

        let mut r = truck();
        r.reefer_on = true;
        r.apu_on = false;
        let reefer_before = r.fuel_gal;
        r.advance_hotel_power(3600.0, 3600.0);
        let reefer_burned = reefer_before - r.fuel_gal;
        assert!(reefer_burned > burned);
    }

    #[test]
    fn old_truck_defaults_load_without_hotel_fields() {
        let t = TruckState::new(TruckSpecs::default());
        assert!(!t.reefer_on);
        assert!(!t.apu_on);
        assert!(!t.cargo_needs_reefer);
        assert_eq!(t.reefer_out_of_range_min, 0.0);
        assert_eq!(t.cargo_temp_c, AMBIENT_C);
        assert_eq!(t.ambient_temp_c, AMBIENT_C);
    }

    #[test]
    fn reefer_starves_when_tank_empty() {
        let mut t = truck();
        t.fuel_gal = 0.05;
        t.reefer_on = true;
        let ev = t.advance_hotel_power(3600.0, 3600.0);
        assert!(ev.reefer_starved);
        assert!(!t.reefer_on);
        assert_eq!(t.fuel_gal, 0.0);
    }

    #[test]
    fn fresh_reefer_load_starts_at_setpoint_with_unit_on() {
        let mut t = TruckState::new(TruckSpecs::default());
        t.fuel_gal = 50.0;
        t.ambient_temp_c = 30.0;
        t.cargo_temp_c = 30.0;
        t.cargo_kg = 10_000.0;
        t.trailer_attached = true;
        t.set_cargo_needs_reefer(true);
        assert!(
            (t.cargo_temp_c - REEFER_SETPOINT_C).abs() < 1e-9,
            "temp {}",
            t.cargo_temp_c
        );
        assert!(t.reefer_on, "TRU should auto-start on cold pickup");
    }

    #[test]
    fn empty_tank_does_not_auto_start_reefer_on_fresh_load() {
        let mut t = TruckState::new(TruckSpecs::default());
        t.fuel_gal = 0.0;
        t.ambient_temp_c = 30.0;
        t.cargo_temp_c = 30.0;
        t.cargo_kg = 10_000.0;
        t.trailer_attached = true;
        t.set_cargo_needs_reefer(true);
        assert!(
            (t.cargo_temp_c - REEFER_SETPOINT_C).abs() < 1e-9,
            "temp still snaps to setpoint"
        );
        assert!(
            !t.reefer_on,
            "empty tank must not leave the TRU running after auto-start"
        );
    }

    #[test]
    fn reefer_running_announcement_respects_units() {
        assert_eq!(
            reefer_running_announcement(REEFER_SETPOINT_C, true),
            "Reefer running at 36 degrees."
        );
        assert_eq!(
            reefer_running_announcement(REEFER_SETPOINT_C, false),
            "Reefer running at 2 degrees Celsius."
        );
        // Live cargo temp (slightly warm of setpoint) rounds in each unit.
        assert_eq!(
            reefer_running_announcement(3.0, true),
            "Reefer running at 37 degrees."
        );
        assert_eq!(
            reefer_running_announcement(3.0, false),
            "Reefer running at 3 degrees Celsius."
        );
    }

    #[test]
    fn reefer_temp_status_text_respects_units() {
        let mut t = truck();
        t.cargo_temp_c = REEFER_SETPOINT_C;
        assert_eq!(
            t.reefer_temp_status_text(true),
            "Cargo temperature 36 degrees"
        );
        assert_eq!(
            t.reefer_temp_status_text(false),
            "Cargo temperature 2 degrees Celsius"
        );
    }
}
