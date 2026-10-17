//! The per-frame tick and what it announces beyond the limits: resume
//! seeding, the weather and clock advance, stops, navigation cues, traffic
//! pressures, CB heads-ups and inspections (the update section of `trip.py`;
//! zone and posted-limit lines are in `limits.rs`).

use crate::pyfmt::{fmt_f, round_py_int};
use crate::sim::enforcement_posts::{KIND_FIXED_SCALE, KIND_SCALE_APRON};
use crate::sim::traffic_manager::BrakingZone;
use crate::sim::trip_models::*;
use crate::sim::trip_route_helpers::{stop_offset_for_direction, zone_key};
use crate::speech_text::{stop_callout, terse_silent, SpokenMessage, StopCalloutParts};

use super::{
    Trip, EXIT_APPROACH_RELEASE_S, LOCAL_TURN_LOOKAHEAD_MI, NAV_LEAD_MIN_MI,
    STOP_AHEAD_LOOKAHEAD_MI,
};

impl Trip {
    /// Jump to a saved point without re-announcing what is behind it.
    pub fn restore(&mut self, position_mi: f64, game_minutes: f64) {
        self.position_mi = position_mi.clamp(0.0, self.total_miles().max(0.0));
        self.game_minutes = game_minutes;
        // Seed the spoken limit at the resume point so it is not re-announced.
        self.announced_speed_limit = Some(self.corridor_limit_at(self.position_mi));
        for stop in &self.stops {
            // Passed stops AND stops already inside the "stop ahead" window
            // were announced before the save.
            if stop.at_mi <= self.position_mi + STOP_AHEAD_LOOKAHEAD_MI {
                self.announced_stops.insert(stop.key());
            }
        }
        for cue in &self.navigation_cues {
            if cue.at_mi <= self.position_mi {
                self.announced_navigation
                    .insert(format!("{}:advance", cue.key));
                self.announced_navigation
                    .insert(format!("{}:near", cue.key));
            }
        }
        for callout in self.landmarks.iter().chain(self.billboards.iter()) {
            if callout.at_mi <= self.position_mi {
                if callout.category == "billboard" {
                    self.announced_billboards.insert(callout.key.clone());
                } else {
                    self.announced_landmarks.insert(callout.key.clone());
                }
            }
        }
        self.latch_passed_roadside();
        for (i, (start, leg)) in self
            .leg_starts
            .iter()
            .zip(self.route.legs.iter())
            .enumerate()
        {
            let forward = self.route.cities[i] == leg.a;
            for toll in leg.toll_events() {
                let offset = stop_offset_for_direction(toll.at_mi, leg.miles, forward);
                if start + offset <= self.position_mi {
                    self.charged_tolls.insert(format!(
                        "{i}:{}:{}",
                        crate::pyfmt::py_str_float(toll.at_mi),
                        toll.name
                    ));
                }
            }
        }
        for stop in &self.stops {
            if stop.at_mi <= self.position_mi && stop.stop_type == "weigh_station" {
                self.announced_enforcement.insert(format!(
                    "weigh:{}:{}",
                    stop.name,
                    fmt_f(stop.at_mi, 1)
                ));
            }
        }
        for (i, start) in self.leg_starts.iter().enumerate() {
            if i != 0 && self.position_mi >= *start {
                self.announced_cities.insert(i);
            }
        }
    }

    /// Latch the per-mile markers the restore path and the staged-drive
    /// settle path both owe the road already under the truck: passed curves
    /// count as called, posts inside their watch were heard, traffic
    /// pressures behind are old news, and the zone/timezone under the truck
    /// is "entered" so the first frame does not announce it as new.
    pub(crate) fn latch_passed_roadside(&mut self) {
        // Only curves already passed are certainly history.
        for cr in &self.curves {
            if cr.start_mi <= self.position_mi {
                self.announced_curves.insert(format!(
                    "curve:{}:{}",
                    fmt_f(cr.start_mi, 3),
                    cr.direction
                ));
            }
        }
        let pos = self.position_mi;
        for post in self.posts.iter_mut() {
            // A post whose watch the truck is inside was heard before the
            // handoff; one still ahead must get its cue, and one wholly
            // behind has nothing left to watch -- `announced` stays "this
            // post made a noise", it must not say a finished post spoke.
            if post.watch_start_mi() <= pos && pos <= post.at_mi + post.reach_mi {
                post.announced = true;
                self.heads_up_seen.insert(post.id());
            }
        }
        for pressure in &self.traffic_pressures {
            if pressure.start_mi <= self.position_mi {
                self.announced_traffic_pressures
                    .insert(traffic_pressure_key(pressure));
            }
        }
        self.entered_zone = self.active_zone_at(pos);
        self.last_timezone = self.timezone_at(pos);
    }

