//! The Prism-backed speech object: the Python `Speech` class.
//!
//! Main thread only -- see the threading note in [`crate::speech`].

use std::env;
use std::time::Duration;

use super::backend::{
    narrator_running, pick_backend_gated, pick_event_backend, preserve_backend_default_pitch,
    usable, PrismRegistry, VoiceBackend, VoiceFeatures, VoiceRegistry,
};
use super::{PreviewFeature, SpeechSink, EVENT_BACKEND, REFRESH_INTERVAL_S};

/// How long the first lines wait when the game starts on VoiceOver. VoiceOver
/// reads the newly focused game window aloud as the game opens, and that cuts
/// off any announcement already posted: the opening screen went unheard.
const VOICEOVER_STARTUP_HOLD: Duration = Duration::from_millis(1500);

/// The observer's terminal stream is written at the real speech-sink
/// boundary. That excludes pacing and transcript bookkeeping that never reaches the player.
fn stream_player_speech(text: &str) {
    if env::var_os("FREIGHT_FATE_STREAM_TRANSCRIPT").is_some_and(|value| !value.is_empty()) {
        println!("{text}");
    }
}

/// The player's speech parameters as last pushed, so a backend swap
/// mid-session can re-apply them to the new voice (`Speech._config`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SpeechConfig {
    pub rate: Option<f64>,
    pub pitch: Option<f64>,
    pub volume: Option<f64>,
    pub voice: Option<String>,
}

impl SpeechConfig {
    fn is_empty(&self) -> bool {
        self.rate.is_none() && self.pitch.is_none() && self.volume.is_none() && self.voice.is_none()
    }
}

/// Speech output channel for the whole game.
///
/// All game text flows through [`SpeechSink::say`]. `interrupt == true` cuts
/// off the previous utterance, which is what menu navigation wants; pass
/// `false` for queued announcements such as tutorial text.
pub struct Speech {
    // Declaration order is the drop order: both voices are released before
    // the context that produced them, which Prism requires.
    backend: Option<Box<dyn VoiceBackend>>,
    event_backend: Option<Box<dyn VoiceBackend>>,
    ctx: Option<Box<dyn VoiceRegistry>>,
    override_name: Option<String>,
    event_pref: Option<String>,
    config: SpeechConfig,
    /// Settings > Speech, "Output: braille only". Kept on the object rather
    /// than the backend so it survives a backend swap the way `config` does.
    braille_only: bool,
    refresh_timer: f64,
    /// The Narrator probe `pick_backend` gates the UIA backend on; the real
    /// process scan in the game, swappable so a test can stand Narrator up.
    narrator_probe: fn() -> bool,
}

impl Default for Speech {
    fn default() -> Self {
        Self::new()
    }
}

impl Speech {
    /// Start speech the way the game does: disabled by
    /// `FREIGHT_FATE_NO_SPEECH`, forced to a backend by
    /// `FREIGHT_FATE_SPEECH_BACKEND`, otherwise the best voice that is usable
    /// right now. Never fails: with no Prism, no backend, or an error during
    /// start-up the game continues silently.
    pub fn new() -> Self {
        Self::start(false)
    }

    /// Start speech for a replacement worker after the previous one wedged:
    /// the same selection as [`Speech::new`], on freshly created backend
    /// instances rather than the cached ones the stuck worker still holds
    /// (see [`PrismRegistry::new_fresh`]).
    pub fn new_after_wedge() -> Self {
        Self::start(true)
    }

    fn start(after_wedge: bool) -> Self {
        let override_name = env::var(super::SPEECH_BACKEND_ENV)
            .ok()
            .filter(|name| !name.is_empty());
        if env::var_os(super::NO_SPEECH_ENV).is_some_and(|value| !value.is_empty()) {
            log::info!("Speech disabled via FREIGHT_FATE_NO_SPEECH");
            return Self::disabled_with_override(override_name);
        }
        let registry = if after_wedge {
            PrismRegistry::new_fresh()
        } else {
            PrismRegistry::new()
        };
        match registry {
            Ok(registry) => {
                let speech = Self::with_registry(Box::new(registry), override_name);
                if let Some(ctx) = &speech.ctx {
                    ctx.settle();
                }
                speech
            }
            Err(err) => {
                log::error!("Speech unavailable; continuing silently: {err}");
                Self::disabled_with_override(override_name)
            }
        }
    }

    /// No context at all: what `FREIGHT_FATE_NO_SPEECH` yields. Every call
    /// is a safe no-op and every query answers "nothing bound".
    pub fn disabled() -> Self {
        Self::disabled_with_override(None)
    }

