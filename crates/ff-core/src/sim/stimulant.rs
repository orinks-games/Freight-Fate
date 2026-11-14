//! Stimulants as a loan: the lift a coffee or an energy drink gives comes
//! back when it wears off (`docs/fatigue-design.md`, part 5).
//!
//! The driver holds at most one open loan. Taking a stimulant with none
//! open applies the instant lift and opens a loan for exactly what it took
//! off; taking another while one is open lifts nothing and leaves the due
//! time where it was. When the loan comes due the amount goes back on the
//! fatigue number and the loan closes.
//!
//! [`Loan::next`] is the only place the loan moves. The profile keeps it as
//! one entry in `active_buffs` (see [`Loan::to_entry`]): that list already
//! round-trips through every save and cloud backup, where a new top-level
//! save field would be refused by the backup service until its field list
//! caught up.

use serde_json::{json, Value};

/// The `group` an open loan's entry carries in `active_buffs`.
pub const LOAN_GROUP: &str = "stimulant_loan";

/// Game hours before the rest-stop coffee break's lift wears off.
///
/// ASSUMED tuning, not a measured figure: the design's "two hours for the
/// coffee break", about how long a cup of coffee keeps a driver sharp.
pub const COFFEE_BREAK_LOAN_H: f64 = 2.0;

/// The one line that marks the coffee break's lift wearing off.
pub const COFFEE_WORN_OFF: &str = "The coffee has worn off.";

/// The stimulant loan: nothing owed, or one lift owed back at `due_h`.
#[derive(Debug, Clone, PartialEq)]
pub enum Loan {
    Clear,
    Open {
        /// Fatigue points the lift removed, owed back in full.
        amount: f64,
        /// Absolute game hour the lift wears off.
        due_h: f64,
        /// The line spoken when it does; empty when a buff's own worn-off
        /// line carries it.
        worn_off: String,
    },
}

/// What happens to the loan.
#[derive(Debug, Clone, PartialEq)]
pub enum LoanEvent {
    /// A stimulant taken at `fatigue`: `lift` points off at once, owed back
    /// after `lasts_h` game hours.
    Take {
        fatigue: f64,
        lift: f64,
        now_h: f64,
        lasts_h: f64,
        worn_off: String,
    },
    /// Game time has reached `now_h`.
    Tick { now_h: f64 },
}

/// What the caller applies to the fatigue number.
#[derive(Debug, Clone, PartialEq)]
pub enum LoanOutcome {
    None,
    /// Take this many points off fatigue now.
    Lifted(f64),
    /// A stimulant taken while a loan was open: no lift.
    AlreadyOwed,
    /// Put `amount` back on fatigue; speak `worn_off` if it is not empty.
    Repaid {
        amount: f64,
        worn_off: String,
    },
}

impl Loan {
    /// The only place the loan moves.
    ///
    /// | From  | Event                   | To    | Outcome     |
    /// | ----- | ----------------------- | ----- | ----------- |
    /// | Clear | Take                    | Open  | Lifted      |
    /// | Open  | Take                    | Open  | AlreadyOwed |
    /// | Open  | Tick, due_h reached     | Clear | Repaid      |
    /// | any   | Tick, nothing due       | same  | None        |
    pub fn next(self, event: LoanEvent) -> (Loan, LoanOutcome) {
        match (self, event) {
            (
                Loan::Clear,
                LoanEvent::Take {
                    fatigue,
                    lift,
                    now_h,
                    lasts_h,
                    worn_off,
                },
            ) => {
                // Never below zero: the loan is what was actually removed.
                let amount = lift.max(0.0).min(fatigue.max(0.0));
                if amount <= 0.0 {
                    return (Loan::Clear, LoanOutcome::None);
                }
                let loan = Loan::Open {
                    amount,
                    due_h: now_h + lasts_h.max(0.0),
                    worn_off,
                };
                (loan, LoanOutcome::Lifted(amount))
            }
            (open @ Loan::Open { .. }, LoanEvent::Take { .. }) => (open, LoanOutcome::AlreadyOwed),
            (
                Loan::Open {
                    amount,
                    due_h,
                    worn_off,
                },
                LoanEvent::Tick { now_h },
            ) if now_h >= due_h => (Loan::Clear, LoanOutcome::Repaid { amount, worn_off }),
            (loan, LoanEvent::Tick { .. }) => (loan, LoanOutcome::None),
        }
    }

    pub fn is_open(&self) -> bool {
        matches!(self, Loan::Open { .. })
    }

