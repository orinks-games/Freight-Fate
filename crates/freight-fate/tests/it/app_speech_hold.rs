//! Road lines wait for a quiet voice when the voice can say it is speaking
//! (NVDA 2026.3 and later, SAPI, OneCore): `speech_delivery::hold`.

use freight_fate::app::testing::{FakeClock, TestApp};
use freight_fate::app::SayEvent;
use freight_fate::speech::EventPriority;

fn rig(app: &mut TestApp) -> FakeClock {
    app.ctx.settings.sapi_events = true;
    app.fake_pacer_clock()
}

/// Report the voice's state and run the per-frame release.
fn frame(app: &mut TestApp, speaking: bool) {
    app.speech_mut().set_voice_speaking(Some(speaking));
    app.ctx.update_event_voice_report();
}

fn heard(app: &TestApp, text: &str) -> bool {
    app.speech().lines().iter().any(|line| line.contains(text))
}

/// The first line goes straight out (nothing has reported yet); the voice
/// then reports it is speaking.
fn talking(app: &mut TestApp) {
    app.ctx
        .say_event_with("Weigh station open.", SayEvent::queued());
    frame(app, true);
}

#[test]
fn test_a_line_waits_while_the_voice_talks_and_goes_when_it_stops() {
    let mut app = TestApp::new();
    let clock = rig(&mut app);
    talking(&mut app);

    app.ctx
        .say_event_with("Rest area in 5 miles.", SayEvent::queued());
    assert_eq!(app.ctx.held_event_count(), 1);
    assert!(!heard(&app, "Rest area"));

    clock.advance(1.0);
    frame(&mut app, true);
    assert!(!heard(&app, "Rest area"), "still talking");

    clock.advance(0.5);
    frame(&mut app, false);
    assert!(heard(&app, "Rest area"));
    assert_eq!(app.ctx.held_event_count(), 0);
    app.shutdown();
}

#[test]
fn test_chatter_that_waited_too_long_is_dropped_not_read_late() {
    let mut app = TestApp::new();
    let clock = rig(&mut app);
    talking(&mut app);
    app.ctx
        .say_event_with("Rest area in 5 miles.", SayEvent::queued());
    for _ in 0..8 {
        clock.advance(0.5);
        frame(&mut app, true);
    }
    frame(&mut app, false);
    assert!(!heard(&app, "Rest area"));
    assert_eq!(app.ctx.held_event_count(), 0);
    app.shutdown();
}

#[test]
fn test_the_most_important_held_line_goes_first() {
    let mut app = TestApp::new();
    let clock = rig(&mut app);
    talking(&mut app);
    app.ctx
        .say_event_with("Rest area in 5 miles.", SayEvent::queued());
    app.ctx.say_event_with(
        "Exit 42 in half a mile.",
        SayEvent::queued().priority(EventPriority::Important),
    );
    clock.advance(1.0);
    frame(&mut app, false);
    assert!(heard(&app, "Exit 42"));
    assert!(
        !heard(&app, "Rest area"),
        "waits for the exit line to finish"
    );

    clock.advance(0.5);
    frame(&mut app, false);
    assert!(heard(&app, "Rest area"));
    app.shutdown();
}

#[test]
fn test_an_interrupting_line_is_never_held() {
    let mut app = TestApp::new();
    let _clock = rig(&mut app);
    talking(&mut app);
    app.ctx.say_event("Brake now!");
    assert!(heard(&app, "Brake now!"));
    assert_eq!(app.ctx.held_event_count(), 0);
    app.shutdown();
}

#[test]
fn test_an_urgent_line_is_not_kept_waiting_forever() {
    let mut app = TestApp::new();
    let clock = rig(&mut app);
    talking(&mut app);
    app.ctx.say_event_with(
        "Exit 42 in half a mile.",
        SayEvent::queued().priority(EventPriority::Important),
    );
    for _ in 0..30 {
        clock.advance(0.5);
        frame(&mut app, true);
    }
    assert!(heard(&app, "Exit 42"));
    app.shutdown();
}

#[test]
fn test_a_voice_that_stops_reporting_gets_what_was_held() {
    let mut app = TestApp::new();
    let clock = rig(&mut app);
    talking(&mut app);
    app.ctx
        .say_event_with("Rest area in 5 miles.", SayEvent::queued());
    app.speech_mut().set_voice_speaking(None);
    clock.advance(1.5);
    app.ctx.update_event_voice_report();
    assert!(heard(&app, "Rest area"));
    app.shutdown();
}

#[test]
fn test_without_reports_lines_go_straight_to_the_voice() {
    let mut app = TestApp::new();
    let _clock = rig(&mut app);
    app.ctx
        .say_event_with("Weigh station open.", SayEvent::queued());
    app.ctx
        .say_event_with("Rest area in 5 miles.", SayEvent::queued());
    assert!(heard(&app, "Rest area"));
    assert_eq!(app.ctx.held_event_count(), 0);
    app.shutdown();
}
