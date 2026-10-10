//! Laying the run out at trip build -- stops, navigation cues, roadside
//! callouts, billboards, curves, lane runs, zones and chain-law areas --
//! plus the per-tick checks that walk those schedules (the placement half of
//! `trip.py`).

use crate::data::billboards::{corridor_signs, random_billboard, regional_genre_signs, SignAnchor};
use crate::data::curves::{bend_bank, route_curves, superelevation_at, RouteCurve};
use crate::pyfmt::{fmt_f, py_str_float};
use crate::pyrandom::PyRandom;
use crate::sim::road_event_pacing::CHATTER_GAP_REAL_S;
use crate::sim::trip_models::*;
use crate::sim::trip_route_helpers::{
    leg_heading, served_interchange, stop_exit_label, stop_offset_for_direction,
};
use crate::speech_text::SpokenMessage;
use crate::units::spoken_distance;

use super::{
    cue_direction, Trip, EXIT_APPROACH_DECOMPRESS_SLACK, LIMIT_SCAN_STRIDE_MI,
    PACENOTE_BRAKE_MPH_PER_S, PACENOTE_GENTLE_MARGIN_MPH, PACENOTE_LEAD_FLOOR_S,
    PACENOTE_LINK_GAP_MI, PACENOTE_MARGIN_MPH, PACENOTE_MAX_LEAD_MI, PACENOTE_MIN_LEAD_MI,
    PACENOTE_REACTION_S,
};

/// Python `text[:1].lower() + text[1:]`.
fn lower_first(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        None => String::new(),
        Some(first) => {
            let mut out: String = first.to_lowercase().collect();
            out.push_str(chars.as_str());
            out
        }
    }
}

/// ", then {distance} on it" -- the length of the road just joined, said
/// as time on it rather than as a bare trailing distance.
fn on_it_tail(distance: &str) -> String {
    format!(", then {distance} on it")
}

/// The spoken length of a surface segment (see `Trip::surface_distance_tail`):
/// empty under a fifth of a mile, quarter-mile / half-mile (500 meters / 1
/// kilometer) buckets below a mile, whole units above.
fn surface_tail_text(miles: f64, imperial: bool) -> String {
    if miles < 0.2 {
        return String::new();
    }
    if imperial {
        if miles < 0.4 {
            return on_it_tail("a quarter mile");
        }
        if miles < 0.75 {
            return on_it_tail("half a mile");
        }
        return on_it_tail(&spoken_distance(miles, "mile"));
    }
    let km = miles * 1.609344;
    if km < 0.65 {
        return on_it_tail("500 meters");
    }
    if km < 1.2 {
        return on_it_tail("1 kilometer");
    }
    on_it_tail(&spoken_distance(km, "kilometer"))
}

impl Trip {
    pub fn place_stops(&self) -> Vec<RoadStop> {
        Self::route_stops(&self.route, self.bobtail)
    }

    /// The same direction and access filtering for dispatch and active trips.
    pub fn route_stops(route: &crate::data::world_models::Route, bobtail: bool) -> Vec<RoadStop> {
        let mut out = Vec::new();
        let mut start = 0.0;
        for (i, leg) in route.legs.iter().enumerate() {
            let forward = route.cities[i] == leg.a;
            let mut leg_stops: Vec<_> = leg.stops.iter().collect();
            leg_stops.sort_by(|a, b| {
                stop_offset_for_direction(a.at_mi, leg.miles, forward)
                    .partial_cmp(&stop_offset_for_direction(b.at_mi, leg.miles, forward))
                    .expect("finite mileposts")
            });
            for stop in leg_stops {
                if !(stop.curated()
                    && stop.applies_to_direction(forward)
                    && stop.accessible_to(bobtail))
                {
                    continue;
                }
                let offset = stop_offset_for_direction(stop.at_mi, leg.miles, forward);
                let at = start + offset;
                let exit_label = stop_exit_label(leg, stop, 2.0);
                // The served interchange in the route's frame, by the same
                // arithmetic the interchange lookups use, so it matches one
                // exactly.
                let interchange_mi = served_interchange(leg, stop)
                    .map(|ix| start + stop_offset_for_direction(ix.at_mi, leg.miles, forward));
                out.push(RoadStop {
                    name: stop.name.clone(),
                    at_mi: at,
                    stop_type: stop.stop_type.clone(),
                    actions: stop.actions.clone(),
                    services: stop.services.clone(),
                    parking: stop.parking.clone(),
                    exit_label,
                    parking_spaces: stop.parking_spaces,
                    vehicle_access: stop.effective_vehicle_access().to_string(),
                    interchange_mi,
                });
            }
            start += leg.miles;
        }
        Self::merge_shared_city_stops(out)
    }

