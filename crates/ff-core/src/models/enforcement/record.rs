//! The license file: `DrivingRecord` and the legacy-save seeding, split out
//! of `enforcement.py` by size only.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::{
    FATIGUE_EVENTS_BEFORE_SERIOUS, HOURS_PER_DAY, MAJOR_FIRST_DISQUALIFICATION_DAYS,
    REVIEW_WINDOW_DAYS, SERIOUS_SECOND_SUSPENSION_DAYS, SERIOUS_THIRD_SUSPENSION_DAYS,
    SERIOUS_WINDOW_DAYS, SUSPENSION_LIFETIME, SUSPENSION_MAJOR, SUSPENSION_SERIOUS,
};
use crate::models::save_migration::{json_f64, json_i64};

/// One line of the record as the driver can read it back: what it was,
/// why, what it cost, when, and where. The counts and timestamps above are
/// what the ladder and the review are computed from; these are the reasons
/// behind them, kept only since this build (owner, 2026-09-14), so a
/// count can exceed the entries that explain it and the screen says so.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct RecordEntry {
    /// `RECORD_CITATION`, `RECORD_SERIOUS`, `RECORD_MAJOR`, `RECORD_FATIGUE`
    /// or `RECORD_CRASH`.
    pub kind: String,
    pub reason: String,
    pub fine: f64,
    pub game_hours: f64,
    pub place: String,
}

pub const RECORD_CITATION: &str = "citation";
pub const RECORD_SERIOUS: &str = "serious";
pub const RECORD_MAJOR: &str = "major";
pub const RECORD_FATIGUE: &str = "fatigue";
/// A crash on the accident register (49 CFR 390.15), such as a rollover.
pub const RECORD_CRASH: &str = "crash";

/// How many explained entries the record keeps; the oldest fall off first.
pub const RECORD_ENTRIES_KEPT: usize = 60;

/// What the license file remembers about this driver, for the whole career.
///
/// Times are career game hours -- the same clock `Profile.game_hours` runs
/// on -- so a suspension is served in game time and survives save and load.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct DrivingRecord {
    /// Career game hours at which each serious violation was recorded.
    pub serious_violations: Vec<f64>,
    /// Career game hours at which each major offense was recorded.
    pub major_offenses: Vec<f64>,
    /// Every spoken roadside citation, lifetime.
    pub citations: i64,
    /// Career game hours of each citation booked since the record started
    /// keeping times, newest last. `citations` is the lifetime tally; this is
    /// what a carrier's annual review (49 CFR 391.25) and an insurer's
    /// surcharge actually read, because both count a window, not a life.
    /// A citation from before this field existed has no time and is never
    /// inside any window.
    pub citation_times: Vec<f64>,
    /// Career hour the carrier's review and the insurer's surcharge started
    /// counting from. Zero for a career that started under them. A career
    /// saved before they existed gets the hour it was first loaded under
    /// them, so a serious violation it already carried -- served through the
    /// 383.51 ladder the driver was told about at the time -- cannot fire a
    /// hold or a termination the driver was never warned of on the first
    /// boot of a new build. The license ladder itself is not affected.
    pub review_started_h: f64,
    /// Lifetime enforcement money, all sources.
    pub fines_paid: f64,
    /// Times this driver ran off the road asleep.
    pub fatigue_events: i64,
    /// Career game hours of each fatigue event since this field existed.
    /// `fatigue_events` stays the lifetime tally; the safety record a scale
    /// reads counts a window, like a real carrier score.
    pub fatigue_times: Vec<f64>,
    /// Career game hours of each out-of-service order since this field
    /// existed. The lifetime count is `Profile::out_of_service_events`.
    pub out_of_service_times: Vec<f64>,
    /// Crashes on the accident register (49 CFR 390.15), lifetime.
    pub crashes: i64,
    /// Career game hours of each crash, newest last: the safety record and
    /// reputation count a window, like the other serious events.
    pub crash_times: Vec<f64>,
    /// The trust band the driver has already been told about, so a change is
    /// spoken once when it happens and never repeated on a timer.
    pub trust_band_heard: String,
    /// The debt warning the driver has already been given, on the same
    /// discipline: rungs are spoken when they move, never on a timer.
    pub debt_rung_heard: i64,
    /// Times a lender has taken an owner-operator's tractor back.
    pub repossessions: i64,
    /// A career-changing setback that has happened but not yet been read: the
    /// lines are composed when it lands and kept until the driver acknowledges
    /// them, so the longest and most consequential text in the game survives a
    /// keypress, a save, and a reload.
    /// `""` | `"termination"` | `"repossession"`.
    pub setback_notice_kind: String,
    pub setback_notice_lines: Vec<String>,
    /// Career game hours the CDL comes back.
    pub suspended_until_h: f64,
    /// `SUSPENSION_SERIOUS` / `SUSPENSION_MAJOR`.
    pub suspension_reason: String,
    pub lifetime_disqualified: bool,
    pub carrier_terminations: i64,
    /// A career that predates the record loaded with offenses already on it and
    /// has not yet heard the one-time explanation of where it now stands.
    pub notice_pending: bool,
    /// Career game hour the inspection decal on the windshield expires; zero
    /// when there is none. A clean Level 1 inspection earns it, and an open
    /// scale waves a decaled truck through unless the record is targeted
    /// (CVSA Operational Policy 5).
    pub decal_until_h: f64,
    /// The explained entries, oldest first (see [`RecordEntry`]).
    pub entries: Vec<RecordEntry>,
}

