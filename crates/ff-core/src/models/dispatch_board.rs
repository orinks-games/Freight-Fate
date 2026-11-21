//! The dispatch board as one state machine.
//!
//! Three things travel together on the board: who picks the load
//! ([`BoardMode`]), what a select still owes the driver before it accepts
//! ([`AcceptGate`]), and which load dispatch marks
//! ([`crate::models::dispatch_fit::Recommendation`]). They used to be five
//! loose fields on the game's board screen. Now they are one value with one
//! [`DispatchBoard::next`], and the screen only feeds it events and acts on
//! the [`BoardStep`] that comes back: each step is one spoken line or sound.
//!
//! The accept gate keeps the board's two confirm-once warnings: selecting a
//! load that needs an extra rest on the driver's current hours speaks the
//! hours warning once, the no-chains reminder is heard once after it, and
//! the next select accepts. Selecting a different load, a decline, or a
//! pulled offer starts over. The chain check reads the weather, so the
//! machine asks for it ([`BoardStep::CheckChains`]) only when the gate gets
//! that far, and the screen answers with [`BoardEvent::ChainsChecked`].

use crate::models::dispatch_fit::Recommendation;

/// Who picks the load.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BoardMode {
    /// Dispatch offers one load at a time, the first of `queue` (board
    /// indices, in assignment order); the driver accepts or declines.
    Assigned { queue: Vec<usize> },
    /// The driver picks; dispatch may recommend one row.
    Browse,
}

/// What a select still owes the driver before the load is accepted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AcceptGate {
    /// Nothing heard yet.
    #[default]
    Clear,
    /// The hours warning was heard for this load.
    HoursHeard(usize),
    /// The no-chains reminder was heard for this load.
    ChainsHeard(usize),
}

impl AcceptGate {
    /// The load this gate is about, if any.
    fn load(self) -> Option<usize> {
        match self {
            AcceptGate::Clear => None,
            AcceptGate::HoursHeard(i) | AcceptGate::ChainsHeard(i) => Some(i),
        }
    }

    /// The name the agent server's `status` reports.
    pub fn key(self) -> String {
        match self {
            AcceptGate::Clear => "clear".to_string(),
            AcceptGate::HoursHeard(i) => format!("hours_heard job {}", i + 1),
            AcceptGate::ChainsHeard(i) => format!("chains_heard job {}", i + 1),
        }
    }
}

/// What moves the board.
#[derive(Debug, Clone, PartialEq)]
pub enum BoardEvent {
    /// Dispatch planned the board: it opened, a load was declined, or an
    /// offer was pulled. A non-empty `assigned` queue means dispatch
    /// assigns. The gate starts over.
    Planned {
        assigned: Vec<usize>,
        recommendation: Option<Recommendation>,
    },
    /// The driver selected `load`; `needs_hours` when it needs an extra
    /// rest on the hours already spent this shift.
    Select { load: usize, needs_hours: bool },
    /// The answer to [`BoardStep::CheckChains`]: whether the no-chains
    /// reminder applies to `load`.
    ChainsChecked { load: usize, due: bool },
}

/// What the screen does after an event: one line or sound each.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoardStep {
    /// Nothing to say.
    Quiet,
    /// Speak the hours warning (and hold the load for the rest).
    WarnHours,
    /// Evaluate the chain reminder and send [`BoardEvent::ChainsChecked`].
    CheckChains,
    /// Speak the no-chains reminder.
    RemindChains,
    /// Accept the load.
    Accept,
}

/// The whole board state: mode, gate and recommendation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DispatchBoard {
    pub mode: BoardMode,
    pub gate: AcceptGate,
    /// Only ever set while browsing.
    pub recommendation: Option<Recommendation>,
}

impl Default for DispatchBoard {
    fn default() -> Self {
        DispatchBoard {
            mode: BoardMode::Browse,
            gate: AcceptGate::Clear,
            recommendation: None,
        }
    }
}

impl DispatchBoard {
    /// The one place the board moves.
    pub fn next(&self, event: BoardEvent) -> (DispatchBoard, BoardStep) {
        match event {
            BoardEvent::Planned {
                assigned,
                recommendation,
            } => {
                let (mode, recommendation) = if assigned.is_empty() {
                    (BoardMode::Browse, recommendation)
                } else {
                    (BoardMode::Assigned { queue: assigned }, None)
                };
                (
                    DispatchBoard {
                        mode,
                        gate: AcceptGate::Clear,
                        recommendation,
                    },
                    BoardStep::Quiet,
                )
            }
            BoardEvent::Select { load, needs_hours } => {
                // A warning belongs to the load it was heard for.
                let gate = if self.gate.load() == Some(load) {
                    self.gate
                } else {
                    AcceptGate::Clear
                };
                let (gate, step) = match gate {
                    AcceptGate::Clear if needs_hours => {
                        (AcceptGate::HoursHeard(load), BoardStep::WarnHours)
                    }
                    AcceptGate::Clear | AcceptGate::HoursHeard(_) => (gate, BoardStep::CheckChains),
                    AcceptGate::ChainsHeard(_) => (AcceptGate::Clear, BoardStep::Accept),
                };
                (self.with_gate(gate), step)
            }
            BoardEvent::ChainsChecked { load, due } => {
                if due {
                    (
                        self.with_gate(AcceptGate::ChainsHeard(load)),
                        BoardStep::RemindChains,
                    )
                } else {
                    (self.with_gate(AcceptGate::Clear), BoardStep::Accept)
                }
            }
        }
    }

