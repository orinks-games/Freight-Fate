//! The ELD shift ledger: `HosEvent` and `HosClock`.

use serde_json::{json, Value};

use super::pyjson::{py_float_or, py_iter, py_max, py_repr_str, py_str, py_str_or};
use super::{
    cycle_limit, duration_text, is_duty_status, is_non_enforced, limits, positive_minutes,
    HosRules, ALASKA_CYCLE_LIMIT_MIN, ALASKA_DRIVE_LIMIT_MIN, ALASKA_ON_DUTY_LIMIT_MIN,
    ALASKA_RESTART_MIN, BREAK_MIN, CANADA_CYCLE_WINDOW_MIN, CANADA_NORTH_CYCLE_LIMIT_MIN,
    CANADA_NORTH_DRIVE_LIMIT_MIN, CANADA_NORTH_ON_DUTY_LIMIT_MIN, CANADA_NORTH_SHIFT_LIMIT_MIN,
    CANADA_RESTART_MIN, CANADA_SOUTH_CYCLE_LIMIT_MIN, CANADA_SOUTH_DRIVE_LIMIT_MIN,
    CANADA_SOUTH_ON_DUTY_LIMIT_MIN, CANADA_SOUTH_SHIFT_LIMIT_MIN, CYCLE_SPEAK_MIN,
    CYCLE_WINDOW_MIN, HOS_HISTORY_MAX, HOS_SPLIT_REST_HISTORY_MAX, RESTART_MIN, SLEEP_MIN,
    SPLIT_LONG_ALT_MIN, SPLIT_LONG_MIN, SPLIT_SHORT_ALT_MIN, SPLIT_SHORT_MIN,
};
use crate::pyfmt::py_str_float;

