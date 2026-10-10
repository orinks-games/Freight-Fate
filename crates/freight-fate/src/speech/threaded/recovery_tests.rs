use super::*;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Condvar;

#[derive(Default)]
struct Gate {
    entered: AtomicBool,
    released: Mutex<bool>,
    wake: Condvar,
}

impl Gate {
    fn block(&self) {
        self.entered.store(true, Ordering::SeqCst);
        let released = self.released.lock().unwrap();
        let (released, timeout) = self
            .wake
            .wait_timeout_while(released, Duration::from_secs(5), |open| !*open)
            .unwrap();
        assert!(
            *released && !timeout.timed_out(),
            "test failed to release gate"
        );
    }

    fn wait(&self) {
        let until = Instant::now() + Duration::from_secs(2);
        while !self.entered.load(Ordering::SeqCst) {
            assert!(Instant::now() < until, "worker never reached gate");
            std::thread::yield_now();
        }
    }

    fn release(&self) {
        *self.released.lock().unwrap() = true;
        self.wake.notify_all();
    }
}

// Release every blocked worker even when an assertion unwinds the test.
struct ReleaseOnDrop(Vec<Arc<Gate>>);
impl Drop for ReleaseOnDrop {
    fn drop(&mut self) {
        for gate in &self.0 {
            gate.release();
        }
    }
}

struct ControlledSink {
    replacement: bool,
    say_gate: Arc<Gate>,
    replay_gate: Arc<Gate>,
    calls: Arc<Mutex<Vec<String>>>,
}

impl ControlledSink {
    fn record(&self, call: &str) {
        self.calls
            .lock()
            .unwrap()
            .push(format!("{}:{call}", self.replacement));
    }
}

impl SpeechSink for ControlledSink {
    fn say(&mut self, text: &str, _: bool) {
        self.record(text);
        if !self.replacement && text == "blocked" {
            self.say_gate.block();
        }
    }
    fn say_event(&mut self, _: &str, _: bool) {
        self.record("event");
    }
    fn stop_main(&mut self) {
        self.record("stop_main");
    }
    fn stop_event(&mut self) {
        self.record("stop_event");
    }
    fn stop(&mut self) {
        self.record("stop");
    }
    fn poll(&mut self, _: f64) {
        self.record("poll");
    }
    fn request_refresh(&mut self) {
        self.record("request_refresh");
    }
    fn available(&self) -> bool {
        true
    }
    fn backend_name(&self) -> String {
        "controlled".into()
    }
    fn event_backend_name(&self) -> String {
        "event".into()
    }
    fn has_separate_event_voice(&self) -> bool {
        true
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
        true
    }
    fn supports_braille(&self) -> bool {
        true
    }
    fn event_backend_options(&self) -> Vec<String> {
        vec!["event".into()]
    }
    fn voice_names(&self) -> Vec<String> {
        vec!["voice".into()]
    }
    fn select_event_backend(&mut self, name: Option<&str>) {
        self.record(&format!("select:{name:?}"));
    }
    fn set_braille_only(&mut self, on: bool) {
        self.record(&format!("braille:{on}"));
    }
    fn configure(
        &mut self,
        rate: Option<f64>,
        pitch: Option<f64>,
        volume: Option<f64>,
        voice: Option<&str>,
    ) {
        self.record(&format!(
            "configure:{rate:?}:{pitch:?}:{volume:?}:{voice:?}"
        ));
        if self.replacement {
            self.replay_gate.block();
        }
    }
    fn refresh(&mut self) -> bool {
        self.record("refresh");
        true
    }
    fn say_adjustment_preview(&mut self, _: &str, _: &str, _: bool) -> bool {
        true
    }
    fn shutdown(&mut self) {
        self.record("shutdown");
    }
}

fn age_worker(sink: &mut ThreadedSpeech) {
    *sink.heartbeat.lock().unwrap() = Instant::now() - Duration::from_secs(30);
    sink.poll(0.0);
    assert_eq!(sink.respawns, 1);
}