impl DrivingRecord {
    pub fn new() -> Self {
        Self::default()
    }

    // -- reads --------------------------------------------------------------

    /// Serious violations still inside the three-year counting window.
    pub fn serious_in_window(&self, game_hours: f64) -> i64 {
        let cutoff = game_hours - SERIOUS_WINDOW_DAYS as f64 * HOURS_PER_DAY;
        self.serious_violations
            .iter()
            .filter(|&&at| at >= cutoff)
            .count() as i64
    }

    pub fn major_count(&self) -> i64 {
        self.major_offenses.len() as i64
    }

    /// The start of the carrier's review window: a game year back, or the
    /// hour the review began for this career, whichever is later.
    fn review_cutoff(&self, game_hours: f64) -> f64 {
        self.cutoff_days_back(game_hours, REVIEW_WINDOW_DAYS)
    }

    /// `days` back from now, floored at the hour the review began, so a
    /// window of any length never reaches a violation the driver was never
    /// warned counted.
    fn cutoff_days_back(&self, game_hours: f64, days: i64) -> f64 {
        (game_hours - days as f64 * HOURS_PER_DAY).max(self.review_started_h)
    }

    fn count_within(&self, times: &[f64], game_hours: f64, days: i64) -> i64 {
        let cutoff = self.cutoff_days_back(game_hours, days);
        times.iter().filter(|&&at| at >= cutoff).count() as i64
    }

    /// Citations inside the last `days`, since the review began.
    pub fn citations_within(&self, game_hours: f64, days: i64) -> i64 {
        self.count_within(&self.citation_times, game_hours, days)
    }

    /// Serious violations inside the last `days`, since the review began.
    pub fn serious_within(&self, game_hours: f64, days: i64) -> i64 {
        self.count_within(&self.serious_violations, game_hours, days)
    }

    /// Fatigue events inside the last `days`, since the review began.
    pub fn fatigue_within(&self, game_hours: f64, days: i64) -> i64 {
        self.count_within(&self.fatigue_times, game_hours, days)
    }

    /// Fatigue events inside the last `days` that were booked as a safety
    /// incident: the ones before running off the road asleep became a
    /// serious violation, which the serious count already holds.
    pub fn safety_incidents_within(&self, game_hours: f64, days: i64) -> i64 {
        let cutoff = self.cutoff_days_back(game_hours, days);
        let untimed = self.fatigue_events - self.fatigue_times.len() as i64;
        self.fatigue_times
            .iter()
            .enumerate()
            .filter(|&(i, &at)| {
                at >= cutoff && untimed + (i as i64) < FATIGUE_EVENTS_BEFORE_SERIOUS - 1
            })
            .count() as i64
    }

    /// Out-of-service orders inside the last `days`, since the review began.
    pub fn out_of_service_within(&self, game_hours: f64, days: i64) -> i64 {
        self.count_within(&self.out_of_service_times, game_hours, days)
    }

    /// Citations still inside the window a carrier reviews.
    pub fn citations_in_window(&self, game_hours: f64) -> i64 {
        self.citations_within(game_hours, REVIEW_WINDOW_DAYS)
    }

    /// Serious violations the carrier's review and the insurer count: inside
    /// the review window AND since the review began. Distinct from
    /// [`Self::serious_in_window`], which is the 383.51 license ladder, looks
    /// back three years and counts every one.
    pub fn serious_in_review_window(&self, game_hours: f64) -> i64 {
        self.serious_within(game_hours, REVIEW_WINDOW_DAYS)
    }

