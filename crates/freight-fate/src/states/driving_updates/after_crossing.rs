//! Lines that follow a state, province or country line, in a fixed order.
//!
//! At a line the crossing call speaks first and finishes, then what changed
//! with it: the time zone, the units line (Automatic units), then the hours
//! rules. Spoken
//! as each was ready, a later line flushed the crossing call mid-sentence
//! (the north-of-60 hours line at the Yukon line, 2026-11-21). So a
//! follow-up waits here until the road voice is quiet and no ambient line
//! (the crossing call itself, when it was held) is still waiting, and the
//! queue releases one line at a time in [`AfterCrossing`] order.

use crate::app::{GameContext, SayEvent};
use crate::speech::EventPriority;
use crate::states::driving::DrivingState;
use ff_core::speech_pacing::SpeechCategory;

/// What follows a crossing call, in the order it is spoken.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum AfterCrossing {
    /// The clock changed with the time zone.
    TimeZone,
    /// The distance and speed units changed with the country.
    Units,
    /// The hours-of-service rules changed with the jurisdiction.
    HoursRules,
}

impl AfterCrossing {
    /// The priority the line is released at. Only the hours rules are
    /// Important; the time zone and units lines keep the default (ambient)
    /// priority they had, so neither can cut an exit or merge call.
    pub fn priority(self) -> Option<EventPriority> {
        match self {
            AfterCrossing::HoursRules => Some(EventPriority::Important),
            AfterCrossing::TimeZone | AfterCrossing::Units => None,
        }
    }
}

/// The longest a follow-up waits for a quiet voice, in real seconds. Past
/// it the line queues behind whatever is speaking rather than be lost.
pub const AFTER_CROSSING_MAX_WAIT_S: f64 = 20.0;

/// A follow-up waiting for its turn.
#[derive(Debug, Clone, PartialEq)]
pub struct PendingAfterCrossing {
    pub kind: AfterCrossing,
    pub text: String,
    /// `None` for a flavour line the speech ladder does not govern.
    pub category: Option<SpeechCategory>,
    pub waited_s: f64,
}

impl DrivingState {
    /// Queue a line to follow the crossing call. A newer line of the same
    /// kind replaces the one waiting: a truck that weaves across a line
    /// hears only where it ended up.
    pub fn queue_after_crossing(
        &mut self,
        kind: AfterCrossing,
        text: impl Into<String>,
        category: Option<SpeechCategory>,
    ) {
        self.after_crossing.retain(|line| line.kind != kind);
        self.after_crossing.push(PendingAfterCrossing {
            kind,
            text: text.into(),
            category,
            waited_s: 0.0,
        });
        self.after_crossing.sort_by_key(|line| line.kind);
    }

    /// Per frame: release the next follow-up once the crossing call is done.
    pub fn update_after_crossing(&mut self, ctx: &mut GameContext, dt: f64) {
        let Some(first) = self.after_crossing.first_mut() else {
            return;
        };
        first.waited_s += dt;
        let overdue = first.waited_s >= AFTER_CROSSING_MAX_WAIT_S;
        let quiet = self.pending_ambient_events.is_empty() && !ctx.event_voice_busy();
        if !(quiet || overdue) {
            return;
        }
        let line = self.after_crossing.remove(0);
        let mut opts = SayEvent::queued();
        opts.priority = line.kind.priority();
        opts.category = line.category;
        ctx.say_event_with(line.text, opts);
    }
}
