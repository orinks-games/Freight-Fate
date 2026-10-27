//! Reputation reads the driving record: the ledger less what the record
//! still inside the review window costs, aging back out as the record does.

use super::*;
use crate::models::profile::Profile;

fn a_driver_at(ledger: f64) -> Profile {
    let mut p = Profile::named("Record Test");
    p.career.reputation = ledger;
    p.game_hours = 10.0 * HOURS_PER_DAY;
    p
}

#[test]
fn a_clean_record_leaves_reputation_alone() {
    let p = a_driver_at(98.0);
    assert_eq!(p.standing(), 98.0);
    assert_eq!(
        record_reputation_penalty(&p.driving_record, p.game_hours),
        0.0
    );
}

#[test]
fn citations_and_serious_violations_come_off_the_number_everyone_reads() {
    // Jess: two citations, three serious violations, a pinned ledger.
    let mut p = a_driver_at(98.0);
    let now = p.game_hours;
    p.driving_record
        .record_citation_at(150.0, now - HOURS_PER_DAY);
    p.driving_record
        .record_citation_at(150.0, now - 2.0 * HOURS_PER_DAY);
    for days in [3.0, 4.0, 5.0] {
        p.driving_record
            .record_serious_violation(now - days * HOURS_PER_DAY);
    }
    assert_eq!(record_reputation_penalty(&p.driving_record, now), 38.0);
    assert_eq!(p.standing(), 60.0);
    // The ledger itself is untouched.
    assert_eq!(p.career.reputation, 98.0);
    // Dispatch trust and the gates read the standing, not the ledger.
    assert_eq!(StandingProfile::career_reputation(&p), 60.0);
    assert_eq!(
        trust_band(StandingProfile::career_reputation(&p)),
        TRUST_FULL
    );
}

#[test]
fn the_record_lifts_off_reputation_after_a_game_year_though_the_review_still_sees_it() {
    let mut p = a_driver_at(90.0);
    let booked = p.game_hours;
    p.driving_record.record_serious_violation(booked);
    assert_eq!(p.standing(), 80.0);
    // Most of a year on: still weighing.
    p.game_hours = booked + 300.0 * HOURS_PER_DAY;
    assert_eq!(p.standing(), 80.0);
    // A year and a day: the points are back, and the carrier's review has
    // let it go too, while the license ladder still counts it for three.
    p.game_hours = booked + (REPUTATION_WINDOW_DAYS as f64 + 1.0) * HOURS_PER_DAY;
    assert_eq!(p.standing(), 90.0);
    assert_eq!(p.driving_record.serious_in_review_window(p.game_hours), 0);
    assert_eq!(p.driving_record.serious_in_window(p.game_hours), 1);
}

#[test]
fn a_major_offense_counts_for_good_and_the_record_alone_cannot_zero_a_driver() {
    let mut p = a_driver_at(100.0);
    let now = p.game_hours;
    p.driving_record.record_major_offense(now);
    assert_eq!(p.standing(), 80.0);
    p.game_hours = now + 20.0 * 365.0 * HOURS_PER_DAY;
    assert_eq!(p.standing(), 80.0, "a major offense never ages out");

    // Pile it on: the cap holds the record's cost at sixty.
    let now = p.game_hours;
    for _ in 0..8 {
        p.driving_record.record_serious_violation(now);
    }
    assert_eq!(
        record_reputation_penalty(&p.driving_record, now),
        RECORD_REPUTATION_CAP
    );
    assert_eq!(p.standing(), 40.0);
    p.career.reputation = 20.0;
    assert_eq!(p.standing(), 0.0, "clamped, never negative");
}

#[test]
fn the_save_carries_the_standing_for_the_public_profile() {
    let mut p = a_driver_at(98.0);
    p.driving_record.record_serious_violation(p.game_hours);
    let d = p.to_unsigned_dict();
    assert_eq!(d["career"]["reputation"], 98.0);
    assert_eq!(d["career"]["standing"], 88.0);
}

#[test]
fn a_crash_counts_as_a_serious_event_and_ages_out_like_one() {
    // Owner ruling 2026-09-24: a rollover goes on the record as a crash (the
    // 49 CFR 390.15 accident register), weighing as a serious event does.
    use crate::models::safety_record::{score_for_profile, SAFETY_RECORD_WINDOW_DAYS};
    let mut p = a_driver_at(98.0);
    let before = score_for_profile(&p, 0.0);
    let now = p.game_hours;
    p.driving_record.record_crash(now);
    p.driving_record.note(
        RECORD_CRASH,
        "Rolled the truck over in a bend",
        0.0,
        now,
        "I-40",
    );
    assert_eq!(p.driving_record.crashes, 1);
    assert_eq!(p.standing(), 98.0 - RECORD_SERIOUS_REPUTATION);
    let mut serious = a_driver_at(98.0);
    serious.driving_record.record_serious_violation(now);
    assert_eq!(
        score_for_profile(&p, 0.0),
        score_for_profile(&serious, 0.0),
        "a crash weighs on the safety record as a serious violation does"
    );
    assert!(score_for_profile(&p, 0.0) > before);
    // Not a citation, and not a serious violation on the license ladder.
    assert_eq!(p.driving_record.unexplained_citations(), 0);
    assert!(p.driving_record.serious_violations.is_empty());
    // A year on, it has aged out of both windows.
    p.game_hours =
        now + (SAFETY_RECORD_WINDOW_DAYS.max(REPUTATION_WINDOW_DAYS) as f64 + 1.0) * HOURS_PER_DAY;
    assert_eq!(p.standing(), 98.0);
    assert_eq!(p.driving_record.crashes, 1, "the lifetime count stays");
    // And it rides the save inside the driving record, not as a new
    // top-level field the cloud validator would refuse.
    let d = p.to_unsigned_dict();
    assert_eq!(d["driving_record"]["crashes"], 1);
    assert_eq!(d["driving_record"]["crash_times"][0], now);
}

/// Running off the road asleep goes on the record, and the Record line
/// says so instead of "clean" over the list that names it (issue #347).
#[test]
fn the_record_line_names_a_safety_incident_and_a_crash() {
    let mut p = a_driver_at(98.0);
    let now = p.game_hours;
    assert_eq!(standing_text(&p), "Record: clean.");
    p.driving_record.record_fatigue_event(now - HOURS_PER_DAY);
    assert_eq!(
        standing_text(&p),
        "Record: one safety incident in the last year."
    );
    p.driving_record.record_citation_at(150.0, now);
    p.driving_record.record_crash(now);
    assert!(
        standing_text(&p).starts_with(
            "Record: one citation, one safety incident and one crash in the last year."
        ),
        "{}",
        standing_text(&p)
    );
    // The second time asleep is a serious violation, counted once as that.
    let mut q = a_driver_at(98.0);
    let now = q.game_hours;
    q.driving_record
        .record_fatigue_event(now - 2.0 * HOURS_PER_DAY);
    q.driving_record.record_fatigue_event(now - HOURS_PER_DAY);
    assert!(
        standing_text(&q)
            .starts_with("Record: one safety incident in the last year, one serious violation."),
        "{}",
        standing_text(&q)
    );
    // A year on, the incident has aged out like a citation does.
    p.game_hours = now + (REVIEW_WINDOW_DAYS as f64 + 2.0) * HOURS_PER_DAY;
    assert_eq!(standing_text(&p), "Record: clean.");
}
