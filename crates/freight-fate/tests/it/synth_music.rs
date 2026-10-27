//! Synthesized music: the settings rows, menu and Roadhouse rotations, and
//! the fallback when a piece is not ready.

use crate::audio_support::{hearken_rig, sine_wav};
use crate::states_main_menu_support::*;
use ff_core::data::world::get_world;
use ff_core::models::jobs::{Job, CARGO_CATALOG};
use ff_core::models::profile::Profile;
use ff_core::radio::{dial_group, SAFE_ROUTE_PLAYLIST};
use ff_core::sim::hos::is_night;
use freight_fate::app::testing::TestApp;
use freight_fate::states::base::{InputEvent, Key, Mods};
use freight_fate::states::driving::DrivingState;
use freight_fate::states::driving_core::DRIVE_PHASE_DELIVERY;
use freight_fate::states::main_menu::{SettingsCategoryState, SettingsState};

type Cat = SettingsCategoryState;

fn open_audio(app: &mut TestApp) {
    app.push_state(SettingsState::new());
    select::<SettingsState>(app, "Audio");
    assert!(is::<Cat>(app));
}

#[test]
fn defaults_are_original_and_a_fixed_seed() {
    let app = TestApp::new();
    assert!(!app.ctx.settings.synth_music);
    assert_eq!(app.ctx.settings.music_seed, 48213);
}

#[test]
fn music_source_row_toggles_and_persists() {
    let mut app = TestApp::new();
    open_audio(&mut app);
    move_to::<Cat>(&mut app, "Music source");
    assert_eq!(current_label::<Cat>(&app), "Music source: Original");
    key(&mut app, Key::Return);
    assert_eq!(current_label::<Cat>(&app), "Music source: Synthesized");
    assert!(app.ctx.settings.synth_music);
}

#[test]
fn music_seed_row_rolls_a_new_seed_in_range() {
    let mut app = TestApp::new();
    open_audio(&mut app);
    move_to::<Cat>(&mut app, "Music seed");
    assert_eq!(current_label::<Cat>(&app), "Music seed: 48213");
    app.clear_speech();
    key(&mut app, Key::Right);
    let transcript = app.speech().transcript();
    assert!(transcript.contains("New music seed, "), "{transcript}");
    let seed = app.ctx.settings.music_seed;
    assert!((10_000..=99_999).contains(&seed), "{seed}");
    assert_ne!(seed, 48213);
}

fn type_text(app: &mut TestApp, text: &str) {
    for ch in text.chars() {
        app.dispatch_to_state(&InputEvent::typed(ch));
    }
}

#[test]
fn a_typed_music_seed_saves_and_changes_the_menu_music() {
    use freight_fate::states::text_entry::TextEntryState;
    let mut app = TestApp::new();
    app.ctx.settings.synth_music = true;
    open_audio(&mut app);
    move_to::<Cat>(&mut app, "Music seed");
    app.clear_speech();
    key(&mut app, Key::Return);
    assert!(is::<TextEntryState>(&app));
    let prompt = app.speech().transcript();
    assert!(
        prompt.contains("Music seed. Type, then press Enter."),
        "{prompt}"
    );

    // Anything but a whole number is refused and the field stays open.
    type_text(&mut app, "12a");
    app.clear_speech();
    key(&mut app, Key::Return);
    assert!(is::<TextEntryState>(&app));
    assert!(app.speech().transcript().contains("Type a whole number."));
    assert_eq!(app.ctx.settings.music_seed, 48213);

    key(&mut app, Key::Backspace);
    type_text(&mut app, "34");
    assert_eq!(
        app.state()
            .unwrap()
            .borrow()
            .as_any()
            .downcast_ref::<TextEntryState>()
            .unwrap()
            .name(),
        "1234"
    );
    app.clear_speech();
    key(&mut app, Key::Return);
    assert!(is::<Cat>(&app));
    let back = app.speech().transcript();
    assert!(back.contains("Music seed: 1234"), "{back}");
    assert_eq!(app.ctx.settings.music_seed, 1234);
    assert_eq!(ff_core::settings::Settings::load().music_seed, 1234);
    let before = ff_core::music_synth::select_synth_menu_sequence(None, 48213);
    let after = ff_core::music_synth::select_synth_menu_sequence(None, 1234);
    assert!(after[1..].iter().all(|k| k.contains("_1234_")), "{after:?}");
    assert!(after[1..].iter().all(|k| !before.contains(k)));
}

