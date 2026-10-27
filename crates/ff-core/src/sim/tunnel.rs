//! Data-led gates for roads that share a one-lane tunnel with rail traffic.

use chrono::{Duration, NaiveDate, NaiveDateTime, NaiveTime, Timelike};

use crate::data::world_models::{CrossingLoad, Route, TunnelData, WindDecision};
use crate::models::jobs::cargo_type;
use crate::sim::hos::HosClock;
use crate::sim::trip::{compute_leg_starts, Trip};
use crate::sim::trip_models::{TripEventData, TripEventKind};
use crate::sim::vehicle::KG_PER_LB;
use crate::speech_text::SpokenMessage;

/// Queueing for a wind escort, in minutes. The authorities publish the wind
/// levels but not the wait, so this is the game's figure.
pub const ESCORT_WAIT_MINUTES: f64 = 20.0;
/// How often a truck waiting out a wind closure checks the wind again.
pub const WIND_WAIT_STEP_MINUTES: f64 = 30.0;
/// The longest a closure is waited out before dispatch looks for a detour.
pub const MAX_WIND_WAIT_MINUTES: f64 = 4.0 * 60.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TunnelDirection {
    ToWhittier,
    FromWhittier,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TunnelDecision {
    Cross,
    Wait { minutes: i64, spoken: String },
    Closed { minutes: i64, spoken: String },
    HoursNotPublished { spoken: String },
}

fn record_at(
    data: &TunnelData,
    at: NaiveDateTime,
) -> Option<&crate::data::world_models::TunnelHours> {
    data.hours.iter().find(|h| {
        let from = chrono::NaiveDate::parse_from_str(&h.valid_from, "%Y-%m-%d").ok();
        let to = chrono::NaiveDate::parse_from_str(&h.valid_to, "%Y-%m-%d").ok();
        matches!((from, to), (Some(from), Some(to)) if from <= at.date() && at.date() <= to)
    })
}

fn opens_for(data: &TunnelData, at: NaiveDateTime, direction: TunnelDirection) -> bool {
    let Some(hours) = record_at(data, at) else {
        return false;
    };
    let Ok(open) = NaiveTime::parse_from_str(&hours.opens, "%H:%M") else {
        return false;
    };
    let Ok(close) = NaiveTime::parse_from_str(&hours.closes, "%H:%M") else {
        return false;
    };
    let time = at.time();
    if time < open || time >= close {
        return false;
    }
    let opening_minute =
        (time.minute() / data.schedule.interval_minutes) * data.schedule.interval_minutes;
    if time.minute() - opening_minute >= data.schedule.opening_minutes {
        return false;
    }
    let bv = opening_minute == 30;
    matches!(
        (bv, direction),
        (true, TunnelDirection::ToWhittier) | (false, TunnelDirection::FromWhittier)
    )
}

fn next_opening(
    data: &TunnelData,
    at: NaiveDateTime,
    direction: TunnelDirection,
) -> Option<NaiveDateTime> {
    let mut cursor = at.with_second(0)?.with_nanosecond(0)?;
    // Openings start on interval marks.  Move to the next mark strictly after
    // the arrival minute, so a :20 arrival catches the :30 opening.
    let interval = i64::from(data.schedule.interval_minutes);
    let past_mark = i64::from(cursor.time().minute()) % interval;
    cursor += Duration::minutes(interval - past_mark);
    for _ in 0..(366 * 48) {
        if opens_for(data, cursor, direction) {
            return Some(cursor);
        }
        cursor += Duration::minutes(30);
    }
    None
}

fn clock_text(at: NaiveDateTime) -> String {
    at.format("%-I:%M %p").to_string()
}

fn within_operating_hours(data: &TunnelData, at: NaiveDateTime) -> bool {
    let Some(hours) = record_at(data, at) else {
        return false;
    };
    match (
        NaiveTime::parse_from_str(&hours.opens, "%H:%M"),
        NaiveTime::parse_from_str(&hours.closes, "%H:%M"),
    ) {
        (Ok(open), Ok(close)) => at.time() >= open && at.time() < close,
        _ => false,
    }
}

/// Speak a wait in minutes below an hour and in hours and minutes above it.
pub fn wait_text(minutes: i64) -> String {
    let unit = |n: i64, word: &str| {
        if n == 1 {
            format!("{n} {word}")
        } else {
            format!("{n} {word}s")
        }
    };
    if minutes < 60 {
        return unit(minutes, "minute");
    }
    let (h, m) = (minutes / 60, minutes % 60);
    if m == 0 {
        unit(h, "hour")
    } else {
        format!("{} {}", unit(h, "hour"), unit(m, "minute"))
    }
}

pub fn gate_decision(
    data: &TunnelData,
    at: NaiveDateTime,
    direction: TunnelDirection,
) -> TunnelDecision {
    if record_at(data, at).is_none() {
        return TunnelDecision::HoursNotPublished {
            spoken: data.hours_not_published_text.clone(),
        };
    }
    if opens_for(data, at, direction) {
        return TunnelDecision::Cross;
    }
    let Some(next) = next_opening(data, at, direction) else {
        return TunnelDecision::HoursNotPublished {
            spoken: data.hours_not_published_text.clone(),
        };
    };
    let minutes = (next - at).num_minutes();
    if next.date() != at.date() || !within_operating_hours(data, at) {
        return TunnelDecision::Closed {
            minutes,
            spoken: format!(
                "The tunnel is closed for the night. The next opening is at {}.",
                clock_text(next)
            ),
        };
    }
    let target = match direction {
        TunnelDirection::ToWhittier => "Whittier",
        TunnelDirection::FromWhittier => "the staging area",
    };
    TunnelDecision::Wait {
        minutes,
        spoken: format!(
            "Next opening toward {target} at {}. You wait about {}.",
            clock_text(next),
            wait_text(minutes)
        ),
    }
}

/// Translate the career calendar into a dated schedule lookup on a place's
/// wall clock. The trip clock (`start_hour`, `game_minutes`) and career
/// hours run on the Eastern reference clock, so the moment is shifted into
/// `zone` before the date and time are read. Tunnel schedules are published
/// for the 2026 season while the career calendar is cyclic, so the game's
/// month and day are kept on the published season year.
pub fn career_datetime(
    career_hours: Option<f64>,
    start_hour: f64,
    game_minutes: f64,
    zone: crate::sim::timezones::TimeZone,
) -> NaiveDateTime {
    let local = career_local_hours(career_hours, start_hour, game_minutes, zone);
    let day = crate::sim::season::day_of_year(local).floor() as u32;
    let date = NaiveDate::from_yo_opt(2026, day.clamp(1, 365)).expect("valid calendar day");
    let minute = (local.rem_euclid(24.0) * 60.0).floor() as u32;
    date.and_hms_opt(minute / 60, minute % 60, 0)
        .expect("valid clock time")
}

/// The career clock on `zone`'s wall clock, in career hours: the day comes
/// from the career calendar, the time of day from the trip's start hour.
pub fn career_local_hours(
    career_hours: Option<f64>,
    start_hour: f64,
    game_minutes: f64,
    zone: crate::sim::timezones::TimeZone,
) -> f64 {
    let day_start = (career_hours.unwrap_or(0.0) / 24.0).floor() * 24.0;
    crate::sim::timezones::to_local(day_start + start_hour + game_minutes / 60.0, zone)
}

/// A short gate wait in the staging line is duty time.  A closure for the
/// night is logged as sleeper-berth time; a wait of ten hours or more
/// completes the 10-hour reset.  Overnight staging parking is unverified, so
/// the log is a modelling choice recorded in the roadmap.
pub fn log_wait_to_hos(clock: &mut HosClock, wait_minutes: i64, closed: bool) {
    if wait_minutes <= 0 {
        return;
    }
    if closed {
        clock.sleeper(wait_minutes as f64);
    } else {
        clock.on_duty(wait_minutes as f64);
    }
}

impl Trip {
    /// The time zone of the city a leg starts from: the wall clock its
    /// crossings keep. Eastern, the reference clock, when the city is unknown.
    pub(crate) fn leg_start_zone(&self, leg_index: usize) -> crate::sim::timezones::TimeZone {
        self.route
            .cities
            .get(leg_index)
            .and_then(|id| self.world.cities.get(id))
            .filter(|city| city.lat != 0.0 || city.lon != 0.0)
            .map_or(crate::sim::timezones::EASTERN, |city| {
                crate::sim::timezones::city_zone(city)
            })
    }

    /// The truck and load as a crossing judges them: the whole combination
    /// with a trailer on, the tractor alone without one.
    pub fn crossing_load(&self) -> CrossingLoad {
        let length_ft = if self.truck.trailer_attached {
            self.truck.rig_length_ft
        } else {
            crate::sim::cross_traffic::TRACTOR_LENGTH_FT
        };
        let cargo_lb = (self.truck.cargo_kg / KG_PER_LB).round().max(0.0) as u32;
        let key = self.truck.cargo_key.as_str();
        let tank = cargo_type(key).is_some_and(|cargo| cargo.tank);
        CrossingLoad::plain(length_ft, self.truck.rig_width_ft, self.truck.rig_height_ft)
            .with_cargo(key, tank, cargo_lb)
            .with_trailer(self.truck.trailer_attached)
    }

    /// Swap in a road that shares everything behind the truck and differs
    /// ahead, and re-place what the road carries the way `Trip::new` does.
    pub fn reroute_ahead(&mut self, route: Route) {
        self.route = route;
        self.leg_starts = compute_leg_starts(&self.route);
        let mut mileposts = self.leg_starts.clone();
        mileposts.push(self.route.miles());
        self.city_mileposts = mileposts;
        self.traffic_manager
            .set_route(&self.route, &self.leg_starts);
        let (_, crossings) = self.compute_timezone_crossings();
        self.timezone_crossings = crossings;
        self.stops = self.place_stops();
        self.zones = self.place_zones();
        self.traffic_pressures = self.place_traffic_pressures();
        let (navigation_cues, border_booths) = self.build_navigation_cues_and_border_booths();
        self.navigation_cues = navigation_cues;
        self.border_booths = border_booths;
        self.landmarks = self.place_landmarks();
        self.billboards = self.place_billboards();
        self.chain_law_areas = self.place_chain_law_areas();
        self.posts = self.place_enforcement_posts();
        self.curves = self.place_curves();
    }

    /// Where live weather reads a wind-rule crossing on this leg: the leg's
    /// toll point named for the crossing, as (station key, lat, lon). None
    /// when the leg names no such point; the truck's own station is used.
    pub fn crossing_station(&self, leg_index: usize) -> Option<(String, f64, f64)> {
        let leg = self.route.legs.get(leg_index)?;
        let crossing = leg.crossings.iter().find(|c| c.wind.is_some())?;
        let toll = leg
            .toll_events()
            .iter()
            .find(|t| t.name.starts_with(&crossing.name))?;
        let forward = self.route.cities.get(leg_index)? == &leg.a;
        let offset = if forward {
            toll.at_mi
        } else {
            leg.miles - toll.at_mi
        };
        let start = *self.leg_starts.get(leg_index)?;
        let (lat, lon) = self.latlon_at(Some(start + offset.clamp(0.0, leg.miles)));
        Some((format!("crossing:{}", crossing.name), lat, lon))
    }

    /// The wind call at a crossing on this leg, if it has one, judged at
    /// the sky's current sustained wind: (refused, spoken rule).
    fn wind_call(&mut self, leg_index: usize, load: &CrossingLoad) -> Option<(bool, String)> {
        let station = self.crossing_station(leg_index);
        let wind_mph = self.weather.wind_mph_near(
            station
                .as_ref()
                .map(|(key, lat, lon)| (key.as_str(), *lat, *lon)),
        );
        self.route
            .legs
            .get(leg_index)?
            .crossings
            .iter()
            .find_map(|c| match c.wind.as_ref()?.decide(load, wind_mph) {
                WindDecision::Cross => None,
                WindDecision::Escort(text) => Some((false, text.to_string())),
                WindDecision::Refused(text) => Some((true, text.to_string())),
            })
    }

    /// Wind at a crossing: an escort is a short wait on duty; a closure is
    /// waited out on duty, then driven around when a legal road exists, and
    /// otherwise held at the approach until the wind drops. Returns Some
    /// with whether the truck is held, or None when the wind lets it on.
    fn process_wind_gate(&mut self, leg_index: usize, load: &CrossingLoad) -> Option<bool> {
        let (refused, text) = match self.wind_call(leg_index, load) {
            Some(call) => call,
            None => {
                if self.wind_held_gates.remove(&leg_index) {
                    self.emit(
                        TripEventKind::GpsCue,
                        SpokenMessage::new("The wind has dropped. You may cross.".to_string()),
                        TripEventData {
                            context: Some("crossing_wind_dropped".to_string()),
                            ..Default::default()
                        },
                    );
                }
                return None;
            }
        };
        if self.wind_held_gates.contains(&leg_index) {
            return Some(true);
        }
        if !refused {
            self.game_minutes += ESCORT_WAIT_MINUTES;
            self.weather.update(ESCORT_WAIT_MINUTES);
            self.log_wind_wait(
                ESCORT_WAIT_MINUTES,
                format!(
                    "{text} You wait {} on duty.",
                    wait_text(ESCORT_WAIT_MINUTES as i64)
                ),
            );
            return None;
        }
        let mut waited = 0.0;
        while waited < MAX_WIND_WAIT_MINUTES {
            self.game_minutes += WIND_WAIT_STEP_MINUTES;
            self.weather.update(WIND_WAIT_STEP_MINUTES);
            waited += WIND_WAIT_STEP_MINUTES;
            if self
                .wind_call(leg_index, load)
                .is_none_or(|(refused, _)| !refused)
            {
                break;
            }
        }
        self.log_wind_wait(
            waited,
            format!(
                "{text} You wait {} on duty for the wind to drop.",
                wait_text(waited.round() as i64)
            ),
        );
        match self.wind_call(leg_index, load) {
            Some((true, _)) => {}
            Some((false, _)) => return self.process_wind_gate(leg_index, load),
            None => return None,
        }
        let ahead = leg_index;
        let avoid = self.route.legs.get(leg_index)?.id;
        let city = self.route.cities.get(ahead).cloned().unwrap_or_default();
        if let Some(route) = self.world.detour_ahead(&self.route, ahead, load, avoid) {
            self.reroute_ahead(route);
            self.processed_tunnel_gates.retain(|i| *i < leg_index);
            self.refused_tunnel_gates.retain(|i| *i < leg_index);
            let spoken = format!(
                "The wind is still too strong. Dispatch reroutes you from {}.",
                self.world.spoken_city(&city, None)
            );
            self.emit(
                TripEventKind::GpsCue,
                SpokenMessage::new(spoken),
                TripEventData {
                    context: Some("crossing_reroute".to_string()),
                    ..Default::default()
                },
            );
            return Some(false);
        }
        self.wind_held_gates.insert(leg_index);
        self.emit(
            TripEventKind::GpsCue,
            SpokenMessage::new(
                "There is no other road. You hold here until the wind drops.".to_string(),
            ),
            TripEventData {
                context: Some("crossing_wind_hold".to_string()),
                ..Default::default()
            },
        );
        Some(true)
    }

    fn log_wind_wait(&mut self, minutes: f64, spoken: String) {
        if minutes <= 0.0 {
            return;
        }
        self.emit(
            TripEventKind::GpsCue,
            SpokenMessage::new(spoken),
            TripEventData {
                amount: Some(minutes),
                context: Some("tunnel_wait".to_string()),
                ..Default::default()
            },
        );
    }

    /// Judge a leg's crossings and tunnel gate at the city where the leg
    /// starts. A crossing that refuses the load re-plans the road from that
    /// city, so the truck never waits at a portal it may not enter. A
    /// scheduled tunnel advances the clock to its opening. True holds the
    /// truck short of the leg.
    pub fn process_tunnel_gate(&mut self, leg_index: usize) -> bool {
        if self.processed_tunnel_gates.contains(&leg_index) {
            return self.refused_tunnel_gates.contains(&leg_index);
        }
        let Some(leg) = self.route.legs.get(leg_index) else {
            return false;
        };
        let load = self.crossing_load();
        if leg.crossings.iter().any(|c| c.refusal(&load).is_some()) {
            let start = self.leg_starts.get(leg_index).copied().unwrap_or(0.0);
            let city = self
                .route
                .cities
                .get(leg_index)
                .cloned()
                .unwrap_or_default();
            match self.world.legalize_route_ahead(&self.route, start, &load) {
                Some((route, reason)) => {
                    self.reroute_ahead(route);
                    self.processed_tunnel_gates.retain(|i| *i < leg_index);
                    self.refused_tunnel_gates.retain(|i| *i < leg_index);
                    let spoken = format!(
                        "{reason} Dispatch reroutes you from {}.",
                        self.world.spoken_city(&city, None)
                    );
                    self.emit(
                        TripEventKind::GpsCue,
                        SpokenMessage::new(spoken),
                        TripEventData {
                            context: Some("crossing_reroute".to_string()),
                            ..Default::default()
                        },
                    );
                    return false;
                }
                None => {
                    // Unreachable for a load dispatch planned: the origin
                    // had a legal road, so a way back to it always exists.
                    self.processed_tunnel_gates.insert(leg_index);
                    self.refused_tunnel_gates.insert(leg_index);
                    let reason = leg
                        .crossings
                        .iter()
                        .find_map(|c| c.refusal(&load))
                        .unwrap_or_default()
                        .to_string();
                    self.emit(
                        TripEventKind::GpsCue,
                        SpokenMessage::new(reason),
                        TripEventData {
                            context: Some("crossing_refused".to_string()),
                            ..Default::default()
                        },
                    );
                    return true;
                }
            }
        }
        if let Some(held) = self.process_wind_gate(leg_index, &load) {
            return held;
        }
        let Some(leg) = self.route.legs.get(leg_index) else {
            return false;
        };
        let Some(data) = leg.corridor().tunnel.clone() else {
            self.processed_tunnel_gates.insert(leg_index);
            return false;
        };
        let leg_from = leg.a.clone();
        self.processed_tunnel_gates.insert(leg_index);
        // The tunnel's name and the gate's verdict are one line. Two route
        // lines in one tick made the pacer flush the first and replay it in
        // front of the second, so the name was delivered twice.
        let gate_line = |verdict: &str| format!("{} {verdict}", data.planning_text);
        let forward = self.route.cities.get(leg_index) == Some(&leg_from);
        let direction = if forward {
            TunnelDirection::ToWhittier
        } else {
            TunnelDirection::FromWhittier
        };
        match gate_decision(
            &data,
            career_datetime(
                self.career_hours,
                self.start_hour,
                self.game_minutes,
                self.leg_start_zone(leg_index),
            ),
            direction,
        ) {
            TunnelDecision::Cross => {
                self.emit(
                    TripEventKind::GpsCue,
                    SpokenMessage::new(data.planning_text.clone()),
                    TripEventData {
                        context: Some("tunnel_planning".to_string()),
                        ..Default::default()
                    },
                );
                false
            }
            TunnelDecision::HoursNotPublished { spoken } => {
                self.refused_tunnel_gates.insert(leg_index);
                self.emit(
                    TripEventKind::GpsCue,
                    SpokenMessage::new(gate_line(&spoken)),
                    TripEventData {
                        context: Some("tunnel_hours_not_published".to_string()),
                        ..Default::default()
                    },
                );
                true
            }
            TunnelDecision::Wait { minutes, spoken } => {
                self.game_minutes += minutes as f64;
                self.emit(
                    TripEventKind::GpsCue,
                    SpokenMessage::new(gate_line(&spoken)),
                    TripEventData {
                        amount: Some(minutes as f64),
                        context: Some("tunnel_wait".to_string()),
                        ..Default::default()
                    },
                );
                false
            }
            TunnelDecision::Closed { minutes, spoken } => {
                self.game_minutes += minutes as f64;
                self.emit(
                    TripEventKind::GpsCue,
                    SpokenMessage::new(gate_line(&spoken)),
                    TripEventData {
                        amount: Some(minutes as f64),
                        context: Some("tunnel_closed".to_string()),
                        ..Default::default()
                    },
                );
                false
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn data() -> TunnelData {
        serde_json::from_str(r#"{"source_url":"x","access_date":"2026-10-09","planning_text":"p","toll_text":"t","closed_text":"c","hours_not_published_text":"Hours not published","hours":[{"valid_from":"2026-01-01","valid_to":"2026-04-30","opens":"07:00","closes":"22:30","source_url":"x","access_date":"2026-10-09"},{"valid_from":"2026-10-01","valid_to":"2026-12-31","opens":"07:00","closes":"22:30","source_url":"x","access_date":"2026-10-09"},{"valid_from":"2026-05-01","valid_to":"2026-09-30","opens":"05:30","closes":"23:15","source_url":"x","access_date":"2026-10-09"}],"schedule":{"opening_minutes":15,"interval_minutes":30,"source_url":"x","access_date":"2026-10-09"},"speed_mph":25,"length_mi":2.5,"speed_source_url":"x","speed_access_date":"2026-10-09","length_source_url":"x","length_access_date":"2026-10-09"}"#).unwrap()
    }
    #[test]
    fn schedule_edges() {
        let d = data();
        let t = NaiveDateTime::parse_from_str("2026-05-01 05:35", "%F %R").unwrap();
        assert_eq!(
            gate_decision(&d, t, TunnelDirection::ToWhittier),
            TunnelDecision::Cross
        );
        let edge = NaiveDateTime::parse_from_str("2026-05-01 05:45", "%F %R").unwrap();
        assert!(matches!(
            gate_decision(&d, edge, TunnelDirection::ToWhittier),
            TunnelDecision::Wait { .. }
        ));
    }
    #[test]
    fn hours_and_hos_are_accounted() {
        let mut d = data();
        d.hours.push(crate::data::world_models::TunnelHours {
            valid_from: "2026-10-01".into(),
            valid_to: "2027-04-30".into(),
            opens: "07:00".into(),
            closes: "22:45".into(),
            source_url: "x".into(),
            access_date: "2026-10-09".into(),
        });
        let may = NaiveDateTime::parse_from_str("2026-05-01 05:30", "%F %R").unwrap();
        let oct = NaiveDateTime::parse_from_str("2026-10-01 07:00", "%F %R").unwrap();
        assert_eq!(
            gate_decision(&d, may, TunnelDirection::ToWhittier),
            TunnelDecision::Cross
        );
        assert_eq!(
            gate_decision(&d, oct, TunnelDirection::FromWhittier),
            TunnelDecision::Cross
        );
        let outside = NaiveDateTime::parse_from_str("2027-05-01 07:00", "%F %R").unwrap();
        assert!(matches!(
            gate_decision(&d, outside, TunnelDirection::FromWhittier),
            TunnelDecision::HoursNotPublished { .. }
        ));
        let mut hos = HosClock::new();
        log_wait_to_hos(&mut hos, 22, false);
        hos.drive(d.length_mi / d.speed_mph * 60.0);
        assert_eq!(hos.status, "driving");
        assert!(hos.driving_min > 5.9 && hos.driving_min < 6.1);
        assert!(hos.duty_min > 27.9 && hos.duty_min < 28.1);
    }
    #[test]
    fn missing_hours_or_source_fails_validation() {
        let mut d = data();
        d.hours.clear();
        assert!(d.validate("test").is_err());
        let mut d = data();
        d.speed_source_url.clear();
        assert!(d.validate("test").is_err());
    }
    fn at(s: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(s, "%F %R").unwrap()
    }
    fn wait_of(d: &TunnelDecision) -> i64 {
        match d {
            TunnelDecision::Wait { minutes, .. } | TunnelDecision::Closed { minutes, .. } => {
                *minutes
            }
            other => panic!("expected a wait, got {other:?}"),
        }
    }
    #[test]
    fn next_opening_uses_the_half_hour_mark() {
        // Official summer table: Bear Valley to Whittier opens at :30,
        // Whittier to Bear Valley at :00.
        let d = data();
        let to = TunnelDirection::ToWhittier;
        let from = TunnelDirection::FromWhittier;
        assert_eq!(wait_of(&gate_decision(&d, at("2026-06-01 10:10"), to)), 20);
        assert_eq!(wait_of(&gate_decision(&d, at("2026-06-01 10:20"), to)), 10);
        // :10 is inside the away-from-Whittier opening that starts at :00.
        assert_eq!(
            gate_decision(&d, at("2026-06-01 10:10"), from),
            TunnelDecision::Cross
        );
        assert_eq!(
            wait_of(&gate_decision(&d, at("2026-06-01 10:20"), from)),
            40
        );
        // :40 is inside the toward-Whittier opening that starts at :30.
        assert_eq!(
            gate_decision(&d, at("2026-06-01 10:40"), to),
            TunnelDecision::Cross
        );
        assert_eq!(wait_of(&gate_decision(&d, at("2026-06-01 10:50"), to)), 40);
        assert_eq!(
            wait_of(&gate_decision(&d, at("2026-06-01 10:40"), from)),
            20
        );
        assert_eq!(wait_of(&gate_decision(&d, at("2026-06-01 05:20"), to)), 10);
        assert_eq!(wait_of(&gate_decision(&d, at("2026-06-01 06:05"), to)), 25);
        assert_eq!(wait_of(&gate_decision(&d, at("2026-06-01 06:29"), to)), 1);
        match gate_decision(&d, at("2026-06-01 10:20"), to) {
            TunnelDecision::Wait { spoken, .. } => assert_eq!(
                spoken,
                "Next opening toward Whittier at 10:30 AM. You wait about 10 minutes."
            ),
            other => panic!("{other:?}"),
        }
    }
    #[test]
    fn night_arrivals_are_closed_not_long_waits() {
        let d = data();
        for t in ["2026-06-01 23:15", "2026-06-02 00:30", "2026-06-02 03:00"] {
            match gate_decision(&d, at(t), TunnelDirection::ToWhittier) {
                TunnelDecision::Closed { spoken, .. } => assert_eq!(
                    spoken,
                    "The tunnel is closed for the night. The next opening is at 5:30 AM."
                ),
                other => panic!("{t}: {other:?}"),
            }
        }
        assert_eq!(wait_text(45), "45 minutes");
        assert_eq!(wait_text(60), "1 hour");
        assert_eq!(wait_text(150), "2 hours 30 minutes");
    }
    #[test]
    fn overnight_closure_is_sleeper_and_short_waits_are_duty() {
        let d = data();
        let decision = gate_decision(&d, at("2026-06-01 23:15"), TunnelDirection::ToWhittier);
        let TunnelDecision::Closed { minutes, .. } = decision else {
            panic!("expected closed");
        };
        assert_eq!(minutes, 375);
        let mut hos = HosClock::new();
        log_wait_to_hos(&mut hos, minutes, true);
        assert!(hos
            .history
            .iter()
            .any(|e| e.status == "sleeper_berth" && (e.minutes - 375.0).abs() < 0.1));
        assert!(!hos
            .history
            .iter()
            .any(|e| e.status == "on_duty_not_driving"));
        let mut short = HosClock::new();
        log_wait_to_hos(&mut short, 20, false);
        assert_eq!(short.status, "on_duty_not_driving");
        let mut long = HosClock::new();
        long.drive(300.0);
        log_wait_to_hos(&mut long, 650, true);
        assert_eq!(long.driving_min, 0.0);
    }
    #[test]
    fn career_datetime_reads_the_career_calendar_day_not_the_next_one() {
        use crate::sim::season::calendar_date;
        use crate::sim::timezones::EASTERN;
        use chrono::Datelike;
        for hours in [0.0, 30.0, 24.0 * 100.0 + 7.0] {
            let at = career_datetime(Some(hours), hours.rem_euclid(24.0), 0.0, EASTERN);
            let shown = calendar_date(hours);
            assert_eq!((at.month(), at.day()), (shown.month(), shown.day()));
        }
    }

    #[test]
    fn career_datetime_is_the_crossing_wall_clock() {
        use crate::sim::timezones::{ALASKA, EASTERN, PACIFIC};
        let monday_eight = Some(24.0 * 5.0 + 8.0);
        let eastern = career_datetime(monday_eight, 8.0, 30.0, EASTERN);
        let pacific = career_datetime(monday_eight, 8.0, 30.0, PACIFIC);
        let alaska = career_datetime(monday_eight, 8.0, 30.0, ALASKA);
        assert_eq!(eastern.time(), NaiveTime::from_hms_opt(8, 30, 0).unwrap());
        assert_eq!(pacific.time(), NaiveTime::from_hms_opt(5, 30, 0).unwrap());
        assert_eq!(alaska.time(), NaiveTime::from_hms_opt(4, 30, 0).unwrap());
        assert_eq!(eastern.date(), alaska.date());
        let after_midnight = career_datetime(Some(24.0 * 5.0 + 1.0), 1.0, 0.0, ALASKA);
        assert_eq!(after_midnight.date() + Duration::days(1), eastern.date());
    }
}
