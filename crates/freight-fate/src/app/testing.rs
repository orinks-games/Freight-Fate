//! Shared rigging for tests that build a headless [`App`]: the conftest of
//! the Python suite in one place, so every later state port reuses it.
//!
//! * [`TestApp`] pins its own save directory on its OWN THREAD, so any
//!   number of them run at once without seeing each other's saves. It used
//!   to set the process-global `FREIGHT_FATE_DATA_DIR` instead, which meant
//!   holding [`env_lock`] for the whole life of the app -- and that lock,
//!   not the work, was what the suite's runtime actually was: 123.6 seconds
//!   on one thread against 117.4 on eight, twenty-seven cores idle.
//! * [`env_lock`] remains for the handful of tests that genuinely do write a
//!   process-global environment variable (`FREIGHT_FATE_ONLINE_URL`,
//!   `FREIGHT_FATE_LOG_FILE`). Those still have one environment between
//!   them and still have to queue; there are few enough that it costs
//!   nothing. Reach for a pinned root instead wherever the variable is only
//!   standing in for "this test's own directory".
//! * [`TestApp`] is `App()` under the conftest fixtures: headless drivers,
//!   an isolated data directory, the first-run online offer already seen,
//!   a `CaptureSpeech` in place of Prism, the null audio backend.
//! * [`RecordingAudio`] is the `monkeypatch.setattr(app.ctx.audio, "play",
//!   ...)` seam: an `Audio` that records `play` and `set_speech_duck`.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{Mutex, MutexGuard, TryLockError};
use std::time::{Duration, Instant};

use ff_core::settings::Settings;
use ff_core::speech_pacing::EventSpeechPacer;

use crate::audio::{Audio, AudioError, SustainLoopSpec, VolumeUpdate};
use crate::speech::{CaptureSpeech, SpeechSink};

use super::App;

static ENV_LOCK: Mutex<()> = Mutex::new(());
/// Which thread holds [`ENV_LOCK`] right now, as the hash of its `ThreadId`,
/// or 0 for nobody. Only [`EnvGuard`] writes it, so it is accurate for as
/// long as a guard is alive and cleared the moment one drops.
static ENV_OWNER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static DIR_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// A fresh directory under the system temp dir, removed on drop (the
/// pytest `tmp_path`).
pub struct TempDir {
    path: std::path::PathBuf,
}

impl TempDir {
    pub fn new(prefix: &str) -> TempDir {
        let n = DIR_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "{prefix}-{}-{n}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&path).expect("a temp data dir");
        TempDir { path }
    }

    pub fn path(&self) -> &std::path::Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// Holds the environment lock and remembers which thread it belongs to, so
/// [`env_lock`] can tell a real self-deadlock from an honest queue.
pub struct EnvGuard {
    _inner: MutexGuard<'static, ()>,
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        ENV_OWNER.store(0, std::sync::atomic::Ordering::SeqCst);
    }
}

fn thread_key() -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    std::thread::current().id().hash(&mut h);
    // 0 means "nobody", so never hand it back as a thread's key.
    h.finish() | 1
}

/// Take the environment lock (poison-tolerant: a failed test must not take
/// the rest of the file down with it).
///
/// A test that builds a second [`TestApp`] while the first is still alive
/// would block here forever -- the guard is held for the app's whole
/// lifetime, and shadowing a binding does not drop it. That deadlock is
/// invisible: the test simply never finishes, and every other test in the
/// binary queues behind it. So the guard records its owner and the panic
/// fires the instant a thread asks for a lock it already holds; `drop(app)`
/// before building the next one is the fix.
///
/// Waiting on ANOTHER thread is not a deadlock, and must not be treated as
/// one. Every `TestApp` in a binary queues on this lock, so the serial time
/// of a whole file is what the last test waits: `playtest_harness.rs` alone
/// runs 39 seconds of drives back to back. A wall-clock deadline turned that
/// into a failure naming whichever test happened to be last in the queue.
pub fn env_lock() -> EnvGuard {
    let me = thread_key();
    // A backstop for a hang that is not this thread's own doing: long enough
    // that no honest queue reaches it, short enough that CI still reports.
    let deadline = Instant::now() + Duration::from_secs(600);
    loop {
        match ENV_LOCK.try_lock() {
            Ok(guard) => {
                ENV_OWNER.store(me, std::sync::atomic::Ordering::SeqCst);
                return EnvGuard { _inner: guard };
            }
            Err(TryLockError::Poisoned(e)) => {
                ENV_OWNER.store(me, std::sync::atomic::Ordering::SeqCst);
                return EnvGuard {
                    _inner: e.into_inner(),
                };
            }
            Err(TryLockError::WouldBlock) => {
                assert!(
                    ENV_OWNER.load(std::sync::atomic::Ordering::SeqCst) != me,
                    "this thread already holds the test environment lock. A \
                     TestApp holds it until it is dropped, so building a \
                     second one in the same scope deadlocks -- shadowing the \
                     binding does not drop the first. Call drop(app) before \
                     building the next TestApp."
                );
                assert!(
                    Instant::now() < deadline,
                    "the test environment lock was held by another thread for \
                     over ten minutes; something in this binary is hung."
                );
                std::thread::sleep(Duration::from_millis(5));
            }
        }
    }
}

