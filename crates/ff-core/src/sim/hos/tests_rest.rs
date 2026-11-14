//! Rest by the hour, on-duty fatigue, and the night-hours line
//! (`docs/fatigue-design.md` parts 2, 3 and 5).

use super::*;

fn approx(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

#[test]
fn proper_sleep_takes_off_eight_points_an_hour() {
    assert_eq!(rest_sleep(95.0, SLEEP_MIN), 15.0);
    assert_eq!(rest_sleep(80.0, SLEEP_MIN), 0.0);
    assert_eq!(rest_sleep(30.0, SLEEP_MIN), 0.0);
    assert_eq!(rest_sleep(50.0, 120.0), 34.0);
    assert_eq!(rest_sleep(50.0, f64::NAN), 50.0);
}

#[test]
fn poor_sleep_takes_off_five_an_hour_down_to_thirty() {
    assert_eq!(rest_poor_sleep(95.0, SLEEP_MIN), 45.0);
    assert_eq!(rest_poor_sleep(40.0, SLEEP_MIN), 30.0);
    assert_eq!(rest_poor_sleep(20.0, SLEEP_MIN), 20.0);
    assert_eq!(rest_poor_sleep(30.0, SLEEP_MIN), 30.0);
}

#[test]
fn on_duty_time_builds_a_third_of_the_driving_rate() {
    let day = on_duty_fatigue_gain(10.0, 60.0, |_| 1.0);
    assert!(approx(day, 60.0 * 0.115 / 3.0));
    let night = on_duty_fatigue_gain(23.0, 60.0, |_| 1.0);
    assert!(approx(night, 60.0 * 0.17 / 3.0));
    // A four-hour daytime dock wait costs about nine points.
    let dock = on_duty_fatigue_gain(9.0, 240.0, |_| 1.0);
    assert!((dock - 9.2).abs() < 0.01, "{dock}");
    // The mode and buff scale multiply it, as they do driving.
    assert!(approx(on_duty_fatigue_gain(10.0, 60.0, |_| 0.8), day * 0.8));
    assert_eq!(on_duty_fatigue_gain(10.0, 0.0, |_| 1.0), 0.0);
}

#[test]
fn night_hours_cross_nine_pm_and_five_am() {
    // 6 PM for 6 hours: 9 PM to midnight is night.
    assert!(approx(night_driving_hours(18.0, 6.0), 3.0));
    // 3 AM for 6 hours: 3 AM to 5 AM.
    assert!(approx(night_driving_hours(3.0, 6.0), 2.0));
    // All day: 7 AM to 7 PM.
    assert_eq!(night_driving_hours(7.0, 12.0), 0.0);
    // All night: 10 PM to 4 AM.
    assert!(approx(night_driving_hours(22.0, 6.0), 6.0));
    // Two whole nights in a 44-hour run from noon: 8 hours each.
    assert!(approx(night_driving_hours(12.0, 44.0), 16.0));
    // Departure hours past 24 are the same clock.
    assert!(approx(night_driving_hours(18.0 + 48.0, 6.0), 3.0));
    assert_eq!(night_driving_hours(18.0, 0.0), 0.0);
    assert_eq!(night_driving_hours(f64::NAN, 4.0), 0.0);
}

#[test]
fn night_hours_follow_the_local_clock() {
    use crate::sim::timezones::{to_local, EASTERN, PACIFIC};
    // 7 PM Eastern on the reference clock: in Eastern time a 4-hour drive
    // runs two hours into the night; in Pacific time it is 4 PM, all day.
    let reference = 19.0;
    let eastern = to_local(reference, EASTERN);
    let pacific = to_local(reference, PACIFIC);
    assert!(night_driving_hours(eastern, 4.0) > night_driving_hours(pacific, 4.0));
}

#[test]
fn the_night_hours_line_rounds_to_whole_hours() {
    assert_eq!(
        night_driving_line(18.0, 7.0).as_deref(),
        Some("About 4 hours of night driving.")
    );
    assert_eq!(
        night_driving_line(20.0, 2.0).as_deref(),
        Some("About 1 hour of night driving.")
    );
    // Twenty minutes of night rounds to nothing, and nothing is said.
    assert_eq!(night_driving_line(20.0, 1.33), None);
    assert_eq!(night_driving_line(8.0, 8.0), None);
}
