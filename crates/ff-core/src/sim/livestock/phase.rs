//! The load: from booking to the receiver's ruling.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LoadPhase {
    Booked,
    Loading,
    Aboard,
    OverLimit,
    Unloading,
    Delivered,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoadEvent {
    /// The driver starts loading at the shipper.
    StartLoading,
    /// The last gate shuts.
    GateShut,
    /// The 28-hour limit runs out.
    LimitRanOut,
    /// The driver starts unloading at the receiver.
    StartUnloading,
    /// The count is finished and the receiver has ruled.
    Ruled,
}

impl LoadPhase {
    /// The one place the load moves. An event that does not apply to the
    /// state leaves it where it is.
    pub fn next(self, event: LoadEvent) -> LoadPhase {
        use LoadEvent::*;
        use LoadPhase::*;
        match (self, event) {
            (Booked, StartLoading) => Loading,
            (Loading, GateShut) => Aboard,
            (Aboard, LimitRanOut) => OverLimit,
            (Aboard | OverLimit, StartUnloading) => Unloading,
            (Unloading, Ruled) => Delivered,
            (state, _) => state,
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            LoadPhase::Booked => "booked",
            LoadPhase::Loading => "loading",
            LoadPhase::Aboard => "aboard",
            LoadPhase::OverLimit => "over_limit",
            LoadPhase::Unloading => "unloading",
            LoadPhase::Delivered => "delivered",
        }
    }

    pub fn from_key(key: &str) -> Option<LoadPhase> {
        [
            LoadPhase::Booked,
            LoadPhase::Loading,
            LoadPhase::Aboard,
            LoadPhase::OverLimit,
            LoadPhase::Unloading,
            LoadPhase::Delivered,
        ]
        .into_iter()
        .find(|phase| phase.key() == key)
    }
}

#[cfg(test)]
mod tests {
    use super::LoadEvent::*;
    use super::LoadPhase::*;
    use super::*;

    #[test]
    fn the_load_moves_only_along_its_table() {
        let table = [
            (Booked, StartLoading, Loading),
            (Loading, GateShut, Aboard),
            (Aboard, LimitRanOut, OverLimit),
            (Aboard, StartUnloading, Unloading),
            (OverLimit, StartUnloading, Unloading),
            (Unloading, Ruled, Delivered),
            // Everything else stays put.
            (Booked, GateShut, Booked),
            (Booked, StartUnloading, Booked),
            (Loading, StartUnloading, Loading),
            (Aboard, StartLoading, Aboard),
            (OverLimit, LimitRanOut, OverLimit),
            (Unloading, LimitRanOut, Unloading),
            (Delivered, StartLoading, Delivered),
            (Delivered, Ruled, Delivered),
        ];
        for (from, event, to) in table {
            assert_eq!(from.next(event), to, "{from:?} on {event:?}");
        }
    }

    #[test]
    fn every_phase_round_trips_through_its_key() {
        for phase in [Booked, Loading, Aboard, OverLimit, Unloading, Delivered] {
            assert_eq!(LoadPhase::from_key(phase.key()), Some(phase));
        }
        assert_eq!(LoadPhase::from_key("galloping"), None);
    }
}
