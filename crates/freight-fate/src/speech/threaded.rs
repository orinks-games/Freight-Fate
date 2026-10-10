//! The speech worker: Prism on its own thread, so a wedged screen-reader
//! call can never freeze the game.
//!
//! # Why
//!
//! Every spoken line used to be a synchronous Prism/SAPI call on the main
//! game loop. The one time that call did not return -- tester Shane,
//! 2026-08-30, an event-voice interrupt at an I-77 on-ramp -- the whole
//! game froze with it, silently and permanently: the log's last line is
//! the transcript entry written immediately before `say_event`, and the
//! only thing left to do was kill the process. A screen reader or SAPI
//! wedging is outside this game's control; the game staying drivable
//! through it is not.
//!
//! # The threading invariant, kept
//!
//! Every Prism context is created and used on its own worker thread;
//! [`super::live::Speech`] stays `!Send`, and the game holds only channels.
//! Recovery abandons a generation that may still be inside a native call.
//! Once that call returns, it cannot dispatch further operations. Its native
//! objects are retained until process exit, since even release may wedge.
//!
//! # Semantics preserved
//!
//! Commands are processed strictly in send order by one worker, so the
//! relative order of says, interrupts, and stops is exactly what it was.
//! What changes is who waits: nobody. If the backend wedges, the queue
//! bounds itself (a full queue drops new SAY lines -- the transcript and
//! review log already have them from the main side -- and never grows),
//! the watchdog in [`ThreadedSpeech::poll`] says so once in the log, and
//! the game keeps driving on earcons until the backend comes back.
//!
//! The two calls that genuinely need an answer (`refresh`,
//! `say_adjustment_preview`) wait a BOUNDED couple of seconds and answer
//! pessimistically on timeout -- a settings-menu hiccup, never a freeze.
//!
//! # Respawn
//!
//! A wedge that never clears used to cost speech for the rest of the
//! session: tester Chris, 2026-09-03, pressed Control to stop the road
//! voice mid-sentence, the SAPI purge under it never returned, and both
//! voices were gone for the remaining half hour of the drive (NVDA itself
//! stayed fine -- it had just spoken the game's previous line). Now, once
//! the heartbeat has been stale for [`RESPAWN_AFTER_S`], the watchdog
//! abandons the stuck worker and starts a replacement on FRESH backend
//! instances ([`super::live::Speech::new_after_wedge`]): Prism caches one
//! instance per backend across contexts, so a replacement that re-acquired
//! SAPI would block on the same stuck voice. The player's speech settings
//! are replayed to the new worker before readiness is acknowledged, and the log reads "stopped responding",
//! "abandoned ... replacement", "recovered". Bounded to [`MAX_RESPAWNS`]
//! per session so a backend that wedges on every line cannot spawn threads
//! forever. Verified against Prism 0.18.2 before building (a second
//! context speaks while the first is alive, a created SAPI instance is idle
//! while the cached one is mid-utterance, and the replacement outlives the
//! abandoned context's eventual shutdown).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, TrySendError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use super::SpeechSink;

/// How long a silent worker is allowed before the watchdog calls it wedged.
const WEDGE_AFTER_S: f64 = 8.0;
/// How long a wedged worker is given to come back before it is abandoned
/// and replaced. Longer than a screen reader's own freeze recovery (NVDA's
/// watchdog gives its core about ten seconds), so a stall that will clear
/// on its own is not answered with a second voice.
const RESPAWN_AFTER_S: f64 = 20.0;
/// Replacement workers per session, at most.
const MAX_RESPAWNS: u32 = 3;
/// Command queue depth; beyond it, new say lines are dropped, not queued.
const QUEUE_DEPTH: usize = 256;
/// How often the worker asks Prism to re-check the live speech backends.
const HEALTH_POLL: Duration = Duration::from_secs(3);
/// Bounded wait for the two calls that need an answer.
const REPLY_WAIT: Duration = Duration::from_secs(2);

