use super::*;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

/// A sink that records calls and can be told to wedge forever.
struct StubSink {
    calls: Arc<Mutex<Vec<String>>>,
    wedge: Arc<AtomicBool>,
    entered_say: Arc<AtomicBool>,
    slow: Arc<AtomicBool>,
    /// Whether the stub's main voice reports a line still in flight.
    speaking: Arc<AtomicBool>,
    polls: Arc<AtomicUsize>,
    available: Arc<AtomicBool>,
    /// How many times the worker enumerated the event-voice options.
    enumerations: Arc<AtomicUsize>,
    /// Flipped by a test to make the main voice read as a different one.
    renamed: Arc<AtomicBool>,
    hold: Duration,
}

impl SpeechSink for StubSink {
    fn say(&mut self, text: &str, _interrupt: bool) {
        self.entered_say.store(true, Ordering::SeqCst);
        if self.wedge.load(Ordering::SeqCst) {
            // A wedged SAPI call: never returns (bounded here so the
            // test process itself can exit).
            std::thread::sleep(Duration::from_secs(600));
        }
        if self.slow.load(Ordering::SeqCst) {
            // A realistic utterance: long enough that everything a
            // test sends meanwhile is queued behind it, so the batch
            // tests are deterministic instead of racing the worker.
            std::thread::sleep(Duration::from_millis(200));
        }
        self.calls.lock().unwrap().push(format!("say {text}"));
    }
    fn say_event(&mut self, text: &str, _interrupt: bool) {
        self.calls.lock().unwrap().push(format!("event {text}"));
    }
    fn stop_main(&mut self) {
        self.calls.lock().unwrap().push("stop_main".into());
    }
    fn stop_event(&mut self) {}
    fn stop(&mut self) {}
    fn poll(&mut self, _dt: f64) {
        self.polls.fetch_add(1, Ordering::SeqCst);
    }
    fn request_refresh(&mut self) {}
    fn available(&self) -> bool {
        self.available.load(Ordering::SeqCst)
    }
    fn backend_name(&self) -> String {
        if self.renamed.load(Ordering::SeqCst) {
            "stub-two".to_string()
        } else {
            "stub".to_string()
        }
    }
    fn has_separate_event_voice(&self) -> bool {
        true
    }
    fn event_backend_name(&self) -> String {
        "stub-event".to_string()
    }
    fn supports_rate(&self) -> bool {
        true
    }
    fn supports_pitch(&self) -> bool {
        false
    }
    fn supports_volume(&self) -> bool {
        true
    }
    fn event_supports_rate(&self) -> bool {
        false
    }
    fn event_backend_options(&self) -> Vec<String> {
        self.enumerations.fetch_add(1, Ordering::SeqCst);
        vec!["stub-event".to_string()]
    }
    fn select_event_backend(&mut self, name: Option<&str>) {
        self.calls
            .lock()
            .unwrap()
            .push(format!("select_event {}", name.unwrap_or("none")));
    }
    fn set_braille_only(&mut self, on: bool) {
        self.calls
            .lock()
            .unwrap()
            .push(format!("braille_only {on}"));
    }
    fn supports_braille(&self) -> bool {
        false
    }
    fn voice_names(&self) -> Vec<String> {
        vec!["Stub Voice".to_string()]
    }
    fn configure(
        &mut self,
        rate: Option<f64>,
        _pitch: Option<f64>,
        _volume: Option<f64>,
        _voice: Option<&str>,
    ) {
        self.calls
            .lock()
            .unwrap()
            .push(format!("configure rate={rate:?}"));
    }
    fn say_adjustment_preview(&mut self, setting: &str, _t: &str, _i: bool) -> bool {
        self.calls
            .lock()
            .unwrap()
            .push(format!("preview {setting}"));
        true
    }
    fn refresh(&mut self) -> bool {
        true
    }
    fn is_speaking(&self) -> bool {
        self.speaking.load(Ordering::SeqCst)
    }
    fn shutdown(&mut self) {
        self.calls.lock().unwrap().push("shutdown".into());
    }
    fn startup_hold(&self) -> Duration {
        self.hold
    }
}

type CallLog = Arc<Mutex<Vec<String>>>;

/// The worker under test plus the stub's shared handles.
struct Rig {
    sink: ThreadedSpeech,
    calls: CallLog,
    wedge: Arc<AtomicBool>,
    entered_say: Arc<AtomicBool>,
    slow: Arc<AtomicBool>,
    speaking: Arc<AtomicBool>,
    polls: Arc<AtomicUsize>,
    available: Arc<AtomicBool>,
    /// How many sinks the factory has built: one, plus one per respawn.
    spawns: Arc<AtomicUsize>,
    enumerations: Arc<AtomicUsize>,
    renamed: Arc<AtomicBool>,
}

