//! `TruckState` per-frame integration: motion, rpm, fuel, temperatures,
//! wear, and the freight riding on the same accelerations.

use std::f64::consts::PI;

use super::{
    TruckState, AMBIENT_C, BRAKE_COOL_BASE_PER_S, BRAKE_COOL_SPEED_PER_S, BRAKE_WEAR_HOT_MULT,
    BRAKE_WEAR_PCT_PER_MJ, CARGO_BRAKE_PCT_PER_G_S, CARGO_CORNER_PCT_PER_G_S, CARGO_HARD_BRAKE_G,
    CHAIN_SAFE_MPH, CHAIN_SNAP_DAMAGE_PCT, CHAIN_WEAR_BARE_MULT, CHAIN_WEAR_OVERSPEED_MULT,
    CHAIN_WEAR_PCT_PER_MILE, ENGINE_MOTORING_DRAG_FRACTION, ENGINE_ROTATING_INERTIA_KG_M2,
    ENGINE_WEAR_FUEL_PENALTY, ENGINE_WEAR_LUG_PCT_PER_S, ENGINE_WEAR_OVER_REV_PCT_PER_S,
    ENGINE_WEAR_PCT_PER_H_FULL_LOAD, ENGINE_WEAR_PCT_PER_H_IDLE, G, LUG_RPM_FRACTION, LUG_THROTTLE,
    MAX_REVERSE_MPS, OVER_REV_RPM_MULT, ROAD_OVERSPEED_RPM_MULT, ROLL_WARN_SHARE,
    RUNAWAY_DAMAGE_PCT_PER_S, RUNAWAY_SPEED_MPH, SHIFT_SYNC_BRAKE_FRACTION,
    SHIFT_SYNC_FUEL_FRACTION, TIRE_WEAR_BRAKING_PCT, TIRE_WEAR_PCT_PER_MILE, TIRE_WINTER,
    WINTER_TREAD_WEAR_MULT,
};

impl TruckState {
    // -- per-frame update ---------------------------------------------------------

    pub fn update(&mut self, dt: f64) {
        self.transmission.update(dt);
        self.update_air_system(dt);

        // Four terms, not three. A liquid load pushes forward exactly when the
        // brakes are trying to stop -- it belongs here as its own force and not
        // inside resistance_force(), whose sign convention always opposes travel
        // and which also feeds the cruise controller's feed-forward.
        let surge = self.surge_force_n();
        let net = self.drive_force() - self.resistance_force() - self.brake_force() + surge;
        let accel = net / self.gross_mass_kg();
        let jake_decel = self.jake_brake_force() / self.gross_mass_kg();
        let old_v = self.velocity_mps;
        let mut new_v = self.velocity_mps + accel * dt;
        let drive_force = self.drive_force();
        if self.air_brakes_holding() && old_v.abs() < 0.05 && new_v.abs() < 0.05 {
            new_v = 0.0;
        }
        if (old_v > 0.0 && 0.0 > new_v && drive_force <= 0.0)
            || (old_v < 0.0 && 0.0 < new_v && drive_force >= 0.0)
        {
            new_v = 0.0;
        }
        if self.transmission.in_reverse() {
            new_v = new_v.max(-MAX_REVERSE_MPS);
        } else if new_v < 0.0 {
            new_v = 0.0;
        }
        self.velocity_mps = new_v;
        self.odometer_mi += self.velocity_mps.abs() * dt / 1609.344;

        // The freight rides on the same acceleration the tractor just felt --
        // and a liquid load rides it by running to one end of the tank.
        self.update_liquid(dt, accel);
        // Less the jake's share. A full service stop with the jake on is the
        // ordinary way a driver stops, and in a low gear the retarder adds a
        // fifth of a g on top of the drums: counted, it put a stop at a
        // street corner over the securement line ("the service brakes used
        // to damage your load for no reason", player report, 2026-10-14).
        // What the load still feels is the emergency application, a grade
        // adding its own g, and hitting something.
        let decel_g = if old_v > 0.0 {
            ((-accel - jake_decel) / G).max(0.0)
        } else {
            0.0
        };
        self.update_cargo(dt, decel_g);
        self.update_rpm(dt);
        self.update_fuel(dt);
        self.update_temps(dt);
        self.update_wear(dt);
    }

