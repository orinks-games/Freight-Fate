//! Recurring road-event checks for trip simulation (port of
//! `freight_fate/sim/trip_road_events.py`, the former `TripRoadEventMixin`):
//! traffic pressures, tolls, city passages, hazards and too-fast-for-
//! conditions incidents.

use crate::data::world_models::py_capitalize;
use crate::pyfmt::{fmt_f, py_str_float};
use crate::sim::hos::is_night;
use crate::sim::trip::Trip;
use crate::sim::trip_models::*;
use crate::sim::trip_route_helpers::stop_offset_for_direction;
use crate::sim::weather::WeatherKind;
use crate::speech_text::{
    hazard_call, in_lane_hazard_call_named, passing_hazard_call, toll_charged, SpokenMessage,
};

impl Trip {
    pub fn traffic_pressure_intensity(&self, mile: f64, kind: &str) -> f64 {
        let (leg_i, _) = self.leg_at_mile(mile);
        let leg = &self.route.legs[leg_i];
        let mut intensity = 0.18;
        match kind {
            "exit" => intensity += 0.16,
            "route_merge" => intensity += 0.20,
            "construction_merge" => intensity += 0.34,
            _ => {}
        }
        if self.near_city(mile) {
            intensity += 0.22;
        }
        if !leg.checkpoints().is_empty() {
            intensity += 0.12;
        }
        if self.rush_hour_traffic_bias(leg) != 0.0 {
            intensity += 0.14;
        }
        if is_night(self.local_start_hour()) {
            intensity -= 0.06;
        }
        let effects = self.weather.effects();
        if effects.grip < 0.9 {
            intensity += (0.9 - effects.grip) * 0.35;
        }
        if effects.visibility_mi < 3.0 {
            intensity += (3.0 - effects.visibility_mi) * 0.04;
        }
        (intensity * self.hazard_scale).clamp(0.0, 0.95)
    }

    pub fn traffic_pressure_speed(&self, mile: f64, intensity: f64) -> f64 {
        let posted = self.corridor_limit_at(mile);
        30.0_f64.max(posted.min(posted - intensity * 26.0))
    }

    pub fn place_traffic_pressures(&self) -> Vec<TrafficPressure> {
        if self.is_facility_approach_route() {
            // Merge/exit spacing pressure is highway language; city streets
            // get their pacing from per-street speed zones instead.
            return Vec::new();
        }
        let mut pressures: Vec<TrafficPressure> = Vec::new();
        let add = |pressures: &mut Vec<TrafficPressure>,
                   start: f64,
                   end: f64,
                   kind: &str,
                   direction: &str,
                   reason: String| {
            let start = start.max(0.0);
            let end = self.total_miles().min((start + 0.2).max(end));
            let intensity = self.traffic_pressure_intensity(start, kind);
            if intensity < TRAFFIC_PRESSURE_MIN_INTENSITY {
                return;
            }
            pressures.push(TrafficPressure {
                start_mi: start,
                end_mi: end,
                kind: kind.to_string(),
                direction: direction.to_string(),
                intensity,
                target_speed_mph: self.traffic_pressure_speed(start, intensity),
                reason,
            });
        };

        for stop in &self.stops {
            let label = if stop.exit_label.is_empty() {
                stop.spoken_name()
            } else {
                stop.exit_label.clone()
            };
            add(
                &mut pressures,
                stop.at_mi - 2.0,
                stop.at_mi + 0.4,
                "exit",
                "right",
                format!("exit traffic for {label}"),
            );
        }
        for i in 1..self.leg_starts.len() {
            let start = self.leg_starts[i];
            if self.route.legs[i - 1].highway != self.route.legs[i].highway {
                add(
                    &mut pressures,
                    start - 1.5,
                    start + 0.6,
                    "route_merge",
                    "right",
                    format!("traffic merging for {}", self.route.legs[i].highway),
                );
            }
        }
        for zone in &self.zones {
            if zone.reason == "construction merge" {
                add(
                    &mut pressures,
                    zone.start_mi,
                    zone.end_mi,
                    "construction_merge",
                    "left",
                    "construction taper traffic".to_string(),
                );
            }
        }
        pressures.sort_by(|a, b| {
            a.start_mi
                .partial_cmp(&b.start_mi)
                .expect("finite mileposts")
        });
        pressures
    }

