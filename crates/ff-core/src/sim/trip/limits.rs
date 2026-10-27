//! What the road posts and what the zones say: zone warnings and entries,
//! the time-zone line, posted-limit arrivals and the advance "drops to X"
//! pacenote (the limit section of `trip.py`).

use crate::pyfmt::round_py_n;
use crate::sim::hos::clock_text;
use crate::sim::road_event_pacing::SPOKEN_CHARS_PER_REAL_S;
use crate::sim::trip_models::*;
use crate::sim::trip_route_helpers::zone_key;
use crate::speech_text::SpokenMessage;

use super::{
    limit_reason_by_stop_type, spoken_short_miles, Trip, LIMIT_DOWNGRADE_MIN_MI,
    LIMIT_DOWNGRADE_PCT, LIMIT_DROP_WARN_MIN_DELTA_MPH, LIMIT_REASON_LOOKAHEAD_MI,
    LIMIT_SCAN_MAX_MI, LIMIT_SCAN_STRIDE_MI, LIMIT_SHORT_ZONE_MI, LIMIT_WARNING_MAX_LEAD_MI,
    LIMIT_WARNING_REAL_S, PACENOTE_MARGIN_MPH, PACENOTE_MIN_LEAD_MI, ZONE_WARNING_MIN_MI,
};

impl Trip {
    /// Lead distance for a zone warning, scaled so the player gets roughly
    /// `ZONE_WARNING_REAL_S` of real time despite speed and compression.
    pub fn zone_warning_lookahead_mi(&self) -> f64 {
        let speed = self.truck.speed_mph().max(1.0);
        let miles = ZONE_WARNING_REAL_S * speed * self.effective_time_scale() / 3600.0;
        ZONE_WARNING_LOOKAHEAD_MI.max(miles.min(ZONE_WARNING_MAX_MI))
    }

    /// (closed lane name, direction to merge) for a zone's coned-off lane.
    pub fn closure_phrases(zone: &Zone) -> (&'static str, &'static str) {
        let shut = match zone.closed_side.as_deref() {
            Some("right") => "right",
            Some(_) => "left",
            None => {
                if zone.closed_lane == Some(0) {
                    "right"
                } else {
                    "left"
                }
            }
        };
        (shut, if shut == "right" { "left" } else { "right" })
    }

    pub fn zone_warning_message(&self, zone: &Zone, ahead: f64) -> String {
        // NOT "Brake now!". This is an advance warning that fires up to
        // eight miles out, and "Brake now" is the emergency hazard opening;
        // Shane had been braking for work eight miles away because he was
        // told to (2026-08-24). It opens with the distance like its siblings.
        // One short form for every verbosity: the full warning ran three
        // sentences (speech mode audit, 2026-10-03).
        if let Some(short) = self.zone_warning_terse(zone, ahead) {
            return short;
        }
        if self.street_zone_before(zone).is_some() {
            // The next street's lower number, the way the limit pacenote says
            // it on a highway; "facility access road ahead" was the road the
            // truck is already on.
            return format!(
                "Speed limit drops to {} in {}.",
                self.speed_value(zone.limit_mph),
                self.ahead_text(ahead)
            );
        }
        if super::streets::is_street_zone_reason(&zone.reason) {
            return format!(
                "In {}, speed limit {}.",
                self.ahead_text(ahead),
                self.speed_value(zone.limit_mph)
            );
        }
        if zone.reason == "heavy traffic" && zone.aadt.is_some() {
            return format!(
                "In {}, {} ahead. Traffic slowing to {}.",
                self.ahead_text(ahead),
                self.congestion_phrase(),
                self.speed_value(zone.limit_mph)
            );
        }
        format!(
            "In {}, {} ahead. Speed limit {}.",
            self.ahead_text(ahead),
            zone.reason,
            self.speed_value(zone.limit_mph)
        )
    }

