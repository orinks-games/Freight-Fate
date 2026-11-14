//! What fatigue costs off the wheel (`docs/fatigue-design.md`, parts 6 and
//! 7): an officer who deals with a severely tired driver, and the call to
//! dispatch that trades the on-time bonus for a night's sleep.
//!
//! Slow work (part 4) is [`Alertness::work_time_mult`]. Everything here is
//! pure: the driving state asks, then speaks and applies what comes back.
//!
//! Two career facts outlive a load, and both live in the profile's
//! `achievement_stats` map, which already round-trips through every save
//! and cloud backup (the team-driving introduction keeps its flag there
//! too). A new top-level save field would be refused by the backup service.
//!
//! * [`STAT_OFFICER_WARNED`]: relaxed pacing's one warning has been given.
//! * [`STAT_REST_CALLS`]: the career hours of recent rest calls, pruned to
//!   the [`REST_CALL_WINDOW_H`] window.

use serde_json::{Map, Value};

use super::alertness::Alertness;

// ---------------------------------------------------------------------------
// An officer can see it
// ---------------------------------------------------------------------------

/// The `achievement_stats` key set once relaxed pacing's warning is given.
pub const STAT_OFFICER_WARNED: &str = "fatigue:officer_warned";

/// Relaxed pacing's first stop at Severe: what the officer says instead of
/// the order.
pub const OFFICER_FATIGUE_WARNING: &str =
    "The officer warns you about driving this tired. Next time it is an out-of-service order.";

/// How a stop that has already settled its own business ends for the
/// driver's fatigue.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FatigueStopOutcome {
    /// Nothing about fatigue.
    None,
    /// Relaxed pacing, the first time in a career: a warning.
    Warning,
    /// A fatigue out-of-service order: ten hours off duty.
    OutOfService,
}

/// What an officer does about the driver's fatigue once a stop's own result
/// is settled (49 CFR 392.3). Certain at Severe, never a roll; nothing below
/// it. Nothing when enforcement is bypassed, and nothing when the stop
/// already put the driver out of service: one order per stop.
pub fn fatigue_stop_outcome(
    band: Alertness,
    relaxed: bool,
    warned_before: bool,
    bypassed: bool,
    already_out_of_service: bool,
) -> FatigueStopOutcome {
    if band != Alertness::Severe || bypassed || already_out_of_service {
        return FatigueStopOutcome::None;
    }
    if relaxed && !warned_before {
        return FatigueStopOutcome::Warning;
    }
    FatigueStopOutcome::OutOfService
}

/// Whether relaxed pacing's one warning has been given in this career.
pub fn officer_warned(stats: &Map<String, Value>) -> bool {
    stats
        .get(STAT_OFFICER_WARNED)
        .and_then(Value::as_i64)
        .is_some_and(|n| n > 0)
}

/// Record that relaxed pacing's one warning has been given.
pub fn mark_officer_warned(stats: &mut Map<String, Value>) {
    stats.insert(STAT_OFFICER_WARNED.to_string(), Value::from(1));
}

// ---------------------------------------------------------------------------
// Calling dispatch for rest
// ---------------------------------------------------------------------------

/// The `achievement_stats` key holding recent rest calls' career hours.
pub const STAT_REST_CALLS: &str = "dispatch:rest_calls";

/// The window rest calls are counted over: seven game days.
pub const REST_CALL_WINDOW_H: f64 = 7.0 * 24.0;

/// Rest calls inside the window that cost nothing beyond the bonus. The
/// next one, and each after it, costs dispatch trust.
pub const REST_CALLS_FREE_PER_WINDOW: usize = 2;

/// The rest the appointment moves for: a ten-hour off-duty period.
pub const REST_CALL_REST_H: f64 = 10.0;

/// How far the appointment moves when no stop with parking is known ahead:
/// the rest plus half an hour to find a place. ASSUMED.
pub const REST_CALL_FALLBACK_H: f64 = 10.5;

/// The rest call on one load.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RestCall {
    /// Not called yet on this load.
    #[default]
    Available,
    /// Called: the appointment moved, and the load pays its base rate.
    Used,
}

