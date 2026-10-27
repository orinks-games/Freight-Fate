//! Spoken in-cab Record of Duty Status screens (port of
//! `freight_fate/states/logbook.py`).
//!
//! The Python functions took the optional `DrivingState` to read
//! `driving.trip.game_minutes`; here the caller passes that number
//! (`trip_game_minutes`, `None` when not driving), so the logbook does not
//! depend on the driving state's type.
//!
//! The lines, in order: what you are doing now and since when; the hours
//! limits, one per line; today's totals; then the entries, newest first,
//! each led by what you were doing. It used to open with the status twice
//! (once bare, once inside the hours sentence), carry a heading row that
//! did nothing when pressed, and lead every entry with its clock span, so a
//! driver arrowing down heard "8 AM to 10 AM" before hearing what that was
//! (owner, 2026-09-14).

use ff_core::sim::hos::{clock_text, duration_text, duty_status_label};

use crate::app::GameContext;
use crate::impl_state_for_menu;
use crate::states::base::{Menu, MenuCore, MenuItem};

/// How many entries the screen reads back.
pub const LOGBOOK_ENTRIES: usize = 8;

/// The logbook as spoken lines.
pub fn logbook_lines(ctx: &GameContext, trip_game_minutes: Option<f64>) -> Vec<String> {
    let Some(p) = ctx.profile.as_ref() else {
        return Vec::new();
    };
    let now = current_hour(p.game_hours, trip_game_minutes);
    let log = &p.duty_log;
    let day_start = (now / 24.0).floor() * 24.0;
    let totals = log.totals_since(day_start, now);
    let mut lines = Vec::new();
    match log.recent(1).last() {
        Some(open) => {
            let mut status = sentence_start(&duty_status_label(&open.status));
            status.push_str(&format!(" since {}", clock_text(open.start_hour)));
            if !open.location.is_empty() {
                status.push_str(&format!(" at {}", open.location));
            }
            status.push('.');
            lines.push(status);
        }
        None => lines.push("Off duty. No logbook entries yet.".to_string()),
    }
    lines.extend(p.hos.summary_lines(&ctx.settings.hos_mode));
    lines.push(format!(
        "Today: driving {}, on duty not driving {}, off duty {}, sleeper berth {}.",
        duration_text(totals.driving),
        duration_text(totals.on_duty_not_driving),
        duration_text(totals.off_duty),
        duration_text(totals.sleeper_berth),
    ));
    for segment in log.recent(LOGBOOK_ENTRIES).iter().rev() {
        lines.push(entry_line(segment));
    }
    lines
}

/// One entry: what, when, how long, where, and the note if any.
fn entry_line(segment: &ff_core::sim::hos::DutySegment) -> String {
    let note = if segment.note.is_empty() {
        String::new()
    } else {
        format!(", {}", segment.note)
    };
    format!(
        "{}, {} to {}, {}, {}{note}.",
        sentence_start(&duty_status_label(&segment.status)),
        clock_text(segment.start_hour),
        clock_text(segment.end_hour),
        duration_text(segment.duration_hours()),
        segment.location,
    )
}

/// A reviewable spoken Record of Duty Status.
pub struct LogbookState {
    menu: MenuCore<Self>,
    /// `driving.trip.game_minutes` when opened from a drive.
    pub trip_game_minutes: Option<f64>,
}

impl LogbookState {
    /// `LogbookState(ctx, driving=None)`.
    pub fn new(trip_game_minutes: Option<f64>) -> Self {
        Self {
            menu: MenuCore::new("Logbook").with_intro_help(
                "Use up and down arrows to review logbook lines. Enter repeats the \
                 current line. Escape goes back.",
            ),
            trip_game_minutes,
        }
    }
}

impl Default for LogbookState {
    fn default() -> Self {
        Self::new(None)
    }
}

impl Menu for LogbookState {
    fn menu(&self) -> &MenuCore<Self> {
        &self.menu
    }

    fn menu_mut(&mut self) -> &mut MenuCore<Self> {
        &mut self.menu
    }

    fn announce_entry(&mut self, ctx: &mut GameContext) {
        let text = format!("{}. {}", self.menu.title, self.current_text(ctx));
        ctx.say(&text);
    }

    fn build_items(&mut self, ctx: &mut GameContext) -> Vec<MenuItem<Self>> {
        let mut items: Vec<MenuItem<Self>> = logbook_lines(ctx, self.trip_game_minutes)
            .into_iter()
            .map(|line| {
                let spoken = line.clone();
                MenuItem::new(line, move |_s: &mut Self, ctx| ctx.say(&spoken))
                    .help("Repeat this logbook line.")
            })
            .collect();
        items.push(
            MenuItem::new("Back", |s: &mut Self, ctx| s.go_back(ctx))
                .help("Return to the previous menu."),
        );
        items
    }
}

impl_state_for_menu!(LogbookState);

fn current_hour(game_hours: f64, trip_game_minutes: Option<f64>) -> f64 {
    match trip_game_minutes {
        None => game_hours,
        Some(minutes) => game_hours + minutes / 60.0,
    }
}

/// "driving" as the start of a line: "Driving".
fn sentence_start(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}