    /// One entry per facility, not one per leg that lists it: a city's stops
    /// are picked up twice, two miles apart, and the truck passes a single
    /// building. Keep the one reached first and let it borrow the twin's
    /// exit label if it has none.
    pub fn merge_shared_city_stops(stops: Vec<RoadStop>) -> Vec<RoadStop> {
        let mut merged: Vec<RoadStop> = Vec::new();
        for stop in stops {
            let twin = merged.iter_mut().rev().find(|kept| {
                kept.name == stop.name
                    && (stop.at_mi - kept.at_mi).abs() <= SHARED_CITY_STOP_MERGE_MI
            });
            match twin {
                None => merged.push(stop),
                Some(twin) => {
                    if twin.exit_label.is_empty() && !stop.exit_label.is_empty() {
                        twin.exit_label = stop.exit_label.clone();
                    }
                    if twin.interchange_mi.is_none() {
                        twin.interchange_mi = stop.interchange_mi;
                    }
                }
            }
        }
        merged
    }

    /// How far the truck stays on the street it is turning onto, worded so
    /// it cannot be mistaken for the distance TO the turn: the countdown
    /// before it already ran 400 meters, 300 meters, so a bare "; 1
    /// kilometer" at the corner sounded like the turn moving away (agent
    /// drive, 2026-09-01). City blocks under a fifth of a mile say nothing.
    pub fn surface_distance_tail(&self, miles: f64) -> String {
        surface_tail_text(miles, self.imperial())
    }

