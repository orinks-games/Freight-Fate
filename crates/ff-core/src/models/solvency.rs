//! What the driver is behind by, what a settlement may take back, and when
//! being behind costs the seat or the tractor (port of
//! `freight_fate/models/solvency.py`).
//!
//! Debt used to be inert. Every place that took money noted that the balance
//! "can go negative; never a game over", and nothing downstream ever read it: a
//! driver could sit fifty-one thousand dollars in the hole, keep gaining levels,
//! and keep being handed better equipment because the fleet tier was a pure
//! function of career level. This module is the missing half.
//!
//! Sources for the numbers here:
//!
//! * **FLSA, 29 CFR 531.35 and 531.36, with DOL Field Operations Handbook
//!   30c10(b).** Wages must be paid "free and clear", and a deduction for damage,
//!   loss, or equipment is unlawful to the extent it cuts the week below the
//!   minimum wage. That is the operative floor for the money a driver owes here,
//!   because what this game carries forward is fines, deductibles, and freight
//!   claims -- exactly the class the law will not let an employer claw back to
//!   zero. The take-home share below is therefore a legal floor, not a courtesy.
//!   (Advance *principal* is the one class the FOH does let a carrier recover
//!   past that floor. This code is deliberately kinder than the law there; see
//!   [`deductions_from_settlement`].)
//! * **New Jersey DOL v. STG Logistics, July 2026, 2,775,000 dollars.** The
//!   carrier deducted fuel, tolls, insurance, and maintenance until "deductions
//!   were sometimes greater than a driver's entire gross pay, resulting in
//!   negative net pay during some pay periods". A company driver settling for
//!   nothing is not hard realism -- it is the conduct a state regulator just
//!   fined. Company-driver collection is capped here for that reason; the
//!   open-ended version lives on the owner-operator side, where it belongs.
//! * **Consumer Credit Protection Act Title III, 15 USC 1673(a).** The standard
//!   US answer to "how much of one cheque may a creditor reach" is 25 percent of
//!   disposable earnings. Title III governs third-party garnishment rather than
//!   an employer recovering its own money, so it is the calibration for
//!   [`COLLECTION_SHARE`] rather than its authority -- but it is the number every
//!   other US wage-attachment rule is written against.
//! * **Company-driver ceiling.** No published figure exists for the balance at
//!   which a carrier terminates a driver who owes it money; the research is clear
//!   that it is not a documented industry standard. The nearest well-evidenced
//!   debt a carrier really does carry against a company driver is CDL-school
//!   tuition on a 6-to-24-month contract, reported in the 3,000 to 8,000 dollar
//!   range, with the unamortized balance due on early departure and collections
//!   after that. [`COMPANY_CEILING_FLOOR`] sits in the middle of that band, and
//!   the per-driver scaling above it is a design choice, not a cited rate.
//! * **49 CFR 376.12(k)** (truth in leasing) bounds and accounts for driver
//!   escrow, and requires the balance back within 45 days of termination. A
//!   company driver's exposure to a carrier is meant to be small and bounded;
//!   the carrier's remedy past that is to end the employment, not to run an
//!   ever-growing tab.
//! * **UCC Article 9: 9-609, 9-610, 9-615(d), 9-623.** A secured lender may
//!   repossess on default, must dispose of the collateral in a commercially
//!   reasonable way, applies the proceeds to the debt, and the obligor is liable
//!   for any deficiency. Default is whatever the security agreement says -- one
//!   missed payment technically qualifies -- but lenders in practice escalate to
//!   a recovery agent around ninety days. The three spoken warning rungs below
//!   are that escalation window. Once what is owed passes what the tractor would
//!   bring at sale, the collateral no longer covers the loan: a five-year-old
//!   sleeper resold around 49,000 to 67,000 dollars in 2025 against six-figure
//!   new prices, and a realistic post-auction deficiency runs 20,000 to 50,000.
//!   [`REPOSSESSION_EQUITY_SHARE`] is that resale-to-book ratio, applied to this
//!   game's catalog values, which are already used-market prices.
//!
//! The player-facing shape of all of it: money first, then what it cost, then
//! what you keep, then where you go from here. Nothing here is ever a dead end.
//! Every rule below leaves a driver able to work their way out, and the take-home
//! floor is the guarantee that working always helps.
//!
//! Profile access goes through [`enforcement::StandingProfile`] for reads and
//! [`SolvencyProfile`] for the writes the two endings make.

