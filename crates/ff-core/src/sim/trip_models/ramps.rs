//! Acceleration and deceleration lanes, ramp design speeds and a loaded
//! truck's merge speed (the ramp half of `freight_fate/sim/trip_models.py`).

use crate::pyfmt::round_py;
use crate::sim::vehicle::TruckState;

// -- Getting onto the highway: the acceleration lane -------------------------
// HOW LONG IS THE LANE comes from AASHTO Green Book Table 10-3 (TxDOT RDM
// Table 3-13): feet of acceleration lane from a STOP by design speed. HOW
// FAST THE TRUCK GETS comes from Long, TRR 1737 (2000), for a loaded 200
// lb/hp WB-15. Keeping both is the point: the lane is sized for a car and the
// truck is a truck.
pub const ACCELERATION_LANE_FT: [(f64, f64); 7] = [
    (40.0, 360.0),
    (50.0, 720.0),
    (55.0, 960.0),
    (60.0, 1200.0),
    (65.0, 1410.0),
    (70.0, 1620.0),
    (75.0, 1790.0),
];

/// Long's model: a = ALPHA - BETA * v, feet and feet per second.
pub const TRUCK_ACCEL_ALPHA_FPS2: f64 = 1.90;
pub const TRUCK_ACCEL_BETA: f64 = 0.0199;
pub const GRADE_MODEL_MIN_PCT: f64 = -4.0;
pub const GRADE_MODEL_MAX_PCT: f64 = 2.0;

/// AASHTO's acceleration-lane design target: enter the mainline at 75 percent
/// of its design speed. This is the traffic-relative floor used for both the
/// assist handoff and the truthful slow-merge warning.
pub const MERGE_TRAFFIC_SPEED_SHARE: f64 = 0.75;

/// AASHTO's own grade multipliers on the lane length (TxDOT Table 3-14).
pub const ACCELERATION_LANE_GRADE_FACTOR: [(f64, f64); 4] =
    [(-3.0, 0.6), (-5.0, 0.55), (3.0, 1.5), (5.0, 2.2)];

fn interpolate_lane_ft(table: &[(f64, f64)], highway_mph: f64) -> f64 {
    // The tables are written in ascending speed order.
    let (first_speed, first_ft) = table[0];
    let (last_speed, last_ft) = table[table.len() - 1];
    if highway_mph <= first_speed {
        return first_ft;
    }
    if highway_mph >= last_speed {
        return last_ft;
    }
    let lo = table
        .iter()
        .filter(|(s, _)| *s <= highway_mph)
        .map(|(s, _)| *s)
        .fold(f64::NEG_INFINITY, f64::max);
    let hi = table
        .iter()
        .filter(|(s, _)| *s >= highway_mph)
        .map(|(s, _)| *s)
        .fold(f64::INFINITY, f64::min);
    let ft_at = |s: f64| {
        table
            .iter()
            .find(|(k, _)| *k == s)
            .map(|(_, v)| *v)
            .expect("speed is a table key")
    };
    if lo == hi {
        return ft_at(lo);
    }
    let span = (highway_mph - lo) / (hi - lo);
    ft_at(lo) + span * (ft_at(hi) - ft_at(lo))
}

/// Miles of acceleration lane an entrance at `highway_mph` really has,
/// interpolated between the table's design speeds then adjusted for grade.
pub fn acceleration_lane_mi(highway_mph: f64, grade_pct: f64) -> f64 {
    let feet = interpolate_lane_ft(&ACCELERATION_LANE_FT, highway_mph);
    let mut factor = 1.0;
    for (threshold, value) in ACCELERATION_LANE_GRADE_FACTOR {
        let downhill_enough = threshold < 0.0 && grade_pct <= threshold;
        let uphill_enough = threshold > 0.0 && grade_pct >= threshold;
        if downhill_enough || uphill_enough {
            factor = value;
        }
    }
    feet * factor / 5280.0
}

