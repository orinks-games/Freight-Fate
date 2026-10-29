//! The ELD shift ledger: `HosEvent` and `HosClock`.

use serde_json::{json, Value};

use super::pyjson::{py_float_or, py_iter, py_max, py_repr_str, py_str, py_str_or};
use super::{
    duration_text, is_duty_status, is_non_enforced, limits, positive_minutes, BREAK_MIN,
    HOS_HISTORY_MAX, HOS_SPLIT_REST_HISTORY_MAX, SLEEP_MIN, SPLIT_LONG_ALT_MIN, SPLIT_LONG_MIN,
    SPLIT_SHORT_ALT_MIN, SPLIT_SHORT_MIN,
};
use crate::pyfmt::py_str_float;

/// One duty-status entry in the shift ledger.
#[derive(Clone, Debug, PartialEq)]
pub struct HosEvent {
    pub status: String,
    pub minutes: f64,
    pub drive_before: f64,
    pub duty_before: f64,
    pub since_break_before: f64,
    pub source: String,
}

impl Default for HosEvent {
    fn default() -> Self {
        Self {
            status: "off_duty".to_string(),
            minutes: 0.0,
            drive_before: 0.0,
            duty_before: 0.0,
            since_break_before: 0.0,
            source: "normal".to_string(),
        }
    }
}

impl HosEvent {
    pub fn new(
        status: &str,
        minutes: f64,
        drive_before: f64,
        duty_before: f64,
        since_break_before: f64,
        source: &str,
    ) -> Self {
        Self {
            status: status.to_string(),
            minutes,
            drive_before,
            duty_before,
            since_break_before,
            source: source.to_string(),
        }
    }

    pub fn to_dict(&self) -> Value {
        json!({
            "status": self.status,
            "minutes": self.minutes,
            "drive_before": self.drive_before,
            "duty_before": self.duty_before,
            "since_break_before": self.since_break_before,
            "source": self.source,
        })
    }

    /// `None` for anything that is not a readable event.
    pub fn from_dict(data: &Value) -> Option<Self> {
        let obj = data.as_object()?;
        let status = py_str_or(obj.get("status"), "off_duty");
        if !is_duty_status(&status) {
            return None;
        }
        Some(Self {
            status,
            minutes: py_float_or(obj.get("minutes"), 0.0)?,
            drive_before: py_float_or(obj.get("drive_before"), 0.0)?,
            duty_before: py_float_or(obj.get("duty_before"), 0.0)?,
            since_break_before: py_float_or(obj.get("since_break_before"), 0.0)?,
            source: py_str_or(obj.get("source"), "normal"),
        })
    }
}

/// The nearest-limit answer of [`HosClock::next_limit`]: which rule, how many
/// game minutes are left on it, and the spoken "what is due" clause.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HosLimit {
    pub kind: &'static str,
    pub remaining_min: f64,
    pub due: &'static str,
}

/// The Python `repr` of the two events' field tuples, which is what the
/// save stores as `split_credit_key`: strings single-quoted, floats as
/// Python prints them, `((...), (...))` with `", "` separators.
pub(super) fn split_event_key(first: &HosEvent, second: &HosEvent) -> String {
    fn event_key(event: &HosEvent) -> String {
        format!(
            "({}, {}, {}, {}, {}, {})",
            py_repr_str(&event.status),
            py_repr_str(&event.source),
            py_str_float(event.minutes),
            py_str_float(event.drive_before),
            py_str_float(event.duty_before),
            py_str_float(event.since_break_before),
        )
    }
    format!("({}, {})", event_key(first), event_key(second))
}

/// Whether a rest stops the 14-hour window while it runs.
///
/// 49 CFR 395.1(g)(1)(iii)(B) excludes qualifying sleeper-berth periods from
/// the 14-hour window, and an ELD stops the window the moment a period
/// qualifies on its own -- 7 or more consecutive hours in the berth -- rather
/// than waiting for its 2-hour partner. A shorter rest keeps counting until
/// the pair is credited. Before this, an 8-hour sleeper after 7 hours of duty
/// closed the window in the driver's sleep (tester report, 2026-09-11).
/// Only a rest recorded as a normal split candidate pauses; the fallback
/// rests (shoulder, cramped lot) never do.
fn pauses_duty_window(status: &str, minutes: f64, source: &str) -> bool {
    source == "normal" && status == "sleeper_berth" && minutes >= SPLIT_LONG_MIN
}

