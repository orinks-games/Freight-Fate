//! Ported from `tests/test_hos.py` (hours of service, fatigue, day/night,
//! and overnight parking), plus the pure HOS cases from
//! `tests/test_truck_parking_capacity.py` and `tests/test_stop_detail.py`.
//!
//! The twenty-nine cases that drive the app shell are not here: `ff-core`
//! cannot depend on the game crate, so `RestStopState`, `ParkingFullState`,
//! the pause menu, the terminal and the dispatch board are all invisible from
//! this file. They ran here as `#[ignore]`d stubs, which is no coverage at
//! all, and now run for real in
//! `crates/freight-fate/tests/states_driving_hos.rs` (the wheel, the rest
//! menus, the shoulder, the inspections) and
//! `crates/freight-fate/tests/states_city_hos.rs` (the bunk room and the
//! dispatch board's hours warning).

use serde_json::{json, Value};

use super::clock::split_event_key;
use super::pyjson::{py_float_str, py_iter, py_repr, py_repr_str, py_str};
use super::*;

/// `pytest.approx` with its defaults: rel 1e-6, abs 1e-12.
fn approx(actual: f64, expected: f64) -> bool {
    (actual - expected).abs() <= (1e-6 * expected.abs()).max(1e-12)
}

#[test]
fn test_hazard_scale_only_relaxes_relaxed_mode() {
    assert_eq!(hazard_scale("relaxed"), RELAXED_HAZARD_SCALE);
    assert!(hazard_scale("relaxed") < 1.0);
    assert_eq!(hazard_scale("realistic"), 1.0);
    assert_eq!(hazard_scale("debug_off"), 1.0);
}

// -- clock math -------------------------------------------------------------------

#[test]
fn test_drive_accumulates_all_three_meters() {
    let mut c = HosClock::new();
    c.drive(90.0);
    assert_eq!(c.driving_min, 90.0);
    assert_eq!(c.duty_min, 90.0);
    assert_eq!(c.since_break_min, 90.0);
}

#[test]
fn test_parked_time_counts_against_duty_window_only() {
    let mut c = HosClock::new();
    c.on_duty(60.0);
    assert_eq!(c.duty_min, 60.0);
    assert_eq!(c.driving_min, 0.0);
    assert_eq!(c.since_break_min, 0.0);
    assert_eq!(c.status, "on_duty_not_driving");
}

#[test]
fn test_break_resets_break_rule_but_not_the_shift() {
    let mut c = HosClock::new();
    c.drive(480.0);
    c.take_break(30.0);
    assert_eq!(c.since_break_min, 0.0);
    assert_eq!(c.driving_min, 480.0);
    assert_eq!(c.duty_min, 510.0); // the break itself burns duty window
    assert_eq!(c.status, "off_duty");
}

#[test]
fn test_on_duty_not_driving_satisfies_break_rule() {
    let mut c = HosClock::new();
    c.drive(480.0);
    c.on_duty(30.0);
    assert_eq!(c.status, "on_duty_not_driving");
    assert_eq!(c.since_break_min, 0.0);
    assert_eq!(c.driving_min, 480.0);
    assert_eq!(c.duty_min, 510.0);
}

#[test]
fn test_short_break_does_not_satisfy_the_break_rule() {
    let mut c = HosClock::new();
    c.drive(100.0);
    c.take_break(15.0);
    assert_eq!(c.since_break_min, 100.0);
}

#[test]
fn test_sleep_resets_the_shift() {
    let mut c = HosClock::new();
    c.drive(600.0);
    c.check_warnings("realistic");
    c.sleep();
    assert_eq!(c.driving_min, 0.0);
    assert_eq!(c.duty_min, 0.0);
    assert_eq!(c.since_break_min, 0.0);
    assert_eq!(c.status, "sleeper_berth");
    assert!(c.warned.is_empty());
}

#[test]
fn test_eight_two_sleeper_split_restores_time_without_full_reset() {
    let mut c = HosClock::new();
    c.drive(300.0);
    c.sleeper_split_rest(480.0);
    c.drive(300.0);
    c.sleeper_split_rest(120.0);

    assert!(approx(c.driving_min, 300.0));
    assert!(approx(c.duty_min, 300.0));
    assert_eq!(c.since_break_min, 0.0);
    assert_eq!(c.status, "sleeper_berth");
    assert_eq!(c.split_pending_summary(), None);
}

#[test]
fn test_long_sleeper_period_pauses_duty_window_until_paired() {
    // 7 hours of duty, then 8 in the berth: the window must not close in
    // the driver's sleep (tester report, 2026-09-11).
    let mut c = HosClock::new();
    c.drive(420.0);
    assert!(!c.sleeper_split_rest(480.0));
    assert!(approx(c.duty_min, 420.0));
    assert!(approx(c.driving_min, 420.0));
    assert!(!c.in_violation("realistic"));
    assert!(c.split_pending_summary().is_some());

    // The 2-hour partner then moves the window's start to the end of the
    // long rest, so only the duty between the two rests remains.
    c.drive(120.0);
    assert!(c.sleeper_split_rest(120.0));
    assert!(approx(c.duty_min, 120.0));
    assert!(approx(c.driving_min, 120.0));
    assert_eq!(c.split_pending_summary(), None);
}

#[test]
fn test_short_first_sleeper_period_still_counts_against_window() {
    let mut c = HosClock::new();
    c.drive(420.0);
    assert!(!c.sleeper_split_rest(120.0));
    assert!(approx(c.duty_min, 540.0));

    // An off-duty stretch of 7 hours is not a berth period and never pauses.
    let mut off = HosClock::new();
    off.drive(420.0);
    off.off_duty(420.0);
    assert!(approx(off.duty_min, 840.0));
}

#[test]
fn test_normal_sleeper_periods_can_complete_split_credit() {
    let mut c = HosClock::new();
    c.drive(300.0);
    c.sleeper(480.0);
    c.drive(300.0);
    c.sleeper(120.0);

    assert!(approx(c.driving_min, 300.0));
    assert!(approx(c.duty_min, 300.0));
    assert_eq!(c.split_pending_summary(), None);
}

#[test]
fn test_rolling_sleeper_split_reuses_previous_short_rest() {
    let mut c = HosClock::new();
    c.sleeper(480.0);
    c.drive(240.0);
    c.off_duty(120.0);
    assert_eq!(c.split_pending_summary(), None);
    c.drive(300.0);
    c.sleeper(480.0);

    assert!(approx(c.driving_min, 300.0));
    assert!(approx(c.duty_min, 300.0));
    assert_eq!(c.split_pending_summary(), None);
}

#[test]
fn test_short_first_sleeper_split_preserves_between_rest_driving() {
    let mut c = HosClock::new();
    c.drive(60.0);
    c.sleeper(120.0);
    c.drive(600.0);
    c.sleeper(480.0);

    assert!(approx(c.driving_min, 600.0));
    assert!(approx(c.duty_min, 600.0));
    assert_eq!(c.split_pending_summary(), None);
}

#[test]
fn test_long_first_sleeper_split_preserves_between_rest_driving() {
    let mut c = HosClock::new();
    c.drive(60.0);
    c.sleeper(480.0);
    c.drive(300.0);
    c.sleeper(120.0);

    assert!(approx(c.driving_min, 300.0));
    assert!(approx(c.duty_min, 300.0));
    assert_eq!(c.split_pending_summary(), None);
}

#[test]
fn test_long_first_split_survives_fragmented_driving_history() {
    let mut c = HosClock::new();
    c.drive(60.0);
    c.sleeper(480.0);
    for _ in 0..120 {
        c.drive(2.0);
    }
    c.sleeper(120.0);

    assert!(approx(c.driving_min, 240.0));
    assert!(approx(c.duty_min, 240.0));
    assert_eq!(c.split_pending_summary(), None);
}

#[test]
fn test_short_first_split_survives_fragmented_driving_history() {
    let mut c = HosClock::new();
    c.drive(60.0);
    c.sleeper(120.0);
    for _ in 0..120 {
        c.drive(2.0);
    }
    c.sleeper(480.0);

    assert!(approx(c.driving_min, 240.0));
    assert!(approx(c.duty_min, 240.0));
    assert_eq!(c.split_pending_summary(), None);
}

