//! Career progression: experience, levels, endorsements, and reputation
//! (port of `freight_fate/models/career.py`).
//!
//! This module also hosts the two read-only views the career-side modules
//! (`carrier_fleet`, `dispatch_policy`, `career_training`,
//! `career_level_guidance`, `career_objectives`, `trailer_yard`) take in
//! place of the Python `Profile` and `Job` objects, which arrive in wave 2:
//! [`CareerProfile`] and [`JobView`]. Every method is one `getattr` the
//! Python did, with the same default where it had one. (`trailer_yard` reads
//! the profile through its own two-method `TrailerOwner`.)

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::models::career_ladder::{
    next_rank_for_level, rank_for_level, CareerRank, MAX_CAREER_LEVEL,
};
use crate::models::credentials::{credential, CREDENTIALS};
use crate::models::enforcement::StandingProfile;
use crate::pyfmt::{fmt_f, fmt_grouped};

#[cfg(test)]
mod pacing_tests;
#[cfg(test)]
pub(crate) mod test_profile;
#[cfg(test)]
mod tests;

/// XP needed for each level in the 30-level career ladder. The first 20
/// thresholds stay fixed so existing saves keep their current level. Levels
/// 21 through 30 are new on the 1.9 line and are paced so each late rank
/// lands after a solid evening or two of freight instead of a lost weekend:
/// the whole arc is a months-long career, but every level keeps paying out.
pub const LEVEL_XP: [f64; 30] = [
    0.0, 1000.0, 2500.0, 4500.0, 7000.0, 10_000.0, 14_000.0, 19_000.0, 25_000.0, 32_000.0,
    40_000.0, 50_000.0, 62_000.0, 76_000.0, 92_000.0, 110_000.0, 130_000.0, 152_000.0, 176_000.0,
    202_000.0, 216_000.0, 231_000.0, 247_000.0, 264_000.0, 282_000.0, 301_000.0, 321_000.0,
    342_000.0, 364_000.0, 387_000.0,
];

/// The credential a carrier sponsors at a level, if any: the carrier pays
/// for the training once dispatch trusts you with the freight.
pub fn endorsement_level(key: &str) -> Option<i64> {
    credential(key).and_then(|c| c.grant_level)
}

/// The self-paid course price: paying for the course yourself earns a
/// credential before the carrier would sponsor it -- real drivers buy their
/// own training to get ahead.
pub fn endorsement_course_cost(key: &str) -> Option<f64> {
    credential(key).map(|c| c.course_cost)
}

// Experience scales with what the freight demands, not just its miles:
// specialty (endorsement) cargo and premium mid-level cargo teach more per
// mile, a run of consecutive on-time deliveries compounds the lesson, and
// bringing the cargo in undamaged proves the lesson stuck. Every settled
// load also teaches a flat slice of the trade -- docks, paperwork, people --
// so short early hauls still move the career.
//
// Cloud-save screening re-derives the highest XP an honest career could hold
// from these, via profile_integrity_invariants. Change any of them and the
// export must be regenerated: the server once kept its own 1.2 per mile,
// which these rates outgrew, and honest drivers had backups refused for it.
pub const XP_SPECIALTY_MULT: f64 = 1.5;
pub const XP_PREMIUM_MULT: f64 = 1.25;
/// Extra share per consecutive on-time delivery.
pub const XP_STREAK_STEP: f64 = 0.05;
pub const XP_STREAK_MAX_BONUS: f64 = 0.45;
/// Bonus share for delivering with no real damage.
pub const XP_CLEAN_BONUS: f64 = 0.15;
/// Flat lesson per settled load (halved late).
pub const DELIVERY_COMPLETION_XP: f64 = 150.0;
pub const XP_PER_MILE_ON_TIME: f64 = 1.6;
pub const XP_PER_MILE_LATE: f64 = 0.9;

// Nobody grooms a driver on a final warning for promotion. A carrier that has
// stopped trusting a driver puts them on routine freight and stops investing
// in them, and the career moves slower for it.
//
// Three rules bind these numbers. A driver in full trust is at exactly 1.0, so
// the tuned arc to level 30 does not move a minute for anyone running clean.
// Nothing ever reaches zero, because a driver digging out has to be able to
// make progress or the career is a trap with a menu. And every rate is at or
// below 1.0, so the XP ceiling exported to cloud-save screening still bounds
// every honest career.
//
// Keyed by the spoken band strings from ``enforcement`` rather than importing
// them, which would close an import cycle. A test pins the two together.
pub const STANDING_XP_RATE: &[(&str, f64)] = &[
    ("full", 1.0),
    ("guarded", 0.9),
    ("poor", 0.75),
    ("last chance", 0.6),
];