fn split_pair_qualifies(first: &HosEvent, second: &HosEvent) -> bool {
    if first.source == "full_reset"
        || second.source == "full_reset"
        || first.minutes >= SLEEP_MIN
        || second.minutes >= SLEEP_MIN
    {
        return false;
    }
    let total = first.minutes + second.minutes;
    if total < SLEEP_MIN {
        return false;
    }
    let (long_event, short_event) = if first.minutes >= second.minutes {
        (first, second)
    } else {
        (second, first)
    };
    if long_event.status != "sleeper_berth" {
        return false;
    }
    if long_event.minutes >= SPLIT_LONG_ALT_MIN && short_event.minutes >= SPLIT_SHORT_MIN {
        return true;
    }
    long_event.minutes >= SPLIT_LONG_MIN && short_event.minutes >= SPLIT_SHORT_ALT_MIN
}

/// Keep only the last `max` entries (Python `items[-max:]`).
fn keep_last<T>(items: &mut Vec<T>, max: usize) {
    if items.len() > max {
        let excess = items.len() - max;
        items.drain(..excess);
    }
}

const ENFORCEMENT_OFF: &str = "Hours enforcement off.";
const RESET_ADVICE: &str = "Sleep 10 hours at a rest stop to reset.";

/// kind -> (clause after "you are", sentence that can lead a readout)
fn shift_over(kind: &str) -> (&'static str, &'static str) {
    match kind {
        "drive" => (
            "out of driving time for this shift",
            "Out of driving time for this shift.",
        ),
        "duty" => ("past your duty window", "Your duty window has closed."),
        other => panic!("no shift-over wording for HOS kind {other:?}"),
    }
}

/// One ELD-style shift ledger, in game minutes.
///
/// `duty_min` is the elapsed 14-hour window since the last qualifying
/// 10-hour reset, not just on-duty labor. FMCSA's 14-hour window is not
/// extended by short off-duty breaks, so short breaks keep advancing it.
#[derive(Clone, Debug, PartialEq)]
pub struct HosClock {
    /// time at the wheel this shift
    pub driving_min: f64,
    /// elapsed 14-hour duty window
    pub duty_min: f64,
    /// driving since the last 30-minute break
    pub since_break_min: f64,
    pub status: String,
    /// consecutive non-driving time
    pub non_driving_min: f64,
    /// consecutive off-duty/sleeper time
    pub off_duty_min: f64,
    /// thresholds already spoken
    pub warned: Vec<String>,
    pub history: Vec<HosEvent>,
    pub split_rest_history: Vec<HosEvent>,
    pub split_credit_key: Option<String>,
}

impl Default for HosClock {
    fn default() -> Self {
        Self {
            driving_min: 0.0,
            duty_min: 0.0,
            since_break_min: 0.0,
            status: "off_duty".to_string(),
            non_driving_min: 0.0,
            off_duty_min: 0.0,
            warned: Vec::new(),
            history: Vec::new(),
            split_rest_history: Vec::new(),
            split_credit_key: None,
        }
    }
}

impl HosClock {
    pub fn new() -> Self {
        Self::default()
    }

    // -- time accounting ------------------------------------------------------

    pub fn drive(&mut self, minutes: f64) {
        let minutes = positive_minutes(minutes);
        self.record_event("driving", minutes, "normal");
        self.driving_min += minutes;
        self.duty_min += minutes;
        self.since_break_min += minutes;
        self.status = "driving".to_string();
        self.non_driving_min = 0.0;
        self.off_duty_min = 0.0;
    }

    /// Work time away from the wheel: fueling, loading, inspections, service.
    pub fn on_duty(&mut self, minutes: f64) {
        let minutes = positive_minutes(minutes);
        self.record_event("on_duty_not_driving", minutes, "normal");
        self.duty_min += minutes;
        self.status = "on_duty_not_driving".to_string();
        self.off_duty_min = 0.0;
        self.record_non_driving(minutes);
    }

    /// Off-duty time. Short breaks do not extend the 14-hour window.
    pub fn off_duty(&mut self, minutes: f64) {
        let minutes = positive_minutes(minutes);
        self.record_event("off_duty", minutes, "normal");
        self.duty_min += minutes;
        self.status = "off_duty".to_string();
        self.record_non_driving(minutes);
        self.off_duty_min += minutes;
        if self.off_duty_min >= SLEEP_MIN {
            self.sleep_as("off_duty");
            return;
        }
        self.apply_split_credit();
    }

