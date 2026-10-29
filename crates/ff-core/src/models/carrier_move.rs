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
//! dispatch trust -- service, license, record and money all clear. A trainee
//! reaches it by finishing training; a driver who was let go reaches it by
//! rebuilding exactly what cost them the last seat. The carriers on offer
//! are the ones that hire where the driver lives, as for any other hire.
//! The career and the record come along; the new carrier's pay plan,
//! dispatch habits and terminal are new.

use crate::data::world::World;
use crate::models::business::BusinessProfile;
use crate::models::business_constants::is_owner_operator;
use crate::models::career_ladder::rank_for_level;
use crate::models::carriers::{carrier, home_terminal_city_or_fallback};
use crate::models::enforcement::{
    standing_band, standing_cause, CAUSE_DEBT, CAUSE_LICENCE, CAUSE_RECORD,
    LAST_CHANCE_CARRIER_KEY, TRUST_FULL,
};
use crate::models::profile::Profile;
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

/// The company carriers a training-fleet driver living in `home_city` can
/// move to: every other one that hires there.
pub fn move_up_carriers(world: &World, home_city: &str) -> Vec<&'static CareerStartOption> {
    company_start_options()
        .into_iter()
        .filter(|option| option.key != LAST_CHANCE_CARRIER_KEY)
        .filter(|option| carrier(option.key).is_none_or(|c| c.hires_in(world, home_city)))
        .collect()
}

/// Where the driver lives, for the move: the home city, else (an older
/// save with none on file) the home terminal city.
pub fn move_up_home(profile: &Profile) -> &str {
    if profile.home_city.trim().is_empty() {
        &profile.home_terminal_city
    } else {
        &profile.home_city
    }
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
/// Only the seat changes, the way any hire changes it: the board is rebuilt
/// under the new carrier's dispatch, the home terminal becomes the new
/// carrier's terminal for the driver's home, and the tractor waits there.
/// The level, record, money and dispatch refusals used this level are the
/// driver's, not the carrier's.
pub fn apply_carrier_move(
    profile: &mut Profile,
    world: &World,
    option: &CareerStartOption,
) -> String {
    let former = profile.carrier_name.clone();
    let home = move_up_home(profile).to_string();
    profile.carrier_key = option.key.to_string();
    profile.carrier_name = option.carrier_name.to_string();
    profile.dispatch_board_cache = None;
    let terminal = home_terminal_city_or_fallback(carrier(option.key), world, &home);
    let moved =
        world.resolve_city_key(&terminal) != world.resolve_city_key(&profile.home_terminal_city);
    profile.home_terminal_city = terminal.clone();
    let report = if moved && profile.active_trip.is_none() {
        profile.current_city = terminal.clone();
        profile.parked_facility.clear();
        format!(
            " You start at their {} terminal, where your assigned truck is waiting.",
            world.spoken_city(&terminal, None)
        )
    } else {
        " The next load runs on their pay and their tractor.".to_string()
    };
    format!(
        "{} hires you away from {former}. Your level and record come with you.{report}",
        option.carrier_name
    )
}
