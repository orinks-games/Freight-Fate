//! Real-seconds breathing gaps for the routine road talkers.
//!
//! Time compression spends road 10-40x faster than a real cab, so systems
//! that announce on road distance -- posted-limit arrivals, traffic calls,
//! zone chatter -- pile their lines back to back in every driving mode
//! (owner report, 2026-08-13). The clock stays (career pacing is balanced
//! on it); the ANNOUNCEMENTS space out instead, in wall-clock seconds, the
//! same law the corner warnings and the keeper's ease already follow.
//!
//! The gate lives at the SOURCE, before any state mutates: a caller that
//! finds its window shut simply does nothing, and the next natural check
//! after the window opens announces the CURRENT state. Superseding is free
//! -- nothing is held, so nothing goes stale.
//!
//! Safety and action lines never come here: hazards, AEB, pacenotes, scale
//! and stop calls, maneuvers, enforcement, merge warnings, and every answer
//! to a player's key speak immediately, always.
//!
//! Port of `freight_fate/sim/road_event_pacing.py`.

use std::collections::HashMap;

use crate::speech_pacing::{monotonic_seconds, Clock};

pub const LIMIT_GAP_REAL_S: f64 = 12.0; // posted-limit arrival lines
/// How fast the driving voice gets through a line, in characters per real
/// second: the event pacer's own estimate (Hearken's default pacer
/// configuration, which the game's pacer inherits), so a line judged too
/// long for its road is judged by the same rate that queues it.
pub const SPOKEN_CHARS_PER_REAL_S: f64 = 13.0;
pub const TRAFFIC_GAP_REAL_S: f64 = 10.0; // NPC traffic situation calls
pub const ZONE_GAP_REAL_S: f64 = 15.0; // zone-entry colour
/// Billboards and flavor landmarks: a sitting-time budget, not a sim-hour
/// rate. ~90s is just under 1x highway spacing of the densest scenery
/// (2 miles at 65), so Real time still hears the places; 20x cannot poke
/// once per compressed mile.
pub const CHATTER_GAP_REAL_S: f64 = 90.0;

/// The window for a category; a category no caller defines is a
/// programming error, as the Python `KeyError` was.
fn gap_for(category: &str) -> f64 {
    match category {
        "limit" => LIMIT_GAP_REAL_S,
        "traffic" => TRAFFIC_GAP_REAL_S,
        "zone" => ZONE_GAP_REAL_S,
        other => panic!("unknown road event category {other:?}"),
    }
}

/// One window per category, measured on the wall clock.
pub struct RoadEventBreather {
    clock: Clock,
    last_spoke: HashMap<String, f64>,
}

impl Default for RoadEventBreather {
    fn default() -> Self {
        Self::new()
    }
}

impl RoadEventBreather {
    /// A breather on the real monotonic clock.
    pub fn new() -> Self {
        Self::with_clock(Box::new(monotonic_seconds))
    }

    /// A breather on an injected clock (tests drive it without sleeping).
    pub fn with_clock(clock: Clock) -> Self {
        Self {
            clock,
            last_spoke: HashMap::new(),
        }
    }

    /// Swap the clock under a live breather (what the Python tests did by
    /// monkeypatching `_clock`).
    pub fn set_clock(&mut self, clock: Clock) {
        self.clock = clock;
    }

    pub fn ready(&mut self, category: &str) -> bool {
        match self.last_spoke.get(category) {
            None => true,
            Some(&last) => (self.clock)() - last >= gap_for(category),
        }
    }

    pub fn spoke(&mut self, category: &str) {
        let now = (self.clock)();
        self.last_spoke.insert(category.to_string(), now);
    }
}

#[cfg(test)]
mod tests {
    //! Ported from the pure parts of `tests/test_road_event_pacing.py`. The
    //! `Trip._check_speed_limit` gating tests there drive a Trip and belong
    //! to the sim::trip bucket.
    use std::cell::Cell;
    use std::rc::Rc;

    use super::*;

    /// `FakeClock`: a settable clock shared between the test and the breather.
    struct FakeClock {
        now: Rc<Cell<f64>>,
    }

    impl FakeClock {
        fn new() -> Self {
            Self {
                now: Rc::new(Cell::new(1000.0)),
            }
        }

        fn clock(&self) -> Clock {
            let now = Rc::clone(&self.now);
            Box::new(move || now.get())
        }

        fn advance(&self, seconds: f64) {
            self.now.set(self.now.get() + seconds);
        }
    }

    #[test]
    fn test_first_line_of_a_category_is_always_ready() {
        let mut b = RoadEventBreather::with_clock(FakeClock::new().clock());
        assert!(b.ready("limit"));
        assert!(b.ready("traffic"));
        assert!(b.ready("zone"));
    }

    #[test]
    fn test_speaking_closes_the_window_for_the_gap() {
        let clock = FakeClock::new();
        let mut b = RoadEventBreather::with_clock(clock.clock());
        b.spoke("limit");
        clock.advance(LIMIT_GAP_REAL_S - 0.5);
        assert!(!b.ready("limit"));
        clock.advance(1.0);
        assert!(b.ready("limit"));
    }

    #[test]
    fn test_categories_are_independent() {
        let clock = FakeClock::new();
        let mut b = RoadEventBreather::with_clock(clock.clock());
        b.spoke("limit");
        assert!(b.ready("traffic"));
        assert!(b.ready("zone"));
    }

    #[test]
    fn test_ready_never_consumes() {
        let mut b = RoadEventBreather::with_clock(FakeClock::new().clock());
        assert!(b.ready("limit"));
        assert!(b.ready("limit")); // polling twice is not speaking twice
    }

    #[test]
    fn test_gap_constants_are_real_seconds_apart() {
        // The gaps are the design's numbers; a drive-by refactor that halves
        // them silently reintroduces the chatter this exists to kill.
        assert_eq!(LIMIT_GAP_REAL_S, 12.0);
        assert_eq!(TRAFFIC_GAP_REAL_S, 10.0);
        assert_eq!(ZONE_GAP_REAL_S, 15.0);
        assert_eq!(CHATTER_GAP_REAL_S, 90.0);
    }
}
