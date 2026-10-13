//! Pacing, repeat suppression, and priority for the driving event voice.
//!
//! The event channel is a queue the game cannot inspect: it hands lines to a
//! voice that speaks them in submission order, and nothing in Prism will say
//! what is still waiting. Three things go wrong when the road talks straight
//! into it, all three reported from the same tester transcript (2026-08-11):
//!
//! * **The same moment said several times.** A code path that notices
//!   something runs every frame, so one sideswipe arrives as three identical
//!   lines inside half a second.
//! * **A standing condition read out forever.** A damaged load, an engine
//!   held at redline -- the truck is still in that state, so the warning
//!   fires again every few seconds for the rest of the drive. The player
//!   needs it when it starts and again when it gets worse, not on a loop.
//! * **The line that mattered buried.** The stop the player planned waits
//!   behind weather, tolls, and traffic chatter, and is still waiting when
//!   the exit has gone by.
//!
//! [`EventSpeechPacer`] answers all three from one place.
//!
//! Its original job -- and still its core -- is the backlog projection: each
//! submitted line extends a projected clear time by its estimated speaking
//! duration, so the game knows roughly when the voice falls silent. A queued
//! line that would START speaking more than its priority's budget after the
//! moment it described is by definition stale, and the caller delivers it
//! interrupting instead, which purges the dead backlog. Interrupting lines
//! reset the projection to truth, so estimate drift never outlives one
//! backlog.
//!
//! On top of that it remembers what the player has already heard (so a
//! repeat inside `REPEAT_WINDOW_S` never reaches the voice twice), what each
//! standing condition last said (so it speaks again only when it has
//! something new to say), and how long a line of each priority is willing to
//! wait -- a route announcement waits a moment behind chatter and then goes
//! ahead of it.
//!
//! The purge that delivers an interrupting line cuts both ways: it flushes
//! dead chatter, but it also lands on whatever the voice was mid-way through
//! -- and when that was a ROUTE or CRITICAL line (the weigh station notice,
//! the planned stop), the player lost an instruction, not colour (a tester
//! blew a weigh station this way, 2026-08-12). The pacer therefore keeps the
//! newest such line alongside its projected finish time, and an interrupt
//! arriving before that moment hands the line back to the caller to queue
//! right behind the interrupting one: safety line first, then the line it
//! stepped on. Only when the cut landed EARLY, though: a line the player had
//! mostly heard is dropped rather than said again whole, because a repeat
//! from the top reads as a stutter ("Start on West 14th Avenue" twice, a
//! whole state-crossing welcome twice in a tester's log, 2026-09-01). See
//! `MOSTLY_HEARD_FRACTION`.
//!
//! Durations are estimated from text length at a conservative default-voice
//! speaking rate. A faster voice just flushes a little less eagerly than it
//! could; a slower one flushes a little late but stays bounded -- either way
//! the player never again waits through a paragraph of expired narration.
//!
//! Port of `freight_fate/speech_pacing.py`.

use std::collections::HashMap;
use std::sync::OnceLock;
use std::time::Instant;

mod receipts;
use receipts::DeliveryReceipts;
pub use receipts::DeliveryStatus;

/// Seconds since the first call: the default pacer clock, the equivalent of
/// Python's `time.monotonic`.
pub fn monotonic_seconds() -> f64 {
    static START: OnceLock<Instant> = OnceLock::new();
    START.get_or_init(Instant::now).elapsed().as_secs_f64()
}

/// How long a line is willing to wait behind what is already speaking.
///
/// `Ambient` is the running commentary of the road -- weather, tolls, state
/// lines, roadside colour. Missing one costs the player nothing.
///
/// `Route` is the drive itself: the stop the player planned, the exit they
/// have to take. It gets a much shorter patience than chatter, so it goes
/// ahead of a backlog rather than behind it.
///
/// `Critical` is a warning to act on now. It is always delivered
/// interrupting, so it never consults the budget at all.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum EventPriority {
    Ambient = 0,
    Route = 1,
    Critical = 2,
}

/// What a line of informational speech is ABOUT.
///
/// Orthogonal to [`EventPriority`], which says how long a line waits and
/// whether staleness may drop it. Urgency alone gave the verbosity system
/// only one lever -- length -- which is why compressing every message (stage
/// S2) did not make the drive quieter: it never reduced how many things
/// speak. The rung table below cuts by category instead.
///
/// Flavor -- billboards, place names, landmarks, roadside colour -- is
/// deliberately absent. It answers to the chatter switches and the
/// place-callouts ladder, and the owner set those separately (2026-08-15).
///
/// A recurring miscategorisation the review caught three times (2026-08-16):
/// a line that names a key the player must press to keep moving is never
/// CONFIRMATION. CONFIRMATION is an outcome report -- the assist cleared it,
/// the latch caught, here is what happened. A stalled engine, a grounded
/// tractor, a scrapped chain set are unrequested failures that stop the
/// truck and demand a next action; at urgent_only CONFIRMATION is
/// an EARCON, so miscategorising one of these turns the instruction that
/// gets the truck moving again into a chime. Route it by what actually
/// changed instead: SAFETY for a failure or recovery instruction, MONEY
/// for a cost report that does not require immediate action.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SpeechCategory {
    Safety,
    Navigation,
    /// Navigation you cannot recover from -- take this exit, turn here, you
    /// missed it -- against navigation that is a heads-up on what the road
    /// is about to do. Both are navigation and both speak at quiet; they
    /// part company at urgent_only, where the heads-up becomes a tone and
    /// the unrecoverable one keeps its words. Splitting them is what makes
    /// the two quietest rungs different settings rather than near-copies
    /// (owner, 2026-08-17: the strict "only if you must act" rule describes
    /// urgent_only, and quiet should be "still very little" above it).
    NavigationAdvisory,
    Money,
    Coaching,
    Confirmation,
    Status,
    /// The other vehicles on the road when nothing is asked of the driver:
    /// a slow car or brake lights miles ahead, adaptive cruise settling in
    /// behind someone, a traffic pack, lane keeping going around a slow
    /// truck. The pass-by and engine sounds already carry it, and the assist
    /// is the one answering it, so only standard puts it into words. Quiet
    /// is meant to be quiet: a player's report (2026-10-03) was that quiet
    /// still read out every slow box truck and its speed. Traffic the driver
    /// must answer -- closing on a slow vehicle, stopped traffic, a merge
    /// into the lane -- is a hazard and stays SAFETY.
    Traffic,
}

