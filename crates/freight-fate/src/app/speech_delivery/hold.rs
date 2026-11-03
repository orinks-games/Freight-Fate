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

/// How long after a main-channel line is handed to a shared voice a quiet
/// report still counts as stale: the screen reader may not have started on
/// it yet. The pacer's start grace, for lines it never sees.
const MAIN_START_GRACE_S: f64 = 0.4;

/// The most waiting time a player-asked readout may excuse from a held
/// line's budget. A road line waits out a Clock or Route readout (about ten
/// seconds at a normal rate) without going stale, but a player reading
/// readout after readout does not hold the road back forever.
const HELD_READOUT_MAX_S: f64 = 15.0;

/// What the road voice has said about itself.
#[derive(Default)]
pub(crate) struct VoiceWatch {
    /// The voice has answered at least once since the event-voice setting
    /// last changed (the value is that setting), so it is asked every
    /// frame from then on: an info readout on the main channel must count
    /// as speech even when no road line is playing.
    answers_for: Option<bool>,
    /// When the latest report was taken, on the pacer's clock.
    seen_at: f64,
    /// When the last main-channel line went to a voice shared with the
    /// road, on the pacer's clock.
    main_said_at: f64,
    /// Whether the last `[hold]` log line said the voice reports.
    logged: bool,
    /// A main-channel line on the shared voice has not yet been followed
    /// by a quiet report: the player is hearing a readout.
    readout_playing: bool,
    /// The previous frame's pacer time, to measure readout time.
    last_frame_at: Option<f64>,
}

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
    /// Seconds of that wait spent behind a player-asked readout, which do
    /// not count against the line's budget (up to `HELD_READOUT_MAX_S`).
    readout_s: f64,
}

impl GameContext {
    /// Whether this line waits for the voice instead of going now.
    pub(super) fn should_hold_event(&mut self, line: &EventLine) -> bool {
        !line.interrupt
            && !line.receipt
            && self.event_pacer.voice_reporting()
            && (!self.held_events.is_empty() || !self.road_voice_quiet())
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
        self.held_events.push_back(HeldEvent {
            line,
            raised_at,
            readout_s: 0.0,
        });
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
        if !self.road_voice_quiet() {
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
            // The owner's third drive (2026-11-03) lost "Exit traffic, 2
            // miles. Hold right" and a 5-mile scale notice to a ten-second
            // Clock readout: chatter's 3-second budget is for waiting on
            // other road lines, not on an answer the player asked for.
            let waited = now - held.raised_at - held.readout_s;
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

    /// Log once when the road voice starts or stops reporting whether it
    /// is speaking, so a session log says whether holding was in play.
    pub(super) fn note_voice_reporting(&mut self) {
        let reporting = self.event_pacer.voice_reporting();
        if reporting != self.voice_watch.logged {
            self.voice_watch.logged = reporting;
            let voice = if self.settings.sapi_events {
                self.speech.event_backend_name()
            } else {
                self.speech.backend_name()
            };
            log::info!(
                target: TRANSCRIPT_TARGET,
                "[hold] {voice} {} whether it is speaking",
                if reporting { "reports" } else { "stopped reporting" }
            );
        }
    }

    /// Per-frame: let the road voice correct the pacer's estimate when it
    /// can say whether it is speaking (NVDA 2026.3 and later, SAPI,
    /// OneCore), then speak the next held line if it is quiet. The threaded
    /// sink answers from its last report, so the frame never waits on the
    /// screen reader.
    pub fn update_event_voice_report(&mut self) {
        let sapi_events = self.settings.sapi_events;
        if self
            .voice_watch
            .answers_for
            .is_some_and(|s| s != sapi_events)
        {
            self.voice_watch.answers_for = None;
        }
        if self.event_pacer.wants_voice_report()
            || !self.held_events.is_empty()
            || self.voice_watch.answers_for.is_some()
        {
            // Ask the voice the road lines are spoken by. With the SAPI
            // event voice off they go to the main voice, and the idle event
            // voice beside it would always answer quiet (owner's NVDA
            // 2026.3 drive, 2026-11-03).
            let report = if sapi_events {
                self.speech.event_voice_report()
            } else {
                self.speech.main_voice_report()
            };
            if let Some(report) = report {
                let age = report.age.as_secs_f64();
                self.voice_watch.answers_for = Some(sapi_events);
                self.voice_watch.seen_at = self.event_pacer.now() - age;
                self.event_pacer.observe_voice(report.speaking, age);
            }
        }
        self.note_voice_reporting();
        self.excuse_readout_time();
        self.release_held_event();
    }

    /// Credit held lines with the frame's time while a readout plays.
    fn excuse_readout_time(&mut self) {
        let now = self.event_pacer.now();
        let elapsed = self
            .voice_watch
            .last_frame_at
            .map_or(0.0, |last| (now - last).max(0.0));
        self.voice_watch.last_frame_at = Some(now);
        if self.voice_watch.readout_playing && self.road_voice_quiet() {
            self.voice_watch.readout_playing = false;
        }
        if !self.voice_watch.readout_playing || !self.event_pacer.voice_reporting() {
            return;
        }
        for held in &mut self.held_events {
            held.readout_s = (held.readout_s + elapsed).min(HELD_READOUT_MAX_S);
        }
    }

    /// Note a main-channel line handed to the voice. When that voice also
    /// carries the road, a readout is speech the road lines must wait for,
    /// though the pacer never sees it (owner's second NVDA 2026.3 drive,
    /// 2026-11-03: a held line went out the instant Clock was pressed).
    pub(super) fn note_main_said(&mut self) {
        if self.event_voice_shares_main() {
            self.voice_watch.main_said_at = self.event_pacer.now();
            self.voice_watch.readout_playing = true;
        }
    }

    /// Quiet by a report taken after the last line either channel handed
    /// to the road voice had time to start.
    fn road_voice_quiet(&mut self) -> bool {
        self.event_pacer.voice_quiet()
            && self.voice_watch.seen_at >= self.voice_watch.main_said_at + MAIN_START_GRACE_S
    }

    /// How many road lines are waiting for the voice to fall quiet.
    pub fn held_event_count(&self) -> usize {
        self.held_events.len()
    }
}