    pub(crate) fn update_rpm(&mut self, dt: f64) {
        let s = self.specs.clone();
        // Latched high idle drops the instant its conditions break -- the
        // parking brake releasing is the real fast-idle cancel.
        if self.high_idle_rpm.is_some() && !self.high_idle_allowed() {
            self.high_idle_rpm = None;
        }
        if !self.engine_on {
            self.rpm = (self.rpm - 1500.0 * dt).max(0.0);
            return;
        }
        let tr = &self.transmission;
        let ratio = if !tr.in_neutral() {
            tr.ratio_for(tr.gear)
        } else {
            0.0
        };
        let coupled = ratio != 0.0 && tr.clutch <= 0.5 && !tr.shifting();
        // A manual downshift with the clutch held matches revs the way the
        // automatic does, so the engine follows the braking truck down
        // instead of dropping straight to idle (tester request, 2026-09-28).
        // The driver's own blip still free-revs, and a missed shift into a
        // gear the road would over-rev is never blipped past the governor.
        let manual_rev_match = !tr.automatic
            && tr.rev_match
            && !coupled
            && self.throttle <= 0.05
            && self.coupled_rpm(None) <= s.max_rpm;
        if ((tr.automatic && tr.shifting()) || manual_rev_match) && ratio != 0.0 {
            // Rev-match through the torque interrupt: head for the NEW
            // gear's synchronous speed in both directions, at the rate the
            // engine's spare torque over its inertia allows, and hold there
            // once reached so engagement is a clunk, not a jump. The old
            // target was the LOWER of current and road rpm, so a downshift
            // sat at the old note for its whole second and then leapt.
            let wheel_rps = self.velocity_mps.abs() / (2.0 * PI * s.wheel_radius_m);
            let road_rpm = wheel_rps * 60.0 * ratio.abs();
            let sync = s
                .idle_rpm
                .max(road_rpm.min(s.max_rpm * ROAD_OVERSPEED_RPM_MULT));
            let drag_nm = ENGINE_MOTORING_DRAG_FRACTION * s.max_torque_nm;
            let rpm_per_s = |net_torque_nm: f64| {
                net_torque_nm.max(0.0) / ENGINE_ROTATING_INERTIA_KG_M2 * 60.0 / (2.0 * PI)
            };
            if self.rpm < sync {
                // The blip: fueled up, unloaded, against its own friction.
                let fuel_nm =
                    SHIFT_SYNC_FUEL_FRACTION * self.torque_at(self.rpm).max(drag_nm * 2.0);
                let rate = rpm_per_s(fuel_nm - drag_nm);
                self.rpm = sync.min(self.rpm + rate * dt);
            } else {
                // The fall: throttle closed, the engine brake pulling it down.
                let brake_nm = SHIFT_SYNC_BRAKE_FRACTION * s.engine_brake_torque_nm;
                let rate = rpm_per_s(drag_nm + brake_nm);
                self.rpm = sync.max(self.rpm - rate * dt);
            }
            return;
        }
        if coupled {
            let wheel_rps = self.velocity_mps.abs() / (2.0 * PI * s.wheel_radius_m);
            let road_rpm = wheel_rps * 60.0 * ratio.abs();
            if road_rpm < s.idle_rpm {
                // Standing still with the parking brake holding the rig: the
                // driver is revving in place -- warming the engine, building
                // air, or (for a blind player) listening to confirm the engine
                // answers the throttle. Let it free-rev like an idle-in-neutral
                // instead of lugging against the held brake or stalling; the
                // brake, not the driveline, is what keeps the truck stopped.
                if self.parking_brake && self.velocity_mps.abs() < 0.1 {
                    let floor = self.idle_floor_rpm();
                    let target = floor.max(s.idle_rpm + (s.max_rpm - s.idle_rpm) * self.throttle);
                    self.rpm += (target - self.rpm) * (4.0 * dt).min(1.0);
                    return;
                }
                // Launch regime: in a low gear the clutch slips and the engine
                // holds idle-or-better. In a high gear the engine lugs.
                let gear = tr.gear;
                let automatic = tr.automatic;
                if gear >= 4 && road_rpm < s.idle_rpm * 0.5 {
                    let braking = self.brake > 0.01 || self.emergency_brake;
                    if !automatic {
                        // Braking to a stop in a tall gear holds idle rather
                        // than stalling, so a manual driver can gear down and
                        // brake for traffic at once (owner ruling,
                        // 2026-09-28). Off the brake it still stalls.
                        if !braking {
                            self.stall();
                            return;
                        }
                    } else if !braking {
                        // A real automatic kicks down rather than lugging to
                        // a stall while still rolling. The RPM-threshold
                        // downshift can be outrun by a hard deceleration
                        // during the shift delay, so force the drop here.
                        self.transmission.kickdown();
                    }
                }
                self.rpm = s
                    .idle_rpm
                    .max(s.idle_rpm + (s.max_rpm - s.idle_rpm) * self.throttle * 0.3);
            } else {
                // Road-driven: a downgrade can push past the governor, and
                // that overspeed (not governed running) is what wears the
                // engine. An automatic upshifts to protect itself first.
                self.rpm = (s.max_rpm * ROAD_OVERSPEED_RPM_MULT).min(road_rpm);
            }
        } else {
            let floor = self.idle_floor_rpm();
            let target = floor.max(s.idle_rpm + (s.max_rpm - s.idle_rpm) * self.throttle);
            self.rpm += (target - self.rpm) * (4.0 * dt).min(1.0);
        }
    }