#[test]
fn test_seven_three_sleeper_split_restores_time_without_full_reset() {
    let mut c = HosClock::new();
    c.drive(90.0);
    c.sleeper_split_rest(420.0);
    c.on_duty(60.0);
    c.drive(180.0);
    c.sleeper_split_rest(180.0);

    assert!(approx(c.driving_min, 180.0));
    assert!(approx(c.duty_min, 240.0));
    assert_eq!(c.since_break_min, 0.0);
    assert_eq!(c.split_pending_summary(), None);
}

#[test]
fn test_short_off_duty_can_complete_sleeper_split() {
    let mut c = HosClock::new();
    c.drive(300.0);
    c.sleeper_split_rest(480.0);
    c.drive(60.0);
    c.off_duty(120.0);

    assert!(approx(c.driving_min, 60.0));
    assert!(approx(c.duty_min, 60.0));
    assert_eq!(c.split_pending_summary(), None);
}

#[test]
fn test_repeated_sleeper_splits_can_each_apply_credit() {
    let mut c = HosClock::new();
    c.drive(300.0);
    assert!(!c.sleeper_split_rest(480.0));
    c.drive(60.0);
    c.off_duty(120.0);
    assert!(approx(c.driving_min, 60.0));
    assert!(approx(c.duty_min, 60.0));

    c.drive(300.0);
    assert!(c.sleeper_split_rest(480.0));
    assert!(approx(c.driving_min, 300.0));
    assert!(approx(c.duty_min, 300.0));
    c.drive(60.0);
    c.off_duty(120.0);

    assert!(approx(c.driving_min, 60.0));
    assert!(approx(c.duty_min, 60.0));
    assert_eq!(c.split_pending_summary(), None);
}

#[test]
fn test_full_off_duty_and_sleeper_resets_clear_split_pending_summary() {
    let mut off = HosClock::new();
    off.drive(300.0);
    off.off_duty(600.0);
    assert_eq!(off.driving_min, 0.0);
    assert_eq!(off.split_pending_summary(), None);

    let mut sleeper = HosClock::new();
    sleeper.drive(300.0);
    sleeper.sleeper(600.0);
    assert_eq!(sleeper.driving_min, 0.0);
    assert_eq!(sleeper.split_pending_summary(), None);
}

#[test]
fn test_back_to_back_berth_rests_are_one_period() {
    // Two 3-hour picks at one stop are 6 hours in a row, not two 3-hour
    // halves (tester report, 2026-09-23).
    let mut c = HosClock::new();
    c.drive(500.0);
    c.on_duty(235.0);
    c.sleeper_split_rest(180.0);
    c.sleeper_split_rest(180.0);
    assert_eq!(c.split_rest_history.len(), 1);
    assert_eq!(c.split_rest_history[0].minutes, 360.0);
    assert_eq!(c.reset_minutes_left(), Some(240.0));

    // Reaching 7 hours takes the whole period off the window.
    c.sleeper_split_rest(60.0);
    assert_eq!(c.duty_min, 735.0);

    // And ten in a row is a full reset.
    c.sleeper_split_rest(180.0);
    assert_eq!(c.driving_min, 0.0);
    assert_eq!(c.duty_min, 0.0);
    assert_eq!(c.reset_minutes_left(), None);
}

#[test]
fn test_split_long_period_must_be_sleeper_berth() {
    let mut c = HosClock::new();
    c.drive(300.0);
    c.off_duty(480.0);
    c.drive(60.0);
    let completed = c.sleeper_split_rest(120.0);

    assert!(!completed);
    assert!(approx(c.driving_min, 360.0));
    assert!(approx(c.duty_min, 960.0));
}

#[test]
fn test_split_pending_summary_names_needed_pair() {
    let mut c = HosClock::new();
    c.drive(300.0);
    c.sleeper_split_rest(480.0);

    assert_eq!(
        c.split_pending_summary(),
        Some("Sleeper split open: 2 more hours off duty or in the sleeper to pair it.")
    );
}

#[test]
fn test_hos_summary_mentions_pending_sleeper_split() {
    let mut c = HosClock::new();
    c.drive(300.0);
    c.sleeper_split_rest(480.0);

    let summary = c.summary("realistic");

    assert!(summary.contains("Sleeper split open"));
    assert!(summary.contains("2 more hours"));
}

#[test]
fn test_completed_split_summary_stays_clear_after_dict_roundtrip() {
    let mut c = HosClock::new();
    c.drive(300.0);
    c.sleeper_split_rest(480.0);
    c.drive(300.0);
    c.sleeper_split_rest(120.0);

    let again = HosClock::from_dict(&c.to_dict());

    assert_eq!(again.split_pending_summary(), None);
}

#[test]
fn test_remaining_is_the_nearest_limit() {
    let mut c = HosClock::new();
    c.drive(400.0);
    // break binds first: 480 - 400 = 80 vs drive 260 vs duty 440
    assert!(approx(c.remaining_min("realistic").unwrap(), 80.0));
}

#[test]
fn test_violation_detection() {
    let mut c = HosClock::new();
    c.drive(481.0);
    assert!(c.in_violation("realistic"));
    let mut c2 = HosClock::new();
    c2.drive(479.0);
    assert!(!c2.in_violation("realistic"));
}

// -- warnings -------------------------------------------------------------------

fn drive_collecting(c: &mut HosClock, minutes: f64, mode: &str, step: f64) -> Vec<String> {
    let mut msgs = Vec::new();
    let mut elapsed = 0.0;
    while elapsed < minutes {
        c.drive(step);
        elapsed += step;
        msgs.extend(c.check_warnings(mode));
    }
    msgs
}

fn drive_collecting_realistic(c: &mut HosClock, minutes: f64) -> Vec<String> {
    drive_collecting(c, minutes, "realistic", 5.0)
}

#[test]
fn test_warnings_fire_once_per_threshold() {
    let mut c = HosClock::new();
    let msgs = drive_collecting_realistic(&mut c, 485.0); // past the 8-hour break rule
    assert_eq!(
        msgs.iter().filter(|m| m.contains("2 hours until")).count(),
        1
    );
    assert_eq!(
        msgs.iter().filter(|m| m.contains("1 hour until")).count(),
        1
    );
    assert_eq!(
        msgs.iter()
            .filter(|m| m.contains("30 minutes until"))
            .count(),
        1
    );
    assert_eq!(msgs.iter().filter(|m| m.contains("violation")).count(), 1);
    // driving on never repeats a break warning; only the separate
    // 11-hour drive limit may speak up as it approaches
    let later = drive_collecting_realistic(&mut c, 60.0);
    assert!(!later.iter().any(|m| m.contains("break")));
    assert!(later.iter().all(|m| m.contains("driving time")));
}

#[test]
fn test_warnings_mention_what_is_due() {
    let mut c = HosClock::new();
    let msgs = drive_collecting_realistic(&mut c, 365.0); // crosses the 2-hour break threshold
    assert_eq!(msgs.len(), 1);
    assert!(msgs[0].contains("break"));
}

#[test]
fn test_break_rearms_break_warnings_only() {
    let mut c = HosClock::new();
    drive_collecting_realistic(&mut c, 485.0); // all break warnings + violation spoken
    c.take_break(30.0);
    // next binding limit is the 11-hour drive clock (660): at 540 driving
    // the 2-hour warning for it fires once
    let msgs = drive_collecting_realistic(&mut c, 60.0); // driving_min 485 -> 545
    assert!(msgs
        .iter()
        .any(|m| m.contains("driving time") && m.contains("2 hours")));
    // break thresholds can fire again on the fresh break window
    let msgs = drive_collecting_realistic(&mut c, 60.0); // since_break 60 -> 120... not yet
    assert!(!msgs.iter().any(|m| m.contains("break")));
}

#[test]
fn test_skipping_thresholds_speaks_only_the_most_urgent() {
    let mut c = HosClock::new();
    c.drive(470.0); // jump straight to 10 minutes before the break rule
    let msgs = c.check_warnings("realistic");
    assert_eq!(msgs.len(), 1);
    assert!(msgs[0].contains("30 minutes"));
    // the swallowed thresholds never fire later
    assert!(!drive_collecting_realistic(&mut c, 5.0)
        .iter()
        .any(|m| m.contains("2 hours")));
}

