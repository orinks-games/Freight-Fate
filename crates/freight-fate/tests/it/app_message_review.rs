//! Port of `tests/test_message_review.py`: regression coverage for the
//! speech-review controls.
//!
//! These drive `App::dispatch_to_state`, the one place key events reach a
//! state, so they cover the wiring a player actually presses rather than the
//! log in isolation -- the message-log tests cover the log itself.
//!
//! The Python tests pushed `MainMenuState`; until `states::main_menu` is
//! ported the menu here is a `SimpleMenuState`, which exercises the same
//! review path (a menu on top, not the driving state).

use crate::states_driving_menus_support as drive_support;
use ff_core::message_log::MessageCategory;
use ff_core::sim::trip_models::{TripEvent, TripEventData, TripEventKind};
use ff_core::speech_text::SpokenMessage;
use freight_fate::app::testing::TestApp;
use freight_fate::app::GameContext;
use freight_fate::states::base::{InputEvent, Key, MenuItem, Mods, SimpleMenuState, State};
use freight_fate::states::driving_core::{profile_mut_of, HAZARD_SAFE_MPH};
use freight_fate::states::text_entry::TextEntryState;

fn key_event(key: Key) -> InputEvent {
    InputEvent::key(key)
}

fn ctrl_key_event(key: Key) -> InputEvent {
    InputEvent::key_mods(key, Mods::CTRL)
}

fn menu() -> SimpleMenuState {
    SimpleMenuState::new(
        "Main menu",
        vec![
            MenuItem::new("New career", |_, _| {}),
            MenuItem::new("Continue career", |_, _| {}),
            MenuItem::new("Settings", |_, _| {}),
        ],
    )
}

/// The last thing said on the main channel.
fn last(app: &TestApp) -> String {
    app.main_lines().last().cloned().unwrap_or_default()
}

/// A bare state speaks nothing on entry, so the log holds only what a test
/// puts in it.
struct BareState;
impl State for BareState {}

#[test]
fn test_hazard_warning_and_outcome_replay_on_a_comma_and_period() {
    let mut app = TestApp::new();
    let drive = drive_support::a_drive(&mut app);
    let warning = "Brake now! A slow vehicle ahead.";
    let outcome = "Clear.";
    let mut event = TripEvent {
        kind: TripEventKind::Hazard,
        message: SpokenMessage::new(warning),
        data: TripEventData::default(),
    };
    event.data.deadline_s = Some(3.0);
    drive_support::with_drive(&drive, |d| d.handle_trip_event(&mut app.ctx, &event));

    app.dispatch_to_state(&key_event(Key::A));
    assert_eq!(last(&app), warning);
    app.dispatch_to_state(&key_event(Key::Comma));
    assert_eq!(last(&app), warning);

    // Python silenced `ctx.award_achievement`. There is no seam here, so the
    // badge is arranged to have been earned already: dodging a hazard awards
    // it, and its line would otherwise land after the outcome this replays.
    profile_mut_of(&mut app.ctx)
        .achievements
        .push("hazard_avoided".to_string());
    drive_support::with_drive(&drive, |d| {
        d.trip.truck.velocity_mps = (HAZARD_SAFE_MPH - 1.0) / 2.2369362920544;
        d.update_hazard(&mut app.ctx, 1.0 / 60.0);
    });

    app.dispatch_to_state(&key_event(Key::A));
    assert_eq!(last(&app), outcome);
    app.dispatch_to_state(&key_event(Key::Comma));
    assert_eq!(last(&app), outcome);
    app.dispatch_to_state(&key_event(Key::Comma));
    assert_eq!(last(&app), warning);
    app.dispatch_to_state(&key_event(Key::Period));
    assert_eq!(last(&app), outcome);
}