    /// Restore settlement toll expenses from an active-drive snapshot: each
    /// entry is `{"name": ..., "amount": ...}`.
    pub fn restore_toll_charges(&mut self, charges: &[serde_json::Value]) {
        self.toll_charges = Vec::new();
        for raw in charges {
            let name = raw
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim()
                .to_string();
            let event = self
                .route
                .legs
                .iter()
                .flat_map(|leg| leg.toll_events().iter())
                .find(|toll| toll.name == name)
                .cloned();
            let Some(event) = event else {
                continue;
            };
            let amount = raw
                .get("amount")
                .and_then(|v| v.as_f64())
                .unwrap_or(event.amount);
            self.toll_charges.push(TollCharge { event, amount });
        }
    }

    /// Advance the trip by real seconds; returns events for the UI layer.
    pub fn update(&mut self, dt: f64) -> Vec<TripEvent> {
        self.events = Vec::new();
        if self.finished {
            return self.events.clone();
        }

        // What this run has burned, read as the DROP in the tank since the
        // last frame. The burn model is the truck's and runs outside this
        // call, so reading the level is the one place that sees every path;
        // and only a drop counts, so a fuel island adds gallons without ever
        // crediting the run with ones it did not spend.
        let fuel_now = self.truck.fuel_gal;
        if let Some(previous) = self.fuel_seen_gal {
            self.fuel_used_gal += (previous - fuel_now).max(0.0);
        }
        self.fuel_seen_gal = Some(fuel_now);

        // Any release path disarms waiting.
        if self.waiting && !self.truck.parking_brake {
            self.waiting = false;
        }

        // Arm or run down the approach's release tail before the scale is
        // read, so pacing eases back over real seconds.
        if self.armed_exit_decompression() {
            self.exit_approach_release_s = EXIT_APPROACH_RELEASE_S;
        } else {
            self.exit_approach_release_s = (self.exit_approach_release_s - dt).max(0.0);
        }
        // Night, fuel, ELD/HOS stay on drive time (game_min). Weather color
        // and the sitting budget chatter reads tick on real dt instead, so
        // 20x does not spawn 20x pokes.
        self.sitting_s += dt;
        let scale = self.effective_time_scale();
        let game_min = dt * scale / 60.0;
        self.game_minutes += game_min;
        let region = self.region_at(self.position_mi);
        self.weather.set_region(&region);
        if let Some((weather_key, lat, lon)) = self.weather_location() {
            let previous_weather_key = self.weather.city.clone();
            let location_changed = previous_weather_key.as_deref() != Some(weather_key.as_str());
            if location_changed && previous_weather_key.is_some() {
                self.weather_location_refreshing = true;
            }
            self.weather.set_city(&weather_key, lat, lon);
        }
        let changed = self.weather.update_paced(game_min, dt / 60.0);
        let source_status = self.weather.source_status();
        let imperial = self.imperial();
        if let Some(changed) = changed {
            let mut source_details = self.weather.source_conditions(imperial);
            if source_status == "last_known" {
                let notice = self.weather.last_known_notice();
                if !notice.is_empty() {
                    source_details += &format!(". {notice}");
                }
            }
            let label = self.weather.event_source_label();
            self.emit(
                TripEventKind::WeatherChange,
                SpokenMessage::new(format!("{label} changing: {source_details}")),
                TripEventData {
                    weather: Some(changed),
                    ..Default::default()
                },
            );
            if source_status == "live" || source_status == "fallback" {
                self.weather_location_refreshing = false;
            }
        } else {
            let refresh_failure_started = source_status == "last_known"
                && self.weather.live_weather_refresh_failed()
                && !self.weather_refresh_issue_announced;
            let source_changed = source_status != self.weather_source_status;
            if source_changed || refresh_failure_started {
                let suppress_location_refresh = self.weather_location_refreshing
                    && (source_status == "live" || source_status == "last_known");
                let suppress_routine_refresh = (source_status == "last_known"
                    && self.weather.live_weather_refreshing())
                    || (source_status == "live"
                        && self.weather_source_status == "last_known"
                        && !self.weather_refresh_issue_announced);
                if !suppress_location_refresh
                    && (source_status == "live"
                        || source_status == "last_known"
                        || source_status == "fallback")
                    && !suppress_routine_refresh
                {
                    let message = match source_status {
                        "live" => "Live weather ready.".to_string(),
                        "last_known" => {
                            let notice = self.weather.last_known_notice();
                            if notice.is_empty() {
                                "Last-known conditions in use.".to_string()
                            } else {
                                format!("{notice}. Last-known conditions in use.")
                            }
                        }
                        _ => "Live weather unavailable.".to_string(),
                    };
                    let current = self.weather.current;
                    self.emit(
                        TripEventKind::WeatherChange,
                        SpokenMessage::new(message),
                        TripEventData {
                            weather: Some(current),
                            ..Default::default()
                        },
                    );
                    self.weather_refresh_issue_announced =
                        source_status == "last_known" && self.weather.live_weather_refresh_failed();
                }
                if source_status == "live" || source_status == "fallback" {
                    self.weather_location_refreshing = false;
                    self.weather_refresh_issue_announced = false;
                }
            }
        }
        // A new route cell's first fetch reads as loading for a moment; while
        // simulated weather is already in use that is not news, and failing
        // back to it must not announce the fallback a second time.
        if !(source_status == "loading" && self.weather_source_status == "fallback") {
            self.weather_source_status = source_status;
        }
        let effects = self.weather.effects();
        self.truck.grip = effects.grip;
        self.truck.water_mm = effects.water_mm;
        self.truck.surface = effects.surface.to_string();
        self.truck.chain_control_road =
            self.chain_law_area_at(self.position_mi).is_some() && self.chain_law_level() > 0;
        self.truck.drag_mult = effects.drag_mult;
        self.truck.grade = match self.ramp_grade {
            Some(grade) if self.on_ramp => grade,
            _ => self.grade_at(self.position_mi),
        };
        self.truck.fuel_burn_mult = scale;

        let moved_mi = self.truck.velocity_mps * dt * scale / 1609.344;
        self.last_moved_mi = moved_mi;
        if self.on_ramp {
            // Off the highway on the exit ramp: hand this movement to the
            // ramp and pause highway events until the truck rejoins the road.
            return self.events.clone();
        }
        self.position_mi += moved_mi;
        if self.position_mi < 0.0 {
            self.position_mi = 0.0;
        } else if self.position_mi > self.total_miles() {
            self.position_mi = self.total_miles();
        }

        // effective_time_scale, not time_scale: the manager turns real
        // seconds into game hours, and that is exactly the conversion
        // effective_time_scale exists to own. Local hour, not the departure
        // hour: the density model is about the road the truck is on now.
        let time_scale = self.effective_time_scale();
        let hour = self.local_hour();
        let weekend = self.is_weekend_now();
        self.traffic_manager
            .sync_environment(self.truck.speed_mph(), effects);
        // Where traffic has a reason to be on the brakes: the congestion the
        // trip placed from real volumes, plus its approach, with the zone's
        // limit riding along as the pace braking traffic settles at
        // (Brandon, 2026-08-20).
        //
        // Handed over BEFORE the manager runs, not after. It used to be set
        // below, which left the manager driving one tick on the previous
        // tick's list -- and on the first tick of a trip, on no list at all.
        // Now that a braking vehicle's label is read off these zones, a queue
        // injected into a jam and updated before the jam was handed over lost
        // its brake lights on the spot.
        self.sync_braking_zones();
        self.traffic_manager.closed_lane = self.closed_lane_at(None, None);
        self.traffic_manager
            .update(dt, self.position_mi, time_scale, Some(hour), Some(weekend));
        self.check_zones();
        self.sync_chain_posts();
        self.check_chain_law();
        self.check_speed_limit();
        self.check_limit_drop_ahead();
        // Navigation before stop notices: the actionable instruction must
        // reach the event voice first.
        self.check_facility_leg_reset();
        self.check_navigation_cues();
        self.check_npc_traffic_cues();
        self.check_traffic_pressures();
        self.check_real_traffic_events();
        self.check_curves();
        self.check_lane_changes();
        self.check_stops();
        self.check_roadside_callouts();
        self.check_tolls();
        self.check_cities();
        self.check_timezone();
        if moved_mi > 0.0 {
            self.check_enforcement_heads_up();
            self.check_hazards(moved_mi);
            self.check_conditions_speed(moved_mi);
            self.check_inspections(moved_mi);
        }

        if self.position_mi >= self.total_miles() {
            self.finished = true;
            let city = self.route.cities.last().cloned().unwrap_or_default();
            let message = format!(
                "You have arrived in {}.",
                self.world.spoken_city(&city, None)
            );
            self.emit(
                TripEventKind::Arrived,
                SpokenMessage::new(message),
                TripEventData::default(),
            );
        }
        self.events.clone()
    }

