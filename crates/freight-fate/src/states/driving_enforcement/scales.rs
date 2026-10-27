//! Weigh stations: whether one is open, how far out its notice reads, the
//! last reminder and the real-time hold it starts, the re-announcement after
//! a stop or a pause, who owns the exit key, and the screening draw at the
//! scale house.

use ff_core::models::business::has_weigh_station_transponder;
use ff_core::models::safety_record::{
    inspection_selection_chance, refresh_selection_score, safety_record_text,
};
use ff_core::pyrandom::PyRandom;
use ff_core::settings::short_distance_text_for;
use ff_core::sim::enforcement_posts::{post_seed, KIND_FIXED_SCALE};
use ff_core::sim::trip_models::{RoadStop, ENFORCEMENT_WARNING_MAX_MI, SCALE_WARNING_REAL_S};
use ff_core::speech_pacing::SpeechCategory;

use crate::app::{GameContext, SayEvent};
use crate::states::driving::DrivingState;
use crate::states::driving_core::*;
use crate::states::driving_updates::live;

use super::{
    SCALE_NOTICE_SAMPLE, SCALE_REMINDER_REAL_LEAD_S, WEIGH_STATION_REMINDER_MAX_MI,
    WEIGH_STATION_REMINDER_MI,
};

impl DrivingState {
    /// Whether this weigh station is open today.
    ///
    /// Settled once when the trip was built, from a named seeded draw over the
    /// stop's own key, so a reload cannot reopen a dark scale.
    pub fn scale_is_open(&self, stop: &RoadStop) -> bool {
        let key = stop.key();
        for post in &self.trip.posts {
            if post.anchor == key {
                return post.kind == KIND_FIXED_SCALE;
            }
        }
        false
    }

    /// Lead distance for the open-scale call, sized in real seconds.
    ///
    /// An open scale costs money and time, so it gets a longer lead than a
    /// heads-up does -- and the lead is derived from the actual sentence, not a
    /// constant, because enforcement lines differ a lot in length.
    pub fn scale_notice_lookahead_mi(&self, ctx: &GameContext) -> f64 {
        let speed = self.trip.truck.speed_mph().max(1.0);
        let seconds =
            SCALE_WARNING_REAL_S.max(self.pull_over_grace_seconds(ctx, SCALE_NOTICE_SAMPLE));
        let miles = seconds * speed * self.trip.effective_time_scale() / 3600.0;
        WEIGH_STATION_NOTICE_MI.max(miles.min(ENFORCEMENT_WARNING_MAX_MI))
    }

    /// The nearest open weigh station strictly ahead, or None.
    ///
    /// Returns `(stop, ahead_mi)`. A closed scale never matches -- its guards
    /// must stay inert so the silence-means-closed rule holds.
    pub fn open_scale_ahead(&self, within_mi: f64) -> Option<(RoadStop, f64)> {
        let mut best: Option<(RoadStop, f64)> = None;
        for stop in &self.trip.stops {
            if stop.stop_type != "weigh_station" {
                continue;
            }
            let ahead = stop.at_mi - self.trip.position_mi;
            if 0.0 < ahead
                && ahead <= within_mi
                && self.scale_is_open(stop)
                && best
                    .as_ref()
                    .is_none_or(|(_, best_ahead)| ahead < *best_ahead)
            {
                best = Some((stop.clone(), ahead));
            }
        }
        best
    }

    /// Whether this open scale wants the truck in. A transponder's green
    /// light clears it past (rolled here if the notice has not rolled it
    /// yet: the verdict is seeded and stored, so it is the same one the
    /// notice speaks); a red light, or no transponder, means pull in.
    pub fn scale_wants_truck(&mut self, ctx: &GameContext, stop: &RoadStop) -> bool {
        let transponder = ctx
            .profile
            .as_ref()
            .is_some_and(has_weigh_station_transponder);
        if !transponder {
            return true;
        }
        let key = self.weigh_station_key(stop);
        self.roll_transponder_verdict(stop, &key) != "green"
    }

