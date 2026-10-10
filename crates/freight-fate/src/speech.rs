//! Port of `freight_fate/speech.py` — screen reader output via Prism.
//!
//! Prism is a screen reader abstraction layer that unifies NVDA, JAWS, SAPI,
//! VoiceOver, Speech Dispatcher, and many other backends behind one API. This
//! module wraps it in a small game-friendly interface that:
//!
//! * never crashes the game if speech is unavailable (silent fallback),
//! * picks the best backend that is actually usable on this machine: Prism's
//!   `acquire_best` cannot be trusted for this -- it returns the highest
//!   priority backend that already has a live cached instance (whatever the
//!   game happens to be holding), and otherwise ranks by static registry
//!   priority whether or not that screen reader is running -- so the registry
//!   is enumerated in priority order and every candidate is validated against
//!   its live `is_supported_at_runtime` check,
//! * treats Prism's `UIA` backend (the route to Narrator, via UI Automation
//!   notifications) as a gated last resort: it reports runtime support on every
//!   modern Windows whether or not anyone is listening, so it is skipped unless
//!   Narrator is actually running -- and even then it only wins when no other
//!   voice works, because the backend raises every notification with
//!   `NotificationProcessing_ImportantAll`, which Narrator queues without
//!   ever cancelling: interrupt and stop are no-ops, so menu browsing through
//!   it piles up unread items,
//! * keeps watching while the game runs: if the player switches screen readers
//!   mid-session (NVDA off, Narrator on, back to NVDA), a periodic health check
//!   re-detects the running one and reconnects speech instead of going silent,
//! * prefers `output` (speech + braille) and falls back to `speak`; with
//!   the braille-only setting on it sends every line, driving events
//!   included, to the main voice's `braille` instead and speaks nothing,
//! * can be disabled with the `FREIGHT_FATE_NO_SPEECH=1` environment variable
//!   (used by the headless test suite and CI), and forced to a specific backend
//!   with `FREIGHT_FATE_SPEECH_BACKEND=<name>` (for example `SAPI`).
//!
//! # Threading: each Prism context stays on its speech worker
//!
//! Every call on a Prism [`prismer::Prism`] -- construction, health checks,
//! speaking, configuring and shutdown -- happens on the worker that created
//! it. [`Speech`] is deliberately not `Send` or `Sync`. Recovery may leave an
//! old generation inside an in-flight native call while a replacement uses
//! private backend instances. After that call returns, the old worker stops
//! dispatching operations and retains its native objects until process exit.
//!
//! That one thread used to be the game loop's, which made every spoken line
//! a synchronous screen-reader/SAPI call the game waited on -- and the one
//! time such a call wedged (Shane, 2026-08-30, an event-voice interrupt at a
//! merge), the whole game froze with it, permanently. The windowed game now
//! builds [`ThreadedSpeech`] instead: the context lives on a dedicated
//! worker thread (created there, never moved), the game loop only queues
//! commands, and a wedged backend costs sentences, never the drive. The
//! headless app and the tests keep their direct sinks; nothing about
//! capture-based testing changed.
//!
//! # Shape of the port
//!
//! The one Python class becomes three things: the [`SpeechSink`] trait (what
//! `GameContext` needs from a speech object), the Prism-backed [`Speech`]
//! (`speech::live`), and the test doubles [`CaptureSpeech`] and [`NullSpeech`]
//! (`speech::capture`). The Prism calls themselves sit behind the
//! [`backend::VoiceRegistry`] / [`backend::VoiceBackend`] traits so the
//! selection logic -- `pick_backend`, the event voice, the refresh poll -- is
//! tested against fake registries exactly as `tests/test_speech_audio.py`
//! tests it against `FakeContext` and `RecordingBackend`.

use ff_core::settings::Settings;

pub mod backend;
pub mod capture;
pub mod fakes;
pub mod live;
pub mod threaded;

// The event-voice pacer moved to its own module when it grew repeat
// suppression and priority; re-exported here because it is part of what a
// caller means by "the speech channel", and every existing import says so.
pub use ff_core::speech_pacing::{EventPriority, EventSpeechPacer};

pub use backend::{
    narrator_running, pick_backend, pick_backend_gated, pick_event_backend,
    preserve_backend_default_pitch, usable, BackendId, PrismRegistry, PrismVoice, VoiceBackend,
    VoiceFeatures, VoiceRegistry,
};
pub use capture::{
    CaptureProfile, CaptureSpeech, ConfigureCall, NullSpeech, SpeechChannel, SpokenEntry,
};
pub use live::{Speech, SpeechConfig};
pub use threaded::ThreadedSpeech;

/// Seconds between runtime health checks of the speech backend. Short enough
/// that a player who switches screen readers hears the game again within a few
/// seconds, long enough that the registry scan costs nothing per frame.
pub const REFRESH_INTERVAL_S: f64 = 3.0;