    fn disabled_with_override(override_name: Option<String>) -> Self {
        Self {
            backend: None,
            event_backend: None,
            ctx: None,
            override_name,
            event_pref: None,
            config: SpeechConfig::default(),
            braille_only: false,
            refresh_timer: 0.0,
            narrator_probe: narrator_running,
        }
    }

    /// Start speech on a registry the caller supplies (the real Prism one or
    /// a fake): the selection `Speech.__init__` runs after `prism.Context()`.
    pub fn with_registry(ctx: Box<dyn VoiceRegistry>, override_name: Option<String>) -> Self {
        let mut speech = Self::disabled_with_override(override_name);
        speech.backend = pick_backend_gated(
            ctx.as_ref(),
            speech.override_name.as_deref(),
            speech.narrator_probe,
        );
        speech.ctx = Some(ctx);
        match &speech.backend {
            None => {
                // Keep the context: the player may start their screen reader
                // after the game, and refresh() will connect to it then.
                log::warn!("No usable speech backend yet; will keep checking");
            }
            Some(backend) => {
                log::info!("Speech backend: {}", backend.name());
                speech.select_event_backend(Some(EVENT_BACKEND));
                if speech.event_backend.is_some() {
                    log::info!("Event speech backend: {}", speech.event_backend_name());
                }
            }
        }
        speech
    }

    /// Assemble a speech object from already-chosen parts, skipping the
    /// start-up selection. This is how the Python tests set `s._ctx`,
    /// `s._backend` and `s._event_backend` by hand; nothing in the game
    /// calls it.
    pub fn from_parts(
        ctx: Option<Box<dyn VoiceRegistry>>,
        backend: Option<Box<dyn VoiceBackend>>,
        event_backend: Option<Box<dyn VoiceBackend>>,
    ) -> Self {
        Self {
            backend,
            event_backend,
            ctx,
            override_name: None,
            event_pref: None,
            config: SpeechConfig::default(),
            braille_only: false,
            refresh_timer: 0.0,
            narrator_probe: narrator_running,
        }
    }

    /// Replace the Narrator probe (tests only): `|| true` stands Narrator
    /// up so the UIA route can be picked without the real process.
    pub fn set_narrator_probe(&mut self, probe: fn() -> bool) {
        self.narrator_probe = probe;
    }

    /// Replace the main voice outright (a test standing in for "the screen
    /// reader the game happens to hold").
    pub fn set_main_backend(&mut self, backend: Option<Box<dyn VoiceBackend>>) {
        self.backend = backend;
    }

    /// Replace the event voice outright.
    pub fn set_event_backend(&mut self, backend: Option<Box<dyn VoiceBackend>>) {
        self.event_backend = backend;
    }

    /// The remembered speech parameters.
    pub fn config(&self) -> &SpeechConfig {
        &self.config
    }

    // -- adjustable parameters -------------------------------------------------
    //
    // Prism exposes rate, pitch, volume (each a 0..1 float) and voice (an index)
    // per backend, gated by feature flags. Running screen readers such as NVDA
    // report no support -- they own those settings themselves -- while software
    // voices like SAPI and OneCore support all of them. A change is pushed to
    // every backend that supports it, so the main voice and the separate event
    // voice stay in sync.

    fn backends(&self) -> impl Iterator<Item = &dyn VoiceBackend> {
        self.backend
            .as_deref()
            .into_iter()
            .chain(self.event_backend.as_deref())
    }

    fn backends_mut(&mut self) -> impl Iterator<Item = &mut Box<dyn VoiceBackend>> {
        self.backend
            .as_mut()
            .into_iter()
            .chain(self.event_backend.as_mut())
    }

    fn any_supports(&self, feature: fn(VoiceFeatures) -> bool) -> bool {
        self.backends().any(|backend| feature(backend.features()))
    }

