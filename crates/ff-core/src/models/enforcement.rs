//! The driver's enforcement record: citations, serious violations, CDL standing
//! (port of `freight_fate/models/enforcement.py`).
//!
//! This models real US commercial-driver enforcement rather than a softened
//! version of it, because the softened version taught players nothing: a driver
//! could run from troopers twice, take spike strips twice, and keep every load on
//! the board.
//!
//! Sources for every number here:
//!
//! * 49 CFR 383.51 Table 2 (serious traffic violations). Speeding 15 mph or more
//!   over the limit, reckless driving, improper lane changes, and following too
//!   closely are serious violations. A **second** conviction inside three years
//!   disqualifies the CDL for 60 days; a **third or subsequent** for 120 days.
//! * 49 CFR 383.51 Table 1 (major offenses). Using a commercial vehicle in the
//!   commission of a felony -- which is what fleeing and eluding a police officer
//!   is in most states -- disqualifies the CDL for one year on the first offense
//!   and for **life** on the second. Leaving the scene carries the same pair.
//! * CDL speeding fines. The top-of-range fine for a first offense of 15 mph or
//!   more over the limit is 2,500 dollars (Illinois; Arizona matches it), so that
//!   is the ceiling a single speeding citation can reach here.
//! * 49 CFR 386 Appendix B. Violating an out-of-service order carries a civil
//!   penalty of not less than 3,961 dollars for a first conviction and not less
//!   than 7,924 dollars for a second.
//! * Work zone penalty doubling. Nevada, Texas and Pennsylvania are among the
//!   states that double the fine for a violation committed inside a marked work
//!   zone, whatever the violation was. Every citation here doubles the same way
//!   inside a construction zone, and the driver is told that is why.
//! * FMCSA CSA Unsafe Driving BASIC. Every roadside citation is weighted and
//!   follows the *carrier*, which is why a company driver's citations cost the
//!   carrier's standing and eventually the job, while an owner-operator simply
//!   pays and watches their own authority.
//!
//! Nothing here is harsher than the real rule. Where the real rule gives a range,
//! this takes the severe end of it.
//!
//! Profile access goes through [`StandingProfile`]: the Python read these
//! fields with `getattr`, and `models::profile::Profile` implements the trait.

mod record;
#[cfg(test)]
mod tests;

pub use record::{
    seed_record_from_save, DrivingRecord, RecordEntry, RECORD_CITATION, RECORD_CRASH,
    RECORD_ENTRIES_KEPT, RECORD_FATIGUE, RECORD_MAJOR, RECORD_SERIOUS,
};

use crate::models::business_constants::is_owner_operator;
use crate::models::solvency::{debt_owed, debt_rung, money_text};
use crate::pyfmt::{round_py_int, round_py_n};
use crate::settings::ScoreDisplay;
use crate::sim::season::{date_text, weekday_name, CAREER_START_DAY_OF_YEAR, DAYS_PER_YEAR};

pub const HOURS_PER_DAY: f64 = 24.0;

// -- serious traffic violations (49 CFR 383.51 Table 2) ----------------------

/// Speeding this far over the posted limit is a serious traffic violation, not
/// an expensive inconvenience.
pub const SERIOUS_SPEED_MPH_OVER: f64 = 15.0;
/// Convictions count against each other for three years (the 383.51 license
/// ladder; this is the law, not the carrier).
pub const SERIOUS_WINDOW_DAYS: i64 = 3 * 365;
/// How far back the carrier's record review, the insurer's surcharge and the
/// reputation weight look: one game year. 391.25 says three, and that is
/// what the license ladder above keeps, but the clock only moves on the road
/// -- a game day per delivery, in practice -- so a three-year hold on
/// equipment and money was one no career ever drove off (owner, 2026-09-14).
pub const REVIEW_WINDOW_DAYS: i64 = 365;
pub const SERIOUS_SECOND_SUSPENSION_DAYS: i64 = 60;
pub const SERIOUS_THIRD_SUSPENSION_DAYS: i64 = 120;

// -- major offenses (49 CFR 383.51 Table 1) ---------------------------------

pub const MAJOR_FIRST_DISQUALIFICATION_DAYS: i64 = 365;
// The second major offense is a lifetime disqualification. Represented by the
// flag rather than a duration, because there is no date it clears.

pub const SUSPENSION_SERIOUS: &str = "serious";
pub const SUSPENSION_MAJOR: &str = "major";

// -- the carrier's record review (49 CFR 391.25) ----------------------------
//
// A carrier must review every driver's motor vehicle record at least once a
// year and decide whether they still meet its minimum standard. The rule
// does not set the standard; the carrier's insurer does, and the common
// hiring floors are "no more than three moving violations in three years"
// and "no more than one serious violation". These figures are ASSUMED from
// that practice, not read from a statute: the statute is only that the
// review happens.
//
// Company drivers only. A leased owner-operator answers to an insurer through
// the surcharge below; an independent has no carrier to review them.

/// More than this many citations in the review window and the carrier
/// holds the driver below the equipment their level earns.
pub const CARRIER_REVIEW_CITATIONS: i64 = 3;
/// This many serious violations in the window does the same.
pub const CARRIER_REVIEW_SERIOUS: i64 = 1;
/// At this many citations in the window the insurer will not carry the
/// driver at all, and the carrier ends the employment.
pub const CARRIER_TERMINATION_CITATIONS: i64 = 6;
/// Likewise for serious violations. Two in three years is also the second
/// rung of the 383.51 suspension ladder, so the CDL is suspended at the
/// same moment -- which is exactly when a real carrier lets a driver go.
pub const CARRIER_TERMINATION_SERIOUS: i64 = 2;

// -- the owner-operator's insurance surcharge -------------------------------
//
// Commercial auto underwriters rate a policy on the driver's record, and the
// surcharge schedules are proprietary. These steps are ASSUMED: a tenth of
// the reserve per citation and a third per serious violation, both inside
// the same review window, and never past double the clean rate.

pub const INSURANCE_SURCHARGE_PER_CITATION: f64 = 0.10;
pub const INSURANCE_SURCHARGE_PER_SERIOUS: f64 = 0.35;
pub const INSURANCE_SURCHARGE_MAX: f64 = 2.0;
pub const SUSPENSION_LIFETIME: &str = "lifetime";

// -- money ------------------------------------------------------------------

/// Speeding citation by how far over the limit, taking the severe end of the
/// published state schedules. The 15-mph step is where a citation also becomes
/// a serious traffic violation, which is why the money jumps there too.
pub const SPEEDING_FINE_STEPS: [(f64, f64); 5] = [
    (0.0, 250.0),
    (10.0, 400.0),
    (15.0, 1_000.0),
    (20.0, 1_600.0),
    (30.0, 2_500.0),
];
// Prior citations anywhere in the career make the next one cost more. The step
// schedule above is the severe end of real first-offense CDL law, but a first
// offense is the cheapest a violation ever is: repeat and aggravated speeding
// is charged as a misdemeanor in several states, habitual-offender statutes
// stack penalties further, and court costs and surcharges ride on top of every
// count.
//
// The same step now scales every flat fine below, not just speeding. There is
// deliberately one knob: a second repeat ladder would be a second thing to
// tune, a second thing to explain, and a second thing to drift.
pub const CITATION_REPEAT_STEP: f64 = 0.5;