#[test]
fn test_collision_outcome_replays_on_a_and_message_review() {
    let mut app = TestApp::new();
    let drive = drive_support::a_drive(&mut app);
    let warning = "Brake now! Debris on the road.";
    let mut event = TripEvent {
        kind: TripEventKind::Hazard,
        message: SpokenMessage::new(warning),
        data: TripEventData::default(),
    };
    event.data.deadline_s = Some(0.0);
    let outcome = drive_support::with_drive(&drive, |d| {
        d.handle_trip_event(&mut app.ctx, &event);
        d.trip.truck.velocity_mps = 40.0 / 2.2369362920544;
        d.hazard_deadline = Some(0.0);
        d.update_hazard(&mut app.ctx, 1.0 / 60.0);
        format!(
            "Collision! Damage {} percent.",
            ff_core::pyfmt::fmt_f(d.trip.truck.damage_pct, 0)
        )
    });

    app.dispatch_to_state(&key_event(Key::A));
    assert_eq!(last(&app), outcome);
    app.dispatch_to_state(&key_event(Key::Comma));
    assert_eq!(last(&app), outcome);
    app.dispatch_to_state(&key_event(Key::Comma));
    assert_eq!(last(&app), warning);
}

#[test]
fn test_name_entry_keeps_punctuation_for_driver_names() {
    // `NameEntryState` is the driver-name `TextEntryState`; the field itself
    // is what keeps the punctuation.
    let mut app = TestApp::new();
    app.push_state(TextEntryState::new("New career", "Driver name", |_, _| {}));
    assert!(app.state().unwrap().borrow().captures_text_input());
    app.dispatch_to_state(&InputEvent::key_text(Key::Comma, ','));
    app.dispatch_to_state(&InputEvent::key_text(Key::Period, '.'));
    let state = app.state().unwrap();
    let state = state.borrow();
    let entry = state.as_any().downcast_ref::<TextEntryState>().unwrap();
    assert_eq!(entry.name(), ",.");
    drop(state);
    app.shutdown();
}

#[test]
fn test_review_works_outside_driving() {
    // The old review path was wired into the driving state alone.
    let mut app = TestApp::new();
    app.push_state(menu());
    app.ctx.say("Fuel is running low.");
    app.ctx.say("Weigh station ahead.");
    app.clear_speech();

    app.dispatch_to_state(&key_event(Key::Comma));
    assert_eq!(app.main_lines().last().unwrap(), "Weigh station ahead.");
    app.dispatch_to_state(&key_event(Key::Comma));
    assert_eq!(app.main_lines().last().unwrap(), "Fuel is running low.");
    app.shutdown();
}

#[test]
fn test_menu_navigation_stays_out_of_the_review_log() {
    let mut app = TestApp::new();
    app.push_state(menu());
    app.ctx.say("Fuel is running low.");
    app.dispatch_to_state(&key_event(Key::Down));
    app.dispatch_to_state(&key_event(Key::Down));

    let texts: Vec<String> = app
        .ctx
        .message_log
        .messages
        .iter()
        .map(|m| m.text.clone())
        .collect();
    assert_eq!(texts.last().unwrap(), "Fuel is running low.");
    app.shutdown();
}

#[test]
fn test_pausing_mid_run_leaves_no_trace_in_the_history() {
    // Checking the pause menu is where you are, not something that happened.
    let mut app = TestApp::new();
    let drive = drive_support::a_drive(&mut app);
    // `start_drive` reached the wheel by driving the menus, so the drive had
    // already been entered once. `DrivingState::enter` is a no-op after that
    // (`entered_once`), which is what keeps the resume below silent -- and
    // silent is the whole point of the case.
    drive_support::with_drive(&drive, |d| d.entered_once = true);
    app.ctx.message_log.messages.clear();
    app.ctx.message_log.index = -1;

    for index in 0..3 {
        let mut event = TripEvent {
            kind: TripEventKind::Hazard,
            message: SpokenMessage::new(format!("Announcement {index}.")),
            data: TripEventData::default(),
        };
        event.data.deadline_s = Some(9.0);
        drive_support::with_drive(&drive, |d| {
            d.handle_trip_event(&mut app.ctx, &event);
            // Each announcement stands alone: clear the hazard state a real
            // resolution or collision would have cleared, so the next one arms
            // fresh instead of folding into (and resolving) this one.
            d.hazard_deadline = None;
            d.hazard_names.clear();
        });
        app.dispatch_to_state(&key_event(Key::Escape)); // open the pause menu
        app.ctx.run_deferred();
        app.dispatch_to_state(&key_event(Key::Return)); // resume
        app.ctx.run_deferred();
    }

    let texts: Vec<String> = app
        .ctx
        .message_log
        .messages
        .iter()
        .map(|m| m.text.clone())
        .collect();
    assert_eq!(
        texts,
        vec![
            "Announcement 0.".to_string(),
            "Announcement 1.".to_string(),
            "Announcement 2.".to_string(),
        ]
    );
}

