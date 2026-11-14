//! Vehicles that see each other.
//!
//! Until 2026-11-05 every vehicle in the bubble drove its own line: a faster
//! one that caught a slower one in the same lane drove straight through it,
//! two sounds sat on one spot, and no platoon ever formed behind a slow
//! truck on a two-lane road. Now a driver who catches a slower vehicle does
//! what you do. With a lane to the left, and the room the L key needs before
//! it calls a lane open, they move over, pass, and come back once they are
//! clear. With no lane, or no room, they drop in two seconds behind and hold
//! that gap, so a queue forms where the road has one lane and a pass is
//! heard where it has two. The truck is a vehicle on this road like any
//! other: traffic behind it passes it or waits behind it, and nothing moves
//! into its lane inside the window the sideswipe test reads.
//!
//! Everything here reads the road as the frame FOUND it, so the order the
//! vehicles are stepped in cannot change what any of them decides.

use super::{
    TrafficManager, TrafficVehicle, DODGE_CLEARANCE_AHEAD_MI, DODGE_CLEARANCE_BEHIND_MI,
    HOLD_BEHIND_HEADWAY_S, LANE_GAP_ACT_REAL_S, LANE_GAP_MARGIN_MI,
};

/// How much faster than the vehicle ahead a driver wants to go before they
/// move over to pass it rather than settle in behind. Under this, a pass
/// would take miles, and a real driver sits.
pub const PASS_WANT_MPH: f64 = 3.0;
/// How far ahead a driver reads the vehicle in front: at this many seconds
/// of closing they either move over or start easing off, and easing off
/// brings them onto the following gap over about this long.
pub const DECIDE_AHEAD_S: f64 = 20.0;
/// The tractor and trailer on the road, for the gap kept behind the truck:
/// a 72-foot rig.
pub const TRUCK_LENGTH_MI: f64 = 0.014;
/// The least gap any vehicle keeps to the one ahead, whatever the speed.
pub const MIN_GAP_MI: f64 = 0.02;

/// One occupant of the road as the frame began. `position_mi` is its rear
/// and it reaches `length_mi` forward, the way `lead_vehicle` and the status
/// gap read a vehicle. `index` orders vehicles at the same mile (the later
/// one counts as ahead); the truck is `None`.
#[derive(Debug, Clone)]
pub(super) struct Slot {
    pub lane: i64,
    pub position_mi: f64,
    pub length_mi: f64,
    pub speed_mph: f64,
    pub index: Option<usize>,
    pub key: String,
}

impl Slot {
    fn is_ahead_of(&self, index: usize, position_mi: f64) -> bool {
        self.position_mi > position_mi
            || (self.position_mi == position_mi && self.index.is_none_or(|i| i > index))
    }
}

/// What a vehicle settling in behind another holds to: the occupant, and
/// the most it may do this frame while it closes onto its gap.
pub(super) struct Follow<'a> {
    pub ahead: &'a Slot,
    pub pace_mph: f64,
}

/// The road as this frame found it, for every decision the frame makes.
pub(super) struct Road {
    slots: Vec<Slot>,
    /// Game hours the lane-clearance read sweeps forward, on the clock the
    /// traffic actually moves on (`LANE_GAP_ACT_REAL_S` at the time scale).
    horizon_hr: f64,
}

impl TrafficManager {
    /// Everyone on the road before the frame moves anybody: the vehicles
    /// where they were, and the truck in its lane and in the lane a tap
    /// change is taking it into.
    pub(super) fn road_snapshot(
        &self,
        vehicles: &[TrafficVehicle],
        position_mi: f64,
        time_scale: f64,
    ) -> Road {
        let mut slots: Vec<Slot> = vehicles
            .iter()
            .enumerate()
            .filter(|(_, v)| v.lane >= 0)
            .map(|(index, v)| Slot {
                lane: v.lane,
                position_mi: v.position_mi,
                length_mi: v.length_mi,
                speed_mph: v.speed_mph,
                index: Some(index),
                key: v.key.clone(),
            })
            .collect();
        let truck_speed = self.truck_speed_mph.abs();
        let mut truck_lanes = vec![self.player_lane];
        if let Some(target) = self.player_lane_target {
            if target != self.player_lane {
                truck_lanes.push(target);
            }
        }
        for lane in truck_lanes {
            // The trip's mile is the truck's front; the rig reaches back.
            slots.push(Slot {
                lane,
                position_mi: position_mi - TRUCK_LENGTH_MI,
                length_mi: TRUCK_LENGTH_MI,
                speed_mph: truck_speed,
                index: None,
                key: "truck".to_string(),
            });
        }
        Road {
            slots,
            horizon_hr: LANE_GAP_ACT_REAL_S * time_scale.max(f64::EPSILON) / 3600.0,
        }
    }

