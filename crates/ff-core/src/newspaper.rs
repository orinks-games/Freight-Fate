//! The driver newspaper as the game holds it: one issue the site published,
//! already worded as spoken lines (`GET /api/freight-fate/news` on
//! orinks.net, body version 1), and the small state machine that says
//! whether the game has one.
//!
//! The game has no switch of its own for the paper. It holds an issue only
//! once the site has published one, and everything that mentions the paper
//! (the Driver apps row, the Online menu row) is present only while
//! [`PaperState::Holding`]. A failed fetch keeps whatever is held; the site
//! answering that no issue is published drops it.
//!
//! The issue's text comes from the site in English and is spoken as it
//! arrives. [`PaperIssue::spoken_text`] lists every line of it, so the
//! speech boundary can keep those lines out of the translation catalog
//! (`crate::l10n::Translator::keep_verbatim`).

use serde_json::Value;

/// The only body version this build reads.
pub const PAPER_VERSION: u64 = 1;
/// The longest line accepted, in characters.
pub const MAX_LINE_CHARS: usize = 600;
/// The most lines one issue may hold, headings not counted.
pub const MAX_ISSUE_LINES: usize = 400;
/// A held issue older than this is fetched again when the app opens: the
/// length of the site's cached read, so a driver who turns Profile sharing
/// off leaves the game's copy about as fast as the site's page, and an
/// opening sooner than that would only get the same answer.
pub const REFRESH_AFTER_S: f64 = 5.0 * 60.0;
/// Where the last issue is kept, under the data directory.
pub const PAPER_FILE: &str = "newspaper.json";

/// One section: a heading and its spoken lines, in page order.
#[derive(Debug, Clone, PartialEq)]
pub struct PaperSection {
    pub heading: String,
    pub lines: Vec<String>,
}

/// One published issue.
#[derive(Debug, Clone, PartialEq)]
pub struct PaperIssue {
    pub issue: u32,
    pub masthead: String,
    /// "November 16 to 22, 2026".
    pub dates: String,
    /// The open week so far, when the site printed it: its heading and one
    /// sentence.
    pub since_monday: Option<PaperSection>,
    pub sections: Vec<PaperSection>,
}

impl PaperIssue {
    /// "The Mile Marker, issue 25. November 16 to 22, 2026."
    pub fn title(&self) -> String {
        format!("{}, issue {}. {}.", self.masthead, self.issue, self.dates)
    }

    /// The sections as the app lists them: Since Monday first when present.
    pub fn reading_order(&self) -> Vec<&PaperSection> {
        self.since_monday
            .iter()
            .chain(self.sections.iter())
            .collect()
    }

    /// Every piece of the site's text the game may speak: the title, the
    /// masthead alone, each heading and each line.
    pub fn spoken_text(&self) -> Vec<String> {
        let mut out = vec![self.title(), self.masthead.clone()];
        for section in self.reading_order() {
            out.push(section.heading.clone());
            out.extend(section.lines.iter().cloned());
        }
        out
    }

    pub fn line_count(&self) -> usize {
        self.reading_order().iter().map(|s| s.lines.len()).sum()
    }
}

/// Why a body was refused. Logged, never spoken.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PaperRefusal {
    NotJson,
    Version,
    Shape(&'static str),
    LineTooLong,
    TooManyLines,
}

fn text(value: &Value, key: &'static str) -> Result<String, PaperRefusal> {
    let s = value
        .get(key)
        .and_then(Value::as_str)
        .ok_or(PaperRefusal::Shape(key))?;
    checked_line(s)
}

/// A line as the game will speak it: trimmed, not empty, not too long, no
/// control characters.
fn checked_line(s: &str) -> Result<String, PaperRefusal> {
    let s = s.trim();
    if s.is_empty() || s.chars().any(char::is_control) {
        return Err(PaperRefusal::Shape("line"));
    }
    if s.chars().count() > MAX_LINE_CHARS {
        return Err(PaperRefusal::LineTooLong);
    }
    Ok(s.to_string())
}