/// How long a pumped quit lets the utterance in flight finish before the
/// backend is released anyway: long enough for "Installing the update. The
/// game closes and restarts by itself." to play out.
const FINISH_LINE_BOUND: Duration = Duration::from_secs(5);
/// The main-thread wait bound on a pumped shutdown: the line finishes,
/// then the release itself gets the same three seconds it always had.
const SHUTDOWN_PUMPED_BOUND: Duration = FINISH_LINE_BOUND.saturating_add(Duration::from_secs(3));

enum Command {
    ReplayComplete,
    Say {
        text: String,
        interrupt: bool,
    },
    SayEvent {
        text: String,
        interrupt: bool,
    },
    StopMain,
    StopEvent,
    Stop,
    RequestRefresh,
    Refresh {
        reply: mpsc::Sender<bool>,
    },
    Configure {
        rate: Option<f64>,
        pitch: Option<f64>,
        volume: Option<f64>,
        voice: Option<String>,
    },
    SelectEventBackend(Option<String>),
    SetBrailleOnly(bool),
    Preview {
        setting: String,
        text: String,
        interrupt: bool,
        reply: mpsc::Sender<bool>,
    },
    Shutdown {
        done: mpsc::Sender<()>,
        /// Let the line in flight finish (bounded by
        /// [`FINISH_LINE_BOUND`]) before releasing the backend. Only the
        /// pumped quit path asks for it: an unpumped `stop()` used to
        /// deadlock into the three-second timeout, which incidentally let
        /// the line play out; now that the pump drains the main queue,
        /// `stop()` lands immediately and would cut it off (issue 266).
        finish_line: bool,
    },
}

/// The query answers, published by the worker after every state change so
/// the main thread can answer without asking Prism anything.
#[derive(Clone, Default)]
struct Snapshot {
    available: bool,
    backend_name: String,
    event_backend_name: String,
    has_separate_event_voice: bool,
    supports_rate: bool,
    supports_pitch: bool,
    supports_volume: bool,
    event_supports_rate: bool,
    supports_braille: bool,
    event_backend_options: Vec<String>,
    voice_names: Vec<String>,
}

/// Apply the interrupt semantics to a drained batch, the way the direct
/// backend applied them to live audio: an interrupting say purges the
/// pending sentences on its own channel, and the stop commands purge
/// everything theirs. Only says are ever dropped -- every other command
/// (configure, refresh, previews, shutdown) keeps its place and order.
fn coalesce(batch: &mut Vec<Command>) {
    let mut cut_main: Option<usize> = None;
    let mut cut_event: Option<usize> = None;
    for (index, command) in batch.iter().enumerate() {
        match command {
            Command::Say {
                interrupt: true, ..
            }
            | Command::StopMain => cut_main = Some(index),
            Command::SayEvent {
                interrupt: true, ..
            }
            | Command::StopEvent => cut_event = Some(index),
            Command::Stop => {
                cut_main = Some(index);
                cut_event = Some(index);
            }
            _ => {}
        }
    }
    let mut index = 0;
    batch.retain(|command| {
        let keep = match command {
            Command::Say { .. } => cut_main.is_none_or(|cut| index >= cut),
            Command::SayEvent { .. } => cut_event.is_none_or(|cut| index >= cut),
            _ => true,
        };
        index += 1;
        keep
    });
}

// Query methods may enter native code too. Stop the snapshot sequence as
// soon as an abandoned in-flight query returns.
macro_rules! query {
    ($abandoned:expr, $operation:expr) => {{
        if $abandoned.load(Ordering::Acquire) {
            return false;
        }
        let value = $operation;
        if $abandoned.load(Ordering::Acquire) {
            return false;
        }
        value
    }};
}

fn publish(
    snapshot: &Arc<Mutex<Snapshot>>,
    inner: &dyn SpeechSink,
    abandoned: &AtomicBool,
) -> bool {
    let fresh = Snapshot {
        available: query!(abandoned, inner.available()),
        backend_name: query!(abandoned, inner.backend_name()),
        event_backend_name: query!(abandoned, inner.event_backend_name()),
        has_separate_event_voice: query!(abandoned, inner.has_separate_event_voice()),
        supports_rate: query!(abandoned, inner.supports_rate()),
        supports_pitch: query!(abandoned, inner.supports_pitch()),
        supports_volume: query!(abandoned, inner.supports_volume()),
        event_supports_rate: query!(abandoned, inner.event_supports_rate()),
        supports_braille: query!(abandoned, inner.supports_braille()),
        event_backend_options: query!(abandoned, inner.event_backend_options()),
        voice_names: query!(abandoned, inner.voice_names()),
    };
    *snapshot.lock().expect("speech snapshot lock") = fresh;
    true
}

