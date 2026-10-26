//! Ported from `tests/test_career_objectives.py`. The pure `career_objective`
//! cases run against the career-side `FakeProfile`; the cases that drive the
//! app shell (`App`, `CityMenuState`, `JobBoardState`) are ignored with the
//! reason and their bodies say what they checked.

use super::*;
use crate::models::business_constants::LEASED_OWNER_OPERATOR;
use crate::models::career::test_profile::FakeProfile;
use crate::models::career::LEVEL_XP;

fn profile(name: &str) -> FakeProfile {
    FakeProfile::named(name)
}

#[test]
fn test_company_driver_objective_tapers_from_first_week_to_trust() {
    let mut profile = profile("Career Plan");
    profile.achievements.push("first_dispatch".to_string());

    let first_load = career_objective(&profile);
    assert_eq!(first_load.title, "First dispatch");
    assert!(first_load.terminal_text.contains("real freight"));
    assert!(first_load.terminal_text.contains("short standard load"));
    assert!(!first_load
        .spoken_summary()
        .to_lowercase()
        .contains("probation"));

    profile.career.deliveries = 2;
    let reminder = career_objective(&profile);
    assert_eq!(reminder.title, "First-week service record");
    assert!(reminder
        .terminal_text
        .contains("steady service, not perfection"));

    profile.career.deliveries = 4;
    profile.career.reputation = 62.0;
    let trust = career_objective(&profile);
    assert_eq!(trust.title, "Build dispatcher trust");
    assert!(trust.terminal_text.contains("on-time service"));
}

#[test]
fn test_low_reputation_company_driver_keeps_trust_objective_after_training() {
    let mut profile = profile("Trust Plan");
    profile.achievements.push("first_dispatch".to_string());
    profile.career.deliveries = 12;
    profile.career.reputation = 62.0;

    let objective = career_objective(&profile);

    assert_eq!(objective.title, "Build dispatcher trust");
    assert!(objective.terminal_text.contains("on-time service"));
}

#[test]
fn test_company_driver_objective_uses_level_band_guidance_after_training() {
    let mut profile = profile("Regional Plan");
    profile.achievements.push("first_dispatch".to_string());
    profile.career.xp = LEVEL_XP[3];
    profile.career.deliveries = 12;
    profile.career.reputation = 82.0;

    let objective = career_objective(&profile);

    assert_eq!(objective.title, "Build a regional service record");
    assert!(objective.terminal_text.contains("Broader company lanes"));
    assert_eq!(objective.recommendation, "reputation-building lane");
}

#[test]
fn test_ready_unlock_states_override_level_band_guidance() {
    let mut profile = profile("Buy In");
    profile.achievements.push("first_dispatch".to_string());
    profile.career.xp = LEVEL_XP[(OWNER_OPERATOR_LEVEL - 1) as usize];
    profile.career.deliveries = 35;
    profile.career.reputation = 82.0;
    profile.money = 60_000.0;

    let objective = career_objective(&profile);

    assert_eq!(objective.title, "Owner-operator buy-in ready");
    assert_eq!(objective.recommendation, "clean company load");
}

#[test]
fn test_owner_operator_objective_emphasizes_working_capital() {
    let mut profile = profile("Owner Plan");
    profile.business_status = LEASED_OWNER_OPERATOR.to_string();
    profile.owned_trucks = vec!["rig".to_string()];
    profile.money = 12_000.0;
    profile.achievements.push("first_dispatch".to_string());

    let objective = career_objective(&profile);

    assert_eq!(objective.title, "Protect working capital");
    assert!(objective
        .terminal_text
        .contains("Fuel, maintenance, insurance"));
}

// `test_terminal_career_plan_is_keyboard_reachable_and_spoken` is live in `crates/freight-fate/tests/states_city.rs`.

// `test_terminal_career_plan_speaks_senior_company_level_guidance` is live in `crates/freight-fate/tests/states_city.rs`.

// `test_dispatch_board_speaks_objective_and_marks_recommended_job` is live in `crates/freight-fate/tests/states_city.rs`.

// `test_dispatch_board_speaks_authority_level_recommendation` is live in `crates/freight-fate/tests/states_city.rs`.

#[test]
fn test_late_company_driver_plan_points_to_owner_operator_prep() {
    let mut profile = profile("Prep Plan");
    profile.achievements.push("first_dispatch".to_string());
    profile.career.xp = LEVEL_XP[(OWNER_OPERATOR_LEVEL - 4) as usize];
    profile.career.deliveries = 25;
    profile.career.reputation = 75.0;

    let objective = career_objective(&profile);

    assert_eq!(objective.title, "Owner-operator preparation");
    assert!(objective.terminal_text.contains("cash cushion"));
    assert_eq!(
        objective.terminal_text,
        "Work toward level 18, 35 deliveries, 80 reputation, and a cash cushion."
    );
}