impl SpeechCategory {
    /// Every category, in declaration order (Traffic is the Rust-era
    /// addition, appended so the Python order of the rest is unchanged).
    pub const ALL: [SpeechCategory; 8] = [
        SpeechCategory::Safety,
        SpeechCategory::Navigation,
        SpeechCategory::NavigationAdvisory,
        SpeechCategory::Money,
        SpeechCategory::Coaching,
        SpeechCategory::Confirmation,
        SpeechCategory::Status,
        SpeechCategory::Traffic,
    ];

    /// The Python `StrEnum` value.
    pub fn value(self) -> &'static str {
        match self {
            SpeechCategory::Safety => "safety",
            SpeechCategory::Navigation => "navigation",
            SpeechCategory::NavigationAdvisory => "navigation_advisory",
            SpeechCategory::Money => "money",
            SpeechCategory::Coaching => "coaching",
            SpeechCategory::Confirmation => "confirmation",
            SpeechCategory::Status => "status",
            SpeechCategory::Traffic => "traffic",
        }
    }

    /// `SpeechCategory(value)`.
    pub fn from_value(value: &str) -> Option<SpeechCategory> {
        SpeechCategory::ALL
            .into_iter()
            .find(|category| category.value() == value)
    }
}

/// What a rung does with a category.
///
/// `Silent` stops the words and makes no sound in their place, and keeps
/// them out of message review. Status-query keys still answer on demand.
///
/// There used to be an `Earcon` disposition as well: four synthesized notes
/// (Road ahead, Coaching, Confirmation and Status note) stood in for the
/// words a rung cut. The owner retired it on 2026-10-03 -- a quiet rung
/// should be quiet, not a run of bloops announcing lines nobody hears.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Disposition {
    /// speaks, normal rendering
    Full,
    /// speaks, terse rendering -- never silence
    Terse,
    /// speaks the first time per leg, then silent
    FirstOccurrence,
    /// speaks on enter, worsen, and clear only
    Transitions,
    /// no words, no sound; status keys still answer
    Silent,
}

impl Disposition {
    pub const ALL: [Disposition; 5] = [
        Disposition::Full,
        Disposition::Terse,
        Disposition::FirstOccurrence,
        Disposition::Transitions,
        Disposition::Silent,
    ];

    /// The Python `StrEnum` value.
    pub fn value(self) -> &'static str {
        match self {
            Disposition::Full => "full",
            Disposition::Terse => "terse",
            Disposition::FirstOccurrence => "first",
            Disposition::Transitions => "transitions",
            Disposition::Silent => "silent",
        }
    }
}

// "coaching" was a fourth rung above standard, and it was removed on
// 2026-08-17 because it never differed from standard at the voice. Its two
// table cells (COACHING full rather than once-per-leg, STATUS full rather
// than on transitions) only bite where a coaching tip repeats, and exactly
// one line in the game carries SpeechCategory::Coaching -- so cycling to it
// changed nothing a player could hear. In a game read entirely by ear, a
// setting that offers a choice and produces no audible difference is worse
// than one fewer choice: it reads as broken. The CATEGORY stays (that one
// line); it is the RUNG
// that is gone, and re-adding it is one row of a data table once there are
// tips to put in it.
pub const DRIVING_SPEECH_MODES: [&str; 3] = ["standard", "quiet", "urgent_only"];

/// One rung's row of the table: a disposition per category.
pub type DispositionRow = [(SpeechCategory, Disposition); 8];

/// The rung table. Read a row as "at this rung, a line of this category is
/// delivered this way". Safety and immediate navigation always speak. Quiet
/// keeps short confirmations and status but not the traffic around the
/// truck; Urgent only omits routine costs, status and traffic.
pub const DRIVING_SPEECH_DISPOSITIONS: [(&str, DispositionRow); 3] = [
    (
        "standard",
        [
            (SpeechCategory::Safety, Disposition::Full),
            (SpeechCategory::Money, Disposition::Full),
            (SpeechCategory::Navigation, Disposition::Full),
            (SpeechCategory::NavigationAdvisory, Disposition::Full),
            (SpeechCategory::Coaching, Disposition::FirstOccurrence),
            (SpeechCategory::Confirmation, Disposition::Full),
            (SpeechCategory::Status, Disposition::Transitions),
            (SpeechCategory::Traffic, Disposition::Full),
        ],
    ),
    (
        "quiet",
        [
            (SpeechCategory::Safety, Disposition::Terse),
            (SpeechCategory::Money, Disposition::Terse),
            (SpeechCategory::Navigation, Disposition::Terse),
            (SpeechCategory::NavigationAdvisory, Disposition::Terse),
            (SpeechCategory::Coaching, Disposition::Silent),
            (SpeechCategory::Confirmation, Disposition::Terse),
            (SpeechCategory::Status, Disposition::Terse),
            (SpeechCategory::Traffic, Disposition::Silent),
        ],
    ),
    (
        "urgent_only",
        [
            (SpeechCategory::Safety, Disposition::Terse),
            (SpeechCategory::Money, Disposition::Silent),
            (SpeechCategory::Navigation, Disposition::Terse),
            (SpeechCategory::NavigationAdvisory, Disposition::Silent),
            (SpeechCategory::Coaching, Disposition::Silent),
            (SpeechCategory::Confirmation, Disposition::Silent),
            (SpeechCategory::Status, Disposition::Silent),
            (SpeechCategory::Traffic, Disposition::Silent),
        ],
    ),
];

