use serde_json::{Map, Value};

use super::*;
use crate::models::business_constants::{COMPANY_DRIVER, LEASED_OWNER_OPERATOR};

fn stats() -> Map<String, Value> {
    Map::new()
}

#[test]
fn every_cast_key_is_unique_and_every_member_has_a_bio() {
    let mut keys: Vec<&str> = CAST.iter().map(|c| c.key).collect();
    keys.sort_unstable();
    keys.dedup();
    assert_eq!(keys.len(), CAST.len());
    for member in CAST {
        assert!(!member.bio.is_empty(), "{} has no bio", member.key);
        assert!(
            !member.habits.leans.is_empty(),
            "{} has no station",
            member.key
        );
        assert!(
            !member.bio.contains("  "),
            "{} bio has a doubled space",
            member.key
        );
    }
}

#[test]
fn the_first_team_load_is_always_dan() {
    for seed in 0..50 {
        let pairing = pair(&stats(), COMPANY_DRIVER, 3, &seed.to_string());
        assert_eq!(pairing.co_driver.key, "dan");
        assert_eq!(pairing.reason, PairingReason::FirstTeam);
    }
}

#[test]
fn later_loads_draw_from_the_class_and_meridian_turns_up_once() {
    let mut s = stats();
    record_team_load(&mut s, "dan", COMPANY_DRIVER);
    let mut drawn = std::collections::BTreeSet::new();
    let mut lou_seed = None;
    for seed in 0..400 {
        let pairing = pair(&s, COMPANY_DRIVER, 5, &seed.to_string());
        drawn.insert(pairing.co_driver.key);
        assert_ne!(
            pairing.co_driver.key, "ray",
            "the trainer waits for level 20"
        );
        if pairing.reason == PairingReason::Meridian {
            lou_seed.get_or_insert(seed);
        }
    }
    assert!(drawn.len() >= 7, "only {drawn:?} drawn");
    let seed = lou_seed.expect("Lou is drawn on some seed");
    record_team_load(&mut s, "lou", COMPANY_DRIVER);
    assert_ne!(
        pair(&s, COMPANY_DRIVER, 5, &seed.to_string()).co_driver.key,
        "lou"
    );
}

#[test]
fn the_trainer_rides_once_late_in_the_career() {
    let mut s = stats();
    record_team_load(&mut s, "dan", COMPANY_DRIVER);
    let pairing = pair(&s, COMPANY_DRIVER, TRAINER_LEVEL, "7");
    assert_eq!(pairing.co_driver.key, "ray");
    record_team_load(&mut s, "ray", COMPANY_DRIVER);
    assert_ne!(
        pair(&s, COMPANY_DRIVER, TRAINER_LEVEL, "7").co_driver.key,
        "ray"
    );
}

#[test]
fn an_owner_operator_keeps_the_classmate_they_teamed_with_most() {
    let mut s = stats();
    for _ in 0..3 {
        record_team_load(&mut s, "june", COMPANY_DRIVER);
    }
    record_team_load(&mut s, "walt", COMPANY_DRIVER);
    let pairing = pair(&s, LEASED_OWNER_OPERATOR, 18, "1");
    assert_eq!(pairing.co_driver.key, "june");
    assert_eq!(pairing.reason, PairingReason::Hire);
    record_team_load(&mut s, "june", LEASED_OWNER_OPERATOR);
    for _ in 0..5 {
        record_team_load(&mut s, "walt", COMPANY_DRIVER);
    }
    // The hire stays the hire, even after more loads with someone else.
    assert_eq!(
        pair(&s, LEASED_OWNER_OPERATOR, 19, "2").co_driver.key,
        "june"
    );
}

#[test]
fn a_company_team_pays_half_for_co_driver_miles() {
    let split = team_split(COMPANY_DRIVER, 1000.0, 600.0, 300.0);
    assert!((split.co_driver_fraction - 0.5).abs() < 1e-9);
    assert!((split.co_driver_pay - 250.0).abs() < 1e-9);
    assert!((split.xp_rate - 0.75).abs() < 1e-9);
}

#[test]
fn an_owner_pays_the_hire_thirty_percent_for_their_miles() {
    let split = team_split(LEASED_OWNER_OPERATOR, 2000.0, 1000.0, 1000.0);
    assert!((split.co_driver_pay - 600.0).abs() < 1e-9);
    assert!((split.xp_rate - CO_DRIVER_XP_RATE).abs() < 1e-9);
}

#[test]
fn a_solo_trip_splits_nothing() {
    let split = team_split(COMPANY_DRIVER, 800.0, 400.0, 0.0);
    assert_eq!(split.co_driver_pay, 0.0);
    assert_eq!(split.xp_rate, 1.0);
    let empty = team_split(COMPANY_DRIVER, 800.0, 0.0, 0.0);
    assert_eq!(empty.co_driver_pay, 0.0);
}

