//! Truck physics: engine, forces, fuel, temperatures, and wear.
//!
//! Forces are computed in SI units on a longitudinal (1-D) model:
//! engine drive force, aerodynamic drag, rolling resistance, grade force,
//! and braking. The numbers are tuned around a loaded Class 8 tractor-trailer:
//! ~36 t gross, ~475 hp, 10-speed box with overdrive, ~70 mph governed top
//! speed, and ~6.5 mpg at cruise.
//!
//! Port of `freight_fate/sim/vehicle.py`. `TruckState` is one struct; its
//! method groups live in the `vehicle/` submodules (forces, air brakes,
//! per-frame updates, shifting, condition) so no one file outgrows itself.

use std::f64::consts::PI;

use crate::sim::surge::LiquidLoad;
use crate::sim::transmission::Transmission;

mod air;
mod axles;
mod condition;
mod descent;
mod forces;
mod mass;
mod roll;
mod shifting;
mod updates;

pub use axles::{AxleLoads, TANDEM_LIMIT_LB};
pub use descent::{DESCENT_SPEED_STEP_MPH, GSRS_BRAKE_LIMIT_C};
pub use mass::DIESEL_KG_PER_GAL;
pub use roll::ROLL_WARN_SHARE;

#[cfg(test)]
mod damage_band_tests;
#[cfg(test)]
mod physics_bench_tests;
#[cfg(test)]
mod realism_tests;
#[cfg(test)]
mod tests;

pub const G: f64 = 9.81;
pub const AIR_DENSITY: f64 = 1.225;
pub const MPS_TO_MPH: f64 = 2.23694;
pub const M_PER_FT: f64 = 0.3048;

// Floor under the deceleration a stopping-distance estimate is allowed to
// assume. A rig that cannot out-brake its own grade is a runaway and belongs
// to the descent systems; this only keeps the arithmetic finite.
pub const MIN_STOPPING_DECEL_MPS2: f64 = 0.35;

// What counts as "the driver was already doing the right thing" when a liquid
// load carries them past a bar anyway: a real brake application, and a real
// forward shove from the tank at that moment.
pub const SURGE_EXCUSE_BRAKE: f64 = 0.6;
pub const SURGE_EXCUSE_FORCE_N: f64 = 1500.0;

// Full service application plus the spring brakes: the hardest stop the rig
// can make, still scaled by weather grip and brake fade.
pub const EMERGENCY_BRAKE_MULT: f64 = 1.6;

/// Foundation-brake application used by estimates and live force calculations.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BrakeApplication {
    /// Normal treadle-valve application, from released through full service.
    Service(f64),
    /// Full service plus the emergency/spring-brake force used by the B key.
    Emergency,
}

/// About 10 mph: backing speed, not road speed.
pub const MAX_REVERSE_MPS: f64 = 4.5;

/// Game cargo "tons" are treated as metric tonnes.
pub const KG_PER_TON: f64 = 1000.0;
/// International avoirdupois pound, for the federal 80,000 lb GVW cap.
pub const KG_PER_LB: f64 = 0.45359237;
/// Federal combination gross vehicle weight limit, including fuel.
pub const LEGAL_GVW_LB: f64 = 80_000.0;
pub const LEGAL_GVW_KG: f64 = LEGAL_GVW_LB * KG_PER_LB;
// Reference loaded Class 8: ~36 t gross at a full ~21.5 t payload, leaving a
// ~14.5 t tractor-and-empty-trailer tare. A TruckState's default cargo equals
// this reference payload, so an unconfigured truck keeps the original loaded
// behavior; lighter loads (and empty deadheads) weigh proportionally less.
pub const REFERENCE_CARGO_KG: f64 = 21_500.0;
// A dry van's empty weight (~14,100 lb). Dropping the trailer takes this much
// off the tare, so a true bobtail runs five-plus tonnes lighter than a
// deadhead hauling an empty box.
pub const TRAILER_TARE_KG: f64 = 6_400.0;

/// Tractor, empty trailer, and full fuel tank for the stock specs.
/// Dispatch reserves this operating tare so filling up keeps its loads legal.
pub fn combination_tare_kg(specs: &TruckSpecs) -> f64 {
    (specs.mass_kg - REFERENCE_CARGO_KG).max(0.0)
}

/// Heaviest cargo a combination of this tare can haul without going over
/// the federal 80,000 lb cap.
pub fn max_legal_cargo_kg(tare_kg: f64) -> f64 {
    (LEGAL_GVW_KG - tare_kg).max(0.0)
}

pub fn max_legal_cargo_tons(tare_kg: f64) -> f64 {
    max_legal_cargo_kg(tare_kg) / KG_PER_TON
}

pub const LAUNCH_TRACTION_LOW_SPEED_MPH: f64 = 25.0;
pub const LAUNCH_TRACTION_START_G: f64 = 0.12;
pub const LAUNCH_TRACTION_ROLLING_G: f64 = 0.33;
/// Climbs this steep get the full cap at any speed.
pub const LAUNCH_TRACTION_FULL_GRADE: f64 = 0.03;