    /// The nearest open scale strictly ahead that wants the truck in: the
    /// one the exit key and the co-driver answer to. A scale the
    /// transponder cleared is passed on the highway, as the rules allow.
    pub fn check_in_scale_ahead(
        &mut self,
        ctx: &GameContext,
        within_mi: f64,
    ) -> Option<(RoadStop, f64)> {
        let mut scales: Vec<(RoadStop, f64)> = self
            .trip
            .stops
            .iter()
            .filter(|stop| stop.stop_type == "weigh_station" && self.scale_is_open(stop))
            .map(|stop| (stop.clone(), stop.at_mi - self.trip.position_mi))
            .filter(|(_, ahead)| 0.0 < *ahead && *ahead <= within_mi)
            .collect();
        scales.sort_by(|a, b| a.1.total_cmp(&b.1));
        scales
            .into_iter()
            .find(|(stop, _)| self.scale_wants_truck(ctx, stop))
    }

    /// How far out the last reminder speaks, sized in real seconds.
    ///
    /// The reminder starts the real-time hold (`Trip::scale_reminder_hold`),
    /// so the clock that runs over this window is the real one, not the
    /// trip's compressed pacing: the road it must cover is speed times that
    /// clock times [`SCALE_REMINDER_REAL_LEAD_S`]. At every legal truck speed
    /// that is under the half-mile floor (61 mph covers half a mile in about
    /// thirty real seconds), so the window only grows for a truck going
    /// faster than half a mile can hold, and never past
    /// [`WEIGH_STATION_REMINDER_MAX_MI`].
    pub fn scale_reminder_mi(&self) -> f64 {
        let speed = self.trip.truck.speed_mph().max(1.0);
        let held_clock = self.trip.time_scale.min(1.0);
        let miles = speed * held_clock * SCALE_REMINDER_REAL_LEAD_S / 3600.0;
        miles.clamp(WEIGH_STATION_REMINDER_MI, WEIGH_STATION_REMINDER_MAX_MI)
    }

    /// Whether a crossing of this open scale at speed may be charged.
    ///
    /// Audible before it can bite. Once the last reminder has spoken for this
    /// scale, the driver gets [`SCALE_REMINDER_REAL_LEAD_S`] of real driving
    /// to act on it before a crossing counts; a pause does not count toward
    /// it, the drive is not running. With no reminder, the full notice is
    /// what told the driver: a reminder skipped because they were slow or
    /// signalled until the gore leaves the bypass theirs, but a scale never
    /// announced at all is the game's miss.
    ///
    /// The real seconds only excuse a reminder the GAME made late
    /// (`scale_reminder_held_by_game`). A tester crawled at fourteen to 0.06
    /// of a mile, under the bypass speed so the reminder stayed quiet, then
    /// crossed at forty-five: the reminder spoke seven real seconds out and
    /// the crossing was excused for good (QA, 2026-10-07). The window was
    /// half a mile of real clock -- thirty seconds at any legal speed -- and
    /// the driver spent it under fifteen, after a notice that already said
    /// pull in. That reminder is late by the driver's own pace, so the
    /// crossing is judged as if it had never been needed.
    pub fn scale_bypass_judgeable(&self, key: &str) -> bool {
        if self.weigh_station_reminder_key == key {
            self.weigh_station_reminder_age_s >= SCALE_REMINDER_REAL_LEAD_S
                || !self.scale_reminder_held_by_game.contains(key)
        } else {
            self.weigh_station_noticed.contains(key)
        }
    }

    /// The cab is taken while the truck is inside an open scale's reminder
    /// window: the reminder that follows is the game's late, not the
    /// driver's.
    ///
    /// Runs on the frames the scale check is held off (a pull-over, a ramp,
    /// a hazard or microsleep window, the arrival menu) and on a departure
    /// lane, where a truck pulling out of a facility is slow because it is
    /// supposed to be. Only announced, open scales; a green transponder
    /// light needs no reminder at all.
    ///
    /// A reminder already spoken counts too. QA (2026-10-07): the reminder
    /// spoke half a mile out, a hazard held the cab from 0.48 to 0.15 of a
    /// mile, and the crossing was charged -- the hazard had eaten most of
    /// the real seconds the reminder promised, and those seconds were never
    /// the driver's to use. The reminder's age only counts seconds the cab
    /// was free, so the excuse lasts exactly as long as the driver is owed.
    /// One the driver's own crawl or signal made late
    /// (`scale_reminder_late_by_driver`) stays theirs: a hazard after it,
    /// whether before or after the reminder finally speaks, does not reopen
    /// the crawl-then-speed dodge.
    pub fn note_scale_reminders_held_by_game(&mut self) {
        let window = self.scale_reminder_mi();
        let held: Vec<String> = self
            .trip
            .stops
            .iter()
            .filter(|stop| stop.stop_type == "weigh_station")
            .filter(|stop| {
                let ahead = stop.at_mi - self.trip.position_mi;
                0.0 < ahead && ahead <= window && self.scale_is_open(stop)
            })
            .map(|stop| self.weigh_station_key(stop))
            .filter(|key| {
                self.weigh_station_noticed.contains(key)
                    && !self.scale_reminder_late_by_driver.contains(key)
                    && self
                        .weigh_station_transponder_verdict
                        .get(key)
                        .map(String::as_str)
                        != Some("green")
            })
            .collect();
        self.scale_reminder_held_by_game.extend(held);
    }

