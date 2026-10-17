//! The clock key and the three Alt hours keys, plus the shoulder-sleep
//! question the pause menu and the rest key both ask.
//!
//! Alt A, Alt S, and Alt D split the three hours numbers a driver plans around
//! out of the C readout, one key each, left to right in the shape of a shift:
//! what is behind you, what stops you next, what ends the day. The Alt chord
//! keeps the right hand on the arrows, where the accelerator is, and a slipped
//! modifier lands on A, S, or D -- all spoken info, nothing that moves the
//! truck. Controllers keep the combined readout on D-pad right; a pad has
//! nowhere to put three more info buttons.

use ff_core::models::jobs::route_drive_hours;
use ff_core::sim::trip_models::RoadStop;

use crate::app::GameContext;
use crate::bindings::Action;
use crate::states::driving::DrivingState;
use crate::states::driving_core::*;

/// The arrival estimate's pace for a truck not yet moving on the highway:
/// the same typical pace the stop list's estimate names.
const HIGHWAY_PACE_MPH: f64 = crate::states::driving_stop_detail::FALLBACK_MPH;
/// The same for the road into the facility. Assumed, not measured: the
/// street-limit bake's own fallback for a street it has no statute for.
const STREET_PACE_MPH: f64 = ff_core::data::street_limits::FALLBACK_MPH;

