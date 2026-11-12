//! The cattle's footing: what the player hears behind the cab.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Footing {
    /// A slow shuffle and an occasional low call. Costs nothing.
    Settled,
    /// More shuffling and calling, after a firm stop or bend, or in the heat.
    Restless,
    /// Hooves clattering and bodies against the gates: condition is being
    /// lost while this lasts.
    Scrambling,
    /// One animal is down and stays down until the driver checks the
    /// cattle.
    OneDown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FootingEvent {
    /// A firm stop or bend that stays under the footing threshold.
    FirmMove,
    /// The truck is standing in the heat.
    HotStanding,
    /// Several calm minutes, with the heat not standing.
    Calmed,
    /// Braking or cornering past the footing threshold.
    Thrown,
    /// The force that threw them has ended.
    ForceEnded,
    /// An animal went down during a scramble (a seeded draw).
    Fell,
    /// The driver checked the cattle.
    Checked,
}

impl Footing {
    pub fn next(self, event: FootingEvent) -> Footing {
        use Footing::*;
        use FootingEvent::*;
        match (self, event) {
            // One down holds through everything but the check.
            (OneDown, Checked) => Restless,
            (OneDown, _) => OneDown,
            (Settled, FirmMove | HotStanding) => Restless,
            (Restless, Calmed) => Settled,
            (Settled | Restless, Thrown) => Scrambling,
            (Scrambling, ForceEnded) => Restless,
            (Scrambling, Fell) => OneDown,
            (state, _) => state,
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Footing::Settled => "settled",
            Footing::Restless => "restless",
            Footing::Scrambling => "scrambling",
            Footing::OneDown => "one_down",
        }
    }

    pub fn from_key(key: &str) -> Option<Footing> {
        [
            Footing::Settled,
            Footing::Restless,
            Footing::Scrambling,
            Footing::OneDown,
        ]
        .into_iter()
        .find(|state| state.key() == key)
    }

    /// The load readout's word for it.
    pub fn spoken(self) -> &'static str {
        match self {
            Footing::Settled => "Cattle settled",
            Footing::Restless => "Cattle restless",
            Footing::Scrambling => "Cattle losing their footing",
            Footing::OneDown => "One of the cattle is down",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Footing::*;
    use super::FootingEvent::*;
    use super::*;

    #[test]
    fn the_footing_moves_only_along_its_table() {
        let table = [
            (Settled, FirmMove, Restless),
            (Settled, HotStanding, Restless),
            (Settled, Thrown, Scrambling),
            (Settled, Calmed, Settled),
            (Settled, ForceEnded, Settled),
            (Settled, Fell, Settled),
            (Settled, Checked, Settled),
            (Restless, Calmed, Settled),
            (Restless, Thrown, Scrambling),
            (Restless, FirmMove, Restless),
            (Restless, HotStanding, Restless),
            (Restless, Checked, Restless),
            (Scrambling, ForceEnded, Restless),
            (Scrambling, Fell, OneDown),
            (Scrambling, Thrown, Scrambling),
            (Scrambling, Calmed, Scrambling),
            (Scrambling, Checked, Scrambling),
            (OneDown, Checked, Restless),
            (OneDown, Calmed, OneDown),
            (OneDown, Thrown, OneDown),
            (OneDown, ForceEnded, OneDown),
            (OneDown, FirmMove, OneDown),
            (OneDown, HotStanding, OneDown),
        ];
        for (from, event, to) in table {
            assert_eq!(from.next(event), to, "{from:?} on {event:?}");
        }
    }

    #[test]
    fn every_footing_round_trips_through_its_key() {
        for state in [Settled, Restless, Scrambling, OneDown] {
            assert_eq!(Footing::from_key(state.key()), Some(state));
        }
    }
}