/// Traffic-relative speed at which an acceleration-lane merge is no longer a
/// materially slow join.
pub fn merge_traffic_target_mph(highway_mph: f64) -> f64 {
    highway_mph.max(0.0) * MERGE_TRAFFIC_SPEED_SHARE
}

/// The longest the merge after a slow taper may keep the clock at real time,
/// in real seconds, however the truck is driven.
///
/// DERIVED from Long's loaded design truck (the `TRUCK_ACCEL_*` curve
/// above): from a standstill it reaches the merge speed of the fastest road
/// the lane table sizes, 75 percent of 75 mph, in about 100 seconds and a
/// mile of road. A truck that has not joined traffic by then is not still
/// merging; it is driving slowly, and slow driving runs on the trip's own
/// pacing. Before this bound the handoff only let go at the merge speed, so
/// a driver who settled below it kept a relaxed haul on the wall clock for
/// three hours (issue 293).
pub const MERGE_RECOVERY_MAX_REAL_S: f64 = 120.0;

/// The same bound in road: the mainline the truck has covered since the
/// taper. DERIVED as above; the design truck needs about 1.03 miles from a
/// standstill, so a mile and a half is the join with room to spare.
pub const MERGE_RECOVERY_MAX_MI: f64 = 1.5;

/// What this exact truck can reach by the taper with its actual drivetrain,
/// load, transmission, wear, weather drag and grip, and the mapped grade.
///
/// The clone is an instrument only: the live truck still covers every foot
/// and gains every mile per hour through the ordinary vehicle physics.
pub fn acceleration_lane_capability_mph(truck: &TruckState, lane_mi: f64, grade: f64) -> f64 {
    const DT: f64 = 1.0 / 60.0;
    const TIMEOUT_S: f64 = 180.0;

    let mut simulated = truck.clone();
    let start_mi = simulated.odometer_mi;
    simulated.grade = grade;
    simulated.brake = 0.0;
    let mut elapsed = 0.0;
    while simulated.odometer_mi - start_mi < lane_mi.max(0.0) && elapsed < TIMEOUT_S {
        simulated.throttle = 1.0;
        simulated.auto_shift();
        simulated.update(DT);
        elapsed += DT;
    }
    simulated.speed_mph()
}

// -- Getting off the highway: the deceleration lane --------------------------
// READ: AASHTO Green Book 2018 (7th ed.) Table 10-6, "Minimum Deceleration
// Lane Lengths for Exit Terminals with Flat Grades of Less Than 3 Percent",
// as reproduced in the NCHRP 15-75 appendices (Report 1081, 2024) and
// matching WSDOT Design Manual Exhibit 1360-11. Earlier editions numbered it
// 10-5, and so did this comment until 2026-09-24, when it also had 40 mph at
// 315 feet (the book says 320) and no 45 mph row at all.
//
// Feet of lane from the point the lane is full width to the ramp's
// controlling feature, by highway design speed (rows) and the design speed
// of that controlling feature (columns). Column 0 is the Stop condition. A
// row ends where the book prints "--": a ramp feature that fast off a road
// that slow is not a combination the table sizes.
pub const DECELERATION_LANE_RAMP_MPH: [f64; 9] =
    [0.0, 15.0, 20.0, 25.0, 30.0, 35.0, 40.0, 45.0, 50.0];
pub const DECELERATION_LANE_FT: [(f64, &[f64]); 11] = [
    (30.0, &[235.0, 200.0, 170.0, 140.0]),
    (35.0, &[280.0, 250.0, 210.0, 185.0, 150.0]),
    (40.0, &[320.0, 295.0, 265.0, 235.0, 185.0, 155.0]),
    (45.0, &[385.0, 350.0, 325.0, 295.0, 250.0, 220.0]),
    (
        50.0,
        &[435.0, 405.0, 385.0, 355.0, 315.0, 285.0, 225.0, 175.0],
    ),
    (
        55.0,
        &[480.0, 455.0, 440.0, 410.0, 380.0, 350.0, 285.0, 235.0],
    ),
    (
        60.0,
        &[
            530.0, 500.0, 480.0, 460.0, 430.0, 405.0, 350.0, 300.0, 240.0,
        ],
    ),
    (
        65.0,
        &[
            570.0, 540.0, 520.0, 500.0, 470.0, 440.0, 390.0, 340.0, 280.0,
        ],
    ),
    (
        70.0,
        &[
            615.0, 590.0, 570.0, 550.0, 520.0, 490.0, 440.0, 390.0, 340.0,
        ],
    ),
    (
        75.0,
        &[
            660.0, 635.0, 620.0, 600.0, 575.0, 535.0, 490.0, 440.0, 390.0,
        ],
    ),
    (
        80.0,
        &[
            705.0, 680.0, 665.0, 645.0, 620.0, 580.0, 535.0, 490.0, 440.0,
        ],
    ),
];