fn rig() -> Rig {
    rig_with_hold(Duration::ZERO)
}

fn rig_with_hold(hold: Duration) -> Rig {
    let calls: Arc<Mutex<Vec<String>>> = Arc::default();
    let wedge = Arc::new(AtomicBool::new(false));
    let entered_say = Arc::new(AtomicBool::new(false));
    let slow = Arc::new(AtomicBool::new(false));
    let speaking = Arc::new(AtomicBool::new(false));
    let polls = Arc::new(AtomicUsize::new(0));
    let available = Arc::new(AtomicBool::new(true));
    let spawns = Arc::new(AtomicUsize::new(0));
    let enumerations = Arc::new(AtomicUsize::new(0));
    let renamed = Arc::new(AtomicBool::new(false));
    let (enumerations2, renamed2) = (enumerations.clone(), renamed.clone());
    let (calls2, wedge2, entered_say2, slow2, speaking2, polls2, available2, spawns2) = (
        calls.clone(),
        wedge.clone(),
        entered_say.clone(),
        slow.clone(),
        speaking.clone(),
        polls.clone(),
        available.clone(),
        spawns.clone(),
    );
    let sink = ThreadedSpeech::spawn_with(move |after_wedge| {
        spawns2.fetch_add(1, Ordering::SeqCst);
        // Only a replacement leaves a mark in the call log: the tests
        // that pin exact call sequences never see one.
        if after_wedge {
            calls2
                .lock()
                .unwrap()
                .push("spawn after_wedge=true".to_string());
        }
        Box::new(StubSink {
            calls: calls2.clone(),
            wedge: wedge2.clone(),
            entered_say: entered_say2.clone(),
            slow: slow2.clone(),
            speaking: speaking2.clone(),
            polls: polls2.clone(),
            available: available2.clone(),
            enumerations: enumerations2.clone(),
            renamed: renamed2.clone(),
            hold,
        })
    });
    Rig {
        sink,
        calls,
        wedge,
        entered_say,
        slow,
        speaking,
        polls,
        available,
        spawns,
        enumerations,
        renamed,
    }
}