#[test]
fn test_warning_batch_speaks_only_most_urgent_limit() {
    let mut c = HosClock::new();
    c.drive(900.0);

    let msgs = c.check_warnings("realistic");

    assert_eq!(msgs.len(), 1);
    assert!(msgs[0].contains("violation"));
    assert!(msgs[0].contains("driving time"));
}

#[test]
fn test_break_only_violation_summary_requests_break_not_sleep() {
    let mut c = HosClock::new();
    c.drive(481.0);

    let summary = c.summary("realistic");

    assert!(summary.contains("30-minute break") || summary.contains("30 minute break"));
    assert!(!summary.contains("Sleep 10 hours"));
}

#[test]
fn test_hos_summary_includes_time_units() {
    let mut c = HosClock::new();
    c.drive(120.0);

    let summary = c.summary("realistic");

    assert!(summary.contains("9 hours of driving left"));
    assert!(summary.contains("30-minute break due in 6 hours"));
    assert!(summary.contains("12 hours left in the 14-hour window"));
}

#[test]
fn test_hos_summary_omits_break_when_duty_window_closes_first() {
    let mut c = HosClock::new();
    c.driving_min = 414.0;
    c.since_break_min = 270.0;
    c.duty_min = 732.0;
    c.status = "driving".to_string();

    let summary = c.summary("realistic");

    assert!(summary.contains("1.8 hours left in the 14-hour window"));
    assert!(!summary.contains("30-minute break due"));
}

// -- the one-answer readouts (Alt A, Alt S, Alt D) ---------------------------------

/// A shift where the 14-hour window runs out before the break is due.
fn duty_closes_first() -> HosClock {
    let mut c = HosClock::new();
    c.driving_min = 414.0;
    c.since_break_min = 270.0;
    c.duty_min = 732.0;
    c.status = "driving".to_string();
    c
}

#[test]
fn test_wheel_time_leads_with_its_own_noun_and_names_both_spent_clocks() {
    let mut c = HosClock::new();
    c.drive(324.0);

    let said = c.wheel_time_summary("realistic", false);

    assert!(said.starts_with("Driven "));
    assert!(said.contains("Driven 5.4 hours"));
    assert!(said.contains("On duty 5.4 hours"));
}

#[test]
fn test_terse_wheel_time_keeps_the_driving_number_alone() {
    let mut c = HosClock::new();
    c.drive(324.0);

    let said = c.wheel_time_summary("realistic", true);

    assert_eq!(said, "At the wheel 5.4 hours.");
}

#[test]
fn test_wheel_time_speaks_short_stretches_as_minutes() {
    let mut c = HosClock::new();
    c.drive(24.0);

    assert!(c
        .wheel_time_summary("realistic", false)
        .contains("Driven 24 minutes"));
}

#[test]
fn test_wheel_time_says_no_driving_yet_on_a_fresh_shift() {
    assert!(HosClock::new()
        .wheel_time_summary("realistic", false)
        .contains("Driven nothing yet"));
}

#[test]
fn test_wheel_time_flags_being_out_of_hours() {
    let mut c = HosClock::new();
    c.drive(11.0 * 60.0 + 30.0);

    assert!(c
        .wheel_time_summary("realistic", false)
        .contains("Out of hours."));
    assert!(c
        .wheel_time_summary("realistic", true)
        .contains("Out of hours."));
}

#[test]
fn test_wheel_time_flags_an_overdue_break_without_calling_it_out_of_hours() {
    let mut c = HosClock::new();
    c.drive(481.0);

    let said = c.wheel_time_summary("realistic", false);

    assert!(said.contains("30-minute break is overdue"));
    assert!(!said.contains("out of hours"));
}

#[test]
fn test_wheel_time_answers_instead_of_going_quiet_with_enforcement_off() {
    let mut c = HosClock::new();
    c.drive(324.0);

    for mode in ["off", "debug_off"] {
        let said = c.wheel_time_summary(mode, false);
        assert!(said.contains("Driven 5.4 hours"));
        assert!(said.contains("Hours enforcement off."));
    }
}

#[test]
fn test_break_key_leads_with_the_break_and_counts_driving_time() {
    let mut c = HosClock::new();
    c.drive(120.0);

    assert_eq!(
        c.break_summary("realistic", false),
        "30-minute break due in 6 hours of driving."
    );
    assert_eq!(
        c.break_summary("realistic", true),
        "30-minute break due in 6 hours."
    );
}

#[test]
fn test_break_key_speaks_minutes_when_under_an_hour() {
    let mut c = HosClock::new();
    c.drive(456.0);

    assert!(c
        .break_summary("realistic", false)
        .contains("30-minute break due in 24 minutes"));
}

#[test]
fn test_break_key_answers_the_break_first_then_the_window_that_closes_first() {
    // summary() omits the break here; a key the player pressed for the break
    // has to answer the break, then add the fact that overrides it.
    let said = duty_closes_first().break_summary("realistic", false);

    assert!(said.starts_with("30-minute break due in 3.5 hours"));
    assert!(said.contains(", 1.8 hours left in the 14-hour window"));
}

#[test]
fn test_terse_break_key_still_names_the_window_that_closes_first() {
    let said = duty_closes_first().break_summary("realistic", true);

    assert!(said.contains("30-minute break due in 3.5 hours"));
    assert!(said.contains("duty window 1.8 hours"));
}

#[test]
fn test_break_key_reports_an_overdue_break_with_what_to_do() {
    let mut c = HosClock::new();
    c.drive(481.0);

    assert_eq!(
        c.break_summary("realistic", false),
        "Break overdue. Take a 30 minute break at a rest stop."
    );
    assert_eq!(c.break_summary("realistic", true), "Break overdue.");
}

#[test]
fn test_break_key_says_a_break_will_not_help_once_the_shift_is_over() {
    let mut c = HosClock::new();
    c.drive(11.0 * 60.0 + 30.0);

    let said = c.break_summary("realistic", false);

    assert!(said.contains("out of driving time for this shift"));
    assert!(said.contains("Sleep 10 hours"));
}

#[test]
fn test_break_key_names_the_duty_window_when_that_is_the_blown_clock() {
    let mut c = HosClock::new();
    c.driving_min = 100.0;
    c.duty_min = 14.0 * 60.0 + 30.0;
    c.since_break_min = 60.0;

    let said = c.break_summary("realistic", false);

    assert!(said.contains("past your duty window"));
    assert_eq!(said.matches("but").count(), 1); // one overriding fact, not two stacked clauses
}

#[test]
fn test_break_key_says_none_required_with_enforcement_off() {
    for mode in ["off", "debug_off"] {
        assert!(HosClock::new()
            .break_summary(mode, false)
            .contains("Break: none required."));
        assert!(HosClock::new()
            .break_summary(mode, false)
            .contains("Hours enforcement off."));
    }
}

#[test]
fn test_drive_time_key_names_both_clocks_and_leads_with_driving_time() {
    let mut c = HosClock::new();
    c.drive(300.0);

    assert_eq!(
        c.drive_time_summary("realistic", false),
        "Driving time left: 6 hours. Duty window closes in 9 hours."
    );
    assert_eq!(
        c.drive_time_summary("realistic", true),
        "Driving time left: 6 hours, duty window 9 hours."
    );
}

#[test]
fn test_drive_time_key_leads_with_the_duty_window_when_that_binds() {
    let said = duty_closes_first().drive_time_summary("realistic", false);

    assert!(said.starts_with("Duty window closes in 1.8 hours"));
    assert!(said.contains("Driving time left: 4.1 hours"));
}

#[test]
fn test_drive_time_key_names_which_clock_ran_out() {
    let mut over_drive = HosClock::new();
    over_drive.drive(11.0 * 60.0 + 30.0);
    let mut over_duty = HosClock::new();
    over_duty.driving_min = 100.0;
    over_duty.duty_min = 14.0 * 60.0 + 30.0;

    assert_eq!(
        over_drive.drive_time_summary("realistic", false),
        "Out of driving time for this shift. Sleep 10 hours at a rest stop to reset."
    );
    assert_eq!(
        over_duty.drive_time_summary("realistic", false),
        "Your duty window has closed. Sleep 10 hours at a rest stop to reset."
    );
}

