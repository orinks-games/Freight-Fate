//! Port of the `GameContext` tests of `tests/test_event_speech_pacer.py`:
//! the event-voice pacer as `ctx.say_event` runs it (the pure pacer tests
//! live in `ff_core::speech_pacing`).
//!
//! Owner playtest 2026-07-15: arriving at the yard played the whole
//! approach script late -- "slow down to dock, at dock, delivering" heard
//! after the trailer was already empty -- because the event voice queues
//! faster than it speaks. The pacer projects when the channel falls silent;
//! a queued line that would start speaking more than STALE_WAIT_S after the
//! moment it described flushes the backlog and speaks fresh.

use freight_fate::app::testing::TestApp;
use freight_fate::app::SayEvent;
use freight_fate::speech::CaptureSpeech;
use hearken::speech::EventPriority;

const LONG_LINE_LEN: usize = 130; // ~10 seconds at the default 13 chars per second
const SIDESWIPE: &str = "You sideswiped a box truck in the right lane! The truck took damage, \
now 13 percent. Check your mirrors before moving over.";
const CHATTER: &str = "Rain easing off, roads still wet."; // ~2.4s estimated
const STOP_LINE: &str = "Planned stop, Iowa 80 Truckstop at Exit 284 in five miles.";
const HAZARD: &str = "Hazard! Stopped traffic ahead.";
const SCALE_LINE: &str = "Open weigh station ahead in two miles. All trucks must pull in.";
const INFO_REPLY: &str = "Fifty five miles per hour.";

fn long_line() -> String {
    "x".repeat(LONG_LINE_LEN)
}

fn sapi_app() -> TestApp {
    let mut app = TestApp::new();
    app.ctx.settings.sapi_events = true;
    app
}

fn calls(pairs: &[(&str, bool)]) -> Vec<(String, bool)> {
    pairs.iter().map(|(t, i)| (t.to_string(), *i)).collect()
}

fn logged(app: &TestApp) -> Vec<String> {
    app.ctx
        .message_log
        .messages
        .iter()
        .map(|m| m.text.clone())
        .collect()
}

const APPROACH: [&str; 4] = [
    "Slow down for the dock, twenty five miles per hour through the yard.",
    "Passing the fuel island, dock doors ahead on the left.",
    "At the dock. Line up square and ease it back.",
    "Delivering. The forklift crew is unloading the trailer.",
];

#[test]
fn test_say_event_queues_a_burst_of_route_lines_whole_end_to_end() {
    // Four ROUTE lines in one frame are one moment of road: all of it is
    // current and none of it heard yet. Each used to flush the one before
    // it inside its pre-utterance pause and hand it straight back, so the
    // voice was sent 0, 0, 1, 1, 2, 2, 3 and every playtest log read as
    // each line said twice ("1000 feet.", "Light yellow.", the take line;
    // live drive into Abilene, 2026-09-24). Now they queue, once each, in
    // order, and nothing is handed back.
    let mut app = sapi_app();
    for line in APPROACH {
        app.ctx
            .say_event_with(line, SayEvent::queued().priority(EventPriority::Important));
    }
    assert_eq!(
        app.event_calls(),
        APPROACH
            .iter()
            .map(|line| (line.to_string(), false))
            .collect::<Vec<_>>()
    );
    assert_eq!(app.ctx.handed_back_count(""), 0);
    app.shutdown();
}

#[test]
fn test_say_event_flushes_a_stale_route_backlog_end_to_end() {
    // A backlog the voice has been working through is another matter: once
    // a ROUTE line would start stale behind it, the channel is purged and
    // the line speaks now -- staleness changes delivery, never drops the
    // newest information.
    let mut app = sapi_app();
    let clock = app.fake_pacer_clock();
    for line in &APPROACH[..3] {
        app.ctx
            .say_event_with(*line, SayEvent::queued().priority(EventPriority::Important));
    }
    clock.advance(1.0);
    app.ctx.say_event_with(
        APPROACH[3],
        SayEvent::queued().priority(EventPriority::Important),
    );
    let calls = app.event_calls();
    assert!(calls.iter().take(3).all(|(_, interrupt)| !interrupt));
    assert!(
        calls[3..].iter().any(|(_, interrupt)| *interrupt),
        "a stale backlog was performed in full -- the pacer never flushed"
    );
    assert_eq!(calls.last(), Some(&(APPROACH[3].to_string(), false)));
    app.shutdown();
}

#[test]
fn test_stale_ambient_chatter_is_dropped_not_promoted() {
    // R1: chatter that would start speaking after the moment it described
    // is discarded silently -- the old stale-flush promoted it to an
    // interrupt, making the least important class the only one guaranteed
    // to preempt. The review log still keeps the dropped line.
    let mut app = sapi_app();
    let clock = app.fake_pacer_clock();

    app.ctx.say_event_with(long_line(), SayEvent::queued()); // ~10s of speaking
    app.ctx.say_event_with(CHATTER, SayEvent::queued()); // would start far too late

    assert_eq!(
        app.event_calls(),
        calls(&[(&long_line(), false)]),
        "stale chatter reached the voice"
    );
    // Dropped from the air, kept in the log: recovery is what it is for.
    assert!(logged(&app).iter().any(|m| m == CHATTER));
    // Never marked heard either: the player did not hear it, so the same
    // observation made fresh later speaks normally.
    clock.advance(30.0);
    app.ctx.say_event_with(CHATTER, SayEvent::queued());
    assert_eq!(
        app.event_calls().last().unwrap(),
        &(CHATTER.to_string(), false)
    );
    app.shutdown();
}