// -- rig wear -------------------------------------------------------------------
// Tires, brakes, and the engine wear from how the truck is driven, not just
// from miles. Distance- and energy-coupled terms scale with the trip's time
// compression (carried in ``fuel_burn_mult``) so wear per game mile stays
// honest at any time scale; the abuse terms (over-rev, lugging) charge per
// real second of the behavior, like the damage accrual they replace.
// Absolute rates are compressed for playability; the ratios are the point:
// jake-braked descents spare the service brakes, heavy loads chew tires,
// and redline abuse eats the engine.
/// Tread loss per mile at the rated gross.
pub const TIRE_WEAR_PCT_PER_MILE: f64 = 0.003;
/// Extra per (application x m/s x s): stops scrub tread.
pub const TIRE_WEAR_BRAKING_PCT: f64 = 2.0e-4;
/// Per megajoule actually dissipated in the shoes.
pub const BRAKE_WEAR_PCT_PER_MJ: f64 = 4.0e-3;
/// Glazing: wear doubles once the shoes are past fade.
pub const BRAKE_WEAR_HOT_MULT: f64 = 2.0;
pub const ENGINE_WEAR_PCT_PER_H_IDLE: f64 = 0.03;
pub const ENGINE_WEAR_PCT_PER_H_FULL_LOAD: f64 = 0.15;
/// Was the damage_pct redline penalty.
pub const ENGINE_WEAR_OVER_REV_PCT_PER_S: f64 = 0.8;
/// Heavy throttle far below the torque band.
pub const ENGINE_WEAR_LUG_PCT_PER_S: f64 = 0.05;
pub const LUG_THROTTLE: f64 = 0.7;
/// Of peak-torque RPM.
pub const LUG_RPM_FRACTION: f64 = 0.7;
/// Gameplay threshold for an advance service warning.
pub const COMPONENT_SERVICE_WARNING_PCT: f64 = 80.0;
/// Gameplay threshold where the truck requires service before it can continue.
pub const COMPONENT_SERVICE_LIMIT_PCT: f64 = 100.0;

// -- incident damage bands ---------------------------------------------------------
// A real truck does not fail all at once, and it does not shrug off a wreck
// either. An electronic engine meets a serious fault with a staged inducement:
// a warning first, then a torque derate (Cummins induces about 25 percent),
// then a road-speed derate the truck cannot drive out of, and finally idle
// only. Volvo publishes the same ladder as amber alert, torque limitation,
// 5 mph derate, idle only.
//
// The last rung is not the engine's decision at all, it is the law's. Under
// the CVSA North American Standard Out-of-Service Criteria a commercial
// vehicle carrying a qualifying defect is an imminent hazard and is
// prohibited from operating until it is repaired -- an inspector places it
// out of service at the roadside and it does not drive away. Brakes are the
// leading cause and carry an automatic trigger at 20 percent of the service
// brakes defective, which is the useful calibration here: losing a fifth of
// one safety system is already the wall, so a truck that has consumed nine
// tenths of its whole condition is far past any single out-of-service line.
// That is why the wall sits at DAMAGE_OUT_OF_SERVICE_PCT and not at 100:
// a wrecked truck has to stop while it still has paint on it.
//
// Below the first band nothing at all changes, so a driver who keeps the
// truck straight never meets any of this.
/// Reduced power begins.
pub const DAMAGE_DERATE_PCT: f64 = 50.0;
/// Limp mode: the road-speed cap comes in.
pub const DAMAGE_LIMP_PCT: f64 = 75.0;
/// Advisory band: names the wall before it lands.
pub const DAMAGE_LAST_CALL_PCT: f64 = 85.0;
/// The wall: the truck may not be driven.
pub const DAMAGE_OUT_OF_SERVICE_PCT: f64 = 90.0;
/// The top of the meter, for the derate ramp's anchor.
pub const DAMAGE_MAX_PCT: f64 = 100.0;
// Torque lost to the derate, ramped across each band so no crossing is a
// cliff: nothing at the bottom of reduced power, a quarter of the engine by
// limp mode, and near half at the top of the meter.
pub const DAMAGE_DERATE_TORQUE_LOSS: f64 = 0.25;
pub const DAMAGE_LIMP_TORQUE_LOSS: f64 = 0.45;
/// Extra burn at the top of the meter.
pub const DAMAGE_DERATE_FUEL_PENALTY: f64 = 0.25;
/// What limp mode governs the truck down to.
pub const DAMAGE_LIMP_CAP_MPH: f64 = 45.0;
// Out of service is not "slow", it is "not driveable". The engine may still
// run and the truck may still crawl clear of a live lane -- leaving a
// stricken truck stopped in traffic would be the more dangerous rule -- but
// there is no road speed left in it and the trip cannot continue.
pub const DAMAGE_CREEP_CAP_MPH: f64 = 10.0;

pub const DAMAGE_BAND_NONE: i32 = 0;
pub const DAMAGE_BAND_REDUCED: i32 = 1;
pub const DAMAGE_BAND_LIMP: i32 = 2;
pub const DAMAGE_BAND_LAST_CALL: i32 = 3;
pub const DAMAGE_BAND_OUT_OF_SERVICE: i32 = 4;

/// The named damage band a damage figure falls in.
pub fn damage_band_for(damage: f64) -> i32 {
    if damage >= DAMAGE_OUT_OF_SERVICE_PCT {
        return DAMAGE_BAND_OUT_OF_SERVICE;
    }
    if damage >= DAMAGE_LAST_CALL_PCT {
        return DAMAGE_BAND_LAST_CALL;
    }
    if damage >= DAMAGE_LIMP_PCT {
        return DAMAGE_BAND_LIMP;
    }
    if damage >= DAMAGE_DERATE_PCT {
        return DAMAGE_BAND_REDUCED;
    }
    DAMAGE_BAND_NONE
}

