//! Port of the speech half of `tests/test_speech_audio.py` (backend
//! selection, the event voice, configure, runtime re-detection) and the
//! `speech_stub` contract from `tests/speech_capture.py`, all against fake
//! registries. The checks against the Prism on this machine are in
//! `speech_live.rs`.
//!
//! Test names keep the Python names so the two suites diff by `grep`.

use ff_core::speech_text::SpokenMessage;
use freight_fate::speech::fakes::{FakeRegistry, FakeVoice};
use freight_fate::speech::{
    apply_speech_settings, pick_backend_gated, pick_event_backend, CaptureSpeech, NullSpeech,
    Speech, SpeechChannel, SpeechSink, VoiceFeatures, EVENT_BACKEND, REFRESH_INTERVAL_S,
};

const SPEAKING: VoiceFeatures = VoiceFeatures::SPEAKING;
const ADJUSTABLE: VoiceFeatures = VoiceFeatures::ADJUSTABLE;

fn runtime(supported: bool) -> VoiceFeatures {
    VoiceFeatures {
        is_supported_at_runtime: supported,
        ..SPEAKING
    }
}

fn narrator_off() -> bool {
    false
}

fn narrator_on() -> bool {
    true
}

fn spoken(lines: &[(&str, bool)]) -> Vec<(String, bool)> {
    lines
        .iter()
        .map(|(text, interrupt)| (text.to_string(), *interrupt))
        .collect()
}

/// The shape of Prism's real registry: NVDA outranks everything even when it
/// is not running.
fn registry(nvda_running: bool) -> FakeRegistry {
    FakeRegistry::new(vec![
        FakeVoice::new("NVDA", 103, runtime(nvda_running)),
        FakeVoice::new("JAWS", 100, runtime(false)),
        FakeVoice::new("ONE_CORE", 98, SPEAKING),
        FakeVoice::new("SAPI", 97, SPEAKING),
    ])
}

fn picked_name(ctx: &FakeRegistry, override_name: Option<&str>) -> Option<String> {
    pick_backend_gated(ctx, override_name, narrator_off).map(|backend| backend.name())
}

// -- pick_backend -----------------------------------------------------------

#[test]
fn test_running_screen_reader_wins() {
    assert_eq!(picked_name(&registry(true), None).as_deref(), Some("NVDA"));
}

#[test]
fn test_falls_past_not_running_screen_readers() {
    // NVDA is the registry's "best" but is not running: the highest-priority
    // backend that actually works at runtime must win instead.
    assert_eq!(
        picked_name(&registry(false), None).as_deref(),
        Some("ONE_CORE")
    );
}

#[test]
fn test_env_override_is_honored() {
    assert_eq!(
        picked_name(&registry(false), Some("SAPI")).as_deref(),
        Some("SAPI")
    );
}

#[test]
fn test_unusable_override_falls_back_to_automatic_choice() {
    assert_eq!(
        picked_name(&registry(false), Some("JAWS")).as_deref(),
        Some("ONE_CORE")
    );
    assert_eq!(
        picked_name(&registry(false), Some("NoSuch")).as_deref(),
        Some("ONE_CORE")
    );
}

#[test]
fn test_no_usable_backend_returns_none() {
    let ctx = FakeRegistry::new(vec![FakeVoice::new("NVDA", 103, runtime(false))]);
    assert!(picked_name(&ctx, None).is_none());
}

#[test]
fn test_backend_without_speak_or_output_is_skipped() {
    let ctx = FakeRegistry::new(vec![
        FakeVoice::new(
            "BRAILLE_ONLY",
            103,
            VoiceFeatures {
                supports_output: false,
                supports_speak: false,
                ..SPEAKING
            },
        ),
        FakeVoice::new("SAPI", 97, SPEAKING),
    ]);
    assert_eq!(picked_name(&ctx, None).as_deref(), Some("SAPI"));
}

#[test]
fn test_cached_fallback_does_not_mask_a_screen_reader_that_started() {
    // The acquire_best trap: it returns the highest-priority backend with a
    // live cached instance -- whatever voice the game is already holding --
    // not what is running. Selection must ignore it and find NVDA through
    // live runtime checks. (The fake registry has no acquire_best at all:
    // the port never calls it.)
    let ctx = FakeRegistry::new(vec![
        FakeVoice::new("NVDA", 103, SPEAKING),
        FakeVoice::new("ONE_CORE", 98, SPEAKING),
    ]);
    assert_eq!(picked_name(&ctx, None).as_deref(), Some("NVDA"));
}

/// A Windows registry shape including the UIA backend, which claims runtime
/// support whether or not Narrator is listening.
fn narrator_registry(nvda_running: bool) -> FakeRegistry {
    FakeRegistry::new(vec![
        FakeVoice::new("NVDA", 103, runtime(nvda_running)),
        FakeVoice::new("OneCore", 98, SPEAKING),
        FakeVoice::new("SAPI", 97, SPEAKING),
        FakeVoice::new("UIA", 97, SPEAKING),
    ])
}

#[test]
fn test_uia_is_skipped_when_narrator_is_not_running() {
    // Without the gate the game would talk into UIA notifications nobody
    // reads aloud -- silence. The plain software voice must win instead.
    let picked = pick_backend_gated(&narrator_registry(false), None, narrator_off);
    assert_eq!(picked.unwrap().name(), "OneCore");
}

