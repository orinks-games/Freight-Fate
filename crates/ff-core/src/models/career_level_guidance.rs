//! Save-compatible level-band guidance for the 30-level career ladder (port
//! of `freight_fate/models/career_level_guidance.py`).

use crate::models::business::OWNER_OPERATOR_LEVEL;
use crate::models::business_constants::{is_owner_operator, INDEPENDENT_AUTHORITY};
use crate::models::career::CareerProfile;
use crate::models::dispatch_policy::{dispatch_policy, SENIOR_LOAD_CHOICE_LEVEL};

#[cfg(test)]
mod tests;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CareerLevelGuidance {
    pub title: String,
    pub terminal_text: String,
    pub recommendation: String,
    pub milestone_text: String,
}

impl CareerLevelGuidance {
    fn new(
        title: &str,
        terminal_text: impl Into<String>,
        recommendation: &str,
        milestone_text: &str,
    ) -> Self {
        CareerLevelGuidance {
            title: title.to_string(),
            terminal_text: terminal_text.into(),
            recommendation: recommendation.to_string(),
            milestone_text: milestone_text.to_string(),
        }
    }

    pub fn spoken_summary(&self) -> String {
        format!("{}. {}", self.title, self.terminal_text)
    }
}

pub fn career_level_guidance<P: CareerProfile + ?Sized>(profile: &P) -> CareerLevelGuidance {
    let level = profile.career().level();
    let status = profile.business_status();

    if status == INDEPENDENT_AUTHORITY {
        if level >= 30 {
            return CareerLevelGuidance::new(
                "Run the business you built",
                "The top of the ladder: your name, your authority, and the freight you choose to keep.",
                "signature freight",
                "",
            );
        }
        if level >= 25 {
            return CareerLevelGuidance::new(
                "Grow a freight business",
                "Use direct customer trust, trailer fit, and cash reserves to shape the freight you want.",
                "direct freight with margin",
                "",
            );
        }
        return CareerLevelGuidance::new(
            "Prove independent authority",
            "Use authority carefully while direct-freight reputation and reserves are still forming.",
            "authority-building freight",
            "",
        );
    }
    if is_owner_operator(status) && level >= 30 {
        return CareerLevelGuidance::new(
            "Top of the leased-on ladder",
            "You reached level 30 as a leased-on owner-operator; own authority remains the one step left.",
            "proven owner-operator lane",
            "",
        );
    }
    if is_owner_operator(status) && level >= 21 {
        return CareerLevelGuidance::new(
            "Build authority readiness",
            "Keep working capital, trailer strategy, and direct-freight readiness in view.",
            "authority-readiness lane",
            "",
        );
    }
    if is_owner_operator(status) {
        return CareerLevelGuidance::new(
            "Protect owner-operator margin",
            "Fuel, maintenance, and trailer costs come out of every load.",
            "reserve-safe owner-operator freight",
            "",
        );
    }
    if level >= OWNER_OPERATOR_LEVEL && profile.owner_operator_declined() {
        return CareerLevelGuidance::new(
            "Company driver by choice",
            "You chose to stay on carrier equipment; the buy-in stays open under Business \
             status if you ever want it.",
            "top-seniority company lane",
            "",
        );
    }
    if level >= 30 {
        return CareerLevelGuidance::new(
            "Career company driver, by choice",
            "Level 30, the top rank. The buy-in stays open under Business status.",
            "top-seniority company lane",
            "",
        );
    }
    if level >= 25 {
        return CareerLevelGuidance::new(
            "Plan the next business step",
            "You have senior company experience; use it to compare buy-in, lease, and authority timing.",
            "business-decision lane",
            "",
        );
    }
    if level >= 21 {
        return CareerLevelGuidance::new(
            "Build authority readiness",
            "Keep savings, trailer knowledge, and direct-freight readiness in view before authority is real.",
            "authority-readiness lane",
            "",
        );
    }
    if level >= 18 {
        return CareerLevelGuidance::new(
            "Protect owner-operator readiness",
            "The TWIC port card is open now, and LCV training once you hold the doubles \
             endorsement with a clean record.",
            "reserve-building freight",
            "",
        );
    }
    if level >= 14 {
        return CareerLevelGuidance::new(
            "Prepare for owner-operator risk",
            "Build reputation, deliveries, and a cash cushion before taking on truck costs.",
            "business-prep load",
            "",
        );
    }
    if level >= SENIOR_LOAD_CHOICE_LEVEL && dispatch_policy(profile).assigns_load {
        return CareerLevelGuidance::new(
            "Earn back your load choice",
            "Dispatch is assigning your loads because your standing has slipped.",
            "standing-repair lane",
            "Load choice returns when your service, license, record and debt are back in good standing.",
        );
    }
    if level >= 10 {
        return CareerLevelGuidance::new(
            "Run like a senior company driver",
            "Premium lanes and specialized freight are open. Under Licenses and training, the \
             doubles endorsement opens twin-trailer freight, and the hazmat background check \
             clears while you keep driving.",
            "senior company lane",
            "",
        );
    }
    if level >= SENIOR_LOAD_CHOICE_LEVEL {
        return CareerLevelGuidance::new(
            "Choose your own freight",
            "Dispatch now trusts you to pick loads from the board; routing \
             stays assigned until you run your own truck.",
            "self-picked reliable lane",
            "",
        );
    }
    if level >= 4 {
        return CareerLevelGuidance::new(
            "Build a regional service record",
            format!(
                "Broader company lanes are opening. Dispatch assigns your loads until level \
                 {SENIOR_LOAD_CHOICE_LEVEL}."
            ),
            "reputation-building lane",
            "",
        );
    }
    CareerLevelGuidance::new(
        "Build first-week trust",
        "Use trainer support and safer freight to start a clean service record.",
        "good first-week run",
        "",
    )
}
