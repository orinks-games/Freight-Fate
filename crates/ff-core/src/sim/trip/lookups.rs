//! Where-am-I answers on the route: grade, terrain, lanes, state, region,
//! the posted limit and the zones, stops and cues around the truck (the
//! query half of `trip.py`).

use crate::data::curves::RouteCurve;
use crate::data::regions::classify_region;
use crate::data::street_limits::load_street_limits;
use crate::data::world_models::{lane_word, py_capitalize, Interchange, Stop};
use crate::pyfmt::{fmt_f, round_py_int};
use crate::sim::traffic_manager::{
    TrafficVehicle, DODGE_CLEARANCE_AHEAD_MI, DODGE_CLEARANCE_BEHIND_MI, LANE_GAP_ACT_REAL_S,
    LANE_GAP_MARGIN_MI,
};
use crate::sim::trip_models::*;
use crate::sim::trip_route_helpers::{fallback_grade, stop_offset_for_direction};
use crate::units::{distance_unit, spoken_distance, to_distance};

use super::{Trip, ETA_MIN_MPH, LANE_RUN_MIN_MI};

/// One direction-aware lane run across the route in travel order (Python's
/// `[route_start_mi, route_end_mi, lanes_your_side, divided]`).
#[derive(Debug, Clone, PartialEq)]
pub struct LaneRun {
    pub start_mi: f64,
    pub end_mi: f64,
    pub lanes: i64,
    pub divided: bool,
}

impl Trip {
    /// Whether this stop belongs on the run at all: curated, facing the
    /// direction of travel, and physically enterable by the rig being driven.
    /// One gate for both places that read a leg's stops.
    pub fn stop_is_real(&self, stop: &Stop, forward: bool) -> bool {
        stop.curated() && stop.applies_to_direction(forward) && stop.accessible_to(self.bobtail)
    }

    pub fn grade_at(&self, mile: f64) -> f64 {
        let (leg_i, leg_start) = self.leg_at_mile(mile);
        let leg = &self.route.legs[leg_i];
        let forward = self.route.cities[leg_i] == leg.a;
        let offset = (mile - leg_start).clamp(0.0, leg.miles.max(0.0));
        let sample_offset = if forward { offset } else { leg.miles - offset };
        for segment in leg.grade_segments() {
            if segment.start_mi <= sample_offset && sample_offset <= segment.end_mi {
                let grade = segment.avg_grade_pct / 100.0;
                return if forward { grade } else { -grade };
            }
        }
        fallback_grade(&leg.terrain, mile, &leg.highway)
    }

    pub fn terrain_at(&self, mile: Option<f64>) -> String {
        let sample_mile = mile.unwrap_or(self.position_mi);
        let (leg_i, leg_start) = self.leg_at_mile(sample_mile);
        let leg = &self.route.legs[leg_i];
        let forward = self.route.cities[leg_i] == leg.a;
        let offset = (sample_mile - leg_start).clamp(0.0, leg.miles.max(0.0));
        let sample_offset = if forward { offset } else { leg.miles - offset };
        for segment in leg.grade_segments() {
            if segment.start_mi <= sample_offset && sample_offset <= segment.end_mi {
                return segment.terrain.clone();
            }
        }
        leg.terrain.clone()
    }

    /// (lanes in the direction of travel, divided) at a route mile, or None
    /// where the lane bake found no tag -- honest absence, speak nothing.
    pub fn lanes_at(&self, mile: Option<f64>) -> Option<(i64, bool)> {
        let sample_mile = mile.unwrap_or(self.position_mi);
        let (leg_i, leg_start) = self.leg_at_mile(sample_mile);
        let leg = &self.route.legs[leg_i];
        let forward = self.route.cities[leg_i] == leg.a;
        let offset = (sample_mile - leg_start).clamp(0.0, leg.miles.max(0.0));
        let sample_offset = if forward { offset } else { leg.miles - offset };
        for seg in leg.lane_segments() {
            if seg.start_mi <= sample_offset && sample_offset <= seg.end_mi {
                return Some((seg.your_side(), seg.divided()));
            }
        }
        None
    }

    /// Lanes on our side at a route mile, from the best data available,
    /// capped at `MAX_DRIVABLE_LANES`: the spoken vocabulary has three lane
    /// names. This is the same answer the driving state steers by.
    pub fn lane_count_at(&self, mile: Option<f64>) -> i64 {
        if let Some((lanes, _)) = self.lanes_at(mile) {
            return 1.max(MAX_DRIVABLE_LANES.min(lanes));
        }
        let (leg_i, _) = self.leg_at_mile(mile.unwrap_or(self.position_mi));
        let leg = &self.route.legs[leg_i];
        if leg.divided == Some(false) {
            return 1;
        }
        MAX_DRIVABLE_LANES.min(leg_lane_count(Some(leg)))
    }

    /// The roadwork zone whose cones cover this mile, taper included. Not
    /// `active_zone`: that answers with the SLOWEST zone at the mile, so a
    /// jam laid over the roadwork hid the closure.
    pub fn active_closure(&self, mile: Option<f64>) -> Option<Zone> {
        let sample = mile.unwrap_or(self.position_mi);
        let mut best: Option<&Zone> = None;
        for z in &self.zones {
            if !(CONSTRUCTION_ZONE_REASONS.contains(&z.reason.as_str())
                && z.closed_side.is_some()
                && z.start_mi <= sample
                && sample <= z.end_mi)
            {
                continue;
            }
            let key = |zone: &Zone| (zone.reason != "construction", zone.start_mi);
            if best.is_none_or(|b| {
                key(z)
                    .partial_cmp(&key(b))
                    .is_some_and(|o| o == std::cmp::Ordering::Less)
            }) {
                best = Some(z);
            }
        }
        best.cloned()
    }