#[test]
fn a_team_is_worthwhile_only_when_the_rest_needs_a_sleep() {
    let fresh = HosClock::new();
    assert!(!team_worthwhile(&fresh, "realistic", 300.0));
    assert!(team_worthwhile(&fresh, "realistic", 12.0 * 60.0));
    let mut tired = HosClock::new();
    tired.drive(9.0 * 60.0);
    assert!(team_worthwhile(&tired, "realistic", 3.0 * 60.0));
    // Nine hours of driving fits the 11-hour limit, but with the break it
    // needs it does not fit what is left of a window 4.75 hours in.
    let mut late = HosClock::new();
    late.on_duty(4.75 * 60.0);
    assert!(team_worthwhile(&late, "realistic", 9.0 * 60.0));
    assert!(!team_worthwhile(&fresh, "realistic", 9.0 * 60.0));
    assert!(!team_worthwhile(&fresh, "off", 600.0));
    assert!(team_worthwhile(&fresh, "off", 700.0));
}

#[test]
fn the_shift_runs_to_the_co_drivers_own_limit() {
    let mut hos = HosClock::new();
    hos.drive(8.0 * 60.0);
    hos.take_break(30.0);
    hos.drive(2.0 * 60.0);
    assert!(!shift_over(&hos, "realistic"));
    hos.drive(50.0);
    assert!(shift_over(&hos, "realistic"));
    assert!(!shift_over(&hos, "off"));
}

#[test]
fn the_break_is_planned_ahead_and_handed_back_only_when_overdue() {
    let mut hos = HosClock::new();
    hos.drive(5.0 * 60.0);
    assert!(!break_wanted(&hos, "realistic"));
    hos.drive(40.0);
    assert!(break_wanted(&hos, "realistic"));
    assert!(!break_overdue(&hos, "realistic"));
    hos.drive(130.0);
    assert!(break_overdue(&hos, "realistic"));
    hos.take_break(30.0);
    assert!(!break_wanted(&hos, "realistic"));
    assert!(!break_wanted(&HosClock::new(), "off"));
}

#[test]
fn a_share_of_loads_are_drawn_as_team_loads_the_same_way_every_time() {
    let drawn = (0..1000)
        .filter(|i| team_load_drawn(&load_seed("a", "b", "general", f64::from(*i))))
        .count();
    assert!((200..400).contains(&drawn), "{drawn} of 1000");
    let seed = load_seed("chicago_il_us", "denver_co_us", "general", 16.0);
    assert_eq!(team_load_drawn(&seed), team_load_drawn(&seed));
}

#[test]
fn only_the_surprise_pairings_carry_a_badge() {
    assert_eq!(pairing_badge("lou"), Some("meridian_classmate"));
    assert_eq!(pairing_badge("ray"), Some("trainer_ride_along"));
    for member in class_members() {
        assert_eq!(pairing_badge(member.key), None);
    }
}

#[test]
fn ten_hours_in_the_sleeper_clears_fatigue() {
    assert!((sleeper_fatigue_relief(hos::SLEEP_MIN) - 100.0).abs() < 1e-9);
}

#[test]
fn the_player_is_rested_after_ten_unbroken_hours_in_the_sleeper() {
    let mut player = HosClock::new();
    assert!(fully_rested(&player), "an empty logbook needs no rest");
    let mut loaded = HosClock::new();
    loaded.on_duty(90.0);
    assert!(
        fully_rested(&loaded),
        "loading after a sleep does not use the rest up"
    );
    player.drive(6.0 * 60.0);
    assert!(!fully_rested(&player));
    assert_eq!(rest_left_min(&player), hos::SLEEP_MIN);
    player.sleeper(4.0 * 60.0);
    assert_eq!(rested_min(&player), 4.0 * 60.0);
    assert!(!rest_complete(&player, "realistic", true));
    player.sleeper(6.0 * 60.0);
    assert!(fully_rested(&player));
    assert!(rest_complete(&player, "realistic", true));
    assert!(
        !rest_complete(&player, "realistic", false),
        "a player rested at the handover gets the co-driver's whole shift"
    );
    assert!(
        !rest_complete(&player, "debug_off", true),
        "with hours off, nobody's rest ends the shift"
    );
}

#[test]
fn the_introduction_is_heard_once_per_career() {
    let mut heard = stats();
    assert!(!introduced(&heard));
    mark_introduced(&mut heard);
    assert!(introduced(&heard));
    let mut teamed = stats();
    record_team_load(&mut teamed, "dan", COMPANY_DRIVER);
    assert!(
        introduced(&teamed),
        "a career that has teamed knows already"
    );
}

#[test]
fn the_shift_left_leaves_out_the_break_the_co_driver_stops_for() {
    let mut hos = HosClock::new();
    hos.drive(7.0 * 60.0);
    assert_eq!(shift_left_min(&hos, "realistic"), Some(4.0 * 60.0));
    assert_eq!(shift_left_min(&hos, "debug_off"), None);
}

#[test]
fn the_roster_lists_only_the_drivers_you_have_teamed_with() {
    let mut stats = stats();
    assert!(teamed_with(&stats).is_empty());
    record_team_load(&mut stats, "june", COMPANY_DRIVER);
    record_team_load(&mut stats, "dan", COMPANY_DRIVER);
    record_team_load(&mut stats, "june", COMPANY_DRIVER);
    let roster: Vec<(&str, i64)> = teamed_with(&stats)
        .into_iter()
        .map(|(member, loads)| (member.key, loads))
        .collect();
    assert_eq!(roster, vec![("dan", 1), ("june", 2)]);
}