#[test]
fn test_narrator_route_stays_last_resort_even_when_narrator_runs() {
    // Prism's UIA backend cannot interrupt or stop (Narrator queues every
    // notification), so menu browsing through it is unusable. While any
    // software voice works, it must win over the Narrator route.
    let picked = pick_backend_gated(&narrator_registry(false), None, narrator_on);
    assert_eq!(picked.unwrap().name(), "OneCore");
}

#[test]
fn test_narrator_route_used_when_nothing_else_can_speak() {
    // Queued speech through Narrator still beats total silence.
    let ctx = FakeRegistry::new(vec![
        FakeVoice::new("NVDA", 103, runtime(false)),
        FakeVoice::new("OneCore", 98, runtime(false)),
        FakeVoice::new("UIA", 97, SPEAKING),
    ]);
    let picked = pick_backend_gated(&ctx, None, narrator_on);
    assert_eq!(picked.unwrap().name(), "UIA");
}

#[test]
fn test_running_screen_reader_beats_narrator() {
    // NVDA and Narrator both up: the richer screen reader API wins.
    let picked = pick_backend_gated(&narrator_registry(true), None, narrator_on);
    assert_eq!(picked.unwrap().name(), "NVDA");
}

#[test]
fn test_event_channel_uses_sapi_alongside_the_screen_reader() {
    let ctx = registry(true);
    let main = pick_backend_gated(&ctx, None, narrator_off).unwrap();
    assert_eq!(main.name(), "NVDA");
    let event = pick_event_backend(&ctx, Some(main.as_ref()), EVENT_BACKEND).unwrap();
    assert_eq!(event.name(), "SAPI");
}

#[test]
fn test_event_channel_skipped_when_main_voice_is_already_sapi() {
    let ctx = FakeRegistry::new(vec![
        FakeVoice::new("NVDA", 103, runtime(false)),
        FakeVoice::new("SAPI", 97, SPEAKING),
    ]);
    let main = pick_backend_gated(&ctx, None, narrator_off).unwrap();
    assert_eq!(main.name(), "SAPI");
    assert!(pick_event_backend(&ctx, Some(main.as_ref()), EVENT_BACKEND).is_none());
}

#[test]
fn test_event_channel_absent_when_sapi_is_unusable() {
    let ctx = FakeRegistry::new(vec![
        FakeVoice::new("NVDA", 103, SPEAKING),
        FakeVoice::new("SAPI", 97, runtime(false)),
    ]);
    let main = pick_backend_gated(&ctx, None, narrator_off).unwrap();
    assert!(pick_event_backend(&ctx, Some(main.as_ref()), EVENT_BACKEND).is_none());
    assert!(pick_event_backend(&ctx, None, EVENT_BACKEND).is_none());
}

// -- configure / preview ------------------------------------------------------

/// A Speech with an unsupported main voice and a fully adjustable event
/// voice -- the common real layout (running NVDA plus a SAPI event voice).
fn configurable_speech() -> (Speech, FakeVoice, FakeVoice) {
    let main = FakeVoice::new("NVDA", 0, SPEAKING);
    let event = FakeVoice::with_voices(
        "SAPI",
        0,
        VoiceFeatures {
            supports_stop: false,
            ..ADJUSTABLE
        },
        &["David", "Zira"],
    );
    let speech = Speech::from_parts(None, Some(main.boxed()), Some(event.boxed()));
    (speech, main, event)
}

#[test]
fn test_configure_pushes_params_to_supporting_backends_only() {
    let (mut s, main, event) = configurable_speech();
    s.configure(Some(0.8), Some(0.3), Some(0.5), Some("Zira"));
    {
        let event = event.state();
        assert_eq!(
            (event.rate, event.pitch, event.volume, event.voice),
            (Some(0.8), Some(0.3), Some(0.5), Some(1))
        );
    }
    // the unsupported main voice is left untouched
    let main = main.state();
    assert_eq!(
        (main.rate, main.pitch, main.volume, main.voice),
        (None, None, None, None)
    );
}

#[test]
fn test_adjustment_preview_uses_configurable_voice() {
    let (mut s, main, event) = configurable_speech();
    assert!(s.say_adjustment_preview("speech_rate", "Speech rate: 60 percent.", true));
    assert!(main.spoken().is_empty());
    assert_eq!(
        event.spoken(),
        spoken(&[("Speech rate: 60 percent.", true)])
    );
}

#[test]
fn test_adjustment_preview_falls_back_when_setting_is_not_configurable() {
    let (mut s, _main, _event) = configurable_speech();
    assert!(!s.say_adjustment_preview("speech_verbosity", "Speech verbosity: normal.", true));
}

#[test]
fn test_configure_preserves_onecore_default_pitch_at_midpoint() {
    let event = FakeVoice::new(
        "OneCore",
        0,
        VoiceFeatures {
            supports_set_rate: true,
            supports_set_pitch: true,
            ..SPEAKING
        },
    );
    let mut s = Speech::from_parts(None, None, Some(event.boxed()));
    s.configure(Some(0.5), Some(0.5), None, None);
    assert_eq!(event.state().rate, Some(0.5));
    assert_eq!(event.state().pitch, None);
    s.configure(None, Some(0.7), None, None);
    assert_eq!(event.state().pitch, Some(0.7));
}