#[test]
fn test_drive_time_key_adds_the_overdue_break_that_comes_first() {
    let mut c = HosClock::new();
    c.drive(481.0);

    assert!(c
        .drive_time_summary("realistic", false)
        .contains("break is overdue and comes first"));
    assert!(c
        .drive_time_summary("realistic", true)
        .contains("break overdue"));
}

#[test]
fn test_drive_time_key_carries_the_pending_sleeper_split() {
    let mut c = HosClock::new();
    c.drive(300.0);
    c.sleeper_split_rest(480.0);

    for terse in [false, true] {
        assert!(c
            .drive_time_summary("realistic", terse)
            .contains("Sleeper split open"));
    }
    // The split belongs to the shift-ending key, not the other two.
    assert!(!c
        .break_summary("realistic", false)
        .contains("Sleeper split"));
    assert!(!c
        .wheel_time_summary("realistic", false)
        .contains("Sleeper split"));
}

#[test]
fn test_drive_time_key_says_no_limit_with_enforcement_off() {
    let mut c = HosClock::new();
    c.drive(20.0 * 60.0);

    for mode in ["off", "debug_off"] {
        let said = c.drive_time_summary(mode, false);
        assert!(said.contains("no limit"));
        assert!(said.contains("Hours enforcement off."));
    }
}

#[test]
fn test_the_three_hours_keys_never_share_a_first_word() {
    let mut c = HosClock::new();
    c.drive(120.0);

    for terse in [false, true] {
        let firsts: std::collections::HashSet<String> = [
            c.wheel_time_summary("realistic", terse),
            c.break_summary("realistic", terse),
            c.drive_time_summary("realistic", terse),
        ]
        .iter()
        .map(|said| said.split_whitespace().next().unwrap().to_string())
        .collect();
        assert_eq!(firsts.len(), 3);
    }
}

#[test]
fn test_relaxed_does_not_speak_thirteen_seventy_five_as_hours_of_service() {
    let mut c = HosClock::new();
    c.drive(10.0 * 60.0);
    for said in [
        c.wheel_time_summary("relaxed", false),
        c.break_summary("relaxed", false),
        c.drive_time_summary("relaxed", false),
        c.summary("relaxed"),
    ] {
        assert!(!said.contains("13.75"), "{said}");
        assert!(!said.contains("13.8"), "{said}");
    }
}

// -- modes -------------------------------------------------------------------

#[test]
fn test_relaxed_keeps_the_same_eleven_fourteen_and_break() {
    assert_eq!(limits("relaxed"), limits("realistic"));
    assert_eq!(
        limits("relaxed").unwrap(),
        (11.0 * 60.0, 14.0 * 60.0, 8.0 * 60.0)
    );
}

#[test]
fn test_relaxed_softens_fines_not_the_clock() {
    assert_eq!(hos_fine("realistic", 0), 200.0);
    assert_eq!(hos_fine("relaxed", 0), 100.0);
    assert_eq!(hos_fine("relaxed", 3), 1000.0);
    let mut c = HosClock::new();
    c.drive(481.0); // past the 8-hour break, still inside 11 and 14
    assert!(c.in_violation("relaxed"));
    assert!(c.is_break_only_violation("relaxed"));
    assert_eq!(c.out_of_service_minutes("relaxed"), 30.0);
    c.drive(200.0); // 681: past the 11-hour drive
    assert!(!c.is_break_only_violation("relaxed"));
    assert_eq!(c.out_of_service_minutes("relaxed"), 600.0);
}

#[test]
fn test_relaxed_mode_warns_on_the_same_clock_as_realistic() {
    let mut c = HosClock::new();
    c.drive(470.0); // 10 minutes left before the 8-hour break
    assert!(!c.check_warnings("relaxed").is_empty());
    assert!(!c.in_violation("relaxed"));
    c.drive(20.0); // 490: past the 8-hour break
    assert!(c.in_violation("relaxed"));
}

#[test]
fn test_off_mode_never_warns_or_violates() {
    let mut c = HosClock::new();
    c.drive(10_000.0);
    assert!(c.check_warnings("off").is_empty());
    assert!(!c.in_violation("off"));
    assert_eq!(c.remaining_min("off"), None);
    assert!(!c.summary("off").contains("developer mode"));
    assert!(c.summary("off").contains("Hours enforcement off."));
}

// -- serialization and compatibility ----------------------------------------------

#[test]
fn test_clock_roundtrips_through_dict() {
    let mut c = HosClock::new();
    c.drive(123.0);
    c.check_warnings("realistic");
    let again = HosClock::from_dict(&c.to_dict());
    assert_eq!(again, c);
}

#[test]
fn test_legacy_clock_data_migrates_to_eld_fields() {
    let data = json!({"driving_min": 120, "duty_min": 180, "since_break_min": 60});
    let clock = HosClock::from_dict(&data);
    assert_eq!(clock.driving_min, 120.0);
    assert_eq!(clock.duty_min, 180.0);
    assert_eq!(clock.since_break_min, 60.0);
    assert_eq!(clock.status, "off_duty");
    assert_eq!(clock.non_driving_min, 0.0);
}

#[test]
fn test_legacy_history_full_reset_does_not_become_pending_split() {
    let mut c = HosClock::new();
    c.drive(300.0);
    c.sleeper(600.0);
    let mut data = c.to_dict();
    data.as_object_mut().unwrap().remove("split_rest_history");

    let mut again = HosClock::from_dict(&data);

    assert_eq!(again.split_pending_summary(), None);
    again.drive(60.0);
    again.off_duty(120.0);
    assert!(approx(again.driving_min, 60.0));
    assert!(approx(again.duty_min, 180.0));
}

#[test]
fn test_clock_from_garbage_is_fresh() {
    assert_eq!(HosClock::from_dict(&Value::Null), HosClock::new());
    assert_eq!(HosClock::from_dict(&json!("nonsense")), HosClock::new());
    assert_eq!(
        HosClock::from_dict(&json!({"driving_min": "NaN-ish?"})),
        HosClock::new()
    );
    assert_eq!(
        HosClock::from_dict(&json!({"driving_min": []})),
        HosClock::new()
    );
}

#[test]
fn clock_from_dict_keeps_the_python_edges() {
    // A numeric string is a float; a bool is an int; a non-iterable history
    // or warned list is a TypeError (fresh clock); a bad status is off_duty;
    // an unreadable event is skipped, not fatal.
    let loaded = HosClock::from_dict(&json!({
        "driving_min": " 12.5 ",
        "duty_min": true,
        "status": "napping",
        "history": [{"status": "driving", "minutes": 5}, {"status": "bogus"}, 7],
        "warned": "ab",
        "split_credit_key": 42,
    }));
    assert_eq!(loaded.driving_min, 12.5);
    assert_eq!(loaded.duty_min, 1.0);
    assert_eq!(loaded.status, "off_duty");
    assert_eq!(loaded.history.len(), 1);
    assert_eq!(loaded.warned, vec!["a".to_string(), "b".to_string()]);
    assert_eq!(loaded.split_credit_key.as_deref(), Some("42"));
    assert_eq!(HosClock::from_dict(&json!({"history": 3})), HosClock::new());
    assert_eq!(
        HosClock::from_dict(&json!({"warned": null})),
        HosClock::new()
    );
    assert_eq!(
        HosClock::from_dict(&json!({"history": [{"status": "driving", "minutes": "x"}]})),
        HosClock::new()
    );
}

#[test]
fn test_v2_profile_loads_with_fresh_clock_and_no_fatigue() {
    use crate::models::profile::{tests::with_data_dir, Profile};
    // A version-2 save predates the clock and the fatigue meter. Version 2 is
    // a 1.8-line number, so the load gate would refuse it on disk; the shape
    // tolerance it pinned lives in `from_dict`, which every entry point runs.
    with_data_dir(|_| {
        let p = Profile::named("V2 Driver");
        let mut data = p.to_dict();
        data.insert("version".to_string(), json!(2));
        data.remove("_signature");
        data.remove("_signature_version");
        data.remove("hos");
        data.remove("fatigue");
        let loaded = Profile::from_dict(&data);
        assert_eq!(loaded.hos, HosClock::new());
        assert_eq!(loaded.fatigue, 0.0);
    });
}

