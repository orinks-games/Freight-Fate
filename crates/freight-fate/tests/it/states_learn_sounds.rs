//! Learn game sounds: the screens that drive the demo sequencer (port of
//! the state half of `tests/test_learn_sounds_state.py`; the sequencer's own
//! tests live in `ff_core::sound_catalog::demo`).

use std::cell::RefCell;
use std::rc::Rc;

use crate::audio_support::{recordings_rig, HearkenRig};
use crate::states_driving_menus_support as drive_support;
use crate::states_main_menu_support::*;
use ff_core::sound_catalog::{Cue, SoundCategory, SoundEntry, CATALOG};
use freight_fate::app::testing::TestApp;
use freight_fate::audio::{
    Audio, AudioError, SustainLoopSpec, VolumeUpdate, JAKE_CLASSIC_KEY, JAKE_RECORDED_KEY,
};
use freight_fate::states::base::{Key, Menu, State};
use freight_fate::states::driving_core::siren::SIGNATURE_KEY;
use freight_fate::states::driving_menu_states::DriveRef;
use freight_fate::states::driving_pause_states::PauseMenuState;
use freight_fate::states::learn_sounds::{LearnSoundCategoryState, LearnSoundsState};
use freight_fate::states::main_menu::MainMenuState;

/// What the demo asked the audio engine for, in order (the Python
/// `FakeAudio` / `monkeypatch.setattr(app.ctx.audio, ...)` seams).
#[derive(Debug, Default)]
struct DemoCalls {
    played: Vec<(String, f64, f64)>,
    holds: Vec<(String, f64)>,
    released: usize,
}

type DemoLog = Rc<RefCell<DemoCalls>>;

/// An `Audio` that records what the demo does and answers `has_asset` for
/// every key except the `missing/` and `nothing/` families.
#[derive(Default)]
struct DemoAudio {
    log: DemoLog,
}

impl DemoAudio {
    fn install(app: &mut TestApp) -> DemoLog {
        let audio = DemoAudio::default();
        let log = Rc::clone(&audio.log);
        app.ctx.audio = Box::new(audio);
        log
    }
}

impl Audio for DemoAudio {
    fn enabled(&self) -> bool {
        false
    }
    fn backend_name(&self) -> &str {
        "demo-log"
    }
    fn master_volume(&self) -> f64 {
        1.0
    }
    fn sfx_volume(&self) -> f64 {
        1.0
    }
    fn music_volume(&self) -> f64 {
        1.0
    }
    fn weather_volume(&self) -> f64 {
        1.0
    }
    fn engine_volume(&self) -> f64 {
        1.0
    }
    fn ui_volume(&self) -> f64 {
        1.0
    }
    fn engine_running(&self) -> bool {
        false
    }
    fn engine_starting(&self) -> bool {
        false
    }
    fn voice_key(&self, key: &str) -> String {
        key.to_string()
    }
    fn play_with(&mut self, key: &str, volume: f64, pan: f64) {
        self.log
            .borrow_mut()
            .played
            .push((key.to_string(), volume, pan));
    }
    fn play_bank_with(&mut self, base: &str, _fallback: &str, volume: f64, pan: f64) {
        self.play_with(base, volume, pan);
    }
    fn set_engine_duck(&mut self, _duck: f64) {}
    fn set_speech_duck(&mut self, _duck: f64) {}
    fn set_engine_voice(&mut self, _classic: bool) {}
    fn set_jake_voice(&mut self, _classic: bool) {}
    fn has_asset(&mut self, key: &str) -> bool {
        !(key.starts_with("missing/") || key.starts_with("nothing/"))
    }
    fn start_loop_with(&mut self, _channel: u32, _key: &str, _volume: f64, _fade_ms: u32) {}
    fn set_loop_volume(&mut self, _channel: u32, _volume: f64) {}
    fn set_loop_pan(&mut self, _channel: u32, _pan: f64) {}
    fn stop_loop_with(&mut self, _channel: u32, _fade_ms: u32) {}
    fn start_sustain_loop_with(
        &mut self,
        _channel: u32,
        _key: &str,
        _spec: SustainLoopSpec,
        _volume: f64,
    ) {
    }
    fn release_sustain_loop_with(&mut self, _channel: u32, _fade_ms: u32) {}
    fn hold_alert_with(&mut self, key: &str, volume: f64, _fade_ms: u32) {
        self.log.borrow_mut().holds.push((key.to_string(), volume));
    }
    fn release_alert_with(&mut self, _fade_ms: u32) {
        self.log.borrow_mut().released += 1;
    }
    fn hold_cue(&mut self, _name: &str) {}
    fn cue_held(&self, _name: &str) -> bool {
        false
    }
    fn release_cue(&mut self, _name: &str) {}
    fn engine_start_with(&mut self, _play_start_sound: bool) {}
    fn engine_stop_with(&mut self, _shutdown_sound: bool) {}
    fn update(&mut self, _dt: f64) {}
    fn set_engine_rpm_with(&mut self, _rpm: f64, _throttle: f64) {}
    fn set_road_noise(&mut self, _speed_mps: f64) {}
    fn set_weather_with(&mut self, _key: Option<&str>, _intensity: f64) {}
    fn set_wind(&mut self, _intensity: f64) {}
    fn set_ambient_with(&mut self, _key: Option<&str>, _volume: f64) {}
    fn horn_start(&mut self) {}
    fn horn_stop(&mut self) {}
    fn reverse_start(&mut self) {}
    fn reverse_stop(&mut self) {}
    fn stop_world(&mut self) {}
    fn play_music_with(&mut self, _track: &str, _fade_ms: u32) {}
    fn play_radio_stream_with(&mut self, _url: &str, _fade_ms: u32) -> Result<(), AudioError> {
        Ok(())
    }
    fn play_music_file_with(&mut self, _path: &str, _fade_ms: u32) -> Result<(), AudioError> {
        Ok(())
    }
    fn music_playing(&self) -> bool {
        false
    }
    fn radio_now_playing(&self) -> Option<String> {
        None
    }
    fn stop_music_with(&mut self, _fade_ms: u32) {}
    fn set_volumes(&mut self, _volumes: &VolumeUpdate) {}
    fn shutdown(&mut self) {}
}

