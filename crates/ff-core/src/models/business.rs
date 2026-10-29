//! Career business status and settlement economics (port of
//! `freight_fate/models/business.py`).
//!
//! Freight Fate keeps the business arc playable rather than fully
//! accounting-like: the player starts as a company driver, then can buy into a
//! leased-on owner-operator track once the 30-level career ladder has enough
//! reputation, cash, and miles behind it.

use crate::models::career::CareerProfile;
use crate::models::career_ladder::{
    display_rank_for_level, next_company_rank_for_level, next_rank_for_level, rank_for_level,
    STARTER_CARRIER_NAME,
};
use crate::models::enforcement::StandingProfile;
use crate::models::jobs::Job;
use crate::models::solvency::debt_line;
use crate::models::start_options::{
    option_for_profile, pay_plan_for_key, START_MODE_OWNER_OPERATOR,
};
use crate::models::trailers::{owned_trailer_charge_per_mile, trailer_program_charge_per_mile};
use crate::pyfmt::{fmt_f, fmt_grouped, round_py_n};

// The status keys and `is_owner_operator` are hoisted into
// `business_constants` so enforcement and solvency can read them without a
// cycle; re-exported here so callers keep the Python spelling.
pub use crate::models::business_constants::{
    is_owner_operator, COMPANY_DRIVER, DIRECT_FREIGHT_PAY_MULT, INDEPENDENT_AUTHORITY,
    LEASED_OWNER_OPERATOR,
};

#[cfg(test)]
mod tests;

pub const OWNER_OPERATOR_PREP_CHECKLIST_LEVEL: i64 = 14;
pub const OWNER_OPERATOR_LEVEL: i64 = 18;
pub const OWNER_OPERATOR_REPUTATION: f64 = 80.0;
pub const OWNER_OPERATOR_DELIVERIES: i64 = 35;
pub const OWNER_OPERATOR_BUY_IN: f64 = 35_000.0;
pub const OWNER_OPERATOR_WORKING_CAPITAL: f64 = 10_000.0;
pub const OWNER_OPERATOR_REVENUE_MULT: f64 = 1.12;
pub const AUTHORITY_READY_LEVEL: i64 = 21;
pub const AUTHORITY_READY_DELIVERIES: i64 = 60;
pub const AUTHORITY_READY_REPUTATION: f64 = 90.0;
pub const AUTHORITY_READY_RESERVE: f64 = 12_500.0;
pub const AUTHORITY_READY_WORKING_CAPITAL: f64 = 25_000.0;
pub const AUTHORITY_ACTIVATION_DELIVERIES: i64 = 75;
pub const AUTHORITY_ACTIVATION_LEVEL: i64 = 25;
pub const AUTHORITY_ACTIVATION_REPUTATION: f64 = 92.0;
pub const AUTHORITY_ACTIVATION_COST: f64 = 15_000.0;
pub const AUTHORITY_ACTIVATION_WORKING_CAPITAL: f64 = 35_000.0;

pub const OWNER_MAINTENANCE_PER_MILE: f64 = 0.18;
pub const OWNER_INSURANCE_PER_MILE: f64 = 0.09;
pub const OWNER_TRUCK_PAYMENT_PER_MILE: f64 = 0.22;
pub const OWNER_SETTLEMENT_FEE_SHARE: f64 = 0.02;
pub const AUTHORITY_INSURANCE_PER_MILE: f64 = 0.14;
pub const AUTHORITY_COMPLIANCE_PER_MILE: f64 = 0.06;
pub const AUTHORITY_FACTORING_FEE_SHARE: f64 = 0.035;