#[test]
fn test_profile_persists_hos_and_fatigue() {
    use crate::models::profile::{tests::with_data_dir, Profile};
    with_data_dir(|_| {
        let mut p = Profile::named("Tired Driver");
        p.hos.drive(345.0);
        p.fatigue = 67.5;
        let loaded = Profile::load(&p.save().unwrap()).unwrap();
        assert_eq!(loaded.hos.driving_min, 345.0);
        assert_eq!(loaded.fatigue, 67.5);
    });
}

#[test]
fn test_duty_log_records_coalesces_and_roundtrips() {
    let mut log = DutyLog::new();
    log.record("driving", 6.0, 7.0, "I-90 from Chicago to Toledo", "");
    log.record("driving", 7.0, 7.5, "I-90 from Chicago to Toledo", "");
    log.record(
        "off_duty",
        7.5,
        8.0,
        "Ohio Turnpike service plaza",
        "30-minute break",
    );

    assert_eq!(log.segments.len(), 2);
    assert!(approx(log.segments[0].duration_hours(), 1.5));
    assert!(approx(log.totals_since(6.0, 8.0).get("driving"), 1.5));

    let again = DutyLog::from_dict(&log.to_dict());
    assert_eq!(again.segments.len(), 2);
    assert_eq!(again.segments[1].note, "30-minute break");
}

#[test]
fn duty_log_prunes_coalesces_and_reads_back_tolerantly() {
    let mut log = DutyLog::new();
    log.record("driving", 0.0, 1.0, "", "");
    assert_eq!(log.segments[0].location, "unknown location");
    log.record("on_duty_not_driving", 2.0, 3.0, "dock", "");
    // the gap is closed by stretching the previous row
    assert_eq!(log.segments[0].end_hour, 2.0);
    log.record("sleeper_berth", 3.0, 3.0, "dock", ""); // zero length: dropped
    log.record("napping", 3.0, 4.0, "dock", ""); // unknown status: dropped
    assert_eq!(log.segments.len(), 2);
    assert_eq!(log.current_status(), "on_duty_not_driving");
    assert_eq!(log.recent(1).len(), 1);
    log.record("off_duty", 3.0, 3.0 + RODS_WINDOW_HOURS + 1.0, "home", "");
    // rows that ended before the window are gone; a straddling row is cut
    assert_eq!(log.segments.len(), 1);
    assert_eq!(log.segments[0].start_hour, 4.0);

    let loaded = DutyLog::from_dict(&json!({"segments": [
        {"status": "driving", "start_hour": 5.0, "end_hour": 4.0, "location": 0},
        {"status": "off_duty", "start_hour": 1.0, "end_hour": 2.0, "note": null},
        {"status": "driving", "start_hour": "inf"},
        {"status": "lunch", "start_hour": 1.0},
        "junk",
    ]}));
    assert_eq!(loaded.segments.len(), 2);
    assert_eq!(loaded.segments[0].status, "off_duty"); // sorted by start
    assert_eq!(loaded.segments[1].end_hour, 5.0); // end never before start
    assert_eq!(loaded.segments[1].location, "unknown location");
    assert_eq!(loaded.segments[0].note, "");
    assert_eq!(DutyLog::from_dict(&json!(5)), DutyLog::new());
    assert_eq!(DutyLog::from_dict(&json!({"segments": 5})), DutyLog::new());
    assert_eq!(DutyTotals::default().entries().len(), 4);
}

#[test]
fn test_profile_persists_duty_log() {
    use crate::models::profile::{tests::with_data_dir, Profile};
    with_data_dir(|_| {
        let mut p = Profile::named("Log Driver");
        p.duty_log.record(
            "on_duty_not_driving",
            6.0,
            6.25,
            "Chicago terminal",
            "pre-trip",
        );
        let loaded = Profile::load(&p.save().unwrap()).unwrap();
        assert_eq!(loaded.duty_log.segments.len(), 1);
        assert_eq!(loaded.duty_log.segments[0].location, "Chicago terminal");
    });
}

// -- day/night ---------------------------------------------------------------------

#[test]
fn test_time_of_day_bands() {
    assert_eq!(time_of_day(6.0), "dawn");
    assert_eq!(time_of_day(12.0), "day");
    assert_eq!(time_of_day(20.0), "dusk");
    assert_eq!(time_of_day(23.0), "night");
    assert_eq!(time_of_day(3.0), "night");
    assert_eq!(time_of_day(27.0), "night"); // wraps past midnight
    assert!(is_night(22.0) && !is_night(10.0));
}

#[test]
fn test_clock_text() {
    assert_eq!(clock_text(6.0), "6 AM");
    assert_eq!(clock_text(0.0), "12 AM");
    assert_eq!(clock_text(12.0), "12 PM");
    assert_eq!(clock_text(23.5), "11:30 PM");
    assert_eq!(clock_text(30.0), "6 AM");
}

#[test]
fn test_clock_text_minute_rounding_carries_the_hour() {
    // 59.99 minutes must round up to the next hour, not speak "11:60 PM",
    // and the AM/PM flip must follow the carried hour.
    assert_eq!(clock_text(23.9999), "12 AM");
    assert_eq!(clock_text(11.9999), "12 PM");
    assert_eq!(clock_text(12.9999), "1 PM");
}

#[test]
fn clock_text_pads_minutes_and_wraps_negative_hours() {
    assert_eq!(clock_text(13.05), "1:03 PM");
    assert_eq!(clock_text(-2.0), "10 PM");
    assert_eq!(duration_text(0.4), "24 minutes");
    assert_eq!(duration_text(1.25), "1.2 hours");
    assert_eq!(duration_text(-3.0), "0 minutes");
    // Whole hours are whole: "three point zero hours" read aloud was the
    // owner's complaint (2026-09-14).
    assert_eq!(duration_text(3.0), "3 hours");
    assert_eq!(duration_text(1.0), "1 hour");
    assert_eq!(duration_text(1.0 / 60.0), "1 minute");
    assert_eq!(duration_text(2.96), "3 hours");
    assert_eq!(
        duty_status_label("on_duty_not_driving"),
        "on duty, not driving"
    );
    assert_eq!(duty_status_label("yard_move"), "yard move");
}

// -- Trip-backed day/night cases (tests/test_weather_trip.py::make_trip) --------

/// `make_trip(world, start, end, seed, start_hour)`: a quiet Chicago run with
/// an automatic, running truck and the rolling traffic bubble off.
fn make_trip(start: &str, end: &str, seed: i64, start_hour: f64) -> crate::sim::trip::Trip {
    use crate::data::world::get_world;
    use crate::sim::trip::{Trip, TripOptions};
    use crate::sim::vehicle::TruckState;
    use crate::sim::weather::test_support::new_system;

    let route = get_world().route_options(start, end, 3, false).unwrap()[0].clone();
    let mut truck = TruckState::default();
    truck.transmission.automatic = true;
    truck.start_engine();
    let mut trip = Trip::new(
        route,
        truck,
        new_system("great_lakes", Some(1), None, None, true),
        TripOptions {
            seed: Some(seed),
            start_hour,
            ..Default::default()
        },
    );
    trip.traffic_manager.rolling_bubble = false;
    trip
}

#[test]
fn test_night_zone_layout_is_deterministic() {
    let a = make_trip("Chicago", "Indianapolis", 11, 23.0);
    let b = make_trip("Chicago", "Indianapolis", 11, 23.0);
    assert_eq!(a.zones, b.zones);
}

