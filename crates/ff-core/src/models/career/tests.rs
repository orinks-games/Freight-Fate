//! Ported from the `Career` cases of `tests/test_models.py`,
//! `tests/test_career_economy.py` (the XP half) and
//! `tests/test_career_progression.py`. Cases that drive the app shell or a
//! `Profile` save round trip are ignored with the reason; their bodies say
//! what they checked.

use super::*;
use crate::models::career_ladder::{rank_for_level, MAX_CAREER_LEVEL};

fn approx(a: f64, b: f64) -> bool {
    // pytest.approx: rel 1e-6, abs 1e-12.
    (a - b).abs() <= (1e-6 * b.abs()).max(1e-12)
}

/// `Career.record_delivery(miles, pay, on_time, damage_pct)` with the defaults.
fn deliver(
    career: &mut Career,
    miles: f64,
    pay: f64,
    on_time: bool,
    damage_pct: f64,
) -> Vec<String> {
    career.record_delivery(miles, pay, on_time, damage_pct, 1.0, 1.0)
}

// --- test_models.py --------------------------------------------------------

#[test]
fn test_level_thresholds() {
    assert_eq!(level_for_xp(0.0), 1);
    assert_eq!(level_for_xp(999.0), 1);
    assert_eq!(level_for_xp(1000.0), 2);
    assert_eq!(level_for_xp(2500.0), 3);
    assert_eq!(level_for_xp(202_000.0), 20);
    assert_eq!(rank_for_level(20).title, "Established Owner-Operator");
    assert_eq!(level_for_xp(LEVEL_XP[29]), MAX_CAREER_LEVEL);
    assert_eq!(level_for_xp(999_999.0), MAX_CAREER_LEVEL);
    assert_eq!(
        rank_for_level(MAX_CAREER_LEVEL).title,
        "Freight Fate Independent"
    );
}

#[test]
fn test_endorsements_unlock_with_levels() {
    let mut c = Career::new();
    assert!(c.endorsements().is_empty());
    c.xp = 1000.0;
    assert!(c.endorsements().contains("refrigerated"));
    c.xp = 2500.0;
    let e = c.endorsements();
    assert!(e.contains("refrigerated") && e.contains("heavy_haul"));
    c.xp = 4500.0;
    let e = c.endorsements();
    assert!(e.contains("refrigerated") && e.contains("heavy_haul") && e.contains("high_value"));
}

#[test]
fn a_background_check_holds_the_credential_until_it_clears() {
    use crate::models::career::PendingCredential;
    let mut c = Career::new();
    c.pending_credentials.push(PendingCredential {
        key: "hazmat".to_string(),
        ready_at_h: 720.0,
    });
    // Before the clear date: nothing granted, nothing spoken, still pending.
    assert!(c.activate_pending(719.0, true).is_empty());
    assert!(!c.endorsements().contains("hazmat"));
    assert_eq!(c.pending_credentials.len(), 1);
    // On the clear date: the endorsement lands, is announced, and queues
    // its terminal repeat.
    let messages = c.activate_pending(720.0, true);
    assert!(messages.iter().any(|m| m.contains("hazmat")));
    assert!(c.endorsements().contains("hazmat"));
    assert!(c.pending_credentials.is_empty());
    let reminders = c.take_unacknowledged_grants();
    assert_eq!(reminders, ["Reminder: you hold a new hazmat endorsement."]);
    // Drained means drained: the reminder is spoken once.
    assert!(c.take_unacknowledged_grants().is_empty());
}

#[test]
fn the_x_combination_is_announced_when_the_pair_completes() {
    use crate::models::career::PendingCredential;
    // A level-16 career already holds tank by level; the hazmat check
    // clearing completes the pair.
    let mut c = Career::with_xp(130_000.0);
    assert!(c.endorsements().contains("tank"));
    c.pending_credentials.push(PendingCredential {
        key: "hazmat".to_string(),
        ready_at_h: 0.0,
    });
    let messages = c.activate_pending(1.0, false);
    assert!(messages.iter().any(|m| m.contains("X combination")));
    // Activation at the terminal (queue_repeat false) leaves no reminder.
    assert!(c.take_unacknowledged_grants().is_empty());
}

#[test]
fn a_level_grant_queues_its_terminal_repeat() {
    let mut c = Career::with_xp(950.0);
    let messages = deliver(&mut c, 100.0, 300.0, true, 0.0);
    assert!(messages
        .iter()
        .any(|m| m.contains("refrigerated certificate")));
    // Level 2 sponsors two certificates at once; both queue, ladder order.
    let reminders = c.take_unacknowledged_grants();
    assert_eq!(
        reminders,
        [
            "Reminder: you hold a new refrigerated certificate.",
            "Reminder: you hold a new flatbed securement certificate.",
        ]
    );
}