/// The headless environment the Python conftest forced for every test.
///
/// Set exactly once per process, however many tests ask. All three values
/// are constants, so the first caller's answer is every caller's answer --
/// and writing a process-global from many threads at once is the one thing
/// worth avoiding now that tests really do run at once.
pub fn set_headless_env() {
    static HEADLESS: std::sync::Once = std::sync::Once::new();
    HEADLESS.call_once(|| {
        std::env::set_var("SDL_VIDEODRIVER", "dummy");
        std::env::set_var("SDL_AUDIODRIVER", "dummy");
        std::env::set_var("FREIGHT_FATE_NO_SPEECH", "1");
    });
}

/// Pins this thread's save directory for as long as it lives, then puts back
/// whatever was pinned before.
///
/// This is what [`TestApp`] holds in place of the old environment-lock
/// guard, and the reason a suite of app tests can now run in parallel: the
/// directory is the thread's, not the process's.
pub struct DataDirGuard {
    previous: Option<std::path::PathBuf>,
}

impl DataDirGuard {
    /// Point this thread's saves at `dir`.
    pub fn pin(dir: std::path::PathBuf) -> DataDirGuard {
        DataDirGuard {
            previous: ff_core::settings::set_thread_data_dir(Some(dir)),
        }
    }
}

impl Drop for DataDirGuard {
    fn drop(&mut self) {
        ff_core::settings::set_thread_data_dir(self.previous.take());
    }
}

