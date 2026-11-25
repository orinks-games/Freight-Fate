//! A 1.9 career carried into 2.0, as one state machine.
//!
//! 2.0 copies every signed 1.9 career forward on its first start
//! (`profile::copy_forward`). In 1.9 every carrier ran nationwide; in 2.0
//! some are regionals. A copied driver whose carrier changed shape under
//! them gets one no-fault choice at the terminal: stay with that carrier
//! (moving home to its nearest terminal when it no longer runs where they
//! live) or choose another carrier that hires where they live. Nothing on
//! the driving record, and nothing the career earned, moves.
//!
//! Stored on the profile as one string field, `carryover`. Which carrier
//! the driver drives for is the Employment machine (ROADMAP); this one only
//! says whether that carrier is still owed a choice.

use crate::models::carriers::Carrier;

/// Where a career stands as a carried 1.9 career.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Carryover {
    /// A career started in 2.0, and every save with no field.
    #[default]
    Native,
    /// A copied career with nothing owed: its carrier kept its shape, or
    /// the choice is made.
    Settled,
    /// A copied career whose carrier is no longer national: the terminal
    /// offers "Choose a carrier" once no load is in hand.
    CarrierChoiceDue,
}

/// What moves a career through the carry-over.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CarryoverEvent {
    /// The copy from 1.9, or a career an earlier 2.0 build copied, seen
    /// once on load. `carrier_reshaped`: see [`carrier_reshaped`].
    CopiedFrom19 { carrier_reshaped: bool },
    /// "Stay with" the same carrier on the choice menu.
    StayedWithCarrier,
    /// Any carrier change by any path: the choice menu, a termination's
    /// fallback, the training-fleet move, a lease to another carrier.
    CarrierChanged,
}

pub const ALL_CARRYOVER: [Carryover; 3] = [
    Carryover::Native,
    Carryover::Settled,
    Carryover::CarrierChoiceDue,
];

pub const ALL_CARRYOVER_EVENTS: [CarryoverEvent; 4] = [
    CarryoverEvent::CopiedFrom19 {
        carrier_reshaped: false,
    },
    CarryoverEvent::CopiedFrom19 {
        carrier_reshaped: true,
    },
    CarryoverEvent::StayedWithCarrier,
    CarryoverEvent::CarrierChanged,
];

impl Carryover {
    /// The state this event moves to, or `None` when it changes nothing.
    pub fn next(self, event: CarryoverEvent) -> Option<Carryover> {
        use Carryover as S;
        use CarryoverEvent as E;
        match (self, event) {
            (
                S::Native,
                E::CopiedFrom19 {
                    carrier_reshaped: false,
                },
            ) => Some(S::Settled),
            (
                S::Native,
                E::CopiedFrom19 {
                    carrier_reshaped: true,
                },
            ) => Some(S::CarrierChoiceDue),
            (S::CarrierChoiceDue, E::StayedWithCarrier | E::CarrierChanged) => Some(S::Settled),
            _ => None,
        }
    }

    /// The saved value, and what the agent server and scenarios use.
    pub fn key(self) -> &'static str {
        match self {
            Carryover::Native => "native",
            Carryover::Settled => "settled",
            Carryover::CarrierChoiceDue => "carrier_choice_due",
        }
    }

    pub fn from_key(key: &str) -> Option<Carryover> {
        ALL_CARRYOVER.into_iter().find(|state| state.key() == key)
    }
}

/// Whether a copied career's carrier changed shape under the driver: it has
/// a hiring carrier (company drivers and leased owner-operators both) and
/// that carrier is not national in 2.0. Every 1.9 carrier hired nationwide,
/// so any other tier is one 2.0 reshaped. Own authority has no hiring
/// carrier.
pub fn carrier_reshaped(hiring: Option<&Carrier>) -> bool {
    hiring.is_some_and(|c| !c.is_national())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::carriers::carrier;
    use Carryover as S;
    use CarryoverEvent as E;

    #[test]
    fn transition_table_every_state_against_every_event() {
        let reshaped = E::CopiedFrom19 {
            carrier_reshaped: true,
        };
        let kept = E::CopiedFrom19 {
            carrier_reshaped: false,
        };
        let table: [(Carryover, CarryoverEvent, Option<Carryover>); 12] = [
            (S::Native, kept, Some(S::Settled)),
            (S::Native, reshaped, Some(S::CarrierChoiceDue)),
            (S::Native, E::StayedWithCarrier, None),
            (S::Native, E::CarrierChanged, None),
            (S::Settled, kept, None),
            (S::Settled, reshaped, None),
            (S::Settled, E::StayedWithCarrier, None),
            (S::Settled, E::CarrierChanged, None),
            (S::CarrierChoiceDue, kept, None),
            (S::CarrierChoiceDue, reshaped, None),
            (S::CarrierChoiceDue, E::StayedWithCarrier, Some(S::Settled)),
            (S::CarrierChoiceDue, E::CarrierChanged, Some(S::Settled)),
        ];
        for state in ALL_CARRYOVER {
            for event in ALL_CARRYOVER_EVENTS {
                let row = table
                    .iter()
                    .find(|(s, e, _)| *s == state && *e == event)
                    .unwrap_or_else(|| panic!("no row for {state:?} {event:?}"));
                assert_eq!(state.next(event), row.2, "{state:?} {event:?}");
            }
        }
    }

    #[test]
    fn keys_round_trip() {
        for state in ALL_CARRYOVER {
            assert_eq!(Carryover::from_key(state.key()), Some(state));
        }
        assert_eq!(Carryover::from_key("fired"), None);
    }

    #[test]
    fn only_a_non_national_hiring_carrier_is_reshaped() {
        assert!(carrier_reshaped(carrier("prairie_link")));
        for national in ["northstar", "great_lakes_training", "summit_value"] {
            assert!(!carrier_reshaped(carrier(national)), "{national}");
        }
        assert!(!carrier_reshaped(None));
    }
}