    /// Which lane index is coned off at a mile, or `None` for none, derived
    /// from the closure's SIDE and the lanes the road has here.
    pub fn closed_lane_at(&self, mile: Option<f64>, lane_count: Option<i64>) -> Option<i64> {
        let zone = self.active_closure(mile)?;
        let side = zone.closed_side.as_deref()?;
        let count = lane_count.unwrap_or_else(|| self.lane_count_at(mile));
        if count < 2 {
            return None;
        }
        Some(if side == "right" { 0 } else { count - 1 })
    }

    /// Whether the ROAD has anywhere on this side to swerve into: a second
    /// lane, not coned off. Which of the truck's neighbours is actually open
    /// -- traffic included -- is [`Trip::open_side_at`], and that is what a
    /// hazard call reads; this is the road-only reading the lane-count
    /// readouts still answer from.
    pub fn has_open_adjacent_lane_at(&self, mile: Option<f64>) -> bool {
        let mut count = self.lane_count_at(mile);
        if count < 2 {
            return false;
        }
        if self.closed_lane_at(mile, Some(count)).is_some() {
            count -= 1;
        }
        count >= 2
    }

    /// The vehicle holding `lane_index` beside the truck, or None.
    ///
    /// The window is the sideswipe test's, widened by `LANE_GAP_MARGIN_MI` at
    /// both ends and swept `LANE_GAP_ACT_REAL_S` real seconds forward through
    /// the traffic's relative motion: whatever this call misses, the collision
    /// check misses too, so "open" can never be the more optimistic of the two
    /// answers -- not now, and not by the time the driver acting on it arrives
    /// in the lane. One reading for the lane-gap cue, the L key, and the
    /// hazard call's named side.
    pub fn lane_blocker_at(&self, mile: Option<f64>, lane_index: i64) -> Option<&TrafficVehicle> {
        let in_lane = |lane| {
            self.traffic_manager.vehicle_in_lane(
                mile.unwrap_or(self.position_mi),
                lane,
                DODGE_CLEARANCE_AHEAD_MI + LANE_GAP_MARGIN_MI,
                DODGE_CLEARANCE_BEHIND_MI + LANE_GAP_MARGIN_MI,
                LANE_GAP_ACT_REAL_S * self.effective_time_scale() / 3600.0,
                self.truck.speed_mph(),
            )
        };
        // A vehicle on the ramp beside the right lane is about to be in it,
        // and joins the moment the truck is a safe gap away, which a truck
        // over in the left lane always is. Left out, the right lane read open
        // with a box truck alongside on the ramp (player report, 2026-10-13).
        in_lane(lane_index).or_else(|| (lane_index == 0).then(|| in_lane(-1)).flatten())
    }

    /// Which of the truck's neighbouring lanes a dodge can go into right
    /// now, so the hazard call can name it (owner, 2026-09-01).
    ///
    /// A side is open by exactly the rules a tap lane change and its arrival
    /// apply: the lane exists in this stretch's count, it is not the one
    /// roadwork has coned off, and nobody is holding it over the window the
    /// change takes (`lane_blocker_at`). The truck's lane is the one the
    /// driving state mirrors into the traffic manager every frame. On a road
    /// with one lane this side, or with both neighbours held, the answer is
    /// `Neither`, and the hazard is not dodgeable at all.
    pub fn open_side_at(&self, mile: Option<f64>) -> OpenSide {
        let count = self.lane_count_at(mile);
        let lane = self.traffic_manager.player_lane;
        let closed = self.closed_lane_at(mile, Some(count));
        let open = |target: i64| {
            (0..count).contains(&target)
                && Some(target) != closed
                && self.lane_blocker_at(mile, target).is_none()
        };
        // Lane 0 is the right lane; higher indexes are further left.
        OpenSide::from_sides(open(lane + 1), open(lane - 1))
    }

    /// The lane answer for `side` at the truck's position, the open
    /// neighbour named the way the L key names it.
    pub fn open_side_spoken(&self, side: OpenSide) -> String {
        side.spoken_from(self.traffic_manager.player_lane, self.lane_count_at(None))
    }

    /// True when every mile of a work zone footprint -- taper included --
    /// has a second lane our side (tester report, Detroit-Mansfield,
    /// 2026-08-11).
    pub fn span_is_multilane(&self, start_mi: f64, end_mi: f64) -> bool {
        let stop = self.total_miles().min(start_mi.max(end_mi));
        let mut mile = 0.0_f64.max(start_mi.min(stop));
        while mile < stop {
            if self.lane_count_at(Some(mile)) < 2 {
                return false;
            }
            mile += LANE_CLOSURE_SAMPLE_MI;
        }
        self.lane_count_at(Some(stop)) >= 2
    }

    /// The lowest posted limit across a span, sampled the way
    /// `span_is_multilane` samples lanes. A work zone steps down from it.
    pub fn lowest_limit_over(&self, start_mi: f64, end_mi: f64) -> f64 {
        let stop = self.total_miles().min(start_mi.max(end_mi));
        let mut mile = 0.0_f64.max(start_mi.min(stop));
        let mut lowest = self.corridor_limit_at(stop);
        while mile < stop {
            lowest = lowest.min(self.corridor_limit_at(mile));
            mile += LANE_CLOSURE_SAMPLE_MI;
        }
        lowest
    }

    /// The state the truck is in, or empty where the bake is silent.
    pub fn state_at(&self, mile: Option<f64>) -> String {
        let sample_mile = mile.unwrap_or(self.position_mi);
        let (leg_i, leg_start) = self.leg_at_mile(sample_mile);
        let leg = &self.route.legs[leg_i];
        let forward = self.route.cities[leg_i] == leg.a;
        let offset = (sample_mile - leg_start).clamp(0.0, leg.miles.max(0.0));
        leg_state_at(leg, if forward { offset } else { leg.miles - offset })
    }

