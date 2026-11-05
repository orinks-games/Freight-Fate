//! Traffic and patrol lookup helpers for trip simulation (port of
//! `freight_fate/sim/trip_traffic.py`, the former `TripTrafficMixin`).

use indexmap::IndexMap;
use regex::Regex;

use crate::data::world_models::Leg;
use crate::pyrandom::PyRandom;
use crate::sim::enforcement_posts::{
    post_seed, EnforcementPost, KIND_CHAIN, KIND_CMV, KIND_FIXED_SCALE, KIND_MEDIAN, KIND_ROVING,
    KIND_SCALE_APRON, KIND_URBAN, KIND_WORK_ZONE,
};
use crate::sim::real_traffic::{road_name_matches, RealTrafficProvider};
use crate::sim::real_traffic_parsers::{split_cars_title, TrafficEvent};
use crate::sim::trip::Trip;
use crate::sim::trip_models::*;
use crate::sim::trip_route_helpers::{leg_states, leg_track, runs_against_travel, snap_to_leg};
use crate::speech_text::SpokenMessage;

/// Incident lookups filter the whole cached state feed by distance, so
/// re-check on a mile cadence rather than every simulation tick.
pub const REAL_TRAFFIC_CHECK_INTERVAL_MI: f64 = 1.0;
/// How far ahead of the truck a live road report is spoken.
pub const LIVE_REPORT_AHEAD_MI: f64 = 10.0;
/// The feed lookup around each probe point: the lookahead, plus room for a
/// road that winds away from the straight line.
const LIVE_REPORT_QUERY_RADIUS_MI: f64 = LIVE_REPORT_AHEAD_MI + 2.0;
/// How far off the line a report that names the truck's road may sit: feed
/// markers drift a little from the mapped road.
const LIVE_REPORT_NAMED_OFF_ROAD_MI: f64 = 1.0;
/// How far off the line a report that names no road may sit before it reads
/// as a crossing road's.
const LIVE_REPORT_UNNAMED_OFF_ROAD_MI: f64 = 0.25;

/// The duck-typed `traffic_provider` the Python trip asked two questions of.
/// The real provider answers from its cache; a test double answers from a
/// list. Python wrapped both calls in `try/except`; a Rust provider simply
/// returns an empty list where it cannot answer.
pub trait TrafficProvider {
    /// Incidents within `radius_mi` of a point in `state`.
    fn get_events_near(
        &self,
        state: &str,
        latitude: f64,
        longitude: f64,
        radius_mi: f64,
    ) -> Vec<TrafficEvent>;

    /// Construction events near a route's geometry on a named road.
    fn get_construction_near_route(
        &self,
        state: &str,
        route_points: &[(f64, f64)],
        road_name: Option<&str>,
        radius_mi: f64,
    ) -> Vec<TrafficEvent>;
}

impl TrafficProvider for RealTrafficProvider {
    fn get_events_near(
        &self,
        state: &str,
        latitude: f64,
        longitude: f64,
        radius_mi: f64,
    ) -> Vec<TrafficEvent> {
        RealTrafficProvider::get_events_near(self, state, latitude, longitude, radius_mi)
    }

    fn get_construction_near_route(
        &self,
        state: &str,
        route_points: &[(f64, f64)],
        road_name: Option<&str>,
        radius_mi: f64,
    ) -> Vec<TrafficEvent> {
        RealTrafficProvider::get_construction_near_route(
            self,
            state,
            route_points,
            road_name,
            radius_mi,
        )
    }
}

impl Trip {
    pub fn traffic_context(&self) -> Option<TrafficContext> {
        self.traffic_manager
            .lead_vehicle(self.position_mi, self.truck.speed_mph())
    }

    pub fn traffic_target_speed(&self) -> Option<f64> {
        self.traffic_context().map(|context| context.lead.speed_mph)
    }

    pub fn npc_traffic_status(&self) -> String {
        let Some(context) = self.traffic_context() else {
            return "Traffic: no close traffic ahead.".to_string();
        };
        let lead = &context.lead;
        format!(
            "Traffic: {}, {} ahead, {}.",
            lead.status_label(),
            self.gap_text(context.gap_mi),
            self.speed_text(lead.speed_mph)
        )
    }

