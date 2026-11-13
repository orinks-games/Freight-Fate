//! Who holds the ambient bed: the one background loop under a screen (the
//! main menu's dispatch office or night lot, the terminal gate, a shipper's
//! dock, the night outside the cab).
//!
//! There is one ambient channel and every screen that plays a bed used to
//! write it directly, and stop it on its way out. That made the bed belong to
//! whoever wrote last, and a screen's exit does not always come last: when a
//! screen leaves from inside its own key handler (Escape at the terminal goes
//! back to the main menu), its exit waits until the handler returns, which is
//! after the next screen has started its bed. The terminal's late "stop" then
//! silenced the main menu it had just opened.
//!
//! The rule now: a screen that starts a bed it will stop later *claims* it
//! and gets a [`BedClaim`]; leaving, it *releases* that claim, which stops
//! the bed only while the claim is still the current one. A screen that
//! claimed after it keeps its bed. Beds set frame by frame (the drive's night
//! outside, a rest stop) are *set* without a claim, so no screen's stale
//! release can stop them either.
//!
//! [`AmbientBed::next`] is the only place the bed moves, and it says what the
//! audio must do in an [`AmbientCue`]; the game's `GameContext` hands the cue
//! to the sound engine.

/// A screen's hold on the bed, from [`AmbientEvent::Claim`]. Each claim gets
/// a new number, so a screen that re-enters (or a second copy of the same
/// screen) never matches a claim made before it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BedClaim(pub u64);

/// What the ambient channel holds, as far as the screens are concerned.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum AmbientBed {
    /// Nothing asked for a bed, or the holder let it go.
    #[default]
    Silent,
    /// A bed is playing. `claim` is the screen that may stop it, or `None`
    /// for a bed set frame by frame, which only another set or claim changes.
    Playing {
        key: String,
        claim: Option<BedClaim>,
    },
}

/// Something that moves the bed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AmbientEvent<'a> {
    /// A screen starts `key` and will release it when it leaves.
    Claim { key: &'a str, claim: BedClaim },
    /// A screen that claimed the bed is leaving.
    Release(BedClaim),
    /// A frame-by-frame bed: `Some` plays that key, `None` stops the bed,
    /// whoever started it.
    Set(Option<&'a str>),
}

/// What the sound engine must do for a transition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AmbientCue {
    /// Start (or keep, if it is already the one playing) this loop.
    Start(String),
    /// Fade the bed out.
    Stop,
    /// Leave the channel as it is.
    Keep,
}

impl AmbientBed {
    /// The bed after `event`, and what the audio does to get there.
    ///
    /// A claim or a set always reaches the engine, as the direct writes did
    /// (starting the loop that is already playing keeps it going), so a bed
    /// some other path cut, such as the world audio stopping for a pause,
    /// comes back the next time a screen asks for it. Only a release is ever
    /// held back: one whose claim is no longer current is a screen leaving a
    /// bed someone else now owns.
    pub fn next(&self, event: AmbientEvent<'_>) -> (AmbientBed, AmbientCue) {
        match event {
            AmbientEvent::Claim { key, claim } => (
                AmbientBed::Playing {
                    key: key.to_string(),
                    claim: Some(claim),
                },
                AmbientCue::Start(key.to_string()),
            ),
            AmbientEvent::Set(Some(key)) => (
                AmbientBed::Playing {
                    key: key.to_string(),
                    claim: None,
                },
                AmbientCue::Start(key.to_string()),
            ),
            AmbientEvent::Set(None) => (AmbientBed::Silent, AmbientCue::Stop),
            AmbientEvent::Release(released) => match self {
                AmbientBed::Playing {
                    claim: Some(current),
                    ..
                } if *current == released => (AmbientBed::Silent, AmbientCue::Stop),
                _ => (self.clone(), AmbientCue::Keep),
            },
        }
    }

    /// The key playing, if any.
    pub fn key(&self) -> Option<&str> {
        match self {
            AmbientBed::Silent => None,
            AmbientBed::Playing { key, .. } => Some(key),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn playing(key: &str, claim: Option<u64>) -> AmbientBed {
        AmbientBed::Playing {
            key: key.to_string(),
            claim: claim.map(BedClaim),
        }
    }

    #[test]
    fn transition_table() {
        use AmbientEvent::*;
        let menu = "ambient/menu_dispatch_office";
        let gate = "poi/facility_gate";
        let night = "ambient/night";
        let start = |k: &str| AmbientCue::Start(k.to_string());
        let rows: Vec<(AmbientBed, AmbientEvent, AmbientBed, AmbientCue)> = vec![
            // A screen claims from silence, over another claim, over a set bed.
            (
                AmbientBed::Silent,
                Claim {
                    key: menu,
                    claim: BedClaim(1),
                },
                playing(menu, Some(1)),
                start(menu),
            ),
            (
                playing(gate, Some(1)),
                Claim {
                    key: menu,
                    claim: BedClaim(2),
                },
                playing(menu, Some(2)),
                start(menu),
            ),
            (
                playing(night, None),
                Claim {
                    key: gate,
                    claim: BedClaim(3),
                },
                playing(gate, Some(3)),
                start(gate),
            ),
            // The holder leaves: the bed stops.
            (
                playing(gate, Some(1)),
                Release(BedClaim(1)),
                AmbientBed::Silent,
                AmbientCue::Stop,
            ),
            // A screen leaves after another one claimed: the new bed stays.
            // This is Escape from the terminal to the main menu.
            (
                playing(menu, Some(2)),
                Release(BedClaim(1)),
                playing(menu, Some(2)),
                AmbientCue::Keep,
            ),
            // A stale release cannot stop a bed set frame by frame either.
            (
                playing(night, None),
                Release(BedClaim(1)),
                playing(night, None),
                AmbientCue::Keep,
            ),
            // Releasing twice, or with nothing playing, does nothing.
            (
                AmbientBed::Silent,
                Release(BedClaim(1)),
                AmbientBed::Silent,
                AmbientCue::Keep,
            ),
            // Set plays or stops whatever is there, claimed or not.
            (
                AmbientBed::Silent,
                Set(Some(night)),
                playing(night, None),
                start(night),
            ),
            (
                playing(gate, Some(4)),
                Set(Some(night)),
                playing(night, None),
                start(night),
            ),
            (
                playing(gate, Some(4)),
                Set(None),
                AmbientBed::Silent,
                AmbientCue::Stop,
            ),
            (
                AmbientBed::Silent,
                Set(None),
                AmbientBed::Silent,
                AmbientCue::Stop,
            ),
        ];
        for (before, event, after, cue) in rows {
            assert_eq!(
                before.next(event),
                (after.clone(), cue.clone()),
                "{before:?} on {event:?}"
            );
        }
    }

    #[test]
    fn key_reports_what_plays() {
        assert_eq!(AmbientBed::Silent.key(), None);
        assert_eq!(playing("ambient/night", None).key(), Some("ambient/night"));
    }
}