    /// Hand the braking zones to the traffic manager, rebuilding the list only
    /// when the zones it is derived from no longer match it. `zones` is a
    /// public field that tests and the density model edit in place, so the
    /// list is compared against its source rather than trusting a flag.
    fn sync_braking_zones(&mut self) {
        let source = self
            .zones
            .iter()
            .filter(|zone| zone.reason == "heavy traffic" || zone.reason == "construction");
        let mut current = self.traffic_manager.braking_zones.iter();
        let mut in_sync = true;
        for zone in source.clone() {
            let matches = current.next().is_some_and(|braking| {
                braking.start_mi == (zone.start_mi - 1.0).max(0.0)
                    && braking.end_mi == zone.end_mi
                    && braking.reason == zone.reason
                    && braking.pace_mph == Some(zone.limit_mph)
            });
            if !matches {
                in_sync = false;
                break;
            }
        }
        if in_sync && current.next().is_none() {
            return;
        }
        self.traffic_manager.braking_zones = source
            .map(|zone| {
                BrakingZone::new(
                    (zone.start_mi - 1.0).max(0.0),
                    zone.end_mi,
                    &zone.reason,
                    Some(zone.limit_mph),
                )
            })
            .collect();
    }

    // -- event checks -----------------------------------------------------------------

    /// Full form on the first mention of a facility this leg, the proper
    /// name alone after (research doc R6). Marking is a side effect.
    pub fn name_facility(&mut self, plain_name: &str, full_name: &str) -> String {
        let key = plain_name.trim().to_lowercase();
        if self.facilities_named.contains(&key) {
            return plain_name.to_string();
        }
        self.facilities_named.insert(key);
        full_name.to_string()
    }