    pub fn build_navigation_cues(&self) -> Vec<NavigationCue> {
        let mut cues: Vec<NavigationCue> = Vec::new();
        let facility_route = self.is_facility_approach_route();
        let mainline_cities: Vec<String> = self
            .route
            .cities
            .iter()
            .map(|c| self.world.spoken_city(c, Some(false)))
            .collect();
        for (i, (start, leg)) in self
            .leg_starts
            .iter()
            .zip(self.route.legs.iter())
            .enumerate()
        {
            let start = *start;
            let forward = self.route.cities[i] == leg.a;
            let toward_key = &self.route.cities[i + 1];
            let toward = self.world.spoken_city(toward_key, None);
            // A relayed pickup is a corridor joined to the shipper's street
            // chain, so the chain's streets are city-to-same-city legs inside
            // a highway route. Read as highway changes they were each called
            // "Keep right for ... toward Indianapolis", left turns included
            // (owner drive to Indianapolis Dry Warehouse, 2026-10-10).
            let street_leg = i > 0 && leg.a == leg.b && self.route.cities[i] == *toward_key;
            if facility_route || street_leg {
                // Tier-1 surface segments carry their baked maneuver; speak
                // it verbatim with the segment distance.
                if street_leg && self.route.legs[i - 1].a != self.route.legs[i - 1].b {
                    // Off the corridor onto the chain: its "Start on" is
                    // where a chain driven on its own begins, not a corner.
                    let text = format!("Continue onto {}", leg.highway);
                    cues.push(
                        NavigationCue::new(
                            &format!("local:join:{i}"),
                            "local_turn",
                            start,
                            &lower_first(&text),
                            &format!("{text}{}.", self.surface_distance_tail(leg.miles)),
                        )
                        .with_direction("ahead"),
                    );
                } else if i == 0 {
                    let raw = if leg.local_cue.is_empty() {
                        format!("Start on {}.", leg.highway)
                    } else {
                        leg.local_cue.clone()
                    };
                    let text = raw.trim_end_matches('.').to_string();
                    let direction = cue_direction(&text);
                    cues.push(
                        NavigationCue::new(
                            "local:start",
                            "local_turn",
                            start + 0.05,
                            &lower_first(&text),
                            &format!("{text}{}.", self.surface_distance_tail(leg.miles)),
                        )
                        .with_direction(if direction.is_empty() {
                            "ahead"
                        } else {
                            direction
                        }),
                    );
                } else if self.route.legs[i - 1].highway != leg.highway {
                    let raw = if leg.local_cue.is_empty() {
                        format!("Turn onto {}.", leg.highway)
                    } else {
                        leg.local_cue.clone()
                    };
                    let text = raw.trim_end_matches('.').to_string();
                    cues.push(
                        NavigationCue::new(
                            &format!("local:turn:{i}"),
                            "local_turn",
                            start,
                            &lower_first(&text),
                            &format!("{text}{}.", self.surface_distance_tail(leg.miles)),
                        )
                        .with_direction(cue_direction(&text)),
                    );
                }
                continue;
            }
            let heading = leg_heading(self.world, &leg.highway, &self.route.cities[i], toward_key);
            let shield = format!("{} {heading}", leg.highway).trim().to_string();
            let segment_miles = leg.miles;
            if i == 0 {
                cues.push(NavigationCue::new(
                    "onramp:0",
                    "onramp",
                    start + 0.05,
                    &format!("merge onto {shield} toward {toward}"),
                    &format!(
                        "Merge onto {shield} toward {toward}{}.",
                        on_it_tail(&self.distance_text(segment_miles))
                    ),
                ));
            } else if segment_miles >= 40.0 {
                cues.push(NavigationCue::new(
                    &format!("continue:{i}"),
                    "continue",
                    start + 0.1,
                    &format!(
                        "Continue on {} for {} toward {toward}.",
                        leg.highway,
                        self.distance_text(segment_miles)
                    ),
                    "",
                ));
            }
            if i > 0 && self.route.legs[i - 1].highway != leg.highway {
                cues.push(NavigationCue::new(
                    &format!("maneuver:{i}"),
                    "maneuver",
                    start,
                    &format!("keep right for {shield} toward {toward}"),
                    &format!("Keep right for {shield} toward {toward}."),
                ));
            }
            for crossing in leg.state_crossings() {
                let offset = stop_offset_for_direction(crossing.at_mi, leg.miles, forward);
                let (into_state, from_state) = if forward {
                    (&crossing.state, &crossing.from_state)
                } else {
                    (&crossing.from_state, &crossing.state)
                };
                let place = &crossing.place;
                cues.push(NavigationCue::new(
                    &format!("state:{i}:{}:{into_state}", py_str_float(crossing.at_mi)),
                    "state_crossing",
                    start + offset,
                    &format!("crossing from {from_state} into {into_state} near {place}"),
                    &format!("Crossing into {into_state} near {place}."),
                ));
            }
            for checkpoint in leg.checkpoints() {
                let offset = stop_offset_for_direction(checkpoint.at_mi, leg.miles, forward);
                let place = &checkpoint.name;
                let state = if checkpoint.state.is_empty() {
                    String::new()
                } else {
                    format!(", {}", checkpoint.state)
                };
                let highway = if checkpoint.highway.is_empty() {
                    &leg.highway
                } else {
                    &checkpoint.highway
                };
                cues.push(NavigationCue::new(
                    &format!("checkpoint:{i}:{}:{place}", py_str_float(checkpoint.at_mi)),
                    "checkpoint",
                    start + offset,
                    &format!("{place}{state} on {highway}"),
                    &format!("Passing {place}{state} on {highway}."),
                ));
            }
            for toll in leg.toll_events() {
                let offset = stop_offset_for_direction(toll.at_mi, leg.miles, forward);
                // A sentence of its own after the point's name, so it starts
                // like one: "...ticket entry. Estimated toll 18 dollars..."
                // (agent playtest, 2026-09-02).
                let toll_text = if toll.amount > 0.0 {
                    let estimate = if toll.estimated { "Estimated " } else { "" };
                    format!(
                        "{estimate}{}oll {} dollars, billed to carrier settlement.",
                        if toll.estimated { "t" } else { "T" },
                        fmt_f(toll.amount, 0)
                    )
                } else {
                    "Entry recorded for carrier settlement.".to_string()
                };
                cues.push(NavigationCue::new(
                    &format!("toll:{i}:{}:{}", py_str_float(toll.at_mi), toll.name),
                    "toll",
                    start + offset,
                    &format!("toll road ahead: {}", toll.road),
                    &format!(
                        "{} toll point ahead, {}. {toll_text}",
                        toll.method_label(),
                        toll.name
                    ),
                ));
            }
            for restriction in leg.restrictions() {
                let offset = stop_offset_for_direction(restriction.at_mi, leg.miles, forward);
                cues.push(NavigationCue::new(
                    &format!(
                        "restriction:{i}:{}:{}",
                        py_str_float(restriction.at_mi),
                        restriction.kind
                    ),
                    "restriction",
                    start + offset,
                    &restriction.spoken_ahead(),
                    &restriction.spoken_near(),
                ));
            }
            for ix in leg.interchanges() {
                // Destinations the leg's other exits sign too are the
                // mainline's own promise, not this exit's: on I-35 south the
                // merge put "Dallas" on exit 31B while three other exits
                // signed it for the road the driver is already on.
                let siblings: Vec<String> = leg
                    .interchanges()
                    .iter()
                    .filter(|other| other.at_mi != ix.at_mi)
                    .flat_map(|other| other.destinations.iter().cloned())
                    .collect();
                let offset = stop_offset_for_direction(ix.at_mi, leg.miles, forward);
                cues.push(NavigationCue::new(
                    &format!("interchange:{i}:{}:{}", py_str_float(ix.at_mi), ix.exit_ref),
                    "interchange",
                    start + offset,
                    &ix.spoken_phrase_on(&leg.highway, &mainline_cities, &siblings),
                    &ix.near_phrase_on(&leg.highway, &mainline_cities, &siblings),
                ));
            }
            for stop in &leg.stops {
                if !self.stop_is_real(stop, forward) {
                    continue;
                }
                let offset = stop_offset_for_direction(stop.at_mi, leg.miles, forward);
                let exit_label = stop_exit_label(leg, stop, 2.0);
                let at_part = if exit_label.is_empty() {
                    String::new()
                } else {
                    format!(" at {exit_label}")
                };
                cues.push(NavigationCue::new(
                    &format!("rest_stop:{i}:{}:{}", py_str_float(stop.at_mi), stop.name),
                    "rest_stop",
                    start + offset,
                    &format!("{} ahead{at_part}", stop.label()),
                    "",
                ));
            }
        }
        cues.sort_by(|a, b| a.at_mi.partial_cmp(&b.at_mi).expect("finite mileposts"));
        cues
    }