/// What can happen to the rest call.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RestCallEvent {
    /// The driver called dispatch too tired to drive safely.
    Called,
}

impl RestCall {
    /// The only place the rest call moves. Returns the new state and
    /// whether dispatch granted the call (once per load).
    pub fn next(self, event: RestCallEvent) -> (RestCall, bool) {
        match (self, event) {
            (RestCall::Available, RestCallEvent::Called) => (RestCall::Used, true),
            (RestCall::Used, RestCallEvent::Called) => (RestCall::Used, false),
        }
    }

    /// A stable key for the trip snapshot and the agent server's `status`.
    pub fn key(self) -> &'static str {
        match self {
            RestCall::Available => "available",
            RestCall::Used => "used",
        }
    }

    /// The state a [`key`](Self::key) names.
    pub fn from_key(key: &str) -> Option<RestCall> {
        match key.trim().to_ascii_lowercase().as_str() {
            "available" => Some(RestCall::Available),
            "used" => Some(RestCall::Used),
            _ => None,
        }
    }
}

/// Whether the dispatch call offers "Too tired to drive safely": Drowsy or
/// Severe, a load on, the call not yet used on it, and not a team load whose
/// co-driver still has hours (handing over the wheel is the answer there).
pub fn rest_call_offered(
    band: Alertness,
    on_load: bool,
    state: RestCall,
    team_with_hours: bool,
) -> bool {
    band >= Alertness::Drowsy && on_load && state == RestCall::Available && !team_with_hours
}

/// Whether a rest call costs dispatch trust, given how many rest calls
/// (this one included) fall inside the last seven game days: the third and
/// every one after it.
pub fn rest_call_costs_trust(times_in_last_7_days: usize) -> bool {
    times_in_last_7_days > REST_CALLS_FREE_PER_WINDOW
}

/// How far the appointment moves: the rest plus the drive to the nearest
/// stop with parking, or [`REST_CALL_FALLBACK_H`] when none is known.
pub fn rest_call_delay_h(nearest_stop_travel_min: Option<f64>) -> f64 {
    match nearest_stop_travel_min {
        Some(minutes) => REST_CALL_REST_H + minutes.max(0.0) / 60.0,
        None => REST_CALL_FALLBACK_H,
    }
}

/// Book a rest call at career hour `now_h`: older calls fall out of the
/// window. Returns how many calls the window now holds, this one included.
pub fn record_rest_call(stats: &mut Map<String, Value>, now_h: f64) -> usize {
    let mut times: Vec<f64> = stats
        .get(STAT_REST_CALLS)
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(Value::as_f64).collect())
        .unwrap_or_default();
    times.retain(|h| now_h - h < REST_CALL_WINDOW_H && *h <= now_h);
    times.push(now_h);
    let count = times.len();
    stats.insert(
        STAT_REST_CALLS.to_string(),
        Value::Array(times.into_iter().map(Value::from).collect()),
    );
    count
}

#[cfg(test)]
mod tests {
    use super::*;
    use Alertness::*;
    use FatigueStopOutcome as O;

    #[test]
    fn fatigue_stop_outcome_table() {
        // (band, relaxed, warned_before, bypassed, already_oos, outcome)
        let rows: &[(Alertness, bool, bool, bool, bool, FatigueStopOutcome)] = &[
            (Rested, false, false, false, false, O::None),
            (Worn, false, false, false, false, O::None),
            (Drowsy, false, false, false, false, O::None),
            (Drowsy, true, false, false, false, O::None),
            (Severe, false, false, false, false, O::OutOfService),
            (Severe, false, true, false, false, O::OutOfService),
            (Severe, true, false, false, false, O::Warning),
            (Severe, true, true, false, false, O::OutOfService),
            (Severe, false, false, true, false, O::None),
            (Severe, true, false, true, false, O::None),
            (Severe, false, false, false, true, O::None),
            (Severe, true, false, false, true, O::None),
        ];
        for &(band, relaxed, warned, bypassed, oos, want) in rows {
            assert_eq!(
                fatigue_stop_outcome(band, relaxed, warned, bypassed, oos),
                want,
                "{band:?} relaxed {relaxed} warned {warned} bypassed {bypassed} oos {oos}"
            );
        }
    }