    /// Bring the full form back once -- a resume from a pause.
    pub fn reset_facility_mentions(&mut self) {
        self.facilities_named.clear();
    }

    /// A new leg brings every facility's full form back once.
    pub fn check_facility_leg_reset(&mut self) {
        let mut leg = 0;
        for (i, start) in self.leg_starts.iter().enumerate() {
            if self.position_mi >= *start {
                leg = i;
            }
        }
        if leg != self.facility_leg {
            self.facility_leg = leg;
            self.facilities_named.clear();
        }
    }

    pub fn check_stops(&mut self) {
        if let Some(planned_key) = self.planned_stop_key.clone() {
            let planned_at = self.planned_stop().map(|s| s.at_mi);
            if self.exit_in_progress.as_deref() == Some(planned_key.as_str()) {
                // Signaled and taking the exit: the plan is fulfilled quietly
                // when the stop opens, or the too-fast miss cancels it.
            } else if planned_at.is_none_or(|at| at < self.position_mi) {
                // Past the exit marker with no exit in progress.
                let name = self.planned_stop_label();
                self.planned_stop_key = None;
                self.emit(
                    TripEventKind::GpsCue,
                    SpokenMessage::new(format!("Past your planned stop, {name}. Plan canceled.")),
                    TripEventData {
                        planned: Some(true),
                        ..Default::default()
                    },
                );
            }
        }
        for i in 0..self.stops.len() {
            let ahead = self.stops[i].at_mi - self.position_mi;
            if !(0.0 < ahead && ahead <= STOP_AHEAD_LOOKAHEAD_MI) {
                continue;
            }
            let key = self.stops[i].key();
            if !self.announced_stops.contains(&key) {
                self.announced_stops.insert(key);
                let stop = self.stops[i].clone();
                let typed = self.name_facility(&stop.name, &stop.spoken_name());
                let distance = self.ahead_text(ahead);
                let parking_normal = stop.parking_text();
                let planned = self.is_planned(&stop);
                let message = stop_callout(&StopCalloutParts {
                    planned_prefix: self.planned_prefix(&stop),
                    typed_name: &typed,
                    plain_name: &stop.name,
                    exit_label: &stop.exit_label,
                    distance: &distance,
                    parking_normal: &parking_normal,
                    parking_certainty: &stop.parking,
                    exit_hint: &self.exit_hint,
                });
                // The plan flag rides the event so the driving layer can rank
                // the stop the player chose above ambient roadside chatter.
                self.emit(
                    TripEventKind::StopAhead,
                    message,
                    TripEventData {
                        stop: Some(stop.clone()),
                        planned: Some(planned),
                        ..Default::default()
                    },
                );
            }
        }
    }