#[test]
fn recovery_waits_for_replacement_construction_and_settings_replay() {
    let initial = Arc::new(Gate::default());
    let constructor = Arc::new(Gate::default());
    let replay = Arc::new(Gate::default());
    let _release = ReleaseOnDrop(vec![initial.clone(), constructor.clone(), replay.clone()]);
    let calls = Arc::new(Mutex::new(Vec::new()));
    let mut sink = ThreadedSpeech::spawn_with({
        let (initial, constructor, replay, calls) = (
            initial.clone(),
            constructor.clone(),
            replay.clone(),
            calls.clone(),
        );
        move |replacement| {
            if replacement {
                constructor.block();
            }
            Box::new(ControlledSink {
                replacement,
                say_gate: initial.clone(),
                replay_gate: replay.clone(),
                calls: calls.clone(),
            })
        }
    });
    sink.select_event_backend(Some("event"));
    sink.configure(Some(0.7), Some(0.4), Some(0.9), Some("voice"));
    sink.set_braille_only(true);
    sink.say("blocked", false);
    initial.wait();
    age_worker(&mut sink);
    constructor.wait();
    sink.poll(0.0);
    let recovered_during_constructor = !sink.wedged;
    constructor.release();
    replay.wait();
    sink.poll(0.0);
    let available_during_replay = sink.available();
    let recovered_during_replay = !sink.wedged;
    replay.release();
    assert!(sink.refresh(), "replacement did not finish replay");
    sink.poll(0.0);
    let recovered_after_replay = sink.available() && !sink.wedged;
    initial.release();
    sink.shutdown();
    let replayed: Vec<_> = calls
        .lock()
        .unwrap()
        .iter()
        .filter(|call| call.starts_with("true:"))
        .cloned()
        .collect();
    assert_eq!(
        &replayed[..3],
        [
            "true:select:Some(\"event\")",
            "true:configure:Some(0.7):Some(0.4):Some(0.9):Some(\"voice\")",
            "true:braille:true",
        ]
    );
    assert!(
        !recovered_during_constructor,
        "constructor heartbeat falsely reported recovery"
    );
    assert!(
        !available_during_replay,
        "speech became available before settings replay completed"
    );
    assert!(
        !recovered_during_replay,
        "settings replay falsely reported recovery"
    );
    assert!(recovered_after_replay);
}

#[test]
fn abandoned_worker_discards_queued_native_commands_when_blocked_call_returns() {
    let initial = Arc::new(Gate::default());
    let constructor = Arc::new(Gate::default());
    let replay = Arc::new(Gate::default());
    replay.release();
    let _release = ReleaseOnDrop(vec![initial.clone(), replay.clone(), constructor.clone()]);
    let calls = Arc::new(Mutex::new(Vec::new()));
    let mut sink = ThreadedSpeech::spawn_with({
        let (initial, replay, calls, constructor) = (
            initial.clone(),
            replay.clone(),
            calls.clone(),
            constructor.clone(),
        );
        move |replacement| {
            if !replacement {
                constructor.block();
            }
            Box::new(ControlledSink {
                replacement,
                say_gate: initial.clone(),
                replay_gate: replay.clone(),
                calls: calls.clone(),
            })
        }
    });
    constructor.wait();
    sink.say("blocked", false);
    sink.say("stale", false);
    sink.configure(Some(0.6), None, None, None);
    sink.stop_event();
    sink.request_refresh();
    constructor.release();
    initial.wait();
    let old_commands = sink.commands.clone();
    age_worker(&mut sink);
    assert!(sink.refresh());
    initial.release();
    // A disconnected receiver proves the old worker exited. Barrier markers
    // never invoke a native operation, even in the broken implementation.
    let until = Instant::now() + Duration::from_secs(2);
    loop {
        if matches!(
            old_commands.try_send(Command::ReplayComplete),
            Err(TrySendError::Disconnected(_))
        ) {
            break;
        }
        assert!(
            Instant::now() < until,
            "abandoned worker did not exit after its call returned"
        );
        std::thread::yield_now();
    }
    sink.shutdown();
    let old_calls: Vec<_> = calls
        .lock()
        .unwrap()
        .iter()
        .filter(|call| call.starts_with("false:"))
        .cloned()
        .collect();
    assert_eq!(
        old_calls,
        ["false:blocked"],
        "abandoned worker re-entered native operations"
    );
}
