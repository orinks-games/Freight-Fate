//! Hours of service, ELD duty status, fatigue, and the day/night clock.
//!
//! Simplified FMCSA-style rules running entirely on the in-game clock (the
//! trip's `game_minutes`, never wall time): 11 hours of driving after a
//! 10-hour reset, a 14-hour duty window after coming on duty, and a
//! 30-minute break after 8 cumulative hours of driving. The break may be any
//! 30 consecutive non-driving minutes, including on-duty-not-driving work.
//!
//! The model includes 7/3 and 8/2 sleeper split credits but intentionally skips
//! 60/70-hour cycle limits for now; the save schema records explicit duty
//! statuses so those rules can be added without changing how drive, facility,
//! and POI time is classified.
//!
//! Everything here is deterministic and platform-free so the headless tests
//! can exercise the rules directly.
//!
//! Port of `freight_fate/sim/hos.py`. The shift ledger lives in `clock`,
//! the logbook in `duty_log`; the constants, fatigue model, day/night clock
//! and overnight parking rolls stay here.

mod clock;
mod duty_log;
mod pyjson;
#[cfg(test)]
mod tests;
mod warnings;

pub use clock::{HosClock, HosEvent, HosLimit};
pub use duty_log::{DutyLog, DutySegment, DutyTotals};

use crate::pyfmt::{fmt_f, pct02, py_int, round_py_int};
use crate::pyrandom::PyRandom;

pub const HOS_MODES: [&str; 3] = ["realistic", "relaxed", "debug_off"];
pub const HOS_NON_ENFORCED_MODES: [&str; 2] = ["off", "debug_off"];
pub const DUTY_STATUSES: [&str; 4] = [
    "driving",
    "on_duty_not_driving",
    "off_duty",
    "sleeper_berth",
];
pub const RODS_WINDOW_HOURS: f64 = 8.0 * 24.0;
pub const SPLIT_SHORT_MIN: f64 = 120.0;
pub const SPLIT_SHORT_ALT_MIN: f64 = 180.0;
pub const SPLIT_LONG_MIN: f64 = 420.0;
pub const SPLIT_LONG_ALT_MIN: f64 = 480.0;
pub const HOS_HISTORY_MAX: usize = 96;
pub const HOS_SPLIT_REST_HISTORY_MAX: usize = 16;

/// minimum break that resets the 8-hour rule
pub const BREAK_MIN: f64 = 30.0;
/// a full 10-hour off-duty reset
pub const SLEEP_MIN: f64 = 600.0;

/// (drive limit, duty window, driving allowed before a 30-minute break),
/// all in game minutes.
pub const REALISTIC_LIMITS: (f64, f64, f64) = (11.0 * 60.0, 14.0 * 60.0, 8.0 * 60.0);
/// Relaxed keeps the same 11-hour drive, 14-hour window, and 8-hour-before-break
/// rule as realistic. What it eases is money and odds, not the clock.
pub const RELAXED_LIMITS: (f64, f64, f64) = REALISTIC_LIMITS;

/// `LIMITS[mode]`: the enforced limits of a mode, None for the modes that
/// enforce nothing.
pub fn limits(mode: &str) -> Option<(f64, f64, f64)> {
    match mode {
        "realistic" => Some(REALISTIC_LIMITS),
        "relaxed" => Some(RELAXED_LIMITS),
        _ => None,
    }
}

/// `mode in HOS_NON_ENFORCED_MODES`.
pub fn is_non_enforced(mode: &str) -> bool {
    HOS_NON_ENFORCED_MODES.contains(&mode)
}

/// `status in DUTY_STATUSES`.
pub fn is_duty_status(status: &str) -> bool {
    DUTY_STATUSES.contains(&status)
}

/// The spoken name of a duty status.
pub fn duty_status_label(status: &str) -> String {
    match status {
        "driving" => "driving".to_string(),
        "on_duty_not_driving" => "on duty, not driving".to_string(),
        "off_duty" => "off duty".to_string(),
        "sleeper_berth" => "sleeper berth".to_string(),
        other => other.replace('_', " "),
    }
}