#[test]
fn test_supports_and_voice_names_reflect_backend_features() {
    let (s, _main, _event) = configurable_speech();
    assert!(s.supports_rate() && s.supports_pitch() && s.supports_volume());
    assert_eq!(s.voice_names(), vec!["David", "Zira"]);
}

#[test]
fn test_configure_skips_unknown_voice_name() {
    let (mut s, _main, event) = configurable_speech();
    s.configure(None, None, None, Some("Nonexistent"));
    assert_eq!(event.state().voice, None);
}

#[test]
fn test_no_configurable_backend_reports_no_support() {
    let main = FakeVoice::new("NVDA", 0, SPEAKING);
    let s = Speech::from_parts(None, Some(main.boxed()), None);
    assert!(!s.supports_rate());
    assert!(!s.supports_pitch());
    assert!(!s.supports_volume());
    assert!(s.voice_names().is_empty());
}

// -- say / say_event / stop -----------------------------------------------------

fn stoppable(name: &str) -> FakeVoice {
    FakeVoice::new(
        name,
        0,
        VoiceFeatures {
            supports_stop: true,
            ..SPEAKING
        },
    )
}

#[test]
fn test_interrupting_main_speech_uses_backend_interrupt_without_extra_stop() {
    let backend = stoppable("SAPI");
    let mut s = Speech::from_parts(None, Some(backend.boxed()), None);
    s.say("Fresh menu item.", true);
    assert_eq!(backend.stop_calls(), 0);
    assert_eq!(backend.spoken(), spoken(&[("Fresh menu item.", true)]));
}

#[test]
fn test_main_speech_flush_does_not_stop_event_voice() {
    let main = stoppable("NVDA");
    let event = stoppable("SAPI");
    let mut s = Speech::from_parts(None, Some(main.boxed()), Some(event.boxed()));
    s.say("Fresh menu item.", true);
    assert_eq!(main.stop_calls(), 0);
    assert_eq!(event.stop_calls(), 0);
    assert_eq!(main.spoken(), spoken(&[("Fresh menu item.", true)]));
}

#[test]
fn test_urgent_event_speech_uses_backend_interrupt_without_extra_stop() {
    let main = stoppable("NVDA");
    let event = stoppable("SAPI");
    let mut s = Speech::from_parts(None, Some(main.boxed()), Some(event.boxed()));
    s.say_event("Brake now.", true);
    assert_eq!(main.stop_calls(), 0);
    assert_eq!(event.stop_calls(), 0);
    assert_eq!(event.spoken(), spoken(&[("Brake now.", true)]));
}

#[test]
fn test_urgent_event_without_event_voice_flushes_main_before_fallback() {
    let main = stoppable("NVDA");
    let mut s = Speech::from_parts(None, Some(main.boxed()), None);
    s.say_event("Brake now.", true);
    assert_eq!(main.stop_calls(), 1);
    assert_eq!(main.spoken(), spoken(&[("Brake now.", false)]));
}

#[test]
fn test_failed_urgent_event_voice_falls_back_without_main_interrupt() {
    let main = stoppable("NVDA");
    let event = stoppable("SAPI");
    event.set_fail_output(true);
    let mut s = Speech::from_parts(None, Some(main.boxed()), Some(event.boxed()));
    s.say_event("Brake now.", true);
    assert_eq!(event.stop_calls(), 0);
    assert_eq!(main.stop_calls(), 1);
    assert_eq!(main.spoken(), spoken(&[("Brake now.", false)]));
}

#[test]
fn test_nonurgent_event_speech_can_queue_on_event_voice() {
    let event = stoppable("SAPI");
    let mut s = Speech::from_parts(None, None, Some(event.boxed()));
    s.say_event("Weather changing.", false);
    assert_eq!(event.stop_calls(), 0);
    assert_eq!(event.spoken(), spoken(&[("Weather changing.", false)]));
    s.configure(Some(0.9), None, None, Some("David")); // must not panic
}

#[test]
fn empty_text_is_never_sent_to_a_backend() {
    let main = stoppable("NVDA");
    let event = stoppable("SAPI");
    let mut s = Speech::from_parts(None, Some(main.boxed()), Some(event.boxed()));
    s.say("", true);
    s.say_event("", true);
    assert!(main.spoken().is_empty());
    assert!(event.spoken().is_empty());
    assert_eq!(main.stop_calls(), 0);
}

#[test]
fn stop_only_reaches_backends_that_support_it() {
    let main = FakeVoice::new("NVDA", 0, SPEAKING); // no supports_stop
    let event = stoppable("SAPI");
    let mut s = Speech::from_parts(None, Some(main.boxed()), Some(event.boxed()));
    s.stop_main();
    s.stop_event();
    s.stop();
    assert_eq!(main.stop_calls(), 0);
    assert_eq!(event.stop_calls(), 2);
}

// -- event voice selection -----------------------------------------------------

/// A registry shaped like a real Windows box with NVDA running: the screen
/// reader plus two controllable software voices.
fn multi_voice_ctx() -> FakeRegistry {
    FakeRegistry::new(vec![
        FakeVoice::new("NVDA", 103, SPEAKING),
        FakeVoice::new("OneCore", 98, ADJUSTABLE),
        FakeVoice::new("SAPI", 97, ADJUSTABLE),
    ])
}

fn speech_on(ctx: &FakeRegistry, main: &str) -> Speech {
    let main = ctx.voice(main).unwrap().boxed();
    Speech::from_parts(Some(Box::new(ctx.clone())), Some(main), None)
}