    pub fn leg_at_mile(&self, mile: f64) -> (usize, f64) {
        let clamped = mile.clamp(0.0, self.total_miles().max(0.0));
        for i in (0..self.route.legs.len()).rev() {
            if clamped >= self.leg_starts[i] {
                return (i, self.leg_starts[i]);
            }
        }
        (0, 0.0)
    }

    pub fn speed_limit_at(&mut self, mile: f64) -> (f64, Option<String>) {
        if let Some(zone) = self.active_zone_at(mile) {
            return (zone.limit_mph, Some(zone.reason));
        }
        (self.corridor_limit_at(mile), None)
    }

    /// Whether a truck-specific limit is in force here, and the state to
    /// credit for it. A zone answers first: inside construction the cone is
    /// the reason the number dropped, not the state line.
    pub fn truck_limit_at(&mut self, mile: f64) -> (bool, Option<String>) {
        if self.active_zone_at(mile).is_some() {
            return (false, None);
        }
        let (leg_i, leg_start) = self.leg_at_mile(mile);
        let leg = &self.route.legs[leg_i];
        let forward = self.route.cities[leg_i] == leg.a;
        let route_offset = mile - leg_start;
        let leg_offset = if forward {
            route_offset
        } else {
            leg.miles - route_offset
        };
        truck_limit_at(leg, leg_offset)
    }

    pub fn region_at(&self, mile: f64) -> String {
        let state = self.state_at(Some(mile));
        let (lat, lon) = self.latlon_at(Some(mile));
        if !state.is_empty() && (lat != 0.0 || lon != 0.0) {
            let code = self
                .state_codes
                .get(&state)
                .cloned()
                .unwrap_or_else(|| state.clone());
            if code.len() == 2 {
                if let Ok(region) = classify_region(&code, lat, lon) {
                    return region.to_string();
                }
            }
        }
        let (leg_i, leg_start) = self.leg_at_mile(mile);
        let leg = &self.route.legs[leg_i];
        let nearer = if mile - leg_start < leg.miles / 2.0 {
            leg_i
        } else {
            leg_i + 1
        };
        // A synthetic route names cities the world does not carry: region
        // only tunes how thick enforcement and weather are, so an unknown
        // road simply gets the neutral default.
        self.route
            .cities
            .get(nearer)
            .and_then(|c| self.world.cities.get(c))
            .map(|c| c.region.clone())
            .unwrap_or_default()
    }

    pub fn near_city(&self, mile: f64) -> bool {
        self.city_mileposts
            .iter()
            .any(|mp| (mile - mp).abs() <= URBAN_RADIUS_MI)
    }

    /// The nearest route city within the urban radius, with its milepost.
    pub fn nearest_urban_city(&self, mile: f64) -> Option<(String, f64)> {
        let mut best: Option<(String, f64)> = None;
        let mut best_d = URBAN_RADIUS_MI;
        for (i, mp) in self.city_mileposts.iter().enumerate() {
            let d = (mile - mp).abs();
            if d <= best_d && i < self.route.cities.len() {
                best = Some((self.route.cities[i].clone(), *mp));
                best_d = d;
            }
        }
        best
    }

    /// The route city whose no-engine-brake ordinance covers this mile.
    pub fn engine_brake_ban_at(&self, mile: f64) -> Option<String> {
        if !self.near_city(mile) {
            return None;
        }
        self.nearest_urban_city(mile).map(|(city, _)| city)
    }

    /// Start mile and city of the next ban zone ahead, inside the window.
    pub fn next_engine_brake_ban(&self, within_mi: f64) -> Option<(f64, String)> {
        let pos = self.position_mi;
        let mut best: Option<(f64, String)> = None;
        for (i, mp) in self.city_mileposts.iter().enumerate() {
            let start = mp - URBAN_RADIUS_MI;
            if pos < start
                && start <= pos + within_mi
                && best.as_ref().is_none_or(|(b, _)| start < *b)
            {
                let city = self.route.cities[i.min(self.route.cities.len() - 1)].clone();
                best = Some((start, city));
            }
        }
        best
    }

    pub fn corridor_limit_at(&self, mile: f64) -> f64 {
        let (leg_i, leg_start) = self.leg_at_mile(mile);
        let leg = &self.route.legs[leg_i];
        let forward = self.route.cities[leg_i] == leg.a;
        let route_offset = mile - leg_start;
        let leg_offset = if forward {
            route_offset
        } else {
            leg.miles - route_offset
        };
        if let Some(baked) = truck_capped_speed_limit(leg, leg_offset) {
            return baked;
        }
        let base = corridor_speed_limit(&leg.highway, &self.region_at(mile));
        if self.near_city(mile) {
            return base.min(URBAN_LIMIT_MPH);
        }
        base
    }

    /// Mainline curves whose entry lies ahead within the window.
    pub fn curves_within(&self, within_mi: f64) -> Vec<RouteCurve> {
        self.curves
            .iter()
            .filter(|c| {
                !c.connector
                    && 0.0 < c.start_mi - self.position_mi
                    && c.start_mi - self.position_mi <= within_mi
            })
            .cloned()
            .collect()
    }

    pub fn next_zone_within(&mut self, within_mi: f64) -> Option<Zone> {
        let pos = self.position_mi;
        let mut best: Option<usize> = None;
        for i in 0..self.zones.len() {
            let ahead = self.zones[i].start_mi - pos;
            if !(0.0 < ahead && ahead <= within_mi && self.zone_is_active_index(i)) {
                continue;
            }
            if best.is_none_or(|b| self.zones[i].start_mi < self.zones[b].start_mi) {
                best = Some(i);
            }
        }
        best.map(|i| self.zones[i].clone())
    }