#[test]
fn test_say_event_speaks_a_repeated_event_once() {
    let mut app = sapi_app();
    for _ in 0..3 {
        // the burst the tester heard in six tenths of a second
        app.ctx.say_event(SIDESWIPE);
    }
    assert_eq!(app.event_lines(), vec![SIDESWIPE.to_string()]);
    // And a suppressed repeat is not left in the review history either.
    assert_eq!(logged(&app).iter().filter(|m| *m == SIDESWIPE).count(), 1);
    app.shutdown();
}

#[test]
fn test_say_event_standing_condition_speaks_again_only_when_it_worsens() {
    let mut app = sapi_app();
    let clock = app.fake_pacer_clock();

    let at_45 = "The load has shifted hard and is badly damaged, 45 percent.";
    let at_60 = "The load has shifted hard and is badly damaged, 60 percent.";
    app.ctx
        .say_event_with(at_45, SayEvent::new().key("cargo_condition"));
    for _ in 0..4 {
        // the rest of the drive, nothing about it changing
        clock.advance(10.0);
        app.ctx
            .say_event_with(at_45, SayEvent::new().key("cargo_condition"));
    }
    clock.advance(10.0);
    app.ctx
        .say_event_with(at_60, SayEvent::new().key("cargo_condition"));

    assert_eq!(
        app.event_lines(),
        vec![at_45.to_string(), at_60.to_string()]
    );
    app.shutdown();
}

#[test]
fn test_say_event_forced_line_is_heard_even_when_it_repeats() {
    // A status key answers every press, repeat or not.
    let mut app = sapi_app();
    let line = "Load secure.";
    app.ctx.say_event(line);
    app.ctx.say_event_with(line, SayEvent::new().force(true));
    assert_eq!(app.event_lines(), vec![line.to_string(), line.to_string()]);
    app.shutdown();
}

#[test]
fn test_pausing_silences_the_road_and_resuming_does_not_replay_it() {
    let mut app = sapi_app();
    app.ctx
        .say_event_with("Travel plaza in five miles.", SayEvent::queued());
    app.ctx
        .say_event_with("Rain easing off, roads still wet.", SayEvent::queued());

    let stops_before = app.speech().stop_event_calls();
    app.ctx.pause_event_speech();
    // Silencing the channel is what actually purges the voice's own queue;
    // without it the backlog is simply performed on the way back.
    assert!(
        app.speech().stop_event_calls() > stops_before,
        "pausing left the event voice holding the road's backlog"
    );

    app.ctx.resume_event_speech();
    app.ctx.say_event_with(
        "Speed limit reduced to 55 miles per hour.",
        SayEvent::queued(),
    );
    // The first line back purges rather than queueing behind anything that
    // survived the pause.
    assert_eq!(
        app.event_calls().last().unwrap(),
        &(
            "Speed limit reduced to 55 miles per hour.".to_string(),
            true
        )
    );
    app.shutdown();
}

#[test]
fn test_say_event_requeues_the_route_line_a_hazard_cut() {
    // ctx.say_event: safety line first, then the line it stepped on.
    let mut app = sapi_app();
    let _clock = app.fake_pacer_clock();

    app.ctx.say_event_with(
        STOP_LINE,
        SayEvent::queued().priority(EventPriority::Important),
    );
    app.ctx.say_event(HAZARD);

    assert_eq!(
        app.event_calls(),
        calls(&[(STOP_LINE, false), (HAZARD, true), (STOP_LINE, false)])
    );
    // Requeued, not re-reported: the review log still holds it once.
    assert_eq!(logged(&app).iter().filter(|m| *m == STOP_LINE).count(), 1);
    app.shutdown();
}

#[test]
fn test_say_event_drops_the_route_line_a_hazard_cut_in_its_last_words() {
    // Owner ruling 2026-09-01: a line the player had mostly heard is not
    // said again from the top -- the agent drives that day heard every such
    // requeue as a stutter. The review log still holds it once.
    let mut app = sapi_app();
    let clock = app.fake_pacer_clock();

    app.ctx.say_event_with(
        STOP_LINE,
        SayEvent::queued().priority(EventPriority::Important),
    );
    clock.advance(4.0); // ~4.9 s estimated: cut in its last words
    app.ctx.say_event(HAZARD);

    assert_eq!(
        app.event_calls(),
        calls(&[(STOP_LINE, false), (HAZARD, true)]),
        "a mostly-heard line was said again whole"
    );
    assert_eq!(logged(&app).iter().filter(|m| *m == STOP_LINE).count(), 1);
    app.shutdown();
}