    pub fn stall(&mut self) {
        self.engine_on = false;
        self.stalled = true;
        self.rpm = 0.0;
        self.air_compressor_active = false;
    }

    /// A tired engine burns more fuel for the power it still makes, and
    /// a damaged one working against its own derate burns more again.
    fn fuel_wear_penalty_mult(&self) -> f64 {
        let mult = 1.0 + ENGINE_WEAR_FUEL_PENALTY * self.engine_wear_pct / 100.0;
        mult * (1.0 + self.damage_fuel_penalty())
    }

    /// Gallons per game-second if idling right now: no wheel-power term,
    /// base rate scaled by rpm -- a parked truck revving high still burns
    /// real fuel -- with the same wear and damage penalties every gallon
    /// pays. ~0.8 gal/h at a normal idle rpm.
    pub fn idle_fuel_burn_rate(&self) -> f64 {
        let base = 0.00022 * (self.rpm / self.specs.idle_rpm).max(1.0);
        base * self.specs.fuel_burn_factor * self.fuel_wear_penalty_mult()
    }

    pub(crate) fn update_fuel(&mut self, dt: f64) {
        if !self.engine_on {
            return;
        }
        // ~0.8 gal/h at idle; load burn calibrated for ~6.5-7 mpg at 60 mph cruise
        let burn = if self.velocity_mps.abs() < 0.3 {
            // Standing still the wheel-power term is zero, so an unloaded rev
            // would burn nothing extra: scale the base burn with rpm instead.
            // High idle and parked revving cost real fuel; the moving-truck
            // calibration below is untouched.
            self.idle_fuel_burn_rate()
        } else {
            let power_kw = self.drive_force().abs() * self.velocity_mps.abs() / 1000.0;
            let mut burn = (0.00022 + power_kw * 1.5e-5) * self.specs.fuel_burn_factor;
            burn *= self.fuel_wear_penalty_mult();
            burn
        };
        self.fuel_gal = (self.fuel_gal - burn * dt * self.fuel_burn_mult).max(0.0);
        if self.fuel_gal <= 0.0 {
            self.stop_engine();
        }
    }