// ...but the step is capped, which it was not when it applied to speeding
// alone. Two reasons, and the second is the binding one.
//
// Real repeat offenders are charged "double, even triple" a standard fine, so
// the shape is right but the runaway is not. Uncapped, a career driver's scale
// bypass reached 19,800 dollars by twenty priors, and 39,600 inside roadwork,
// against a federal ceiling of 10,000 for the real offense.
//
// The binding constraint is solvency, not statute. An owner-operator who owes
// more than ``solvency.REPOSSESSION_FLOOR`` (12,000) loses the tractor. A
// single citation must never be able to reach that on its own, or one traffic
// stop repossesses a truck -- which no player would read as a rule, only as
// the game breaking. Capping the step at 2.0 leaves the construction-zone
// doubling to ride on top for a worst case of 4x base: unsafe equipment tops
// out at 9,200 and the very worst case in the game, 30-plus over in a work
// zone as a repeat offender, at 10,000. Punishing, survivable, and clear of
// the floor.
//
// Deliberately capping the STEP and not the total: the zone doubling stays
// whole so the spoken "it is doubled because you were in a construction zone"
// is always true. A total cap would silently swallow it for repeat offenders
// and make that line a lie.
//
// The money therefore stops climbing after the third citation -- 1,800 then
// 2,700 then 3,600 for a bypass -- and that is the intended shape rather than
// a gap. Past that the deterrent is the record, not the wallet: the serious
// violation ladder and the suspension tiers above keep escalating for the
// whole career, and losing the license costs far more than any fine.
pub const CITATION_REPEAT_MAX_MULTIPLIER: f64 = 2.0;
/// A fine earned inside an active construction zone is doubled. That is the
/// real rule, not a game rule -- Nevada, Texas and Pennsylvania all double the
/// penalty for a violation committed inside a marked work zone, and the sign at
/// the taper that says so is a sign real drivers read. It multiplies the
/// repeat-offender figure rather than adding to it, because a state that does
/// both doubles whatever the driver already owed.
pub const CONSTRUCTION_ZONE_FINE_MULTIPLIER: f64 = 2.0;

// -- what each citation costs before those multipliers ----------------------
//
// Every fine amount in the game lives here, beside the schedule and the
// multipliers that scale it. They used to be scattered through the driving
// layer, where the chain-law citation had drifted into two separate constants
// that each claimed to be the Colorado number.

/// FMCSA driver-level penalty for operating a commercial vehicle with unsafe
/// conditions: 2,304 dollars.
pub const UNSAFE_DAMAGE_FINE: f64 = 2300.0;
/// Running an open scale. State fines run from 250 dollars into four figures,
/// and California and New York both pass 1,000 on a first offense; the federal
/// exposure standing behind them reaches 10,000.
pub const WEIGH_STATION_BYPASS_FINE: f64 = 1800.0;
/// The chain-law citation where no state figure has been sourced (CA, WA,
/// NV, ID, MT, UT): UNVERIFIED placeholder, the old Colorado figure,
/// until each state's own schedule is read.
pub const CHAIN_LAW_FINE: f64 = 580.0;
/// Colorado: 500 plus 79 (csp.colorado.gov/chain-law-information; the
/// CDOT freight fact sheet of Aug 2026 prints 78).
pub const CHAIN_LAW_FINE_CO: f64 = 579.0;
/// Oregon: trucks not chaining where required, 880 dollars (ODOT winter
/// commercial vehicle page).
pub const CHAIN_LAW_FINE_OR: f64 = 880.0;
/// Colorado: blocking a lane for not chaining, 1,000 plus 157
/// (csp.colorado.gov/chain-law-information). For a future stuck-in-the-lane
/// outcome.
pub const CHAIN_BLOCKING_FINE_CO: f64 = 1157.0;
/// Wyoming: 250 dollars for not chaining where required (WYDOT chain law;
/// W.S. 31-5-956).
pub const CHAIN_LAW_FINE_WY: f64 = 250.0;
/// Wyoming: 750 dollars when the violation causes a road closure. Closures
/// aren't modeled yet, so this is for a future outcome.
pub const CHAIN_CLOSURE_FINE_WY: f64 = 750.0;
/// Colorado: a commercial vehicle with no chains aboard on a carry corridor
/// in season, 100 plus 32 (CDOT freight fact sheet, Aug 2026).
pub const CHAIN_CARRY_FINE_CO: f64 = 132.0;

/// The chain-law citation in `state` (two-letter code). Only Colorado,
/// Oregon and Wyoming are sourced; every other state keeps the placeholder.
pub fn chain_law_fine(state: Option<&str>) -> f64 {
    match state {
        Some("CO") => CHAIN_LAW_FINE_CO,
        Some("OR") => CHAIN_LAW_FINE_OR,
        Some("WY") => CHAIN_LAW_FINE_WY,
        _ => CHAIN_LAW_FINE,
    }
}
/// Following too closely is a serious traffic violation under 49 CFR 383.51
/// Table 2 -- two inside three years disqualify the CDL for 60 days -- so it is
/// priced as one, not as the fender-bender it looks like from the cab.
pub const FOLLOWING_TOO_CLOSE_FINE: f64 = 600.0;
/// Improper lane use is a serious traffic violation on the same table. Virginia
/// charges 250 dollars for it, or 500 in a designated highway safety corridor;
/// this takes the corridor figure.
pub const LANE_MISUSE_FINE: f64 = 500.0;
/// Running dark after sunset: an equipment violation everywhere, written as a
/// fix-it-plus-fine in most states.
pub const LIGHTS_FINE: f64 = 350.0;
/// Running the red light where a ramp meets the surface road. California
/// Vehicle Code 21453(a) carries a 100-dollar base fine that the uniform bail
/// schedule's penalty assessments bring to about 490 dollars, the figure a
/// driver actually pays; most states land in the same range.
pub const RED_LIGHT_FINE: f64 = 490.0;
/// Rolling or blowing the stop sign at a ramp end. California Vehicle Code
/// 22450 carries a 35-dollar base fine, about 238 dollars with the same
/// assessments -- half the red light, as the schedules everywhere price it.
pub const STOP_SIGN_FINE: f64 = 240.0;
/// Driving through the barrels instead of merging out of a coned-off lane.
/// Missouri RSMo 304.585 (endangerment of a highway worker) lists striking or
/// moving barrels, barriers and signs as an offense in its own right -- the one
/// category in that statute that does not need workers to be present -- at up
/// to 1,000 dollars. This takes the top of that range: it sits above the
/// equipment fines because what it risks is the crew, not the truck.
pub const WORK_ZONE_BARRELS_FINE: f64 = 1000.0;
/// Failing to pull over promptly -- and then stopping -- is not fleeing. It is
/// charged as failing to obey a lawful order, which rides in with reckless
/// driving as a serious traffic violation under 49 CFR 383.51 Table 2, above
/// the ordinary commercial citation range.
pub const FAILURE_TO_STOP_CITATION_FINE: f64 = 1500.0;
/// Fleeing and eluding a police officer in a commercial vehicle is a felony in
/// most states -- a third-degree felony in Florida, for one, carrying a fine of
/// up to 5,000 dollars. This takes that top of range, and the conviction is a
/// major offense under 49 CFR 383.51 Table 1 on top of the money.
pub const FAILURE_TO_STOP_FINE: f64 = 5000.0;

