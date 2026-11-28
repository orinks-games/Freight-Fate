//! The crossing call at a state, province or country line, and the lines
//! that follow it, in a fixed order.
//!
//! At a line the crossing call speaks first and finishes, then what changed
//! with it: the time zone, the units line (Automatic units), then the hours
//! rules. Spoken as each was ready, a later line flushed the crossing call
//! mid-sentence (the north-of-60 hours line at the Yukon line, 2026-11-21).
//! Held in the ambient queue, the crossing call aged out behind a long ice
//! hazard and was never said (brief 5 drives, 2026-11-22). These lines carry
//! rule changes, so none of them is ever dropped: each waits here until no
//! hazard is up, no ambient line is waiting and the road voice is quiet,
//! however long that takes, and the queue releases one line at a time in
//! [`AfterCrossing`] order. While a co-driver has the wheel the units and
//! hours rules lines wait for the player to take it back.

use crate::app::{GameContext, SayEvent};
use crate::speech::EventPriority;
use crate::states::driving::DrivingState;
use ff_core::sim::driving_modes::tuning_for_time_scale;
use ff_core::speech_pacing::SpeechCategory;

/// What is said at a line, in the order it is spoken.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum AfterCrossing {
    /// The crossing call itself: "Crossing into Yukon near ...".
    Crossing,
    /// The clock changed with the time zone.
    TimeZone,
    /// The distance and speed units changed with the country.
    Units,
    /// The hours-of-service rules changed with the jurisdiction.
    HoursRules,
}

impl AfterCrossing {
    /// The priority the line is released at. The crossing call and the hours
    /// rules are Important, as they were; the time zone and units lines keep
    /// the default (ambient) priority they had, so neither can cut an exit
    /// or merge call.
    pub fn priority(self) -> Option<EventPriority> {
        match self {
            AfterCrossing::Crossing | AfterCrossing::HoursRules => Some(EventPriority::Important),
            AfterCrossing::TimeZone | AfterCrossing::Units => None,
        }
    }
}

/// A line waiting for its turn.
#[derive(Debug, Clone, PartialEq)]
pub struct PendingAfterCrossing {
    pub kind: AfterCrossing,
    pub text: String,
    /// `None` for a flavour line the speech ladder does not govern.
    pub category: Option<SpeechCategory>,
    /// The line's own tone, played as it is spoken.
    pub sound: Option<String>,
}

impl DrivingState {
    /// Queue a line for its place at the line. A newer line of the same kind
    /// replaces the one waiting: a truck that weaves across a line hears
    /// only where it ended up.
    pub fn queue_after_crossing(
        &mut self,
        kind: AfterCrossing,
        text: impl Into<String>,
        category: Option<SpeechCategory>,
    ) {
        self.queue_line_at_crossing(kind, text.into(), category, None);
    }

    /// Queue the crossing call, with the tone it plays.
    pub fn queue_crossing_call(
        &mut self,
        text: impl Into<String>,
        category: Option<SpeechCategory>,
        sound: Option<&str>,
    ) {
        self.queue_line_at_crossing(
            AfterCrossing::Crossing,
            text.into(),
            category,
            sound.map(str::to_string),
        );
    }

    fn queue_line_at_crossing(
        &mut self,
        kind: AfterCrossing,
        text: String,
        category: Option<SpeechCategory>,
        sound: Option<String>,
    ) {
        self.after_crossing.retain(|line| line.kind != kind);
        self.after_crossing.push(PendingAfterCrossing {
            kind,
            text,
            category,
            sound,
        });
        self.after_crossing.sort_by_key(|line| line.kind);
    }

    /// Per frame: release the next line once the road is quiet.
    pub fn update_after_crossing(&mut self, ctx: &mut GameContext, _dt: f64) {
        // The co-driver's road is silent to the player, so the units and
        // hours rules lines wait until the player has the wheel: they are
        // still true then. The crossing call and the clock change are of
        // their moment, and pass with the shift.
        if self.co_driver_driving() {
            self.after_crossing.retain(|line| {
                matches!(line.kind, AfterCrossing::Units | AfterCrossing::HoursRules)
            });
            return;
        }
        if self.after_crossing.is_empty()
            || self.hazard_deadline.is_some()
            || !self.pending_ambient_events.is_empty()
            || ctx.event_voice_busy()
        {
            return;
        }
        let line = self.after_crossing.remove(0);
        if let Some(sound) = line.sound.as_deref() {
            // The line's own tone: nothing when the rung drops the words.
            ctx.play_event_cue(line.category, sound, 1.0, 0.0);
        }
        // Said once: a status readout, the customs booth or a police stop
        // that cuts one of these lines does not bring it back in full after
        // (cross-border drives, 2026-11-23, heard each line twice). The
        // review keeps every one, and the hours rules are on the logbook.
        let mut opts = SayEvent::queued().said_once();
        opts.review = line.kind != AfterCrossing::Crossing;
        opts.priority = line.kind.priority();
        opts.category = line.category;
        ctx.say_event_with(line.text, opts);
        // Space the road's chatter after it, as any ambient line does.
        self.ambient_event_cooldown_s =
            tuning_for_time_scale(self.trip.time_scale).ambient_spacing_s;
    }
}
