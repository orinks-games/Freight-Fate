//! Ported from `tests/test_enforcement_record.py`, plus the dispatch-trust
//! half of `tests/test_debt_and_standing.py`. What is here is the record's own
//! arithmetic: the fine schedule, the three-year window, the suspension dates
//! and the standing lines built from them.
//!
//! Cases that drive the app shell cannot run in this crate at all -- `ff-core`
//! cannot depend on the game crate, so the roadside screens and the drive are
//! invisible from here. They run where those screens are:
//!
//! - the stops, the ladder, the fatigue events, the pursuit opt-in and the
//!   reload exploits -- `crates/freight-fate/tests/states_driving_enforcement_record.rs`.
//! - the dispatch board and the terminal while suspended --
//!   `crates/freight-fate/tests/states_city.rs`.
//!
//! Cases needing a `Profile` save round trip, `dispatch_policy`, `business` or
//! `career` are still ignored with the reason; their bodies say what they
//! checked.

use serde_json::json;

use super::*;
use crate::models::solvency::test_profile::FakeProfile;
use crate::models::solvency::REPOSSESSION_FLOOR;
use crate::settings::ScoreDisplay;

const DAY: f64 = 24.0;

fn record() -> DrivingRecord {
    DrivingRecord::new()
}

fn profile() -> FakeProfile {
    FakeProfile::new()
}

/// `_profile()` in `tests/test_debt_and_standing.py`: the real save object,
/// for the cases that read the career off it.
fn real_profile() -> crate::models::profile::Profile {
    let mut p = crate::models::profile::Profile::named("Dale");
    p.current_city = "Buffalo".to_string();
    p
}

// --- money ------------------------------------------------------------------

#[test]
fn test_speeding_fine_climbs_with_how_far_over_the_limit() {
    let mild = speeding_citation_fine(8.0, 0, false);
    let serious = speeding_citation_fine(15.0, 0, false);
    let reckless = speeding_citation_fine(31.0, 0, false);
    assert!(mild < serious);
    assert!(serious < reckless);
    // The 15-mph step is where a citation also becomes a serious violation.
    assert!(serious >= 1_000.0);
}

#[test]
fn test_repeat_offenders_pay_more_up_to_the_cap() {
    // Priors compound the way real repeat offenders are charged "double, even
    // triple" a standard fine -- but the step stops at
    // CITATION_REPEAT_MAX_MULTIPLIER. Uncapped it reached 39,600 for a scale
    // bypass in roadwork, which reads as a broken game rather than a severe
    // one. See the constant for why solvency, not statute, sets the ceiling.
    let first = speeding_citation_fine(16.0, 0, false);
    let third = speeding_citation_fine(16.0, 2, false);
    assert!(third > first);
    // Beyond the cap the money stops climbing; the record keeps escalating.
    let many = speeding_citation_fine(35.0, 20, false);
    let plenty = speeding_citation_fine(35.0, 40, false);
    assert_eq!(plenty, many);
    assert_eq!(
        citation_fine(900.0, 99, false, None),
        900.0 * CITATION_REPEAT_MAX_MULTIPLIER
    );
}

#[test]
fn test_priors_climb_monotonically_and_then_hold() {
    let escalating: Vec<f64> = (0..12)
        .map(|n| speeding_citation_fine(16.0, n, false))
        .collect();
    let mut sorted = escalating.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    assert_eq!(escalating, sorted); // never cheaper for more priors
    let distinct: std::collections::BTreeSet<u64> =
        escalating.iter().map(|f| f.to_bits()).collect();
    assert!(distinct.len() > 1); // and it really does climb before holding

    // The same rule governs non-speeding citations.
    assert_eq!(
        citation_fine(900.0, 30, false, None),
        citation_fine(900.0, 5, false, None)
    );
    assert!(citation_fine(900.0, 2, false, None) > citation_fine(900.0, 0, false, None));
}

#[test]
fn test_no_single_citation_can_repossess_a_truck_on_its_own() {
    // The binding balance constraint, not a statutory one: the worst case the
    // game can actually produce -- the top speeding step, a habitual
    // offender, inside a construction zone -- stays under REPOSSESSION_FLOOR.
    let worst = speeding_citation_fine(99.0, 999, true);
    assert!(worst < REPOSSESSION_FLOOR);
    let flat = [
        UNSAFE_DAMAGE_FINE,
        WEIGH_STATION_BYPASS_FINE,
        FAILURE_TO_STOP_CITATION_FINE,
        WORK_ZONE_BARRELS_FINE,
    ]
    .into_iter()
    .map(|base| citation_fine(base, 999, true, None))
    .fold(f64::MIN, f64::max);
    assert!(flat < REPOSSESSION_FLOOR);
}

#[test]
fn test_the_construction_zone_doubling_survives_the_repeat_cap() {
    // Capping the step, not the total -- so the spoken line stays true.
    for priors in [0, 1, 2, 5, 40] {
        let plain = citation_fine(1_800.0, priors, false, None);
        let zoned = citation_fine(1_800.0, priors, true, None);
        assert_eq!(zoned, round_py_n(plain * 2.0, 2));
    }
}

#[test]
fn test_every_fine_is_anchored_to_the_real_penalty_it_names() {
    // The amounts themselves, so a rebalance is a deliberate edit here too.
    assert_eq!(UNSAFE_DAMAGE_FINE, 2300.0); // FMCSA unsafe conditions: 2,304
    assert_eq!(WEIGH_STATION_BYPASS_FINE, 1800.0); // CA/NY pass 1,000 on a first offense
    assert_eq!(CHAIN_LAW_FINE, 580.0); // UNVERIFIED placeholder outside CO, OR and WY
    assert_eq!(chain_law_fine(Some("CO")), 579.0);
    assert_eq!(chain_law_fine(Some("WY")), 250.0);
    assert_eq!(CHAIN_CLOSURE_FINE_WY, 750.0);
    assert_eq!(chain_law_fine(Some("OR")), 880.0);
    assert_eq!(chain_law_fine(Some("CA")), CHAIN_LAW_FINE);
    assert_eq!(chain_law_fine(None), CHAIN_LAW_FINE);
    assert_eq!(CHAIN_BLOCKING_FINE_CO, 1157.0);
    assert_eq!(FOLLOWING_TOO_CLOSE_FINE, 600.0); // a 383.51 Table 2 serious violation
    assert_eq!(LANE_MISUSE_FINE, 500.0); // Virginia's highway safety corridor figure
    assert_eq!(LIGHTS_FINE, 350.0);
    assert_eq!(FAILURE_TO_STOP_CITATION_FINE, 1500.0);
    assert_eq!(FAILURE_TO_STOP_FINE, 5000.0); // already a felony-sized number
    assert_eq!(WORK_ZONE_BARRELS_FINE, 1000.0); // Missouri RSMo 304.585, top of range
    assert_eq!(SPEEDING_FINE_STEPS[0].1, 250.0);
    assert_eq!(SPEEDING_FINE_STEPS[SPEEDING_FINE_STEPS.len() - 1].1, 2500.0);
}