    /// Schedule the baked roadside landmarks along this route,
    /// direction-resolved and thinned to the minimum spacing. Villages are
    /// baked wide and displayed tight: only the ones the route actually runs
    /// through or skirts are scheduled.
    pub fn place_landmarks(&self) -> Vec<RoadsideCallout> {
        if self.is_facility_approach_route() {
            return Vec::new();
        }
        let mut callouts: Vec<RoadsideCallout> = Vec::new();
        let mut villages: Vec<(f64, f64, RoadsideCallout)> = Vec::new();
        for (i, (start, leg)) in self
            .leg_starts
            .iter()
            .zip(self.route.legs.iter())
            .enumerate()
        {
            let forward = self.route.cities[i] == leg.a;
            for landmark in leg.landmarks() {
                if !landmark.applies_to_direction(forward) {
                    continue;
                }
                let offset = stop_offset_for_direction(landmark.at_mi, leg.miles, forward);
                let at_mi = start + offset;
                if landmark.category == "billboard_sign"
                    && self.commercial_billboards_banned_at(at_mi)
                {
                    continue;
                }
                let mut callout = RoadsideCallout::new(
                    &format!(
                        "landmark:{i}:{}:{}",
                        py_str_float(landmark.at_mi),
                        landmark.name
                    ),
                    at_mi,
                    &landmark.category,
                    // Landmark text is a bare name ("Lake Texoma") or, for a
                    // billboard, whole sentences already ending in one.
                    &if landmark.spoken.ends_with(['.', '!', '?']) {
                        landmark.spoken.clone()
                    } else {
                        format!("{}.", landmark.spoken)
                    },
                );
                if landmark.category == "village" {
                    if landmark.off_mi > VILLAGE_PASS_OFF_MI {
                        continue;
                    }
                    if self.village_explains_drop(callout.at_mi) {
                        callout.explains_limit = true;
                    }
                    villages.push((callout.at_mi, landmark.off_mi, callout));
                    continue;
                }
                callouts.push(callout);
            }
        }
        // Town names are placed first and scenery fills the gaps around them.
        let mut spaced = Self::thin_villages(villages);
        callouts.sort_by(|a, b| a.at_mi.partial_cmp(&b.at_mi).expect("finite mileposts"));
        for callout in callouts {
            if callout.category != "billboard_sign"
                && spaced
                    .iter()
                    .any(|kept| (callout.at_mi - kept.at_mi).abs() < LANDMARK_MIN_SPACING_MI)
            {
                continue;
            }
            spaced.push(callout);
            spaced.sort_by(|a, b| a.at_mi.partial_cmp(&b.at_mi).expect("finite mileposts"));
        }
        spaced
    }

    /// Keep one village per spacing window, nearest the road winning. A
    /// village that explains a limit change is never thinned away.
    pub fn thin_villages(villages: Vec<(f64, f64, RoadsideCallout)>) -> Vec<RoadsideCallout> {
        let mut ordered = villages;
        ordered.sort_by(|a, b| {
            (a.1, a.0)
                .partial_cmp(&(b.1, b.0))
                .expect("finite mileposts")
        });
        let mut chosen: Vec<(f64, RoadsideCallout)> = Vec::new();
        for (at_mi, _off, callout) in &ordered {
            if callout.explains_limit {
                chosen.push((*at_mi, callout.clone()));
            }
        }
        for (at_mi, _off, callout) in &ordered {
            if callout.explains_limit {
                continue;
            }
            if chosen
                .iter()
                .any(|(taken, _)| (at_mi - taken).abs() < VILLAGE_MIN_SPACING_MI)
            {
                continue;
            }
            chosen.push((*at_mi, callout.clone()));
        }
        chosen.sort_by(|a, b| a.0.partial_cmp(&b.0).expect("finite mileposts"));
        chosen.into_iter().map(|(_, c)| c).collect()
    }

    /// Whether a town-scale limit takes effect just past this callout, or the
    /// name is inside the town's zone which ends just ahead. Probes the
    /// baked corridor limit only.
    pub fn village_explains_drop(&self, at_mi: f64) -> bool {
        let here = self.corridor_limit_at(at_mi);
        let mut mi = at_mi + LIMIT_SCAN_STRIDE_MI;
        let end = (at_mi + VILLAGE_PAIR_WINDOW_MI).min(self.total_miles());
        let inside_town_limit = here <= VILLAGE_PAIR_MAX_LIMIT_MPH;
        while mi <= end {
            let there = self.corridor_limit_at(mi);
            if there < here && there <= VILLAGE_PAIR_MAX_LIMIT_MPH {
                return true; // the town's zone starts just ahead of its name
            }
            if inside_town_limit && there > here {
                return true; // the name is inside the town's zone, which ends here
            }
            mi += LIMIT_SCAN_STRIDE_MI;
        }
        false
    }

