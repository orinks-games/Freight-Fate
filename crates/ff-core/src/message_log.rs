//! The reviewable log of everything the game has said.
//!
//! One log backs every review control. `GameContext::say` and `say_event`
//! file each spoken line here under its category, and
//! `State::handle_message_review` walks it. Text the player does not need to
//! review a second time -- menu items as they arrow past them, the review
//! announcements themselves -- is spoken with `review=false` and never
//! reaches the log, which is what keeps navigation noise out of the history.
//!
//! Port of `freight_fate/message_log.py`.

use crate::speech_pacing::monotonic_seconds;

/// How many messages to keep. Long careers speak a lot, and a log that grows
/// for the whole session is both unbounded memory and a history nobody can
/// walk.
pub const DEFAULT_LIMIT: usize = 200;

/// How long a review session stays open after the last review key. Inside
/// the window the player is browsing, so new speech must not drag the cursor
/// out from under them. Outside it they have gone back to driving, and the
/// next press means "repeat what was just said" rather than "resume where I
/// left off twenty messages ago".
pub const REVIEW_WINDOW_S: f64 = 10.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MessageCategory {
    General,
    Event,
    /// What the co-driver did and decided while they held the wheel: their
    /// own review, so the player coming back from the sleeper can hear the
    /// shift without the road's lines in between (owner, 2026-10-19).
    CoDriver,
}