#[test]
fn test_night_produces_sparser_traffic() {
    // Congestion zones are fixed in space (volume-prone stretches) but
    // follow the clock: the same stretch that jams at the evening rush is
    // open road in the small hours.
    use crate::data::world::get_world;
    use crate::data::world_models::{Route, TrafficVolumeSample};
    use crate::sim::trip::{Trip, TripOptions};
    use crate::sim::vehicle::TruckState;
    use crate::sim::weather::test_support::new_system;
    use std::sync::Arc;

    let cached = get_world()
        .route_options("Atlanta", "Dallas", 3, false)
        .unwrap()[0]
        .clone();
    let mut detail = cached.legs[0].corridor().clone();
    detail.traffic_volumes = vec![
        TrafficVolumeSample {
            at_mi: 0.0,
            aadt: 150000.0,
            lanes: 3,
            source: String::new(),
        },
        TrafficVolumeSample {
            at_mi: 12.0,
            aadt: 20000.0,
            lanes: 2,
            source: String::new(),
        },
    ];
    let mut legs = cached.legs.clone();
    legs[0] = Arc::new((*cached.legs[0]).clone().with_detail(detail));
    let route = Route::new(cached.cities.clone(), legs);

    let trip_at = |hour: f64| {
        let mut truck = TruckState::default();
        truck.transmission.automatic = true;
        Trip::new(
            route.clone(),
            truck,
            new_system("atlantic_southeast", Some(1), None, None, true),
            TripOptions {
                seed: Some(2),
                start_hour: hour,
                ..Default::default()
            },
        )
    };

    let rush = trip_at(17.0);
    let mut jams: Vec<_> = rush
        .zones
        .iter()
        .filter(|z| z.reason == "heavy traffic")
        .cloned()
        .collect();
    assert!(!jams.is_empty() && jams.iter().all(|z| z.aadt.is_some()));
    assert!(jams.iter_mut().any(|z| rush.zone_is_active(z)));

    let night = trip_at(3.0);
    let mut night_jams: Vec<_> = night
        .zones
        .iter()
        .filter(|z| z.reason == "heavy traffic")
        .cloned()
        .collect();
    assert!(!night_jams.is_empty());
    assert!(!night_jams.iter_mut().any(|z| night.zone_is_active(z)));
}

#[test]
fn test_rush_hour_increases_corridor_traffic_density() {
    let rush = make_trip("Chicago", "Indianapolis", 2, 8.0);
    let midday = make_trip("Chicago", "Indianapolis", 2, 12.0);
    let leg = rush.route.legs[0].clone();
    assert!(
        rush.leg_traffic_density(&leg, 0.0, false) > midday.leg_traffic_density(&leg, 0.0, false)
    );
}

#[test]
fn test_night_raises_hazard_risk() {
    let day = make_trip("Chicago", "Indianapolis", 2, 12.0);
    let night = make_trip("Chicago", "Indianapolis", 2, 23.0);
    assert!(approx(night.hazard_risk(), day.hazard_risk() + 0.10));
}

#[test]
fn test_trip_current_hour_advances_with_game_time() {
    let mut trip = make_trip("Chicago", "Indianapolis", 2, 6.0);
    trip.game_minutes = 18.0 * 60.0;
    assert!(trip.current_hour().abs() < 1e-9); // 6 AM + 18 h = midnight
}

// -- fatigue ---------------------------------------------------------------------

#[test]
fn test_fatigue_grows_faster_at_night() {
    assert!(fatigue_rate_per_min(true) > fatigue_rate_per_min(false));
}

#[test]
fn awake_fatigue_gain_uses_day_and_night_rates() {
    // Eight daytime hours at the day rate.
    let day_8h = awake_fatigue_gain(8.0, 8.0 * 60.0);
    assert!(approx(day_8h, fatigue_rate_per_min(false) * 8.0 * 60.0));
    // A full day crosses night, so it outpaces pure day rate and stays finite.
    let day_24h = awake_fatigue_gain(8.0, 24.0 * 60.0);
    assert!(day_24h > fatigue_rate_per_min(false) * 24.0 * 60.0);
    assert!(day_24h.is_finite());
}

#[test]
fn course_fatigue_is_half_the_driving_rate_day_and_night() {
    // An 8 h day class from 10: 0.0575/min x 480 = 27.6, landing at 37.6.
    let day = course_fatigue(10.0, 8.0, 8.0 * 60.0, |_| 1.0);
    assert!(approx(day, 37.6), "8 h day class: {day}");
    assert!(approx(
        day - 10.0,
        CLASSROOM_FATIGUE_FACTOR * awake_fatigue_gain(8.0, 8.0 * 60.0)
    ));
    // 9 PM to 5 AM is all night: 0.085/min x 480 = 40.8, landing at 50.8.
    let night = course_fatigue(10.0, 21.0, 8.0 * 60.0, |_| 1.0);
    assert!(approx(night, 50.8), "8 h night class: {night}");
}

#[test]
fn course_fatigue_scale_multiplies_each_minute() {
    // A buff at rate 0.5 for the first four hours only.
    let buffed = course_fatigue(0.0, 8.0, 8.0 * 60.0, |h| if h < 4.0 { 0.5 } else { 1.0 });
    let plain_half = 0.0575 * 240.0;
    assert!(approx(buffed, plain_half * 0.5 + plain_half), "{buffed}");
}

#[test]
fn multi_day_course_counts_only_the_last_class_day() {
    // 24 h from 7 AM: the last class day is 8 AM to 4 PM, all day rate:
    // 0.0575 x 480 = 27.6, whatever the driver walked in with.
    let fresh = course_fatigue(0.0, 7.0, 24.0 * 60.0, |_| 1.0);
    let tired = course_fatigue(95.0, 7.0, 24.0 * 60.0, |_| 1.0);
    assert!(approx(fresh, 27.6), "24 h course: {fresh}");
    assert!(
        approx(fresh, tired),
        "arrival fatigue is replaced, not added"
    );
    assert!(approx(
        fresh,
        CLASSROOM_FATIGUE_FACTOR * awake_fatigue_gain(8.0, 8.0 * 60.0)
    ));
    // The 10 h threshold: a 10 h course is two days, a 9.9 h course is one.
    let ten = course_fatigue(50.0, 8.0, 10.0 * 60.0, |_| 1.0);
    assert!(approx(ten, 27.6), "10 h course: {ten}");
    let under = course_fatigue(50.0, 8.0, 9.9 * 60.0, |_| 1.0);
    assert!(under > 50.0);
    // The cliff the cutoff comment describes: 599 min from 8 AM at 50.
    let cliff = course_fatigue(50.0, 8.0, 599.0, |_| 1.0);
    assert!(approx(cliff, 50.0 + 0.0575 * 599.0), "599 min: {cliff}");
}

#[test]
fn multi_day_course_class_day_is_daytime_from_any_start_hour() {
    // Schools teach 8 AM to 4 PM: a course starting at 3 to 7 AM must not
    // run its last class day overnight.
    for start in [3.0, 5.0, 7.0, 12.0, 21.0, 23.5, 0.0] {
        for arrival in [0.0, 40.0, 95.0] {
            let after = course_fatigue(arrival, start, 24.0 * 60.0, |_| 1.0);
            assert!(
                approx(after, 27.6),
                "start {start}, arrival {arrival}: {after}"
            );
        }
    }
    // Days past the first and a long-running game clock change nothing.
    assert!(approx(
        course_fatigue(0.0, 1000.0 + 5.0, 24.0 * 60.0, |_| 1.0),
        27.6
    ));
    assert!(approx(
        course_fatigue(0.0, 21.0, 10.0 * 60.0, |_| 1.0),
        27.6
    ));
}

#[test]
fn multi_day_course_buffs_follow_the_class_day_hours() {
    // From 3 AM the class day runs 5 to 13 hours into the course. A half
    // rate buff for the first 9 hours covers its first four class hours.
    let buffed = course_fatigue(0.0, 3.0, 24.0 * 60.0, |h| if h < 9.0 { 0.5 } else { 1.0 });
    let half = 0.0575 * 240.0;
    assert!(approx(buffed, half * 0.5 + half), "from 3 AM: {buffed}");
    // From 9 PM the class day is 8 AM to 4 PM next day, 11 to 19 hours in.
    let buffed = course_fatigue(0.0, 21.0, 24.0 * 60.0, |h| if h < 15.0 { 0.5 } else { 1.0 });
    assert!(approx(buffed, half * 0.5 + half), "from 9 PM: {buffed}");
    // A flat mode multiplier scales the whole day.
    assert!(approx(
        course_fatigue(0.0, 5.0, 24.0 * 60.0, |_| 0.8),
        0.8 * 27.6
    ));
}