    /// The nearest occupant ahead of this vehicle in its own lane.
    pub(super) fn company_ahead<'a>(
        &self,
        road: &'a Road,
        index: usize,
        vehicle: &TrafficVehicle,
    ) -> Option<&'a Slot> {
        road.slots
            .iter()
            .filter(|slot| {
                slot.lane == vehicle.lane
                    && slot.index != Some(index)
                    && slot.is_ahead_of(index, vehicle.position_mi)
            })
            .min_by(|a, b| {
                a.position_mi
                    .partial_cmp(&b.position_mi)
                    .expect("finite vehicle positions")
            })
    }

    /// Whether `lane` has room for this vehicle to move into: the window
    /// the L key and the hazard call read before they call a lane open,
    /// seen from the other side. Whoever is already in the lane keeps the
    /// clearance the truck would need: an occupant behind keeps the room
    /// ahead of it, an occupant ahead the room behind it, both swept forward
    /// through the relative motion the way the truck's own read is. So a
    /// vehicle never arrives in the truck's lane inside the sideswipe test.
    pub(super) fn lane_has_room(
        &self,
        road: &Road,
        index: usize,
        vehicle: &TrafficVehicle,
        lane: i64,
    ) -> bool {
        if self.closed_lane == Some(lane) {
            return false;
        }
        let ahead_mi = DODGE_CLEARANCE_AHEAD_MI + LANE_GAP_MARGIN_MI;
        let behind_mi = DODGE_CLEARANCE_BEHIND_MI + LANE_GAP_MARGIN_MI;
        !road.slots.iter().any(|slot| {
            if slot.lane != lane || slot.index == Some(index) {
                return false;
            }
            // Rear to rear, now and at the end of the sweep.
            let gap = slot.position_mi - vehicle.position_mi;
            let later = gap + (slot.speed_mph - vehicle.speed_mph) * road.horizon_hr;
            // Its rear closer than the room an occupant ahead keeps behind
            // it, and its front closer than the room an occupant behind
            // keeps ahead of it.
            gap.min(later) <= vehicle.length_mi + behind_mi
                && gap.max(later) + slot.length_mi >= -ahead_mi
        })
    }

    /// The gap this vehicle keeps to whatever is ahead of it, in miles.
    fn following_gap_mi(vehicle: &TrafficVehicle) -> f64 {
        MIN_GAP_MI.max(vehicle.speed_mph.max(0.0) * HOLD_BEHIND_HEADWAY_S / 3600.0)
    }

    /// Before the frame moves this vehicle: come back from a pass once the
    /// home lane has room, then look at what is ahead. A vehicle that wants
    /// to go faster than the one ahead moves over where the road has a lane
    /// with room in it; otherwise it settles in behind, and what it holds
    /// to is returned: a pace for this frame's target, and the occupant for
    /// [`Self::hold_behind`] after the move.
    pub(super) fn see_the_road<'a>(
        &self,
        road: &'a Road,
        index: usize,
        vehicle: &mut TrafficVehicle,
        lanes_here: i64,
    ) -> Option<Follow<'a>> {
        vehicle.holding_behind = None;
        if vehicle.lane < 0 {
            return None;
        }
        if let Some(home) = vehicle.home_lane {
            if home >= vehicle.lane {
                // The road folded the lanes back under it; nothing to return to.
                vehicle.home_lane = None;
            } else if self.lane_has_room(road, index, vehicle, home) {
                vehicle.lane = home;
                vehicle.home_lane = None;
            }
        }
        let ahead = self.company_ahead(road, index, vehicle)?;
        let gap = ahead.position_mi - (vehicle.position_mi + vehicle.length_mi);
        let closing = (vehicle.speed_mph - ahead.speed_mph).max(0.0);
        let horizon_hr = DECIDE_AHEAD_S / 3600.0;
        let excess = gap - Self::following_gap_mi(vehicle);
        if excess > closing * horizon_hr {
            return None;
        }
        let wants_past = vehicle.target_speed_mph >= ahead.speed_mph + PASS_WANT_MPH;
        let left = vehicle.lane + 1;
        if wants_past && left < lanes_here && self.lane_has_room(road, index, vehicle, left) {
            if vehicle.home_lane.is_none() {
                vehicle.home_lane = Some(vehicle.lane);
            }
            vehicle.lane = left;
            return None;
        }
        vehicle.holding_behind = Some(ahead.key.clone());
        // Close the rest of the way over the horizon, then match the pace.
        let pace_mph = ahead.speed_mph + excess.max(0.0) / horizon_hr;
        Some(Follow { ahead, pace_mph })
    }

    /// After the frame moved this vehicle: keep it its following gap behind
    /// the occupant ahead, at that occupant's pace. One already overlapping
    /// (two drawn on one spot, or a lane that folded under a pass) is not
    /// jumped back; it drops off a little slower than the one ahead until
    /// the gap is real.
    pub(super) fn hold_behind(&self, vehicle: &mut TrafficVehicle, ahead: &Slot, game_hours: f64) {
        let moved = if ahead.index.is_some() {
            ahead.speed_mph.max(0.0) * game_hours
        } else {
            0.0 // the truck's own move is the trip's, not this frame's
        };
        let rear_now = ahead.position_mi + moved;
        let hold_at = rear_now - Self::following_gap_mi(vehicle) - vehicle.length_mi;
        if vehicle.position_mi <= hold_at {
            return;
        }
        if vehicle.position_mi + vehicle.length_mi <= rear_now {
            vehicle.position_mi = hold_at;
            vehicle.speed_mph = vehicle.speed_mph.min(ahead.speed_mph);
        } else {
            vehicle.speed_mph = vehicle.speed_mph.min(ahead.speed_mph - 2.0).max(0.0);
        }
    }
}