    /// Schedule parody billboards along the highway, seeded per trip; each
    /// sign text appears at most once per trip.
    pub fn place_billboards(&self) -> Vec<RoadsideCallout> {
        if self.is_facility_approach_route() {
            return Vec::new();
        }
        let mut rng = match self.seed {
            None => PyRandom::new_unseeded(),
            Some(seed) => PyRandom::new_from_i64(seed ^ 0xB111_B0A2),
        };
        let mut callouts = Vec::new();
        let mut used: Vec<&'static str> = Vec::new();
        let mut at = BILLBOARD_LEAD_IN_MI + rng.uniform(0.0, BILLBOARD_MIN_GAP_MI);
        while at < self.total_miles() - 5.0 {
            // Scenic America / FHWA: ME, VT, AK, and HI opted out of commercial
            // billboards entirely. Keep the mile cadence but place nothing there
            // so I-95 Maine cannot carry a joke board.
            if self.commercial_billboards_banned_at(at) {
                at += rng.uniform(BILLBOARD_MIN_GAP_MI, BILLBOARD_MAX_GAP_MI);
                continue;
            }
            let (leg_i, _) = self.leg_at_mile(at);
            let pool = corridor_signs(&self.route.legs[leg_i].highway);
            let fresh_corridor: Vec<&'static str> = pool
                .iter()
                .chain(regional_genre_signs().iter())
                .filter(|sign| !used.contains(&sign.text) && self.sign_belongs_at(&sign.anchor, at))
                .map(|sign| sign.text)
                .collect();
            let text = if !fresh_corridor.is_empty() && rng.random() < 0.5 {
                *rng.choice(&fresh_corridor)
            } else {
                let mut text = random_billboard(&mut rng);
                for _ in 0..6 {
                    if !used.contains(&text) {
                        break;
                    }
                    text = random_billboard(&mut rng);
                }
                text
            };
            if !used.contains(&text) {
                used.push(text);
                callouts.push(RoadsideCallout::new(
                    &format!("billboard:{}", fmt_f(at, 1)),
                    at,
                    "billboard",
                    &format!("Billboard: {text}"),
                ));
            }
            at += rng.uniform(BILLBOARD_MIN_GAP_MI, BILLBOARD_MAX_GAP_MI);
        }
        callouts
    }

    /// Whether a corridor sign's copy is true at this trip milepost.
    ///
    /// This is the whole placement fix. A shield is not a place: Interstate 40
    /// runs through Oklahoma and Tennessee both, so drawing its pool anywhere
    /// on the shield read Okemah to a driver outside Knoxville. A billboard is
    /// one of the few things telling a driver who cannot see the road where
    /// they are, so a sign that names a place has to be refused everywhere the
    /// place is not, and the pool simply falls through to the anywhere signs.
    fn sign_belongs_at(&self, anchor: &SignAnchor, at: f64) -> bool {
        match anchor {
            SignAnchor::Corridor => true,
            SignAnchor::States(states) => match self.state_code_at(at) {
                // An unbaked state is not a licence to claim a place. The
                // roadside falls back to signs that are true anywhere.
                None => false,
                Some(code) => states.contains(&code.as_str()),
            },
            SignAnchor::Approaching { cities, within_mi } => {
                // Measured ALONG THE ROUTE, not as the crow flies: a billboard
                // is read from a road, on the way to the thing it advertises.
                // A city the route never reaches is never "ahead", which is
                // why an off-route anchor stays silent instead of guessing.
                self.route
                    .cities
                    .iter()
                    .enumerate()
                    .filter(|(_, slug)| cities.contains(&slug.as_str()))
                    .filter_map(|(i, _)| self.city_mileposts.get(i))
                    .any(|milepost| {
                        let ahead = milepost - at;
                        ahead > 0.0 && ahead <= *within_mi
                    })
            }
        }
    }

    /// Maine, Vermont, Alaska, and Hawaii ban commercial billboards (Scenic
    /// America / FHWA). Scenic-byway spans are stored in each leg's own
    /// direction and are mirrored for reverse travel.
    fn commercial_billboards_banned_at(&self, at: f64) -> bool {
        if matches!(
            self.state_code_at(at).as_deref(),
            Some("ME") | Some("VT") | Some("AK") | Some("HI")
        ) {
            return true;
        }
        let (leg_index, leg_start) = self.leg_at_mile(at);
        let leg = &self.route.legs[leg_index];
        let local_mi = at - leg_start;
        let local_mi = if self.route.cities[leg_index] == leg.a {
            local_mi
        } else {
            leg.miles - local_mi
        };
        leg.billboard_bans()
            .iter()
            .any(|ban| ban.from_mi <= local_mi && local_mi < ban.to_mi)
    }