    /// Burn fuel at the idle-rate floor for a stretch of GAME time that
    /// never passed through the per-frame loop -- a scripted maneuver (a
    /// missed facility gate's loop-back), not a rest. ``fuel_burn_mult``
    /// already tracks the same real-to-game scale ``update_fuel`` applies
    /// every frame (``truck.fuel_burn_mult == trip.effective_time_scale``,
    /// set each ``Trip.update``), which is why that per-frame burn is
    /// already denominated in game-seconds and this can burn directly
    /// against ``game_seconds`` with no extra conversion. Returns gallons
    /// burned; a no-op with the engine off.
    pub fn burn_idle_fuel_over_game_time(&mut self, game_seconds: f64) -> f64 {
        if !self.engine_on || game_seconds <= 0.0 {
            return 0.0;
        }
        let burned = self.fuel_gal.min(self.idle_fuel_burn_rate() * game_seconds);
        self.fuel_gal -= burned;
        if self.fuel_gal <= 0.0 {
            self.stop_engine();
        }
        burned
    }

    pub(crate) fn update_temps(&mut self, dt: f64) {
        let s = &self.specs;
        let load = if self.engine_on {
            self.throttle * (self.rpm / s.max_rpm)
        } else {
            0.0
        };
        let target = 60.0
            + (if self.engine_on {
                28.0 + 45.0 * load
            } else {
                0.0
            });
        self.engine_temp_c += (target - self.engine_temp_c) * 0.03 * dt;

        let speed = self.velocity_mps.abs();
        // Real energy accounting: the power the shoes actually dissipate
        // (force times speed) soaks into the drums' thermal mass. Heavier
        // loads brake with more force and so heat faster; faded shoes grip
        // less and heat less, which is what keeps the model stable.
        let heating = self.service_brake_force() * speed / s.brake_thermal_mass_j_per_c;
        let cool_frac = BRAKE_COOL_BASE_PER_S + BRAKE_COOL_SPEED_PER_S * speed.sqrt();
        let cooling = (self.brake_temp_c - AMBIENT_C) * cool_frac;
        self.brake_temp_c = (self.brake_temp_c + (heating - cooling) * dt).max(AMBIENT_C);
    }