/// One duty-status entry in the shift ledger.
#[derive(Clone, Debug, PartialEq)]
pub struct HosEvent {
    pub status: String,
    pub minutes: f64,
    pub drive_before: f64,
    pub duty_before: f64,
    pub shift_on_duty_before: f64,
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
            shift_on_duty_before: 0.0,
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
            shift_on_duty_before: 0.0,
            since_break_before,
            source: source.to_string(),
        }
    }

    fn new_with_shift_on_duty(
        status: &str,
        minutes: f64,
        drive_before: f64,
        duty_before: f64,
        shift_on_duty_before: f64,
        since_break_before: f64,
        source: &str,
    ) -> Self {
        let mut event = Self::new(
            status,
            minutes,
            drive_before,
            duty_before,
            since_break_before,
            source,
        );
        event.shift_on_duty_before = shift_on_duty_before;
        event
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

    fn to_dict_with_shift_on_duty(&self) -> Value {
        let mut data = self.to_dict();
        if self.shift_on_duty_before != 0.0 {
            data.as_object_mut().unwrap().insert(
                "shift_on_duty_before".to_string(),
                json!(self.shift_on_duty_before),
            );
        }
        data
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
            shift_on_duty_before: py_float_or(obj.get("shift_on_duty_before"), 0.0).unwrap_or(0.0),
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

#[derive(Clone, Copy)]
struct RuleLimits {
    drive_limit: f64,
    duty_limit: f64,
    shift_limit: Option<f64>,
    cycle_limit: f64,
    drive_used: f64,
    duty_used: f64,
    shift_used: f64,
}

/// One on-duty span on the 8-day cycle ledger. `end_min` is where the
/// ledger clock stood when it ended.
#[derive(Clone, Debug, PartialEq)]
pub struct CycleEntry {
    pub end_min: f64,
    pub on_duty_min: f64,
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
fn reset_advice(kind: &str, rules: HosRules) -> &'static str {
    match (rules, kind) {
        (HosRules::CanadaSouth60 | HosRules::CanadaNorth60, "cycle") => {
            "Take 36 hours off to restart your cycle, or wait for hours to age off your 7-day ledger."
        }
        (HosRules::CanadaSouth60 | HosRules::CanadaNorth60, _) => {
            "Take 8 hours off at a rest stop to reset."
        }
        (HosRules::Alaska, "cycle") => {
            "Take a 34-hour restart, or wait for hours to age off your 8-day ledger."
        }
        (HosRules::Us, "cycle") => {
            "Take a 34-hour restart, or wait for hours to age off your 8-day ledger."
        }
        _ => "Sleep 10 hours at a rest stop to reset.",
    }
}

/// kind -> (clause after "you are", sentence that can lead a readout)
fn shift_over(kind: &str, rules: HosRules) -> (&'static str, &'static str) {
    match (rules, kind) {
        (HosRules::Us, "drive") | (HosRules::Alaska, "drive") => (
            "out of driving time for this shift",
            "Out of driving time for this shift.",
        ),
        (HosRules::Us, "duty") => ("past your duty window", "Your duty window has closed."),
        (HosRules::Alaska, "duty") => (
            "past your on-duty limit",
            "Your on-duty limit has been reached.",
        ),
        (HosRules::CanadaSouth60 | HosRules::CanadaNorth60, "drive") => (
            "past your driving limit",
            "Your driving limit has been reached.",
        ),
        (HosRules::CanadaSouth60 | HosRules::CanadaNorth60, "duty") => (
            "past your on-duty limit",
            "Your on-duty limit has been reached.",
        ),
        (HosRules::CanadaSouth60 | HosRules::CanadaNorth60, "shift") => (
            "past your shift limit",
            "Your shift limit has been reached.",
        ),
        (HosRules::Us, "cycle") => ("out of cycle hours", "Your 70-hour cycle is used up."),
        (HosRules::Alaska, "cycle") => ("out of cycle hours", "Your 80-hour cycle is used up."),
        (HosRules::CanadaSouth60, "cycle") => (
            "out of cycle hours",
            "Your 70-hour Canadian cycle is used up.",
        ),
        (HosRules::CanadaNorth60, "cycle") => (
            "out of cycle hours",
            "Your 80-hour Canadian cycle is used up.",
        ),
        (_, "break") => ("past your break limit", "Your break is overdue."),
        _ => (
            "past your hours limit",
            "Your hours limit has been reached.",
        ),
    }
}

/// One jurisdiction-selected ELD shift ledger, in game minutes.
///
/// Under US rules, `duty_min` is the elapsed 14-hour window since the last
/// qualifying 10-hour reset, not just on-duty labor. FMCSA's 14-hour window
/// is not extended by short off-duty breaks, so short breaks keep advancing it.
#[derive(Clone, Debug, PartialEq)]
pub struct HosClock {
    /// time at the wheel this shift
    pub driving_min: f64,
    /// elapsed 14-hour duty window
    pub duty_min: f64,
    /// Rule set selected for the truck's current jurisdiction.
    pub rules: HosRules,
    /// Driving and on-duty minutes since the last full off-duty reset.
    pub shift_on_duty_min: f64,
    /// Canadian driving since the last qualifying 8-hour off-duty period.
    pub ca_driving_min: f64,
    /// Canadian on-duty time since the last qualifying 8-hour off-duty period.
    pub ca_on_duty_min: f64,
    /// Canadian elapsed time since the last qualifying 8-hour off-duty period.
    pub ca_elapsed_min: f64,
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
    /// total game minutes the clock has been told about since the ledger began
    pub cycle_clock_min: f64,
    /// on-duty spans within the trailing 8-day cycle window
    pub cycle_entries: Vec<CycleEntry>,
    /// consecutive off-duty/sleeper minutes toward the selected cycle restart;
    /// unlike `off_duty_min` this is never capped at `SLEEP_MIN`
    pub restart_off_min: f64,
}

impl Default for HosClock {
    fn default() -> Self {
        Self {
            driving_min: 0.0,
            duty_min: 0.0,
            rules: HosRules::Us,
            shift_on_duty_min: 0.0,
            ca_driving_min: 0.0,
            ca_on_duty_min: 0.0,
            ca_elapsed_min: 0.0,
            since_break_min: 0.0,
            status: "off_duty".to_string(),
            non_driving_min: 0.0,
            off_duty_min: 0.0,
            warned: Vec::new(),
            history: Vec::new(),
            split_rest_history: Vec::new(),
            split_credit_key: None,
            cycle_clock_min: 0.0,
            cycle_entries: Vec::new(),
            restart_off_min: 0.0,
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
        self.shift_on_duty_min += minutes;
        self.ca_driving_min += minutes;
        self.ca_on_duty_min += minutes;
        self.ca_elapsed_min += minutes;
        self.status = "driving".to_string();
        self.non_driving_min = 0.0;
        self.off_duty_min = 0.0;
        self.advance_cycle(minutes, true);
    }

    /// Work time away from the wheel: fueling, loading, inspections, service.
    pub fn on_duty(&mut self, minutes: f64) {
        let minutes = positive_minutes(minutes);
        self.record_event("on_duty_not_driving", minutes, "normal");
        self.duty_min += minutes;
        self.shift_on_duty_min += minutes;
        self.ca_on_duty_min += minutes;
        self.ca_elapsed_min += minutes;
        self.status = "on_duty_not_driving".to_string();
        self.off_duty_min = 0.0;
        self.record_non_driving(minutes);
        self.advance_cycle(minutes, true);
    }

    /// Off-duty time. Short breaks do not extend the 14-hour window.
    pub fn off_duty(&mut self, minutes: f64) {
        let minutes = positive_minutes(minutes);
        let consecutive_off_before = self.restart_off_min;
        self.advance_cycle(minutes, false);
        self.advance_canada_rest(minutes, consecutive_off_before);
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
        let consecutive_off_before = self.restart_off_min;
        self.advance_cycle(minutes, false);
        self.advance_canada_rest(minutes, consecutive_off_before);
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
        self.advance_cycle(SLEEP_MIN, false);
        self.sleep_as("sleeper_berth");
    }

    /// A full cycle restart under the selected rules, recorded as a
    /// sleeper-berth reset.
    pub fn restart(&mut self) {
        self.advance_cycle(self.restart_threshold_min(), false);
        self.sleep_as("sleeper_berth");
    }

    /// A full 10-hour off-duty reset recorded under `status`.
    pub fn sleep_as(&mut self, status: &str) {
        self.record_event(status, SLEEP_MIN, "full_reset");
        self.driving_min = 0.0;
        self.duty_min = 0.0;
        self.shift_on_duty_min = 0.0;
        self.ca_driving_min = 0.0;
        self.ca_on_duty_min = 0.0;
        self.ca_elapsed_min = 0.0;
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

    /// Advance the cycle ledger. `on_duty` minutes accrue; anything else
    /// builds toward the selected restart. An entry counts whole until its
    /// end falls out of the trailing 8-day window.
    fn advance_cycle(&mut self, minutes: f64, on_duty: bool) {
        self.cycle_clock_min += minutes;
        if on_duty {
            self.restart_off_min = 0.0;
            match self.cycle_entries.last_mut() {
                Some(last)
                    if (last.end_min / 60.0).floor() == (self.cycle_clock_min / 60.0).floor() =>
                {
                    last.on_duty_min += minutes;
                    last.end_min = self.cycle_clock_min;
                }
                _ => self.cycle_entries.push(CycleEntry {
                    end_min: self.cycle_clock_min,
                    on_duty_min: minutes,
                }),
            }
        } else {
            self.restart_off_min += minutes;
            if self.restart_off_min >= self.restart_threshold_min() {
                self.cycle_entries.clear();
                self.warned.retain(|w| !w.starts_with("cycle:"));
            }
        }
        self.cycle_entries
            .retain(|e| e.end_min > self.cycle_clock_min - CYCLE_WINDOW_MIN);
    }

    /// On-duty minutes currently in the cycle ledger.
    pub fn cycle_used_min(&self) -> f64 {
        self.cycle_entries.iter().map(|e| e.on_duty_min).sum()
    }

    /// On-duty minutes in ledger entries ending within a trailing window.
    pub fn cycle_used_min_within(&self, window_min: f64) -> f64 {
        self.cycle_entries
            .iter()
            .filter(|entry| entry.end_min > self.cycle_clock_min - window_min)
            .map(|entry| entry.on_duty_min)
            .sum()
    }

    /// Minutes left on the selected cycle for a mode, None when unenforced.
    pub fn cycle_remaining_min(&self, mode: &str) -> Option<f64> {
        self.cycle_limit_min(mode)
            .map(|limit| limit - self.cycle_used_min_within(self.cycle_window_min()))
    }

    /// Minutes of off-duty time still needed for the selected restart.
    pub fn restart_remaining_min(&self) -> f64 {
        (self.restart_threshold_min() - self.restart_off_min).max(0.0)
    }

    /// Select new rules and clear limit warnings so the next thresholds fire.
    pub fn set_rules(&mut self, rules: HosRules) -> bool {
        if self.rules == rules {
            return false;
        }
        self.rules = rules;
        self.warned.retain(|key| {
            !["drive:", "duty:", "shift:", "break:", "cycle:"]
                .iter()
                .any(|prefix| key.starts_with(prefix))
        });
        true
    }

    fn restart_threshold_min(&self) -> f64 {
        match self.rules {
            HosRules::CanadaSouth60 | HosRules::CanadaNorth60 => CANADA_RESTART_MIN,
            HosRules::Alaska => ALASKA_RESTART_MIN,
            HosRules::Us => RESTART_MIN,
        }
    }

    fn cycle_limit_min(&self, mode: &str) -> Option<f64> {
        cycle_limit(mode)?;
        Some(match self.rules {
            HosRules::Us => super::CYCLE_LIMIT_MIN,
            HosRules::Alaska => ALASKA_CYCLE_LIMIT_MIN,
            HosRules::CanadaSouth60 => CANADA_SOUTH_CYCLE_LIMIT_MIN,
            HosRules::CanadaNorth60 => CANADA_NORTH_CYCLE_LIMIT_MIN,
        })
    }

    fn cycle_window_min(&self) -> f64 {
        match self.rules {
            HosRules::CanadaSouth60 | HosRules::CanadaNorth60 => CANADA_CYCLE_WINDOW_MIN,
            HosRules::Us | HosRules::Alaska => CYCLE_WINDOW_MIN,
        }
    }

    fn non_us_limits(&self) -> Option<RuleLimits> {
        Some(match self.rules {
            HosRules::Us => return None,
            HosRules::Alaska => RuleLimits {
                drive_limit: ALASKA_DRIVE_LIMIT_MIN,
                duty_limit: ALASKA_ON_DUTY_LIMIT_MIN,
                shift_limit: None,
                cycle_limit: ALASKA_CYCLE_LIMIT_MIN,
                drive_used: self.driving_min,
                duty_used: self.shift_on_duty_min,
                shift_used: 0.0,
            },
            HosRules::CanadaSouth60 => RuleLimits {
                drive_limit: CANADA_SOUTH_DRIVE_LIMIT_MIN,
                duty_limit: CANADA_SOUTH_ON_DUTY_LIMIT_MIN,
                shift_limit: Some(CANADA_SOUTH_SHIFT_LIMIT_MIN),
                cycle_limit: CANADA_SOUTH_CYCLE_LIMIT_MIN,
                drive_used: self.ca_driving_min,
                duty_used: self.ca_on_duty_min,
                shift_used: self.ca_elapsed_min,
            },
            HosRules::CanadaNorth60 => RuleLimits {
                drive_limit: CANADA_NORTH_DRIVE_LIMIT_MIN,
                duty_limit: CANADA_NORTH_ON_DUTY_LIMIT_MIN,
                shift_limit: Some(CANADA_NORTH_SHIFT_LIMIT_MIN),
                cycle_limit: CANADA_NORTH_CYCLE_LIMIT_MIN,
                drive_used: self.ca_driving_min,
                duty_used: self.ca_on_duty_min,
                shift_used: self.ca_elapsed_min,
            },
        })
    }

    fn advance_canada_rest(&mut self, minutes: f64, consecutive_off_before: f64) {
        self.ca_elapsed_min += minutes;
        if consecutive_off_before < 8.0 * 60.0 && self.restart_off_min >= 8.0 * 60.0 {
            self.ca_driving_min = 0.0;
            self.ca_on_duty_min = 0.0;
            self.ca_elapsed_min = (self.restart_off_min - 8.0 * 60.0).max(0.0);
        }
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
        let event = HosEvent::new_with_shift_on_duty(
            status,
            minutes,
            self.driving_min,
            self.duty_min,
            if self.rules == HosRules::Us {
                0.0
            } else {
                self.shift_on_duty_min
            },
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
        self.shift_on_duty_min =
            second_event.shift_on_duty_before - self.split_shift_after_rest(first);
        self.since_break_min = 0.0;
        self.split_credit_key = Some(key);
        self.warned.retain(|w| {
            !(w.starts_with("drive:")
                || w.starts_with("duty:")
                || w.starts_with("shift:")
                || w.starts_with("break:"))
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

    fn split_shift_after_rest(&self, event: usize) -> f64 {
        if let Some((first, second)) = self.previous_qualifying_split_pair(event) {
            return self.split_rest_history[second].shift_on_duty_before
                - self.split_shift_after_rest(first);
        }
        self.split_rest_history[event].shift_on_duty_before
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
        let consecutive_off_before = self.restart_off_min;
        self.advance_cycle(minutes, false);
        self.advance_canada_rest(minutes, consecutive_off_before);
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
    pub(crate) fn statuses(&self, mode: &str) -> Vec<HosLimit> {
        let Some((us_drive_limit, us_duty_limit, break_after)) = limits(mode) else {
            return Vec::new();
        };
        if self.rules == HosRules::Us {
            return vec![
                HosLimit {
                    kind: "break",
                    remaining_min: break_after - self.since_break_min,
                    due: "you must take a 30 minute break at a rest stop",
                },
                HosLimit {
                    kind: "drive",
                    remaining_min: us_drive_limit - self.driving_min,
                    due: "your driving time for this shift ends. You need 10 hours of sleep",
                },
                HosLimit {
                    kind: "duty",
                    remaining_min: us_duty_limit - self.duty_min,
                    due: "your duty window closes. You need 10 hours of sleep",
                },
                HosLimit {
                    kind: "cycle",
                    remaining_min: cycle_limit(mode).unwrap_or(0.0) - self.cycle_used_min(),
                    due: "your 70-hour cycle is used up. You need a 34-hour restart",
                },
            ];
        }

        let rule_limits = self.non_us_limits().expect("non-US rules selected");
        let drive_due = match self.rules {
            HosRules::Alaska => "your Alaska driving limit is reached. You need 10 hours off",
            HosRules::CanadaSouth60 | HosRules::CanadaNorth60 => {
                "your Canadian driving limit is reached. You need 8 hours off"
            }
            HosRules::Us => unreachable!(),
        };
        let duty_due = match self.rules {
            HosRules::Alaska => "your 20 hours on duty are used. You need 10 hours off",
            HosRules::CanadaSouth60 | HosRules::CanadaNorth60 => {
                "your Canadian on-duty limit is reached. You need 8 hours off"
            }
            HosRules::Us => unreachable!(),
        };
        let cycle_due = match self.rules {
            HosRules::Alaska => "your 80-hour cycle is used up. You need a 34-hour restart",
            HosRules::CanadaSouth60 => {
                "your 70-hour Canadian cycle is used up. You need 36 hours off"
            }
            HosRules::CanadaNorth60 => {
                "your 80-hour Canadian cycle is used up. You need 36 hours off"
            }
            HosRules::Us => unreachable!(),
        };
        let mut statuses = vec![
            HosLimit {
                kind: "drive",
                remaining_min: rule_limits.drive_limit - rule_limits.drive_used,
                due: drive_due,
            },
            HosLimit {
                kind: "duty",
                remaining_min: rule_limits.duty_limit - rule_limits.duty_used,
                due: duty_due,
            },
        ];
        if let Some(shift_limit) = rule_limits.shift_limit {
            statuses.push(HosLimit {
                kind: "shift",
                remaining_min: shift_limit - rule_limits.shift_used,
                due:
                    "your shift has run too long since your last 8 hours off. You need 8 hours off",
            });
        }
        statuses.push(HosLimit {
            kind: "cycle",
            remaining_min: self.cycle_remaining_min(mode).unwrap_or(0.0),
            due: cycle_due,
        });
        statuses
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
        self.statuses(mode)
            .iter()
            .any(|s| self.limit_is_blown(s.remaining_min))
    }

    /// Limit kinds currently blown, in status order.
    pub fn blown_kinds(&self, mode: &str) -> Vec<&'static str> {
        self.statuses(mode)
            .iter()
            .filter(|s| self.limit_is_blown(s.remaining_min))
            .map(|s| s.kind)
            .collect()
    }

    fn limit_is_blown(&self, remaining_min: f64) -> bool {
        remaining_min <= 0.0
    }

    /// A missed 30-minute break, with drive and duty still legal.
    pub fn is_break_only_violation(&self, mode: &str) -> bool {
        let blown = self.blown_kinds(mode);
        blown.len() == 1 && blown[0] == "break"
    }

    /// Out-of-service time for the current violation under the selected rules.
    pub fn out_of_service_minutes(&self, mode: &str) -> f64 {
        if self.is_break_only_violation(mode) {
            return BREAK_MIN;
        }
        let blown = self.blown_kinds(mode);
        if blown.contains(&"cycle")
            && !blown.contains(&"drive")
            && !blown.contains(&"duty")
            && !blown.contains(&"shift")
        {
            return match self.rules {
                HosRules::CanadaSouth60 | HosRules::CanadaNorth60 => CANADA_RESTART_MIN,
                HosRules::Alaska => ALASKA_RESTART_MIN,
                HosRules::Us => RESTART_MIN,
            };
        }
        if matches!(
            self.rules,
            HosRules::CanadaSouth60 | HosRules::CanadaNorth60
        ) {
            return if blown.contains(&"cycle") {
                CANADA_RESTART_MIN
            } else {
                8.0 * 60.0
            };
        }
        SLEEP_MIN
    }

    /// Plain spoken phrases for every limit currently blown.
    ///
    /// The roadside out-of-service stop uses these BEFORE the reset wipes
    /// the ledger, so the officer can say exactly why the order stands.
    pub fn violation_causes(&self, mode: &str) -> Vec<String> {
        self.statuses(mode)
            .iter()
            .filter(|s| self.limit_is_blown(s.remaining_min))
            .filter_map(|s| {
                let cause = match (self.rules, s.kind) {
                    (HosRules::Us, "drive") => {
                        "you had driven past the 11-hour driving limit".to_string()
                    }
                    (HosRules::Us, "duty") => "your 14-hour duty window had expired".to_string(),
                    (HosRules::Us, "break") => {
                        "you were past the 30-minute break requirement".to_string()
                    }
                    (HosRules::Us, "cycle") => "you had used up your 70-hour cycle".to_string(),
                    (HosRules::Alaska, "drive") => {
                        "you had passed the 15-hour driving limit".to_string()
                    }
                    (HosRules::Alaska, "duty") => {
                        "you had passed the 20-hour on-duty limit".to_string()
                    }
                    (HosRules::Alaska, "cycle") => "you had used up your 80-hour cycle".to_string(),
                    (HosRules::CanadaSouth60, "drive") => {
                        "you had passed the 13-hour driving limit".to_string()
                    }
                    (HosRules::CanadaNorth60, "drive") => {
                        "you had passed the 15-hour driving limit".to_string()
                    }
                    (HosRules::CanadaSouth60, "duty") => {
                        "you had passed the 14-hour on-duty limit".to_string()
                    }
                    (HosRules::CanadaNorth60, "duty") => {
                        "you had passed the 18-hour on-duty limit".to_string()
                    }
                    (HosRules::CanadaSouth60, "shift") => {
                        "you had driven 16 hours after your last 8 hours off".to_string()
                    }
                    (HosRules::CanadaNorth60, "shift") => {
                        "you had driven 20 hours after your last 8 hours off".to_string()
                    }
                    (HosRules::CanadaSouth60, "cycle") => {
                        "you had used up your 70-hour cycle".to_string()
                    }
                    (HosRules::CanadaNorth60, "cycle") => {
                        "you had used up your 80-hour cycle".to_string()
                    }
                    (_, _) => return None,
                };
                Some(cause)
            })
            .collect()
    }

    /// Spoken status for the C key and Tab report.
    pub fn summary(&self, mode: &str) -> String {
        if is_non_enforced(mode) {
            return ENFORCEMENT_OFF.to_string();
        }
        if self.rules != HosRules::Us {
            return self.summary_non_us(mode);
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
                .filter(|s| self.limit_is_blown(s.remaining_min))
                .map(|s| s.kind)
                .collect();
            if blown == ["break"] {
                return "Hours of service: 30-minute break overdue.".to_string();
            }
            if blown == ["cycle"] {
                return "Hours of service: your 70-hour cycle is used up. \
                        Take a 34-hour restart, or wait for hours to age off your \
                        8-day ledger."
                    .to_string();
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
        if let Some(remaining) = self
            .cycle_remaining_min(mode)
            .filter(|r| *r <= CYCLE_SPEAK_MIN)
        {
            sentences.push(format!(
                "Cycle: {} of 70 hours used, {} left.",
                duration_text(self.cycle_used_min() / 60.0),
                duration_text(remaining / 60.0),
            ));
        }
        if let Some(pending) = self.split_pending_summary() {
            sentences.push(pending.to_string());
        }
        sentences.join(" ")
    }

    fn summary_non_us(&self, mode: &str) -> String {
        let rule_limits = self.non_us_limits().expect("non-US rules selected");
        if self.in_violation(mode) {
            let blown: Vec<&str> = self
                .statuses(mode)
                .iter()
                .filter(|status| self.limit_is_blown(status.remaining_min))
                .map(|status| status.kind)
                .collect();
            let advice = reset_advice(blown.first().copied().unwrap_or("drive"), self.rules);
            if blown == ["cycle"] {
                let cycle_text = match self.rules {
                    HosRules::Alaska => "your 80-hour cycle is used up",
                    HosRules::CanadaSouth60 => "your 70-hour Canadian cycle is used up",
                    HosRules::CanadaNorth60 => "your 80-hour Canadian cycle is used up",
                    HosRules::Us => unreachable!(),
                };
                return format!("Hours of service: {cycle_text}. {advice}");
            }
            return format!("Hours of service: past your limit. {advice}");
        }

        let drive_left = py_max(0.0, rule_limits.drive_limit - rule_limits.drive_used) / 60.0;
        let duty_left = py_max(0.0, rule_limits.duty_limit - rule_limits.duty_used) / 60.0;
        let shift_left = rule_limits
            .shift_limit
            .map(|limit| py_max(0.0, limit - rule_limits.shift_used) / 60.0);
        let lead = match (self.rules, shift_left) {
            (HosRules::Alaska, _) => format!(
                "Alaska hours rules. {} of driving and {} on duty left.",
                duration_text(drive_left),
                duration_text(duty_left),
            ),
            (HosRules::CanadaSouth60, Some(shift_left)) => format!(
                "Canadian hours rules. {} of driving, {} on duty, and {} of shift left.",
                duration_text(drive_left),
                duration_text(duty_left),
                duration_text(shift_left),
            ),
            (HosRules::CanadaNorth60, Some(shift_left)) => format!(
                "North of 60 hours rules. {} of driving, {} on duty, and {} of shift left.",
                duration_text(drive_left),
                duration_text(duty_left),
                duration_text(shift_left),
            ),
            (HosRules::Us, _) | (_, None) => unreachable!(),
        };
        let status = self.status.replace('_', " ");
        let cycle_clause = match self.cycle_remaining_min(mode) {
            Some(remaining) if remaining <= CYCLE_SPEAK_MIN => format!(
                " Cycle: {} used of {}, {} left.",
                duration_text(self.cycle_used_min_within(self.cycle_window_min()) / 60.0),
                duration_text(rule_limits.cycle_limit / 60.0),
                duration_text(remaining / 60.0),
            ),
            _ => String::new(),
        };
        let suffix = if self.rules == HosRules::Alaska {
            self.split_pending_summary()
                .map(|pending| format!(" {pending}"))
                .unwrap_or_default()
        } else {
            String::new()
        };
        format!("{lead} ELD status {status}.{cycle_clause}{suffix}")
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
        if self.rules != HosRules::Us {
            return vec![self.summary(mode)];
        }
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
        if let Some(remaining) = self.cycle_remaining_min(mode) {
            if remaining <= CYCLE_SPEAK_MIN {
                lines.push(format!(
                    "Cycle: {} of 70 hours used, {} left.",
                    duration_text(self.cycle_used_min() / 60.0),
                    duration_text(remaining / 60.0),
                ));
            }
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
                .any(|s| s.kind == kind && self.limit_is_blown(s.remaining_min))
        };
        if blown("drive") {
            Some("drive")
        } else if blown("duty") {
            Some("duty")
        } else if blown("shift") {
            Some("shift")
        } else if blown("cycle") {
            Some("cycle")
        } else {
            None
        }
    }

    /// Alt A: how much of this shift is spent -- not the hours on this run.
    pub fn wheel_time_summary(&self, mode: &str, terse: bool) -> String {
        if let Some(rule_limits) = self.non_us_limits() {
            let fresh = rule_limits.drive_used <= 0.0;
            let driven = duration_text(rule_limits.drive_used / 60.0);
            let lead = if terse {
                if fresh {
                    "At the wheel: no driving yet".to_string()
                } else {
                    format!("At the wheel {driven}")
                }
            } else {
                let spent = if fresh {
                    "no driving yet".to_string()
                } else {
                    format!("{driven} driving")
                };
                format!(
                    "At the wheel so far: {spent}, {} on duty this shift",
                    duration_text(rule_limits.duty_used / 60.0)
                )
            };
            let note = if limits(mode).is_none() {
                ENFORCEMENT_OFF
            } else if self.shift_over_kind(mode).is_some() {
                "Out of hours."
            } else {
                ""
            };
            return format!("{lead}. {note}").trim_end().to_string();
        }
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
        if self.rules != HosRules::Us {
            if limits(mode).is_none() {
                return format!("Break: none required. {ENFORCEMENT_OFF}");
            }
            if let Some(kind) = self.shift_over_kind(mode) {
                return format!(
                    "No 30-minute break is required. {} {}",
                    shift_over(kind, self.rules).1,
                    reset_advice(kind, self.rules),
                );
            }
            return match self.rules {
                HosRules::Alaska => {
                    "No 30-minute break is required under Alaska hours rules.".to_string()
                }
                HosRules::CanadaSouth60 | HosRules::CanadaNorth60 => {
                    "No 30-minute break is required under Canadian hours rules.".to_string()
                }
                HosRules::Us => unreachable!(),
            };
        }
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
                "{answer}, but you are {}. {}",
                shift_over(kind, self.rules).0,
                reset_advice(kind, self.rules)
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
        if let Some(rule_limits) = self.non_us_limits() {
            if let Some(kind) = self.shift_over_kind(mode) {
                return format!(
                    "{} {}",
                    shift_over(kind, self.rules).1,
                    reset_advice(kind, self.rules)
                );
            }
            let drive_left = py_max(0.0, rule_limits.drive_limit - rule_limits.drive_used) / 60.0;
            let duty_left = py_max(0.0, rule_limits.duty_limit - rule_limits.duty_used) / 60.0;
            let mut clauses = vec![
                format!("Driving time left: {}", duration_text(drive_left)),
                format!("On-duty time left: {}", duration_text(duty_left)),
            ];
            if let Some(shift_limit) = rule_limits.shift_limit {
                let shift_left = py_max(0.0, shift_limit - rule_limits.shift_used) / 60.0;
                clauses.push(format!("Shift time left: {}", duration_text(shift_left)));
            }
            return if terse {
                format!("{}.", clauses.join(", "))
            } else {
                format!("{}.", clauses.join(". "))
            };
        }
        let (drive_left, duty_left, break_left) = self.hours_left(mode);
        if let Some(kind) = self.shift_over_kind(mode) {
            return format!(
                "{} {}",
                shift_over(kind, self.rules).1,
                reset_advice(kind, self.rules)
            );
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
        if let Some(rule_limits) = self.non_us_limits() {
            let cycle_left = self.cycle_remaining_min(mode).unwrap_or(f64::INFINITY);
            let (drive_phrase, duty_phrase, cycle_phrase, cycle_name) = match self.rules {
                HosRules::Alaska => (
                    "your Alaska driving limit runs out",
                    "your 20-hour on-duty limit runs out",
                    "your 80-hour cycle runs out",
                    "80-hour cycle",
                ),
                HosRules::CanadaSouth60 => (
                    "your Canadian driving limit runs out",
                    "your Canadian on-duty limit runs out",
                    "your 70-hour Canadian cycle runs out",
                    "70-hour Canadian cycle",
                ),
                HosRules::CanadaNorth60 => (
                    "your Canadian driving limit runs out",
                    "your Canadian on-duty limit runs out",
                    "your 80-hour Canadian cycle runs out",
                    "80-hour Canadian cycle",
                ),
                HosRules::Us => unreachable!(),
            };
            let mut candidates = vec![
                (
                    "drive",
                    rule_limits.drive_limit - rule_limits.drive_used,
                    drive_phrase,
                    "driving limit",
                ),
                (
                    "duty",
                    rule_limits.duty_limit - rule_limits.duty_used,
                    duty_phrase,
                    "on-duty limit",
                ),
            ];
            if let Some(shift_limit) = rule_limits.shift_limit {
                let shift_phrase = match self.rules {
                    HosRules::CanadaSouth60 => {
                        "your shift reaches 16 hours after your last 8 hours off"
                    }
                    HosRules::CanadaNorth60 => {
                        "your shift reaches 20 hours after your last 8 hours off"
                    }
                    _ => unreachable!(),
                };
                candidates.push((
                    "shift",
                    shift_limit - rule_limits.shift_used,
                    shift_phrase,
                    "shift limit",
                ));
            }
            candidates.push(("cycle", cycle_left, cycle_phrase, cycle_name));
            let mut nearest = candidates[0];
            for candidate in &candidates[1..] {
                if candidate.1 < nearest.1 {
                    nearest = *candidate;
                }
            }
            if eta_min < nearest.1 {
                return format!(" You would arrive before {}.", nearest.2);
            }
            return format!(
                " Your {} comes about {} before you would reach it.",
                nearest.3,
                duration_text((eta_min - nearest.1) / 60.0),
            );
        }
        let (drive_limit, duty_limit, break_after) =
            limits(mode).expect("HOS mode is realistic or relaxed");
        let drive_left = drive_limit - self.driving_min;
        let duty_left = duty_limit - self.duty_min;
        let break_left = break_after - self.since_break_min;
        let cycle_left = self.cycle_remaining_min(mode).unwrap_or(f64::INFINITY);
        let mut candidates = vec![
            (
                "drive",
                drive_left,
                "your driving time for this shift runs out",
            ),
            ("duty", duty_left, "your duty window closes"),
            ("cycle", cycle_left, "your 70-hour cycle runs out"),
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
            "cycle" => "70-hour cycle",
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
        let mut data = json!({
            "driving_min": self.driving_min,
            "duty_min": self.duty_min,
            "since_break_min": self.since_break_min,
            "status": self.status,
            "non_driving_min": self.non_driving_min,
            "off_duty_min": self.off_duty_min,
            "warned": self.warned,
            "history": self.history.iter().map(|event| event.to_dict()).collect::<Vec<_>>(),
            "split_rest_history": self.split_rest_history.iter().map(|event| event.to_dict()).collect::<Vec<_>>(),
            "split_credit_key": self.split_credit_key,
            "cycle_clock_min": self.cycle_clock_min,
            "restart_off_min": self.restart_off_min,
            "cycle_entries": self
                .cycle_entries
                .iter()
                .map(|e| json!({"end_min": e.end_min, "on_duty_min": e.on_duty_min}))
                .collect::<Vec<_>>(),
        });
        if self.rules != HosRules::Us {
            let obj = data.as_object_mut().expect("clock serializes as an object");
            obj.insert("hos_rules".to_string(), json!(self.rules.key()));
            obj.insert(
                "history".to_string(),
                json!(self
                    .history
                    .iter()
                    .map(HosEvent::to_dict_with_shift_on_duty)
                    .collect::<Vec<_>>()),
            );
            obj.insert(
                "split_rest_history".to_string(),
                json!(self
                    .split_rest_history
                    .iter()
                    .map(HosEvent::to_dict_with_shift_on_duty)
                    .collect::<Vec<_>>()),
            );
        }
        if let Some(obj) = data.as_object_mut() {
            for (key, value) in [
                ("shift_on_duty_min", self.shift_on_duty_min),
                ("ca_driving_min", self.ca_driving_min),
                ("ca_on_duty_min", self.ca_on_duty_min),
                ("ca_elapsed_min", self.ca_elapsed_min),
            ] {
                if value != 0.0 {
                    obj.insert(key.to_string(), json!(value));
                }
            }
        }
        data
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
        let mut cycle_entries = Vec::new();
        if let Some(Value::Array(raw_entries)) = obj.get("cycle_entries") {
            for raw in raw_entries {
                let Some(entry) = raw.as_object() else {
                    continue;
                };
                let (Some(end_min), Some(on_duty_min)) = (
                    py_float_or(entry.get("end_min"), 0.0),
                    py_float_or(entry.get("on_duty_min"), 0.0),
                ) else {
                    continue;
                };
                cycle_entries.push(CycleEntry {
                    end_min,
                    on_duty_min,
                });
            }
        }
        let split_credit_key = match obj.get("split_credit_key") {
            None | Some(Value::Null) => None,
            Some(value) => Some(py_str(value)),
        };
        let rules = obj
            .get("hos_rules")
            .and_then(Value::as_str)
            .map(HosRules::from_key)
            .unwrap_or_default();
        keep_last(&mut history, HOS_HISTORY_MAX);
        keep_last(&mut split_rest_history, HOS_SPLIT_REST_HISTORY_MAX);
        Some(Self {
            driving_min: py_float_or(obj.get("driving_min"), 0.0)?,
            duty_min: py_float_or(obj.get("duty_min"), 0.0)?,
            rules,
            shift_on_duty_min: py_float_or(obj.get("shift_on_duty_min"), 0.0).unwrap_or(0.0),
            ca_driving_min: py_float_or(obj.get("ca_driving_min"), 0.0).unwrap_or(0.0),
            ca_on_duty_min: py_float_or(obj.get("ca_on_duty_min"), 0.0).unwrap_or(0.0),
            ca_elapsed_min: py_float_or(obj.get("ca_elapsed_min"), 0.0).unwrap_or(0.0),
            since_break_min: py_float_or(obj.get("since_break_min"), 0.0)?,
            status,
            non_driving_min: py_float_or(obj.get("non_driving_min"), 0.0)?,
            off_duty_min: py_float_or(obj.get("off_duty_min"), 0.0)?,
            warned,
            history,
            split_rest_history,
            split_credit_key,
            cycle_clock_min: py_float_or(obj.get("cycle_clock_min"), 0.0).unwrap_or(0.0),
            cycle_entries,
            restart_off_min: py_float_or(obj.get("restart_off_min"), 0.0).unwrap_or(0.0),
        })
    }
}