impl MessageCategory {
    /// The Python enum's `.value`.
    pub fn value(self) -> &'static str {
        match self {
            MessageCategory::General => "general",
            MessageCategory::Event => "event",
            MessageCategory::CoDriver => "co_driver",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Message {
    pub text: String,
    pub category: MessageCategory,
}

/// A source of monotonic seconds; injectable so tests can drive the review
/// window without sleeping.
pub type Clock = Box<dyn FnMut() -> f64>;

// The review filters, in the order the category keys cycle through them.
//
// The co-driver's filter is last, and only on offer once a co-driver has
// logged something: a career that never drove as a team never steps onto
// an empty list.
const FILTERS: [Option<MessageCategory>; 4] = [
    None,
    Some(MessageCategory::General),
    Some(MessageCategory::Event),
    Some(MessageCategory::CoDriver),
];

pub struct MessageLog {
    pub messages: Vec<Message>,
    pub limit: usize,
    clock: Clock,

    /// When the last review key was pressed, or None if the player has not
    /// reviewed yet. Drives the review-session window.
    reviewed_at: Option<f64>,

    /// None means show messages from every category.
    pub filter: Option<MessageCategory>,

    /// Position within the currently filtered list.
    /// -1 means there is no current message.
    pub index: isize,

    /// True while the cursor sits on a message the player has not reviewed
    /// yet. The first press of the back key then re-speaks it rather than
    /// stepping past it, so the key reads as "repeat what was just said"
    /// before it reads as "go back one".
    fresh: bool,

    /// How many lines have ever been added (not reduced by trimming), so a
    /// caller can tell whether anything new was logged since a mark.
    added: u64,
}

impl Default for MessageLog {
    fn default() -> Self {
        Self::new()
    }
}

impl MessageLog {
    /// The default log: `DEFAULT_LIMIT` messages, the monotonic clock.
    pub fn new() -> Self {
        Self::with_limit(DEFAULT_LIMIT)
    }

    pub fn with_limit(limit: usize) -> Self {
        Self::with_clock(limit, Box::new(monotonic_seconds))
    }

    pub fn with_clock(limit: usize, clock: Clock) -> Self {
        Self {
            messages: Vec::new(),
            limit,
            clock,
            reviewed_at: None,
            filter: None,
            index: -1,
            fresh: false,
            added: 0,
        }
    }

    pub fn add(&mut self, text: &str, category: MessageCategory) {
        if text.is_empty() {
            return;
        }
        // The same line twice running is one thing to review, not two: a
        // status key pressed twice should not make the reviewer step over a
        // duplicate.
        if self.messages.last().is_some_and(|last| last.text == text) {
            return;
        }

        let old_filtered = self.filtered_positions();
        let was_at_latest =
            old_filtered.is_empty() || self.index >= old_filtered.len() as isize - 1;
        // The absolute position of the message the cursor is on, so it can
        // be found again after a trim (the Python tracked the object itself).
        let current = if self.index >= 0 && (self.index as usize) < old_filtered.len() {
            Some(old_filtered[self.index as usize])
        } else {
            None
        };

        self.messages.push(Message {
            text: text.to_string(),
            category,
        });
        self.added += 1;
        let dropped = self.trim();

        let new_filtered = self.filtered_positions();

        // Follow new messages only when the reviewer was already at the end.
        if was_at_latest && !new_filtered.is_empty() {
            self.index = new_filtered.len() as isize - 1;
            self.fresh = true;
        } else {
            // Trimming can drop messages from under a reviewer who has walked
            // back, so track the message itself rather than its old position.
            let shifted = current.and_then(|abs| abs.checked_sub(dropped));
            self.index = Self::locate(shifted, &new_filtered);
        }
    }

    /// Positions (into `messages`) of the messages the filter shows.
    fn filtered_positions(&self) -> Vec<usize> {
        match self.filter {
            None => (0..self.messages.len()).collect(),
            Some(filter) => self
                .messages
                .iter()
                .enumerate()
                .filter(|(_, message)| message.category == filter)
                .map(|(position, _)| position)
                .collect(),
        }
    }

    /// Lines ever added, trimming aside. Only grows.
    pub fn total_added(&self) -> u64 {
        self.added
    }

    pub fn filtered_messages(&self) -> Vec<&Message> {
        self.filtered_positions()
            .into_iter()
            .map(|position| &self.messages[position])
            .collect()
    }

    pub fn current_message(&mut self) -> Option<&Message> {
        let positions = self.filtered_positions();
        if positions.is_empty() {
            self.index = -1;
            return None;
        }
        self.clamp_index();
        Some(&self.messages[positions[self.index as usize]])
    }

    /// Open or extend a review session.
    ///
    /// A press that arrives after the window has closed starts over: back at
    /// the newest message, showing every category. Without this a player who
    /// glanced at the history mid-run stays parked there for the rest of the
    /// drive, and the next press reads them something from twenty messages
    /// ago -- or, with a category filter still set, silently hides half of
    /// what has happened since.
    fn begin_review(&mut self) {
        let now = (self.clock)();
        let lapsed = match self.reviewed_at {
            None => true,
            Some(at) => now - at > REVIEW_WINDOW_S,
        };
        self.reviewed_at = Some(now);
        if lapsed {
            // The POSITION goes stale after ten seconds; the chosen category
            // does not. Clearing both meant a driver who set the filter to
            // events and then spent eleven seconds actually driving got
            // dropped back to all messages -- which from the seat is
            // indistinguishable from it happening at random (Tim S,
            // 2026-08-21). "Show me events" is a stated preference, and it is
            // exactly the preference someone sets to make the cab navigable.
            //
            // Clearing it was NOT arbitrary, though, and the reason has to
            // survive: a filter left on Event once hid an entire delivery
            // settlement in review (playtest). The harm there was SILENCE --
            // the settlement was not merely filtered, it was unreachable
            // without the player knowing there was anything to reach. So the
            // filter stands and `hidden_newer_count` reports what it is
            // keeping out, which answers the old bug without discarding the
            // preference that fixed Tim's.
            self.move_to_latest();
        }
    }

    /// The message a review action acts on, opening a session first.
    ///
    /// `current_message` stays a plain query; this is what a key press
    /// should use, so copying after a long silence copies what was just said
    /// rather than wherever the cursor was left.
    pub fn message_in_review(&mut self) -> Option<&Message> {
        self.begin_review();
        self.current_message()
    }

    pub fn previous_message(&mut self) -> Option<&Message> {
        self.begin_review();
        let positions = self.filtered_positions();

        if positions.is_empty() {
            self.index = -1;
            return None;
        }

        // The first press after new speech repeats it instead of stepping back.
        if self.fresh {
            self.fresh = false;
            self.clamp_index();
            return Some(&self.messages[positions[self.index as usize]]);
        }

        if self.index <= 0 {
            return None;
        }
        self.index -= 1;

        Some(&self.messages[positions[self.index as usize]])
    }

    pub fn next_message(&mut self) -> Option<&Message> {
        self.begin_review();
        let positions = self.filtered_positions();
        self.fresh = false;

        if positions.is_empty() {
            self.index = -1;
            return None;
        }

        if self.index >= positions.len() as isize - 1 {
            return None;
        }
        self.index += 1;

        Some(&self.messages[positions[self.index as usize]])
    }

    pub fn first_message(&mut self) -> Option<&Message> {
        self.begin_review();
        let positions = self.filtered_positions();
        self.fresh = false;

        if positions.is_empty() {
            self.index = -1;
            return None;
        }

        self.index = 0;
        Some(&self.messages[positions[0]])
    }

    pub fn last_message(&mut self) -> Option<&Message> {
        self.begin_review();
        let positions = self.filtered_positions();
        self.fresh = false;

        if positions.is_empty() {
            self.index = -1;
            return None;
        }

        self.index = positions.len() as isize - 1;
        Some(&self.messages[positions[self.index as usize]])
    }

    fn filter_position(&self) -> usize {
        FILTERS
            .iter()
            .position(|candidate| *candidate == self.filter)
            .expect("the filter is always one of FILTERS")
    }

    pub fn previous_category(&mut self) -> Option<String> {
        self.begin_review();
        let position = self.filter_position();
        if position == 0 {
            return None;
        }
        self.filter = FILTERS[position - 1];
        self.move_to_latest();
        Some(self.category_name())
    }

    pub fn next_category(&mut self) -> Option<String> {
        self.begin_review();
        let position = self.filter_position();
        if position >= FILTERS.len() - 1 {
            return None;
        }
        let next = FILTERS[position + 1];
        if next == Some(MessageCategory::CoDriver) && !self.has(MessageCategory::CoDriver) {
            return None;
        }
        self.filter = next;
        self.move_to_latest();
        Some(self.category_name())
    }

    /// Messages newer than the cursor that the filter is keeping out.
    ///
    /// Zero when nothing is filtered. This is what stops a category filter
    /// from hiding something the driver needed: the review can say "and two
    /// newer messages outside this filter" rather than leaving them to find
    /// out later that the settlement never appeared.
    pub fn hidden_newer_count(&mut self) -> usize {
        let Some(filter) = self.filter else {
            return 0;
        };
        let visible = self.filtered_positions();
        if visible.is_empty() {
            return self
                .messages
                .iter()
                .filter(|m| m.category != filter)
                .count();
        }
        self.clamp_index();
        let newest_shown = &self.messages[visible[self.index as usize]];
        // The Python asked `list.index(newest_shown)`, which finds the first
        // message EQUAL to the one shown (text and category), not necessarily
        // the one the cursor is on; kept as is so the count matches.
        let after = self
            .messages
            .iter()
            .position(|m| m == newest_shown)
            .expect("the shown message is in the log")
            + 1;
        self.messages[after..]
            .iter()
            .filter(|m| m.category != filter)
            .count()
    }

    pub fn category_name(&self) -> String {
        match self.filter {
            None => "All".to_string(),
            Some(MessageCategory::General) => "General".to_string(),
            Some(MessageCategory::Event) => "Event".to_string(),
            Some(MessageCategory::CoDriver) => "Co-driver".to_string(),
        }
    }

    /// Whether any message in the log is of this category.
    pub fn has(&self, category: MessageCategory) -> bool {
        self.messages
            .iter()
            .any(|message| message.category == category)
    }

    /// Drop the oldest messages past the limit; returns how many went.
    fn trim(&mut self) -> usize {
        if self.limit > 0 && self.messages.len() > self.limit {
            let excess = self.messages.len() - self.limit;
            self.messages.drain(..excess);
            excess
        } else {
            0
        }
    }

    fn locate(message: Option<usize>, positions: &[usize]) -> isize {
        if positions.is_empty() {
            return -1;
        }
        if let Some(abs) = message {
            if let Some(position) = positions.iter().position(|&candidate| candidate == abs) {
                return position as isize;
            }
        }
        // The message the player was on has aged out; the oldest kept one is
        // the closest place to leave them.
        0
    }

    fn move_to_latest(&mut self) {
        let count = self.filtered_positions().len();
        // Landing on a category shows its newest message, which the player
        // has not heard in this category yet.
        self.fresh = count > 0;

        if count > 0 {
            self.index = count as isize - 1;
        } else {
            self.index = -1;
        }
    }

    fn clamp_index(&mut self) {
        let count = self.filtered_positions().len() as isize;

        if count == 0 {
            self.index = -1;
        } else if self.index < 0 {
            self.index = 0;
        } else if self.index >= count {
            self.index = count - 1;
        }
    }
}

#[cfg(test)]
mod tests {
    //! Ported from `tests/test_message_log.py`.
    use super::*;
    use std::cell::Cell;
    use std::rc::Rc;

    struct FakeClock {
        now: Rc<Cell<f64>>,
    }

    impl FakeClock {
        fn new() -> Self {
            Self {
                now: Rc::new(Cell::new(1000.0)),
            }
        }

        fn clock(&self) -> Clock {
            let now = Rc::clone(&self.now);
            Box::new(move || now.get())
        }

        fn advance(&self, seconds: f64) {
            self.now.set(self.now.get() + seconds);
        }
    }

    fn text(message: Option<&Message>) -> &str {
        &message.expect("a message").text
    }

    #[test]
    fn test_messages_can_be_reviewed() {
        let mut log = MessageLog::new();

        log.add("General one", MessageCategory::General);
        log.add("Event one", MessageCategory::Event);
        log.add("General two", MessageCategory::General);

        assert_eq!(text(log.current_message()), "General two");
        // The first step back repeats what was just said, then walks.
        assert_eq!(text(log.previous_message()), "General two");
        assert_eq!(text(log.previous_message()), "Event one");
        assert_eq!(text(log.previous_message()), "General one");
    }

    #[test]
    fn test_new_speech_re_arms_the_repeat_for_a_reviewer_at_the_latest() {
        let mut log = MessageLog::new();

        log.add("General one", MessageCategory::General);
        assert_eq!(text(log.previous_message()), "General one");

        log.add("General two", MessageCategory::General);
        assert_eq!(text(log.previous_message()), "General two");
    }

    #[test]
    fn test_new_speech_does_not_move_a_reviewer_who_walked_back() {
        let mut log = MessageLog::new();

        log.add("One", MessageCategory::General);
        log.add("Two", MessageCategory::General);
        log.previous_message(); // repeats "Two"
        log.previous_message(); // steps to "One"

        log.add("Three", MessageCategory::General);

        // Still parked where they were; getting back to now is Ctrl+period.
        assert_eq!(text(log.current_message()), "One");
        assert_eq!(text(log.last_message()), "Three");
    }

    #[test]
    fn test_stepping_forward_does_not_repeat() {
        let mut log = MessageLog::new();

        log.add("One", MessageCategory::General);
        log.add("Two", MessageCategory::General);

        assert_eq!(text(log.first_message()), "One");
        assert_eq!(text(log.next_message()), "Two");
        // Explicit positioning cancels the repeat, so back really goes back.
        assert_eq!(text(log.previous_message()), "One");
    }

    #[test]
    fn test_category_change_moves_to_latest_matching_message() {
        let mut log = MessageLog::new();

        log.add("General one", MessageCategory::General);
        log.add("Event one", MessageCategory::Event);
        log.add("General two", MessageCategory::General);

        assert_eq!(log.next_category().as_deref(), Some("General"));
        assert_eq!(text(log.current_message()), "General two");

        assert_eq!(log.next_category().as_deref(), Some("Event"));
        assert_eq!(text(log.current_message()), "Event one");
    }

    #[test]
    fn test_log_is_bounded_and_drops_the_oldest() {
        let mut log = MessageLog::with_limit(3);

        for index in 0..5 {
            log.add(&format!("Message {index}"), MessageCategory::General);
        }

        let texts: Vec<&str> = log.messages.iter().map(|m| m.text.as_str()).collect();
        assert_eq!(texts, ["Message 2", "Message 3", "Message 4"]);
        assert_eq!(text(log.first_message()), "Message 2");
    }

    #[test]
    fn test_reviewer_keeps_their_place_when_the_log_trims() {
        let mut log = MessageLog::with_limit(3);

        for index in 0..3 {
            log.add(&format!("Message {index}"), MessageCategory::General);
        }

        log.previous_message(); // repeat "Message 2"
        log.previous_message(); // step to "Message 1"
        assert_eq!(text(log.current_message()), "Message 1");

        log.add("Message 3", MessageCategory::General); // drops "Message 0"

        // Still on the same message, not shifted by the drop.
        assert_eq!(text(log.current_message()), "Message 1");
    }

    #[test]
    fn test_reviewer_on_a_trimmed_away_message_lands_on_the_oldest_kept() {
        let mut log = MessageLog::with_limit(2);

        log.add("One", MessageCategory::General);
        log.add("Two", MessageCategory::General);
        log.previous_message(); // repeat "Two"
        log.previous_message(); // step to "One"

        log.add("Three", MessageCategory::General); // "One" ages out

        assert_eq!(text(log.current_message()), "Two");
    }

    #[test]
    fn test_the_same_line_twice_running_is_logged_once() {
        let mut log = MessageLog::new();

        log.add("Speed limit 55.", MessageCategory::General);
        log.add("Speed limit 55.", MessageCategory::General);
        log.add("Fuel is low.", MessageCategory::General);
        log.add("Speed limit 55.", MessageCategory::General);

        let texts: Vec<&str> = log.messages.iter().map(|m| m.text.as_str()).collect();
        assert_eq!(
            texts,
            ["Speed limit 55.", "Fuel is low.", "Speed limit 55."]
        );
    }

    /// The playtest case: glance at the history mid-run, drive on, press again.
    #[test]
    fn test_a_lapsed_review_session_starts_again_at_the_newest() {
        let clock = FakeClock::new();
        let mut log = MessageLog::with_clock(DEFAULT_LIMIT, clock.clock());

        for line in ["One.", "Two.", "Three."] {
            log.add(line, MessageCategory::General);
        }

        log.previous_message(); // repeats "Three."
        log.previous_message(); // steps to "Two."
        assert_eq!(text(log.current_message()), "Two.");

        // Back to driving. More happens while the cursor sits in the past.
        clock.advance(REVIEW_WINDOW_S + 1.0);
        for line in ["Four.", "Five."] {
            log.add(line, MessageCategory::General);
        }

        assert_eq!(text(log.previous_message()), "Five.");
    }

    #[test]
    fn test_an_active_review_session_is_not_dragged_forward() {
        let clock = FakeClock::new();
        let mut log = MessageLog::with_clock(DEFAULT_LIMIT, clock.clock());

        for line in ["One.", "Two.", "Three."] {
            log.add(line, MessageCategory::General);
        }

        log.previous_message();
        log.previous_message();
        assert_eq!(text(log.current_message()), "Two.");

        // Still browsing: a message arriving must not move the cursor.
        clock.advance(REVIEW_WINDOW_S / 2.0);
        log.add("Four.", MessageCategory::General);

        assert_eq!(text(log.previous_message()), "One.");
    }

    /// Two tester reports pull opposite ways, and both are right.
    ///
    /// A filter left on Event once hid a whole delivery settlement in review,
    /// so the lapse used to clear it. But clearing it meant Tim S, who sets
    /// the filter to Event precisely because it makes the cab navigable, lost
    /// his choice every time he spent eleven seconds driving instead of
    /// browsing -- which reads as the filter dropping at random (2026-08-21).
    ///
    /// The harm in the first report was SILENCE, not filtering: the
    /// settlement was unreachable without any sign it existed. So the
    /// preference stands, and the log can say what the filter is keeping out.
    #[test]
    fn test_a_lapsed_session_keeps_the_filter_but_never_hides_in_silence() {
        let clock = FakeClock::new();
        let mut log = MessageLog::with_clock(DEFAULT_LIMIT, clock.clock());

        log.add("Hazard ahead.", MessageCategory::Event);
        assert_eq!(log.next_category().as_deref(), Some("General"));
        assert_eq!(log.next_category().as_deref(), Some("Event"));

        clock.advance(REVIEW_WINDOW_S + 1.0);
        log.add("Delivery complete.", MessageCategory::General);

        // The driver's choice survives the lapse.
        assert_eq!(log.category_name(), "Event");
        assert_eq!(text(log.previous_message()), "Hazard ahead.");
        // And the settlement is not silently gone: the log knows it is out there.
        assert_eq!(log.hidden_newer_count(), 1);

        // Winding the filter back the way the player does reaches it, as ever.
        assert_eq!(log.previous_category().as_deref(), Some("General"));
        assert_eq!(log.previous_category().as_deref(), Some("All"));
        assert_eq!(text(log.previous_message()), "Delivery complete.");
        assert_eq!(log.hidden_newer_count(), 0);
    }

    #[test]
    fn test_copying_after_a_lapse_copies_the_newest() {
        let clock = FakeClock::new();
        let mut log = MessageLog::with_clock(DEFAULT_LIMIT, clock.clock());

        log.add("One.", MessageCategory::General);
        log.add("Two.", MessageCategory::General);
        log.previous_message();
        log.previous_message();
        assert_eq!(text(log.current_message()), "One.");

        clock.advance(REVIEW_WINDOW_S + 1.0);
        assert_eq!(text(log.message_in_review()), "Two.");
    }

    #[test]
    fn test_empty_log_returns_none() {
        let mut log = MessageLog::new();

        assert!(log.current_message().is_none());
        assert!(log.previous_message().is_none());
        assert!(log.next_message().is_none());
        assert!(log.first_message().is_none());
        assert!(log.last_message().is_none());
    }
}
