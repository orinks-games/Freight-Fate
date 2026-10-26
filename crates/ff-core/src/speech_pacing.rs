//! The driving speech ladder: what each line is about, and what each rung
//! of the driving speech setting does with it.
//!
//! The event voice's pacer -- repeat suppression, the backlog projection,
//! the priority wait budgets and the hand-back of a cut line -- is
//! Hearken's `hearken::speech::EventSpeechPacer`, run with Freight Fate's
//! tuning (`freight_fate::app::FF_PACER`). What stays here is game content
//! the headless crates need too: the [`SpeechCategory`] of a line, the
//! [`Disposition`] a rung gives it, the rung table itself, and the
//! monotonic clock the road-event and message-log timers read.
//!
//! Port of `freight_fate/speech_pacing.py`.

use std::sync::OnceLock;
use std::time::Instant;

/// Seconds since the first call: the default pacer clock, the equivalent of
/// Python's `time.monotonic`.
pub fn monotonic_seconds() -> f64 {
    static START: OnceLock<Instant> = OnceLock::new();
    START.get_or_init(Instant::now).elapsed().as_secs_f64()
}

/// What a line of informational speech is ABOUT.
///
/// Orthogonal to the pacer's priority, which says how long a line waits
/// and whether staleness may drop it. Urgency alone gave the verbosity system
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

#[cfg(test)]
mod tests {
    mod settings;
}