thread_local! {
    /// Whether this thread already has a live [`TestApp`].
    ///
    /// Two at once on one thread would share the thread's pinned directory
    /// and its save-listener slot, and the second one's shutdown would tear
    /// out the first one's hook -- the same corruption the process-global
    /// lock used to prevent, now scoped to the thread that can actually
    /// cause it. Building a second one panics with the fix, exactly as the
    /// old self-deadlock assertion did.
    static APP_ALIVE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// A headless app over an isolated data directory, pinned to this thread
/// until dropped.
///
/// Field order is drop order and matters: `app` shuts down first and writes
/// its final settings and profile, `data_dir` then deletes the directory it
/// wrote them to, and `_guard` unpins the thread last -- so the writes land
/// where the test could see them and nothing leaks into the next test on
/// this thread.
pub struct TestApp {
    pub app: App,
    pub data_dir: TempDir,
    capture: Rc<RefCell<CaptureSpeech>>,
    _guard: DataDirGuard,
}

impl std::ops::Deref for TestApp {
    type Target = App;
    fn deref(&self) -> &App {
        &self.app
    }
}

impl std::ops::DerefMut for TestApp {
    fn deref_mut(&mut self) -> &mut App {
        &mut self.app
    }
}

/// The first dispatch board every test app rolls (see
/// `GameContext::dispatch_board_seed`). Any value works; this one is the
/// date the game stopped rolling the test boards from OS entropy.
const DISPATCH_BOARD_SEED: i64 = 20_260_902;

impl TestApp {
    /// `App()` under the conftest fixtures, speech captured.
    pub fn new() -> TestApp {
        Self::with_speech(CaptureSpeech::new())
    }

    /// `App()` with a particular capture (e.g. `CaptureSpeech::full_voice()`
    /// for a machine with a separate event voice).
    pub fn with_speech(speech: CaptureSpeech) -> TestApp {
        assert!(
            !APP_ALIVE.with(|alive| alive.replace(true)),
            "this thread already has a live TestApp. A TestApp pins the \
             thread's save directory and save-listener hook until it is \
             dropped, so building a second one in the same scope would let \
             the two share both -- shadowing the binding does not drop the \
             first. Call drop(app) before building the next TestApp."
        );
        set_headless_env();
        let data_dir = TempDir::new("ff-rust-app");
        // The thread's own saves, not the process's: see the module note.
        let guard = DataDirGuard::pin(data_dir.path().join("data"));
        // Seed the one-time first-run orinks.net offer as already spent, so a
        // test that is not about the offer can drive a career through the
        // app without knowing it exists.
        let settings = Settings {
            online_offer_seen: true,
            // The one-time Driving assistance picker likewise.
            assist_preset_chosen: true,
            // The live diesel feed is on by default for players; a test app
            // must not ask the network for it on every frame (the guard
            // would refuse it, noisily, from a worker thread). Tests that
            // want the week's price seed an offline provider and turn it on.
            real_fuel_prices: false,
            // Off here, on for players since the fresh install moved to the
            // Balanced preset (2026-09-18). A test that drives a gate or an
            // arrival BY HAND must not have an assist reaching for the same
            // pedals, and twenty-two of them stopped arriving the moment this
            // default flipped. The cases that are about the assist turn it on
            // themselves, which is the right way round: a fixture should not
            // inherit a player-facing default that changes what the truck
            // does for you.
            destination_approach_assist: false,
            ..Default::default()
        };
        let _ = settings.save();
        let capture = Rc::new(RefCell::new(speech));
        let mut app = App::new_headless(Box::new(SharedCapture(Rc::clone(&capture))));
        app.ctx.dispatch_board_seed = Some(DISPATCH_BOARD_SEED);
        TestApp {
            app,
            data_dir,
            capture,
            _guard: guard,
        }
    }

    /// The capture behind `ctx.speech`.
    pub fn speech(&self) -> std::cell::Ref<'_, CaptureSpeech> {
        self.capture.borrow()
    }

    pub fn speech_mut(&self) -> std::cell::RefMut<'_, CaptureSpeech> {
        self.capture.borrow_mut()
    }

    /// Drop everything said so far.
    pub fn clear_speech(&self) {
        self.capture.borrow_mut().clear();
    }

    /// Main-channel `(text, interrupt)` calls since the last clear.
    pub fn main_calls(&self) -> Vec<(String, bool)> {
        self.speech().calls(crate::speech::SpeechChannel::Main)
    }

    /// Event-channel `(text, interrupt)` calls since the last clear.
    pub fn event_calls(&self) -> Vec<(String, bool)> {
        self.speech().calls(crate::speech::SpeechChannel::Event)
    }

    /// Main-channel texts since the last clear.
    pub fn main_lines(&self) -> Vec<String> {
        self.speech().main_lines()
    }

    /// Event-channel texts since the last clear.
    pub fn event_lines(&self) -> Vec<String> {
        self.speech().event_lines()
    }

    /// Replace `ctx.audio` with a [`RecordingAudio`] and hand back its log.
    pub fn record_audio(&mut self) -> AudioLog {
        let audio = RecordingAudio::new();
        let log = audio.log();
        self.app.ctx.audio = Box::new(audio);
        log
    }

    /// Replace the event pacer with one on a [`FakeClock`] (`app.ctx._event_pacer
    /// = EventSpeechPacer(clock=clock)`).
    pub fn fake_pacer_clock(&mut self) -> FakeClock {
        let clock = FakeClock::new(100.0);
        self.app.ctx.event_pacer = EventSpeechPacer::with_clock(clock.boxed());
        clock
    }

    /// Shut the app down (also runs on drop).
    pub fn shutdown(&mut self) {
        self.app.shutdown();
    }
}

