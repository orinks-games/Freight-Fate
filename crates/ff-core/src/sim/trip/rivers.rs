//! Where the route crosses a river: the water under the bridge and the
//! river's name, moved together by one state (`sim::river_crossing`).

use super::Trip;
use crate::sim::outside::RIVER_SPAN_MI;
use crate::sim::river_crossing::{RiverCall, RiverCrossing, RiverEvent, RouteRiver, Withheld};
use crate::sim::trip_models::{TripEventData, TripEventKind};
use crate::sim::trip_route_helpers::stop_offset_for_direction;
use crate::speech_text::SpokenMessage;

/// The game's transcript log target (`freight_fate::app::TRANSCRIPT_TARGET`),
/// which a player's game.log carries.
const TRANSCRIPT_TARGET: &str = "freight_fate.transcript";

impl Trip {
    /// Every `river` landmark on the route in route miles, sorted, resolved
    /// by direction the way [`Trip::place_landmarks`] places scenery. No
    /// spacing and no chatter budget thin them: each river's water plays, and
    /// its name is said as the water starts (see [`Trip::check_rivers`]).
    /// Empty on a facility approach route. Reads `self.bridges`, so it runs
    /// after [`Trip::place_bridges`].
    pub fn place_rivers(&self) -> Vec<RouteRiver> {
        if self.is_facility_approach_route() {
            return Vec::new();
        }
        let mut rivers: Vec<RouteRiver> = Vec::new();
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
                let at_mi = start + stop_offset_for_direction(landmark.at_mi, leg.miles, forward);
                // One name per crossing: a named bridge over the river's
                // water says the bridge (or its toll line did).
                let under_bridge = self
                    .bridges
                    .iter()
                    .any(|b| b.from_mi <= at_mi + RIVER_SPAN_MI && at_mi - RIVER_SPAN_MI < b.to_mi);
                let spoken = if landmark.spoken.ends_with(['.', '!', '?']) {
                    landmark.spoken.clone()
                } else {
                    format!("{}.", landmark.spoken)
                };
                rivers.push(RouteRiver {
                    at_mi,
                    name: landmark.name.clone(),
                    spoken,
                    under_bridge,
                });
            }
        }
        rivers.sort_by(|a, b| a.at_mi.total_cmp(&b.at_mi));
        rivers
    }

    /// Every route mile where the water plays, sorted.
    pub fn river_crossing_miles(&self) -> Vec<f64> {
        self.rivers.iter().map(|r| r.at_mi).collect()
    }

    /// Move the river state over `dt_real_s` real seconds, saying a river's
    /// name as its water starts. The line is roadside chatter in the `river`
    /// category (the rivers switch decides whether it is heard), it never
    /// waits on the chatter budget, and it spends it, so the next joke
    /// billboard waits behind it.
    pub fn check_rivers(&mut self, dt_real_s: f64) {
        let event = RiverEvent::Tick {
            dt_real_s,
            at_mi: self.position_mi,
            water_bridge: self.bridge.over_water(),
        };
        let (state, call) = self.river.next(event, &self.rivers);
        self.river = state;
        match call {
            None => {}
            // On the transcript target, so a player's game.log shows why a
            // river they heard was not named, beside the speech ladder's
            // own lines.
            Some(RiverCall::Withheld(i, Withheld::Bridge)) => {
                log::info!(
                    target: TRANSCRIPT_TARGET,
                    "[river] withheld: {} (bridge line)",
                    self.rivers[i].name
                );
            }
            Some(RiverCall::Withheld(i, Withheld::MergedWater)) => {
                log::info!(
                    target: TRANSCRIPT_TARGET,
                    "[river] withheld: {} (merged water)",
                    self.rivers[i].name
                );
            }
            Some(RiverCall::Say(i)) => {
                let line = self.rivers[i].spoken.clone();
                self.emit(
                    TripEventKind::Landmark,
                    SpokenMessage::new(line),
                    TripEventData {
                        category: Some("river".to_string()),
                        explains_limit: Some(false),
                        ..Default::default()
                    },
                );
                self.note_chatter_spoke();
            }
        }
    }

    /// Put the river state where the truck already is, saying nothing.
    pub(crate) fn settle_rivers(&mut self) {
        let event = RiverEvent::Placed {
            at_mi: self.position_mi,
            water_bridge: self.bridge.over_water(),
        };
        self.river = self.river.next(event, &self.rivers).0;
    }

    /// The river the water is for now, or was last, and the last one named.
    pub fn river_state(&self) -> RiverCrossing {
        self.river
    }
}