type Cat = LearnSoundCategoryState;

fn played(log: &DemoLog) -> Vec<String> {
    log.borrow()
        .played
        .iter()
        .map(|(k, _, _)| k.clone())
        .collect()
}

fn demo_running(app: &TestApp) -> bool {
    with_state::<Cat, _>(app, |s, _| s.demo.running())
}

/// A category with a held cue and a one-shot, leaked so its entries are
/// `'static` like the catalog's.
fn held_and_other_category() -> SoundCategory {
    static HELD: [Cue; 1] = [Cue::new("vehicle/bar_solid").hold_s(5.0)];
    static OTHER: [Cue; 1] = [Cue::new("vehicle/lane_centered")];
    let entries: &'static [SoundEntry] = Box::leak(Box::new([
        SoundEntry::new("Held cue", &HELD, "why"),
        SoundEntry::new("Other cue", &OTHER, "why"),
    ]));
    SoundCategory {
        name: "Two",
        entries,
    }
}

#[test]
fn test_the_category_screen_lists_every_catalog_category() {
    let mut app = TestApp::new();
    app.push_state(LearnSoundsState::new());
    let mut expected: Vec<String> = CATALOG.iter().map(|c| c.name.to_string()).collect();
    expected.push("Back".to_string());
    // Every category, in catalog order, then the way out.
    assert_eq!(labels::<LearnSoundsState>(&app), expected);
}

#[test]
fn test_arrowing_speaks_the_name_and_plays_no_cue() {
    let mut app = TestApp::new();
    let log = DemoAudio::install(&mut app);
    app.push_state(LearnSoundCategoryState::new(CATALOG[0]));
    log.borrow_mut().played.clear();
    app.clear_speech();
    key(&mut app, Key::Down);
    let current = current_label::<Cat>(&app);
    assert!(app.main_lines().iter().any(|line| line.contains(&current)));
    // Only the menu's own movement click, never a catalogued cue.
    assert_eq!(played(&log), vec!["ui/menu_move"]);
}