pub const DEFAULT_DRIVING_SPEECH: &str = "standard";

/// `DRIVING_SPEECH_DISPOSITIONS[mode]`, or None for a rung not in the table.
pub fn disposition_row(mode: &str) -> Option<&'static DispositionRow> {
    DRIVING_SPEECH_DISPOSITIONS
        .iter()
        .find(|(name, _)| *name == mode)
        .map(|(_, row)| row)
}

/// One cell of a row.
pub fn row_disposition(row: &DispositionRow, category: SpeechCategory) -> Option<Disposition> {
    row.iter()
        .find(|(candidate, _)| *candidate == category)
        .map(|(_, disposition)| *disposition)
}

/// How this rung delivers this category.
///
/// An unknown rung reads as the default rather than raising: a settings
/// file edited by hand must not be able to crash the drive. A `None`
/// category is an unclassified call site and always speaks -- the rendering
/// still follows the rung, so it gets shorter but never disappears.
pub fn disposition_for(mode: &str, category: Option<SpeechCategory>) -> Disposition {
    let row = disposition_row(mode)
        .or_else(|| disposition_row(DEFAULT_DRIVING_SPEECH))
        .expect("the default rung is in the table");
    match category {
        None => row_disposition(row, SpeechCategory::Safety).expect("every row rules on safety"),
        Some(category) => row_disposition(row, category).unwrap_or(Disposition::Full),
    }
}

/// A source of monotonic seconds; injectable so tests can drive the
/// projection without sleeping.
pub type Clock = Box<dyn FnMut() -> f64>;

/// "Is the moment this line described still live?" -- consulted before a
/// cut line is handed back.
pub type Valid = Box<dyn Fn() -> bool>;

/// A cut ROUTE or CRITICAL line handed back for requeueing.
pub type Cut = (String, EventPriority);

// The newest ROUTE or CRITICAL line submitted, with the projection's
// estimate of when it finishes speaking. (The Python kept this as a 4-tuple
// of text, priority, done_at, valid.)
struct Protected {
    text: String,
    priority: EventPriority,
    done_at: f64,
    valid: Option<Valid>,
}

/// Keeps the dedicated event voice from performing the past.
///
/// See the module docs for the whole picture. The caller's contract:
///
/// * `is_repeat` decides whether the player has already heard this; a true
///   means say nothing at all.
/// * `note_spoken` records a line that did reach the voice.
/// * `is_silenced_repeat`/`note_silenced` are the same pair for a line the
///   driving speech rung cut to nothing -- a private
///   namespace so a silenced occurrence can dedupe itself without
///   ever registering as something `is_repeat` would recognise as heard.
/// * `note_interrupt` for an interrupting line (it purges the channel), or
///   `should_flush` for a queued one -- true there means the backlog has
///   gone stale and this line must be submitted interrupting instead.
/// * `note_interrupt` may hand back the ROUTE or CRITICAL line the purge cut
///   off mid-sentence; the caller resubmits it queued (`note_queued`) so the
///   player still hears it, right behind the line that cut it.
/// * `note_channel_purged` when speech outside the pacer's view (an info
///   reply on a shared voice) purges the channel -- same hand-back.
/// * `pause`/`resume` around a screen that takes the player off the road.
pub struct EventSpeechPacer {
    clock: Clock,
    clear_at: f64,
    /// text -> when the player last heard it.
    recent: HashMap<String, f64>,
    /// condition key -> the last thing said about it.
    conditions: HashMap<String, String>,
    // The same two maps, but for occurrences the driving speech rung
    // silenced (never the words). Kept separate from
    // `recent`/`conditions` on purpose: those two belong to what the player
    // actually heard, and `is_repeat` consults them to decide whether a
    // genuinely spoken line would be news. If a silenced occurrence wrote
    // into that same state, raising the rung mid-drive while a standing
    // condition was still active (still locked out, still at redline) would
    // find the SILENCED text sitting in `conditions`, read the now-audible
    // occurrence as an unchanged repeat, and skip it -- exactly the rung
    // promising full sentences going quiet for the condition the player
    // raised it to hear about.
    silenced_recent: HashMap<String, f64>,
    silenced_conditions: HashMap<String, String>,
    /// Set by pause(): the next line purges the channel, so anything the
    /// voice was still holding when the player stepped away cannot surface
    /// behind it.
    purge_next: bool,
    /// The newest ROUTE or CRITICAL line submitted, with the projection's
    /// estimate of when it finishes speaking. An interrupt landing before
    /// that moment plausibly cut it off mid-sentence; it is handed back to
    /// the caller so the player still hears it.
    protected: Option<Protected>,
    /// Set by should_flush when its purge cut a line still speaking;
    /// collected once by take_flush_cut. See that method.
    flush_cut: Option<Cut>,
    /// text -> until when further rescues of it are refused. See
    /// `take_protected`: one rescue per line per window.
    rescued_until: HashMap<String, f64>,
    /// The line the latest cut destroyed because the player had already
    /// heard most of it; collected once by `take_mostly_heard` so the
    /// caller can name it in the transcript. See `MOSTLY_HEARD_FRACTION`.
    mostly_heard: Option<String>,
    receipts: DeliveryReceipts,
    /// When the voice last went from quiet to speaking: the start of the
    /// backlog `clear_at` projects the end of. See `should_flush`.
    busy_since: f64,
    /// Whether every line in that backlog is ROUTE or CRITICAL.
    backlog_urgent: bool,
}