#[test]
fn test_record_delivery_announces_level_up() {
    let mut c = Career::with_xp(950.0);
    let messages = deliver(&mut c, 100.0, 300.0, true, 0.0);
    assert!(messages.iter().any(|m| m.contains("Level up")));
    assert!(messages
        .iter()
        .any(|m| m.contains("New Hire Company Driver")));
    assert_eq!(c.deliveries, 1);
    assert_eq!(c.on_time_deliveries, 1);
}

#[test]
fn test_career_summary_includes_rank_and_next_step() {
    let c = Career::new();

    let text = c.summary();

    assert!(text.contains("Yard Trainee"));
    assert!(text.contains("Career stage: Company driver"));
    assert!(text.contains("Next: level 2, New Hire Company Driver"));
}

#[test]
fn test_reputation_moves_with_performance() {
    let mut c = Career::new();
    let start = c.reputation;
    deliver(&mut c, 100.0, 300.0, true, 0.0);
    assert!(c.reputation > start);
    deliver(&mut c, 100.0, 300.0, false, 40.0);
    assert!(c.reputation < start + 2.0 + 0.01);
}

// --- test_career_economy.py: XP multipliers ---------------------------------

/// The four `CARGO_CATALOG` entries the Python test names, on the two fields
/// `xp_class_multiplier` reads.
struct FakeCargo {
    endorsement: Option<&'static str>,
    min_level: i64,
}

impl XpCargo for FakeCargo {
    fn endorsement(&self) -> Option<&str> {
        self.endorsement
    }
    fn min_level(&self) -> i64 {
        self.min_level
    }
}

#[test]
fn test_specialty_and_premium_cargo_teach_more_per_mile() {
    let refrigerated = FakeCargo {
        endorsement: Some("refrigerated"),
        min_level: 1,
    };
    let electronics = FakeCargo {
        endorsement: Some("high_value"),
        min_level: 1,
    };
    let automotive = FakeCargo {
        endorsement: None,
        min_level: 2,
    };
    let general = FakeCargo {
        endorsement: None,
        min_level: 1,
    };
    assert!(approx(xp_class_multiplier(&refrigerated), 1.5));
    assert!(approx(xp_class_multiplier(&electronics), 1.5));
    assert!(approx(xp_class_multiplier(&automotive), 1.25));
    assert!(approx(xp_class_multiplier(&general), 1.0));
}

#[test]
fn test_on_time_streak_compounds_and_late_resets_it() {
    let mut career = Career::new();
    deliver(&mut career, 100.0, 500.0, true, 50.0);
    assert_eq!(career.on_time_streak, 1);
    let first = career.xp;
    // completion XP plus per-mile XP, no streak bonus on the first run
    assert!(approx(first, 150.0 + 100.0 * 1.6));

    deliver(&mut career, 100.0, 500.0, true, 50.0);
    assert!(approx(career.xp - first, (150.0 + 100.0 * 1.6) * 1.05)); // streak of 2

    deliver(&mut career, 100.0, 500.0, false, 50.0);
    assert_eq!(career.on_time_streak, 0);
}

#[test]
fn test_late_deliveries_still_teach_a_reduced_lesson() {
    let mut career = Career::new();
    deliver(&mut career, 100.0, 500.0, false, 50.0);
    assert!(approx(career.xp, 75.0 + 100.0 * 0.9));
}

#[test]
fn test_clean_cargo_pays_a_bonus_lesson() {
    let mut career = Career::new();
    deliver(&mut career, 100.0, 500.0, true, 0.0);
    assert!(approx(career.xp, (150.0 + 100.0 * 1.6) * 1.15));
}

#[test]
fn test_streak_bonus_caps_near_half() {
    assert!(approx(xp_streak_bonus(1), 0.0));
    assert!(approx(xp_streak_bonus(3), 0.10));
    assert!(approx(xp_streak_bonus(6), 0.25));
    assert!(approx(xp_streak_bonus(10), 0.45));
    assert!(approx(xp_streak_bonus(40), 0.45));
}

#[test]
fn test_short_hop_streak_bonus_is_capped_at_the_mileage_xp() {
    // A board-minimum 25-mile hop at a deep on-time streak: the streak bonus
    // is capped at what the miles themselves taught (25 * 1.6 = 40 XP), not
    // 45 percent of the whole award including the flat completion XP. This is
    // the short-hop farming cap: a streak can at most double the road lesson,
    // it cannot mint XP off the flat per-delivery award.
    let mut career = Career::new();
    career.on_time_streak = 9; // this delivery makes it 10, the share cap
    deliver(&mut career, 25.0, 300.0, true, 0.0);
    let base = 150.0 + 25.0 * 1.6;
    let mileage_xp = 25.0 * 1.6;
    assert!(approx(career.xp, (base + mileage_xp) * 1.15));
}

