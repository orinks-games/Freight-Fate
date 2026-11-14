//! The profile's side of the stimulant loan (`crate::sim::stimulant`): the
//! one open loan lives as an entry in `active_buffs`, and these apply what
//! [`Loan::next`] decides to the fatigue number.

use crate::sim::stimulant::{Loan, LoanEvent, LoanOutcome};

use super::Profile;

impl Profile {
    /// The open stimulant loan, `Loan::Clear` when nothing is owed.
    pub fn stimulant_loan(&self) -> Loan {
        Loan::from_entries(&self.active_buffs)
    }

    fn set_stimulant_loan(&mut self, loan: &Loan) {
        self.active_buffs.retain(|entry| !Loan::is_entry(entry));
        if let Some(entry) = loan.to_entry() {
            self.active_buffs.push(entry);
        }
    }

    /// Take a stimulant at absolute game hour `now_h`: `lift` points off
    /// fatigue, owed back after `lasts_h`, unless a loan is already open,
    /// in which case nothing comes off and its due time holds. A loan that
    /// has come due but not yet been paid back still counts as open: it is
    /// paid back on the road with its one line, never swallowed here.
    /// Returns the points actually lifted.
    pub fn take_stimulant(&mut self, lift: f64, now_h: f64, lasts_h: f64, worn_off: &str) -> f64 {
        let (loan, outcome) = self.stimulant_loan().next(LoanEvent::Take {
            fatigue: self.fatigue,
            lift,
            now_h,
            lasts_h,
            worn_off: worn_off.to_string(),
        });
        self.set_stimulant_loan(&loan);
        match outcome {
            LoanOutcome::Lifted(amount) => {
                self.fatigue = (self.fatigue - amount).max(0.0);
                amount
            }
            _ => 0.0,
        }
    }

    /// Settle a loan due by `now_h`: its amount goes back on fatigue
    /// (capped at 100). Returns the line that marks it (possibly empty, when
    /// a buff's own worn-off line carries it), or `None` when nothing came
    /// due. A caller inside a sleep or other block of time ignores the line.
    pub fn repay_stimulant_loan(&mut self, now_h: f64) -> Option<String> {
        let (loan, outcome) = self.stimulant_loan().next(LoanEvent::Tick { now_h });
        match outcome {
            LoanOutcome::Repaid { amount, worn_off } => {
                self.set_stimulant_loan(&loan);
                self.fatigue = (self.fatigue + amount).min(100.0);
                Some(worn_off)
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::sim::stimulant::{COFFEE_BREAK_LOAN_H, COFFEE_WORN_OFF};

    #[test]
    fn a_coffee_is_repaid_once_when_it_wears_off() {
        let mut p = Profile {
            fatigue: 50.0,
            ..Default::default()
        };
        assert_eq!(
            p.take_stimulant(8.0, 10.0, COFFEE_BREAK_LOAN_H, COFFEE_WORN_OFF),
            8.0
        );
        assert_eq!(p.fatigue, 42.0);
        // A second coffee before the first wears off adds nothing.
        assert_eq!(
            p.take_stimulant(8.0, 11.0, COFFEE_BREAK_LOAN_H, COFFEE_WORN_OFF),
            0.0
        );
        assert_eq!(p.fatigue, 42.0);
        assert_eq!(p.repay_stimulant_loan(11.9), None);
        assert_eq!(
            p.repay_stimulant_loan(12.0).as_deref(),
            Some(COFFEE_WORN_OFF)
        );
        assert_eq!(p.fatigue, 50.0);
        assert_eq!(p.repay_stimulant_loan(13.0), None);
        assert!(!p.stimulant_loan().is_open());
    }

    #[test]
    fn repayment_caps_at_100_and_leaves_other_buffs_alone() {
        let mut p = Profile {
            fatigue: 50.0,
            ..Default::default()
        };
        p.add_timed_buff(json!({"group": "fatigue", "rate": 0.85, "expires_h": 12.0}));
        p.take_stimulant(3.0, 10.0, 2.0, "");
        // A meal replacing the drink leaves the loan where it was.
        p.add_timed_buff(json!({"group": "fatigue", "rate": 0.75, "expires_h": 14.0}));
        assert!(p.stimulant_loan().is_open());
        // The loan's entry is not a buff that expires and is announced.
        assert!(p.expire_buffs(11.0).is_empty());
        p.fatigue = 99.0;
        assert_eq!(p.repay_stimulant_loan(12.0).as_deref(), Some(""));
        assert_eq!(p.fatigue, 100.0);
        assert_eq!(p.active_buffs.len(), 1);
    }

    #[test]
    fn the_loan_survives_a_save_and_an_old_save_has_none() {
        super::super::tests::with_data_dir(|_| {
            let mut p = Profile {
                fatigue: 50.0,
                ..Default::default()
            };
            p.take_stimulant(8.0, 10.0, COFFEE_BREAK_LOAN_H, COFFEE_WORN_OFF);
            let loaded = Profile::from_dict(&p.to_dict());
            assert_eq!(loaded.stimulant_loan(), p.stimulant_loan());

            let mut old = Profile::default().to_dict();
            old.remove("active_buffs");
            assert_eq!(Profile::from_dict(&old).stimulant_loan(), Loan::Clear);
        });
    }
}