#[test]
fn test_the_shoulder_parking_ticket_moved_with_the_rest() {
    assert_eq!(crate::sim::hos::SHOULDER_FINE, 400.0);
}

#[test]
fn test_a_fine_earned_in_a_construction_zone_is_doubled() {
    assert_eq!(CONSTRUCTION_ZONE_FINE_MULTIPLIER, 2.0);
    let plain = citation_fine(WEIGH_STATION_BYPASS_FINE, 0, false, None);
    let in_zone = citation_fine(WEIGH_STATION_BYPASS_FINE, 0, true, None);
    assert_eq!(plain, 1_800.0);
    assert_eq!(in_zone, 3_600.0);
    // Speeding doubles the same way; it is not exempt for going through its
    // own schedule first.
    assert_eq!(
        speeding_citation_fine(16.0, 0, true),
        speeding_citation_fine(16.0, 0, false) * 2.0
    );
}

#[test]
fn test_priors_and_the_construction_zone_compound_rather_than_add() {
    // The owner's explicit call: 1,800 x 1.5 x 2 on a second offense.
    let second_offense_in_zone = citation_fine(WEIGH_STATION_BYPASS_FINE, 1, true, None);
    assert_eq!(second_offense_in_zone, 5_400.0);
    // Adding the two multipliers instead of compounding them would give 2.5x.
    assert_ne!(second_offense_in_zone, WEIGH_STATION_BYPASS_FINE * 2.5);
}

#[test]
fn test_a_missing_driving_record_reads_as_a_first_offender() {
    let mut bare = profile();
    bare.driving_record = None;
    assert_eq!(career_citations(&bare), 0);
    let mut three = profile();
    three.record_mut().citations = 3;
    assert_eq!(career_citations(&three), 3);
}

#[test]
fn test_the_doubled_fine_says_why_in_plain_player_language() {
    assert_eq!(construction_zone_fine_clause(false), "");
    let clause = construction_zone_fine_clause(true);
    assert!(clause.contains("doubled"));
    // The canonical spoken noun from docs/ontology.md, never "work zone".
    assert!(clause.contains("construction zone"));
    assert!(!clause.contains("work zone"));
}

#[test]
fn a_statutory_ceiling_is_the_last_word() {
    assert_eq!(citation_fine(1_800.0, 5, true, Some(2_000.0)), 2_000.0);
    assert_eq!(citation_fine(1_800.0, 0, false, Some(2_000.0)), 1_800.0);
    assert!(is_serious_speed(15.0));
    assert!(!is_serious_speed(14.9));
}

// --- the serious-violation ladder ------------------------------------------

#[test]
fn test_first_serious_violation_does_not_suspend() {
    let mut r = record();
    assert_eq!(r.record_serious_violation(100.0), 1);
    assert!(!r.suspended(100.0));
}

#[test]
fn test_second_serious_violation_suspends_the_cdl_for_sixty_days() {
    let mut r = record();
    r.record_serious_violation(100.0);
    assert_eq!(r.record_serious_violation(200.0), 2);
    assert!(r.suspended(200.0));
    assert_eq!(
        round_py_int(r.days_left(200.0)),
        SERIOUS_SECOND_SUSPENSION_DAYS
    );
}

#[test]
fn test_third_serious_violation_suspends_for_a_hundred_and_twenty_days() {
    let mut r = record();
    for at in [100.0, 200.0] {
        r.record_serious_violation(at);
    }
    // Serve the 60 days out, then earn a third.
    let later = r.suspended_until_h + 1.0;
    assert_eq!(r.record_serious_violation(later), 3);
    assert_eq!(
        round_py_int(r.days_left(later)),
        SERIOUS_THIRD_SUSPENSION_DAYS
    );
}

#[test]
fn test_violations_older_than_three_years_stop_counting() {
    let mut r = record();
    r.record_serious_violation(0.0);
    let much_later = (SERIOUS_WINDOW_DAYS + 10) as f64 * DAY;
    assert_eq!(r.serious_in_window(much_later), 0);
    // So the next one is a first offense again, not a suspension.
    assert_eq!(r.record_serious_violation(much_later), 1);
    assert!(!r.suspended(much_later));
}

// --- major offenses: Jerry's case ------------------------------------------

#[test]
fn test_first_major_offense_disqualifies_the_cdl_for_a_year() {
    let mut r = record();
    assert_eq!(r.record_major_offense(50.0), SUSPENSION_MAJOR);
    assert!(r.suspended(50.0));
    assert_eq!(
        round_py_int(r.days_left(50.0)),
        MAJOR_FIRST_DISQUALIFICATION_DAYS
    );
    assert!(!r.lifetime_disqualified);
}

#[test]
fn test_second_major_offense_is_a_lifetime_disqualification() {
    let mut r = record();
    r.record_major_offense(50.0);
    assert_eq!(r.record_major_offense(60.0), SUSPENSION_LIFETIME);
    assert!(r.lifetime_disqualified);
    // There is no date this clears, at any point on the career clock.
    assert!(r.suspended(999_999.0));
    assert_eq!(r.days_left(999_999.0), f64::INFINITY);
}

#[test]
fn test_a_served_suspension_gives_the_licence_back() {
    let mut r = record();
    r.record_major_offense(50.0);
    let cleared = r.suspended_until_h + 1.0;
    assert!(!r.suspended(cleared));
    r.serve_until(cleared);
    assert_eq!(r.suspended_until_h, 0.0);
    assert_eq!(r.suspension_reason, "");
}

// --- fatigue (49 CFR 392.3) -------------------------------------------------