    /// Sleeper-berth time. A full 10 hours resets; shorter rests may split.
    pub fn sleeper(&mut self, minutes: f64) {
        let minutes = positive_minutes(minutes);
        self.record_event("sleeper_berth", minutes, "normal");
        self.advance_window_for_berth(minutes);
        self.status = "sleeper_berth".to_string();
        self.record_non_driving(minutes);
        self.off_duty_min += minutes;
        if self.off_duty_min >= SLEEP_MIN {
            self.sleep_as("sleeper_berth");
            return;
        }
        self.apply_split_credit();
    }

    /// A short off-duty rest. Kept for old callers and explicit break actions.
    pub fn take_break(&mut self, minutes: f64) {
        self.off_duty(minutes);
    }

    /// A full 10-hour off-duty reset: a fresh shift (`sleep()` in Python,
    /// which defaults the status to the sleeper berth).
    pub fn sleep(&mut self) {
        self.sleep_as("sleeper_berth");
    }

    /// A full 10-hour off-duty reset recorded under `status`.
    pub fn sleep_as(&mut self, status: &str) {
        self.record_event(status, SLEEP_MIN, "full_reset");
        self.driving_min = 0.0;
        self.duty_min = 0.0;
        self.since_break_min = 0.0;
        self.status = if is_duty_status(status) {
            status.to_string()
        } else {
            "sleeper_berth".to_string()
        };
        self.non_driving_min = SLEEP_MIN;
        self.off_duty_min = SLEEP_MIN;
        self.split_credit_key = None;
        self.warned.clear();
    }

    fn record_non_driving(&mut self, minutes: f64) {
        self.non_driving_min += minutes;
        if self.non_driving_min >= BREAK_MIN {
            self.since_break_min = 0.0;
            self.warned.retain(|w| !w.starts_with("break:"));
        }
    }

    fn record_event(&mut self, status: &str, minutes: f64, source: &str) {
        // One unbroken rest is one period, however many menu picks it took:
        // 3 hours in the berth and then 3 more is a 6-hour berth period, as
        // an ELD logs it, not two 3-hour ones (tester report, 2026-09-23).
        let continues = source == "normal"
            && (status == "off_duty" || status == "sleeper_berth")
            && self
                .history
                .last()
                .is_some_and(|prev| prev.source == "normal" && prev.status == status);
        if continues {
            let Some(prev) = self.history.last_mut() else {
                return;
            };
            let was_split_rest = prev.minutes >= SPLIT_SHORT_MIN;
            prev.minutes += minutes;
            let period = prev.clone();
            if was_split_rest {
                if let Some(last) = self.split_rest_history.last_mut() {
                    *last = period;
                }
            } else if period.minutes >= SPLIT_SHORT_MIN {
                self.split_rest_history.push(period);
                keep_last(&mut self.split_rest_history, HOS_SPLIT_REST_HISTORY_MAX);
            }
            return;
        }
        let event = HosEvent::new(
            status,
            minutes,
            self.driving_min,
            self.duty_min,
            self.since_break_min,
            source,
        );
        self.history.push(event.clone());
        keep_last(&mut self.history, HOS_HISTORY_MAX);
        if source == "full_reset" {
            self.split_rest_history.clear();
        } else if source == "normal"
            && (status == "off_duty" || status == "sleeper_berth")
            && minutes >= SPLIT_SHORT_MIN
        {
            self.split_rest_history.push(event);
            keep_last(&mut self.split_rest_history, HOS_SPLIT_REST_HISTORY_MAX);
        }
    }

    /// The window runs through a berth rest until the period it belongs to
    /// reaches 7 hours; from then the whole period is off the window, from
    /// its first minute, however many picks it took to get there.
    fn advance_window_for_berth(&mut self, minutes: f64) {
        match self.history.last() {
            Some(period) if pauses_duty_window(&period.status, period.minutes, &period.source) => {
                self.duty_min = period.duty_before;
            }
            _ => self.duty_min += minutes,
        }
    }

    /// Minutes still owed on a 10-hour reset already under way: resting
    /// since the wheel, and not yet ten hours of it.
    pub fn reset_minutes_left(&self) -> Option<f64> {
        (self.off_duty_min > 0.0 && self.off_duty_min < SLEEP_MIN)
            .then_some(SLEEP_MIN - self.off_duty_min)
    }