fn wait_for_polls(polls: &Arc<AtomicUsize>, count: usize) {
    for _ in 0..800 {
        if polls.load(Ordering::SeqCst) >= count {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!(
        "the health probe never reached {count} poll(s): {}",
        polls.load(Ordering::SeqCst)
    );
}

/// The three-second health probe must not enumerate the event-voice
/// options unless the probe switched a voice. Enumerating acquires every
/// Prism backend, and Prism's OneCore backend leaks a USER object, two
/// handles and about 30 KiB per acquire (measured 2026-09-12), so a probe
/// that enumerated every 3 s drained the desktop heap over a long session
/// until NVDA could no longer restart beside the game.
#[test]
fn the_health_probe_enumerates_voices_only_when_one_changed() {
    let Rig {
        sink,
        polls,
        enumerations,
        renamed,
        ..
    } = rig();
    wait_for_polls(&polls, 2);
    assert_eq!(
        enumerations.load(Ordering::SeqCst),
        1,
        "two idle health probes re-enumerated the event voices"
    );

    renamed.store(true, Ordering::SeqCst);
    let seen = polls.load(Ordering::SeqCst);
    wait_for_polls(&polls, seen + 1);
    for _ in 0..50 {
        if enumerations.load(Ordering::SeqCst) == 2 {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(
        enumerations.load(Ordering::SeqCst),
        2,
        "a probe that found a different main voice must refresh the options"
    );
    drop(sink);
}

fn wait_for(calls: &Arc<Mutex<Vec<String>>>, count: usize) {
    for _ in 0..200 {
        if calls.lock().unwrap().len() >= count {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!(
        "worker never processed {count} call(s): {:?}",
        calls.lock().unwrap()
    );
}

/// Quit must not wait through the say backlog: the shutdown flag makes
/// the worker DROP every sentence still queued, so the screen reader
/// gets the desk back after at most the in-flight utterance (Brandon,
/// 2026-08-31: closing the game took visibly longer than before the
/// speech worker existed).
#[test]
fn shutdown_skips_the_queued_backlog_instead_of_speaking_it() {
    let Rig {
        mut sink,
        calls,
        slow,
        ..
    } = rig();
    // A slow utterance in flight, and a backlog a player would
    // otherwise sit through queued behind it.
    slow.store(true, Ordering::SeqCst);
    for i in 0..50 {
        sink.say(&format!("queued {i}"), false);
    }
    let started = Instant::now();
    sink.shutdown();
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "shutdown waited through the backlog: {:?}",
        started.elapsed()
    );
    let spoken = calls.lock().unwrap();
    let says = spoken.iter().filter(|c| c.starts_with("say ")).count();
    assert!(
        says < 50,
        "every queued sentence was spoken before release ({says})"
    );
    assert!(
        spoken.iter().any(|c| c == "shutdown"),
        "the backend was never released: {spoken:?}"
    );
}

/// Quit keeps pumping events while an utterance is still in flight:
/// the three-second wait for the backend to release is sliced, and the
/// pump runs between slices, so macOS never reads the window as not
/// responding (issue 266).
#[test]
fn shutdown_keeps_pumping_while_an_utterance_is_in_flight() {
    let Rig {
        mut sink,
        calls,
        entered_say,
        slow,
        ..
    } = rig();
    slow.store(true, Ordering::SeqCst);
    sink.say("in flight", false);
    for _ in 0..500 {
        if entered_say.load(Ordering::SeqCst) {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(
        entered_say.load(Ordering::SeqCst),
        "say never reached the worker"
    );
    let mut pumps = 0u32;
    sink.shutdown_pumping(&mut || pumps += 1);
    assert!(
        pumps >= 2,
        "expected pumps between wait slices, got {pumps}"
    );
    assert!(
        calls.lock().unwrap().iter().any(|c| c == "shutdown"),
        "the backend was never released"
    );
}

/// The pumped quit lets the utterance in flight finish (bounded) before
/// the backend is released: without it, the now-prompt `stop()` would cut
/// "Installing the update..." off mid-word the way the Mac runtime test
/// caught it doing (issue 266).
#[test]
fn quit_lets_the_line_in_flight_finish_before_release() {
    let Rig {
        mut sink,
        calls,
        speaking,
        ..
    } = rig();
    speaking.store(true, Ordering::SeqCst);
    let clear = Arc::clone(&speaking);
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(300));
        clear.store(false, Ordering::SeqCst);
    });
    let started = Instant::now();
    let mut last_pump = Duration::ZERO;
    sink.shutdown_pumping(&mut || last_pump = started.elapsed());
    assert!(
        started.elapsed() >= Duration::from_millis(250),
        "the line in flight was cut off: {:?}",
        started.elapsed()
    );
    // When the last pump landed, not how many: a loaded macOS runner
    // oversleeps each 10 ms slice several times over, so a count only
    // measures the scheduler.
    assert!(
        last_pump >= Duration::from_millis(150),
        "events stopped being pumped during the finish-line wait: last pump at {last_pump:?}"
    );
    assert!(
        calls.lock().unwrap().iter().any(|c| c == "shutdown"),
        "the backend was never released"
    );
}

/// Plain `shutdown` keeps the old semantics: it does not wait for the
/// line in flight, so a voice that never reports quiet still releases
/// inside the three-second bound.
#[test]
fn plain_shutdown_does_not_wait_for_the_line() {
    let Rig {
        mut sink,
        calls,
        speaking,
        ..
    } = rig();
    speaking.store(true, Ordering::SeqCst);
    let started = Instant::now();
    sink.shutdown();
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "plain shutdown waited on the line: {:?}",
        started.elapsed()
    );
    assert!(
        calls.lock().unwrap().iter().any(|c| c == "shutdown"),
        "the backend was never released"
    );
}

#[test]
fn says_arrive_on_the_worker_in_send_order() {
    let Rig {
        mut sink, calls, ..
    } = rig();
    sink.say("one", false);
    sink.say("two", false);
    sink.say_event("three", false);
    wait_for(&calls, 3);
    assert_eq!(
        calls.lock().unwrap().as_slice(),
        ["say one", "say two", "event three"]
    );
    sink.shutdown();
    assert!(calls.lock().unwrap().iter().any(|c| c == "shutdown"));
}

/// The interrupt semantics apply to the QUEUE, not just the audio: an
/// interrupting say or a stop purges the sentences still waiting on
/// its channel, exactly as the direct backend purged their audio the
/// instant it was called. Without this the voice ran seconds behind
/// the game and kept the synthesizer busier than any pre-worker build
/// ("the screen reader got sluggish with the game open" -- Brandon,
/// 2026-08-31). The other channel's sentences are untouched.
#[test]
fn queued_says_are_purged_by_a_later_interrupt_before_they_speak() {
    let Rig {
        mut sink,
        calls,
        slow,
        ..
    } = rig();
    // A slow first utterance holds the worker; once it is mid-say the
    // rest of the sends are guaranteed to queue together behind it.
    slow.store(true, Ordering::SeqCst);
    sink.say("in flight", false);
    std::thread::sleep(Duration::from_millis(50));
    sink.say("stale", false);
    sink.say_event("event stays", false);
    sink.say("fresh", true);
    wait_for(&calls, 3);
    std::thread::sleep(Duration::from_millis(100));
    let spoken = calls.lock().unwrap().clone();
    assert!(
        spoken.iter().any(|c| c == "say in flight"),
        "the in-flight sentence was cut: {spoken:?}"
    );
    assert!(
        !spoken.iter().any(|c| c == "say stale"),
        "the purged sentence was spoken anyway: {spoken:?}"
    );
    assert!(
        spoken.iter().any(|c| c == "event event stays"),
        "the event channel was wrongly purged: {spoken:?}"
    );
    assert!(
        spoken.iter().any(|c| c == "say fresh"),
        "the interrupting say itself was lost: {spoken:?}"
    );
    sink.shutdown();
}

/// VoiceOver reads the new game window aloud as the game opens and cuts off
/// whatever was announced first, so the opening screen went unheard. The
/// worker holds the first lines, then speaks the screen current at the end.
#[test]
fn a_startup_hold_delays_the_first_lines_and_keeps_the_current_screen() {
    let hold = Duration::from_millis(400);
    let Rig {
        mut sink, calls, ..
    } = rig_with_hold(hold);
    let sent = Instant::now();
    sink.say("loading", true);
    sink.say("first screen", true);
    sink.say("first option", false);
    std::thread::sleep(Duration::from_millis(150));
    assert!(
        calls
            .lock()
            .unwrap()
            .iter()
            .all(|call| !call.starts_with("say ")),
        "a line reached the voice during the start-up hold"
    );
    wait_for(&calls, 2);
    assert!(sent.elapsed() >= hold);
    assert_eq!(
        *calls.lock().unwrap(),
        vec![
            "say first screen".to_string(),
            "say first option".to_string()
        ]
    );
}

#[test]
fn queries_answer_from_the_snapshot_without_touching_the_backend() {
    let Rig { sink, .. } = rig();
    // Give the worker a beat to publish its first snapshot.
    for _ in 0..100 {
        if sink.available() {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(sink.available());
    assert_eq!(sink.backend_name(), "stub");
    assert_eq!(sink.event_backend_name(), "stub-event");
    assert!(sink.supports_rate());
    assert!(!sink.supports_pitch());
    assert_eq!(sink.voice_names(), ["Stub Voice"]);
}

/// Chris, 2026-09-03: a SAPI purge that never returned took both voices
/// for the rest of the drive. Past the respawn threshold the stuck
/// worker is abandoned, a replacement is built with `after_wedge` set
/// (fresh backend instances in production), the player's settings are
/// replayed to it in the order the game applied them, and speech
/// resumes. The cap holds: once it is spent, a second wedge is logged
/// but not answered with yet another thread.
#[test]
fn a_worker_stuck_past_the_respawn_threshold_is_replaced_and_speech_resumes() {
    let Rig {
        mut sink,
        calls,
        wedge,
        entered_say,
        spawns,
        ..
    } = rig();
    sink.set_wedge_after_s(0.3);
    sink.set_respawn(0.6, 1);
    sink.select_event_backend(Some("stub-event"));
    sink.configure(Some(80.0), None, None, None);
    sink.set_braille_only(false);
    wait_for(&calls, 3);
    wedge.store(true, Ordering::SeqCst);
    sink.say("this one wedges the backend", false);
    for _ in 0..200 {
        if entered_say.load(Ordering::SeqCst) {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(entered_say.load(Ordering::SeqCst));
    // Before the threshold: wedged, unavailable, still one worker.
    std::thread::sleep(Duration::from_millis(400));
    sink.poll(0.016);
    assert!(!sink.available());
    assert_eq!(spawns.load(Ordering::SeqCst), 1);
    // Past it: a replacement, built as an after-wedge sink.
    std::thread::sleep(Duration::from_millis(400));
    wedge.store(false, Ordering::SeqCst);
    sink.poll(0.016);
    // The factory runs on the new thread; give it a moment to start.
    for _ in 0..200 {
        if spawns.load(Ordering::SeqCst) == 2 {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(spawns.load(Ordering::SeqCst), 2);
    sink.say("after the respawn", false);
    wait_for(&calls, 8);
    let log = calls.lock().unwrap().clone();
    let replacement = log
        .iter()
        .position(|c| c == "spawn after_wedge=true")
        .expect("a replacement worker was built");
    assert_eq!(
        &log[replacement + 1..replacement + 5],
        &[
            "select_event stub-event".to_string(),
            "configure rate=Some(80.0)".to_string(),
            "braille_only false".to_string(),
            "say after the respawn".to_string(),
        ],
        "settings replay then speech, in the game's order: {log:?}"
    );
    // The replacement's heartbeat is fresh: available again.
    std::thread::sleep(Duration::from_millis(50));
    sink.poll(0.016);
    assert!(sink.available());
    // The cap: a second wedge on the replacement is not respawned.
    wedge.store(true, Ordering::SeqCst);
    sink.say("wedges the replacement", false);
    std::thread::sleep(Duration::from_millis(900));
    sink.poll(0.016);
    assert_eq!(spawns.load(Ordering::SeqCst), 2);
    assert!(!sink.available());
    let quitting = Instant::now();
    sink.shutdown();
    assert!(quitting.elapsed() < Duration::from_secs(5));
}

#[test]
fn a_wedged_backend_never_blocks_a_say_and_the_watchdog_notices() {
    let Rig {
        mut sink,
        wedge,
        entered_say,
        ..
    } = rig();
    // One second of patience instead of eight: the test is about what
    // the watchdog does once the heartbeat is stale, not about how long
    // the shipped value gives a slow screen reader.
    sink.set_wedge_after_s(1.0);
    wedge.store(true, Ordering::SeqCst);
    sink.say("this one wedges the backend", false);
    for _ in 0..200 {
        if entered_say.load(Ordering::SeqCst) {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(
        entered_say.load(Ordering::SeqCst),
        "speech worker never entered the deliberately wedged call"
    );
    // The whole point: further speech returns instantly while the
    // backend sits inside its stuck call.
    let started = Instant::now();
    for i in 0..300 {
        sink.say(&format!("line {i}"), false);
    }
    sink.stop();
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "queueing speech must never wait on the backend"
    );
    // The watchdog reads the stale heartbeat as unavailable well before
    // it logs the wedge.
    std::thread::sleep(Duration::from_secs(2));
    sink.poll(0.016);
    assert!(!sink.available());
    // Bounded replies answer pessimistically instead of hanging.
    let answered = Instant::now();
    assert!(!sink.say_adjustment_preview("speech_rate", "preview", false));
    assert!(answered.elapsed() < Duration::from_secs(4));
    // Shutdown of a wedged worker is bounded too: abandoned, not joined.
    let quitting = Instant::now();
    sink.shutdown();
    assert!(quitting.elapsed() < Duration::from_secs(5));
}

#[test]
fn the_worker_polls_the_backend_on_its_own_cadence() {
    let Rig {
        mut sink, polls, ..
    } = rig();
    std::thread::sleep(Duration::from_millis(900));
    assert_eq!(
        polls.load(Ordering::SeqCst),
        0,
        "the idle worker must not probe Prism every 200 ms"
    );

    for _ in 0..300 {
        if polls.load(Ordering::SeqCst) == 1 {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(
        polls.load(Ordering::SeqCst),
        1,
        "the autonomous three-second health probe never ran"
    );

    // Returning focus is the one reason to probe before the ordinary
    // three-second health interval: the player may have switched screen
    // readers while another window was active.
    sink.request_refresh();
    for _ in 0..100 {
        if polls.load(Ordering::SeqCst) == 2 {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(polls.load(Ordering::SeqCst), 2);
    sink.shutdown();
}

#[test]
fn utterance_failure_status_is_immediate_and_idle_poll_recovers_it() {
    let Rig {
        mut sink,
        calls,
        available,
        ..
    } = rig();
    for _ in 0..100 {
        if sink.available() {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }

    // Stand in for a live utterance discovering that NVDA disappeared.
    available.store(false, Ordering::SeqCst);
    sink.say("backend failure", false);
    wait_for(&calls, 1);
    for _ in 0..100 {
        if !sink.available() {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(!sink.available());

    // A backend that returns while the game stays focused is found by
    // the ordinary autonomous health poll.
    available.store(true, Ordering::SeqCst);
    for _ in 0..350 {
        if sink.available() {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(sink.available());
    sink.shutdown();
}

#[test]
fn bounded_replies_reach_the_backend_when_it_is_healthy() {
    let Rig {
        mut sink, calls, ..
    } = rig();
    assert!(sink.say_adjustment_preview("speech_rate", "faster", false));
    assert!(sink.refresh());
    wait_for(&calls, 1);
    assert!(calls.lock().unwrap()[0].starts_with("preview"));
    sink.shutdown();
}