#[test]
fn course_fatigue_clamps_and_ignores_bad_minutes() {
    assert_eq!(course_fatigue(95.0, 21.0, 8.0 * 60.0, |_| 1.0), 100.0);
    assert_eq!(course_fatigue(12.0, 8.0, f64::NAN, |_| 1.0), 12.0);
    assert_eq!(course_fatigue(12.0, 8.0, -60.0, |_| 1.0), 12.0);
}

#[test]
fn test_fatigue_shortens_the_reaction_window() {
    assert_eq!(reaction_window_mult(0.0), 1.0);
    assert_eq!(reaction_window_mult(FATIGUE_DROWSY), 1.0);
    assert!(reaction_window_mult(90.0) < 1.0);
    assert!(approx(reaction_window_mult(100.0), 0.6));
}

#[test]
fn test_rest_helpers() {
    assert!(approx(rest_coffee_break(50.0), 42.0));
    assert_eq!(rest_coffee_break(6.0), 0.0);
    assert!(rest_coffee_break(50.0) > rest_break(50.0));
    let daytime_boost_min = (50.0 - rest_coffee_break(50.0)) / fatigue_rate_per_min(false);
    let daytime_break_min = (50.0 - rest_break(50.0)) / fatigue_rate_per_min(false);
    assert!(60.0 < daytime_boost_min && daytime_boost_min < daytime_break_min);
    assert!(approx(rest_break(50.0), 15.0));
    assert_eq!(rest_break(10.0), 0.0);
    assert_eq!(rest_sleep(99.0), 0.0);
    assert_eq!(rest_shoulder(90.0), 30.0); // poor rest floor
    assert_eq!(rest_shoulder(10.0), 10.0); // never adds fatigue
}

#[test]
fn rest_sleeper_split_floors_by_completion() {
    assert_eq!(rest_sleeper_split(50.0, 120.0, false), 32.0);
    assert_eq!(rest_sleeper_split(50.0, 480.0, false), 20.0);
    assert_eq!(rest_sleeper_split(50.0, 480.0, true), 10.0);
    assert_eq!(rest_sleeper_split(5.0, 120.0, true), 5.0);
    assert_eq!(rest_sleeper_split(0.0, 180.0, false), 0.0);
}

#[test]
fn test_shoulder_damage_is_deterministic() {
    for seed in 0..20 {
        assert_eq!(
            shoulder_damage_due(seed, 88.0),
            shoulder_damage_due(seed, 88.0)
        );
    }
    let results: std::collections::HashSet<bool> = (0..100)
        .map(|seed| shoulder_damage_due(seed, 88.0))
        .collect();
    assert_eq!(results.len(), 2);
}

#[test]
fn shoulder_fine_is_deterministic_and_sometimes_due() {
    for seed in 0..20 {
        assert_eq!(shoulder_fine_due(seed, 88.0), shoulder_fine_due(seed, 88.0));
    }
    let results: std::collections::HashSet<bool> =
        (0..100).map(|seed| shoulder_fine_due(seed, 88.0)).collect();
    assert_eq!(results.len(), 2);
}

// -- overnight parking ----------------------------------------------------------------

#[test]
fn test_parking_is_only_scarce_at_night() {
    assert_eq!(parking_full_probability(12.0, 0), 0.0);
    assert_eq!(parking_full_probability(19.9, 0), 0.0);
    assert!(0.0 < parking_full_probability(20.0, 0));
    assert!(parking_full_probability(20.0, 0) < parking_full_probability(23.0, 0));
    assert!(parking_full_probability(1.0, 0) > parking_full_probability(20.0, 0));
    assert!(parking_full_probability(3.9, 0) > 0.0);
    assert_eq!(parking_full_probability(4.0, 0), 0.0);
}

#[test]
fn test_parking_full_is_deterministic_per_seed_and_stop() {
    for seed in 0..20 {
        assert_eq!(
            parking_is_full(seed, 88.0, 23.0, 0),
            parking_is_full(seed, 88.0, 23.0, 0)
        );
    }
    // both outcomes occur across seeds
    let results: std::collections::HashSet<bool> = (0..100)
        .map(|s| parking_is_full(s, 88.0, 23.0, 0))
        .collect();
    assert_eq!(results.len(), 2);
}

#[test]
fn test_parking_fills_more_often_later_in_the_evening() {
    let full_at = |h: f64| (0..200).filter(|&s| parking_is_full(s, 88.0, h, 0)).count();
    assert!(full_at(20.5) < full_at(23.5));
}

// Ported from tests/test_truck_parking_capacity.py (capacity-aware crunch).

#[test]
fn test_parking_crunch_unchanged_when_capacity_unknown() {
    assert_eq!(
        parking_full_probability(23.0, 0),
        parking_full_probability(23.0, 0)
    );
}

#[test]
fn test_small_lots_fill_earlier_and_big_lots_later() {
    let base = parking_full_probability(23.0, 0);
    assert!(base > 0.0);
    assert!(parking_full_probability(23.0, 8) > base);
    assert!(parking_full_probability(23.0, 150) < base);
    assert!(parking_full_probability(23.0, 60) < base);
}

#[test]
fn test_capacity_never_creates_daytime_crunch() {
    assert_eq!(parking_full_probability(12.0, 5), 0.0);
}

// Ported from tests/test_stop_detail.py (the stop details screen's HOS clause).

#[test]
fn test_arrival_note_names_only_the_limit_that_matters() {
    let mut clock = HosClock::new();
    // Fresh clock: the 8-hour break is the nearest limit.
    assert!(clock
        .arrival_note("realistic", 60.0)
        .contains("break is due"));
    // Arriving after the nearest limit warns instead.
    let late = clock.arrival_note("realistic", 10.0 * 60.0);
    assert!(late.contains("before you would reach it"));
    assert!(late.contains("break"));
    // Duty window closing before the break drops the break entirely.
    clock.duty_min = 13.5 * 60.0;
    let note = clock.arrival_note("realistic", 15.0);
    assert!(note.contains("duty window closes"));
    assert!(!note.contains("break"));
    // Non-enforced modes stay quiet.
    assert_eq!(HosClock::new().arrival_note("off", 60.0), "");
}

#[test]
fn arrival_note_spells_the_gap_in_hours() {
    let clock = HosClock::new();
    assert_eq!(
        clock.arrival_note("realistic", 10.0 * 60.0),
        " Your break comes about 2 hours before you would reach it."
    );
    assert_eq!(
        clock.arrival_note("realistic", 60.0),
        " You would arrive before your 30-minute break is due."
    );
}

// -- what a Trip alone can answer -------------------------------------------------------
//
// The rest of the `test_hos.py` integration block needs a `DrivingState`; see
// the module note for where those cases live now.

#[test]
fn test_inspections_fire_only_in_violation() {
    use crate::sim::trip_models::TripEventKind;

    let run_trip = |violating: bool| {
        let mut trip = make_trip("Chicago", "Indianapolis", 5, 12.0);
        trip.truck.start_engine();
        trip.truck.throttle = 0.85;
        trip.hos_violation = violating;
        let mut inspections = 0;
        for _ in 0..(60 * 60 * 30) {
            trip.truck.auto_shift();
            trip.truck.update(1.0 / 60.0);
            inspections += trip
                .update(1.0 / 60.0)
                .iter()
                .filter(|e| e.kind == TripEventKind::Inspection)
                .count();
            if trip.finished {
                break;
            }
        }
        inspections
    };

    assert_eq!(run_trip(false), 0);
    assert!(run_trip(true) >= 1);
}

