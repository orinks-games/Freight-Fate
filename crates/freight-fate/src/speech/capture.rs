//! Test doubles at the [`SpeechSink`] level: [`CaptureSpeech`] records what
//! would have been spoken, [`NullSpeech`] swallows it.
//!
//! `CaptureSpeech` is `tests/speech_capture.py`'s `speech_stub` and the
//! playtest harness's transcript recorder in one object: every line lands
//! as a [`SpokenEntry`] with its channel, interrupt flag and sequence number,
//! and `transcript()` renders them the way the harness did (`"[event] "` in
//! front of event lines).

use ff_core::speech_text::SpokenMessage;

use super::SpeechSink;

/// One `configure` call as the capture records it: `(rate, pitch, volume,
/// voice)`.
pub type ConfigureCall = (Option<f64>, Option<f64>, Option<f64>, Option<String>);

/// Which voice a line went to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SpeechChannel {
    Main,
    Event,
}

impl SpeechChannel {
    /// The tag `speech_stub(tag=...)` used for this channel.
    pub const fn tag(self) -> &'static str {
        match self {
            SpeechChannel::Main => "main",
            SpeechChannel::Event => "event",
        }
    }
}

/// One recorded utterance.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpokenEntry {
    /// 0-based position in the recording, across both channels.
    pub sequence: u64,
    pub channel: SpeechChannel,
    /// The text as recorded (prefix applied).
    pub text: String,
    pub interrupt: bool,
}

/// How the capture answers the query half of [`SpeechSink`].
///
/// The defaults are what the Python headless suite saw: with
/// `FREIGHT_FATE_NO_SPEECH=1` the real `Speech` had no context, so it
/// reported no voice, no separate event voice, no adjustable parameters and
/// no voice names. The 119 transcript tests were recorded against those
/// answers (the settings menu shows no rate/pitch/volume rows, the driving
/// states do not speed the event voice up), so a transcript-preserving
/// capture keeps them. [`CaptureSpeech::full_voice`] answers like a real
/// Windows machine -- NVDA running plus a SAPI event voice -- for tests of
/// the code paths those answers unlock.
#[derive(Clone, Debug, PartialEq)]
pub struct CaptureProfile {
    pub available: bool,
    pub backend_name: String,
    /// The event voice bound when one is selected. `None` means the capture
    /// has no separate voice to offer, whatever is asked for.
    pub event_backend_name: Option<String>,
    pub supports_rate: bool,
    pub supports_pitch: bool,
    pub supports_volume: bool,
    pub event_supports_rate: bool,
    pub event_backend_options: Vec<String>,
    pub voice_names: Vec<String>,
}

impl CaptureProfile {
    /// The headless Python `Speech` (no Prism context).
    pub fn headless() -> Self {
        Self {
            available: false,
            backend_name: "none".to_string(),
            event_backend_name: None,
            supports_rate: false,
            supports_pitch: false,
            supports_volume: false,
            event_supports_rate: false,
            event_backend_options: Vec::new(),
            voice_names: Vec::new(),
        }
    }

    /// A Windows box with NVDA running and SAPI (David, Zira) for events.
    pub fn full_voice() -> Self {
        Self {
            available: true,
            backend_name: "NVDA".to_string(),
            event_backend_name: Some("SAPI".to_string()),
            supports_rate: true,
            supports_pitch: true,
            supports_volume: true,
            event_supports_rate: true,
            event_backend_options: vec!["OneCore".to_string(), "SAPI".to_string()],
            voice_names: vec!["David".to_string(), "Zira".to_string()],
        }
    }
}

impl Default for CaptureProfile {
    fn default() -> Self {
        Self::headless()
    }
}

/// Records every line handed to the speech layer instead of speaking it.
#[derive(Debug)]
pub struct CaptureSpeech {
    entries: Vec<SpokenEntry>,
    terse: bool,
    prefix: String,
    profile: CaptureProfile,
    /// The event-voice preference last selected; `Some` binds the profile's
    /// event voice (when it has one), `None` sends events to the main voice.
    event_pref: Option<String>,
    stop_main_calls: u32,
    stop_event_calls: u32,
    stop_calls: u32,
    previews: Vec<(String, String, bool)>,
    configure_calls: Vec<ConfigureCall>,
    refresh_requests: u32,
    shutdown_calls: u32,
}

