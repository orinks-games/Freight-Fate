//! Where the route crosses a river, for the water under the bridge.

use super::Trip;
use crate::sim::trip_route_helpers::stop_offset_for_direction;

impl Trip {
    /// Every route mile where a `river` landmark sits, sorted, resolved by
    /// direction the way [`Trip::place_landmarks`] places them, but with no
    /// spacing and no chatter setting: a river's sound plays whether or not
    /// its name is spoken. Empty on a facility approach route.
    pub fn river_crossing_miles(&self) -> Vec<f64> {
        if self.is_facility_approach_route() {
            return Vec::new();
        }
        let mut miles: Vec<f64> = Vec::new();
        for (i, (start, leg)) in self
            .leg_starts
            .iter()
            .zip(self.route.legs.iter())
            .enumerate()
        {
            let forward = self.route.cities[i] == leg.a;
            for landmark in leg.landmarks() {
                if landmark.category != "river" || !landmark.applies_to_direction(forward) {
                    continue;
                }
                miles.push(start + stop_offset_for_direction(landmark.at_mi, leg.miles, forward));
            }
        }
        miles.sort_by(f64::total_cmp);
        miles
    }
}
