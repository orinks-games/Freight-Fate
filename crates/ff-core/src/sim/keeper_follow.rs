//! Whether the speed keeper is following a slower vehicle ahead.
//!
//! The keeper starts shedding for a slower lead once the gap is inside its
//! ease window, and that window is priced from the truck's speed NOW. So the
//! moment the keeper's brake took a few miles an hour off, the window shrank
//! back past the car, the keeper went back to the posted number, let the
//! brake go and throttled up, the window grew over the car again, and it
//! braked again. Every one of those was a fresh brake application, and the
//! air system charges every application: behind a car doing 21 in a 30 mph
//! work zone the tanks went from 123 psi to the 40 psi spring-brake trip in
//! under a minute, twice (issue 520, a 1.9.5 log at Waco; on a bench, 34 full
//! applications in fifteen seconds).
//!
//! The decision now has a memory. The window decides when following STARTS;
//! once it has, the keeper keeps following that same vehicle for as long as
//! it is still the lead and still slower than the keeper's own number. It
//! lets go when the lead speeds up past that number, leaves the lane, or a
//! different vehicle becomes the lead outside the window.

/// The keeper's traffic state.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum KeeperFollow {
    /// Holding the road's number; no slower vehicle is setting the speed.
    #[default]
    Free,
    /// Matching the lead vehicle with this traffic key.
    Following { lead: String },
}

/// What the keeper saw ahead this tick.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FollowEvent<'a> {
    /// No vehicle ahead in the truck's lane.
    NoLead,
    /// The lead vehicle in the truck's lane.
    Lead {
        key: &'a str,
        /// Slower than the number the keeper would hold without it.
        slower: bool,
        /// Inside the keeper's ease window: close enough that shedding for
        /// it has to start now.
        in_window: bool,
    },
}

impl KeeperFollow {
    /// The state after this tick's reading of the road ahead.
    pub fn next(&self, event: FollowEvent<'_>) -> KeeperFollow {
        match event {
            FollowEvent::NoLead | FollowEvent::Lead { slower: false, .. } => KeeperFollow::Free,
            FollowEvent::Lead { key, in_window, .. } => match self {
                KeeperFollow::Following { lead } if lead == key => self.clone(),
                _ if in_window => KeeperFollow::Following {
                    lead: key.to_string(),
                },
                _ => KeeperFollow::Free,
            },
        }
    }

    pub fn following(&self) -> bool {
        matches!(self, KeeperFollow::Following { .. })
    }

    /// True when the transition freshly locked onto a lead (`Free -> Following`)
    /// or switched to a different lead vehicle.
    pub fn locked_on_from(previous: &KeeperFollow, current: &KeeperFollow) -> bool {
        match (previous, current) {
            (KeeperFollow::Free, KeeperFollow::Following { .. }) => true,
            (KeeperFollow::Following { lead: prev }, KeeperFollow::Following { lead: cur }) => {
                prev != cur
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn following(key: &str) -> KeeperFollow {
        KeeperFollow::Following {
            lead: key.to_string(),
        }
    }

    fn lead(key: &str, slower: bool, in_window: bool) -> FollowEvent<'_> {
        FollowEvent::Lead {
            key,
            slower,
            in_window,
        }
    }

    #[test]
    fn transition_table() {
        let cases: Vec<(KeeperFollow, FollowEvent, KeeperFollow)> = vec![
            // Free: the window decides when following starts.
            (KeeperFollow::Free, FollowEvent::NoLead, KeeperFollow::Free),
            (
                KeeperFollow::Free,
                lead("a", true, false),
                KeeperFollow::Free,
            ),
            (KeeperFollow::Free, lead("a", true, true), following("a")),
            (
                KeeperFollow::Free,
                lead("a", false, true),
                KeeperFollow::Free,
            ),
            // Following: the same slower lead is kept even when the window
            // shrinks back past it -- the case that pumped the brakes.
            (following("a"), lead("a", true, false), following("a")),
            (following("a"), lead("a", true, true), following("a")),
            // The lead speeds up past the keeper's number, or is gone.
            (following("a"), lead("a", false, true), KeeperFollow::Free),
            (following("a"), FollowEvent::NoLead, KeeperFollow::Free),
            // A different vehicle is now the lead: judged afresh.
            (following("a"), lead("b", true, true), following("b")),
            (following("a"), lead("b", true, false), KeeperFollow::Free),
            (following("a"), lead("b", false, true), KeeperFollow::Free),
        ];
        for (from, event, to) in cases {
            assert_eq!(from.next(event), to, "{from:?} on {event:?}");
        }
    }
}