// -- cargo condition ------------------------------------------------------------------
// What moves freight is what the truck does abruptly, so the rates live with
// the physics that produce them; what the receiver DOES about the resulting
// condition lives in models/cargo_condition, away from the sim.
//
// Both thresholds are the securement standard rather than round numbers. Under
// 49 CFR 393.102 a load must be restrained against 0.8 g forward and 0.5 g
// lateral, and a Class 8 rig cannot brake at half of the forward figure -- so
// freight tied down to spec is not hurt by stopping hard, however alarming the
// stop. It is hurt by the sideways case, because the same regulation asks less
// there and a loaded van starts lifting a wheel around 0.35 g anyway.
//
// So: braking bites only past what a full service application can produce,
// which leaves the emergency application, a grade adding its own g to the
// stop, and collisions. Cornering bites below the rollover threshold, where
// the load is working against its straps before the truck is in trouble: the
// roll model's warning share of THIS load's threshold (`ROLL_WARN_SHARE`,
// `vehicle/roll.rs`), so a full van, a tank and an empty trailer each start
// moving freight where their own rollover ladder begins.
/// Decel past which freight starts moving.
pub const CARGO_HARD_BRAKE_G: f64 = 0.45;
/// Per g of excess, per real second.
pub const CARGO_BRAKE_PCT_PER_G_S: f64 = 6.0;
// Lateral, and geometric: what the bend actually pulls, from its radius and
// the speed it is being taken at. The old model read raw mph over the posted
// advisory, which ranked bends backwards -- a hairpin and a sweeper taken the
// same margin over their signs are not the same manoeuvre, and the hairpin,
// which throws the load half again as hard, was costing it a third as much
// because a short bend is over sooner. It was also a flat 0.40 g, past the
// 0.35 g a full trailer rolls over at, so a full load never shifted before it
// would have gone over.
/// Per g of excess, per real second.
pub const CARGO_CORNER_PCT_PER_G_S: f64 = 12.0;
// What a posted advisory is worth in lateral g, for bends whose data carries
// no radius: the shipped advisories bake out at very nearly this figure, so a
// curve missing its geometry still behaves like its neighbours.
pub const CARGO_ADVISORY_LAT_G: f64 = 0.30;
/// At full severity.
pub const CARGO_COLLISION_PCT: f64 = 40.0;

// -- runaway ------------------------------------------------------------------------
// Losing a loaded truck down a grade is the classic way to destroy one, and
// coasting out of gear is how it happens: no driveline to hold the load back,
// no retarder, and drums that fade long before the bottom. Past this speed a
// tractor-trailer is not "going fast", it is coming apart -- tires past their
// rated speed, driveline whipping, the trailer steering the tractor -- so it
// takes real damage for every second it stays there, hard enough that a full
// runaway ends the run rather than merely sounding an alarm.
pub const RUNAWAY_SPEED_MPH: f64 = 85.0;
/// Per 10 mph past the threshold, per real second.
pub const RUNAWAY_DAMAGE_PCT_PER_S: f64 = 1.0;

// -- driveline abuse ------------------------------------------------------------------
// Selecting reverse while rolling forward is not a shift, it is a collision
// inside the gearbox. Real synchro-less truck boxes simply will not take it:
// the teeth crash and the lever stops. Above a walking pace the request is
// refused and the attempt costs the driveline.
pub const REVERSE_ENGAGE_MAX_MPH: f64 = 3.0;
/// Per refused attempt at speed.
pub const REVERSE_CRASH_DAMAGE_PCT: f64 = 4.0;

// -- jake brake -----------------------------------------------------------------
// The engine brake is retarding TORQUE at the crank, not a flat force at the
// wheels: wheel force = torque x gear ratio / wheel radius, so the jake bites
// hard in a low gear at high RPM and does almost nothing in overdrive at low
// RPM. Three stages (two, four, six cylinders) scale the torque, and retard
// grows with RPM -- which is the whole grade discipline: pick the gear and
// the speed BEFORE the hill, because the jake rewards being set up early.
pub const JAKE_STAGES: i32 = 3;
/// Fraction of full retard left near idle speed.
pub const JAKE_RPM_FLOOR: f64 = 0.3;

// -- parked high idle -------------------------------------------------------------
// Fast-idle/PTO mode: a parked driver latches an rpm setpoint on the cruise
// buttons (exactly how electronic trucks do it) -- warm-up, faster air build,
// hearing the engine out. It cancels the instant the parking brake releases.
pub const HIGH_IDLE_DEFAULT_RPM: f64 = 1000.0;
pub const HIGH_IDLE_MIN_RPM: f64 = 800.0;
pub const HIGH_IDLE_MAX_RPM: f64 = 1500.0;
pub const HIGH_IDLE_STEP_RPM: f64 = 100.0;

// A hill can drive the engine past the governor through the wheels; power
// alone cannot. Sitting AT governed speed is safe -- overspeed wear starts
// just beyond it, which is what actually hurts a diesel.
/// How far the road can spin the engine past governed.
pub const ROAD_OVERSPEED_RPM_MULT: f64 = 1.15;
/// Abuse wear begins past this multiple of governed speed.
pub const OVER_REV_RPM_MULT: f64 = 1.02;

// -- rev-matching through an automated shift ----------------------------------------
// An automated box cuts torque, goes to neutral, brings the engine to the NEW
// gear's synchronous speed, then re-engages: an upshift is a fall (the
// engine brake helps, which is why modern upshifts are quick), a downshift
// is a fueled blip UP. How fast the engine crosses the gap is its spare
// torque over its rotating inertia, so the blip scales with each truck's
// own engine. The interrupt LENGTH stays the transmission's owner-tuned
// timer; the engine simply arrives at sync inside it and holds there.
//
// Provenance: all three figures are ASSUMED. Engine makers do not publish
// rotating inertia; 3 kg m^2 is the order of a heavy-duty inline-six with
// its flywheel and clutch pack. Motoring drag of 12 percent of rated torque
// and rev-match fueling at 35 percent of the curve give a 500 rpm downshift
// blip in about 0.3 s and an upshift fall in about 0.15 s, the durations
// heard on I-Shift, DT12 and Endurant demonstrations. Replace with a
// measured figure if one ever surfaces; these are not readings.
/// Engine plus flywheel plus clutch, kg m^2.
pub const ENGINE_ROTATING_INERTIA_KG_M2: f64 = 3.0;
/// Closed-throttle friction and pumping, as a fraction of rated torque.
pub const ENGINE_MOTORING_DRAG_FRACTION: f64 = 0.12;
/// Rev-match fueling during a downshift, as a fraction of the torque curve.
pub const SHIFT_SYNC_FUEL_FRACTION: f64 = 0.35;
/// The share of the engine brake an automated upshift uses to pull the revs
/// down to sync.
pub const SHIFT_SYNC_BRAKE_FRACTION: f64 = 0.5;

