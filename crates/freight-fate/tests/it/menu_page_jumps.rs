//! Page Up and Page Down in a menu (`Menu::page_by`): ten rows at a time,
//! stopping at the ends without wrapping, unless the menu pages for itself
//! (`Menu::handle_extra_key`). On a touch screen the three-finger flicks
//! left and right press the same keys.

use freight_fate::app::{testing::TestApp, GameContext};
use freight_fate::states::base::{InputEvent, Key, Menu, MenuCore, MenuItem};
use freight_fate::touch::Gesture;

struct Rows {
    menu: MenuCore<Self>,
    count: usize,
    /// Takes Page Up and Page Down itself, the way a paged search does.
    own_pages: bool,
    pages_turned: i32,
}

impl Rows {
    fn new(count: usize) -> Self {
        Rows {
            menu: MenuCore::new("Rows"),
            count,
            own_pages: false,
            pages_turned: 0,
        }
    }
}

impl Menu for Rows {
    fn menu(&self) -> &MenuCore<Self> {
        &self.menu
    }
    fn menu_mut(&mut self) -> &mut MenuCore<Self> {
        &mut self.menu
    }
    fn build_items(&mut self, _: &mut GameContext) -> Vec<MenuItem<Self>> {
        (1..=self.count)
            .map(|n| MenuItem::inert(format!("Row {n}")))
            .collect()
    }
    fn handle_extra_key(&mut self, _: &mut GameContext, key: Key) -> bool {
        if !self.own_pages {
            return false;
        }
        match key {
            Key::PageDown => self.pages_turned += 1,
            Key::PageUp => self.pages_turned -= 1,
            _ => return false,
        }
        true
    }
}

freight_fate::impl_state_for_menu!(Rows);

fn press(app: &mut TestApp, rows: &mut Rows, key: Key) {
    rows.handle_event(&mut app.ctx, &InputEvent::key(key));
}

fn last_line(app: &TestApp) -> String {
    app.main_lines().last().cloned().unwrap_or_default()
}

#[test]
fn page_down_and_up_move_ten_rows_and_stop_at_the_ends() {
    let mut app = TestApp::new();
    let mut rows = Rows::new(34);
    rows.refresh(&mut app.ctx, false);

    press(&mut app, &mut rows, Key::PageDown);
    assert_eq!(rows.menu.index, 10);
    assert_eq!(last_line(&app), "Row 11. 11 of 34.");
    press(&mut app, &mut rows, Key::PageDown);
    press(&mut app, &mut rows, Key::PageDown);
    assert_eq!(rows.menu.index, 30);
    // Four rows from the end, the jump stops on the last row.
    press(&mut app, &mut rows, Key::PageDown);
    assert_eq!(rows.menu.index, 33);
    assert_eq!(last_line(&app), "Row 34. 34 of 34.");
    // At the end it reads the row again and does not wrap.
    app.clear_speech();
    press(&mut app, &mut rows, Key::PageDown);
    assert_eq!(rows.menu.index, 33);
    assert_eq!(last_line(&app), "Row 34. 34 of 34.");

    // From the middle, back ten.
    rows.menu.index = 15;
    press(&mut app, &mut rows, Key::PageUp);
    assert_eq!(rows.menu.index, 5);
    press(&mut app, &mut rows, Key::PageUp);
    assert_eq!(rows.menu.index, 0);
    app.clear_speech();
    press(&mut app, &mut rows, Key::PageUp);
    assert_eq!(rows.menu.index, 0);
    assert_eq!(last_line(&app), "Row 1. 1 of 34.");
    app.shutdown();
}

#[test]
fn a_held_page_key_at_an_end_says_nothing_more() {
    let mut app = TestApp::new();
    let mut rows = Rows::new(12);
    rows.refresh(&mut app.ctx, false);
    let mut held = InputEvent::key(Key::PageUp);
    if let InputEvent::KeyDown { repeat, .. } = &mut held {
        *repeat = true;
    }
    app.clear_speech();
    rows.handle_event(&mut app.ctx, &held);
    assert!(app.main_lines().is_empty(), "{:?}", app.main_lines());
    app.shutdown();
}

#[test]
fn a_short_menu_jumps_straight_to_its_ends() {
    let mut app = TestApp::new();
    let mut rows = Rows::new(3);
    rows.refresh(&mut app.ctx, false);
    press(&mut app, &mut rows, Key::PageDown);
    assert_eq!(rows.menu.index, 2);
    press(&mut app, &mut rows, Key::PageUp);
    assert_eq!(rows.menu.index, 0);
    app.shutdown();
}

#[test]
fn a_menu_that_pages_for_itself_keeps_its_page_keys() {
    let mut app = TestApp::new();
    let mut rows = Rows::new(34);
    rows.own_pages = true;
    rows.refresh(&mut app.ctx, false);
    press(&mut app, &mut rows, Key::PageDown);
    press(&mut app, &mut rows, Key::PageDown);
    press(&mut app, &mut rows, Key::PageUp);
    assert_eq!(rows.pages_turned, 1);
    assert_eq!(rows.menu.index, 0);
    app.shutdown();
}

#[test]
fn three_finger_flicks_left_and_right_jump_on_a_touch_screen() {
    let mut app = TestApp::new();
    app.dispatch_gesture(Gesture::KeyboardDisconnected);
    let mut rows = Rows::new(34);
    rows.refresh(&mut app.ctx, false);
    app.push_state(rows);
    app.dispatch_gesture(Gesture::ThreeFingerSwipeRight);
    // The touch screen's Back row makes it 35.
    assert_eq!(last_line(&app), "Row 11. 11 of 35.");
    app.dispatch_gesture(Gesture::ThreeFingerSwipeLeft);
    assert_eq!(last_line(&app), "Row 1. 1 of 35.");
    app.shutdown();
}
