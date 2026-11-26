//! The cycle restart the driver can choose: whether one is on offer, the
//! rest that takes it, and every line the menus and results speak about it.
//!
//! One derived view, [`RestartOffer`], answers for the terminal row, the
//! rest stop rows, the forecast, the result lines and the agent server's
//! `status`, so the state lives in the clock alone and is never copied.

use super::clock::HosClock;
use super::{cycle_limit, duration_text, CYCLE_SPEAK_MIN};

/// What a restart row offers right now ([`HosClock::restart_offer`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RestartOffer {
    /// Hours are not enforced, a day or more of the cycle is left, or the
    /// restart is already complete: no row and nothing said.
    None,
    /// No rest under way: the whole restart, `hours` long (34, or 36 under
    /// Canadian rules).
    Fresh { hours: i64 },
    /// Rest already under way counts: `left_min` of the `hours` are still
    /// needed.
    Finish { left_min: f64, hours: i64 },
}

/// Where the restart is taken, which names its help line and, in the lot,
/// its label.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RestartPlace {
    Terminal,
    Sleeper,
    Lot,
}

impl RestartOffer {
    /// Minutes this pick rests: the whole restart, or what is left of it.
    pub fn minutes(self) -> f64 {
        match self {
            Self::None => 0.0,
            Self::Fresh { hours } => hours as f64 * 60.0,
            Self::Finish { left_min, .. } => left_min,
        }
    }

    /// The restart's length in hours, None when nothing is on offer.
    pub fn hours(self) -> Option<i64> {
        match self {
            Self::None => None,
            Self::Fresh { hours } | Self::Finish { hours, .. } => Some(hours),
        }
    }

    /// The state's name, as the agent server's `status` reports it.
    pub fn key(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Fresh { .. } => "fresh",
            Self::Finish { .. } => "finish",
        }
    }

    /// The span this pick rests, as it is spoken.
    fn span(self) -> String {
        duration_text(self.minutes() / 60.0)
    }

    /// The row's label; None when no row is shown.
    pub fn label(self, place: RestartPlace) -> Option<String> {
        let lot = place == RestartPlace::Lot;
        Some(match (self, lot) {
            (Self::None, _) => return None,
            (Self::Fresh { hours: 36 }, false) => "Take 36 hours off".to_string(),
            (Self::Fresh { hours: 36 }, true) => "Take 36 hours off in the lot".to_string(),
            (Self::Fresh { hours }, false) => format!("Take a {hours}-hour restart"),
            (Self::Fresh { hours }, true) => format!("Take a {hours}-hour restart in the lot"),
            (
                Self::Finish {
                    left_min,
                    hours: 36,
                },
                false,
            ) => format!(
                "Rest {} more to finish 36 hours off",
                duration_text(left_min / 60.0)
            ),
            (
                Self::Finish {
                    left_min,
                    hours: 36,
                },
                true,
            ) => format!(
                "Rest {} more to finish 36 hours off in the lot",
                duration_text(left_min / 60.0)
            ),
            (Self::Finish { left_min, hours }, false) => format!(
                "Rest {} more to finish a {hours}-hour restart",
                duration_text(left_min / 60.0)
            ),
            (Self::Finish { left_min, hours }, true) => format!(
                "Rest {} more to finish a {hours}-hour restart in the lot",
                duration_text(left_min / 60.0)
            ),
        })
    }

    /// The row's help line; None when no row is shown.
    pub fn help(self, place: RestartPlace) -> Option<String> {
        let span = self.span();
        let body = match (self, place) {
            (Self::None, _) => return None,
            (_, RestartPlace::Terminal) => format!(
                "A fresh cycle and fresh hours of service. Sleep takes off 8 points of fatigue \
                 an hour. Clock advances {span}."
            ),
            (_, RestartPlace::Sleeper) => format!(
                "A fresh cycle and fresh hours of service. Sleep takes off 8 points of fatigue \
                 an hour. Clock and deadline advance {span}."
            ),
            (_, RestartPlace::Lot) => format!(
                "A fresh cycle with poor rest: 5 points of fatigue off an hour, never below 30. \
                 Clock and deadline advance {span}."
            ),
        };
        Some(match self {
            Self::Finish { .. } => format!("Your rest so far counts. {body}"),
            _ => body,
        })
    }

    /// The terminal's result line, from the offer the pick took.
    pub fn rested_line(self, clock: &str, time_of_day: &str) -> String {
        let span = self.span();
        match self {
            Self::Finish { .. } => {
                format!("Rested {span} more. It is {clock}, {time_of_day}. Cycle clock reset.")
            }
            _ => format!("Rested {span}. It is {clock}, {time_of_day}. Cycle clock reset."),
        }
    }

    /// The road's result lead, from the offer the pick took: how long the
    /// driver rested and how they woke.
    pub fn woke_line(self, rested: bool) -> String {
        let span = self.span();
        match (self, rested) {
            (Self::Finish { hours, .. }, true) => {
                format!("You rested {span} more, {hours} hours off in a row, and woke rested.")
            }
            (Self::Finish { hours, .. }, false) => {
                format!("You rested {span} more, {hours} hours off in a row, and woke still tired.")
            }
            (_, true) => format!("You rested {span} and woke rested."),
            (_, false) => format!("You rested {span} and woke still tired."),
        }
    }

    /// What the restart does, as the forecast says it.
    pub fn effect_text(self) -> String {
        let hours = self.hours().unwrap_or(0);
        format!(
            "{hours} hours off in a row restart your cycle and reset your driving hours and legal \
             driving window."
        )
    }
}

impl HosClock {
    /// The restart on offer under `mode`: shown only while hours are
    /// enforced and a day or less of the cycle is left
    /// ([`CYCLE_SPEAK_MIN`], where the ELD line starts naming the cycle).
    pub fn restart_offer(&self, mode: &str) -> RestartOffer {
        if cycle_limit(mode).is_none() || self.restart_complete() {
            return RestartOffer::None;
        }
        match self.cycle_remaining_min(mode) {
            Some(left) if left <= CYCLE_SPEAK_MIN => {}
            _ => return RestartOffer::None,
        }
        let hours = (self.restart_threshold_min() / 60.0).round() as i64;
        if self.restart_off_min > 0.0 {
            RestartOffer::Finish {
                left_min: self.restart_remaining_min(),
                hours,
            }
        } else {
            RestartOffer::Fresh { hours }
        }
    }

    /// Rest exactly what the restart still needs (the whole threshold when
    /// no rest is under way) as one rest period of its true length, under
    /// `status`. The ledger clears, the shift starts fresh as after a full
    /// night, and the Canadian counts end with it. Returns the minutes
    /// rested; nothing changes when the restart is already complete.
    pub fn finish_restart(&mut self, status: &str) -> f64 {
        let minutes = self.restart_remaining_min();
        if minutes <= 0.0 {
            return 0.0;
        }
        self.advance_cycle(minutes, false);
        self.reset_as(status, minutes);
        minutes
    }

    /// Said after a full night that left the cycle used up: hours are
    /// enforced, the cycle limit is still blown and the restart is not
    /// complete. None in every other case.
    pub fn cycle_still_used_up_text(&self, mode: &str) -> Option<String> {
        if self.restart_complete() {
            return None;
        }
        let left = self.cycle_remaining_min(mode)?;
        (left <= 0.0).then(|| {
            format!(
                "Your cycle is still used up. Rest {} more to restart it.",
                duration_text(self.restart_remaining_min() / 60.0)
            )
        })
    }
}
