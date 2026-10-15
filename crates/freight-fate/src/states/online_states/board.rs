//! The live drivers board (the second half of
//! `freight_fate/states/online_states.py`, split out for length).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use ff_core::pyfmt::round_py_int;
use serde_json::Value;

use crate::app::GameContext;
use crate::impl_state_for_menu;
use crate::online_presence;
use crate::states::base::{Menu, MenuCore, MenuItem};

use super::profile::DriverProfileState;
use super::support::{menu_default_enter, online_transport, run_worker, wall_time, Mailbox};

/// A speakable freshness phrase from a server epoch-milliseconds stamp.
pub fn updated_text(updated_at_ms: f64) -> String {
    let age_s = (wall_time() - updated_at_ms / 1000.0).max(0.0);
    if age_s < 90.0 {
        return "updated just now".to_string();
    }
    let minutes = round_py_int(age_s / 60.0);
    format!("updated {minutes} minutes ago")
}

/// `str(value)` for the board's string fields, `""` for a missing one.
fn field_text(entry: &Value, key: &str) -> String {
    match entry.get(key) {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(s)) => s.clone(),
        Some(other) => online_presence::py_str(other),
    }
}

/// `float(entry.get("updatedAt", 0))`.
fn updated_at_ms(entry: &Value) -> f64 {
    match entry.get("updatedAt") {
        Some(Value::Number(n)) => n.as_f64().unwrap_or(0.0),
        Some(Value::String(s)) => s.trim().parse().unwrap_or(0.0),
        _ => 0.0,
    }
}

/// How long the open board waits before quietly asking again.
///
/// The site answers this from a sixty-second cache, so asking faster only
/// gets the same answer back and spends a request to do it.
const BOARD_POLL_S: f64 = 60.0;

/// The name a row is filed under, for ordering and for keeping the cursor on
/// the same person across a re-check.
fn row_driver_id(entry: &Value) -> String {
    field_text(entry, "driverId")
}

/// Sort a board by name, and keep it that way.
///
/// The site sends the drivers whose status moved most recently first, which
/// is the right hundred to send and the wrong order to read: sorted that way
/// the list reshuffles under a player every time a truck anywhere reports a
/// few more percent, and the position they memorised now belongs to somebody
/// else. Sorted by name, a re-check rewrites a line and moves nothing.
fn by_name(board: &mut [Value]) {
    board.sort_by(|a, b| {
        field_text(a, "displayName")
            .to_lowercase()
            .cmp(&field_text(b, "displayName").to_lowercase())
            .then_with(|| row_driver_id(a).cmp(&row_driver_id(b)))
    });
}

/// What a row on the board is, so the cursor can be put back on it after the
/// list is rebuilt under the player.
#[derive(Debug, Clone, PartialEq, Eq)]
enum RowKey {
    Driver(String),
    /// "Checking", "could not be reached", or "no drivers on duty".
    Status,
    Back,
}

// -- DriversOnlineState ----------------------------------------------------------------------

/// The live drivers board as a spoken list.
///
/// Public data, so it works with or without the player's own sharing set
/// up. The fetch happens on a daemon thread; until it lands the menu holds a
/// single "checking" line.
pub struct DriversOnlineState {
    pub menu: MenuCore<Self>,
    /// The board once fetched: `Some(None)` when orinks.net could not be
    /// reached, `Some(Some(rows))` otherwise; `None` until the fetch lands.
    pub board: Option<Option<Vec<Value>>>,
    result: Mailbox<Option<Vec<Value>>>,
    fetched: Arc<AtomicBool>,
    announced: bool,
    pub threaded: bool,
    /// Seconds before the open board asks again. A test seam beside
    /// `threaded`: the clock behind it is the real one, so a test that wants
    /// to see a re-check sets this to zero rather than waiting a minute.
    pub poll_after_s: f64,
    /// When the fetch in hand was started, so the board can ask again on its
    /// own while the player has it open.
    last_fetch_s: f64,
    /// True while the fetch running was started by the clock rather than by
    /// the player. A re-check nobody asked for must not speak.
    quiet_fetch: bool,
    /// What each row IS, in the order the rows are built: a driver's id, or
    /// one of the fixed rows. `refresh()` preserves the index and not the
    /// identity (see OnlineSetupState on why menus here are usually static),
    /// so this is what lets a re-check put the cursor back on the same thing
    /// rather than the same row number.
    ///
    /// Back is keyed too, not just the drivers, so the fixed row under the
    /// list is found by what it is rather than by a row number that moves
    /// every time the list shortens.
    row_keys: Vec<RowKey>,
    /// A driver the player is sitting on who has since gone off duty. Kept on
    /// the list, marked, until they move off it: taking the row away would
    /// slide somebody else silently under the cursor, and the next Enter
    /// would open a driver they never chose.
    held: Option<Value>,
}