impl Default for EventSpeechPacer {
    fn default() -> Self {
        Self::new()
    }
}

impl EventSpeechPacer {
    /// a queued line may start at most this far in the past
    pub const STALE_WAIT_S: f64 = 3.0;
    /// per-utterance pause before the voice gets going
    pub const BASE_UTTERANCE_S: f64 = 0.4;
    /// the default Windows voice at its default rate
    pub const CHARS_PER_S: f64 = 13.0;

    /// Two identical lines this close together are one thing happening, not
    /// two, whichever code path noticed it. Deliberately short: it collapses
    /// a burst of frames without ever swallowing the second press of a key
    /// the player pushed on purpose.
    pub const REPEAT_WINDOW_S: f64 = 2.5;

    /// Bound on the remembered-lines map, so a long career cannot grow it
    /// without limit.
    pub const RECENT_LIMIT: usize = 256;
    pub const RECENT_MEMORY_S: f64 = 300.0;

    /// After a line is rescued once, further cuts within this window drop it
    /// instead of replaying it -- long enough to cover any burst of stacked
    /// urgent lines (the trooper escalation runs well under this), short
    /// enough that the same words minutes later are a new moment that earns
    /// its own rescue.
    pub const RESCUE_ONCE_WINDOW_S: f64 = 30.0;

    /// How much of a cut line the player must already have heard for the
    /// pacer to drop it rather than hand it back to be said again whole.
    /// The fraction is elapsed speaking time over the line's estimated
    /// duration, so it is only as exact as the duration model.
    ///
    /// A line cut in its first words still owes the player the whole
    /// instruction; one cut in its last words has already delivered it, and
    /// repeating it from the top reads as a stutter (the same day's agent
    /// drives: "Start on West 14th Avenue", "Out of the gate and onto city
    /// streets", "In 1.4 kilometers, the destination exit" each spoken
    /// twice; a whole state-crossing welcome twice in a tester's log). The
    /// message log still holds a dropped line.
    ///
    /// The owner set it at a half on 2026-09-01 as the starting point;
    /// testers' ears move it.
    pub const MOSTLY_HEARD_FRACTION: f64 = 0.5;

    /// How long a line of each priority will wait behind a backlog before it
    /// is better to purge the channel and speak now. Route announcements
    /// have almost no patience: a planned stop that arrives after its exit
    /// is worse than a piece of chatter cut off mid-word.
    pub fn wait_budget_s(priority: EventPriority) -> f64 {
        match priority {
            EventPriority::Ambient => Self::STALE_WAIT_S,
            EventPriority::Route => 0.8,
            EventPriority::Critical => Self::STALE_WAIT_S,
        }
    }

    /// A pacer on the monotonic clock.
    pub fn new() -> Self {
        Self::with_clock(Box::new(monotonic_seconds))
    }

    pub fn with_clock(clock: Clock) -> Self {
        Self {
            clock,
            clear_at: 0.0,
            recent: HashMap::new(),
            conditions: HashMap::new(),
            silenced_recent: HashMap::new(),
            silenced_conditions: HashMap::new(),
            purge_next: false,
            protected: None,
            flush_cut: None,
            rescued_until: HashMap::new(),
            mostly_heard: None,
            receipts: DeliveryReceipts::default(),
            busy_since: 0.0,
            backlog_urgent: false,
        }
    }

    /// Extend the projection by `text`, noting when a quiet voice starts
    /// and whether everything it holds is a ROUTE or CRITICAL line.
    fn extend_projection(&mut self, now: f64, text: &str, priority: EventPriority) {
        if now >= self.clear_at {
            self.busy_since = now;
            self.backlog_urgent = true;
        }
        self.backlog_urgent &= priority >= EventPriority::Route;
        self.clear_at = now.max(self.clear_at) + Self::duration_s(text);
    }

    /// Start the projection over with `text`: the purge silenced the voice.
    fn restart_projection(&mut self, now: f64, text: &str, priority: EventPriority) {
        self.clear_at = now;
        self.extend_projection(now, text, priority);
    }

    /// Whether everything the voice holds is ROUTE or CRITICAL, started
    /// inside its pre-utterance pause and still true: this same instant of
    /// road, with nothing in it heard yet and nothing stale.
    fn backlog_is_this_instant(&self, now: f64) -> bool {
        self.backlog_urgent
            && now - self.busy_since < Self::BASE_UTTERANCE_S
            && self
                .protected
                .as_ref()
                .is_none_or(|held| held.valid.as_ref().is_none_or(|valid| valid()))
    }