impl Default for TestApp {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for TestApp {
    fn drop(&mut self) {
        self.app.shutdown();
        APP_ALIVE.with(|alive| alive.set(false));
    }
}

/// A `SpeechSink` over a shared `CaptureSpeech`, so the test keeps a handle
/// to read what the app said while the app owns the sink.
pub struct SharedCapture(pub Rc<RefCell<CaptureSpeech>>);

impl SpeechSink for SharedCapture {
    fn say(&mut self, text: &str, interrupt: bool) {
        self.0.borrow_mut().say(text, interrupt)
    }
    fn say_event(&mut self, text: &str, interrupt: bool) {
        self.0.borrow_mut().say_event(text, interrupt)
    }
    fn stop_main(&mut self) {
        self.0.borrow_mut().stop_main()
    }
    fn stop_event(&mut self) {
        self.0.borrow_mut().stop_event()
    }
    fn stop(&mut self) {
        self.0.borrow_mut().stop()
    }
    fn poll(&mut self, dt: f64) {
        self.0.borrow_mut().poll(dt)
    }
    fn request_refresh(&mut self) {
        self.0.borrow_mut().request_refresh()
    }
    fn available(&self) -> bool {
        self.0.borrow().available()
    }
    fn backend_name(&self) -> String {
        self.0.borrow().backend_name()
    }
    fn has_separate_event_voice(&self) -> bool {
        self.0.borrow().has_separate_event_voice()
    }
    fn event_backend_name(&self) -> String {
        self.0.borrow().event_backend_name()
    }
    fn supports_rate(&self) -> bool {
        self.0.borrow().supports_rate()
    }
    fn supports_pitch(&self) -> bool {
        self.0.borrow().supports_pitch()
    }
    fn supports_volume(&self) -> bool {
        self.0.borrow().supports_volume()
    }
    fn event_supports_rate(&self) -> bool {
        self.0.borrow().event_supports_rate()
    }
    fn event_backend_options(&self) -> Vec<String> {
        self.0.borrow().event_backend_options()
    }
    fn select_event_backend(&mut self, name: Option<&str>) {
        self.0.borrow_mut().select_event_backend(name)
    }
    fn set_braille_only(&mut self, on: bool) {
        self.0.borrow_mut().set_braille_only(on)
    }
    fn supports_braille(&self) -> bool {
        self.0.borrow().supports_braille()
    }
    fn voice_names(&self) -> Vec<String> {
        self.0.borrow().voice_names()
    }
    fn configure(
        &mut self,
        rate: Option<f64>,
        pitch: Option<f64>,
        volume: Option<f64>,
        voice: Option<&str>,
    ) {
        self.0.borrow_mut().configure(rate, pitch, volume, voice)
    }
    fn say_adjustment_preview(&mut self, setting: &str, text: &str, interrupt: bool) -> bool {
        self.0
            .borrow_mut()
            .say_adjustment_preview(setting, text, interrupt)
    }
    fn refresh(&mut self) -> bool {
        self.0.borrow_mut().refresh()
    }
    fn shutdown(&mut self) {
        self.0.borrow_mut().shutdown()
    }
}

/// A controllable clock for the pacer (`FakeClock` in the Python tests).
#[derive(Clone)]
pub struct FakeClock {
    now: Rc<RefCell<f64>>,
}

impl FakeClock {
    pub fn new(now: f64) -> Self {
        Self {
            now: Rc::new(RefCell::new(now)),
        }
    }

    pub fn now(&self) -> f64 {
        *self.now.borrow()
    }

    pub fn set(&self, now: f64) {
        *self.now.borrow_mut() = now;
    }

    pub fn advance(&self, seconds: f64) {
        *self.now.borrow_mut() += seconds;
    }