/// Refresh the answers that can change when a live utterance discovers a
/// vanished backend, without enumerating backend options or installed voices.
fn publish_status(
    snapshot: &Arc<Mutex<Snapshot>>,
    inner: &dyn SpeechSink,
    abandoned: &AtomicBool,
) -> bool {
    // Native backend queries stay outside the shared lock. Even if Prism is
    // slow, the game thread can continue answering from the prior snapshot.
    let available = query!(abandoned, inner.available());
    let backend_name = query!(abandoned, inner.backend_name());
    let event_backend_name = query!(abandoned, inner.event_backend_name());
    let has_separate_event_voice = query!(abandoned, inner.has_separate_event_voice());
    let supports_rate = query!(abandoned, inner.supports_rate());
    let supports_pitch = query!(abandoned, inner.supports_pitch());
    let supports_volume = query!(abandoned, inner.supports_volume());
    let event_supports_rate = query!(abandoned, inner.event_supports_rate());
    let supports_braille = query!(abandoned, inner.supports_braille());
    let mut current = snapshot.lock().expect("speech snapshot lock");
    current.available = available;
    current.backend_name = backend_name;
    current.event_backend_name = event_backend_name;
    current.has_separate_event_voice = has_separate_event_voice;
    current.supports_rate = supports_rate;
    current.supports_pitch = supports_pitch;
    current.supports_volume = supports_volume;
    current.event_supports_rate = event_supports_rate;
    current.supports_braille = supports_braille;
    true
}

/// What a health probe publishes: the cheap status always, the
/// enumerations (event-voice options, installed voices) only when the probe
/// switched a voice, because that is the only time they can differ.
///
/// Enumerating acquires every Prism backend, and Prism's OneCore backend
/// leaks one USER object, two handles and about 30 KiB per acquire
/// (measured 2026-09-12 with `prism --example handle_leak_probe`). Published
/// on every 3 s probe that was 1,200 USER objects an hour: an evening's
/// session walked into the 10,000-object process limit and drained the
/// desktop heap, and NVDA could no longer restart beside the game.
fn publish_after_probe(
    snapshot: &Arc<Mutex<Snapshot>>,
    inner: &dyn SpeechSink,
    abandoned: &AtomicBool,
) -> bool {
    let voices_before = {
        let current = snapshot.lock().expect("speech snapshot lock");
        (
            current.backend_name.clone(),
            current.event_backend_name.clone(),
        )
    };
    if !publish_status(snapshot, inner, abandoned) {
        return false;
    }
    let voices_after = {
        let current = snapshot.lock().expect("speech snapshot lock");
        (
            current.backend_name.clone(),
            current.event_backend_name.clone(),
        )
    };
    if voices_before == voices_after {
        return true;
    }
    publish(snapshot, inner, abandoned)
}

/// Builds the sink a worker drives. The argument is true for a replacement
/// worker started after a wedge, which the production factory answers with
/// fresh backend instances.
type SinkFactory = dyn Fn(bool) -> Box<dyn SpeechSink> + Send + Sync;

/// The main thread's handles on one worker thread.
struct Worker {
    commands: mpsc::SyncSender<Command>,
    snapshot: Arc<Mutex<Snapshot>>,
    heartbeat: Arc<Mutex<Instant>>,
    shutting_down: Arc<std::sync::atomic::AtomicBool>,
    abandoned: Arc<AtomicBool>,
    ready: Arc<AtomicBool>,
}

/// `configure`'s arguments: rate, pitch, volume, voice.
type ConfigureArgs = (Option<f64>, Option<f64>, Option<f64>, Option<String>);

/// The settings the main thread has sent so far, replayed to a replacement
/// worker in the order `apply_speech_settings` sends them.
#[derive(Default)]
struct Replay {
    event_pref: Option<Option<String>>,
    configure: Option<ConfigureArgs>,
    braille_only: Option<bool>,
}

