//! Road lines wait for a quiet voice.
//!
//! A voice that can say whether it is speaking (NVDA 2026.3 and later,
//! SAPI, OneCore) decides when the next road line is spoken. While it is
//! talking -- a road line, an info readout, or the screen reader's own
//! speech -- a new queued line is held here instead of being stacked in the
//! screen reader's queue. When the voice reports quiet, the most important
//! held line that is still true is spoken, and chatter that waited past its
//! budget is dropped instead of read late.
//!
//! Held chatter keeps the pacer's 3-second budget; a route line waits up
//! to 8 seconds; a queued critical line is never dropped and goes after 6
//! seconds even if the voice never falls quiet. Interrupting lines, lines
//! waiting on a delivery receipt, and every line while the voice cannot
//! report go straight to the voice, exactly as before.

use ff_core::message_log::MessageCategory;

use crate::speech::{EventPriority, Protect, Valid};

use super::{GameContext, TRANSCRIPT_TARGET};

/// How long an `Important` line may wait before its moment has passed.
/// Longer than chatter's budget, so a road instruction outlasts a readout
/// or two; past it, the road has moved on and usually says it again. The
/// owner's drive on 2026-11-03 heard "Construction zone ahead, cruise
/// easing to 55" spoken twelve seconds late, after a hazard brake, and
/// then again when cruise resumed.
const HELD_IMPORTANT_MAX_S: f64 = 8.0;

/// A queued `Critical` line is never dropped: past this wait it is spoken
/// behind whatever the voice is saying, quiet or not.
const HELD_CRITICAL_MAX_S: f64 = 6.0;

/// One road line after the speech ladder and the repeat checks, ready for
/// the voice.
pub(crate) struct EventLine {
    pub text: String,
    pub priority: EventPriority,
    pub interrupt: bool,
    pub protect: Protect,
    pub valid: Option<Valid>,
    pub key: Option<String>,
    pub receipt: bool,
    pub review: bool,
}

/// A line held for a quiet voice, and when it was raised.
pub(crate) struct HeldEvent {
    line: EventLine,
    raised_at: f64,
}

impl GameContext {
    /// Whether this line waits for the voice instead of going now.
    pub(super) fn should_hold_event(&mut self, line: &EventLine) -> bool {
        !line.interrupt
            && !line.receipt
            && self.event_pacer.voice_reporting()
            && (!self.held_events.is_empty() || !self.event_pacer.voice_quiet())
    }

    /// Queue a line for the quiet voice. A newer line about the same
    /// condition (the same key) replaces the older one, and the same words
    /// already waiting are not held twice.
    pub(super) fn hold_event(&mut self, line: EventLine) {
        if self
            .held_events
            .iter()
            .any(|held| held.line.text == line.text)
        {
            return;
        }
        if let Some(key) = line.key.as_deref() {
            self.held_events
                .retain(|held| held.line.key.as_deref() != Some(key));
        }
        log::info!(target: TRANSCRIPT_TARGET, "[hold] {}", line.text);
        let raised_at = self.event_pacer.now();
        self.held_events.push_back(HeldEvent { line, raised_at });
    }

    /// Per-frame: speak the next held line once the voice is quiet.
    ///
    /// A voice that has stopped reporting gets everything still held, in
    /// order, the way lines went before holding existed.
    pub(super) fn release_held_event(&mut self) {
        if self.held_events.is_empty() {
            return;
        }
        if !self.event_pacer.voice_reporting() {
            while let Some(held) = self.held_events.pop_front() {
                self.deliver_event(held.line);
            }
            return;
        }
        self.drop_stale_held();
        self.release_overdue_critical();
        if !self.event_pacer.voice_quiet() {
            return;
        }
        if let Some(index) = self.next_held_index() {
            if let Some(held) = self.held_events.remove(index) {
                self.deliver_event(held.line);
            }
        }
    }

    /// Drop held lines whose moment has passed: chatter older than its
    /// wait budget, an `Important` line older than `HELD_IMPORTANT_MAX_S`,
    /// and anything its own check says is no longer true. A `Critical` line
    /// is never dropped for age.
    fn drop_stale_held(&mut self) {
        let now = self.event_pacer.now();
        let ambient_budget = self.event_pacer.wait_budget_s(EventPriority::Ambient);
        let mut kept = std::collections::VecDeque::with_capacity(self.held_events.len());
        while let Some(held) = self.held_events.pop_front() {
            let waited = now - held.raised_at;
            let untrue = held.line.valid.as_ref().is_some_and(|valid| !valid());
            let stale = match held.line.priority {
                EventPriority::Ambient => waited > ambient_budget,
                EventPriority::Important => waited > HELD_IMPORTANT_MAX_S,
                EventPriority::Critical => false,
            };
            if untrue || stale {
                log::info!(
                    target: TRANSCRIPT_TARGET,
                    "[hold] dropped after {waited:.1}s: {}",
                    held.line.text
                );
                if held.line.review && !untrue {
                    self.message_log
                        .add(&held.line.text, MessageCategory::Event);
                }
                continue;
            }
            kept.push_back(held);
        }
        self.held_events = kept;
    }

    /// The most important held line, oldest first among equals.
    fn next_held_index(&self) -> Option<usize> {
        self.held_events
            .iter()
            .enumerate()
            .max_by(|(ia, a), (ib, b)| {
                a.line
                    .priority
                    .cmp(&b.line.priority)
                    .then_with(|| ib.cmp(ia))
            })
            .map(|(index, _)| index)
    }

    /// A `Critical` line held past `HELD_CRITICAL_MAX_S` goes now, queued
    /// behind whatever the voice is saying.
    fn release_overdue_critical(&mut self) {
        let now = self.event_pacer.now();
        let overdue = self.held_events.iter().position(|held| {
            held.line.priority == EventPriority::Critical
                && now - held.raised_at > HELD_CRITICAL_MAX_S
        });
        if let Some(index) = overdue {
            if let Some(held) = self.held_events.remove(index) {
                self.deliver_event(held.line);
            }
        }
    }

    /// How many road lines are waiting for the voice to fall quiet.
    pub fn held_event_count(&self) -> usize {
        self.held_events.len()
    }
}