// Relaxed mode keeps random road hazards rare so the player can focus on
// driver responsibility -- hours of service, fueling, repairs, fatigue --
// instead of constant emergency braking. Realistic and debug modes leave
// hazard frequency untouched.
pub const RELAXED_HAZARD_SCALE: f64 = 0.2;

/// Random road-hazard frequency multiplier for a difficulty mode.
pub fn hazard_scale(mode: &str) -> f64 {
    if mode == "relaxed" {
        RELAXED_HAZARD_SCALE
    } else {
        1.0
    }
}

pub const WARNING_THRESHOLDS_MIN: [f64; 3] = [120.0, 60.0, 30.0];

pub fn warning_is_urgent(message: &str) -> bool {
    message.starts_with("Hours of service violation:")
}

/// The Python raised `ValueError` here; the ledger methods panic with the
/// same message, since a negative or non-finite increment is a programming
/// error upstream, never player input.
fn positive_minutes(minutes: f64) -> f64 {
    if !minutes.is_finite() || minutes < 0.0 {
        panic!("HOS time increments must be finite positive minutes");
    }
    minutes
}

/// A span of time as it is spoken: minutes under an hour, whole hours as a
/// whole number ("3 hours", never "three point zero"), the rest to a tenth.
pub fn duration_text(hours: f64) -> String {
    let minutes = pyjson::py_max(0.0, hours * 60.0);
    if minutes < 60.0 {
        let text = fmt_f(minutes, 0);
        return if text == "1" {
            "1 minute".to_string()
        } else {
            format!("{text} minutes")
        };
    }
    let text = fmt_f(minutes / 60.0, 1);
    match text.strip_suffix(".0") {
        Some("1") => "1 hour".to_string(),
        Some(whole) => format!("{whole} hours"),
        None => format!("{text} hours"),
    }
}

// ---------------------------------------------------------------------------
// Fatigue
// ---------------------------------------------------------------------------

/// yawns and a spoken warning
pub const FATIGUE_DROWSY: f64 = 60.0;
/// rumble strip drift, urgent warning
pub const FATIGUE_SEVERE: f64 = 80.0;

/// Escalating fines for failed roadside inspections while over hours.
pub const HOS_FINES: [f64; 4] = [200.0, 500.0, 1000.0, 2000.0];
/// Relaxed mode keeps the 11/14/30 clock and halves the ticket.
pub const RELAXED_FINE_SCALE: f64 = 0.5;
/// Relaxed mode also thins scale-house inspection-lane odds.
pub const RELAXED_INSPECTION_SCALE: f64 = 0.5;

/// Fine for this inspection, scaled down in relaxed mode.
pub fn hos_fine(mode: &str, prior_count: i64) -> f64 {
    let base = HOS_FINES[(prior_count as usize).min(HOS_FINES.len() - 1)];
    if mode == "relaxed" {
        base * RELAXED_FINE_SCALE
    } else {
        base
    }
}

pub const HOS_REPUTATION_HIT: f64 = 3.0;
pub const FATIGUE_COFFEE_RELIEF: f64 = 8.0;
pub const FATIGUE_BREAK_RELIEF: f64 = 35.0;
pub const FATIGUE_SHOULDER_FLOOR: f64 = 30.0;
/// How long before a sleep/duty limit the shoulder-sleep option opens up, paired
/// with a reachability check (no stop you can legally reach before the limit).
/// A real driver starts hunting for parking a couple of hours out, not in the
/// last half hour -- 30 min left you stranded with no action available.
pub const SHOULDER_SLEEP_LIMIT_BUFFER_MIN: f64 = 120.0;
pub const SHOULDER_FINE_CHANCE: f64 = 0.15;
/// Base only: models/enforcement.citation_fine scales it for priors and zone.
pub const SHOULDER_FINE: f64 = 400.0;
pub const SHOULDER_DAMAGE_CHANCE: f64 = 0.10;
pub const SHOULDER_DAMAGE_PCT: f64 = 3.0;

