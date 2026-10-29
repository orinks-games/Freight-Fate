//! Save-compatible company-driver training guidance (port of
//! `freight_fate/models/career_training.py`).

use crate::models::business_constants::COMPANY_DRIVER;
use crate::models::career::{carrier_name_of, CareerProfile, JobView};

#[cfg(test)]
mod tests;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TrainingStage {
    FirstDispatch,
    TrainerReminders,
    TrustOpening,
    TrustBuilding,
    NormalGuidance,
}

impl TrainingStage {
    /// The Python enum's `.value`.
    pub fn value(self) -> &'static str {
        match self {
            TrainingStage::FirstDispatch => "first_dispatch",
            TrainingStage::TrainerReminders => "trainer_reminders",
            TrainingStage::TrustOpening => "trust_opening",
            TrainingStage::TrustBuilding => "trust_building",
            TrainingStage::NormalGuidance => "normal_guidance",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrainingGuidance {
    pub stage: TrainingStage,
    pub title: String,
    pub terminal_text: String,
    pub dispatch_text: String,
    pub recommendation_label: String,
}

impl TrainingGuidance {
    fn new(
        stage: TrainingStage,
        title: &str,
        terminal_text: impl Into<String>,
        dispatch_text: impl Into<String>,
        recommendation_label: &str,
    ) -> Self {
        TrainingGuidance {
            stage,
            title: title.to_string(),
            terminal_text: terminal_text.into(),
            dispatch_text: dispatch_text.into(),
            recommendation_label: recommendation_label.to_string(),
        }
    }

    pub fn spoken_summary(&self) -> String {
        if self.dispatch_text.is_empty() {
            return format!("{}. {}", self.title, self.terminal_text);
        }
        format!(
            "{}. {} {}",
            self.title, self.terminal_text, self.dispatch_text
        )
    }
}

pub fn is_company_training_profile<P: CareerProfile + ?Sized>(profile: &P) -> bool {
    profile.business_status() == COMPANY_DRIVER
}

pub fn company_training_stage<P: CareerProfile + ?Sized>(profile: &P) -> TrainingStage {
    let deliveries = profile.career().deliveries;
    if deliveries <= 0 {
        return TrainingStage::FirstDispatch;
    }
    if deliveries < 3 {
        return TrainingStage::TrainerReminders;
    }
    if deliveries == 3 {
        return TrainingStage::TrustOpening;
    }
    if deliveries < 10 {
        return TrainingStage::TrustBuilding;
    }
    TrainingStage::NormalGuidance
}

pub fn training_guidance<P: CareerProfile + ?Sized>(profile: &P) -> TrainingGuidance {
    let stage = company_training_stage(profile);
    let carrier = carrier_name_of(profile);
    match stage {
        TrainingStage::FirstDispatch => TrainingGuidance::new(
            stage,
            "First dispatch",
            format!("{carrier} has you on real freight with trainer support close by."),
            "Dispatch starts you on a short standard load with room on the appointment.",
            "trainer-recommended",
        ),
        TrainingStage::TrainerReminders => TrainingGuidance::new(
            stage,
            "First-week service record",
            format!(
                "{carrier} is looking for steady service, not perfection, with trainer notes still close by."
            ),
            "Expect short regional freight.",
            "good first-week run",
        ),
        TrainingStage::TrustOpening => TrainingGuidance::new(
            stage,
            "Dispatch trust opening",
            format!("{carrier} has enough first-week history to widen the board."),
            "",
            "good lane to build your record",
        ),
        TrainingStage::TrustBuilding => TrainingGuidance::new(
            stage,
            "Build dispatcher trust",
            format!("{carrier} is watching on-time service, damage, and steady miles."),
            "",
            "good service-record load",
        ),
        TrainingStage::NormalGuidance => TrainingGuidance::new(
            stage,
            "Trusted company guidance",
            "Keep building seniority, clean service, endorsements, and better carrier lanes.",
            "",
            "trusted carrier lane",
        ),
    }
}

pub fn training_recommendation_score<P, J>(profile: &P, job: &J) -> f64
where
    P: CareerProfile + ?Sized,
    J: JobView + ?Sized,
{
    let stage = company_training_stage(profile);
    let miles = job.distance_mi();
    let deadline = job.deadline_game_h();
    let margin = (deadline - miles / 55.0).max(0.0);
    let cargo = job.cargo_key();
    let specialty_penalty = if matches!(cargo, "electronics" | "machinery" | "hazmat") {
        60.0
    } else {
        0.0
    };

    match stage {
        TrainingStage::FirstDispatch => miles + specialty_penalty - margin * 18.0,
        TrainingStage::TrainerReminders => miles * 0.85 + specialty_penalty * 0.5 - margin * 14.0,
        TrainingStage::TrustOpening | TrainingStage::TrustBuilding => {
            miles * 0.65 - margin * 20.0 + specialty_penalty * 0.25
        }
        TrainingStage::NormalGuidance => miles,
    }
}