    pub fn check_tolls(&mut self) {
        let legs = self.route.legs.clone();
        for (i, (start, leg)) in self.leg_starts.clone().iter().zip(legs.iter()).enumerate() {
            let forward = self.route.cities[i] == leg.a;
            for toll in leg.toll_events() {
                let offset = stop_offset_for_direction(toll.at_mi, leg.miles, forward);
                let at_mi = start + offset;
                let key = format!("{i}:{}:{}", py_str_float(toll.at_mi), toll.name);
                if self.position_mi < at_mi || self.charged_tolls.contains(&key) {
                    continue;
                }
                self.charged_tolls.insert(key);
                if toll.amount <= 0.0 {
                    self.emit(
                        TripEventKind::GpsCue,
                        SpokenMessage::new(format!(
                            "{} entry recorded at {}. Toll billed at carrier settlement.",
                            toll.method_label(),
                            toll.name
                        )),
                        TripEventData {
                            toll: Some(toll.clone()),
                            ..Default::default()
                        },
                    );
                    continue;
                }
                self.toll_charges.push(TollCharge {
                    event: toll.clone(),
                    amount: toll.amount,
                });
                self.emit(
                    TripEventKind::TollCharged,
                    toll_charged(
                        &toll.method_label(),
                        &toll.name,
                        &fmt_f(toll.amount, 0),
                        toll.estimated,
                    ),
                    TripEventData {
                        toll: Some(toll.clone()),
                        amount: Some(toll.amount),
                        ..Default::default()
                    },
                );
            }
        }
    }

    pub fn toll_expense(&self) -> f64 {
        // Fold from 0.0 rather than `sum()`: Rust's `Sum for f64` starts at
        // -0.0, so an empty list yields negative zero and `fmt_grouped`
        // renders it "-0" -- a screen reader then says "minus zero
        // dollars". Python's `sum([])` is a plain 0.
        self.toll_charges
            .iter()
            .map(|charge| charge.amount)
            .fold(0.0, |total, amount| total + amount)
    }

    pub fn check_cities(&mut self) {
        let starts = self.leg_starts.clone();
        for (i, start) in starts.iter().enumerate() {
            if i == 0 || self.announced_cities.contains(&i) {
                continue;
            }
            if self.route.cities[i] == self.route.cities[i - 1] {
                // A same-city boundary is a surface-street segment change,
                // not a city passage; the turn cue already covers it.
                self.announced_cities.insert(i);
                continue;
            }
            if self.position_mi >= *start {
                self.announced_cities.insert(i);
                let prev = self.route.cities[i - 1].clone();
                let city = self.route.cities[i].clone();
                let nxt = self.route.cities[i + 1].clone();
                let highway = self.route.legs[i].highway.clone();
                let world = self.world;
                let city_state = world
                    .cities
                    .get(&city)
                    .unwrap_or_else(|| panic!("KeyError: {city:?}"))
                    .state
                    .clone();
                let prev_state = world
                    .cities
                    .get(&prev)
                    .unwrap_or_else(|| panic!("KeyError: {prev:?}"))
                    .state
                    .clone();
                // The mapped boundary is authoritative when the route has
                // one: it is announced at the surveyed mile, so repeating it
                // here would say the same state line twice. Ambient lines
                // ride a queue now, so the prefix goes back to being the
                // duplicate it always was.
                let crossing = if city_state != prev_state
                    && !self.leg_maps_crossing_into(i - 1, &city_state)
                {
                    format!("Crossing into {city_state}. ")
                } else {
                    String::new()
                };
                // Off a corridor onto a relayed pickup's street chain the
                // truck has arrived, not passed through: the turn cue names
                // the street, and "toward" this same city said nothing.
                if city == nxt {
                    if !crossing.is_empty() {
                        self.emit(
                            TripEventKind::CityReached,
                            SpokenMessage::new(crossing.trim_end().to_string()),
                            TripEventData::default(),
                        );
                    }
                    continue;
                }
                let message = format!(
                    "{crossing}Passing {}, {city_state}. Continuing on {highway} toward {}.",
                    world.spoken_city(&city, Some(false)),
                    world.spoken_city(&nxt, None)
                );
                self.emit(
                    TripEventKind::CityReached,
                    SpokenMessage::new(message),
                    TripEventData::default(),
                );
            }
        }
    }

    /// Whether this leg carries a surveyed crossing into `state`, read the
    /// same way `build_navigation_cues` reads it, direction and all.
    pub fn leg_maps_crossing_into(&self, leg_index: usize, state: &str) -> bool {
        let Some(leg) = self.route.legs.get(leg_index) else {
            return false;
        };
        let forward = self.route.cities[leg_index] == leg.a;
        leg.state_crossings().iter().any(|crossing| {
            let into_state = if forward {
                &crossing.state
            } else {
                &crossing.from_state
            };
            into_state == state
        })
    }