// -- brake heat -------------------------------------------------------------------
// Heating is the real dissipated power (service brake force times speed)
// soaked into the drums' thermal mass, so faded shoes that grip less also
// heat less and the model finds its own equilibrium. Cooling is convective
// and grows with the square root of speed: outrunning your brakes does not
// also air-condition them.
pub const AMBIENT_C: f64 = 20.0;
/// Fraction of excess heat shed per second, parked.
pub const BRAKE_COOL_BASE_PER_S: f64 = 0.0006;
/// Extra fraction per sqrt(m/s) of airflow.
pub const BRAKE_COOL_SPEED_PER_S: f64 = 0.0006;

// -- traction -----------------------------------------------------------------
// Hydroplaning follows the Horne relation: onset speed goes with the square
// root of tire pressure, about 106 mph for a fresh ribbed truck tire at
// highway pressure -- which is why a properly shod truck essentially never
// planes. Worn tread and deeper standing water pull the onset down into the
// speeds the game actually drives; past onset the tires ride the water film
// and grip collapses toward its floor over a short speed band.
/// ~10.35 x sqrt(105 psi), fresh tread in a thin film.
pub const HYDRO_ONSET_BASE_MPH: f64 = 106.0;
/// Bald tires plane at 55 percent of the fresh onset speed.
pub const HYDRO_TREAD_LOSS: f64 = 0.45;
/// A thinner film cannot float a loaded truck tire.
pub const HYDRO_MIN_WATER_MM: f64 = 0.8;
/// Onset drop per millimeter past the minimum film.
pub const HYDRO_WATER_LOSS_PER_MM: f64 = 0.06;
/// Mph past onset where the collapse bottoms out.
pub const HYDRO_FULL_BAND_MPH: f64 = 12.0;
/// Fraction of wet grip left when fully planing.
pub const HYDRO_GRIP_FLOOR: f64 = 0.30;

// The jake retards through the drive axle alone, so its force is capped by
// that axle's share of the grip -- not the whole rig's. Half the axle's
// static grip is the usable margin before compression braking breaks the
// drive wheels loose (the start of a trolley jackknife). Dry pavement never
// reaches the cap; glare ice puts full stage 3 in a low gear well past it.
/// Tandem drives carry 34k of an 80k gross.
pub const DRIVE_AXLE_LOAD_FRACTION: f64 = 0.425;
/// Usable fraction of drive-axle grip before the wheels slide.
pub const JAKE_LOCK_MARGIN: f64 = 0.5;

// Traction equipment. Tire compound and chains multiply the weather's grip on
// the surfaces they were made for, and both trades are honest: winter rubber
// is a softer compound that bites cold snow but wears faster and gives up a
// little on warm dry pavement; chains are the only thing that truly holds
// glare ice, and they are consumable -- sized for packed snow at chain speed,
// they grind themselves apart on bare pavement or past CHAIN_SAFE_MPH until a
// cross chain lets go into the fender. Wear rates are compressed for
// playability like every other wear constant; the trade-offs are not.
pub const TIRE_ALL_SEASON: &str = "all_season";
pub const TIRE_WINTER: &str = "winter";
pub const WINTER_SNOW_GRIP_MULT: f64 = 1.30;
/// 0.15 becomes 0.22: better, still frightening.
pub const WINTER_ICE_GRIP_MULT: f64 = 1.50;
/// The soft compound squirms a little on warm dry roads.
pub const WINTER_DRY_GRIP_LOSS: f64 = 0.03;
/// And it wears half again as fast.
pub const WINTER_TREAD_WEAR_MULT: f64 = 1.5;
/// Chained packed snow drives about like rain.
pub const CHAIN_SNOW_GRIP_MULT: f64 = 1.50;
/// 0.15 becomes 0.38: crossable, carefully.
pub const CHAIN_ICE_GRIP_MULT: f64 = 2.50;
/// Steel between the tread and dry pavement.
pub const CHAIN_BARE_GRIP_LOSS: f64 = 0.15;
/// Chain speed; faster hammers the links apart.
pub const CHAIN_SAFE_MPH: f64 = 30.0;
/// About 500 miles of life used right.
pub const CHAIN_WEAR_PCT_PER_MILE: f64 = 0.2;
pub const CHAIN_WEAR_OVERSPEED_MULT: f64 = 6.0;
/// Bare pavement eats a set in a couple of miles.
pub const CHAIN_WEAR_BARE_MULT: f64 = 40.0;
/// The freed cross chains flail the fender and lines.
pub const CHAIN_SNAP_DAMAGE_PCT: f64 = 4.0;

// What wear does to the physics.
/// Bald tires lose a quarter of their grip.
pub const TIRE_WEAR_GRIP_LOSS: f64 = 0.25;
/// Worn shoes lose up to 30% braking force.
pub const BRAKE_WEAR_FORCE_LOSS: f64 = 0.30;
/// And start fading this much sooner.
pub const BRAKE_WEAR_FADE_LOSS_C: f64 = 150.0;
/// A tired engine is down up to a quarter.
pub const ENGINE_WEAR_POWER_LOSS: f64 = 0.25;
/// And burns up to 15% more fuel for its power.
pub const ENGINE_WEAR_FUEL_PENALTY: f64 = 0.15;