    /// Swap the clock under a live pacer (the Python tests poked
    /// `pacer._clock`; the app's ladder tests need the same).
    pub fn set_clock(&mut self, clock: Clock) {
        self.clock = clock;
    }

    fn now(&mut self) -> f64 {
        (self.clock)()
    }

    fn duration_s(text: &str) -> f64 {
        Self::BASE_UTTERANCE_S + text.chars().count() as f64 / Self::CHARS_PER_S
    }

    // -- what the player has already heard ---------------------------------

    /// True when saying this again would tell the player nothing new.
    ///
    /// `key` names a standing condition -- a state of the world rather than
    /// a moment in it. A condition speaks when it starts and again only when
    /// what there is to say about it has changed, so a worsening number is
    /// news and the same number is not.
    ///
    /// `force` is for a line the player asked for: a status key, a
    /// deliberate replay. It is always heard.
    pub fn is_repeat(
        &mut self,
        text: &str,
        key: Option<&str>,
        force: bool,
        window: Option<f64>,
    ) -> bool {
        if force || text.is_empty() {
            return false;
        }
        if let Some(key) = key {
            if self.conditions.get(key).is_some_and(|said| said == text) {
                return true;
            }
        }
        let budget = window.unwrap_or(Self::REPEAT_WINDOW_S);
        if budget <= 0.0 {
            return false;
        }
        match self.recent.get(text).copied() {
            Some(last) => self.now() - last < budget,
            None => false,
        }
    }

    /// Record a line that reached the voice.
    pub fn note_spoken(&mut self, text: &str, key: Option<&str>) {
        if text.is_empty() {
            return;
        }
        let now = self.now();
        self.recent.insert(text.to_string(), now);
        if let Some(key) = key {
            self.conditions.insert(key.to_string(), text.to_string());
        }
        if self.recent.len() > Self::RECENT_LIMIT {
            self.recent
                .retain(|_, said| now - *said < Self::RECENT_MEMORY_S);
        }
    }

    /// A standing condition has cleared; let it announce itself afresh.
    pub fn forget_condition(&mut self, key: &str) {
        if let Some(text) = self.conditions.remove(key) {
            self.recent.remove(&text);
        }
        if let Some(text) = self.silenced_conditions.remove(key) {
            self.silenced_recent.remove(&text);
        }
        self.receipts.forget(key);
    }

    // -- what the rung silenced ----------------

    /// True when this silenced occurrence was already marked.
    ///
    /// The silenced branches' own dedup: mirrors [`Self::is_repeat`]'s rules
    /// exactly, but reads a namespace private to occurrences the rung cut,
    /// never `conditions`/`recent`. A silenced repeat must not go unmarked
    /// (a re-firing standing condition would log again every time), but it must
    /// equally never be mistaken for a genuinely spoken occurrence by
    /// [`Self::is_repeat`] once the rung changes and the condition is still
    /// active -- that would silence the very line the player raised the rung
    /// to hear.
    pub fn is_silenced_repeat(
        &mut self,
        text: &str,
        key: Option<&str>,
        window: Option<f64>,
    ) -> bool {
        if text.is_empty() {
            return false;
        }
        if let Some(key) = key {
            if self
                .silenced_conditions
                .get(key)
                .is_some_and(|said| said == text)
            {
                return true;
            }
        }
        let budget = window.unwrap_or(Self::REPEAT_WINDOW_S);
        if budget <= 0.0 {
            return false;
        }
        match self.silenced_recent.get(text).copied() {
            Some(last) => self.now() - last < budget,
            None => false,
        }
    }

    /// Record a silenced occurrence.
    pub fn note_silenced(&mut self, text: &str, key: Option<&str>) {
        if text.is_empty() {
            return;
        }
        let now = self.now();
        self.silenced_recent.insert(text.to_string(), now);
        if let Some(key) = key {
            self.silenced_conditions
                .insert(key.to_string(), text.to_string());
        }
        if self.silenced_recent.len() > Self::RECENT_LIMIT {
            self.silenced_recent
                .retain(|_, said| now - *said < Self::RECENT_MEMORY_S);
        }
    }

    // -- the backlog projection ---------------------------------------------

    /// Remember the newest line worth rescuing if an interrupt lands on it.
    ///
    /// A CONFIRMATION never takes the slot, whatever priority it was spoken
    /// at. Confirmations default to CRITICAL because they answer something
    /// the player just did, so they used to qualify -- and then the next
    /// interrupting line on the main channel handed the finished
    /// confirmation back to be requeued, where it resurfaced AFTER, and
    /// could bury, the line the player had actually just asked for. The slot
    /// exists to rescue a warning cut off mid-sentence; an outcome report
    /// that already finished, and whose outcome may since have been
    /// contradicted (the transmission flipped back, the units changed
    /// again), is not that. Found by the adversarial harness on
    /// settings_flips_mid_drive, and made routine rather than rare once
    /// pressed keys began interrupting again (2026-08-16).
    ///
    /// Public because the driving layer (and the Python tests) re-arm the
    /// slot with a liveness check after a plain submission.
    pub fn track(
        &mut self,
        text: &str,
        priority: EventPriority,
        category: Option<SpeechCategory>,
        valid: Option<Valid>,
    ) {
        if category == Some(SpeechCategory::Confirmation) {
            self.protected = None;
            return;
        }
        if priority >= EventPriority::Route {
            self.protected = Some(Protected {
                text: text.to_string(),
                priority,
                done_at: self.clear_at,
                valid,
            });
        }
    }