#[test]
fn test_review_jumps_to_first_and_last() {
    let mut app = TestApp::new();
    app.push_state(BareState);
    for text in ["One.", "Two.", "Three."] {
        app.ctx.say(text);
    }
    app.clear_speech();

    app.dispatch_to_state(&ctrl_key_event(Key::Comma));
    assert_eq!(app.main_lines().last().unwrap(), "One.");
    app.dispatch_to_state(&ctrl_key_event(Key::Period));
    assert_eq!(app.main_lines().last().unwrap(), "Three.");
    app.shutdown();
}

#[test]
fn test_review_replay_stops_the_event_voice() {
    let mut app = TestApp::new();
    app.push_state(menu());
    app.ctx.say_event("Hazard warning.");
    let before = app.speech().stop_event_calls();
    app.dispatch_to_state(&key_event(Key::Comma));
    assert_eq!(app.speech().stop_event_calls(), before + 1);
    app.shutdown();
}

#[test]
fn test_a_replay_stops_the_event_voice() {
    // A replay is the player asking to hear this line now, so whatever the
    // event voice is working through gives way.
    let mut app = TestApp::new();
    let drive = drive_support::a_drive(&mut app);
    drive_support::with_drive(&drive, |d| {
        d.last_event_message = "Hazard warning.".to_string()
    });
    // Python counted calls to a replaced `ctx.stop_event_speech`. There is no
    // seam here, so the capture's own record of the stop is the evidence.
    app.clear_speech();

    app.dispatch_to_state(&key_event(Key::A));

    assert_eq!(app.speech().stop_event_calls(), 1);
}

#[test]
fn test_a_filter_says_what_it_is_holding_back() {
    // The filter keeps the driver's choice, so it must never keep a secret.
    //
    // Tim S sets the category to Event because it makes the cab navigable,
    // and that preference now survives a lapse instead of dropping back to
    // All. The bug that used to be prevented by dropping it -- a settlement
    // sitting invisible behind a filter, with nothing to say it was there --
    // is prevented instead by counting it out loud (2026-08-21).
    let mut app = TestApp::new();
    app.push_state(menu());
    app.ctx
        .message_log
        .add("Brake now! Debris on the road.", MessageCategory::Event);
    app.clear_speech();

    // Wind the filter round to Event, the way the brackets do.
    app.dispatch_to_state(&key_event(Key::RightBracket));
    app.dispatch_to_state(&key_event(Key::RightBracket));
    assert_eq!(app.main_lines().last().unwrap(), "Event messages.");

    // The settlement lands in a category the filter hides.
    app.ctx.message_log.add(
        "Delivery complete. You earned 900 dollars.",
        MessageCategory::General,
    );

    // Stepping to the newest thing the filter shows says what is beyond it.
    app.dispatch_to_state(&key_event(Key::Comma));
    assert_eq!(
        app.main_lines().last().unwrap(),
        "Brake now! Debris on the road. 1 newer message outside this filter."
    );

    // And pressing forward at the end of the list does not answer in silence.
    app.dispatch_to_state(&key_event(Key::Period));
    assert_eq!(
        app.main_lines().last().unwrap(),
        "1 newer message outside this filter."
    );

    // Winding back to All reaches it, and the notice stops.
    app.dispatch_to_state(&key_event(Key::LeftBracket));
    app.dispatch_to_state(&key_event(Key::LeftBracket));
    assert_eq!(app.main_lines().last().unwrap(), "All messages.");
    app.dispatch_to_state(&key_event(Key::Comma));
    assert_eq!(
        app.main_lines().last().unwrap(),
        "Delivery complete. You earned 900 dollars."
    );
    app.shutdown();
}

