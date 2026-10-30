//! Adaptive cruise and a posted drop under time compression. The truck's
//! physics runs on the real clock and only the road it covers is
//! compressed, so the distance cruise needs to shed speed grows with the
//! time scale. Sized at real time, cruise at ten times reached a drop from
//! 55 to 35 in the rain still over it, and the overspeed warning fired for
//! a speed cruise itself had chosen (agent drive into Wausau on WI-29,
//! 2026-10-27).

use ff_core::sim::weather::WeatherKind;
use freight_fate::states::driving_core::ACC_LIMIT_OFFSET_MPH;

use crate::transcript_cruise_support::*;

const DROP_MI: f64 = START_MI + 3.0;
const SECOND_DROP_MI: f64 = START_MI + 5.0;

/// Cruise set at `set_mph` on a 65 road that drops to 35 three miles on and
/// to 25 two miles after that. Returns the speed at each sign.
fn into_town(name: &str, set_mph: f64, scale: f64, rain: bool) -> ((f64, f64), Vec<String>) {
    let limits = [(0.0, 65.0), (DROP_MI, 35.0), (SECOND_DROP_MI, 25.0)];
    let mut harness = cruising(name, set_mph, 65.0, &[(0.0, BENCH_MILES, 0.0)]);
    harness.with_drive(move |d, _| {
        bench_road_segments(d, &limits, &[(0.0, BENCH_MILES, 0.0)], scale);
        d.trip.position_mi = START_MI;
        d.truck_mut().velocity_mps = set_mph * MPS_PER_MPH;
        if rain {
            d.weather_mut().current = WeatherKind::Rain;
        }
    });
    harness.app.ctx.settings.time_scale = scale;
    harness.with_drive(move |d, ctx| d.engage_cruise(ctx, set_mph, false));
    harness.clear_speech();
    roll_to(&mut harness, DROP_MI, 60 * 60 * 20);
    let first = harness.read_drive(|d| d.truck().speed_mph());
    roll_to(&mut harness, SECOND_DROP_MI, 60 * 60 * 20);
    let second = harness.read_drive(|d| d.truck().speed_mph());
    frames(&mut harness, 600, DT);
    ((first, second), spoken(&harness))
}

fn assert_under_both(name: &str, set_mph: f64, scale: f64, rain: bool) {
    let ((first, second), lines) = into_town(name, set_mph, scale, rain);
    let transcript = lines.join("\n");
    assert!(
        first <= 35.0 + ACC_LIMIT_OFFSET_MPH + 0.5,
        "{first:.1} mph at the 35 sign:\n{transcript}"
    );
    assert!(
        second <= 25.0 + ACC_LIMIT_OFFSET_MPH + 0.5,
        "{second:.1} mph at the 25 sign:\n{transcript}"
    );
    assert!(
        !lines.iter().any(|line| line.contains("Over the limit of")),
        "{transcript}"
    );
}

#[test]
fn cruise_reaches_a_drop_into_town_at_its_number_at_ten_times_in_the_rain() {
    assert_under_both("Compressed Drop Rain", 55.0, 10.0, true);
}

#[test]
fn cruise_reaches_a_drop_into_town_at_its_number_at_twenty_times() {
    assert_under_both("Compressed Drop Twenty", 65.0, 20.0, false);
}

#[test]
fn at_real_time_cruise_still_eases_late_enough_to_hold_the_offset() {
    // Real time is where the drop was already right: the truck arrives at
    // the with-traffic pace, not far under it.
    let ((first, _), lines) = into_town("Real Time Drop", 55.0, 1.0, false);
    assert!(
        first > 35.0 + ACC_LIMIT_OFFSET_MPH - 2.0,
        "{first:.1} mph at the 35 sign:\n{}",
        lines.join("\n")
    );
    assert_under_both("Real Time Drop Under", 55.0, 1.0, false);
}