    /// The reduced-limit zone the truck is currently inside, if any.
    pub fn active_zone(&mut self) -> Option<Zone> {
        let pos = self.position_mi;
        self.active_zone_at(pos)
    }

    /// Inside the signed footprint of roadwork -- taper included.
    pub fn in_construction_zone(&mut self) -> bool {
        let pos = self.position_mi;
        for i in 0..self.zones.len() {
            let z = &self.zones[i];
            if CONSTRUCTION_ZONE_REASONS.contains(&z.reason.as_str())
                && z.start_mi <= pos
                && pos <= z.end_mi
                && self.zone_is_active_index(i)
            {
                return true;
            }
        }
        false
    }

    /// Baked OSM ramp-terminal control at the interchange nearest a route
    /// mile, or "" when no interchange within `tol_mi` carries one.
    pub fn ramp_control_at(&self, route_mile: f64, tol_mi: f64) -> String {
        let mut best = String::new();
        let mut best_dist = tol_mi;
        for (i, (start, leg)) in self
            .leg_starts
            .iter()
            .zip(self.route.legs.iter())
            .enumerate()
        {
            let forward = self.route.cities[i] == leg.a;
            for ix in leg.interchanges() {
                if ix.ramp_control.is_empty() {
                    continue;
                }
                let offset = stop_offset_for_direction(ix.at_mi, leg.miles, forward);
                let dist = (start + offset - route_mile).abs();
                if dist <= best_dist {
                    best_dist = dist;
                    best = ix.ramp_control.clone();
                }
            }
        }
        best
    }

    pub fn ramp_advisory_at(&self, route_mile: f64) -> RampAdvisorySpeed {
        let interchange = self.interchange_with_direction_at(route_mile, 2.0);
        let directional = interchange.is_some_and(|(ix, _, _)| ix.ramp_far_end == "motorway");
        let calculated_mph = ramp_speed_mph(self.corridor_limit_at(route_mile), directional);
        if let Some((ix, forward, _)) = interchange {
            let observed = if forward {
                ix.ramp_advisory_mph_forward
            } else {
                ix.ramp_advisory_mph_backward
            };
            if let Some(posted_mph) = observed {
                return RampAdvisorySpeed::Observed {
                    posted_mph,
                    truck_target_mph: posted_mph.min(calculated_mph),
                };
            }
        }
        RampAdvisorySpeed::Calculated {
            mph: calculated_mph,
        }
    }

    pub fn ramp_speed_at(&self, route_mile: f64) -> f64 {
        self.ramp_advisory_at(route_mile).mph()
    }

    /// OSM-derived length in miles of the exit ramp nearest a route mile
    /// (within 2 miles, like `ramp_advisory_at`), for this trip's direction
    /// of travel; None when that exit has no baked length that way.
    pub fn ramp_length_mi_at(&self, route_mile: f64) -> Option<f64> {
        let (ix, forward, _) = self.interchange_with_direction_at(route_mile, 2.0)?;
        let feet = if forward {
            ix.ramp_length_ft_forward
        } else {
            ix.ramp_length_ft_backward
        };
        feet.map(|ft| ft / 5280.0)
    }

    /// The OSM node the exit ramp nearest a route mile ends at, for this
    /// trip's direction of travel -- the key of the facility street chain
    /// that starts there (`World::facility_exit_route`). None for a ramp
    /// ending in a merge, or with no baked terminal that way.
    pub fn ramp_terminal_node_at(&self, route_mile: f64) -> Option<i64> {
        let (ix, forward, _) = self.interchange_with_direction_at(route_mile, 2.0)?;
        if forward {
            ix.ramp_terminal_node_forward
        } else {
            ix.ramp_terminal_node_backward
        }
    }

    /// The baked interchange nearest a route mile, or None.
    pub fn interchange_at(&self, route_mile: f64, tol_mi: f64) -> Option<&Interchange> {
        self.interchange_with_direction_at(route_mile, tol_mi)
            .map(|(ix, _, _)| ix)
    }

    /// The route mile of the baked interchange nearest `route_mile`: the
    /// frame a stop's `interchange_mi` is in. The record's own `at_mi` is
    /// counted from the start of its leg, and matches only on a first leg
    /// driven forward.
    pub fn interchange_mile_at(&self, route_mile: f64, tol_mi: f64) -> Option<f64> {
        self.interchange_with_direction_at(route_mile, tol_mi)
            .map(|(_, _, at)| at)
    }

    fn interchange_with_direction_at(
        &self,
        route_mile: f64,
        tol_mi: f64,
    ) -> Option<(&Interchange, bool, f64)> {
        let mut best: Option<&Interchange> = None;
        let mut best_forward = true;
        let mut best_at = 0.0;
        let mut best_dist = tol_mi;
        for (i, (start, leg)) in self
            .leg_starts
            .iter()
            .zip(self.route.legs.iter())
            .enumerate()
        {
            let forward = self.route.cities[i] == leg.a;
            for ix in leg.interchanges() {
                let at = start + stop_offset_for_direction(ix.at_mi, leg.miles, forward);
                let dist = (at - route_mile).abs();
                if dist <= best_dist {
                    best_dist = dist;
                    best = Some(ix);
                    best_forward = forward;
                    best_at = at;
                }
            }
        }
        best.map(|ix| (ix, best_forward, best_at))
    }

    /// The live traffic speed of a congestion zone here, or None when it
    /// flows free right now.
    fn congestion_limit_now(&self, aadt: f64, lanes: i64, start_mi: f64) -> Option<f64> {
        let ratio = congestion_ratio(aadt, self.local_hour(), lanes, self.is_weekend_now());
        congestion_limit_mph(ratio, self.corridor_limit_at(start_mi))
    }