#[test]
fn test_an_unfiltered_review_never_mentions_a_filter() {
    // The common case stays exactly as quiet as it was.
    let mut app = TestApp::new();
    app.push_state(menu());
    app.ctx.say("Fuel is running low.");
    app.clear_speech();

    app.dispatch_to_state(&key_event(Key::Comma));
    assert_eq!(app.main_lines().last().unwrap(), "Fuel is running low.");
    app.dispatch_to_state(&key_event(Key::Period));
    assert_eq!(app.main_lines().last().unwrap(), "Fuel is running low.");
    app.shutdown();
}

/// Ctrl+C puts the reviewed message on the clipboard and says so.
#[test]
fn ctrl_c_copies_the_message_in_review() {
    let mut app = TestApp::new();
    app.push_state(BareState);
    app.ctx.say("Weigh station ahead.");
    app.dispatch_to_state(&key_event(Key::Comma));
    app.clear_speech();
    app.dispatch_to_state(&ctrl_key_event(Key::C));
    assert_eq!(
        app.ctx.clipboard.get_text().as_deref(),
        Some("Weigh station ahead.")
    );
    assert_eq!(app.main_lines(), vec!["Message copied to clipboard."]);
    app.shutdown();
}

fn copy_event() -> InputEvent {
    InputEvent::key_mods(
        Key::C,
        Mods {
            shift: true,
            ctrl: true,
            alt: false,
        },
    )
}

struct RefusingClipboard;

impl freight_fate::app::context::Clipboard for RefusingClipboard {
    fn get_text(&self) -> Option<String> {
        None
    }
    fn set_text(&mut self, _text: &str) -> bool {
        false
    }
}

/// Ctrl+Shift+C copies the newest message-log line and says so. Moving
/// through a menu is not logged, so it does not replace that line.
#[test]
fn ctrl_shift_c_copies_the_last_spoken_line() {
    let mut app = TestApp::new();
    app.push_state(menu());
    app.ctx.say("Weigh station ahead.");
    app.clear_speech();
    app.dispatch_to_state(&copy_event());
    assert_eq!(
        app.ctx.clipboard.get_text().as_deref(),
        Some("Weigh station ahead.")
    );
    assert_eq!(app.main_lines(), vec!["Copied."]);
    // The confirmation is not what the next press copies.
    app.dispatch_to_state(&copy_event());
    assert_eq!(
        app.ctx.clipboard.get_text().as_deref(),
        Some("Weigh station ahead.")
    );
    // Moving focus copies the newly focused row, without its position cue.
    app.dispatch_to_state(&key_event(Key::Down));
    app.dispatch_to_state(&copy_event());
    assert_eq!(
        app.ctx.clipboard.get_text().as_deref(),
        Some("Continue career.")
    );
    app.shutdown();
}

/// A result line spoken in a menu (a search result, a status reply) is what
/// is copied, not the option that has focus.
#[test]
fn copy_last_spoken_copies_a_menu_result_line_not_the_focused_option() {
    let mut app = TestApp::new();
    app.push_state(menu());
    app.ctx
        .say("Stations matching network. 52 matches. Results 1 to 25 of 52.");
    app.dispatch_to_state(&copy_event());
    assert_eq!(
        app.ctx.clipboard.get_text().as_deref(),
        Some("Stations matching network. 52 matches. Results 1 to 25 of 52.")
    );
    // Moving on copies the focused row.
    app.dispatch_to_state(&key_event(Key::Down));
    app.dispatch_to_state(&copy_event());
    assert_eq!(
        app.ctx.clipboard.get_text().as_deref(),
        Some("Continue career.")
    );
    app.shutdown();
}

/// Until something new is logged on a screen, the copy key copies the
/// focused row (without its position cue) rather than the screen heading.
#[test]
fn copy_last_spoken_copies_the_focused_row_on_a_fresh_screen() {
    let mut app = TestApp::new();
    app.push_state(menu());
    // The first copy gives the line the screen opened with.
    app.dispatch_to_state(&copy_event());
    assert_eq!(app.ctx.clipboard.get_text().as_deref(), Some("Main menu."));
    app.dispatch_to_state(&key_event(Key::Down));
    app.dispatch_to_state(&copy_event());
    assert_eq!(
        app.ctx.clipboard.get_text().as_deref(),
        Some("Continue career.")
    );
    // Once a new line is logged on the screen, that line wins.
    app.ctx.say("Saved.");
    app.dispatch_to_state(&copy_event());
    assert_eq!(app.ctx.clipboard.get_text().as_deref(), Some("Saved."));
    app.shutdown();
}