#[test]
fn test_first_run_off_road_asleep_costs_standing_not_the_licence() {
    let mut r = record();
    let (count, serious) = r.record_fatigue_event(100.0);
    assert_eq!((count, serious), (1, 0));
    assert!(!r.suspended(100.0));
}

#[test]
fn test_second_run_off_road_asleep_is_a_serious_violation() {
    let mut r = record();
    r.record_fatigue_event(100.0);
    let (count, serious) = r.record_fatigue_event(200.0);
    assert_eq!(count, 2);
    assert_eq!(serious, 1);
    assert_eq!(r.serious_in_window(200.0), 1);
}

// --- dispatch trust ---------------------------------------------------------

#[test]
fn test_a_clean_driver_sees_the_board_they_have_always_seen() {
    assert_eq!(board_offers_for_reputation(6, 50.0), 6);
    assert_eq!(board_offers_for_reputation(6, 100.0), 6);
    assert_eq!(trust_band(50.0), TRUST_FULL); // where every new career starts
}

#[test]
fn test_trust_band_follows_the_rounded_score_the_player_hears() {
    assert_eq!(trust_band(39.4), TRUST_GUARDED);
    assert_eq!(trust_band(39.5), TRUST_FULL);
    assert_eq!(trust_band(39.7), TRUST_FULL);
    assert_eq!(trust_band(27.4), TRUST_POOR);
    assert_eq!(trust_band(27.7), TRUST_GUARDED);
    assert_eq!(trust_band(15.4), TRUST_LAST_CHANCE);
    assert_eq!(trust_band(15.7), TRUST_POOR);
    for rep in [39.4, 39.5, 39.7, 27.7, 15.7] {
        let word = reputation_band_word(rep);
        let band = trust_band(rep);
        // Band words and trust bands share their edges at 40, 28 and 16.
        let expected = match word {
            "Unproven" | "Established" | "Well regarded" | "Outstanding" => TRUST_FULL,
            "Shaky" => TRUST_GUARDED,
            "Damaged" => TRUST_POOR,
            _ => TRUST_LAST_CHANCE,
        };
        assert_eq!(band, expected, "reputation {rep}");
    }
}

#[test]
fn test_dispatch_trust_slides_the_whole_way_down() {
    let steps: Vec<i64> = [50.0, 40.0, 25.0, 5.0]
        .into_iter()
        .map(|rep| board_offers_for_reputation(6, rep))
        .collect();
    let mut sorted = steps.clone();
    sorted.sort_by(|a, b| b.cmp(a));
    assert_eq!(steps, sorted);
    assert!(steps[0] > steps[steps.len() - 1]);
    assert_eq!(steps[steps.len() - 1], 1);
}

/// A senior driver past the load-choice level goes back to assigned loads at
/// reputation 18 and loses every refusal at 10.
#[test]
fn test_losing_trust_takes_back_the_right_to_pick_your_own_loads() {
    use crate::models::dispatch_policy::{dispatch_policy, SENIOR_LOAD_CHOICE_LEVEL};
    use crate::models::profile::Profile;

    let mut p = Profile::named("Senior");
    p.career.xp = 200_000.0; // comfortably past the load-choice level
    assert!(p.career.level() >= SENIOR_LOAD_CHOICE_LEVEL);
    assert!(!dispatch_policy(&p).assigns_load);
    p.career.reputation = 18.0;
    // back to taking what dispatch gives
    assert!(dispatch_policy(&p).assigns_load);
    p.career.reputation = 10.0;
    // and no refusals left at all
    assert_eq!(dispatch_policy(&p).decline_budget, 0);
}

#[test]
fn the_trust_ladder_matches_the_python_tables() {
    assert!(trust_revokes_load_choice(20.0));
    assert!(trust_revokes_load_choice(5.0));
    assert!(!trust_revokes_load_choice(30.0));
    assert_eq!(trust_decline_penalty(50.0), 0);
    assert_eq!(trust_decline_penalty(30.0), 1);
    assert_eq!(trust_decline_penalty(20.0), 2);
    assert_eq!(trust_decline_penalty(10.0), 99);
    assert_eq!(board_offers_for_reputation(4, 30.0), 3);
    assert_eq!(
        trust_text(50.0),
        "Dispatch trust: full. You get the whole board."
    );
    assert_eq!(
        trust_text(30.0),
        "Dispatch trust: guarded. Dispatch is holding back some of the freight and \
         allows fewer refusals. Clean on-time runs rebuild it."
    );
    assert_eq!(board_reputation_note(50.0), "");
    assert_eq!(board_reputation_note(20.0), trust_text(20.0));
    assert_eq!(
        worst_band(&[TRUST_FULL, TRUST_POOR, TRUST_GUARDED]),
        TRUST_POOR
    );
    assert_eq!(worst_band(&[TRUST_FULL]), TRUST_FULL);
}

#[test]
fn reputation_words_round_before_banding_and_never_overlap_trust_words() {
    for (score, word) in [
        (-1.0, "Ruined"),
        (15.49, "Ruined"),
        (15.5, "Damaged"),
        (27.49, "Damaged"),
        (27.5, "Shaky"),
        (39.49, "Shaky"),
        (39.5, "Unproven"),
        (50.0, "Unproven"),
        (59.49, "Unproven"),
        (59.5, "Established"),
        (74.5, "Established"),
        (74.6, "Well regarded"),
        (89.49, "Well regarded"),
        (89.6, "Outstanding"),
        (101.0, "Outstanding"),
    ] {
        assert_eq!(reputation_band_word(score), word);
    }
    for word in [
        "Outstanding",
        "Well regarded",
        "Established",
        "Unproven",
        "Shaky",
        "Damaged",
        "Ruined",
    ] {
        assert!(![TRUST_FULL, TRUST_GUARDED, TRUST_POOR, TRUST_LAST_CHANCE].contains(&word));
    }
}