#[test]
fn escape_leaves_the_music_seed_alone() {
    let mut app = TestApp::new();
    open_audio(&mut app);
    move_to::<Cat>(&mut app, "Music seed");
    key(&mut app, Key::Return);
    type_text(&mut app, "777");
    key(&mut app, Key::Escape);
    assert!(is::<Cat>(&app));
    assert_eq!(app.ctx.settings.music_seed, 48213);
}

#[test]
fn the_three_classics_play_from_the_executable() {
    freight_fate::audio::classic_music::register();
    for key in [
        "classic_menu_theme",
        "classic_open_road",
        "classic_night_haul",
    ] {
        let (bytes, ext) = freight_fate::audio::assets::asset_bytes(
            &format!("music/{key}"),
            freight_fate::audio::assets::MUSIC_EXTENSIONS,
        )
        .unwrap_or_else(|| panic!("{key} missing"));
        assert_eq!(ext, "ogg");
        assert_eq!(&bytes[..4], b"OggS");
    }
}

#[test]
fn synthesized_menus_open_on_headlights_west_and_hold_no_pack_music() {
    let mut app = TestApp::new();
    app.ctx.settings.synth_music = true;
    let original = ff_core::music::select_menu_music_sequence(None);
    let refs: Vec<&str> = original.iter().map(String::as_str).collect();
    let track = app.ctx.play_music_sequence("menu", &refs);
    assert_eq!(track, ff_core::music_synth::CLASSIC_MENU);
}