/// READ: Green Book 2018 Table 10-5, the DECELERATION column (TxDOT RDM Table
/// 4-19, WSDOT Exhibit 1360-11): `(grade percent, factor)`. Upgrades help a
/// truck shed speed, so less lane; downgrades fight it, so more. The book's
/// bands are "3 to 4" and "5 to 6" percent; WSDOT's reading, "3 to less than
/// 5" and "5 or more", closes the gap between them and is what this uses.
pub const DECELERATION_LANE_GRADE_FACTOR: [(f64, f64); 4] =
    [(3.0, 0.9), (5.0, 0.8), (-3.0, 1.2), (-5.0, 1.35)];

/// Feet of Table 10-6 for one highway row at one ramp speed, clamped to the
/// columns the row prints.
fn deceleration_row_ft(row: &[f64], ramp_mph: f64) -> f64 {
    let columns = &DECELERATION_LANE_RAMP_MPH[..row.len()];
    let table: Vec<(f64, f64)> = columns.iter().copied().zip(row.iter().copied()).collect();
    interpolate_lane_ft(&table, ramp_mph)
}

/// AASHTO ramp design speed as a share of the mainline: directional ramps
/// take the top of the 70-85 percent band, surface-road ramps the lower end.
pub const RAMP_DIRECTIONAL_SHARE: f64 = 0.85;
pub const RAMP_SURFACE_SHARE: f64 = 0.70;
pub const RAMP_MIN_DESIGN_MPH: f64 = 30.0;

/// Miles of deceleration lane an exit at `highway_mph` has for a ramp whose
/// controlling feature is designed for `ramp_mph` (0 for a stop), on the
/// mainline's `grade_pct`. Interpolated both ways across Table 10-6 and
/// clamped to its edges, then multiplied by the book's own deceleration
/// grade factor.
pub fn deceleration_lane_mi(highway_mph: f64, ramp_mph: f64, grade_pct: f64) -> f64 {
    let rows: Vec<(f64, f64)> = DECELERATION_LANE_FT
        .iter()
        .map(|(speed, row)| (*speed, deceleration_row_ft(row, ramp_mph)))
        .collect();
    let feet = interpolate_lane_ft(&rows, highway_mph);
    let mut factor = 1.0;
    for (threshold, value) in DECELERATION_LANE_GRADE_FACTOR {
        let downhill_enough = threshold < 0.0 && grade_pct <= threshold;
        let uphill_enough = threshold > 0.0 && grade_pct >= threshold;
        if downhill_enough || uphill_enough {
            factor = value;
        }
    }
    feet * factor / 5280.0
}

// -- The ramp past the deceleration lane -------------------------------------
// Nothing about an exit's shape is baked (research 2026-09-24, section 7): no
// per-exit length, deflection or grade. So the ramp is three pieces, each
// labelled for what it is.