    /// The career hour at which the window stops meeting `holds(citations,
    /// serious)`, with nothing new added, or `None` when it already does not.
    /// Each event leaves the window a year after it, so walk the expiries in
    /// order. This is the date a record-based hold can honestly promise: the
    /// oldest event leaving is not it when the rest still meet the floor.
    pub fn window_clears_at(
        &self,
        game_hours: f64,
        holds: impl Fn(i64, i64) -> bool,
    ) -> Option<f64> {
        let window = REVIEW_WINDOW_DAYS as f64 * HOURS_PER_DAY;
        let cutoff = self.review_cutoff(game_hours);
        let mut events: Vec<(f64, bool)> = self
            .citation_times
            .iter()
            .map(|&at| (at, false))
            .chain(self.serious_violations.iter().map(|&at| (at, true)))
            .filter(|&(at, _)| at >= cutoff)
            .collect();
        events.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut citations = events.iter().filter(|e| !e.1).count() as i64;
        let mut serious = events.len() as i64 - citations;
        if !holds(citations, serious) {
            return None;
        }
        for (at, is_serious) in events {
            if is_serious {
                serious -= 1;
            } else {
                citations -= 1;
            }
            if !holds(citations, serious) {
                return Some(at + window);
            }
        }
        None
    }

    pub fn suspended(&self, game_hours: f64) -> bool {
        self.lifetime_disqualified || game_hours < self.suspended_until_h
    }

    /// Hours of suspension left; infinite for a lifetime disqualification.
    pub fn hours_left(&self, game_hours: f64) -> f64 {
        if self.lifetime_disqualified {
            return f64::INFINITY;
        }
        (self.suspended_until_h - game_hours).max(0.0)
    }

    pub fn days_left(&self, game_hours: f64) -> f64 {
        let left = self.hours_left(game_hours);
        if left == f64::INFINITY {
            left
        } else {
            left / HOURS_PER_DAY
        }
    }

    /// No standing a player would want explained to them.
    pub fn clean(&self, game_hours: f64) -> bool {
        !self.lifetime_disqualified
            && !self.suspended(game_hours)
            && self.serious_in_window(game_hours) == 0
            && self.major_offenses.is_empty()
    }

    // -- writes -------------------------------------------------------------

    /// A citation with no career time: the legacy seeding path, and nothing
    /// else. Every live stop books through [`Self::record_citation_at`].
    pub fn record_citation(&mut self, fine: f64) {
        self.citations += 1;
        self.fines_paid += fine.max(0.0);
    }

    /// Keep the reason behind a count just booked. Never changes the counts
    /// or the ladder; those are the `record_*` methods' business.
    pub fn note(&mut self, kind: &str, reason: &str, fine: f64, game_hours: f64, place: &str) {
        self.entries.push(RecordEntry {
            kind: kind.to_string(),
            reason: reason.to_string(),
            fine,
            game_hours,
            place: place.to_string(),
        });
        let extra = self.entries.len().saturating_sub(RECORD_ENTRIES_KEPT);
        if extra > 0 {
            self.entries.drain(..extra);
        }
    }

    /// Citations the counts know about that no entry explains: booked before
    /// reasons were kept, or fallen off the end of the list.
    pub fn unexplained_citations(&self) -> i64 {
        let explained = self
            .entries
            .iter()
            .filter(|e| e.kind != RECORD_FATIGUE && e.kind != RECORD_CRASH)
            .count() as i64;
        (self.citations - explained).max(0)
    }

    /// Book a citation at career hour `game_hours`, so the carrier's review
    /// window and the insurer's surcharge can count it.
    pub fn record_citation_at(&mut self, fine: f64, game_hours: f64) {
        self.record_citation(fine);
        self.citation_times.push(game_hours);
    }

    /// Log a serious traffic violation; returns the count in the window.
    ///
    /// Applies the 383.51 Table 2 ladder: the second conviction inside three
    /// years suspends the CDL for 60 days, the third and every one after for
    /// 120 days.
    pub fn record_serious_violation(&mut self, game_hours: f64) -> i64 {
        self.serious_violations.push(game_hours);
        let count = self.serious_in_window(game_hours);
        if count == 2 {
            self.suspend(
                game_hours,
                SERIOUS_SECOND_SUSPENSION_DAYS,
                SUSPENSION_SERIOUS,
            );
        } else if count >= 3 {
            self.suspend(
                game_hours,
                SERIOUS_THIRD_SUSPENSION_DAYS,
                SUSPENSION_SERIOUS,
            );
        }
        count
    }