/// How fast the career moves at this level of dispatch trust.
pub fn standing_xp_rate(band: &str) -> f64 {
    STANDING_XP_RATE
        .iter()
        .find(|(b, _)| *b == band)
        .map(|(_, rate)| *rate)
        .unwrap_or(1.0)
}

/// One sentence saying the career has slowed, and why. Empty when it has not.
///
/// Never the number. A multiplier a player cannot check turns every
/// settlement into arithmetic and reads as an accusation; what they need is
/// the cause and the way out, which they can act on.
pub fn xp_rate_clause(band: &str) -> String {
    if standing_xp_rate(band) >= 1.0 {
        return String::new();
    }
    // Whole literals per band so a translation can agree with "confianza"
    // without changing the bare word "full" that lane assist also uses.
    match band {
        "guarded" => "While dispatch trust is guarded, experience comes slower.".to_string(),
        "poor" => "While dispatch trust is poor, experience comes slower.".to_string(),
        _ => "While dispatch trust is last chance, experience comes slower.".to_string(),
    }
}

/// The same fact as a clause, to ride an existing settlement line.
pub fn xp_rate_settlement_clause(band: &str) -> String {
    if standing_xp_rate(band) >= 1.0 {
        return String::new();
    }
    match band {
        "guarded" => "at the slower rate that comes with guarded dispatch trust".to_string(),
        "poor" => "at the slower rate that comes with poor dispatch trust".to_string(),
        _ => "at the slower rate that comes with last chance dispatch trust".to_string(),
    }
}

/// What `xp_class_multiplier` reads off a `Cargo` (`models::jobs`, wave 2):
/// `getattr(cargo, "endorsement", None)` and `getattr(cargo, "min_level", 1)`.
// TODO(lead): implement for models::jobs::Cargo.
pub trait XpCargo {
    /// The endorsement this cargo needs, or `None` for plain freight.
    fn endorsement(&self) -> Option<&str>;
    /// The level the cargo first appears at (default 1).
    fn min_level(&self) -> i64 {
        1
    }
}

/// How much more a delivery teaches, by cargo demands.
pub fn xp_class_multiplier<C: XpCargo + ?Sized>(cargo: &C) -> f64 {
    if cargo.endorsement().is_some_and(|e| !e.is_empty()) {
        return XP_SPECIALTY_MULT;
    }
    if cargo.min_level() >= 2 {
        return XP_PREMIUM_MULT;
    }
    1.0
}

/// Bonus XP share for consecutive on-time deliveries (0 for the first).
pub fn xp_streak_bonus(streak: i64) -> f64 {
    XP_STREAK_MAX_BONUS.min(XP_STREAK_STEP * (streak - 1).max(0) as f64)
}

/// XP the on-time streak adds, capped at what the miles themselves taught.
///
/// The share applies to the whole award, flat completion XP included -- but
/// a streak can at most double the road lesson, never mint XP off the flat
/// per-delivery award. Without the cap, chaining board-minimum 25-mile hops
/// farmed the streak against a base that is nearly all completion XP. The
/// cap only binds below about 77 miles at plain freight; the honest pacing
/// model's shortest deal is 105 miles, so the contracted arc to level 30 is
/// unchanged to the float.
pub fn streak_bonus_xp(streak: i64, base_xp: f64, mileage_xp: f64) -> f64 {
    (xp_streak_bonus(streak) * base_xp).min(mileage_xp)
}

/// Spoken once when the tank and hazmat endorsements are both on the
/// license: on a real CDL the pair prints as the single letter X
/// (49 CFR 383.153(a)(9)(v)), and it is what the fuel-tanker fleet hires on.
pub const X_COMBINATION_ANNOUNCEMENT: &str =
    "You now hold both the tank vehicle and hazmat endorsements, the X \
     combination on a real license. Bulk fuel freight is open.";

/// Experience still owed before the next level, or None at the ceiling.
///
/// The summary said what level you are and what the next RANK is, but never
/// the number between them, so the one question a player actually asks --
/// how much more -- had no answer anywhere in the game (Brandon, tester
/// report 2026-08-17).
pub fn xp_to_next_level(xp: f64) -> Option<f64> {
    let level = level_for_xp(xp);
    if level >= MAX_CAREER_LEVEL {
        return None;
    }
    Some((LEVEL_XP[level as usize] - xp).max(0.0))
}