/// A screen that says more than its title on opening (the main menu's
/// welcome line) still lets the focused row be copied.
#[test]
fn copy_last_spoken_copies_the_focused_row_after_a_welcome_line() {
    use freight_fate::states::main_menu::MainMenuState;
    let mut app = TestApp::new();
    app.push_state(MainMenuState::new());
    // The first copy gives the opening line; a focus move gives the row.
    app.dispatch_to_state(&copy_event());
    let opening = app.ctx.clipboard.get_text().unwrap_or_default();
    assert!(!opening.is_empty());
    app.dispatch_to_state(&key_event(Key::Down));
    app.dispatch_to_state(&copy_event());
    let first = app.ctx.clipboard.get_text().unwrap_or_default();
    assert!(!first.is_empty());
    assert!(!first.contains("Welcome"), "{first:?}");
    assert!(!first.contains(" of "), "{first:?}");
    assert_ne!(first, opening);
    app.dispatch_to_state(&key_event(Key::Down));
    app.dispatch_to_state(&copy_event());
    let second = app.ctx.clipboard.get_text().unwrap_or_default();
    assert!(!second.contains("Welcome"), "{second:?}");
    assert_ne!(first, second);
    app.shutdown();
}

/// A prompt screen that opens on a question copies the focused answer.
#[test]
fn copy_last_spoken_copies_the_focused_row_on_a_prompt_screen() {
    use freight_fate::states::assist_picker::AssistPickerState;
    let mut app = TestApp::new();
    app.push_state(AssistPickerState::new());
    app.dispatch_to_state(&copy_event());
    let opening = app.ctx.clipboard.get_text().unwrap_or_default();
    assert!(opening.contains("How much should the truck"), "{opening:?}");
    app.dispatch_to_state(&key_event(Key::Down));
    app.dispatch_to_state(&copy_event());
    let copied = app.ctx.clipboard.get_text().unwrap_or_default();
    assert!(!copied.is_empty());
    assert!(!copied.contains("How much should the truck"), "{copied:?}");
    assert!(!copied.contains(" of "), "{copied:?}");
    app.shutdown();
}

/// Enter re-reading a row logs it; moving on copies the new row, not the
/// re-read one.
#[test]
fn copy_last_spoken_after_enter_then_down_copies_the_new_row() {
    let mut app = TestApp::new();
    app.push_state(SimpleMenuState::new(
        "Career stats",
        vec![
            MenuItem::new("Dispatch trust: full", |_, ctx: &mut GameContext| {
                ctx.say("Dispatch trust: full. You get the whole board.")
            }),
            MenuItem::new("CDL: clear", |_, _| {}),
            MenuItem::new("Record: clean", |_, _| {}),
        ],
    ));
    app.dispatch_to_state(&key_event(Key::Return));
    app.dispatch_to_state(&copy_event());
    assert_eq!(
        app.ctx.clipboard.get_text().as_deref(),
        Some("Dispatch trust: full. You get the whole board.")
    );
    app.dispatch_to_state(&key_event(Key::Down));
    app.dispatch_to_state(&copy_event());
    assert_eq!(app.ctx.clipboard.get_text().as_deref(), Some("CDL: clear."));
    app.dispatch_to_state(&key_event(Key::Down));
    app.dispatch_to_state(&copy_event());
    assert_eq!(
        app.ctx.clipboard.get_text().as_deref(),
        Some("Record: clean.")
    );
    app.shutdown();
}

/// A menu position cue is never part of what is copied.
#[test]
fn copy_last_spoken_leaves_out_the_position_cue() {
    use freight_fate::states::base::strip_position_cue;
    assert_eq!(strip_position_cue("Achievements. 2 of 8."), "Achievements.");
    assert_eq!(
        strip_position_cue("750 Network Radio, Web radio, always available. 1 of 27."),
        "750 Network Radio, Web radio, always available."
    );
    assert_eq!(
        strip_position_cue("52 matches. Results 1 to 25 of 52."),
        "52 matches. Results 1 to 25 of 52."
    );
    assert_eq!(strip_position_cue("Fuel is low."), "Fuel is low.");
    let mut app = TestApp::new();
    app.push_state(BareState);
    app.ctx.say("Achievements. 2 of 8.");
    app.dispatch_to_state(&copy_event());
    assert_eq!(
        app.ctx.clipboard.get_text().as_deref(),
        Some("Achievements.")
    );
    app.shutdown();
}

