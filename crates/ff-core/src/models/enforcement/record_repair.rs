//! One-time repairs to driving records saved by older builds.
//!
//! Barrel strikes: before full lane keeping learned to merge out of a closed
//! lane (PR #528, first in the 2026-10-28 tester build), the assist could hold
//! the truck in a coned-off lane straight into the barrels, and the driver was
//! cited for it as a serious violation. That was the game's fault, not the
//! driver's, so every barrel strike a save carries from before this repair is
//! taken back off the record and its fine refunded. A save carries no build
//! stamp, so the repair cannot tell a strike the assist caused from one the
//! driver steered into; it forgives both, once.

use super::record::RECORD_SERIOUS;
use super::{
    DrivingRecord, HOURS_PER_DAY, SERIOUS_SECOND_SUSPENSION_DAYS, SERIOUS_THIRD_SUSPENSION_DAYS,
    SUSPENSION_SERIOUS, WORK_ZONE_BARRELS_REASON,
};

/// Two record times this close are the same booking: a barrel strike writes
/// the citation, the serious violation and the entry from one game hour.
const SAME_BOOKING_H: f64 = 1e-6;

/// Remove the first time in `times` that matches `at`; whether one did.
fn remove_time(times: &mut Vec<f64>, at: f64) -> bool {
    match times.iter().position(|&t| (t - at).abs() < SAME_BOOKING_H) {
        Some(index) => {
            times.remove(index);
            true
        }
        None => false,
    }
}

impl DrivingRecord {
    /// Forgive every barrel strike the record explains, once, and return the
    /// fines to refund. Does nothing after the first call.
    ///
    /// Each strike was booked as a citation, a serious violation and an
    /// explained entry at one game hour; all three come off, along with the
    /// lifetime tallies and, when the strike was the one that suspended the
    /// CDL, the suspension it started. A strike older than the entries the
    /// record keeps has no reason left to find it by and stays.
    pub fn repair_barrel_strikes(&mut self, game_hours: f64) -> f64 {
        if self.barrel_strikes_repaired {
            return 0.0;
        }
        self.barrel_strikes_repaired = true;
        let (strikes, kept): (Vec<_>, Vec<_>) = std::mem::take(&mut self.entries)
            .into_iter()
            .partition(|entry| {
                entry.kind == RECORD_SERIOUS && entry.reason == WORK_ZONE_BARRELS_REASON
            });
        self.entries = kept;
        let mut refund = 0.0;
        for strike in &strikes {
            let at = strike.game_hours;
            if remove_time(&mut self.citation_times, at) {
                self.citations = (self.citations - 1).max(0);
            }
            remove_time(&mut self.serious_violations, at);
            self.lift_suspension_from(at, game_hours);
            let fine = strike.fine.max(0.0);
            self.fines_paid = (self.fines_paid - fine).max(0.0);
            refund += fine;
        }
        refund
    }

    /// Take back the serious-violation suspension a strike at `at` started,
    /// if it is still running. Suspensions stack end to end, so only the one
    /// at the end of the stack can be told apart by its length alone.
    fn lift_suspension_from(&mut self, at: f64, game_hours: f64) {
        if self.lifetime_disqualified
            || self.suspension_reason != SUSPENSION_SERIOUS
            || self.suspended_until_h <= game_hours
        {
            return;
        }
        for days in [
            SERIOUS_SECOND_SUSPENSION_DAYS,
            SERIOUS_THIRD_SUSPENSION_DAYS,
        ] {
            let length = days as f64 * HOURS_PER_DAY;
            if (self.suspended_until_h - length - at).abs() < SAME_BOOKING_H {
                self.suspended_until_h -= length;
                self.serve_until(game_hours);
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::enforcement::{RECORD_CITATION, WORK_ZONE_BARRELS_FINE};

    /// Book a barrel strike the way the driving state does.
    fn strike(record: &mut DrivingRecord, at: f64) {
        record.record_citation_at(WORK_ZONE_BARRELS_FINE, at);
        record.note(
            RECORD_SERIOUS,
            WORK_ZONE_BARRELS_REASON,
            WORK_ZONE_BARRELS_FINE,
            at,
            "I-70",
        );
        record.record_serious_violation(at);
    }

    fn old_save_record() -> DrivingRecord {
        DrivingRecord {
            barrel_strikes_repaired: false,
            ..DrivingRecord::default()
        }
    }

    #[test]
    fn a_new_record_has_nothing_to_repair() {
        let mut record = DrivingRecord::new();
        strike(&mut record, 100.0);
        assert_eq!(record.repair_barrel_strikes(200.0), 0.0);
        assert_eq!(record.citations, 1);
        assert_eq!(record.serious_violations.len(), 1);
    }

    #[test]
    fn a_barrel_strike_comes_off_and_its_fine_is_refunded() {
        let mut record = old_save_record();
        record.record_citation_at(250.0, 50.0);
        record.note(RECORD_CITATION, "Speeding", 250.0, 50.0, "I-44");
        strike(&mut record, 100.0);

        let refund = record.repair_barrel_strikes(200.0);

        assert_eq!(refund, WORK_ZONE_BARRELS_FINE);
        assert_eq!(record.citations, 1);
        assert_eq!(record.citation_times, vec![50.0]);
        assert!(record.serious_violations.is_empty());
        assert_eq!(record.fines_paid, 250.0);
        assert_eq!(record.entries.len(), 1);
        assert_eq!(record.entries[0].reason, "Speeding");
        assert!(record.barrel_strikes_repaired);
        assert_eq!(record.citations_in_window(200.0), 1);
        assert_eq!(record.serious_in_review_window(200.0), 0);
    }

    #[test]
    fn the_repair_runs_once() {
        let mut record = old_save_record();
        strike(&mut record, 100.0);
        record.repair_barrel_strikes(200.0);
        strike(&mut record, 300.0);
        assert_eq!(record.repair_barrel_strikes(400.0), 0.0);
        assert_eq!(record.serious_violations, vec![300.0]);
    }

    #[test]
    fn a_suspension_the_strike_started_is_lifted() {
        let mut record = old_save_record();
        record.record_serious_violation(10.0);
        strike(&mut record, 100.0);
        assert!(record.suspended(150.0));

        record.repair_barrel_strikes(150.0);

        assert!(!record.suspended(150.0));
        assert_eq!(record.suspension_reason, "");
        assert_eq!(record.serious_violations, vec![10.0]);
    }

    #[test]
    fn a_suspension_from_another_violation_stays() {
        let mut record = old_save_record();
        strike(&mut record, 10.0);
        record.record_serious_violation(100.0);
        assert!(record.suspended(150.0));

        record.repair_barrel_strikes(150.0);

        assert!(record.suspended(150.0));
        assert_eq!(record.serious_violations, vec![100.0]);
    }
}