#[test]
fn test_event_backend_options_lists_software_voices_by_priority() {
    let ctx = multi_voice_ctx();
    let s = speech_on(&ctx, "NVDA"); // the main voice is excluded
    assert_eq!(s.event_backend_options(), vec!["OneCore", "SAPI"]);
}

#[test]
fn test_select_event_backend_switches_and_clears() {
    let ctx = multi_voice_ctx();
    let mut s = speech_on(&ctx, "NVDA");
    s.select_event_backend(Some("OneCore"));
    assert_eq!(s.event_backend_name(), "OneCore");
    s.select_event_backend(None); // back to the main voice
    assert_eq!(s.event_backend_name(), "none");
    // Asking for the main voice by name is not a real separate option, so the
    // preference falls back to the best available one (None is how you pick
    // the main voice for events).
    s.select_event_backend(Some("NVDA"));
    assert_eq!(s.event_backend_name(), "OneCore");
}

#[test]
fn test_event_backend_falls_back_to_platform_voice() {
    // A macOS-shaped registry: VoiceOver running, AVSpeech as the software
    // voice, no SAPI. A Windows save's "SAPI" preference must still land on a
    // real voice.
    let ctx = FakeRegistry::new(vec![
        FakeVoice::new("VoiceOver", 103, SPEAKING),
        FakeVoice::new("AVSpeech", 98, ADJUSTABLE),
    ]);
    let mut s = speech_on(&ctx, "VoiceOver");
    s.select_event_backend(Some("SAPI")); // not on this machine
    assert_eq!(s.event_backend_name(), "AVSpeech"); // best available wins
}

#[test]
fn test_event_backend_none_when_no_separate_voice_exists() {
    // Only a screen reader is usable: there is nothing to separate onto.
    let ctx = FakeRegistry::new(vec![FakeVoice::new("VoiceOver", 103, SPEAKING)]);
    let mut s = speech_on(&ctx, "VoiceOver");
    s.select_event_backend(Some("SAPI"));
    assert_eq!(s.event_backend_name(), "none");
}

// -- runtime screen reader switching (NVDA -> Narrator -> NVDA) ---------------

/// A speakable registry for runtime-switch tests: NVDA (a screen reader
/// that may or may not be running) plus SAPI (always-on software voice).
fn live_registry(nvda_running: bool) -> (Speech, FakeVoice, FakeVoice) {
    let nvda = FakeVoice::new(
        "NVDA",
        103,
        VoiceFeatures {
            is_supported_at_runtime: nvda_running,
            supports_stop: true,
            ..SPEAKING
        },
    );
    let sapi = FakeVoice::with_voices("SAPI", 97, ADJUSTABLE, &["David", "Zira"]);
    let ctx = FakeRegistry::new(vec![nvda.clone(), sapi.clone()]);
    let speech = Speech::from_parts(Some(Box::new(ctx)), None, None);
    (speech, nvda, sapi)
}

#[test]
fn test_say_failure_switches_to_a_live_voice_and_retries() {
    // NVDA quits mid-game: the utterance that fails must come out of the
    // fallback voice instead of muting the game forever.
    let (mut s, nvda, sapi) = live_registry(false);
    s.set_main_backend(Some(nvda.boxed()));
    nvda.set_fail_output(true);
    s.say("Turn left ahead.", true);
    assert_eq!(s.backend_name(), "SAPI");
    assert_eq!(sapi.spoken(), spoken(&[("Turn left ahead.", true)]));
}

#[test]
fn test_say_failure_with_no_live_voice_recovers_when_one_returns() {
    let (mut s, nvda, sapi) = live_registry(false);
    sapi.set_runtime_supported(false); // nothing else to speak with
    s.set_main_backend(Some(nvda.boxed()));
    nvda.set_fail_output(true);
    s.say("hello", true);
    assert!(!s.available());
    nvda.set_fail_output(false);
    nvda.set_runtime_supported(true); // the screen reader is back
    s.poll(REFRESH_INTERVAL_S);
    assert_eq!(s.backend_name(), "NVDA");
    assert!(nvda.spoken().is_empty()); // a switch is silent
}

/// ZDSR as Prism drives it: a screen reader with a braille call that fails
/// when no display is attached.
fn zdsr_without_display() -> (Speech, FakeVoice) {
    let zdsr = FakeVoice::new("ZDSR", 101, VoiceFeatures::BRAILLING);
    zdsr.set_fail_braille(true);
    let sapi = FakeVoice::new("SAPI", 97, ADJUSTABLE);
    let ctx = FakeRegistry::new(vec![zdsr.clone(), sapi]);
    let speech = Speech::with_registry(Box::new(ctx), None);
    assert_eq!(speech.backend_name(), "ZDSR");
    (speech, zdsr)
}

#[test]
fn a_failed_braille_call_does_not_drop_a_screen_reader_that_spoke() {
    // AudioGames forum, 2026-10-10: with ZDSR and no braille display, every
    // arrow key and every few idle seconds said "Speech is now using ZDSR."
    // The braille half of each line failed after the speech half worked, so
    // the line counted as a dead voice, was spoken again, and the health
    // check rebound ZDSR and announced it.
    let (mut s, zdsr) = zdsr_without_display();
    s.say("Main menu", true);
    s.say("Settings", true);
    for _ in 0..3 {
        s.poll(REFRESH_INTERVAL_S);
    }
    assert_eq!(s.backend_name(), "ZDSR");
    assert_eq!(
        zdsr.spoken(),
        spoken(&[("Main menu", true), ("Settings", true)])
    );
}