#[cfg(test)]
pub(crate) mod test_profile;
#[cfg(test)]
mod tests;

use crate::models::business::OWNER_OPERATOR_BUY_IN;
use crate::models::business_constants::{is_owner_operator, COMPANY_DRIVER};
use crate::models::enforcement::{
    DrivingRecord, StandingProfile, LAST_CHANCE_CARRIER_KEY, LAST_CHANCE_CARRIER_NAME,
    REPUTATION_TERMINATION,
};
use crate::models::trailers::trailer_type;
use crate::pyfmt::{fmt_grouped, round_py_n};

// -- what a settlement may take ---------------------------------------------

/// The most one settlement can put toward a carried balance. Title III's
/// garnishment ceiling, used here for the same reason the law sets it: a
/// collection that takes the whole cheque stops the driver being able to work.
pub const COLLECTION_SHARE: f64 = 0.25;
/// The other side of the same number, named so the spoken text can promise it.
pub const TAKE_HOME_SHARE: f64 = 1.0 - COLLECTION_SHARE;

// -- ceilings ---------------------------------------------------------------

// A company driver's exposure to the carrier is bounded by what the carrier
// will carry before it ends the employment. Scaled to the driver's own
// settlements so a senior driver on long freight is not terminated by a single
// bad week, with a floor for a new hire whose average is still tiny.
pub const COMPANY_CEILING_SETTLEMENTS: f64 = 8.0;
pub const COMPANY_CEILING_FLOOR: f64 = 6_000.0;
/// What one settlement is assumed to be worth before a career has any history.
pub const NOMINAL_SETTLEMENT: f64 = 750.0;

/// An owner-operator's lender repossesses when what is owed passes what the
/// tractor would bring at sale. Used sleeper tractors resell well under book,
/// so the collateral stops covering the loan around here.
pub const REPOSSESSION_EQUITY_SHARE: f64 = 0.6;
/// A tractor with no catalog price (the starter rig) still stands behind a
/// loan; this is the floor under the same rule.
pub const REPOSSESSION_FLOOR: f64 = 12_000.0;

// Warning rungs, as a share of the ceiling. The last one also has to leave
// real room -- see `final_rung_debt`.
pub const RUNG_HALFWAY_SHARE: f64 = 0.5;
pub const RUNG_FINAL_SHARE: f64 = 0.8;
/// The final warning has to leave at least this many of *this driver's own*
/// settlements of headroom. A fixed dollar gap is worthless to a driver whose
/// average run is bigger than the gap.
pub const RUNG_FINAL_SETTLEMENTS: f64 = 2.0;

/// The writes the two endings, the hard cap and the cash payoff make on a
/// Profile, on top of the reads in [`StandingProfile`].
// TODO(lead): implement for models::profile::Profile. The three hooks at the
// end stand in for Profile.active_truck_key(), models::trucks::TRUCK_CATALOG
// and models::carrier_fleet::assigned_truck_key until those land.
pub trait SolvencyProfile: StandingProfile {
    fn set_money(&mut self, money: f64);
    fn set_fines_owed(&mut self, fines_owed: f64);
    fn driving_record_mut(&mut self) -> &mut DrivingRecord;
    /// `profile.carrier_key = key; profile.carrier_name = name`.
    fn set_carrier(&mut self, key: &str, name: &str);
    fn set_pay_advance(&mut self, amount: f64);
    fn set_pay_advance_used_for_load(&mut self, used: bool);
    /// `profile.dispatch_board_cache = None`.
    fn clear_dispatch_board_cache(&mut self);
    fn set_owned_trucks(&mut self, trucks: Vec<String>);
    fn set_owned_trailers(&mut self, trailers: Vec<String>);
    fn set_business_status(&mut self, status: &str);
    fn set_authority_readiness(&mut self, ready: bool);
    /// `profile.weigh_station_transponder = on` (the owner-operator
    /// subscription; a company driver's is the fleet's).
    fn set_weigh_station_transponder(&mut self, on: bool);
    fn set_truck(&mut self, key: &str);
    /// `profile.owned_trucks`.
    fn owned_trucks(&self) -> Vec<String>;
    /// `profile.owned_trailers`.
    fn owned_trailers(&self) -> Vec<String>;
    /// `profile.active_truck_key()`: the tractor being driven right now.
    // TODO(lead): wire to Profile.active_truck_key.
    fn active_truck_key(&self) -> String;
    /// `TRUCK_CATALOG[key].label`.
    // TODO(lead): wire to models::trucks::TRUCK_CATALOG.
    fn truck_catalog_label(&self, key: &str) -> String;
    /// `carrier_fleet.assigned_truck_key(profile)` for the profile as it
    /// stands when called (after the repossession has made it a company
    /// driver again).
    // TODO(lead): wire to models::carrier_fleet::assigned_truck_key.
    fn assigned_truck_key(&self) -> String;
}