/// ASSUMED: how far the ramp's controlling curve turns, 45 degrees. A diamond
/// ramp bends off the mainline toward its crossroad; nothing records by how
/// much, and this is a middling bend rather than a loop. Its radius is not
/// assumed -- it is the ramp speed's own AASHTO minimum, `min_radius_ft`.
pub const RAMP_CURVE_DEFLECTION_RAD: f64 = std::f64::consts::FRAC_PI_4;
/// DERIVED: the climb or drop to the crossroad. A grade separation of about
/// 23.5 feet (the middle of the 22-25 foot band in the research) taken at
/// the 4 percent a ramp grade should preferably stay under (Green Book / TxDOT
/// RDM Table 15-2, READ) is 23.5 / 0.04 = 587 feet, rounded.
pub const RAMP_TANGENT_CLIMB_FT: f64 = 590.0;
/// ASSUMED: storage for the queue at the terminal. Five vehicles at TxDOT's
/// 40 feet per vehicle for 15 to 19 percent trucks (RDM Table 4-14, READ);
/// the vehicle count is the assumption.
pub const RAMP_QUEUE_FT: f64 = 200.0;
/// READ: the deceleration the Green Book's stopping sight distance is built
/// on, 11.2 ft/s^2 (2018 section 3.2.2). A measured ramp too short to hold
/// its curve and a stop from the curve speed at that rate keeps that stop
/// anyway: the curve is our assumed shape, the stop at the bar is not.
pub const RAMP_STOP_DECEL_FT_S2: f64 = 11.2;

/// One exit ramp, gore to stop bar: the deceleration lane, the controlling
/// curve, and the tangent run down to the terminal.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ExitRampLayout {
    pub decel_mi: f64,
    pub curve_mi: f64,
    pub tangent_mi: f64,
    /// The speed the curve is built for, which the deceleration lane is
    /// sized to reach.
    pub curve_mph: f64,
    /// The speed the run past the curve is held to: the ramp's own design
    /// speed. The advisory sign is for the curve (MUTCD W13-2 stands where
    /// the curve's speed applies), so once the curve is behind the truck
    /// the sign's number no longer binds it. Never under `curve_mph`.
    pub tangent_mph: f64,
    /// ASSUMED 0 percent: the ramp past the deceleration lane has no baked
    /// grade, and a guessed climb or drop would read as a survey.
    pub grade: f64,
}

impl ExitRampLayout {
    /// The whole ramp, gore to stop bar.
    pub fn length_mi(&self) -> f64 {
        self.decel_mi + self.curve_mi + self.tangent_mi
    }
}

/// Miles of the ramp's controlling curve: its AASHTO minimum radius at the
/// ramp speed, turned through [`RAMP_CURVE_DEFLECTION_RAD`].
pub fn ramp_curve_mi(ramp_mph: f64) -> f64 {
    crate::data::curves::min_radius_ft(ramp_mph) * RAMP_CURVE_DEFLECTION_RAD / 5280.0
}

/// Lay out an exit ramp. `length_mi` is a measured gore-to-terminal length
/// when one is known; the lane and the curve are never shortened for it, so
/// a length shorter than those two is read as having no tangent at all.
pub fn exit_ramp_layout(
    highway_mph: f64,
    ramp_mph: f64,
    mainline_grade_pct: f64,
    length_mi: Option<f64>,
) -> ExitRampLayout {
    let decel_mi = deceleration_lane_mi(highway_mph, ramp_mph, mainline_grade_pct);
    let curve_mi = ramp_curve_mi(ramp_mph);
    let tangent_mi = match length_mi {
        Some(length) => {
            let curve_fps = ramp_mph * 5280.0 / 3600.0;
            let stop_mi = curve_fps * curve_fps / (2.0 * RAMP_STOP_DECEL_FT_S2) / 5280.0;
            (length - decel_mi - curve_mi).max(stop_mi)
        }
        None => (RAMP_TANGENT_CLIMB_FT + RAMP_QUEUE_FT) / 5280.0,
    };
    ExitRampLayout {
        decel_mi,
        curve_mi,
        tangent_mi,
        curve_mph: ramp_mph,
        tangent_mph: ramp_mph,
        grade: 0.0,
    }
}

/// The speed this ramp is built for, from the road it leaves.
pub fn ramp_speed_mph(highway_mph: f64, directional: bool) -> f64 {
    let share = if directional {
        RAMP_DIRECTIONAL_SHARE
    } else {
        RAMP_SURFACE_SHARE
    };
    RAMP_MIN_DESIGN_MPH.max(round_py(highway_mph * share))
}

