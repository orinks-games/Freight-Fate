//! The lines at a state, province or country line are said once: a status
//! readout, the customs booth or a police stop that cuts one does not bring
//! it back in full after (cross-border drives, 2026-11-23: the crossing call
//! and the hours rules were each heard twice).

use ff_core::speech_pacing::SpeechCategory;
use freight_fate::app::SayEvent;
use freight_fate::playtest::harness::{PlaytestHarness, RouteSetup};
use freight_fate::speech::EventPriority;
use freight_fate::states::driving_updates::after_crossing::AfterCrossing;

const HOURS_LINE: &str =
    "Canadian hours rules: 13 hours of driving and 14 on duty in a 16-hour window.";
const CUTTER: &str = "Lights behind you. Signal and stop on the shoulder.";

fn a_drive() -> PlaytestHarness {
    let mut harness = PlaytestHarness::new();
    harness.start_route(
        "whitehorse_yt_ca",
        "tok_ak_us",
        RouteSetup::seeded(2202).cities(&[
            "whitehorse_yt_ca",
            "haines_junction_yt_ca",
            "tok_ak_us",
        ]),
    );
    harness.with_drive(|drive, _| {
        drive.tutorial = None;
        drive.departure_checked = true;
    });
    harness.clear_speech();
    harness
}

/// Speak `line` through `say`, cut it a moment in with an interrupting
/// line, and let a few seconds pass. Returns how often the line was heard
/// and how often it was handed back.
fn cut_a_moment_in(
    harness: &mut PlaytestHarness,
    say: impl FnOnce(&mut PlaytestHarness),
) -> (usize, usize) {
    say(harness);
    harness.advance_clock(0.3);
    harness.with_drive(|_, ctx| {
        ctx.say_event_with(
            CUTTER,
            SayEvent::new()
                .priority(EventPriority::Critical)
                .category(SpeechCategory::Safety),
        );
    });
    for _ in 0..600 {
        harness.advance_clock(1.0 / 60.0);
        harness.with_drive(|drive, ctx| drive.update_after_crossing(ctx, 1.0 / 60.0));
    }
    let heard = harness
        .app
        .event_calls()
        .iter()
        .filter(|(text, _)| text == HOURS_LINE)
        .count();
    (heard, harness.app.ctx.handed_back_count("hours rules"))
}

#[test]
fn a_cut_hours_rules_line_is_said_once() {
    let mut harness = a_drive();
    let (heard, handed_back) = cut_a_moment_in(&mut harness, |harness| {
        harness.with_drive(|drive, ctx| {
            drive.queue_after_crossing(
                AfterCrossing::HoursRules,
                HOURS_LINE,
                Some(SpeechCategory::Navigation),
            );
            drive.update_after_crossing(ctx, 0.0);
        });
    });
    assert_eq!(heard, 1, "{:?}", harness.app.event_calls());
    assert_eq!(handed_back, 0);
}

/// The same line said any other way is handed back when cut: what the test
/// above pins is the crossing lines' own rule, not a cut that never landed.
#[test]
fn the_same_line_outside_the_crossing_queue_is_handed_back() {
    let mut harness = a_drive();
    let (heard, handed_back) = cut_a_moment_in(&mut harness, |harness| {
        harness.with_drive(|_, ctx| {
            ctx.say_event_with(
                HOURS_LINE,
                SayEvent::queued()
                    .priority(EventPriority::Important)
                    .category(SpeechCategory::Navigation),
            );
        });
    });
    assert_eq!(handed_back, 1, "{:?}", harness.app.event_calls());
    assert_eq!(heard, 2);
}
