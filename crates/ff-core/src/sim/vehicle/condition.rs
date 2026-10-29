//! `TruckState` condition and recovery: damage, the freight's condition,
//! refuelling, roadside repair, and the driveline guards on a manual shift.

use super::{
    TruckState, CARGO_ADVISORY_LAT_G, CARGO_COLLISION_PCT, DAMAGE_MAX_PCT, G, MPS_TO_MPH, M_PER_FT,
    REVERSE_CRASH_DAMAGE_PCT, REVERSE_ENGAGE_MAX_MPH,
};
use crate::sim::transmission::{ShiftResult, REVERSE};

impl TruckState {
    /// True after a forced over-rev, such as an unsafe downshift.
    ///
    /// Reaching the normal fuel governor under power is safe; damage begins
    /// only when road speed mechanically drives the engine beyond its limit.
    pub fn over_revving(&self) -> bool {
        self.engine_on
            && self.transmission.drive_ratio() != 0.0
            && self.coupled_rpm(None) > self.specs.max_rpm * 1.05
    }

    /// Fill the tank (or add `gallons`); returns gallons added.
    pub fn refuel(&mut self, gallons: Option<f64>) -> f64 {
        let space = self.specs.fuel_tank_gal - self.fuel_gal;
        let added = match gallons {
            None => space,
            Some(g) => space.min(g.max(0.0)),
        };
        self.fuel_gal += added;
        added
    }

    /// Leave a rescued truck safely stopped and ready for a normal restart.
    pub fn recover_from_fuel_depletion(&mut self) {
        self.stop_engine();
        self.velocity_mps = 0.0;
        self.rpm = 0.0;
        self.brake = 0.0;
        self.emergency_brake = false;
        self.set_engine_brake(false);
        self.parking_brake = true;
        self.transmission.reset_to_neutral();
    }

    /// Move the freight's condition meter; returns what was added.
    pub fn add_cargo_damage(&mut self, pct: f64) -> f64 {
        if !self.trailer_attached || self.cargo_kg <= 0.0 {
            return 0.0; // nothing on the fifth wheel to hurt
        }
        let added = (self.cargo_damage_pct + pct.max(0.0)).min(100.0) - self.cargo_damage_pct;
        self.cargo_damage_pct += added;
        added
    }

    /// What the bend the truck is in is pulling sideways, in g.
    ///
    /// Geometric, from the corner's own radius: a pallet does not know what
    /// the advisory sign said, only how hard it is being pushed against the
    /// trailer wall. Zero on a straight. Bends whose data carries no radius
    /// fall back to one implied by the advisory, so a gap in the map reads
    /// like its neighbours rather than like a straight road.
    pub fn corner_lateral_g(&self) -> f64 {
        let mut radius_m = self.corner_radius_ft * M_PER_FT;
        if radius_m <= 0.0 {
            let advisory_mps = self.corner_advisory_mph / MPS_TO_MPH;
            if advisory_mps <= 0.0 {
                return 0.0;
            }
            radius_m = advisory_mps * advisory_mps / (CARGO_ADVISORY_LAT_G * G);
        }
        let speed_mps = self.speed_mph() / MPS_TO_MPH;
        speed_mps * speed_mps / (radius_m * G)
    }

    /// Put incident damage on the truck; returns what was actually added.
    ///
    /// Every damage site goes through here so the preventable share is
    /// counted once, at the moment the cause is known. Preventable is the
    /// default because nearly everything that damages a truck is somebody's
    /// decision; the exceptions have to say so.
    pub fn add_damage(&mut self, pct: f64, preventable: bool) -> f64 {
        let added = (self.damage_pct + pct.max(0.0)).min(DAMAGE_MAX_PCT) - self.damage_pct;
        self.damage_pct += added;
        if preventable {
            self.preventable_damage_pct += added;
        }
        added
    }

    /// Roadside repair: patch the truck down to `damage_pct` and leave it
    /// safely stopped, ungoverned, and ready for a normal restart.
    pub fn recover_from_breakdown(&mut self, damage_pct: f64) {
        self.damage_pct = self.damage_pct.min(damage_pct);
        self.speed_cap_mph = None;
        self.recover_from_fuel_depletion();
    }

    /// Manual gear selection, with the guards a real driveline enforces.
    ///
    /// The gearbox itself knows nothing about road speed, so the one rule it
    /// cannot enforce alone lives here: reverse while rolling forward is a
    /// crash of gears, not a shift. It is refused, and the attempt costs the
    /// driveline -- a real box would be short some teeth afterwards.
    pub fn request_gear(&mut self, target: i32) -> ShiftResult {
        if target == REVERSE
            && !self.transmission.automatic
            && self.speed_mph() > REVERSE_ENGAGE_MAX_MPH
        {
            self.add_damage(REVERSE_CRASH_DAMAGE_PCT, true);
            return ShiftResult {
                ok: false,
                message: "Reverse will not go in at this speed. Stop the truck first.".to_string(),
                grind: true,
            };
        }
        self.transmission.request_gear(target)
    }

    /// severity 0..1; slows the truck and adds damage.
    pub fn apply_collision(&mut self, severity: f64, preventable: bool) {
        self.velocity_mps *= (1.0 - severity).max(0.2);
        self.add_damage(severity * 18.0, preventable);
        // Whatever hit the tractor went through the freight as well.
        self.add_cargo_damage(severity * CARGO_COLLISION_PCT * self.cargo_fragility);
    }
}