// -- fatigue (49 CFR 392.3 / 392.5) -----------------------------------------

/// 49 CFR 392.3 forbids operating a commercial vehicle while ability or
/// alertness is impaired by fatigue, and 392.5 lets an officer put a fatigued
/// driver out of service on the spot. Running off the road asleep is also a
/// preventable safety incident: carriers discipline it and repeat it is a
/// termination, and it feeds the CSA fatigued-driving BASIC.
pub const FATIGUE_EVENT_REPUTATION_HIT: f64 = 6.0;
/// Do it more than once in a career and it stops being an accident. The second
/// and every later run-off-road fatigue event is a 392.3 violation on the
/// record, joining the serious-violation ladder.
pub const FATIGUE_EVENTS_BEFORE_SERIOUS: i64 = 2;
/// The standard fatigue out-of-service order is ten consecutive hours off duty.
pub const FATIGUE_OUT_OF_SERVICE_HOURS: f64 = 10.0;

// -- dispatch trust ---------------------------------------------------------

// Reputation has always paid a trust bonus. It now also decides what dispatch
// will put in front of you and how much choice you get, and it slides the
// whole way down instead of tripping one gate at the bottom.
//
// New careers start at 50 and every on-time delivery adds 2, so a driver who
// runs clean never leaves the top band and never sees any of this. The full
// band reaches down to 40 on purpose: refusing an assigned load costs 2 and
// is a sanctioned move with its own budget, so spending that budget must not
// by itself read as losing dispatch's confidence.
pub const TRUST_FULL: &str = "full";
pub const TRUST_GUARDED: &str = "guarded";
pub const TRUST_POOR: &str = "poor";
pub const TRUST_LAST_CHANCE: &str = "last chance";

pub const REPUTATION_FULL_BOARD: f64 = 40.0;
pub const REPUTATION_GUARDED: f64 = 28.0;
pub const REPUTATION_POOR: f64 = 16.0;
/// A company driver below this has run out of carrier patience.
pub const REPUTATION_TERMINATION: f64 = 8.0;

// -- the driving record's weight on reputation --------------------------------
//
// The delivery ledger (+2 on time, -4 late, claims and stops on top) pins at
// 100 for anyone who runs a lot of loads, and a citation or a serious
// violation never touched it at all: a driver with two citations, three
// serious violations and seven claims read 98 beside that record (owner,
// 2026-09-14). The reputation the game shows, gates on and pays a trust bonus
// for is the ledger LESS the record still inside a window, so a bad CDL
// record shows on the number everyone reads and ages off it again. A major
// offense counts for life, the way it stays on a motor vehicle record. Capped
// so the record alone cannot zero a driver -- the ledger still has to have
// been spent too -- and the ledger itself is never written down for it, so a
// record that ages out gives the points back.
//
// The window is the review's one game year (REVIEW_WINDOW_DAYS): the clock
// only moves while the truck rolls or rests, so a delivery is about a game
// day and a three-year window is longer than any career yet played -- a
// weight that never lifts is a lifetime mark by another name. A year is long
// enough to outlast a bad month and short enough to be driven off (owner,
// 2026-09-14).
pub const REPUTATION_WINDOW_DAYS: i64 = REVIEW_WINDOW_DAYS;
pub const RECORD_CITATION_REPUTATION: f64 = 4.0;
pub const RECORD_SERIOUS_REPUTATION: f64 = 10.0;
pub const RECORD_MAJOR_REPUTATION: f64 = 20.0;
pub const RECORD_REPUTATION_CAP: f64 = 60.0;

/// What the driving record costs off reputation right now. A crash counts as
/// a serious event (owner ruling, 2026-09-24).
pub fn record_reputation_penalty(record: &DrivingRecord, game_hours: f64) -> f64 {
    let serious = record.serious_within(game_hours, REPUTATION_WINDOW_DAYS)
        + record.crashes_within(game_hours, REPUTATION_WINDOW_DAYS);
    let penalty = RECORD_CITATION_REPUTATION
        * record.citations_within(game_hours, REPUTATION_WINDOW_DAYS) as f64
        + RECORD_SERIOUS_REPUTATION * serious as f64
        + RECORD_MAJOR_REPUTATION * record.major_count() as f64;
    penalty.min(RECORD_REPUTATION_CAP)
}

/// The reputation everyone reads: the delivery ledger less the record.
pub fn standing_reputation(ledger: f64, record: &DrivingRecord, game_hours: f64) -> f64 {
    (ledger - record_reputation_penalty(record, game_hours)).clamp(0.0, 100.0)
}

#[cfg(test)]
mod standing_tests;

/// The fleet a terminated driver can still get hired by.
pub const LAST_CHANCE_CARRIER_KEY: &str = "great_lakes_training";
pub const LAST_CHANCE_CARRIER_NAME: &str = "Great Lakes Training Transport";

/// What the enforcement and solvency layers read off a live Profile.
///
/// The Python read these with `getattr(profile, ..., default)`; every method
/// here is that read, with the same default where one existed. Mutation lives
/// on `solvency::SolvencyProfile`.
// TODO(lead): implement for models::profile::Profile. The three catalogue /
// fleet hooks stand in for models::trucks::TRUCK_CATALOG and
// models::carrier_fleet::equipment_hold_clause until those land; replace the
// trait calls with the real functions then.
pub trait StandingProfile {
    /// `profile.career.reputation`.
    fn career_reputation(&self) -> f64;
    /// `profile.career.deliveries`.
    fn career_deliveries(&self) -> i64;
    /// `profile.career.total_earnings`.
    fn career_total_earnings(&self) -> f64;
    /// `profile.game_hours`.
    fn game_hours(&self) -> f64;
    /// The hour the player's own calendar reads now
    /// (`Profile::player_calendar_hours`): dates are spoken counted from it.
    fn calendar_now_hours(&self) -> f64;
    /// Whether that calendar is the real one (live weather drives it), so a
    /// spoken weekday comes from the real date.
    fn calendar_is_live(&self) -> bool {
        false
    }
    /// `getattr(profile, "driving_record", None)`.
    fn driving_record(&self) -> Option<&DrivingRecord>;
    /// `profile.business_status`.
    fn business_status(&self) -> &str;
    /// `profile.carrier_key`.
    fn carrier_key(&self) -> &str;
    /// `profile.carrier_name`.
    fn carrier_name(&self) -> &str;
    /// `profile.money`.
    fn money(&self) -> f64;
    /// `profile.fines_owed`.
    fn fines_owed(&self) -> f64;
    /// `profile.truck`: the owner-operator's tractor, or the assignment key.
    fn truck(&self) -> &str;
    /// `TRUCK_CATALOG.get(key).price`, 0.0 for an unknown key.
    // TODO(lead): wire to models::trucks::TRUCK_CATALOG.
    fn truck_catalog_price(&self, key: &str) -> f64;
    /// `carrier_fleet.equipment_hold_clause(profile)`: the clause saying the
    /// yard is holding a truck back, or `""`.
    // TODO(lead): wire to models::carrier_fleet::equipment_hold_clause.
    fn equipment_hold_clause(&self) -> String {
        String::new()
    }
}