/// A [`SpeechSink`] whose Prism lives on a worker thread.
pub struct ThreadedSpeech {
    commands: mpsc::SyncSender<Command>,
    snapshot: Arc<Mutex<Snapshot>>,
    heartbeat: Arc<Mutex<Instant>>,
    abandoned: Arc<AtomicBool>,
    ready: Arc<AtomicBool>,
    factory: Arc<SinkFactory>,
    replay: Replay,
    /// [`RESPAWN_AFTER_S`], test-adjustable like the wedge threshold.
    respawn_after_s: f64,
    respawns: u32,
    max_respawns: u32,
    /// Set by [`shutdown`](SpeechSink::shutdown) BEFORE the shutdown
    /// command is queued: the worker checks it per command and drops
    /// queued sentences instead of speaking them. Without it, quitting
    /// waited through every queued synchronous say before the release --
    /// which read as the game "taking longer to hand the screen reader
    /// back" than before the worker existed (Brandon, 2026-08-31).
    shutting_down: Arc<std::sync::atomic::AtomicBool>,
    /// Watchdog latch: the wedge is reported once, and once more only if
    /// the worker recovers and wedges again.
    wedged: bool,
    /// [`WEDGE_AFTER_S`], except in the one test that wedges the worker on
    /// purpose and should not have to sit through eight real seconds to
    /// watch the watchdog notice.
    wedge_after_s: f64,
    dropped_lines: u64,
}

impl ThreadedSpeech {
    /// The production sink: Prism built inside the worker. A replacement
    /// worker gets fresh backend instances (see the module docs).
    pub fn spawn() -> Self {
        Self::spawn_with(|after_wedge| {
            if after_wedge {
                Box::new(super::live::Speech::new_after_wedge())
            } else {
                Box::new(super::live::Speech::new())
            }
        })
    }

    /// A worker around any sink factory -- the tests hand in fakes that
    /// block or record. The factory runs ON the worker thread, which is
    /// what lets the `!Send` production sink live there; it is kept so a
    /// replacement worker can be built from it after a wedge.
    pub fn spawn_with<F>(factory: F) -> Self
    where
        F: Fn(bool) -> Box<dyn SpeechSink> + Send + Sync + 'static,
    {
        let factory: Arc<SinkFactory> = Arc::new(factory);
        let Worker {
            commands,
            snapshot,
            heartbeat,
            shutting_down,
            abandoned,
            ready,
        } = Self::start_worker(&factory, false);
        ThreadedSpeech {
            commands,
            snapshot,
            heartbeat,
            abandoned,
            ready,
            factory,
            replay: Replay::default(),
            respawn_after_s: RESPAWN_AFTER_S,
            respawns: 0,
            max_respawns: MAX_RESPAWNS,
            shutting_down,
            wedged: false,
            wedge_after_s: WEDGE_AFTER_S,
            dropped_lines: 0,
        }
    }

