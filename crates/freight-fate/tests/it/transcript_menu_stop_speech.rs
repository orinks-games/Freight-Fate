//! Left or Right Control silences speech in menus and the help reader
//! (port of `tests/test_menu_stop_speech.py`).
//!
//! The driving screen already stops its event voice with Control; menus and
//! the how-to-play reader speak on the main channel, so they need the same
//! key to quiet a long readout (job details, cargo loading, a whole help
//! page).
//!
//! Python counted calls to a `ctx.stop_speech` stand-in. Here the real
//! `GameContext::stop_speech` runs and the count is read off the
//! `CaptureSpeech` underneath it, which also proves the two channels are
//! both silenced -- what the Python file asserted separately.

use freight_fate::app::testing::TestApp;
use freight_fate::states::base::{Key, Menu, MenuItem, SimpleMenuState, State, DEFAULT_INTRO_HELP};
use freight_fate::states::main_menu_help::HelpState;

/// `_menu_with_stop`: the two-row throwaway screen, items already built.
fn menu(app: &mut TestApp) -> SimpleMenuState {
    app.ctx.settings.announce_menu_position = true;
    let mut state = SimpleMenuState::new(
        "Menu",
        vec![
            MenuItem::new("One", |_, _| {}),
            MenuItem::new("Two", |_, _| {}),
        ],
    );
    state.refresh(&mut app.ctx, false);
    state
}

#[test]
fn test_left_and_right_control_stop_speech_in_menus() {
    let mut app = TestApp::new();
    let mut m = menu(&mut app);
    app.clear_speech();
    Menu::handle_event(
        &mut m,
        &mut app.ctx,
        &freight_fate::states::base::InputEvent::key(Key::LCtrl),
    );
    Menu::handle_event(
        &mut m,
        &mut app.ctx,
        &freight_fate::states::base::InputEvent::key(Key::RCtrl),
    );
    assert_eq!(app.speech().stop_main_calls(), 2);
    assert_eq!(app.speech().stop_event_calls(), 2);
}

#[test]
fn test_control_stops_speech_in_the_help_reader() {
    let mut app = TestApp::new();
    let mut state = HelpState::at_page(0);
    app.clear_speech();
    state.handle_event(
        &mut app.ctx,
        &freight_fate::states::base::InputEvent::key(Key::LCtrl),
    );
    assert_eq!(app.speech().stop_main_calls(), 1);
    assert_eq!(app.speech().stop_event_calls(), 1);
}

#[test]
fn test_help_reader_announces_controller_controls_and_stops_speech() {
    use ff_core::input_hints::CONTROLLER;
    let mut app = TestApp::new();
    app.ctx.controller.set_enabled(true);
    app.ctx.controller.bind_device(
        Box::new(freight_fate::controller::fakes::FakePad::new(0)),
        "test pad",
    );
    app.ctx.controller.set_id_pending(false);
    app.ctx.controller.active_device = CONTROLLER;
    let mut state = HelpState::new();
    state.enter(&mut app.ctx);
    assert!(app.main_lines().join(" ").contains("Back stops speech"));
    state.handle_event(
        &mut app.ctx,
        &freight_fate::states::base::InputEvent::key(Key::F1),
    );
    assert_eq!(app.speech().stop_main_calls(), 1);
    assert_eq!(app.speech().stop_event_calls(), 1);
}

#[test]
fn test_stop_speech_silences_both_channels() {
    // Python reached into a bare `GameContext` with a two-method speech
    // stub; the headless app already has one, and `stop_speech` also resets
    // the event pacer on the way through.
    let mut app = TestApp::new();
    app.clear_speech();
    app.ctx.stop_speech();
    assert_eq!(app.speech().stop_main_calls(), 1);
    assert_eq!(app.speech().stop_event_calls(), 1);
}

#[test]
fn test_menu_intro_help_documents_the_stop_key() {
    assert!(DEFAULT_INTRO_HELP.contains("Control stops the current speech"));
}
