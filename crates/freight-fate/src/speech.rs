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
//! Every call on a Prism [`hearken::speech::prismer::Prism`] -- construction, health checks,
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
//! The one Python class became three things: the [`SpeechSink`] trait (what
//! `GameContext` needs from a speech object), the Prism-backed [`Speech`]
//! (`speech::live`), and the test doubles [`CaptureSpeech`] and [`NullSpeech`]
//! (`speech::capture`). The Prism calls themselves sit behind the
//! [`backend::VoiceRegistry`] / [`backend::VoiceBackend`] traits so the
//! selection logic -- `pick_backend`, the event voice, the refresh poll -- is
//! tested against fake registries exactly as `tests/test_speech_audio.py`
//! tested it against `FakeContext` and `RecordingBackend`.
//!
//! # Hearken
//!
//! That whole sink stack now lives in Hearken's `hearken-speech` crate and is
//! re-exported here at the paths the game and its tests always used
//! (`crate::speech::capture`, `crate::speech::fakes`, ...). What stays in
//! Freight Fate: the environment variable names ([`env`]), the settings side
//! ([`apply_speech_settings`] over `ff_core`'s `Settings`). The event pacer
//! is Hearken's too, re-exported here with its types, so the app shell takes
//! every speech service through this one module.

use ff_core::settings::Settings;

pub use hearken::speech::{backend, capture, fakes, live, threaded};

// The event-voice pacer is Hearken's, run with `app::FF_PACER`; re-exported
// here because it is part of what a caller means by "the speech channel",
// and so the app shell names no `hearken::speech` path of its own.
pub use hearken::speech::{
    Cut, DeliveryStatus, EventPriority, EventSpeechPacer, PacerConfig, Protect, Valid,
};

pub use hearken::speech::backend::{
    narrator_running, pick_backend, pick_backend_gated, pick_event_backend,
    preserve_backend_default_pitch, usable, BackendId, PrismRegistry, PrismVoice, VoiceBackend,
    VoiceFeatures, VoiceRegistry,
};
pub use hearken::speech::capture::{
    CaptureProfile, CaptureSpeech, ConfigureCall, NullSpeech, SharedCapture, SpeechChannel,
    SpokenEntry,
};
pub use hearken::speech::live::{Speech, SpeechConfig};
pub use hearken::speech::threaded::ThreadedSpeech;
pub use hearken::speech::{
    PreviewFeature, SpeechEnv, SpeechSink, EVENT_BACKEND, PREVIEW_FEATURES, REFRESH_INTERVAL_S,
    UIA_LAST_RESORT_PRIORITY,
};

/// Environment variable that disables speech entirely (no Prism is loaded).
pub const NO_SPEECH_ENV: &str = "FREIGHT_FATE_NO_SPEECH";

/// Environment variable that forces a specific Prism backend by registry name.
pub const SPEECH_BACKEND_ENV: &str = "FREIGHT_FATE_SPEECH_BACKEND";

/// The environment variables the game's speech reads: [`NO_SPEECH_ENV`],
/// [`SPEECH_BACKEND_ENV`] and `FREIGHT_FATE_STREAM_TRANSCRIPT`.
pub fn env() -> SpeechEnv {
    SpeechEnv::with_prefix("FREIGHT_FATE")
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
