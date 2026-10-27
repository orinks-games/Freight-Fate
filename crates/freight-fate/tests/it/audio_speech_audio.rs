//! Port of the audio tests of `tests/test_speech_audio.py`: the facade is
//! safe headless, and every catalog music track resolves. (The speech
//! backend tests of that file belong to the speech port; the Python-source
//! sound-key sweep stays a Python test.)
//!
use ff_core::music::ALL_MUSIC_TRACKS;
use freight_fate::audio::{asset_bytes, Audio, VolumeUpdate, MUSIC_EXTENSIONS};

use crate::audio_support::{rig, shipped_music};

#[test]
fn test_audio_engine_headless_noops() {
    let mut r = rig();
    // With the dummy driver the backend is Hearken on its silent backend;
    // every call must be safe.
    let audio = &mut r.engine;
    audio.play("ui/menu_select");
    audio.play("nonexistent/sound");
    audio.engine_start();
    audio.set_engine_rpm_with(1500.0, 0.5);
    audio.set_road_noise(20.0);
    audio.set_weather_with(Some("weather/rain_light"), 0.8);
    audio.set_wind(0.5);
    audio.play_music("menu_theme");
    audio.play_music("not_a_track");
    audio.set_volumes(&VolumeUpdate::default().master(0.5).sfx(0.5).music(0.5));
    audio.stop_world();
    audio.stop_music();
    audio.shutdown();
}

#[test]
fn test_music_tracks_exist() {
    // A content invariant of the music pack, not of the code: with only a
    // pointer here every track is "missing" for a reason that has nothing
    // to do with the catalog.
    if !shipped_music() {
        return;
    }
    for track in ALL_MUSIC_TRACKS.iter() {
        assert!(
            asset_bytes(&format!("music/{}", track.key), MUSIC_EXTENSIONS).is_some(),
            "{}",
            track.key
        );
    }
}