#[derive(Debug, Clone, PartialEq)]
pub struct TruckSpecs {
    /// Gross weight at the reference payload.
    pub mass_kg: f64,
    pub drag_coefficient: f64,
    pub frontal_area_m2: f64,
    pub rolling_resistance: f64,
    pub wheel_radius_m: f64,
    /// ~1770 lb-ft
    pub max_torque_nm: f64,
    pub idle_rpm: f64,
    /// Parked high idle while the compressor builds air.
    pub fast_idle_rpm: f64,
    pub max_rpm: f64,
    pub peak_torque_rpm: f64,
    pub driveline_efficiency: f64,
    pub max_brake_decel_g: f64,
    /// Brakes fade above this temperature.
    pub brake_fade_temp_c: f64,
    /// Drums and shoes, all ten positions.
    pub brake_thermal_mass_j_per_c: f64,
    pub fuel_tank_gal: f64,
    /// Model-specific thirst multiplier.
    pub fuel_burn_factor: f64,
    /// Stage-3 retarding torque near rated RPM.
    pub engine_brake_torque_nm: f64,
    // Air-brake thresholds follow official CDL references: FMCSA gives
    // typical compressor cut-out/cut-in ranges, California places low-air
    // warnings at 55-75 psi, and Georgia describes spring brakes applying
    // around 20-45 psi. Runtime build rates are intentionally compressed for
    // playability; see README.md for source URLs and simplification notes.
    pub air_governor_cut_out_psi: f64,
    pub air_governor_cut_in_psi: f64,
    pub air_low_warning_psi: f64,
    // Hysteresis for the low-air warning cue: repeated service braking makes
    // pressure hover right around air_low_warning_psi while the compressor
    // catches up, so the warning must not re-arm until pressure has climbed
    // well above the threshold, not merely ticked a fraction over it.
    pub air_low_warning_clear_psi: f64,
    pub air_spring_brake_psi: f64,
    pub air_parking_release_psi: f64,
    pub air_cold_start_psi: f64,
    pub air_build_idle_psi_per_s: f64,
    pub air_build_fast_psi_per_s: f64,
    // Parked-time leakage is compressed with game time so a full overnight
    // rest returns a charged truck to the same low-air state as a cold start.
    pub air_leak_psi_per_game_hour: f64,
    pub air_loss_primary_per_application_psi: f64,
    pub air_loss_secondary_per_application_psi: f64,
    pub air_loss_trailer_per_application_psi: f64,
    /// Legacy tuning reference.
    pub air_loss_per_application_psi: f64,
    pub air_loss_hold_psi_per_s: f64,
}

impl Default for TruckSpecs {
    fn default() -> Self {
        TruckSpecs {
            mass_kg: 36_000.0,
            drag_coefficient: 0.65,
            frontal_area_m2: 10.0,
            rolling_resistance: 0.0065,
            wheel_radius_m: 0.5,
            max_torque_nm: 2_400.0,
            idle_rpm: 600.0,
            fast_idle_rpm: 900.0,
            max_rpm: 2_200.0,
            peak_torque_rpm: 1_300.0,
            driveline_efficiency: 0.85,
            max_brake_decel_g: 0.35,
            brake_fade_temp_c: 400.0,
            brake_thermal_mass_j_per_c: 180_000.0,
            fuel_tank_gal: 150.0,
            fuel_burn_factor: 1.0,
            engine_brake_torque_nm: 1_800.0,
            air_governor_cut_out_psi: 125.0,
            air_governor_cut_in_psi: 100.0,
            air_low_warning_psi: 60.0,
            air_low_warning_clear_psi: 68.0,
            air_spring_brake_psi: 40.0,
            air_parking_release_psi: 100.0,
            air_cold_start_psi: 55.0,
            air_build_idle_psi_per_s: 4.0,
            air_build_fast_psi_per_s: 7.0,
            air_leak_psi_per_game_hour: 7.0,
            air_loss_primary_per_application_psi: 4.5,
            air_loss_secondary_per_application_psi: 3.5,
            air_loss_trailer_per_application_psi: 2.0,
            air_loss_per_application_psi: 4.0,
            air_loss_hold_psi_per_s: 0.25,
        }
    }
}

/// Miles per gallon at a 60 mph cruise, the figure `update_fuel` is
/// calibrated to (6.5 to 7).
pub const CRUISE_MPG: f64 = 6.5;

#[derive(Debug, Clone, PartialEq)]
pub struct TruckState {
    pub specs: TruckSpecs,
    pub transmission: Transmission,

    pub engine_on: bool,
    pub velocity_mps: f64,
    pub rpm: f64,
    pub throttle: f64,
    pub brake: f64,
    /// 0 = off, 1..JAKE_STAGES = cylinders engaged
    pub engine_brake_stage: i32,
    pub emergency_brake: bool,
    pub parking_brake: bool,
    /// The air horn valve is open; drains the tanks.
    pub horn_on: bool,
    pub primary_air_psi: f64,
    pub secondary_air_psi: f64,
    pub trailer_air_psi: f64,
    pub air_compressor_active: bool,