    pub(crate) fn update_wear(&mut self, dt: f64) {
        let s = self.specs.clone();
        // Distance- and energy-coupled wear scales with the trip's time
        // compression (fuel_burn_mult, same trick fuel uses) so a game mile
        // costs the same tread at any time scale.
        let sim_dt = dt * self.fuel_burn_mult;
        let speed = self.velocity_mps.abs();
        let load = self.gross_mass_kg() / s.mass_kg;
        let application = if self.emergency_brake || self.air_brakes_holding() {
            1.0
        } else {
            self.brake
        };

        let mut tire = speed * sim_dt / 1609.344 * TIRE_WEAR_PCT_PER_MILE * load;
        if speed > 0.01 {
            tire += application * speed * sim_dt * TIRE_WEAR_BRAKING_PCT;
        }
        if self.tire_type == TIRE_WINTER {
            tire *= WINTER_TREAD_WEAR_MULT;
        }
        self.tire_wear_pct = (self.tire_wear_pct + tire * self.tire_wear_buff_mult).min(100.0);

        // Chains wear by the mile, brutally faster on bare pavement or past
        // chain speed. A set ground to nothing lets a cross chain go: the
        // links flail into the fender and the whole set is scrap.
        if self.chains_on && speed > 0.01 {
            let mut rate = CHAIN_WEAR_PCT_PER_MILE;
            if self.surface != "snow" && self.surface != "ice" {
                rate *= CHAIN_WEAR_BARE_MULT;
            }
            if self.speed_mph() > CHAIN_SAFE_MPH {
                rate *= CHAIN_WEAR_OVERSPEED_MULT;
            }
            self.chain_wear_pct =
                (self.chain_wear_pct + rate * speed * sim_dt / 1609.344).min(100.0);
            if self.chain_wear_pct >= 100.0 {
                self.chains_on = false;
                self.chains_just_snapped = true;
                // Grinding a set to destruction on bare pavement is a choice.
                self.add_damage(CHAIN_SNAP_DAMAGE_PCT, true);
            }
        }

        // The service brakes wear with the energy they actually dissipate;
        // the jake dumps its share out the exhaust and costs the shoes nothing.
        let service_force = self.service_brake_force();
        if service_force > 0.0 && speed > 0.01 {
            let mut brake = service_force * speed * sim_dt / 1.0e6 * BRAKE_WEAR_PCT_PER_MJ;
            if self.brake_temp_c >= self.brake_fade_onset_c() {
                brake *= BRAKE_WEAR_HOT_MULT;
            }
            self.brake_wear_pct = (self.brake_wear_pct + brake).min(100.0);
        }

        // A runaway is mechanical destruction, not a speeding offence, and it
        // does not care whether the engine is running -- coasting out of gear
        // down a grade is the classic way to arrive here. Charged per real
        // second, like the other abuse terms, and scaled by how far past the
        // threshold the truck has gone.
        if self.speed_mph() > RUNAWAY_SPEED_MPH {
            let over = (self.speed_mph() - RUNAWAY_SPEED_MPH) / 10.0;
            self.add_damage(RUNAWAY_DAMAGE_PCT_PER_S * over * dt, true);
        }

        if self.engine_on {
            let duty = self.throttle * (self.rpm / s.max_rpm);
            let rate = ENGINE_WEAR_PCT_PER_H_IDLE
                + (ENGINE_WEAR_PCT_PER_H_FULL_LOAD - ENGINE_WEAR_PCT_PER_H_IDLE) * duty.min(1.0);
            // Care buffs slow honest duty-cycle wear only; the abuse terms
            // below stay full price -- fresh oil does not excuse over-revving.
            let mut engine = rate / 3600.0 * sim_dt * self.engine_wear_buff_mult;
            // Abuse penalties charge per real second of the behavior, like
            // the damage accrual the over-rev term replaces.
            if self.rpm > s.max_rpm * OVER_REV_RPM_MULT {
                engine += ENGINE_WEAR_OVER_REV_PCT_PER_S * dt;
            }
            let lugging = !self.transmission.in_neutral()
                && self.throttle > LUG_THROTTLE
                && speed > 0.5
                && self.rpm < s.peak_torque_rpm * LUG_RPM_FRACTION;
            if lugging {
                engine += ENGINE_WEAR_LUG_PCT_PER_S * dt;
            }
            self.engine_wear_pct = (self.engine_wear_pct + engine).min(100.0);
        }
    }

    /// Freight moves when the truck does something abrupt.
    ///
    /// Two forces, both already in the sim, and the securement standard sets
    /// both thresholds. Braking throws the load forward against restraint
    /// rated to hold it through 0.8 g, which is more than the brakes can
    /// produce -- so an ordinary hard stop, however loud, costs the freight
    /// nothing, and what does cost is the emergency application, a grade
    /// adding its own g to the stop, or hitting something. Cornering throws
    /// it sideways against restraint rated for only 0.5 g, in a truck that
    /// starts lifting a wheel before that, which is why a bend is the
    /// manoeuvre that actually moves freight.
    pub(crate) fn update_cargo(&mut self, dt: f64, decel_g: f64) {
        if !self.trailer_attached || self.cargo_kg <= 0.0 {
            return;
        }
        let mut rate = 0.0;
        let over_brake = decel_g - CARGO_HARD_BRAKE_G;
        if over_brake > 0.0 && self.speed_mph() > 5.0 {
            rate += over_brake * CARGO_BRAKE_PCT_PER_G_S;
        }
        // From the roll model's warning share of this load's threshold, as
        // it stands this moment: a tank's wave running past its steady place
        // brings the line down with the threshold (the surge tier).
        let over_corner = self.roll_lateral_g() - ROLL_WARN_SHARE * self.live_roll_threshold_g();
        if over_corner > 0.0 {
            rate += over_corner * CARGO_CORNER_PCT_PER_G_S;
        }
        if rate > 0.0 {
            self.add_cargo_damage(rate * self.cargo_fragility * dt);
        }
    }
}