#[test]
fn test_route_backed_weigh_station_emits_evidence() {
    use crate::sim::trip_models::{RoadStop, TripEventKind};

    let mut trip = make_trip("Chicago", "Indianapolis", 5, 12.0);
    let mut scale = RoadStop::new("Example Scale", 10.0, "weigh_station");
    scale.actions = vec!["inspect".to_string()];
    trip.stops = vec![scale];
    trip.position_mi = 10.1;
    trip.hos_violation = true;
    trip.events = Vec::new();

    trip.check_inspections(1.0);

    let events: Vec<_> = trip
        .events
        .iter()
        .filter(|e| e.kind == TripEventKind::Inspection)
        .collect();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].data.context.as_deref(), Some("weigh_station"));
    assert_eq!(
        events[0].data.evidence,
        Some(vec!["HOS/ELD violation".to_string()])
    );
}

#[test]
fn test_re_arm_warnings_speaks_the_countdown_again_after_a_non_reset_rest() {
    // A pending-split sleep used to leave the once-per-shift warning marks in
    // place, so the driver woke to silence and hit the window with no
    // countdown (owner, 2026-07-24).
    let mut clock = HosClock::new();
    clock.drive(13.0 * 60.0); // deep into the window: all thresholds fired
    assert!(!clock.check_warnings("realistic").is_empty());
    assert!(clock.check_warnings("realistic").is_empty()); // marks hold within a shift
    clock.re_arm_warnings();
    assert!(!clock.check_warnings("realistic").is_empty()); // spoken again after waking
}

#[test]
fn test_violation_causes_name_the_blown_limits_plainly() {
    let mut clock = HosClock::new();
    clock.drive(14.0 * 60.0 + 30.0);
    let causes = clock.violation_causes("realistic");
    assert!(causes.iter().any(|c| c.contains("11-hour driving limit")));
    assert!(causes.iter().any(|c| c.contains("14-hour duty window")));
}

// -- port-specific pins ---------------------------------------------------------------

#[test]
fn split_event_key_is_the_python_repr() {
    let first = HosEvent::new("sleeper_berth", 480.0, 120.0, 300.0, 0.0, "normal");
    let second = HosEvent::new("off_duty", 120.0, 480.0, 780.0, 0.0, "normal");
    assert_eq!(
        split_event_key(&first, &second),
        "(('sleeper_berth', 'normal', 480.0, 120.0, 300.0, 0.0), \
         ('off_duty', 'normal', 120.0, 480.0, 780.0, 0.0))"
    );
    // The key a real 8/2 split stores, worked by hand from the ledger. The
    // second event's duty figure is 600, not 1080: the 8-hour berth period
    // paused the window, so only the two 300-minute drives had counted.
    let mut c = HosClock::new();
    c.drive(300.0);
    c.sleeper_split_rest(480.0);
    c.drive(300.0);
    c.sleeper_split_rest(120.0);
    assert_eq!(
        c.split_credit_key.as_deref(),
        Some(
            "(('sleeper_berth', 'normal', 480.0, 300.0, 300.0, 300.0), \
             ('sleeper_berth', 'normal', 120.0, 600.0, 600.0, 300.0))"
        )
    );
    assert_eq!(py_repr_str("it's"), "\"it's\"");
    assert_eq!(py_repr_str("a\\b\n"), "'a\\\\b\\n'");
}

#[test]
#[should_panic(expected = "HOS time increments must be finite positive minutes")]
fn drive_rejects_negative_minutes() {
    HosClock::new().drive(-1.0);
}

#[test]
#[should_panic(expected = "HOS time increments must be finite positive minutes")]
fn sleeper_rejects_non_finite_minutes() {
    HosClock::new().sleeper(f64::NAN);
}

#[test]
fn python_float_and_str_coercions() {
    assert_eq!(py_float_str(" 1_000.5 "), Some(1000.5));
    assert_eq!(py_float_str("1__0"), None);
    assert_eq!(py_float_str("_1"), None);
    assert_eq!(py_float_str("-inf"), Some(f64::NEG_INFINITY));
    assert!(py_float_str("nan").unwrap().is_nan());
    assert_eq!(py_float_str("1e3"), Some(1000.0));
    assert_eq!(py_float_str(""), None);
    assert_eq!(py_float_str("0x10"), None);
    assert_eq!(py_str(&json!(5)), "5");
    assert_eq!(py_str(&json!(5.0)), "5.0");
    assert_eq!(py_str(&json!(true)), "True");
    assert_eq!(py_str(&json!(null)), "None");
    assert_eq!(py_repr(&json!(["a", 1, null])), "['a', 1, None]");
    assert_eq!(py_repr(&json!({"k": "v"})), "{'k': 'v'}");
    assert_eq!(py_iter(None), Some(vec![]));
    assert_eq!(py_iter(Some(&json!(3))), None);
    assert_eq!(
        py_iter(Some(&json!({"b": 1}))),
        Some(vec![Value::String("b".into())])
    );
}

#[test]
fn next_limit_keeps_the_first_on_a_tie() {
    // break and drive both at 480 remaining: Python's min keeps "break".
    let mut c = HosClock::new();
    c.driving_min = 180.0;
    c.since_break_min = 0.0;
    let limit = c.next_limit("realistic").unwrap();
    assert_eq!(limit.kind, "break");
    assert_eq!(limit.remaining_min, 480.0);
    assert!(warning_is_urgent("Hours of service violation: x"));
    assert!(!warning_is_urgent("Hours of service: 2 hours until x."));
    assert!(HOS_MODES.contains(&"relaxed"));
    assert_eq!(HOS_FINES[3], 2000.0);
    assert_eq!(SHOULDER_SLEEP_LIMIT_BUFFER_MIN, 120.0);
    assert_eq!(SHOULDER_DAMAGE_PCT, 3.0);
    assert_eq!(FATIGUE_SEVERE, 80.0);
}

#[test]
fn test_drive_and_duty_share_one_sleep_countdown() {
    // Driving time and the duty window end ten minutes apart, in the same
    // sleep: the run hears one countdown to it, not two interleaved.
    let mut c = HosClock::new();
    c.on_duty(170.0);
    let msgs = drive_collecting_realistic(&mut c, 655.0);
    let sleep_lines: Vec<&String> = msgs
        .iter()
        .filter(|m| m.contains("10 hours of sleep") && !m.contains("violation"))
        .collect();
    assert_eq!(sleep_lines.len(), 3, "{sleep_lines:?}");
    assert!(sleep_lines.iter().all(|m| m.contains("driving time")));
}

#[test]
fn test_duty_window_speaks_when_it_is_the_nearer_limit() {
    let mut c = HosClock::new();
    c.on_duty(600.0);
    let msgs = drive_collecting_realistic(&mut c, 125.0);
    assert!(
        msgs.iter()
            .any(|m| m.contains("2 hours") && m.contains("duty window")),
        "{msgs:?}"
    );
    assert!(!msgs.iter().any(|m| m.contains("driving time")));
}

#[test]
fn test_a_driving_mode_speaks_only_its_own_marks() {
    let mut c = HosClock::new();
    c.on_duty(170.0);
    let mut msgs = Vec::new();
    let mut elapsed = 0.0;
    while elapsed < 655.0 {
        c.drive(5.0);
        elapsed += 5.0;
        msgs.extend(c.check_warnings_with("realistic", &[60.0], |_| false));
    }
    let countdown: Vec<&String> = msgs
        .iter()
        .filter(|m| m.starts_with("Hours of service: "))
        .collect();
    assert!(
        countdown.iter().all(|m| m.contains("1 hour")),
        "{countdown:?}"
    );
    assert_eq!(
        countdown
            .iter()
            .filter(|m| m.contains("10 hours of sleep"))
            .count(),
        1
    );
}

#[test]
fn test_no_countdown_for_a_limit_the_trip_ends_inside() {
    let mut c = HosClock::new();
    c.drive(530.0);
    c.take_break(30.0);
    // 130 minutes of driving left; the destination is 90 minutes out.
    let mut spoken = Vec::new();
    for _ in 0..3 {
        c.drive(5.0);
        spoken.extend(c.check_warnings_with("realistic", &[120.0], |limit| {
            90.0 + 5.0 <= limit.remaining_min
        }));
    }
    assert!(spoken.is_empty(), "{spoken:?}");
    // The mark was spent in silence: it does not fire later either.
    c.drive(5.0);
    assert!(c
        .check_warnings_with("realistic", &[120.0], |_| false)
        .is_empty());
}
