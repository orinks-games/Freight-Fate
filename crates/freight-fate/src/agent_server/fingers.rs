//! The `fingers` tool: raw finger events through the shared recognizer,
//! the way an iPhone with New gesture reading on (and Android) reads them.
//!
//! A call is a list of finger events, each with its time from the start of
//! the call. They are handed to [`Recognizer`] on their own times as the
//! call's clock reaches them, and the recognizer ticks on that clock every
//! frame, so a flick, a hold, a waiting tap and a second finger's steering
//! begin exactly as they would under a real finger. The clock runs only
//! while a call is being served in lockstep (the world is frozen between
//! calls), and all the time otherwise. A finger left down at the end of a
//! call stays down until a later call lifts it. What the recognizer makes
//! goes through the same [`TouchInput`] the `gesture` tool feeds.
//!
//! [`TouchInput`]: crate::touch::TouchInput

use std::collections::VecDeque;

use super::{AgentPolicy, Reply, Until};
use crate::app::PlayerInputFrame;
use crate::touch::recognizer::Recognizer;
use crate::touch::{fingers, FingerEvent, FingerKind, TouchMode};

/// How long a call waits after its last event, so the reply carries what
/// the game said back and a lone tap has outlived its double tap window.
const FINGERS_SETTLE_SECONDS: f64 = 1.0;

/// The latest a call's last event may come, in milliseconds.
pub(super) const MAX_FINGERS_MS: f64 = 30_000.0;

/// One finger event as a call gives it: its time is from the call's start.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScriptedFinger {
    pub finger: i64,
    pub kind: FingerKind,
    pub x: f32,
    pub y: f32,
    pub at_ms: f64,
}

/// The recognizer the tool plays, its clock, and the events still to come.
#[derive(Debug, Default)]
pub(super) struct FingerPlay {
    recognizer: Recognizer,
    /// Seconds since the server started, counted only while it runs.
    clock: f64,
    script: VecDeque<FingerEvent>,
}

impl FingerPlay {
    /// The recognizer's phase, for `status`.
    pub(super) fn phase_key(&self) -> &'static str {
        self.recognizer.phase_key()
    }

    /// Events are still to be played.
    pub(super) fn playing(&self) -> bool {
        !self.script.is_empty()
    }
}

impl AgentPolicy {
    /// Start one `fingers` call. Returns true when its reply was parked.
    pub(super) fn send_fingers(&mut self, events: Vec<ScriptedFinger>, reply: Reply) -> bool {
        if self.fingers.playing() {
            let _ = reply.send(Err(
                "A fingers call is still playing; its reply arrives when it ends.".to_string(),
            ));
            return false;
        }
        let start = self.fingers.clock;
        let mut last_ms: f64 = 0.0;
        for event in events {
            last_ms = last_ms.max(event.at_ms);
            self.fingers.script.push_back(FingerEvent {
                finger: event.finger,
                kind: event.kind,
                x: event.x,
                y: event.y,
                t: start + last_ms / 1000.0,
            });
        }
        self.wait_until(
            last_ms / 1000.0 + FINGERS_SETTLE_SECONDS,
            Until::Elapsed,
            false,
            reply,
        );
        true
    }

    /// Each frame: run the clock, hand over the events it has reached, and
    /// script what the recognizer made.
    pub(super) fn advance_fingers(&mut self, input: &mut PlayerInputFrame<'_>, dt: f64) {
        if self.lockstep && self.waiting.is_none() && !self.fingers.playing() {
            return;
        }
        let play = &mut self.fingers;
        play.clock += dt;
        play.recognizer.set_tuning(input.touch_tuning());
        play.recognizer
            .set_report_travel(input.touch_mode() == TouchMode::Practice);
        let mut due = Vec::new();
        while play.script.front().is_some_and(|e| e.t <= play.clock) {
            due.extend(play.script.pop_front());
        }
        let gestures = fingers::read(&mut play.recognizer, due, play.clock);
        for gesture in gestures {
            let events = self.touch.handle(gesture).events;
            if !events.is_empty() {
                self.scripted.push_back(events);
            }
        }
    }
}
