//! Settings, Speech: the rows a voice decides (rate, pitch, volume, Speech
//! voice) change in place after a voice row changes, once the speech sink
//! has caught up, and silently when a voice comes or goes from outside.

use crate::states_main_menu_support::*;
use freight_fate::app::testing::TestApp;
use freight_fate::speech::{CaptureProfile, CaptureSpeech};
use freight_fate::states::base::Key;
use freight_fate::states::main_menu::SettingsCategoryState;

type Cat = SettingsCategoryState;

const FRAME: f64 = 0.016;

/// NVDA alone: no rate, pitch, volume or installed voices. OneCore is on
/// offer as the event voice and as the main voice.
fn screen_reader_only() -> CaptureProfile {
    CaptureProfile {
        event_backend_name: Some("OneCore".to_string()),
        supports_rate: false,
        supports_pitch: false,
        supports_volume: false,
        event_supports_rate: false,
        event_backend_options: vec!["OneCore".to_string()],
        main_backend_options: vec!["NVDA".to_string(), "OneCore".to_string()],
        voice_names: Vec::new(),
        ..CaptureProfile::full_voice()
    }
}

/// The same machine once OneCore is speaking: every control is there.
fn with_onecore() -> CaptureProfile {
    CaptureProfile {
        supports_rate: true,
        supports_pitch: true,
        supports_volume: true,
        event_supports_rate: true,
        voice_names: vec!["David".to_string(), "Zira".to_string()],
        ..screen_reader_only()
    }
}

fn has_row(app: &TestApp, prefix: &str) -> bool {
    labels::<Cat>(app)
        .iter()
        .any(|label| label.starts_with(prefix))
}

fn control_rows(app: &TestApp) -> [bool; 4] {
    [
        "Speech rate",
        "Speech pitch",
        "Speech volume",
        "Speech voice",
    ]
    .map(|prefix| has_row(app, prefix))
}

/// A sink that runs voice changes only when the test steps it, the way the
/// speech worker does, in the Speech menu with events on the main voice.
fn lagged_speech_menu() -> TestApp {
    let mut sink = CaptureSpeech::with_profile(screen_reader_only());
    sink.set_worker_lag(true);
    let mut app = TestApp::with_speech(sink);
    app.ctx.settings.sapi_events = false;
    app.ctx.apply_speech();
    app.speech_mut().step_worker();
    app.push_state(SettingsCategoryState::new("speech"));
    app
}

/// The row as spoken, with its position in the list as it is now.
fn spoken_row(app: &TestApp) -> String {
    let at = index::<Cat>(app) + 1;
    let of = labels::<Cat>(app).len();
    format!("{}. {at} of {of}.", current_label::<Cat>(app))
}

#[test]
fn the_event_voice_row_waits_for_the_switch_then_speaks_once_with_the_new_rows() {
    let mut app = lagged_speech_menu();
    move_to::<Cat>(&mut app, "Driving event voice");
    let at = index::<Cat>(&app);
    assert_eq!(control_rows(&app), [false; 4]);
    app.speech_mut().stage_profile(with_onecore());
    app.clear_speech();
    key(&mut app, Key::Right);
    assert!(app.ctx.settings.sapi_events);
    assert_eq!(app.ctx.settings.event_backend, "OneCore");
    // The switch has not run: the old rows, and only the click so far.
    app.tick(FRAME);
    assert_eq!(control_rows(&app), [false; 4]);
    assert!(app.main_lines().is_empty(), "{:?}", app.main_lines());
    app.speech_mut().step_worker();
    app.tick(FRAME);
    assert_eq!(control_rows(&app), [true; 4]);
    assert_eq!(index::<Cat>(&app), at, "the cursor stays on the row");
    assert_eq!(
        current_label::<Cat>(&app),
        "Driving event voice: Windows OneCore"
    );
    assert_eq!(app.main_lines(), [spoken_row(&app)]);
    // Nothing more once it has spoken.
    app.tick(FRAME);
    assert_eq!(app.main_lines().len(), 1);
}