pub fn level_for_xp(xp: f64) -> i64 {
    let mut level = 1;
    for (i, threshold) in LEVEL_XP[1..].iter().enumerate() {
        if xp >= *threshold {
            level = i as i64 + 2;
        }
    }
    level.min(MAX_CAREER_LEVEL)
}

/// A credential whose course is done but whose federal background check
/// has not cleared yet: hazmat's TSA threat assessment, TWIC's enrollment.
/// The check clears on the game clock while the driver keeps working.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PendingCredential {
    pub key: String,
    /// `Profile.game_hours` at which the check clears.
    pub ready_at_h: f64,
}

/// The `Career` dataclass: saved as `dataclasses.asdict`, so the field names
/// are the JSON keys and every field has its Python default.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Career {
    pub xp: f64,
    /// 0..100: the delivery ledger. What the game shows and gates on is
    /// `Profile::standing`, this less the driving record; readers in the game
    /// go through that, never this field directly.
    pub reputation: f64,
    /// `Profile::standing` as of the last save, refreshed every time the
    /// profile is written (see `to_unsigned_dict`) so the public profile on
    /// orinks.net shows the same number the game does. Never read back.
    pub standing: f64,
    pub deliveries: i64,
    pub on_time_deliveries: i64,
    pub total_miles: f64,
    pub total_earnings: f64,
    /// Assigned-load refusals since last level-up.
    pub dispatch_declines_used: i64,
    /// Consecutive on-time deliveries.
    pub on_time_streak: i64,
    /// Self-paid courses (and cleared background-check credentials).
    pub purchased_endorsements: Vec<String>,
    /// Courses taken whose background check has not cleared yet.
    pub pending_credentials: Vec<PendingCredential>,
    /// Grants not yet repeated at a terminal. A level-up announcement is
    /// spoken once inside the delivery-summary chatter and gone; the owner
    /// once declined a reefer load he was already cleared for. Each grant
    /// is repeated on the next terminal entry, then dropped.
    pub unacknowledged_grants: Vec<String>,
}

impl Default for Career {
    fn default() -> Self {
        Career {
            xp: 0.0,
            reputation: 50.0,
            standing: 50.0,
            deliveries: 0,
            on_time_deliveries: 0,
            total_miles: 0.0,
            total_earnings: 0.0,
            dispatch_declines_used: 0,
            on_time_streak: 0,
            purchased_endorsements: Vec::new(),
            pending_credentials: Vec::new(),
            unacknowledged_grants: Vec::new(),
        }
    }
}

impl Career {
    /// `Career()`.
    pub fn new() -> Self {
        Self::default()
    }

    /// `Career(xp=...)`.
    pub fn with_xp(xp: f64) -> Self {
        Career {
            xp,
            ..Self::default()
        }
    }

    pub fn level(&self) -> i64 {
        level_for_xp(self.xp)
    }