    pub fuel_gal: f64,
    pub engine_temp_c: f64,
    pub brake_temp_c: f64,
    /// Incident damage: collisions, leaving the road.
    pub damage_pct: f64,
    /// 0 = fresh tread, 100 = bald
    pub tire_wear_pct: f64,
    /// 0 = new shoes, 100 = metal on metal
    pub brake_wear_pct: f64,
    /// 0 = fresh overhaul, 100 = worn out
    pub engine_wear_pct: f64,
    /// all_season or winter, follows the truck
    pub tire_type: String,
    /// Steel on the drives; installed and pulled at a stop.
    pub chains_on: bool,
    /// 0 = fresh set, 100 = snapped or scrap
    pub chain_wear_pct: f64,
    /// One-shot event flag, consumed by the cue layer.
    pub chains_just_snapped: bool,
    /// Inside a chain control the state has posted. A posted control means
    /// the road calls for chains, so they wear as on snow whatever the sky
    /// model reads underfoot: a live winter-storm alert can post the control
    /// while the simulated surface is still wet, and the bare-pavement rate
    /// then ground a lawful set to scrap in about a dozen miles at chain
    /// speed (player report, 2026-10-17).
    pub chain_control_road: bool,
    pub odometer_mi: f64,
    /// Payload aboard; default = full reference load.
    pub cargo_kg: f64,
    /// False = true bobtail: the tractor alone, nothing on the fifth wheel.
    /// Deadheading with an empty box keeps this True; the difference is the
    /// trailer's tare, its air line, and how a light box gets shifted.
    pub trailer_attached: bool,

    // environment, set each frame by the trip/weather layer
    /// +uphill, e.g. 0.06 = 6%
    pub grade: f64,
    /// Weather traction multiplier.
    pub grip: f64,
    /// Standing water on the road; drives hydroplaning.
    pub water_mm: f64,
    /// dry, wet, snow, or ice; keys the traction equipment
    pub surface: String,
    /// Weather aero drag multiplier (headwinds/storms).
    pub drag_mult: f64,
    /// Trip time compression so mpg stays honest.
    pub fuel_burn_mult: f64,
    /// Driver-care buff on tread wear (data/buffs.py).
    pub tire_wear_buff_mult: f64,
    /// Driver-care buff on duty-cycle engine wear.
    pub engine_wear_buff_mult: f64,

    /// How much of the damage on this truck a safety committee would call
    /// preventable. Carriers rule every incident preventable or not and run
    /// progressive discipline on the preventable ones, so the model has to
    /// know the difference: hitting something, drifting off the road asleep,
    /// crashing the box into reverse and losing it down a grade all are;
    /// damage taken reacting correctly to a hazard is not. Per-trip, not
    /// persisted onto the profile -- the career layer reads it at settlement.
    pub preventable_damage_pct: f64,

    /// What the freight is in. 0 = tendered condition, 100 = a trailer of
    /// scrap. Moved by hard stops, bends taken past their advisory, and
    /// collisions, scaled by how well this class of freight survives being
    /// thrown about (``cargo_fragility``, set by the driving layer from the
    /// job). Lives with cargo_kg because the same forces move both.
    pub cargo_damage_pct: f64,
    pub cargo_fragility: f64,
    /// Set each frame by the driving layer: how far past the posted advisory
    /// the truck is taking the bend it is in, in mph. Zero on a straight or
    /// inside the advisory. The vehicle model has no map, so the road has to
    /// tell it; the force it produces is real and belongs here.
    pub corner_overspeed_mph: f64,
    /// The advisory itself, alongside how far past it the truck is. A liquid
    /// load needs the ratio, not the excess: the pull in a bend goes with the
    /// square of how far over the posting you are taking it.
    pub corner_advisory_mph: f64,
    /// The bend's own geometry, in feet. Dry freight needs the radius rather
    /// than the advisory, because what moves a pallet is the sideways pull, and
    /// that is a fact about the corner rather than about the sign beside it.
    pub corner_radius_ft: f64,
    /// The bank the bend is built with, as a slope, set with the radius. The
    /// share of the pull the bank carries never reaches the truck's own
    /// frame, so the roll model reads what is left.
    pub corner_bank: f64,

    /// The liquid aboard, if this is a tank load. None for every other kind of
    /// freight, and every surge term short-circuits on that -- a driver hauling
    /// boxes must not be able to tell this code exists.
    pub liquid: Option<LiquidLoad>,

    /// ECM road-speed governor: above this the engine simply stops fuelling,
    /// exactly like the rpm governor below. None = ungoverned. Not persisted --
    /// the driving layer sets it from the damage bands every frame.
    pub speed_cap_mph: Option<f64>,

    pub stalled: bool,
    /// Driver-latched parked high idle (fast-idle/PTO mode, on the cruise
    /// buttons like a real electronic truck). None = off. Not persisted:
    /// a real ECM drops fast idle at the key cycle.
    pub high_idle_rpm: Option<f64>,
    pub last_service_air_application: f64,
    /// Descent control is holding a downgrade: the automatic keeps its gear
    /// off the throttle the way it does under a brake application, rather than
    /// taking an economy upshift over the crest that the retarder then has
    /// to take straight back. Not persisted -- the driving layer sets it
    /// every frame cruise runs.
    pub descent_gear_hold: bool,
}

impl Default for TruckState {
    fn default() -> Self {
        TruckState::new(TruckSpecs::default())
    }
}