/// The record a profile always carries; `None` is the `SimpleNamespace` of
/// the tests, which Python would have crashed on here too.
fn record_of<P: StandingProfile + ?Sized>(profile: &P) -> &DrivingRecord {
    profile
        .driving_record()
        .expect("a profile carries a driving record")
}

/// What a citation actually costs: the base, the priors, and where it happened.
///
/// The one place a fine is priced. Every charge in the game comes through
/// here so that neither multiplier can be applied twice, skipped, or applied
/// in a different order somewhere else.
///
/// They compound rather than add. The repeat-offender step decides what this
/// driver owes for this violation; the construction zone then doubles that
/// whole figure, which is what a state that does both actually does to a
/// repeat offender. A second bypass inside roadwork is 1,800 x 1.5 x 2.
///
/// The repeat step is capped at [`CITATION_REPEAT_MAX_MULTIPLIER`]. Repeat
/// offenders really are charged "double, even triple" a standard fine, and
/// that is the shape this models -- but the step alone is unbounded, and
/// left to run it priced a career driver's scale bypass at 19,800 dollars
/// by twenty priors and 39,600 inside roadwork, against a federal ceiling
/// of 10,000 for the real offense. An owner-operator starts with 18,000, so
/// a single stop could exceed a whole career's capital. Past a point the
/// arithmetic stops reading as severity and starts reading as a bug.
///
/// The construction-zone doubling is applied AFTER the cap, not folded into
/// it: the cap governs how much being a repeat offender can cost you, and
/// where the violation happened is a separate fact about this one citation.
///
/// `ceiling` is optional and names a statutory maximum for a specific
/// citation; it is the last word, applied after everything else.
pub fn citation_fine(
    base_fine: f64,
    prior_citations: i64,
    construction_zone: bool,
    ceiling: Option<f64>,
) -> f64 {
    let step = 1.0 + CITATION_REPEAT_STEP * prior_citations.max(0) as f64;
    let mut multiplier = step.min(CITATION_REPEAT_MAX_MULTIPLIER);
    if construction_zone {
        multiplier *= CONSTRUCTION_ZONE_FINE_MULTIPLIER;
    }
    let fine = base_fine * multiplier;
    round_py_n(
        match ceiling {
            None => fine,
            Some(ceiling) => ceiling.min(fine),
        },
        2,
    )
}

/// What a speeding citation costs, by how far over, how many priors, and where.
pub fn speeding_citation_fine(mph_over: f64, prior_citations: i64, construction_zone: bool) -> f64 {
    let mut fine = SPEEDING_FINE_STEPS[0].1;
    for (threshold, amount) in SPEEDING_FINE_STEPS {
        if mph_over >= threshold {
            fine = amount;
        }
    }
    citation_fine(fine, prior_citations, construction_zone, None)
}

/// How many citations this career already carries, for the repeat scaling.
///
/// Tolerates a profile with no record at all: a career that predates the
/// license file is a first offender, not a crash.
pub fn career_citations<P: StandingProfile + ?Sized>(profile: &P) -> i64 {
    profile
        .driving_record()
        .map(|record| record.citations)
        .unwrap_or(0)
}

/// Below this career level an officer writes a warning, not a citation,
/// for anything short of a serious violation or an out-of-service order
/// (owner ruling, 2026-10-18). A new career starts at 50 reputation, and a
/// run of minor tickets on the first loads took it to zero before the
/// driver had learned the road.
pub const NEW_DRIVER_WARNING_LEVEL: i64 = 10;

/// Whether a driver at `level` is still let off minor violations.
pub fn new_driver_warned(level: i64) -> bool {
    level < NEW_DRIVER_WARNING_LEVEL
}

/// Why the figure is twice what it would be anywhere else.
///
/// Spoken after the amount, never instead of it. A driver who hears a doubled
/// number with no explanation hears a bug. Empty when nothing was doubled, so
/// every caller can append it unconditionally.
pub fn construction_zone_fine_clause(construction_zone: bool) -> &'static str {
    if construction_zone {
        " It is doubled because you were in a construction zone."
    } else {
        ""
    }
}

/// Whether this overage is an FMCSA serious traffic violation.
pub fn is_serious_speed(mph_over: f64) -> bool {
    mph_over >= SERIOUS_SPEED_MPH_OVER
}

// -- dispatch access --------------------------------------------------------

/// How far dispatch trusts this driver right now.
///
/// The band is decided from the rounded score the player hears, so a spoken
/// "40 out of 100" never sits beside a "guarded" trust line.
pub fn trust_band(reputation: f64) -> &'static str {
    let reputation = round_py_int(reputation.clamp(0.0, 100.0)) as f64;
    if reputation >= REPUTATION_FULL_BOARD {
        return TRUST_FULL;
    }
    if reputation >= REPUTATION_GUARDED {
        return TRUST_GUARDED;
    }
    if reputation >= REPUTATION_POOR {
        return TRUST_POOR;
    }
    TRUST_LAST_CHANCE
}

/// The plain-language reputation band, using the rounded score players hear.
pub fn reputation_band_word(score: f64) -> &'static str {
    match round_py_int(score.clamp(0.0, 100.0)) {
        90..=100 => "Outstanding",
        75..=89 => "Well regarded",
        60..=74 => "Established",
        40..=59 => "Unproven",
        28..=39 => "Shaky",
        16..=27 => "Damaged",
        _ => "Ruined",
    }
}

/// A reputation score in the player's preferred spoken form.
pub fn reputation_text(score: f64, mode: ScoreDisplay) -> String {
    let word = reputation_band_word(score);
    let number = round_py_int(score.clamp(0.0, 100.0));
    match mode {
        ScoreDisplay::Both => format!("{word}. {number} out of 100."),
        ScoreDisplay::Percent => format!("{number} out of 100."),
        ScoreDisplay::Description => format!("{word}."),
    }
}

/// How many loads dispatch will still put in front of this driver.
pub fn board_offers_for_reputation(base: i64, reputation: f64) -> i64 {
    board_offers_for_band(base, trust_band(reputation))
}

/// The board for a trust band. Callers pass [`standing_band`], the band the
/// dispatch trust line speaks, so a record or debt that holds the line at
/// guarded holds the board there too (owner, 2026-09-28).
pub fn board_offers_for_band(base: i64, band: &str) -> i64 {
    if band == TRUST_FULL {
        return base;
    }
    if band == TRUST_GUARDED {
        return (base - 2).max(3);
    }
    if band == TRUST_POOR {
        return 2;
    }
    1
}