    /// Whether the truck is still going round a street corner it has reached:
    /// the front is past the corner by less than the truck's own length.
    ///
    /// The turn chime sounds as the truck reaches the corner, and the next
    /// corner's call used to go out on the same tick, so "Turn right onto
    /// North Freeway Road" landed on the left-turn chime of the corner being
    /// taken, four turns out of four (agent drive, Tucson, 2026-09-24). The
    /// next call waits until the rear is round.
    pub fn turning_through_corner(&self) -> bool {
        use crate::sim::cross_traffic::{COMBINATION_LENGTH_FT, TRACTOR_LENGTH_FT};
        let length_ft = if self.truck.trailer_attached {
            COMBINATION_LENGTH_FT
        } else {
            TRACTOR_LENGTH_FT
        };
        let clear_mi = length_ft / 5280.0;
        self.navigation_cues.iter().any(|cue| {
            let past = self.position_mi - cue.at_mi;
            cue.kind == "local_turn"
                && matches!(
                    cue.direction.trim().to_ascii_lowercase().as_str(),
                    "left" | "right"
                )
                && (0.0..clear_mi).contains(&past)
        })
    }

    pub fn check_navigation_cues(&mut self) {
        // One maneuver at a time on street chains: only the nearest
        // not-yet-passed local turn may speak each tick, and none ahead while
        // the truck is still round the last one.
        let turning = self.turning_through_corner();
        let mut next_turn_key: Option<String> = None;
        let mut next_turn_ahead: Option<f64> = None;
        for cue in &self.navigation_cues {
            if cue.kind != "local_turn" {
                continue;
            }
            if turning && cue.at_mi > self.position_mi {
                continue;
            }
            // A turn already called and already taken is done speaking. Left
            // in the running, a corner just taken stayed "nearest" for the
            // tenth of a mile past it, and a second corner inside that tenth
            // had its call held until the truck was round it: "Turn left onto
            // 3rd Avenue Southeast" after the lean had already closed (agent
            // drive, Aberdeen yard, 2026-09-23). One still ahead keeps the
            // floor, or the turn after it is announced over it.
            let ahead = cue.at_mi - self.position_mi;
            if ahead < -0.1 {
                continue;
            }
            if ahead <= 0.0
                && self
                    .announced_navigation
                    .contains(&format!("{}:near", cue.key))
            {
                continue;
            }
            if next_turn_ahead.is_none_or(|best| ahead < best) {
                next_turn_key = Some(cue.key.clone());
                next_turn_ahead = Some(ahead);
            }
        }
        for i in 0..self.navigation_cues.len() {
            let cue = &self.navigation_cues[i];
            let ahead = cue.at_mi - self.position_mi;
            if cue.kind == "interchange" {
                continue;
            }
            if cue.kind == "local_turn" && next_turn_key.as_deref() != Some(cue.key.as_str()) {
                continue;
            }
            if cue.kind == "continue" || cue.kind == "onramp" {
                if !(-0.5..=0.5).contains(&ahead) {
                    continue;
                }
                let key = format!("{}:near", cue.key);
                if !self.announced_navigation.contains(&key) {
                    let cue = cue.clone();
                    self.announced_navigation.insert(key);
                    let text = if cue.near_text.is_empty() {
                        cue.text.clone()
                    } else {
                        cue.near_text.clone()
                    };
                    self.emit(
                        TripEventKind::GpsCue,
                        SpokenMessage::new(text),
                        TripEventData {
                            cue: Some(cue.clone()),
                            ..Default::default()
                        },
                    );
                }
                continue;
            }
            if cue.kind == "rest_stop" {
                // Road stops already receive one actionable announcement
                // from check_stops at five miles.
                continue;
            }
            if cue.kind == "traffic" {
                if !(0.0 < ahead && ahead <= 2.0) {
                    continue;
                }
                let key = format!("{}:advance", cue.key);
                if !self.announced_navigation.contains(&key) {
                    let cue = cue.clone();
                    self.announced_navigation.insert(key);
                    let speed = match cue.speed_mph {
                        Some(mph) => format!(" at {} miles per hour", fmt_f(mph, 0)),
                        None => String::new(),
                    };
                    let message = format!(
                        "Traffic slowing ahead in {}; {}{speed}.",
                        self.ahead_text(ahead),
                        cue.text
                    );
                    self.emit(
                        TripEventKind::GpsCue,
                        SpokenMessage::new(message),
                        TripEventData {
                            cue: Some(cue.clone()),
                            ..Default::default()
                        },
                    );
                }
                continue;
            }
            if cue.kind == "toll" {
                if !(0.0 < ahead && ahead <= 2.0) {
                    continue;
                }
                let advance_key = format!("{}:advance", cue.key);
                if !self.announced_navigation.contains(&advance_key) {
                    let cue = cue.clone();
                    self.announced_navigation.insert(advance_key);
                    // The heads-up is a preview: terse drops it whole.
                    self.emit(
                        TripEventKind::GpsCue,
                        terse_silent(cue.near_text.clone()),
                        TripEventData {
                            cue: Some(cue.clone()),
                            ..Default::default()
                        },
                    );
                }
                continue;
            }
            if cue.kind == "state_crossing" {
                if ahead > 0.0 {
                    continue;
                }
                let near_key = format!("{}:near", cue.key);
                if !self.announced_navigation.contains(&near_key) {
                    let cue = cue.clone();
                    self.announced_navigation.insert(near_key);
                    self.emit(
                        TripEventKind::StateCrossing,
                        SpokenMessage::new(cue.near_text.clone()),
                        TripEventData {
                            cue: Some(cue.clone()),
                            ..Default::default()
                        },
                    );
                }
                continue;
            }
            // Street maneuvers use a block-scale lookahead.
            let lookahead = if cue.kind == "local_turn" {
                LOCAL_TURN_LOOKAHEAD_MI
            } else {
                2.0
            };
            let in_advance_window = NAV_LEAD_MIN_MI < ahead && ahead <= lookahead;
            let in_near_window = (-0.1..=0.1).contains(&ahead);
            if !in_advance_window && !in_near_window {
                continue;
            }
            let cue = cue.clone();
            if in_advance_window {
                let advance_key = format!("{}:advance", cue.key);
                if !self.announced_navigation.contains(&advance_key) {
                    self.announced_navigation.insert(advance_key);
                    // The ladder never says zero, so the cue is spoken with a
                    // real distance instead of dropped.
                    let message = format!("In {}, {}.", self.ahead_text(ahead), cue.text);
                    self.emit(
                        TripEventKind::GpsCue,
                        SpokenMessage::new(message),
                        TripEventData {
                            cue: Some(cue.clone()),
                            advance: Some(true),
                            ..Default::default()
                        },
                    );
                }
            }
            if !in_near_window {
                continue;
            }
            let near_key = format!("{}:near", cue.key);
            if !self.announced_navigation.contains(&near_key) {
                self.announced_navigation.insert(near_key);
                let kind = if cue.kind == "checkpoint" {
                    TripEventKind::Checkpoint
                } else {
                    TripEventKind::GpsCue
                };
                self.emit(
                    kind,
                    SpokenMessage::new(cue.near_text.clone()),
                    TripEventData {
                        cue: Some(cue.clone()),
                        ..Default::default()
                    },
                );
            }
        }
    }