    pub fn rank(&self) -> &'static CareerRank {
        rank_for_level(self.level())
    }

    /// Every credential held: carrier-granted (by level) and course-earned.
    /// A set, as in Python; iterate [`CREDENTIALS`] where order matters.
    /// A course still waiting on its background check is not held yet.
    pub fn endorsements(&self) -> BTreeSet<&'static str> {
        let level = self.level();
        let mut out: BTreeSet<&'static str> = CREDENTIALS
            .iter()
            .filter(|c| c.grant_level.is_some_and(|lvl| level >= lvl))
            .map(|c| c.key)
            .collect();
        for purchased in &self.purchased_endorsements {
            if let Some(cred) = credential(purchased) {
                out.insert(cred.key);
            }
        }
        out
    }

    /// Move every cleared background check onto the license. Returns the
    /// grant announcements to speak; `queue_repeat` also queues each for
    /// the terminal repeat (pass it where the announcement rides busy
    /// delivery chatter, not when it is being spoken at the terminal
    /// itself). `now_h` is the profile's `game_hours`.
    pub fn activate_pending(&mut self, now_h: f64, queue_repeat: bool) -> Vec<String> {
        let mut messages: Vec<String> = Vec::new();
        let before = self.endorsements();
        let (ready, waiting): (Vec<PendingCredential>, Vec<PendingCredential>) = self
            .pending_credentials
            .drain(..)
            .partition(|p| now_h >= p.ready_at_h);
        self.pending_credentials = waiting;
        for pending in ready {
            if !self.purchased_endorsements.contains(&pending.key) {
                self.purchased_endorsements.push(pending.key.clone());
            }
            if let Some(cred) = credential(&pending.key) {
                messages.push(cred.announcement.to_string());
                if queue_repeat {
                    self.unacknowledged_grants.push(cred.key.to_string());
                }
            }
        }
        self.push_x_combination(&before, &mut messages);
        messages
    }

    /// The X-combination line, when tank and hazmat just became a pair.
    fn push_x_combination(&self, before: &BTreeSet<&'static str>, messages: &mut Vec<String>) {
        let after = self.endorsements();
        let holds_pair = after.contains("tank") && after.contains("hazmat");
        let held_pair = before.contains("tank") && before.contains("hazmat");
        if holds_pair && !held_pair {
            messages.push(X_COMBINATION_ANNOUNCEMENT.to_string());
        }
    }

    /// The queued grant repeats for a terminal entry, drained.
    ///
    /// Each is the grant announcement again, behind a reminder lead-in, so
    /// a clearance heard once through delivery chatter is heard a second
    /// time somewhere quiet.
    pub fn take_unacknowledged_grants(&mut self) -> Vec<String> {
        let keys = std::mem::take(&mut self.unacknowledged_grants);
        keys.iter()
            .filter_map(|key| credential(key))
            .map(|cred| format!("Reminder: you hold a new {}.", cred.gate_label))
            .collect()
    }

    /// Apply a finished delivery; returns announcements (level ups etc.).
    ///
    /// `cargo_class_mult` and `standing_rate` both default to 1.0 in Python.
    /// `standing_rate` slows the career for a driver the carrier has stopped
    /// investing in. It is applied only when it is not 1.0, so a clean
    /// driver's XP is the same arithmetic it has always been, down to the
    /// float.
    pub fn record_delivery(
        &mut self,
        miles: f64,
        pay: f64,
        on_time: bool,
        damage_pct: f64,
        cargo_class_mult: f64,
        standing_rate: f64,
    ) -> Vec<String> {
        let before_level = self.level();
        let before_endorsements = self.endorsements();

        self.deliveries += 1;
        self.total_miles += miles;
        self.total_earnings += pay;
        if on_time {
            self.on_time_streak += 1;
        } else {
            self.on_time_streak = 0;
        }
        let completion = DELIVERY_COMPLETION_XP * if on_time { 1.0 } else { 0.5 };
        let per_mile = if on_time {
            XP_PER_MILE_ON_TIME
        } else {
            XP_PER_MILE_LATE
        };
        let mileage_xp = miles * per_mile * cargo_class_mult.max(1.0);
        let mut gained = completion + mileage_xp;
        if on_time {
            gained += streak_bonus_xp(self.on_time_streak, gained, mileage_xp);
        }
        if damage_pct <= 1.0 {
            gained *= 1.0 + XP_CLEAN_BONUS;
        }
        if standing_rate != 1.0 {
            gained *= standing_rate.clamp(0.0, 1.0);
        }
        self.xp += gained;
        if on_time {
            self.on_time_deliveries += 1;
            self.reputation = (self.reputation + 2.0).min(100.0);
        } else {
            self.reputation = (self.reputation - 4.0).max(0.0);
        }
        if damage_pct > 25.0 {
            self.reputation = (self.reputation - 3.0).max(0.0);
        }

        let mut messages: Vec<String> = Vec::new();
        let level = self.level();
        if level > before_level {
            // A promotion clears the assigned-load refusals dispatch remembers.
            self.dispatch_declines_used = 0;
            // A big delivery can jump several ranks at once; every rank
            // passed through gets its own line so its unlock is actually
            // heard, not just the final rank's (single-level phrasing is
            // unchanged: the loop runs once and reads identically).
            for gained_level in (before_level + 1)..=level {
                let rank = rank_for_level(gained_level);
                messages.push(format!(
                    "Level up! You are now level {gained_level}: {}. Unlock: {}",
                    rank.title, rank.unlock
                ));
            }
        }
        // `self.endorsements - before_endorsements`: a Python set difference,
        // whose iteration order is hash-dependent; the catalogue order is
        // the one stable choice.
        let after = self.endorsements();
        for cred in CREDENTIALS {
            if after.contains(cred.key) && !before_endorsements.contains(cred.key) {
                messages.push(cred.announcement.to_string());
                self.unacknowledged_grants.push(cred.key.to_string());
            }
        }
        self.push_x_combination(&before_endorsements, &mut messages);
        messages
    }

    pub fn summary(&self) -> String {
        let pct = if self.deliveries != 0 {
            100.0 * self.on_time_deliveries as f64 / self.deliveries as f64
        } else {
            100.0
        };
        let level = self.level();
        let rank = self.rank();
        let next_text = match next_rank_for_level(level) {
            Some(next_rank) => format!(" Next: level {}, {}.", next_rank.level, next_rank.title),
            None => " You are at the top career rank.".to_string(),
        };
        // The number the player is actually asking for, next to the level it
        // belongs to rather than buried at the end of a long readout.
        let owed_text = match xp_to_next_level(self.xp) {
            Some(owed) => format!(" {} more to level {}.", fmt_grouped(owed, 0), level + 1),
            None => String::new(),
        };
        format!(
            "Level {level}, {}. {} experience.{owed_text} \
             Reputation {} out of 100. \
             {} deliveries, {} percent on time. \
             {} lifetime miles, \
             {} dollars earned. \
             Career stage: {}. {}.{next_text}",
            rank.title,
            fmt_f(self.xp, 0),
            fmt_f(self.reputation, 0),
            self.deliveries,
            fmt_f(pct, 0),
            fmt_grouped(self.total_miles, 0),
            fmt_grouped(self.total_earnings, 0),
            rank.stage,
            rank.status,
        )
    }
}

