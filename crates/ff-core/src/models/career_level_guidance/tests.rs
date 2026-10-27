//! Ported from `tests/test_career_level_guidance.py`. The `Profile(...)` of
//! the Python tests is the career-side `FakeProfile`.

use super::*;
use crate::models::business_constants::{INDEPENDENT_AUTHORITY, LEASED_OWNER_OPERATOR};
use crate::models::career::test_profile::FakeProfile;
use crate::models::career::LEVEL_XP;

fn profile(level: usize, status: &str) -> FakeProfile {
    let mut profile = FakeProfile::named("Level Guide");
    profile.career.xp = LEVEL_XP[level - 1];
    profile.career.deliveries = 12;
    profile.career.reputation = 82.0;
    profile.business_status = status.to_string();
    if status != "company_driver" {
        profile.owned_trucks = vec!["rig".to_string()];
        profile.money = 80_000.0;
    }
    profile
}

fn company(level: usize) -> FakeProfile {
    profile(level, "company_driver")
}

#[test]
fn test_a_senior_driver_dispatch_stopped_trusting_is_not_told_they_choose_freight() {
    let mut slipped = company(SENIOR_LOAD_CHOICE_LEVEL as usize + 1);
    slipped.career.reputation = 5.0;
    let guidance = career_level_guidance(&slipped);
    assert_eq!(guidance.title, "Earn back your load choice");
    assert!(!guidance.spoken_summary().contains("trusts you"));
}

#[test]
fn test_company_level_guidance_moves_from_regional_to_senior_to_business_prep() {
    let regional = career_level_guidance(&company(4));
    assert_eq!(regional.title, "Build a regional service record");
    assert!(regional.terminal_text.contains("Broader company lanes"));
    assert_eq!(regional.recommendation, "reputation-building lane");

    let senior = career_level_guidance(&company(10));
    assert_eq!(senior.title, "Run like a senior company driver");
    assert!(senior.terminal_text.contains("Premium"));
    assert_eq!(senior.recommendation, "senior company lane");

    let prep = career_level_guidance(&company(14));
    assert_eq!(prep.title, "Prepare for owner-operator risk");
    assert!(prep.terminal_text.contains("cash cushion"));
    assert_eq!(prep.recommendation, "business-prep load");
}

#[test]
fn test_owner_operator_guidance_tracks_margin_authority_and_independence() {
    let leased = career_level_guidance(&profile(18, LEASED_OWNER_OPERATOR));
    assert_eq!(leased.title, "Protect owner-operator margin");
    assert!(leased.terminal_text.contains("trailer costs"));
    assert_eq!(leased.recommendation, "reserve-safe owner-operator freight");

    let authority_prep = career_level_guidance(&profile(22, LEASED_OWNER_OPERATOR));
    assert_eq!(authority_prep.title, "Build authority readiness");
    assert!(authority_prep.terminal_text.contains("trailer strategy"));
    assert_eq!(authority_prep.recommendation, "authority-readiness lane");

    let independent = career_level_guidance(&profile(27, INDEPENDENT_AUTHORITY));
    assert_eq!(independent.title, "Grow a freight business");
    assert_eq!(independent.recommendation, "direct freight with margin");
}

#[test]
fn test_high_level_company_bands_do_not_overstate_business_status() {
    let owner_ready = career_level_guidance(&company(18));
    assert_eq!(owner_ready.title, "Protect owner-operator readiness");
    assert!(owner_ready.terminal_text.contains("TWIC"));
    assert_eq!(owner_ready.recommendation, "reserve-building freight");

    let authority_ready = career_level_guidance(&company(22));
    assert_eq!(authority_ready.title, "Build authority readiness");
    assert!(authority_ready
        .terminal_text
        .contains("before authority is real"));
    assert_eq!(authority_ready.recommendation, "authority-readiness lane");

    let high_level_company = career_level_guidance(&company(25));
    assert_eq!(high_level_company.title, "Plan the next business step");
    assert!(!high_level_company.terminal_text.contains("direct freight"));
    assert_eq!(high_level_company.recommendation, "business-decision lane");

    let leased_without_authority = career_level_guidance(&profile(27, LEASED_OWNER_OPERATOR));
    assert_eq!(leased_without_authority.title, "Build authority readiness");
    assert!(!leased_without_authority
        .terminal_text
        .contains("Use authority"));
    assert_eq!(
        leased_without_authority.recommendation,
        "authority-readiness lane"
    );

    let early_independent = career_level_guidance(&profile(22, INDEPENDENT_AUTHORITY));
    assert_eq!(early_independent.title, "Prove independent authority");
    assert_eq!(
        early_independent.recommendation,
        "authority-building freight"
    );
}

#[test]
fn test_level_30_guidance_is_distinct_per_business_path() {
    let company_30 = career_level_guidance(&company(30));
    let leased = career_level_guidance(&profile(30, LEASED_OWNER_OPERATOR));
    let authority = career_level_guidance(&profile(30, INDEPENDENT_AUTHORITY));

    let titles: std::collections::BTreeSet<&str> = [
        company_30.title.as_str(),
        leased.title.as_str(),
        authority.title.as_str(),
    ]
    .into_iter()
    .collect();
    assert_eq!(titles.len(), 3);
    assert_ne!(company_30.title, career_level_guidance(&company(25)).title);
    assert_ne!(
        authority.title,
        career_level_guidance(&profile(25, INDEPENDENT_AUTHORITY)).title
    );
}

#[test]
fn test_first_week_and_load_choice_bands_name_the_level() {
    let rookie = career_level_guidance(&company(1));
    assert_eq!(rookie.title, "Build first-week trust");
    assert_eq!(
        rookie.spoken_summary(),
        "Build first-week trust. Use trainer support and safer freight to start a clean \
         service record."
    );
    let regional = career_level_guidance(&company(4));
    assert!(regional
        .terminal_text
        .ends_with("Dispatch assigns your loads until level 8."));
    let chooser = career_level_guidance(&company(SENIOR_LOAD_CHOICE_LEVEL as usize));
    assert_eq!(chooser.title, "Choose your own freight");
    assert_eq!(chooser.recommendation, "self-picked reliable lane");
}