impl DriversOnlineState {
    pub const TITLE: &'static str = "Drivers on duty";

    /// `DriversOnlineState(ctx)`.
    pub fn new(_ctx: &mut GameContext) -> Self {
        Self {
            menu: MenuCore::new(Self::TITLE),
            board: None,
            result: Mailbox::new(),
            fetched: Arc::new(AtomicBool::new(false)),
            announced: false,
            threaded: true,
            poll_after_s: BOARD_POLL_S,
            last_fetch_s: 0.0,
            quiet_fetch: false,
            row_keys: Vec::new(),
            held: None,
        }
    }

    /// Whether the fetch has answered (`self._fetched.is_set()`).
    pub fn fetched(&self) -> bool {
        self.fetched.load(Ordering::SeqCst)
    }

    fn start_fetch(&mut self) {
        self.board = None;
        self.quiet_fetch = false;
        self.begin_fetch();
    }

    /// Ask again without disturbing what is on screen.
    ///
    /// Unlike start_fetch this leaves the current board in place, so the list
    /// the player is reading stays whole and readable while the answer is in
    /// flight -- and if the answer never comes, they keep the drivers they
    /// had rather than watching the list fall back to "checking".
    fn start_quiet_fetch(&mut self) {
        self.quiet_fetch = true;
        self.begin_fetch();
    }

    fn begin_fetch(&mut self) {
        self.result = Mailbox::new();
        self.fetched = Arc::new(AtomicBool::new(false));
        self.announced = false;
        self.last_fetch_s = wall_time();
        let result = self.result.clone();
        let fetched = Arc::clone(&self.fetched);
        let transport = online_transport();
        run_worker(self.threaded, "online-board", move || {
            result.post(online_presence::fetch_board(transport.as_ref()));
            fetched.store(true, Ordering::SeqCst);
        });
    }

    /// What the cursor is on.
    fn selected_key(&self) -> Option<RowKey> {
        self.row_keys.get(self.menu.index).cloned()
    }

    /// The driver the cursor is on, if it is on one at all.
    fn selected_driver(&self) -> Option<String> {
        match self.selected_key() {
            Some(RowKey::Driver(id)) => Some(id),
            _ => None,
        }
    }

    /// Rebuild the list and put the cursor back on whatever it was on.
    ///
    /// By identity, never by row number: that is the whole point. Falls back
    /// to the old number only if the row is genuinely gone, which for the
    /// fixed rows cannot happen.
    fn rebuild_keeping_place(&mut self, ctx: &mut GameContext) {
        let wanted = self.selected_key();
        let previous = self.menu.index;
        let items = self.build_items(ctx);
        self.menu.items = items;
        self.menu.index = wanted
            .and_then(|key| self.row_keys.iter().position(|row| *row == key))
            .unwrap_or(previous);
        let last = self.menu.items.len().saturating_sub(1);
        self.menu.index = self.menu.index.min(last);
    }