/// Preferred separate event voice per platform; the first one that exists wins.
/// These are the controllable software TTS engines, not screen readers: SAPI is
/// Windows, AVSpeech is macOS, Speech Dispatcher is Linux. `select_event_backend`
/// falls back to whatever the machine actually has, so this is only a hint.
pub const EVENT_BACKEND: &str = "SAPI";

/// Ranks the UIA backend below every other voice even while Narrator is
/// running. Prism's UIA backend raises all notifications with
/// `NotificationProcessing_ImportantAll`, which Narrator queues without ever
/// cancelling -- interrupt and stop are no-ops -- so menu browsing through it
/// stacks up unread items. Until that is fixed upstream (the fix is
/// `ImportantMostRecent` for the interrupt case), UIA is only for machines
/// where nothing else can speak at all: queued speech beats silence.
pub const UIA_LAST_RESORT_PRIORITY: i32 = -1;

/// Environment variable that disables speech entirely (no Prism is loaded).
pub const NO_SPEECH_ENV: &str = "FREIGHT_FATE_NO_SPEECH";

/// Environment variable that forces a specific Prism backend by registry name.
pub const SPEECH_BACKEND_ENV: &str = "FREIGHT_FATE_SPEECH_BACKEND";

/// Settings keys whose preview speaks through the voice that setting adjusts,
/// paired with the backend feature that voice must support.
pub const PREVIEW_FEATURES: [(&str, PreviewFeature); 4] = [
    ("speech_rate", PreviewFeature::Rate),
    ("speech_pitch", PreviewFeature::Pitch),
    ("speech_volume", PreviewFeature::Volume),
    ("speech_voice", PreviewFeature::Voice),
];

/// The backend capability a settings preview needs, one per adjustable
/// speech setting (`Speech._PREVIEW_FEATURES` in Python).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PreviewFeature {
    Rate,
    Pitch,
    Volume,
    Voice,
}

impl PreviewFeature {
    /// The feature a settings key previews through, or `None` for a setting
    /// that is not voice-adjustable (`speech_verbosity`, ...).
    pub fn for_setting(setting: &str) -> Option<Self> {
        PREVIEW_FEATURES
            .iter()
            .find(|(key, _)| *key == setting)
            .map(|(_, feature)| *feature)
    }

    /// Whether `features` lets a voice be adjusted along this axis.
    pub fn supported_by(self, features: VoiceFeatures) -> bool {
        match self {
            PreviewFeature::Rate => features.supports_set_rate,
            PreviewFeature::Pitch => features.supports_set_pitch,
            PreviewFeature::Volume => features.supports_set_volume,
            PreviewFeature::Voice => features.supports_set_voice,
        }
    }
}

/// What `GameContext` needs from a speech object: the public surface of the
/// Python `Speech` class, method for method.
///
/// Two channels: `say` is the main voice (menus, info keys, everything the
/// player asked for), `say_event` the dedicated event voice (road events,
/// warnings) that falls back to the main channel when no separate voice is
/// bound. `interrupt` cuts off the previous utterance on that channel.
///
/// A backend switch is silent: the new voice simply carries on with the
/// next line (owner call, 2026-10-10).
pub trait SpeechSink {
    /// Speak (and braille, where supported) `text` on the main channel.
    fn say(&mut self, text: &str, interrupt: bool);

    /// Speak on the dedicated event voice (SAPI), so the player's screen
    /// reader cannot talk over it; falls back to the main channel.
    fn say_event(&mut self, text: &str, interrupt: bool);

    /// Silence in-progress main speech without cutting off event speech.
    fn stop_main(&mut self);

    /// Silence in-progress event speech without cutting off main speech.
    fn stop_event(&mut self);

    /// Silence any in-progress speech on both channels.
    fn stop(&mut self);

    /// Periodic health check, driven every frame by the game loop.
    fn poll(&mut self, dt: f64);

    /// Make the next `poll` re-detect immediately (the window regained focus).
    fn request_refresh(&mut self);

    /// Whether a main voice is bound right now.
    fn available(&self) -> bool;

    /// Registry name of the main voice; `"none"` when there is none.
    fn backend_name(&self) -> String;

    /// Whether events currently have their own backend. Without one,
    /// `say_event` falls back to the main channel, so events and menu speech
    /// share a single voice and can cut each other.
    fn has_separate_event_voice(&self) -> bool;

    /// Registry name of the event voice; `"none"` when events share the main
    /// voice.
    fn event_backend_name(&self) -> String;

    /// Whether any bound voice honors the in-game rate setting.
    fn supports_rate(&self) -> bool;

    /// Whether any bound voice honors the in-game pitch setting.
    fn supports_pitch(&self) -> bool;

    /// Whether any bound voice honors the in-game volume setting.
    fn supports_volume(&self) -> bool;