/// Ctrl+Shift+C right after Ctrl+C copies the line, not the
/// "Message copied to clipboard." confirmation.
#[test]
fn copy_last_spoken_skips_the_ctrl_c_confirmation() {
    let mut app = TestApp::new();
    app.push_state(BareState);
    app.ctx.say("Rest area in 5 miles.");
    app.dispatch_to_state(&key_event(Key::Comma));
    app.dispatch_to_state(&ctrl_key_event(Key::C));
    app.ctx.clipboard.set_text("something else");
    app.dispatch_to_state(&copy_event());
    assert_eq!(
        app.ctx.clipboard.get_text().as_deref(),
        Some("Rest area in 5 miles.")
    );
    app.shutdown();
}

/// Ctrl+C keeps copying the message in review, not the last spoken line.
#[test]
fn ctrl_c_still_copies_the_reviewed_message() {
    let mut app = TestApp::new();
    app.push_state(BareState);
    app.ctx.say("First line.");
    app.ctx.say("Second line.");
    app.dispatch_to_state(&key_event(Key::Comma));
    app.dispatch_to_state(&key_event(Key::Comma));
    app.dispatch_to_state(&ctrl_key_event(Key::C));
    assert_eq!(app.ctx.clipboard.get_text().as_deref(), Some("First line."));
    app.shutdown();
}

/// A clipboard that refuses the text is spoken, not silent.
#[test]
fn copy_last_spoken_says_when_the_clipboard_fails() {
    let mut app = TestApp::new();
    app.push_state(BareState);
    app.ctx.clipboard = Box::new(RefusingClipboard);
    app.ctx.say("Fuel is low.");
    app.clear_speech();
    app.dispatch_to_state(&copy_event());
    assert_eq!(app.main_lines(), vec!["Could not copy to the clipboard."]);
    app.shutdown();
}

/// With nothing spoken yet the key says so.
#[test]
fn copy_last_spoken_with_nothing_spoken() {
    let mut app = TestApp::new();
    app.push_state(BareState);
    app.clear_speech();
    app.dispatch_to_state(&copy_event());
    assert_eq!(app.main_lines(), vec!["Nothing to copy yet."]);
    app.shutdown();
}

/// The key is a rebindable action with a keyboard default only.
#[test]
fn copy_last_spoken_is_rebindable_and_keyboard_only() {
    use freight_fate::bindings::{Action, Chord};
    let mut app = TestApp::new();
    assert!(app
        .ctx
        .bindings
        .pad_chords(Action::CopyLastSpoken)
        .is_empty());
    app.push_state(BareState);
    app.ctx
        .bindings
        .set_chord(Action::CopyLastSpoken, Chord::alt(Key::K));
    app.ctx.say("Rest area ahead.");
    app.clear_speech();
    app.dispatch_to_state(&InputEvent::key_mods(Key::K, Mods::ALT));
    assert_eq!(app.main_lines(), vec!["Copied."]);
    assert_eq!(
        app.ctx.clipboard.get_text().as_deref(),
        Some("Rest area ahead.")
    );
    app.shutdown();
}

/// A state that takes typed text keeps every review key for itself.
#[test]
fn text_capture_declines_the_review_keys() {
    struct Field;
    impl State for Field {
        fn captures_text_input(&self) -> bool {
            true
        }
        fn handle_event(&mut self, ctx: &mut GameContext, _event: &InputEvent) {
            ctx.say("field got it");
        }
    }
    let mut app = TestApp::new();
    app.push_state(Field);
    app.ctx.say("One.");
    app.clear_speech();
    app.dispatch_to_state(&key_event(Key::Comma));
    assert_eq!(app.main_lines(), vec!["field got it"]);
    app.shutdown();
}