/// Below guarded, a senior driver goes back to taking what dispatch gives.
///
/// The career already earns the right to pick loads at level 8. Losing
/// dispatch's trust takes that privilege back -- the game's own language for
/// "we do not let you choose any more".
pub fn trust_revokes_load_choice(reputation: f64) -> bool {
    band_revokes_load_choice(trust_band(reputation))
}

/// [`trust_revokes_load_choice`] for a band (see [`board_offers_for_band`]).
pub fn band_revokes_load_choice(band: &str) -> bool {
    band == TRUST_POOR || band == TRUST_LAST_CHANCE
}

/// Refusals dispatch takes off the budget as trust falls.
pub fn trust_decline_penalty(reputation: f64) -> i64 {
    band_decline_penalty(trust_band(reputation))
}

/// [`trust_decline_penalty`] for a band (see [`board_offers_for_band`]).
pub fn band_decline_penalty(band: &str) -> i64 {
    if band == TRUST_GUARDED {
        return 1;
    }
    if band == TRUST_POOR {
        return 2;
    }
    if band == TRUST_LAST_CHANCE {
        return 99; // no refusals at all: take it or leave the job
    }
    0
}

/// What a trust band means, without saying what brings it back.
///
/// Split out because "clean on-time runs rebuild it" stopped being true once
/// money and the license fed the same band: a driver whose service is fine
/// and whose debt is the problem can run clean forever and watch nothing
/// move. The way back has to name the thing that is actually holding it.
pub fn trust_band_text(band: &str) -> &'static str {
    if band == TRUST_FULL {
        return "Dispatch trust: full. You get the whole board.";
    }
    if band == TRUST_GUARDED {
        return "Dispatch trust: guarded. Dispatch is holding back some of the \
                freight and allows fewer refusals.";
    }
    if band == TRUST_POOR {
        return "Dispatch trust: poor. You are back to assigned loads whatever your \
                level, the board is down to two, and the good freight is going to \
                other drivers.";
    }
    "Dispatch trust: last chance. One assigned load at a time, no refusals, \
     and the carrier is deciding whether to keep you."
}

/// What a trust band means, after its label: the sentences that follow
/// "Dispatch trust: full." Each is a whole literal, so a translation finds
/// it when a reputation number stands between the label and the meaning.
pub fn trust_band_meaning(band: &str) -> &'static str {
    if band == TRUST_FULL {
        return "You get the whole board.";
    }
    if band == TRUST_GUARDED {
        return "Dispatch is holding back some of the freight and allows fewer refusals.";
    }
    if band == TRUST_POOR {
        return "You are back to assigned loads whatever your level, the board is down \
                to two, and the good freight is going to other drivers.";
    }
    "One assigned load at a time, no refusals, and the carrier is deciding whether \
     to keep you."
}

/// Where the driver stands with dispatch on service alone, in one line.
pub fn trust_text(reputation: f64) -> String {
    let band = trust_band(reputation);
    if band == TRUST_FULL {
        return trust_band_text(band).to_string();
    }
    format!("{} Clean on-time runs rebuild it.", trust_band_text(band))
}

/// [`trust_text`] in the player's preferred score-display mode.
pub fn trust_text_with_display(reputation: f64, mode: ScoreDisplay) -> String {
    let band = trust_band(reputation);
    let meaning = trust_band_meaning(band);
    let rebuild = if band == TRUST_FULL {
        ""
    } else {
        " Clean on-time runs rebuild it."
    };
    format!("{} {meaning}{rebuild}", trust_score_head(reputation, mode))
}

/// The opening of a trust line. The number is always labelled as reputation
/// so it never reads as a trust score of its own.
///
/// Each band is a whole English literal so a translation can agree in gender
/// with "confianza" without changing the bare word "full", which lane assist
/// also uses.
fn trust_score_head(reputation: f64, mode: ScoreDisplay) -> String {
    let band = trust_band(reputation);
    let number = round_py_int(reputation.clamp(0.0, 100.0));
    // Percentage-only drops reputation's band word elsewhere; trust has no
    // number of its own, so the trust band stays in Percent too.
    match mode {
        ScoreDisplay::Both | ScoreDisplay::Percent => match band {
            TRUST_FULL => {
                format!("Dispatch trust: full. Reputation {number} out of 100.")
            }
            TRUST_GUARDED => {
                format!("Dispatch trust: guarded. Reputation {number} out of 100.")
            }
            TRUST_POOR => {
                format!("Dispatch trust: poor. Reputation {number} out of 100.")
            }
            _ => format!("Dispatch trust: last chance. Reputation {number} out of 100."),
        },
        ScoreDisplay::Description => match band {
            TRUST_FULL => "Dispatch trust: full.".to_string(),
            TRUST_GUARDED => "Dispatch trust: guarded.".to_string(),
            TRUST_POOR => "Dispatch trust: poor.".to_string(),
            _ => "Dispatch trust: last chance.".to_string(),
        },
    }
}

// -- the whole picture: service, license, and money -------------------------

// Dispatch trust is one ladder with three inputs, not three ladders. Service
// has always fed it. The license and what the driver owes now feed the same
// rungs, because a yard that will not give a driver its good freight is the
// same yard that will not give them its good iron, for the same reasons.
//
// It stays one ladder on purpose: a second parallel status would cost a screen
// reader user a whole extra concept to hold, and there is only one question
// behind all of it -- how much is this carrier willing to put in your hands.

/// Worst first, so the binding input is simply the minimum.
const BAND_ORDER: [&str; 4] = [TRUST_LAST_CHANCE, TRUST_POOR, TRUST_GUARDED, TRUST_FULL];

pub const CAUSE_SERVICE: &str = "service";
pub const CAUSE_LICENCE: &str = "licence";
pub const CAUSE_DEBT: &str = "debt";
pub const CAUSE_RECORD: &str = "record";

fn band_index(band: &str) -> usize {
    BAND_ORDER
        .iter()
        .position(|known| *known == band)
        .unwrap_or_else(|| panic!("{band:?} is not a trust band"))
}

/// `min(bands, key=_BAND_ORDER.index)`: the worst of the given bands.
pub fn worst_band(bands: &[&str]) -> &'static str {
    let worst = bands
        .iter()
        .copied()
        .min_by_key(|band| band_index(band))
        .expect("worst_band() of at least one band");
    BAND_ORDER[band_index(worst)]
}

/// A license that is not valid takes the seat, whatever the service record.
///
/// Only a suspension counts, never a serious violation on its own. A
/// suspension is a fact with a date on it that the driver can already hear
/// and wait out; violations age off over three game years, and gating the
/// equipment on those would be a hold no amount of good driving could clear.
/// Those already reach dispatch trust the honest way, through reputation.
pub fn licence_band<P: StandingProfile + ?Sized>(profile: &P) -> &'static str {
    let Some(record) = profile.driving_record() else {
        return TRUST_FULL;
    };
    if record.suspended(profile.game_hours()) {
        TRUST_LAST_CHANCE
    } else {
        TRUST_FULL
    }
}