    /// Book a crash at career hour `game_hours`: the accident register a
    /// motor carrier keeps under 49 CFR 390.15, which lists every accident
    /// (390.5: an occurrence involving a commercial vehicle on a highway that
    /// results in a fatality, an injury treated away from the scene, or a
    /// vehicle towed away). A truck that rolls over is towed away.
    pub fn record_crash(&mut self, game_hours: f64) {
        self.crashes += 1;
        self.crash_times.push(game_hours);
    }

    /// Crashes inside the last `days`, since the review began.
    pub fn crashes_within(&self, game_hours: f64, days: i64) -> i64 {
        self.count_within(&self.crash_times, game_hours, days)
    }

    /// Log running off the road asleep. Returns (fatigue events, serious).
    ///
    /// The first one is a preventable safety incident: it costs standing but
    /// not the license. From the second on it is a 49 CFR 392.3 violation --
    /// operating a commercial vehicle impaired by fatigue -- and it joins the
    /// serious-violation ladder like any other.
    pub fn record_fatigue_event(&mut self, game_hours: f64) -> (i64, i64) {
        self.fatigue_events += 1;
        self.fatigue_times.push(game_hours);
        let mut serious = 0;
        if self.fatigue_events >= FATIGUE_EVENTS_BEFORE_SERIOUS {
            serious = self.record_serious_violation(game_hours);
        }
        (self.fatigue_events, serious)
    }

    /// Log a major offense; returns `SUSPENSION_MAJOR` or `SUSPENSION_LIFETIME`.
    ///
    /// Table 1: one year for the first, life for the second.
    pub fn record_major_offense(&mut self, game_hours: f64) -> &'static str {
        self.major_offenses.push(game_hours);
        if self.major_offenses.len() >= 2 {
            self.lifetime_disqualified = true;
            self.suspension_reason = SUSPENSION_LIFETIME.to_string();
            return SUSPENSION_LIFETIME;
        }
        self.suspend(
            game_hours,
            MAJOR_FIRST_DISQUALIFICATION_DAYS,
            SUSPENSION_MAJOR,
        );
        SUSPENSION_MAJOR
    }

    fn suspend(&mut self, game_hours: f64, days: i64, reason: &str) {
        // Suspensions run consecutively: a new one starts where the last one
        // ends, exactly as a state licensing agency stacks them.
        let start = game_hours.max(self.suspended_until_h);
        self.suspended_until_h = start + days as f64 * HOURS_PER_DAY;
        self.suspension_reason = reason.to_string();
    }

    /// Called when the career clock has been advanced past a suspension.
    pub fn serve_until(&mut self, game_hours: f64) {
        if !self.lifetime_disqualified && game_hours >= self.suspended_until_h {
            self.suspended_until_h = 0.0;
            self.suspension_reason = String::new();
        }
    }
}

// -- legacy careers ---------------------------------------------------------

/// Build a record for a career saved before the record existed.
///
/// No amnesty: every offense the save actually still holds is counted, and
/// the driver hears about it once. Offenses are read out of the mid-delivery
/// trip snapshot, which is the only place the old build kept them.
pub fn seed_record_from_save(data: &Map<String, Value>) -> DrivingRecord {
    let mut record = DrivingRecord::new();
    let game_hours = json_f64(data.get("game_hours"), 0.0);
    if let Some(trip) = data.get("active_trip").and_then(Value::as_object) {
        for _ in 0..json_i64(trip.get("failure_to_stop_count"), 0).max(0) {
            record.record_major_offense(game_hours);
        }
        for _ in 0..json_i64(trip.get("speeding_tickets"), 0).max(0) {
            record.record_citation(json_f64(trip.get("ticket_fines_paid"), 0.0));
        }
    }
    // `float((data.get("career") or {}).get("reputation", 50.0) or 50.0)`:
    // a missing career, a missing field and a falsy zero all read as 50.
    let reputation = match data
        .get("career")
        .and_then(Value::as_object)
        .map(|career| json_f64(career.get("reputation"), 50.0))
    {
        Some(rep) if rep != 0.0 => rep,
        _ => 50.0,
    };
    if !record.clean(game_hours) || super::trust_band(reputation) != super::TRUST_FULL {
        record.notice_pending = true;
    }
    record
}