fn section(value: &Value) -> Result<PaperSection, PaperRefusal> {
    let heading = text(value, "heading")?;
    let lines = value
        .get("lines")
        .and_then(Value::as_array)
        .ok_or(PaperRefusal::Shape("lines"))?
        .iter()
        .map(|line| line.as_str().ok_or(PaperRefusal::Shape("line")))
        .map(|line| line.and_then(checked_line))
        .collect::<Result<Vec<_>, _>>()?;
    if lines.is_empty() {
        return Err(PaperRefusal::Shape("lines"));
    }
    Ok(PaperSection { heading, lines })
}

/// A version 1 body, or why it was refused. Refuses another version, a
/// missing or mistyped field, an empty section, any line over
/// [`MAX_LINE_CHARS`] and any issue over [`MAX_ISSUE_LINES`] lines.
pub fn parse_issue(body: &str) -> Result<PaperIssue, PaperRefusal> {
    let value: Value = serde_json::from_str(body).map_err(|_| PaperRefusal::NotJson)?;
    parse_issue_value(&value)
}

pub fn parse_issue_value(value: &Value) -> Result<PaperIssue, PaperRefusal> {
    if value.get("version").and_then(Value::as_u64) != Some(PAPER_VERSION) {
        return Err(PaperRefusal::Version);
    }
    let issue = value
        .get("issue")
        .and_then(Value::as_u64)
        .filter(|n| (1..=u64::from(u32::MAX)).contains(n))
        .ok_or(PaperRefusal::Shape("issue"))? as u32;
    let since_monday = match value.get("sinceMonday") {
        None | Some(Value::Null) => None,
        Some(brief) => Some(PaperSection {
            heading: text(brief, "heading")?,
            lines: vec![text(brief, "sentence")?],
        }),
    };
    let sections = value
        .get("sections")
        .and_then(Value::as_array)
        .ok_or(PaperRefusal::Shape("sections"))?
        .iter()
        .map(section)
        .collect::<Result<Vec<_>, _>>()?;
    if sections.is_empty() {
        return Err(PaperRefusal::Shape("sections"));
    }
    let paper = PaperIssue {
        issue,
        masthead: text(value, "masthead")?,
        dates: text(value, "dates")?,
        since_monday,
        sections,
    };
    if paper.line_count() > MAX_ISSUE_LINES {
        return Err(PaperRefusal::TooManyLines);
    }
    Ok(paper)
}

/// Whether the game holds an issue. The only place this moves is
/// [`PaperState::next`].
#[derive(Debug, Clone, PartialEq, Default)]
pub enum PaperState {
    #[default]
    NoIssue,
    /// `held_at`: seconds since the Unix epoch when the site last gave the
    /// game this issue.
    Holding { issue: PaperIssue, held_at: f64 },
}

/// What can happen to the held issue.
#[derive(Debug, Clone, PartialEq)]
pub enum PaperEvent {
    /// The saved copy read at launch, a fetch that answered an issue, or a
    /// test scenario staging one.
    Received { issue: PaperIssue, at: f64 },
    /// The site answered that no issue is published (the paper is off).
    Withdrawn,
    /// The site could not be reached, or answered with a body this build
    /// refuses. Keeps whatever is held.
    FetchFailed,
}

impl PaperState {
    pub fn next(self, event: PaperEvent) -> PaperState {
        match (self, event) {
            (_, PaperEvent::Received { issue, at }) => PaperState::Holding { issue, held_at: at },
            (_, PaperEvent::Withdrawn) => PaperState::NoIssue,
            (state, PaperEvent::FetchFailed) => state,
        }
    }

    pub fn issue(&self) -> Option<&PaperIssue> {
        match self {
            PaperState::NoIssue => None,
            PaperState::Holding { issue, .. } => Some(issue),
        }
    }

    /// Whether opening the app should ask the site again: nothing held, or
    /// the held issue is older than [`REFRESH_AFTER_S`].
    pub fn refresh_due(&self, now: f64) -> bool {
        match self {
            PaperState::NoIssue => true,
            PaperState::Holding { held_at, .. } => now - held_at > REFRESH_AFTER_S,
        }
    }

    /// For the agent server's `status`: "no_issue" or "holding issue 25".
    pub fn status(&self) -> String {
        match self {
            PaperState::NoIssue => "no_issue".to_string(),
            PaperState::Holding { issue, .. } => format!("holding issue {}", issue.issue),
        }
    }
}

#[cfg(test)]
mod tests;