    /// The two-letter state code at a trip milepost, or None where the bake is
    /// silent and the route names no city we can fall back on.
    pub(crate) fn state_code_at(&self, at: f64) -> Option<String> {
        let name = self.state_at(Some(at));
        if !name.is_empty() {
            if let Some(code) = self.state_codes.get(&name) {
                return Some(code.clone());
            }
            if name.len() == 2 {
                return Some(name.to_uppercase());
            }
        }
        // Fallback for a leg with no state bake: the nearer endpoint city.
        // World keys carry their state ("memphis_tn_us"), which is the same
        // fact the bake would have given, from the other end.
        let (leg_i, leg_start) = self.leg_at_mile(at);
        let leg = &self.route.legs[leg_i];
        let nearer = if at - leg_start < leg.miles / 2.0 {
            leg_i
        } else {
            leg_i + 1
        };
        let slug = self.route.cities.get(nearer)?;
        let mut parts = slug.rsplitn(3, '_');
        parts.next()?; // country
        let state = parts.next()?;
        (state.len() == 2 && state.chars().all(|c| c.is_ascii_alphabetic()))
            .then(|| state.to_uppercase())
    }

    // -- curves ---------------------------------------------------------------------

    /// Every baked curve on the route in trip-mile coordinates. Connector
    /// ramps stay in the list for curve physics but are filtered from the
    /// spoken layers.
    pub fn place_curves(&self) -> Vec<RouteCurve> {
        if self.is_facility_approach_route() {
            return Vec::new();
        }
        route_curves(&self.route, &self.route.cities, false)
    }

    /// The curve whose footprint contains this milepost, or None. Some baked
    /// curves may have start past end after direction resolution, so check
    /// both orderings.
    pub fn curve_at(&self, mile: f64) -> Option<RouteCurve> {
        self.curves
            .iter()
            .find(|cr| {
                let lo = cr.start_mi.min(cr.end_mi);
                let hi = cr.start_mi.max(cr.end_mi);
                lo <= mile && mile <= hi
            })
            .copied()
    }

    /// Distance to the next mainline curve start inside `lead_mi`, or None.
    pub fn curve_ahead_mi(&self, lead_mi: f64) -> Option<f64> {
        for cr in &self.curves {
            let ahead = cr.start_mi - self.position_mi;
            if ahead <= 0.0 {
                continue;
            }
            if ahead > lead_mi {
                break;
            }
            if cr.connector {
                continue;
            }
            return Some(ahead);
        }
        None
    }

    /// The nearest mainline bend inside `lead_mi`, with how far ahead its
    /// start is. Unlike `next_curve_approach` this applies no speed test: the
    /// steering guide leans for the SHAPE of the road whatever the truck is
    /// doing, where a spoken warning only fires for a bend taken too fast.
    pub fn next_curve_within(&self, lead_mi: f64) -> Option<(f64, RouteCurve)> {
        for cr in &self.curves {
            let ahead = cr.start_mi - self.position_mi;
            if ahead <= 0.0 {
                continue;
            }
            if ahead > lead_mi {
                break;
            }
            if cr.connector {
                continue;
            }
            return Some((ahead, *cr));
        }
        None
    }

    /// The fastest the truck takes this bend before it costs anything
    /// (`TruckState::curve_safe_mph`: the load, with the bank the bend's sign
    /// was priced with, and the lane, with the bank the lane model credits).
    pub fn bend_costs_above_mph(&self, cr: &RouteCurve) -> f64 {
        let radius_ft = (cr.min_radius_ft as f64).max(1.0);
        let design = self.leg_design_speed_mph();
        self.truck.curve_safe_mph(
            radius_ft,
            bend_bank(radius_ft, Some(design)),
            superelevation_at(radius_ft, design),
            self.lane_steers,
        )
    }

    /// The speed past which a bend is called, and the speed the call's lead
    /// is sized to shed to.
    ///
    /// The sign plus a margin, so the words stay quiet on bends an ordinary
    /// truck takes at road speed -- but never past where the bend starts
    /// costing THIS load. The margins predate the rollover model: a full
    /// trailer goes over 1.2 mph past a 15 mph sign and about 4 past a 65
    /// (`roll.rs`), so a sharp bend's 3 and a gentle one's 8 let it roll
    /// without a word, and a driver obeying every number the cab spoke went
    /// over on US-550 and the Salt River Canyon (bend sweep, 2026-09-24).
    pub fn curve_call_mph(&self, cr: &RouteCurve) -> (f64, f64) {
        let margin = if cr.severity() == "gentle" {
            PACENOTE_GENTLE_MARGIN_MPH
        } else {
            PACENOTE_MARGIN_MPH
        };
        let advisory = cr.advisory_mph as f64;
        let costs = self.bend_costs_above_mph(cr);
        ((advisory + margin).min(costs), advisory.min(costs))
    }

    /// The next curve ahead that deserves a spoken approach warning and has
    /// not had it.
    ///
    /// Not had it: a bend already called (or riding the last call's "then"
    /// tail) used to be returned here too, and `check_curves` stopped at it,
    /// so in a dense run every bend behind it waited until the truck was past
    /// it. On US-62's esses a 25 mph bend was never called at all: the
    /// called bends ahead of it masked it until the truck was in it (bend
    /// sweep, 2026-09-24).
    pub fn next_curve_approach(&self) -> Option<RouteCurve> {
        let speed = self.truck.speed_mph();
        for cr in &self.curves {
            let ahead = cr.start_mi - self.position_mi;
            if ahead <= 0.0 {
                continue;
            }
            if ahead > PACENOTE_MAX_LEAD_MI {
                break;
            }
            if cr.connector || self.curve_called(cr) {
                continue;
            }
            let (call_above, target) = self.curve_call_mph(cr);
            if speed <= call_above {
                continue;
            }
            if ahead > Self::curve_pacenote_lead_mi(speed, target) {
                continue;
            }
            return Some(*cr);
        }
        None
    }

