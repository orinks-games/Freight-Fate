//! `states/driving_updates.rs`, the radio half: engine power, the dial keys,
//! station rotation and the break slots, personal playlists, and the FM
//! fringe.
//!
//! Ported from `tests/test_radio_engine_power.py`, the drive-side cases of
//! `tests/test_radio_breaks.py` and `tests/test_music_selection.py`'s drive
//! rotation. The pure catalog/content halves of those files are already
//! covered by the `ff-core` radio and music suites.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use ff_core::data::world::get_world;
use ff_core::models::jobs::{Job, CARGO_CATALOG};
use ff_core::models::profile::Profile;
use ff_core::radio::{
    dial_group, RadioPlaybackBackend, RadioStation, PERSONAL_PLAYLIST_SOURCE_TYPE,
    SAFE_ROUTE_PLAYLIST, TERRESTRIAL_GROUP,
};
use ff_core::radio_content::content_duration_s;

use freight_fate::app::testing::TestApp;
use freight_fate::audio::{Audio, AudioError, SustainLoopSpec, VolumeUpdate};
use freight_fate::states::driving::DrivingState;
use freight_fate::states::driving_core::DRIVE_PHASE_DELIVERY;

// -- rigging -------------------------------------------------------------------------

/// `_drive_job()`: the Denver run every radio fixture in the Python suite uses.
fn a_denver_drive(app: &mut TestApp, trip_seed: i64) -> DrivingState {
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
        Some(trip_seed),
        DRIVE_PHASE_DELIVERY,
        Some(13.0),
    );
    drive.trip.set_npc_vehicles(Vec::new());
    drive
}

/// The music side of the facade: what was played, what stopped it, and the
/// music bus level `apply_radio_volume` writes.
#[derive(Default)]
struct MusicAudio {
    music: Rc<RefCell<Vec<(String, u32)>>>,
    /// Seconds into the track each `music` entry was asked to start at.
    starts: Rc<RefCell<Vec<f64>>>,
    stops: Rc<RefCell<Vec<u32>>>,
    /// Every live stream URL the cab was asked to open, in order.
    streams: Rc<RefCell<Vec<String>>>,
    /// The song title the playing stream reports, as the backend reads it.
    now_playing: Rc<RefCell<Option<String>>>,
    volume: Rc<Cell<f64>>,
    playing: Rc<Cell<bool>>,
    engine_on: Rc<Cell<bool>>,
}

#[derive(Clone, Default)]
struct MusicTape {
    music: Rc<RefCell<Vec<(String, u32)>>>,
    starts: Rc<RefCell<Vec<f64>>>,
    stops: Rc<RefCell<Vec<u32>>>,
    streams: Rc<RefCell<Vec<String>>>,
    now_playing: Rc<RefCell<Option<String>>>,
}

impl MusicTape {
    /// The live stream the cab was last asked to open.
    fn last_stream(&self) -> Option<String> {
        self.streams.borrow().last().cloned()
    }

    /// What the stream is broadcasting as its current song.
    fn set_now_playing(&self, title: &str) {
        *self.now_playing.borrow_mut() = Some(title.to_string());
    }

    fn tracks(&self) -> Vec<String> {
        self.music
            .borrow()
            .iter()
            .map(|(track, _)| track.clone())
            .collect()
    }

    fn last(&self) -> (String, u32) {
        self.music.borrow().last().cloned().expect("music played")
    }

    fn is_empty(&self) -> bool {
        self.music.borrow().is_empty()
    }

    /// How far into the last track playback was asked to start.
    fn last_start(&self) -> f64 {
        self.starts.borrow().last().copied().expect("music played")
    }

    fn clear_stops(&self) {
        self.stops.borrow_mut().clear();
    }

    fn stopped(&self) -> Vec<u32> {
        self.stops.borrow().clone()
    }
}

impl MusicAudio {
    fn install(app: &mut TestApp) -> MusicTape {
        let audio = MusicAudio::default();
        let tape = MusicTape {
            music: Rc::clone(&audio.music),
            starts: Rc::clone(&audio.starts),
            stops: Rc::clone(&audio.stops),
            streams: Rc::clone(&audio.streams),
            now_playing: Rc::clone(&audio.now_playing),
        };
        app.ctx.audio = Box::new(audio);
        tape
    }
}

