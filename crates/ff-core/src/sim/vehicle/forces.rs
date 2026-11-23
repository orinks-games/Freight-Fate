//! `TruckState` forces: drive, resistance, service and jake braking, the
//! liquid surge terms, and the stopping-distance estimate built on them.

use super::{
    BrakeApplication, TruckState, AIR_DENSITY, DRIVE_AXLE_LOAD_FRACTION, EMERGENCY_BRAKE_MULT, G,
    JAKE_LOCK_MARGIN, JAKE_RPM_FLOOR, JAKE_STAGES, LAUNCH_TRACTION_FULL_GRADE,
    LAUNCH_TRACTION_LOW_SPEED_MPH, LAUNCH_TRACTION_ROLLING_G, LAUNCH_TRACTION_START_G,
    MIN_STOPPING_DECEL_MPS2, SURGE_EXCUSE_BRAKE, SURGE_EXCUSE_FORCE_N,
};
use crate::sim::surge::lateral_accel_mps2;

impl TruckState {
    // -- forces -----------------------------------------------------------------

    /// Unclamped tractive force demanded at the drive wheels before tire grip.
    pub(super) fn drive_force_demand(&self) -> f64 {
        if !self.engine_on || self.stalled || self.air_brakes_holding() {
            return 0.0;
        }
        let ratio = self.transmission.drive_ratio();
        if ratio == 0.0 {
            return 0.0;
        }
        if self.coupled_rpm(None) >= self.specs.max_rpm {
            // The diesel governor cuts fuel at governed RPM. Continuing to hold
            // the throttle must not accelerate through a fixed low gear.
            return 0.0;
        }
        if self.speed_governed() {
            // The road-speed governor does the same thing at a road speed
            // instead of an engine speed. It cuts fuel; it does not brake, so
            // gravity can still carry the truck past the cap on a downgrade.
            return 0.0;
        }
        let mut torque = self.torque_at(self.rpm) * self.throttle * self.health_factor();
        torque *= self.engine_wear_factor();
        torque * ratio.abs() * self.specs.driveline_efficiency / self.specs.wheel_radius_m
    }

    pub fn drive_force(&self) -> f64 {
        let force = self.drive_force_demand();
        if force == 0.0 {
            return 0.0;
        }
        let direction = if self.transmission.drive_ratio() < 0.0 {
            -1.0
        } else {
            1.0
        };
        let cap = self.drive_traction_limit() * self.drive_traction_state.traction_factor();
        direction * force.min(cap)
    }

    /// The launch-ramped traction cap the drive wheels can actually use.
    ///
    /// Drive wheels can use roughly a third of gross weight once rolling,
    /// but a loaded tractor-trailer eases into that force instead of
    /// launching at the full traction cap from a dead stop. The easing
    /// belongs to the load: a bobtail or empty rig jumps off the line while
    /// a grossed-out one creeps into its traction. It is a launch feel,
    /// not a physical ceiling: a steep climb gets the full cap at any
    /// speed, because capping a grade crawl below its resistance would
    /// trap the truck and churn the automatic through pointless shifts.
    /// The automatic's post-shift force prediction shares this cap so it
    /// never promises a gear more force than the tires will deliver.
    pub fn drive_traction_limit(&self) -> f64 {
        let launch = (self.speed_mph() / LAUNCH_TRACTION_LOW_SPEED_MPH).min(1.0);
        let load_fraction = self.load_fraction();
        let start_g = LAUNCH_TRACTION_ROLLING_G
            - (LAUNCH_TRACTION_ROLLING_G - LAUNCH_TRACTION_START_G) * load_fraction;
        let mut traction_g = start_g + (LAUNCH_TRACTION_ROLLING_G - start_g) * launch;
        let climb = (self.grade.max(0.0) / LAUNCH_TRACTION_FULL_GRADE).min(1.0);
        traction_g += (LAUNCH_TRACTION_ROLLING_G - traction_g) * climb;
        self.gross_mass_kg() * G * traction_g * self.effective_grip()
    }

    pub fn resistance_force(&self) -> f64 {
        let s = &self.specs;
        let v = self.velocity_mps;
        let direction = if v > 0.01 {
            1.0
        } else if v < -0.01 {
            -1.0
        } else {
            0.0
        };
        let drag = 0.5
            * AIR_DENSITY
            * s.drag_coefficient
            * s.frontal_area_m2
            * self.drag_mult
            * v
            * v.abs();
        let rolling = self.gross_mass_kg() * G * s.rolling_resistance * direction;
        let grade_f = self.gross_mass_kg() * G * self.grade.atan().sin();
        drag + rolling + grade_f
    }

