//! The backend surface behind the [`AudioEngine`](super::AudioEngine)
//! facade: what the Python `_BassBackend` / `_NullBackend` duck type
//! provided, as a trait, plus the volume buses and category routing both
//! backends share.

use std::any::Any;
use std::fmt;

use ff_core::audio_loops::SustainLoopSpec;

use super::{AudioError, CH_ENGINE, CH_SIREN, CH_WEATHER, CH_WEATHER_B, ENGINE_RPM_IDLE};

/// The mix bus a sound rides: one of the seven volume settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Category {
    Engine,
    Weather,
    Ui,
    Siren,
    Sfx,
}

impl Category {
    /// The Python category string.
    pub fn as_str(&self) -> &'static str {
        match self {
            Category::Engine => "engine",
            Category::Weather => "weather",
            Category::Ui => "ui",
            Category::Siren => "siren",
            Category::Sfx => "sfx",
        }
    }

    /// The category a Python string names (`"sfx"` for anything unknown,
    /// as the dict lookup's default did).
    pub fn parse(name: &str) -> Category {
        match name {
            "engine" => Category::Engine,
            "weather" => Category::Weather,
            "ui" => Category::Ui,
            "siren" => Category::Siren,
            _ => Category::Sfx,
        }
    }
}

/// The bus a one-shot rides, by key prefix.
pub fn one_shot_category(key: &str) -> Category {
    if key.starts_with("enforcement/") || key == "events/police_siren" {
        Category::Siren
    } else if key.starts_with("ui/") {
        Category::Ui
    } else if key.starts_with("weather/") {
        Category::Weather
    } else if key.starts_with("engine/") {
        Category::Engine
    } else {
        Category::Sfx
    }
}

/// The bus a loop rides, by reserved slot.
pub fn loop_category(channel: u32) -> Category {
    if CH_ENGINE.contains(&channel) {
        Category::Engine
    } else if channel == CH_WEATHER || channel == CH_WEATHER_B {
        Category::Weather
    } else if channel == CH_SIREN {
        // Off the shared sfx bus on purpose: a siren behind you is the one
        // sound in the game that must be raisable without dragging every
        // clunk, hiss and chime up with it.
        Category::Siren
    } else {
        Category::Sfx
    }
}

/// The keyword arguments of the Python `set_volumes(master=None, sfx=None,
/// ...)`: `None` leaves a bus alone. `VolumeUpdate::default()` changes
/// nothing and is the "reapply everything" pass.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct VolumeUpdate {
    pub master: Option<f64>,
    pub sfx: Option<f64>,
    pub music: Option<f64>,
    pub weather: Option<f64>,
    pub engine: Option<f64>,
    pub ui: Option<f64>,
    pub siren: Option<f64>,
}

impl VolumeUpdate {
    pub fn master(mut self, v: f64) -> Self {
        self.master = Some(v);
        self
    }
    pub fn sfx(mut self, v: f64) -> Self {
        self.sfx = Some(v);
        self
    }
    pub fn music(mut self, v: f64) -> Self {
        self.music = Some(v);
        self
    }
    pub fn weather(mut self, v: f64) -> Self {
        self.weather = Some(v);
        self
    }
    pub fn engine(mut self, v: f64) -> Self {
        self.engine = Some(v);
        self
    }
    pub fn ui(mut self, v: f64) -> Self {
        self.ui = Some(v);
        self
    }
    pub fn siren(mut self, v: f64) -> Self {
        self.siren = Some(v);
        self
    }
}

fn fmt_level(f: &mut fmt::Formatter<'_>, name: &str, level: Option<f64>) -> fmt::Result {
    match level {
        Some(v) => write!(f, "{name}={v:?}"),
        None => write!(f, "{name}=None"),
    }
}

/// The Python log line: `master=0.5 sfx=None music=0.5 ...`.
impl fmt::Display for VolumeUpdate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt_level(f, "master", self.master)?;
        f.write_str(" ")?;
        fmt_level(f, "sfx", self.sfx)?;
        f.write_str(" ")?;
        fmt_level(f, "music", self.music)?;
        f.write_str(" ")?;
        fmt_level(f, "weather", self.weather)?;
        f.write_str(" ")?;
        fmt_level(f, "engine", self.engine)?;
        f.write_str(" ")?;
        fmt_level(f, "ui", self.ui)?;
        f.write_str(" ")?;
        fmt_level(f, "siren", self.siren)
    }
}

/// The seven volume buses plus the speech duck that rides on top of them.
///
/// `speech_duck` is not a setting value: settings own the volumes, this
/// multiplies engine, weather and music while the event voice speaks (see
/// `AudioEngine::set_speech_duck`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Buses {
    pub master: f64,
    pub sfx: f64,
    pub music: f64,
    pub weather: f64,
    pub engine: f64,
    pub ui: f64,
    pub siren: f64,
    pub speech_duck: f64,
}

