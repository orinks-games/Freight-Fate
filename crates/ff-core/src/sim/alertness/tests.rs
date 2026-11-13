//! Transition tables for the band and the nod-off: one row per transition in
//! `docs/fatigue-design.md`, plus the hysteresis and multi-band rows.

use super::*;
use Alertness::*;

#[test]
fn band_transition_table() {
    // (from, fatigue, to, cue)
    let rows: &[(Alertness, f64, Alertness, Option<BandCue>)] = &[
        // Upward, one band at a time.
        (Rested, 39.9, Rested, None),
        (Rested, 40.0, Worn, Some(BandCue::Worn)),
        (Worn, 59.9, Worn, None),
        (Worn, 60.0, Drowsy, Some(BandCue::Drowsy)),
        (Drowsy, 79.9, Drowsy, None),
        (Drowsy, 80.0, Severe, Some(BandCue::Severe)),
        (Severe, 100.0, Severe, None),
        // Downward, silent, five points under the entry.
        (Severe, 75.0, Severe, None),
        (Severe, 74.9, Drowsy, None),
        (Drowsy, 55.0, Drowsy, None),
        (Drowsy, 54.9, Worn, None),
        (Worn, 35.0, Worn, None),
        (Worn, 34.9, Rested, None),
        // A jump gives only the cue of the band landed in.
        (Rested, 85.0, Severe, Some(BandCue::Severe)),
        (Rested, 65.0, Drowsy, Some(BandCue::Drowsy)),
        (Worn, 90.0, Severe, Some(BandCue::Severe)),
        // A long drop falls through every band it clears, silently.
        (Severe, 20.0, Rested, None),
        (Severe, 50.0, Worn, None),
        (Severe, 57.0, Drowsy, None),
        (Drowsy, 10.0, Rested, None),
    ];
    for &(from, fatigue, to, cue) in rows {
        assert_eq!(from.next(fatigue), (to, cue), "{from:?} at {fatigue}");
    }
}

#[test]
fn a_small_lift_does_not_replay_the_cue() {
    // 61 -> 57 -> 61: one "Getting drowsy." only.
    let (band, cue) = Worn.next(61.0);
    assert_eq!((band, cue), (Drowsy, Some(BandCue::Drowsy)));
    let (band, cue) = band.next(57.0);
    assert_eq!((band, cue), (Drowsy, None));
    let (band, cue) = band.next(61.0);
    assert_eq!((band, cue), (Drowsy, None));
    // Down to 54 leaves the band, so 61 is a new entry.
    let (band, cue) = band.next(54.0);
    assert_eq!((band, cue), (Worn, None));
    assert_eq!(band.next(61.0), (Drowsy, Some(BandCue::Drowsy)));
}

#[test]
fn from_fatigue_has_no_history() {
    for (fatigue, band) in [
        (0.0, Rested),
        (39.9, Rested),
        (40.0, Worn),
        (60.0, Drowsy),
        (79.9, Drowsy),
        (80.0, Severe),
        (100.0, Severe),
    ] {
        assert_eq!(Alertness::from_fatigue(fatigue), band, "{fatigue}");
    }
    // The 60 and 80 lines are the hours module's.
    assert_eq!(Drowsy.entry(), crate::sim::hos::FATIGUE_DROWSY);
    assert_eq!(Severe.entry(), crate::sim::hos::FATIGUE_SEVERE);
}

#[test]
fn keys_and_words_round_trip() {
    for band in Alertness::ALL {
        assert_eq!(Alertness::from_key(band.key()), Some(band));
    }
    assert_eq!(Alertness::from_key("Drowsy"), Some(Drowsy));
    assert_eq!(Alertness::from_key("sleepy"), None);
    assert_eq!(Rested.readout_word(), None);
    assert_eq!(Worn.readout_word(), Some("worn"));
    assert_eq!(Drowsy.readout_word(), Some("drowsy"));
    assert_eq!(Severe.readout_word(), Some("severe"));
}