    /// The throttle that balances what the truck is fighting right now.
    ///
    /// Grade, drag, and rolling resistance all land in ``resistance_force``,
    /// so dividing it by the force full throttle can make in this gear gives
    /// the pedal position that holds the current speed. Automatic speed
    /// control uses it as a feed-forward term: the grade is answered as the
    /// wheels reach it rather than integrated up to over the following ten
    /// seconds. Zero on a downgrade gravity will carry, and one where the
    /// grade asks for more than the engine has in this gear.
    pub fn hold_throttle(&self) -> f64 {
        if !self.engine_on || self.stalled || self.air_brakes_holding() {
            return 0.0;
        }
        let ratio = self.transmission.drive_ratio();
        if ratio == 0.0 || self.coupled_rpm(None) >= self.specs.max_rpm || self.speed_governed() {
            return 0.0;
        }
        let full_force = self.torque_at(self.rpm)
            * self.health_factor()
            * self.engine_wear_factor()
            * ratio.abs()
            * self.specs.driveline_efficiency
            / self.specs.wheel_radius_m;
        if full_force <= 0.0 {
            return 1.0;
        }
        (self.resistance_force() / full_force).clamp(0.0, 1.0)
    }

    /// How much of the rated brake effort survives the current drum heat.
    pub(super) fn brake_fade_factor(&self) -> f64 {
        let fade_temp = self.brake_fade_onset_c();
        if self.brake_temp_c < fade_temp {
            return 1.0;
        }
        (1.0 - (self.brake_temp_c - fade_temp) / 300.0).max(0.20)
    }

    /// Magnitude of the foundation-brake force for a chosen application.
    pub fn foundation_brake_force(&self, application: BrakeApplication) -> f64 {
        let (application, boost) = match application {
            BrakeApplication::Service(pedal) => (pedal.clamp(0.0, 1.0), 1.0),
            BrakeApplication::Emergency => (1.0, EMERGENCY_BRAKE_MULT),
        };
        let s = &self.specs;
        let effort = G
            * s.max_brake_decel_g
            * application
            * boost
            * self.brake_fade_factor()
            * self.brake_wear_factor();
        // Tire friction scales with the weight on the tires and weather grip.
        // The foundation brakes have a fixed force ceiling sized for the rated
        // gross. Above that rating, the same hardware stops the extra mass more
        // slowly. This is also the live force path, so an estimate cannot assume
        // a stronger or weaker application than the truck receives.
        let friction = self.gross_mass_kg() * effort * self.effective_grip();
        let capacity = s.mass_kg * effort;
        friction.min(capacity)
    }

    /// Magnitude of the foundation-brake force biting the drums right now.
    ///
    /// This is the force that heats and wears the shoes; the jake is kept
    /// separate because its energy goes out the exhaust, not into the drums.
    /// Foundation-brake demand vs. road-grip ceiling for ABS slip detection.
    pub(super) fn service_brake_demand_and_grip_limit(&self) -> (f64, f64) {
        if self.velocity_mps.abs() <= 0.01 {
            return (0.0, 0.0);
        }
        let holding = self.air_brakes_holding();
        let (application, boost) = if self.emergency_brake || holding {
            (1.0, EMERGENCY_BRAKE_MULT)
        } else {
            (self.brake.clamp(0.0, 1.0), 1.0)
        };
        if application <= 0.0 {
            return (0.0, 0.0);
        }
        let s = &self.specs;
        let max_decel = G * s.max_brake_decel_g;
        let effort =
            max_decel * application * boost * self.brake_fade_factor() * self.brake_wear_factor();
        let brake_demand = (self.gross_mass_kg() * effort).min(s.mass_kg * effort);
        let road_grip_limit = self.gross_mass_kg() * max_decel * self.effective_grip();
        (brake_demand, road_grip_limit)
    }

    pub fn service_brake_force(&self) -> f64 {
        if self.velocity_mps.abs() <= 0.01 {
            return 0.0;
        }
        let holding = self.air_brakes_holding();
        let application = if self.emergency_brake || holding {
            BrakeApplication::Emergency
        } else {
            BrakeApplication::Service(self.brake)
        };
        self.foundation_brake_force(application) * self.abs_state.modulation_factor()
    }

    /// Foundation-brake deceleration for a chosen application, including any
    /// active ABS modulation so stopping budgets agree with delivered force.
    pub fn braking_decel_mps2(&self, application: BrakeApplication) -> f64 {
        (self.foundation_brake_force(application) * self.abs_state.modulation_factor())
            / self.gross_mass_kg().max(1.0)
    }