    /// Whether a zone applies right now. Fixed zones always do; congestion
    /// zones follow the clock, and an active one gets its effective traffic
    /// speed refreshed here.
    pub fn zone_is_active(&self, zone: &mut Zone) -> bool {
        let Some(aadt) = zone.aadt else {
            return true;
        };
        // Today's volume, not the annual mean: the same draw the zone formed
        // under, so a run stays consistent with itself.
        match self.congestion_limit_now(aadt * zone.day_factor, zone.lanes, zone.start_mi) {
            None => false,
            Some(limit) => {
                zone.limit_mph = limit;
                true
            }
        }
    }

    /// `zone_is_active` for the zone at `index` in `self.zones`.
    pub fn zone_is_active_index(&mut self, index: usize) -> bool {
        let Some(aadt) = self.zones[index].aadt else {
            return true;
        };
        let (lanes, start_mi) = (self.zones[index].lanes, self.zones[index].start_mi);
        let aadt = aadt * self.zones[index].day_factor;
        match self.congestion_limit_now(aadt, lanes, start_mi) {
            None => false,
            Some(limit) => {
                self.zones[index].limit_mph = limit;
                true
            }
        }
    }

    /// Index into `zones` of the slowest zone active at this mile.
    pub fn active_zone_index_at(&mut self, mile: f64) -> Option<usize> {
        let mut best: Option<usize> = None;
        for i in 0..self.zones.len() {
            if !(self.zones[i].start_mi <= mile && mile <= self.zones[i].end_mi) {
                continue;
            }
            if !self.zone_is_active_index(i) {
                continue;
            }
            if best.is_none_or(|b| self.zones[i].limit_mph < self.zones[b].limit_mph) {
                best = Some(i);
            }
        }
        best
    }

    pub fn active_zone_at(&mut self, mile: f64) -> Option<Zone> {
        self.active_zone_index_at(mile)
            .map(|i| self.zones[i].clone())
    }

    /// The stop closest to the truck, not the first one listed.
    pub fn nearest_stop_within(&self, radius_mi: f64) -> Option<&RoadStop> {
        let mut best: Option<&RoadStop> = None;
        let mut best_dist = radius_mi;
        for stop in &self.stops {
            let dist = (stop.at_mi - self.position_mi).abs();
            if dist <= best_dist && (best.is_none() || dist < best_dist) {
                best = Some(stop);
                best_dist = dist;
            }
        }
        best
    }

    /// The next stop whose exit lies ahead within the given distance.
    pub fn upcoming_stop(&self, within_mi: f64) -> Option<&RoadStop> {
        let mut best: Option<&RoadStop> = None;
        for stop in &self.stops {
            let ahead = stop.at_mi - self.position_mi;
            if 0.0 <= ahead && ahead <= within_mi && best.is_none_or(|b| stop.at_mi < b.at_mi) {
                best = Some(stop);
            }
        }
        best
    }

    /// The stop the player planned for, or None if the plan is stale.
    pub fn planned_stop(&self) -> Option<&RoadStop> {
        let key = self.planned_stop_key.as_ref()?;
        self.stops.iter().find(|stop| stop.key() == *key)
    }

    /// The planned stop's spoken name, even if the stop itself is gone.
    pub fn planned_stop_label(&self) -> String {
        let Some(key) = self.planned_stop_key.as_ref() else {
            return String::new();
        };
        match self.planned_stop() {
            Some(stop) => stop.name.clone(),
            None => RoadStop::name_from_key(key),
        }
    }

    /// The key of the first stop with this name at or ahead of the truck
    /// (for restoring a save written before plans carried a key).
    pub fn resolve_stop_key(&self, name: &str) -> Option<String> {
        let ahead = self
            .stops
            .iter()
            .filter(|s| s.name == name && s.at_mi >= self.position_mi)
            .min_by(|a, b| a.at_mi.partial_cmp(&b.at_mi).expect("finite mileposts"));
        if let Some(stop) = ahead {
            return Some(stop.key());
        }
        self.stops.iter().find(|s| s.name == name).map(|s| s.key())
    }

    pub fn is_planned(&self, stop: &RoadStop) -> bool {
        self.planned_stop_key
            .as_ref()
            .is_some_and(|key| stop.key() == *key)
    }