/// How far what the driver owes has eaten into the carrier's patience.
pub fn solvency_band<P: StandingProfile + ?Sized>(profile: &P) -> &'static str {
    [TRUST_FULL, TRUST_GUARDED, TRUST_POOR, TRUST_LAST_CHANCE][debt_rung(profile) as usize]
}

/// Whether the record is past the carrier's review floor.
pub fn record_over_review_floor(record: &DrivingRecord, game_hours: f64) -> bool {
    record.citations_in_window(game_hours) > CARRIER_REVIEW_CITATIONS
        || record.serious_in_review_window(game_hours) >= CARRIER_REVIEW_SERIOUS
}

/// Whether the record is past what the carrier's insurer will carry.
pub fn record_past_termination_floor(record: &DrivingRecord, game_hours: f64) -> bool {
    record.citations_in_window(game_hours) >= CARRIER_TERMINATION_CITATIONS
        || record.serious_in_review_window(game_hours) >= CARRIER_TERMINATION_SERIOUS
}

/// Whether the carrier's record review applies to this driver at all.
fn under_carrier_review<P: StandingProfile + ?Sized>(profile: &P) -> bool {
    !is_owner_operator(profile.business_status())
}

/// What the carrier's annual record review makes of this driver.
///
/// Unlike the license band, a violation on its own DOES count here -- but
/// only inside the review window, and the hold names the day the window
/// empties, so it is never a hold no amount of good driving can clear. A
/// driver already at the fleet of last resort cannot be let go again, so the
/// termination floor reads as poor trust there instead of full.
pub fn record_band<P: StandingProfile + ?Sized>(profile: &P) -> &'static str {
    if !under_carrier_review(profile) {
        return TRUST_FULL;
    }
    let Some(record) = profile.driving_record() else {
        return TRUST_FULL;
    };
    let game_hours = profile.game_hours();
    if record_past_termination_floor(record, game_hours) {
        TRUST_POOR
    } else if record_over_review_floor(record, game_hours) {
        TRUST_GUARDED
    } else {
        TRUST_FULL
    }
}

/// The band the carrier actually acts on: the worst of the four inputs.
///
/// Internal name only. Spoken text calls this dispatch trust, because that is
/// the noun the game already uses and `docs/ontology.md` already rules
/// "standing" out as a synonym for it.
pub fn standing_band<P: StandingProfile + ?Sized>(profile: &P) -> &'static str {
    worst_band(&[
        trust_band(profile.career_reputation()),
        licence_band(profile),
        record_band(profile),
        solvency_band(profile),
    ])
}

/// Which input is holding the band down. Empty when nothing is.
pub fn standing_cause<P: StandingProfile + ?Sized>(profile: &P) -> &'static str {
    let band = standing_band(profile);
    if band == TRUST_FULL {
        return "";
    }
    if licence_band(profile) == band {
        return CAUSE_LICENCE;
    }
    if record_band(profile) == band {
        return CAUSE_RECORD;
    }
    if solvency_band(profile) == band {
        return CAUSE_DEBT;
    }
    CAUSE_SERVICE
}

/// "three citations and one serious violation in the last year".
pub fn record_window_phrase(record: &DrivingRecord, game_hours: f64) -> String {
    let citations = record.citations_in_window(game_hours);
    let serious = record.serious_in_review_window(game_hours);
    let mut parts = Vec::new();
    if citations > 0 {
        let noun = if citations == 1 {
            "citation"
        } else {
            "citations"
        };
        parts.push(format!("{} {noun}", count_word(citations)));
    }
    if serious > 0 {
        let noun = if serious == 1 {
            "serious violation"
        } else {
            "serious violations"
        };
        parts.push(format!("{} {noun}", count_word(serious)));
    }
    if parts.is_empty() {
        return String::new();
    }
    format!("{} in the last year", parts.join(" and "))
}

/// The spoken calendar day the record stops meeting `holds(citations,
/// serious)` as its violations age out, with nothing new added.
pub fn record_clears_text<P: StandingProfile + ?Sized>(
    profile: &P,
    holds: impl Fn(i64, i64) -> bool,
) -> String {
    let record = record_of(profile);
    let Some(at) = record.window_clears_at(profile.game_hours(), holds) else {
        return String::new();
    };
    calendar_day_text(profile, at)
}

/// A point on the career clock as the spoken day the player will reach it.
///
/// Counted forward from the date the player hears now, never from the
/// career's own calendar: with live weather driving the calendar the two
/// differ, and a suspension once "cleared" on a day already gone
/// (reported 2026-09-29). A day a new year away says so, or a year-long
/// disqualification would name today's own date.
fn calendar_day_text<P: StandingProfile + ?Sized>(profile: &P, career_hours: f64) -> String {
    let now = profile.calendar_now_hours();
    let at = now + (career_hours - profile.game_hours()).max(0.0);
    let year = |h: f64| ((CAREER_START_DAY_OF_YEAR + h / HOURS_PER_DAY) / DAYS_PER_YEAR).floor();
    let when = match (year(at) - year(now)) as i64 {
        0 => "",
        1 => ", next year",
        _ => ", the year after next",
    };
    let weekday = if profile.calendar_is_live() {
        crate::sim::season::real_weekday_name(at, None)
    } else {
        weekday_name(at)
    };
    format!("{weekday}, {}{when}", date_text(at))
}

/// The day the carrier's record review lets go: back under its floor.
pub fn record_review_clears_text<P: StandingProfile + ?Sized>(profile: &P) -> String {
    record_clears_text(profile, |citations, serious| {
        citations > CARRIER_REVIEW_CITATIONS || serious >= CARRIER_REVIEW_SERIOUS
    })
}

/// The insurer's multiplier on an owner-operator's insurance reserve.
///
/// 1.0 for a clean window, for a company driver (the carrier's policy, not
/// theirs), and for a profile with no record at all.
pub fn record_insurance_surcharge<P: StandingProfile + ?Sized>(profile: &P) -> f64 {
    if !is_owner_operator(profile.business_status()) {
        return 1.0;
    }
    let Some(record) = profile.driving_record() else {
        return 1.0;
    };
    let game_hours = profile.game_hours();
    let raw = 1.0
        + INSURANCE_SURCHARGE_PER_CITATION * record.citations_in_window(game_hours) as f64
        + INSURANCE_SURCHARGE_PER_SERIOUS * record.serious_in_review_window(game_hours) as f64;
    round_py_n(raw.min(INSURANCE_SURCHARGE_MAX), 2)
}

