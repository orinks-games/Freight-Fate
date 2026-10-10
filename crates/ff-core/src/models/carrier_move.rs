//! Moving up from the training fleet to another carrier.
//!
//! Great Lakes Training Transport is two things: a gentle place to start, and
//! the fleet that takes on a driver another carrier let go. Either way it was
//! a seat with no door out: nothing in the game ever moved a company driver
//! to another carrier, so a career that started there or landed there stayed
//! there, and the termination notice's "until you build back up with them"
//! promised a way up that did not exist (player report, 2026-10-10).
//!
//! The door is a hiring bar, the same one for both kinds of driver: the
//! level where dispatch moves a driver up to the regional fleet, and full
//! dispatch trust -- service, licence, record and money all clear. A trainee
//! reaches it by finishing training; a driver who was let go reaches it by
//! rebuilding exactly what cost them the last seat. Nothing else changes in
//! the move: the career, the record and the home terminal come along, and
//! only the carrier's pay plan and dispatch habits are new.

use crate::models::business::BusinessProfile;
use crate::models::business_constants::is_owner_operator;
use crate::models::career_ladder::rank_for_level;
use crate::models::enforcement::{
    standing_band, standing_cause, CAUSE_DEBT, CAUSE_LICENCE, CAUSE_RECORD,
    LAST_CHANCE_CARRIER_KEY, TRUST_FULL,
};
use crate::models::solvency::SolvencyProfile;
use crate::models::start_options::{company_start_options, CareerStartOption};

#[cfg(test)]
mod tests;

/// The level a training-fleet driver can be hired away at: Regional Company
/// Driver, the rank where dispatch already moves a driver up to the regional
/// fleet.
pub const MOVE_UP_LEVEL: i64 = 4;

/// A company driver at the training fleet, whichever way they got there.
pub fn at_training_fleet<P: BusinessProfile + ?Sized>(profile: &P) -> bool {
    !is_owner_operator(profile.business_status())
        && profile.carrier_key() == LAST_CHANCE_CARRIER_KEY
}

/// The company carriers a training-fleet driver can move to.
pub fn move_up_carriers() -> Vec<&'static CareerStartOption> {
    company_start_options()
        .into_iter()
        .filter(|option| option.key != LAST_CHANCE_CARRIER_KEY)
        .collect()
}

/// Whether another carrier will hire this driver away, and if not, what is
/// still in the way, one sentence per gate.
pub fn move_up_eligibility<P: BusinessProfile + ?Sized>(profile: &P) -> (bool, Vec<String>) {
    let mut reasons = Vec::new();
    if !at_training_fleet(profile) {
        return (false, reasons);
    }
    if profile.career().level() < MOVE_UP_LEVEL {
        reasons.push(format!(
            "Reach level {MOVE_UP_LEVEL}: {}.",
            rank_for_level(MOVE_UP_LEVEL).title
        ));
    }
    if standing_band(profile) != TRUST_FULL {
        reasons.push(
            match standing_cause(profile) {
                CAUSE_LICENCE => "Hold a clear CDL.",
                CAUSE_RECORD => "Let your driving record clear the carrier's review.",
                CAUSE_DEBT => "Pay down what you owe.",
                _ => "Bring dispatch trust back to full with clean on-time runs.",
            }
            .to_string(),
        );
    }
    if profile.pay_advance() >= 1.0 {
        reasons.push("Settle the pay advance on your current load.".to_string());
    }
    (reasons.is_empty(), reasons)
}

/// Move the driver to `option`'s carrier. Returns the line to speak.
///
/// Only the seat changes, the way a termination changes it: the board is
/// rebuilt under the new carrier's dispatch, and the tractor comes from its
/// yard. The level, record, money, home terminal and dispatch refusals used
/// this level are the driver's, not the carrier's.
pub fn apply_carrier_move<P: SolvencyProfile + ?Sized>(
    profile: &mut P,
    option: &CareerStartOption,
) -> String {
    let former = profile.carrier_name().to_string();
    profile.set_carrier(option.key, option.carrier_name);
    profile.clear_dispatch_board_cache();
    format!(
        "{} hires you away from {former}. Your level, record, and home terminal come with \
         you, and the next load runs on their pay and their tractor.",
        option.carrier_name
    )
}
