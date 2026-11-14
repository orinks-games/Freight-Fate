//! "Cruise easing to X" is spoken only when the cap slows the truck.
//!
//! Climbing out of a town at 44 with cruise set at 80 in a 55, the 60 cap
//! and then a 45 cap for a drop ahead each said "Cruise easing to" a second
//! apart. The truck was under both and never eased (owner's drive out of
//! Buffalo, 2026-11-04).

use crate::transcript_cruise_support::*;

const FLAT: [(f64, f64, f64); 1] = [(0.0, BENCH_MILES, 0.0)];

/// Cruise holding `speed_mph` on a `road_mph` road, then set to `set_mph`,
/// with a drop to 40 `drop_ahead_mi` on. Returns what was said over the
/// next `seconds` and the speed at the end.
fn easing_run(
    name: &str,
    speed_mph: f64,
    set_mph: f64,
    road_mph: f64,
    drop_ahead_mi: f64,
    seconds: f64,
) -> (Vec<String>, f64) {
    let mut harness = cruising(name, speed_mph, road_mph, &FLAT);
    harness.with_drive(move |d, _| {
        let limits = [(0.0, road_mph), (START_MI + drop_ahead_mi, 40.0)];
        bench_road_segments(d, &limits, &FLAT, 1.0);
        d.trip.position_mi = START_MI;
    });
    harness.with_drive(move |d, _| d.cruise_mph = Some(set_mph));
    harness.clear_speech();
    frames(&mut harness, (seconds / DT) as usize, DT);
    let speed = harness.read_drive(|d| d.truck().speed_mph());
    (spoken(&harness), speed)
}

#[test]
fn a_cap_over_a_climbing_truck_says_nothing() {
    let (lines, _) = easing_run("Easing Climb", 44.0, 80.0, 55.0, 0.3, 12.0);
    assert!(
        !lines.iter().any(|line| line.contains("easing to")),
        "{}",
        lines.join("\n")
    );
}

#[test]
fn a_cap_that_slows_the_truck_still_says_so() {
    let (lines, speed) = easing_run("Easing Drop", 65.0, 65.0, 65.0, 1.2, 90.0);
    let transcript = lines.join("\n");
    assert!(
        lines.iter().any(|line| line.contains("easing to 45")),
        "{transcript}"
    );
    assert!(
        lines
            .iter()
            .filter(|line| line.contains("easing to"))
            .count()
            == 1,
        "{transcript}"
    );
    assert!(speed <= 45.5, "{speed:.1} mph:\n{transcript}");
}

/// A drop to 30 four miles on, caught at full pace, then the clock goes to
/// real time (a signaled exit does this). Cruise must take the 55 back for
/// the real miles left before the drop, not crawl them at 35.
#[test]
fn a_drop_held_at_full_pace_lets_go_when_the_clock_goes_real() {
    let mut harness = cruising("Easing Pinned", 60.0, 55.0, &FLAT);
    harness.with_drive(|d, _| {
        let limits = [(0.0, 55.0), (START_MI + 4.0, 30.0)];
        bench_road_segments(d, &limits, &FLAT, 20.0);
        d.trip.position_mi = START_MI;
    });
    harness.app.ctx.settings.time_scale = 20.0;
    frames(&mut harness, 30, DT);
    let held = harness.read_drive(|d| d.cruise_held_mph.unwrap_or(0.0));
    assert!(
        held <= 35.5,
        "the drop was not caught at full pace: {held:.1}"
    );
    harness.app.ctx.settings.time_scale = 1.0;
    harness.with_drive(|d, _| d.trip.time_scale = 1.0);
    frames(&mut harness, (15.0 / DT) as usize, DT);
    let (held, ahead) = harness.read_drive(|d| {
        (
            d.cruise_held_mph.unwrap_or(0.0),
            START_MI + 4.0 - d.trip.position_mi,
        )
    });
    assert!(ahead > 1.0, "{ahead:.2} miles left");
    assert!(
        held >= 55.0,
        "still holding {held:.1} with {ahead:.2} miles to the drop"
    );
}

/// Cruise set 75 at 40 mph with a 30 limit just ahead, and `exit_mph` the
/// exit's own cruise number, if any. What was said over six seconds.
fn exit_cap_run(name: &str, exit_mph: Option<f64>) -> Vec<String> {
    let mut harness = cruising(name, 40.0, 55.0, &FLAT);
    harness.with_drive(move |d, _| {
        let limits = [(0.0, 55.0), (START_MI + 0.05, 30.0)];
        bench_road_segments(d, &limits, &FLAT, 1.0);
        d.trip.position_mi = START_MI;
        d.cruise_mph = Some(75.0);
        d.cruise_exit_mph = exit_mph;
    });
    harness.clear_speech();
    frames(&mut harness, (6.0 / DT) as usize, DT);
    spoken(&harness)
}

/// Set for the exit at 30, then the 30 limit's cap of 35 over a truck doing
/// 40: the exit's number was just spoken and the truck is on its way down to
/// it, so "easing to 35" would read as cruise speeding back up (agent drive,
/// Jamestown, 2026-11-04).
#[test]
fn a_limit_cap_over_the_exit_number_says_nothing() {
    let lines = exit_cap_run("Easing Exit", Some(30.0));
    assert!(
        !lines.iter().any(|line| line.contains("easing to")),
        "{}",
        lines.join("\n")
    );
    // With no exit armed the same cap is news.
    let lines = exit_cap_run("Easing No Exit", None);
    assert!(
        lines.iter().any(|line| line.contains("easing to 35")),
        "{}",
        lines.join("\n")
    );
}