/// What the record is costing this driver right now, in one sentence, or
/// nothing when it costs nothing. The carrier's review for a company driver,
/// the insurer's surcharge for an owner-operator.
pub fn record_consequence_text<P: StandingProfile + ?Sized>(profile: &P) -> String {
    let Some(record) = profile.driving_record() else {
        return String::new();
    };
    let game_hours = profile.game_hours();
    if is_owner_operator(profile.business_status()) {
        let surcharge = record_insurance_surcharge(profile);
        if surcharge <= 1.0 {
            return String::new();
        }
        let percent = round_py_int((surcharge - 1.0) * 100.0);
        let until = record_clears_text(profile, |citations, serious| citations + serious > 0);
        if until.is_empty() {
            return format!("Insurance surcharge: {percent} percent.");
        }
        return format!("Insurance surcharge: {percent} percent until {until}.");
    }
    if !under_carrier_review(profile) {
        return String::new();
    }
    if record_past_termination_floor(record, game_hours) {
        if profile.carrier_key() == LAST_CHANCE_CARRIER_KEY {
            return format!(
                "The carrier's insurer will not carry a record like that; {} keeps you on \
                 sufferance until {}.",
                LAST_CHANCE_CARRIER_NAME,
                record_clears_text(profile, |citations, serious| {
                    citations >= CARRIER_TERMINATION_CITATIONS
                        || serious >= CARRIER_TERMINATION_SERIOUS
                })
            );
        }
        return "The carrier's insurer will not carry that record. The carrier ends your \
                employment at the next terminal."
            .to_string();
    }
    if record_over_review_floor(record, game_hours) {
        let citations = record.citations_in_window(game_hours);
        let serious = record.serious_in_review_window(game_hours);
        let next = if serious + 1 >= CARRIER_TERMINATION_SERIOUS {
            "One more serious violation and the carrier lets you go.".to_string()
        } else {
            let left = CARRIER_TERMINATION_CITATIONS - citations;
            format!(
                "{} more {} and the carrier lets you go.",
                py_capitalize_word(&count_word(left)),
                if left == 1 { "citation" } else { "citations" }
            )
        };
        return format!(
            "The carrier's record review holds your equipment back until {}. {next}",
            record_review_clears_text(profile)
        );
    }
    String::new()
}

// -- the career that is over ------------------------------------------------
//
// A second major offense disqualifies the CDL for life (49 CFR 383.51 Table
// 1), and the conviction stays on the state record for 55 years (49 CFR
// 384.225). So the career ends the way a real one does: the driving stops and
// the record stays. Nothing is deleted by the game. The save can be opened
// and read, its stats, journal and achievements stand, and its cloud backup
// keeps the last verified career on the public profile. Removing all of that
// is the player's own act, "Close out this career", and nothing else.

/// The setback-notice kind for the lifetime disqualification, read once at
/// the terminal like a termination or a repossession.
pub const SETBACK_DISQUALIFICATION: &str = "disqualification";

/// Whether this career can never drive again.
pub fn career_ended<P: StandingProfile + ?Sized>(profile: &P) -> bool {
    profile
        .driving_record()
        .is_some_and(|record| record.lifetime_disqualified)
}

/// The terminal greeting's sentence for a career that is over.
pub fn career_ended_text() -> &'static str {
    "Your CDL is disqualified for life, so this career cannot drive again. Close out this \
     career removes the save."
}

/// The notice the terminal reads once, the first time the driver comes in
/// after the disqualification. Long on purpose, like the termination and
/// repossession notices: the most consequential thing the game ever says
/// should not be a line gone to the first keypress.
pub fn disqualification_notice_lines() -> Vec<String> {
    vec![
        "That was the second major offense on your CDL. Under the federal rules a second \
         major offense disqualifies a commercial license for life, and there is no waiting \
         it out."
            .to_string(),
        "No carrier can put you in a seat, the dispatch board is closed to you, and the \
         owner-operator buy-in is off the table. Rest, the garage, the logbook and your \
         stats still work here."
            .to_string(),
        "Nothing is taken away. Your money, your levels, your achievements, your road journal \
         and your whole record stay, and this career can be opened and read any time."
            .to_string(),
        "When you are done with it, Close out this career on the terminal menu removes the \
         save from this computer and every cloud backup of it from your account. Nothing \
         does that for you. When you want the road again, start a new career from the title \
         menu."
            .to_string(),
    ]
}

fn py_capitalize_word(word: &str) -> String {
    let mut chars = word.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

/// What actually brings the band up, naming the thing that is holding it.
pub fn standing_way_back<P: StandingProfile + ?Sized>(profile: &P) -> String {
    let cause = standing_cause(profile);
    if cause == CAUSE_LICENCE {
        let clears = clears_text(profile);
        if clears.is_empty() {
            return "Your CDL is disqualified for life, so the seat is not coming back."
                .to_string();
        }
        return format!(
            "Your CDL is suspended, so the yard holds your seat until it clears {clears}."
        );
    }
    if cause == CAUSE_RECORD {
        let record = record_of(profile);
        return format!(
            "Your driving record is what is holding it: {}. The carrier's review keeps it \
             there until {}; keep the record clean until then.",
            record_window_phrase(record, profile.game_hours()),
            record_review_clears_text(profile)
        );
    }
    if cause == CAUSE_DEBT {
        let line = format!(
            "You owe {}, and that is what is holding it.",
            money_text(debt_owed(profile))
        );
        if trust_band(profile.career_reputation()) != TRUST_FULL {
            return format!(
                "{line} Paying it down brings it back, and clean on-time runs help too."
            );
        }
        return format!("{line} Paying it down brings it back.");
    }
    if cause == CAUSE_SERVICE {
        return "Clean on-time runs rebuild it.".to_string();
    }
    String::new()
}

use crate::models::career::xp_rate_clause;

/// Everything dispatch trust is doing to this driver, on demand, in one go.
///
/// Spoken when the band changes and whenever the player asks for it. Never on
/// a timer, and never for a driver in full trust beyond the one plain line
/// they already heard.
pub fn dispatch_trust_line<P: StandingProfile + ?Sized>(profile: &P) -> String {
    dispatch_trust_line_with_display(profile, ScoreDisplay::Both)
}

/// [`dispatch_trust_line`] in the player's preferred score-display mode.
pub fn dispatch_trust_line_with_display<P: StandingProfile + ?Sized>(
    profile: &P,
    mode: ScoreDisplay,
) -> String {
    let band = standing_band(profile);
    let service_sets_band = band == trust_band(profile.career_reputation());
    let mut parts = vec![if service_sets_band {
        let suffix = trust_band_text(band)
            .strip_prefix(&format!("Dispatch trust: {band}."))
            .unwrap_or("");
        format!(
            "{}{suffix}",
            trust_score_head(profile.career_reputation(), mode)
        )
    } else {
        trust_band_text(band).to_string()
    }];
    let way_back = standing_way_back(profile);
    if !way_back.is_empty() {
        parts.push(way_back);
    }
    let hold = profile.equipment_hold_clause();
    if !hold.is_empty() {
        parts.push(hold);
    }
    let rate = xp_rate_clause(band);
    if !rate.is_empty() {
        parts.push(rate);
    }
    parts.join(" ")
}

/// Why the board is short, said plainly. Empty when it is not short.
pub fn board_reputation_note(reputation: f64) -> String {
    if trust_band(reputation) == TRUST_FULL {
        String::new()
    } else {
        trust_text(reputation)
    }
}

/// A company driver the carrier will not keep on the insurance any longer.
pub fn carrier_termination_due<P: StandingProfile + ?Sized>(profile: &P) -> bool {
    if is_owner_operator(profile.business_status()) {
        return false;
    }
    if profile.carrier_key() == LAST_CHANCE_CARRIER_KEY {
        return false; // already at the fleet of last resort; nowhere further down
    }
    if profile.career_reputation() < REPUTATION_TERMINATION {
        return true;
    }
    // The annual record review, 49 CFR 391.25: a record the insurer will not
    // carry ends the employment whatever the service record says.
    profile
        .driving_record()
        .is_some_and(|record| record_past_termination_floor(record, profile.game_hours()))
}

// -- spoken standing --------------------------------------------------------

const COUNT_WORDS: [&str; 10] = [
    "no", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine",
];
const ORDINAL_WORDS: [&str; 8] = [
    "", "first", "second", "third", "fourth", "fifth", "sixth", "seventh",
];

pub fn count_word(n: i64) -> String {
    if (0..COUNT_WORDS.len() as i64).contains(&n) {
        COUNT_WORDS[n as usize].to_string()
    } else {
        n.to_string()
    }
}

pub fn ordinal_word(n: i64) -> String {
    if n > 0 && n < ORDINAL_WORDS.len() as i64 {
        ORDINAL_WORDS[n as usize].to_string()
    } else {
        format!("{n}th")
    }
}

/// A suspension length in game days, never raw hours.
pub fn days_text(days: f64) -> String {
    let whole = round_py_int(days).max(1);
    if whole == 1 {
        "1 day".to_string()
    } else {
        format!("{whole} days")
    }
}

/// When the suspension clears, as a spoken game-calendar date.
pub fn clears_text<P: StandingProfile + ?Sized>(profile: &P) -> String {
    let record = record_of(profile);
    if record.lifetime_disqualified {
        return String::new();
    }
    calendar_day_text(profile, record.suspended_until_h)
}

/// A serious-violation ladder suspends; a major offense disqualifies.
pub fn status_verb(record: &DrivingRecord) -> &'static str {
    if record.suspension_reason == SUSPENSION_MAJOR {
        "disqualified"
    } else {
        "suspended"
    }
}