/// Fatigue points per game minute of continuous driving.
///
/// About 8 daytime hours to the drowsy threshold; night driving gets
/// there in under 6.
pub fn fatigue_rate_per_min(night: bool) -> f64 {
    if night {
        0.17
    } else {
        0.115
    }
}

/// Fatigue points gained while awake for `minutes` of game time starting at
/// `start_game_hours`. Uses [`fatigue_rate_per_min`] with the same day/night
/// clock as driving, so sitting awake off duty matches the road model.
///
/// Pass a local wall-clock hour (`timezones::to_local`) so day and night
/// follow the sun where the driver actually is.
pub fn awake_fatigue_gain(start_game_hours: f64, minutes: f64) -> f64 {
    awake_fatigue_gain_scaled(start_game_hours, minutes, |_| 1.0)
}

/// [`awake_fatigue_gain`] with each minute's rate multiplied by
/// `scale(hours_since_start)`, for factors that change part way through the
/// stretch (a coffee buff wearing off, say).
pub fn awake_fatigue_gain_scaled(
    start_game_hours: f64,
    minutes: f64,
    scale: impl Fn(f64) -> f64,
) -> f64 {
    let minutes = finite_minutes(minutes);
    if minutes <= 0.0 {
        return 0.0;
    }
    let whole = minutes.floor() as i64;
    let mut gain = 0.0;
    for i in 0..whole {
        let offset = (i as f64) / 60.0;
        gain += fatigue_rate_per_min(is_night(start_game_hours + offset)) * scale(offset);
    }
    let frac = minutes - whole as f64;
    if frac > 0.0 {
        let offset = (whole as f64) / 60.0;
        gain += fatigue_rate_per_min(is_night(start_game_hours + offset)) * scale(offset) * frac;
    }
    gain
}

fn finite_minutes(minutes: f64) -> f64 {
    if minutes.is_finite() {
        minutes.max(0.0)
    } else {
        0.0
    }
}

/// Share of the continuous-driving fatigue rate a classroom day costs.
///
/// ASSUMED tuning, not a measured figure. [`fatigue_rate_per_min`] is the
/// rate for driving, which carries a vigilance load a seat in a classroom
/// does not; half that rate puts an 8-hour day class taken at 10 near 38.
pub const CLASSROOM_FATIGUE_FACTOR: f64 = 0.5;

/// Longest single class day, in minutes. Schools teach in day-length blocks.
pub const COURSE_CLASS_DAY_MAX_MIN: f64 = 8.0 * 60.0;

/// Local hour a multi-day course's class day starts: 8 AM to 4 PM. Schools
/// teach in the daytime and students sleep at night.
pub const COURSE_CLASS_DAY_START_LOCAL_H: f64 = 8.0;

/// A course this long or longer (minutes) runs as several class days with a
/// night's sleep between them, not one unbroken stretch awake.
///
/// This is a cliff: walking in at 50 at 8 AM, a 599-minute course ends near
/// 84.4 and a 600-minute one at 27.6, the fixed daytime class day. No course
/// sits near it -- the single-day courses run 4 to 8 hours, the multi-day
/// one 24.
pub const COURSE_MULTI_DAY_MIN: f64 = 10.0 * 60.0;