#[test]
fn test_say_event_leaves_a_finished_route_line_alone() {
    let mut app = sapi_app();
    let clock = app.fake_pacer_clock();

    app.ctx.say_event_with(
        STOP_LINE,
        SayEvent::queued().priority(EventPriority::Important),
    );
    clock.advance(30.0); // heard in full long before the hazard
    app.ctx.say_event(HAZARD);

    assert_eq!(
        app.event_calls(),
        calls(&[(STOP_LINE, false), (HAZARD, true)])
    );
    app.shutdown();
}

#[test]
fn test_a_repeated_hazard_cannot_ping_pong_the_requeue() {
    // The same hazard firing in a burst is one cut, one requeue -- the
    // repeat suppression drops the copies before they reach the channel.
    let mut app = sapi_app();
    let _clock = app.fake_pacer_clock();

    app.ctx.say_event_with(
        STOP_LINE,
        SayEvent::queued().priority(EventPriority::Important),
    );
    for _ in 0..3 {
        app.ctx.say_event(HAZARD);
    }

    let calls = app.event_calls();
    assert_eq!(
        calls
            .iter()
            .filter(|c| **c == (HAZARD.to_string(), true))
            .count(),
        1
    );
    assert_eq!(
        calls
            .iter()
            .filter(|c| **c == (STOP_LINE.to_string(), false))
            .count(),
        2
    ); // the original and one requeue
    app.shutdown();
}

#[test]
fn test_a_requeued_line_cut_again_is_dropped_not_replayed() {
    // Two genuine warnings in a row do not destroy the stop notice -- it is
    // rescued once and finishes -- but they do not replay it either. One
    // rescue is the contract (the 21 August build note ruled the repeat the
    // bug: a chain of five trooper warnings spoke "Signal for the scale
    // exit" five times).
    let mut app = sapi_app();
    let _clock = app.fake_pacer_clock();

    app.ctx.say_event_with(
        STOP_LINE,
        SayEvent::queued().priority(EventPriority::Important),
    );
    app.ctx.say_event(HAZARD);
    app.ctx
        .say_event("Emergency vehicle approaching from behind.");

    assert_eq!(
        app.event_calls()
            .iter()
            .filter(|c| **c == (STOP_LINE.to_string(), false))
            .count(),
        2
    ); // original, one rescue
    app.shutdown();
}

// -- info keys on a shared voice (tester report, 2026-08-12) --------------------
//
// When events ride the main channel -- the player chose the main voice for
// them, or no separate voice could be bound -- an info key's reply
// interrupts whatever event line was mid-sentence there. The reply still
// answers first; the cut ROUTE or CRITICAL line queues right behind it.

#[test]
fn test_info_reply_on_the_main_voice_requeues_the_cut_event_line() {
    let mut app = TestApp::new();
    app.ctx.settings.sapi_events = false; // events through the main voice
    let _clock = app.fake_pacer_clock();

    app.ctx.say_event_with(
        SCALE_LINE,
        SayEvent::queued().priority(EventPriority::Important),
    );
    app.ctx.say(INFO_REPLY); // an info key answering, interrupt=True default

    assert_eq!(
        app.main_calls(),
        calls(&[(SCALE_LINE, false), (INFO_REPLY, true), (SCALE_LINE, false)])
    );
    app.shutdown();
}

#[test]
fn test_info_reply_requeues_when_no_separate_event_voice_bound() {
    // The player asked for a dedicated event voice but Prism bound none, so
    // events fall back to the main channel and need the same protection.
    let mut app = TestApp::new();
    app.ctx.settings.sapi_events = true; // asked for, but no backend bound
    let _clock = app.fake_pacer_clock();

    app.ctx.say_event_with(
        SCALE_LINE,
        SayEvent::queued().priority(EventPriority::Important),
    );
    app.ctx.say(INFO_REPLY);

    assert_eq!(app.main_calls(), calls(&[(INFO_REPLY, true)]));
    assert_eq!(
        app.event_calls(),
        calls(&[(SCALE_LINE, false), (SCALE_LINE, false)])
    ); // cut, requeued
    app.shutdown();
}

#[test]
fn test_info_reply_with_a_dedicated_event_voice_leaves_the_road_alone() {
    let mut app = TestApp::with_speech(CaptureSpeech::full_voice()); // a separate voice is bound
    app.ctx.settings.sapi_events = true;
    let _clock = app.fake_pacer_clock();

    app.ctx.say_event_with(
        SCALE_LINE,
        SayEvent::queued().priority(EventPriority::Important),
    );
    app.ctx.say(INFO_REPLY);

    // Two channels, two voices: the reply cannot cut the event line, so
    // nothing is requeued.
    assert_eq!(app.main_calls(), calls(&[(INFO_REPLY, true)]));
    assert_eq!(app.event_calls(), calls(&[(SCALE_LINE, false)]));
    app.shutdown();
}