    fn key_is_credited(&self, first: usize, second: usize) -> bool {
        match &self.split_credit_key {
            Some(key) => {
                *key == split_event_key(
                    &self.split_rest_history[first],
                    &self.split_rest_history[second],
                )
            }
            None => false,
        }
    }

    /// The Python walks `(first, second)` object pairs and compares them by
    /// identity; the same events are addressed here by their positions in
    /// `split_rest_history`, which is what identity meant there.
    fn qualifying_split_pair(&self) -> Option<(usize, usize)> {
        let n = self.split_rest_history.len();
        let last_first = n.saturating_sub(1);
        let mut start = 0;
        if self.split_credit_key.is_some() {
            for i in 0..last_first {
                if self.key_is_credited(i, i + 1) {
                    start = i + 1;
                    break;
                }
            }
        }
        for i in (start..last_first).rev() {
            if self.key_is_credited(i, i + 1) {
                continue;
            }
            if split_pair_qualifies(&self.split_rest_history[i], &self.split_rest_history[i + 1]) {
                return Some((i, i + 1));
            }
        }
        None
    }

    fn apply_split_credit(&mut self) {
        let Some((first, second)) = self.qualifying_split_pair() else {
            return;
        };
        let key = split_event_key(
            &self.split_rest_history[first],
            &self.split_rest_history[second],
        );
        if self.split_credit_key.as_deref() == Some(key.as_str()) {
            return;
        }
        let second_event = &self.split_rest_history[second];
        self.driving_min = second_event.drive_before - self.split_drive_after_rest(first);
        self.duty_min = second_event.duty_before - self.split_duty_after_rest(first);
        self.since_break_min = 0.0;
        self.split_credit_key = Some(key);
        self.warned.retain(|w| {
            !(w.starts_with("drive:") || w.starts_with("duty:") || w.starts_with("break:"))
        });
    }

    fn split_drive_after_rest(&self, event: usize) -> f64 {
        if let Some((first, second)) = self.previous_qualifying_split_pair(event) {
            return self.split_rest_history[second].drive_before
                - self.split_drive_after_rest(first);
        }
        self.split_rest_history[event].drive_before
    }

    fn split_duty_after_rest(&self, event: usize) -> f64 {
        if let Some((first, second)) = self.previous_qualifying_split_pair(event) {
            return self.split_rest_history[second].duty_before - self.split_duty_after_rest(first);
        }
        let event = &self.split_rest_history[event];
        if pauses_duty_window(&event.status, event.minutes, &event.source) {
            // The window never counted this rest, so the end of the rest
            // is where the window stood when the rest began.
            return event.duty_before;
        }
        event.duty_before + event.minutes
    }

    /// The pair whose second event IS this one: the one ending at `event`.
    fn previous_qualifying_split_pair(&self, event: usize) -> Option<(usize, usize)> {
        if event >= 1
            && split_pair_qualifies(
                &self.split_rest_history[event - 1],
                &self.split_rest_history[event],
            )
        {
            return Some((event - 1, event));
        }
        None
    }

    fn credited_split_pair(&self) -> Option<(usize, usize)> {
        self.split_credit_key.as_ref()?;
        let n = self.split_rest_history.len();
        (0..n.saturating_sub(1))
            .find(|&i| self.key_is_credited(i, i + 1))
            .map(|i| (i, i + 1))
    }

    /// A sleeper-berth rest that may complete a split; `true` when it
    /// credited one (`sleeper_split_rest(minutes)` in Python).
    pub fn sleeper_split_rest(&mut self, minutes: f64) -> bool {
        self.sleeper_split_rest_from(minutes, "normal")
    }

    /// [`Self::sleeper_split_rest`] recorded under an explicit event source.
    pub fn sleeper_split_rest_from(&mut self, minutes: f64, source: &str) -> bool {
        let minutes = positive_minutes(minutes);
        self.record_event("sleeper_berth", minutes, source);
        self.advance_window_for_berth(minutes);
        self.status = "sleeper_berth".to_string();
        self.record_non_driving(minutes);
        self.off_duty_min += minutes;
        if self.off_duty_min >= SLEEP_MIN {
            self.sleep_as("sleeper_berth");
            return false;
        }
        let before_key = self.split_credit_key.clone();
        self.apply_split_credit();
        self.split_credit_key != before_key
    }