    /// Whether the dedicated event voice honors the in-game rate setting.
    fn event_supports_rate(&self) -> bool;

    /// Usable backends that can serve as a separate event voice (the
    /// controllable software voices other than the main one), in registry
    /// order.
    fn event_backend_options(&self) -> Vec<String>;

    /// Choose which backend speaks driving events (`None` = the main voice).
    /// The name is a preference: when it is not on this machine the best
    /// available separate software voice is used instead.
    fn select_event_backend(&mut self, name: Option<&str>);

    /// Installed voice names from the first backend that lets us pick one;
    /// empty when none does.
    fn voice_names(&self) -> Vec<String>;

    /// Push speech parameters to every backend that supports them; `None`
    /// leaves that parameter alone. Values are remembered so a backend swap
    /// mid-session can re-apply them to the new voice.
    fn configure(
        &mut self,
        rate: Option<f64>,
        pitch: Option<f64>,
        volume: Option<f64>,
        voice: Option<&str>,
    );

    /// Send every line to the main voice's braille display and speak nothing
    /// (Settings > Speech, "Output: braille only"): menus, readouts, and the
    /// driving events that would otherwise go to the separate event voice.
    /// Honoured only while the bound main voice can braille
    /// (`supports_braille`); with any other voice speech carries on
    /// unchanged, so the switch can never leave a player with nothing.
    fn set_braille_only(&mut self, on: bool);

    /// Whether the main voice can put text on a braille display by itself
    /// (NVDA and JAWS can; SAPI, OneCore and Narrator cannot).
    fn supports_braille(&self) -> bool;

    /// Speak a settings preview through the voice affected by `setting`
    /// (`"speech_rate"`, `"speech_pitch"`, `"speech_volume"`,
    /// `"speech_voice"`). `false` when no bound voice can be adjusted that way
    /// or the setting is not voice-adjustable.
    fn say_adjustment_preview(&mut self, setting: &str, text: &str, interrupt: bool) -> bool;

    /// Re-detect which screen reader or voice should be speaking. `true` when
    /// the main voice changed.
    fn refresh(&mut self) -> bool;

    /// Whether the main voice is still speaking a line. Sinks that cannot
    /// answer it report `false`.
    fn is_speaking(&self) -> bool {
        false
    }

    /// How long the speech worker holds this sink's first lines after
    /// start-up, so the screen reader's own announcement of the new window
    /// cannot talk over the game's first screen. Most voices need none.
    fn startup_hold(&self) -> std::time::Duration {
        std::time::Duration::ZERO
    }

    /// Release the backends and context. Safe to call more than once.
    fn shutdown(&mut self);

    /// Release the backends like [`shutdown`](SpeechSink::shutdown), calling
    /// `pump` between wait slices so the caller can keep the window
    /// answering the OS while an in-flight utterance finishes (issue 266).
    /// Sinks whose shutdown never waits use the default.
    fn shutdown_pumping(&mut self, pump: &mut dyn FnMut()) {
        let _ = pump;
        self.shutdown();
    }
}

/// `GameContext.apply_speech`: reflect the speech settings on the sink.
///
/// Selects the event voice the player chose (or the main voice when
/// `sapi_events` is off), records the voice the sink binds in its place when
/// the saved one is not on this machine (a Windows save's SAPI opened on
/// macOS) so the menu and later sessions reflect reality, and pushes rate,
/// pitch, volume and voice. Lives here rather than in the app shell so the
/// settings side of speech has one definition.
///
/// The substitute is decided from the option list, never read back from the
/// sink: the threaded sink queues the switch and answers `event_backend_name`
/// from a snapshot that still names the voice bound BEFORE it, and writing
/// that stale name over the setting is how a freshly chosen OneCore turned
/// back into SAPI in memory and then on disk at the quit-time save
/// (MariahL, 2026-09-02). The fallback mirrors `select_event_backend`: a
/// preference missing from a non-empty option list becomes the first option.
pub fn apply_speech_settings(sink: &mut dyn SpeechSink, settings: &mut Settings) {
    let preference = settings.sapi_events.then(|| settings.event_backend.clone());
    sink.select_event_backend(preference.as_deref());
    if settings.sapi_events {
        let options = sink.event_backend_options();
        if !options.is_empty() && !options.contains(&settings.event_backend) {
            log::info!(
                "Event speech backend {} is not on this machine; using {}",
                settings.event_backend,
                options[0]
            );
            settings.event_backend = options[0].clone();
        }
    }
    let voice = (!settings.speech_voice.is_empty()).then(|| settings.speech_voice.clone());
    sink.configure(
        Some(settings.speech_rate),
        Some(settings.speech_pitch),
        Some(settings.speech_volume),
        voice.as_deref(),
    );
    sink.set_braille_only(settings.braille_only);
}