    /// Start one worker thread. `after_wedge` is handed to the factory.
    fn start_worker(factory: &Arc<SinkFactory>, after_wedge: bool) -> Worker {
        let (commands, rx) = mpsc::sync_channel::<Command>(QUEUE_DEPTH);
        let snapshot: Arc<Mutex<Snapshot>> = Arc::default();
        let heartbeat = Arc::new(Mutex::new(Instant::now()));
        let shutting_down = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let abandoned = Arc::new(AtomicBool::new(false));
        let ready = Arc::new(AtomicBool::new(false));
        let worker_abandoned = Arc::clone(&abandoned);
        let worker_ready = Arc::clone(&ready);
        let worker_snapshot = Arc::clone(&snapshot);
        let worker_heartbeat = Arc::clone(&heartbeat);
        let worker_shutting_down = Arc::clone(&shutting_down);
        let factory = Arc::clone(factory);
        std::thread::Builder::new()
            .name("speech".to_string())
            .spawn(move || {
                let mut inner = factory(after_wedge);
                // Abandoned native objects cannot be safely stopped or
                // destroyed: either operation may re-enter a poisoned backend.
                // Retention is bounded by MAX_RESPAWNS, and ends with the process.
                macro_rules! active {
                    () => {
                        if worker_abandoned.load(Ordering::Acquire) {
                            std::mem::forget(inner);
                            return;
                        }
                    };
                }
                macro_rules! call {
                    ($operation:expr) => {{
                        active!();
                        let result = $operation;
                        active!();
                        result
                    }};
                }
                active!();
                call!(publish(&worker_snapshot, inner.as_ref(), &worker_abandoned));
                if !after_wedge {
                    worker_ready.store(true, Ordering::Release);
                }
                let beat = || {
                    *worker_heartbeat.lock().expect("speech heartbeat lock") = Instant::now();
                };
                // Lines sent meanwhile wait in the queue and are coalesced
                // like any other batch, so the player hears the screen that
                // is current when the hold ends.
                let hold = call!(inner.startup_hold());
                if !after_wedge && !hold.is_zero() {
                    beat();
                    std::thread::sleep(hold);
                }
                let mut last_health_poll = Instant::now();
                loop {
                    active!();
                    beat();
                    // Quitting: everything still queued is a sentence the
                    // player chose not to wait for. Skip the says, keep
                    // answering everything that carries a reply, and let
                    // the Shutdown command through to the release.
                    let until_health_poll = HEALTH_POLL.saturating_sub(last_health_poll.elapsed());
                    let first = match rx.recv_timeout(until_health_poll) {
                        Ok(command) => command,
                        Err(RecvTimeoutError::Timeout) => {
                            // Backend discovery can enumerate Prism voices
                            // and cross COM boundaries. Do it at the promised
                            // three-second cadence, not at a 200 ms wake-up
                            // cadence that competes with NVDA on slower PCs.
                            let elapsed = last_health_poll.elapsed();
                            beat();
                            call!(inner.poll(elapsed.as_secs_f64()));
                            call!(publish_after_probe(
                                &worker_snapshot,
                                inner.as_ref(),
                                &worker_abandoned
                            ));
                            last_health_poll = Instant::now();
                            continue;
                        }
                        Err(RecvTimeoutError::Disconnected) => {
                            call!(inner.shutdown());
                            return;
                        }
                    };
                    // Drain whatever else is already queued and apply the
                    // interrupt semantics BEFORE speaking. The direct
                    // backend purged pending audio the instant an
                    // interrupting say arrived; a queue that speaks every
                    // sentence in arrival order instead runs the voice
                    // seconds behind the game and keeps the synthesizer
                    // busier than any pre-worker build -- which is what
                    // "the screen reader got sluggish with the game open"
                    // was (Brandon, 2026-08-31). A sentence dropped here is
                    // one the purge would have cut off mid-word anyway.
                    let mut batch = vec![first];
                    while let Ok(command) = rx.try_recv() {
                        batch.push(command);
                    }
                    coalesce(&mut batch);
                    for command in batch {
                        active!();
                        let draining = worker_shutting_down.load(Ordering::Acquire);
                        beat();
                        match command {
                            Command::ReplayComplete => worker_ready.store(true, Ordering::Release),
                            Command::Say { text, interrupt } => {
                                if !draining {
                                    call!(inner.say(&text, interrupt));
                                    call!(publish_status(
                                        &worker_snapshot,
                                        inner.as_ref(),
                                        &worker_abandoned
                                    ));
                                }
                            }
                            Command::SayEvent { text, interrupt } => {
                                if !draining {
                                    call!(inner.say_event(&text, interrupt));
                                    call!(publish_status(
                                        &worker_snapshot,
                                        inner.as_ref(),
                                        &worker_abandoned
                                    ));
                                }
                            }
                            Command::StopMain => call!(inner.stop_main()),
                            Command::StopEvent => call!(inner.stop_event()),
                            Command::Stop => call!(inner.stop()),
                            Command::RequestRefresh => {
                                // Focus returning is the one deliberate early
                                // probe: the player may have changed screen
                                // readers in the other window.
                                call!(inner.request_refresh());
                                call!(inner.poll(0.0));
                                call!(publish_after_probe(
                                    &worker_snapshot,
                                    inner.as_ref(),
                                    &worker_abandoned
                                ));
                                last_health_poll = Instant::now();
                            }
                            Command::Refresh { reply } => {
                                let changed = call!(inner.refresh());
                                call!(publish(&worker_snapshot, inner.as_ref(), &worker_abandoned));
                                let _ = reply.send(changed);
                            }
                            Command::Configure {
                                rate,
                                pitch,
                                volume,
                                voice,
                            } => {
                                call!(inner.configure(rate, pitch, volume, voice.as_deref()));
                                call!(publish(&worker_snapshot, inner.as_ref(), &worker_abandoned));
                            }
                            Command::SelectEventBackend(name) => {
                                call!(inner.select_event_backend(name.as_deref()));
                                call!(publish(&worker_snapshot, inner.as_ref(), &worker_abandoned));
                            }
                            Command::SetBrailleOnly(on) => call!(inner.set_braille_only(on)),
                            Command::Preview {
                                setting,
                                text,
                                interrupt,
                                reply,
                            } => {
                                let spoke =
                                    call!(inner.say_adjustment_preview(&setting, &text, interrupt));
                                let _ = reply.send(spoke);
                            }
                            Command::Shutdown { done, finish_line } => {
                                if finish_line {
                                    let started = Instant::now();
                                    while call!(inner.is_speaking())
                                        && started.elapsed() < FINISH_LINE_BOUND
                                    {
                                        std::thread::sleep(Duration::from_millis(20));
                                    }
                                }
                                call!(inner.shutdown());
                                let _ = done.send(());
                                return;
                            }
                        }
                    }
                    // A steady stream of speech commands must not starve the
                    // health check indefinitely.
                    if last_health_poll.elapsed() >= HEALTH_POLL {
                        let elapsed = last_health_poll.elapsed();
                        beat();
                        call!(inner.poll(elapsed.as_secs_f64()));
                        call!(publish_after_probe(
                            &worker_snapshot,
                            inner.as_ref(),
                            &worker_abandoned
                        ));
                        last_health_poll = Instant::now();
                    }
                }
            })
            .expect("the speech worker spawns");
        Worker {
            commands,
            snapshot,
            heartbeat,
            shutting_down,
            abandoned,
            ready,
        }
    }

