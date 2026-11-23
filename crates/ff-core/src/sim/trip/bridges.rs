//! The named bridges along the route (`sim::bridges`), resolved by leg and
//! direction, and the tick that moves the truck on and off them.

use super::Trip;
use crate::sim::bridges::{choose_spoken, BridgeCrossing, BridgeEvent, RouteBridge};
use crate::sim::trip_models::{TripEventData, TripEventKind};
use crate::sim::trip_route_helpers::stop_offset_for_direction;
use crate::speech_text::SpokenMessage;

impl Trip {
    /// Every named bridge on the route in route miles and mile order, with
    /// the names that are spoken chosen. Empty on a facility approach.
    pub fn place_bridges(&self) -> Vec<RouteBridge> {
        if self.is_facility_approach_route() {
            return Vec::new();
        }
        let mut out = Vec::new();
        let mut tolls: Vec<(f64, String)> = Vec::new();
        for (i, (start, leg)) in self
            .leg_starts
            .iter()
            .zip(self.route.legs.iter())
            .enumerate()
        {
            let forward = self.route.cities[i] == leg.a;
            let route = |native: f64| start + stop_offset_for_direction(native, leg.miles, forward);
            for b in leg.bridges().for_direction(forward) {
                let (a, z) = (route(b.from_mi), route(b.to_mi));
                out.push(RouteBridge {
                    from_mi: a.min(z),
                    to_mi: a.max(z),
                    name: b.name.clone(),
                    water: b.water,
                    line: None,
                });
            }
            for toll in leg.toll_events() {
                if !toll.applies_to_direction(forward) {
                    continue;
                }
                tolls.push((route(toll.at_mi), toll.name.clone()));
            }
        }
        out.sort_by(|a, b| a.from_mi.total_cmp(&b.from_mi));
        choose_spoken(&mut out, &tolls);
        out
    }

    /// The named bridge under route mile `mile`, if any.
    pub fn bridge_at_mile(&self, mile: f64) -> Option<&RouteBridge> {
        self.bridges
            .iter()
            .find(|b| b.from_mi <= mile && mile < b.to_mi)
    }

    /// Move the bridge state to the truck, saying a bridge's name as the
    /// truck drives onto it. The line is roadside chatter in the `bridge`
    /// category (the rivers switch), and it spends the chatter budget.
    pub fn check_bridges(&mut self) {
        let event = BridgeEvent::Moved {
            at_mi: self.position_mi,
        };
        let (state, says) = self.bridge.next(event, &self.bridges);
        self.bridge = state;
        let Some(line) = says.and_then(|i| self.bridges[i].line.clone()) else {
            return;
        };
        self.emit(
            TripEventKind::Landmark,
            SpokenMessage::new(line),
            TripEventData {
                category: Some("bridge".to_string()),
                explains_limit: Some(false),
                ..Default::default()
            },
        );
        self.note_chatter_spoke();
    }

    /// Put the bridge state where the truck already is, saying nothing.
    pub(crate) fn settle_bridge(&mut self) {
        let event = BridgeEvent::Placed {
            at_mi: self.position_mi,
        };
        self.bridge = self.bridge.next(event, &self.bridges).0;
    }

    pub fn bridge_state(&self) -> BridgeCrossing {
        self.bridge
    }
}