#[test]
fn test_enter_plays_the_entrys_cue_with_its_volume_and_pan() {
    let mut app = TestApp::new();
    let log = DemoAudio::install(&mut app);
    let category = CATALOG[0];
    app.push_state(LearnSoundCategoryState::new(category));
    // Land on an entry whose first cue is a one-shot so the assert is direct.
    let index = category
        .entries
        .iter()
        .position(|e| e.plays[0].hold_s == 0.0)
        .unwrap();
    set_index::<Cat>(&mut app, index);
    log.borrow_mut().played.clear();
    with_state_mut::<Cat, _>(&mut app, |s, ctx| s.activate(ctx));
    let cue = category.entries[index].plays[0];
    assert!(log
        .borrow()
        .played
        .contains(&(cue.key.to_string(), cue.volume, cue.pan)));
}

#[test]
fn test_f1_speaks_the_meaning_and_the_when_note() {
    let mut app = TestApp::new();
    let category = CATALOG
        .iter()
        .find(|c| c.entries.iter().any(|e| !e.when.is_empty()))
        .unwrap();
    let entry_index = category
        .entries
        .iter()
        .position(|e| !e.when.is_empty())
        .unwrap();
    app.push_state(LearnSoundCategoryState::new(*category));
    set_index::<Cat>(&mut app, entry_index);
    let help_text = current_help::<Cat>(&app);
    let entry = category.entries[entry_index];
    assert!(help_text.contains(entry.meaning));
    assert!(help_text.contains(entry.when));
}

#[test]
fn test_leaving_the_screen_releases_a_held_cue() {
    let mut app = TestApp::new();
    let log = DemoAudio::install(&mut app);
    static HELD: [Cue; 1] = [Cue::new("vehicle/bar_solid").hold_s(5.0)];
    let entries: &'static [SoundEntry] =
        Box::leak(Box::new([SoundEntry::new("Held cue", &HELD, "why")]));
    let held = SoundCategory {
        name: "Held",
        entries,
    };
    app.push_state(LearnSoundCategoryState::new(held));
    with_state_mut::<Cat, _>(&mut app, |s, ctx| s.activate(ctx));
    with_state_mut::<Cat, _>(&mut app, State::exit);
    assert!(
        log.borrow().released > 0,
        "a held cue must not survive the screen closing"
    );
}

#[test]
fn test_jump_stops_a_running_held_demo() {
    // Home and End go through `jump`, a route separate from `move_by`.
    let mut app = TestApp::new();
    let log = DemoAudio::install(&mut app);
    app.push_state(LearnSoundCategoryState::new(held_and_other_category()));
    with_state_mut::<Cat, _>(&mut app, |s, ctx| s.activate(ctx)); // starts the held demo
    assert!(demo_running(&app));
    with_state_mut::<Cat, _>(&mut app, |s, ctx| s.jump(ctx, 1));
    assert!(
        log.borrow().released > 0,
        "Home/End must stop a running held demo"
    );
    assert!(!demo_running(&app));
}

#[test]
fn test_first_letter_jump_stops_a_running_held_demo() {
    let mut app = TestApp::new();
    let log = DemoAudio::install(&mut app);
    app.push_state(LearnSoundCategoryState::new(held_and_other_category()));
    with_state_mut::<Cat, _>(&mut app, |s, ctx| s.activate(ctx));
    assert!(demo_running(&app));
    with_state_mut::<Cat, _>(&mut app, |s, ctx| s.first_letter_jump(ctx, "o")); // "Other cue"
    assert!(
        log.borrow().released > 0,
        "a first-letter jump must stop a running held demo"
    );
    assert!(!demo_running(&app));
}

#[test]
fn test_arrow_move_stops_a_running_held_demo() {
    let mut app = TestApp::new();
    let log = DemoAudio::install(&mut app);
    app.push_state(LearnSoundCategoryState::new(held_and_other_category()));
    with_state_mut::<Cat, _>(&mut app, |s, ctx| s.activate(ctx));
    assert!(demo_running(&app));
    key(&mut app, Key::Down);
    assert!(
        log.borrow().released > 0,
        "an ordinary arrow move must stop a running held demo"
    );
    assert!(!demo_running(&app));
}

#[test]
fn test_reentering_the_screen_stops_a_running_held_demo() {
    // `pop_state` calls `enter` again on whatever it uncovered, which
    // re-announces the title -- while a demo whose clock froze under the
    // covering state would pick its hold straight back up underneath it.
    let mut app = TestApp::new();
    let log = DemoAudio::install(&mut app);
    app.push_state(LearnSoundCategoryState::new(held_and_other_category()));
    with_state_mut::<Cat, _>(&mut app, |s, ctx| s.activate(ctx));
    assert!(demo_running(&app));
    with_state_mut::<Cat, _>(&mut app, State::enter);
    assert!(
        log.borrow().released > 0,
        "re-entry must stop a running held demo"
    );
    assert!(!demo_running(&app));
}

