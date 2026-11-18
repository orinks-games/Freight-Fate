//! Settings, Speech: the Main voice row and how the Driving event voice and
//! Speech voice rows read beside it, driven through the capture sink.

use crate::states_main_menu_support::*;
use ff_core::settings::Settings;
use freight_fate::app::testing::TestApp;
use freight_fate::speech::{apply_speech_settings, CaptureProfile, CaptureSpeech};
use freight_fate::states::base::Key;
use freight_fate::states::main_menu::SettingsCategoryState;

type Cat = SettingsCategoryState;

fn speech_settings(app: &mut TestApp) {
    app.push_state(SettingsCategoryState::new("speech"));
}

fn row(app: &TestApp, prefix: &str) -> String {
    labels::<Cat>(app)
        .into_iter()
        .find(|label| label.starts_with(prefix))
        .unwrap_or_else(|| panic!("no {prefix} row"))
}

#[test]
fn main_voice_row_sits_above_driving_event_voice_with_its_help() {
    let mut app = TestApp::with_speech(CaptureSpeech::full_voice());
    speech_settings(&mut app);
    let rows = labels_and_help::<Cat>(&app);
    let main = rows
        .iter()
        .position(|(label, _)| label.starts_with("Main voice"))
        .unwrap();
    assert!(rows[main + 1].0.starts_with("Driving event voice"));
    assert_eq!(rows[main].0, "Main voice: automatic");
    assert_eq!(
        rows[main].1,
        "The voice that reads menus and readouts. Automatic uses your screen \
         reader when one is running, and a system voice when none is. A screen \
         reader is listed only while it is running."
    );
}

#[test]
fn main_voice_row_cycles_both_ways_and_selects_each_voice() {
    let mut app = TestApp::with_speech(CaptureSpeech::full_voice());
    speech_settings(&mut app);
    move_to::<Cat>(&mut app, "Main voice");
    let start = index::<Cat>(&app);
    app.speech_mut().clear();
    let mut heard = Vec::new();
    for _ in 0..5 {
        key(&mut app, Key::Right);
        heard.push(current_label::<Cat>(&app));
        assert_eq!(index::<Cat>(&app), start, "the cursor stays on the row");
    }
    assert_eq!(
        heard,
        [
            "Main voice: NVDA",
            "Main voice: Windows OneCore",
            "Main voice: SAPI",
            "Main voice: built-in voice",
            "Main voice: automatic",
        ]
    );
    key(&mut app, Key::Left);
    assert_eq!(current_label::<Cat>(&app), "Main voice: built-in voice");
    assert_eq!(app.ctx.settings.main_backend, "Genny");
    key(&mut app, Key::Left);
    assert_eq!(app.ctx.settings.main_backend, "SAPI");
    let selections = app.speech().main_selections().to_vec();
    let last_seven: Vec<Option<&str>> = selections[selections.len() - 7..]
        .iter()
        .map(|choice| choice.as_deref())
        .collect();
    assert_eq!(
        last_seven,
        [
            Some("NVDA"),
            Some("OneCore"),
            Some("SAPI"),
            Some("Genny"),
            None,
            Some("Genny"),
            Some("SAPI"),
        ]
    );
    // Saved as chosen.
    assert_eq!(Settings::load().main_backend, "SAPI");
}

#[test]
fn an_unavailable_main_voice_is_named_and_kept() {
    let mut app = TestApp::with_speech(CaptureSpeech::full_voice());
    app.ctx.settings.main_backend = "JAWS".to_string();
    app.ctx.apply_speech();
    assert_eq!(app.ctx.settings.main_backend, "JAWS");
    speech_settings(&mut app);
    assert_eq!(row(&app, "Main voice"), "Main voice: JAWS, unavailable");
    move_to::<Cat>(&mut app, "Main voice");
    // Not a stop: Left goes to the last voice.
    key(&mut app, Key::Left);
    assert_eq!(app.ctx.settings.main_backend, "Genny");
}

#[test]
fn the_event_row_reads_main_voice_when_both_name_one_voice() {
    let mut app = TestApp::with_speech(CaptureSpeech::full_voice());
    assert_eq!(app.ctx.settings.event_backend, "SAPI");
    speech_settings(&mut app);
    assert_eq!(row(&app, "Driving event voice"), "Driving event voice: SAPI");
    move_to::<Cat>(&mut app, "Main voice");
    for _ in 0..3 {
        key(&mut app, Key::Right);
    }
    assert_eq!(current_label::<Cat>(&app), "Main voice: SAPI");
    assert_eq!(
        row(&app, "Driving event voice"),
        "Driving event voice: main voice"
    );
    // The saved event voice is untouched, in memory and on disk.
    assert!(app.ctx.settings.sapi_events);
    assert_eq!(app.ctx.settings.event_backend, "SAPI");
    assert_eq!(Settings::load().event_backend, "SAPI");
    // Back to automatic: the separate voice returns.
    move_to::<Cat>(&mut app, "Main voice");
    key(&mut app, Key::Right);
    key(&mut app, Key::Right);
    assert_eq!(current_label::<Cat>(&app), "Main voice: automatic");
    assert_eq!(row(&app, "Driving event voice"), "Driving event voice: SAPI");
}

#[test]
fn an_event_voice_on_neither_list_is_still_replaced() {
    let mut sink = CaptureSpeech::full_voice();
    let mut settings = Settings {
        event_backend: "AVSpeech".to_string(),
        ..Settings::default()
    };
    apply_speech_settings(&mut sink, &mut settings);
    assert_eq!(settings.event_backend, "OneCore");
    // On the main list only: kept.
    let mut settings = Settings {
        event_backend: "Genny".to_string(),
        ..Settings::default()
    };
    apply_speech_settings(&mut sink, &mut settings);
    assert_eq!(settings.event_backend, "Genny");
}

#[test]
fn the_speech_voice_row_reads_default_for_a_voice_the_bound_list_lacks() {
    let profile = CaptureProfile::full_voice();
    let mut app = TestApp::with_speech(CaptureSpeech::with_profile(profile));
    app.ctx.settings.speech_voice = "Kathy".to_string();
    speech_settings(&mut app);
    assert_eq!(row(&app, "Speech voice"), "Speech voice: default");
    app.pop_state();
    app.ctx.settings.speech_voice = "Zira".to_string();
    speech_settings(&mut app);
    assert_eq!(row(&app, "Speech voice"), "Speech voice: Zira");
}
