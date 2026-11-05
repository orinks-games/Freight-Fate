//! The exit countdown survives a busy voice.
//!
//! With a voice that reports whether it is speaking, a road line waits for
//! it to fall quiet, and a route line that waited eight seconds used to be
//! dropped. Into Traverse City the two-mile and one-mile calls went that
//! way, exit speed and all, and only "in 8 miles" and "half a mile" were
//! heard (owner's drive, 2026-11-04). The countdown is kept now: a newer
//! anchor replaces one still waiting, and a call that cannot get a quiet
//! voice goes behind it.

use ff_core::sim::trip_models::RoadStop;

use freight_fate::app::SayEvent;
use freight_fate::playtest::harness::{PlaytestHarness, StartDelivery};

const DT: f64 = 1.0 / 60.0;

fn a_busy_drive(name: &str) -> (PlaytestHarness, f64) {
    let mut harness = PlaytestHarness::new();
    harness.start_delivery(StartDelivery::named(name));
    harness.app.ctx.settings.sapi_events = true;
    let at_mi = harness.read_drive(|d| d.trip.position_mi) + 3.0;
    harness.with_drive(move |drive, _| {
        drive.tutorial = None;
        let mut stop = RoadStop::new("Test Receiver", at_mi, "delivery_destination");
        stop.actions = vec!["deliver".to_string()];
        drive.exit_stop = Some(stop);
        drive.exit_countdown_said.clear();
    });
    harness.clear_speech();
    harness
        .app
        .ctx
        .say_event_with("Weigh station open.", SayEvent::queued());
    voice(&mut harness, true);
    (harness, at_mi)
}

/// The voice reports, and the per-frame release runs.
fn voice(harness: &mut PlaytestHarness, speaking: bool) {
    harness.app.speech_mut().set_voice_speaking(Some(speaking));
    harness.app.ctx.update_event_voice_report();
}

fn reach(harness: &mut PlaytestHarness, at_mi: f64, ahead: f64) {
    harness.with_drive(move |drive, ctx| {
        drive.trip.position_mi = at_mi - ahead;
        drive.update_exit(ctx, 0.0, DT);
    });
}

/// Let `seconds` pass with the voice talking the whole time.
fn talk_for(harness: &mut PlaytestHarness, seconds: f64) {
    for _ in 0..(seconds / 0.5) as usize {
        harness.advance_clock(0.5);
        voice(harness, true);
    }
}

fn countdown(harness: &PlaytestHarness) -> Vec<String> {
    harness
        .app
        .speech()
        .lines()
        .into_iter()
        .filter(|line| line.contains("Destination exit in"))
        .collect()
}

#[test]
fn a_countdown_call_behind_a_voice_that_never_stops_still_goes() {
    let (mut harness, at_mi) = a_busy_drive("Hold Never Quiet");
    reach(&mut harness, at_mi, 1.9);
    talk_for(&mut harness, 12.0);
    let calls = countdown(&harness);
    assert_eq!(calls.len(), 1, "{calls:?}");
    assert!(
        calls[0].starts_with("Destination exit in 2 miles"),
        "{calls:?}"
    );
    assert_eq!(harness.app.ctx.held_event_count(), 0);
}

#[test]
fn the_mile_call_replaces_a_waiting_two_mile_call_and_keeps_the_exit_speed() {
    let (mut harness, at_mi) = a_busy_drive("Hold Replace");
    reach(&mut harness, at_mi, 1.9);
    talk_for(&mut harness, 3.0);
    reach(&mut harness, at_mi, 0.9);
    talk_for(&mut harness, 3.0);
    // Still waiting when the half-mile anchor comes: that call carries the
    // exit speed the mile's call would have said.
    reach(&mut harness, at_mi, 0.45);
    assert_eq!(harness.app.ctx.held_event_count(), 1);
    harness.advance_clock(0.5);
    voice(&mut harness, false);
    let calls = countdown(&harness);
    assert_eq!(calls.len(), 1, "{calls:?}");
    assert!(
        calls[0].starts_with("Destination exit in half a mile. Exit speed"),
        "{calls:?}"
    );
}