impl Default for Buses {
    fn default() -> Self {
        Self::new()
    }
}

impl Buses {
    /// The defaults every Python backend started with.
    pub const fn new() -> Self {
        Self {
            master: 1.0,
            sfx: 0.8,
            music: 0.5,
            weather: 0.65,
            engine: 0.55,
            ui: 0.9,
            siren: 1.0,
            speech_duck: 1.0,
        }
    }

    /// The bus level for a category, with the speech duck applied to the
    /// two buses it covers (the music slot is ducked separately, through
    /// [`Buses::music_level`]).
    pub fn category_volume(&self, category: Category) -> f64 {
        let volume = match category {
            Category::Engine => self.engine,
            Category::Weather => self.weather,
            Category::Ui => self.ui,
            Category::Siren => self.siren,
            Category::Sfx => self.sfx,
        };
        match category {
            Category::Engine | Category::Weather => volume * self.speech_duck,
            _ => volume,
        }
    }

    /// The music channel's absolute level: bus, master and duck, clamped.
    pub fn music_level(&self) -> f64 {
        (self.music * self.master * self.speech_duck).clamp(0.0, 1.0)
    }

    /// Apply the non-`None` fields, each clamped to [0, 1].
    pub fn apply(&mut self, update: &VolumeUpdate) {
        if let Some(v) = update.master {
            self.master = v.clamp(0.0, 1.0);
        }
        if let Some(v) = update.sfx {
            self.sfx = v.clamp(0.0, 1.0);
        }
        if let Some(v) = update.music {
            self.music = v.clamp(0.0, 1.0);
        }
        if let Some(v) = update.weather {
            self.weather = v.clamp(0.0, 1.0);
        }
        if let Some(v) = update.engine {
            self.engine = v.clamp(0.0, 1.0);
        }
        if let Some(v) = update.ui {
            self.ui = v.clamp(0.0, 1.0);
        }
        if let Some(v) = update.siren {
            self.siren = v.clamp(0.0, 1.0);
        }
    }
}

/// What a backend offers the facade: the Python `_BassBackend` /
/// `_NullBackend` surface. Every method has a no-op default so a test
/// double implements only what it observes;
/// [`HearkenBackend`](super::HearkenBackend) implements all of them.
pub trait AudioBackend {
    /// `"hearken"` or `"none"`.
    fn name(&self) -> &'static str;
    /// False for the null backend, and for Hearken after `shutdown`.
    fn enabled(&self) -> bool;
    fn buses(&self) -> &Buses;
    fn buses_mut(&mut self) -> &mut Buses;
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;

    /// True once when the sound device, opened off the game loop, would not
    /// open after all: the facade's cue to tell the player.
    fn take_device_notice(&mut self) -> bool {
        false
    }

    // -- one-shots and loops ----------------------------------------------------
    fn play_if_idle(&mut self, key: &str, volume: f64, pan: f64) {
        self.play(key, volume, pan);
    }
    fn play(&mut self, _key: &str, _volume: f64, _pan: f64) {}
    fn update_cue(&mut self, _key: &str, _volume: f64, _pan: f64) {}
    fn stop_cue(&mut self, _key: &str) {}
    fn start_loop(&mut self, _channel: u32, _key: &str, _volume: f64, _fade_ms: u32) {}
    fn set_loop_volume(&mut self, _channel: u32, _volume: f64) {}
    fn set_loop_pan(&mut self, _channel: u32, _pan: f64) {}
    fn set_loop_rate(&mut self, _channel: u32, _rate: f64) {}
    fn stop_loop(&mut self, _channel: u32, _fade_ms: u32) {}
    /// The `(key, gain)` sounding on a loop slot, for the facade's live
    /// jake re-voice (the Python facade read `_loops[CH_JAKE]`).
    fn loop_entry(&self, _channel: u32) -> Option<(String, f64)> {
        None
    }
    fn start_sustain_loop(
        &mut self,
        _channel: u32,
        _key: &str,
        _spec: SustainLoopSpec,
        _volume: f64,
    ) {
    }
    fn release_sustain_loop(&mut self, _channel: u32, _fade_ms: u32) {}