/// Fatigue when a course of `minutes` starting at local hour
/// `start_local_hours` ends, given `arrival` fatigue.
///
/// A short course adds classroom-rate awake fatigue to what the driver
/// walked in with, on the real clock: a short course can run into the
/// evening. A course of [`COURSE_MULTI_DAY_MIN`] or more is several class
/// days with sleep between, so only the last class day counts, scored as a
/// fixed 8 AM to 4 PM local block whatever hour the course started, and it
/// replaces the arrival fatigue rather than adding to it.
///
/// `scale(hours_since_course_start)` multiplies each minute's rate on top
/// of the classroom factor: food and drink buffs, the driving mode's
/// fatigue setting.
pub fn course_fatigue(
    arrival: f64,
    start_local_hours: f64,
    minutes: f64,
    scale: impl Fn(f64) -> f64,
) -> f64 {
    let minutes = finite_minutes(minutes);
    let classroom = |h: f64| CLASSROOM_FATIGUE_FACTOR * scale(h);
    if minutes >= COURSE_MULTI_DAY_MIN {
        let class_start = last_class_day_start(start_local_hours, minutes);
        let offset_h = class_start - start_local_hours;
        let gain = awake_fatigue_gain_scaled(class_start, COURSE_CLASS_DAY_MAX_MIN, |h| {
            classroom(offset_h + h)
        });
        gain.clamp(0.0, 100.0)
    } else {
        (arrival + awake_fatigue_gain_scaled(start_local_hours, minutes, classroom)).min(100.0)
    }
}

/// Local hours at which a multi-day course's last class day starts: the
/// latest 8 AM whose class day ends by the end of the course, or, if none
/// fits after the start, the first 8 AM after it. Only buff timing reads
/// where the block falls; the class day itself is always daytime.
fn last_class_day_start(start_local_hours: f64, minutes: f64) -> f64 {
    let day_h = COURSE_CLASS_DAY_MAX_MIN / 60.0;
    let latest_start = start_local_hours + minutes / 60.0 - day_h;
    let latest = latest_start - (latest_start - COURSE_CLASS_DAY_START_LOCAL_H).rem_euclid(24.0);
    if latest >= start_local_hours {
        latest
    } else {
        start_local_hours + (COURSE_CLASS_DAY_START_LOCAL_H - start_local_hours).rem_euclid(24.0)
    }
}

/// Scale factor for hazard reaction windows: 1.0 fresh, 0.6 exhausted.
pub fn reaction_window_mult(fatigue: f64) -> f64 {
    if fatigue <= FATIGUE_DROWSY {
        return 1.0;
    }
    let t = pyjson::py_min(1.0, (fatigue - FATIGUE_DROWSY) / (100.0 - FATIGUE_DROWSY));
    1.0 - 0.4 * t
}

/// Fatigue after a 30-minute break.
pub fn rest_break(fatigue: f64) -> f64 {
    pyjson::py_max(0.0, fatigue - FATIGUE_BREAK_RELIEF)
}

/// Fatigue after a short food and coffee stop.
pub fn rest_coffee_break(fatigue: f64) -> f64 {
    pyjson::py_max(0.0, fatigue - FATIGUE_COFFEE_RELIEF)
}

/// Fatigue after a proper 10-hour sleep.
pub fn rest_sleep(_fatigue: f64) -> f64 {
    0.0
}

/// Shoulder parking is poor rest: fatigue never drops below 30.
pub fn rest_shoulder(fatigue: f64) -> f64 {
    pyjson::py_min(fatigue, FATIGUE_SHOULDER_FLOOR)
}

pub fn rest_sleeper_split(fatigue: f64, minutes: f64, completed: bool) -> f64 {
    let relief = if minutes <= 180.0 { 18.0 } else { 55.0 };
    let floor = if completed { 10.0 } else { 20.0 };
    pyjson::py_min(
        fatigue,
        pyjson::py_max(floor, pyjson::py_max(0.0, fatigue - relief)),
    )
}

// ---------------------------------------------------------------------------
// Day/night clock
// ---------------------------------------------------------------------------

pub const DAWN_START: f64 = 5.0;
pub const DAY_START: f64 = 7.0;
pub const DUSK_START: f64 = 19.0;
pub const NIGHT_START: f64 = 21.0;