    fn configure_backend(backend: &mut dyn VoiceBackend, config: &SpeechConfig) {
        let features = backend.features();
        let name = backend.name();
        let axes: [(Option<f64>, bool, &str); 3] = [
            (config.rate, features.supports_set_rate, "rate"),
            (config.pitch, features.supports_set_pitch, "pitch"),
            (config.volume, features.supports_set_volume, "volume"),
        ];
        for (value, supported, attr) in axes {
            let Some(value) = value else { continue };
            if !supported {
                continue;
            }
            if attr == "pitch" && preserve_backend_default_pitch(backend, value) {
                continue;
            }
            let result = match attr {
                "rate" => backend.set_rate(value),
                "pitch" => backend.set_pitch(value),
                _ => backend.set_volume(value),
            };
            if let Err(err) = result {
                log::warn!("Could not set speech {attr} on {name}: {err}");
            }
        }
        let Some(voice) = config.voice.as_deref().filter(|voice| !voice.is_empty()) else {
            return;
        };
        if !features.selects_voices() {
            return;
        }
        let result = (|| -> Result<(), prismer::Error> {
            for index in 0..backend.voices_count()? {
                if backend.voice_name(index)? == voice {
                    backend.set_voice(index)?;
                    break;
                }
            }
            Ok(())
        })();
        if let Err(err) = result {
            log::warn!("Could not set speech voice on {name}: {err}");
        }
    }

    /// Deliver `text` the way the player asked for: to the braille display
    /// alone when braille-only is on and this voice has one, otherwise
    /// spoken. A display call that fails is spoken instead -- the game
    /// must never go quiet because a display unplugged -- and logged, so a
    /// session log explains why a braille-only player suddenly heard it.
    fn deliver_with_backend(
        backend: &mut dyn VoiceBackend,
        text: &str,
        interrupt: bool,
        braille_only: bool,
    ) -> bool {
        if braille_only && backend.features().supports_braille {
            match backend.braille(text) {
                Ok(()) => return true,
                Err(err) => log::warn!("Braille output failed; speaking instead: {err}"),
            }
        }
        Self::speak_with_backend(backend, text, interrupt)
    }

    fn speak_with_backend(backend: &mut dyn VoiceBackend, text: &str, interrupt: bool) -> bool {
        let features = backend.features();
        let result = if features.supports_speak && features.supports_braille {
            // Prism's output() for a braille-capable screen reader is speak()
            // then braille(), and it fails the whole call when the braille
            // half fails, after the line was already spoken. ZDSR with no
            // display did that on every line, so each one counted as a dead
            // voice and was spoken twice. Do the two halves here, and let
            // only speech count.
            backend.speak(text, interrupt).map(|()| {
                if let Err(err) = backend.braille(text) {
                    log::debug!("Braille output failed: {err}");
                }
            })
        } else if features.supports_output {
            backend.output(text, interrupt)
        } else if features.supports_speak {
            backend.speak(text, interrupt)
        } else {
            return false;
        };
        match result {
            Ok(()) => true,
            Err(err) => {
                log::warn!("Speech output failed: {err}");
                false
            }
        }
    }

    fn stop_backend(backend: Option<&mut Box<dyn VoiceBackend>>) {
        let Some(backend) = backend else { return };
        if backend.features().supports_stop {
            // A stop that fails is nothing to act on; the next utterance
            // interrupts anyway.
            let _ = backend.stop();
        }
    }

    fn reapply_config(&mut self) {
        let config = self.config.clone();
        for backend in self.backends_mut() {
            Self::configure_backend(backend.as_mut(), &config);
        }
    }
}

impl SpeechSink for Speech {
    fn available(&self) -> bool {
        self.backend.is_some()
    }

    fn startup_hold(&self) -> Duration {
        match &self.backend {
            Some(backend) if backend.name().starts_with("VoiceOver") => VOICEOVER_STARTUP_HOLD,
            _ => Duration::ZERO,
        }
    }

    fn backend_name(&self) -> String {
        match &self.backend {
            None => "none".to_string(),
            Some(backend) => backend.name(),
        }
    }

    fn has_separate_event_voice(&self) -> bool {
        self.event_backend.is_some()
    }

    fn event_backend_name(&self) -> String {
        match &self.event_backend {
            None => "none".to_string(),
            Some(backend) => backend.name(),
        }
    }

    fn supports_rate(&self) -> bool {
        self.any_supports(|features| features.supports_set_rate)
    }

    fn event_supports_rate(&self) -> bool {
        self.event_backend
            .as_ref()
            .is_some_and(|backend| backend.features().supports_set_rate)
    }

    fn supports_pitch(&self) -> bool {
        self.any_supports(|features| features.supports_set_pitch)
    }

    fn supports_volume(&self) -> bool {
        self.any_supports(|features| features.supports_set_volume)
    }