/// Everything the driver is behind by, in one number.
///
/// Two things put a driver behind and a player experiences them as one: cash
/// run past zero, and charges a settlement could not cover and carried
/// forward. Anything that reads the driver's solvency reads this.
pub fn debt_owed<P: StandingProfile + ?Sized>(profile: &P) -> f64 {
    let overdraft = (-profile.money()).max(0.0);
    let balance = profile.fines_owed().max(0.0);
    round_py_n(overdraft + balance, 2)
}

pub fn in_debt<P: StandingProfile + ?Sized>(profile: &P) -> bool {
    debt_owed(profile) >= 1.0
}

/// What one settlement is worth to this driver, from their own history.
pub fn average_settlement<P: StandingProfile + ?Sized>(profile: &P) -> f64 {
    let deliveries = profile.career_deliveries();
    if deliveries <= 0 {
        return NOMINAL_SETTLEMENT;
    }
    let earned = profile.career_total_earnings();
    NOMINAL_SETTLEMENT.max(earned / deliveries as f64)
}

/// Book value of the tractor standing behind an owner-operator's loan.
pub fn tractor_value<P: StandingProfile + ?Sized>(profile: &P) -> f64 {
    profile.truck_catalog_price(profile.truck())
}

/// What an owner-operator may owe before the tractor no longer covers it.
pub fn repossession_threshold<P: StandingProfile + ?Sized>(profile: &P) -> f64 {
    round_py_n(
        REPOSSESSION_FLOOR.max(tractor_value(profile) * REPOSSESSION_EQUITY_SHARE),
        2,
    )
}

/// What the carrier will carry before it ends a company driver's employment.
pub fn company_debt_ceiling<P: StandingProfile + ?Sized>(profile: &P) -> f64 {
    round_py_n(
        COMPANY_CEILING_FLOOR.max(average_settlement(profile) * COMPANY_CEILING_SETTLEMENTS),
        2,
    )
}

/// The number that matters to this driver, whichever kind of driver they are.
pub fn debt_ceiling<P: StandingProfile + ?Sized>(profile: &P) -> f64 {
    if is_owner_operator(profile.business_status()) {
        return repossession_threshold(profile);
    }
    company_debt_ceiling(profile)
}

/// How far toward the ceiling this driver is, 0 to 1.
pub fn debt_share<P: StandingProfile + ?Sized>(profile: &P) -> f64 {
    let ceiling = debt_ceiling(profile);
    if ceiling <= 0.0 {
        return 0.0;
    }
    (debt_owed(profile) / ceiling).clamp(0.0, 1.0)
}

/// The debt at which the last warning is owed.
///
/// Whichever comes first: four fifths of the ceiling, or the point that still
/// leaves two of this driver's own settlements of room. A driver whose runs
/// settle for more than the gap would otherwise get the warning and the
/// consequence in the same breath.
pub fn final_rung_debt<P: StandingProfile + ?Sized>(profile: &P) -> f64 {
    let ceiling = debt_ceiling(profile);
    let headroom_point = ceiling - RUNG_FINAL_SETTLEMENTS * average_settlement(profile);
    round_py_n((ceiling * RUNG_FINAL_SHARE).min(headroom_point).max(0.0), 2)
}