    #[test]
    fn work_time_table() {
        // (band, relaxed, base, minutes)
        let rows: &[(Alertness, bool, f64, f64)] = &[
            (Rested, false, 20.0, 20.0),
            (Worn, false, 20.0, 22.0),
            (Drowsy, false, 20.0, 25.0),
            (Severe, false, 20.0, 30.0),
            (Rested, true, 20.0, 20.0),
            (Worn, true, 20.0, 21.0),
            (Drowsy, true, 20.0, 22.0),
            (Severe, true, 20.0, 25.0),
            // A half rounds up.
            (Drowsy, false, 30.0, 38.0),
            (Severe, false, 15.0, 23.0),
            (Severe, false, 30.0, 45.0),
            (Worn, false, 5.0, 6.0),
        ];
        for &(band, relaxed, base, want) in rows {
            assert_eq!(
                band.hand_work_min(base, relaxed),
                want,
                "{band:?} {relaxed} {base}"
            );
        }
    }

    #[test]
    fn rest_call_transition_table() {
        let rows: &[(RestCall, RestCallEvent, RestCall, bool)] = &[
            (
                RestCall::Available,
                RestCallEvent::Called,
                RestCall::Used,
                true,
            ),
            (RestCall::Used, RestCallEvent::Called, RestCall::Used, false),
        ];
        for &(from, event, to, granted) in rows {
            assert_eq!(from.next(event), (to, granted), "{from:?} {event:?}");
        }
        for state in [RestCall::Available, RestCall::Used] {
            assert_eq!(RestCall::from_key(state.key()), Some(state));
        }
        assert_eq!(RestCall::from_key("nonsense"), None);
    }

    #[test]
    fn rest_call_offered_table() {
        // (band, on_load, state, team_with_hours, offered)
        let rows: &[(Alertness, bool, RestCall, bool, bool)] = &[
            (Rested, true, RestCall::Available, false, false),
            (Worn, true, RestCall::Available, false, false),
            (Drowsy, true, RestCall::Available, false, true),
            (Severe, true, RestCall::Available, false, true),
            (Drowsy, false, RestCall::Available, false, false),
            (Drowsy, true, RestCall::Used, false, false),
            (Severe, true, RestCall::Available, true, false),
        ];
        for &(band, on_load, state, team, want) in rows {
            assert_eq!(
                rest_call_offered(band, on_load, state, team),
                want,
                "{band:?} {on_load} {state:?} {team}"
            );
        }
    }

    #[test]
    fn rest_call_trust_rule() {
        assert!(!rest_call_costs_trust(1));
        assert!(!rest_call_costs_trust(2));
        assert!(rest_call_costs_trust(3));
        assert!(rest_call_costs_trust(4));
    }

    #[test]
    fn rest_calls_are_counted_over_seven_days() {
        let mut stats = Map::new();
        assert_eq!(record_rest_call(&mut stats, 0.0), 1);
        assert_eq!(record_rest_call(&mut stats, 50.0), 2);
        assert_eq!(record_rest_call(&mut stats, 100.0), 3);
        // A week after the first, it has fallen out of the window.
        assert_eq!(record_rest_call(&mut stats, 168.0), 3);
        assert_eq!(record_rest_call(&mut stats, 400.0), 1);
        assert_eq!(
            stats.get(STAT_REST_CALLS),
            Some(&Value::Array(vec![Value::from(400.0)]))
        );
    }

    #[test]
    fn the_appointment_moves_for_the_rest_and_the_drive() {
        assert_eq!(rest_call_delay_h(Some(30.0)), 10.5);
        assert_eq!(rest_call_delay_h(Some(12.0)), 10.2);
        assert_eq!(rest_call_delay_h(None), 10.5);
    }

    #[test]
    fn the_warning_is_remembered() {
        let mut stats = Map::new();
        assert!(!officer_warned(&stats));
        mark_officer_warned(&mut stats);
        assert!(officer_warned(&stats));
    }
}
