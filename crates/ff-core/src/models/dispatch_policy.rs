//! Dispatch autonomy policy: who picks the load and who picks the route
//! (port of `freight_fate/models/dispatch_policy.py`).
//!
//! Freedom to choose freight and routing is earned across the 30-level
//! career instead of being available from minute one:
//!
//! - New company hires are *assigned* a load and a route by dispatch. Their
//!   real agency is accept or decline, and declines go on the service record.
//! - Senior company drivers pick their own loads from the board, but still
//!   run the lane dispatch gives them.
//! - Leased-on owner-operators and independent authority choose both -- that
//!   is the independence they bought into.
//!
//! The policy is a pure function of business status and career level,
//! mirroring `career_level_guidance` / `career_objective`. State code
//! consults it to decide whether to present a menu or auto-assign; it never
//! mutates saves.

use crate::models::business_constants::is_owner_operator;
use crate::models::career::CareerProfile;
use crate::models::enforcement::{band_decline_penalty, band_revokes_load_choice, standing_band};

#[cfg(test)]
mod tests;

/// Company level where dispatch starts letting the driver pick loads.
pub const SENIOR_LOAD_CHOICE_LEVEL: i64 = 8;
/// Assigned-load refusals a company driver can spend before the next level-up.
pub const NEW_HIRE_DECLINE_BUDGET: i64 = 3;
/// Regional Regulars (level 5+) have earned one more refusal per level band.
pub const REGIONAL_REGULAR_LEVEL: i64 = 5;
/// Declining an assigned load is remembered: one on-time delivery wins it back.
pub const DECLINE_REPUTATION_PENALTY: f64 = 2.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DispatchPolicy {
    pub assigns_load: bool,
    pub assigns_route: bool,
    pub decline_budget: i64,
}

/// The dispatch autonomy band for this profile, derived from saves as-is.
///
/// Autonomy is earned by level and kept by reputation. A driver dispatch has
/// stopped trusting loses the privilege of picking loads again, and loses
/// refusals with it -- the same ladder, walked backwards.
pub fn dispatch_policy<P: CareerProfile + ?Sized>(profile: &P) -> DispatchPolicy {
    if is_owner_operator(profile.business_status()) {
        return DispatchPolicy {
            assigns_load: false,
            assigns_route: false,
            decline_budget: 0,
        };
    }
    let career = profile.career();
    let level = career.level();
    // The band the dispatch trust line speaks: service, license, record and
    // debt. On service alone, a record or debt that holds the line at
    // guarded promised fewer refusals that never came (owner, 2026-09-28).
    let band = standing_band(profile);
    let mut budget = NEW_HIRE_DECLINE_BUDGET
        + if level >= REGIONAL_REGULAR_LEVEL {
            1
        } else {
            0
        };
    budget = (budget - band_decline_penalty(band)).max(0);
    DispatchPolicy {
        assigns_load: level < SENIOR_LOAD_CHOICE_LEVEL || band_revokes_load_choice(band),
        assigns_route: true,
        decline_budget: budget,
    }
}

/// Assigned-load refusals left before dispatch stops offering alternatives.
pub fn declines_remaining<P: CareerProfile + ?Sized>(profile: &P) -> i64 {
    let budget = dispatch_policy(profile).decline_budget;
    let used = profile.career().dispatch_declines_used;
    (budget - used).max(0)
}