    /// Deceleration a full service-brake application delivers right now.
    ///
    /// Fade, wear, load, and grip included -- what a service-braking
    /// budget must use. The spec-sheet number overpromises exactly when it
    /// matters most: hot or worn brakes on a loaded rig.
    pub fn full_service_decel_mps2(&self) -> f64 {
        self.braking_decel_mps2(BrakeApplication::Service(1.0))
    }

    // -- liquid surge -------------------------------------------------------------

    /// Forward push from a sloshing liquid load, newtons. Zero otherwise.
    pub fn surge_force_n(&self) -> f64 {
        match &self.liquid {
            Some(liquid) if self.trailer_attached && self.cargo_kg > 0.0 => {
                liquid.force_n(self.cargo_kg)
            }
            _ => 0.0,
        }
    }

    /// Whether the liquid, not the driver, is what carried the truck on.
    ///
    /// True only when the load is genuinely shoving forward while the driver
    /// is already hard on the brakes. A driver who braked correctly and in
    /// time, and got pushed through anyway by a load doing exactly what a
    /// tank load does, has not made a preventable mistake -- and a safety
    /// committee that ruled otherwise would be teaching them nothing except
    /// that the game is arbitrary.
    pub fn pushed_through_by_surge(&self) -> bool {
        if self.liquid.is_none() {
            return false;
        }
        if self.brake < SURGE_EXCUSE_BRAKE && !self.emergency_brake {
            return false;
        }
        self.surge_force_n() >= SURGE_EXCUSE_FORCE_N
    }

    /// How much deceleration this load can take away at the worst moment.
    ///
    /// A property of the tank and how full it is rather than of where the
    /// wave happens to be, so a stopping distance built on it is a number the
    /// driver can learn rather than one that breathes with the water.
    pub fn surge_decel_penalty_mps2(&self) -> f64 {
        match &self.liquid {
            Some(liquid) if self.trailer_attached && self.cargo_kg > 0.0 => {
                liquid.peak_force_n(self.cargo_kg) / self.gross_mass_kg().max(1.0)
            }
            _ => 0.0,
        }
    }

    pub(super) fn update_liquid(&mut self, dt: f64, accel_mps2: f64) {
        if self.liquid.is_none() || !self.trailer_attached || self.cargo_kg <= 0.0 {
            return;
        }
        let lat = lateral_accel_mps2(self.speed_mph(), self.corner_advisory_mph);
        if let Some(liquid) = self.liquid.as_mut() {
            liquid.update(dt, accel_mps2, lat);
        }
    }

    // -- stopping distance --------------------------------------------------------

    /// Road needed to bring this truck, as it is right now, to a stop.
    ///
    /// The one number every stop cue in the game was missing. It answers with
    /// what the truck can actually do -- fade, worn shoes, the load aboard,
    /// the weather under the tyres, the grade it is on, and a liquid load
    /// pushing back -- rather than with a constant chosen for a good day.
    ///
    /// `reaction_s` adds the ground covered before the pedal moves, for
    /// callers budgeting a driver's response as well as the truck's.
    /// `speed_mps` None means the truck's current speed.
    pub fn stopping_distance_for_m(
        &self,
        speed_mps: Option<f64>,
        reaction_s: f64,
        include_surge: bool,
        application: BrakeApplication,
    ) -> f64 {
        let v = match speed_mps {
            None => self.velocity_mps,
            Some(s) => s,
        }
        .abs();
        if v <= 0.0 {
            return 0.0;
        }
        // Uphill helps and downhill hurts, at g times the grade.
        let mut decel = self.braking_decel_mps2(application) + G * self.grade;
        if include_surge {
            decel -= self.surge_decel_penalty_mps2();
        }
        // A truck that cannot out-brake the hill it is on is a runaway, not a
        // stopping-distance problem; the floor keeps the number finite so the
        // cue layer degrades into "as early as possible" instead of dividing
        // by zero. The descent and runaway systems own that case.
        decel = decel.max(MIN_STOPPING_DECEL_MPS2);
        v * reaction_s.max(0.0) + (v * v) / (2.0 * decel)
    }

    /// Full-service stopping distance retained for existing planning callers.
    pub fn stopping_distance_m(
        &self,
        speed_mps: Option<f64>,
        reaction_s: f64,
        include_surge: bool,
    ) -> f64 {
        self.stopping_distance_for_m(
            speed_mps,
            reaction_s,
            include_surge,
            BrakeApplication::Service(1.0),
        )
    }

    /// Braking time from one speed to another for a chosen application.
    pub fn braking_time_for_s(
        &self,
        speed_mps: f64,
        target_mps: f64,
        include_surge: bool,
        application: BrakeApplication,
    ) -> f64 {
        let speed_delta = (speed_mps.abs() - target_mps.abs()).max(0.0);
        if speed_delta <= 0.0 {
            return 0.0;
        }
        let mut decel = self.braking_decel_mps2(application) + G * self.grade;
        if include_surge {
            decel -= self.surge_decel_penalty_mps2();
        }
        speed_delta / decel.max(MIN_STOPPING_DECEL_MPS2)
    }