    /// One short line before the bypass point, if nothing has changed.
    ///
    /// The full notice latches miles out; between it and the gore the old
    /// build said nothing at all, so a driver who mis-read the instruction
    /// crossed at speed in silence. Fires once per scale, only while the truck
    /// is still over the bypass speed with no scale exit armed.
    ///
    /// From this line to the gore the clock runs real
    /// (`Trip::scale_reminder_hold_mi`): a fixed half mile on the compressed
    /// clock was three real seconds at ten times speed, and the bypass charge
    /// landed before the driver could reach the key (tester log, 2026-10-07).
    pub fn check_scale_reminder(
        &mut self,
        ctx: &mut GameContext,
        stop: &RoadStop,
        ahead: f64,
        key: &str,
    ) {
        if !(0.0 < ahead && ahead <= self.scale_reminder_mi()) {
            return;
        }
        if !self.weigh_station_noticed.contains(key) || key == self.weigh_station_reminder_key {
            return;
        }
        if self.trip.truck.speed_mph() <= WEIGH_STATION_BYPASS_MPH || self.exit_is_armed_for(stop) {
            // Inside the window and held quiet by the driver's own pace or
            // signal: whatever this reminder's lateness costs is theirs, even
            // if the cab is taken after it finally speaks.
            self.scale_reminder_late_by_driver.insert(key.to_string());
            return;
        }
        if self
            .weigh_station_transponder_verdict
            .get(key)
            .map(String::as_str)
            == Some("green")
        {
            // A weigh-in-motion green light already told this driver they
            // need no exit for this scale. "Signal for the scale exit" would
            // contradict that, so the reminder stays silent rather than
            // reintroducing an instruction the verdict just retired.
            return;
        }
        self.weigh_station_reminder_key = key.to_string();
        self.weigh_station_reminder_age_s = 0.0;
        self.trip.scale_reminder_hold_mi = Some(stop.at_mi);
        // The distance was hard-coded to the threshold, so a reminder that
        // fired at two hundred yards still announced "half a mile" -- and
        // after the approach line above was fixed to speak a real short
        // distance, the pair read as the scale moving further away while the
        // truck closed on it (gate harness, 2026-08-15). Speak the road that
        // is actually left.
        // valid: an instruction to signal for an exit dies the moment the
        // exit is behind the truck. Without this, a rescue after a chain of
        // failure-to-stop warnings replayed "Signal for the scale exit" when
        // there was no exit left to take (21 August build note).
        //
        // Rust: `valid` outlives the borrow of `self`, so it reads the drive
        // through `live`, exactly as the red-light line beside it does. It
        // used to project the road left onto the wall clock instead, which
        // answered "still ahead" for a truck that had sped up past the gore
        // (and for the whole of any run the wall clock outpaces) -- the
        // adversarial battery heard the reminder replayed after the bypass
        // charge had already been written.
        let scale_mi = stop.at_mi;
        // Named: with another scale anywhere near -- a closed one first, or
        // a second open one inside the lookahead -- "Weigh station in half a
        // mile" did not say which, and a tester checked in at the wrong one.
        let text = format!(
            "{} in {}. Signal for the scale exit.",
            stop.name,
            ctx.settings.short_distance_text(ahead)
        );
        self.refresh_live_facts();
        ctx.say_event_with(
            text,
            SayEvent::queued()
                .priority(EventPriority::Important)
                .category(SpeechCategory::Navigation)
                .valid(move || live::position_mi() < scale_mi),
        );
    }

