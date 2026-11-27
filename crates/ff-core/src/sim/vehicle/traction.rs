//! Explicit state machines for slick-surface ABS modulation, drive-axle
//! wheelspin, and single-trailer crosswind sway / tight-ramp off-tracking.

use std::f64::consts::PI;

/// Minimum road speed (mph) at which ABS cycles brake pressure.
pub const ABS_MIN_SPEED_MPH: f64 = 5.0;
/// Effective grip below which hard braking can trigger ABS modulation.
pub const ABS_GRIP_CEILING: f64 = 0.85;
/// Ratio of demanded brake force to grip limit that engages ABS.
pub const ABS_SLIP_RATIO: f64 = 0.95;
/// Highest pedal an assist holds on a slick road: under `ABS_SLIP_RATIO`, so
/// an assist's own braking never sets the ABS pulsing.
pub const ASSIST_BRAKE_PEDAL_CAP: f64 = 0.90;
/// Share of the drive-wheel traction limit an assist's throttle may demand
/// while the tires hold: a little under the limit, so they stay hooked up.
pub const ASSIST_TRACTION_MARGIN: f64 = 0.95;
/// The same share once the wheels are spinning: under
/// `WHEELSPIN_RECOVER_RATIO`, so wheels the driver broke loose hook back up
/// once an assist has the pedal.
pub const ASSIST_TRACTION_RECOVER_MARGIN: f64 = 0.85;
/// ABS pulse frequency in Hz (typical air-brake commercial ABS: 6-10 Hz).
pub const ABS_CYCLE_HZ: f64 = 8.0;
/// Minimum fraction of peak grip-limited brake force at the trough of an ABS cycle.
pub const ABS_MIN_FACTOR: f64 = 0.88;

/// Effective grip below which excessive throttle breaks the drive tires loose.
pub const WHEELSPIN_GRIP_CEILING: f64 = 0.85;
/// Minimum throttle fraction that can initiate drive-wheel spin.
pub const WHEELSPIN_MIN_THROTTLE: f64 = 0.15;
/// Kinetic friction multiplier while drive tires are spinning (feathering
/// throttle below the grip limit restores full static traction).
pub const WHEELSPIN_KINETIC_FACTOR: f64 = 0.88;
/// Demand-to-limit ratio below which spinning drive tires hook back up.
const WHEELSPIN_RECOVER_RATIO: f64 = 0.88;