#[test]
fn score_display_formats_reputation_and_trust_honestly() {
    assert_eq!(
        reputation_text(62.0, ScoreDisplay::Both),
        "Established. 62 out of 100."
    );
    assert_eq!(
        reputation_text(62.0, ScoreDisplay::Percent),
        "62 out of 100."
    );
    assert_eq!(
        reputation_text(62.0, ScoreDisplay::Description),
        "Established."
    );
    let mut service = profile();
    service.reputation = 85.0;
    assert!(
        dispatch_trust_line_with_display(&service, ScoreDisplay::Both)
            .starts_with("Dispatch trust: full. Reputation 85 out of 100.")
    );
    assert!(
        dispatch_trust_line_with_display(&service, ScoreDisplay::Percent)
            .starts_with("Dispatch trust: full. Reputation 85 out of 100.")
    );
    assert_eq!(
        trust_text_with_display(85.0, ScoreDisplay::Percent),
        trust_text_with_display(85.0, ScoreDisplay::Both)
    );
    assert!(
        dispatch_trust_line_with_display(&service, ScoreDisplay::Description)
            .starts_with("Dispatch trust: full.")
    );
    let mut debt = service;
    debt.money = -5_000.0;
    for mode in [
        ScoreDisplay::Both,
        ScoreDisplay::Percent,
        ScoreDisplay::Description,
    ] {
        assert!(!dispatch_trust_line_with_display(&debt, mode).contains("out of 100"));
    }
}

// --- persistence and retroactivity -----------------------------------------