#[test]
fn test_streak_beyond_the_cap_adds_nothing_more() {
    // Once both caps are saturated, a longer streak earns the same bonus.
    let mut at_cap = Career::new();
    at_cap.on_time_streak = 19;
    deliver(&mut at_cap, 25.0, 300.0, true, 0.0);

    let mut far_beyond = Career::new();
    far_beyond.on_time_streak = 39;
    deliver(&mut far_beyond, 25.0, 300.0, true, 0.0);

    assert!(approx(far_beyond.xp, at_cap.xp));
}

#[test]
fn test_honest_haul_streak_values_unchanged_below_the_cap() {
    // Real freight is untouched: on any haul long enough that the road XP
    // exceeds the capped share of the award (about 77 miles and up at plain
    // freight), the streak bonus is the same arithmetic it has always been.
    let mut long_haul = Career::new();
    long_haul.on_time_streak = 11; // deep streak, share already at 0.45
    deliver(&mut long_haul, 500.0, 1800.0, true, 0.0);
    assert!(approx(long_haul.xp, (150.0 + 500.0 * 1.6) * 1.45 * 1.15));

    // Even the shortest freight the honest pacing model deals (105 miles at
    // level 1) sits above the threshold; 80 miles still clears it.
    let mut short_honest = Career::new();
    short_honest.on_time_streak = 14;
    deliver(&mut short_honest, 80.0, 400.0, true, 0.0);
    assert!(approx(short_honest.xp, (150.0 + 80.0 * 1.6) * 1.45 * 1.15));
}

#[test]
fn test_specialty_multiplier_applies_through_record_delivery() {
    let mut career = Career::new();
    career.record_delivery(100.0, 500.0, true, 50.0, 1.5, 1.0);
    assert!(approx(career.xp, 150.0 + 100.0 * 1.6 * 1.5));
}

#[test]
fn test_single_level_up_speaks_the_one_rank() {
    // A one-rank promotion keeps the exact wording players already expect.
    let mut career = Career::new();
    // 150 completion + 600 * 1.6 mileage XP = 1110: past the 1000 threshold
    // for level 2, short of the 2500 threshold for level 3.
    let messages = deliver(&mut career, 600.0, 900.0, true, 50.0);
    assert_eq!(career.level(), 2);
    let rank = rank_for_level(2);
    let level_ups: Vec<&String> = messages
        .iter()
        .filter(|m| m.starts_with("Level up!"))
        .collect();
    assert_eq!(
        level_ups,
        vec![&format!(
            "Level up! You are now level 2: {}. Unlock: {}",
            rank.title, rank.unlock
        )]
    );
}

#[test]
fn test_multi_level_up_speaks_every_rank_passed_through() {
    // A delivery big enough to jump several ranks must not go silent on the
    // ranks in between -- every passed rank's unlock is spoken, in order.
    let mut career = Career::new();
    career.xp = 999.0; // one XP short of level 2
    assert_eq!(career.level(), 1);
    // 150 completion + 3000 * 1.6 mileage XP = 4950; 999 + 4950 = 5949,
    // which lands inside the level-4 band (4500..7000): levels 2 and 3 are
    // passed through in the same delivery.
    let messages = deliver(&mut career, 3000.0, 5000.0, true, 50.0);
    assert_eq!(career.level(), 4);
    let level_ups: Vec<String> = messages
        .into_iter()
        .filter(|m| m.starts_with("Level up!"))
        .collect();
    let expected: Vec<String> = [2, 3, 4]
        .iter()
        .map(|lvl| {
            format!(
                "Level up! You are now level {lvl}: {}. Unlock: {}",
                rank_for_level(*lvl).title,
                rank_for_level(*lvl).unlock
            )
        })
        .collect();
    assert_eq!(level_ups, expected);
}

#[test]
fn test_first_twenty_thresholds_stay_save_compatible() {
    // Shipped 1.8 careers were leveled against these numbers; changing them
    // would silently re-level existing saves.
    assert_eq!(
        &LEVEL_XP[..20],
        &[
            0.0, 1000.0, 2500.0, 4500.0, 7000.0, 10_000.0, 14_000.0, 19_000.0, 25_000.0, 32_000.0,
            40_000.0, 50_000.0, 62_000.0, 76_000.0, 92_000.0, 110_000.0, 130_000.0, 152_000.0,
            176_000.0, 202_000.0,
        ]
    );
    assert_eq!(LEVEL_XP.len(), 30);
    assert!(LEVEL_XP.windows(2).all(|w| w[0] <= w[1]));
}

