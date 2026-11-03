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
fn test_a_route_line_whose_moment_passed_is_dropped() {
    // Held behind a hazard brake and its aftermath for over eight seconds:
    // the road has moved on, and saying it now would only precede the
    // game saying it again.
    let mut app = TestApp::new();
    let clock = rig(&mut app);
    talking(&mut app);
    app.ctx.say_event_with(
        "Construction zone ahead, cruise easing to 55.",
        SayEvent::queued().priority(EventPriority::Important),
    );
    for _ in 0..18 {
        clock.advance(0.5);
        frame(&mut app, true);
    }
    assert_eq!(app.ctx.held_event_count(), 0);
    frame(&mut app, false);
    assert!(!heard(&app, "cruise easing"));
    app.shutdown();
}

#[test]
fn test_a_queued_critical_line_is_never_dropped() {
    let mut app = TestApp::new();
    let clock = rig(&mut app);
    talking(&mut app);
    app.ctx.say_event_with(
        "Engine stalled.",
        SayEvent::queued().priority(EventPriority::Critical),
    );
    for _ in 0..14 {
        clock.advance(0.5);
        frame(&mut app, true);
    }
    assert!(heard(&app, "Engine stalled."));
    app.shutdown();
}

#[test]
fn test_chatter_is_held_not_dropped_by_the_estimate() {
    // With the voice reporting, the hold judges chatter by how long it
    // actually waited, not by the length-based estimate of what is ahead.
    let mut app = TestApp::new();
    let clock = rig(&mut app);
    app.ctx.say_event_with(
        "Open weigh station ahead in two miles. Have your paperwork ready.",
        SayEvent::queued(),
    );
    frame(&mut app, true);
    app.ctx.say_event_with(
        "End of construction zone. Speed limit 65.",
        SayEvent::queued(),
    );
    assert_eq!(app.ctx.held_event_count(), 1);
    clock.advance(1.0);
    frame(&mut app, false);
    assert!(heard(&app, "End of construction zone"));
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

#[test]
fn test_with_both_voices_on_the_screen_reader_lines_still_wait() {
    // SAPI event voice off: road lines go to the main voice, and that is
    // the voice asked.
    let mut app = TestApp::new();
    let clock = app.fake_pacer_clock();
    app.ctx.settings.sapi_events = false;
    talking(&mut app);
    app.ctx
        .say_event_with("Rest area in 5 miles.", SayEvent::queued());
    assert_eq!(app.ctx.held_event_count(), 1);
    clock.advance(1.0);
    frame(&mut app, false);
    assert!(heard(&app, "Rest area"));
    app.shutdown();
}

#[test]
fn test_a_road_line_waits_for_an_info_readout_on_the_shared_voice() {
    // Both voices on the screen reader, long after the last road line: the
    // player asks for the clock, and a road line raised in the same frame
    // waits for the readout instead of going out over it (owner's NVDA
    // 2026.3 drive, 2026-11-03).
    let mut app = TestApp::new();
    let clock = app.fake_pacer_clock();
    app.ctx.settings.sapi_events = false;
    talking(&mut app);
    for _ in 0..20 {
        clock.advance(1.0);
        frame(&mut app, false);
    }

    app.ctx.say("It's 3:15 PM.");
    app.ctx
        .say_event_with("Rest area in 5 miles.", SayEvent::queued());
    assert_eq!(app.ctx.held_event_count(), 1, "the readout is speech");

    clock.advance(0.1);
    frame(&mut app, false);
    assert!(!heard(&app, "Rest area"), "too soon for the voice to start");
    clock.advance(1.0);
    frame(&mut app, true);
    assert!(!heard(&app, "Rest area"), "still reading the clock");
    clock.advance(1.0);
    frame(&mut app, false);
    assert!(heard(&app, "Rest area"));
    app.shutdown();
}

#[test]
fn test_chatter_outlasts_a_long_readout_instead_of_going_stale() {
    // A ten-second Clock readout used to cost "Exit traffic, 2 miles. Hold
    // right" its 3-second budget (owner's third drive, 2026-11-03).
    let mut app = TestApp::new();
    let clock = app.fake_pacer_clock();
    app.ctx.settings.sapi_events = false;
    talking(&mut app);
    for _ in 0..20 {
        clock.advance(1.0);
        frame(&mut app, false);
    }
    app.ctx.say("It's 3:15 PM, day 2. Next rest due in 6 hours.");
    app.ctx.say_event_with(
        "Exit traffic, 2 miles. Hold right.",
        SayEvent::queued().priority(EventPriority::Ambient),
    );
    assert_eq!(app.ctx.held_event_count(), 1);
    for _ in 0..80 {
        clock.advance(0.1);
        frame(&mut app, true);
    }
    assert_eq!(app.ctx.held_event_count(), 1, "kept through the readout");
    clock.advance(0.1);
    frame(&mut app, false);
    assert!(heard(&app, "Exit traffic"));
    app.shutdown();
}