    pub fn stopping_distance_mi(
        &self,
        speed_mps: Option<f64>,
        reaction_s: f64,
        include_surge: bool,
    ) -> f64 {
        self.stopping_distance_m(speed_mps, reaction_s, include_surge) / 1609.344
    }

    /// Extra road the liquid alone asks for, miles. Zero without a tank.
    pub fn surge_stopping_penalty_mi(&self, speed_mps: Option<f64>) -> f64 {
        if self.liquid.is_none() {
            return 0.0;
        }
        let with_liquid = self.stopping_distance_mi(speed_mps, 0.0, true);
        let without = self.stopping_distance_mi(speed_mps, 0.0, false);
        (with_liquid - without).max(0.0)
    }

    /// Compression-brake torque at the crank for the current stage and RPM.
    pub fn jake_retard_torque_nm(&self) -> f64 {
        if self.engine_brake_stage <= 0
            || !self.engine_on
            || self.throttle > 0.05
            || self.transmission.in_neutral()
        {
            return 0.0;
        }
        let s = &self.specs;
        let rpm_frac = (self.rpm / s.max_rpm).clamp(0.0, 1.0);
        let stage = JAKE_STAGES.min(self.engine_brake_stage) as f64 / JAKE_STAGES as f64;
        s.engine_brake_torque_nm * stage * (JAKE_RPM_FLOOR + (1.0 - JAKE_RPM_FLOOR) * rpm_frac)
    }

    /// Wheel force the jake asks for: crank torque through the gearing.
    ///
    /// The gear ratio is the multiplier, so the same stage that pins the
    /// speed in 7th barely leans on the truck in overdrive -- and it drops
    /// out entirely mid-shift, exactly like the drive torque does.
    fn jake_force_demand(&self) -> f64 {
        let ratio = self.transmission.drive_ratio().abs();
        if ratio == 0.0 || self.velocity_mps.abs() <= 0.01 {
            return 0.0;
        }
        let s = &self.specs;
        self.jake_retard_torque_nm() * ratio * s.driveline_efficiency / s.wheel_radius_m
    }

    /// The most retard the drive axle can transmit before its wheels slide.
    pub(super) fn jake_traction_cap(&self) -> f64 {
        self.gross_mass_kg()
            * G
            * DRIVE_AXLE_LOAD_FRACTION
            * JAKE_LOCK_MARGIN
            * self.effective_grip()
    }

    /// Retarding force actually delivered: demand, capped by drive-axle grip.
    ///
    /// On dry pavement the cap sits far above anything the jake can ask for;
    /// on ice a hard stage in a low gear runs into it, which is the physics
    /// behind the CDL rule about compression brakes on slick roads.
    pub fn jake_brake_force(&self) -> f64 {
        let demand = self.jake_force_demand();
        if demand <= 0.0 {
            return 0.0;
        }
        demand.min(self.jake_traction_cap())
    }

    /// Whether the jake is asking for more than the drive axle can hold --
    /// the drive wheels breaking loose, the start of a trolley jackknife.
    pub fn jake_slipping(&self) -> bool {
        let demand = self.jake_force_demand();
        demand > 0.0 && demand > self.jake_traction_cap()
    }

    pub fn brake_force(&self) -> f64 {
        if self.velocity_mps.abs() <= 0.01 {
            return 0.0;
        }
        let direction = if self.velocity_mps > 0.0 { 1.0 } else { -1.0 };
        direction * (self.service_brake_force() + self.jake_brake_force())
    }

    /// Is the truck gaining speed or losing it, right now, in mph per second?
    ///
    /// Everything pushing the truck along less everything holding it back --
    /// gravity, drag, rolling resistance, the drums and the retarder -- over
    /// the gross mass. The one number that answers "is this being held", and
    /// deliberately ONE copy of it: the spoken G readout and the descent
    /// control both ask this, and a descent control that decided it could not
    /// hold a hill while the G key told the driver the speed was in hand is
    /// exactly the disagreement two copies produce (owner playtest, I-70 west
    /// of Vail, 2026-08-24 -- warned to get on the brakes while the truck was
    /// already shedding five miles an hour a second).
    pub fn net_accel_mph_per_s(&self) -> f64 {
        let net = self.drive_force() - self.resistance_force() - self.brake_force();
        net / self.gross_mass_kg() * super::MPS_TO_MPH
    }
}