    // -- truck engine -------------------------------------------------------------
    fn engine_start(&mut self, _play_start_sound: bool) {}
    fn engine_stop(&mut self, _shutdown_sound: bool) {}
    fn set_engine_pan(&mut self, _pan: f64) {}
    fn set_engine_rpm(&mut self, _rpm: f64, _throttle: f64) {}
    fn set_engine_duck(&mut self, _duck: f64) {}
    fn set_road_noise(&mut self, _speed_mps: f64) {}
    /// The driver window's lift on the tire loop; see `Audio::set_outside_lift`.
    fn set_outside_lift(&mut self, _lift: f64) {}
    /// The radio's voicing; see `Audio::set_radio_voicing`.
    fn set_radio_voicing(&mut self, _voicing: Option<super::RadioVoicing>) {}
    fn update(&mut self, _dt: f64) {}
    fn reverse_start(&mut self) {}
    fn reverse_stop(&mut self) {}
    fn engine_running(&self) -> bool {
        false
    }
    fn engine_starting(&self) -> bool {
        false
    }
    /// The engine voice preference, or `None` for a backend with one model
    /// (the Python facade's `getattr(impl, "engine_voice_classic", None)`).
    fn engine_voice_classic(&self) -> Option<bool> {
        None
    }
    fn set_engine_voice_classic(&mut self, _classic: bool) {}
    /// The rpm and throttle last applied, for a live re-voice.
    fn engine_last_rpm_throttle(&self) -> (f64, f64) {
        (ENGINE_RPM_IDLE, 0.0)
    }

    // -- music ----------------------------------------------------------------------
    fn play_music(&mut self, _track: &str, _fade_ms: u32) {}
    /// Start a track part way in; a backend without seeking plays it whole.
    fn play_music_at(&mut self, track: &str, fade_ms: u32, _start_s: f64) {
        self.play_music(track, fade_ms);
    }
    fn play_radio_stream(&mut self, _url: &str, _fade_ms: u32) -> Result<(), AudioError> {
        Err(AudioError::new("radio stream unavailable"))
    }
    fn radio_now_playing(&self) -> Option<String> {
        None
    }
    fn play_music_file(&mut self, _path: &str, _fade_ms: u32) -> Result<(), AudioError> {
        Err(AudioError::new("audio disabled"))
    }
    fn music_playing(&self) -> bool {
        false
    }
    /// How long the playing music track runs, when the stream can tell.
    fn music_length_s(&self) -> Option<f64> {
        None
    }
    fn stop_music(&mut self, _fade_ms: u32) {}

    // -- volume control ---------------------------------------------------------------
    /// Scale engine, weather, and music under the event voice, live.
    fn set_speech_duck(&mut self, duck: f64) {
        let duck = duck.clamp(0.0, 1.0);
        if duck == self.buses().speech_duck {
            return;
        }
        self.buses_mut().speech_duck = duck;
        // Reapply everything the factor touches; set_volumes with no
        // arguments is exactly that pass.
        self.set_volumes(&VolumeUpdate::default());
    }
    fn set_volumes(&mut self, volumes: &VolumeUpdate) {
        self.buses_mut().apply(volumes);
    }
    fn shutdown(&mut self) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::{CH_ALERT, CH_JAKE};

    #[test]
    fn one_shot_categories_follow_the_key_prefix() {
        assert_eq!(one_shot_category("enforcement/siren"), Category::Siren);
        assert_eq!(one_shot_category("events/police_siren"), Category::Siren);
        assert_eq!(one_shot_category("ui/menu_select"), Category::Ui);
        assert_eq!(one_shot_category("weather/rain_light"), Category::Weather);
        assert_eq!(one_shot_category("engine/start"), Category::Engine);
        assert_eq!(one_shot_category("vehicle/horn"), Category::Sfx);
    }

    #[test]
    fn loop_categories_follow_the_slot() {
        assert_eq!(loop_category(CH_ENGINE[2]), Category::Engine);
        assert_eq!(loop_category(CH_WEATHER), Category::Weather);
        assert_eq!(loop_category(CH_WEATHER_B), Category::Weather);
        assert_eq!(loop_category(CH_SIREN), Category::Siren);
        assert_eq!(loop_category(CH_JAKE), Category::Sfx);
        assert_eq!(loop_category(CH_ALERT), Category::Sfx);
    }

    #[test]
    fn the_duck_reaches_engine_and_weather_only() {
        let mut buses = Buses::new();
        let engine = buses.category_volume(Category::Engine);
        let ui = buses.category_volume(Category::Ui);
        buses.speech_duck = 0.5;
        assert!((buses.category_volume(Category::Engine) - engine * 0.5).abs() < 1e-12);
        assert_eq!(buses.category_volume(Category::Ui), ui);
        assert!((buses.music_level() - 0.25).abs() < 1e-12);
    }

    #[test]
    fn volume_updates_clamp_and_leave_none_alone() {
        let mut buses = Buses::new();
        buses.apply(&VolumeUpdate::default().master(2.0).sfx(-1.0));
        assert_eq!(buses.master, 1.0);
        assert_eq!(buses.sfx, 0.0);
        assert_eq!(buses.music, 0.5);
        assert_eq!(
            VolumeUpdate::default().master(0.5).to_string(),
            "master=0.5 sfx=None music=None weather=None engine=None ui=None siren=None"
        );
    }
}
