use super::*;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use hearken::audio::{Call, Recorder, RecordingBackend};

use crate::audio::CH_WEATHER;

/// A facade over a Hearken backend whose device is still opening, the
/// recorder the device will be, and the switch that lets it open.
fn engine_with_pending_open() -> (AudioEngine, Recorder, mpsc::Sender<()>) {
    let (backend, rec) = RecordingBackend::new();
    let (go, wait) = mpsc::channel::<()>();
    let hearken = HearkenBackend::deferred_with(move || {
        let _ = wait.recv();
        Ok(Box::new(backend) as Box<dyn hearken::audio::Backend>)
    });
    let engine = AudioEngine::with_backend(Box::new(hearken));
    assert!(engine.hearken().unwrap().opening());
    (engine, rec, go)
}

/// Frames pass until the device is in (or has failed).
fn open(engine: &mut AudioEngine, go: &mpsc::Sender<()>) {
    let _ = go.send(());
    let deadline = Instant::now() + Duration::from_secs(5);
    while engine.hearken().unwrap().opening() {
        assert!(Instant::now() < deadline, "the device never opened");
        engine.update(0.0);
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// A short generated clip under `key`, so the test needs no shipped pack.
fn register(key: &str) {
    let wav = hearken::synth::wav_bytes(&[0.25; 4_410], 44_100);
    ff_core::assets_pack::register_generated_sound(key, wav, "wav");
}

fn played(rec: &Recorder, key: &str) -> Vec<Call> {
    rec.calls()
        .into_iter()
        .filter(|call| matches!(call, Call::Play { key: k, .. } if k == key))
        .collect()
}

/// The requests boot makes while the device is still opening -- the menu
/// theme, the volume settings, the engine voice -- reach the device the
/// moment it opens.
#[test]
fn late_backend_hears_what_boot_asked_for() {
    register("music/test/engine_open_theme");
    let (mut engine, rec, go) = engine_with_pending_open();
    engine.play_music_with("test/engine_open_theme", 1500);
    engine.set_volumes(&VolumeUpdate::default().master(0.5));
    engine.set_engine_voice(true);
    assert!(played(&rec, "test/engine_open_theme").is_empty());

    open(&mut engine, &go);

    let plays = played(&rec, "test/engine_open_theme");
    assert_eq!(plays.len(), 1, "music not replayed: {:?}", rec.calls());
    let channel = rec.channel_of("test/engine_open_theme").unwrap();
    let level = engine.buses().music_level() as f32;
    assert!((engine.buses().master - 0.5).abs() < 1e-12, "volumes lost");
    assert!(
        (rec.last_mix(channel).unwrap().gain - level).abs() < 1e-6,
        "the replayed track fades to the music level"
    );
    assert_eq!(engine.backend().engine_voice_classic(), Some(true));
    assert!(engine.music_playing());
    assert!(!engine.take_silence_notice());
}

/// A device that comes back empty-handed leaves the game silent and arms
/// the notice, so the main menu can say out loud that this run has no
/// sound.
#[test]
fn failed_probe_runs_silent_and_says_so() {
    let hearken = HearkenBackend::deferred_with(|| Err("no device here".to_string()));
    let mut engine = AudioEngine::with_backend(Box::new(hearken));
    let deadline = Instant::now() + Duration::from_secs(5);
    while engine.hearken().unwrap().opening() {
        assert!(Instant::now() < deadline, "the open never finished");
        engine.update(0.0);
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(engine.backend_name(), "hearken");
    assert!(engine.hearken().unwrap().no_device());
    assert!(engine.take_silence_notice());
    assert!(!engine.take_silence_notice(), "the notice must speak once");
}

/// Music stopped before the device opens must not come back from the dead
/// when it does.
#[test]
fn stopped_music_is_not_replayed() {
    register("music/test/engine_open_stopped");
    let (mut engine, rec, go) = engine_with_pending_open();
    engine.play_music_with("test/engine_open_stopped", 1500);
    engine.stop_music_with(400);
    open(&mut engine, &go);
    assert!(
        played(&rec, "test/engine_open_stopped").is_empty(),
        "stopped music was resurrected"
    );
    assert!(!engine.music_playing());
}

/// The main menu sets its ambience once, on entry, which at launch is
/// before the device has opened. The bed must start when the device opens,
/// not wait for the next screen change to ask again; a bed stopped in the
/// meantime stays stopped, and none is started twice.
#[test]
fn late_backend_starts_the_loops_boot_left_running() {
    for key in [
        "test/engine_open_ambient",
        "test/engine_open_rain",
        "test/engine_open_road",
    ] {
        register(key);
    }
    let (mut engine, rec, go) = engine_with_pending_open();
    engine.set_ambient(Some("test/engine_open_ambient"));
    engine.start_loop_with(CH_WEATHER, "test/engine_open_rain", 0.4, 0);
    engine.set_loop_volume(CH_WEATHER, 0.7);
    engine.start_loop_with(CH_ROAD, "test/engine_open_road", 0.5, 0);
    engine.stop_loop_with(CH_ROAD, 0);

    open(&mut engine, &go);

    for key in ["test/engine_open_ambient", "test/engine_open_rain"] {
        let plays = played(&rec, key);
        assert_eq!(plays.len(), 1, "{key}: {:?}", rec.calls());
        assert!(matches!(plays[0], Call::Play { looping: true, .. }));
    }
    let rain = rec.channel_of("test/engine_open_rain").unwrap();
    let buses = engine.buses();
    let expected =
        (0.7 * buses.category_volume(super::super::Category::Weather) * buses.master) as f32;
    assert!(
        (rec.last_mix(rain).unwrap().gain - expected).abs() < 1e-6,
        "loop volume change lost"
    );
    assert!(
        played(&rec, "test/engine_open_road").is_empty(),
        "stopped loop was resurrected"
    );
    assert_eq!(
        engine.backend().loop_entry(CH_AMBIENT).map(|e| e.0),
        Some("test/engine_open_ambient".to_string())
    );
}