    fn event_backend_options(&self) -> Vec<String> {
        // Usable backends that can serve as a separate event voice: the
        // controllable software voices (SAPI, OneCore, ...) other than the
        // main voice -- screen readers are excluded because they cannot be
        // driven independently. In registry order, as Python walked it.
        let (Some(ctx), Some(main)) = (&self.ctx, &self.backend) else {
            return Vec::new();
        };
        let main_name = main.name();
        let ids: Vec<_> = (0..ctx.backend_count())
            .filter_map(|index| ctx.id_at(index))
            .collect();
        let mut options = Vec::new();
        for backend_id in ids {
            let Ok(backend) = ctx.acquire(backend_id) else {
                continue;
            };
            let name = backend.name();
            if name == main_name || !usable(backend.as_ref()) {
                continue;
            }
            let features = backend.features();
            if features.supports_set_voice || features.supports_set_rate {
                options.push(name);
            }
        }
        options
    }

    fn select_event_backend(&mut self, name: Option<&str>) {
        // Remember the preference even when it cannot be honored right now:
        // refresh() re-runs the selection after a backend swap or when a
        // screen reader appears mid-session.
        let name = name.filter(|name| !name.is_empty());
        self.event_pref = name.map(str::to_string);
        if self.ctx.is_none() || self.backend.is_none() {
            return;
        }
        let Some(mut name) = name.map(str::to_string) else {
            self.event_backend = None;
            return;
        };
        let options = self.event_backend_options();
        if !options.contains(&name) && !options.is_empty() {
            name = options[0].clone();
        }
        let (Some(ctx), Some(main)) = (&self.ctx, &self.backend) else {
            return;
        };
        self.event_backend = pick_event_backend(ctx.as_ref(), Some(main.as_ref()), &name);
    }

    fn voice_names(&self) -> Vec<String> {
        // Installed voice names from the first backend that lets us pick one.
        // Empty when no backend supports voice selection (for example when the
        // only voice is a running screen reader).
        for backend in self.backends() {
            if !backend.features().selects_voices() {
                continue;
            }
            let names = (|| -> Result<Vec<String>, prismer::Error> {
                (0..backend.voices_count()?)
                    .map(|index| backend.voice_name(index))
                    .collect()
            })();
            if let Ok(names) = names {
                return names;
            }
        }
        Vec::new()
    }

    fn configure(
        &mut self,
        rate: Option<f64>,
        pitch: Option<f64>,
        volume: Option<f64>,
        voice: Option<&str>,
    ) {
        // Unsupported parameters (and backends) are skipped silently, and any
        // backend failure is logged without disturbing the others or the
        // game. The values are remembered so a backend swap mid-session (see
        // `refresh`) can re-apply the player's settings to the new voice.
        if rate.is_some() {
            self.config.rate = rate;
        }
        if pitch.is_some() {
            self.config.pitch = pitch;
        }
        if volume.is_some() {
            self.config.volume = volume;
        }
        if let Some(voice) = voice {
            self.config.voice = Some(voice.to_string());
        }
        let request = SpeechConfig {
            rate,
            pitch,
            volume,
            voice: voice.map(str::to_string),
        };
        for backend in self.backends_mut() {
            Self::configure_backend(backend.as_mut(), &request);
        }
    }

    fn say(&mut self, text: &str, interrupt: bool) {
        if text.is_empty() {
            return;
        }
        let Some(backend) = self.backend.as_mut() else {
            return;
        };
        let braille_only = self.braille_only;
        let mut spoken =
            Self::deliver_with_backend(backend.as_mut(), text, interrupt, braille_only);
        if !spoken {
            // The utterance failed: the screen reader probably just quit or was
            // switched. Re-detect immediately and retry once so this line is not
            // lost; if nothing can speak right now, poll() keeps looking.
            self.backend = None;
            if self.refresh() {
                if let Some(backend) = self.backend.as_mut() {
                    spoken =
                        Self::deliver_with_backend(backend.as_mut(), text, interrupt, braille_only);
                    if !spoken {
                        self.backend = None;
                    }
                }
            }
        }
        if spoken {
            stream_player_speech(text);
        }
    }

    fn say_event(&mut self, text: &str, interrupt: bool) {
        if text.is_empty() {
            return;
        }
        if self.braille_only && self.supports_braille() {
            // The display is the one place the player reads, so the event
            // voice has nothing to add: the line goes where the menus go.
            // No stop_main: there is no speech in progress to cut, and a
            // display shows one line at a time regardless.
            self.say(text, interrupt);
            return;
        }
        let Some(backend) = self.event_backend.as_mut() else {
            if interrupt {
                self.stop_main();
            }
            self.say(text, false);
            return;
        };
        // Let the backend perform the interruption as part of the new output
        // call. Calling stop() immediately before output(..., interrupt=True)
        // is redundant and could crash inside Prism's Windows SAPI stop path
        // when urgent road events arrived back to back (issue #85).
        if Self::speak_with_backend(backend.as_mut(), text, interrupt) {
            stream_player_speech(text);
        } else {
            self.event_backend = None;
            if interrupt {
                self.stop_main();
            }
            self.say(text, false);
        }
    }

