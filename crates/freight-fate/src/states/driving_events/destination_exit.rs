//! The destination exit: finding it on the baked interchanges, naming it, and
//! announcing it while there is still road to take it with.

use crate::speech::EventPriority;
use ff_core::sim::trip_models::RoadStop;
use ff_core::speech_pacing::SpeechCategory;

use crate::app::{GameContext, SayEvent};
use crate::states::driving::DrivingState;
use crate::states::driving_core::*;

impl DrivingState {
    /// `_destination_exit_stop()`.
    pub fn destination_exit_stop(&mut self, ctx: &mut GameContext) -> Option<RoadStop> {
        if self.phase != DRIVE_PHASE_DELIVERY || self.destination_exit_taken {
            return None;
        }
        if self.departure_chain {
            // Still on the origin's streets: the end of the active trip is
            // the on-ramp merge, not the delivery exit.
            return None;
        }
        let details = self.destination_exit_details(ctx, false);
        let (at_mi, exit_label) = match details {
            // The estimate is for a route with no labeled exit at all. On one
            // that has it, "none ahead" means the truck is at or past the real
            // exit, and inventing a second one there announced "In 3 miles,
            // the destination exit" at the gore of exit 286A, seconds before
            // the truck took it (agent drive into Abilene, 2026-09-22).
            None if self.route_has_labeled_destination_exit(ctx) => return None,
            None => (
                0.0f64.max(self.trip.total_miles() - DESTINATION_EXIT_BEFORE_END_MI),
                String::new(),
            ),
            Some((at_mi, exit_label, _)) => (at_mi, exit_label),
        };
        if at_mi <= self.trip.position_mi + 0.05 {
            return None;
        }
        let mut stop = RoadStop::new(
            &self.destination_facility_text(ctx),
            at_mi,
            "delivery_destination",
        );
        stop.actions = vec!["deliver".to_string()];
        stop.exit_label = exit_label;
        Some(stop)
    }

    /// Whether the route carries a labeled destination exit anywhere, past
    /// or ahead. Scanned once per trip: it walks every interchange.
    fn route_has_labeled_destination_exit(&mut self, ctx: &GameContext) -> bool {
        let generation = self.trip_generation;
        if let Some((cached, labeled)) = self.destination_exit_labeled {
            if cached == generation {
                return labeled;
            }
        }
        let labeled = self.scan_destination_exit_details(ctx, true).is_some();
        self.destination_exit_labeled = Some((generation, labeled));
        labeled
    }

    /// `_destination_exit_key(stop)`.
    pub fn destination_exit_key(stop: &RoadStop) -> String {
        format!("{:.3}:{}:{}", stop.at_mi, stop.exit_label, stop.name)
    }

    /// The interchange phrase a destination-exit stop carries.
    ///
    /// Python hung `exit_phrase` on the `RoadStop` instance; the Rust
    /// `RoadStop` has a fixed shape, so the phrase is looked back up from the
    /// same scan that produced the stop. `include_past` so an exit taken this
    /// frame still names itself.
    pub fn exit_phrase_of(&mut self, ctx: &mut GameContext, stop: &RoadStop) -> String {
        if stop.stop_type != "delivery_destination" {
            return String::new();
        }
        match self.destination_exit_details(ctx, true) {
            Some((at_mi, _, phrase)) if (at_mi - stop.at_mi).abs() <= 0.001 => phrase,
            _ => String::new(),
        }
    }

    /// `_destination_exit_phrase(stop)`.
    pub fn destination_exit_phrase(&mut self, ctx: &mut GameContext, stop: &RoadStop) -> String {
        let phrase = self.exit_phrase_of(ctx, stop);
        if !phrase.is_empty() {
            return phrase;
        }
        if !stop.exit_label.is_empty() {
            return format!("{} for {}", stop.exit_label, stop.name);
        }
        format!("the exit for {}", stop.name)
    }

    /// `_missed_exit_phrase(stop)`.
    pub fn missed_exit_phrase(&mut self, ctx: &mut GameContext, stop: &RoadStop) -> String {
        if stop.stop_type == "delivery_destination" {
            // The exit phrase already carries its own label; naming both
            // would speak the same exit twice in one sentence.
            return self.destination_exit_phrase(ctx, stop);
        }
        if !stop.exit_label.is_empty() {
            return format!("{} for {}", stop.exit_label, stop.spoken_name());
        }
        format!("the exit for {}", stop.spoken_name())
    }