    /// A traffic advisory: one short form for every verbosity.
    pub fn traffic_pressure_message(
        &self,
        pressure: &TrafficPressure,
        ahead: f64,
    ) -> SpokenMessage {
        let distance = self.ahead_text(ahead);
        let speed = self.speed_value(pressure.target_speed_mph);
        let side = &pressure.direction;
        let text = match pressure.kind.as_str() {
            "exit" => format!("Exit traffic, {distance}. Hold {side}, {speed}."),
            // No target speed: the taper's posted limit is spoken separately.
            "construction_merge" => format!("Taper squeezing, {distance}. Merge {side}."),
            "route_merge" => format!("Merging traffic, {distance}. Keep {side}."),
            _ => format!("Traffic pack, {distance}. {speed}."),
        };
        SpokenMessage::new(text)
    }

    pub fn check_traffic_pressures(&mut self) {
        for i in 0..self.traffic_pressures.len() {
            let ahead = self.traffic_pressures[i].start_mi - self.position_mi;
            if !(0.0 < ahead && ahead <= TRAFFIC_PRESSURE_LOOKAHEAD_MI) {
                continue;
            }
            let key = traffic_pressure_key(&self.traffic_pressures[i]);
            if !self.announced_traffic_pressures.contains(&key) {
                let pressure = self.traffic_pressures[i].clone();
                if pressure.kind == "construction_merge"
                    && self.zones.iter().any(|zone| {
                        zone.reason == "construction"
                            && (zone.start_mi - pressure.end_mi).abs() < 0.01
                            && self.announced_zone_warnings.contains(&zone_key(zone))
                    })
                {
                    self.announced_traffic_pressures.insert(key);
                    continue;
                }
                self.announced_traffic_pressures.insert(key);
                let message = self.traffic_pressure_message(&pressure, ahead);
                self.emit(
                    TripEventKind::GpsCue,
                    message,
                    TripEventData {
                        traffic_pressure: Some(pressure.clone()),
                        ..Default::default()
                    },
                );
                return;
            }
        }
    }