#[test]
fn rebinding_a_voice_that_dropped_out_is_silent() {
    // A screen reader whose speech call reports failure is dropped and
    // rebound by the health check; the rebind says nothing.
    let (mut s, zdsr) = zdsr_without_display();
    zdsr.set_fail_output(true);
    s.say("Main menu", true);
    zdsr.set_fail_output(false);
    for _ in 0..3 {
        s.poll(REFRESH_INTERVAL_S);
    }
    assert_eq!(s.backend_name(), "ZDSR");
    assert!(zdsr.spoken().is_empty());
}

#[test]
fn test_poll_returns_to_the_screen_reader_when_it_comes_back() {
    // The game fell back to SAPI while NVDA was closed; when NVDA reappears
    // the periodic check must switch back, silently.
    let (mut s, nvda, sapi) = live_registry(false);
    s.set_main_backend(Some(sapi.boxed()));
    s.poll(REFRESH_INTERVAL_S);
    assert_eq!(s.backend_name(), "SAPI"); // nothing better yet: stays put
    nvda.set_runtime_supported(true);
    s.poll(REFRESH_INTERVAL_S);
    assert_eq!(s.backend_name(), "NVDA");
    assert!(nvda.spoken().is_empty());
}

#[test]
fn test_switch_reselects_event_voice_and_reapplies_settings() {
    // While NVDA was closed the main voice fell back to SAPI, so there was no
    // separate event voice. Switching back to NVDA must revive the SAPI event
    // voice and push the player's saved speech settings onto it.
    let (mut s, nvda, sapi) = live_registry(false);
    s.set_main_backend(Some(sapi.boxed()));
    s.select_event_backend(Some("SAPI")); // SAPI is the main voice: nothing separate
    assert_eq!(s.event_backend_name(), "none");
    s.configure(Some(0.8), None, None, Some("Zira"));
    nvda.set_runtime_supported(true);
    s.poll(REFRESH_INTERVAL_S);
    assert_eq!(s.backend_name(), "NVDA");
    assert_eq!(s.event_backend_name(), "SAPI");
    assert_eq!(sapi.state().rate, Some(0.8));
    assert_eq!(sapi.state().voice, Some(1));
}

#[test]
fn test_event_voice_revives_after_a_failure() {
    let (mut s, nvda, sapi) = live_registry(true);
    s.set_main_backend(Some(nvda.boxed()));
    s.select_event_backend(Some("SAPI"));
    assert_eq!(s.event_backend_name(), "SAPI");
    sapi.set_fail_output(true);
    s.say_event("Brake now.", true); // falls back to the main voice...
    assert_eq!(s.event_backend_name(), "none");
    assert_eq!(nvda.spoken(), spoken(&[("Brake now.", false)]));
    sapi.set_fail_output(false);
    s.poll(REFRESH_INTERVAL_S); // ...and the health check brings it back
    assert_eq!(s.event_backend_name(), "SAPI");
}

#[test]
fn test_request_refresh_makes_the_next_poll_immediate() {
    let (mut s, nvda, sapi) = live_registry(false);
    s.set_main_backend(Some(sapi.boxed()));
    nvda.set_runtime_supported(true);
    s.poll(0.016); // a normal frame: far too soon for the periodic check
    assert_eq!(s.backend_name(), "SAPI");
    s.request_refresh(); // the game window regained focus
    s.poll(0.016);
    assert_eq!(s.backend_name(), "NVDA");
}

#[test]
fn test_speech_appears_for_a_screen_reader_started_after_the_game() {
    let (mut s, nvda, sapi) = live_registry(false);
    sapi.set_runtime_supported(false);
    s.poll(REFRESH_INTERVAL_S);
    assert!(!s.available()); // still nothing on the machine can speak
    nvda.set_runtime_supported(true);
    s.poll(REFRESH_INTERVAL_S);
    assert_eq!(s.backend_name(), "NVDA");
}

#[test]
fn test_healthy_backend_is_kept_without_announcements() {
    let (mut s, nvda, _sapi) = live_registry(true);
    s.set_main_backend(Some(nvda.boxed()));
    for _ in 0..3 {
        s.poll(REFRESH_INTERVAL_S);
    }
    assert_eq!(s.backend_name(), "NVDA");
    assert!(nvda.spoken().is_empty());
}

#[test]
fn test_poll_is_safe_without_prism() {
    let mut s = Speech::disabled(); // headless: no context at all
    s.poll(REFRESH_INTERVAL_S); // must not panic
    s.request_refresh();
    assert!(!s.refresh());
    assert!(!s.available());
    assert_eq!(s.backend_name(), "none");
    assert_eq!(s.event_backend_name(), "none");
    assert!(s.event_backend_options().is_empty());
    s.say("nothing", true);
    s.say_event("nothing", true);
    s.shutdown();
    s.shutdown(); // safe more than once
}