    /// The construction warning, in the terse slot grammar: the thing and
    /// how far, the lane to be in, the limit. The mile ahead of the work is
    /// the reduced-speed approach, the way a real interstate work zone steps
    /// its limit down through its advance warning area (MUTCD Part 6; owner
    /// ruling, 2026-09-14), at the approach's own limit (2026-09-28). Other
    /// zones keep their one-sentence wording.
    pub fn zone_warning_terse(&self, zone: &Zone, ahead: f64) -> Option<String> {
        if zone.reason != "construction" {
            return None;
        }
        let merge = if zone.closed_side.is_some() {
            let (_, keep) = Self::closure_phrases(zone);
            format!(" Merge {keep}.")
        } else {
            String::new()
        };
        let taper_mph = self
            .zones
            .iter()
            .find(|z| z.reason == "construction merge" && (z.end_mi - zone.start_mi).abs() < 1e-6)
            .map_or(CONSTRUCTION_TAPER_LIMIT_MPH, |z| z.limit_mph);
        let limit = if taper_mph <= zone.limit_mph {
            self.speed_value(zone.limit_mph)
        } else {
            format!(
                "{}, then {}",
                self.speed_value(taper_mph),
                self.speed_value(zone.limit_mph)
            )
        };
        Some(format!(
            "Construction, {}.{merge} Limit {limit}.",
            self.ahead_text(ahead)
        ))
    }