/// Which warning this driver is owed: 0 none, 1 exists, 2 halfway, 3 final.
pub fn debt_rung<P: StandingProfile + ?Sized>(profile: &P) -> i64 {
    let owed = debt_owed(profile);
    if owed < 1.0 {
        return 0;
    }
    if owed >= final_rung_debt(profile) {
        return 3;
    }
    if owed >= debt_ceiling(profile) * RUNG_HALFWAY_SHARE {
        return 2;
    }
    1
}

// -- collection -------------------------------------------------------------

/// What this settlement puts toward a carried balance.
///
/// Never the whole of it. A settlement that pays nothing leaves the driver
/// with a truck, a board, and no reachable state where working helps, which
/// is not a hard career -- it is a soft lock with a menu.
pub fn collection_from_settlement(balance: f64, net_pay: f64) -> f64 {
    let owed = balance.max(0.0);
    let pay = net_pay.max(0.0);
    round_py_n(owed.min(pay * COLLECTION_SHARE), 2)
}

/// What always reaches the driver, whatever they owe.
pub fn take_home_floor(net_pay: f64) -> f64 {
    round_py_n(net_pay.max(0.0) * TAKE_HOME_SHARE, 2)
}

/// Split one settlement into (collected toward the balance, advance repaid).
///
/// With nothing owed this is exactly what the game has always done: the
/// advance comes back out of the settlement in full. That path is untouched,
/// so a solvent driver's money is arithmetically identical.
///
/// With a balance owed, the two together stay inside the take-home floor. The
/// FOH would let a carrier take advance principal past that floor, but a
/// driver whose settlement is being collected against and whose advance is
/// also being recovered ends every run with nothing -- and a driver with
/// nothing cannot buy the fuel that earns the next settlement. The floor wins.
pub fn deductions_from_settlement(balance: f64, advance: f64, net_pay: f64) -> (f64, f64) {
    let owed = balance.max(0.0);
    let outstanding = advance.max(0.0);
    let pay = net_pay.max(0.0);
    if owed < 0.01 {
        return (0.0, round_py_n(outstanding.min(pay), 2));
    }
    let budget = round_py_n(pay * COLLECTION_SHARE, 2);
    let collected = round_py_n(owed.min(budget), 2);
    let repaid = round_py_n(outstanding.min((budget - collected).max(0.0)), 2);
    (collected, repaid)
}

/// Whether a share of every settlement is currently going to a balance.
pub fn collection_active<P: StandingProfile + ?Sized>(profile: &P) -> bool {
    profile.fines_owed().max(0.0) >= 1.0
}

/// Why a dispatcher will not front cash right now. Empty when they will.
///
/// A real trip advance is a hundred dollars against next week's settlement,
/// not a line of credit. Once a balance is already being collected, drawing
/// more is borrowing against money that is already spoken for -- and because
/// the advance is only ever offered below ten dollars of cash, a driver in
/// debt would be offered it after every single run, forever.
pub fn advance_refused_reason<P: StandingProfile + ?Sized>(profile: &P) -> String {
    if !collection_active(profile) {
        return String::new();
    }
    format!(
        "Dispatch will not front you cash while settlements are paying down what \
         you owe. You have {} outstanding, and three quarters of each settlement \
         still reaches you.",
        money_text(debt_owed(profile))
    )
}

// -- paying it down from cash ------------------------------------------------

/// A driver paying down debt from their own cash must keep fuel money to move
/// the truck. This is roughly one fuel stop's out-of-pocket cost so the payoff
/// option never strands the truck dry.
pub const PAYOFF_CASH_CUSHION: f64 = 200.0;
/// Below this floor, offering payoff options is noise: the menu clutter is worse
/// than the tiny amounts it would move.
pub const PAYOFF_MIN_CASH: f64 = 10.0;