#[test]
fn narrator_route_is_picked_silently() {
    let uia = FakeVoice::new("UIA", 97, SPEAKING);
    let ctx = FakeRegistry::new(vec![uia.clone()]);
    let mut s = Speech::from_parts(Some(Box::new(ctx)), None, None);
    s.set_narrator_probe(narrator_on);
    assert!(s.refresh());
    assert_eq!(s.backend_name(), "UIA");
    assert!(uia.spoken().is_empty());
}

#[test]
fn with_registry_runs_the_startup_selection() {
    // `Speech.__init__` after `prism.Context()`: pick the main voice, then
    // bind the SAPI event voice alongside it.
    let ctx = multi_voice_ctx();
    let s = Speech::with_registry(Box::new(ctx.clone()), None);
    assert_eq!(s.backend_name(), "NVDA");
    assert_eq!(s.event_backend_name(), "SAPI");
    assert!(s.has_separate_event_voice());
    assert!(s.event_supports_rate());
    let forced = Speech::with_registry(Box::new(ctx), Some("OneCore".to_string()));
    assert_eq!(forced.backend_name(), "OneCore");
    assert_eq!(forced.event_backend_name(), "SAPI");
}

// -- apply_speech_settings (GameContext.apply_speech) ---------------------------

#[test]
fn apply_speech_settings_records_the_voice_actually_bound() {
    let ctx = FakeRegistry::new(vec![
        FakeVoice::new("VoiceOver", 103, SPEAKING),
        FakeVoice::new("AVSpeech", 98, ADJUSTABLE),
    ]);
    let mut s = speech_on(&ctx, "VoiceOver");
    let mut settings = ff_core::settings::Settings {
        sapi_events: true,
        event_backend: "SAPI".to_string(), // a Windows save opened on macOS
        speech_rate: 0.8,
        speech_voice: String::new(),
        ..Default::default()
    };
    apply_speech_settings(&mut s, &mut settings);
    assert_eq!(settings.event_backend, "AVSpeech");
    assert_eq!(ctx.voice("AVSpeech").unwrap().state().rate, Some(0.8));
    assert_eq!(s.config().voice, None);

    settings.sapi_events = false;
    apply_speech_settings(&mut s, &mut settings);
    assert_eq!(s.event_backend_name(), "none");
    assert_eq!(settings.event_backend, "AVSpeech"); // untouched when events share main
}

// -- CaptureSpeech: the speech_stub contract ---------------------------------------

#[test]
fn capture_records_both_channels_in_order_with_interrupt() {
    let mut capture = CaptureSpeech::new();
    capture.say("Main menu.", true);
    capture.say_event("Brake now.", true);
    capture.say("Settings.", false);
    assert_eq!(
        capture.lines(),
        vec!["Main menu.", "Brake now.", "Settings."]
    );
    assert_eq!(capture.main_lines(), vec!["Main menu.", "Settings."]);
    assert_eq!(capture.event_lines(), vec!["Brake now."]);
    assert_eq!(
        capture.calls(SpeechChannel::Main),
        spoken(&[("Main menu.", true), ("Settings.", false)])
    );
    assert_eq!(
        capture.tagged(),
        vec![
            ("main", "Main menu.".to_string()),
            ("event", "Brake now.".to_string()),
            ("main", "Settings.".to_string())
        ]
    );
    let sequences: Vec<u64> = capture.entries().iter().map(|e| e.sequence).collect();
    assert_eq!(sequences, vec![0, 1, 2]);
    assert_eq!(
        capture.transcript(),
        "Main menu.\n[event] Brake now.\nSettings."
    );
    capture.clear();
    assert!(capture.lines().is_empty());
    assert!(capture.transcript().is_empty());
}

#[test]
fn capture_resolves_spoken_messages_by_its_terse_flag() {
    let pair = SpokenMessage::with_terse("Weigh station ahead, all trucks must enter.", "Scale.");
    let silent_when_terse = SpokenMessage::with_terse("Nice weather today.", "");

    let mut normal = CaptureSpeech::new();
    normal.say_message(&pair, true);
    normal.say_event_message(&silent_when_terse, false);
    assert_eq!(
        normal.lines(),
        vec![
            "Weigh station ahead, all trucks must enter.",
            "Nice weather today."
        ]
    );

    let mut terse = CaptureSpeech::new().terse();
    terse.say_message(&pair, true);
    terse.say_event_message(&silent_when_terse, false); // dropped, as the real method drops it
    assert_eq!(terse.lines(), vec!["Scale."]);
    assert!(terse.is_terse());
}

#[test]
fn capture_prefix_marks_every_line() {
    let mut capture = CaptureSpeech::new().with_prefix("[menu] ");
    capture.say("Main menu.", true);
    capture.say_event("Brake now.", true);
    assert_eq!(
        capture.lines(),
        vec!["[menu] Main menu.", "[menu] Brake now."]
    );
    assert!(capture.contains("Brake"));
}

#[test]
fn capture_drops_empty_text_like_the_real_channel() {
    let mut capture = CaptureSpeech::new();
    capture.say("", true);
    capture.say_event("", false);
    assert!(capture.entries().is_empty());
}