// ---------------------------------------------------------------------------
// Read-only views of Profile and Job for the career-side modules.
// ---------------------------------------------------------------------------

/// What the career-side modules read off a live `Profile` beyond what
/// [`StandingProfile`] already exposes. Each method is the `getattr` (or
/// method call) the Python made, with its default.
// TODO(lead): implement for models::profile::Profile (wave 2). The three
// eligibility hooks stand in for `models::business::owner_operator_eligibility`,
// `authority_readiness_eligibility` and `authority_activation_eligibility`
// (their `eligible` flag); wire them to those functions when business lands.
pub trait CareerProfile: StandingProfile {
    /// `profile.career`.
    fn career(&self) -> &Career;
    /// `profile.name` (`"Driver"` when empty, see `carrier_fleet._driver_seed`).
    fn name(&self) -> &str;
    /// `profile.active_truck_key()`: the tractor the simulation drives.
    fn active_truck_key(&self) -> String;
    /// `business.owner_operator_eligibility(profile)[0]`.
    // TODO(lead): wire to models::business.
    fn owner_operator_eligible(&self) -> bool {
        false
    }
    /// `business.authority_readiness_eligibility(profile)[0]`.
    // TODO(lead): wire to models::business.
    fn authority_readiness_eligible(&self) -> bool {
        false
    }
    /// `business.authority_activation_eligibility(profile)[0]`.
    // TODO(lead): wire to models::business.
    fn authority_activation_eligible(&self) -> bool {
        false
    }
    /// `profile.owner_operator_declined`: the driver chose to stay a company
    /// driver with the buy-in open, so nothing steers them toward it.
    fn owner_operator_declined(&self) -> bool {
        false
    }
}

/// `business.carrier_name(profile)`: the carrier on the profile, or the
/// starter carrier when the profile carries none.
pub fn carrier_name_of<P: StandingProfile + ?Sized>(profile: &P) -> String {
    crate::models::business::carrier_name(profile)
}

/// What the career-side modules read off a `Job` (`models::jobs`, wave 2):
/// each method is one `getattr(job, ..., default)`. Defaults match the
/// Python fall-backs (`0.0` / `""`).
// TODO(lead): implement for models::jobs::Job.
pub trait JobView {
    /// `job.distance_mi`.
    fn distance_mi(&self) -> f64;
    /// `job.weight_tons`.
    fn weight_tons(&self) -> f64 {
        0.0
    }
    /// `job.deadline_game_h` (the Python also accepted a legacy `deadline_h`).
    fn deadline_game_h(&self) -> f64 {
        0.0
    }
    /// `job.cargo.key`.
    fn cargo_key(&self) -> &str {
        ""
    }
    /// `job.origin_type`.
    fn origin_type(&self) -> &str {
        ""
    }
    /// `job.origin_facility_id`.
    fn origin_facility_id(&self) -> &str {
        ""
    }
    /// `job.origin_location`.
    fn origin_location(&self) -> &str {
        ""
    }
    /// `job.destination_type`.
    fn destination_type(&self) -> &str {
        ""
    }
    /// `job.destination_facility_id`.
    fn destination_facility_id(&self) -> &str {
        ""
    }
    /// `job.destination_location`.
    fn destination_location(&self) -> &str {
        ""
    }
}