#[test]
fn test_the_enforcement_marker_plays_on_a_cold_open() {
    // Opening the screen with no drive in progress must still play the marker.
    //
    // The enforcement signature is synthesized, and the only other thing that
    // publishes it is a drive starting. Opened from the main menu it resolved
    // to nothing, so the entry that teaches the one cue the enforcement
    // contract rests on would have demonstrated silence.
    //
    // Python put the process back at a cold launch by deleting the generated
    // sound and clearing `driving_siren._registered`. Rust guards that with a
    // `Once`, which cannot be reset, so the cold open is asserted the other
    // way round: the screen's own `enter` is what publishes the key, and the
    // marker plays it. (This is what caught the port defect -- `enter` was
    // registering the ladder earcons and the lane guide tone but not the
    // signature, so a main-menu open demonstrated silence.)
    let mut app = TestApp::new();
    let log = DemoAudio::install(&mut app);
    let enforcement = CATALOG
        .iter()
        .find(|c| c.name == "Enforcement")
        .expect("the catalog has an Enforcement category");
    let mut state = LearnSoundCategoryState::new(*enforcement);

    State::enter(&mut state, &mut app.ctx);

    assert!(
        ff_core::assets_pack::generated_sound(SIGNATURE_KEY).is_some(),
        "opening the screen publishes the synthesized signature"
    );
    let index = enforcement
        .entries
        .iter()
        .position(|e| e.name == "Enforcement marker")
        .expect("the category teaches the marker");
    state.menu_mut().index = index;
    log.borrow_mut().played.clear();

    Menu::activate(&mut state, &mut app.ctx);

    let played: Vec<String> = log
        .borrow()
        .played
        .iter()
        .map(|(key, _, _)| key.clone())
        .collect();
    assert_eq!(played, vec![SIGNATURE_KEY.to_string()]);
}

#[test]
fn test_an_unplayable_cue_is_spoken_about_rather_than_silent() {
    // Silence would teach the player that a real cue makes no sound.
    let mut app = TestApp::new();
    let log = DemoAudio::install(&mut app);
    static GONE: [Cue; 1] = [Cue::new("nothing/at_all")];
    let entries: &'static [SoundEntry] =
        Box::leak(Box::new([SoundEntry::new("Missing cue", &GONE, "why")]));
    let gone = SoundCategory {
        name: "Gone",
        entries,
    };
    app.push_state(LearnSoundCategoryState::new(gone));
    log.borrow_mut().played.clear();
    app.clear_speech();
    with_state_mut::<Cat, _>(&mut app, |s, ctx| s.activate(ctx));
    assert!(
        played(&log).is_empty(),
        "nothing resolved, so nothing should have been played"
    );
    assert!(app
        .main_lines()
        .iter()
        .any(|line| line.contains("Missing cue") && line.contains("not available")));
    assert!(!demo_running(&app));
}

#[test]
fn test_the_category_help_does_not_promise_a_stop_it_cannot_make() {
    // Escape releases a held cue; a one-shot already playing finishes.
    let state = LearnSoundCategoryState::new(CATALOG[0]);
    let help_text = state.menu().intro_help.clone();
    assert!(help_text.contains("finishes on its own"));
    assert!(!help_text.contains("stops the sound and goes back"));
}

#[test]
fn test_the_main_menu_offers_learn_game_sounds() {
    let mut app = TestApp::new();
    app.push_state(MainMenuState::new());
    let rows = labels::<MainMenuState>(&app);
    assert!(rows.iter().any(|label| label == "Learn game sounds"));
    // It sits with the other learning material, after How to play.
    let learn = rows.iter().position(|l| l == "Learn game sounds").unwrap();
    let help = rows.iter().position(|l| l == "How to play").unwrap();
    assert_eq!(learn, help + 1);
}