#[test]
fn test_streak_survives_the_save_round_trip() {
    use crate::models::profile::{tests::with_data_dir, Profile};
    with_data_dir(|_| {
        let mut profile = Profile::named_in("Streak Save", "Chicago");
        profile.career.on_time_streak = 4;
        let reloaded = Profile::from_dict(&profile.to_dict());
        assert_eq!(reloaded.career.on_time_streak, 4);
    });
}

#[test]
fn test_career_round_trips_through_asdict_json() {
    // `dataclasses.asdict(Career())` keys, and an old save missing a newer
    // field still loads with the default.
    let career = Career::new();
    // Field order is the dataclass order (serde_json::Value sorts keys, so
    // read it off the serialized text).
    let text = serde_json::to_string(&career).unwrap();
    let keys: Vec<&str> = text
        .split('"')
        .skip(1)
        .step_by(2)
        .filter(|k| !k.is_empty())
        .collect();
    assert_eq!(
        keys,
        vec![
            "xp",
            "reputation",
            "standing",
            "deliveries",
            "on_time_deliveries",
            "total_miles",
            "total_earnings",
            "dispatch_declines_used",
            "on_time_streak",
            "purchased_endorsements",
            "pending_credentials",
            "unacknowledged_grants",
        ]
    );
    let old = serde_json::json!({"xp": 1200.0, "reputation": 61.0, "deliveries": 3});
    let loaded: Career = serde_json::from_value(old).unwrap();
    assert_eq!(loaded.level(), 2);
    assert_eq!(loaded.dispatch_declines_used, 0);
    assert_eq!(loaded.on_time_streak, 0);
    assert!(loaded.purchased_endorsements.is_empty());
    assert!(loaded.pending_credentials.is_empty());
    assert!(loaded.unacknowledged_grants.is_empty());
}

// --- test_career_progression.py ---------------------------------------------

#[test]
fn test_the_summary_says_how_much_experience_the_next_level_needs() {
    // Brandon, tester report 2026-08-17: "put in a way to check to see how
    // much experience you need to go up to the next level."
    //
    // The summary named the level and the next RANK, and gave the raw XP
    // total, but never the gap between them -- so the one question a player
    // actually asks had no answer anywhere in the game.
    let career = Career::with_xp(LEVEL_XP[1] + 10.0);
    let owed = xp_to_next_level(career.xp);
    assert_eq!(owed, Some(LEVEL_XP[2] - career.xp));
    // And it is spoken, next to the level it belongs to.
    assert!(career
        .summary()
        .contains(&format!("more to level {}", career.level() + 1)));

    // Landing exactly on a threshold owes the whole of the next step, not zero.
    let on_threshold = LEVEL_XP[2];
    assert_eq!(
        xp_to_next_level(on_threshold),
        Some(LEVEL_XP[3] - on_threshold)
    );
    assert_eq!(level_for_xp(on_threshold), 3);

    // At the ceiling there is no next level to owe anything to, and the
    // summary must not promise one.
    let top = Career::with_xp(LEVEL_XP[29] + 5000.0);
    assert_eq!(top.level(), MAX_CAREER_LEVEL);
    assert_eq!(xp_to_next_level(top.xp), None);
    assert!(!top.summary().contains("more to level"));
}

// --- standing XP rate (pinned against the enforcement bands) ----------------

#[test]
fn test_standing_xp_rate_keys_match_the_enforcement_bands() {
    use crate::models::enforcement::{TRUST_FULL, TRUST_GUARDED, TRUST_LAST_CHANCE, TRUST_POOR};
    let bands: Vec<&str> = STANDING_XP_RATE.iter().map(|(b, _)| *b).collect();
    assert_eq!(
        bands,
        vec![TRUST_FULL, TRUST_GUARDED, TRUST_POOR, TRUST_LAST_CHANCE]
    );
    assert_eq!(standing_xp_rate("full"), 1.0);
    assert!(STANDING_XP_RATE.iter().all(|(_, r)| *r > 0.0 && *r <= 1.0));
    assert_eq!(xp_rate_clause("full"), "");
    assert!(xp_rate_clause("poor").contains("dispatch trust is poor"));
    assert_eq!(
        xp_rate_settlement_clause("guarded"),
        "at the slower rate that comes with guarded dispatch trust"
    );
}

#[test]
fn test_summary_spells_money_and_miles_with_thousands_separators() {
    let mut career = Career::with_xp(LEVEL_XP[2] + 0.4);
    career.total_miles = 12_345.6;
    career.total_earnings = 98_765.4;
    career.deliveries = 3;
    career.on_time_deliveries = 2;
    let text = career.summary();
    assert!(
        text.starts_with("Level 3, Solo Company Driver. 2500 experience. 2,000 more to level 4. ")
    );
    assert!(text.contains(
        "Deliveries 3, 67 percent on time. 12,346 lifetime miles, 98,765 dollars earned."
    ));
}