    // -- rule queries ----------------------------------------------------------

    /// (kind, minutes remaining, what is due) per enforced limit.
    pub(super) fn statuses(&self, mode: &str) -> Vec<HosLimit> {
        let Some((drive_limit, duty_limit, break_after)) = limits(mode) else {
            return Vec::new();
        };
        vec![
            HosLimit {
                kind: "break",
                remaining_min: break_after - self.since_break_min,
                due: "you must take a 30 minute break at a rest stop",
            },
            HosLimit {
                kind: "drive",
                remaining_min: drive_limit - self.driving_min,
                due: "your driving time for this shift ends. You need 10 hours of sleep",
            },
            HosLimit {
                kind: "duty",
                remaining_min: duty_limit - self.duty_min,
                due: "your duty window closes. You need 10 hours of sleep",
            },
        ]
    }

    /// Game minutes until the nearest limit, or None when not enforced.
    pub fn remaining_min(&self, mode: &str) -> Option<f64> {
        self.next_limit(mode).map(|limit| limit.remaining_min)
    }

    /// Nearest enforced HOS limit as (kind, minutes, due_text); the first
    /// one on a tie, as Python's `min` picks it.
    pub fn next_limit(&self, mode: &str) -> Option<HosLimit> {
        let mut best: Option<HosLimit> = None;
        for status in self.statuses(mode) {
            let nearer = match best {
                Some(b) => status.remaining_min < b.remaining_min,
                None => true,
            };
            if nearer {
                best = Some(status);
            }
        }
        best
    }

    pub fn in_violation(&self, mode: &str) -> bool {
        self.statuses(mode).iter().any(|s| s.remaining_min <= 0.0)
    }

