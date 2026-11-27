//! Ported from `tests/test_career_pacing.py`, with the pacing model it drives
//! (`tools/career_pacing.py`) carried along as the test helper below.
//!
//! The 40-level arc is a months-long career with steady level-up pacing.
//! These tests drive the deterministic pacing model against the real
//! `Career::record_delivery` math, the real level thresholds, and the real
//! dispatch distance caps. They are the design contract for the career arc
//! (levels 1 to 30 from the 1.9 line, 31 to 40 from 2.0):
//!
//! - early levels land within a session or two, so a new driver feels motion;
//! - the whole ladder takes on the order of months of real evenings, not days;
//! - no single late level turns into a wall.

use std::collections::HashMap;

use super::*;
use crate::pyrandom::PyRandom;

// Real-time model: highway cruise under the default clock compression.
const CRUISE_MPH: f64 = 60.0;
/// settings.py default.
const TIME_SCALE: f64 = 10.0;
/// Deliberate waiting runs the clock faster.
const PARKED_TIME_SCALE_MULT: f64 = 2.0;
/// Dispatch board, pickup, dock work per delivery.
const MENU_OVERHEAD_REAL_H: f64 = 0.15;
const SLEEP_GAME_H: f64 = 10.0;
const DRIVE_H_PER_SHIFT: f64 = 11.0;

// Player model: a steady, competent driver, not a perfect one.
const ON_TIME_RATE: f64 = 0.90;
const CLEAN_RATE: f64 = 0.80;
/// Share of runs on endorsement freight once unlocked.
const SPECIALTY_RATE: f64 = 0.30;
/// Level 11+ boards weight endorsement freight up.
const SENIOR_SPECIALTY_RATE: f64 = 0.45;
/// Share of runs on premium mid-level freight.
const PREMIUM_RATE: f64 = 0.30;