/// Kingpin-to-rear-tandem wheelbase (ft) for a standard 53-foot semi-trailer.
pub const KINGPIN_TO_TANDEM_FT: f64 = 41.0;
/// Minimum curve curvature (rad/ft, ~400 ft radius) where single-trailer
/// off-tracking becomes significant (tight ramps and street corners).
pub const OFFTRACK_MIN_CURVATURE: f64 = 0.0025;
/// Half-lane width (ft) used to compare off-track margin against tractor offset.
const HALF_LANE_FT: f64 = 6.0;
/// Crosswind sway angle (degrees) at which single-trailer sway warning engages.
pub const TRAILER_SWAY_WARN_DEG: f64 = 3.0;
/// Hysteresis threshold (degrees) below which sway clears back to steady.
const TRAILER_SWAY_CLEAR_DEG: f64 = 2.0;
/// Maximum modeled single-trailer sway angle (degrees).
const TRAILER_SWAY_MAX_DEG: f64 = 8.0;

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum AbsState {
    #[default]
    Inactive,
    Modulating {
        phase_s: f64,
    },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AbsEvent {
    pub braking_demand_n: f64,
    pub grip_limit_n: f64,
    pub effective_grip: f64,
    pub speed_mph: f64,
    pub dt: f64,
}

impl AbsState {
    pub fn next(&self, event: AbsEvent) -> Self {
        let should_modulate = event.speed_mph >= ABS_MIN_SPEED_MPH
            && event.effective_grip < ABS_GRIP_CEILING
            && event.grip_limit_n > 0.0
            && event.braking_demand_n > event.grip_limit_n * ABS_SLIP_RATIO;
        if !should_modulate {
            return Self::Inactive;
        }
        let period = 1.0 / ABS_CYCLE_HZ;
        let prev = match *self {
            Self::Inactive => 0.0,
            Self::Modulating { phase_s } => phase_s,
        };
        Self::Modulating {
            phase_s: (prev + event.dt.max(0.0)) % period,
        }
    }

    pub fn modulation_factor(&self) -> f64 {
        match *self {
            Self::Inactive => 1.0,
            Self::Modulating { phase_s } => {
                let wave = 0.5 * (1.0 + (2.0 * PI * ABS_CYCLE_HZ * phase_s).cos());
                ABS_MIN_FACTOR + (1.0 - ABS_MIN_FACTOR) * wave
            }
        }
    }

    pub const fn is_active(&self) -> bool {
        matches!(self, Self::Modulating { .. })
    }

    pub const fn label(&self) -> &'static str {
        match self {
            Self::Inactive => "inactive",
            Self::Modulating { .. } => "modulating",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DriveTractionState {
    #[default]
    Grip,
    Spinning,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DriveTractionEvent {
    pub drive_demand_n: f64,
    pub traction_limit_n: f64,
    pub effective_grip: f64,
    pub throttle: f64,
}

impl DriveTractionState {
    pub fn next(&self, event: DriveTractionEvent) -> Self {
        if event.throttle <= WHEELSPIN_MIN_THROTTLE
            || event.effective_grip >= WHEELSPIN_GRIP_CEILING
            || event.traction_limit_n <= 0.0
        {
            return Self::Grip;
        }
        match *self {
            Self::Grip => {
                if event.drive_demand_n > event.traction_limit_n {
                    Self::Spinning
                } else {
                    Self::Grip
                }
            }
            Self::Spinning => {
                if event.drive_demand_n > event.traction_limit_n * WHEELSPIN_RECOVER_RATIO {
                    Self::Spinning
                } else {
                    Self::Grip
                }
            }
        }
    }

    pub const fn traction_factor(&self) -> f64 {
        match self {
            Self::Grip => 1.0,
            Self::Spinning => WHEELSPIN_KINETIC_FACTOR,
        }
    }

    pub const fn is_spinning(&self) -> bool {
        matches!(self, Self::Spinning)
    }

    pub const fn label(&self) -> &'static str {
        match self {
            Self::Grip => "grip",
            Self::Spinning => "spinning",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TrailerSwayState {
    #[default]
    Steady,
    Swaying,
    OffTracking,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TrailerSwayEvent {
    pub trailer_attached: bool,
    pub doubles_hooked: bool,
    pub sway_deg: f64,
    pub offtrack_clip: bool,
}

impl TrailerSwayState {
    pub fn next(&self, event: TrailerSwayEvent) -> Self {
        if !event.trailer_attached || event.doubles_hooked {
            return Self::Steady;
        }
        if event.offtrack_clip {
            return Self::OffTracking;
        }
        let sway_threshold = if matches!(self, Self::Swaying) {
            TRAILER_SWAY_CLEAR_DEG
        } else {
            TRAILER_SWAY_WARN_DEG
        };
        if event.sway_deg >= sway_threshold {
            Self::Swaying
        } else {
            Self::Steady
        }
    }

    pub const fn label(&self) -> &'static str {
        match self {
            Self::Steady => "steady",
            Self::Swaying => "swaying",
            Self::OffTracking => "off_tracking",
        }
    }
}

/// Steady-state rear-tandem off-tracking (ft) for a 53-foot single trailer
/// on a curve of `curvature_per_ft` (1 / radius_ft). Zero on gentle highway
/// curves; grows as `L^2 / (2R)` on tight ramps and street corners.
pub fn single_trailer_offtrack_ft(curvature_per_ft: f64) -> f64 {
    let k = curvature_per_ft.abs();
    if k < OFFTRACK_MIN_CURVATURE {
        return 0.0;
    }
    (0.5 * KINGPIN_TO_TANDEM_FT * KINGPIN_TO_TANDEM_FT * k).min(6.0)
}

/// Whether the tractor's lane offset (`-1` left edge, `+1` right edge) leaves
/// too little inside clearance for the trailing tandems on a tight turn.
/// Positive `curvature_per_ft` turns right; negative turns left.
pub fn single_trailer_offtrack_clipping(
    curvature_per_ft: f64,
    lane_offset: f64,
    speed_mph: f64,
) -> bool {
    if speed_mph < 5.0 {
        return false;
    }
    let offtrack_ft = single_trailer_offtrack_ft(curvature_per_ft);
    if offtrack_ft <= 0.0 {
        return false;
    }
    // Inside edge clearance in feet: hugging the inside of the turn shrinks it.
    let inside_offset = if curvature_per_ft > 0.0 {
        lane_offset
    } else {
        -lane_offset
    };
    let clearance_ft = (1.0 - inside_offset) * HALF_LANE_FT;
    offtrack_ft > clearance_ft + 1.5
}

/// Sustained crosswind lateral loading per unit `WeatherEffects::wind`
/// (`0.0..=1.0`), in g. Calibrated so `WeatherKind::Wind` (`0.9`) pushes an
/// empty or light high-profile box trailer past `TRAILER_SWAY_WARN_DEG` at
/// highway speed while clear/rain weather and loaded trailers stay steady.
pub const SUSTAINED_CROSSWIND_LATERAL_G: f64 = 0.16;

/// Combined crosswind lateral loading (g) from sustained weather wind
/// (`0.0..=1.0`, from `WeatherEffects::wind`) and transient gust acceleration.
pub fn crosswind_lateral_g(weather_wind: f64, gust_lateral_g: f64) -> f64 {
    weather_wind.max(0.0) * SUSTAINED_CROSSWIND_LATERAL_G + gust_lateral_g.abs()
}

/// Advances single-trailer crosswind sway angle (degrees) over `dt` seconds.
/// Lighter high-profile trailers at highway speeds sway more in strong winds.
pub fn single_trailer_sway_step(
    current_deg: f64,
    wind_lateral_g: f64,
    load_fraction: f64,
    speed_mph: f64,
    dt: f64,
) -> f64 {
    let lightness = (1.0 - 0.65 * load_fraction.clamp(0.0, 1.0)).clamp(0.35, 1.0);
    let speed_factor = ((speed_mph - 20.0) / 40.0).clamp(0.0, 1.3);
    let target = (wind_lateral_g.abs() * 36.0 * lightness * speed_factor).min(TRAILER_SWAY_MAX_DEG);
    let tau = if target > current_deg { 1.4 } else { 2.0 };
    let alpha = 1.0 - (-dt.max(0.0) / tau).exp();
    (current_deg + (target - current_deg) * alpha).clamp(0.0, TRAILER_SWAY_MAX_DEG)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn abs_transition_table() {
        let dry_hard = AbsEvent {
            braking_demand_n: 80_000.0,
            grip_limit_n: 60_000.0,
            effective_grip: 1.0,
            speed_mph: 55.0,
            dt: 0.02,
        };
        let slick_hard = AbsEvent {
            effective_grip: 0.45,
            ..dry_hard
        };
        let slick_light = AbsEvent {
            braking_demand_n: 20_000.0,
            ..slick_hard
        };
        let slick_slow = AbsEvent {
            speed_mph: 3.0,
            ..slick_hard
        };

        assert_eq!(AbsState::Inactive.next(dry_hard), AbsState::Inactive);
        let engaged = AbsState::Inactive.next(slick_hard);
        assert!(engaged.is_active());
        assert!(engaged.modulation_factor() >= ABS_MIN_FACTOR);
        assert!(engaged.modulation_factor() <= 1.0);

        let advanced = engaged.next(slick_hard);
        assert!(advanced.is_active());
        assert_ne!(engaged, advanced);

        assert_eq!(engaged.next(slick_light), AbsState::Inactive);
        assert_eq!(engaged.next(slick_slow), AbsState::Inactive);
        assert_eq!(engaged.next(dry_hard), AbsState::Inactive);
    }

    #[test]
    fn drive_traction_transition_table() {
        let dry_floor = DriveTractionEvent {
            drive_demand_n: 45_000.0,
            traction_limit_n: 30_000.0,
            effective_grip: 1.0,
            throttle: 1.0,
        };
        let snow_floor = DriveTractionEvent {
            effective_grip: 0.45,
            ..dry_floor
        };
        let snow_partial_still_high = DriveTractionEvent {
            drive_demand_n: 27_500.0,
            throttle: 0.6,
            ..snow_floor
        };
        let snow_feathered = DriveTractionEvent {
            drive_demand_n: 24_000.0,
            throttle: 0.4,
            ..snow_floor
        };
        let snow_off_throttle = DriveTractionEvent {
            throttle: 0.05,
            ..snow_floor
        };

        let cases = [
            (
                "dry floor stays grip",
                DriveTractionState::Grip,
                dry_floor,
                DriveTractionState::Grip,
            ),
            (
                "snow floor enters spinning",
                DriveTractionState::Grip,
                snow_floor,
                DriveTractionState::Spinning,
            ),
            (
                "spinning stays spinning inside hysteresis band",
                DriveTractionState::Spinning,
                snow_partial_still_high,
                DriveTractionState::Spinning,
            ),
            (
                "feathering throttle below hysteresis hooks back up",
                DriveTractionState::Spinning,
                snow_feathered,
                DriveTractionState::Grip,
            ),
            (
                "lifting throttle restores grip",
                DriveTractionState::Spinning,
                snow_off_throttle,
                DriveTractionState::Grip,
            ),
            (
                "returning to dry pavement restores grip",
                DriveTractionState::Spinning,
                dry_floor,
                DriveTractionState::Grip,
            ),
        ];

        for (label, start, event, expected) in cases {
            assert_eq!(start.next(event), expected, "{label}");
        }
        assert!(DriveTractionState::Spinning.traction_factor() < 1.0);
        assert_eq!(DriveTractionState::Grip.traction_factor(), 1.0);
    }

    #[test]
    fn trailer_sway_transition_table() {
        let base = TrailerSwayEvent {
            trailer_attached: true,
            doubles_hooked: false,
            sway_deg: 0.0,
            offtrack_clip: false,
        };
        let cases = [
            (
                "bobtail stays steady",
                TrailerSwayState::Steady,
                TrailerSwayEvent {
                    trailer_attached: false,
                    sway_deg: 6.0,
                    offtrack_clip: true,
                    ..base
                },
                TrailerSwayState::Steady,
            ),
            (
                "doubles defer to rear_trailer_whip",
                TrailerSwayState::Steady,
                TrailerSwayEvent {
                    doubles_hooked: true,
                    sway_deg: 6.0,
                    offtrack_clip: true,
                    ..base
                },
                TrailerSwayState::Steady,
            ),
            (
                "high sway enters Swaying",
                TrailerSwayState::Steady,
                TrailerSwayEvent {
                    sway_deg: 3.5,
                    ..base
                },
                TrailerSwayState::Swaying,
            ),
            (
                "Swaying holds above clear hysteresis",
                TrailerSwayState::Swaying,
                TrailerSwayEvent {
                    sway_deg: 2.3,
                    ..base
                },
                TrailerSwayState::Swaying,
            ),
            (
                "Swaying clears below hysteresis",
                TrailerSwayState::Swaying,
                TrailerSwayEvent {
                    sway_deg: 1.5,
                    ..base
                },
                TrailerSwayState::Steady,
            ),
            (
                "offtrack clip takes priority over Swaying",
                TrailerSwayState::Swaying,
                TrailerSwayEvent {
                    sway_deg: 4.0,
                    offtrack_clip: true,
                    ..base
                },
                TrailerSwayState::OffTracking,
            ),
        ];

        for (label, start, event, expected) in cases {
            assert_eq!(start.next(event), expected, "{label}");
        }
    }

    #[test]
    fn offtrack_triggers_on_tight_ramp_hugging_inside() {
        assert_eq!(single_trailer_offtrack_ft(0.0005), 0.0);
        assert!(single_trailer_offtrack_ft(0.005) > 3.5);
        // Right turn (curvature > 0), hugging inside right edge (offset +0.6)
        assert!(single_trailer_offtrack_clipping(0.005, 0.6, 25.0));
        // Swinging wide to the outside (offset -0.4) clears the inside edge
        assert!(!single_trailer_offtrack_clipping(0.005, -0.4, 25.0));
    }

    #[test]
    fn empty_trailer_sways_more_than_loaded_in_crosswind() {
        let mut empty = 0.0;
        let mut loaded = 0.0;
        for _ in 0..40 {
            empty = single_trailer_sway_step(empty, 0.14, 0.0, 62.0, 0.1);
            loaded = single_trailer_sway_step(loaded, 0.14, 1.0, 62.0, 0.1);
        }
        assert!(empty > TRAILER_SWAY_WARN_DEG);
        assert!(empty > loaded * 1.8);
    }

    #[test]
    fn integrated_truck_abs_engages_on_firm_slick_braking_only() {
        use crate::sim::vehicle::{TruckSpecs, TruckState};

        let mut dry = TruckState::new(TruckSpecs::default());
        dry.engine_on = true;
        dry.parking_brake = false;
        dry.velocity_mps = 22.0;
        dry.grip = 1.0;
        dry.brake = 1.0;
        dry.update(0.05);
        assert!(!dry.abs_active());

        let mut slick_light = TruckState::new(TruckSpecs::default());
        slick_light.engine_on = true;
        slick_light.parking_brake = false;
        slick_light.velocity_mps = 22.0;
        slick_light.grip = 0.5;
        slick_light.brake = 0.2;
        slick_light.update(0.05);
        assert!(
            !slick_light.abs_active(),
            "gentle braking on snow should not trigger ABS"
        );

        let mut slick_hard = TruckState::new(TruckSpecs::default());
        slick_hard.engine_on = true;
        slick_hard.parking_brake = false;
        slick_hard.velocity_mps = 22.0;
        slick_hard.grip = 0.45;
        slick_hard.brake = 0.9;
        slick_hard.update(0.05);
        assert!(
            !slick_hard.abs_active(),
            "an assist's capped pedal on snow is short of the wheels' limit"
        );
        slick_hard.brake = 1.0;
        slick_hard.update(0.05);
        assert!(
            slick_hard.abs_active(),
            "a full application on snow/ice should trigger ABS"
        );

        slick_hard.brake = 0.0;
        slick_hard.update(0.05);
        assert!(!slick_hard.abs_active(), "releasing brake clears ABS");
    }

    #[test]
    fn integrated_truck_wheelspin_on_winter_launch_and_recovers() {
        use crate::sim::vehicle::{TruckSpecs, TruckState};

        let mut truck = TruckState::new(TruckSpecs::default());
        truck.engine_on = true;
        truck.parking_brake = false;
        truck.transmission.gear = 1;
        truck.rpm = 1300.0;
        truck.velocity_mps = 2.0;
        truck.grip = 0.25;
        truck.throttle = 1.0;
        truck.update(0.05);
        assert!(
            truck.drive_wheels_spinning(),
            "full throttle in low gear on ice should spin drive wheels"
        );

        let spinning_force = truck.drive_force();
        truck.grip = 1.0;
        truck.update(0.05);
        assert!(!truck.drive_wheels_spinning());
        assert!(truck.drive_force() > spinning_force);
    }

    #[test]
    fn integrated_truck_single_trailer_stability_and_doubles_delegation() {
        use crate::sim::vehicle::{TrailerSet, TruckSpecs, TruckState};

        let mut single = TruckState::new(TruckSpecs::default());
        single.trailer_attached = true;
        single.cargo_kg = 0.0;
        single.velocity_mps = 28.0; // ~62.6 mph
        let mut state = TrailerSwayState::Steady;
        for _ in 0..40 {
            state = single.update_single_trailer_stability(0.1, 0.14, 0.0, 0.0);
        }
        assert_eq!(state, TrailerSwayState::Swaying);

        // Tight ramp hugging inside edge transitions to OffTracking
        let offtrack = single.update_single_trailer_stability(0.1, 0.0, 0.005, 0.6);
        assert_eq!(offtrack, TrailerSwayState::OffTracking);

        // Doubles delegate to trailer_set and stay Steady on single-trailer state
        let mut doubles = TruckState::new(TruckSpecs::default());
        doubles.trailer_attached = true;
        doubles.trailer_set = TrailerSet::for_trailer("double_van", 80_000.0);
        doubles.velocity_mps = 28.0;
        let dbl_state = doubles.update_single_trailer_stability(0.1, 0.2, 0.005, 0.6);
        assert_eq!(dbl_state, TrailerSwayState::Steady);
        assert_eq!(doubles.trailer_sway_deg, 0.0);
    }

    #[test]
    fn integrated_truck_jake_brake_traction_limits_on_ice() {
        use crate::sim::vehicle::{TruckSpecs, TruckState};

        let mut truck = TruckState::new(TruckSpecs::default());
        truck.engine_on = true;
        truck.parking_brake = false;
        truck.velocity_mps = 22.0;
        truck.transmission.gear = 6;
        truck.rpm = 1800.0;
        truck.throttle = 0.0;
        truck.engine_brake_stage = 3;

        truck.grip = 1.0;
        let dry_jake = truck.jake_brake_force();
        assert!(!truck.jake_slipping());

        truck.grip = 0.2;
        let ice_jake = truck.jake_brake_force();
        assert!(ice_jake < dry_jake);
        assert!(truck.jake_slipping());
    }

    #[test]
    fn braking_decel_and_stopping_distance_include_abs_modulation() {
        use crate::sim::vehicle::{BrakeApplication, TruckSpecs, TruckState};

        let mut truck = TruckState::new(TruckSpecs::default());
        truck.velocity_mps = 22.0;
        truck.parking_brake = false;
        truck.grip = 0.25;
        truck.brake = 1.0;

        let unmodulated_decel = truck.full_service_decel_mps2();
        let unmodulated_stop = truck.stopping_distance_m(None, 0.0, false);

        // Choose a phase where ABS modulation is at its trough (0.88).
        truck.abs_state = AbsState::Modulating {
            phase_s: 0.5 / ABS_CYCLE_HZ,
        };
        assert!(truck.abs_state.modulation_factor() < 1.0);

        let modulated_decel = truck.full_service_decel_mps2();
        let delivered_decel = truck.service_brake_force() / truck.gross_mass_kg();
        assert!((modulated_decel - delivered_decel).abs() < 1e-9);
        assert!(
            modulated_decel < unmodulated_decel,
            "ABS modulation must reduce estimated deceleration"
        );
        assert!(
            truck.stopping_distance_m(None, 0.0, false) > unmodulated_stop,
            "ABS modulation must lengthen estimated stopping distance"
        );
        assert!(
            (truck.braking_decel_mps2(BrakeApplication::Service(1.0)) - delivered_decel).abs()
                < 1e-9
        );
    }

    #[test]
    fn sustained_high_wind_triggers_sway_even_without_gust_acceleration() {
        use crate::sim::vehicle::{TruckSpecs, TruckState};
        use crate::sim::weather::{effects as weather_effects, WeatherKind};

        let wind_g = crosswind_lateral_g(weather_effects(WeatherKind::Wind).wind, 0.0);
        let mut empty = TruckState::new(TruckSpecs::default());
        empty.trailer_attached = true;
        empty.cargo_kg = 0.0;
        empty.velocity_mps = 28.0; // ~62.6 mph

        let mut state = TrailerSwayState::Steady;
        for _ in 0..50 {
            state = empty.update_single_trailer_stability(0.1, wind_g, 0.0, 0.0);
        }
        assert_eq!(
            state,
            TrailerSwayState::Swaying,
            "sustained high wind should sway an empty single trailer even with zero gust acceleration"
        );

        // Easing off to 35 mph recovers to Steady.
        empty.velocity_mps = 15.6;
        for _ in 0..80 {
            state = empty.update_single_trailer_stability(0.1, wind_g, 0.0, 0.0);
        }
        assert_eq!(state, TrailerSwayState::Steady);

        // A fully loaded trailer stays Steady in the same sustained wind.
        let mut loaded = TruckState::new(TruckSpecs::default());
        loaded.trailer_attached = true;
        loaded.cargo_kg = 20_000.0;
        loaded.velocity_mps = 28.0;
        let mut loaded_state = TrailerSwayState::Steady;
        for _ in 0..50 {
            loaded_state = loaded.update_single_trailer_stability(0.1, wind_g, 0.0, 0.0);
        }
        assert_eq!(loaded_state, TrailerSwayState::Steady);
    }

    #[test]
    fn uphill_wheelspin_does_not_trigger_fictitious_pull_downshift() {
        use crate::sim::vehicle::{TruckSpecs, TruckState};

        let mut truck = TruckState::new(TruckSpecs::default());
        truck.engine_on = true;
        truck.parking_brake = false;
        truck.trailer_attached = true;
        truck.cargo_kg = 15_000.0;
        truck.grade = 0.06;
        truck.grip = 0.2;
        truck.throttle = 1.0;
        truck.transmission.gear = 6;
        truck.velocity_mps = 12.0;
        truck.rpm = truck.coupled_rpm(Some(6));
        truck.drive_traction_state = DriveTractionState::Spinning;

        // Sanity check: we are traction-limited and losing ground on the climb.
        assert!(truck.drive_force() < truck.resistance_force());
        assert!(truck.drive_force_demand() > truck.drive_traction_limit());

        let starting_gear = truck.transmission.gear;
        for _ in 0..30 {
            truck.update(0.1);
            // Keep speed/rpm in the mid-band where only a fictitious pull downshift would fire.
            truck.velocity_mps = 12.0;
            truck.rpm = truck.coupled_rpm(Some(truck.transmission.gear));
        }
        assert_eq!(
            truck.transmission.gear, starting_gear,
            "auto_shift must not pull-downshift during traction-limited uphill wheelspin"
        );
    }
}
