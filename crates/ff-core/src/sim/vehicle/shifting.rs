//! `TruckState.auto_shift`: the vehicle's half of the automatic gearbox --
//! everything the box is told about the road, the load and the retarder.

use super::{TruckState, G, JAKE_RPM_FLOOR, JAKE_STAGES};
use crate::sim::transmission::{
    AutoUpdateArgs, AUTO_DOWNSHIFT_RPM, DOWNSHIFT_TIME, PROGRESSIVE_UPSHIFT_RPM,
};

/// Where an empty truck's skip-shift may land. At a 900 floor the skip almost
/// never cleared in the low range at moderate throttle (shift near 1400, land
/// near 780), so an empty truck still rattled up through every single gear a
/// second apart. Dropping it further for a bobtail just lands the skip in the
/// weak end of the torque curve and bogs away the weight advantage.
const SKIP_LANDING_EMPTY_RPM: f64 = 780.0;

impl TruckState {
    /// Run automatic shifting from road-speed-coupled RPM (immune to the
    /// free-revving RPM spike during a shift's torque interruption).
    pub fn auto_shift(&mut self) -> Option<i32> {
        let tr = &self.transmission;
        if !tr.automatic || !self.engine_on {
            return None;
        }
        let mut rpm_est = if !tr.in_neutral() {
            self.coupled_rpm(None)
        } else {
            self.rpm
        };
        rpm_est = rpm_est.max(self.specs.idle_rpm * (0.5 + 0.5 * self.throttle));
        let braking = self.brake > 0.01 || self.emergency_brake || self.air_brakes_holding();
        let mut jaking = self.engine_brake_stage > 0 && self.engine_on && self.throttle <= 0.05;
        // The jake switch armed on a downgrade is grade-hold mode: a throttle
        // blip to keep the target speed must not release the hold and grab a
        // taller gear that guts the retarder mid-descent.
        jaking = jaking || (self.engine_brake() && self.engine_on && self.grade < -0.01);
        // Descent control holding the grade holds the gear the way a brake
        // application does: a downgrade is no place for an economy upshift.
        // Not as the retarder does, though: with no stage on, its pre-select
        // walked a bobtail down from eighth to sixth on Red Mountain's 8.4
        // percent, into a gear the truck then rode at the rev ceiling (bend
        // sweep, 2026-09-25). The engine still upshifts past the ceiling.
        let descent_hold = self.descent_gear_hold && self.engine_on && self.throttle <= 0.05;
        let bobtail = !self.trailer_attached;
        let load_fraction = self.load_fraction();
        let base_interval = if bobtail { 1.1 } else { 1.25 };
        let minimum_shift_interval_s = if braking {
            1.75
        } else {
            base_interval + 0.55 * load_fraction
        };
        let mut start_gear = if self.grade >= 0.02 || load_fraction >= 0.75 {
            1
        } else {
            2
        };
        if load_fraction <= 0.2 && self.grade <= 0.01 {
            start_gear = 3;
        }
        let current_gear = tr.gear.max(1);
        let progressive = PROGRESSIVE_UPSHIFT_RPM[(current_gear - 1) as usize];
        let load_raise = 150.0 * load_fraction;
        let grade_raise = (self.grade.max(0.0) * 3000.0).min(200.0);
        // Under power, hold the LOW gears toward peak power before upshifting.
        // Without this the box short-shifted at ~1000 rpm even at full throttle,
        // so an empty truck on the flat banged up through the gears without ever
        // revving -- "shifting way too fast, not natural". The boost is largest in
        // 1st and fades out by the cruise gears (gone by 8th), so the truck still
        // reaches top gear and a calm RPM at highway speed. Light throttle keeps
        // the low, economy-minded progression, so gentle driving still upshifts
        // early and quietly.
        let launch_taper = (1.0 - (current_gear - 1) as f64 / 7.0).max(0.0);
        let throttle_raise = 600.0 * (self.throttle - 0.15).max(0.0) * launch_taper;
        let upshift_rpm =
            (self.specs.max_rpm * 0.9).min(progressive + load_raise + grade_raise + throttle_raise);
        let mut upshift_steps = 1;
        if 0 < tr.gear && tr.gear < tr.num_gears() && self.grade <= 0.01 {
            // Real drivers skip gears when the load allows instead of
            // machine-gunning every hole in the box, and the lighter the load
            // the more they skip. Gating the skip on "empty" alone left a
            // quarter-loaded truck shifting through all ten gears (player
            // report, 2026-10-13). The skip now has to land where the engine
            // can pull what is behind it: just off idle when empty, rising
            // with the load to the torque peak, which a full load never
            // reaches from the gear below, so a grossed-out truck still takes
            // every gear. The skip gear must also out-pull the road.
            let skip_gear = tr.num_gears().min(tr.gear + 2);
            let landing_rpm = self.coupled_rpm(Some(skip_gear));
            let landing_floor = SKIP_LANDING_EMPTY_RPM
                + (self.specs.peak_torque_rpm - SKIP_LANDING_EMPTY_RPM) * load_fraction;
            if skip_gear > tr.gear + 1 && landing_rpm >= landing_floor {
                let skip_force = (self.torque_at(landing_rpm)
                    * self.throttle
                    * self.health_factor()
                    * self.engine_wear_factor()
                    * tr.ratio_for(skip_gear).abs()
                    * self.specs.driveline_efficiency
                    / self.specs.wheel_radius_m)
                    .min(self.drive_traction_limit());
                if skip_force >= self.resistance_force() * 1.05 {
                    upshift_steps = 2;
                }
            }
        }
        let mut can_upshift = true;
        let target_gear = tr.num_gears().min(tr.gear.max(1) + upshift_steps);
        if 0 < tr.gear && tr.gear < tr.num_gears() && self.throttle > 0.5 && self.grade > 0.02 {
            let next_gear = target_gear;
            // The shift itself costs a torque interruption, and on
            // a grade gravity bleeds real speed through it -- so judge the new
            // gear at the speed the truck will actually have when the clutch
            // comes back, not the speed it has now. At crawl speeds on a steep
            // pull that keeps the box in the low gear all the way to the
            // governor instead of hunting across a boundary it cannot hold.
            let shift_loss_mps = G * self.grade.max(0.0) * DOWNSHIFT_TIME;
            let v = self.velocity_mps.abs().max(0.1);
            let landing_frac = ((v - shift_loss_mps) / v).max(0.1);
            let next_rpm =
                (self.coupled_rpm(Some(next_gear)) * landing_frac).max(self.specs.idle_rpm);
            let next_ratio = tr.ratio_for(next_gear).abs();
            let next_torque = self.torque_at(next_rpm) * self.throttle * self.health_factor();
            let mut next_force = next_torque * next_ratio * self.specs.driveline_efficiency
                / self.specs.wheel_radius_m;
            next_force = next_force.min(self.drive_traction_limit());
            // Do not grab a taller gear that cannot pull the current road load.
            // This predicts the post-shift tractive force instead of repeatedly
            // shifting up, losing speed, and kicking straight back down.
            let minimum_post_shift_rpm = if self.grade >= 0.02 { 1050.0 } else { 900.0 };
            can_upshift =
                next_rpm >= minimum_post_shift_rpm && next_force >= self.resistance_force() * 1.05;
        }
        let mut downshift_target = None;
        if braking && tr.gear > 1 {
            let candidates: Vec<i32> = (1..tr.gear)
                .filter(|&gear| (1050.0..=1700.0).contains(&self.coupled_rpm(Some(gear))))
                .collect();
            if let Some(best) = candidates.iter().max() {
                downshift_target = Some(*best);
            }
        }
        // The lug guard scales with the load. Grossed out, falling under 1050
        // under power really is lugging and earns the downshift. Empty, the
        // engine pulls up happily from 800 -- holding the loaded threshold
        // bounced every skip-shift straight back down a gear, and the launch
        // churned through torque interruptions instead of accelerating.
        let mut downshift_rpm = AUTO_DOWNSHIFT_RPM - 300.0 * (1.0 - load_fraction);
        // Pulling away in the starting gear, the engine runs under the lug
        // guard until the truck gathers speed. That is the launch, not
        // lugging: the gear was picked for this load and grade, and the
        // engine is still out-pulling the road. Without this a light truck
        // pulling away gently in third dropped to second at a walking pace,
        // then had to shift back up past it. Once the road wins -- a part
        // load on a rise at part throttle -- the revs are falling and the
        // guard applies again.
        let launching = tr.gear <= start_gear
            && self.throttle > 0.05
            && self.drive_force() > self.resistance_force();
        if launching {
            downshift_rpm = 0.0;
        }
        // The pull downshift. A real automated box does not sit at full
        // throttle watching the hill win: pedal on the floor, road going up,
        // truck still losing ground, and it goes hunting for torque even
        // though the revs are nowhere near lugging. Without it the box held
        // top gear while automatic speed control floored the accelerator, and
        // a loaded truck bled twenty mph up a 4 percent grade with a lower
        // hole sitting right there (bench trace, 2026-07-25: 62 set, 31.5 mph
        // low; the same pull now holds a gear that can actually turn it).
        // Only taken when the next gear down genuinely makes more wheel force
        // than the one it is in, so the box walks down the pull and stops
        // rather than hunting past the torque peak.
        if self.throttle >= 0.9
            && self.grade > 0.01
            && tr.gear > 1
            && !tr.in_neutral()
            && self.drive_force() < self.resistance_force()
        {
            let lower_gear = tr.gear - 1;
            let lower_gear_rpm = self.coupled_rpm(Some(lower_gear));
            if lower_gear_rpm <= self.specs.max_rpm * 0.95 {
                let lower_gear_force = (self.torque_at(lower_gear_rpm)
                    * self.throttle
                    * self.health_factor()
                    * self.engine_wear_factor()
                    * tr.ratio_for(lower_gear).abs()
                    * self.specs.driveline_efficiency
                    / self.specs.wheel_radius_m)
                    .min(self.drive_traction_limit());
                if lower_gear_force > self.drive_force() * 1.02 {
                    downshift_rpm = downshift_rpm.max(rpm_est + 1.0);
                }
            }
        }
        // Traction-linked retarder management: refuse a jake pre-select into a
        // gear whose retard demand would break the drive axle loose (predicted
        // the same way the upshift path predicts tractive force). Without
        // this, on glare ice the box lands one gear too deep and grinds the
        // cap for the whole descent -- real automated retarders are slip-
        // gated for exactly this reason.
        let mut retarder_slipping = self.jake_slipping();
        if jaking && !retarder_slipping && tr.gear > 1 && !tr.in_neutral() {
            let s = &self.specs;
            let lower_rpm = s.max_rpm.min(self.coupled_rpm(Some(tr.gear - 1)));
            let rpm_frac = (lower_rpm / s.max_rpm).clamp(0.0, 1.0);
            let stage = JAKE_STAGES.min(self.engine_brake_stage) as f64 / JAKE_STAGES as f64;
            let lower_torque = s.engine_brake_torque_nm
                * stage
                * (JAKE_RPM_FLOOR + (1.0 - JAKE_RPM_FLOOR) * rpm_frac);
            let lower_demand =
                lower_torque * tr.ratio_for(tr.gear - 1).abs() * s.driveline_efficiency
                    / s.wheel_radius_m;
            if lower_demand > self.jake_traction_cap() {
                retarder_slipping = true;
            }
        }
        let args = AutoUpdateArgs {
            rpm: rpm_est,
            throttle: self.throttle,
            moving: self.velocity_mps > 0.5,
            braking: braking || descent_hold,
            can_upshift,
            minimum_shift_interval_s,
            upshift_rpm,
            start_gear,
            upshift_steps,
            downshift_target,
            engine_braking: jaking,
            downshift_rpm,
            retarder_slipping,
        };
        self.transmission.auto_update(args)
    }
}