/// Payment options a driver holding cash may put toward the balance right now.
///
/// Every option keeps [`PAYOFF_CASH_CUSHION`] of fuel money in the driver's
/// pocket -- paying down debt must never strand the truck dry. "All" is
/// offered only when cash covers the whole balance and still leaves the
/// cushion behind. "Half" is half the balance, capped at cash minus the
/// cushion. "Cushion" is everything above the withheld fuel cushion.
/// Amounts under a dollar or duplicating an earlier option are dropped.
pub fn out_of_pocket_options<P: StandingProfile + ?Sized>(profile: &P) -> Vec<(&'static str, f64)> {
    let balance = profile.fines_owed().max(0.0);
    let cash = profile.money();
    if balance < 1.0 || cash < PAYOFF_MIN_CASH {
        return Vec::new();
    }
    let mut options: Vec<(&'static str, f64)> = Vec::new();

    let mut offer = |kind: &'static str, amount: f64| {
        let amount = round_py_n(amount, 2);
        if amount >= 1.0 && options.iter().all(|(_, a)| (amount - a).abs() >= 0.01) {
            options.push((kind, amount));
        }
    };

    if cash >= balance + PAYOFF_CASH_CUSHION {
        offer("all", balance);
    }
    offer("half", (balance / 2.0).min(cash - PAYOFF_CASH_CUSHION));
    offer("cushion", balance.min(cash - PAYOFF_CASH_CUSHION));
    options
}

/// Execute an out-of-pocket payment toward the balance; return what was paid.
///
/// Working money must survive the payment: cash never goes below zero and the
/// balance never below the amount paid. The driver keeps every cent to keep
/// moving.
pub fn pay_out_of_pocket<P: SolvencyProfile + ?Sized>(profile: &mut P, amount: f64) -> f64 {
    let balance = profile.fines_owed().max(0.0);
    let cash = profile.money();
    let paid = round_py_n(amount.max(0.0).min(balance).min(cash.max(0.0)), 2);
    if paid < 0.01 {
        return 0.0;
    }
    profile.set_fines_owed(round_py_n(balance - paid, 2));
    profile.set_money(round_py_n(cash - paid, 2));
    paid
}

// -- the fleet of last resort cannot let debt run away ----------------------

/// A company driver already at the fleet that hires anyone.
///
/// There is nowhere further down, so ending their employment is not a move
/// the game can make. Their debt therefore stops at the ceiling instead of
/// climbing toward a consequence that can never arrive.
pub fn hard_capped<P: StandingProfile + ?Sized>(profile: &P) -> bool {
    if is_owner_operator(profile.business_status()) {
        return false;
    }
    profile.carrier_key() == LAST_CHANCE_CARRIER_KEY
}

/// Write off anything past the ceiling for a driver with nowhere to fall.
///
/// Returns what was written off, so the terminal can say it happened.
pub fn apply_hard_cap<P: SolvencyProfile + ?Sized>(profile: &mut P) -> f64 {
    if !hard_capped(profile) {
        return 0.0;
    }
    let excess = debt_owed(profile) - debt_ceiling(profile);
    if excess < 1.0 {
        return 0.0;
    }
    let written_off = round_py_n(excess, 2);
    let balance = profile.fines_owed().max(0.0);
    let from_balance = balance.min(written_off);
    profile.set_fines_owed(round_py_n(balance - from_balance, 2));
    let remainder = round_py_n(written_off - from_balance, 2);
    if remainder >= 0.01 {
        let money = profile.money();
        profile.set_money(round_py_n(money + remainder, 2));
    }
    written_off
}

// -- consequences -----------------------------------------------------------

/// A company driver whose balance has passed what the carrier will carry.
pub fn company_termination_due<P: StandingProfile + ?Sized>(profile: &P) -> bool {
    if is_owner_operator(profile.business_status()) {
        return false;
    }
    if hard_capped(profile) {
        return false; // nowhere further down; the cap above holds the line
    }
    debt_owed(profile) >= company_debt_ceiling(profile)
}

/// An owner-operator who owes more than the tractor would bring at sale.
pub fn repossession_due<P: StandingProfile + ?Sized>(profile: &P) -> bool {
    if !is_owner_operator(profile.business_status()) {
        return false;
    }
    debt_owed(profile) >= repossession_threshold(profile)
}

/// What the tractor brings at auction, applied to what is owed.
pub fn sale_proceeds<P: StandingProfile + ?Sized>(profile: &P) -> f64 {
    round_py_n(tractor_value(profile) * REPOSSESSION_EQUITY_SHARE, 2)
}

