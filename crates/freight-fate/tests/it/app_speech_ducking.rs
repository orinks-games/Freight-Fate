//! Port of the `App`/`GameContext` half of `tests/test_speech_ducking.py`:
//! game audio steps back while the event voice speaks (R13, XAG 105).
//!
//! A warning must survive a loud cab -- engine, weather, and the radio --
//! without the voice itself getting louder. The duck engages the moment a
//! line reaches the event channel and restores on the pacer's own
//! projection of when the voice falls silent: no polling of the speech
//! backend, which cannot be asked. (The backend half -- which channels the
//! duck scales -- is `tests/audio_speech_ducking.rs`.)

use ff_core::settings::Settings;
use ff_core::speech_pacing::SpeechCategory;
use freight_fate::app::testing::{AudioLog, FakeClock, TestApp};
use freight_fate::app::{Say, SayEvent};
use freight_fate::audio::SPEECH_DUCK_LEVEL;

fn rig(app: &mut TestApp) -> (AudioLog, FakeClock) {
    app.ctx.settings.sapi_events = true;
    // Opt in: the duck ships off by default (the engine is the instrument
    // panel), and these tests exercise what it does once a player enables
    // it.
    app.ctx.settings.duck_audio_for_speech = true;
    let audio = app.record_audio();
    let clock = app.fake_pacer_clock();
    (audio, clock)
}

#[test]
fn test_ducking_defaults_off() {
    // In an audio-first sim the engine is the instrument panel -- a blind
    // driver reads speed off it -- so ducking is opt-in for players who need
    // it, not a default that changes what everyone hears (owner, 2026-08-12).
    assert!(!Settings::default().duck_audio_for_speech);
}

#[test]
fn test_event_speech_ducks_the_mix_and_the_frame_after_silence_restores_it() {
    let mut app = TestApp::new();
    let (audio, clock) = rig(&mut app);

    app.ctx
        .say_event_with("Open weigh station ahead in two miles.", SayEvent::queued());
    assert_eq!(audio.borrow().ducks, vec![SPEECH_DUCK_LEVEL]);

    // Voice still speaking: the per-frame check leaves the duck alone.
    app.ctx.update_speech_duck();
    assert_eq!(audio.borrow().ducks, vec![SPEECH_DUCK_LEVEL]);

    // The projection says the line has finished: the mix comes back.
    clock.advance(30.0);
    app.ctx.update_speech_duck();
    assert_eq!(audio.borrow().ducks, vec![SPEECH_DUCK_LEVEL, 1.0]);

    // And it is restored exactly once, not every frame.
    app.ctx.update_speech_duck();
    assert_eq!(audio.borrow().ducks, vec![SPEECH_DUCK_LEVEL, 1.0]);
    app.shutdown();
}

#[test]
fn test_the_setting_turns_the_duck_off() {
    let mut app = TestApp::new();
    let (audio, _) = rig(&mut app);
    app.ctx.settings.duck_audio_for_speech = false;

    app.ctx
        .say_event_with("Open weigh station ahead in two miles.", SayEvent::queued());

    assert!(audio.borrow().ducks.is_empty());
    app.shutdown();
}

#[test]
fn test_a_suppressed_repeat_does_not_duck() {
    // A line the pacer never lets reach the voice must not touch the mix.
    let mut app = TestApp::new();
    let (audio, _) = rig(&mut app);
    let line = "You sideswiped a box truck in the right lane!";
    app.ctx.say_event(line);
    audio.borrow_mut().ducks.clear();

    app.ctx.say_event(line); // inside the repeat window

    assert!(audio.borrow().ducks.is_empty());
    app.shutdown();
}

#[test]
fn test_a_line_the_rung_silences_leaves_the_mix_alone() {
    // Nothing plays in place of a silenced line any more (owner,
    // 2026-10-03), so there is nothing to make room for: stepping the road
    // back for a line nobody hears would read as the engine dipping for no
    // reason.
    for rung in ["quiet", "urgent_only"] {
        let mut app = TestApp::new();
        let audio = app.record_audio();
        app.ctx.settings.duck_audio_for_speech = true;
        app.ctx.settings.driving_speech = rung.to_string();

        app.ctx.say_event_with(
            "Traffic ahead, slowing.",
            SayEvent::queued().category(SpeechCategory::Traffic),
        );

        assert!(audio.borrow().ducks.is_empty(), "{rung}");
        assert!(audio.borrow().played.is_empty(), "{rung}");
        assert!(!app.ctx.speech_ducked(), "{rung}");
        app.shutdown();
    }
}

#[test]
#[ignore = "Python swept its own source text for the setting check at every duck engage point; a source sweep has no Rust equivalent. The Rust engage point is engage_speech_duck, gated, plus the driving-state ducks"]
fn test_nothing_anywhere_ducks_when_the_player_turned_ducking_off() {}

#[test]
fn test_with_ducking_off_an_earcon_leaves_the_mix_alone() {
    // The behavioral half of the rule, end to end.
    let mut app = TestApp::new();
    let audio = app.record_audio();
    app.ctx.settings.duck_audio_for_speech = false;
    app.ctx.settings.driving_speech = "urgent_only".to_string();

    app.ctx.say_event_with(
        "Automatic braking.",
        SayEvent::queued().category(SpeechCategory::Confirmation),
    );

    assert!(
        audio.borrow().ducks.is_empty(),
        "the mix was stepped back anyway: {:?}",
        audio.borrow().ducks
    );
    assert!(!app.ctx.speech_ducked());
    app.shutdown();
}

#[test]
fn test_with_ducking_off_a_say_path_earcon_leaves_the_mix_alone() {
    let mut app = TestApp::new();
    let audio = app.record_audio();
    app.ctx.settings.duck_audio_for_speech = false;
    app.ctx.settings.driving_speech = "urgent_only".to_string();

    app.ctx.say_with(
        "Cruise set.",
        Say::new().category(SpeechCategory::Confirmation),
    );

    assert!(
        audio.borrow().ducks.is_empty(),
        "the mix was stepped back anyway: {:?}",
        audio.borrow().ducks
    );
    assert!(!app.ctx.speech_ducked());
    app.shutdown();
}