    /// True while a warning-worthy bend is inside its reaction window
    /// (owner, 2026-07-24): from inside the pacenote lead, widened, until
    /// the curve's end the clock runs real.
    pub fn severe_curve_decompression(&self) -> bool {
        let speed = self.truck.speed_mph();
        for cr in &self.curves {
            if cr.end_mi < self.position_mi {
                continue;
            }
            let ahead = cr.start_mi - self.position_mi;
            if ahead > PACENOTE_MAX_LEAD_MI {
                break;
            }
            if cr.connector {
                continue;
            }
            let (call_above, target) = self.curve_call_mph(cr);
            if speed <= call_above {
                continue;
            }
            let window = Self::curve_pacenote_lead_mi(speed, target) * 1.5;
            if ahead <= window {
                return true;
            }
        }
        false
    }

    /// True while a signalled exit is inside the road the truck must shed,
    /// sized from the exit's own ramp speed (Shane, 2026-08-15).
    pub fn armed_exit_decompression(&self) -> bool {
        let Some(ahead) = self.exit_approach_mi else {
            return false;
        };
        if ahead <= 0.0 {
            return false;
        }
        let speed = self.truck.speed_mph();
        let ramp_mph = self.ramp_speed_at(self.position_mi + ahead);
        if speed <= ramp_mph {
            return false; // already slow enough for the gore: nothing to shed
        }
        // Keep the whole assist window on the real clock, while retaining
        // a longer physics-based shed for the exit's own advisory speed.
        let window = EXIT_SPEED_ASSIST_START_MI
            .max(approach_shed_mi(speed, ramp_mph) * EXIT_APPROACH_DECOMPRESS_SLACK);
        ahead <= window
    }

    pub fn curve_pacenote_lead_mi(speed_mph: f64, advisory_mph: f64) -> f64 {
        let over = (speed_mph - advisory_mph).max(0.0);
        let react_mi = speed_mph * PACENOTE_REACTION_S / 3600.0;
        let brake_s = over / PACENOTE_BRAKE_MPH_PER_S;
        let brake_mi = (speed_mph + advisory_mph) / 2.0 * brake_s / 3600.0;
        let floor_mi = PACENOTE_MIN_LEAD_MI.max(speed_mph * PACENOTE_LEAD_FLOOR_S / 3600.0);
        PACENOTE_MAX_LEAD_MI.min(floor_mi.max(react_mi + brake_mi))
    }

    /// Whether this bend's approach call has gone out (or it is behind the
    /// truck): the point from which the clock runs real for it.
    pub fn curve_called(&self, curve: &RouteCurve) -> bool {
        let key = format!("curve:{}:{}", fmt_f(curve.start_mi, 3), curve.direction);
        self.announced_curves.contains(&key)
    }

    /// Emit a CURVE event when approaching a meaningful curve.
    pub fn check_curves(&mut self) {
        if self.is_facility_approach_route() {
            return;
        }
        let Some(cr) = self.next_curve_approach() else {
            return;
        };
        let ahead = cr.start_mi - self.position_mi;
        let key = format!("curve:{}:{}", fmt_f(cr.start_mi, 3), cr.direction);
        self.announced_curves.insert(key);
        // The immediate follower rides this call's "then ..." tail.
        let linked = self.curves.iter().find(|c| {
            !c.connector && c.start_mi > cr.end_mi && c.start_mi <= cr.end_mi + PACENOTE_LINK_GAP_MI
        });
        if let Some(linked) = linked {
            self.announced_curves.insert(format!(
                "curve:{}:{}",
                fmt_f(linked.start_mi, 3),
                linked.direction
            ));
        }
        let direction = if cr.direction == 'L' { "left" } else { "right" };
        let prefix = if cr.severity() == "hairpin" || cr.severity() == "sharp" {
            "sharp "
        } else {
            ""
        };
        let distance = self.ahead_text(ahead);
        let message = format!(
            "{prefix}curve {direction}, {distance}, advisory {}.",
            cr.advisory_mph
        );
        self.emit(
            TripEventKind::Curve,
            SpokenMessage::new(message),
            TripEventData {
                curve: Some(cr),
                advisory_mph: Some(cr.advisory_mph as f64),
                ahead_mi: Some(ahead),
                ..Default::default()
            },
        );
    }

