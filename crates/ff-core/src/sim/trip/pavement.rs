//! The pavement layer along the route: the surface under the truck and the
//! spans with bridge joints, resolved by leg and direction.

use super::Trip;
use crate::data::pavement::StructureKind;
use crate::sim::outside::RIVER_SPAN_MI;
use crate::sim::road_texture::joints::JointSpan;
use crate::sim::road_texture::song::Underfoot;
use crate::sim::road_texture::Reading;
use crate::sim::trip_route_helpers::stop_offset_for_direction;

impl Trip {
    /// The leg under route mile `mile`: its index, whether the truck drives
    /// it forward, and the offset in the leg's own (native) frame.
    fn native_at(&self, mile: f64) -> (usize, bool, f64) {
        let (i, start) = self.leg_at_mile(mile);
        let leg = &self.route.legs[i];
        let forward = self.route.cities[i] == leg.a;
        (
            i,
            forward,
            stop_offset_for_direction(mile - start, leg.miles, forward),
        )
    }

    /// What the pavement layer reads under route mile `mile`; `None` where
    /// it reads nothing, the leg has no layer, or the route is a facility
    /// approach.
    pub fn pavement_reading_at(&self, mile: f64) -> Option<Reading> {
        if self.route.legs.is_empty() || self.is_facility_approach_route() {
            return None;
        }
        let (i, forward, offset) = self.native_at(mile);
        let run = self.route.legs[i].pavement()?.surface_at(offset, forward)?;
        Some(Reading {
            surface: run.surface,
            faulting: run.faulting,
        })
    }

    /// What the tires roll on at route mile `mile`, for their song: a steel
    /// grating, concrete grooved across the lane, or plain road. Plain on a
    /// facility approach and wherever the layer says nothing.
    pub fn underfoot_at(&self, mile: f64) -> Underfoot {
        if self.route.legs.is_empty() || self.is_facility_approach_route() {
            return Underfoot::Plain;
        }
        let (i, forward, offset) = self.native_at(mile);
        let Some(pavement) = self.route.legs[i].pavement() else {
            return Underfoot::Plain;
        };
        if pavement.on_grating(offset, forward) {
            return Underfoot::Grating;
        }
        match pavement.cross_grooved_at(offset, forward, &self.state_at(Some(mile))) {
            Some(grooves) => Underfoot::Grooved(grooves),
            None => Underfoot::Plain,
        }
    }

    /// The structure under route mile `mile`, with its length in miles.
    pub fn structure_at_mile(&self, mile: f64) -> Option<(StructureKind, f64)> {
        if self.route.legs.is_empty() || self.is_facility_approach_route() {
            return None;
        }
        let (i, forward, offset) = self.native_at(mile);
        self.route.legs[i]
            .pavement()?
            .structures_for(forward)
            .into_iter()
            .find(|s| s.0 <= offset && offset < s.1)
            .map(|s| (s.2, s.1 - s.0))
    }

    /// Every span with a bridge joint at each end, in route miles and mile
    /// order (see `sim::road_texture::joints`). A leg with the layer gives
    /// its bridges and causeways, and an assumed span at each river point
    /// with no structure within [`RIVER_SPAN_MI`]; a leg without it gives an
    /// assumed span at every river point. Empty on a facility approach.
    pub fn bridge_joint_spans(&self) -> Vec<JointSpan> {
        if self.is_facility_approach_route() {
            return Vec::new();
        }
        let mut spans: Vec<JointSpan> = Vec::new();
        for (i, (start, leg)) in self
            .leg_starts
            .iter()
            .zip(self.route.legs.iter())
            .enumerate()
        {
            let forward = self.route.cities[i] == leg.a;
            let route = |native: f64| start + stop_offset_for_direction(native, leg.miles, forward);
            let structures = leg
                .pavement()
                .map(|p| p.structures_for(forward))
                .unwrap_or_default();
            for &(from, to, kind) in &structures {
                let (a, b) = (route(from), route(to));
                if let Some(span) = JointSpan::from_structure(a.min(b), a.max(b), kind) {
                    spans.push(span);
                }
            }
            for landmark in leg.landmarks() {
                if landmark.category != "river" || !landmark.applies_to_direction(forward) {
                    continue;
                }
                let at = landmark.at_mi;
                let near = structures
                    .iter()
                    .any(|s| s.0 - RIVER_SPAN_MI <= at && at <= s.1 + RIVER_SPAN_MI);
                if !near {
                    spans.push(JointSpan::river(route(at)));
                }
            }
        }
        spans.sort_by(|a, b| a.from_mi.total_cmp(&b.from_mi));
        spans
    }
}