    /// The line a stale flush cut off mid-sentence, once, for requeueing.
    ///
    /// `note_interrupt` returns its cut directly; `should_flush` cannot,
    /// because its return value is the flush verdict. Same contract either
    /// way: a ROUTE or CRITICAL line still plausibly speaking is handed back.
    pub fn take_flush_cut(&mut self) -> Option<Cut> {
        self.flush_cut.take()
    }

    /// The line the latest cut dropped as mostly heard, once, for the
    /// transcript.
    ///
    /// The drop itself is the pacer's verdict (`MOSTLY_HEARD_FRACTION`);
    /// this only lets the caller say which line it was, the way it names a
    /// requeue, so a playtest log can be read for a drop that should not
    /// have happened.
    pub fn take_mostly_heard(&mut self) -> Option<String> {
        self.mostly_heard.take()
    }

    /// Hand over the protected line if it was plausibly cut mid-speech.
    ///
    /// The slot empties either way: a line is given back at most once per
    /// cut, and a line whose projected finish had already passed was heard
    /// in full, not destroyed. A line cutting itself is one line, not two,
    /// so it is never handed back behind its own delivery.
    ///
    /// A line still speaking is handed back only when the cut landed early
    /// in it. Past `MOSTLY_HEARD_FRACTION` of its estimated duration the
    /// player has the instruction and the tail is expendable; saying the
    /// whole line again is the stutter the 1 September drives were full of.
    /// A line that had not started speaking at all (queued behind a
    /// backlog, its start still ahead) has a negative fraction and is
    /// always handed back -- the never-dropped contract for a line the
    /// player never heard a word of.
    ///
    /// And at most ONE rescue per line per window. A rescued line is
    /// re-queued and re-protected, so without this a CHAIN of urgent lines
    /// replays it after every one of them -- the 21 August build note has
    /// "Signal for the scale exit" speaking five times through a trooper
    /// escalation, and the transponder's green light three times. One
    /// rescue is the whole contract (the cut line gets to finish once); a
    /// line cut a second time in a moment that busy has been overtaken by
    /// events, and message review holds the words.
    fn take_protected(&mut self, cutting_text: Option<&str>) -> Option<Cut> {
        let held = self.protected.take()?;
        let Protected {
            text,
            priority,
            done_at,
            valid,
        } = held;
        if self.now() >= done_at || Some(text.as_str()) == cutting_text {
            return None;
        }
        if let Some(valid) = valid {
            if !valid() {
                // The moment the line described has passed -- the scale is
                // behind the truck, the damage total has moved on. Replaying
                // the words verbatim would state something that is no longer
                // true, which is worse than the silence (the adversarial
                // battery's "44% total damage while the truck was at 51%",
                // and the scale-exit instruction offered after the scale).
                // Message review holds it.
                return None;
            }
        }
        let now = self.now();
        let duration = Self::duration_s(&text);
        let heard = (now - (done_at - duration)) / duration;
        if heard >= Self::MOSTLY_HEARD_FRACTION {
            // Most of it was heard; the message log holds the rest.
            self.mostly_heard = Some(text);
            return None;
        }
        if now < self.rescued_until.get(&text).copied().unwrap_or(0.0) {
            return None;
        }
        let until = self.now() + Self::RESCUE_ONCE_WINDOW_S;
        self.rescued_until.insert(text.clone(), until);
        Some((text, priority))
    }

    /// Whether the only thing ahead of `text` in the voice is the protected
    /// ROUTE or CRITICAL line, saying its words right now and still true (its
    /// `valid`, when it has one), with nothing queued behind it.
    ///
    /// "Saying its words" is past the pre-utterance pause: a line cut inside
    /// that pause has not been heard at all, so the flush below restarting it
    /// ahead of the new line costs the player nothing -- that is the burst of
    /// route lines landing in one frame, and its hand-back stays. A line past
    /// it has been heard in part, and cutting it either replays it from the
    /// top or drops its tail.
    ///
    /// A line superseded by what happened since -- "assistance is holding for
    /// your gap" once the gap has come -- is not live, so the flush takes it
    /// and it is never handed back.
    fn only_a_live_line_ahead(&self, now: f64, text: &str) -> bool {
        self.protected.as_ref().is_some_and(|held| {
            let started_at = held.done_at - Self::duration_s(&held.text);
            now - started_at >= Self::BASE_UTTERANCE_S
                && now < held.done_at
                && self.clear_at <= held.done_at
                && held.text != text
                && held.valid.as_ref().is_none_or(|valid| valid())
        })
    }

    /// An interrupting line purges the channel: the projection restarts.
    ///
    /// Returns the ROUTE or CRITICAL line the purge plausibly cut off
    /// mid-sentence -- its projected finish had not yet passed -- so the
    /// caller can queue it right back behind the interrupting line. Chatter,
    /// lines already heard in full or mostly (`take_mostly_heard` names
    /// those), and a line interrupting itself return None: nothing worth
    /// giving back was destroyed.
    pub fn note_interrupt(
        &mut self,
        text: &str,
        priority: EventPriority,
        category: Option<SpeechCategory>,
        valid: Option<Valid>,
    ) -> Option<Cut> {
        let cut = self.take_protected(Some(text));
        self.interrupt_deliveries();
        self.purge_next = false;
        let now = self.now();
        self.restart_projection(now, text, priority);
        self.track(text, priority, category, valid);
        cut
    }