/// Clear what the driver owes, and return the figure that was settled.
///
/// Both endings settle the account, for the same reason and with the same
/// honesty: the carrier writes off what it could not collect from a driver it
/// no longer employs, and the lender's sale closes out the loan it wrote. A
/// deficiency after that is a civil matter between the driver and a finance
/// company, not something that follows them into the next cab -- carriers
/// hire drivers with one every day. Carrying it forward would only guarantee
/// the next seat ended the same way, which is a trap wearing a consequence's
/// clothes.
pub fn settle_account<P: SolvencyProfile + ?Sized>(profile: &mut P) -> f64 {
    let settled = debt_owed(profile);
    profile.set_fines_owed(0.0);
    if profile.money() < 0.0 {
        profile.set_money(0.0);
    }
    round_py_n(settled, 2)
}

// -- spoken numbers ---------------------------------------------------------

/// `f"{amount:,.0f} dollars"`.
pub fn money_text(amount: f64) -> String {
    format!("{} dollars", fmt_grouped(amount, 0))
}

/// What happens at the ceiling, in one plain clause. Never a threat.
pub fn ceiling_consequence_text<P: StandingProfile + ?Sized>(profile: &P) -> &'static str {
    if is_owner_operator(profile.business_status()) {
        return "the tractor is worth less than the loan on it and the lender takes it back";
    }
    "the carrier ends your employment and you move to another fleet"
}

// -- the two endings --------------------------------------------------------
//
// Both are written to the same shape: the money first, then what it cost, then
// what the driver keeps, then where they go from here. Neither is an ending in
// the sense the words usually mean -- the save is intact, the career is intact,
// and there is freight on the board in the morning. Nothing here says failed,
// over, bankrupt, or terminated out loud, and nothing here blames the driver.

pub const KEPT_LINE: &str = "You keep your career level, your experience, your endorsements, your \
                             driving record, and everything else you own. Nothing about your career \
                             was reset and no save was lost.";
pub const BACK_TO_WORK_LINE: &str =
    "There is freight waiting. Open the dispatch board whenever you are ready.";

/// End a company driver's employment over an unpayable balance.
///
/// The carrier writes off what it could not collect -- which is what really
/// happens, and what has to happen here, because a driver who arrives at the
/// next fleet still carrying the debt that cost them the last one is on rails
/// to lose that seat too.
pub fn apply_company_termination<P: SolvencyProfile + ?Sized>(profile: &mut P) -> Vec<String> {
    let former = match profile.carrier_name() {
        "" => "your carrier".to_string(),
        name => name.to_string(),
    };
    let settled = settle_account(profile);
    profile.driving_record_mut().carrier_terminations += 1;
    profile.set_carrier(LAST_CHANCE_CARRIER_KEY, LAST_CHANCE_CARRIER_NAME);
    profile.set_pay_advance(0.0);
    profile.set_pay_advance_used_for_load(false);
    profile.clear_dispatch_board_cache();
    let lines = vec![
        format!(
            "You owed {}, which is more than {former} carries on a driver, and they have ended your employment.",
            money_text(settled)
        ),
        "That balance is closed. You do not owe it to anyone any more, and \
         your cash is back to zero."
            .to_string(),
        KEPT_LINE.to_string(),
        format!(
            "What changes is the seat. Your assigned tractor goes back to the \
             {former} yard, and you go on the payroll at \
             {LAST_CHANCE_CARRIER_NAME}: shorter freight, lower pay, and \
             equipment to match. Once your dispatch trust is full again, \
             Business status lets you move to another carrier."
        ),
        BACK_TO_WORK_LINE.to_string(),
    ];
    let record = profile.driving_record_mut();
    record.setback_notice_kind = "termination".to_string();
    record.setback_notice_lines = lines.clone();
    lines
}

