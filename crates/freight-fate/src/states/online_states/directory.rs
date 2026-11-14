//! The driver directory: every driver with a public profile, on duty or not,
//! and when each was last on duty.
//!
//! The drivers list answers "who is out right now"; this answers "who is
//! there at all, and when did I last miss them". The site sends the rows in
//! reading order -- drivers on duty first, then everyone else by how recently
//! they went off duty, then drivers it has never seen on duty, by name -- and
//! the list keeps that order: a directory is read top to bottom once, not
//! watched, so it never reshuffles under the player and never re-checks on
//! its own. Enter on a driver opens the same profile screen the drivers list
//! does.

use crate::states::base::default_enter;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use serde_json::Value;

use crate::app::GameContext;
use crate::impl_state_for_menu;
use crate::online_presence;
use crate::states::base::{Menu, MenuCore, MenuItem};

use super::profile::DriverProfileState;
use super::support::{online_transport, run_worker, wall_time, Mailbox};

/// "Last on duty 3 days ago", from a server epoch-milliseconds stamp.
pub fn last_on_duty_text(last_on_duty_ms: f64) -> String {
    last_on_duty_text_at(last_on_duty_ms, wall_time())
}

/// The same phrase against an explicit clock (seconds since the epoch).
///
/// Deliberately coarse, and the same bands the site uses: hours inside two
/// days, days inside two weeks, weeks inside two months, months beyond. The
/// directory says when a driver was last around, not when their game closed
/// to the minute -- a detail nobody needs about an offline player and one
/// they did not sign up to publish.
pub fn last_on_duty_text_at(last_on_duty_ms: f64, now_s: f64) -> String {
    let age_s = (now_s - last_on_duty_ms / 1000.0).max(0.0);
    let hour = 3600.0;
    let day = 24.0 * hour;
    if age_s < hour {
        return "Last on duty less than an hour ago".to_string();
    }
    let (count, unit, one) = if age_s < 2.0 * day {
        ((age_s / hour).floor() as u64, "hours", "an hour ago")
    } else if age_s < 14.0 * day {
        ((age_s / day).floor() as u64, "days", "yesterday")
    } else if age_s < 61.0 * day {
        ((age_s / (7.0 * day)).floor() as u64, "weeks", "last week")
    } else {
        (
            (age_s / (30.0 * day)).floor() as u64,
            "months",
            "last month",
        )
    };
    if count <= 1 {
        format!("Last on duty {one}")
    } else {
        format!("Last on duty {count} {unit} ago")
    }
}

/// A non-empty string field, or nothing.
fn text(entry: &Value, key: &str) -> Option<String> {
    match entry.get(key) {
        Some(Value::String(s)) if !s.trim().is_empty() => Some(s.trim().to_string()),
        _ => None,
    }
}

/// A numeric field, or nothing.
fn num(entry: &Value, key: &str) -> Option<f64> {
    match entry.get(key) {
        Some(Value::Number(n)) => n.as_f64(),
        Some(Value::String(s)) => s.trim().parse().ok(),
        _ => None,
    }
}