// A PrePass-style weigh-in-motion transponder: the fleet's own equipment, so
// a company driver gets one issued once dispatch trusts them with it, the
// same shape as an endorsement the carrier sponsors at a level. An
// owner-operator has no fleet behind them and buys the subscription
// themselves, carried as a per-mile settlement reserve like every other
// owner-operator recurring cost in this file (trailer program, insurance).
pub const WEIGH_STATION_TRANSPONDER_LEVEL: i64 = 4;
// A real PrePass lease deposit plus activation runs in this neighborhood; an
// assumed startup cost, not a measured one, sized against the trailer
// program's own lease deposits above.
pub const WEIGH_STATION_TRANSPONDER_SIGNUP_FEE: f64 = 180.0;
pub const WEIGH_STATION_TRANSPONDER_PER_MILE: f64 = 0.015;

/// What the business gates read off a `Profile` beyond the career-side
/// views: the owner-operator flags and the pay advance. `Profile` implements
/// this; the tests' fakes carry the same fields.
pub trait BusinessProfile: CareerProfile {
    /// `profile.authority_readiness`.
    fn authority_readiness(&self) -> bool;
    /// `profile.weigh_station_transponder`.
    fn weigh_station_transponder(&self) -> bool;
    /// `profile.pay_advance`.
    fn pay_advance(&self) -> f64;
    /// `profile.start_mode`.
    fn start_mode(&self) -> &str;
    /// `profile.active_trailer_programs()`.
    fn active_trailer_programs(&self) -> Vec<String>;
    /// Whether the CDL is valid right now: no live suspension, no lifetime
    /// disqualification. A lessor does not put a driver who cannot legally
    /// drive into a truck, so the buy-in waits for it.
    fn cdl_clear(&self) -> bool {
        true
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct BusinessCharge {
    pub label: &'static str,
    pub amount: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BusinessSettlement {
    pub status: String,
    pub status_label: &'static str,
    pub gross_pay: f64,
    pub driver_charges: f64,
    pub business_charges: Vec<BusinessCharge>,
    pub net_before_advance: f64,
    // Fines this load could not cover. Net pay floors at zero, but the money
    // is not forgiven -- saying "this cost you 400 dollars" while quietly
    // writing off 250 of it told the player something that did not happen.
    pub uncollected_charges: f64,
}

impl BusinessSettlement {
    pub fn business_charge_total(&self) -> f64 {
        round_py_n(
            self.business_charges.iter().map(|c| c.amount).sum::<f64>(),
            2,
        )
    }

    pub fn business_charge_summary(&self) -> String {
        if self.business_charges.is_empty() {
            return "none".to_string();
        }
        self.business_charges
            .iter()
            .map(|charge| format!("{} {} dollars", charge.label, fmt_grouped(charge.amount, 0)))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

pub fn has_independent_authority<P: StandingProfile + ?Sized>(profile: &P) -> bool {
    profile.business_status() == INDEPENDENT_AUTHORITY
}

pub fn status_label(status: &str) -> &'static str {
    if status == INDEPENDENT_AUTHORITY {
        return "own authority";
    }
    if is_owner_operator(status) {
        return "leased-on owner-operator";
    }
    "company driver"
}

pub fn pay_label(status: &str) -> &'static str {
    if status == INDEPENDENT_AUTHORITY {
        return "Direct gross";
    }
    if status == LEASED_OWNER_OPERATOR {
        return "Gross revenue";
    }
    "Carrier gross"
}

pub fn player_pays_operating_costs(status: &str) -> bool {
    is_owner_operator(status)
}

/// `business.carrier_name(profile)`: the carrier on the profile, or the
/// starter carrier when the profile carries none.
pub fn carrier_name<P: StandingProfile + ?Sized>(profile: &P) -> String {
    let name = profile.carrier_name();
    if name.is_empty() {
        STARTER_CARRIER_NAME.to_string()
    } else {
        name.to_string()
    }
}

pub fn carrier_key<P: StandingProfile + ?Sized>(profile: &P) -> String {
    profile.carrier_key().to_string()
}

pub fn has_authority_readiness<P: BusinessProfile + ?Sized>(profile: &P) -> bool {
    profile.authority_readiness()
}

/// Whether this driver's cab carries a weigh-in-motion bypass transponder.
///
/// A company driver's fleet issues one once dispatch trusts them with it --
/// no purchase, just the level gate, matching how `ENDORSEMENT_LEVELS`
/// grants carrier-sponsored training. An owner-operator has no fleet paying
/// their bills and only has one if they bought the subscription themselves.
pub fn has_weigh_station_transponder<P: BusinessProfile + ?Sized>(profile: &P) -> bool {
    if is_owner_operator(profile.business_status()) {
        return profile.weigh_station_transponder();
    }
    profile.career().level() >= WEIGH_STATION_TRANSPONDER_LEVEL
}

/// Whether an owner-operator can subscribe to the transponder now.
pub fn weigh_station_transponder_eligibility<P: BusinessProfile + ?Sized>(
    profile: &P,
) -> (bool, Vec<String>) {
    if !is_owner_operator(profile.business_status()) {
        return (
            false,
            vec![format!(
                "Company drivers get a fleet transponder free once dispatch trusts them with it, at level {WEIGH_STATION_TRANSPONDER_LEVEL}."
            )],
        );
    }
    if has_weigh_station_transponder(profile) {
        return (
            false,
            vec!["The weigh station transponder subscription is already active.".to_string()],
        );
    }
    if profile.money() < WEIGH_STATION_TRANSPONDER_SIGNUP_FEE {
        return (
            false,
            vec![format!(
                "Have {} dollars first for the transponder lease and activation.",
                fmt_grouped(WEIGH_STATION_TRANSPONDER_SIGNUP_FEE, 0)
            )],
        );
    }
    (true, Vec::new())
}

/// Whether an owner-operator can set aside an authority-readiness reserve.
pub fn authority_readiness_eligibility<P: BusinessProfile + ?Sized>(
    profile: &P,
) -> (bool, Vec<String>) {
    if has_authority_readiness(profile) {
        return (
            false,
            vec!["Authority readiness reserve is already set.".to_string()],
        );
    }
    if !is_owner_operator(profile.business_status()) {
        return (
            false,
            vec!["Become a leased-on owner-operator first.".to_string()],
        );
    }
    let career = profile.career();
    let mut reasons: Vec<String> = Vec::new();
    if career.level() < AUTHORITY_READY_LEVEL {
        let rank = rank_for_level(AUTHORITY_READY_LEVEL);
        reasons.push(format!(
            "Reach level {AUTHORITY_READY_LEVEL}: {}.",
            rank.title
        ));
    }
    if career.deliveries < AUTHORITY_READY_DELIVERIES {
        reasons.push(format!("Complete {AUTHORITY_READY_DELIVERIES} deliveries."));
    }
    if profile.career_reputation() < AUTHORITY_READY_REPUTATION {
        reasons.push(format!(
            "Build reputation to {}.",
            fmt_f(AUTHORITY_READY_REPUTATION, 0)
        ));
    }
    let needed_cash = AUTHORITY_READY_RESERVE + AUTHORITY_READY_WORKING_CAPITAL;
    if profile.money() < needed_cash {
        reasons.push(format!(
            "Have {} dollars first: {} for the reserve plus {} working capital.",
            fmt_grouped(needed_cash, 0),
            fmt_grouped(AUTHORITY_READY_RESERVE, 0),
            fmt_grouped(AUTHORITY_READY_WORKING_CAPITAL, 0)
        ));
    }
    if profile.pay_advance() >= 1.0 {
        reasons.push("Pay off your dispatcher advance.".to_string());
    }
    (reasons.is_empty(), reasons)
}

/// Whether a prepared owner-operator can activate own authority.
pub fn authority_activation_eligibility<P: BusinessProfile + ?Sized>(
    profile: &P,
) -> (bool, Vec<String>) {
    if has_independent_authority(profile) {
        return (false, vec!["Own authority is already active.".to_string()]);
    }
    if !is_owner_operator(profile.business_status()) {
        return (
            false,
            vec!["Become a leased-on owner-operator first.".to_string()],
        );
    }
    let career = profile.career();
    let mut reasons: Vec<String> = Vec::new();
    if !has_authority_readiness(profile) {
        reasons.push("Set the authority prep reserve first.".to_string());
    }
    if career.level() < AUTHORITY_ACTIVATION_LEVEL {
        let rank = rank_for_level(AUTHORITY_ACTIVATION_LEVEL);
        reasons.push(format!(
            "Reach level {AUTHORITY_ACTIVATION_LEVEL}: {}.",
            rank.title
        ));
    }
    if career.deliveries < AUTHORITY_ACTIVATION_DELIVERIES {
        reasons.push(format!(
            "Complete {AUTHORITY_ACTIVATION_DELIVERIES} deliveries."
        ));
    }
    if profile.career_reputation() < AUTHORITY_ACTIVATION_REPUTATION {
        reasons.push(format!(
            "Build reputation to {}.",
            fmt_f(AUTHORITY_ACTIVATION_REPUTATION, 0)
        ));
    }
    let specialty = profile
        .active_trailer_programs()
        .iter()
        .any(|program| program != "dry_van");
    if !specialty {
        reasons.push("Add at least one specialty trailer program.".to_string());
    }
    let needed_cash = AUTHORITY_ACTIVATION_COST + AUTHORITY_ACTIVATION_WORKING_CAPITAL;
    if profile.money() < needed_cash {
        reasons.push(format!(
            "Have {} dollars first: {} for authority startup plus {} working capital.",
            fmt_grouped(needed_cash, 0),
            fmt_grouped(AUTHORITY_ACTIVATION_COST, 0),
            fmt_grouped(AUTHORITY_ACTIVATION_WORKING_CAPITAL, 0)
        ));
    }
    if profile.pay_advance() >= 1.0 {
        reasons.push("Pay off your dispatcher advance.".to_string());
    }
    (reasons.is_empty(), reasons)
}

/// Whether the profile can buy into owner-operator status now.
pub fn owner_operator_eligibility<P: BusinessProfile + ?Sized>(profile: &P) -> (bool, Vec<String>) {
    if is_owner_operator(profile.business_status()) {
        return (
            false,
            vec!["You are already running as an owner-operator.".to_string()],
        );
    }
    let career = profile.career();
    let mut reasons: Vec<String> = Vec::new();
    if !profile.cdl_clear() {
        reasons.push(
            "Hold a clear CDL: no lessor puts a suspended or disqualified driver in a truck."
                .to_string(),
        );
    }
    if career.level() < OWNER_OPERATOR_LEVEL {
        let rank = rank_for_level(OWNER_OPERATOR_LEVEL);
        reasons.push(format!(
            "Reach level {OWNER_OPERATOR_LEVEL}: {}.",
            rank.title
        ));
    }
    if career.deliveries < OWNER_OPERATOR_DELIVERIES {
        reasons.push(format!("Complete {OWNER_OPERATOR_DELIVERIES} deliveries."));
    }
    if profile.career_reputation() < OWNER_OPERATOR_REPUTATION {
        reasons.push(format!(
            "Build reputation to {}.",
            fmt_f(OWNER_OPERATOR_REPUTATION, 0)
        ));
    }
    let needed_cash = OWNER_OPERATOR_BUY_IN + OWNER_OPERATOR_WORKING_CAPITAL;
    if profile.money() < needed_cash {
        reasons.push(format!(
            "Save {} dollars: {} for the truck buy-in and {} for working capital.",
            fmt_grouped(needed_cash, 0),
            fmt_grouped(OWNER_OPERATOR_BUY_IN, 0),
            fmt_grouped(OWNER_OPERATOR_WORKING_CAPITAL, 0)
        ));
    }
    if profile.pay_advance() >= 1.0 {
        reasons.push("Pay off your dispatcher advance.".to_string());
    }
    (reasons.is_empty(), reasons)
}

pub fn business_path_label<P: BusinessProfile + ?Sized>(profile: &P) -> String {
    let rank = display_rank_for(profile);
    let option = option_for_profile(profile);
    format!(
        "{}. Level {}: {}. {}. {}",
        carrier_name(profile),
        rank.level,
        rank.title,
        rank.stage,
        option.menu_summary
    )
}

pub fn next_business_unlock<P: BusinessProfile + ?Sized>(profile: &P) -> String {
    let status = profile.business_status();
    if status == INDEPENDENT_AUTHORITY {
        return "Own authority active. Direct freight is available on the dispatch \
                board, with insurance, compliance, and factoring costs in settlement."
            .to_string();
    }
    let level = profile.career().level();
    if is_owner_operator(status) {
        if has_authority_readiness(profile) {
            let (ok, reasons) = authority_activation_eligibility(profile);
            if ok {
                return "Next: activate own authority from Business status.".to_string();
            }
            return format!("Own authority locked: {}", reasons.join(" "));
        }
        let (ok, reasons) = authority_readiness_eligibility(profile);
        if ok {
            return "Next: set aside an authority prep reserve from Business status.".to_string();
        }
        if level >= AUTHORITY_READY_LEVEL {
            return format!("Authority prep locked: {}", reasons.join(" "));
        }
        return match next_rank_for_level(level) {
            None => "You are at the top career rank.".to_string(),
            Some(next_rank) => format!(
                "Next: level {}, {}. {}",
                next_rank.level, next_rank.title, next_rank.unlock
            ),
        };
    }

    let (ok, reasons) = owner_operator_eligibility(profile);
    if ok && profile.owner_operator_declined() {
        // The driver said no. The plan reads on down the ladder, and the
        // buy-in is mentioned as a door, never as the next step.
        return format!("Company driver by choice. {}", company_ladder_next(level));
    }
    if ok {
        return "Next: the owner-operator buy-in under Business status, where you buy the truck you drive."
            .to_string();
    }
    // The Business Prep Driver rank (14) starts reading the real checklist,
    // matching the ladder's "owner-operator checklist starts to matter."
    if level < OWNER_OPERATOR_PREP_CHECKLIST_LEVEL {
        return match next_rank_for_level(level) {
            None => "You are at the top career rank.".to_string(),
            Some(next_rank) => format!(
                "Next: level {}, {}. {}",
                next_rank.level, next_rank.title, next_rank.unlock
            ),
        };
    }
    format!("Owner-operator gate locked: {}", reasons.join(" "))
}

/// The next rung of the ladder read as a company career: what a driver who
/// turned the buy-in down hears in place of the offer.
fn company_ladder_next(level: i64) -> String {
    match next_company_rank_for_level(level) {
        None => "You are at the top career rank. The owner-operator buy-in stays open under \
                 Business status if you ever want it."
            .to_string(),
        Some(next_rank) => format!(
            "Next: level {}, {}. {}",
            next_rank.level, next_rank.title, next_rank.unlock
        ),
    }
}

/// Rank title/stage for spoken Business status and path labels.
pub fn display_rank_for<P: CareerProfile + ?Sized>(
    profile: &P,
) -> &'static crate::models::career_ladder::CareerRank {
    display_rank_for_level(
        profile.career().level(),
        profile.business_status(),
        profile.owner_operator_declined(),
    )
}

pub fn uses_company_career_ranks_for<P: CareerProfile + ?Sized>(profile: &P) -> bool {
    crate::models::career_ladder::uses_company_career_ranks(
        profile.career().level(),
        profile.business_status(),
        profile.owner_operator_declined(),
    )
}

pub fn business_status_summary<P: BusinessProfile + ?Sized>(profile: &P) -> String {
    // What is owed and the ceiling on it belong on the screen that explains
    // the business, and have to be askable at any time rather than only when
    // a settlement brings them up.
    let owed = debt_line(profile);
    let summary = business_status_summary_inner(profile);
    if owed.is_empty() {
        summary
    } else {
        format!("{summary} {owed}")
    }
}

fn business_status_summary_inner<P: BusinessProfile + ?Sized>(profile: &P) -> String {
    let status = profile.business_status();
    let rank = display_rank_for(profile);
    if is_owner_operator(status) {
        let transponder = if has_weigh_station_transponder(profile) {
            "Weigh station transponder subscription is active. "
        } else {
            ""
        };
        if status == INDEPENDENT_AUTHORITY {
            return format!(
                "Own authority active. Level {}: {}. Direct freight pays higher gross. You pay fuel, repairs, insurance, trailer reserve, truck reserve, compliance reserve, and factoring costs. {transponder}{}",
                rank.level,
                rank.title,
                next_business_unlock(profile)
            );
        }
        let lead = if profile.start_mode() == START_MODE_OWNER_OPERATOR {
            "You chose the owner-operator start. "
        } else {
            ""
        };
        let readiness = if has_authority_readiness(profile) {
            "Authority prep reserve is set. "
        } else {
            ""
        };
        return format!(
            "{lead}Leased to {}. Level {}: {}. Gross revenue is higher. You pay fuel, repairs, maintenance reserve, insurance, trailer program, truck reserve, and settlement fees. {readiness}{transponder}{}",
            carrier_name(profile),
            rank.level,
            rank.title,
            next_business_unlock(profile)
        );
    }
    let (ok, _reasons) = owner_operator_eligibility(profile);
    if ok && profile.owner_operator_declined() {
        return format!(
            "You are a company driver for {} by choice, level {} {}. The carrier supplies the tractor, fuel, repairs, trailer, authority, and insurance. Settlements are driver wages and bonuses. The owner-operator buy-in stays open here if you change your mind. {}",
            carrier_name(profile),
            rank.level,
            rank.title,
            company_ladder_next(profile.career().level())
        );
    }
    if ok {
        return format!(
            "You are a company driver for {}, level {} {}. You qualify for the owner-operator buy-in: you buy the truck you drive and keep hauling for the carrier as a leased-on owner-operator. The buy-in costs {} dollars and keeps {} dollars of working capital in the bank.",
            carrier_name(profile),
            rank.level,
            rank.title,
            fmt_grouped(OWNER_OPERATOR_BUY_IN, 0),
            fmt_grouped(OWNER_OPERATOR_WORKING_CAPITAL, 0)
        );
    }
    format!(
        "Company driver for {}. Level {}: {}. {} The carrier supplies the tractor, fuel, repairs, trailer, authority, and insurance. Settlements are driver wages and bonuses. {}",
        carrier_name(profile),
        rank.level,
        rank.title,
        option_for_profile(profile).menu_summary,
        next_business_unlock(profile)
    )
}

// Dispatch trust pays continuously, not only at business gates: a driver at
// reputation 100 earns this share of gross on top of the wage plan, scaling
// down to nothing at the 50-point starting reputation.
pub const REPUTATION_BONUS_MAX_SHARE: f64 = 0.06;

/// Extra company pay for dispatch trust above the starting reputation.
pub fn reputation_pay_bonus(gross_pay: f64, reputation: Option<f64>) -> f64 {
    let Some(reputation) = reputation else {
        return 0.0;
    };
    let trust = ((reputation - 50.0) / 50.0).clamp(0.0, 1.0);
    round_py_n(gross_pay * REPUTATION_BONUS_MAX_SHARE * trust, 2)
}

pub fn company_driver_pay(
    job: &Job,
    gross_pay: f64,
    on_time: bool,
    carrier_key_value: Option<&str>,
    reputation: Option<f64>,
) -> f64 {
    let plan = pay_plan_for_key(carrier_key_value);
    let wage_floor = plan.stop_pay + job.distance_mi * plan.min_per_mile;
    let wage_share = gross_pay * plan.pay_share;
    let mut bonus = if on_time {
        gross_pay * plan.on_time_bonus_share
    } else {
        0.0
    };
    bonus += reputation_pay_bonus(gross_pay, reputation);
    round_py_n(wage_floor.max(wage_share) + bonus, 2)
}

pub fn owner_operator_gross(gross_pay: f64) -> f64 {
    round_py_n(gross_pay * OWNER_OPERATOR_REVENUE_MULT, 2)
}

pub fn direct_freight_gross(gross_pay: f64) -> f64 {
    round_py_n(gross_pay, 2)
}

pub fn owner_operator_charges(
    job: &Job,
    gross_pay: f64,
    transponder: bool,
    record_surcharge: f64,
) -> Vec<BusinessCharge> {
    let miles = job.distance_mi;
    let mut charges = vec![
        BusinessCharge {
            label: "maintenance reserve",
            amount: round_py_n(miles * OWNER_MAINTENANCE_PER_MILE, 2),
        },
        insurance_charge(miles, OWNER_INSURANCE_PER_MILE, record_surcharge),
        BusinessCharge {
            label: "trailer program",
            amount: round_py_n(miles * trailer_program_charge_per_mile(job.cargo.key), 2),
        },
        BusinessCharge {
            label: "truck payment reserve",
            amount: round_py_n(miles * OWNER_TRUCK_PAYMENT_PER_MILE, 2),
        },
        BusinessCharge {
            label: "settlement service fee",
            amount: round_py_n(gross_pay * OWNER_SETTLEMENT_FEE_SHARE, 2),
        },
    ];
    if transponder {
        charges.push(BusinessCharge {
            label: "weigh station transponder subscription",
            amount: round_py_n(miles * WEIGH_STATION_TRANSPONDER_PER_MILE, 2),
        });
    }
    charges
}

pub fn independent_authority_charges(job: &Job, gross_pay: f64) -> Vec<BusinessCharge> {
    independent_authority_charges_for_trailers::<&str>(job, gross_pay, &[], false, 1.0)
}

pub fn independent_authority_charges_for_trailers<S: AsRef<str>>(
    job: &Job,
    gross_pay: f64,
    owned_trailers: &[S],
    transponder: bool,
    record_surcharge: f64,
) -> Vec<BusinessCharge> {
    let miles = job.distance_mi;
    let owned_trailer_charge = owned_trailer_charge_per_mile(job.cargo.key, owned_trailers);
    let trailer_charge = match owned_trailer_charge {
        None => BusinessCharge {
            label: "trailer program",
            amount: round_py_n(miles * trailer_program_charge_per_mile(job.cargo.key), 2),
        },
        Some(charge) => BusinessCharge {
            label: "owned trailer reserve",
            amount: round_py_n(miles * charge, 2),
        },
    };
    let mut charges = vec![
        BusinessCharge {
            label: "maintenance reserve",
            amount: round_py_n(miles * OWNER_MAINTENANCE_PER_MILE, 2),
        },
        insurance_charge(miles, AUTHORITY_INSURANCE_PER_MILE, record_surcharge),
        trailer_charge,
        BusinessCharge {
            label: "truck payment reserve",
            amount: round_py_n(miles * OWNER_TRUCK_PAYMENT_PER_MILE, 2),
        },
        BusinessCharge {
            label: "authority compliance reserve",
            amount: round_py_n(miles * AUTHORITY_COMPLIANCE_PER_MILE, 2),
        },
        BusinessCharge {
            label: "factoring fee",
            amount: round_py_n(gross_pay * AUTHORITY_FACTORING_FEE_SHARE, 2),
        },
    ];
    if transponder {
        charges.push(BusinessCharge {
            label: "weigh station transponder subscription",
            amount: round_py_n(miles * WEIGH_STATION_TRANSPONDER_PER_MILE, 2),
        });
    }
    charges
}

/// How much of the driver's fines this load could not pay.
///
/// Fines are the last thing the settlement covers, so a shortfall eats into
/// them first. Whatever is left stands as an outstanding balance instead of
/// quietly disappearing when net pay floors at zero.
fn uncollected(driver_charges: f64, raw_net: f64) -> f64 {
    if raw_net >= 0.0 {
        return 0.0;
    }
    round_py_n(driver_charges.min(-raw_net), 2)
}

/// The keyword arguments of `build_business_settlement`, each with its
/// Python default.
#[derive(Debug, Clone)]
pub struct SettlementTerms<'a> {
    pub carrier_key: Option<&'a str>,
    pub owned_trailers: &'a [&'a str],
    pub reputation: Option<f64>,
    pub transponder: bool,
    /// The insurer's multiplier on the insurance reserve for the driver's
    /// record (`enforcement::record_insurance_surcharge`); 1.0 is a clean
    /// window, and a company driver is always 1.0.
    pub record_surcharge: f64,
}

