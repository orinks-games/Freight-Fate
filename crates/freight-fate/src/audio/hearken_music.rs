//! Music on the Hearken backend: shipped tracks and the player's own files,
//! faded the way BASS faded them.
//!
//! Every track is played once, never looped: the station rotation and the
//! playlist decide what comes next. A new track fades the old one out over
//! 800 ms while it fades itself in to the music level. Tracker modules are
//! refused (Hearken has no player for them), so a playlist skips them.

use std::path::Path;

use hearken::audio::{Bus, Sound, Start};

use super::assets::MODULE_EXTENSIONS;
use super::hearken_backend::{HearkenBackend, MusicReplay};
use super::hearken_clips::FfClips;
use super::AudioError;

/// Whether `ext` names a tracker module, which Hearken has no player for.
fn is_module(ext: &str) -> bool {
    MODULE_EXTENSIONS
        .iter()
        .any(|module| module.eq_ignore_ascii_case(ext))
}

/// How long the outgoing track takes to fade when another replaces it.
const MUSIC_CROSSFADE_OUT_MS: u32 = 800;

impl HearkenBackend {
    pub(super) fn play_music(&mut self, track: &str, fade_ms: u32) {
        if self.music_track.as_deref() == Some(track) {
            return;
        }
        self.play_music_at(track, fade_ms, 0.0);
    }

    /// Play a shipped track from `start_s` seconds in. Like BASS, this does
    /// not short-circuit on the track already being the one loaded: the
    /// point of the call is the position.
    pub(super) fn play_music_at(&mut self, track: &str, fade_ms: u32, start_s: f64) {
        self.cancel_radio_connect();
        let Some((data, ext)) = FfClips::music(track) else {
            log::warn!("Missing music track: {track}");
            return;
        };
        if is_module(&ext) {
            log::warn!("Could not open stream: {track} (tracker modules do not play)");
            return;
        }
        self.fade_out_music();
        self.music_request = Some(MusicReplay::Track {
            track: track.to_string(),
            fade_ms,
        });
        let start = Start::ONCE.volume(0.0).from_s(start_s.max(0.0));
        match self.audio.try_play_clip(track, data, Bus::MUSIC, start) {
            Ok(sound) => {
                self.fade_in_music(sound, fade_ms);
                self.music_track = Some(track.to_string());
            }
            Err(err) => log::warn!("Could not open stream: {track} ({err})"),
        }
    }

    /// Play one of the player's own files. Fails when the file cannot be
    /// read or decoded, so the radio layer can skip to the next entry.
    pub(super) fn play_music_file(&mut self, path: &str, fade_ms: u32) -> Result<(), AudioError> {
        let key = format!("file:{path}");
        self.cancel_radio_connect();
        let data: std::sync::Arc<[u8]> = match std::fs::read(Path::new(path)) {
            Ok(data) => data.into(),
            Err(_) => return Err(AudioError::new(format!("could not read {path}"))),
        };
        self.fade_out_music();
        let ext = Path::new(path)
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("");
        // Hearken has no tracker player, and a file it cannot decode must
        // fail here whatever the device (the silent one takes any bytes),
        // so the playlist skips the entry.
        if is_module(ext) || super::probe_clip(data.clone()).is_err() {
            return Err(AudioError::new(format!("could not decode {path}")));
        }
        let start = Start::ONCE.volume(0.0);
        match self.audio.try_play_clip(&key, data, Bus::MUSIC, start) {
            Ok(sound) => {
                self.fade_in_music(sound, fade_ms);
                self.music_track = Some(key);
                self.music_request = Some(MusicReplay::File {
                    path: path.to_string(),
                    fade_ms,
                });
                Ok(())
            }
            Err(_) => Err(AudioError::new(format!("could not decode {path}"))),
        }
    }

    pub(super) fn music_playing(&self) -> bool {
        self.music_sound
            .is_some_and(|sound| self.audio.is_playing(sound))
            && self.radio_playing()
    }

    /// The current track's length, as read when it started: never the
    /// outgoing one's.
    pub(super) fn music_length_s(&self) -> Option<f64> {
        self.music_sound?;
        self.music_length.filter(|s| *s > 0.0)
    }

    pub(super) fn stop_music(&mut self, fade_ms: u32) {
        // Cancel before the early return: a radio still connecting has no
        // sound yet, and stopping the radio must orphan that connect too.
        self.cancel_radio_connect();
        self.music_request = None;
        let Some(sound) = self.music_sound.take() else {
            return;
        };
        self.audio.stop(sound, fade_ms);
        self.forget_music();
    }

    pub(super) fn fade_out_music(&mut self) {
        self.music_request = None;
        if let Some(sound) = self.music_sound.take() {
            self.audio.stop(sound, MUSIC_CROSSFADE_OUT_MS);
            self.forget_music();
        }
    }

    /// The music channel is empty: no track, no length, no station.
    pub(super) fn forget_music(&mut self) {
        self.music_sound = None;
        self.music_track = None;
        self.music_length = None;
        self.radio = None;
    }

    /// The sound device has just opened. A track or file started while it
    /// was opening was a one-shot on the silent stand-in, which the swap
    /// let go; play it again from the top, as the boot asked. A station is
    /// a feed, which the pool carried across itself.
    pub(super) fn replay_music_after_open(&mut self) {
        let lost = self
            .music_sound
            .is_some_and(|sound| !self.audio.is_playing(sound));
        if !lost || self.radio.is_some() {
            return;
        }
        let request = self.music_request.take();
        self.forget_music();
        match request {
            Some(MusicReplay::Track { track, fade_ms }) => self.play_music(&track, fade_ms),
            Some(MusicReplay::File { path, fade_ms }) => {
                let _ = self.play_music_file(&path, fade_ms);
            }
            None => {}
        }
    }

    fn fade_in_music(&mut self, sound: Sound, fade_ms: u32) {
        let level = self.buses.music_level() as f32;
        self.audio.set_volume_over(sound, level, fade_ms);
        self.music_length = self.audio.length_s(sound);
        self.music_sound = Some(sound);
    }
}
