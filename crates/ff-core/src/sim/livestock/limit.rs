//! The 28-hour limit (49 USC 80502): a clock on the load, not on the driver.
//!
//! It starts when the last gate shuts (time spent loading does not count),
//! runs through every stop and every hour of sleep, and stops when the
//! unloading begins. It moves one way, through each warning mark to Over.

use serde::{Deserialize, Serialize};

/// Read: "more than 28 consecutive hours" (49 USC 80502(a)(1)).
pub const LIMIT_HOURS: f64 = 28.0;
/// The same marks the hours-of-service warnings use: 2 hours, 1 hour and
/// 30 minutes left.
pub const LIMIT_WARNING_MARKS_MIN: [f64; 3] = [120.0, 60.0, 30.0];

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LimitMark {
    NotRunning,
    Running,
    TwoHoursLeft,
    OneHourLeft,
    ThirtyMinutesLeft,
    Over,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LimitEvent {
    /// The last gate shut.
    Start,
    /// Time passed; this much is left on the limit (negative once over).
    Tick { left_min: f64 },
    /// The unloading started.
    Stop,
}

impl LimitMark {
    /// The mark a clock with `left_min` remaining has reached.
    fn reached(left_min: f64) -> LimitMark {
        if left_min <= 0.0 {
            LimitMark::Over
        } else if left_min <= LIMIT_WARNING_MARKS_MIN[2] {
            LimitMark::ThirtyMinutesLeft
        } else if left_min <= LIMIT_WARNING_MARKS_MIN[1] {
            LimitMark::OneHourLeft
        } else if left_min <= LIMIT_WARNING_MARKS_MIN[0] {
            LimitMark::TwoHoursLeft
        } else {
            LimitMark::Running
        }
    }

    pub fn next(self, event: LimitEvent) -> LimitMark {
        match (self, event) {
            (_, LimitEvent::Stop) => LimitMark::NotRunning,
            (LimitMark::NotRunning, LimitEvent::Start) => LimitMark::Running,
            (LimitMark::NotRunning, LimitEvent::Tick { .. }) => LimitMark::NotRunning,
            (state, LimitEvent::Start) => state,
            // One way only: a step that jumps several marks (a night's
            // sleep) lands on the furthest, and nothing moves back.
            (state, LimitEvent::Tick { left_min }) => state.max(LimitMark::reached(left_min)),
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            LimitMark::NotRunning => "not_running",
            LimitMark::Running => "running",
            LimitMark::TwoHoursLeft => "two_hours_left",
            LimitMark::OneHourLeft => "one_hour_left",
            LimitMark::ThirtyMinutesLeft => "thirty_minutes_left",
            LimitMark::Over => "over",
        }
    }

    /// The warning spoken when the limit reaches this mark, if any.
    pub fn warning(self) -> Option<&'static str> {
        match self {
            LimitMark::TwoHoursLeft => Some("2 hours left on the 28-hour limit."),
            LimitMark::OneHourLeft => Some("1 hour left on the 28-hour limit."),
            LimitMark::ThirtyMinutesLeft => Some("30 minutes left on the 28-hour limit."),
            LimitMark::Over => Some("The cattle have been aboard 28 hours."),
            LimitMark::NotRunning | LimitMark::Running => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::LimitMark::*;
    use super::*;

    fn tick(left_min: f64) -> LimitEvent {
        LimitEvent::Tick { left_min }
    }

    #[test]
    fn the_limit_moves_only_along_its_table() {
        let table = [
            (NotRunning, LimitEvent::Start, Running),
            (NotRunning, tick(10.0), NotRunning),
            (NotRunning, LimitEvent::Stop, NotRunning),
            (Running, tick(600.0), Running),
            (Running, tick(120.0), TwoHoursLeft),
            (Running, tick(90.0), TwoHoursLeft),
            (TwoHoursLeft, tick(60.0), OneHourLeft),
            (OneHourLeft, tick(30.0), ThirtyMinutesLeft),
            (ThirtyMinutesLeft, tick(0.0), Over),
            (ThirtyMinutesLeft, tick(-5.0), Over),
            // A long step lands on the furthest mark it reached.
            (Running, tick(45.0), OneHourLeft),
            (Running, tick(-120.0), Over),
            // Never backwards, whatever the clock says.
            (OneHourLeft, tick(500.0), OneHourLeft),
            (Over, tick(500.0), Over),
            (Running, LimitEvent::Start, Running),
            // Unloading stops it from anywhere.
            (Running, LimitEvent::Stop, NotRunning),
            (TwoHoursLeft, LimitEvent::Stop, NotRunning),
            (Over, LimitEvent::Stop, NotRunning),
        ];
        for (from, event, to) in table {
            assert_eq!(from.next(event), to, "{from:?} on {event:?}");
        }
    }

    #[test]
    fn each_mark_has_one_warning_and_the_quiet_states_have_none() {
        assert_eq!(NotRunning.warning(), None);
        assert_eq!(Running.warning(), None);
        for mark in [TwoHoursLeft, OneHourLeft, ThirtyMinutesLeft, Over] {
            assert!(mark.warning().is_some_and(|line| line.ends_with('.')));
        }
    }
}