    /// Chance of a hazard at each check; worse in fog and after dark.
    pub fn hazard_risk(&self) -> f64 {
        let vis = self.weather.effects().visibility_mi;
        let mut risk = 0.25 + if vis < 2.0 { 0.25 } else { 0.0 };
        if is_night(self.local_hour()) {
            risk += NIGHT_HAZARD_BONUS;
        }
        risk * self.hazard_scale
    }

    /// Relative hazard-check frequency for the current corridor.
    pub fn corridor_hazard_factor_at(&self, mile: f64) -> f64 {
        let (leg_i, _) = self.leg_at_mile(mile);
        let leg = &self.route.legs[leg_i];
        let cls = highway_class(&leg.highway);
        let mut factor = match cls {
            "interstate" => 1.05,
            "us_highway" => 0.92,
            _ => 0.82,
        };
        factor += 0.18_f64.min(leg.checkpoints().len() as f64 * 0.06);
        let region = self.region_at(mile);
        if HOT_PATROL_REGIONS.contains(&region.as_str()) {
            factor += 0.12;
        } else if COLD_PATROL_REGIONS.contains(&region.as_str()) {
            factor -= 0.12;
        }
        if self.near_city(mile) {
            factor += 0.18;
        }
        factor.clamp(CORRIDOR_HAZARD_MIN_FACTOR, CORRIDOR_HAZARD_MAX_FACTOR)
    }

    pub fn next_hazard_check_interval_mi(&mut self) -> f64 {
        let base = self.rng.uniform(20.0, 60.0);
        base / self.corridor_hazard_factor_at(self.position_mi)
    }

    /// Occasional road hazards that demand braking.
    pub fn check_hazards(&mut self, moved_mi: f64) {
        if self.is_facility_approach_route() {
            // A deadhead crawl down a facility access road is minutes long
            // at yard speeds; a "brake now" ambush there is noise.
            return;
        }
        if let Some(context) = self.traffic_context() {
            if context.closing_mph > 8.0
                && context.gap_seconds() <= TRAFFIC_WARNING_GAP_S
                && self.position_mi >= self.traffic_warning_mi
            {
                self.traffic_warning_mi = self.position_mi + 8.0;
                // A lead vehicle blocks one lane: braking always works, and a
                // clear neighboring lane lets the player pass around it --
                // where there IS one. On a road with one lane your side there
                // is no going around, and calling the hazard dodgeable there
                // handed the driver a lane-change allowance for a move they
                // could not make, so the assist waited while the truck kept
                // closing (owner, 2026-08-24). Which neighbour is open is
                // read the way a lane change is refused -- count, cones,
                // and the traffic holding it -- and the call names it, so
                // one tap is the whole answer (owner, 2026-09-01).
                let side = self.open_side_at(None);
                let dodgeable = side.is_open();
                let reason = context
                    .lead
                    .reason()
                    .strip_suffix(" ahead")
                    .unwrap_or(context.lead.reason())
                    .to_string();
                let where_ = if context.gap_mi < 0.1 {
                    "right ahead".to_string()
                } else {
                    format!("{} ahead", self.gap_text(context.gap_mi))
                };
                // "Or change lanes" is only true advice where there is
                // somewhere to send it (playtest report, US-285, 2026-08-12).
                let body = format!("{} {where_}.", py_capitalize(&reason));
                let message = in_lane_hazard_call_named(&body, side, &self.open_side_spoken(side));
                self.emit(
                    TripEventKind::Hazard,
                    message,
                    TripEventData {
                        deadline_s: Some(2.5),
                        traffic: Some(context),
                        dodgeable: Some(dodgeable),
                        // A vehicle ahead is in your lane and no other, so
                        // brake-alone still has to reach ITS speed rather
                        // than the road-spanning safe speed.
                        in_lane: Some(true),
                        open_side: Some(side),
                        pass_message: passing_hazard_call(&body, side),
                        name: Some(format!("the {reason}")),
                        ..Default::default()
                    },
                );
                return;
            }
        }
        self.hazard_check_mi -= moved_mi;
        if self.hazard_check_mi > 0.0 {
            return;
        }
        self.hazard_check_mi = self.next_hazard_check_interval_mi();
        if self.rng.random() < self.hazard_risk() {
            let choices = eligible_hazards(
                self.current_region(),
                self.weather.current,
                &self.terrain_at(Some(self.position_mi)),
                self.local_hour(),
            );
            if choices.is_empty() {
                return;
            }
            let weights: Vec<f64> = choices.iter().map(|(_, w)| *w).collect();
            let idx = self.rng.choices_indices_weighted(&weights, 1)[0];
            let hazard = choices[idx].0;
            // The same two questions, in the same order, for every hazard:
            // is the thing confined to one lane, and does the road give you
            // another one. Only both together mean a lane change answers it.
            // "Brake now!" is reserved for a hazard no lane change could ever
            // answer -- fog, ice, a crosswind, a deer that may bolt either
            // way -- so it never loses its meaning to a road that merely
            // happens to be narrow here.
            let in_lane = hazard_is_in_lane(hazard);
            let side = self.open_side_at(None);
            let dodgeable = in_lane && side.is_open();
            let mut chars = hazard.chars();
            let first: String = chars
                .next()
                .map(|c| c.to_uppercase().collect())
                .unwrap_or_default();
            let body = format!("{first}{}.", chars.as_str());
            let message = if in_lane {
                in_lane_hazard_call_named(&body, side, &self.open_side_spoken(side))
            } else {
                hazard_call("Brake now!", &body)
            };
            let deadline_s = self.rng.uniform(3.0, 4.5) * self.visibility_reaction_factor();
            self.emit(
                TripEventKind::Hazard,
                message,
                TripEventData {
                    deadline_s: Some(deadline_s),
                    dodgeable: Some(dodgeable),
                    in_lane: Some(in_lane),
                    name: Some(hazard_name(hazard).to_string()),
                    ..Default::default()
                },
            );
        }
    }