    /// Abandon the wedged worker and start a replacement on fresh voices.
    ///
    /// The stuck thread is left where it is: it holds the old context, and
    /// if its call ever returns it stops dispatching native work. Its native
    /// objects are retained, because even their destruction may block again.
    fn respawn(&mut self, stale_s: f64) {
        self.respawns += 1;
        log::error!(
            "speech worker abandoned after {stale_s:.0}s inside a stuck speech call; \
             starting a replacement with fresh voices (attempt {} of {})",
            self.respawns,
            self.max_respawns
        );
        self.shutting_down
            .store(true, std::sync::atomic::Ordering::Relaxed);
        self.abandoned.store(true, Ordering::Release);
        let Worker {
            commands,
            snapshot,
            heartbeat,
            shutting_down,
            abandoned,
            ready,
        } = Self::start_worker(&self.factory, true);
        self.commands = commands;
        self.snapshot = snapshot;
        self.heartbeat = heartbeat;
        self.shutting_down = shutting_down;
        self.abandoned = abandoned;
        self.ready = ready;
        self.dropped_lines = 0;
        // The player's settings, in the order the game applied them.
        if let Some(pref) = self.replay.event_pref.clone() {
            self.send_lossy(Command::SelectEventBackend(pref));
        }
        if let Some((rate, pitch, volume, voice)) = self.replay.configure.clone() {
            self.send_lossy(Command::Configure {
                rate,
                pitch,
                volume,
                voice,
            });
        }
        if let Some(on) = self.replay.braille_only {
            self.send_lossy(Command::SetBrailleOnly(on));
        }
        self.send_lossy(Command::ReplayComplete);
    }

    /// Shorten the watchdog's patience. Test-only: the production value is
    /// [`WEDGE_AFTER_S`], and it must stay longer than [`HEALTH_POLL`] or an
    /// idle, healthy worker reads as wedged between two heartbeats.
    #[cfg(test)]
    fn set_wedge_after_s(&mut self, seconds: f64) {
        self.wedge_after_s = seconds;
    }