// `JobBoard.distance_cap(level)`: the caps the pacing model deals against.
fn distance_cap(level: i64) -> f64 {
    crate::models::jobs::JobBoard::distance_cap(level)
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct LevelCheckpoint {
    level: i64,
    deliveries: i64,
    real_hours: f64,
    xp: f64,
}

/// Real minutes a haul costs: driving, dock time, and sleeper shifts.
fn delivery_real_hours(miles: f64) -> f64 {
    let drive_game_h = miles / CRUISE_MPH;
    let mut hours = drive_game_h / TIME_SCALE + MENU_OVERHEAD_REAL_H;
    let sleeps = (drive_game_h / DRIVE_H_PER_SHIFT).trunc();
    hours += sleeps * SLEEP_GAME_H / (TIME_SCALE * PARKED_TIME_SCALE_MULT);
    hours
}

/// Level-by-level checkpoints for one simulated career.
fn simulate_career(seed: i64) -> Vec<LevelCheckpoint> {
    let target_level = MAX_CAREER_LEVEL;
    let max_deliveries = 20_000;
    let mut rng = PyRandom::new_from_i64(seed);
    let mut career = Career::new();
    let mut hours = 0.0;
    let mut deliveries = 0;
    let mut timeline = vec![LevelCheckpoint {
        level: 1,
        deliveries: 0,
        real_hours: 0.0,
        xp: 0.0,
    }];
    while career.level() < target_level && deliveries < max_deliveries {
        let level = career.level();
        let cap = distance_cap(level);
        let miles = (rng.uniform(0.35, 0.95) * cap).clamp(60.0, 1_500.0);
        let on_time = rng.random() < ON_TIME_RATE;
        let clean = rng.random() < CLEAN_RATE;
        let roll = rng.random();
        let specialty_rate = if level >= 11 {
            SENIOR_SPECIALTY_RATE
        } else {
            SPECIALTY_RATE
        };
        let mult = if level >= 3 && roll < specialty_rate {
            XP_SPECIALTY_MULT
        } else if level >= 2 && roll < specialty_rate + PREMIUM_RATE {
            XP_PREMIUM_MULT
        } else {
            1.0
        };
        career.record_delivery(
            miles,
            0.0,
            on_time,
            if clean { 0.0 } else { 30.0 },
            mult,
            1.0,
        );
        deliveries += 1;
        hours += delivery_real_hours(miles);
        while career.level() > timeline.last().unwrap().level {
            let next = timeline.last().unwrap().level + 1;
            timeline.push(LevelCheckpoint {
                level: next,
                deliveries,
                real_hours: hours,
                xp: career.xp,
            });
        }
    }
    timeline
}

fn timeline() -> Vec<LevelCheckpoint> {
    simulate_career(42)
}

fn by_level(timeline: &[LevelCheckpoint]) -> HashMap<i64, LevelCheckpoint> {
    timeline.iter().map(|c| (c.level, *c)).collect()
}

#[test]
fn test_simulation_reaches_the_top_of_the_ladder() {
    let timeline = timeline();
    assert_eq!(timeline.last().unwrap().level, 40);
    let levels: Vec<i64> = timeline.iter().map(|c| c.level).collect();
    assert_eq!(levels, (1..=40).collect::<Vec<i64>>());
    let hours: Vec<f64> = timeline.iter().map(|c| c.real_hours).collect();
    assert!(hours.windows(2).all(|w| w[0] <= w[1]));
}

#[test]
fn test_the_first_session_already_levels_up() {
    let timeline = timeline();
    let by_level = by_level(&timeline);
    assert!(by_level[&2].real_hours <= 2.0);
    assert!(by_level[&3].real_hours <= 5.0);
}

#[test]
fn test_the_early_arc_moves_and_the_whole_arc_takes_months() {
    let timeline = timeline();
    let by_level = by_level(&timeline);
    // Load choice (8) lands inside the first stretch of evenings.
    assert!((8.0..=30.0).contains(&by_level[&8].real_hours));
    // The owner-operator gate (18) is a real investment.
    assert!((70.0..=170.0).contains(&by_level[&18].real_hours));
    // Level 30 lands after months of real play (roughly five months at two
    // hours a night, eleven at one), but not so far out nobody sees it.
    assert!((220.0..=400.0).contains(&by_level[&30].real_hours));
    // The top, level 40, is about nine months at two hours a night.
    assert!((480.0..=620.0).contains(&by_level[&40].real_hours));
}

#[test]
fn test_the_arc_takes_real_life_months_at_any_schedule() {
    // The design contract, in calendar terms.
    //
    // The model's `real_hours` are wall-clock hours at the keyboard (driving
    // under the default 10x clock compression, plus dock and menu time), so
    // dividing by a daily schedule gives real-life calendar time. Level 30
    // must cost months of a player's actual life -- even for a dedicated
    // player -- while a casual hour-a-night player still finishes within
    // about a year instead of never.
    let timeline = timeline();
    let hours: HashMap<i64, f64> = timeline.iter().map(|c| (c.level, c.real_hours)).collect();
    let days_per_month = 30.4;

    let dedicated_months = hours[&30] / (2.5 * days_per_month); // 2.5 h every day
    assert!(dedicated_months >= 3.0);
    assert!(dedicated_months <= 8.0);

    let casual_months = hours[&30] / (1.0 * days_per_month); // an hour per evening
    assert!((9.0..=14.0).contains(&casual_months));

    // The mid-arc pivot (owner-operator at 18) is itself a real-life
    // commitment measured in months for an evening player.
    assert!(hours[&18] / (1.0 * days_per_month) >= 3.0);
}

#[test]
fn test_no_late_level_becomes_a_wall() {
    let timeline = timeline();
    let by_level = by_level(&timeline);
    for level in 21..=30 {
        let gap = by_level[&level].real_hours - by_level[&(level - 1)].real_hours;
        assert!(
            (1.0..=25.0).contains(&gap),
            "level {level} took {gap:.1} hours"
        );
    }
}

#[test]
fn test_no_level_past_thirty_becomes_a_wall() {
    for seed in [42, 1, 7, 99] {
        let timeline = simulate_career(seed);
        let by_level = by_level(&timeline);
        for level in 31..=40 {
            let gap = by_level[&level].real_hours - by_level[&(level - 1)].real_hours;
            assert!(
                (1.0..=30.0).contains(&gap),
                "seed {seed}: level {level} took {gap:.1} hours"
            );
        }
    }
}

#[test]
fn test_pacing_holds_across_seeds() {
    for seed in [1, 7, 99] {
        let timeline = simulate_career(seed);
        let by_level = by_level(&timeline);
        assert!((220.0..=420.0).contains(&by_level[&30].real_hours));
        assert!((460.0..=640.0).contains(&by_level[&40].real_hours));
        assert!(by_level[&18].real_hours >= 60.0);
    }
}

#[test]
fn test_pacing_model_matches_cpython_to_the_float() {
    // `tools/career_pacing.py simulate_career(seed)` under CPython: the same
    // MT19937 draws, the same `Career.record_delivery` arithmetic.
    for (seed, deliveries, hours, xp, l18_deliveries, l18_hours) in [
        (
            42,
            108,
            335.00119902452906,
            387904.1290523517,
            48,
            124.85147794583455,
        ),
        (
            7,
            110,
            323.6905352267114,
            391591.732226486,
            49,
            118.98522099405939,
        ),
    ] {
        let timeline = simulate_career(seed);
        // Level 30, the top of the 1.9 ladder: the thresholds below it did
        // not move, so neither does it.
        let last = timeline.iter().find(|c| c.level == 30).unwrap();
        let l18 = timeline.iter().find(|c| c.level == 18).unwrap();
        assert_eq!(last.deliveries, deliveries, "seed {seed}");
        assert_eq!(last.real_hours, hours, "seed {seed}");
        assert_eq!(last.xp, xp, "seed {seed}");
        assert_eq!(l18.deliveries, l18_deliveries, "seed {seed}");
        assert_eq!(l18.real_hours, l18_hours, "seed {seed}");
    }
}

#[test]
fn test_the_forty_level_arc_matches_the_director_model() {
    // Re-run in CPython with the 2.0 thresholds (25,000 apart past 30):
    // seed 42 reaches 35 at 141 deliveries and about 449 hours, 40 at 167
    // and about 540.
    let timeline = simulate_career(42);
    let by_level = by_level(&timeline);
    assert_eq!(by_level[&35].deliveries, 141);
    assert!((by_level[&35].real_hours - 449.0).abs() < 1.0);
    assert_eq!(by_level[&40].deliveries, 167);
    assert!((by_level[&40].real_hours - 540.0).abs() < 1.0);
}