// `test_first_day_terminal_entry_speaks_training_arc_without_tutorial_language` is live in `crates/freight-fate/tests/states_city.rs`.

// `test_out_of_sync_company_terminal_entry_uses_first_week_guidance` is live in `crates/freight-fate/tests/states_city.rs`.

// `test_dispatch_board_recommendation_label_is_spoken_and_visible` is live in `crates/freight-fate/tests/states_city.rs`.

// `test_owner_operator_first_day_terminal_keeps_cash_cushion_guidance` is live in `crates/freight-fate/tests/states_city.rs`.

#[test]
#[ignore = "wrong crate: ff-core cannot see the game crate, so this case belongs in crates/freight-fate/tests/ -- needs freight_fate::app (App, CityMenuState, JobBoardState)"]
fn test_out_of_sync_owner_operator_uses_career_guidance() {
    // Owner-operator start past day one: the city menu says "Career
    // objective:" with "Fuel, maintenance, insurance" and lists "Career plan";
    // the board entry says "cash-positive load".
    unimplemented!("needs the app shell")
}

// `test_owner_operator_first_day_dispatch_board_keeps_business_cost_guidance` is live in `crates/freight-fate/tests/states_city.rs`.

// --- the owner-operator and authority branches the Python suite reaches
// --- only through the app shell -----------------------------------------------

#[test]
fn test_owner_operator_objective_moves_from_capital_to_authority_prep() {
    let mut profile = profile("Owner Arc");
    profile.business_status = LEASED_OWNER_OPERATOR.to_string();
    profile.owned_trucks = vec!["rig".to_string()];
    profile.money = 80_000.0;
    profile.career.xp = LEVEL_XP[18];
    profile.career.deliveries = 40;
    profile.career.reputation = 85.0;

    let prep = career_objective(&profile);
    assert_eq!(prep.title, "Authority preparation");
    assert_eq!(
        prep.terminal_text,
        "Build reputation, deliveries, and at least 25,000 dollars in working capital."
    );
    assert_eq!(prep.recommendation, "owner-operator margin load");

    profile.career.xp = LEVEL_XP[20];
    profile.career.deliveries = 60;
    profile.career.reputation = 90.0;
    let ready = career_objective(&profile);
    assert_eq!(ready.title, "Authority prep ready");
    assert_eq!(ready.recommendation, "reserve-safe load");

    profile.authority_readiness = true;
    profile.career.xp = LEVEL_XP[17];
    let level_band = career_objective(&profile);
    assert_eq!(level_band.title, "Protect owner-operator margin");
    assert_eq!(level_band.milestone_text, "");
    assert_eq!(
        level_band.spoken_summary(),
        "Protect owner-operator margin. Fuel, maintenance, and trailer costs come out of every \
         load."
    );
}

#[test]
fn test_independent_authority_objective_grows_then_follows_the_level_band() {
    use crate::models::business_constants::INDEPENDENT_AUTHORITY;
    let mut profile = profile("Independent");
    profile.business_status = INDEPENDENT_AUTHORITY.to_string();
    profile.owned_trucks = vec!["rig".to_string()];
    profile.money = 90_000.0;
    profile.career.xp = LEVEL_XP[21];
    profile.career.deliveries = 80;
    profile.career.reputation = 94.0;

    let growing = career_objective(&profile);
    assert_eq!(growing.title, "Grow direct freight reputation");
    assert_eq!(growing.recommendation, "direct freight with margin");

    profile.career.xp = LEVEL_XP[24];
    let band = career_objective(&profile);
    assert_eq!(band.title, "Grow a freight business");
    assert_eq!(band.recommendation, "direct freight with margin");
}

#[test]
fn test_a_company_driver_who_chose_to_stay_gets_a_company_plan() {
    let mut profile = profile("Stay Company");
    profile.achievements.push("first_dispatch".to_string());
    profile.career.xp = LEVEL_XP[(OWNER_OPERATOR_LEVEL - 1) as usize];
    profile.career.deliveries = 35;
    profile.career.reputation = 82.0;
    profile.money = 60_000.0;
    assert_eq!(
        career_objective(&profile).title,
        "Owner-operator buy-in ready"
    );

    profile.owner_operator_declined = true;

    let objective = career_objective(&profile);
    assert_eq!(objective.title, "Company driver by choice");
    assert!(objective.terminal_text.contains("stays open"));
    assert!(!objective.spoken_summary().contains("buy-in ready"));
}
