//! Where a co-driver's shift stands, and the one place it moves.
//!
//! A team load's shift used to be three flags on the co-driver's working
//! memory (`shift_over`, `rested`, `last_lot`) that travelled together and
//! went stale: nothing cleared them when the co-driver's clock reset under
//! them. A tester's co-driver ran out of hours five miles short of the lot
//! he chose, a trooper's log check put the truck out of service for ten
//! hours, and after the order was served the stale "shift over" flag handed
//! a sleeping player the wheel at 43 miles per hour with "Their hours are
//! up", which the reset had made untrue (tester report, 2026-11-19).
//!
//! One enum, one [`TeamShift::next`]. The drive feeds it what it sees each
//! frame: the wheel handed over or taken back, the player's rest coming in,
//! the last lot the co-driver's hours reach, the hours running short, the
//! chosen lot falling out of reach or being passed, and the co-driver's
//! clock resetting. The fuel and break errands are not shift states: they
//! are the stops the co-driver makes for on the way, whatever the shift is
//! doing.

/// Where the co-driver's shift stands.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TeamShift {
    /// The co-driver is in the bunk and the player drives.
    #[default]
    Bunk,
    /// The co-driver drives and nothing ends the shift yet.
    Driving,
    /// The player's 10-hour rest is in: the co-driver hands back at the
    /// next lot with room, without hurrying for it.
    EndingRested,
    /// The co-driver's hours end the shift, and the lot ahead is the last
    /// one those hours reach.
    EndingLastLot,
    /// The co-driver's hours are nearly gone on the way into a lot that is
    /// still in reach: its ramp, or close enough to make before the clock
    /// runs out.
    EndingHoursShort,
    /// No lot in the hours left: braking to a stop on the shoulder.
    Shoulder,
}

/// What moves the shift.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TeamShiftEvent {
    /// The player handed the co-driver the wheel.
    HandedOver,
    /// The player in the sleeper has had the rest they handed over for.
    RestIn,
    /// The next lot is the last one the co-driver's hours reach.
    LastLotInReach,
    /// The co-driver's hours are nearly gone; `lot_in_reach` when the lot
    /// they are making for can still be made.
    HoursShort { lot_in_reach: bool },
    /// The lot they were making for is beyond the hours left, at the pace
    /// they can make now.
    LotOutOfReach,
    /// The lot they were making for is behind the truck.
    LotPassed,
    /// The co-driver's clock reset under the shift (ten hours served out of
    /// service, a sleep on the shoulder): the hours that were ending it are
    /// back.
    ClockReset,
    /// The player took the wheel back, or the co-driver handed it over.
    TakenBack,
}

impl TeamShift {
    /// The one place the shift moves.
    pub fn next(self, event: TeamShiftEvent) -> TeamShift {
        use TeamShift::*;
        use TeamShiftEvent::*;
        match (self, event) {
            (_, TakenBack) => Bunk,
            (_, HandedOver) => Driving,
            // Nothing moves the shift of a co-driver in the bunk.
            (Bunk, _) => Bunk,
            (Driving | EndingLastLot, RestIn) => EndingRested,
            (Driving, LastLotInReach) => EndingLastLot,
            (Shoulder, HoursShort { .. }) => Shoulder,
            (_, HoursShort { lot_in_reach: true }) => EndingHoursShort,
            (
                _,
                HoursShort {
                    lot_in_reach: false,
                },
            ) => Shoulder,
            // The last lot gone, the shift runs on until another is the
            // last one in reach, or the hours run short.
            (EndingLastLot, LotOutOfReach | LotPassed) => Driving,
            // Short of hours, there is no next lot to try.
            (EndingHoursShort, LotOutOfReach | LotPassed) => Shoulder,
            (EndingLastLot | EndingHoursShort | Shoulder, ClockReset) => Driving,
            (state, _) => state,
        }
    }

    /// Whether the co-driver is at the wheel.
    pub fn driving(self) -> bool {
        self != TeamShift::Bunk
    }

    /// Whether the co-driver's own hours are what ends the shift.
    pub fn hours_ending(self) -> bool {
        matches!(
            self,
            TeamShift::EndingLastLot | TeamShift::EndingHoursShort | TeamShift::Shoulder
        )
    }