    /// Lead distance for an enforcement cue, in miles, sized in real time.
    pub fn enforcement_warning_lookahead_mi(&self) -> f64 {
        let speed = self.truck.speed_mph().max(1.0);
        let miles = ENFORCEMENT_WARNING_REAL_S * speed * self.effective_time_scale() / 3600.0;
        CB_PATROL_LOOKAHEAD_MI.max(miles.min(ENFORCEMENT_WARNING_MAX_MI))
    }

    /// Mark posts heard, and spend the run's small CB speech budget well.
    /// Every post inside the lead window is marked announced (a post the
    /// player was never cued for is not allowed to observe them); the CB
    /// heads-up on top is rationed to CB_CALLS_PER_RUN, spent on the posts
    /// the driver's current speed actually exposes them to.
    pub fn check_enforcement_heads_up(&mut self) {
        let lookahead = self.enforcement_warning_lookahead_mi();
        let mut candidates: Vec<(f64, usize, f64)> = Vec::new();
        for i in 0..self.posts.len() {
            let ahead = self.posts[i].watch_start_mi() - self.position_mi;
            if !(0.0 < ahead && ahead <= lookahead) {
                continue;
            }
            let id = self.posts[i].id();
            if self.heads_up_seen.contains(&id) {
                continue;
            }
            self.heads_up_seen.insert(id);
            self.posts[i].announced = true;
            if self.posts[i].kind == KIND_FIXED_SCALE || self.posts[i].kind == KIND_SCALE_APRON {
                continue; // the scale has its own approach cue; the CB stays out of it
            }
            let post = self.posts[i].clone();
            let urgency = self.cb_urgency(&post);
            candidates.push((ahead, i, urgency));
        }
        if candidates.is_empty() || self.cb_calls_made >= CB_CALLS_PER_RUN {
            return;
        }
        // Urgency first, then whichever is nearest.
        let mut best = candidates[0];
        for c in &candidates[1..] {
            if (c.2, -c.0) > (best.2, -best.0) {
                best = *c;
            }
        }
        let (ahead, i, urgency) = best;
        if urgency <= 0.0 {
            return;
        }
        let post = self.posts[i].clone();
        if post.tableau && (self.pull_over_active || post.declined) {
            // The tableau line waits its turn; dropped, not spoken later.
            return;
        }
        self.cb_calls_made += 1;
        let message = if post.tableau {
            self.cb_tableau_message(&post, ahead)
        } else {
            self.cb_patrol_message(&post, ahead)
        };
        self.emit(
            TripEventKind::GpsCue,
            SpokenMessage::new(message),
            TripEventData {
                cb_patrol: Some(post),
                ..Default::default()
            },
        );
    }

    /// How much this post matters to how the truck is being driven now.
    /// Never zero for a staffed post; speed only raises the priority.
    pub fn cb_urgency(&mut self, post: &crate::sim::enforcement_posts::EnforcementPost) -> f64 {
        let (limit, _) = self.speed_limit_at(post.at_mi);
        let over = (self.truck.speed_mph() - limit).max(0.0);
        let base = if post.staffed { 1.0 } else { 0.35 };
        base + (over / 10.0).min(2.0)
    }

    /// Odds a random roadside log-check fires when the driver is in HOS
    /// violation, thinned by `hazard_scale`.
    pub fn random_inspection_odds(&self, leg: &crate::data::world_models::Leg) -> f64 {
        let base = if leg.checkpoints().is_empty() {
            0.25
        } else {
            0.55
        };
        base * self.hazard_scale
    }