    /// Extend the projection for a line delivered queued, no verdict asked.
    ///
    /// For the deliveries that must never flush: a rescued cut-off line (it
    /// has to fall in BEHIND the line that cut it, never purge it) and event
    /// lines riding the main channel, where the backlog belongs to the main
    /// voice rather than the pacer.
    pub fn note_queued(
        &mut self,
        text: &str,
        priority: EventPriority,
        category: Option<SpeechCategory>,
        valid: Option<Valid>,
    ) {
        let now = self.now();
        self.extend_projection(now, text, priority);
        self.track(text, priority, category, valid);
    }

    /// Track a warning whose saved acknowledgement waits for delivery.
    pub fn track_delivery(&mut self, key: &str, text: &str) {
        self.receipts.track(key, text, self.clear_at);
    }

    /// Mark a configured silent disposition as handled immediately.
    pub fn complete_delivery(&mut self, key: &str, text: &str) {
        let now = self.now();
        self.receipts.track(key, text, now);
    }

    /// Consume a completed or interrupted receipt; pending receipts remain.
    pub fn delivery_status(&mut self, key: &str) -> Option<DeliveryStatus> {
        let now = self.now();
        self.receipts.status(key, now)
    }

    /// Whether one receipt-tracked warning is still reaching the player.
    pub fn delivery_pending(&mut self) -> bool {
        let now = self.now();
        self.receipts.has_pending(now)
    }

    /// A handed-back line spoken AHEAD of the line that flushed it: the
    /// channel is busy for its length too, but the flushing line stays the
    /// newest protected one, so a later flush rescues that line rather than
    /// this one a second time.
    ///
    /// The flushing line now starts after this one, so its projected finish
    /// moves with the channel's. Left where the flush put it, the line read
    /// as speaking since the flush, and a second flush or an interrupt while
    /// the hand-back still played dropped it as heard -- a turn call the
    /// player never heard a word of.
    pub fn note_ahead(&mut self, text: &str) {
        let duration = Self::duration_s(text);
        self.clear_at += duration;
        if let Some(held) = self.protected.as_mut() {
            held.done_at += duration;
        }
    }

    /// A rescued line is the same delivery, now queued to finish.
    pub fn resume_delivery(&mut self, text: &str) {
        self.receipts.resume_text(text, self.clear_at);
    }

    fn interrupt_deliveries(&mut self) {
        let now = self.now();
        self.receipts.interrupt_pending(now);
    }

    /// Speech outside the pacer's view purged the channel events ride on.
    ///
    /// Only meaningful when the event voice is collapsed onto the main
    /// channel: an info reply's interrupt there lands on whatever event line
    /// was mid-sentence. Returns the cut ROUTE or CRITICAL line exactly as
    /// [`Self::note_interrupt`] would; the projection falls with the purge
    /// (the interrupting speech itself is not the pacer's to time).
    pub fn note_channel_purged(&mut self) -> Option<Cut> {
        let cut = self.take_protected(None);
        self.interrupt_deliveries();
        self.clear_at = 0.0;
        cut
    }

    /// Whether the projection says the event voice is still speaking.
    ///
    /// The channel cannot be asked directly (nothing in Prism reports queue
    /// state), so this is the same estimate the staleness budget runs on. It
    /// is what audio ducking restores on: the mix steps back while this is
    /// true and comes back the frame it goes false. Purges, pauses, and
    /// resets zero the projection, so a silenced channel reads not-busy at
    /// once instead of waiting out a stale estimate.
    pub fn busy(&mut self) -> bool {
        self.now() < self.clear_at
    }

    /// Whether a CRITICAL line is still being spoken, by the same projection.
    pub fn speaking_critical(&mut self) -> bool {
        let now = self.now();
        self.protected
            .as_ref()
            .is_some_and(|held| held.priority == EventPriority::Critical && now < held.done_at)
    }

    /// Whether this queued line would start past its priority's budget.
    ///
    /// A pure reading -- the projection is not touched, nothing is tracked.
    /// The caller uses it to decide a line's fate BEFORE committing it to
    /// the channel: chatter that would start stale is dropped silently (UIA
    /// MostRecent semantics -- superseded telemetry is discarded, not read
    /// late), where [`Self::should_flush`] would instead deliver it
    /// interrupting. A purge armed by pause() is not staleness; that path
    /// stays with should_flush, whose first line back purges the backlog.
    pub fn would_start_stale(&mut self, _text: &str, priority: EventPriority) -> bool {
        if self.purge_next {
            return false;
        }
        let now = self.now();
        let start = now.max(self.clear_at);
        let budget = Self::wait_budget_s(priority);
        start - now > budget
    }