/// Provenance travels with ramp speed so a calculated design fallback cannot
/// be mistaken for an observed advisory sign.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RampAdvisorySpeed {
    /// A roadside value was read, but the truck target remains no faster than
    /// Freight Fate's existing conservative ramp calculation. Ordinary ramp
    /// advisory signs are generally intended for passenger vehicles.
    Observed {
        posted_mph: f64,
        truck_target_mph: f64,
    },
    Calculated {
        mph: f64,
    },
}

impl RampAdvisorySpeed {
    pub fn mph(self) -> f64 {
        match self {
            Self::Observed {
                truck_target_mph, ..
            } => truck_target_mph,
            Self::Calculated { mph } => mph,
        }
    }
}

/// What a loaded truck is really doing at the end of that lane: Long's curve
/// integrated over the lane, capped at the highway's own limit.
pub fn truck_merge_speed_mph(highway_mph: f64, entry_mph: f64, lane_mi: f64) -> f64 {
    let mut v = entry_mph.max(0.0) * 5280.0 / 3600.0; // feet per second
    let top = TRUCK_ACCEL_ALPHA_FPS2 / TRUCK_ACCEL_BETA;
    let mut remaining = lane_mi.max(0.0) * 5280.0;
    let step: f64 = 10.0;
    while remaining > 0.0 && v < top {
        let accel = TRUCK_ACCEL_ALPHA_FPS2 - TRUCK_ACCEL_BETA * v;
        if accel <= 0.0 {
            break;
        }
        // v dv = a dx
        v = (v * v + 2.0 * accel * step.min(remaining)).max(0.0).sqrt();
        remaining -= step;
    }
    highway_mph.min(v * 3600.0 / 5280.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::trucks::truck_model_or_panic;
    use crate::sim::vehicle::{KG_PER_TON, MPS_TO_MPH};

    fn automatic_truck(model: &str, cargo_tons: f64) -> TruckState {
        let mut truck = TruckState::new(truck_model_or_panic(model).specs.clone());
        truck.engine_on = true;
        truck.transmission.automatic = true;
        // Brandon's 18 mph ramp approach was recorded in sixth at 1,412 RPM.
        // Neutral here would exercise the automatic's standing-start selector,
        // not the rolling Carlisle handoff this capability estimate models.
        truck.transmission.gear = 6;
        truck.rpm = 1412.0;
        truck.trailer_attached = true;
        truck.cargo_kg = cargo_tons * KG_PER_TON;
        truck.velocity_mps = 18.0 / MPS_TO_MPH;
        truck
    }

    #[test]
    fn merge_target_tracks_traffic_speed_instead_of_a_fixed_shortfall() {
        assert_eq!(merge_traffic_target_mph(70.0), 52.5);
        assert_eq!(merge_traffic_target_mph(55.0), 41.25);
    }

    #[test]
    fn the_merge_recovery_bound_covers_the_design_truck_join() {
        // Long's curve in closed form from a standstill: v(t) = a/b (1 - e^-bt).
        let (a, b) = (TRUCK_ACCEL_ALPHA_FPS2, TRUCK_ACCEL_BETA);
        let target_fps = merge_traffic_target_mph(75.0) * 5280.0 / 3600.0;
        let t = -(1.0 - b * target_fps / a).ln() / b;
        let feet = a / b * t - a / (b * b) * (1.0 - (-b * t).exp());
        assert!((t - 100.3).abs() < 0.1, "{t}");
        assert!(t < MERGE_RECOVERY_MAX_REAL_S, "{t}");
        assert!(feet / 5280.0 < MERGE_RECOVERY_MAX_MI, "{feet}");
        // A bound, not a second way to stay pinned: well under three minutes.
        const { assert!(MERGE_RECOVERY_MAX_REAL_S <= 180.0) };
    }

    #[test]
    fn a_measured_ramp_fits_its_run_and_never_loses_room_to_stop() {
        let lane = deceleration_lane_mi(70.0, 45.0, 0.0);
        let curve = ramp_curve_mi(45.0);
        let long = exit_ramp_layout(70.0, 45.0, 0.0, Some(lane + 1500.0 / 5280.0));
        assert!((long.length_mi() - (lane + 1500.0 / 5280.0)).abs() < 1e-9);
        assert!((long.tangent_mi - (1500.0 / 5280.0 - curve)).abs() < 1e-9);
        // 300 ft of ramp cannot hold a 45 mph curve and a stop: the run keeps
        // the Green Book stopping distance, 66^2 / 22.4 = 194.5 ft.
        let short = exit_ramp_layout(70.0, 45.0, 0.0, Some(lane + 300.0 / 5280.0));
        assert!((short.tangent_mi * 5280.0 - 194.46).abs() < 0.1);
    }

    fn lane_ft(highway_mph: f64, ramp_mph: f64, grade_pct: f64) -> f64 {
        (deceleration_lane_mi(highway_mph, ramp_mph, grade_pct) * 5280.0 * 1000.0).round() / 1000.0
    }

    #[test]
    fn deceleration_lane_reads_green_book_table_10_6() {
        // The two rows the old one-column table had wrong or missing.
        assert_eq!(lane_ft(40.0, 0.0, 0.0), 320.0);
        assert_eq!(lane_ft(45.0, 0.0, 0.0), 385.0);
        // The curve columns, not just Stop.
        assert_eq!(lane_ft(70.0, 30.0, 0.0), 520.0);
        assert_eq!(lane_ft(70.0, 45.0, 0.0), 390.0);
        assert_eq!(lane_ft(60.0, 50.0, 0.0), 240.0);
        // Between rows and columns it interpolates.
        assert_eq!(lane_ft(67.5, 30.0, 0.0), 495.0);
        assert_eq!(lane_ft(70.0, 32.5, 0.0), 505.0);
        // A ramp faster than the row prints takes the row's last column.
        assert_eq!(lane_ft(40.0, 45.0, 0.0), 155.0);
    }

    #[test]
    fn deceleration_grade_factors_are_the_books_own() {
        let flat = lane_ft(70.0, 35.0, 0.0);
        assert_eq!(flat, 490.0);
        for (grade, factor) in [
            (2.9, 1.0),
            (3.5, 0.9),
            (6.0, 0.8),
            (-3.5, 1.2),
            (-6.0, 1.35),
        ] {
            let feet = lane_ft(70.0, 35.0, grade);
            assert!((feet - flat * factor).abs() < 0.01, "{grade}: {feet}");
        }
    }

    #[test]
    fn the_default_ramp_lands_in_the_derived_band() {
        // Research section 5: roughly 1,200 to 2,000 feet, gore to terminal.
        for (highway, ramp) in [(55.0, 40.0), (65.0, 45.0), (70.0, 30.0), (75.0, 50.0)] {
            let layout = exit_ramp_layout(highway, ramp, 0.0, None);
            let feet = layout.length_mi() * 5280.0;
            assert!(
                (1_000.0..=2_200.0).contains(&feet),
                "{highway}/{ramp}: {feet}"
            );
            assert_eq!(layout.grade, 0.0);
        }
        // A measured length keeps the lane and the curve and fits the tangent.
        let measured = exit_ramp_layout(70.0, 35.0, 0.0, Some(0.4));
        assert!((measured.length_mi() - 0.4).abs() < 1e-9);
        assert_eq!(measured.decel_mi, deceleration_lane_mi(70.0, 35.0, 0.0));
    }

    #[test]
    fn capability_uses_the_actual_truck_and_load() {
        let lane_mi = acceleration_lane_mi(70.0, -1.1);
        let loaded =
            acceleration_lane_capability_mph(&automatic_truck("yard_mule", 25.0), lane_mi, -0.011);
        let light = acceleration_lane_capability_mph(&automatic_truck("rig", 0.0), lane_mi, -0.011);

        assert!(loaded > 18.0);
        assert!(light > loaded, "loaded={loaded:.1}, light={light:.1}");
    }
}