/// Take an owner-operator's tractor back and put them on a payroll again.
///
/// Every owned tractor goes, not just the one being driven. Leaving a spare
/// behind would settle the loan and leave the driver still an owner-operator,
/// which is a loophole rather than an ending.
pub fn apply_repossession<P: SolvencyProfile + ?Sized>(profile: &mut P) -> Vec<String> {
    let label = profile.truck_catalog_label(&profile.active_truck_key());
    let proceeds = sale_proceeds(profile);
    let settled = settle_account(profile);
    profile.driving_record_mut().repossessions += 1;
    profile.set_owned_trucks(Vec::new());
    profile.set_owned_trailers(Vec::new());
    profile.set_business_status(COMPANY_DRIVER);
    profile.set_authority_readiness(false);
    profile.set_pay_advance(0.0);
    profile.set_pay_advance_used_for_load(false);
    // A driver whose reputation is already at the floor cannot be handed to a
    // carrier that would not have them: they would lose the truck and the seat
    // in the same breath, with nowhere to drive. The fleet that hires anyone
    // catches that case.
    if profile.career_reputation() < REPUTATION_TERMINATION {
        profile.set_carrier(LAST_CHANCE_CARRIER_KEY, LAST_CHANCE_CARRIER_NAME);
    }
    let hiring = match profile.carrier_name() {
        "" => LAST_CHANCE_CARRIER_NAME.to_string(),
        name => name.to_string(),
    };
    let assigned = profile.assigned_truck_key();
    profile.set_truck(&assigned);
    profile.clear_dispatch_board_cache();
    let lines = vec![
        format!(
            "You owed {} against a {label} that would bring about {} at sale, so the loan is no longer \
             covered by the truck behind it, and the lender has taken it back.",
            money_text(settled),
            money_text(proceeds)
        ),
        "The sale closes the loan. What you owed is settled and your cash is back to zero.".to_string(),
        KEPT_LINE.to_string(),
        format!(
            "You are a company driver again, on the payroll at {hiring} and in a \
             carrier tractor. The owner-operator path is still open to you, and \
             the buy-in gates are the same ones you cleared to get here."
        ),
        BACK_TO_WORK_LINE.to_string(),
    ];
    let record = profile.driving_record_mut();
    record.setback_notice_kind = "repossession".to_string();
    record.setback_notice_lines = lines.clone();
    lines
}

// -- the way back by choice --------------------------------------------------

/// What the carrier pays to take an owner-operator's equipment back when the
/// driver chooses a company seat again.
///
/// The tractor the buy-in bought goes back for the used-sale share of its
/// catalog price, capped at the buy-in itself: the same figure both ways, so
/// leaving is neither a sale nor a loss on a carrier tractor worth far more
/// than the buy-in. Anything bought at the dealer since -- tractors and
/// trailers alike -- goes at the used-sale share, the ratio the lender's sale
/// already uses. Nothing here is a penalty; the difference between catalog
/// and used is what the equipment was worth.
pub fn company_return_buy_back<P: SolvencyProfile + ?Sized>(profile: &P) -> f64 {
    let mut total = 0.0;
    for (i, key) in profile.owned_trucks().iter().enumerate() {
        let used = profile.truck_catalog_price(key) * REPOSSESSION_EQUITY_SHARE;
        total += if i == 0 {
            used.min(OWNER_OPERATOR_BUY_IN)
        } else {
            used
        };
    }
    for key in profile.owned_trailers() {
        if let Some(trailer) = trailer_type(&key) {
            total += trailer.purchase_price * REPOSSESSION_EQUITY_SHARE;
        }
    }
    round_py_n(total, 2)
}