    pub fn check_npc_traffic_cues(&mut self) {
        // Gate BEFORE next_situation: returning a situation marks its vehicle
        // announced, so a gated call would burn the announcement without
        // speaking it (see road_event_pacing).
        if !self.event_breather.ready("traffic") {
            return;
        }
        let Some(situation) = self
            .traffic_manager
            .next_situation(self.position_mi, self.truck.speed_mph())
        else {
            return;
        };
        self.event_breather.spoke("traffic");
        let lead = situation.vehicle;
        let cue = NavigationCue::new(
            &format!("npc:{}", lead.key),
            "traffic",
            lead.position_mi,
            &lead.status_label(),
            "",
        )
        .with_speed(Some(lead.speed_mph));
        self.emit(
            TripEventKind::GpsCue,
            situation.message,
            TripEventData {
                cue: Some(cue),
                npc_vehicle: Some(lead),
                ..Default::default()
            },
        );
    }

    // -- CB chatter ----------------------------------------------------------
    // The officer is a "trooper" here as everywhere else: CB slang ("bear")
    // was dropped because most players never learned it (owner, 2026-10-11).

    /// How firmly the channel is standing behind this report: a blind
    /// player cannot falsify a CB call, so an unreliable channel carries its
    /// unreliability in the words. Wrong in one direction only: it may call
    /// a post that turns out to be empty, never say the road is clear.
    pub fn cb_confidence(&self, post: &EnforcementPost) -> &'static str {
        let rolled =
            PyRandom::new_from_str(&post_seed(self.seed, &post.id(), "cb_confidence")).random();
        if post.staffed {
            if rolled < 0.55 {
                "strong"
            } else {
                "ordinary"
            }
        } else if rolled < 0.7 {
            "thin"
        } else {
            "ordinary"
        }
    }

    /// Where on the road the CB says it is. Words, never pan alone.
    pub fn cb_side(post: &EnforcementPost) -> &'static str {
        match post.kind.as_str() {
            KIND_MEDIAN => "in the median",
            KIND_ROVING => "working with traffic",
            KIND_WORK_ZONE => "in the work zone",
            KIND_SCALE_APRON => "on the scale apron",
            KIND_FIXED_SCALE => "at the scale",
            KIND_URBAN => "coming into town",
            KIND_CMV => "on the right shoulder",
            KIND_CHAIN => "at the chain-up area",
            _ => "up ahead",
        }
    }

    /// Player-facing CB chatter for one enforcement post. The confidence
    /// framing is the same for every kind; what differs is what the post
    /// actually is. A trooper sighting is for an officer on the open road,
    /// never for a fixed inspection facility or a chain checkpoint.
    ///
    /// No "CB chatter" opener: the CB's own sound marks the line as radio
    /// talk, and the words are the report (owner, 2026-10-01). A fresh
    /// report leads with how far; a stale one with who said it.
    pub fn cb_patrol_message(&self, post: &EnforcementPost, ahead_mi: f64) -> String {
        let distance = cap_first(&self.ahead_text(ahead_mi.max(0.0)));
        let confidence = self.cb_confidence(post);
        let side = Self::cb_side(post);
        if post.kind == KIND_WORK_ZONE {
            return match confidence {
                "strong" => format!("{distance}: two drivers say troopers are working {side}."),
                "ordinary" => format!("{distance}: a driver says troopers are working {side}."),
                _ => format!("Somebody said troopers were working {side} a while back."),
            };
        }
        if post.kind == KIND_SCALE_APRON || post.kind == KIND_FIXED_SCALE {
            return match confidence {
                "strong" => format!("{distance}: two drivers say they're checking logs {side}."),
                "ordinary" => format!("{distance}: a driver says they're checking logs {side}."),
                _ => format!("Somebody said they were checking logs {side} a while back."),
            };
        }
        if post.kind == KIND_CMV {
            return match confidence {
                "strong" => format!(
                    "{distance}: two drivers say they're checking logs and equipment {side}."
                ),
                "ordinary" => {
                    format!("{distance}: a driver says they're checking logs and equipment {side}.")
                }
                _ => format!(
                    "Somebody said they were checking logs and equipment {side} a while back."
                ),
            };
        }
        if post.kind == KIND_CHAIN {
            return match confidence {
                "strong" => format!(
                    "{distance}: two drivers say the chain control is checking rigs {side}."
                ),
                "ordinary" => {
                    format!("{distance}: a driver says the chain control is checking rigs {side}.")
                }
                _ => format!(
                    "Somebody said the chain control was checking rigs {side} a while back."
                ),
            };
        }
        match confidence {
            "strong" => format!("{distance}: two drivers call a trooper {side}."),
            "ordinary" => format!("{distance}: a driver reports a trooper {side}."),
            _ => format!("Somebody called a trooper {side} a while back."),
        }
    }

    /// CB chatter for a trooper who already has somebody else stopped.
    pub fn cb_tableau_message(&self, post: &EnforcementPost, ahead_mi: f64) -> String {
        let distance = cap_first(&self.ahead_text(ahead_mi.max(0.0)));
        let confidence = self.cb_confidence(post);
        let side = Self::cb_side(post);
        match confidence {
            "strong" => {
                format!(
                    "{distance}: two drivers say a trooper already has somebody stopped {side}."
                )
            }
            "ordinary" => {
                format!("{distance}: a driver says a trooper already has somebody stopped {side}.")
            }
            _ => format!("Somebody said a trooper had somebody stopped {side} a while back."),
        }
    }

    /// The on-demand road-ahead readout's enforcement clause, at full detail
    /// whatever the presence setting is.
    pub fn cb_patrol_status(&self, post: &EnforcementPost, ahead_mi: f64) -> String {
        let distance = self.ahead_text(ahead_mi.max(0.0));
        let where_ = Self::cb_side(post);
        if ahead_mi <= 0.0 {
            return format!("an enforcement post {where_} on this stretch");
        }
        format!("an enforcement post {where_} in {distance}")
    }

    /// Nearest enforcement post at or ahead of the truck in the lookahead.
    pub fn next_patrol_within(&self, within_mi: f64) -> Option<&EnforcementPost> {
        self.next_post_within(within_mi)
    }

    /// The trip's own rush-hour bias, read at the DEPARTURE hour (the
    /// manager's reads the live clock), on the local clock like the manager's.
    pub fn rush_hour_traffic_bias(&self, leg: &Leg) -> f64 {
        let hour = self.local_start_hour();
        if !RUSH_HOUR_WINDOWS
            .iter()
            .any(|(start, end)| *start <= hour && hour < *end)
        {
            return 0.0;
        }
        if leg.checkpoints().is_empty() {
            0.06
        } else {
            0.14
        }
    }

    pub fn traffic_pressure_at(&self, mile: Option<f64>) -> Option<TrafficPressure> {
        let sample = mile.unwrap_or(self.position_mi);
        self.traffic_pressures
            .iter()
            .filter(|p| p.start_mi <= sample && sample <= p.end_mi)
            .max_by(|a, b| {
                a.intensity
                    .partial_cmp(&b.intensity)
                    .expect("finite intensities")
            })
            .cloned()
    }

    /// Speak real-time incidents from the state 511 feeds that sit on the
    /// truck's own road, on its side of it, within
    /// [`LIVE_REPORT_AHEAD_MI`] ahead. A feed lists every report in the
    /// state; ramps on crossing roads and the other carriageway stay quiet
    /// (Orinks, 2026-11-05: thirteen reports in ninety seconds near Gary,
    /// nearly all for other roads).
    pub fn check_real_traffic_events(&mut self) {
        let Some(provider) = self.traffic_provider.clone() else {
            return;
        };
        if self.position_mi < self.next_real_traffic_check_mi {
            return;
        }
        self.next_real_traffic_check_mi = self.position_mi + REAL_TRAFFIC_CHECK_INTERVAL_MI;

        // The truck's spot and the end of the lookahead, each with the state
        // whose feed covers it: a report just over a state line is in the
        // next state's feed.
        let mut probes: Vec<(String, f64, f64)> = Vec::new();
        for mile in [self.position_mi, self.position_mi + LIVE_REPORT_AHEAD_MI] {
            let Some((state, lat, lon)) = self.route_point_at(mile) else {
                continue;
            };
            if !probes.iter().any(|(s, _, _)| *s == state) {
                probes.push((state, lat, lon));
            }
        }
        let mut seen: Vec<String> = Vec::new();
        let mut events: Vec<TrafficEvent> = Vec::new();
        for (state, lat, lon) in &probes {
            for event in provider.get_events_near(state, *lat, *lon, LIVE_REPORT_QUERY_RADIUS_MI) {
                if !seen.contains(&event.id) {
                    seen.push(event.id.clone());
                    events.push(event);
                }
            }
        }

        let mut reports: Vec<(f64, TrafficEvent, String)> = Vec::new();
        for event in events {
            if event.severity != "high" && event.severity != "medium" {
                continue;
            }
            // Construction is spoken through the construction-zone system.
            if event.event_type == "construction" {
                continue;
            }
            let event_key = format!("real_traffic:{}", event.id);
            if self.announced_real_traffic.contains(&event_key) {
                continue;
            }
            let Some((mile, highway)) = self.live_report_mile(&event) else {
                continue;
            };
            let ahead_mi = mile - self.position_mi;
            // Behind the truck it no longer matters; further out it is
            // spoken once it comes within range.
            if !(0.0..=LIVE_REPORT_AHEAD_MI).contains(&ahead_mi) {
                continue;
            }
            let text = live_report_text(&event, &highway);
            if text.is_empty() {
                continue;
            }
            reports.push((ahead_mi, event, text));
        }
        reports.sort_by(|a, b| a.0.total_cmp(&b.0));

        for (ahead_mi, event, text) in reports {
            let event_key = format!("real_traffic:{}", event.id);
            // "Live road report", not "traffic alert": these describe the
            // REAL road today; the simulation does not act on them at all
            // (Brandon, 2026-08-21; owner call the same day).
            let distance = self.ahead_text(ahead_mi);
            let mut message = format!("Live road report, {distance} ahead: {text}");
            if !message.ends_with('.') {
                message.push('.');
            }
            if let Some(lanes) = event.lanes_affected.as_ref().filter(|l| !l.is_empty()) {
                message.push(' ');
                message.push_str(&format!("{lanes} affected."));
            }
            self.emit(
                TripEventKind::GpsCue,
                SpokenMessage::new(message),
                TripEventData {
                    real_traffic_event: Some(event),
                    ..Default::default()
                },
            );
            self.announced_real_traffic.insert(event_key);
        }
    }

    /// The state, latitude and longitude of the road at a trip mile; None
    /// past the end of the route or on a leg with no route points.
    fn route_point_at(&self, mile: f64) -> Option<(String, f64, f64)> {
        if mile > self.total_miles() {
            return None;
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
        let state = leg_state_at(leg, leg_offset);
        if state.is_empty() {
            return None;
        }
        let point = leg.route_points().iter().min_by(|a, b| {
            (a.at_mi - leg_offset)
                .abs()
                .total_cmp(&(b.at_mi - leg_offset).abs())
        })?;
        Some((state, point.lat, point.lon))
    }

    /// Where a report meets the road the truck will drive in the next
    /// [`LIVE_REPORT_AHEAD_MI`], as a trip mile, with that leg's highway.
    /// None when the report names another road, sits off the road, or holds
    /// only the other direction of travel.
    fn live_report_mile(&self, event: &TrafficEvent) -> Option<(f64, String)> {
        let (Some(lat), Some(lon)) = (event.latitude, event.longitude) else {
            return None;
        };
        let reach = self.position_mi + LIVE_REPORT_AHEAD_MI;
        self.leg_starts
            .iter()
            .zip(self.route.legs.iter())
            .enumerate()
            .filter(|(_, (start, leg))| **start <= reach && **start + leg.miles >= self.position_mi)
            .filter_map(|(i, (start, leg))| {
                // A report that names its road must name this one; one that
                // does not has to sit right on the line.
                let max_off_mi = match report_on_highway(&event.road_name, &leg.highway) {
                    Some(false) => return None,
                    Some(true) => LIVE_REPORT_NAMED_OFF_ROAD_MI,
                    None => LIVE_REPORT_UNNAMED_OFF_ROAD_MI,
                };
                let forward = self.route.cities[i] == leg.a;
                let snap = snap_to_leg(lat, lon, leg, forward, *start)?;
                (snap.off_road_mi <= max_off_mi
                    && !runs_against_travel(&event.direction, snap.bearing_deg))
                .then(|| (snap, leg.highway.clone()))
            })
            .min_by(|a, b| a.0.off_road_mi.total_cmp(&b.0.off_road_mi))
            .map(|(snap, highway)| (snap.mile, highway))
    }

    /// `{(highway, state): [(lat, lon), ...]}` from the route legs, each
    /// leg's road filled in about every mile and listed under every state it
    /// runs in, so a leg into California reads Caltrans for its California
    /// miles.
    pub fn collect_route_geometry(&self) -> IndexMap<(String, String), Vec<(f64, f64)>> {
        let mut geometry: IndexMap<(String, String), Vec<(f64, f64)>> = IndexMap::new();
        for (i, leg) in self.route.legs.iter().enumerate() {
            let forward = self.route.cities[i] == leg.a;
            let track = leg_track(leg);
            let highway = leg.highway.trim().to_uppercase();
            for state in leg_states(leg, forward) {
                geometry
                    .entry((highway.clone(), state))
                    .or_default()
                    .extend_from_slice(&track);
            }
        }
        geometry
    }

    /// Query the traffic provider for real construction events and convert
    /// them into zones mapped to route miles. Empty when no provider is
    /// available, the route is a facility approach, or nothing is close.
    pub fn place_real_construction_zones(&self) -> Vec<Zone> {
        let Some(provider) = self.traffic_provider.as_ref() else {
            return Vec::new();
        };
        if self.is_facility_approach_route() {
            return Vec::new();
        }
        let mut real_zones: Vec<Zone> = Vec::new();
        let mut seen_spans: Vec<(f64, f64)> = Vec::new();
        let route_geo = self.collect_route_geometry();

        for ((highway, state), points) in route_geo.iter() {
            if points.is_empty() {
                continue;
            }
            let events = provider.get_construction_near_route(state, points, Some(highway), 3.0);
            if events.is_empty() {
                continue;
            }
            for event in events {
                let (Some(latitude), Some(longitude)) = (event.latitude, event.longitude) else {
                    continue;
                };
                // Find the nearest leg and snap to route mile; a closure on
                // the far carriageway from the truck is not on its road.
                let nearest = self
                    .leg_starts
                    .iter()
                    .zip(self.route.legs.iter())
                    .enumerate()
                    .filter_map(|(i, (start, leg))| {
                        let forward = self.route.cities[i] == leg.a;
                        snap_to_leg(latitude, longitude, leg, forward, *start)
                    })
                    .min_by(|a, b| a.off_road_mi.total_cmp(&b.off_road_mi));
                let Some(best_leg_mile) = nearest
                    .filter(|snap| !runs_against_travel(&event.direction, snap.bearing_deg))
                    .map(|snap| snap.mile)
                else {
                    continue;
                };
                let zone_length = Self::construction_zone_length(&event);
                let mut start_mi = (best_leg_mile - zone_length / 2.0).max(0.0);
                let end_mi = self.total_miles().min(start_mi + zone_length);
                start_mi = (end_mi - zone_length).max(0.0);

                // A work zone the driver cannot be warned about is one the
                // game must not place (owner report, 2026-08-16): the
                // warning has to fit.
                if start_mi < CONSTRUCTION_TAPER_MI {
                    continue;
                }
                if seen_spans.iter().any(|(s_start, s_end)| {
                    start_mi < s_end + ZONE_MIN_GAP_MI && end_mi > s_start - ZONE_MIN_GAP_MI
                }) {
                    continue;
                }
                let mut closed_side = Self::construction_closed_side(&event);
                let taper_start = (start_mi - CONSTRUCTION_TAPER_MI).max(0.0);
                // Never above the road's own limit (see `place_zones`).
                let road = self.lowest_limit_over(taper_start, end_mi);
                let limit_mph = Self::construction_zone_speed(&event).min(road);
                // A reported closure still needs a lane to merge into.
                if closed_side.is_some() && !self.span_is_multilane(taper_start, end_mi) {
                    closed_side = None;
                }
                real_zones.push(
                    Zone::new(
                        taper_start,
                        start_mi,
                        CONSTRUCTION_TAPER_LIMIT_MPH.min(road),
                        "construction merge",
                    )
                    .with_closed_side(closed_side),
                );
                real_zones.push(
                    Zone::new(start_mi, end_mi, limit_mph, "construction")
                        .with_closed_side(closed_side),
                );
                seen_spans.push((taper_start, end_mi));
            }
        }
        real_zones.sort_by(|a, b| {
            a.start_mi
                .partial_cmp(&b.start_mi)
                .expect("finite mileposts")
        });
        real_zones
    }

    /// Determine the length of a construction zone from event data.
    pub fn construction_zone_length(event: &TrafficEvent) -> f64 {
        // Try to parse from location text (e.g., "Between milepost 45 and 47")
        let location = &event.location_text;
        if !location.is_empty() {
            let re = Regex::new(r"milepost (\d+(?:\.\d+)?) and (\d+(?:\.\d+)?)").expect("static");
            if let Some(caps) = re.captures(location) {
                let start: f64 = caps[1].parse().unwrap_or(0.0);
                let end: f64 = caps[2].parse().unwrap_or(0.0);
                let length = (end - start).abs();
                if (0.5..=20.0).contains(&length) {
                    return length;
                }
            }
        }
        // Default lengths based on work type
        match event.work_type.as_str() {
            "bridge" => 4.0,
            "paving" => 5.0,
            "utility" => 3.0,
            "maintenance" => 2.0,
            "construction" => 5.0,
            _ => 4.0,
        }
    }

    /// Determine the reduced speed limit for a construction zone.
    pub fn construction_zone_speed(event: &TrafficEvent) -> f64 {
        match event.closure.as_str() {
            "full closure" => 15.0,
            "alternating" => 35.0,
            "shoulder" => 55.0, // minimal reduction
            _ => 45.0,          // Single lane closure or default
        }
    }

    /// Which side of the road is coned off, or None if no closure.
    pub fn construction_closed_side(event: &TrafficEvent) -> Option<&'static str> {
        match event.closure.as_str() {
            "full closure" => Some("right"), // right lane closed as part of full closure
            "alternating" | "single lane" => Some("right"), // right lane (typically the closed one)
            "shoulder" => None,              // shoulder work doesn't close a travel lane
            _ => None,
        }
    }
}