fn tick(gm: f64, fatigue: f64, moving: bool, busy: bool) -> NodEvent {
    NodEvent::Tick {
        gm,
        fatigue,
        moving,
        busy,
    }
}

fn awake(since_gm: f64, misses: u8) -> NodState {
    NodState {
        nod: Nod::Awake { since_gm },
        misses,
    }
}

fn nodding(left_s: f64, misses: u8) -> NodState {
    NodState {
        nod: Nod::Nodding { left_s },
        misses,
    }
}

fn recovering(left_gm: f64, misses: u8) -> NodState {
    NodState {
        nod: Nod::Recovering { left_gm },
        misses,
    }
}

#[test]
fn nod_transition_table() {
    let cool = MICROSLEEP_COOLDOWN_GM;
    let window = MICROSLEEP_REACTION_S;
    // (from, event, to, outcome)
    let rows: Vec<(NodState, NodEvent, NodState, NodOutcome)> = vec![
        // Awake, below severe or not moving: the counter resets.
        (
            awake(5.0, 1),
            tick(1.0, 79.9, true, false),
            awake(0.0, 1),
            NodOutcome::None,
        ),
        (
            awake(5.0, 1),
            tick(1.0, 95.0, false, false),
            awake(0.0, 1),
            NodOutcome::None,
        ),
        // Awake, a hazard waiting on the driver: the counter holds.
        (
            awake(5.0, 0),
            tick(1.0, 85.0, true, true),
            awake(5.0, 0),
            NodOutcome::None,
        ),
        // Awake, counting.
        (
            awake(5.0, 0),
            tick(1.0, 80.0, true, false),
            awake(6.0, 0),
            NodOutcome::None,
        ),
        // Awake, the counter reaches the interval: a nod begins.
        (
            awake(8.5, 2),
            tick(0.5, 80.0, true, false),
            nodding(window, 2),
            NodOutcome::Began,
        ),
        (
            awake(2.5, 0),
            tick(0.5, 100.0, true, false),
            nodding(window, 0),
            NodOutcome::Began,
        ),
        // Nodding: the driver reacts.
        (
            nodding(1.0, 2),
            NodEvent::Reacted,
            recovering(cool, 0),
            NodOutcome::Caught,
        ),
        // Nodding: already crawling.
        (
            nodding(1.0, 2),
            NodEvent::Window {
                dt_s: 0.1,
                crawling: true,
            },
            recovering(cool, 0),
            NodOutcome::CaughtSilent,
        ),
        // Nodding: the window runs down.
        (
            nodding(1.0, 0),
            NodEvent::Window {
                dt_s: 0.25,
                crawling: false,
            },
            nodding(0.75, 0),
            NodOutcome::None,
        ),
        // Nodding: time runs out, misses below 2.
        (
            nodding(0.1, 0),
            NodEvent::Window {
                dt_s: 0.1,
                crawling: false,
            },
            recovering(cool, 1),
            NodOutcome::DriftedOff,
        ),
        (
            nodding(0.1, 1),
            NodEvent::Window {
                dt_s: 0.2,
                crawling: false,
            },
            recovering(cool, 2),
            NodOutcome::DriftedOff,
        ),
        // Nodding: time runs out with two misses on the count.
        (
            nodding(0.1, 2),
            NodEvent::Window {
                dt_s: 0.2,
                crawling: false,
            },
            recovering(cool, 0),
            NodOutcome::ForcedStop,
        ),
        // Nodding ignores the game clock; the window has its own.
        (
            nodding(1.0, 1),
            tick(5.0, 95.0, true, false),
            nodding(1.0, 1),
            NodOutcome::None,
        ),
        // Recovering: the quiet period runs down, moving or not, busy or not.
        (
            recovering(4.0, 1),
            tick(1.0, 95.0, false, true),
            recovering(3.0, 1),
            NodOutcome::None,
        ),
        // Recovering: time runs out, and the count starts in the same tick.
        (
            recovering(0.5, 1),
            tick(1.0, 95.0, true, false),
            awake(1.0, 1),
            NodOutcome::None,
        ),
        (
            recovering(0.5, 1),
            tick(1.0, 50.0, true, false),
            awake(0.0, 1),
            NodOutcome::None,
        ),
        // A reaction or a window event outside a nod does nothing.
        (
            awake(3.0, 1),
            NodEvent::Reacted,
            awake(3.0, 1),
            NodOutcome::None,
        ),
        (
            recovering(2.0, 1),
            NodEvent::Window {
                dt_s: 5.0,
                crawling: false,
            },
            recovering(2.0, 1),
            NodOutcome::None,
        ),
        // Any state: the co-driver takes the wheel.
        (
            awake(6.0, 2),
            NodEvent::WheelHandedOver,
            awake(0.0, 2),
            NodOutcome::None,
        ),
        (
            nodding(1.0, 1),
            NodEvent::WheelHandedOver,
            awake(0.0, 1),
            NodOutcome::None,
        ),
        (
            recovering(2.0, 1),
            NodEvent::WheelHandedOver,
            awake(0.0, 1),
            NodOutcome::None,
        ),
        // Staged: a play tester opens the window directly.
        (
            awake(1.0, 1),
            NodEvent::Staged,
            nodding(window, 1),
            NodOutcome::Began,
        ),
        (
            recovering(2.0, 0),
            NodEvent::Staged,
            nodding(window, 0),
            NodOutcome::Began,
        ),
        (
            nodding(0.5, 0),
            NodEvent::Staged,
            nodding(0.5, 0),
            NodOutcome::None,
        ),
    ];
    for (from, event, to, outcome) in rows {
        let mut state = from;
        let got = state.next(event);
        assert_eq!((state, got), (to, outcome), "{from:?} on {event:?}");
    }
}