impl Default for CaptureSpeech {
    fn default() -> Self {
        Self::new()
    }
}

impl CaptureSpeech {
    /// A headless capture: records everything, answers like the Python
    /// `Speech` under `FREIGHT_FATE_NO_SPEECH`.
    pub fn new() -> Self {
        Self::with_profile(CaptureProfile::headless())
    }

    /// A capture that answers like a machine with a screen reader and a
    /// separate SAPI event voice (see [`CaptureProfile::full_voice`]).
    pub fn full_voice() -> Self {
        let mut capture = Self::with_profile(CaptureProfile::full_voice());
        capture.event_pref = Some(super::EVENT_BACKEND.to_string());
        capture
    }

    pub fn with_profile(profile: CaptureProfile) -> Self {
        Self {
            entries: Vec::new(),
            terse: false,
            prefix: String::new(),
            profile,
            event_pref: None,
            stop_main_calls: 0,
            stop_event_calls: 0,
            stop_calls: 0,
            previews: Vec::new(),
            configure_calls: Vec::new(),
            refresh_requests: 0,
            shutdown_calls: 0,
        }
    }

    /// Resolve `SpokenMessage` pairs to their terse rendering, as a player
    /// set to terse would hear them (`speech_stub(terse=True)`).
    pub fn terse(mut self) -> Self {
        self.terse = true;
        self
    }

    /// Mark every recorded line with `prefix` (`speech_stub(prefix=...)`).
    pub fn with_prefix(mut self, prefix: &str) -> Self {
        self.prefix = prefix.to_string();
        self
    }

    pub fn set_terse(&mut self, terse: bool) {
        self.terse = terse;
    }

    pub fn is_terse(&self) -> bool {
        self.terse
    }

    pub fn profile(&self) -> &CaptureProfile {
        &self.profile
    }

    pub fn profile_mut(&mut self) -> &mut CaptureProfile {
        &mut self.profile
    }

    fn record(&mut self, channel: SpeechChannel, text: &str, interrupt: bool) {
        let text = if self.prefix.is_empty() {
            text.to_string()
        } else {
            format!("{}{text}", self.prefix)
        };
        self.entries.push(SpokenEntry {
            sequence: self.entries.len() as u64,
            channel,
            text,
            interrupt,
        });
    }

    /// Record a normal/terse pair on the main channel, resolved by the
    /// capture's terse flag. A pair whose chosen rendering is empty is
    /// dropped, exactly as the real method drops it.
    pub fn say_message(&mut self, message: &SpokenMessage, interrupt: bool) {
        let text = message.render(self.terse);
        if !text.is_empty() {
            self.record(SpeechChannel::Main, text, interrupt);
        }
    }

    /// Record a normal/terse pair on the event channel.
    pub fn say_event_message(&mut self, message: &SpokenMessage, interrupt: bool) {
        let text = message.render(self.terse);
        if !text.is_empty() {
            self.record(SpeechChannel::Event, text, interrupt);
        }
    }

    /// Everything recorded, in order.
    pub fn entries(&self) -> &[SpokenEntry] {
        &self.entries
    }

    /// The recorded texts, both channels, in order.
    pub fn lines(&self) -> Vec<String> {
        self.entries
            .iter()
            .map(|entry| entry.text.clone())
            .collect()
    }

    /// The recorded texts on one channel, in order.
    pub fn channel_lines(&self, channel: SpeechChannel) -> Vec<String> {
        self.entries
            .iter()
            .filter(|entry| entry.channel == channel)
            .map(|entry| entry.text.clone())
            .collect()
    }

    /// Main-channel texts, in order.
    pub fn main_lines(&self) -> Vec<String> {
        self.channel_lines(SpeechChannel::Main)
    }