    /// The row for a driver in the board currently in hand.
    fn board_row(&self, driver_id: &str) -> Option<Value> {
        self.board
            .as_ref()
            .and_then(|b| b.as_ref())
            .and_then(|rows| {
                rows.iter()
                    .find(|row| row_driver_id(row) == driver_id)
                    .cloned()
            })
    }

    /// Move a landed fetch out of the mailbox (the worker sets `_board`
    /// before `_fetched`; here the board travels in the mailbox).
    fn absorb(&mut self) {
        if !self.fetched() {
            return;
        }
        let Some(board) = self.result.take() else {
            return;
        };

        // A re-check nobody asked for that could not reach the site leaves
        // the drivers already on screen alone. The player is mid-read; an
        // unreachable answer to a question they never asked is not worth
        // emptying the list for.
        if self.quiet_fetch && board.is_none() {
            return;
        }

        let selected = self.selected_driver();
        let was_showing = selected.as_ref().and_then(|id| self.board_row(id));
        self.board = Some(board);

        // The driver under the cursor has gone off duty. Hold their row until
        // the player moves off it.
        if let (Some(id), Some(row)) = (selected, was_showing) {
            if self.board_row(&id).is_none() {
                self.held = Some(row);
            }
        }
    }

    /// Enter on a driver: open their public profile.
    ///
    /// Seeded with the row under the cursor, so the name is on the new
    /// screen before the site answers. A held row opens too: the profile is
    /// durable, and the driver going off duty is one of the things it says.
    fn open_selected_profile(&mut self, ctx: &mut GameContext) {
        let Some(driver_id) = self.selected_driver() else {
            self.speak_current(ctx);
            return;
        };
        let seed = self
            .rows_to_show()
            .and_then(|rows| rows.into_iter().find(|row| row_driver_id(row) == driver_id));
        let mut profile = DriverProfileState::new(ctx, &driver_id, seed, false);
        profile.threaded = self.threaded;
        ctx.push_state(profile);
    }

    /// Let go of a held row once the player has moved off it.
    fn release_held(&mut self, ctx: &mut GameContext) {
        let Some(held_id) = self.held.as_ref().map(row_driver_id) else {
            return;
        };
        if self.selected_driver().as_deref() != Some(held_id.as_str()) {
            self.held = None;
            self.rebuild_keeping_place(ctx);
        }
    }

    /// The drivers to show: what the site last said, plus a held row for
    /// anyone the player is still sitting on.
    fn rows_to_show(&self) -> Option<Vec<Value>> {
        let mut rows = self.board.as_ref().and_then(|b| b.as_ref()).cloned()?;
        if let Some(held) = self.held.as_ref() {
            let held_id = row_driver_id(held);
            if !rows.iter().any(|row| row_driver_id(row) == held_id) {
                rows.push(held.clone());
            }
        }
        by_name(&mut rows);
        Some(rows)
    }
}

impl Menu for DriversOnlineState {
    fn menu(&self) -> &MenuCore<Self> {
        &self.menu
    }

    fn menu_mut(&mut self) -> &mut MenuCore<Self> {
        &mut self.menu
    }

    fn enter(&mut self, ctx: &mut GameContext) {
        self.start_fetch();
        menu_default_enter(self, ctx);
    }