#[test]
fn test_the_pause_menu_offers_learn_game_sounds() {
    let mut app = TestApp::new();
    let drive = drive_support::a_drive(&mut app);
    let mut pause = PauseMenuState::with_drive(DriveRef::of(&drive));
    let labels: Vec<String> = pause
        .build_items(&mut app.ctx)
        .iter()
        .map(|item| item.text(&pause, &app.ctx))
        .collect();
    let learn = labels
        .iter()
        .position(|l| l == "Learn game sounds")
        .unwrap_or_else(|| panic!("{labels:?}"));
    let controls = labels
        .iter()
        .position(|l| l == "Controls and help")
        .unwrap_or_else(|| panic!("{labels:?}"));
    assert_eq!(learn, controls + 1, "{labels:?}");
}

#[test]
fn test_jake_stage_demo_resolves_through_the_jake_voice_setting() {
    // The engine-brake demos name the 1600 rpm cut; what plays is the voice
    // the player chose in settings, the way the jake sounds on the road.
    let category = CATALOG
        .iter()
        .find(|c| {
            c.entries
                .iter()
                .any(|e| e.name == "Engine brake, stage one")
        })
        .copied()
        .expect("the engine brake category");
    for (setting, heard, not_heard) in [
        ("classic", JAKE_CLASSIC_KEY, JAKE_RECORDED_KEY),
        ("real", JAKE_RECORDED_KEY, JAKE_CLASSIC_KEY),
    ] {
        let Some(HearkenRig { engine, rec }) = recordings_rig() else {
            return;
        };
        let mut app = TestApp::new();
        app.ctx.audio = Box::new(engine);
        app.ctx.settings.jake_voice = setting.to_string();
        app.ctx.apply_volumes();
        app.push_state(LearnSoundsState::new());
        app.push_state(LearnSoundCategoryState::new(category));
        with_state_mut::<Cat, _>(&mut app, |s, ctx| {
            s.menu_mut().index = 0;
            s.activate(ctx);
        });
        assert!(demo_running(&app), "{setting}");
        assert!(
            rec.channel_of(heard).is_some(),
            "{setting}: {:?}",
            rec.played()
        );
        assert!(
            rec.channel_of(not_heard).is_none(),
            "{setting}: {:?}",
            rec.played()
        );
    }
}

#[test]
fn test_both_entry_points_push_the_same_screen() {
    let mut app = TestApp::new();
    app.push_state(MainMenuState::new());
    select::<MainMenuState>(&mut app, "Learn game sounds");
    assert!(is::<LearnSoundsState>(&app));
}

#[test]
fn test_both_learn_sounds_screens_offer_a_back_row() {
    // Escape always worked; a row you can arrow onto is how the rest of the
    // game offers the way out, and the only way a player finds it without
    // having heard the intro (owner, 2026-08-16).
    let mut app = TestApp::new();
    app.push_state(LearnSoundsState::new());
    assert_eq!(labels::<LearnSoundsState>(&app).last().unwrap(), "Back");
    key(&mut app, Key::End);
    key(&mut app, Key::Return);
    assert_eq!(app.ctx.stack_len(), 0);
    // The second half needs its own app, and TestApp holds the environment
    // lock until it drops -- shadowing the binding would deadlock.
    drop(app);

    let mut app = TestApp::new();
    app.push_state(LearnSoundsState::new());
    app.push_state(LearnSoundCategoryState::new(CATALOG[0]));
    assert_eq!(labels::<Cat>(&app).last().unwrap(), "Back");
    key(&mut app, Key::End);
    key(&mut app, Key::Return);
    assert!(is::<LearnSoundsState>(&app));
}

#[test]
fn test_leaving_a_category_by_the_back_row_stops_a_held_demo() {
    // The row must not leave a cue ringing behind it.
    let mut app = TestApp::new();
    let log = DemoAudio::install(&mut app);
    app.push_state(LearnSoundsState::new());
    app.push_state(LearnSoundCategoryState::new(held_and_other_category()));
    with_state_mut::<Cat, _>(&mut app, |s, ctx| s.activate(ctx));
    assert!(demo_running(&app));
    let released_before = log.borrow().released;
    // The Back row, activated without moving onto it (moving would stop
    // the demo by itself).
    let back = labels::<Cat>(&app).len() - 1;
    with_state_mut::<Cat, _>(&mut app, |s, ctx| {
        s.menu_mut().index = back;
        s.activate(ctx);
    });
    assert!(
        log.borrow().released > released_before,
        "go_back must stop the demo on the way out"
    );
    assert!(is::<LearnSoundsState>(&app));
}