    /// Decide a queued line's fate and update the projection either way.
    ///
    /// Returns true when the line would otherwise start stale -- the caller
    /// must then submit it interrupting (which purges the dead backlog).
    /// `priority` sets how long this line is willing to wait: a route
    /// announcement gives a backlog of chatter under a second before it goes
    /// in front of it.
    ///
    /// The slot is re-armed with the line's category, as `note_interrupt`
    /// and `note_queued` re-arm it, so a confirmation delivered queued does
    /// not take it either. Without it, lane keeping's "The road narrows to
    /// two lanes." was handed back after the automatic braking that cut it
    /// and heard twice, eight seconds apart (QA drive, 2026-10-13).
    pub fn should_flush(
        &mut self,
        text: &str,
        priority: EventPriority,
        category: Option<SpeechCategory>,
        valid: Option<Valid>,
    ) -> bool {
        let now = self.now();
        self.flush_cut = None;
        if self.purge_next {
            // Coming back from a pause. Whatever the voice was still holding
            // when the player stepped away is about a mile they have already
            // been told about, so the first line back purges it -- and that
            // one really is stale, so it is dropped rather than rescued.
            self.purge_next = false;
            self.interrupt_deliveries();
            self.restart_projection(now, text, priority);
            self.protected = None;
            self.track(text, priority, category, valid);
            return true;
        }
        let start = now.max(self.clear_at);
        let budget = Self::wait_budget_s(priority);
        if start - now > budget && self.backlog_is_this_instant(now) {
            // Everything the voice holds is route or safety, still true, and
            // began inside its pre-utterance pause: nothing has been heard
            // and nothing is stale; it is this same instant of road (chatter
            // in it is still flushed, and a line gone untrue is still
            // superseded). A flush here purged it only to hand the
            // newest of it back ahead of this line -- the same order, the
            // same wait -- and anything older than that newest line was lost
            // without a word: "Off the ramp and onto city streets" gone
            // under "Entering ... zone", and every burst logged twice as a
            // requeue (live drive into Abilene, 2026-09-24). It queues.
            self.extend_projection(now, text, priority);
            self.track(text, priority, category, valid);
            return false;
        }
        if start - now > budget && self.only_a_live_line_ahead(now, text) {
            // Nothing stale is waiting: the wait is the rest of one ROUTE or
            // CRITICAL line the player is hearing, still true. A flush here
            // purged nothing but that line, and then either handed it back to
            // be said again from the top ahead of this one ("Exit speed 49.
            // You take exit 113" heard twice, agent drives 2026-09-24) or
            // dropped its tail ("Light red." cutting the take line). Waiting
            // it out is shorter than the replay, and it is heard once.
            self.extend_projection(now, text, priority);
            self.track(text, priority, category, valid);
            return false;
        }
        if start - now > budget {
            // A stale flush takes the backlog -- everything in it described
            // miles already driven -- but NOT a protected line that is still
            // speaking. The incoming line starting stale says nothing about
            // the outgoing one: a CRITICAL warning that began this very tick
            // is not stale, and dropping it here broke the class's own
            // never-dropped contract silently, with no requeue.
            //
            // An engine stall was stepped on exactly this way by the
            // route-start merge cue (owner playtest, 2026-08-19). It was
            // latent until that cue stopped being AMBIENT -- as chatter it
            // was dropped by would_start_stale before ever reaching here.
            //
            // An AGED backlog of ROUTE announcements really does describe
            // road already driven and is right to go -- rescuing those
            // turned one flush into a recital of everything it had just
            // purged. A safety call is the standing exception: it is the one
            // line whose worth does not decay while it waits.
            //
            // But "aged" was read off the INCOMING line only, and the two
            // come apart the moment the road speaks twice in one frame.
            // Measured on the owner's 23 August session: of 59 lines a flush
            // destroyed, 34 were cut inside `BASE_UTTERANCE_S` of their own
            // start -- the pause before the voice gets going -- so by this
            // pacer's own duration model the player had not heard one
            // character of them. Three route lines arriving inside 37 ms
            // took the ramp-exit briefing with them: "Off the ramp and onto
            // city streets: start on unnamed public road. Then turn right
            // now onto Halleck Street" was purged 22 ms in, and that turn
            // was never spoken. A line that young is not a backlog; it is
            // this same instant of road, and destroying it costs the whole
            // line rather than its tail.
            //
            // So the rescue asks how much of the outgoing line the player
            // actually got. Past the pre-utterance window it has said
            // something and its tail is expendable; inside it, nothing was
            // heard and the words are still current, so they are handed back
            // to be queued behind the line that cut them -- the same
            // contract a CRITICAL cut has always had. `RESCUE_ONCE_WINDOW_S`
            // still caps it at one hand-back per line, so a run of urgent
            // lines cannot replay it, and `take_protected` still drops a
            // safety call the player had mostly heard rather than restart
            // it (`MOSTLY_HEARD_FRACTION`).
            let held_rescuable = self.protected.as_ref().is_some_and(|held| {
                held.priority == EventPriority::Critical
                    || now - (held.done_at - Self::duration_s(&held.text)) < Self::BASE_UTTERANCE_S
            });
            if held_rescuable {
                self.flush_cut = self.take_protected(Some(text));
            } else {
                self.protected = None;
            }
            self.interrupt_deliveries();
            self.restart_projection(now, text, priority);
            self.track(text, priority, category, valid);
            return true;
        }
        self.extend_projection(now, text, priority);
        self.track(text, priority, category, valid);
        false
    }

    /// The channel was silenced outside the pacer's view (Ctrl, menus).
    pub fn reset(&mut self) {
        self.interrupt_deliveries();
        self.clear_at = 0.0;
        self.purge_next = false;
        self.protected = None;
    }

    // -- leaving and returning to the road ----------------------------------

    /// The player has stepped off the road (pause menu, a stop, settings).
    ///
    /// The caller silences the event channel; this drops the projection with
    /// it and arms the purge, so the first line spoken back on the road
    /// cannot arrive behind a backlog describing where the truck used to be.
    pub fn pause(&mut self) {
        self.interrupt_deliveries();
        self.clear_at = 0.0;
        self.purge_next = true;
        self.protected = None;
    }

    /// Back at the wheel. Nothing from before the pause is news.
    pub fn resume(&mut self) {
        self.interrupt_deliveries();
        self.clear_at = 0.0;
        self.purge_next = true;
        self.protected = None;
    }
}

#[cfg(test)]
mod tests;
