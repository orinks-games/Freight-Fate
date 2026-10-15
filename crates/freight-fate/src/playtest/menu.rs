//! Reading and driving a menu that the caller does not know the type of.
//!
//! # Why this is not `state.items`
//!
//! The Python harness reached straight for `getattr(app.state, "items", [])`
//! and compared `item.text` to a label. Rust's [`MenuItem`] is generic in the
//! screen that owns it (`MenuItem<S>`), so [`Menu`] is not object safe and
//! there is no `&dyn Menu` to ask. The typed way -- `with_state::<T>` and a
//! turbofish -- is what the per-screen tests use, and it is right there,
//! but the harness walks screens it deliberately does not name: the pickup
//! menu hands over to a shipper it did not choose, the arrival menu offers
//! whichever kind of receiver dispatch drew.
//!
//! So the harness reads the rows the same way a sighted helper does, off
//! `State::lines`. Every menu renders its rows last, one per line, each
//! prefixed `"> "` (focused) or `"  "`, after a blank separator line -- the
//! `Menu::lines` default and every screen that overrides it (the pickup
//! facility's status block, the settings screen) both. That is a contract
//! the visible window already depends on, and a screen that broke it would
//! be unreadable on screen before it was unreadable here.
//!
//! [`MenuItem`]: crate::states::base::MenuItem
//! [`Menu`]: crate::states::base::Menu

use crate::app::GameContext;
use crate::states::base::State;

/// The rows of `state`'s menu and which one has focus, or `None` when this
/// screen is not a menu (the driving state's status readout, a timed
/// message).
pub fn menu_rows(state: &dyn State, ctx: &GameContext) -> Option<(Vec<String>, usize)> {
    parse_rows(&state.lines(ctx))
}

/// [`menu_rows`] on lines already rendered; the visual display reads every
/// screen's menu this way too.
pub fn parse_rows(lines: &[String]) -> Option<(Vec<String>, usize)> {
    // The rows are everything after the last blank separator.
    let start = lines.iter().rposition(|line| line.is_empty())? + 1;
    let rows = &lines[start..];
    if rows.is_empty() {
        return None;
    }
    let mut focus = None;
    let mut labels = Vec::with_capacity(rows.len());
    for (i, row) in rows.iter().enumerate() {
        let text = match row.get(..2) {
            Some("> ") => {
                if focus.is_some() {
                    return None; // two cursors: not a menu render
                }
                focus = Some(i);
                &row[2..]
            }
            Some("  ") => &row[2..],
            _ => return None,
        };
        labels.push(text.to_string());
    }
    Some((labels, focus?))
}

/// Every option on `state`, or an empty list when it is not a menu (the
/// Python `getattr(self.app.state, "items", [])`).
pub fn menu_labels_of(state: &dyn State, ctx: &GameContext) -> Vec<String> {
    menu_rows(state, ctx)
        .map(|(labels, _)| labels)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::testing::TestApp;
    use crate::states::base::{MenuItem, SimpleMenuState, TimedMessageState};

    #[test]
    fn a_menu_reads_back_its_rows_and_its_cursor() {
        let mut app = TestApp::new();
        app.push_state(SimpleMenuState::new(
            "Pickup facility",
            vec![
                MenuItem::inert("Check in at shipping office"),
                MenuItem::inert("Depart for destination"),
            ],
        ));
        let state = app.state().expect("a state");
        let state = state.borrow();
        let (rows, focus) = menu_rows(&*state, &app.ctx).expect("a menu renders rows");
        assert_eq!(
            rows,
            vec![
                "Check in at shipping office".to_string(),
                "Depart for destination".to_string()
            ]
        );
        assert_eq!(focus, 0);
    }

    #[test]
    fn a_screen_that_is_not_a_menu_has_no_rows() {
        let app = TestApp::new();
        let timed = TimedMessageState::new(
            "Pulling into destination",
            "Brakes set.",
            "Please wait.",
            1.0,
            |_| {},
        );
        assert!(menu_rows(&timed, &app.ctx).is_none());
        assert!(menu_labels_of(&timed, &app.ctx).is_empty());
    }
}
