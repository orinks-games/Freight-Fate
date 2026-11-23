//! Oncoming traffic on undivided two-lane highways.
//!
//! `beside.rs` populates the extra lanes of a multi-lane freeway, leaving
//! undivided two-lane corridors (the Alaska Highway, northern Canadian routes,
//! and rural US highways) with no opposing traffic at all. Each new two-lane
//! cell now gives the oncoming lane a seeded chance at a vehicle heading toward
//! the cab. Oncoming vehicles travel with `oncoming = true` and `lane = -1`,
//! so they stay out of same-direction platoon, lead, and sideswipe checks while
//! passing cleanly past the left ear with true combined Doppler pitch.

use super::{freeway_class, TrafficManager, TrafficVehicle, NO_SPAWN_AHEAD_MI, SPAWN_CELL_MI};
use crate::pyrandom::PyRandom;

/// Share of road density that spawns an oncoming vehicle per two-lane cell.
pub const ONCOMING_SHARE: f64 = 0.50;
/// Minimum chance on very quiet two-lane corridors so rural roads still see
/// occasional oncoming traffic.
pub const ONCOMING_FLOOR: f64 = 0.14;

impl TrafficManager {
    /// Optional oncoming vehicle for a newly drawn undivided two-lane `cell`,
    /// drawn from the cell's `rng` after `lanes_beside` so existing freeway
    /// draws are untouched.
    pub(super) fn oncoming_for_cell(
        &self,
        cell: i64,
        density: f64,
        position_mi: f64,
        rng: &mut PyRandom,
    ) -> Option<TrafficVehicle> {
        let cell_mid = (cell as f64 + 0.5) * SPAWN_CELL_MI;
        if self.freeway_presence_at(cell_mid) > 0.0 || self.lane_count_at(cell_mid) != 1 {
            return None;
        }
        let chance = (density * ONCOMING_SHARE).clamp(ONCOMING_FLOOR, 0.55);
        if rng.random() > chance {
            return None;
        }
        let mile = (cell as f64 + rng.uniform(0.15, 0.85)) * SPAWN_CELL_MI;
        if mile <= position_mi + NO_SPAWN_AHEAD_MI || mile > self.route.miles() {
            return None;
        }
        if self.leg_at(mile).is_none()
            || self.freeway_presence_at(mile) > 0.0
            || self.lane_count_at(mile) != 1
        {
            return None;
        }
        let vehicle_class = freeway_class(rng);
        let limit = self.posted_limit_at(mile);
        let limit_share = rng.uniform(0.92, 1.06);
        let speed = (limit * limit_share).max(25.0);
        Some(
            TrafficVehicle::new(
                &format!("oncoming:{cell}"),
                mile,
                speed,
                speed,
                -1,
                "oncoming",
                vehicle_class,
            )
            .with_oncoming(true)
            .with_speed_draw(limit_share, None, 0.0),
        )
    }
}