    fn with_gate(&self, gate: AcceptGate) -> DispatchBoard {
        DispatchBoard {
            gate,
            ..self.clone()
        }
    }

    /// Board indices in the order dispatch assigns them; empty while
    /// browsing.
    pub fn assigned_queue(&self) -> &[usize] {
        match &self.mode {
            BoardMode::Assigned { queue } => queue,
            BoardMode::Browse => &[],
        }
    }

    /// The name of the mode the agent server's `status` reports.
    pub fn mode_key(&self) -> &'static str {
        match self.mode {
            BoardMode::Assigned { .. } => "assigned",
            BoardMode::Browse => "browse",
        }
    }

    /// The load the board puts forward: the assignment, else the
    /// recommended row.
    pub fn offer(&self) -> Option<usize> {
        match &self.mode {
            BoardMode::Assigned { queue } => queue.first().copied(),
            BoardMode::Browse => self.recommendation.map(|r| r.index),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::dispatch_fit::FitReason;
    use AcceptGate::{ChainsHeard, Clear, HoursHeard};
    use BoardStep::{Accept, CheckChains, Quiet, RemindChains, WarnHours};

    fn browse(gate: AcceptGate) -> DispatchBoard {
        DispatchBoard {
            gate,
            ..DispatchBoard::default()
        }
    }

    fn select(load: usize, needs_hours: bool) -> BoardEvent {
        BoardEvent::Select { load, needs_hours }
    }

    fn chains(load: usize, due: bool) -> BoardEvent {
        BoardEvent::ChainsChecked { load, due }
    }

    #[test]
    fn gate_transition_table() {
        let cases: &[(AcceptGate, BoardEvent, AcceptGate, BoardStep)] = &[
            // A load with nothing owed asks for the chain check, then accepts.
            (Clear, select(2, false), Clear, CheckChains),
            (Clear, chains(2, false), Clear, Accept),
            // The hours warning once, then the chain check.
            (Clear, select(2, true), HoursHeard(2), WarnHours),
            (HoursHeard(2), select(2, true), HoursHeard(2), CheckChains),
            (HoursHeard(2), chains(2, false), Clear, Accept),
            // The chain reminder once, then accept.
            (HoursHeard(2), chains(2, true), ChainsHeard(2), RemindChains),
            (Clear, chains(2, true), ChainsHeard(2), RemindChains),
            (ChainsHeard(2), select(2, true), Clear, Accept),
            (ChainsHeard(2), select(2, false), Clear, Accept),
            // Another load starts over.
            (HoursHeard(2), select(3, true), HoursHeard(3), WarnHours),
            (HoursHeard(2), select(3, false), Clear, CheckChains),
            (ChainsHeard(2), select(3, false), Clear, CheckChains),
            (ChainsHeard(2), select(3, true), HoursHeard(3), WarnHours),
        ];
        for (from, event, want_gate, want_step) in cases {
            let (got, step) = browse(*from).next(event.clone());
            assert_eq!(
                (got.gate, step),
                (*want_gate, *want_step),
                "{from:?} + {event:?}"
            );
        }
    }

    #[test]
    fn planning_sets_the_mode_and_clears_the_gate() {
        let recommendation = Some(Recommendation {
            index: 4,
            reason: FitReason::BestPayForHours,
        });
        for from in [Clear, HoursHeard(1), ChainsHeard(1)] {
            let (got, step) = browse(from).next(BoardEvent::Planned {
                assigned: Vec::new(),
                recommendation,
            });
            assert_eq!(step, Quiet);
            assert_eq!(got.gate, Clear);
            assert_eq!(got.mode, BoardMode::Browse);
            assert_eq!(got.offer(), Some(4));
            let (got, _) = browse(from).next(BoardEvent::Planned {
                assigned: vec![3, 0, 1],
                recommendation,
            });
            assert_eq!(got.gate, Clear);
            assert_eq!(got.mode_key(), "assigned");
            assert_eq!(got.recommendation, None, "an assignment marks no row");
            assert_eq!(got.offer(), Some(3));
            assert_eq!(got.assigned_queue(), &[3, 0, 1]);
        }
        let (got, _) = browse(Clear).next(BoardEvent::Planned {
            assigned: Vec::new(),
            recommendation: None,
        });
        assert_eq!(got.offer(), None);
        assert_eq!(got.mode_key(), "browse");
    }

    #[test]
    fn gate_names_are_one_based_rows() {
        assert_eq!(Clear.key(), "clear");
        assert_eq!(HoursHeard(0).key(), "hours_heard job 1");
        assert_eq!(ChainsHeard(5).key(), "chains_heard job 6");
    }
}