    /// Announce when the lane count in the travel direction changes, once
    /// per boundary. Divided-only changes stay quiet.
    pub fn check_lane_changes(&mut self) {
        if self.lane_runs.is_none() {
            let runs = self.build_lane_runs();
            // Seed everything already behind the starting position so a
            // resumed trip does not re-announce a change it passed.
            for run in &runs {
                if run.start_mi <= self.position_mi {
                    self.announced_lane_changes
                        .insert(format!("lane:{}", fmt_f(run.start_mi, 2)));
                }
            }
            self.lane_runs = Some(runs);
        }
        let runs = self.lane_runs.clone().unwrap_or_default();
        for idx in 1..runs.len() {
            let boundary = runs[idx].start_mi;
            let key = format!("lane:{}", fmt_f(boundary, 2));
            if self.announced_lane_changes.contains(&key) {
                continue;
            }
            let behind = self.position_mi - boundary;
            if behind < 0.0 {
                break; // sorted; nothing further along is due yet
            }
            self.announced_lane_changes.insert(key);
            let prev_side = runs[idx - 1].lanes;
            let new_side = runs[idx].lanes;
            if new_side == prev_side {
                continue;
            }
            if behind <= 1.0 {
                // not a stale, overshot boundary from a jump/resume
                self.emit(
                    TripEventKind::Lane,
                    SpokenMessage::new(Self::lane_change_message(prev_side, new_side)),
                    TripEventData {
                        lanes: Some(new_side),
                        ..Default::default()
                    },
                );
            }
        }
    }

    pub fn check_roadside_callouts(&mut self) {
        self.check_callout_list(TripEventKind::Landmark);
        self.check_callout_list(TripEventKind::Billboard);
    }

    fn check_callout_list(&mut self, kind: TripEventKind) {
        let billboards = kind == TripEventKind::Billboard;
        let (callouts, mut announced) = if billboards {
            (
                std::mem::take(&mut self.billboards),
                std::mem::take(&mut self.announced_billboards),
            )
        } else {
            (
                std::mem::take(&mut self.landmarks),
                std::mem::take(&mut self.announced_landmarks),
            )
        };
        for callout in &callouts {
            if announced.contains(&callout.key) {
                continue;
            }
            let behind = self.position_mi - callout.at_mi;
            if behind < 0.0 {
                break; // sorted by mile; nothing further along is due yet
            }
            // Past by more than a mile: stale scenery, consumed either way.
            if behind > 1.0 {
                announced.insert(callout.key.clone());
                continue;
            }
            // Flavor (billboards, rivers, the rest of the scenery) spends a
            // sitting budget like CB_CALLS_PER_RUN: skip extras rather than
            // let 20x multiply pokes. Villages and limit-explaining names
            // are places -- once per milepost, no budget.
            if !callout.is_place_callout() && !self.chatter_ready() {
                announced.insert(callout.key.clone());
                continue;
            }
            announced.insert(callout.key.clone());
            self.emit(
                kind,
                SpokenMessage::new(callout.spoken.clone()),
                TripEventData {
                    category: Some(callout.category.clone()),
                    explains_limit: Some(callout.explains_limit),
                    ..Default::default()
                },
            );
            if !callout.is_place_callout() {
                self.note_chatter_spoke();
            }
        }
        if billboards {
            self.billboards = callouts;
            self.announced_billboards = announced;
        } else {
            self.landmarks = callouts;
            self.announced_landmarks = announced;
        }
    }

    /// True when the sitting budget will still pay for a flavor poke.
    pub fn chatter_ready(&self) -> bool {
        match self.last_chatter_s {
            None => true,
            Some(last) => self.sitting_s - last >= CHATTER_GAP_REAL_S,
        }
    }

    pub fn note_chatter_spoke(&mut self) {
        self.last_chatter_s = Some(self.sitting_s);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn surface_tail_says_it_is_the_road_ahead_not_the_turn() {
        // Imperial buckets.
        assert_eq!(surface_tail_text(0.3, true), ", then a quarter mile on it");
        assert_eq!(surface_tail_text(0.5, true), ", then half a mile on it");
        assert_eq!(surface_tail_text(1.0, true), ", then 1 mile on it");
        assert_eq!(surface_tail_text(2.6, true), ", then 3 miles on it");
        // Metric buckets.
        assert_eq!(surface_tail_text(0.3, false), ", then 500 meters on it");
        assert_eq!(surface_tail_text(0.6, false), ", then 1 kilometer on it");
        assert_eq!(surface_tail_text(2.0, false), ", then 3 kilometers on it");
        // A short city block says nothing in either unit.
        assert_eq!(surface_tail_text(0.1, true), "");
        assert_eq!(surface_tail_text(0.19, false), "");
    }

    #[test]
    fn the_tail_reads_as_one_sentence_after_the_turn() {
        assert_eq!(
            format!(
                "Turn right onto South Columbus Drive{}.",
                surface_tail_text(0.6, false)
            ),
            "Turn right onto South Columbus Drive, then 1 kilometer on it."
        );
        assert_eq!(
            format!(
                "Merge onto I-90 East toward Gary{}.",
                on_it_tail("53 kilometers")
            ),
            "Merge onto I-90 East toward Gary, then 53 kilometers on it."
        );
    }
}