/// Python `game_hours % 24.0`: the result carries the divisor's sign.
pub fn clock_hour(game_hours: f64) -> f64 {
    game_hours.rem_euclid(24.0)
}

pub fn time_of_day(game_hours: f64) -> &'static str {
    let h = clock_hour(game_hours);
    if (DAWN_START..DAY_START).contains(&h) {
        return "dawn";
    }
    if (DAY_START..DUSK_START).contains(&h) {
        return "day";
    }
    if (DUSK_START..NIGHT_START).contains(&h) {
        return "dusk";
    }
    "night"
}

pub fn is_night(game_hours: f64) -> bool {
    time_of_day(game_hours) == "night"
}

/// Spoken 12-hour clock: '6 AM', '11:24 PM'.
pub fn clock_text(game_hours: f64) -> String {
    let h = clock_hour(game_hours);
    let mut hour = py_int(h);
    let mut minute = round_py_int((h - py_int(h) as f64) * 60.0);
    if minute == 60 {
        hour = (hour + 1) % 24;
        minute = 0;
    }
    let ampm = if hour < 12 { "AM" } else { "PM" };
    let h12 = match hour % 12 {
        0 => 12,
        rest => rest,
    };
    if minute == 0 {
        return format!("{h12} {ampm}");
    }
    format!("{h12}:{} {ampm}", pct02(minute))
}

// ---------------------------------------------------------------------------
// Overnight truck parking
// ---------------------------------------------------------------------------

/// 8 PM .. 4 AM
pub const PARKING_CRUNCH_START: f64 = 20.0;
pub const PARKING_CRUNCH_END: f64 = 4.0;

/// Chance the lot is full, rising through the evening; 0 outside 8 PM-4 AM.
///
/// `spaces` is the surveyed truck-parking capacity (FHWA Jason's Law via BTS
/// NTAD) when known: a handful of spots fills earlier than a big travel-center
/// lot. 0 (unsurveyed) keeps the flat baseline.
pub fn parking_full_probability(game_hours: f64, spaces: i64) -> f64 {
    let h = clock_hour(game_hours);
    if (PARKING_CRUNCH_END..PARKING_CRUNCH_START).contains(&h) {
        return 0.0;
    }
    let hours_past_8pm = (h - PARKING_CRUNCH_START).rem_euclid(24.0);
    let p = pyjson::py_min(0.8, 0.2 + 0.1 * hours_past_8pm);
    if spaces <= 0 {
        return p;
    }
    if spaces <= 15 {
        return pyjson::py_min(0.9, p + 0.15);
    }
    if spaces >= 100 {
        return p * 0.6;
    }
    if spaces >= 40 {
        return p * 0.85;
    }
    p
}

/// Deterministic per trip seed and stop, so saves and tests reproduce it.
pub fn parking_is_full(trip_seed: i64, stop_mi: f64, game_hours: f64, spaces: i64) -> bool {
    let p = parking_full_probability(game_hours, spaces);
    if p <= 0.0 {
        return false;
    }
    let mut rng = PyRandom::new_from_str(&format!(
        "parking:{trip_seed}:{}",
        round_py_int(stop_mi * 10.0)
    ));
    rng.random() < p
}

/// Deterministic 15 percent chance of a fine for shoulder parking.
pub fn shoulder_fine_due(trip_seed: i64, stop_mi: f64) -> bool {
    let mut rng = PyRandom::new_from_str(&format!(
        "shoulder:{trip_seed}:{}",
        round_py_int(stop_mi * 10.0)
    ));
    rng.random() < SHOULDER_FINE_CHANCE
}

/// Deterministic small chance of minor damage while sleeping on the shoulder.
pub fn shoulder_damage_due(trip_seed: i64, stop_mi: f64) -> bool {
    let mut rng = PyRandom::new_from_str(&format!(
        "shoulder-damage:{trip_seed}:{}",
        round_py_int(stop_mi * 10.0)
    ));
    rng.random() < SHOULDER_DAMAGE_CHANCE
}