#[test]
fn test_the_record_survives_save_and_load() {
    // The record half: the DrivingRecord's save shape round-trips. The
    // Profile.to_dict / from_dict path needs models::profile.
    let mut r = record();
    let game_hours = 500.0;
    r.record_major_offense(game_hours);
    r.record_citation(1_000.0);
    let text = serde_json::to_string(&r).unwrap();
    let restored: DrivingRecord = serde_json::from_str(&text).unwrap();
    assert_eq!(restored.major_count(), 1);
    assert_eq!(restored.citations, 1);
    assert!(restored.suspended(game_hours));
    assert_eq!(restored, r);
    // A save from before a field existed still loads, with the default.
    let partial: DrivingRecord = serde_json::from_str(r#"{"citations": 2}"#).unwrap();
    assert_eq!(partial.citations, 2);
    assert!(partial.serious_violations.is_empty());
    assert!(!partial.lifetime_disqualified);
}

#[test]
fn test_a_legacy_save_counts_the_felonies_it_still_holds() {
    // Exactly what the old build wrote: the felony count lived on the trip.
    let data = json!({
        "active_trip": {"failure_to_stop_count": 2, "speeding_tickets": 1},
        "game_hours": 400.0,
    });
    let record = seed_record_from_save(data.as_object().unwrap());
    assert_eq!(record.major_count(), 2);
    assert!(record.lifetime_disqualified); // no amnesty
    assert!(record.notice_pending);
    assert_eq!(record.citations, 1);
}

#[test]
fn test_a_clean_legacy_save_gets_a_clean_record_and_no_notice() {
    let data = json!({"name": "Clean", "game_hours": 6.0, "career": {"reputation": 50.0}});
    let record = seed_record_from_save(data.as_object().unwrap());
    assert!(record.clean(6.0));
    assert!(!record.notice_pending);
    // A low reputation alone earns the one-time explanation.
    let low = json!({"game_hours": 6.0, "career": {"reputation": 20.0}});
    assert!(seed_record_from_save(low.as_object().unwrap()).notice_pending);
    // No career block at all reads as the neutral 50.
    let none = json!({"game_hours": 6.0});
    assert!(!seed_record_from_save(none.as_object().unwrap()).notice_pending);
}

// --- spoken surfaces --------------------------------------------------------

#[test]
fn test_standing_is_said_plainly_and_names_the_next_consequence() {
    let mut p = profile();
    p.game_hours = 100.0;
    assert_eq!(standing_text(&p), "Record: clean.");
    p.record_mut().record_serious_violation(100.0);
    let text = standing_text(&p);
    assert!(text.contains("one serious violation"));
    assert!(text.contains("60 days"));
    assert!(!text.to_lowercase().contains("strike")); // that noun belongs to the per-trip counter
                                                      // The license half is unchanged; the carrier's review now speaks after it.
    assert!(
        text.starts_with(
            "Record: one serious violation. One more before your CDL is suspended for 60 days. "
        ),
        "{text}"
    );
    assert!(
        text.ends_with("One more serious violation and the carrier lets you go."),
        "{text}"
    );
}

#[test]
fn test_a_suspended_record_says_when_it_clears() {
    let mut p = profile();
    p.game_hours = 100.0;
    // The ladder suspends; a major offense disqualifies. Two nouns, two things.
    p.record_mut().record_serious_violation(100.0);
    p.record_mut().record_serious_violation(100.0);
    assert!(career_menu_status(&p).contains("suspended"));
    assert!(standing_text(&p).contains("60 days"));

    let mut q = profile();
    q.game_hours = 100.0;
    q.record_mut().record_major_offense(100.0);
    assert!(career_menu_status(&q).contains("disqualified"));
    let text = standing_text(&q);
    assert!(text.contains("365 days"));
    assert!(text.contains("clears"));
    assert!(!text.contains("hours")); // served in game days, never raw hours
}

// `test_the_lifetime_line_states_the_facts_and_the_way_forward` is live in
// `crates/freight-fate/tests/states_driving_enforcement_record.rs`: the text it
// reads comes from `states::driving_rest_states::major_offense_text`.

#[test]
fn the_spoken_standing_lines_match_the_python_f_strings() {
    let mut p = profile();
    p.game_hours = 100.0;
    p.record_mut().record_major_offense(100.0);
    // 100 h + 365 days lands on a weekday and date from the career calendar.
    let clears = clears_text(&p);
    assert!(clears.contains(", "));
    assert_eq!(
        standing_text(&p),
        format!("Record: CDL disqualified, 365 days remaining. It clears {clears}.")
    );
    assert_eq!(
        career_menu_status(&p),
        "CDL: disqualified, 365 days remaining"
    );
    assert_eq!(
        suspension_board_line(&p),
        format!("Dispatch board. Your CDL is disqualified. Driving jobs return {clears}.")
    );
    assert_eq!(
        suspension_refusal_line(&p),
        format!("You cannot take this job while your CDL is disqualified. It clears {clears}.")
    );
    // A calendar offset moves the spoken date.
    p.calendar_offset_days = 3.0;
    assert_ne!(clears_text(&p), clears);

    p.record_mut().record_major_offense(200.0);
    assert_eq!(clears_text(&p), "");
    assert_eq!(
        standing_text(&p),
        "Record: your CDL is disqualified for life. You cannot take driving work."
    );
    assert_eq!(career_menu_status(&p), "CDL: disqualified for life");
    assert_eq!(
        suspension_board_line(&p),
        "Dispatch board. Your CDL is disqualified for life, so there is no driving work \
         here. The board is listed for reference only."
    );
    assert_eq!(
        suspension_refusal_line(&p),
        "You cannot take driving work with a lifetime CDL disqualification."
    );

    let mut clean = profile();
    clean.game_hours = 100.0;
    assert_eq!(career_menu_status(&clean), "CDL: clear");
    // Served violations and a major offense, off suspension.
    clean.record_mut().major_offenses.push(0.0);
    clean.record_mut().serious_violations.push(0.0);
    clean.record_mut().serious_violations.push(0.0);
    let text = standing_text(&clean);
    assert!(
        text.starts_with(
            "Record: two serious violations, one major offense. One more major offense \
             disqualifies your CDL for life."
        ),
        "{text}"
    );
    // Two serious violations in the window is also the carrier's termination
    // floor, and the line says so after the license half.
    assert!(
        text.ends_with("The carrier ends your employment at the next terminal."),
        "{text}"
    );
}

/// Reported 2026-09-29: with live weather driving the calendar, a suspension
/// said it cleared on a day already gone. The date was counted on the
/// career's own calendar while every other date the player heard was real.
#[test]
fn the_clear_date_counts_from_the_calendar_the_player_hears() {
    use crate::sim::season::{date_text, real_clock_game_hours, real_weekday_name};
    let mut p = real_profile();
    p.game_hours = 2000.0; // a career calendar in early June
    p.driving_record.record_serious_violation(p.game_hours);
    p.driving_record.record_serious_violation(p.game_hours);
    p.live_calendar = true;
    let cleared = real_clock_game_hours(None) + 60.0 * DAY;
    // The weekday is the real one: the live clock keeps the day of the year,
    // not 2001's weekdays (seasonal audit, 2026-10-01).
    let expected = format!(
        "{}, {}",
        real_weekday_name(cleared, None),
        date_text(cleared)
    );
    let clears = clears_text(&p);
    assert!(clears.starts_with(&expected), "{clears} vs {expected}");

    // A year-long disqualification lands on today's own date; the year is
    // what tells the two apart.
    let mut q = profile();
    q.game_hours = 100.0;
    q.record_mut().record_major_offense(100.0);
    let today = date_text(q.calendar_now_hours());
    let clears = clears_text(&q);
    assert!(clears.contains(&today), "{clears}");
    assert!(clears.ends_with(", next year"), "{clears}");
}

#[test]
fn count_and_ordinal_words_and_days_text() {
    assert_eq!(count_word(0), "no");
    assert_eq!(count_word(9), "nine");
    assert_eq!(count_word(10), "10");
    assert_eq!(ordinal_word(1), "first");
    assert_eq!(ordinal_word(7), "seventh");
    assert_eq!(ordinal_word(0), "0th");
    assert_eq!(ordinal_word(8), "8th");
    assert_eq!(days_text(0.2), "1 day");
    assert_eq!(days_text(1.4), "1 day");
    assert_eq!(days_text(2.5), "2 days"); // round half to even, like Python
    assert_eq!(days_text(59.6), "60 days");
}

// --- the whole picture (tests/test_debt_and_standing.py) -------------------

#[test]
fn test_debt_alone_holds_the_iron_back_even_with_a_spotless_record() {
    // The standing half; equipment_held_back / assigned_fleet_tier need
    // models::carrier_fleet.
    let mut p = profile();
    p.reputation = 100.0;
    p.money = -5_000.0; // square on service, deep on money
    assert_eq!(trust_band(p.reputation), TRUST_FULL);
    assert_ne!(standing_band(&p), TRUST_FULL);
    assert_eq!(standing_cause(&p), CAUSE_DEBT);
}

#[test]
fn test_a_suspended_licence_holds_the_seat_but_an_old_violation_does_not() {
    // The LICENCE band only moves on a suspension. A lone serious violation
    // reaches the band another way now -- the carrier's record review, which
    // holds at guarded and names its own end date (owner, 2026-09-12) -- and
    // the license band stays out of it until the second one suspends the CDL.
    let mut p = profile();
    let hours = p.game_hours;
    p.record_mut().record_serious_violation(hours);
    assert!(!p.record().suspended(hours));
    assert_eq!(licence_band(&p), TRUST_FULL);
    assert_eq!(standing_band(&p), TRUST_GUARDED);
    assert_eq!(standing_cause(&p), CAUSE_RECORD);

    p.record_mut().record_serious_violation(hours); // second: suspended
    assert!(p.record().suspended(hours));
    assert_eq!(standing_band(&p), TRUST_LAST_CHANCE);
    assert_eq!(standing_cause(&p), CAUSE_LICENCE);
    assert!(standing_way_back(&p).contains("clears"));
}

#[test]
fn test_the_way_back_names_the_cause_rather_than_promising_clean_runs() {
    // "Clean on-time runs rebuild it" is a lie to a driver whose debt is the problem.
    let mut p = profile();
    p.money = -5_000.0;
    let way_back = standing_way_back(&p);
    assert!(way_back.contains("owe") && way_back.contains("Paying it down"));
    assert_ne!(way_back, "Clean on-time runs rebuild it.");
    assert_eq!(
        way_back,
        "You owe 5,000 dollars, and that is what is holding it. Paying it down brings it back."
    );

    let mut q = profile();
    q.reputation = 20.0;
    assert_eq!(standing_way_back(&q), "Clean on-time runs rebuild it.");

    // Debt and poor service together name both.
    let mut both = profile();
    both.money = -5_000.0;
    both.reputation = 20.0;
    assert_eq!(standing_cause(&both), CAUSE_DEBT);
    assert!(standing_way_back(&both).ends_with("and clean on-time runs help too."));

    // The whole line, with the trust clause and the slower-experience clause.
    let line = dispatch_trust_line(&both);
    assert!(line.starts_with(trust_band_text(standing_band(&both))));
    assert!(line.contains("While dispatch trust is"));
    assert_eq!(
        dispatch_trust_line(&profile()),
        "Dispatch trust: full. Reputation 50 out of 100. You get the whole board."
    );
    // A hold clause from the fleet rides between the way back and the rate.
    let mut held = profile();
    held.money = -5_000.0;
    held.hold_clause = "The yard is holding your truck.".to_string();
    assert!(dispatch_trust_line(&held)
        .contains("The yard is holding your truck. While dispatch trust is"));
}

#[test]
fn a_low_reputation_company_driver_is_due_for_termination() {
    let mut p = profile();
    p.reputation = 4.0;
    assert!(carrier_termination_due(&p));
    p.carrier_key = LAST_CHANCE_CARRIER_KEY.to_string();
    assert!(!carrier_termination_due(&p)); // nowhere further down
    let mut owner = profile();
    owner.reputation = 4.0;
    owner.business_status = crate::models::business_constants::LEASED_OWNER_OPERATOR.to_string();
    assert!(!carrier_termination_due(&owner));
}

#[test]
fn test_the_yard_hands_over_the_lower_of_level_and_standing() {
    use crate::models::carrier_fleet::{assigned_fleet_tier, eligible_fleet_tier, FLEET_TIERS};
    use crate::models::enforcement::standing_band;
    use crate::models::profile::Profile;
    use std::collections::BTreeMap;

    let mut p = real_profile();
    p.career.xp = 387_000.0; // top of the ladder: eligible for the best iron
    assert_eq!(eligible_fleet_tier(&p).key, "first_pick");

    // Each band caps the assignment further down, and never above eligibility.
    let mut seen: BTreeMap<&str, &str> = BTreeMap::new();
    for (reputation, expected) in [
        (100.0, "first_pick"),
        (30.0, "long_haul"),     // guarded
        (20.0, "regional"),      // poor
        (10.0, "yard_standard"), // last chance
    ] {
        p.career.reputation = reputation;
        seen.insert(standing_band(&p), assigned_fleet_tier(&p).key);
        assert_eq!(assigned_fleet_tier(&p).key, expected);
    }
    assert_eq!(seen.len(), 4); // all four bands exercised

    // A junior driver is never given *more* than their level earns by standing.
    let mut junior = Profile::named("Dale");
    junior.current_city = "Buffalo".to_string();
    junior.career.xp = 0.0;
    junior.career.reputation = 100.0;
    assert_eq!(assigned_fleet_tier(&junior).key, FLEET_TIERS[0].key);
}

/// The most frequent moment in the change, and the easiest to read as a bug.
#[test]
fn test_a_held_back_truck_names_the_earned_tier_the_cause_and_the_way_out() {
    use crate::models::carrier_fleet::equipment_hold_text;

    let mut p = real_profile();
    p.career.xp = 387_000.0;
    p.set_money(-5_000.0);
    let verbose = equipment_hold_text(&p, false);
    let terse = equipment_hold_text(&p, true);
    for text in [&verbose, &terse] {
        // what the level earns
        assert!(text.contains("first pick of the yard"), "{text}");
        // the cause, in money
        assert!(text.contains("5,000 dollars"), "{text}");
    }
    // and exactly what gives it back
    assert!(verbose.contains("Clear it"));
    assert!(terse.len() < verbose.len());
}

#[test]
fn test_experience_slows_by_band_and_never_reaches_zero() {
    use crate::models::career::{standing_xp_rate, STANDING_XP_RATE};
    use std::collections::BTreeSet;

    let bands = [TRUST_FULL, TRUST_GUARDED, TRUST_POOR, TRUST_LAST_CHANCE];
    // The bands the code keys on are exactly the bands enforcement defines.
    let keyed: BTreeSet<&str> = STANDING_XP_RATE.iter().map(|(band, _)| *band).collect();
    assert_eq!(keyed, bands.iter().copied().collect::<BTreeSet<&str>>());
    let rates: Vec<f64> = bands.iter().map(|band| standing_xp_rate(band)).collect();
    assert_eq!(rates[0], 1.0); // a clean driver's arc does not move
    let mut descending = rates.clone();
    descending.sort_by(|a, b| b.partial_cmp(a).unwrap());
    assert_eq!(rates, descending);
    assert!(rates.iter().all(|rate| *rate > 0.0 && *rate <= 1.0));
    // a driver digging out still makes real progress
    assert!(rates[rates.len() - 1] >= 0.5);
}

#[test]
fn test_a_slowed_career_still_levels_up() {
    use crate::models::career::{standing_xp_rate, Career};

    let mut slowed = Career::default();
    let rate = standing_xp_rate("last chance");
    for _ in 0..30 {
        slowed.record_delivery(500.0, 800.0, true, 0.0, 1.0, rate);
    }
    assert!(slowed.level() > 1);
}

#[test]
fn test_the_slowdown_is_said_in_words_and_never_as_a_number() {
    use crate::models::career::{xp_rate_clause, xp_rate_settlement_clause};

    assert_eq!(xp_rate_clause("full"), "");
    assert_eq!(xp_rate_settlement_clause("full"), "");
    for band in ["guarded", "poor", "last chance"] {
        let sentence = xp_rate_clause(band);
        let clause = xp_rate_settlement_clause(band);
        assert!(sentence.contains(band) && sentence.contains("comes slower"));
        assert!(clause.contains(band) && clause.contains("slower rate"));
        for text in [&sentence, &clause] {
            let scrubbed = text.replace("experience", "e");
            for banned in ["percent", "multiplier", "penalty", "x", "0."] {
                assert!(!scrubbed.contains(banned), "{banned:?} in {text:?}");
            }
        }
    }
}

#[test]
fn test_slowed_experience_stays_under_the_exported_cloud_ceiling() {
    use crate::models::career::{
        standing_xp_rate, Career, DELIVERY_COMPLETION_XP, XP_CLEAN_BONUS, XP_PER_MILE_ON_TIME,
        XP_SPECIALTY_MULT, XP_STREAK_MAX_BONUS,
    };

    // The two exported ceiling terms, from the same arithmetic the export
    // uses (`profile_integrity_invariants`); building the whole export here
    // would need the world data tree this module never touches.
    let best_case = (1.0 + XP_STREAK_MAX_BONUS) * (1.0 + XP_CLEAN_BONUS);
    let xp_per_mile_max = XP_PER_MILE_ON_TIME * XP_SPECIALTY_MULT * best_case;
    let xp_flat_per_delivery = DELIVERY_COMPLETION_XP * XP_SPECIALTY_MULT * best_case;

    let mut career = Career::default();
    for _ in 0..25 {
        career.record_delivery(700.0, 0.0, true, 0.0, 1.5, standing_xp_rate("guarded"));
    }
    let ceiling =
        career.deliveries as f64 * xp_flat_per_delivery + career.total_miles * xp_per_mile_max;
    assert!(career.xp <= ceiling);
}

// --- the road, the stops, the board: app-shell cases ------------------------
//
// Every case in this section of `tests/test_enforcement_record.py` needs a
// real `DrivingState` and the screens it pushes, so it lives in the game
// crate: `crates/freight-fate/tests/states_driving_enforcement_record.rs` for
// the roadside stops, the fatigue events, the pursuit opt-in and the two
// reload exploits, and `crates/freight-fate/tests/states_city.rs` for the
// dispatch board and the terminal while suspended
// (`test_a_clean_driver_hears_and_pays_nothing_new` and the four below).

#[test]
fn test_a_fine_a_load_cannot_cover_stays_owed_and_is_said_so() {
    use crate::models::business::build_business_settlement_basic;
    use crate::models::jobs::{cargo_type, Job};
    let job = Job::new(
        cargo_type("general").unwrap(),
        5.0,
        "Buffalo",
        "yard",
        "Rochester",
        40.0,
        120.0,
        8.0,
    );
    let settled = build_business_settlement_basic("company_driver", &job, 120.0, true, 2_000.0);
    assert_eq!(settled.net_before_advance, 0.0);
    // The shortfall is owed, not quietly forgiven.
    assert!(settled.uncollected_charges > 0.0);
    assert!(settled.uncollected_charges < 2_000.0);
}

// `test_the_board_says_the_suspension_before_it_lists_anything` is live in `crates/freight-fate/tests/states_city.rs`.

// `test_taking_a_job_while_suspended_is_refused_with_the_clear_date` is live in `crates/freight-fate/tests/states_city.rs`.

// `test_waiting_out_the_suspension_gives_the_licence_back` is live in `crates/freight-fate/tests/states_city.rs`.

// `test_a_floor_reputation_company_driver_loses_the_carrier` is live in `crates/freight-fate/tests/states_city.rs`.

// --- the carrier's record review and the insurer's surcharge ----------------
//
// Owner ask, 2026-09-12: citations and serious violations "did not seem to
// mean anything". They scaled fines and scale odds, silently. Now the carrier
// reviews the window (49 CFR 391.25), the insurer surcharges it, and both are
// said.

fn cite(p: &mut FakeProfile, times: i64, at: f64) {
    for _ in 0..times {
        p.record_mut().record_citation_at(200.0, at);
    }
}

#[test]
fn a_record_over_the_review_floor_holds_a_company_driver_at_guarded() {
    let mut p = profile();
    p.game_hours = 400.0 * DAY;
    cite(&mut p, 3, 300.0 * DAY);
    // Three is the floor, not over it.
    assert_eq!(record_band(&p), TRUST_FULL);
    assert_eq!(standing_band(&p), TRUST_FULL);
    cite(&mut p, 1, 350.0 * DAY);
    assert_eq!(record_band(&p), TRUST_GUARDED);
    assert_eq!(standing_band(&p), TRUST_GUARDED);
    assert_eq!(standing_cause(&p), CAUSE_RECORD);
    let way_back = standing_way_back(&p);
    assert!(
        way_back.contains("Your driving record is what is holding it"),
        "{way_back}"
    );
    assert!(
        way_back.contains("four citations in the last year"),
        "{way_back}"
    );
    assert!(way_back.contains("keeps it there until"), "{way_back}");
    // The consequence line counts down to the termination floor.
    let said = record_consequence_text(&p);
    assert!(said.contains("holds your equipment back"), "{said}");
    assert!(
        said.contains("Two more citations and the carrier lets you go."),
        "{said}"
    );
    assert!(!carrier_termination_due(&p));
}

#[test]
fn one_serious_violation_in_the_window_is_over_the_review_floor() {
    let mut p = profile();
    p.game_hours = 400.0 * DAY;
    p.record_mut().record_serious_violation(390.0 * DAY);
    assert_eq!(record_band(&p), TRUST_GUARDED);
    assert_eq!(standing_cause(&p), CAUSE_RECORD);
    assert!(record_consequence_text(&p)
        .contains("One more serious violation and the carrier lets you go."));
}

#[test]
fn six_citations_in_three_years_end_the_employment() {
    let mut p = profile();
    p.game_hours = 400.0 * DAY;
    cite(&mut p, 6, 380.0 * DAY);
    assert_eq!(record_band(&p), TRUST_POOR);
    assert!(carrier_termination_due(&p));
    assert!(record_consequence_text(&p).contains("ends your employment"));
    // At the fleet of last resort there is nowhere further down: kept on
    // sufferance, and told so.
    p.carrier_key = LAST_CHANCE_CARRIER_KEY.to_string();
    assert!(!carrier_termination_due(&p));
    assert_eq!(record_band(&p), TRUST_POOR);
    assert!(record_consequence_text(&p).contains("sufferance"));
}

#[test]
fn the_window_empties_and_the_hold_lets_go() {
    let mut p = profile();
    p.game_hours = 400.0 * DAY;
    cite(&mut p, 4, 380.0 * DAY);
    assert_eq!(record_band(&p), TRUST_GUARDED);
    let ages_out = p
        .driving_record()
        .unwrap()
        .window_clears_at(p.game_hours, |c, s| c + s > 0)
        .expect("something is in the window");
    assert!((ages_out - (380.0 + REVIEW_WINDOW_DAYS as f64) * DAY).abs() < 1e-6);
    p.game_hours = ages_out + 1.0;
    assert_eq!(record_band(&p), TRUST_FULL);
    assert_eq!(standing_text(&p), "Record: clean.");
}

/// The hold lifts when the record is back under the floor, not when its
/// oldest entry leaves: a citation on day 10 and a serious violation on day
/// 100 hold the review until the serious one ages out (2026-09-28).
#[test]
fn the_hold_lifts_when_the_record_is_back_under_the_floor() {
    let mut p = profile();
    p.game_hours = 120.0 * DAY;
    cite(&mut p, 1, 10.0 * DAY);
    p.record_mut().serious_violations.push(100.0 * DAY);
    assert_eq!(record_band(&p), TRUST_GUARDED);
    let lifts = p
        .driving_record()
        .unwrap()
        .window_clears_at(p.game_hours, |c, s| {
            c > CARRIER_REVIEW_CITATIONS || s >= CARRIER_REVIEW_SERIOUS
        })
        .expect("the review holds");
    assert!((lifts - (100.0 + REVIEW_WINDOW_DAYS as f64) * DAY).abs() < 1e-6);
    p.game_hours = (10.0 + REVIEW_WINDOW_DAYS as f64) * DAY + 1.0;
    assert_eq!(
        record_band(&p),
        TRUST_GUARDED,
        "the citation leaving is not it"
    );
    p.game_hours = lifts + 1.0;
    assert_eq!(record_band(&p), TRUST_FULL);
}

#[test]
fn legacy_citations_without_a_time_never_count_against_the_window() {
    let mut p = profile();
    p.game_hours = 400.0 * DAY;
    for _ in 0..10 {
        p.record_mut().record_citation(200.0);
    }
    assert_eq!(p.driving_record().unwrap().citations, 10);
    assert_eq!(
        p.driving_record()
            .unwrap()
            .citations_in_window(p.game_hours),
        0
    );
    assert_eq!(record_band(&p), TRUST_FULL);
    assert_eq!(record_insurance_surcharge(&p), 1.0);
    assert_eq!(standing_text(&p), "Record: clean.");
}

#[test]
fn the_record_review_is_a_company_driver_matter_and_the_surcharge_an_owner_operators() {
    use crate::models::business_constants::{INDEPENDENT_AUTHORITY, LEASED_OWNER_OPERATOR};
    let mut owner = profile();
    owner.business_status = LEASED_OWNER_OPERATOR.to_string();
    owner.game_hours = 400.0 * DAY;
    cite(&mut owner, 4, 380.0 * DAY);
    owner.record_mut().record_serious_violation(390.0 * DAY);
    assert_eq!(record_band(&owner), TRUST_FULL);
    assert!(!carrier_termination_due(&owner));
    // 1 + 4 x 0.10 + 1 x 0.35
    assert_eq!(record_insurance_surcharge(&owner), 1.75);
    let said = record_consequence_text(&owner);
    assert!(
        said.starts_with("Insurance surcharge: 75 percent until "),
        "{said}"
    );
    owner.business_status = INDEPENDENT_AUTHORITY.to_string();
    assert_eq!(record_insurance_surcharge(&owner), 1.75);

    let mut company = profile();
    company.game_hours = owner.game_hours;
    cite(&mut company, 4, 380.0 * DAY);
    assert_eq!(record_insurance_surcharge(&company), 1.0);
}

#[test]
fn the_insurance_surcharge_never_passes_double() {
    use crate::models::business_constants::LEASED_OWNER_OPERATOR;
    let mut owner = profile();
    owner.business_status = LEASED_OWNER_OPERATOR.to_string();
    owner.game_hours = 400.0 * DAY;
    cite(&mut owner, 14, 380.0 * DAY);
    assert_eq!(record_insurance_surcharge(&owner), INSURANCE_SURCHARGE_MAX);
    assert!(record_consequence_text(&owner).contains(": 100 percent"));
}

#[test]
fn the_record_line_counts_recent_citations_and_says_what_they_cost() {
    let mut p = profile();
    p.game_hours = 400.0 * DAY;
    cite(&mut p, 2, 380.0 * DAY);
    // Under the floor: counted, nothing more.
    assert_eq!(standing_text(&p), "Record: two citations in the last year.");
    cite(&mut p, 2, 390.0 * DAY);
    let line = standing_text(&p);
    assert!(
        line.starts_with("Record: four citations in the last year."),
        "{line}"
    );
    assert!(
        line.contains("The carrier's record review holds your equipment back"),
        "{line}"
    );
    // A serious violation keeps its own clause and its own next-step tail.
    p.record_mut().record_serious_violation(395.0 * DAY);
    let line = standing_text(&p);
    assert!(
        line.contains("four citations in the last year, one serious violation."),
        "{line}"
    );
    assert!(
        line.contains("One more before your CDL is suspended for 60 days."),
        "{line}"
    );
}

#[test]
fn a_career_saved_before_the_review_existed_starts_it_from_the_first_load() {
    // Staging, 2026-09-12: two testers carry serious violations inside the
    // window, one of them six. Under the review as first written, both were
    // due for termination on their first terminal visit of the new build,
    // over offenses they served through the license ladder at the time.
    use crate::models::profile::Profile;
    // The round trip reads the save directory for the signing key; pin a
    // throwaway one for this thread, the way the profile tests do.
    let tmp = tempfile::tempdir().expect("a temp dir");
    let previous = crate::settings::paths::set_thread_data_dir(Some(tmp.path().join("data")));
    let mut p = real_profile();
    p.game_hours = 400.0 * DAY;
    p.driving_record.record_serious_violation(390.0 * DAY);
    p.driving_record.record_serious_violation(395.0 * DAY);
    p.driving_record.record_citation_at(200.0, 396.0 * DAY);
    let mut saved = p.to_dict();
    let record = saved
        .get_mut("driving_record")
        .and_then(serde_json::Value::as_object_mut)
        .expect("a record");
    record.remove("review_started_h");
    record.remove("citation_times");

    let loaded = Profile::from_dict(&saved);
    assert_eq!(loaded.driving_record.review_started_h, 400.0 * DAY);
    // The license ladder still sees both: the CDL is suspended as before.
    assert_eq!(
        loaded.driving_record.serious_in_window(loaded.game_hours),
        2
    );
    assert!(loaded.driving_record.suspended(loaded.game_hours));
    // The carrier's review and the insurer see none of it.
    assert_eq!(
        loaded
            .driving_record
            .serious_in_review_window(loaded.game_hours),
        0
    );
    assert_eq!(record_band(&loaded), TRUST_FULL);
    assert!(!record_past_termination_floor(
        &loaded.driving_record,
        loaded.game_hours
    ));
    assert_eq!(record_insurance_surcharge(&loaded), 1.0);

    // From here on, everything counts: the next serious violation is the
    // first the review knows about.
    let mut on = loaded;
    on.game_hours += 10.0 * DAY;
    on.driving_record.record_serious_violation(on.game_hours);
    assert_eq!(on.driving_record.serious_in_review_window(on.game_hours), 1);
    assert_eq!(record_band(&on), TRUST_GUARDED);
    // And a save that carries the field keeps it through a round trip.
    let again = Profile::from_dict(&on.to_dict());
    assert_eq!(again.driving_record.review_started_h, 400.0 * DAY);
    crate::settings::paths::set_thread_data_dir(previous);
}