#[test]
fn the_main_voice_row_waits_for_the_switch_then_speaks_once_with_the_new_rows() {
    let mut app = lagged_speech_menu();
    move_to::<Cat>(&mut app, "Main voice");
    let at = index::<Cat>(&app);
    app.speech_mut().stage_profile(with_onecore());
    app.clear_speech();
    // Left from automatic: the last voice, OneCore.
    key(&mut app, Key::Left);
    assert_eq!(app.ctx.settings.main_backend, "OneCore");
    app.tick(FRAME);
    assert_eq!(control_rows(&app), [false; 4]);
    assert!(app.main_lines().is_empty(), "{:?}", app.main_lines());
    app.speech_mut().step_worker();
    app.tick(FRAME);
    assert_eq!(control_rows(&app), [true; 4]);
    assert_eq!(index::<Cat>(&app), at);
    assert_eq!(current_label::<Cat>(&app), "Main voice: Windows OneCore");
    assert_eq!(app.main_lines(), [spoken_row(&app)]);
}

#[test]
fn a_switch_that_never_settles_speaks_the_row_after_the_deadline() {
    let mut app = lagged_speech_menu();
    move_to::<Cat>(&mut app, "Driving event voice");
    app.clear_speech();
    key(&mut app, Key::Right);
    app.tick(0.1);
    app.tick(0.1);
    assert!(app.main_lines().is_empty());
    app.tick(0.15);
    // Spoken as the rows stand; the sink is still behind.
    assert_eq!(control_rows(&app), [false; 4]);
    assert_eq!(app.main_lines(), [spoken_row(&app)]);
    app.tick(0.5);
    assert_eq!(app.main_lines().len(), 1);
}

#[test]
fn a_voice_that_comes_or_goes_from_outside_changes_the_rows_in_place() {
    let mut app = TestApp::with_speech(CaptureSpeech::with_profile(with_onecore()));
    app.push_state(SettingsCategoryState::new("speech"));
    move_to::<Cat>(&mut app, "Main voice");
    let at = index::<Cat>(&app);
    app.clear_speech();
    // The screen reader's voice takes over: the rows go, silently.
    app.speech_mut().set_profile(screen_reader_only());
    app.tick(FRAME);
    assert_eq!(control_rows(&app), [false; 4]);
    assert_eq!(index::<Cat>(&app), at);
    // And come back, silently.
    app.speech_mut().set_profile(with_onecore());
    app.tick(FRAME);
    assert_eq!(control_rows(&app), [true; 4]);
    assert_eq!(index::<Cat>(&app), at);
    assert!(app.main_lines().is_empty(), "{:?}", app.main_lines());
    // The focused row goes away: the cursor lands on the nearest row, and
    // that row is spoken because the player's place changed.
    move_to::<Cat>(&mut app, "Speech pitch");
    let pitch = index::<Cat>(&app);
    app.clear_speech();
    app.speech_mut().set_profile(screen_reader_only());
    app.tick(FRAME);
    let landed = index::<Cat>(&app);
    assert!(landed <= pitch);
    assert_eq!(app.main_lines(), [spoken_row(&app)]);
}

#[test]
fn moving_on_before_the_switch_settles_speaks_only_the_new_row() {
    let mut app = lagged_speech_menu();
    move_to::<Cat>(&mut app, "Main voice");
    app.speech_mut().stage_profile(with_onecore());
    app.clear_speech();
    key(&mut app, Key::Left);
    // Down inside the wait: that row is spoken, the Main voice row is not.
    key(&mut app, Key::Down);
    app.tick(FRAME);
    app.speech_mut().step_worker();
    app.tick(FRAME);
    app.tick(0.5);
    let heard = app.main_lines();
    assert_eq!(heard.len(), 1, "{heard:?}");
    assert!(heard[0].starts_with("Driving event voice"), "{heard:?}");
    assert!(current_label::<Cat>(&app).starts_with("Driving event voice"));
    // The rows are the new voice's.
    assert_eq!(control_rows(&app), [true; 4]);
}