impl Audio for MusicAudio {
    fn enabled(&self) -> bool {
        false
    }
    fn backend_name(&self) -> &str {
        "music-test"
    }
    fn master_volume(&self) -> f64 {
        1.0
    }
    fn sfx_volume(&self) -> f64 {
        1.0
    }
    fn music_volume(&self) -> f64 {
        self.volume.get()
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
        self.engine_on.get()
    }
    fn engine_starting(&self) -> bool {
        false
    }
    fn voice_key(&self, key: &str) -> String {
        key.to_string()
    }
    fn play_with(&mut self, _key: &str, _volume: f64, _pan: f64) {}
    fn play_bank_with(&mut self, _base: &str, _fallback: &str, _volume: f64, _pan: f64) {}
    fn set_engine_duck(&mut self, _duck: f64) {}
    fn set_speech_duck(&mut self, _duck: f64) {}
    fn set_engine_voice(&mut self, _classic: bool) {}
    fn set_jake_voice(&mut self, _classic: bool) {}
    fn has_asset(&mut self, _key: &str) -> bool {
        true
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
    fn hold_alert_with(&mut self, _key: &str, _volume: f64, _fade_ms: u32) {}
    fn release_alert_with(&mut self, _fade_ms: u32) {}
    fn hold_cue(&mut self, _name: &str) {}
    fn cue_held(&self, _name: &str) -> bool {
        false
    }
    fn release_cue(&mut self, _name: &str) {}
    fn engine_start_with(&mut self, _play_start_sound: bool) {
        self.engine_on.set(true);
    }
    fn engine_stop_with(&mut self, _shutdown_sound: bool) {
        self.engine_on.set(false);
    }
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
    fn play_music_with(&mut self, track: &str, fade_ms: u32) {
        self.play_music_at(track, fade_ms, 0.0);
    }
    fn play_music_at(&mut self, track: &str, fade_ms: u32, start_s: f64) {
        self.music.borrow_mut().push((track.to_string(), fade_ms));
        self.starts.borrow_mut().push(start_s);
        self.playing.set(true);
    }
    fn play_radio_stream_with(&mut self, url: &str, _fade_ms: u32) -> Result<(), AudioError> {
        self.streams.borrow_mut().push(url.to_string());
        self.playing.set(true);
        Ok(())
    }
    fn play_music_file_with(&mut self, path: &str, fade_ms: u32) -> Result<(), AudioError> {
        // A personal playlist entry that will not open is how the skip and
        // the "nothing would play" line are exercised.
        if path.contains("missing") {
            return Err(AudioError::new("no such file"));
        }
        self.music.borrow_mut().push((path.to_string(), fade_ms));
        self.playing.set(true);
        Ok(())
    }
    fn music_playing(&self) -> bool {
        self.playing.get()
    }
    fn radio_now_playing(&self) -> Option<String> {
        self.now_playing.borrow().clone()
    }
    fn stop_music_with(&mut self, fade_ms: u32) {
        self.stops.borrow_mut().push(fade_ms);
        self.playing.set(false);
    }
    fn set_volumes(&mut self, volumes: &VolumeUpdate) {
        if let Some(music) = volumes.music {
            self.volume.set(music);
        }
    }
    fn shutdown(&mut self) {}
}

/// The last thing the main channel said.
fn last(app: &TestApp) -> String {
    app.main_lines().last().cloned().unwrap_or_default()
}

// -- engine power (test_radio_engine_power.py) ----------------------------------------

#[test]
fn test_radio_is_silent_until_the_engine_starts() {
    let mut app = TestApp::new();
    let mut d = a_denver_drive(&mut app, 777);
    let tape = MusicAudio::install(&mut app);
    // The top of every load: radio enabled, engine off, and nothing plays.
    assert!(d.radio.enabled);
    assert!(tape.is_empty());

    d.trip.truck.start_engine();
    d.update_audio(&mut app.ctx, 0.0);

    assert!(
        !tape.is_empty(),
        "the radio comes back on its own with the engine"
    );
}

#[test]
fn test_engine_shutdown_cuts_the_radio() {
    let mut app = TestApp::new();
    let mut d = a_denver_drive(&mut app, 777);
    let tape = MusicAudio::install(&mut app);
    d.trip.truck.start_engine();
    d.update_audio(&mut app.ctx, 0.0);
    assert!(!tape.is_empty());
    tape.clear_stops();

    d.trip.truck.stop_engine();
    d.update_audio(&mut app.ctx, 0.0);

    assert!(
        !tape.stopped().is_empty(),
        "the radio loses power with the engine"
    );
}

#[test]
fn test_leaving_the_cab_mutes_the_radio() {
    // The cab radio is for the cab. Walking to the terminal -- any
    // exit of the drive -- must fade it, or the last station keeps
    // playing over menus that are not the wheel.
    let mut app = TestApp::new();
    let mut d = a_denver_drive(&mut app, 777);
    let tape = MusicAudio::install(&mut app);
    d.trip.truck.start_engine();
    d.update_audio(&mut app.ctx, 0.0);
    assert!(!tape.is_empty());
    tape.clear_stops();

    d.exit_drive(&mut app.ctx);

    assert!(
        tape.stopped().contains(&600),
        "leaving the cab must fade the radio, got {:?}",
        tape.stopped()
    );
}

#[test]
fn test_radio_keys_speak_the_no_power_line_with_the_engine_off() {
    let mut app = TestApp::new();
    let mut d = a_denver_drive(&mut app, 777);
    let tape = MusicAudio::install(&mut app);
    app.clear_speech();

    d.toggle_radio(&mut app.ctx);
    d.tune_radio(&mut app.ctx, 1);
    d.jump_radio_category(&mut app.ctx, 1);

    assert!(tape.is_empty());
    assert_eq!(
        app.main_lines()
            .iter()
            .filter(|line| *line == "The engine is off. The radio has no power.")
            .count(),
        3
    );
    // The player's wish is untouched: the radio is still on for ignition.
    assert!(d.radio.enabled);

    // The status key answers, but explains the silence.
    d.speak_radio_status(&mut app.ctx);
    let said = last(&app);
    assert!(said.starts_with("Radio on."));
    assert!(said.ends_with("The engine is off. The radio has no power."));
}

#[test]
fn test_shift_page_up_raises_radio_volume_ten_percent() {
    let mut app = TestApp::new();
    let mut d = a_denver_drive(&mut app, 777);
    MusicAudio::install(&mut app);
    app.ctx.settings.radio_volume = 0.25;
    app.clear_speech();

    d.adjust_radio_volume(&mut app.ctx, 1);

    assert!((app.ctx.settings.radio_volume - 0.35).abs() < 1e-9);
    assert_eq!(last(&app), "Radio volume 35 percent.");
}

#[test]
fn test_shift_page_down_lowers_radio_volume_ten_percent() {
    let mut app = TestApp::new();
    let mut d = a_denver_drive(&mut app, 777);
    MusicAudio::install(&mut app);
    app.ctx.settings.radio_volume = 0.35;
    app.clear_speech();

    d.adjust_radio_volume(&mut app.ctx, -1);

    assert!((app.ctx.settings.radio_volume - 0.25).abs() < 1e-9);
    assert_eq!(last(&app), "Radio volume 25 percent.");
}

#[test]
fn test_shift_page_down_clamps_at_muted() {
    let mut app = TestApp::new();
    let mut d = a_denver_drive(&mut app, 777);
    MusicAudio::install(&mut app);
    app.ctx.settings.radio_volume = 0.05;
    app.clear_speech();

    d.adjust_radio_volume(&mut app.ctx, -1);
    assert_eq!(app.ctx.settings.radio_volume, 0.0);
    assert_eq!(last(&app), "Radio volume muted.");

    // A second press at the floor stays put, not negative.
    d.adjust_radio_volume(&mut app.ctx, -1);
    assert_eq!(app.ctx.settings.radio_volume, 0.0);
    assert_eq!(last(&app), "Radio volume muted.");
}

#[test]
fn test_shift_page_up_clamps_at_all_the_way_up() {
    let mut app = TestApp::new();
    let mut d = a_denver_drive(&mut app, 777);
    MusicAudio::install(&mut app);
    app.ctx.settings.radio_volume = 0.95;
    app.clear_speech();

    d.adjust_radio_volume(&mut app.ctx, 1);
    assert_eq!(app.ctx.settings.radio_volume, 1.0);
    assert_eq!(last(&app), "Radio volume all the way up.");

    // A second press at the ceiling stays put, not over 100.
    d.adjust_radio_volume(&mut app.ctx, 1);
    assert_eq!(app.ctx.settings.radio_volume, 1.0);
    assert_eq!(last(&app), "Radio volume all the way up.");
}

#[test]
fn test_shift_volume_works_with_the_engine_off_and_radio_off() {
    // The setting is what it is regardless of power state: no "engine is off"
    // line, unlike the plain tune and category keys.
    let mut app = TestApp::new();
    let mut d = a_denver_drive(&mut app, 777);
    MusicAudio::install(&mut app);
    assert!(!d.trip.truck.engine_on);
    d.radio.enabled = false;
    app.ctx.settings.radio_volume = 0.25;
    app.clear_speech();

    d.adjust_radio_volume(&mut app.ctx, 1);

    assert!((app.ctx.settings.radio_volume - 0.35).abs() < 1e-9);
    assert_eq!(last(&app), "Radio volume 35 percent.");
    assert!(!last(&app).to_lowercase().contains("no power"));
}

#[test]
fn test_shift_volume_applies_live_while_the_radio_plays() {
    let mut app = TestApp::new();
    let mut d = a_denver_drive(&mut app, 777);
    let tape = MusicAudio::install(&mut app);
    d.trip.truck.start_engine();
    d.update_audio(&mut app.ctx, 0.0);
    assert!(!tape.is_empty());
    app.ctx.settings.radio_volume = 0.25;

    d.adjust_radio_volume(&mut app.ctx, 1);

    assert!((app.ctx.audio.music_volume() - 0.35).abs() < 1e-9);
}

// -- station rotation and break slots (test_radio_breaks.py) ---------------------------

/// Put one built-in station on the dial and tune to it.
fn tune_to_fixture(d: &mut DrivingState, app: &mut TestApp, id: &str, host: &str) {
    let station = RadioStation {
        playlist: "country".to_string(),
        host: host.to_string(),
        ..RadioStation::new(id, "Fixture", "KFX", "country", "test fixture")
    };
    let mut catalog = d.radio.catalog.clone();
    catalog.push(station);
    d.radio.set_catalog(catalog);
    let id = id.to_string();
    d.with_radio_backend(app_ctx(app), |radio, backend| {
        radio.select_station(&id, Some(backend))
    });
}

fn app_ctx(app: &mut TestApp) -> &mut freight_fate::app::GameContext {
    &mut app.ctx
}

/// `_play_next(driving, played)`: run the current entry out.
fn play_next(d: &mut DrivingState, app: &mut TestApp, tape: &MusicTape) {
    let dt = content_duration_s(&tape.last().0) + 0.1;
    d.update_radio_playback(&mut app.ctx, false, dt);
}

#[test]
fn test_break_queue_delivers_host_id_ad_slots_in_order() {
    // The Python case swapped fixture pools in by monkeypatching the module
    // tables; the Rust tables are `static`, so this runs on the shipped
    // Roadhouse pools instead and asserts the SHAPE of the pattern -- two
    // songs, a break, back to music, at the same fades.
    let mut app = TestApp::new();
    let mut d = a_denver_drive(&mut app, 42);
    let tape = MusicAudio::install(&mut app);
    // Sign the station on at the top of its order: a drive normally opens
    // with the stations already part way through theirs, which is a
    // different case (below) from the shape of the break pattern.
    d.radio_airtime_s = 0.0;
    tune_to_fixture(&mut d, &mut app, "brk-fixture", "roadhouse");
    assert!(tape.last().0.starts_with("radio_country_"));
    assert_eq!(tape.last_start(), 0.0);

    play_next(&mut d, &mut app, &tape); // song 2
    assert!(tape.last().0.starts_with("radio_country_"));
    play_next(&mut d, &mut app, &tape); // after 2 songs: a host break
    let (track, fade) = tape.last();
    assert!(track.starts_with("host_roadhouse_"));
    assert_eq!(fade, 600); // fade into a break

    play_next(&mut d, &mut app, &tape); // break ends, music resumes (song 3)
    let (track, fade) = tape.last();
    assert!(track.starts_with("radio_country_"));
    assert_eq!(fade, 1200); // fade back to music

    play_next(&mut d, &mut app, &tape); // song 4
    play_next(&mut d, &mut app, &tape); // after 4 songs: an id break
    let (track, fade) = tape.last();
    assert!(track.starts_with("id_") || track.starts_with("host_"));
    assert_eq!(fade, 600);
}

#[test]
fn test_no_host_station_chains_songs_without_break() {
    let mut app = TestApp::new();
    let mut d = a_denver_drive(&mut app, 42);
    let tape = MusicAudio::install(&mut app);
    tune_to_fixture(&mut d, &mut app, "brk-nohost", "");
    assert!(tape.last().0.starts_with("radio_country_"));

    for _ in 0..6 {
        play_next(&mut d, &mut app, &tape);
        assert!(tape.last().0.starts_with("radio_country_"));
    }
    assert!(d.radio_break_queue.is_empty());
}

// -- tuning back to a station that kept playing (issue #158) ---------------------------

/// Two fixture stations on the dial, so a drive can flip between them.
fn two_fixture_stations(d: &mut DrivingState) {
    let mut catalog = d.radio.catalog.clone();
    for id in ["keep-a", "keep-b"] {
        catalog.push(RadioStation {
            playlist: "country".to_string(),
            host: "roadhouse".to_string(),
            ..RadioStation::new(id, "Fixture", "KFX", "country", "test fixture")
        });
    }
    d.radio.set_catalog(catalog);
}

fn tune(d: &mut DrivingState, app: &mut TestApp, id: &str) {
    let id = id.to_string();
    d.with_radio_backend(app_ctx(app), |radio, backend| {
        radio.select_station(&id, Some(backend))
    });
}

#[test]
fn test_a_station_keeps_playing_while_you_are_tuned_away() {
    // Issue #158: every tune back to a station restarted its running order at
    // the first song, second zero, so a driver moving around the dial heard
    // the same opening over and over.
    let mut app = TestApp::new();
    let mut d = a_denver_drive(&mut app, 42);
    let tape = MusicAudio::install(&mut app);
    two_fixture_stations(&mut d);

    tune(&mut d, &mut app, "keep-a");
    let (first_track, _) = tape.last();
    let first_start = tape.last_start();
    let order = d.radio_playlist.clone();
    assert!(!order.is_empty());

    // Five minutes of driving with the dial parked somewhere else.
    tune(&mut d, &mut app, "keep-b");
    for _ in 0..300 {
        d.update_audio(&mut app.ctx, 1.0);
    }

    tune(&mut d, &mut app, "keep-a");
    assert_eq!(
        d.radio_playlist, order,
        "the station keeps the running order it had"
    );
    let (again, _) = tape.last();
    let again_start = tape.last_start();
    assert!(
        again != first_track || again_start > first_start,
        "back on {first_track} at {first_start} again: the station restarted"
    );
    // The station moved on while nobody in this cab was listening.
    assert!(d.radio_track_index > 0 || d.radio_break_count > 0);
}

#[test]
fn test_tuning_in_lands_part_way_through_whatever_is_playing() {
    // A real station is already mid-song when you find it.
    let mut app = TestApp::new();
    let mut d = a_denver_drive(&mut app, 42);
    let tape = MusicAudio::install(&mut app);
    two_fixture_stations(&mut d);

    tune(&mut d, &mut app, "keep-a");

    let (track, _) = tape.last();
    let start = tape.last_start();
    if track.starts_with("radio_") {
        assert!(start > 0.0, "{track} started at the very top");
        assert!(start < content_duration_s(&track));
        // The playback loop believes the same thing, so the song ends when it
        // would have ended on the air rather than running long.
        assert!((d.radio_elapsed_s - start).abs() < 1e-9);
    } else {
        // A spoken break plays whole rather than being cut into mid-word.
        assert_eq!(start, 0.0);
        assert_eq!(d.radio_elapsed_s, 0.0);
    }
}

#[test]
fn test_two_stations_have_distinct_running_orders_and_playback_positions() {
    let mut app = TestApp::new();
    let mut d = a_denver_drive(&mut app, 42);
    let tape = MusicAudio::install(&mut app);
    two_fixture_stations(&mut d);

    tune(&mut d, &mut app, "keep-a");
    let a = (tape.last().0, tape.last_start());
    let a_order = d.radio_playlist.clone();
    tune(&mut d, &mut app, "keep-b");
    let b = (tape.last().0, tape.last_start());

    // Stations sharing a genre can legitimately reach the same song at
    // different offsets as the catalog grows. They must not share a timeline.
    assert_ne!(a_order, d.radio_playlist);
    assert_ne!(a, b);
}

#[test]
fn test_the_same_trip_always_finds_a_station_in_the_same_place() {
    // The head start is drawn from the trip seed, never from a clock.
    fn opening(seed: i64) -> (String, f64) {
        let mut app = TestApp::new();
        let mut d = a_denver_drive(&mut app, seed);
        let tape = MusicAudio::install(&mut app);
        two_fixture_stations(&mut d);
        tune(&mut d, &mut app, "keep-a");
        (tape.last().0, tape.last_start())
    }
    assert_eq!(opening(42), opening(42));
    assert_ne!(opening(42), opening(43));
}

#[test]
fn test_a_personal_playlist_still_starts_its_tracks_at_the_top() {
    // The player's own music is not a broadcast: it resumes on the entry it
    // left off on, and never cuts into the middle of a track they chose.
    let mut app = TestApp::new();
    let mut d = a_denver_drive(&mut app, 42);
    let tape = MusicAudio::install(&mut app);
    let station = a_playlist_station("pl-keep", &["C:/music/one.ogg", "C:/music/two.ogg"]);

    d.start_playlist_station(&mut app.ctx, &station, 900, false);
    assert_eq!(d.playlist_entry(&station), "C:/music/one.ogg");
    d.start_playlist_station(&mut app.ctx, &station, 900, true);
    assert_eq!(d.playlist_entry(&station), "C:/music/two.ogg");
    // Tuning back resumes there rather than at the first entry.
    d.radio_station_id = "somewhere-else".to_string();
    d.update_playlist_playback(&mut app.ctx, &station, 0.5);
    assert_eq!(d.playlist_entry(&station), "C:/music/two.ogg");
    assert!(tape.tracks().iter().all(|t| t.starts_with("C:/music/")));
}

#[test]
fn test_shuffle_plays_every_track_once_before_any_repeats() {
    // Hailey (drivers board, 2026-09-18): the Playlists folder is being used
    // for MP3 collections, and those want a shuffle. A lap plays every entry
    // once in a seeded random order, the next lap is a different order that
    // never opens on the track that just ended, and turning shuffle off
    // resumes top-to-bottom from wherever the playlist is.
    let mut app = TestApp::new();
    let mut d = a_denver_drive(&mut app, 42);
    app.ctx.settings.radio_shuffle_playlists = true;
    let tape = MusicAudio::install(&mut app);
    let entries = [
        "C:/music/a.mp3",
        "C:/music/b.mp3",
        "C:/music/c.mp3",
        "C:/music/d.mp3",
        "C:/music/e.mp3",
        "C:/music/f.mp3",
    ];
    let station = a_playlist_station("pl-shuffle", &entries);

    d.start_playlist_station(&mut app.ctx, &station, 900, false);
    for _ in 0..5 {
        d.start_playlist_station(&mut app.ctx, &station, 900, true);
    }
    let first_lap = tape.tracks();
    assert_eq!(first_lap.len(), 6);
    let mut sorted = first_lap.clone();
    sorted.sort();
    assert_eq!(
        sorted,
        entries.map(str::to_string).to_vec(),
        "a lap is every track once"
    );
    // The seed is fixed, so this pins that the order is not the file's own.
    assert_ne!(first_lap, entries.map(str::to_string).to_vec());

    for _ in 0..6 {
        d.start_playlist_station(&mut app.ctx, &station, 900, true);
    }
    let all = tape.tracks();
    let second_lap = &all[6..12];
    let mut sorted = second_lap.to_vec();
    sorted.sort();
    assert_eq!(sorted, entries.map(str::to_string).to_vec());
    assert_ne!(
        second_lap[0], first_lap[5],
        "a new lap never repeats the last track"
    );
    assert_ne!(second_lap.to_vec(), first_lap, "each lap is its own order");

    // Shuffle off: the next advance is the file's next line after the
    // current entry, top to bottom.
    app.ctx.settings.radio_shuffle_playlists = false;
    let current = d.playlist_entry(&station);
    let at = entries.iter().position(|e| *e == current).unwrap();
    d.start_playlist_station(&mut app.ctx, &station, 900, true);
    assert_eq!(
        d.playlist_entry(&station),
        entries[(at + 1) % entries.len()]
    );
}

#[test]
fn test_the_route_station_swaps_its_pool_at_nightfall() {
    // `_station_rotation_pool`: the route playlist is the drive's own
    // day/night sequence, and the rotation restarts when the flag flips.
    let mut app = TestApp::new();
    let mut d = a_denver_drive(&mut app, 42);
    let route_station = RadioStation {
        playlist: "route".to_string(),
        ..RadioStation::new("route-fixture", "Route", "", "mixed", "test fixture")
    };
    let day = d.station_rotation_pool(&app.ctx, &route_station, false);
    let night = d.station_rotation_pool(&app.ctx, &route_station, true);
    assert_eq!(day, d.day_music_sequence);
    assert_eq!(night, d.night_music_sequence);
    assert_ne!(day, night);
    let _ = &mut d;
}

// -- personal playlists ---------------------------------------------------------------

fn a_playlist_station(id: &str, entries: &[&str]) -> RadioStation {
    RadioStation {
        source_type: PERSONAL_PLAYLIST_SOURCE_TYPE.to_string(),
        playlist_entries: entries.iter().map(|entry| entry.to_string()).collect(),
        ..RadioStation::new(id, "My Playlist", "", "playlist", "personal")
    }
}

#[test]
fn test_a_playlist_entry_that_will_not_open_is_skipped_not_pruned() {
    // A NAS that was asleep when the drive started must not erase the tracks
    // behind it: entries are skipped at play time.
    let mut app = TestApp::new();
    let mut d = a_denver_drive(&mut app, 5);
    let tape = MusicAudio::install(&mut app);
    let station = a_playlist_station("pl-1", &["C:/music/missing.ogg", "C:/music/good.ogg"]);
    d.start_playlist_station(&mut app.ctx, &station, 900, false);
    assert_eq!(tape.tracks(), vec!["C:/music/good.ogg".to_string()]);
    // The dead entry is still in the playlist, just not the one playing.
    assert_eq!(station.playlist_entries.len(), 2);
    assert_eq!(d.playlist_entry(&station), "C:/music/good.ogg");
}

#[test]
fn test_a_dead_playlist_reaches_the_dials_fallback() {
    // `RadioState.play` only marks a station unplayable and speaks the
    // handover when the backend REFUSES. Python raised RadioPlaybackError
    // straight through `play_station`; a Rust wrapper that swallowed it left
    // the player parked on a silent station with nothing said about it.
    let mut app = TestApp::new();
    let mut d = a_denver_drive(&mut app, 5);
    let _tape = MusicAudio::install(&mut app);
    let station = a_playlist_station("pl-dead-refuses", &["C:/music/missing-one.ogg"]);
    let refused = d.with_radio_backend(&mut app.ctx, |_radio, backend| {
        backend.play_station(&station, 1.0)
    });
    assert!(
        refused.is_err(),
        "a playlist with nothing playable must refuse so the dial can fall back"
    );

    // A playlist that does open is still accepted.
    let good = a_playlist_station("pl-good", &["C:/music/good.ogg"]);
    let played = d.with_radio_backend(&mut app.ctx, |_radio, backend| {
        backend.play_station(&good, 1.0)
    });
    assert!(played.is_ok());
}

#[test]
fn test_a_playlist_with_nothing_playable_says_so_once() {
    let mut app = TestApp::new();
    let mut d = a_denver_drive(&mut app, 5);
    let tape = MusicAudio::install(&mut app);
    let station = a_playlist_station(
        "pl-dead",
        &["C:/music/missing-a.ogg", "C:/music/missing-b.ogg"],
    );
    app.clear_speech();
    d.playlist_nothing_plays(&mut app.ctx, &station);
    let said = app.event_lines();
    assert_eq!(said.len(), 1);
    assert!(said[0].contains("Check the tracks in your Playlists folder."));
    // Once per station until something in it plays again.
    d.playlist_nothing_plays(&mut app.ctx, &station);
    assert_eq!(app.event_lines().len(), 1);
    assert!(!tape.stopped().is_empty());
}

/// A playlist station tuned in, engine running, with the entry on the air.
fn tune_playlist(
    app: &mut TestApp,
    d: &mut DrivingState,
    id: &str,
    entries: &[&str],
) -> RadioStation {
    app.ctx.settings.radio_streamer_safe = false; // personal media rides that gate
    d.trip.truck.start_engine();
    let station = a_playlist_station(id, entries);
    let mut catalog = d.radio.catalog.clone();
    catalog.push(station.clone());
    d.radio.set_catalog(catalog);
    tune(d, app, id);
    station
}

#[test]
fn test_a_playlist_station_on_a_stream_entry_reads_out_the_song() {
    // A playlist exported from an internet radio app is nothing but
    // stations, and every one of them publishes its song titles the way the
    // curated streams do. The readout answered for all of them that the
    // station sends no song information at all.
    let mut app = TestApp::new();
    let mut d = a_denver_drive(&mut app, 5);
    let tape = MusicAudio::install(&mut app);
    tape.set_now_playing("Waylon Jennings - Lonesome, On'ry and Mean");
    tune_playlist(&mut app, &mut d, "pl-streams", &["https://radio.test/kxyz"]);

    assert_eq!(
        d.radio_now_playing_text(&mut app.ctx),
        "Now playing on My Playlist: Waylon Jennings - Lonesome, On'ry and Mean."
    );
}

#[test]
fn test_a_playlist_stream_song_survives_into_the_reception_tick() {
    // The tick's copy is what the tablet and the arrival readout use when
    // the channel is busy, so it has to be filled for a playlist stream too.
    let mut app = TestApp::new();
    let mut d = a_denver_drive(&mut app, 5);
    let tape = MusicAudio::install(&mut app);
    tape.set_now_playing("Jerry Reed - East Bound and Down");
    tune_playlist(&mut app, &mut d, "pl-tick", &["https://radio.test/kxyz"]);

    radio_frame(&mut d, &mut app);

    assert_eq!(
        d.radio_now_playing.as_deref(),
        Some("Jerry Reed - East Bound and Down")
    );
}

#[test]
fn test_a_playlist_station_on_a_file_entry_still_sends_no_song_information() {
    // A file off the player's own disk is not a broadcast: nothing is
    // publishing a title, so the honest answer is the one it always gave.
    let mut app = TestApp::new();
    let mut d = a_denver_drive(&mut app, 5);
    let tape = MusicAudio::install(&mut app);
    tape.set_now_playing("whatever the channel happens to hold");
    tune_playlist(&mut app, &mut d, "pl-files", &["C:/music/good.ogg"]);

    assert_eq!(
        d.radio_now_playing_text(&mut app.ctx),
        "My Playlist does not send song information."
    );
}

// -- driving out of range -------------------------------------------------------------

/// A live stream sited on Denver's yard with a short contour, so the Denver
/// run drives out of it well inside the first leg.
fn a_ranged_stream(id: &str) -> RadioStation {
    RadioStation {
        lat: Some(39.7392),
        lon: Some(-104.9903),
        range_miles: 20.0,
        real_stream: true,
        stream_url: format!("https://radio.test/{id}"),
        ..RadioStation::new(
            id,
            "Mile High 91.5",
            "KMHF",
            "college variety",
            "test fixture",
        )
    }
}

/// A regional station on the same mast with a contour that still covers the
/// truck once the short one has dropped away.
fn a_regional_stream(id: &str) -> RadioStation {
    RadioStation {
        lat: Some(39.7392),
        lon: Some(-104.9903),
        range_miles: 75.0,
        real_stream: true,
        stream_url: format!("https://radio.test/{id}"),
        source_type: "regional".to_string(),
        ..RadioStation::new(id, "Front Range Country", "KTST", "country", "test fixture")
    }
}

/// One drive frame's worth of radio work, in the order the frame runs it:
/// the settings sync first, then the reception tick.
fn radio_frame(d: &mut DrivingState, app: &mut TestApp) {
    d.sync_radio_settings(&mut app.ctx);
    d.update_radio_reception(&mut app.ctx, 1.5);
}

#[test]
fn test_driving_out_of_a_stations_range_says_so_and_retunes_the_cab() {
    // Owner, Willmar to Owatonna, 2026-09-16: KVSC thinned out on I-35, then
    // came back at full volume with nothing said, and the drivers board had
    // him "listening to AFN Humphreys The Eagle" while the cab still played
    // KVSC. The per-frame settings sync had already re-pointed the dial at
    // the new position, so the reception tick compared the Eagle with itself:
    // no line, no static, and the dead station's stream left running at the
    // fallback's full volume.
    let mut app = TestApp::new();
    app.ctx.settings.radio_streamer_safe = false; // the Eagle is the audible fallback
    let mut d = a_denver_drive(&mut app, 916);
    let tape = MusicAudio::install(&mut app);
    d.trip.truck.start_engine();
    d.update_audio(&mut app.ctx, 0.0);
    let mut catalog = d.radio.catalog.clone();
    // Nothing else on the air out here: with another local station in range
    // the dial goes there instead (the next test).
    catalog.retain(|station| dial_group(station) != TERRESTRIAL_GROUP);
    catalog.push(a_ranged_stream("kmhf-denver"));
    d.radio.set_catalog(catalog);
    tune(&mut d, &mut app, "kmhf-denver");
    radio_frame(&mut d, &mut app);
    assert_eq!(d.radio.tuned_station().id, "kmhf-denver");
    assert_eq!(
        tape.last_stream().as_deref(),
        Some("https://radio.test/kmhf-denver")
    );
    app.clear_speech();

    // Past the contour.
    let tracks_before = tape.tracks().len();
    d.trip.position_mi = 120.0;
    radio_frame(&mut d, &mut app);

    // The announced landing is the Roadhouse, receivable everywhere. The
    // Eagle is only ever where the dial's silent resolution lands, which is
    // exactly what the drivers board used to read out.
    let on_air = d.radio.tuned_station();
    assert_eq!(on_air.id, SAFE_ROUTE_PLAYLIST);
    let said = app.event_lines();
    assert!(
        said.iter()
            .any(|line| line.contains("Mile High 91.5 faded out of range. Falling back to FFR")),
        "{said:?}"
    );
    assert!(
        tape.tracks().len() > tracks_before,
        "the cab retunes to what the dial says"
    );
    let presence = d
        .online_presence_state(&app.ctx)
        .expect("a drivers-board line");
    assert!(
        presence
            .detail
            .contains("listening to FFR, Freight Fate Roadhouse"),
        "{}",
        presence.detail
    );
}

#[test]
fn test_a_station_fading_out_hands_the_dial_to_the_strongest_local_signal() {
    // Brandon, 2026-09-17: listening to local radio and driving out of range
    // sent the dial back to the Freight Fate stations. A driver on local
    // radio wants the next station the truck can hear.
    let mut app = TestApp::new();
    app.ctx.settings.radio_streamer_safe = false;
    let mut d = a_denver_drive(&mut app, 916);
    let tape = MusicAudio::install(&mut app);
    d.trip.truck.start_engine();
    d.update_audio(&mut app.ctx, 0.0);
    let mut catalog = d.radio.catalog.clone();
    catalog.retain(|station| dial_group(station) != TERRESTRIAL_GROUP);
    catalog.push(a_ranged_stream("kmhf-denver"));
    catalog.push(a_regional_stream("ktst-denver"));
    d.radio.set_catalog(catalog);
    tune(&mut d, &mut app, "kmhf-denver");
    radio_frame(&mut d, &mut app);
    assert_eq!(d.radio.tuned_station().id, "kmhf-denver");
    app.clear_speech();

    // Past the short contour, still inside the regional one.
    d.trip.position_mi = 120.0;
    radio_frame(&mut d, &mut app);

    assert_eq!(d.radio.tuned_station().id, "ktst-denver");
    assert_eq!(
        tape.last_stream().as_deref(),
        Some("https://radio.test/ktst-denver"),
        "the cab plays what the dial says"
    );
    let said = app.event_lines();
    assert!(
        said.iter().any(|line| line.contains(
            "Mile High 91.5 faded out of range. Tuned to KTST, Front Range Country, the \
             strongest signal here."
        )),
        "{said:?}"
    );
    // One fade, one line: the landing station is in range, so the next tick
    // has nothing to say.
    app.clear_speech();
    radio_frame(&mut d, &mut app);
    assert!(app.event_lines().is_empty(), "{:?}", app.event_lines());
}

#[test]
fn test_a_favorite_plays_on_past_its_range() {
    // Orinks, 2026-10-12: a station the driver saved stays with them from
    // anywhere, with no fade, no static and no handover.
    let mut app = TestApp::new();
    app.ctx.settings.radio_streamer_safe = false;
    let mut d = a_denver_drive(&mut app, 916);
    let tape = MusicAudio::install(&mut app);
    d.trip.truck.start_engine();
    d.update_audio(&mut app.ctx, 0.0);
    let mut catalog = d.radio.catalog.clone();
    catalog.retain(|station| dial_group(station) != TERRESTRIAL_GROUP);
    catalog.push(a_ranged_stream("kmhf-denver"));
    catalog.push(a_regional_stream("ktst-denver"));
    d.radio.set_catalog(catalog);
    tune(&mut d, &mut app, "kmhf-denver");
    radio_frame(&mut d, &mut app);
    d.radio.toggle_favorite();
    app.clear_speech();

    d.trip.position_mi = 120.0;
    radio_frame(&mut d, &mut app);

    assert_eq!(d.radio.tuned_station().id, "kmhf-denver");
    assert_eq!(
        tape.last_stream().as_deref(),
        Some("https://radio.test/kmhf-denver")
    );
    assert_eq!(d.radio_signal_factor, 1.0);
    assert!(app.event_lines().is_empty(), "{:?}", app.event_lines());
}