    /// The clock as the pacer takes it.
    pub fn boxed(&self) -> ff_core::speech_pacing::Clock {
        let now = Rc::clone(&self.now);
        Box::new(move || *now.borrow())
    }
}

/// A clock that jumps `step` seconds every time it is read (the Python
/// `_stepping_clock`), so a test can assert what the RUNG did with an
/// identical line without the pacer's repeat window deciding it.
pub fn stepping_clock(step: f64) -> ff_core::speech_pacing::Clock {
    let mut now = 0.0;
    Box::new(move || {
        now += step;
        now
    })
}

/// What a [`RecordingAudio`] was asked to do.
#[derive(Debug, Default)]
pub struct AudioCalls {
    /// `play(key, volume, pan)` in order.
    pub played: Vec<(String, f64, f64)>,
    /// `set_speech_duck(level)` in order.
    pub ducks: Vec<f64>,
    pub music: Vec<(String, u32)>,
    /// Seconds into the track each `music` entry was asked to start at.
    /// Parallel to `music`; zero for a track played from the top.
    pub music_start_s: Vec<f64>,
    /// `"start"` / `"stop"` for every horn call, in order.
    pub horn: Vec<&'static str>,
    /// `set_engine_rpm(rpm, throttle)` in order. What the engine loop was
    /// told to sound like, which is not the same as what the truck model
    /// holds: a stop that settles the truck to idle without telling the audio
    /// leaves the loop roaring at highway revs.
    pub engine_rpm: Vec<(f64, f64)>,
    /// The loops sounding now, by channel: what a listener would hear.
    pub loops: std::collections::HashMap<u32, RecordedLoop>,
}

/// One running loop as [`RecordingAudio`] last left it.
#[derive(Debug, Clone, PartialEq)]
pub struct RecordedLoop {
    pub key: String,
    pub volume: f64,
    pub pan: f64,
    pub rate: f64,
}

pub type AudioLog = Rc<RefCell<AudioCalls>>;

/// An `Audio` that records the calls the app-shell tests watch and ignores
/// the rest.
#[derive(Default)]
pub struct RecordingAudio {
    log: AudioLog,
}

impl RecordingAudio {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn log(&self) -> AudioLog {
        Rc::clone(&self.log)
    }
}

impl Audio for RecordingAudio {
    fn enabled(&self) -> bool {
        false
    }
    fn backend_name(&self) -> &str {
        "recording"
    }
    fn master_volume(&self) -> f64 {
        1.0
    }
    fn sfx_volume(&self) -> f64 {
        1.0
    }
    fn music_volume(&self) -> f64 {
        1.0
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
        false
    }
    fn engine_starting(&self) -> bool {
        false
    }
    fn voice_key(&self, key: &str) -> String {
        key.to_string()
    }
    fn play_with(&mut self, key: &str, volume: f64, pan: f64) {
        self.log
            .borrow_mut()
            .played
            .push((key.to_string(), volume, pan));
    }
    fn play_bank_with(&mut self, base: &str, _fallback: &str, volume: f64, pan: f64) {
        self.play_with(base, volume, pan);
    }
    fn set_engine_duck(&mut self, _duck: f64) {}
    fn set_speech_duck(&mut self, duck: f64) {
        self.log.borrow_mut().ducks.push(duck);
    }
    fn set_engine_voice(&mut self, _classic: bool) {}
    fn set_jake_voice(&mut self, _classic: bool) {}
    fn has_asset(&mut self, _key: &str) -> bool {
        true
    }
    fn start_loop_with(&mut self, channel: u32, key: &str, volume: f64, _fade_ms: u32) {
        let mut log = self.log.borrow_mut();
        let entry = log.loops.entry(channel).or_insert_with(|| RecordedLoop {
            key: key.to_string(),
            volume,
            pan: 0.0,
            rate: 1.0,
        });
        if entry.key != key {
            *entry = RecordedLoop {
                key: key.to_string(),
                volume,
                pan: 0.0,
                rate: 1.0,
            };
        }
        entry.volume = volume;
    }
    fn set_loop_volume(&mut self, channel: u32, volume: f64) {
        if let Some(entry) = self.log.borrow_mut().loops.get_mut(&channel) {
            entry.volume = volume;
        }
    }
    fn set_loop_pan(&mut self, channel: u32, pan: f64) {
        if let Some(entry) = self.log.borrow_mut().loops.get_mut(&channel) {
            entry.pan = pan;
        }
    }
    fn set_loop_rate(&mut self, channel: u32, rate: f64) {
        if let Some(entry) = self.log.borrow_mut().loops.get_mut(&channel) {
            entry.rate = rate;
        }
    }
    fn stop_loop_with(&mut self, channel: u32, _fade_ms: u32) {
        self.log.borrow_mut().loops.remove(&channel);
    }
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
    fn engine_start_with(&mut self, _play_start_sound: bool) {}
    fn engine_stop_with(&mut self, _shutdown_sound: bool) {}
    fn update(&mut self, _dt: f64) {}
    fn set_engine_rpm_with(&mut self, rpm: f64, throttle: f64) {
        self.log.borrow_mut().engine_rpm.push((rpm, throttle));
    }
    fn set_road_noise(&mut self, _speed_mps: f64) {}
    fn set_weather_with(&mut self, _key: Option<&str>, _intensity: f64) {}
    fn set_wind(&mut self, _intensity: f64) {}
    fn set_ambient_with(&mut self, _key: Option<&str>, _volume: f64) {}
    fn horn_start(&mut self) {
        self.log.borrow_mut().horn.push("start");
    }
    fn horn_stop(&mut self) {
        self.log.borrow_mut().horn.push("stop");
    }
    fn reverse_start(&mut self) {}
    fn reverse_stop(&mut self) {}
    fn stop_world(&mut self) {}
    fn play_music_with(&mut self, track: &str, fade_ms: u32) {
        self.play_music_at(track, fade_ms, 0.0);
    }
    fn play_music_at(&mut self, track: &str, fade_ms: u32, start_s: f64) {
        let mut log = self.log.borrow_mut();
        log.music.push((track.to_string(), fade_ms));
        log.music_start_s.push(start_s);
    }
    fn play_radio_stream_with(&mut self, _url: &str, _fade_ms: u32) -> Result<(), AudioError> {
        Ok(())
    }
    fn play_music_file_with(&mut self, _path: &str, _fade_ms: u32) -> Result<(), AudioError> {
        Ok(())
    }
    fn music_playing(&self) -> bool {
        false
    }
    fn radio_now_playing(&self) -> Option<String> {
        None
    }
    fn stop_music_with(&mut self, _fade_ms: u32) {}
    fn set_volumes(&mut self, _volumes: &VolumeUpdate) {}
    fn shutdown(&mut self) {}
}