/// One spoken line of where this driver stands. Asked for, never on a timer.
pub fn standing_text<P: StandingProfile + ?Sized>(profile: &P) -> String {
    let record = record_of(profile);
    let game_hours = profile.game_hours();
    if record.lifetime_disqualified {
        return "Record: your CDL is disqualified for life. You cannot take driving work."
            .to_string();
    }
    if record.suspended(game_hours) {
        let left = days_text(record.days_left(game_hours));
        let verb = status_verb(record);
        return format!(
            "Record: CDL {verb}, {left} remaining. It clears {}.",
            clears_text(profile)
        );
    }
    let citations = record.citations_in_window(game_hours);
    let incidents = record.safety_incidents_within(game_hours, REVIEW_WINDOW_DAYS);
    let crashes = record.crashes_within(game_hours, REVIEW_WINDOW_DAYS);
    let serious = record.serious_in_window(game_hours);
    let majors = record.major_count();
    if citations == 0 && incidents == 0 && crashes == 0 && serious == 0 && majors == 0 {
        return "Record: clean.".to_string();
    }
    let mut parts = Vec::new();
    // Plain citations were never spoken here, which is how a driver with a
    // string of them came to ask what the record was for (owner, 2026-09-12).
    // A safety incident and a crash are on the record too: the list below
    // this line names them, and the line said "clean" over them (issue #347).
    let mut last_year = Vec::new();
    for (count, one, many) in [
        (citations, "citation", "citations"),
        (incidents, "safety incident", "safety incidents"),
        (crashes, "crash", "crashes"),
    ] {
        if count != 0 {
            let noun = if count == 1 { one } else { many };
            last_year.push(format!("{} {noun}", count_word(count)));
        }
    }
    if !last_year.is_empty() {
        let join = match last_year.split_last() {
            Some((last, rest)) if !rest.is_empty() => {
                let join = rest.join(", ");
                format!("{join} and {last}")
            }
            _ => last_year.concat(),
        };
        parts.push(format!("{join} in the last year"));
    }
    if serious != 0 {
        let noun = if serious == 1 {
            "serious violation"
        } else {
            "serious violations"
        };
        parts.push(format!("{} {noun}", count_word(serious)));
    }
    if majors != 0 {
        let noun = if majors == 1 {
            "major offense"
        } else {
            "major offenses"
        };
        parts.push(format!("{} {noun}", count_word(majors)));
    }
    let tail = if majors >= 1 {
        " One more major offense disqualifies your CDL for life."
    } else if serious == 1 {
        " One more before your CDL is suspended for 60 days."
    } else {
        ""
    };
    let consequence = record_consequence_text(profile);
    let consequence = if consequence.is_empty() {
        String::new()
    } else {
        format!(" {consequence}")
    };
    format!("Record: {}.{tail}{consequence}", parts.join(", "))
}

/// The first thing the dispatch board says while the CDL is not valid.
pub fn suspension_board_line<P: StandingProfile + ?Sized>(profile: &P) -> String {
    let record = record_of(profile);
    if record.lifetime_disqualified {
        return "Dispatch board. Your CDL is disqualified for life, so there is no \
                driving work here. The board is listed for reference only."
            .to_string();
    }
    format!(
        "Dispatch board. Your CDL is {}. Driving jobs return {}.",
        status_verb(record),
        clears_text(profile)
    )
}

/// Why a job cannot be taken, said once, with the way back.
pub fn suspension_refusal_line<P: StandingProfile + ?Sized>(profile: &P) -> String {
    let record = record_of(profile);
    if record.lifetime_disqualified {
        return "You cannot take driving work with a lifetime CDL disqualification.".to_string();
    }
    format!(
        "You cannot take this job while your CDL is {}. It clears {}.",
        status_verb(record),
        clears_text(profile)
    )
}

/// Why a bobtail (a drive with no trailer to another board) is refused while
/// the CDL is pulled. A bobtail is still driving a commercial vehicle.
pub fn suspension_drive_refusal_line<P: StandingProfile + ?Sized>(profile: &P) -> String {
    let record = record_of(profile);
    if record.lifetime_disqualified {
        return "You cannot drive with a lifetime CDL disqualification, not even bobtail."
            .to_string();
    }
    let verb = status_verb(record);
    let noun = if verb == "disqualified" {
        "disqualification"
    } else {
        "suspension"
    };
    format!(
        "You cannot drive, not even bobtail, while your CDL is {verb}. The {noun} ends {}.",
        clears_text(profile)
    )
}

/// The CDL line on the career screens: short, factual, always available.
pub fn career_menu_status<P: StandingProfile + ?Sized>(profile: &P) -> String {
    let record = record_of(profile);
    let game_hours = profile.game_hours();
    if record.lifetime_disqualified {
        return "CDL: disqualified for life".to_string();
    }
    if record.suspended(game_hours) {
        let left = days_text(record.days_left(game_hours));
        return format!("CDL: {}, {left} remaining", status_verb(record));
    }
    "CDL: clear".to_string()
}
