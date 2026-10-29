//! Derived career objectives for terminal and dispatch-board guidance (port
//! of `freight_fate/models/career_objectives.py`).

use crate::models::business_constants::{is_owner_operator, INDEPENDENT_AUTHORITY};
use crate::models::career::{carrier_name_of, CareerProfile};
use crate::models::career_level_guidance::career_level_guidance;
use crate::models::career_training::{training_guidance, TrainingStage};
use crate::pyfmt::{fmt_f, fmt_grouped};

#[cfg(test)]
mod tests;

// The business gates this module names, owned by `models::business`.
pub use crate::models::business::{
    AUTHORITY_ACTIVATION_LEVEL, AUTHORITY_READY_LEVEL, AUTHORITY_READY_WORKING_CAPITAL,
    OWNER_OPERATOR_DELIVERIES, OWNER_OPERATOR_LEVEL, OWNER_OPERATOR_REPUTATION,
    OWNER_OPERATOR_WORKING_CAPITAL,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CareerObjective {
    pub title: String,
    pub terminal_text: String,
    pub recommendation: String,
    pub milestone_text: String,
}

impl CareerObjective {
    fn new(
        title: impl Into<String>,
        terminal_text: impl Into<String>,
        recommendation: impl Into<String>,
    ) -> Self {
        CareerObjective {
            title: title.into(),
            terminal_text: terminal_text.into(),
            recommendation: recommendation.into(),
            milestone_text: String::new(),
        }
    }

    pub fn spoken_summary(&self) -> String {
        let milestone = if self.milestone_text.is_empty() {
            String::new()
        } else {
            format!(" {}", self.milestone_text)
        };
        format!("{}. {}{milestone}", self.title, self.terminal_text)
    }

    /// The same plan one part per line: the step, what it takes, and the
    /// milestone when there is one.
    pub fn spoken_lines(&self) -> Vec<String> {
        let mut lines = vec![format!("{}.", self.title)];
        for part in [&self.terminal_text, &self.milestone_text] {
            if !part.trim().is_empty() {
                lines.push(part.clone());
            }
        }
        lines
    }
}

/// Return the current practical career objective without changing saves.
pub fn career_objective<P: CareerProfile + ?Sized>(profile: &P) -> CareerObjective {
    let status = profile.business_status();
    if status == INDEPENDENT_AUTHORITY {
        return independent_authority_objective(profile);
    }
    if is_owner_operator(status) {
        return owner_operator_objective(profile);
    }
    company_driver_objective(profile)
}

fn company_driver_objective<P: CareerProfile + ?Sized>(profile: &P) -> CareerObjective {
    let guidance = training_guidance(profile);
    if guidance.stage != TrainingStage::NormalGuidance {
        let terminal_text = if guidance.dispatch_text.is_empty() {
            guidance.terminal_text
        } else {
            format!("{} {}", guidance.terminal_text, guidance.dispatch_text)
        };
        return CareerObjective::new(guidance.title, terminal_text, guidance.recommendation_label);
    }
    if profile.career_reputation() < 70.0 {
        return CareerObjective::new(
            "Build dispatcher trust",
            format!(
                "{} is watching on-time service, damage, and steady miles.",
                carrier_name_of(profile)
            ),
            "reliable unlocked lane",
        );
    }
    if profile.owner_operator_declined() {
        // The buy-in was offered and turned down: the plan is the company
        // career, and nothing here points back at the truck.
        return level_guidance_objective(profile);
    }
    if profile.owner_operator_eligible() {
        return CareerObjective::new(
            "Owner-operator buy-in ready",
            "The truck buy-in is open under Business status.",
            "clean company load",
        );
    }
    if profile.career().level() >= OWNER_OPERATOR_LEVEL - 3 {
        return CareerObjective::new(
            "Owner-operator preparation",
            format!(
                "Work toward level {OWNER_OPERATOR_LEVEL}, \
                 {OWNER_OPERATOR_DELIVERIES} deliveries, \
                 {} reputation, and a cash cushion.",
                fmt_f(OWNER_OPERATOR_REPUTATION, 0)
            ),
            "steady earning lane",
        );
    }
    level_guidance_objective(profile)
}

fn owner_operator_objective<P: CareerProfile + ?Sized>(profile: &P) -> CareerObjective {
    if profile.money() < OWNER_OPERATOR_WORKING_CAPITAL * 2.0 {
        return CareerObjective::new(
            "Protect working capital",
            "Fuel, maintenance, insurance, trailer programs, and truck wear come out of your cash.",
            "cash-positive load",
        );
    }
    if profile.authority_readiness_eligible() {
        return CareerObjective::new(
            "Authority prep ready",
            "Business status can set aside the reserve for your own-authority plan.",
            "reserve-safe load",
        );
    }
    if profile.career().level() >= AUTHORITY_READY_LEVEL - 2 {
        return CareerObjective::new(
            "Authority preparation",
            format!(
                "Build reputation, deliveries, and at least \
                 {} dollars in working capital.",
                fmt_grouped(AUTHORITY_READY_WORKING_CAPITAL, 0)
            ),
            "owner-operator margin load",
        );
    }
    level_guidance_objective(profile)
}

fn independent_authority_objective<P: CareerProfile + ?Sized>(profile: &P) -> CareerObjective {
    if profile.authority_activation_eligible() {
        return CareerObjective::new(
            "Authority activation ready",
            "Business status can activate your own authority.",
            "direct freight with margin",
        );
    }
    if profile.career().level() < AUTHORITY_ACTIVATION_LEVEL {
        return CareerObjective::new(
            "Grow direct freight reputation",
            format!("Activating your own authority needs level {AUTHORITY_ACTIVATION_LEVEL}."),
            "direct freight with margin",
        );
    }
    level_guidance_objective(profile)
}

fn level_guidance_objective<P: CareerProfile + ?Sized>(profile: &P) -> CareerObjective {
    let guidance = career_level_guidance(profile);
    CareerObjective {
        title: guidance.title,
        terminal_text: guidance.terminal_text,
        recommendation: guidance.recommendation,
        milestone_text: guidance.milestone_text,
    }
}