    /// 'Planned stop, ' when this is the stop the player planned for.
    pub fn planned_prefix(&self, stop: &RoadStop) -> &'static str {
        if self.is_planned(stop) {
            "Planned stop, "
        } else {
            ""
        }
    }

    /// Hours to arrival at the current pace; parked or crawling it assumes a
    /// typical highway pace instead of promising infinity.
    pub fn eta_game_hours(&self, fallback_mph: f64) -> f64 {
        let mut mph = self.truck.speed_mph();
        if mph < ETA_MIN_MPH {
            mph = fallback_mph.max(1.0);
        }
        self.remaining_miles() / mph
    }

    /// Whole-percent trip progress, the figure the drivers board shows.
    pub fn progress_percent(&self) -> i64 {
        let total = if self.total_miles() == 0.0 {
            1.0
        } else {
            self.total_miles()
        };
        round_py_int(100.0 * self.position_mi / total).clamp(0, 100)
    }

    pub fn progress_summary(&self, imperial: bool) -> String {
        let remaining = spoken_distance(
            to_distance(self.remaining_miles(), imperial),
            distance_unit(imperial, false),
        );
        let dist = format!(
            "{remaining} remaining of {}",
            fmt_f(to_distance(self.total_miles(), imperial), 0)
        );
        let leg = &self.route.legs[self.current_leg_index()];
        let next_context = self.next_navigation_context(imperial);
        let terrain_text = self.current_grade_text();
        let lane_text = self.current_lane_text();
        let lane_part = if lane_text.is_empty() {
            String::new()
        } else {
            format!(" {}.", py_capitalize(&lane_text))
        };
        let toward_text = if self.is_facility_approach_route() && !self.destination_label.is_empty()
        {
            self.destination_label.clone()
        } else {
            let toward = &self.route.cities[self.current_leg_index() + 1];
            let toward_name = self.world.spoken_city(toward, Some(false));
            let state = self
                .world
                .cities
                .get(toward)
                .map(|c| c.state.as_str())
                .unwrap_or("");
            format!("{toward_name}, {state}")
        };
        // Its own sentence, so a catalog has it whole when the lane and
        // grade around it change.
        let highway = &leg.highway;
        let on_road = format!("On {highway} toward {toward_text}.");
        format!("{dist}. {on_road}{lane_part} {terrain_text}. {next_context}")
    }

    /// Lane count in plain words for the road-status readout, or empty where
    /// the bake is silent here.
    pub fn current_lane_text(&self) -> String {
        let Some((n, divided)) = self.lanes_at(None) else {
            return String::new();
        };
        if divided {
            format!(
                "divided, {} lane{} your side",
                lane_word(n),
                if n != 1 { "s" } else { "" }
            )
        } else {
            format!(
                "{} lane{} your side",
                lane_word(n),
                if n != 1 { "s" } else { "" }
            )
        }
    }

    pub fn current_grade_text(&self) -> String {
        let grade_pct = self.grade_at(self.position_mi) * 100.0;
        if grade_pct.abs() < 0.05 {
            return "Current grade 0.0 percent, level".to_string();
        }
        let direction = if grade_pct > 0.0 {
            "uphill"
        } else {
            "downhill"
        };
        let terrain = self.terrain_at(None);
        let terrain_text = if terrain == "flat" {
            String::new()
        } else {
            format!(", terrain {terrain}")
        };
        format!(
            "Current grade {} percent {direction}{terrain_text}",
            fmt_f(grade_pct.abs(), 1)
        )
    }

    pub fn next_navigation_context(&self, imperial: bool) -> String {
        let Some(cue) = self.next_navigation_cue() else {
            if self.is_facility_approach_route() && !self.destination_label.is_empty() {
                return format!("Destination {} ahead.", self.destination_label);
            }
            return format!(
                "Destination {} ahead.",
                self.world.spoken_city(
                    self.route.cities.last().map(String::as_str).unwrap_or(""),
                    None
                )
            );
        };
        let ahead = (cue.at_mi - self.position_mi).max(0.0);
        let ahead_text =
            spoken_distance(to_distance(ahead, imperial), distance_unit(imperial, false));
        match cue.kind.as_str() {
            "rest_stop" => format!("Next stop in {ahead_text}: {}.", cue.text),
            "state_crossing" => format!("Next state line in {ahead_text}: {}.", cue.text),
            "maneuver" | "onramp" => format!("Next maneuver in {ahead_text}: {}.", cue.text),
            "checkpoint" => format!("Next place in {ahead_text}: {}.", cue.text),
            "interchange" => format!("Next exit in {ahead_text}: {}.", cue.text),
            "traffic" => {
                let speed = match cue.speed_mph {
                    Some(mph) if imperial => format!(" at {} miles per hour", fmt_f(mph, 0)),
                    Some(mph) => {
                        format!(" at {} kilometers per hour", fmt_f(mph * 1.609344, 0))
                    }
                    None => String::new(),
                };
                if ahead < 0.5 {
                    format!("Traffic just ahead: {}{speed}.", cue.text)
                } else {
                    format!("Traffic in {ahead_text}: {}{speed}.", cue.text)
                }
            }
            "toll" => format!("Toll point in {ahead_text}: {}.", cue.text),
            // The cue text is a full clause, so the wrapper only places it on
            // the road (owner, 2026-08-13).
            "restriction" => format!("In {ahead_text}, {}.", cue.text),
            _ => format!("Next guidance in {ahead_text}: {}.", cue.text),
        }
    }

    pub fn next_navigation_cue(&self) -> Option<&NavigationCue> {
        self.navigation_cues.iter().find(|cue| {
            cue.at_mi > self.position_mi + 0.05
                && cue.kind != "continue"
                && cue.kind != "interchange"
        })
    }

    pub fn next_exit_context(&self) -> String {
        let Some(cue) = self.next_exit_cue() else {
            return "No listed highway exit ahead before the destination.".to_string();
        };
        let ahead = (cue.at_mi - self.position_mi).max(0.0);
        format!(
            "Next listed exit in {}: {}.",
            self.ahead_text(ahead),
            cue.text
        )
    }

    pub fn next_exit_cue(&self) -> Option<&NavigationCue> {
        self.navigation_cues
            .iter()
            .find(|cue| cue.at_mi > self.position_mi + 0.05 && cue.kind == "interchange")
    }

    /// A street chain to a gate, never a same-city highway dispatch: a real
    /// approach chain is BUILT from streets (baked local speeds or cues) or
    /// carries no baked route geometry (owner, 2026-07-24, Fernley).
    pub fn is_facility_approach_route(&self) -> bool {
        let cities = &self.route.cities;
        if cities.len() < 2 || cities[0] != cities[cities.len() - 1] {
            return false;
        }
        if self
            .route
            .legs
            .iter()
            .any(|leg| leg.local_speed_mph > 0.0 || !leg.local_cue.is_empty())
        {
            return true;
        }
        self.route
            .legs
            .iter()
            .all(|leg| leg.route_points().len() < 2)
    }

    /// This run's statutory street limit, or None where nothing covers it.
    pub fn statutory_street_mph(&self) -> Option<f64> {
        let mut state = self.local_state.clone();
        if state.is_empty() {
            // Only the corridor bake carries state per leg; not knowing
            // simply means no statutory number.
            if self.route.legs.is_empty() {
                return None;
            }
            state = self.state_at(Some(0.0));
        }
        if state.is_empty() {
            return None;
        }
        load_street_limits().statutory_mph(&state)
    }

    /// The yard entrance's own posted limit, over the road that carries it.
    /// Public because the delivery run has to be able to POST it late.
    pub fn facility_gate_zone(&self) -> Zone {
        let total = self.route.miles();
        let gate_start =
            (total - FACILITY_GATE_ZONE_MI.min(total * FACILITY_GATE_MAX_SHARE)).max(0.0);
        Zone::new(gate_start, total, FACILITY_GATE_LIMIT_MPH, "facility gate")
    }

    /// 0 = no law, 1 = Level 1 (cars may use winter tires; trucks chain,
    /// except tandem-drive trucks in Colorado), 2 = chains on every truck.
    ///
    /// The sky the truck is under sets the floor; an active Weather Service
    /// warning for this stretch (a winter storm, a blizzard, an ice storm)
    /// raises it, the way a state posts the law on the warning, not on the
    /// first flake.
    pub fn chain_law_level(&self) -> i64 {
        let from_sky = match self.weather.effects().surface {
            "ice" => 2,
            "snow" => 1,
            _ => 0,
        };
        from_sky.max(crate::sim::real_weather_alerts::alerts_chain_law_level(
            &self.live_alerts,
        ))
    }

    /// The truck is on a Colorado chain-carry corridor's mountain run in
    /// carry season (`CO_CHAIN_CARRY_CORRIDORS`), with the career calendar
    /// known. Weather doesn't matter: the rule is to carry, not to chain.
    pub fn on_co_chain_carry_corridor(&self, mile: f64) -> bool {
        use crate::sim::trip_models::{in_co_chain_carry_season, CO_CHAIN_CARRY_CORRIDORS};
        use chrono::Datelike;
        let Some(hours) = self.current_career_hours() else {
            return false;
        };
        let month = crate::sim::season::calendar_date(hours).month();
        let highway = &self.route.legs[self.leg_at_mile(mile).0].highway;
        self.chain_law_area_at(mile).is_some()
            && self.state_code_at(mile).as_deref() == Some("CO")
            && CO_CHAIN_CARRY_CORRIDORS.contains(&highway.as_str())
            && in_co_chain_carry_season(month)
    }

    /// The chain-law areas on chain corridors (`on_chain_corridor`) of
    /// chain-control states starting between route miles `from_mi` and
    /// `to_mi`, in route order, that `keep(state, highway)` accepts.
    pub fn chain_corridor_areas(
        &self,
        from_mi: f64,
        to_mi: f64,
        keep: impl Fn(&str, &str) -> bool,
    ) -> Vec<ChainCorridorAhead> {
        if self.route.legs.is_empty() {
            return Vec::new();
        }
        let mut out = Vec::new();
        for &(start, end) in &self.chain_law_areas {
            if start < from_mi || start > to_mi {
                continue;
            }
            // The grade itself, past the chain-up pullout, names the road.
            let probe = (start + CHAIN_LAW_LEAD_MI).min(end);
            let Some(state) = self.state_code_at(probe) else {
                continue;
            };
            if !CHAIN_CONTROL_STATES.contains(&state.as_str()) {
                continue;
            }
            let highway = self.route.legs[self.leg_at_mile(probe).0].highway.clone();
            if !on_chain_corridor(&state, &highway) || !keep(&state, &highway) {
                continue;
            }
            out.push(ChainCorridorAhead {
                key: format!("{state}:{highway}:{}", fmt_f(start, 1)),
                highway,
                state,
                start_mi: start,
                exit_label: self.chain_area_exit_label(start),
            });
        }
        out
    }

    /// Carry-rule corridors in season anywhere on the route
    /// (`in_chain_carry_season`): where chains must be aboard whatever the
    /// weather. Empty without a career calendar.
    pub fn chain_carry_corridors(&self) -> Vec<ChainCorridorAhead> {
        use chrono::Datelike;
        let Some(hours) = self.current_career_hours() else {
            return Vec::new();
        };
        let month = crate::sim::season::calendar_date(hours).month();
        self.chain_corridor_areas(0.0, self.route.miles(), |state, highway| {
            in_chain_carry_season(state, highway, month)
        })
    }

    /// The first chain-law area within `within_mi` ahead of the truck where
    /// the posted law chains this rig (`rig_must_chain`): no reminder for
    /// Colorado or Wyoming Level 1 on a tandem drive, or with no law posted.
    pub fn upcoming_chain_corridor(&self, within_mi: f64) -> Option<ChainCorridorAhead> {
        let level = self.chain_law_level();
        if level <= 0 {
            return None;
        }
        let here = self.position_mi;
        self.chain_corridor_areas(here, here + within_mi, |state, _| {
            crate::sim::trip_models::rig_must_chain(Some(state), level)
        })
        .into_iter()
        .next()
    }

    /// The real exit label of the nearest numbered interchange at or before
    /// route mile `at` (in travel order) on the same leg, within
    /// `CHAIN_CORRIDOR_EXIT_LOOKBACK_MI`. Empty when there is none: never
    /// made up.
    pub fn chain_area_exit_label(&self, at: f64) -> String {
        if self.route.legs.is_empty() {
            return String::new();
        }
        let (i, leg_start) = self.leg_at_mile(at);
        let offset = at - leg_start;
        let leg = &self.route.legs[i];
        let forward = self.route.cities.get(i).is_none_or(|city| *city == leg.a);
        let native = if forward { offset } else { leg.miles - offset };
        let mut best = (CHAIN_CORRIDOR_EXIT_LOOKBACK_MI, String::new());
        for ix in leg.interchanges() {
            let before = if forward {
                native - ix.at_mi
            } else {
                ix.at_mi - native
            };
            let label = ix.exit_label();
            if (0.0..=best.0).contains(&before) && !label.is_empty() {
                best = (before, label);
            }
        }
        best.1
    }

    /// The truck is on a Montana chain-law area (the mountain run) between
    /// October 1 and April 30 (`in_mt_chain_carry_season`).
    pub fn on_mt_chain_carry_run(&self, mile: f64) -> bool {
        use chrono::Datelike;
        let Some(hours) = self.current_career_hours() else {
            return false;
        };
        let month = crate::sim::season::calendar_date(hours).month();
        self.chain_law_area_at(mile).is_some()
            && self.state_code_at(mile).as_deref() == Some("MT")
            && crate::sim::trip_models::in_mt_chain_carry_season(month)
    }

    /// Whether the truck meets the active chain law at this milepost: chains
    /// on, or a law that doesn't chain the rig here (`rig_must_chain`).
    /// Wyoming Level 1 asks for snow tires or chains (W.S. 31-5-956(e)), so
    /// there winter tires count and all-season tires without chains don't.
    pub fn rig_compliant_at(&self, mile: f64) -> bool {
        let level = self.chain_law_level();
        if self.truck.chains_on || level <= 0 {
            return true;
        }
        let state = self.state_code_at(mile);
        if state.as_deref() == Some("WY") && level == 1 {
            return self.truck.tire_type == crate::sim::vehicle::TIRE_WINTER;
        }
        !crate::sim::trip_models::rig_must_chain(state.as_deref(), level)
    }

    /// The chain-law warning's missing equipment: "snow tires or chains" at
    /// Wyoming Level 1, "chains" everywhere else.
    pub fn chain_law_need_at(&self, mile: f64) -> &'static str {
        if self.state_code_at(mile).as_deref() == Some("WY") && self.chain_law_level() == 1 {
            "snow tires or chains"
        } else {
            "chains"
        }
    }

    /// Index of the chain-law area containing this milepost, or None.
    pub fn chain_law_area_at(&self, mile: f64) -> Option<usize> {
        self.chain_law_areas
            .iter()
            .position(|(start, end)| *start <= mile && mile <= *end)
    }

    pub fn leg_traffic_density(
        &self,
        leg: &crate::data::world_models::Leg,
        bad_weather_bias: f64,
        night: bool,
    ) -> f64 {
        let metro_bias = if leg.checkpoints().is_empty() {
            0.0
        } else {
            0.18
        };
        let night_bias = if night { -0.08 } else { 0.0 };
        let rush_bias = self.rush_hour_traffic_bias(leg);
        let density = 0.86_f64.min(0.05_f64.max(
            0.22 + leg.miles / 900.0 + metro_bias + bad_weather_bias + night_bias + rush_bias,
        ));
        density * self.hazard_scale
    }

    /// Direction-aware lane runs across the whole route in travel order.
    /// Adjacent equal runs merge; runs shorter than `LANE_RUN_MIN_MI` are
    /// absorbed into the value before them.
    pub fn build_lane_runs(&self) -> Vec<LaneRun> {
        let mut runs: Vec<LaneRun> = Vec::new();
        for (i, leg) in self.route.legs.iter().enumerate() {
            let leg_start = self.leg_starts[i];
            let forward = self.route.cities[i] == leg.a;
            let segs: Vec<_> = if forward {
                leg.lane_segments().iter().collect()
            } else {
                leg.lane_segments().iter().rev().collect()
            };
            for seg in segs {
                let (s, e) = if forward {
                    (leg_start + seg.start_mi, leg_start + seg.end_mi)
                } else {
                    (
                        leg_start + (leg.miles - seg.end_mi),
                        leg_start + (leg.miles - seg.start_mi),
                    )
                };
                // Capped like every other answer about lanes the DRIVER has
                // (owner playtest, Denver->Silverthorne, 2026-08-19).
                runs.push(LaneRun {
                    start_mi: s,
                    end_mi: e,
                    lanes: MAX_DRIVABLE_LANES.min(seg.your_side()),
                    divided: seg.divided(),
                });
            }
        }
        if runs.is_empty() {
            return Vec::new();
        }
        runs.sort_by(|a, b| {
            a.start_mi
                .partial_cmp(&b.start_mi)
                .expect("finite mileposts")
        });

        fn coalesce(rows: Vec<LaneRun>) -> Vec<LaneRun> {
            let mut out: Vec<LaneRun> = Vec::new();
            for r in rows {
                if let Some(last) = out.last_mut() {
                    if r.lanes == last.lanes
                        && r.divided == last.divided
                        && r.start_mi - last.end_mi <= 0.3
                    {
                        last.end_mi = r.end_mi;
                        continue;
                    }
                }
                out.push(r);
            }
            out
        }

        let merged = coalesce(runs);
        let mut collapsed: Vec<LaneRun> = Vec::new();
        for r in merged {
            if let Some(last) = collapsed.last_mut() {
                if (r.end_mi - r.start_mi) < LANE_RUN_MIN_MI {
                    last.end_mi = r.end_mi;
                    continue;
                }
            }
            collapsed.push(r);
        }
        coalesce(collapsed)
    }

    pub fn lane_change_message(prev_side: i64, new_side: i64) -> String {
        if new_side > prev_side {
            return format!("Road widens to {} lanes your side.", lane_word(new_side));
        }
        format!(
            "Down to {} lane{} your side.",
            lane_word(new_side),
            if new_side != 1 { "s" } else { "" }
        )
    }
}
