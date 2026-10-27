//! Settlement accounting for carrier-paid and driver-responsibility charges
//! (port of `freight_fate/models/settlement.py`).

use crate::models::jobs::Job;
use crate::models::trailer_yard::{detention_charge, pickup_plan, DetentionCharge, TrailerOwner};
use crate::pyfmt::fmt_grouped;

pub const CARRIER_PAID: &str = "carrier_paid";

pub const LUMPER_DESTINATION_TYPES: &[&str] = &[
    "cold_storage",
    "distribution",
    "food_terminal",
    "grocery_retail_dc",
    "retail_distribution",
];

pub const WASHOUT_CARGO: &[&str] = &["food", "refrigerated", "grain"];

/// A charge shown on the settlement ledger.
#[derive(Debug, Clone, PartialEq)]
pub struct SettlementCharge {
    pub key: String,
    pub label: String,
    pub amount: f64,
    pub responsibility: String,
    pub note: String,
}

impl SettlementCharge {
    pub fn new(key: &str, label: &str, amount: f64, responsibility: &str, note: &str) -> Self {
        SettlementCharge {
            key: key.to_string(),
            label: label.to_string(),
            amount,
            responsibility: responsibility.to_string(),
            note: note.to_string(),
        }
    }
}

impl From<DetentionCharge> for SettlementCharge {
    fn from(charge: DetentionCharge) -> Self {
        SettlementCharge {
            key: charge.key.to_string(),
            label: charge.label.to_string(),
            amount: charge.amount,
            responsibility: charge.responsibility.to_string(),
            note: charge.note,
        }
    }
}

/// Approved load-related charges that do not reduce driver pay.
///
/// With a profile, this also picks up detention: a shipper who held the truck
/// past the free time owes for the wait, and that lands on the same ledger as
/// a negative charge because it is money coming the other way.
pub fn carrier_accessorial_charges<P: TrailerOwner + ?Sized>(
    job: &Job,
    profile: Option<&P>,
) -> Vec<SettlementCharge> {
    let mut charges: Vec<SettlementCharge> = Vec::new();
    if let Some(profile) = profile {
        if let Some(detention) = detention_charge(&pickup_plan(job, profile)) {
            charges.push(detention.into());
        }
    }
    if LUMPER_DESTINATION_TYPES.contains(&job.destination_type.as_str()) {
        charges.push(SettlementCharge::new(
            "delivery_lumper",
            "carrier-authorized unloading service",
            185.0,
            CARRIER_PAID,
            "receipt required; billed to the carrier/customer settlement",
        ));
    }
    if WASHOUT_CARGO.contains(&job.cargo.key) {
        charges.push(SettlementCharge::new(
            "trailer_washout",
            "required trailer washout",
            45.0,
            CARRIER_PAID,
            "approved sanitation charge after food or refrigerated freight",
        ));
    }
    charges
}

pub fn charge_total(charges: &[SettlementCharge]) -> f64 {
    // Fold from 0.0 rather than `sum()`: Rust's `Sum for f64` starts at
    // -0.0, so an empty list yields negative zero and `fmt_grouped` renders
    // it "-0" -- a screen reader then says "minus zero dollars". Python's
    // `sum([])` is a plain 0.
    charges
        .iter()
        .map(|charge| charge.amount)
        .fold(0.0, |total, amount| total + amount)
}

pub fn charge_summary(charges: &[SettlementCharge]) -> String {
    if charges.is_empty() {
        return "none".to_string();
    }
    charges
        .iter()
        .map(|charge| format!("{} {} dollars", charge.label, fmt_grouped(charge.amount, 0)))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Wallet movement from the accessorial and toll ledger.
///
/// Detention is a negative charge, so it pays the driver. Lumpers, washouts,
/// and tolls go out. Apply this only to owner-operators; company drivers
/// keep the spoken ledger and the money stays put. Not a second settlement
/// system -- the existing ledger, applied to cash.
pub fn wallet_delta(charges: &[SettlementCharge], toll_expense: f64) -> f64 {
    -(charge_total(charges) + toll_expense)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::jobs::cargo_type;
    use crate::models::trailer_yard::CARRIER_PAID as YARD_CARRIER_PAID;

    struct CompanyDriver;
    impl TrailerOwner for CompanyDriver {}

    fn job(cargo: &str, destination_type: &str) -> Job {
        let mut job = Job::new(
            cargo_type(cargo).unwrap(),
            18.0,
            "New York",
            "New York pickup",
            "Philadelphia",
            78.0,
            2500.0,
            12.0,
        );
        job.origin_type = "air_cargo".to_string();
        job.destination_location = "Philadelphia receiver".to_string();
        job.destination_type = destination_type.to_string();
        job
    }

    #[test]
    fn lumper_and_washout_ride_the_ledger_as_carrier_paid() {
        let charges = carrier_accessorial_charges::<CompanyDriver>(
            &job("refrigerated", "retail_distribution"),
            None,
        );
        let keys: Vec<&str> = charges.iter().map(|c| c.key.as_str()).collect();
        assert_eq!(keys, vec!["delivery_lumper", "trailer_washout"]);
        assert!(charges.iter().all(|c| c.responsibility == CARRIER_PAID));
        assert_eq!(charge_total(&charges), 230.0);
        assert_eq!(
            charge_summary(&charges),
            "carrier-authorized unloading service 185 dollars, required trailer washout 45 dollars"
        );
        assert_eq!(charge_summary(&[]), "none");
        assert_eq!(YARD_CARRIER_PAID, CARRIER_PAID);
        // Lumper + washout out, no detention: the owner-op wallet drops.
        assert_eq!(wallet_delta(&charges, 30.0), -260.0);
        // Detention is a negative charge, so it pays.
        let detention = SettlementCharge::new(
            "detention",
            "detention pay",
            -90.0,
            CARRIER_PAID,
            "held past free time",
        );
        assert_eq!(wallet_delta(&[detention], 0.0), 90.0);
        assert_eq!(wallet_delta(&[], 0.0), 0.0);
    }

    #[test]
    fn a_dry_warehouse_general_load_has_no_accessorials() {
        assert!(carrier_accessorial_charges(
            &job("general", "dry_warehouse"),
            Some(&CompanyDriver)
        )
        .is_empty());
    }
}

#[cfg(test)]
mod negative_zero_tests {
    use super::*;
    use crate::pyfmt::fmt_grouped;

    #[test]
    fn charge_total_of_nothing_is_positive_zero() {
        // Rust's `Sum for f64` folds from -0.0, so an empty charge list used to
        // render as "-0" and a screen reader said "minus zero dollars" at the
        // end of every toll-free run. Python's `sum([])` is a plain 0.
        let total = charge_total(&[]);
        assert!(
            !total.is_sign_negative(),
            "an empty settlement must not be -0"
        );
        assert_eq!(fmt_grouped(total, 0), "0");
    }
}