    /// What to call a live jam: rush hour gets named when it is one.
    pub fn congestion_phrase(&self) -> &'static str {
        let hour = self.local_hour();
        let in_rush = RUSH_HOUR_WINDOWS
            .iter()
            .any(|(start, end)| *start <= hour && hour < *end);
        if in_rush && !self.is_weekend_now() {
            "rush hour congestion"
        } else {
            "heavy traffic"
        }
    }

    /// A work zone entry: the lane to be in and the limit (speech mode
    /// audit, 2026-10-03). Other zones are a sentence already.
    pub fn zone_entry_terse(&self, zone: &Zone) -> Option<String> {
        let limit = self.speed_value(zone.limit_mph);
        let keep = zone
            .closed_side
            .as_ref()
            .map(|_| Self::closure_phrases(zone).1);
        match (zone.reason.as_str(), keep) {
            ("construction merge", Some(keep)) => {
                Some(format!("Reduced speed. Merge {keep}. Limit {limit}."))
            }
            ("construction merge", None) => Some(format!("Flagger ahead. Limit {limit}.")),
            ("construction", Some(keep)) => Some(format!("Work zone. Keep {keep}. Limit {limit}.")),
            ("construction", None) => Some(format!("Work zone. Limit {limit}.")),
            _ => None,
        }
    }

    pub fn zone_entry_message(&self, zone: &Zone) -> String {
        // A work zone's entry is its short form at every verbosity.
        if let Some(short) = self.zone_entry_terse(zone) {
            return short;
        }
        if zone.reason == "heavy traffic" && zone.aadt.is_some() {
            return format!(
                "{}. Traffic slowing to {}.",
                crate::data::world_models::py_capitalize(self.congestion_phrase()),
                self.speed_value(zone.limit_mph)
            );
        }
        if zone.reason == super::streets::YARD_ZONE {
            return format!(
                "Into the yard. Yard limit {}.",
                self.speed_value(zone.limit_mph)
            );
        }
        if zone.reason == super::streets::LOT_ZONE {
            return format!(
                "Into the lot. Lot limit {}.",
                self.speed_value(zone.limit_mph)
            );
        }
        if let Some(before) = self.street_zone_before(zone) {
            // A new street's limit, said the way the road signs it: the
            // number, and which way it went.
            let verb = if zone.limit_mph < before.limit_mph {
                "reduced"
            } else {
                "raised"
            };
            return format!(
                "Speed limit {verb} to {}.",
                self.speed_value(zone.limit_mph)
            );
        }
        if super::streets::is_street_zone_reason(&zone.reason) {
            // The first street: just its limit. The line onto the streets
            // has already named it, and a street is not a zone.
            return format!("Speed limit {}.", self.speed_value(zone.limit_mph));
        }
        // Say you are *in* it, not that it is ahead; pairs with the "End of
        // ... zone" exit.
        format!(
            "Entering {} zone. Speed limit {}.",
            zone.reason,
            self.speed_value(zone.limit_mph)
        )
    }

    /// Adjacent local sections can be one audible queue even though their
    /// inputs must stay separate for changes in the commuter clock.
    fn same_traffic_pace(&self, previous: &Zone, next: &Zone) -> bool {
        if previous.reason != "heavy traffic"
            || next.reason != "heavy traffic"
            || previous.end_mi != next.start_mi
        {
            return false;
        }
        let mut previous = previous.clone();
        let mut next = next.clone();
        self.zone_is_active(&mut previous)
            && self.zone_is_active(&mut next)
            && previous.limit_mph == next.limit_mph
    }

    pub fn check_zones(&mut self) {
        let lookahead = self.zone_warning_lookahead_mi();
        let pos = self.position_mi;
        // The NEXT zone only, not every zone inside the lookahead (owner
        // playtest, 2026-08-17).
        let mut due: Vec<(f64, usize)> = Vec::new();
        for i in 0..self.zones.len() {
            if self.zones[i].reason == "construction merge" {
                continue;
            }
            // The yard begins at the driveway, which is a turn: its own call
            // names the corner and its speed, lower than the yard's.
            if matches!(
                self.zones[i].reason.as_str(),
                super::streets::YARD_ZONE | super::streets::LOT_ZONE
            ) {
                continue;
            }
            // A street's change of limit is signed where it changes. Only a
            // drop big enough to brake for earns a warning ahead, the rule
            // the highway pacenote keeps (`next_limit_drop`); a rise never.
            // Only at the pacenote's own lead, and only to a truck over the
            // new number: a zone warning's reach is sized for the highway, and
            // "drops to 30 in 2 miles" four streets out named a sign the
            // driver could do nothing about yet.
            if let Some(before) = self.street_zone_before(&self.zones[i]) {
                let speed = self.truck.speed_mph();
                if before.limit_mph - self.zones[i].limit_mph < LIMIT_DROP_WARN_MIN_DELTA_MPH
                    || speed <= self.zones[i].limit_mph + PACENOTE_MARGIN_MPH
                    || self.zones[i].start_mi - pos > self.limit_drop_warning_lead_mi(speed)
                {
                    continue;
                }
            }
            if self
                .announced_zone_warnings
                .contains(&zone_key(&self.zones[i]))
            {
                continue;
            }
            let ahead = self.zones[i].start_mi - pos;
            if !(ZONE_WARNING_MIN_MI < ahead && ahead <= lookahead) {
                continue;
            }
            if !self.zone_is_active_index(i) {
                continue;
            }
            if self
                .zones
                .iter()
                .any(|previous| self.same_traffic_pace(previous, &self.zones[i]))
            {
                continue;
            }
            due.push((ahead, i));
        }
        // One warning OUTSTANDING at a time, not one per frame.
        if let Some(pending) = self.pending_zone_warning {
            if pos < pending {
                return;
            }
        }
        // Never a heads-up in the same breath as an arrival. Held, not lost.
        let now = self.active_zone_at(pos);
        let entering = !Self::same_zone(now.as_ref(), self.entered_zone.as_ref());
        if !due.is_empty() && !entering {
            let mut best = due[0];
            for pair in &due[1..] {
                if pair.0 < best.0 {
                    best = *pair;
                }
            }
            let (ahead, i) = best;
            let zone = self.zones[i].clone();
            self.announced_zone_warnings.insert(zone_key(&zone));
            self.pending_zone_warning = Some(zone.start_mi);
            let message = SpokenMessage::new(self.zone_warning_message(&zone, ahead));
            self.emit(
                TripEventKind::GpsCue,
                message,
                TripEventData {
                    zone: Some(zone),
                    ..Default::default()
                },
            );
        }
        let zone = self.active_zone_at(pos);
        if !Self::same_zone(zone.as_ref(), self.entered_zone.as_ref()) {
            if let Some(zone) = zone.clone() {
                if zone.reason == "construction" {
                    self.construction_zone_grace_start
                        .insert(zone_key(&zone), zone.start_mi);
                }
                // Holding the colour line back is only safe when the number
                // is not changing for the worse: an entry that CUTS the limit
                // currently in force never waits.
                let old_limit = match self.entered_zone.as_ref() {
                    Some(entered) => {
                        let key = zone_key(entered);
                        Some(
                            self.zones
                                .iter()
                                .find(|z| zone_key(z) == key)
                                .map(|z| z.limit_mph)
                                .unwrap_or(entered.limit_mph),
                        )
                    }
                    None => self.announced_speed_limit,
                };
                let already_spoken = self.zone_entry_spoken
                    && self
                        .entered_zone
                        .as_ref()
                        .is_some_and(|previous| self.same_traffic_pace(previous, &zone));
                let urgent = old_limit.is_some_and(|old| zone.limit_mph < old);
                if self.yard_entry_held && Self::is_yard_like(&zone) {
                    // Not in until the driveway turn is settled: gated.
                    self.zone_entry_spoken = false;
                } else if already_spoken {
                    // Keep the local section and its queue lifecycle, without
                    // repeating the speed the driver is already following.
                } else if urgent || self.event_breather.ready("zone") {
                    self.speak_zone_entry(&zone);
                } else {
                    // Gated, not dropped.
                    self.zone_entry_spoken = false;
                }
                if zone.reason == "heavy traffic" && zone.aadt.is_some() {
                    // Fill the jam with slow metal -- which disperses past
                    // the zone's end rather than living at jam pace forever.
                    self.traffic_manager.inject_congestion(
                        zone.start_mi,
                        zone.end_mi,
                        zone.limit_mph,
                        pos,
                    );
                }
            } else if let Some(previous) = self.entered_zone.clone() {
                self.construction_zone_grace_start
                    .remove(&zone_key(&previous));
                let resumed = self.corridor_limit_at(pos);
                // A rise shares the arrival line's window: a short zone ended
                // right behind "Speed limit reduced to 45", and two numbers
                // in one breath outran the voice (player report, 2026-10-17).
                // Gated, not lost: the open-road check names the limit in
                // force once the window opens, against the zone's number the
                // driver last heard.
                let rise = resumed > previous.limit_mph;
                if rise && !self.event_breather.ready("limit") {
                    if self.zone_entry_spoken {
                        self.announced_speed_limit = Some(previous.limit_mph);
                    }
                } else {
                    self.announced_speed_limit = Some(resumed);
                    self.event_breather.spoke("limit");
                    let message = if super::streets::is_street_zone_reason(&previous.reason) {
                        format!("Speed limit {}.", self.speed_value(resumed))
                    } else {
                        format!(
                            "End of {} zone. Speed limit {}.",
                            previous.reason,
                            self.speed_value(resumed)
                        )
                    };
                    self.emit(
                        TripEventKind::ZoneExit,
                        SpokenMessage::new(message),
                        TripEventData::default(),
                    );
                }
                self.zone_entry_spoken = true;
            }
            self.entered_zone = zone;
        } else if let Some(zone) = zone {
            let held = self.yard_entry_held && Self::is_yard_like(&zone);
            if !held && !self.zone_entry_spoken && self.event_breather.ready("zone") {
                // Self-supersede: this zone is still the one governing the
                // truck and its own entry was gated when it started.
                self.speak_zone_entry(&zone);
            }
        }
    }

    fn is_yard_like(zone: &Zone) -> bool {
        matches!(
            zone.reason.as_str(),
            super::streets::YARD_ZONE | super::streets::LOT_ZONE
        )
    }

    pub fn speak_zone_entry(&mut self, zone: &Zone) {
        let quiet = zone.reason == "construction"
            && self.zones.iter().any(|z| {
                z.reason == "construction merge" && (z.end_mi - zone.start_mi).abs() < 0.01
            });
        self.event_breather.spoke("zone");
        let message = SpokenMessage::new(self.zone_entry_message(zone));
        self.emit(
            TripEventKind::ZoneEnter,
            message,
            TripEventData {
                zone: Some(zone.clone()),
                suppress_sound: Some(quiet),
                ..Default::default()
            },
        );
        self.zone_entry_spoken = true;
    }

    /// Announce a clock change the moment the truck passes a zone boundary.
    pub fn check_timezone(&mut self) {
        let zone = self.timezone_at(self.position_mi);
        if zone.key == self.last_timezone.key {
            return;
        }
        let previous = self.last_timezone;
        self.last_timezone = zone;
        // The new local time is the whole message.
        let message = format!(
            "Crossing into {}. It is now {}.",
            zone.name,
            clock_text(self.local_hour())
        );
        self.emit(
            TripEventKind::TimezoneCrossing,
            SpokenMessage::new(message),
            TripEventData {
                from_zone: Some(previous),
                to_zone: Some(zone),
                ..Default::default()
            },
        );
    }

    /// Announce a changed posted limit on the open road. While a zone is
    /// active the zone owns the spoken limit, so this stays quiet.
    pub fn check_speed_limit(&mut self) {
        if self.entered_zone.is_some() {
            return;
        }
        let limit = self.corridor_limit_at(self.position_mi);
        let Some(announced) = self.announced_speed_limit else {
            self.announced_speed_limit = Some(limit); // seed at departure, no cue
            return;
        };
        if limit == announced {
            return;
        }
        let lowered = limit < announced;
        // Routine changes breathe; a serious unannounced drop the truck is
        // actually over does not wait.
        let urgent = lowered
            && announced - limit > 10.0
            && self.truck.speed_mph() > limit + PACENOTE_MARGIN_MPH
            && !self.limit_drop_preannounced.contains(&round_py_n(limit, 1));
        if !urgent && !self.event_breather.ready("limit") {
            return; // untouched state; the next check self-supersedes
        }
        if lowered {
            // The advance pacenote or an assist's "easing to X" line may
            // already have named this exact number (owner, 2026-08-12). The
            // driver heard it, so the rise out of it is told too.
            let key = round_py_n(limit, 1);
            if let Some(pos) = self.limit_drop_preannounced.iter().position(|v| *v == key) {
                self.limit_drop_preannounced.remove(pos);
                self.unannounced_limit_end_mi = None;
                self.announced_speed_limit = Some(limit);
                return;
            }
        }
        let length = self.limit_zone_length(limit);
        let message = self.limit_arrival_message(limit, lowered, length);
        // A posting the truck leaves before its line could be spoken is not
        // spoken: at 10x a third of the map's postings last a mile or less,
        // and "reduced to 45" for one ended after the truck was back at 55
        // (player report, 2026-10-17). State stays untouched, so a short
        // 45 between two 55s says nothing and 55, 45, 65 says 65. Speeding
        // through a lower posting nobody named is never charged.
        if let Some(length) = length.filter(|l| self.posting_outruns_line(*l, &message)) {
            if lowered {
                self.unannounced_limit_end_mi = Some(self.position_mi + length);
            }
            return;
        }
        self.unannounced_limit_end_mi = None;
        self.announced_speed_limit = Some(limit);
        self.event_breather.spoke("limit");
        self.emit(
            TripEventKind::GpsCue,
            SpokenMessage::new(message),
            TripEventData {
                // The road's state, not a turn to act on.
                limit_change: Some(true),
                limit_mph: Some(limit),
                ..Default::default()
            },
        );
    }

    /// Whether the truck is inside a lower posting it passed too fast to be
    /// told about: no speed is charged against it, and the dash stays quiet.
    pub fn in_unannounced_posting(&self) -> bool {
        self.unannounced_limit_end_mi
            .is_some_and(|end| self.position_mi <= end)
    }

    /// "Speed limit reduced to 45 approaching Abilene for 3 tenths of a
    /// mile." `length` is how far the new limit holds, when the scan found
    /// its end.
    fn limit_arrival_message(&self, limit: f64, lowered: bool, length: Option<f64>) -> String {
        let verb = if lowered { "reduced" } else { "raised" };
        let near = if lowered {
            self.nearest_urban_city(self.position_mi)
        } else {
            None
        };
        let mut where_ = String::new();
        if let Some((city, city_mp)) = near {
            // A drop while pulling AWAY from town is the road's doing, not
            // the town's (owner-found live, 2026-07-20).
            let direction = if city_mp >= self.position_mi {
                "approaching"
            } else {
                "leaving"
            };
            where_ = format!(" {direction} {}", self.world.spoken_city(&city, None));
        } else if lowered {
            where_ = self.lowered_limit_reason();
        }
        // A short lower zone is a passing event, not a new cruising speed.
        let mut span = String::new();
        if lowered {
            if let Some(length) = length {
                if length <= LIMIT_SHORT_ZONE_MI {
                    span = format!(" for {}", spoken_short_miles(length, self.imperial()));
                }
            }
        }
        format!(
            "Speed limit {verb} to {}{where_}{span}.",
            self.speed_value(limit)
        )
    }

    /// Why a drop with no city to blame is happening: a road stop just
    /// ahead, then a real downgrade just ahead. Bare when neither applies.
    pub fn lowered_limit_reason(&self) -> String {
        let stop_reason = self.lowered_limit_stop_reason();
        if !stop_reason.is_empty() {
            return stop_reason;
        }
        if self.lowered_limit_downgrade_ahead() {
            return " for the downhill grade".to_string();
        }
        String::new()
    }

    /// A road stop that plausibly explains a lower posting, ahead only.
    pub fn lowered_limit_stop_reason(&self) -> String {
        let end = self.position_mi + LIMIT_REASON_LOOKAHEAD_MI;
        for stop in &self.stops {
            if self.position_mi <= stop.at_mi && stop.at_mi <= end {
                if let Some(reason) = limit_reason_by_stop_type(&stop.stop_type) {
                    return reason.to_string();
                }
            }
        }
        String::new()
    }

    /// Whether a sustained downgrade starts here -- steep enough on average
    /// over the next half mile to be the road's own reason.
    pub fn lowered_limit_downgrade_ahead(&self) -> bool {
        let mi = self.position_mi;
        let end = self.total_miles().min(mi + LIMIT_DOWNGRADE_MIN_MI);
        if end - mi < LIMIT_DOWNGRADE_MIN_MI {
            return false; // not enough road left to call the downgrade sustained
        }
        let mut samples = Vec::new();
        let mut probe = mi;
        while probe <= end {
            samples.push(self.grade_at(probe) * 100.0);
            probe += LIMIT_SCAN_STRIDE_MI;
        }
        (samples.iter().sum::<f64>() / samples.len() as f64) <= LIMIT_DOWNGRADE_PCT
    }

    /// How far the just-entered corridor limit holds from the current
    /// position, or `None` when it outlasts the scan cap.
    pub fn limit_zone_length(&self, limit: f64) -> Option<f64> {
        let start_mi = self.position_mi;
        let mut mi = start_mi;
        let end = self.total_miles().min(mi + LIMIT_SCAN_MAX_MI);
        while mi < end {
            mi = end.min(mi + LIMIT_SCAN_STRIDE_MI);
            if self.corridor_limit_at(mi) != limit {
                return Some(mi - start_mi);
            }
        }
        None
    }

    /// Whether a posting `length` miles long goes by, at the truck's pace,
    /// before `message` could be spoken.
    fn posting_outruns_line(&self, length: f64, message: &str) -> bool {
        let mi_per_real_s = self.truck.speed_mph() * self.effective_time_scale() / 3600.0;
        let speaking_s = message.chars().count() as f64 / SPOKEN_CHARS_PER_REAL_S;
        mi_per_real_s > 0.0 && length / mi_per_real_s < speaking_s
    }

    /// Lead distance for the "drops to X" pacenote, in `LIMIT_WARNING_REAL_S`
    /// real seconds at the current pace.
    pub fn limit_drop_warning_lead_mi(&self, speed: f64) -> f64 {
        let speed = speed.max(1.0);
        let miles = LIMIT_WARNING_REAL_S * speed * self.effective_time_scale() / 3600.0;
        PACENOTE_MIN_LEAD_MI.max(miles.min(LIMIT_WARNING_MAX_LEAD_MI))
    }

    /// Record that an assist just spoke the incoming posted limit itself, so
    /// the plain arrival confirmation does not repeat the same number.
    pub fn note_limit_preannounced(&mut self, limit_mph: f64) {
        let key = round_py_n(limit_mph, 1);
        if !self.limit_drop_preannounced.contains(&key) {
            self.limit_drop_preannounced.push(key);
        }
    }

    /// The next corridor limit change ahead, when it is a warn-worthy drop:
    /// `(boundary_mi, new_limit)` for the FIRST change inside the pacenote
    /// window. The scan walks an ABSOLUTE grid so the dedup key stays stable
    /// however the frames land (owner log, 2026-07-23).
    pub fn next_limit_drop(&self) -> Option<(f64, f64)> {
        let current = self.corridor_limit_at(self.position_mi);
        let mut prev = (self.position_mi / LIMIT_SCAN_STRIDE_MI).floor() * LIMIT_SCAN_STRIDE_MI;
        let end = self
            .total_miles()
            .min(self.position_mi + LIMIT_WARNING_MAX_LEAD_MI);
        while prev < end {
            let mi = end.min(prev + LIMIT_SCAN_STRIDE_MI);
            let limit = self.corridor_limit_at(mi);
            if limit != current {
                if current - limit < LIMIT_DROP_WARN_MIN_DELTA_MPH {
                    return None;
                }
                let mut boundary = mi;
                // Anchor the fine probe to ABSOLUTE hundredth-mile marks.
                let mut probe = (prev * 100.0).floor() / 100.0;
                while probe < mi {
                    probe += 0.01;
                    if self.corridor_limit_at(probe) != current {
                        boundary = probe;
                        break;
                    }
                }
                return Some((round_py_n(boundary, 2), limit));
            }
            prev = mi;
        }
        None
    }

    /// Warn before a big posted-limit drop, like a curve pacenote.
    pub fn check_limit_drop_ahead(&mut self) {
        if self.entered_zone.is_some() || self.is_facility_approach_route() {
            return;
        }
        let Some((boundary_mi, limit)) = self.next_limit_drop() else {
            return;
        };
        if self.warned_limit_drops.contains(&boundary_mi) {
            return;
        }
        let speed = self.truck.speed_mph();
        if speed <= limit + PACENOTE_MARGIN_MPH {
            return;
        }
        let ahead = boundary_mi - self.position_mi;
        if ahead > self.limit_drop_warning_lead_mi(speed) {
            return;
        }
        self.warned_limit_drops.push(boundary_mi);
        let message = format!(
            "Speed limit drops to {} in {}.",
            self.speed_value(limit),
            spoken_short_miles(ahead, self.imperial())
        );
        let key = round_py_n(limit, 1);
        if !self.limit_drop_preannounced.contains(&key) {
            self.limit_drop_preannounced.push(key);
        }
        self.emit(
            TripEventKind::GpsCue,
            SpokenMessage::new(message),
            TripEventData {
                limit_change: Some(true),
                ..Default::default()
            },
        );
    }
}