    /// Limit kinds currently blown, in status order (drive, duty, break).
    pub fn blown_kinds(&self, mode: &str) -> Vec<&'static str> {
        self.statuses(mode)
            .iter()
            .filter(|s| s.remaining_min <= 0.0)
            .map(|s| s.kind)
            .collect()
    }

    /// A missed 30-minute break, with drive and duty still legal.
    pub fn is_break_only_violation(&self, mode: &str) -> bool {
        let blown = self.blown_kinds(mode);
        blown.len() == 1 && blown[0] == "break"
    }

    /// Out-of-service time for the current violation: 30 minutes for a missed
    /// break, a full 10-hour reset for drive or duty.
    pub fn out_of_service_minutes(&self, mode: &str) -> f64 {
        if self.is_break_only_violation(mode) {
            BREAK_MIN
        } else {
            SLEEP_MIN
        }
    }

    /// Plain spoken phrases for every limit currently blown.
    ///
    /// The roadside out-of-service stop uses these BEFORE the reset wipes
    /// the ledger, so the officer can say exactly why the order stands.
    pub fn violation_causes(&self, mode: &str) -> Vec<String> {
        self.statuses(mode)
            .iter()
            .filter(|s| s.remaining_min <= 0.0)
            .filter_map(|s| match s.kind {
                "drive" => Some("you had driven past the 11-hour driving limit"),
                "duty" => Some("your 14-hour duty window had expired"),
                "break" => Some("you were past the 30-minute break requirement"),
                _ => None,
            })
            .map(str::to_string)
            .collect()
    }

    /// Spoken status for the C key and Tab report.
    pub fn summary(&self, mode: &str) -> String {
        if is_non_enforced(mode) {
            return ENFORCEMENT_OFF.to_string();
        }
        let (drive_limit, duty_limit, break_after) =
            limits(mode).expect("HOS mode is realistic or relaxed");
        let drive_left = py_max(0.0, drive_limit - self.driving_min) / 60.0;
        let duty_left = py_max(0.0, duty_limit - self.duty_min) / 60.0;
        let break_left = py_max(0.0, break_after - self.since_break_min) / 60.0;
        if self.in_violation(mode) {
            let blown: Vec<&str> = self
                .statuses(mode)
                .iter()
                .filter(|s| s.remaining_min <= 0.0)
                .map(|s| s.kind)
                .collect();
            if blown == ["break"] {
                return "Hours of service: 30-minute break overdue.".to_string();
            }
            return "Hours of service: past your limit. Sleep 10 hours at a rest stop to reset."
                .to_string();
        }
        // The status is its own whole sentence per status, so a catalog
        // translates "on duty not driving" with the sentence around it rather
        // than as a loose fragment.
        let status = match self.status.as_str() {
            "driving" => "ELD status driving.".to_string(),
            "on_duty_not_driving" => "ELD status on duty not driving.".to_string(),
            "off_duty" => "ELD status off duty.".to_string(),
            "sleeper_berth" => "ELD status sleeper berth.".to_string(),
            other => format!("ELD status {}.", other.replace('_', " ")),
        };
        let hours = if duty_left <= break_left {
            format!(
                "Hours of service: {} of driving left, {} left in the 14-hour window.",
                duration_text(drive_left),
                duration_text(duty_left),
            )
        } else {
            format!(
                "Hours of service: {} of driving left, 30-minute break due in {}, \
                 {} left in the 14-hour window.",
                duration_text(drive_left),
                duration_text(break_left),
                duration_text(duty_left),
            )
        };
        let mut sentences = vec![status, hours];
        if let Some(pending) = self.split_pending_summary() {
            sentences.push(pending.to_string());
        }
        sentences.join(" ")
    }

    // -- the one-answer readouts ------------------------------------------------
    //
    // `summary` speaks the whole picture; these three answer one question each,
    // for the Alt A, Alt S, and Alt D keys (see DrivingControlsMixin). None may
    // go silent -- a blind driver cannot tell a quiet key from a dead one -- and
    // none names a limit in hours: remaining time is read from the live
    // clock so the spoken number stays true if the limits ever move.

    /// (driving, duty window, break) hours left, floored at zero.
    /// The summary one limit per line, for the logbook: driving left, the
    /// break, the duty window, and a pending split. Enforcement off and a
    /// violation each stay one line, because each is one fact.
    pub fn summary_lines(&self, mode: &str) -> Vec<String> {
        if is_non_enforced(mode) || self.in_violation(mode) {
            return vec![self.summary(mode)];
        }
        let (drive_left, duty_left, break_left) = self.hours_left(mode);
        let mut lines = vec![format!("Driving left: {}.", duration_text(drive_left))];
        if duty_left <= break_left {
            lines.push(format!("Duty window left: {}.", duration_text(duty_left)));
        } else {
            lines.push(format!(
                "30-minute break due in {}.",
                duration_text(break_left)
            ));
            lines.push(format!(
                "Duty window closes in {}.",
                duration_text(duty_left)
            ));
        }
        if let Some(pending) = self.split_pending_summary() {
            lines.push(pending.to_string());
        }
        lines
    }

    fn hours_left(&self, mode: &str) -> (f64, f64, f64) {
        let (drive_limit, duty_limit, break_after) =
            limits(mode).expect("HOS mode is realistic or relaxed");
        (
            py_max(0.0, drive_limit - self.driving_min) / 60.0,
            py_max(0.0, duty_limit - self.duty_min) / 60.0,
            py_max(0.0, break_after - self.since_break_min) / 60.0,
        )
    }

    /// Which blown limit ended the shift, or None; the driving clock leads.
    fn shift_over_kind(&self, mode: &str) -> Option<&'static str> {
        let statuses = self.statuses(mode);
        let blown = |kind: &str| {
            statuses
                .iter()
                .any(|s| s.kind == kind && s.remaining_min <= 0.0)
        };
        if blown("drive") {
            Some("drive")
        } else if blown("duty") {
            Some("duty")
        } else {
            None
        }
    }

    /// Alt A: how much of this shift is spent -- not the hours on this run.
    pub fn wheel_time_summary(&self, mode: &str, terse: bool) -> String {
        let fresh = self.driving_min <= 0.0;
        let driven = duration_text(self.driving_min / 60.0);
        let lead = if terse {
            if fresh {
                "At the wheel: no driving yet".to_string()
            } else {
                format!("At the wheel {driven}")
            }
        } else {
            let spent = if fresh {
                "nothing yet".to_string()
            } else {
                driven.clone()
            };
            format!(
                "Driven {spent}. On duty {}",
                duration_text(self.duty_min / 60.0)
            )
        };
        let note = if limits(mode).is_none() {
            ENFORCEMENT_OFF
        } else if self.shift_over_kind(mode).is_some() {
            "Out of hours."
        } else if self.hours_left(mode).2 <= 0.0 {
            "Your 30-minute break is overdue."
        } else {
            ""
        };
        format!("{lead}. {note}").trim_end().to_string()
    }

    /// Alt S: when the 30-minute break comes due.
    pub fn break_summary(&self, mode: &str, terse: bool) -> String {
        if limits(mode).is_none() {
            return format!("Break: none required. {ENFORCEMENT_OFF}");
        }
        let (_drive_left, duty_left, break_left) = self.hours_left(mode);
        let over = self.shift_over_kind(mode);
        let mut answer;
        let detail;
        if break_left <= 0.0 {
            answer = "Break overdue".to_string();
            detail = if terse {
                ""
            } else {
                " Take a 30 minute break at a rest stop."
            };
        } else {
            answer = format!(
                "30-minute break due in {}{}",
                duration_text(break_left),
                if terse { "" } else { " of driving" }
            );
            detail = "";
            if over.is_none() && duty_left <= break_left {
                // summary drops the break when the window closes first; a key
                // pressed for the break answers it, then the overriding fact.
                answer.push_str(&if terse {
                    format!(", duty window {}", duration_text(duty_left))
                } else {
                    format!(", {} left in the 14-hour window", duration_text(duty_left))
                });
            }
        }
        if let Some(kind) = over {
            return format!(
                "{answer}, but you are {}. {RESET_ADVICE}",
                shift_over(kind).0
            );
        }
        format!("{answer}.{detail}")
    }

    /// Alt D: what ends this shift. Both clocks are named; the binding one leads.
    pub fn drive_time_summary(&self, mode: &str, terse: bool) -> String {
        if limits(mode).is_none() {
            let no_limit = format!(
                "Driving time left: no limit{}",
                if terse { "" } else { ", and no duty window" }
            );
            return format!("{no_limit}. {ENFORCEMENT_OFF}");
        }
        let (drive_left, duty_left, break_left) = self.hours_left(mode);
        if let Some(kind) = self.shift_over_kind(mode) {
            return format!("{} {RESET_ADVICE}", shift_over(kind).1);
        }
        let drive_text = format!("Driving time left: {}", duration_text(drive_left));
        let duty_text = format!("Duty window closes in {}", duration_text(duty_left));
        let duty_binds = duty_left <= drive_left; // the window, not the wheel, ends this shift
        let mut text = if terse {
            if duty_binds {
                format!(
                    "Duty window {}, {}",
                    duration_text(duty_left),
                    drive_text.to_lowercase()
                )
            } else {
                format!("{drive_text}, duty window {}", duration_text(duty_left))
            }
        } else if duty_binds {
            format!("{duty_text}. {drive_text}")
        } else {
            format!("{drive_text}. {duty_text}")
        };
        if break_left <= 0.0 {
            text.push_str(if terse {
                ", break overdue"
            } else {
                ". Your 30-minute break is overdue and comes first"
            });
        }
        match self.split_pending_summary() {
            Some(pending) => format!("{text}. {pending}"),
            None => format!("{text}."),
        }
    }

    /// One clause relating an ETA to the nearest HOS limit, or ''.
    ///
    /// Used by the stop details screen: says whether the player would reach
    /// the stop before the limit that matters most right now. Mirrors
    /// `summary`'s omission rule -- when the duty window closes before the
    /// break is due, the break is irrelevant and never mentioned.
    pub fn arrival_note(&self, mode: &str, eta_min: f64) -> String {
        if is_non_enforced(mode) || self.in_violation(mode) {
            return String::new();
        }
        let (drive_limit, duty_limit, break_after) =
            limits(mode).expect("HOS mode is realistic or relaxed");
        let drive_left = drive_limit - self.driving_min;
        let duty_left = duty_limit - self.duty_min;
        let break_left = break_after - self.since_break_min;
        let mut candidates = vec![
            (
                "drive",
                drive_left,
                "your driving time for this shift runs out",
            ),
            ("duty", duty_left, "your duty window closes"),
        ];
        if duty_left > break_left {
            candidates.push(("break", break_left, "your 30-minute break is due"));
        }
        let mut nearest = candidates[0];
        for candidate in &candidates[1..] {
            if candidate.1 < nearest.1 {
                nearest = *candidate;
            }
        }
        let (kind, nearest_min, phrase) = nearest;
        if eta_min < nearest_min {
            return format!(" You would arrive before {phrase}.");
        }
        let gap_h = (eta_min - nearest_min) / 60.0;
        let limit_name = match kind {
            "drive" => "driving-time limit",
            "duty" => "duty window",
            _ => "break",
        };
        format!(
            " Your {limit_name} comes about {} before you would reach it.",
            duration_text(gap_h)
        )
    }

    pub fn split_pending_summary(&self) -> Option<&'static str> {
        let rest_events = &self.split_rest_history;
        if let Some((_, second)) = self.credited_split_pair() {
            if second + 1 == rest_events.len() {
                return None;
            }
        }
        if let Some((first, second)) = self.qualifying_split_pair() {
            if self.key_is_credited(first, second) {
                return None;
            }
        }
        let last = rest_events.last()?;
        if last.status == "sleeper_berth" && last.minutes >= SPLIT_LONG_ALT_MIN {
            return Some("Sleeper split open: 2 more hours off duty or in the sleeper to pair it.");
        }
        if last.status == "sleeper_berth" && last.minutes >= SPLIT_LONG_MIN {
            return Some("Sleeper split open: 3 more hours off duty or in the sleeper to pair it.");
        }
        if last.minutes >= SPLIT_SHORT_ALT_MIN {
            return Some("Sleeper split open: 7 more hours in the sleeper to pair it.");
        }
        if last.minutes >= SPLIT_SHORT_MIN {
            return Some("Sleeper split open: 8 more hours in the sleeper to pair it.");
        }
        None
    }

    // -- serialization -----------------------------------------------------------

    pub fn to_dict(&self) -> Value {
        json!({
            "driving_min": self.driving_min,
            "duty_min": self.duty_min,
            "since_break_min": self.since_break_min,
            "status": self.status,
            "non_driving_min": self.non_driving_min,
            "off_duty_min": self.off_duty_min,
            "warned": self.warned,
            "history": self.history.iter().map(HosEvent::to_dict).collect::<Vec<_>>(),
            "split_rest_history": self.split_rest_history.iter().map(HosEvent::to_dict).collect::<Vec<_>>(),
            "split_credit_key": self.split_credit_key,
        })
    }

    /// Tolerant load: anything unreadable becomes a fresh clock.
    pub fn from_dict(data: &Value) -> Self {
        Self::try_from_dict(data).unwrap_or_default()
    }

    fn try_from_dict(data: &Value) -> Option<Self> {
        let obj = data.as_object()?;
        let mut status = py_str_or(obj.get("status"), "off_duty");
        if !is_duty_status(&status) {
            status = "off_duty".to_string();
        }
        let mut history = Vec::new();
        for raw_event in py_iter(obj.get("history"))? {
            if let Some(event) = HosEvent::from_dict(&raw_event) {
                history.push(event);
            }
        }
        let mut split_rest_history = Vec::new();
        match obj.get("split_rest_history") {
            Some(Value::Array(raw_split_rest_history)) => {
                for raw_event in raw_split_rest_history {
                    if let Some(event) = HosEvent::from_dict(raw_event) {
                        split_rest_history.push(event);
                    }
                }
            }
            _ => {
                split_rest_history = history
                    .iter()
                    .filter(|event| {
                        event.source == "normal"
                            && (event.status == "off_duty" || event.status == "sleeper_berth")
                            && event.minutes >= SPLIT_SHORT_MIN
                            && event.minutes < SLEEP_MIN
                    })
                    .cloned()
                    .collect();
            }
        }
        let warned: Vec<String> = py_iter(obj.get("warned"))?.iter().map(py_str).collect();
        let split_credit_key = match obj.get("split_credit_key") {
            None | Some(Value::Null) => None,
            Some(value) => Some(py_str(value)),
        };
        keep_last(&mut history, HOS_HISTORY_MAX);
        keep_last(&mut split_rest_history, HOS_SPLIT_REST_HISTORY_MAX);
        Some(Self {
            driving_min: py_float_or(obj.get("driving_min"), 0.0)?,
            duty_min: py_float_or(obj.get("duty_min"), 0.0)?,
            since_break_min: py_float_or(obj.get("since_break_min"), 0.0)?,
            status,
            non_driving_min: py_float_or(obj.get("non_driving_min"), 0.0)?,
            off_duty_min: py_float_or(obj.get("off_duty_min"), 0.0)?,
            warned,
            history,
            split_rest_history,
            split_credit_key,
        })
    }
}