    /// Whether an `active_buffs` entry is the loan's.
    pub fn is_entry(entry: &Value) -> bool {
        entry.get("group").and_then(Value::as_str) == Some(LOAN_GROUP)
    }

    /// The loan held in `active_buffs`, `Clear` when none is (a save from
    /// before loans existed included).
    pub fn from_entries(entries: &[Value]) -> Loan {
        entries
            .iter()
            .find(|entry| Loan::is_entry(entry))
            .map(|entry| {
                let num = |key: &str| entry.get(key).and_then(Value::as_f64).unwrap_or(0.0);
                Loan::Open {
                    amount: num("amount").clamp(0.0, 100.0),
                    due_h: num("due_h"),
                    worn_off: entry
                        .get("worn_off")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                }
            })
            .unwrap_or(Loan::Clear)
    }

    /// The `active_buffs` entry for an open loan; `None` when clear.
    pub fn to_entry(&self) -> Option<Value> {
        match self {
            Loan::Clear => None,
            Loan::Open {
                amount,
                due_h,
                worn_off,
            } => Some(json!({
                "group": LOAN_GROUP,
                "amount": amount,
                "due_h": due_h,
                "worn_off": worn_off,
            })),
        }
    }

    /// Fatigue owed back by game hour `by_h`, for a forecast.
    pub fn owed_by(&self, by_h: f64) -> f64 {
        match self {
            Loan::Open { amount, due_h, .. } if by_h >= *due_h => *amount,
            _ => 0.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn take(fatigue: f64, lift: f64, now_h: f64) -> LoanEvent {
        LoanEvent::Take {
            fatigue,
            lift,
            now_h,
            lasts_h: COFFEE_BREAK_LOAN_H,
            worn_off: COFFEE_WORN_OFF.to_string(),
        }
    }

    fn open(amount: f64, due_h: f64) -> Loan {
        Loan::Open {
            amount,
            due_h,
            worn_off: COFFEE_WORN_OFF.to_string(),
        }
    }

    #[test]
    fn the_transition_table() {
        let cases: Vec<(Loan, LoanEvent, Loan, LoanOutcome)> = vec![
            (
                Loan::Clear,
                take(50.0, 8.0, 10.0),
                open(8.0, 12.0),
                LoanOutcome::Lifted(8.0),
            ),
            // The lift never takes fatigue below zero, and only what it
            // took is owed.
            (
                Loan::Clear,
                take(5.0, 8.0, 10.0),
                open(5.0, 12.0),
                LoanOutcome::Lifted(5.0),
            ),
            (
                Loan::Clear,
                take(0.0, 8.0, 10.0),
                Loan::Clear,
                LoanOutcome::None,
            ),
            // A second stimulant lifts nothing and the due time holds.
            (
                open(8.0, 12.0),
                take(42.0, 8.0, 11.0),
                open(8.0, 12.0),
                LoanOutcome::AlreadyOwed,
            ),
            (
                open(8.0, 12.0),
                LoanEvent::Tick { now_h: 11.99 },
                open(8.0, 12.0),
                LoanOutcome::None,
            ),
            (
                open(8.0, 12.0),
                LoanEvent::Tick { now_h: 12.0 },
                Loan::Clear,
                LoanOutcome::Repaid {
                    amount: 8.0,
                    worn_off: COFFEE_WORN_OFF.to_string(),
                },
            ),
            (
                Loan::Clear,
                LoanEvent::Tick { now_h: 99.0 },
                Loan::Clear,
                LoanOutcome::None,
            ),
        ];
        for (from, event, to, outcome) in cases {
            let label = format!("{from:?} + {event:?}");
            assert_eq!(from.next(event), (to, outcome), "{label}");
        }
    }

    #[test]
    fn a_loan_round_trips_through_its_buff_entry() {
        let loan = open(7.5, 30.25);
        let entry = loan.to_entry().expect("an open loan has an entry");
        assert!(Loan::is_entry(&entry));
        assert_eq!(
            Loan::from_entries(&[json!({"group": "fatigue"}), entry]),
            loan
        );
        assert_eq!(Loan::Clear.to_entry(), None);
        // A save from before loans: no entry, nothing owed.
        assert_eq!(Loan::from_entries(&[]), Loan::Clear);
    }

    #[test]
    fn owed_by_counts_only_a_loan_due_in_time() {
        let loan = open(8.0, 12.0);
        assert_eq!(loan.owed_by(11.0), 0.0);
        assert_eq!(loan.owed_by(22.0), 8.0);
        assert_eq!(Loan::Clear.owed_by(22.0), 0.0);
    }
}
