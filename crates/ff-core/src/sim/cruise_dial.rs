//! What the cruise dial says while it is being turned.
//!
//! The dial (Plus and Minus, Ctrl for single steps, the controller's cruise
//! buttons) answers every press with the new target. Walked step by step,
//! that was one line per step several times a second ("57, 58, 59, 60",
//! owner's NVDA drive, 2026-11-03), each cutting off the last. Now the first
//! step speaks at once, and a step that follows within [`DIAL_SETTLE_S`] is
//! held: when the dial has been still that long, the setting it landed on is
//! spoken once. A lone press sounds exactly as it did.
//!
//! One enum, [`CruiseDial`], with one [`CruiseDial::next`]. The driving
//! state keeps the line itself and speaks it on [`DialCue::Speak`].

/// Seconds the dial must be still before a held step is spoken.
///
/// The Windows default keyboard repeat delay (500 ms) plus the 150 ms grace
/// Hearken's held-key tracker gives a fresh press (`FRESH_GRACE_MS`): the
/// window in which a held key's first repeat lands, whether it arrives as
/// the keyboard's own repeat or as a screen reader's re-sent press. Every
/// later repeat comes sooner than the first.
pub const DIAL_SETTLE_S: f64 = 0.65;

/// Where the dial's speech stands.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum CruiseDial {
    /// Not turned within the settle window: the next step speaks at once.
    #[default]
    Still,
    /// Turned within the last [`DIAL_SETTLE_S`]. `still_s` counts the
    /// seconds since the last step; `held` is true when a step's line is
    /// waiting to be spoken.
    Turning { still_s: f64, held: bool },
}

/// What happened to the dial.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DialEvent {
    /// The dial moved one step (or was pressed with nothing to move).
    Step,
    /// Real seconds passed with no step.
    Tick { dt: f64 },
    /// The drive was left or resumed: nothing is waiting.
    Reset,
}

/// The one cue a transition gives the driving state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DialCue {
    /// Speak the dial's current line now.
    Speak,
}

impl CruiseDial {
    /// The only place the dial's speech moves.
    pub fn next(self, event: DialEvent) -> (CruiseDial, Option<DialCue>) {
        use CruiseDial::*;
        match (self, event) {
            (_, DialEvent::Reset) => (Still, None),
            (Still, DialEvent::Step) => (
                Turning {
                    still_s: 0.0,
                    held: false,
                },
                Some(DialCue::Speak),
            ),
            (Turning { .. }, DialEvent::Step) => (
                Turning {
                    still_s: 0.0,
                    held: true,
                },
                None,
            ),
            (Still, DialEvent::Tick { .. }) => (Still, None),
            (Turning { still_s, held }, DialEvent::Tick { dt }) => {
                let still_s = still_s + dt.max(0.0);
                if still_s < DIAL_SETTLE_S {
                    (Turning { still_s, held }, None)
                } else if held {
                    (Still, Some(DialCue::Speak))
                } else {
                    (Still, None)
                }
            }
        }
    }

    /// The name the agent server's `status` reports.
    pub fn key(self) -> &'static str {
        match self {
            CruiseDial::Still => "still",
            CruiseDial::Turning { held: false, .. } => "turning",
            CruiseDial::Turning { held: true, .. } => "turning_held",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use CruiseDial::*;

    fn turning(still_s: f64, held: bool) -> CruiseDial {
        Turning { still_s, held }
    }

    #[test]
    fn transition_table() {
        let tick = |dt| DialEvent::Tick { dt };
        let cases: [(CruiseDial, DialEvent, CruiseDial, Option<DialCue>); 9] = [
            // A lone press speaks at once.
            (
                Still,
                DialEvent::Step,
                turning(0.0, false),
                Some(DialCue::Speak),
            ),
            // A step inside the window is held, and restarts the count.
            (
                turning(0.3, false),
                DialEvent::Step,
                turning(0.0, true),
                None,
            ),
            (
                turning(0.5, true),
                DialEvent::Step,
                turning(0.0, true),
                None,
            ),
            // Still for the window: the held line is spoken once.
            (turning(0.6, true), tick(0.05), Still, Some(DialCue::Speak)),
            // Nothing held: the window closes in silence.
            (turning(0.6, false), tick(0.05), Still, None),
            // Inside the window, time only counts.
            (turning(0.0, true), tick(0.25), turning(0.25, true), None),
            (Still, tick(1.0), Still, None),
            // Leaving the drive forgets a held line.
            (turning(0.1, true), DialEvent::Reset, Still, None),
            (Still, DialEvent::Reset, Still, None),
        ];
        for (from, event, to, cue) in cases {
            assert_eq!(from.next(event), (to, cue), "{from:?} + {event:?}");
        }
    }

    #[test]
    fn a_held_key_speaks_the_first_step_and_where_it_landed() {
        // A screen reader's re-sent hold: the first repeat at 512 ms, then
        // about every 250 ms (the owner's JAWS log, 2026-08-24).
        let mut dial = Still;
        let mut spoken = 0;
        let mut step = |dial: &mut CruiseDial, spoken: &mut i32| {
            let (next, cue) = dial.next(DialEvent::Step);
            *dial = next;
            *spoken += i32::from(cue.is_some());
        };
        let mut wait = |dial: &mut CruiseDial, spoken: &mut i32, s: f64| {
            let mut left = s;
            while left > 0.0 {
                let dt = left.min(1.0 / 60.0);
                let (next, cue) = dial.next(DialEvent::Tick { dt });
                *dial = next;
                *spoken += i32::from(cue.is_some());
                left -= dt;
            }
        };
        step(&mut dial, &mut spoken);
        assert_eq!(spoken, 1);
        wait(&mut dial, &mut spoken, 0.512);
        for _ in 0..3 {
            step(&mut dial, &mut spoken);
            wait(&mut dial, &mut spoken, 0.25);
        }
        assert_eq!(spoken, 1, "no step is spoken while the key is held");
        wait(&mut dial, &mut spoken, 1.0);
        assert_eq!(spoken, 2, "the setting it landed on, once");
        assert_eq!(dial, Still);
    }

    #[test]
    fn presses_a_second_apart_each_speak() {
        let mut dial = Still;
        for _ in 0..3 {
            let (next, cue) = dial.next(DialEvent::Step);
            assert_eq!(cue, Some(DialCue::Speak));
            let (next, cue) = next.next(DialEvent::Tick { dt: 1.0 });
            assert_eq!(cue, None);
            dial = next;
        }
    }
}
