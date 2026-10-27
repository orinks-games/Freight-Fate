//! Company in the other lanes of a busy freeway.
//!
//! A bubble cell carries at most one vehicle, and every cell is drawn as it
//! enters the window three miles ahead, so the only traffic a truck at road
//! speed ever meets is traffic slower than it: through the I-65 rush-hour
//! zone the cab heard two vehicles in twenty-five miles (owner drive,
//! 2026-10-09). Each new freeway cell now also gives the lanes left of the
//! right lane a chance at a passer, placed behind the truck well out of
//! earshot, that comes up and goes by. The right lane is left alone on
//! purpose: it is the truck's lane, and a vehicle there is a slowdown or a
//! tailgater the driver has to answer, where one in the lane beside is
//! company.

use super::{choose, TrafficManager, TrafficVehicle, EXIT_AFTER_MAX_MI, EXIT_AFTER_MIN_MI};
use super::{NO_SPAWN_BEHIND_MI, SPAWN_CELL_MI};
use crate::pyrandom::PyRandom;

/// Where a passer is placed behind the truck: just past the clear air
/// nothing is created in, well beyond the farthest a vehicle is heard, so it
/// comes into hearing rather than appearing in it, and near enough to reach
/// the cab before it turns off.
pub const BESIDE_BEHIND_MI: (f64, f64) = (NO_SPAWN_BEHIND_MI, NO_SPAWN_BEHIND_MI + 0.3);
/// The chance a new cell's lane gets a passer, as a share of the road's
/// density: about one every two minutes per lane at highway speed on a busy
/// road, a few in hearing at once.
pub const BESIDE_SHARE: f64 = 0.3;

impl TrafficManager {
    /// The passers a newly drawn freeway `cell` sends up the lanes beside
    /// the truck, drawn from the cell's `rng` after its own vehicle so that
    /// one is placed exactly as before. Empty off a freeway.
    pub(super) fn lanes_beside(
        &self,
        cell: i64,
        density: f64,
        taken_lane: i64,
        position_mi: f64,
        rng: &mut PyRandom,
    ) -> Vec<TrafficVehicle> {
        let cell_mid = (cell as f64 + 0.5) * SPAWN_CELL_MI;
        if self.freeway_presence_at(cell_mid) <= 0.0 || self.freeway_presence_at(position_mi) <= 0.0
        {
            return Vec::new();
        }
        let lanes = self.lane_count_at(position_mi);
        let mut beside = Vec::new();
        for lane in 1..lanes {
            if lane == taken_lane || rng.random() > density * BESIDE_SHARE {
                continue;
            }
            let mile = position_mi - rng.uniform(BESIDE_BEHIND_MI.0, BESIDE_BEHIND_MI.1);
            if mile <= 0.0 {
                continue;
            }
            let Some(leg) = self.leg_at(mile) else {
                continue;
            };
            let intent = "passing";
            let vehicle_class = choose(
                rng,
                &["car", "box truck", "semi", "service vehicle"],
                &[5.0, 1.4, 2.0, 0.3],
            );
            let (limit_offset, governor) = Self::intent_speed_draw(intent, rng, vehicle_class);
            let rush_slowdown = if self.rush_hour_traffic_bias(leg) != 0.0 {
                rng.uniform(4.0, 10.0)
            } else {
                0.0
            };
            let speed = self.road_speed_mph(mile, limit_offset, governor, rush_slowdown);
            let exit_at = mile + rng.uniform(EXIT_AFTER_MIN_MI, EXIT_AFTER_MAX_MI);
            beside.push(
                TrafficVehicle::new(
                    &format!("bubble:{cell}:{lane}"),
                    mile,
                    speed,
                    speed,
                    -lane,
                    intent,
                    vehicle_class,
                )
                .with_lane(lane)
                .with_exit_at(Some(exit_at))
                .with_speed_draw(limit_offset, governor, rush_slowdown),
            );
        }
        beside
    }
}
