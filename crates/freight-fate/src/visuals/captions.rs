//! Captions: what the game just said, on screen for a sighted player.
//!
//! Read from the message log, not tapped from speech: the log already holds
//! exactly the lines worth reading back (menu rows as you arrow past them
//! and typed characters stay out of it, and a line the player's speech
//! settings silence never reaches it), in the language they were spoken.
//! The log counts every line it has ever taken, so a frame picks up only
//! what is new since the last one.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use ff_core::message_log::{MessageCategory, MessageLog};

/// How many captions show at once, newest at the bottom.
pub const MAX_SHOWN: usize = 3;
const FADE: Duration = Duration::from_millis(600);

#[derive(Clone, Debug, PartialEq)]
pub struct Caption {
    pub text: String,
    /// A driving event (`say_event`) rather than a screen announcement.
    pub event: bool,
    born: Instant,
    life: Duration,
}

impl Caption {
    /// Opacity now: 1 while fresh, fading to 0 over the last [`FADE`].
    pub fn alpha(&self, now: Instant) -> f32 {
        let age = now.saturating_duration_since(self.born);
        if age >= self.life {
            return 0.0;
        }
        let left = self.life - age;
        if left >= FADE {
            1.0
        } else {
            left.as_secs_f32() / FADE.as_secs_f32()
        }
    }
}

/// Long enough to read: four seconds, plus time for the words, capped.
fn reading_time(text: &str) -> Duration {
    let chars = text.chars().count() as f32;
    Duration::from_secs_f32((4.0 + chars * 0.055).min(11.0))
}

#[derive(Default)]
pub struct Captions {
    seen: u64,
    shown: VecDeque<Caption>,
}

impl Captions {
    /// Take whatever the log gained since the last call; true when a
    /// caption was added.
    pub fn sync(&mut self, log: &MessageLog, now: Instant) -> bool {
        let total = log.total_added();
        if total < self.seen {
            // A fresh log (a new career): nothing in it is news to caption.
            self.seen = total;
            return false;
        }
        let new = (total - self.seen) as usize;
        self.seen = total;
        if new == 0 {
            return false;
        }
        let before = self.shown.len();
        let start = log.messages.len().saturating_sub(new);
        for message in &log.messages[start..] {
            let text = message.text.trim();
            if text.is_empty() {
                continue;
            }
            self.shown.push_back(Caption {
                text: text.to_string(),
                event: message.category == MessageCategory::Event,
                born: now,
                life: reading_time(text),
            });
        }
        let added = self.shown.len() > before;
        while self.shown.len() > MAX_SHOWN {
            self.shown.pop_front();
        }
        added
    }

    /// Drop expired captions; true while any caption is fading or one just
    /// went, so the display knows the picture changed.
    pub fn tick(&mut self, now: Instant) -> bool {
        let before = self.shown.len();
        self.shown.retain(|c| c.alpha(now) > 0.0);
        self.shown.len() != before || self.shown.iter().any(|c| c.alpha(now) < 1.0)
    }

    pub fn visible(&self) -> impl Iterator<Item = &Caption> {
        self.shown.iter()
    }

    pub fn is_empty(&self) -> bool {
        self.shown.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_lines_added_since_the_last_sync_become_captions() {
        let mut log = MessageLog::new();
        log.add("Freight Fate 1.9.", MessageCategory::General);
        let mut captions = Captions::default();
        let now = Instant::now();
        captions.sync(&log, now);
        assert_eq!(captions.visible().count(), 1);
        captions.sync(&log, now);
        assert_eq!(captions.visible().count(), 1, "nothing new, nothing added");
        log.add("Speed limit 55.", MessageCategory::Event);
        captions.sync(&log, now);
        let shown: Vec<_> = captions
            .visible()
            .map(|c| (c.text.as_str(), c.event))
            .collect();
        assert_eq!(
            shown,
            vec![("Freight Fate 1.9.", false), ("Speed limit 55.", true)]
        );
    }

    #[test]
    fn at_most_three_show_and_the_oldest_goes_first() {
        let mut log = MessageLog::new();
        let mut captions = Captions::default();
        for i in 0..5 {
            log.add(&format!("Line {i}."), MessageCategory::Event);
        }
        captions.sync(&log, Instant::now());
        let shown: Vec<_> = captions.visible().map(|c| c.text.clone()).collect();
        assert_eq!(shown, vec!["Line 2.", "Line 3.", "Line 4."]);
    }

    #[test]
    fn a_caption_fades_and_then_goes() {
        let mut log = MessageLog::new();
        log.add("Cruise off.", MessageCategory::Event);
        let mut captions = Captions::default();
        let start = Instant::now();
        captions.sync(&log, start);
        assert!(!captions.tick(start), "fresh: fully opaque");
        let life = reading_time("Cruise off.");
        let fading = start + life - Duration::from_millis(300);
        assert!(captions.tick(fading));
        assert!(captions.visible().next().unwrap().alpha(fading) < 1.0);
        captions.tick(start + life + Duration::from_millis(1));
        assert!(captions.is_empty());
    }
}