    /// Event-channel texts, in order.
    pub fn event_lines(&self) -> Vec<String> {
        self.channel_lines(SpeechChannel::Event)
    }

    /// `(text, interrupt)` for every line on `channel`, the shape
    /// `speech_stub(with_interrupt=True)` recorded.
    pub fn calls(&self, channel: SpeechChannel) -> Vec<(String, bool)> {
        self.entries
            .iter()
            .filter(|entry| entry.channel == channel)
            .map(|entry| (entry.text.clone(), entry.interrupt))
            .collect()
    }

    /// `(tag, text)` for every line, both channels, the shape
    /// `speech_stub(tag=...)` recorded for order-across-channels tests.
    pub fn tagged(&self) -> Vec<(&'static str, String)> {
        self.entries
            .iter()
            .map(|entry| (entry.channel.tag(), entry.text.clone()))
            .collect()
    }

    /// The playtest harness transcript: one line per utterance, event lines
    /// marked `"[event] "`, joined with newlines.
    pub fn transcript(&self) -> String {
        self.transcript_lines().join("\n")
    }

    /// The transcript as separate lines.
    pub fn transcript_lines(&self) -> Vec<String> {
        self.entries
            .iter()
            .map(|entry| match entry.channel {
                SpeechChannel::Main => entry.text.clone(),
                SpeechChannel::Event => format!("[event] {}", entry.text),
            })
            .collect()
    }

    /// Whether any recorded line contains `needle` (case-sensitive).
    pub fn contains(&self, needle: &str) -> bool {
        self.entries.iter().any(|entry| entry.text.contains(needle))
    }

    /// Forget everything recorded so far; the profile and flags stay.
    pub fn clear(&mut self) {
        self.entries.clear();
        self.previews.clear();
        self.configure_calls.clear();
        self.stop_main_calls = 0;
        self.stop_event_calls = 0;
        self.stop_calls = 0;
        self.refresh_requests = 0;
    }

    /// Take the recording, leaving the capture empty.
    pub fn drain(&mut self) -> Vec<SpokenEntry> {
        std::mem::take(&mut self.entries)
    }

    pub fn stop_main_calls(&self) -> u32 {
        self.stop_main_calls
    }

    pub fn stop_event_calls(&self) -> u32 {
        self.stop_event_calls
    }

    /// Calls to `stop` (both channels), not counting `stop_main`/`stop_event`.
    pub fn stop_calls(&self) -> u32 {
        self.stop_calls
    }

    /// `(setting, text, interrupt)` for every adjustment preview asked for.
    pub fn previews(&self) -> &[(String, String, bool)] {
        &self.previews
    }

    /// `(rate, pitch, volume, voice)` for every `configure` call.
    pub fn configure_calls(&self) -> &[ConfigureCall] {
        &self.configure_calls
    }

    pub fn refresh_requests(&self) -> u32 {
        self.refresh_requests
    }

    pub fn shutdown_calls(&self) -> u32 {
        self.shutdown_calls
    }

    /// The event-voice preference last selected (`None` = the main voice).
    pub fn event_preference(&self) -> Option<&str> {
        self.event_pref.as_deref()
    }
}

impl SpeechSink for CaptureSpeech {
    fn say(&mut self, text: &str, interrupt: bool) {
        if !text.is_empty() {
            self.record(SpeechChannel::Main, text, interrupt);
        }
    }

    fn say_event(&mut self, text: &str, interrupt: bool) {
        if !text.is_empty() {
            self.record(SpeechChannel::Event, text, interrupt);
        }
    }

    fn stop_main(&mut self) {
        self.stop_main_calls += 1;
    }

    fn stop_event(&mut self) {
        self.stop_event_calls += 1;
    }

    fn stop(&mut self) {
        self.stop_calls += 1;
    }

    fn poll(&mut self, _dt: f64) {}

    fn request_refresh(&mut self) {
        self.refresh_requests += 1;
    }

    fn available(&self) -> bool {
        self.profile.available
    }