/// Whether a feed's road name is the leg's highway: None when the feed names
/// no road. Concurrent routes ("I-80/I-94") match on any of their names, and
/// a state route matches by number whatever the state calls it ("IN 912"
/// is the map's "SR-912").
pub fn report_on_highway(report_road: &str, highway: &str) -> Option<bool> {
    let report_road = report_road.trim();
    if report_road.is_empty() {
        return None;
    }
    let highway = highway.trim();
    let names = report_road
        .split(['/', '&', ',', ';'])
        .map(|part| part.split(" - ").next().unwrap_or("").trim())
        .filter(|part| !part.is_empty());
    for name in names {
        if road_name_matches(name, highway) {
            return Some(true);
        }
        if let (Some(a), Some(b)) = (state_route_number(name), state_route_number(highway)) {
            if a == b {
                return Some(true);
            }
        }
    }
    Some(false)
}

/// The number of a state route ("SR-26", "IN 912", "State Road 3"); None
/// for an interstate, a US route, or anything without a plain number.
fn state_route_number(road: &str) -> Option<String> {
    let road = road.trim().to_uppercase();
    let split = road.find(|c: char| c.is_ascii_digit())?;
    let (prefix, rest) = road.split_at(split);
    let prefix: String = prefix.chars().filter(|c| c.is_ascii_alphabetic()).collect();
    if prefix.is_empty()
        || matches!(
            prefix.as_str(),
            "I" | "INTERSTATE" | "US" | "USHWY" | "USROUTE"
        )
    {
        return None;
    }
    let number: String = rest
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric())
        .collect();
    Some(number)
}

/// The spoken text of a report. A CARS title leads with its road and a
/// mile-point range ("US 20 (Mile Point 42.5 - 42.61): Lane closed."); on the
/// truck's own road only the part after the colon is news.
pub fn live_report_text(event: &TrafficEvent, highway: &str) -> String {
    let description = event.description.trim();
    let (road, _, rest) = split_cars_title(description);
    if !road.is_empty() && report_on_highway(&road, highway) == Some(true) && !rest.is_empty() {
        return rest;
    }
    description.to_string()
}

/// A distance phrase opening a sentence: "half a mile" becomes "Half a mile".
fn cap_first(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}