    /// The name the agent server's `status` reports.
    pub fn key(self) -> &'static str {
        match self {
            TeamShift::Bunk => "bunk",
            TeamShift::Driving => "driving",
            TeamShift::EndingRested => "ending_rested",
            TeamShift::EndingLastLot => "ending_last_lot",
            TeamShift::EndingHoursShort => "ending_hours_short",
            TeamShift::Shoulder => "shoulder",
        }
    }

    /// The state a `status` key names, for a scenario to stage.
    pub fn from_key(key: &str) -> Option<TeamShift> {
        [
            TeamShift::Bunk,
            TeamShift::Driving,
            TeamShift::EndingRested,
            TeamShift::EndingLastLot,
            TeamShift::EndingHoursShort,
            TeamShift::Shoulder,
        ]
        .into_iter()
        .find(|state| state.key() == key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use TeamShift::*;
    use TeamShiftEvent::*;

    const SHORT_IN: TeamShiftEvent = HoursShort { lot_in_reach: true };
    const SHORT_OUT: TeamShiftEvent = HoursShort {
        lot_in_reach: false,
    };

    #[test]
    fn transition_table() {
        let all = [
            Bunk,
            Driving,
            EndingRested,
            EndingLastLot,
            EndingHoursShort,
            Shoulder,
        ];
        let cases: &[(TeamShift, TeamShiftEvent, TeamShift)] = &[
            (Bunk, HandedOver, Driving),
            (Bunk, RestIn, Bunk),
            (Bunk, LastLotInReach, Bunk),
            (Bunk, SHORT_OUT, Bunk),
            (Bunk, ClockReset, Bunk),
            (Driving, RestIn, EndingRested),
            (Driving, LastLotInReach, EndingLastLot),
            (Driving, SHORT_IN, EndingHoursShort),
            (Driving, SHORT_OUT, Shoulder),
            (Driving, LotOutOfReach, Driving),
            (Driving, LotPassed, Driving),
            (Driving, ClockReset, Driving),
            (EndingRested, RestIn, EndingRested),
            (EndingRested, LastLotInReach, EndingRested),
            (EndingRested, SHORT_IN, EndingHoursShort),
            (EndingRested, SHORT_OUT, Shoulder),
            (EndingRested, LotOutOfReach, EndingRested),
            (EndingRested, LotPassed, EndingRested),
            (EndingRested, ClockReset, EndingRested),
            (EndingLastLot, RestIn, EndingRested),
            (EndingLastLot, LastLotInReach, EndingLastLot),
            (EndingLastLot, SHORT_IN, EndingHoursShort),
            (EndingLastLot, SHORT_OUT, Shoulder),
            (EndingLastLot, LotOutOfReach, Driving),
            (EndingLastLot, LotPassed, Driving),
            (EndingLastLot, ClockReset, Driving),
            (EndingHoursShort, RestIn, EndingHoursShort),
            (EndingHoursShort, LastLotInReach, EndingHoursShort),
            (EndingHoursShort, SHORT_IN, EndingHoursShort),
            (EndingHoursShort, SHORT_OUT, Shoulder),
            (EndingHoursShort, LotOutOfReach, Shoulder),
            (EndingHoursShort, LotPassed, Shoulder),
            (EndingHoursShort, ClockReset, Driving),
            (Shoulder, RestIn, Shoulder),
            (Shoulder, LastLotInReach, Shoulder),
            (Shoulder, SHORT_IN, Shoulder),
            (Shoulder, SHORT_OUT, Shoulder),
            (Shoulder, LotOutOfReach, Shoulder),
            (Shoulder, LotPassed, Shoulder),
            (Shoulder, ClockReset, Driving),
        ];
        for &(from, event, to) in cases {
            assert_eq!(from.next(event), to, "{from:?} + {event:?}");
        }
        // Handing over starts a shift and taking back ends it, from anywhere.
        for state in all {
            assert_eq!(state.next(HandedOver), Driving, "{state:?}");
            assert_eq!(state.next(TakenBack), Bunk, "{state:?}");
        }
    }

    #[test]
    fn a_clock_reset_clears_every_shift_the_hours_were_ending() {
        // The tester's stale flag: out of service ten hours on the way to a
        // lot, the co-driver's hours are back and the shift is open again.
        for state in [EndingLastLot, EndingHoursShort, Shoulder] {
            assert!(state.hours_ending());
            assert_eq!(state.next(ClockReset), Driving, "{state:?}");
        }
        assert!(!EndingRested.hours_ending());
        assert!(!Driving.hours_ending());
    }

    #[test]
    fn keys_name_every_state_once() {
        for state in [
            Bunk,
            Driving,
            EndingRested,
            EndingLastLot,
            EndingHoursShort,
            Shoulder,
        ] {
            assert_eq!(TeamShift::from_key(state.key()), Some(state));
        }
        assert_eq!(TeamShift::from_key("asleep"), None);
        assert!(!Bunk.driving());
        assert!(Shoulder.driving());
    }
}