impl TruckState {
    /// `TruckState(specs=...)`: the dataclass defaults, then `__post_init__`
    /// (rpm at idle, a full tank).
    pub fn new(specs: TruckSpecs) -> Self {
        let rpm = specs.idle_rpm;
        let fuel_gal = specs.fuel_tank_gal;
        TruckState {
            specs,
            transmission: Transmission::default(),
            engine_on: false,
            velocity_mps: 0.0,
            rpm,
            throttle: 0.0,
            brake: 0.0,
            engine_brake_stage: 0,
            emergency_brake: false,
            parking_brake: false,
            horn_on: false,
            primary_air_psi: 125.0,
            secondary_air_psi: 125.0,
            trailer_air_psi: 125.0,
            air_compressor_active: false,
            fuel_gal,
            engine_temp_c: 60.0,
            brake_temp_c: 20.0,
            damage_pct: 0.0,
            tire_wear_pct: 0.0,
            brake_wear_pct: 0.0,
            engine_wear_pct: 0.0,
            tire_type: TIRE_ALL_SEASON.to_string(),
            chains_on: false,
            chain_wear_pct: 0.0,
            chains_just_snapped: false,
            chain_control_road: false,
            odometer_mi: 0.0,
            cargo_kg: REFERENCE_CARGO_KG,
            trailer_attached: true,
            grade: 0.0,
            grip: 1.0,
            water_mm: 0.0,
            surface: "dry".to_string(),
            drag_mult: 1.0,
            fuel_burn_mult: 1.0,
            tire_wear_buff_mult: 1.0,
            engine_wear_buff_mult: 1.0,
            preventable_damage_pct: 0.0,
            cargo_damage_pct: 0.0,
            cargo_fragility: 1.0,
            corner_overspeed_mph: 0.0,
            corner_advisory_mph: 0.0,
            corner_radius_ft: 0.0,
            corner_bank: 0.0,
            liquid: None,
            speed_cap_mph: None,
            stalled: false,
            high_idle_rpm: None,
            last_service_air_application: 0.0,
            descent_gear_hold: false,
        }
    }

    // Not a dataclass field: the bool view proxies the staged jake so every
    // existing on/off call site keeps working. Switching on selects full
    // retard; the stage keys pick lighter settings.
    pub fn engine_brake(&self) -> bool {
        self.engine_brake_stage > 0
    }

    pub fn set_engine_brake(&mut self, value: bool) {
        self.engine_brake_stage = if value { JAKE_STAGES } else { 0 };
    }

    // -- engine ----------------------------------------------------------------

    pub fn start_engine(&mut self) -> bool {
        if self.engine_on {
            return false;
        }
        if self.fuel_gal <= 0.0 {
            return false;
        }
        self.engine_on = true;
        self.stalled = false;
        self.rpm = self.specs.idle_rpm;
        self.sync_air_compressor();
        true
    }

    pub fn stop_engine(&mut self) {
        self.engine_on = false;
        self.throttle = 0.0;
        self.air_compressor_active = false;
    }

    /// Flat-topped torque curve typical of a big diesel.
    pub fn torque_at(&self, rpm: f64) -> f64 {
        let s = &self.specs;
        if rpm < s.idle_rpm * 0.8 || rpm > s.max_rpm {
            return 0.0;
        }
        let x = (rpm - s.peak_torque_rpm) / (s.max_rpm - s.idle_rpm);
        let shape = (1.0 - 1.8 * x * x).max(0.0);
        s.max_torque_nm * shape
    }

    /// Power multiplier from accumulated incident damage.
    ///
    /// The old linear slide is still the base curve; the band derate rides
    /// on top of it, inside the floor so that a limping truck always keeps
    /// enough engine to reach a repair. The harsh part of the deep bands is
    /// the road-speed cap, not being unable to move at all.
    pub fn health_factor(&self) -> f64 {
        ((1.0 - self.damage_pct / 150.0) * self.damage_derate_factor()).max(0.3)
    }

    /// Which named damage band the truck is in right now.
    ///
    /// Derived, never stored: ``damage_pct`` is already the persisted truth,
    /// so a save can never disagree with itself about the band.
    pub fn damage_band(&self) -> i32 {
        damage_band_for(self.damage_pct)
    }

    /// Torque left after the damage derate; 1.0 below reduced power.
    ///
    /// Ramped inside each band so crossing one is a wind-down rather than a
    /// step, and exactly 1.0 at the reduced-power threshold itself.
    pub fn damage_derate_factor(&self) -> f64 {
        let damage = self.damage_pct;
        if damage < DAMAGE_DERATE_PCT {
            return 1.0;
        }
        if damage < DAMAGE_LIMP_PCT {
            let frac = (damage - DAMAGE_DERATE_PCT) / (DAMAGE_LIMP_PCT - DAMAGE_DERATE_PCT);
            return 1.0 - DAMAGE_DERATE_TORQUE_LOSS * frac;
        }
        let frac = ((damage - DAMAGE_LIMP_PCT) / (DAMAGE_MAX_PCT - DAMAGE_LIMP_PCT)).min(1.0);
        1.0 - DAMAGE_DERATE_TORQUE_LOSS
            - (DAMAGE_LIMP_TORQUE_LOSS - DAMAGE_DERATE_TORQUE_LOSS) * frac
    }

    /// Extra fuel burn fraction from a derated engine; zero below the band.
    pub fn damage_fuel_penalty(&self) -> f64 {
        if self.damage_pct < DAMAGE_DERATE_PCT {
            return 0.0;
        }
        let over = (self.damage_pct - DAMAGE_DERATE_PCT) / (DAMAGE_MAX_PCT - DAMAGE_DERATE_PCT);
        DAMAGE_DERATE_FUEL_PENALTY * over.min(1.0)
    }

    /// The wall: this truck may not be driven until it is repaired.
    ///
    /// The engine is left alone deliberately -- a stricken truck still needs
    /// to be able to crawl out of a live lane -- so what stops the trip is
    /// the creep cap the driving layer holds, not a dead engine.
    pub fn out_of_service(&self) -> bool {
        self.damage_pct >= DAMAGE_OUT_OF_SERVICE_PCT || self.maintenance_required()
    }

    /// Whether ordinary wear has reached the game's required-service limit.
    pub fn maintenance_required(&self) -> bool {
        self.tire_wear_pct >= COMPONENT_SERVICE_LIMIT_PCT
            || self.brake_wear_pct >= COMPONENT_SERVICE_LIMIT_PCT
            || self.engine_wear_pct >= COMPONENT_SERVICE_LIMIT_PCT
    }