/// An owner-operator hands the equipment back and takes a company seat again,
/// by choice. Every owned tractor and trailer goes, for the same reason the
/// repossession takes them all: a spare left behind would leave the driver
/// still an owner-operator. Nothing is written to the driving record, no
/// balance is touched, and the carrier that had the lease keeps the driver.
pub fn apply_return_to_company_driving<P: SolvencyProfile + ?Sized>(
    profile: &mut P,
) -> Vec<String> {
    let label = profile.truck_catalog_label(&profile.active_truck_key());
    let others = profile.owned_trucks().len().saturating_sub(1) + profile.owned_trailers().len();
    let buy_back = company_return_buy_back(profile);
    profile.set_money(round_py_n(profile.money() + buy_back, 2));
    profile.set_owned_trucks(Vec::new());
    profile.set_owned_trailers(Vec::new());
    profile.set_business_status(COMPANY_DRIVER);
    profile.set_authority_readiness(false);
    profile.set_weigh_station_transponder(false);
    let carrier = match profile.carrier_name() {
        "" => "your carrier".to_string(),
        name => name.to_string(),
    };
    let assigned = profile.assigned_truck_key();
    profile.set_truck(&assigned);
    profile.clear_dispatch_board_cache();
    let more = match others {
        0 => String::new(),
        1 => " and one more piece of equipment".to_string(),
        n => format!(" and {n} more pieces of equipment"),
    };
    vec![
        format!(
            "{carrier} takes the {label}{more} back for {}. You are a company driver again, \
             in a carrier tractor, and the carrier pays fuel, repairs, and the trailer.",
            money_text(buy_back)
        ),
        "Settlements are driver wages and bonuses from here. The owner-operator buy-in \
         stays open under Business status, and the gates are the ones you already cleared."
            .to_string(),
    ]
}

pub fn setback_pending<P: StandingProfile + ?Sized>(profile: &P) -> bool {
    profile
        .driving_record()
        .is_some_and(|record| !record.setback_notice_lines.is_empty())
}

pub fn clear_setback_notice<P: SolvencyProfile + ?Sized>(profile: &mut P) {
    let record = profile.driving_record_mut();
    record.setback_notice_kind = String::new();
    record.setback_notice_lines = Vec::new();
}

// -- warnings ---------------------------------------------------------------

/// The warning this rung is owed, or empty when none is.
///
/// Terse keeps every rung and never drops the ceiling or the consequence --
/// they are money with a career attached, which is exactly what terse is for.
/// What terse drops is the sentence about what brings it down.
pub fn debt_warning_line<P: StandingProfile + ?Sized>(profile: &P, terse: bool) -> String {
    let rung = debt_rung(profile);
    if rung == 0 {
        return String::new();
    }
    let owed = money_text(debt_owed(profile));
    if hard_capped(profile) {
        if terse {
            return format!("Owed {owed}. Held there; a quarter of each settlement pays it down.");
        }
        return format!(
            "You owe {owed}. Your carrier holds it there and writes off anything \
             past it. A quarter of each settlement goes to it. \
             You can also pay it down from cash at any terminal or truck stop."
        );
    }
    let ceiling = money_text(debt_ceiling(profile));
    let consequence = ceiling_consequence_text(profile);
    if rung == 1 {
        if terse {
            return format!("Owed {owed}. Ceiling {ceiling}.");
        }
        return format!(
            "You owe {owed}. A quarter of every settlement goes to it. \
             The ceiling is {ceiling}. \
             You can also pay it down from cash at any terminal or truck stop."
        );
    }
    if rung == 2 {
        if terse {
            return format!("Owed {owed}, over halfway to a ceiling of {ceiling}.");
        }
        return format!(
            "You owe {owed}, over halfway to a ceiling of {ceiling}. \
             A quarter of every settlement is paying it down. \
             You can also pay it down from cash at any terminal or truck stop."
        );
    }
    if terse {
        return format!("Owed {owed}. At {ceiling}, {consequence}.");
    }
    format!(
        "You owe {owed}, against a ceiling of {ceiling}. Last warning. At \
         {ceiling}, {consequence}. Room for a couple more settlements, and a \
         quarter of each one is paying it down."
    )
}

/// The reviewable debt line: what you owe, the ceiling, and what happens.
///
/// Empty when there is nothing owed, so a solvent driver never hears it.
pub fn debt_line<P: StandingProfile + ?Sized>(profile: &P) -> String {
    let owed = debt_owed(profile);
    if owed < 1.0 {
        return String::new();
    }
    if hard_capped(profile) {
        return format!(
            "Owed: {}. Your carrier holds it there and writes \
             off anything past it. Part of every settlement pays it down. \
             You can also pay it down from cash at any terminal or truck stop.",
            money_text(owed)
        );
    }
    let ceiling = debt_ceiling(profile);
    format!(
        "Owed: {} of {}. Past that, {}. \
         You can also pay it down from cash at any terminal or truck stop.",
        money_text(owed),
        money_text(ceiling),
        ceiling_consequence_text(profile)
    )
}