impl DrivingState {
    /// The spoken clock is anchored to the computer when Real time is
    /// selected.  Calling that initial wall-clock reading a route timezone
    /// would be misleading when the computer is elsewhere, so name it as the
    /// game's local clock.  Accelerated modes retain the geographic label.
    /// It follows the pace the drive is running: a pace chosen while rolling
    /// waits for the next stop, and so does its clock.
    pub fn clock_zone_label(&self, _ctx: &GameContext) -> &'static str {
        if self.trip.time_scale == 1.0 {
            "local game time"
        } else {
            self.trip.current_timezone().name
        }
    }

    /// `_calendar_phrase()`: calendar date and season for the spoken
    /// readouts; "" when unknown.
    pub fn calendar_phrase(&self, _ctx: &GameContext) -> String {
        let Some(date) = self.trip.weather.date_text() else {
            return String::new();
        };
        match self.trip.weather.season() {
            Some(season) => format!("{date}, {season}"),
            None => date,
        }
    }

    /// `_clock_phrase()`: "5:33 AM Eastern, March 21, spring." -- the time
    /// plus the calendar.
    pub fn clock_phrase(&self, ctx: &GameContext) -> String {
        let cal = self.calendar_phrase(ctx);
        let base = format!(
            "{} {}",
            clock_text(self.trip.local_hour()),
            self.clock_zone_label(ctx)
        );
        if cal.is_empty() {
            format!("{base}.")
        } else {
            format!("{base}, {cal}.")
        }
    }

    /// `_speak_clock(full_hours=False)`: C -- local time, then the deadline
    /// verdict, then the nearest hours limit.
    ///
    /// Ordered for braille as much as speech: a display shows one short line
    /// at a time, so the clock and the on-schedule verdict must land in the
    /// first 40 cells, with detail behind them. Terse speech drops the
    /// calendar and the appointment restatement (Tab still carries both).
    ///
    /// The hours detail moved onto Alt A, Alt S, and Alt D, but C keeps one
    /// clause for whichever limit comes first: a driver can be on schedule and
    /// out of hours at the same time, and hearing only the schedule would be
    /// the wrong half of the answer.
    ///
    /// `full_hours` keeps the whole hours-of-service report in one press for
    /// the controller's D-pad right, which has nowhere to put three more info
    /// buttons. A pad player must not lose hours of service.
    pub fn speak_clock(&mut self, ctx: &mut GameContext, full_hours: bool) {
        let hours_used = self.trip.game_minutes / 60.0;
        let terse = self.terse_speech(ctx);
        let now = if terse {
            format!(
                "{} {}.",
                clock_text(self.trip.local_hour()),
                self.clock_zone_label(ctx)
            )
        } else {
            self.clock_phrase(ctx)
        };
        let tail = if full_hours {
            let mode = ctx.settings.hos_mode.clone();
            let mut clause = hos_of(ctx).summary(&mode);
            let route = if terse {
                String::new()
            } else {
                self.hos_route_context(ctx)
            };
            if !route.is_empty() {
                clause = format!("{clause} {route}");
            }
            format!(" {clause}")
        } else {
            let clause = self.hos_nearest_clause(ctx);
            let mut tail = if clause.is_empty() {
                String::new()
            } else {
                format!(" {clause}")
            };
            tail.push_str(&self.hos_key_notice(ctx));
            tail
        };
        if self.phase == DRIVE_PHASE_PICKUP {
            let facility = self.pickup_facility_text(ctx);
            let remaining = ctx
                .settings
                .distance_text(self.trip.remaining_miles(), false);
            ctx.say(&format!("{now} Pickup {facility}, {remaining}.{tail}"));
            return;
        }
        let remaining = self.job.deadline_game_h - hours_used;
        // On the departure chain `self.trip` is the two miles of streets to
        // the on-ramp and the highway run sits parked in `highway_trip`, so
        // the chain's own estimate is the streets alone: "arrival in 0.1
        // hours" with 123 miles of interstate still to drive (agent drive,
        // Dallas to Sherman, 2026-09-11). Add the highway at its route pace,
        // the same walk the hours-of-service planner already does here.
        let highway_hours = if self.departure_chain {
            self.highway_trip.as_ref().map_or(0.0, |highway| {
                route_drive_hours(Some(&highway.route), highway.position_mi, Some(ctx.world))
            })
        } else {
            0.0
        };
        // Off the highway the road left is the gate's, not the frozen mainline
        // remainder, and a truck stopped at a ramp's stop bar is not a truck
        // doing 1 mph for the rest of the trip: "arrival in 8.6 hours at a
        // typical highway pace" two miles from the gate (agent drive B,
        // 2026-09-24). The fallback pace was the truck's own crawl, not the
        // typical pace the readout names.
        let speed = self.trip.truck.speed_mph();
        let moving = speed >= ff_core::sim::trip::ETA_MIN_MPH;
        let gate_mi = self.gate_distance_mi(ctx);
        let eta = match gate_mi {
            Some(gate) => gate / if moving { speed } else { STREET_PACE_MPH },
            None => self.trip.eta_game_hours(HIGHWAY_PACE_MPH),
        } + highway_hours;
        if remaining <= 0.0 {
            ctx.say(&format!(
                "{now} {:.1} hours late. Pay is dropping.{tail}",
                -remaining
            ));
            return;
        }
        let verdict = if eta < remaining {
            "On schedule"
        } else {
            "Running behind"
        };
        if terse {
            ctx.say(&format!(
                "{now} {verdict}: arrival in {eta:.1} hours, deadline in {remaining:.1}.{tail}"
            ));
            return;
        }
        // With the highway still ahead the estimate is mostly the route's
        // pace, whatever the streets are doing right now.
        let basis = if moving && highway_hours == 0.0 {
            "at this pace"
        } else if gate_mi.is_some() && highway_hours == 0.0 {
            "at a typical street pace"
        } else {
            "at a typical highway pace"
        };
        let appointment = deadline_appointment(self, ctx);
        ctx.say(&format!(
            "{now} {verdict}. Arrival {eta:.1} hours {basis}. Deadline {remaining:.1}, due \
             {appointment}.{tail}"
        ));
    }

    // Alt A, Alt S, and Alt D split the three hours numbers a driver plans
    // around out of the C readout, one key each, left to right in the shape of
    // a shift: what is behind you, what stops you next, what ends the day. The
    // Alt chord keeps the right hand on the arrows, where the accelerator is,
    // and a slipped modifier lands on A, S, or D -- all spoken info, nothing
    // that moves the truck. Controllers keep the combined readout on D-pad
    // right; a pad has nowhere to put three more info buttons.

    /// `_hos_nearest_clause()`: one sentence for the HOS limit that comes
    /// first, or "" when off.
    pub fn hos_nearest_clause(&self, ctx: &GameContext) -> String {
        if let Some(clause) = self.co_driver_clock_clause(ctx) {
            return clause;
        }
        let Some(limit) = hos_of(ctx).next_limit(&ctx.settings.hos_mode) else {
            return String::new();
        };
        if limit.remaining_min <= 0.0 {
            return match limit.kind {
                "break" => "Break overdue.",
                "drive" => "Out of driving time for this shift.",
                _ => "Your duty window has closed.",
            }
            .to_string();
        }
        let left = hos::duration_text(limit.remaining_min / 60.0);
        match limit.kind {
            "break" => format!("30-minute break due in {left}."),
            "drive" => format!("Driving time left: {left}."),
            _ => format!("Duty window closes in {left}."),
        }
    }

    /// `_hos_key_notice()`: where the hours detail went, for the first few
    /// clock presses only.
    ///
    /// Muscle memory says C, so the pointer has to ride C itself. Three
    /// presses is enough to learn it and few enough not to become noise.
    ///
    /// The three keys are keyboard only, and a pad's clock button already
    /// reads all three limits, so the pointer waits for the keyboard.
    pub fn hos_key_notice(&self, ctx: &mut GameContext) -> String {
        let left = profile_of(ctx).hos_key_notice_left;
        if left <= 0 || ctx.controller.device() != ff_core::input_hints::KEYBOARD {
            return String::new();
        }
        profile_mut_of(ctx).hos_key_notice_left = left - 1;
        format!(
            " Hours of service moved to {}, {}, and {}.",
            ctx.control_name(Action::HosWheel),
            ctx.control_name(Action::HosBreak),
            ctx.control_name(Action::HosDrive)
        )
    }

    /// `_speak_hos_wheel_time()`: Alt A -- how much of this shift is already
    /// behind you.
    pub fn speak_hos_wheel_time(&mut self, ctx: &mut GameContext) {
        let terse = self.terse_speech(ctx);
        let mode = ctx.settings.hos_mode.clone();
        let text = hos_of(ctx).wheel_time_summary(&mode, terse);
        ctx.say(&text);
    }

    /// `_speak_hos_break()`: Alt S -- when the 30-minute break comes due.
    pub fn speak_hos_break(&mut self, ctx: &mut GameContext) {
        let terse = self.terse_speech(ctx);
        let mode = ctx.settings.hos_mode.clone();
        let text = hos_of(ctx).break_summary(&mode, terse);
        ctx.say(&text);
    }

    /// `_speak_hos_drive_left()`: Alt D -- what ends this shift, and where you
    /// can legally stop before it.
    pub fn speak_hos_drive_left(&mut self, ctx: &mut GameContext) {
        let terse = self.terse_speech(ctx);
        let mode = ctx.settings.hos_mode.clone();
        let mut text = hos_of(ctx).drive_time_summary(&mode, terse);
        if !terse {
            let route = self.hos_route_context(ctx);
            if !route.is_empty() {
                text = format!("{text} {route}");
            }
        }
        ctx.say(&text);
    }

    /// `_hos_route_context()`.
    pub fn hos_route_context(&self, ctx: &GameContext) -> String {
        let Some(advice) = self.hos_stop_advice(ctx) else {
            return String::new();
        };
        let fallback_distance = ctx.settings.distance_text(advice.ahead_mi, false);
        let suggested_distance = advice
            .suggested
            .as_ref()
            .map(|option| ctx.settings.distance_text(option.ahead_mi, false))
            .unwrap_or_else(|| fallback_distance.clone());
        advice.summary(&suggested_distance, &fallback_distance)
    }

    /// `_legal_miles_for_hos(remaining_min)`.
    pub fn legal_miles_for_hos(&self, remaining_min: f64) -> f64 {
        let speed = self.trip.truck.speed_mph();
        let speed = if speed == 0.0 { 55.0 } else { speed };
        let pace = 35.0f64.max(62.0f64.min(speed));
        0.0f64.max(remaining_min / 60.0 * pace)
    }

    /// `_upcoming_stop_with_action(action, within_mi)`.
    pub fn upcoming_stop_with_action(&self, action: &str, within_mi: f64) -> Option<&RoadStop> {
        let mut best: Option<&RoadStop> = None;
        for stop in &self.trip.stops {
            let ahead = stop.at_mi - self.trip.position_mi;
            if !(0.0..=within_mi).contains(&ahead) {
                continue;
            }
            if !stop
                .actions
                .iter()
                .any(|a| a == action || (action == "break" && a == "sleep"))
                || stop.parking == "none"
                || !stop.accessible_to(self.trip.bobtail)
            {
                continue;
            }
            if best.is_none_or(|b| stop.at_mi < b.at_mi) {
                best = Some(stop);
            }
        }
        best
    }

    /// `emergency_shoulder_sleep_reason()`: why shoulder sleep is offered
    /// now, or None when it is not.
    ///
    /// Available whenever the truck is stopped with no route POI to pull into
    /// -- a driver can always choose to pull over and rest, urgently or not.
    /// The wording escalates with urgency (severe fatigue, or an HOS limit
    /// closing in with no reachable stop) but the option itself is always
    /// there.
    pub fn emergency_shoulder_sleep_reason(&self, ctx: &GameContext) -> Option<String> {
        if self.trip.truck.speed_mph() > DOCKING_MAX_MPH {
            return None;
        }
        if self.trip.nearest_stop_within(1.5).is_some() {
            return None; // a POI is right here; use its rest menu instead
        }
        if profile_of(ctx).fatigue >= hos::FATIGUE_SEVERE {
            return Some("Fatigue is severe, and no route stop is nearby.".to_string());
        }
        let mode = &ctx.settings.hos_mode;
        if !hos::HOS_NON_ENFORCED_MODES.contains(&mode.as_str()) {
            if hos_of(ctx).in_violation(mode) {
                return Some(
                    "You are past your hours-of-service limit, and there is no route POI here."
                        .to_string(),
                );
            }
            if let Some(limit) = hos_of(ctx).next_limit(mode) {
                let action = if limit.kind == "break" {
                    "break"
                } else {
                    "sleep"
                };
                let legal_miles = self.legal_miles_for_hos(limit.remaining_min);
                if limit.remaining_min <= hos::SHOULDER_SLEEP_LIMIT_BUFFER_MIN
                    && self
                        .upcoming_stop_with_action(action, (legal_miles + 5.0).max(5.0))
                        .is_none()
                {
                    return Some(format!(
                        "Your next {action} limit is due in {:.1} hours, and no suitable route \
                         stop is visible before it.",
                        limit.remaining_min / 60.0
                    ));
                }
            }
        }
        Some("No route stop is nearby. You can pull over and rest on the shoulder.".to_string())
    }
}