    // -- runtime re-detection ----------------------------------------------------
    //
    // The backend chosen at startup can die at any time: players switch screen
    // readers mid-session (NVDA to Narrator and back), restart them, or start
    // them after the game. These hooks notice within a few seconds and rebind
    // speech to whatever is running instead of leaving the game mute.

    fn refresh(&mut self) -> bool {
        // Runs the same selection as startup: the environment override first,
        // then the highest-priority backend that is usable right now. When
        // the choice changes, the event voice is re-selected and the player's
        // speech settings are re-applied to the new voice. Returns true when
        // the main voice changed.
        let Some(ctx) = &self.ctx else {
            return false;
        };
        let old_name = self.backend.as_ref().map(|backend| backend.name());
        let picked = pick_backend_gated(
            ctx.as_ref(),
            self.override_name.as_deref(),
            self.narrator_probe,
        );
        let Some(backend) = picked else {
            let Some(old_name) = old_name else {
                return false;
            };
            log::warn!("Speech backend {old_name} went away and nothing else can speak");
            self.backend = None;
            self.event_backend = None;
            return true;
        };
        let new_name = backend.name();
        if old_name.as_deref() == Some(new_name.as_str()) {
            // Same main voice as before; just make sure the event voice is
            // alive too (it can die independently, e.g. a SAPI hiccup).
            if self.event_pref.is_some() && self.event_backend.is_none() {
                let pref = self.event_pref.clone();
                self.select_event_backend(pref.as_deref());
                if self.event_backend.is_some() && !self.config.is_empty() {
                    self.reapply_config();
                }
            }
            return false;
        }
        self.backend = Some(backend);
        log::info!(
            "Speech backend switched: {} -> {new_name}",
            old_name.as_deref().unwrap_or("none")
        );
        let pref = self.event_pref.clone();
        self.select_event_backend(pref.as_deref());
        if !self.config.is_empty() {
            self.reapply_config();
        }
        true
    }

    fn poll(&mut self, dt: f64) {
        if self.ctx.is_none() {
            return;
        }
        self.refresh_timer += dt;
        if self.refresh_timer < REFRESH_INTERVAL_S {
            return;
        }
        self.refresh_timer = 0.0;
        self.refresh();
    }

    fn request_refresh(&mut self) {
        // Called when the game window regains focus: switching screen readers
        // happens outside the game, so that is the moment a change most
        // likely just occurred.
        self.refresh_timer = REFRESH_INTERVAL_S;
    }

    fn set_braille_only(&mut self, on: bool) {
        if self.braille_only != on {
            log::info!("Braille-only output {}", if on { "on" } else { "off" });
        }
        self.braille_only = on;
    }

    fn supports_braille(&self) -> bool {
        self.backend
            .as_ref()
            .is_some_and(|backend| backend.features().supports_braille)
    }

    fn say_adjustment_preview(&mut self, setting: &str, text: &str, interrupt: bool) -> bool {
        // If the main screen reader cannot be configured but a separate SAPI
        // or OneCore voice can, preview changes through that configurable
        // voice.
        let Some(feature) = PreviewFeature::for_setting(setting) else {
            return false;
        };
        if text.is_empty() {
            return false;
        }
        for backend in self.backends_mut() {
            if feature.supported_by(backend.features()) {
                return Self::speak_with_backend(backend.as_mut(), text, interrupt);
            }
        }
        false
    }

    fn stop_main(&mut self) {
        Self::stop_backend(self.backend.as_mut());
    }

    fn stop_event(&mut self) {
        Self::stop_backend(self.event_backend.as_mut());
    }

    fn stop(&mut self) {
        Self::stop_backend(self.backend.as_mut());
        Self::stop_backend(self.event_backend.as_mut());
    }

    fn is_speaking(&self) -> bool {
        self.backend
            .as_ref()
            .is_some_and(|backend| backend.is_speaking().unwrap_or(false))
    }

    fn shutdown(&mut self) {
        self.stop();
        self.backend = None;
        self.event_backend = None;
        self.ctx = None;
    }
}
