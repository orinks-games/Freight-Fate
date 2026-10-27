//! Traffic at a lane closure.
//!
//! Where roadwork cones off a lane, the traffic in the lane beside it lets a
//! truck in, the way drivers at a real lane drop do. It used to drive on as
//! if the truck were not there, so a stream of cars up the open lane held it
//! for as long as the stream lasted: the truck stopped dead in the closed
//! lane, and a "Left lane open" read from the stop went stale while it got
//! rolling, so the merge it invited sideswiped a car that had come up from
//! behind meanwhile (player report, 2026-10-16).
//!
//! The vehicles that wait are the ones still behind the sideswipe window.
//! One already alongside goes on by: pulling it back would be a jump the
//! driver hears, and it clears the truck on its own in a few seconds.

use super::{
    TrafficManager, TrafficVehicle, DODGE_CLEARANCE_BEHIND_MI, HOLD_BEHIND_HEADWAY_S,
    LANE_GAP_ACT_REAL_S, LANE_GAP_MARGIN_MI,
};

/// How far behind the truck a vehicle making room for its merge waits, on
/// top of its own length and a following gap at the truck's speed: outside
/// the window the L key and the hazard call read, with some to spare, so a
/// lane they call open stays open while the truck moves over.
pub const MERGE_ROOM_MI: f64 = DODGE_CLEARANCE_BEHIND_MI + LANE_GAP_MARGIN_MI + 0.03;

impl TrafficManager {
    /// The lane the truck has to merge into: the open neighbour of the
    /// coned-off lane, while the truck is in that coned-off lane.
    pub(super) fn merge_room_lane(&self, truck_lanes: i64) -> Option<i64> {
        let closed = self.closed_lane.filter(|lane| *lane == self.player_lane)?;
        let open = if closed > 0 { closed - 1 } else { closed + 1 };
        (0..truck_lanes).contains(&open).then_some(open)
    }

    /// Where a vehicle in the merge lane waits behind the truck.
    pub(super) fn merge_room_at(&self, vehicle: &TrafficVehicle, position_mi: f64) -> f64 {
        position_mi
            - vehicle.length_mi
            - MERGE_ROOM_MI
            - self.truck_speed_mph.abs() * HOLD_BEHIND_HEADWAY_S / 3600.0
    }

    /// Whether a vehicle in the merge lane, where it was before this frame
    /// moved it, is still behind the sideswipe window and so waits.
    pub(super) fn makes_merge_room(&self, vehicle: &TrafficVehicle, position_mi: f64) -> bool {
        vehicle.position_mi <= position_mi - vehicle.length_mi - DODGE_CLEARANCE_BEHIND_MI
    }

    /// Hold a vehicle making room at its waiting point, and ease it in
    /// toward that point: never closing faster than it can cover the room
    /// left over the seconds the L key's reading looks ahead
    /// (`LANE_GAP_ACT_REAL_S`, on the clock `time_scale` runs). A car still
    /// coming up at fifty, a few hundred feet short of where it would stop,
    /// otherwise read as already in the gap.
    pub(super) fn hold_for_merge(
        &self,
        vehicle: &mut TrafficVehicle,
        position_mi: f64,
        time_scale: f64,
    ) {
        let truck_mph = self.truck_speed_mph.abs();
        let wait_at = self.merge_room_at(vehicle, position_mi);
        if vehicle.position_mi > wait_at {
            vehicle.position_mi = wait_at;
        }
        let horizon_hr = LANE_GAP_ACT_REAL_S * time_scale.max(f64::EPSILON) / 3600.0;
        let closing_mph = (wait_at - vehicle.position_mi) / horizon_hr;
        vehicle.speed_mph = vehicle.speed_mph.min(truck_mph + closing_mph);
    }
}