    fn build_items(&mut self, _ctx: &mut GameContext) -> Vec<MenuItem<Self>> {
        self.absorb();
        self.row_keys.clear();
        if !self.fetched() && self.board.is_none() {
            self.row_keys = vec![RowKey::Status, RowKey::Back];
            return vec![
                MenuItem::new("Checking the drivers list", |s: &mut Self, ctx| {
                    s.speak_current(ctx)
                }),
                MenuItem::new("Back", |s: &mut Self, ctx| s.go_back(ctx)),
            ];
        }
        let mut items: Vec<MenuItem<Self>> = Vec::new();
        let rows = self.rows_to_show();
        let held_id = self.held.as_ref().map(row_driver_id);
        match rows {
            None => {
                items.push(
                    MenuItem::new(
                        "The drivers list could not be reached",
                        |s: &mut Self, ctx| s.speak_current(ctx),
                    )
                    .help("orinks.net did not answer. The list tries again by itself about once a minute."),
                );
                self.row_keys.push(RowKey::Status);
            }
            Some(rows) if rows.is_empty() => {
                items.push(MenuItem::new(
                    "No drivers are on duty right now",
                    |s: &mut Self, ctx| s.speak_current(ctx),
                ));
                self.row_keys.push(RowKey::Status);
            }
            Some(rows) => {
                for entry in &rows {
                    let name = match entry.get("displayName") {
                        None => "A driver".to_string(),
                        Some(_) => field_text(entry, "displayName"),
                    };
                    let driver_id = row_driver_id(entry);
                    let gone = held_id.as_deref() == Some(driver_id.as_str());
                    let mut bits = vec![name, field_text(entry, "activity")];
                    let detail = field_text(entry, "detail");
                    if !detail.is_empty() {
                        bits.push(detail);
                    }
                    // A driver kept on the list only because the player is
                    // standing on them says so, rather than reading as though
                    // the truck were still rolling.
                    bits.push(if gone {
                        "went off duty".to_string()
                    } else {
                        updated_text(updated_at_ms(entry))
                    });
                    let label = bits
                        .into_iter()
                        .filter(|bit| !bit.is_empty())
                        .collect::<Vec<_>>()
                        .join(". ");
                    items.push(
                        MenuItem::new(label, |s: &mut Self, ctx| s.open_selected_profile(ctx))
                            .help("Opens this driver's public profile."),
                    );
                    self.row_keys.push(RowKey::Driver(driver_id));
                }
            }
        }
        // No Refresh row: the list checks by itself about once a minute
        // while it is open, the way the site's copy does, and a re-check
        // sooner than that gets the same cached answer back. Back is keyed
        // like the drivers are, so a player parked on it while the list
        // shortens above them stays on it.
        items.push(MenuItem::new("Back", |s: &mut Self, ctx| s.go_back(ctx)));
        self.row_keys.push(RowKey::Back);
        items
    }

    fn update(&mut self, ctx: &mut GameContext, dt: f64) {
        ctx.update_music_rotation(dt);
        self.release_held(ctx);

        // An answer in hand is dealt with before another is asked for. The
        // other order starves the list: a re-check would fall due again on
        // the very next frame and restart the fetch, so the answer that had
        // just landed would never reach the screen.
        if !self.fetched() {
            return;
        }
        if self.announced {
            // Nothing outstanding, so ask again on the clock -- that is what
            // lets a player who leaves the board open see drivers set off and
            // sign off without pressing anything.
            if wall_time() - self.last_fetch_s >= self.poll_after_s {
                self.start_quiet_fetch();
            }
            return;
        }
        self.announced = true;

        // A re-check nobody asked for changes the list and says nothing. The
        // player is reading; speaking over them to report that a truck
        // somewhere reported a few more percent is the whole reason the site
        // version of this list keeps quiet too. Their place is kept by
        // driver, not by row number.
        if self.quiet_fetch {
            // Cleared AFTER the rebuild, never before: absorb() runs inside
            // it and needs to know this answer was unasked-for, so that an
            // unreachable one leaves the drivers on screen alone.
            self.rebuild_keeping_place(ctx);
            self.quiet_fetch = false;
            return;
        }

        self.refresh(ctx, false);
        match self.board.as_ref().and_then(|b| b.as_ref()) {
            None => ctx.say("The drivers list could not be reached."),
            Some(board) if board.is_empty() => ctx.say("No drivers are on duty right now."),
            Some(board) => {
                let count = format!(
                    "{} driver{}",
                    board.len(),
                    if board.len() != 1 { "s are" } else { " is" }
                );
                let current = self.current_text(ctx);
                ctx.say(&format!("{count} on duty. {current}"));
            }
        }
    }
}

impl_state_for_menu!(DriversOnlineState);