    /// Test-only: how long a wedge lasts before the worker is replaced, and
    /// how many replacements are allowed.
    #[cfg(test)]
    fn set_respawn(&mut self, after_s: f64, max: u32) {
        self.respawn_after_s = after_s;
        self.max_respawns = max;
    }

    fn snapshot(&self) -> Snapshot {
        self.snapshot.lock().expect("speech snapshot lock").clone()
    }

    /// Queue a command without ever waiting. A full queue means the worker
    /// is wedged inside the backend; a say line is then dropped (the
    /// transcript and review log keep it), anything else is tried anyway.
    fn send_lossy(&mut self, command: Command) {
        if let Err(TrySendError::Full(command)) = self.commands.try_send(command) {
            if matches!(command, Command::Say { .. } | Command::SayEvent { .. }) {
                self.dropped_lines += 1;
                if self.dropped_lines.is_power_of_two() {
                    log::warn!(
                        "speech queue full: {} line(s) dropped while the backend is stalled",
                        self.dropped_lines
                    );
                }
            }
            // Non-say commands on a full queue are lost too, but the queue
            // only fills when the backend has been gone for hundreds of
            // lines; the periodic snapshot republish squares state back up
            // when it returns.
        }
    }

    fn bounded_reply(&mut self, build: impl FnOnce(mpsc::Sender<bool>) -> Command) -> bool {
        let (reply_tx, reply_rx) = mpsc::channel();
        self.send_lossy(build(reply_tx));
        reply_rx.recv_timeout(REPLY_WAIT).unwrap_or(false)
    }
}

impl SpeechSink for ThreadedSpeech {
    fn say(&mut self, text: &str, interrupt: bool) {
        self.send_lossy(Command::Say {
            text: text.to_string(),
            interrupt,
        });
    }

    fn say_event(&mut self, text: &str, interrupt: bool) {
        self.send_lossy(Command::SayEvent {
            text: text.to_string(),
            interrupt,
        });
    }

    fn stop_main(&mut self) {
        self.send_lossy(Command::StopMain);
    }

    fn stop_event(&mut self) {
        self.send_lossy(Command::StopEvent);
    }

    fn stop(&mut self) {
        self.send_lossy(Command::Stop);
    }

    /// The main thread's poll is now a WATCHDOG, not a backend call: it
    /// judges the worker by its heartbeat and says so, once, when the
    /// backend has stopped answering. The game keeps running either way --
    /// that sentence is this module's whole reason to exist.
    fn poll(&mut self, _dt: f64) {
        let stale = self
            .heartbeat
            .lock()
            .expect("speech heartbeat lock")
            .elapsed()
            .as_secs_f64();
        if stale > self.wedge_after_s && !self.wedged {
            self.wedged = true;
            log::error!(
                "speech backend stopped responding {stale:.0}s ago (a wedged screen reader \
                 or SAPI call); the game continues without speech until it returns"
            );
        } else if stale <= self.wedge_after_s
            && self.wedged
            && self.ready.load(Ordering::Acquire)
            && self.snapshot().available
        {
            self.wedged = false;
            log::warn!("speech backend recovered");
        }
        // Recovery requires completion of constructor and settings replay,
        // plus a usable voice. A newly allocated heartbeat proves neither.
        if stale > self.respawn_after_s && self.respawns < self.max_respawns {
            self.respawn(stale);
        }
    }

    fn request_refresh(&mut self) {
        self.send_lossy(Command::RequestRefresh);
    }

    fn available(&self) -> bool {
        // A wedged worker cannot speak, whatever the backend last claimed.
        let stale = self
            .heartbeat
            .lock()
            .expect("speech heartbeat lock")
            .elapsed()
            .as_secs_f64();
        self.ready.load(Ordering::Acquire)
            && self.snapshot().available
            && stale <= self.wedge_after_s
    }

    fn backend_name(&self) -> String {
        self.snapshot().backend_name
    }

    fn has_separate_event_voice(&self) -> bool {
        self.snapshot().has_separate_event_voice
    }

    fn event_backend_name(&self) -> String {
        self.snapshot().event_backend_name
    }

    fn supports_rate(&self) -> bool {
        self.snapshot().supports_rate
    }