    /// Ask for the open scale ahead to be re-announced once the cab is free.
    ///
    /// Called when a pause ends or the driver leaves a stop. `here` names the
    /// weigh station being left, if the stop was one, so the line can say
    /// which scale that was and which one is still to come.
    ///
    /// Only when a scale has already been announced: one that first comes
    /// into range after the stop gets its full notice, and a re-announcement
    /// on top of it would say the same thing twice.
    pub fn note_scale_reannounce(&mut self, here: Option<&str>) {
        self.scale_reannounce = self
            .announced_open_scale_ahead()
            .map(|_| here.unwrap_or_default().to_string());
    }

    /// The nearest announced open scale still ahead, or None.
    ///
    /// Only a scale whose notice has been spoken: one inside its lookahead
    /// but not yet announced gets its full notice from the enforcement check,
    /// and repeating a line nobody heard the first time is not a reminder.
    pub fn announced_open_scale_ahead(&self) -> Option<(RoadStop, f64)> {
        self.trip
            .stops
            .iter()
            .filter(|stop| stop.stop_type == "weigh_station")
            .map(|stop| (stop.clone(), stop.at_mi - self.trip.position_mi))
            .filter(|(stop, ahead)| {
                let key = self.weigh_station_key(stop);
                *ahead > 0.0
                    && self.weigh_station_noticed.contains(&key)
                    && self.scale_is_open(stop)
                    && !self.enforcement_events.contains(&key)
                    && self
                        .weigh_station_transponder_verdict
                        .get(&key)
                        .map(String::as_str)
                        != Some("green")
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
    }

    /// After a check-in, a stop or a pause: the open scale still ahead.
    ///
    /// A tester checked in at a closed scale six miles short of the open one
    /// he had been told about, was waved off, took a nine-minute pause, and
    /// heard nothing more about the open scale until three seconds before
    /// its bypass charge (log, 2026-10-07). The notice had been spoken once,
    /// long before; nothing said that the scale he stopped at was not it.
    ///
    /// One line on the driving channel, so the ladder, the pacer and the
    /// duck apply as they do to the notice itself. Waits while the cab has
    /// another demand on it; dropped once the scale is behind or settled.
    pub fn update_scale_reannounce(&mut self, ctx: &mut GameContext) {
        if self.scale_reannounce.is_none() {
            return;
        }
        if self.enforcement_bypassed(ctx) {
            self.scale_reannounce = None;
            return;
        }
        if self.enforcement_busy() {
            return;
        }
        let here = self.scale_reannounce.take().unwrap_or_default();
        let Some((stop, ahead)) = self.announced_open_scale_ahead() else {
            return;
        };
        let name = stop.spoken_name();
        if name == here {
            return;
        }
        let announced = ctx.settings.short_distance_text(ahead);
        let lead = if here.is_empty() {
            String::new()
        } else {
            format!("That was {here}. ")
        };
        // The reminder speaks once per scale. A driver who already heard it
        // and then lost the thread across the stop gets the instruction back
        // with the distance; one who has not yet reached it will hear it there.
        let key = self.weigh_station_key(&stop);
        let instruction =
            if self.weigh_station_reminder_key == key && !self.exit_is_armed_for(&stop) {
                " Signal for the scale exit."
            } else {
                ""
            };
        let scale_mi = stop.at_mi;
        let imperial = ctx.settings.imperial_units;
        self.refresh_live_facts();
        ctx.say_event_with(
            format!("{lead}{name} is still ahead, open, {announced}.{instruction}"),
            SayEvent::queued()
                .priority(EventPriority::Important)
                .category(SpeechCategory::Navigation)
                // The same gate as the notice: a distance is a claim about
                // now, and the line dies once its own words stop being true.
                .valid(move || {
                    let left = scale_mi - live::position_mi();
                    left > 0.0 && short_distance_text_for(left, imperial) == announced
                }),
        );
    }

    /// An open scale ahead owns the next exit; rest planning waits.
    ///
    /// The old announcement told the driver to press the rest key, and the
    /// rest key at speed planned a sleep stop PAST the scale -- two
    /// instructions marching the player into a bypass charge. Says what comes
    /// first and changes nothing; repeats dedupe in the pacer.
    pub fn scale_outranks_rest_planning(&mut self, ctx: &mut GameContext) -> bool {
        if self.enforcement_bypassed(ctx) || self.ramp_mi.is_some() {
            return false;
        }
        let window = self
            .scale_notice_lookahead_mi(ctx)
            .max(self.exit_window_mi());
        let Some((stop, ahead)) = self.check_in_scale_ahead(ctx, window) else {
            return false;
        };
        let distance = ctx.settings.distance_text(ahead, true);
        let text = format!(
            "Weigh station first, {}, {distance} ahead. All trucks must stop.",
            stop.name
        );
        ctx.say_event_with(
            text,
            SayEvent::queued()
                .priority(EventPriority::Important)
                .category(SpeechCategory::Navigation),
        );
        true
    }

    /// The open scale that outranks `stop` for the exit key, or None.
    ///
    /// An exit press with an open scale nearer than the chosen stop belongs to
    /// the scale: arming the farther ramp is exactly the move that carried a
    /// tester past the inspection lane unarmed.
    pub fn scale_claiming_exit(
        &mut self,
        ctx: &mut GameContext,
        stop: Option<&RoadStop>,
    ) -> Option<RoadStop> {
        if self.enforcement_bypassed(ctx) {
            return None;
        }
        let window = self.exit_window_mi();
        let (scale, _) = self.check_in_scale_ahead(ctx, window)?;
        match stop {
            // nothing outranked; the normal arming handles it
            None => None,
            Some(stop) if stop.key() == scale.key() => None,
            // the chosen stop comes first anyway
            Some(stop) if stop.at_mi <= scale.at_mi => None,
            Some(_) => Some(scale),
        }
    }

    /// One demand at a time: a beginning pull-over owns the road.
    ///
    /// An exit armed for a ramp kept announcing and steering for it while the
    /// trooper stop was running, which turned one mistake into a
    /// failure-to-stop cascade. Returns True when something actually stood
    /// down; the plan itself stays on the route map.
    pub fn stand_down_exit_for_stop(&mut self, _ctx: &mut GameContext) -> bool {
        let had_exit = self.exit_signal_on || self.exit_stop.is_some();
        if !had_exit {
            return false;
        }
        let stop = self.exit_stop.take();
        self.exit_signal_on = false;
        self.exit_signal_canceled = false;
        self.cruise_exit_mph = None;
        self.destination_exit_response_s = 0.0;
        self.reset_exit_lane_state();
        if stop.is_some() && self.is_selected_stop(stop.as_ref()) {
            self.clear_selected_stop_intent();
        }
        true
    }

    // -- scale screening -----------------------------------------------------

    /// Whether an open scale sends this truck to the inspection lane.
    ///
    /// The safety record is the dial. A clean career is waved through nearly
    /// every time; a career carrying citations, out-of-service history and a
    /// damaged truck is pulled in at every open scale, which is what makes a
    /// dirty record feel relentless without the game inventing bad luck.
    pub fn scale_selects_driver(&self, ctx: &mut GameContext, stop: &RoadStop) -> bool {
        let damage_pct = self.trip.truck.damage_pct;
        let relaxed = ctx.settings.hos_mode == "relaxed";
        let Some(profile) = ctx.profile.as_mut() else {
            return false;
        };
        let score = refresh_selection_score(profile, damage_pct);
        let mut chance = inspection_selection_chance(score);
        if relaxed {
            chance *= hos::RELAXED_INSPECTION_SCALE;
        }
        let key = post_seed(
            Some(self.trip_seed),
            &format!("scale:{}", stop.key()),
            "select",
        );
        PyRandom::new_from_str(&key).random() < chance
    }

    /// The spoken safety-record line, for the driver's own status readout.
    pub fn safety_record_line(&self, ctx: &mut GameContext) -> String {
        let damage_pct = self.trip.truck.damage_pct;
        let Some(profile) = ctx.profile.as_mut() else {
            return "Safety record: clean. Inspectors have no reason to pull you in.".to_string();
        };
        let score = refresh_selection_score(profile, damage_pct);
        safety_record_text(score).to_string()
    }
}
