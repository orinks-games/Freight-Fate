//! Ported from `tests/test_career_training.py`. `Profile(...)` plus
//! `apply_start_option` is the career-side `FakeProfile`; the `Job(...)` is
//! `FakeJob::lane`.

use super::*;
use crate::models::career::test_profile::{FakeJob, FakeProfile};
use crate::models::start_options::{apply_start_option, start_option};

fn profile(deliveries: i64, reputation: f64, carrier_key: &str) -> FakeProfile {
    let mut profile = FakeProfile::named("Training");
    apply_start_option(&mut profile, start_option(Some(carrier_key)));
    profile.career.deliveries = deliveries;
    profile.career.reputation = reputation;
    profile
}

fn job(miles: f64, deadline_h: f64, cargo: &str) -> FakeJob {
    FakeJob::lane(miles, deadline_h, cargo)
}

#[test]
fn test_company_training_stage_boundaries_do_not_depend_on_perfect_service() {
    assert_eq!(
        company_training_stage(&profile(0, 35.0, "northstar")),
        TrainingStage::FirstDispatch
    );
    assert_eq!(
        company_training_stage(&profile(1, 35.0, "northstar")),
        TrainingStage::TrainerReminders
    );
    assert_eq!(
        company_training_stage(&profile(2, 35.0, "northstar")),
        TrainingStage::TrainerReminders
    );
    assert_eq!(
        company_training_stage(&profile(3, 35.0, "northstar")),
        TrainingStage::TrustOpening
    );
    assert_eq!(
        company_training_stage(&profile(9, 35.0, "northstar")),
        TrainingStage::TrustBuilding
    );
    assert_eq!(
        company_training_stage(&profile(10, 75.0, "northstar")),
        TrainingStage::NormalGuidance
    );
}

#[test]
fn test_training_guidance_names_the_carrier_without_probation_wording() {
    let guidance = training_guidance(&profile(1, 75.0, "great_lakes_training"));

    let combined = [
        guidance.title.as_str(),
        guidance.terminal_text.as_str(),
        guidance.dispatch_text.as_str(),
        guidance.recommendation_label.as_str(),
    ]
    .join(" ")
    .to_lowercase();
    assert!(combined.contains("great lakes training transport"));
    assert!(combined.contains("trainer"));
    assert!(!combined.contains("probation"));
}

#[test]
fn test_ten_delivery_guidance_tapers_to_normal_company_driver_voice() {
    let guidance = training_guidance(&profile(10, 75.0, "northstar"));

    assert_eq!(guidance.title, "Trusted company guidance");
    assert!(!guidance
        .spoken_summary()
        .to_lowercase()
        .contains("first-week"));
    assert!(!guidance.spoken_summary().to_lowercase().contains("trainer"));
}

#[test]
fn test_first_dispatch_recommendation_prefers_short_forgiving_standard_load() {
    let profile = profile(0, 75.0, "northstar");

    let short_standard = job(70.0, 8.0, "general");
    let longer_tight = job(220.0, 4.0, "electronics");

    assert!(
        training_recommendation_score(&profile, &short_standard)
            < training_recommendation_score(&profile, &longer_tight)
    );
}

#[test]
fn test_trust_building_recommendation_allows_broader_but_still_values_time_margin() {
    let profile = profile(5, 75.0, "northstar");

    let roomy_regional = job(180.0, 10.0, "general");
    let tight_short = job(90.0, 2.0, "general");

    assert!(
        training_recommendation_score(&profile, &roomy_regional)
            < training_recommendation_score(&profile, &tight_short)
    );
}

#[test]
fn test_first_dispatch_guidance_is_spoken_with_the_carrier_name() {
    let guidance = training_guidance(&profile(0, 50.0, "prairie_link"));
    assert_eq!(guidance.stage, TrainingStage::FirstDispatch);
    assert_eq!(guidance.stage.value(), "first_dispatch");
    assert_eq!(
        guidance.spoken_summary(),
        "First dispatch. Prairie Link Regional has you on real freight with trainer support \
         close by. Dispatch starts you on a short standard load with room on the appointment."
    );
    assert_eq!(guidance.recommendation_label, "trainer-recommended");
    assert!(is_company_training_profile(&profile(
        0,
        50.0,
        "prairie_link"
    )));
}