#[test]
fn capture_default_answers_like_the_headless_python_speech() {
    // FREIGHT_FATE_NO_SPEECH=1 in conftest left the Python Speech with no
    // context: no voice, no separate event voice, nothing adjustable. The
    // transcript tests were recorded against those answers.
    let mut capture = CaptureSpeech::new();
    assert!(!capture.available());
    assert_eq!(capture.backend_name(), "none");
    assert!(!capture.has_separate_event_voice());
    assert_eq!(capture.event_backend_name(), "none");
    assert!(!capture.supports_rate() && !capture.supports_pitch() && !capture.supports_volume());
    assert!(!capture.event_supports_rate());
    assert!(capture.event_backend_options().is_empty());
    assert!(capture.voice_names().is_empty());
    assert!(!capture.refresh());
    capture.select_event_backend(Some("SAPI"));
    assert!(!capture.has_separate_event_voice()); // nothing to bind headless
    assert_eq!(capture.event_backend_name(), "none");
    capture.poll(REFRESH_INTERVAL_S);
    capture.request_refresh();
    assert_eq!(capture.refresh_requests(), 1);
    capture.shutdown();
    assert_eq!(capture.shutdown_calls(), 1);
}

#[test]
fn capture_full_voice_answers_like_nvda_plus_sapi() {
    let mut capture = CaptureSpeech::full_voice();
    assert!(capture.available());
    assert_eq!(capture.backend_name(), "NVDA");
    assert!(capture.has_separate_event_voice());
    assert_eq!(capture.event_backend_name(), "SAPI");
    assert!(capture.supports_rate() && capture.supports_pitch() && capture.supports_volume());
    assert!(capture.event_supports_rate());
    assert_eq!(capture.event_backend_options(), vec!["OneCore", "SAPI"]);
    assert_eq!(capture.voice_names(), vec!["David", "Zira"]);
    capture.select_event_backend(None); // events on the main voice
    assert!(!capture.has_separate_event_voice());
    assert_eq!(capture.event_backend_name(), "none");
    assert!(!capture.event_supports_rate());
    capture.select_event_backend(Some("OneCore"));
    assert!(capture.has_separate_event_voice());
    assert_eq!(capture.event_preference(), Some("OneCore"));
}

#[test]
fn capture_counts_stops_previews_and_configure_calls() {
    let mut capture = CaptureSpeech::new();
    capture.stop_main();
    capture.stop_event();
    capture.stop_event();
    capture.stop();
    assert_eq!(
        (
            capture.stop_main_calls(),
            capture.stop_event_calls(),
            capture.stop_calls()
        ),
        (1, 2, 1)
    );
    assert!(capture.say_adjustment_preview("speech_rate", "Speech rate: 60 percent.", true));
    assert!(!capture.say_adjustment_preview("speech_verbosity", "Speech verbosity: normal.", true));
    assert!(!capture.say_adjustment_preview("speech_rate", "", true));
    assert_eq!(
        capture.previews(),
        &[(
            "speech_rate".to_string(),
            "Speech rate: 60 percent.".to_string(),
            true
        )]
    );
    capture.configure(Some(0.8), None, Some(1.0), Some("Zira"));
    assert_eq!(
        capture.configure_calls(),
        &[(Some(0.8), None, Some(1.0), Some("Zira".to_string()))]
    );
    assert!(capture.lines().is_empty()); // previews and config are not speech
}

#[test]
fn capture_apply_speech_settings_keeps_the_saved_voice_name() {
    // The headless capture binds nothing, so the Python apply_speech rule
    // ("record the voice actually used") never rewrites the setting.
    let mut capture = CaptureSpeech::new();
    let mut settings = ff_core::settings::Settings {
        event_backend: "OneCore".to_string(),
        ..Default::default()
    };
    apply_speech_settings(&mut capture, &mut settings);
    assert_eq!(settings.event_backend, "OneCore");
    assert_eq!(capture.configure_calls().len(), 1);
    // And the full-voice capture offers OneCore as an option, so the choice
    // stands even though the capture still reports SAPI as bound.
    let mut full = CaptureSpeech::full_voice();
    apply_speech_settings(&mut full, &mut settings);
    assert_eq!(settings.event_backend, "OneCore");
}

#[test]
fn apply_speech_settings_keeps_a_chosen_voice_the_sink_has_not_bound_yet() {
    // MariahL, 2026-09-02: "Windows OneCore" for driving events never
    // survived a restart. The threaded sink queues the switch and answers
    // event_backend_name() from a snapshot that still names the voice bound
    // BEFORE the switch, so the read-back wrote SAPI over the fresh choice in
    // memory and the quit-time settings save put it on disk. The full-voice
    // capture is exactly that lagging sink: OneCore is on offer, SAPI is
    // what it reports.
    let mut lagging = CaptureSpeech::full_voice();
    let mut settings = ff_core::settings::Settings {
        sapi_events: true,
        event_backend: "OneCore".to_string(),
        ..Default::default()
    };
    apply_speech_settings(&mut lagging, &mut settings);
    assert_eq!(lagging.event_backend_name(), "SAPI"); // the stale answer
    assert_eq!(settings.event_backend, "OneCore"); // the choice survives it

    // A voice that is not on this machine at all still resolves to what the
    // sink will bind in its place, so the menu and later sessions read true.
    settings.event_backend = "AVSpeech".to_string();
    apply_speech_settings(&mut lagging, &mut settings);
    assert_eq!(settings.event_backend, "OneCore");
}

#[test]
fn null_speech_swallows_everything() {
    let mut null = NullSpeech;
    null.say("hello", true);
    null.say_event("hello", false);
    null.stop();
    null.poll(REFRESH_INTERVAL_S);
    assert!(!null.available());
    assert_eq!(null.backend_name(), "none");
    assert!(!null.has_separate_event_voice());
    assert!(!null.say_adjustment_preview("speech_rate", "x", true));
    assert!(!null.refresh());
    null.shutdown();
}