impl Default for SettlementTerms<'_> {
    fn default() -> Self {
        Self {
            carrier_key: None,
            owned_trailers: &[],
            reputation: None,
            transponder: false,
            record_surcharge: 1.0,
        }
    }
}

/// The insurance reserve line: the clean per-mile rate, or the surcharged
/// one under a label that says why, so the settlement readout names the
/// record every time it costs money.
fn insurance_charge(miles: f64, per_mile: f64, record_surcharge: f64) -> BusinessCharge {
    if record_surcharge > 1.0 {
        BusinessCharge {
            label: "insurance reserve, surcharged for your driving record",
            amount: round_py_n(miles * per_mile * record_surcharge, 2),
        }
    } else {
        BusinessCharge {
            label: "insurance reserve",
            amount: round_py_n(miles * per_mile, 2),
        }
    }
}

/// `build_business_settlement(status, job, gross_pay, on_time=,
/// driver_charges=, ...)`.
pub fn build_business_settlement(
    status: &str,
    job: &Job,
    gross_pay: f64,
    on_time: bool,
    driver_charges: f64,
    terms: &SettlementTerms<'_>,
) -> BusinessSettlement {
    if status == INDEPENDENT_AUTHORITY {
        let gross_pay = direct_freight_gross(gross_pay);
        let charges = independent_authority_charges_for_trailers(
            job,
            gross_pay,
            terms.owned_trailers,
            terms.transponder,
            terms.record_surcharge,
        );
        let raw = gross_pay - driver_charges - charges.iter().map(|c| c.amount).sum::<f64>();
        return BusinessSettlement {
            status: status.to_string(),
            status_label: status_label(status),
            gross_pay: round_py_n(gross_pay, 2),
            driver_charges,
            business_charges: charges,
            net_before_advance: round_py_n(raw.max(0.0), 2),
            uncollected_charges: uncollected(driver_charges, raw),
        };
    }
    if is_owner_operator(status) {
        let gross_pay = owner_operator_gross(gross_pay);
        let charges =
            owner_operator_charges(job, gross_pay, terms.transponder, terms.record_surcharge);
        let raw = gross_pay - driver_charges - charges.iter().map(|c| c.amount).sum::<f64>();
        return BusinessSettlement {
            status: status.to_string(),
            status_label: status_label(status),
            gross_pay: round_py_n(gross_pay, 2),
            driver_charges,
            business_charges: charges,
            net_before_advance: round_py_n(raw.max(0.0), 2),
            uncollected_charges: uncollected(driver_charges, raw),
        };
    }

    let raw = company_driver_pay(job, gross_pay, on_time, terms.carrier_key, terms.reputation)
        - driver_charges;
    BusinessSettlement {
        status: COMPANY_DRIVER.to_string(),
        status_label: status_label(COMPANY_DRIVER),
        gross_pay: round_py_n(gross_pay, 2),
        driver_charges,
        business_charges: Vec::new(),
        net_before_advance: round_py_n(raw.max(0.0), 2),
        uncollected_charges: uncollected(driver_charges, raw),
    }
}

/// `build_business_settlement(status, job, gross_pay, on_time=, driver_charges=)`
/// with every other keyword at its default.
pub fn build_business_settlement_basic(
    status: &str,
    job: &Job,
    gross_pay: f64,
    on_time: bool,
    driver_charges: f64,
) -> BusinessSettlement {
    build_business_settlement(
        status,
        job,
        gross_pay,
        on_time,
        driver_charges,
        &SettlementTerms::default(),
    )
}