fn on_duty(entry: &Value) -> bool {
    entry
        .get("onDuty")
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

fn driver_id(entry: &Value) -> String {
    text(entry, "driverId").unwrap_or_default()
}

/// What one directory row says: the name, then on duty and what they are
/// doing, or when they were last on duty.
pub fn directory_row_text(entry: &Value) -> String {
    let name = text(entry, "displayName").unwrap_or_else(|| "A driver".to_string());
    if on_duty(entry) {
        let mut bits = vec![name, "On duty".to_string()];
        bits.extend(text(entry, "activity"));
        return bits.join(". ");
    }
    let when = match num(entry, "lastOnDutyAt") {
        Some(stamp) => last_on_duty_text(stamp),
        None => "Not seen on duty yet".to_string(),
    };
    format!("{name}. {when}")
}

// -- DriverDirectoryState ---------------------------------------------------------------------

/// The driver directory as a spoken list.
///
/// Public data, so it works with or without the player's own sharing set
/// up. The fetch happens on a daemon thread; until it lands the menu holds a
/// single "checking" line.
pub struct DriverDirectoryState {
    pub menu: MenuCore<Self>,
    /// The directory once fetched: `Some(None)` when orinks.net could not be
    /// reached, `Some(Some(rows))` otherwise; `None` until the fetch lands.
    pub directory: Option<Option<Vec<Value>>>,
    result: Mailbox<Option<Vec<Value>>>,
    fetched: Arc<AtomicBool>,
    announced: bool,
    pub threaded: bool,
    /// Whether the first fetch has been started. The directory is asked for
    /// once, when the screen first opens: coming back from a profile keeps
    /// the list and the cursor where they were, and Check again is the way
    /// to ask afresh.
    started: bool,
    /// The driver each row is, in row order; `None` for the fixed rows.
    row_ids: Vec<Option<String>>,
}

impl DriverDirectoryState {
    pub const TITLE: &'static str = "Driver directory";

    /// `DriverDirectoryState(ctx)`.
    pub fn new(_ctx: &mut GameContext) -> Self {
        Self {
            menu: MenuCore::new(Self::TITLE),
            directory: None,
            result: Mailbox::new(),
            fetched: Arc::new(AtomicBool::new(false)),
            announced: false,
            threaded: true,
            started: false,
            row_ids: Vec::new(),
        }
    }

    /// Whether the fetch has answered.
    pub fn fetched(&self) -> bool {
        self.fetched.load(Ordering::SeqCst)
    }

    fn start_fetch(&mut self) {
        self.started = true;
        self.directory = None;
        self.result = Mailbox::new();
        self.fetched = Arc::new(AtomicBool::new(false));
        self.announced = false;
        let result = self.result.clone();
        let fetched = Arc::clone(&self.fetched);
        let transport = online_transport();
        run_worker(self.threaded, "online-directory", move || {
            result.post(online_presence::fetch_directory(transport.as_ref()));
            fetched.store(true, Ordering::SeqCst);
        });
    }

    /// Move a landed fetch out of the mailbox.
    fn absorb(&mut self) {
        if !self.fetched() {
            return;
        }
        if let Some(directory) = self.result.take() {
            self.directory = Some(directory);
        }
    }

    fn rows(&self) -> Option<&Vec<Value>> {
        self.directory.as_ref().and_then(|d| d.as_ref())
    }

    /// The driver the cursor is on, if it is on one at all.
    fn selected_driver(&self) -> Option<String> {
        self.row_ids.get(self.menu.index).cloned().flatten()
    }

    /// Enter on a driver: open their public profile, seeded with the row so
    /// the name is on the new screen before the site answers.
    fn open_selected_profile(&mut self, ctx: &mut GameContext) {
        let Some(id) = self.selected_driver() else {
            self.speak_current(ctx);
            return;
        };
        let seed = self
            .rows()
            .and_then(|rows| rows.iter().find(|row| driver_id(row) == id).cloned());
        let mut profile = DriverProfileState::new(ctx, &id, seed, false);
        profile.threaded = self.threaded;
        ctx.push_state(profile);
    }

    /// Ask again, from the top. The directory never re-checks on its own,
    /// so this is the one way to see who has come on duty since it opened.
    fn check_again(&mut self, ctx: &mut GameContext) {
        self.start_fetch();
        self.menu.index = 0;
        self.refresh(ctx, false);
        ctx.say("Checking the driver directory.");
    }
}

impl Menu for DriverDirectoryState {
    fn menu(&self) -> &MenuCore<Self> {
        &self.menu
    }

    fn menu_mut(&mut self) -> &mut MenuCore<Self> {
        &mut self.menu
    }

    fn enter(&mut self, ctx: &mut GameContext) {
        if !self.started {
            self.start_fetch();
        }
        default_enter(self, ctx);
    }

    fn build_items(&mut self, _ctx: &mut GameContext) -> Vec<MenuItem<Self>> {
        self.absorb();
        self.row_ids.clear();
        if !self.fetched() && self.directory.is_none() {
            self.row_ids = vec![None, None];
            return vec![
                MenuItem::new("Checking the driver directory", |s: &mut Self, ctx| {
                    s.speak_current(ctx)
                }),
                MenuItem::new("Back", |s: &mut Self, ctx| s.go_back(ctx)),
            ];
        }
        let mut items: Vec<MenuItem<Self>> = Vec::new();
        match self.rows().cloned() {
            None => {
                items.push(
                    MenuItem::new(
                        "The driver directory could not be reached",
                        |s: &mut Self, ctx| s.speak_current(ctx),
                    )
                    .help("orinks.net did not answer. Check again asks once more."),
                );
                self.row_ids.push(None);
            }
            Some(rows) if rows.is_empty() => {
                items.push(MenuItem::new(
                    "No drivers have a public profile yet",
                    |s: &mut Self, ctx| s.speak_current(ctx),
                ));
                self.row_ids.push(None);
            }
            Some(rows) => {
                for entry in &rows {
                    items.push(
                        MenuItem::new(directory_row_text(entry), |s: &mut Self, ctx| {
                            s.open_selected_profile(ctx)
                        })
                        .help("Opens this driver's public profile."),
                    );
                    self.row_ids.push(Some(driver_id(entry)));
                }
            }
        }
        items.push(
            MenuItem::new("Check again", |s: &mut Self, ctx| s.check_again(ctx)).help(
                "Asks orinks.net for the directory again. The list does not check by itself.",
            ),
        );
        self.row_ids.push(None);
        items.push(MenuItem::new("Back", |s: &mut Self, ctx| s.go_back(ctx)));
        self.row_ids.push(None);
        items
    }

    fn update(&mut self, ctx: &mut GameContext, dt: f64) {
        ctx.update_music_rotation(dt);
        if !self.fetched() || self.announced {
            return;
        }
        self.announced = true;
        self.refresh(ctx, false);
        match self.rows() {
            None => ctx.say("The driver directory could not be reached."),
            Some(rows) if rows.is_empty() => ctx.say("No drivers have a public profile yet."),
            Some(rows) => {
                let total = rows.len();
                let on = rows.iter().filter(|row| on_duty(row)).count();
                let profiles = format!(
                    "{total} driver{} a public profile",
                    if total == 1 { " has" } else { "s have" }
                );
                let duty = match on {
                    0 => "none on duty".to_string(),
                    n => format!("{n} on duty"),
                };
                let current = self.current_text(ctx);
                ctx.say(&format!("{profiles}, {duty}. {current}"));
            }
        }
    }
}

impl_state_for_menu!(DriverDirectoryState);