#[test]
fn only_an_open_window_demands_the_driver() {
    assert!(!awake(3.0, 0).demands_driver());
    assert!(nodding(1.0, 0).demands_driver());
    assert!(!recovering(1.0, 0).demands_driver());
}

#[test]
fn the_interval_shrinks_with_exhaustion() {
    assert!((microsleep_interval_gm(FATIGUE_SEVERE) - MICROSLEEP_BASE_GM).abs() < 1e-9);
    assert!((microsleep_interval_gm(100.0) - MICROSLEEP_MIN_GM).abs() < 1e-9);
    assert!(microsleep_interval_gm(95.0) < microsleep_interval_gm(82.0));
}

#[test]
fn three_misses_in_a_row_force_the_stop_and_a_catch_clears_the_count() {
    let mut state = NodState::default();
    let mut outcomes = Vec::new();
    for _ in 0..3 {
        state.next(NodEvent::Staged);
        outcomes.push(state.next(NodEvent::Window {
            dt_s: 3.0,
            crawling: false,
        }));
    }
    assert_eq!(
        outcomes,
        [
            NodOutcome::DriftedOff,
            NodOutcome::DriftedOff,
            NodOutcome::ForcedStop
        ]
    );
    assert_eq!(state.misses, 0);
    state.next(NodEvent::Staged);
    state.next(NodEvent::Window {
        dt_s: 3.0,
        crawling: false,
    });
    assert_eq!(state.misses, 1);
    state.next(NodEvent::Staged);
    assert_eq!(state.next(NodEvent::Reacted), NodOutcome::Caught);
    assert_eq!(state.misses, 0);
}

#[test]
fn nod_state_round_trips_through_json() {
    for state in [awake(3.25, 1), nodding(1.5, 2), recovering(2.0, 0)] {
        assert_eq!(NodState::from_json(&state.to_json()), Some(state));
    }
    assert_eq!(
        awake(3.25, 1).to_json(),
        json!({"state": "awake", "since_gm": 3.25, "misses": 1})
    );
    assert_eq!(NodState::from_json(&json!({"state": "dreaming"})), None);
    assert_eq!(NodState::from_json(&json!(null)), None);
    // A count past the forced stop cannot come back from a hand-edited save.
    let clamped = NodState::from_json(&json!({"state": "awake", "since_gm": 0.0, "misses": 9}));
    assert_eq!(clamped.map(|s| s.misses), Some(2));
}