    /// `_destination_exit_announcement(stop, ahead)`.
    pub fn destination_exit_announcement(
        &mut self,
        ctx: &mut GameContext,
        stop: &RoadStop,
        ahead: f64,
    ) -> String {
        let phrase = self.exit_phrase_of(ctx, stop);
        let labeled = if phrase.is_empty() {
            stop.exit_label.clone()
        } else {
            phrase
        };
        // Quarter-mile steps once inside a mile: the whole-mile form rounds a
        // third of a mile to nothing, so the last call before the gore was "In
        // 0 miles, the destination exit" -- which reads as already-missed while
        // there is still road to use it (owner playtest, 2026-08-15). Whole
        // miles still answer from a mile out, because "In 5.0 miles" is worse
        // than "In 5 miles" for the calls that come early.
        let distance = if ahead < 1.0 {
            ctx.settings.short_distance_text(ahead)
        } else {
            ctx.settings.distance_text(ahead, false)
        };
        let core = if labeled.is_empty() {
            format!("In {distance}, the destination exit for {}.", stop.name)
        } else {
            format!("In {distance}, {labeled}, destination exit.")
        };
        if !ctx.settings.lane_is_automated() {
            if self.terse_speech(ctx) {
                return core;
            }
            // The signal first, because it is the gate: the lane and the ramp
            // speed are both wasted if it is never set. See
            // `DrivingState::exit_signal_instruction`.
            // Then the right lane, only if the truck is not in it; the exit
            // lane itself is called where it opens.
            let signal = self.exit_signal_instruction();
            let lane = if self.in_right_lane_for_exit() {
                ""
            } else {
                " Move to the right lane."
            };
            return format!("{core} {signal}{lane}");
        }
        // Lane keeping takes this exit with no signal and no lane work; the
        // manual says so, the callout does not (owner, 2026-10-14). No "slow
        // down for the ramp" either: the ramp is braked for past the gore,
        // where taking the exit names its speed (realistic exit, 2026-09-24).
        core
    }

    /// `_check_destination_exit()`.
    pub fn check_destination_exit(&mut self, ctx: &mut GameContext) {
        let Some(stop) = self.destination_exit_stop(ctx) else {
            return;
        };
        let ahead = stop.at_mi - self.trip.position_mi;
        if !(ahead > 0.0 && ahead <= self.exit_window_mi()) {
            return;
        }
        let key = Self::destination_exit_key(&stop);
        if self.canceled_exit_key.as_ref() == Some(&key) {
            return;
        }
        if key != self.destination_exit_announced_key {
            self.destination_exit_announced_key = key;
            // The exact exit stays answerable for a human reaction window even
            // if coasting or automatic braking shrinks the dynamic one.
            self.destination_exit_response_s = DESTINATION_EXIT_RESPONSE_GRACE_S;
            // Cruise stays engaged down the ramp approach, capped at the ramp
            // target, rather than handing the pedal back cold.
            let announcement = self.destination_exit_announcement(ctx, &stop, ahead);
            self.cap_cruise_for_ramp(Some(&stop));
            ctx.audio.play_with("ui/notify", 0.7, 0.0);
            // ROUTE: this line is the only warning the driver gets that the
            // truck is about to leave the highway. Left at the AMBIENT default
            // it was dropped whenever another line landed in the same moment,
            // and the exit read as taking itself (Sarah A, 2026-08-15).
            let mut opts = SayEvent::queued().priority(EventPriority::Important);
            opts.category = Some(SpeechCategory::Navigation);
            ctx.say_event_with(announcement, opts);
        }
        if self.exit_stop.is_none() {
            self.exit_stop = Some(stop);
            self.exit_signal_canceled = false;
            self.reset_exit_lane_state();
            if ctx.settings.lane_is_automated() {
                self.exit_lane_entered = true;
            }
        }
    }