    /// The road-speed governor is holding fuel off right now.
    pub fn speed_governed(&self) -> bool {
        match self.speed_cap_mph {
            Some(cap) => self.speed_mph() >= cap,
            None => false,
        }
    }

    // -- wear effects ------------------------------------------------------------

    /// The speed where the tires start riding the water film, or None when
    /// the road holds too little water to float a loaded truck tire. Chains
    /// bite through the film, so a chained truck cannot plane.
    pub fn hydro_onset_mph(&self) -> Option<f64> {
        if self.chains_on || self.water_mm < HYDRO_MIN_WATER_MM {
            return None;
        }
        let tread = 1.0 - HYDRO_TREAD_LOSS * self.tire_wear_pct / 100.0;
        let water = (1.0 - HYDRO_WATER_LOSS_PER_MM * (self.water_mm - HYDRO_MIN_WATER_MM)).max(0.5);
        Some(HYDRO_ONSET_BASE_MPH * tread * water)
    }

    pub fn hydroplaning(&self) -> bool {
        match self.hydro_onset_mph() {
            Some(onset) => self.speed_mph() > onset,
            None => false,
        }
    }

    /// 1.0 below onset, collapsing toward the floor as speed leaves the
    /// onset behind. Slowing down restores contact -- and grip -- smoothly.
    fn hydro_grip_mult(&self) -> f64 {
        let onset = match self.hydro_onset_mph() {
            Some(onset) if self.speed_mph() > onset => onset,
            _ => return 1.0,
        };
        let frac = ((self.speed_mph() - onset) / HYDRO_FULL_BAND_MPH).min(1.0);
        1.0 - (1.0 - HYDRO_GRIP_FLOOR) * frac
    }

    /// Grip multiplier from chains or the tire compound on this surface.
    ///
    /// Chains put steel between the tread and the road, so they speak for
    /// the contact patch alone -- the tire type under them stops mattering.
    pub fn traction_equipment_mult(&self) -> f64 {
        if self.chains_on {
            if self.surface == "ice" {
                return CHAIN_ICE_GRIP_MULT;
            }
            if self.surface == "snow" {
                return CHAIN_SNOW_GRIP_MULT;
            }
            return 1.0 - CHAIN_BARE_GRIP_LOSS;
        }
        if self.tire_type == TIRE_WINTER {
            if self.surface == "ice" {
                return WINTER_ICE_GRIP_MULT;
            }
            if self.surface == "snow" {
                return WINTER_SNOW_GRIP_MULT;
            }
            if self.surface == "dry" {
                return 1.0 - WINTER_DRY_GRIP_LOSS;
            }
        }
        1.0
    }

    /// Weather grip degraded by tread wear and hydroplaning, helped or
    /// hurt by traction equipment; bald tires make every surface worse and
    /// float sooner in standing water. With chains on, steel is the contact
    /// patch: tread wear and the water film stop mattering.
    pub fn effective_grip(&self) -> f64 {
        let equip = self.traction_equipment_mult();
        if self.chains_on {
            return self.grip * equip;
        }
        let tread = 1.0 - TIRE_WEAR_GRIP_LOSS * self.tire_wear_pct / 100.0;
        self.grip * tread * equip * self.hydro_grip_mult()
    }

    pub fn brake_wear_factor(&self) -> f64 {
        1.0 - BRAKE_WEAR_FORCE_LOSS * self.brake_wear_pct / 100.0
    }

    /// Worn shoes start fading cooler than the spec sheet says.
    pub fn brake_fade_onset_c(&self) -> f64 {
        self.specs.brake_fade_temp_c - BRAKE_WEAR_FADE_LOSS_C * self.brake_wear_pct / 100.0
    }

    pub fn engine_wear_factor(&self) -> f64 {
        1.0 - ENGINE_WEAR_POWER_LOSS * self.engine_wear_pct / 100.0
    }

    /// Unloaded operating weight, including remaining diesel.
    pub fn tare_kg(&self) -> f64 {
        self.dry_tare_kg() + self.fuel_mass_kg()
    }

    /// Current gross weight: tare plus the payload aboard.
    ///
    /// Drives acceleration, grade and rolling resistance, braking, and (via
    /// the forces) fuel burn, so a heavy load pulls away gently, lugs on
    /// grades, and stops longer, while an empty deadhead is light and brisk.
    pub fn gross_mass_kg(&self) -> f64 {
        self.tare_kg() + self.cargo_kg.max(0.0)
    }

    /// Whether this combination is over the federal 80,000 lb GVW cap.
    /// Includes the tractor, attached trailer, cargo, and remaining diesel.
    pub fn is_over_legal_gvw(&self) -> bool {
        self.gross_mass_kg() > LEGAL_GVW_KG
    }

    /// Engine RPM implied by road speed in the given gear (the current gear
    /// when `gear` is None).
    pub fn coupled_rpm(&self, gear: Option<i32>) -> f64 {
        let tr = &self.transmission;
        let ratio = tr.ratio_for(match gear {
            None => self.transmission.gear,
            Some(g) => g,
        });
        if ratio == 0.0 {
            return self.rpm;
        }
        let wheel_rps = self.velocity_mps.abs() / (2.0 * PI * self.specs.wheel_radius_m);
        wheel_rps * 60.0 * ratio.abs()
    }

    // -- convenience ---------------------------------------------------------------

    pub fn speed_mph(&self) -> f64 {
        self.velocity_mps.abs() * 2.23694
    }

    pub fn fuel_fraction(&self) -> f64 {
        self.fuel_gal / self.specs.fuel_tank_gal
    }
}