    /// Route-backed inspections plus rare seeded patrols.
    pub fn check_inspections(&mut self, moved_mi: f64) {
        let previous_mi = self.position_mi - moved_mi;
        for i in 0..self.stops.len() {
            let stop = &self.stops[i];
            if stop.stop_type != "weigh_station"
                || !(previous_mi < stop.at_mi && stop.at_mi <= self.position_mi)
            {
                continue;
            }
            let key = format!("weigh:{}:{}", stop.name, fmt_f(stop.at_mi, 1));
            if self.announced_enforcement.contains(&key) {
                continue;
            }
            let spoken_name = stop.spoken_name();
            self.announced_enforcement.insert(key.clone());
            if self.hos_violation {
                self.emit(
                    TripEventKind::Inspection,
                    SpokenMessage::new(format!(
                        "{spoken_name} is open. Officers wave you in for an ELD check."
                    )),
                    TripEventData {
                        key: Some(key),
                        context: Some("weigh_station".to_string()),
                        evidence: Some(vec!["HOS/ELD violation".to_string()]),
                        ..Default::default()
                    },
                );
            }
            return;
        }

        let (limit, reason) = self.speed_limit_at(self.position_mi);
        if reason.as_deref() == Some("construction") && self.truck.speed_mph() > limit + 9.0 {
            if let Some(active_zone) = self.entered_zone.clone() {
                if active_zone.reason == "construction" {
                    let grace_start = self
                        .construction_zone_grace_start
                        .get(&zone_key(&active_zone))
                        .copied()
                        .unwrap_or(active_zone.start_mi);
                    if self.position_mi - grace_start < CONSTRUCTION_ENFORCEMENT_GRACE_MI {
                        return;
                    }
                }
            }
            let key = format!("construction:{}", round_py_int(self.position_mi));
            if !self.announced_enforcement.contains(&key) {
                self.announced_enforcement.insert(key.clone());
                self.emit(
                    TripEventKind::Inspection,
                    SpokenMessage::new("Trooper in the construction zone clocks your speed."),
                    TripEventData {
                        key: Some(key),
                        context: Some("construction_zone".to_string()),
                        evidence: Some(vec!["speeding in construction zone".to_string()]),
                        ..Default::default()
                    },
                );
                return;
            }
        }

        self.inspection_check_mi -= moved_mi;
        if self.inspection_check_mi > 0.0 {
            return;
        }
        self.inspection_check_mi = self.insp_rng.uniform(15.0, 40.0);
        if !self.hos_violation {
            // Roadcheck week: the CB says so once, at the first check of the
            // run, so the driver can walk around the truck before a trooper
            // does.
            if self.roadcheck_blitz && !self.announced_enforcement.contains("roadcheck") {
                self.announced_enforcement.insert("roadcheck".to_string());
                self.emit(
                    TripEventKind::Inspection,
                    SpokenMessage::new(
                        "It is Roadcheck week. Inspectors are out in force for three days, \
                         scales are open and troopers are checking paperwork.",
                    ),
                    TripEventData {
                        key: Some("roadcheck".to_string()),
                        context: Some("roadcheck_notice".to_string()),
                        evidence: Some(Vec::new()),
                        ..Default::default()
                    },
                );
            }
            // A legal driver still meets the occasional routine inspection:
            // a trooper pulls in behind for a Level 3, driver and paperwork.
            // The odds ride the safety record through
            // `roadside_inspection_scale`, memoryless over the check interval
            // so the interval itself never changes the rate.
            let chance = crate::sim::roadside_inspection::roadside_inspection_chance(
                self.inspection_check_mi,
                self.roadside_inspection_scale,
            );
            if chance > 0.0 && self.insp_rng.random() < chance {
                let key = format!("routine:{}", round_py_int(self.position_mi));
                self.emit(
                    TripEventKind::Inspection,
                    SpokenMessage::new(
                        "A trooper pulls in behind you for a routine roadside inspection.",
                    ),
                    TripEventData {
                        key: Some(key),
                        context: Some("routine_inspection".to_string()),
                        evidence: Some(Vec::new()),
                        ..Default::default()
                    },
                );
            }
            return;
        }
        let leg_index = self.current_leg_index();
        let has_checkpoints = !self.route.legs[leg_index].checkpoints().is_empty();
        let context = if has_checkpoints {
            "checkpoint corridor"
        } else {
            "patrol corridor"
        };
        let odds = self.random_inspection_odds(&self.route.legs[leg_index].clone());
        if self.insp_rng.random() < odds {
            let key = format!("patrol:{leg_index}:{}", round_py_int(self.position_mi));
            self.emit(
                TripEventKind::Inspection,
                SpokenMessage::new(format!(
                    "CB reports a patrol on this {context}. A trooper stops you for a log check."
                )),
                TripEventData {
                    key: Some(key),
                    context: Some(context.to_string()),
                    evidence: Some(vec!["HOS/ELD violation".to_string()]),
                    ..Default::default()
                },
            );
        }
    }
}