    /// `_destination_exit_details(*, include_past=False)`.
    pub fn destination_exit_details(
        &mut self,
        ctx: &GameContext,
        include_past: bool,
    ) -> Option<(f64, String, String)> {
        if include_past {
            return self.scan_destination_exit_details(ctx, true);
        }
        // This runs every frame from _check_destination_exit, and the scan
        // walks every interchange on the route building spoken phrases -- far
        // too much churn to redo per tick on a coast-to-coast route. The
        // winning exit only changes when the truck passes it, so reuse the
        // last answer until then. A backward position move (missed-exit
        // rewind, rescue) invalidates the cache wholesale, because exits
        // behind the compute position come back into play.
        let pos = self.trip.position_mi;
        let stale = match self.destination_exit_cache.as_ref() {
            None => true,
            Some((at, found)) => {
                pos < *at || found.as_ref().is_some_and(|found| found.0 <= pos + 0.05)
            }
        };
        if stale {
            let found = self.scan_destination_exit_details(ctx, false);
            self.destination_exit_cache = Some((pos, found));
        }
        self.destination_exit_cache
            .as_ref()
            .and_then(|(_, found)| found.clone())
    }

    /// `_scan_destination_exit_details(*, include_past=False)`.
    pub fn scan_destination_exit_details(
        &self,
        ctx: &GameContext,
        include_past: bool,
    ) -> Option<(f64, String, String)> {
        scan_destination_exit(ctx.world, &self.trip, include_past)
    }
}

/// The winning destination exit on a trip, off the trip alone.
///
/// Free rather than a method because the playtest road finder has to point at
/// exactly the interchange the drive will announce, and it runs against the
/// world data with no `App` and so no `DrivingState` to ask. Two copies of
/// this ranking would be two answers, and a finder that lands somewhere the
/// game does not call the destination exit is worse than no finder.
///
/// `trip.route` is the drive's own route (`DrivingState` clones it into the
/// trip at construction), so reading it here is the same scan.
pub fn scan_destination_exit(
    world: &ff_core::data::world::World,
    trip: &ff_core::sim::trip::Trip,
    include_past: bool,
) -> Option<(f64, String, String)> {
    let route = &trip.route;
    if route.legs.is_empty() {
        return None;
    }
    // Matched against real interchange sign text, so compare the spoken
    // city name ("Nashville"), never the slug key.
    let destination = world
        .spoken_city(
            route.cities.last().map(String::as_str).unwrap_or(""),
            Some(false),
        )
        .to_lowercase();
    let mainline_cities: Vec<String> = route
        .cities
        .iter()
        .map(|c| world.spoken_city(c, Some(false)))
        .collect();
    let scan_floor = trip.total_miles() - DESTINATION_EXIT_SCAN_WINDOW_MI;
    // (legs from the end, distance from the leg's destination end, whether
    // the sign does NOT name the destination, route mile, label, phrase)
    let mut candidates: Vec<(usize, f64, bool, f64, String, String)> = Vec::new();
    for i in (0..route.legs.len()).rev() {
        let leg = &route.legs[i];
        if trip.leg_starts[i] + leg.miles < scan_floor {
            // This leg ends before the final approach; every earlier leg
            // is farther out still.
            break;
        }
        let forward = route.cities.get(i).map(String::as_str) == Some(leg.a.as_str());
        let target = if forward { leg.miles } else { 0.0 };
        for ix in leg.interchanges() {
            let exit_label = ix.exit_label();
            if exit_label.is_empty() {
                continue;
            }
            let offset = if forward {
                ix.at_mi
            } else {
                leg.miles - ix.at_mi
            };
            let route_mile = trip.leg_starts[i] + offset;
            if route_mile < scan_floor {
                continue;
            }
            if !include_past && route_mile <= trip.position_mi + 0.05 {
                continue;
            }
            let dist_from_destination = (ix.at_mi - target).abs();
            let matches_destination = ix
                .destinations
                .iter()
                .any(|part| part.to_lowercase().contains(&destination));
            let siblings: Vec<String> = leg
                .interchanges()
                .iter()
                .filter(|other| other.at_mi != ix.at_mi)
                .flat_map(|other| other.destinations.iter().cloned())
                .collect();
            candidates.push((
                route.legs.len() - 1 - i,
                dist_from_destination,
                !matches_destination,
                route_mile,
                exit_label,
                ix.spoken_phrase_on(&leg.highway, &mainline_cities, &siblings),
            ));
        }
    }
    if candidates.is_empty() {
        return None;
    }
    candidates.sort_by(|a, b| {
        a.0.cmp(&b.0)
            .then(a.1.total_cmp(&b.1))
            .then(a.2.cmp(&b.2))
            .then(a.3.total_cmp(&b.3))
            .then(a.4.cmp(&b.4))
            .then(a.5.cmp(&b.5))
    });
    let winner = &candidates[0];
    Some((winner.3, winner.4.clone(), winner.5.clone()))
}