    fn supports_pitch(&self) -> bool {
        self.snapshot().supports_pitch
    }

    fn supports_volume(&self) -> bool {
        self.snapshot().supports_volume
    }

    fn event_supports_rate(&self) -> bool {
        self.snapshot().event_supports_rate
    }

    fn event_backend_options(&self) -> Vec<String> {
        self.snapshot().event_backend_options
    }

    fn select_event_backend(&mut self, name: Option<&str>) {
        let name = name.map(str::to_string);
        self.replay.event_pref = Some(name.clone());
        self.send_lossy(Command::SelectEventBackend(name));
    }

    fn set_braille_only(&mut self, on: bool) {
        self.replay.braille_only = Some(on);
        self.send_lossy(Command::SetBrailleOnly(on));
    }

    fn supports_braille(&self) -> bool {
        self.snapshot().supports_braille
    }

    fn voice_names(&self) -> Vec<String> {
        self.snapshot().voice_names
    }

    fn configure(
        &mut self,
        rate: Option<f64>,
        pitch: Option<f64>,
        volume: Option<f64>,
        voice: Option<&str>,
    ) {
        let voice = voice.map(str::to_string);
        self.replay.configure = Some((rate, pitch, volume, voice.clone()));
        self.send_lossy(Command::Configure {
            rate,
            pitch,
            volume,
            voice,
        });
    }

    fn say_adjustment_preview(&mut self, setting: &str, text: &str, interrupt: bool) -> bool {
        let setting = setting.to_string();
        let text = text.to_string();
        self.bounded_reply(|reply| Command::Preview {
            setting,
            text,
            interrupt,
            reply,
        })
    }

    fn refresh(&mut self) -> bool {
        let (reply_tx, reply_rx) = mpsc::channel();
        self.send_lossy(Command::Refresh { reply: reply_tx });
        // Re-detection really can take a beat; give it longer than the
        // preview, still bounded.
        reply_rx
            .recv_timeout(Duration::from_secs(4))
            .unwrap_or(false)
    }

    fn shutdown(&mut self) {
        // The flag first, then the command: the worker skips every say
        // still queued ahead of it, so the wait below covers one in-flight
        // utterance at most, not the whole backlog.
        self.shutting_down
            .store(true, std::sync::atomic::Ordering::Relaxed);
        let (done_tx, done_rx) = mpsc::channel();
        if self
            .commands
            .try_send(Command::Shutdown {
                done: done_tx,
                finish_line: false,
            })
            .is_ok()
        {
            // Give the backend a bounded chance to release cleanly; a
            // wedged one is abandoned, which is exactly what quitting a
            // frozen game by hand used to do -- minus freezing the game.
            let _ = done_rx.recv_timeout(Duration::from_secs(3));
        }
    }

    fn shutdown_pumping(&mut self, pump: &mut dyn FnMut()) {
        // The flag first, then the command: the worker skips every say
        // still queued ahead of it, so the wait below covers the utterance
        // in flight finishing (bounded by FINISH_LINE_BOUND), not the whole
        // backlog.
        self.shutting_down
            .store(true, std::sync::atomic::Ordering::Relaxed);
        let (done_tx, done_rx) = mpsc::channel();
        if self
            .commands
            .try_send(Command::Shutdown {
                done: done_tx,
                finish_line: true,
            })
            .is_ok()
        {
            // Give the backend a bounded chance to release cleanly; a
            // wedged one is abandoned, which is exactly what quitting a
            // frozen game by hand used to do -- minus freezing the game.
            // The wait is sliced so quit can pump events between slices
            // and macOS never reads the window as not responding.
            let started = Instant::now();
            loop {
                match done_rx.recv_timeout(Duration::from_millis(10)) {
                    Ok(()) | Err(RecvTimeoutError::Disconnected) => break,
                    Err(RecvTimeoutError::Timeout) => {
                        if started.elapsed() >= SHUTDOWN_PUMPED_BOUND {
                            break;
                        }
                        pump();
                    }
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "threaded/recovery_tests.rs"]
mod recovery_tests;

#[cfg(test)]
#[path = "threaded/tests.rs"]
mod tests;