    fn backend_name(&self) -> String {
        self.profile.backend_name.clone()
    }

    fn has_separate_event_voice(&self) -> bool {
        self.event_pref.is_some() && self.profile.event_backend_name.is_some()
    }

    fn event_backend_name(&self) -> String {
        if self.has_separate_event_voice() {
            self.profile
                .event_backend_name
                .clone()
                .unwrap_or_else(|| "none".to_string())
        } else {
            "none".to_string()
        }
    }

    fn supports_rate(&self) -> bool {
        self.profile.supports_rate
    }

    fn supports_pitch(&self) -> bool {
        self.profile.supports_pitch
    }

    fn supports_volume(&self) -> bool {
        self.profile.supports_volume
    }

    fn event_supports_rate(&self) -> bool {
        self.has_separate_event_voice() && self.profile.event_supports_rate
    }

    fn event_backend_options(&self) -> Vec<String> {
        self.profile.event_backend_options.clone()
    }

    fn select_event_backend(&mut self, name: Option<&str>) {
        self.event_pref = name.filter(|name| !name.is_empty()).map(str::to_string);
    }

    fn set_braille_only(&mut self, _on: bool) {
        // A transcript reads the same whichever way the line reached the
        // player; the routing itself is pinned against the fake registry.
    }

    fn supports_braille(&self) -> bool {
        false
    }

    fn voice_names(&self) -> Vec<String> {
        self.profile.voice_names.clone()
    }

    fn configure(
        &mut self,
        rate: Option<f64>,
        pitch: Option<f64>,
        volume: Option<f64>,
        voice: Option<&str>,
    ) {
        self.configure_calls
            .push((rate, pitch, volume, voice.map(str::to_string)));
    }

    fn say_adjustment_preview(&mut self, setting: &str, text: &str, interrupt: bool) -> bool {
        if super::PreviewFeature::for_setting(setting).is_none() || text.is_empty() {
            return false;
        }
        self.previews
            .push((setting.to_string(), text.to_string(), interrupt));
        true
    }

    fn refresh(&mut self) -> bool {
        false
    }

    fn shutdown(&mut self) {
        self.shutdown_calls += 1;
    }
}

/// The no-speech sink: every call is a no-op, every query answers like the
/// headless Python `Speech`. For `--headless` runs that want no recording
/// at all.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NullSpeech;

impl SpeechSink for NullSpeech {
    fn say(&mut self, _text: &str, _interrupt: bool) {}
    fn say_event(&mut self, _text: &str, _interrupt: bool) {}
    fn stop_main(&mut self) {}
    fn stop_event(&mut self) {}
    fn stop(&mut self) {}
    fn poll(&mut self, _dt: f64) {}
    fn request_refresh(&mut self) {}
    fn available(&self) -> bool {
        false
    }
    fn backend_name(&self) -> String {
        "none".to_string()
    }
    fn has_separate_event_voice(&self) -> bool {
        false
    }
    fn event_backend_name(&self) -> String {
        "none".to_string()
    }
    fn supports_rate(&self) -> bool {
        false
    }
    fn supports_pitch(&self) -> bool {
        false
    }
    fn supports_volume(&self) -> bool {
        false
    }
    fn event_supports_rate(&self) -> bool {
        false
    }
    fn event_backend_options(&self) -> Vec<String> {
        Vec::new()
    }
    fn select_event_backend(&mut self, _name: Option<&str>) {}
    fn set_braille_only(&mut self, _on: bool) {}
    fn supports_braille(&self) -> bool {
        false
    }
    fn voice_names(&self) -> Vec<String> {
        Vec::new()
    }
    fn configure(
        &mut self,
        _rate: Option<f64>,
        _pitch: Option<f64>,
        _volume: Option<f64>,
        _voice: Option<&str>,
    ) {
    }
    fn say_adjustment_preview(&mut self, _setting: &str, _text: &str, _interrupt: bool) -> bool {
        false
    }
    fn refresh(&mut self) -> bool {
        false
    }
    fn shutdown(&mut self) {}
}