// -- braille only (Settings > Speech, "Output: braille only") -------------------
//
// Asked for on AppleVis, 2026-09-02: a player who reads a braille display
// wants the game's speech off. Prism's NVDA and JAWS backends can braille
// on their own; nothing else can, and the game must never go quiet.

/// NVDA with a display, and the SAPI event voice alongside it.
fn braille_speech() -> (Speech, FakeVoice, FakeVoice) {
    let nvda = FakeVoice::new("NVDA", 103, VoiceFeatures::BRAILLING);
    let sapi = FakeVoice::new("SAPI", 97, ADJUSTABLE);
    let ctx = FakeRegistry::new(vec![nvda.clone(), sapi.clone()]);
    let speech = Speech::with_registry(Box::new(ctx), None);
    assert_eq!(speech.backend_name(), "NVDA");
    assert!(speech.has_separate_event_voice());
    (speech, nvda, sapi)
}

#[test]
fn braille_only_puts_menu_lines_on_the_display_and_speaks_nothing() {
    let (mut speech, nvda, _sapi) = braille_speech();
    assert!(speech.supports_braille());
    speech.set_braille_only(true);
    speech.say("Main menu", true);
    assert_eq!(nvda.brailled(), vec!["Main menu".to_string()]);
    assert!(nvda.spoken().is_empty());

    // Off again: back to speech plus braille.
    speech.set_braille_only(false);
    speech.say("Back", true);
    assert_eq!(nvda.spoken(), spoken(&[("Back", true)]));
    assert_eq!(nvda.brailled().len(), 2);
}

#[test]
fn braille_only_routes_driving_events_to_the_display_instead_of_the_event_voice() {
    let (mut speech, nvda, sapi) = braille_speech();
    speech.set_braille_only(true);
    speech.say_event("Exit 12 in one mile.", true);
    speech.say_event("Speed limit 55.", false);
    assert_eq!(
        nvda.brailled(),
        vec![
            "Exit 12 in one mile.".to_string(),
            "Speed limit 55.".to_string()
        ]
    );
    assert!(sapi.spoken().is_empty());
    assert!(nvda.spoken().is_empty());
    // Nothing was speaking, so nothing was cut.
    assert_eq!(nvda.stop_calls(), 0);
}

#[test]
fn braille_only_keeps_speaking_when_the_voice_cannot_braille() {
    let sapi = FakeVoice::new("SAPI", 97, ADJUSTABLE);
    let ctx = FakeRegistry::new(vec![sapi.clone()]);
    let mut speech = Speech::with_registry(Box::new(ctx), None);
    assert!(!speech.supports_braille());
    speech.set_braille_only(true);
    speech.say("Main menu", true);
    speech.say_event("Exit 12 in one mile.", false);
    assert_eq!(
        sapi.spoken(),
        spoken(&[("Main menu", true), ("Exit 12 in one mile.", false)])
    );
    assert!(sapi.brailled().is_empty());
}

#[test]
fn braille_only_falls_back_to_speech_when_the_display_call_fails() {
    let (mut speech, nvda, _sapi) = braille_speech();
    speech.set_braille_only(true);
    nvda.set_fail_braille(true);
    speech.say("Main menu", true);
    assert!(nvda.brailled().is_empty());
    assert_eq!(nvda.spoken(), spoken(&[("Main menu", true)]));
    assert_eq!(speech.backend_name(), "NVDA"); // not treated as a dead voice
}

#[test]
fn braille_only_survives_a_voice_switch_and_returns_with_the_screen_reader() {
    let (mut speech, nvda, sapi) = braille_speech();
    speech.set_braille_only(true);

    // NVDA quits: the next line is spoken by SAPI, which has no display.
    nvda.set_runtime_supported(false);
    nvda.set_fail_output(true);
    nvda.set_fail_braille(true);
    speech.say("Route planner", true);
    assert_eq!(speech.backend_name(), "SAPI");
    assert!(!speech.supports_braille());
    assert_eq!(sapi.spoken(), spoken(&[("Route planner", true)]));

    // NVDA is back: braille only resumes without the player touching the row.
    nvda.set_runtime_supported(true);
    nvda.set_fail_output(false);
    nvda.set_fail_braille(false);
    speech.poll(REFRESH_INTERVAL_S);
    assert_eq!(speech.backend_name(), "NVDA");
    assert!(speech.supports_braille());
    speech.say("Route planner", true);
    assert_eq!(nvda.brailled(), vec!["Route planner".to_string()]);
    assert!(nvda.spoken().is_empty());
}

#[test]
fn apply_speech_settings_pushes_braille_only() {
    let (mut speech, nvda, _sapi) = braille_speech();
    let mut settings = ff_core::settings::Settings {
        braille_only: true,
        ..Default::default()
    };
    apply_speech_settings(&mut speech, &mut settings);
    speech.say("Main menu", true);
    assert_eq!(nvda.brailled(), vec!["Main menu".to_string()]);
    assert!(nvda.spoken().is_empty());

    settings.braille_only = false;
    apply_speech_settings(&mut speech, &mut settings);
    speech.say("Main menu", true);
    assert_eq!(nvda.spoken(), spoken(&[("Main menu", true)]));
}