    /// Low visibility shortens the normal hazard reaction slack.
    pub fn visibility_reaction_factor(&self) -> f64 {
        let vis = self.weather.effects().visibility_mi;
        if vis >= 3.0 {
            return 1.0;
        }
        0.4_f64.max(vis / 3.0)
    }

    /// The traction-loss phrase for the current conditions.
    pub fn conditions_incident_text(&self) -> &'static str {
        let kind = self.weather.current;
        if self.truck.hydroplaning() {
            return "Hydroplaning.";
        }
        match kind {
            WeatherKind::Ice => "Trailer sliding on the ice.",
            WeatherKind::Snow => "Trailer sliding on the snow.",
            WeatherKind::Rain | WeatherKind::HeavyRain | WeatherKind::Thunderstorm => {
                "Hydroplaning on the wet road."
            }
            _ => "Losing traction.",
        }
    }

    /// The short noun phrase a resolution line names this incident by.
    pub fn conditions_incident_name(&self) -> &'static str {
        let kind = self.weather.current;
        if self.truck.hydroplaning() {
            return "the hydroplaning";
        }
        match kind {
            WeatherKind::Ice => "the ice",
            WeatherKind::Snow => "the snow",
            WeatherKind::Rain | WeatherKind::HeavyRain | WeatherKind::Thunderstorm => {
                "the hydroplaning"
            }
            _ => "the loss of traction",
        }
    }

    /// Risk a traction-loss incident when driving too fast for slick roads.
    /// The truck's own grip decides, and actually hydroplaning counts as deep
    /// overspeed no matter what the sign says.
    pub fn check_conditions_speed(&mut self, moved_mi: f64) {
        let eff = self.weather.effects();
        let grip = self.truck.effective_grip();
        let over = self.truck.speed_mph() - eff.safe_speed_mph;
        let planing = self.truck.hydroplaning();
        if (over <= CONDITIONS_SPEED_MARGIN_MPH && !planing) || grip >= CONDITIONS_GRIP_CEILING {
            self.conditions_check_mi = CONDITIONS_CHECK_MI;
            return;
        }
        self.conditions_check_mi -= moved_mi;
        if self.conditions_check_mi > 0.0 {
            return;
        }
        self.conditions_check_mi = CONDITIONS_CHECK_MI;
        let mut severity = ((over - CONDITIONS_SPEED_MARGIN_MPH).max(0.0) / 25.0).min(1.0);
        if planing {
            severity = severity.max(0.7);
        }
        let risk = severity * (1.0 - grip) * CONDITIONS_INCIDENT_RISK * self.hazard_scale;
        if self.cond_rng.random() < risk {
            let message = hazard_call("Brake now!", self.conditions_incident_text());
            let deadline_s = 1.5_f64.max(2.5 * self.visibility_reaction_factor());
            let name = self.conditions_incident_name().to_string();
            self.emit(
                TripEventKind::Hazard,
                message,
                TripEventData {
                    deadline_s: Some(deadline_s),
                    name: Some(name),
                    ..Default::default()
                },
            );
        }
    }
}