#[test]
fn an_unready_piece_falls_back_to_its_classic_and_is_requested() {
    use ff_core::music_synth::{StyleId, SynthKey, SynthWorker, CLASSIC_MENU};
    let mut app = TestApp::new();
    let key = SynthKey {
        style: StyleId::Regional,
        music_seed: 5,
        index: 0,
    }
    .key();
    assert_eq!(app.ctx.resolve_synth(&key), CLASSIC_MENU);
    // The worker was asked: within a bounded wait the piece is published.
    let t = std::time::Instant::now();
    while !SynthWorker::is_ready(&key) && t.elapsed().as_secs() < 60 {
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert_eq!(app.ctx.resolve_synth(&key), key);
}

/// The Denver run the radio rotation tests drive (`a_denver_drive` in
/// `states_driving_updates_radio.rs`).
fn a_drive(app: &mut TestApp) -> DrivingState {
    a_drive_at(app, 13.0)
}

/// The same run leaving at `start_hour` on the trip's Eastern clock.
fn a_drive_at(app: &mut TestApp, start_hour: f64) -> DrivingState {
    let world = get_world();
    app.ctx.profile = Some(Profile::named_in("Radio Power", "Denver"));
    let route = world
        .route_from_cities(&["Denver", "Salt Lake City"])
        .expect("Denver to Salt Lake City routes");
    let job = Job::new(
        &CARGO_CATALOG["general"],
        12.0,
        "Denver",
        "Denver Dry Warehouse",
        "Salt Lake City",
        520.0,
        2400.0,
        14.0,
    );
    let mut drive = DrivingState::new(
        &mut app.ctx,
        job,
        route,
        Some(42),
        DRIVE_PHASE_DELIVERY,
        Some(start_hour),
    );
    drive.trip.set_npc_vehicles(Vec::new());
    drive
}

/// Denver keeps Mountain time, two hours behind the trip's Eastern clock: at
/// 10 PM Eastern it is 8 PM there, still dusk. The drive calls that day, so
/// the Roadhouse has to as well. When it judged night by the Eastern clock,
/// the two never agreed and it restarted itself every frame until 11 PM.
#[test]
fn the_roadhouse_judges_night_by_the_local_clock() {
    let mut app = TestApp::new();
    let mut d = a_drive_at(&mut app, 22.0);
    assert!(is_night(d.trip.current_hour()) && !d.night_now());
    let station = d.radio.current_station();
    assert_eq!(station.playlist, "route");
    d.start_station_rotation(&mut app.ctx, &station, 0);
    assert_eq!(d.music_night, d.night_now());
    let started = d.radio_elapsed_s;
    for _ in 0..3 {
        let night = d.night_now();
        d.update_radio_playback(&mut app.ctx, night, 1.0);
    }
    assert!(
        (d.radio_elapsed_s - (started + 3.0)).abs() < 1e-9,
        "the Roadhouse restarted"
    );
}

#[test]
fn the_synthesized_roadhouse_plays_synth_music_and_no_host_breaks() {
    let mut app = TestApp::new();
    app.ctx.settings.synth_music = true;
    let mut d = a_drive(&mut app);
    let station = d.radio.current_station();
    assert_eq!(station.playlist, "route");
    let pool = d.station_rotation_pool(&app.ctx, &station, false);
    assert!(pool
        .iter()
        .all(|k| k.starts_with("synth_drive_day_") || k == "classic_open_road"));
    // Play through more tracks than a break interval: nothing from a host break.
    d.start_station_rotation(&mut app.ctx, &station, 0);
    for _ in 0..(ff_core::music::RADIO_TRACKS_PER_HOST_BREAK * 2) {
        d.radio_elapsed_s = 1.0e9;
        d.update_radio_playback(&mut app.ctx, false, 0.0);
        assert!(d.radio_break_queue.is_empty());
    }
}

/// The Roadhouse's rotation start already resolves and plays the track it
/// lands on; it must also queue the one after that so a track change never
/// lands on an unrendered piece and falls back to the same classic twice in
/// a row.
#[test]
fn starting_the_roadhouse_rotation_queues_the_next_synthesized_piece() {
    use ff_core::music_synth::{SynthKey, SynthWorker};
    let mut app = TestApp::new();
    app.ctx.settings.synth_music = true;
    let mut d = a_drive(&mut app);
    let station = d.radio.current_station();
    d.radio_airtime_s = 0.0; // a cold start: track index 0, so `next` is fixed.
    d.start_station_rotation(&mut app.ctx, &station, 0);
    let len = d.radio_playlist.len();
    let next = d.radio_playlist[(d.radio_track_index + 1) % len].clone();
    assert!(
        SynthKey::parse(&next).is_some(),
        "expected a synth key after the opening track, got {next}"
    );
    let t = std::time::Instant::now();
    while !SynthWorker::is_ready(&next) && t.elapsed().as_secs() < 60 {
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(SynthWorker::is_ready(&next), "{next} never rendered");
}

#[test]
fn now_playing_names_a_synthesized_piece() {
    let mut app = TestApp::new();
    app.ctx.settings.synth_music = true;
    let mut d = a_drive(&mut app);
    d.trip.truck.start_engine();
    let text = d.radio_now_playing_text(&mut app.ctx);
    assert!(
        text.contains("Synthesized: Day Drive, number")
            || text.contains("Open Road, from Freight Fate 1.5"),
        "{text}"
    );
}

#[test]
fn original_mode_roadhouse_is_unchanged() {
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    let station = d.radio.current_station();
    assert_eq!(
        d.station_rotation_pool(&app.ctx, &station, false),
        d.day_music_sequence
    );
}

/// The twin of the no-breaks test in Original mode: the same loop does reach
/// a host break, so the synthesized test is not passing on a silent station.
#[test]
fn the_original_roadhouse_still_has_host_breaks() {
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    let station = d.radio.current_station();
    d.start_station_rotation(&mut app.ctx, &station, 0);
    let mut heard_a_break = false;
    for _ in 0..(ff_core::music::RADIO_TRACKS_PER_HOST_BREAK * 2) {
        d.radio_elapsed_s = 1.0e9;
        d.update_radio_playback(&mut app.ctx, false, 0.0);
        heard_a_break |= !d.radio_break_queue.is_empty();
    }
    assert!(heard_a_break);
}

#[test]
fn flipping_the_music_source_mid_drive_restarts_the_roadhouse() {
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    let station = d.radio.current_station();
    d.start_station_rotation(&mut app.ctx, &station, 0);
    // A seed roll in Original mode leaves the Original rotation alone.
    app.ctx.settings.music_seed += 1;
    d.apply_radio_settings_to_drive(&mut app.ctx);
    assert_eq!(d.radio_station_id, station.id);
    app.ctx.settings.synth_music = true;
    d.apply_radio_settings_to_drive(&mut app.ctx);
    assert!(d.radio_station_id.is_empty());
    let night = d.music_night;
    d.update_radio_playback(&mut app.ctx, night, 0.0);
    assert_eq!(d.radio_station_id, station.id);
    assert!(
        d.radio_playlist
            .iter()
            .all(|k| k.starts_with("synth_drive_") || k.starts_with("classic_")),
        "{:?}",
        d.radio_playlist
    );
}

#[test]
fn switching_to_original_mid_drive_restarts_a_synthesized_roadhouse() {
    // The drive's own radio start runs through the backend, which swaps the
    // radio state out while it plays: the rotation recorded Original under a
    // Synthesized playlist, so switching to Original compared equal and the
    // synthesized pieces played on (agent drive, 2026-09-23).
    let mut app = TestApp::new();
    app.ctx.settings.synth_music = true;
    let mut d = a_drive(&mut app);
    d.trip.truck.engine_on = true; // the radio runs on the engine
    d.play_radio_current(&mut app.ctx);
    let station = d.radio.current_station();
    assert_eq!(d.radio_station_id, station.id);
    app.ctx.settings.synth_music = false;
    d.apply_radio_settings_to_drive(&mut app.ctx);
    assert!(
        d.radio_station_id.is_empty(),
        "the Roadhouse kept its synthesized rotation"
    );
}

#[test]
fn switching_back_to_original_restores_the_soundtrack() {
    let mut app = TestApp::new();
    let original = ff_core::music::select_menu_music_sequence(None);
    let refs: Vec<&str> = original.iter().map(String::as_str).collect();
    app.ctx.settings.synth_music = true;
    app.ctx.play_music_sequence("menu", &refs);
    app.ctx.settings.synth_music = false;
    app.ctx.restart_music();
    assert_eq!(app.ctx.music_rotation_track(), Some(original[0].as_str()));
}

/// The smallest ProTracker MOD BASS would load: one 64-row pattern playing a
/// 32-sample square wave on channel 1, row 0. 1084 + 1024 + 32 bytes; it
/// runs 64 rows at speed 6 and 125 BPM, 7.68 seconds.
fn tiny_mod() -> Vec<u8> {
    let mut m = vec![0u8; 1084];
    m[..4].copy_from_slice(b"tiny");
    m[42..44].copy_from_slice(&16u16.to_be_bytes()); // sample 1: 16 words
    m[45] = 64; // volume
    m[48..50].copy_from_slice(&1u16.to_be_bytes()); // repeat 1 word: no loop
    m[950] = 1; // song length: 1 position
    m[951] = 127;
    m[1080..1084].copy_from_slice(b"M.K.");
    let mut pattern = vec![0u8; 1024];
    pattern[..3].copy_from_slice(&[0x01, 0xAC, 0x10]); // sample 1, C-2
    m.extend_from_slice(&pattern);
    m.extend((0..32).map(|i| if i < 16 { 0x40u8 } else { 0xC0 }));
    m
}

#[test]
fn a_tracker_module_is_refused_and_leaves_the_channel_quiet() {
    // BASS played tracker modules; Hearken has no tracker player, so a
    // module track is refused outright rather than handed to a decoder, and
    // a playlist file of one fails so the playlist skips it.
    use freight_fate::audio::Audio;
    let mut r = hearken_rig();
    ff_core::assets_pack::register_generated_sound("music/hand_made/test/tiny", tiny_mod(), "mod");
    r.engine.play_music_with("hand_made/test/tiny", 0);
    assert!(!r.engine.music_playing());
    assert_eq!(r.engine.music_length_s(), None);
    assert!(r.rec.played().is_empty(), "{:?}", r.rec.played());
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("tiny.mod");
    std::fs::write(&path, tiny_mod()).unwrap();
    let path = path.to_str().unwrap().to_string();
    let err = r.engine.play_music_file(&path).unwrap_err();
    assert_eq!(err.message, format!("could not decode {path}"));
}

/// Callers look a track's length up once, right after starting it, while
/// the previous track is still fading out: the answer must be the new one's.
#[test]
fn the_length_is_the_new_tracks_while_the_old_one_fades() {
    use freight_fate::audio::Audio;
    let mut r = hearken_rig();
    ff_core::assets_pack::register_generated_sound(
        "music/hand_made/test/two_s",
        sine_wav(2.0, 2),
        "wav",
    );
    ff_core::assets_pack::register_generated_sound(
        "music/hand_made/test/seven_s",
        sine_wav(7.68, 2),
        "wav",
    );
    // What the device reads off each clip.
    r.rec.set_length("hand_made/test/two_s", 2.0);
    r.rec.set_length("hand_made/test/seven_s", 7.68);
    r.engine.play_music_with("hand_made/test/two_s", 0);
    let a = r.engine.music_length_s().expect("wav length");
    assert!((a - 2.0).abs() < 0.01, "{a}");
    r.engine.play_music_with("hand_made/test/seven_s", 2000);
    let b = r.engine.music_length_s().expect("second length");
    assert!(
        (b - 7.68).abs() < 0.05,
        "reported {b}, not the new track's 7.68 s"
    );
}

/// The lengths the test above stubs on the recorder are the ones Hearken
/// decodes from the same bytes, with no recorder in the way.
#[test]
fn the_stubbed_track_lengths_are_the_decoded_ones() {
    for (seconds, channels) in [(2.0, 2), (7.68, 2), (3.5, 1)] {
        let info = freight_fate::audio::probe_clip(sine_wav(seconds, channels).into())
            .expect("a WAV Hearken decodes");
        let length = info.length_s.expect("a WAV knows its length");
        assert!(
            (length - seconds).abs() < 0.01,
            "decoded {length} s from a {seconds} s clip"
        );
        assert_eq!(info.sample_rate, 44_100);
    }
}

// -- the dial in Synthesized mode ---------------------------------------------------------

/// `a_drive` with the engine running, the radio on, and the given settings.
fn a_radio_drive(app: &mut TestApp, synth_music: bool, streamer_safe: bool) -> DrivingState {
    app.ctx.settings.synth_music = synth_music;
    app.ctx.settings.radio_streamer_safe = streamer_safe;
    let mut d = a_drive(app);
    d.trip.truck.start_engine();
    d.radio.enabled = true;
    d
}

fn tuned_group(d: &mut DrivingState) -> i32 {
    dial_group(&d.radio.current_station())
}

#[test]
fn synthesized_streamer_safe_leaves_every_station_key_silent() {
    let mut app = TestApp::new();
    let mut d = a_radio_drive(&mut app, true, true);
    assert_eq!(d.radio.current_station().id, SAFE_ROUTE_PLAYLIST);
    let log = app.record_audio();
    for event in [
        InputEvent::key(Key::PageDown),
        InputEvent::key(Key::PageUp),
        InputEvent::key(Key::Semicolon),
        InputEvent::key(Key::Quote),
        InputEvent::key_mods(Key::PageDown, Mods::CTRL),
        InputEvent::key_mods(Key::PageUp, Mods::CTRL),
        InputEvent::key(Key::O),
    ] {
        app.clear_speech();
        log.borrow_mut().played.clear();
        d.handle_key_event(&mut app.ctx, &event);
        assert_eq!(d.radio.current_station().id, SAFE_ROUTE_PLAYLIST);
        assert!(app.main_lines().is_empty(), "{:?}", app.main_lines());
        assert!(
            !log.borrow().played.iter().any(|(k, _, _)| k == "ui/error"),
            "{:?}",
            log.borrow().played
        );
    }
    assert!(d.radio.favorite_ids.is_empty());
    assert_eq!(d.tune_radio_to(&mut app.ctx, "afn-tokyo"), "");
    assert_eq!(d.radio.current_station().id, SAFE_ROUTE_PLAYLIST);
    // The power key is still just on and off, and volume still moves.
    d.handle_key_event(&mut app.ctx, &InputEvent::key(Key::M));
    assert!(!d.radio.enabled);
    d.handle_key_event(&mut app.ctx, &InputEvent::key(Key::M));
    assert!(d.radio.enabled);
    assert_eq!(d.radio.current_station().id, SAFE_ROUTE_PLAYLIST);
    let volume = app.ctx.settings.radio_volume;
    d.handle_key_event(
        &mut app.ctx,
        &InputEvent::key_mods(Key::PageUp, Mods::SHIFT),
    );
    assert_ne!(app.ctx.settings.radio_volume, volume);
}

#[test]
fn synthesized_streamer_safe_radio_app_has_no_station_list_to_open() {
    use crate::states_driving_menus_support as dm;
    use freight_fate::states::driving_menu_states::DriveRef;
    use freight_fate::states::driving_radio_app::{
        RadioAppState, RadioSearchEntryState, RadioStationListState,
    };
    let mut app = TestApp::new();
    app.ctx.settings.synth_music = true;
    app.ctx.settings.radio_streamer_safe = true;
    let drive = dm::a_drive_between(&mut app, "Denver", "Salt Lake City", "Radio App");
    dm::with_drive(&drive, |d| d.trip.truck.start_engine());
    let mut state = RadioAppState::new(DriveRef::of(&drive));
    let log = app.record_audio();
    for row in ["Search stations", "Stations in range", "Favorites"] {
        app.clear_speech();
        log.borrow_mut().played.clear();
        dm::activate(&mut state, &mut app.ctx, row);
        assert!(!dm::top_is::<RadioSearchEntryState>(&app), "{row}");
        assert!(!dm::top_is::<RadioStationListState>(&app), "{row}");
        assert!(app.main_lines().is_empty(), "{row}: {:?}", app.main_lines());
        assert!(
            !log.borrow().played.iter().any(|(k, _, _)| k == "ui/error"),
            "{row}: {:?}",
            log.borrow().played
        );
    }
}

#[test]
fn the_synthesized_dial_never_lands_on_a_freight_fate_station() {
    let mut app = TestApp::new();
    let mut d = a_radio_drive(&mut app, true, false);
    assert_eq!(d.radio.current_station().id, SAFE_ROUTE_PLAYLIST);
    // Every category the Ctrl key can reach, twice round.
    let categories = {
        let mut groups: Vec<i32> = Vec::new();
        for r in d.radio.receivable_stations() {
            let g = d.radio.group(&r.station);
            if !groups.contains(&g) {
                groups.push(g);
            }
        }
        groups.len()
    };
    assert!(categories > 1);
    for _ in 0..categories * 2 {
        d.handle_key_event(
            &mut app.ctx,
            &InputEvent::key_mods(Key::PageDown, Mods::CTRL),
        );
        assert_ne!(tuned_group(&mut d), 1);
    }
    // And the plain dial, stepping out of the Roadhouse in both directions.
    let _ = d.tune_radio_to(&mut app.ctx, SAFE_ROUTE_PLAYLIST);
    for _ in 0..30 {
        d.handle_key_event(&mut app.ctx, &InputEvent::key(Key::PageDown));
        assert_ne!(tuned_group(&mut d), 1);
    }
}

#[test]
fn the_original_dial_still_steps_onto_freight_fate_stations() {
    for streamer_safe in [false, true] {
        let mut app = TestApp::new();
        let mut d = a_radio_drive(&mut app, false, streamer_safe);
        assert_eq!(d.radio.current_station().id, SAFE_ROUTE_PLAYLIST);
        d.handle_key_event(&mut app.ctx, &InputEvent::key(Key::PageDown));
        assert_eq!(tuned_group(&mut d), 1);
    }
}

#[test]
fn switching_to_synthesized_moves_a_freight_fate_station_to_the_roadhouse() {
    for (streamer_safe, reason) in [
        (false, "Music source is Synthesized"),
        (true, "streamer-safe mode is on"),
    ] {
        let mut app = TestApp::new();
        let mut d = a_radio_drive(&mut app, false, false);
        d.tune_radio_to(&mut app.ctx, "ff-night-line");
        assert_eq!(d.radio.current_station().id, "ff-night-line");
        let night = d.music_night;
        d.update_radio_playback(&mut app.ctx, night, 0.0);
        app.clear_speech();
        app.ctx.settings.synth_music = true;
        app.ctx.settings.radio_streamer_safe = streamer_safe;
        d.apply_radio_settings_to_drive(&mut app.ctx);
        assert_eq!(d.radio.current_station().id, SAFE_ROUTE_PLAYLIST);
        assert_eq!(app.ctx.settings.radio_station_id, SAFE_ROUTE_PLAYLIST);
        let events = app.event_lines();
        assert!(
            events
                .iter()
                .any(|l| l.contains("left the dial") && l.contains(reason)),
            "{events:?}"
        );
        // The Roadhouse it lands on plays the synthesized rotation.
        d.update_radio_playback(&mut app.ctx, night, 0.0);
        assert!(
            d.radio_playlist
                .iter()
                .all(|k| k.starts_with("synth_drive_") || k.starts_with("classic_")),
            "{:?}",
            d.radio_playlist
        );
    }
}

/// The Tab radio screen's lines over a Denver drive with the engine running.
fn radio_screen_lines(app: &mut TestApp, synth_music: bool, streamer_safe: bool) -> Vec<String> {
    use crate::states_driving_menus_support as dm;
    use freight_fate::states::base::Menu;
    use freight_fate::states::driving_menu_states::{DriveRef, DrivingStatusScreenState};
    app.ctx.settings.synth_music = synth_music;
    app.ctx.settings.radio_streamer_safe = streamer_safe;
    let drive = dm::a_drive_between(app, "Denver", "Salt Lake City", "Radio Screen");
    dm::with_drive(&drive, |d| {
        d.trip.truck.start_engine();
        d.radio.enabled = true;
    });
    let mut state = DrivingStatusScreenState::new(DriveRef::of(&drive), "radio");
    let items = state.build_items(&mut app.ctx);
    items
        .iter()
        .map(|item| item.text(&state, &app.ctx))
        .collect()
}

#[test]
fn the_locked_radio_screen_names_only_what_still_works() {
    let mut app = TestApp::new();
    let lines = radio_screen_lines(&mut app, true, true);
    let power = app
        .ctx
        .bindings
        .spoken(freight_fate::bindings::Action::Radio);
    let help = lines
        .iter()
        .find(|l| l.starts_with("Streamer-safe mode keeps the radio on the Roadhouse."))
        .unwrap_or_else(|| panic!("no locked line: {lines:#?}"));
    assert!(help.contains("Station keys do nothing."), "{help}");
    assert!(
        help.contains(&format!("{power} turns the radio on or off")),
        "{help}"
    );
    assert!(help.contains("changes radio volume"), "{help}");
    for line in &lines {
        let lower = line.to_lowercase();
        assert!(
            !lower.contains("tune")
                && !lower.contains("categor")
                && !lower.contains("favorite")
                && !lower.contains("hidden"),
            "{line}"
        );
    }
}

#[test]
fn the_original_radio_screen_is_unchanged() {
    for (streamer_safe, safety) in [
        (
            false,
            "Streamer-safe mode off. Real public streams and personal playlists are on the dial.",
        ),
        (
            true,
            "Streamer-safe mode on. Real public streams and personal playlists are hidden.",
        ),
    ] {
        let mut app = TestApp::new();
        let lines = radio_screen_lines(&mut app, false, streamer_safe);
        assert!(lines.iter().any(|l| l == safety), "{lines:#?}");
        assert!(
            lines.iter().any(|l| l
                == "Page Down and Page Up tune stations, or semicolon and apostrophe. With \
                    Control they jump categories. With Shift they change radio volume by 10 \
                    percent. O saves the station as a favorite. M toggles the radio."),
            "{lines:#?}"
        );
        assert!(!lines
            .iter()
            .any(|l| l.contains("Streamer-safe mode keeps the radio on the Roadhouse.")));
    }
}

#[test]
fn the_radio_screen_says_where_playlists_go_until_there_is_one() {
    let mut app = TestApp::new();
    let folder = ff_core::models::profile::data_dir().join(ff_core::radio::PLAYLISTS_DIR_NAME);
    let expected = format!(
        "No playlists of your own yet. Put M3U, M3U8 or PLS playlist files in {}.",
        folder.display()
    );
    let lines = radio_screen_lines(&mut app, false, false);
    assert!(lines.iter().any(|l| l == &expected), "{lines:#?}");

    // Streamer-safe mode hides playlists, so it never points at the folder.
    let lines = radio_screen_lines(&mut app, false, true);
    assert!(
        !lines.iter().any(|l| l.starts_with("No playlists")),
        "{lines:#?}"
    );

    // One playlist on the dial and the pointer goes quiet.
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("Road Mix.m3u"), "track.ogg\n").unwrap();
    let lines = radio_screen_lines(&mut app, false, false);
    assert!(
        !lines.iter().any(|l| l.starts_with("No playlists")),
        "{lines:#?}"
    );
}

const OFF_THE_DIAL: &str =
    "Music source Synthesized: Freight Fate's own stations are off the dial.";

#[test]
fn the_synthesized_radio_screen_says_why_the_stations_are_gone() {
    let mut app = TestApp::new();
    let lines = radio_screen_lines(&mut app, true, false);
    let safety = lines
        .iter()
        .position(|l| l.starts_with("Streamer-safe mode off."))
        .unwrap_or_else(|| panic!("{lines:#?}"));
    assert_eq!(
        lines.get(safety + 1).map(String::as_str),
        Some(OFF_THE_DIAL)
    );
    drop(app);
    // Original mode never says it, with streamer-safe on or off.
    for streamer_safe in [false, true] {
        let mut app = TestApp::new();
        let lines = radio_screen_lines(&mut app, false, streamer_safe);
        assert!(!lines.iter().any(|l| l == OFF_THE_DIAL), "{lines:#?}");
    }
}

#[test]
fn a_drive_ending_under_the_menu_music_leaves_that_song_playing() {
    // Owner, 2026-10-25: the settlement screen played the menu music, and
    // quit to the main menu first stopped it for good, then (after a first
    // fix) started the song over. It carries on.
    use crate::states_driving_menus_support::{a_drive_between, drive_and_ctx};
    let mut app = TestApp::new();
    let drive = a_drive_between(&mut app, "Buffalo", "Albany", "Music Driver");
    let menu = ff_core::music::select_menu_music_sequence(None);
    let refs: Vec<&str> = menu.iter().map(String::as_str).collect();
    app.ctx.play_music_sequence("menu", &refs);
    let playing = app.ctx.music_rotation_track().map(str::to_string);
    assert!(playing.is_some());
    drive_and_ctx(&drive, &mut app, |d, ctx| d.exit_drive(ctx));
    assert_eq!(app.ctx.music_rotation_track().map(str::to_string), playing);
    // The title menu asking for the menu pool gets the same song back.
    let again = app.ctx.play_music_sequence("menu", &refs);
    assert_eq!(Some(again), playing);
}
