//! The fatigue band and the nod-off at the wheel: the cue for each band,
//! the hysteresis that keeps a small lift from replaying a line, the band in
//! the readouts, and both machines through a save and resume.
//!
//! The transition tables themselves are `ff_core::sim::alertness`'s unit
//! tests; these cases check what the driver hears.

use ff_core::sim::alertness::{Alertness, Nod, NodState};
use ff_core::sim::weather::WeatherKind;
use freight_fate::playtest::harness::{PlaytestHarness, StartDelivery};
use freight_fate::states::base::Menu;
use freight_fate::states::driving::DrivingState;
use freight_fate::states::driving_menu_states::{DriveRef, DrivingStatusScreenState};

/// A delivery under way on a quiet road, parked, so no fatigue accrues on
/// its own and each case sets the number it needs.
fn a_drive(name: &str) -> PlaytestHarness {
    let mut harness = PlaytestHarness::new();
    harness.start_delivery(StartDelivery::named(name));
    harness.with_drive(|d, _| {
        d.trip.set_npc_vehicles(Vec::new());
        d.weather_mut().current = WeatherKind::Clear;
        d.departure_checked = true;
        d.trip.truck.velocity_mps = 0.0;
    });
    harness.clear_speech();
    harness
}

/// Set the fatigue number and run one hours-and-fatigue frame.
fn at_fatigue(harness: &mut PlaytestHarness, fatigue: f64) {
    harness.app.ctx.profile.as_mut().expect("a career").fatigue = fatigue;
    harness.with_drive(|d, ctx| d.update_hours_and_fatigue(ctx, 0.1));
}

fn count(lines: &[String], text: &str) -> usize {
    lines.iter().filter(|line| line.contains(text)).count()
}

#[test]
fn entering_worn_yawns_once_and_says_nothing() {
    let mut harness = a_drive("Worn Yawn");
    let audio = harness.app.record_audio();
    for _ in 0..5 {
        at_fatigue(&mut harness, 41.0);
    }
    let yawns: Vec<_> = audio
        .borrow()
        .played
        .iter()
        .filter(|(key, _, _)| key == "driver/yawn")
        .cloned()
        .collect();
    assert_eq!(yawns, vec![("driver/yawn".to_string(), 0.7, 0.0)]);
    let said = harness.app.event_lines();
    assert_eq!(count(&said, "drowsy"), 0, "{said:?}");
    assert_eq!(harness.read_drive(|d| d.alertness), Alertness::Worn);
}

#[test]
fn a_small_lift_does_not_replay_getting_drowsy() {
    let mut harness = a_drive("Drowsy Lift");
    let audio = harness.app.record_audio();
    // The drowsy cue's own yawn, louder than the worn one and the periodic.
    let drowsy_yawns = || {
        audio
            .borrow()
            .played
            .iter()
            .filter(|played| **played == ("driver/yawn".to_string(), 0.9, 0.0))
            .count()
    };
    at_fatigue(&mut harness, 61.0);
    at_fatigue(&mut harness, 57.0);
    at_fatigue(&mut harness, 61.0);
    let said = harness.app.event_lines();
    assert_eq!(count(&said, "Getting drowsy."), 1, "{said:?}");
    assert_eq!(drowsy_yawns(), 1);
    // Five points under the line is a real rest, and the next crossing is
    // news again. (The pacer may hold back the same words twice in a row,
    // so the cue's sound is what is counted.)
    at_fatigue(&mut harness, 54.0);
    assert_eq!(harness.read_drive(|d| d.alertness), Alertness::Worn);
    at_fatigue(&mut harness, 61.0);
    assert_eq!(harness.read_drive(|d| d.alertness), Alertness::Drowsy);
    assert_eq!(drowsy_yawns(), 2);
}

#[test]
fn a_jump_to_severe_speaks_only_the_severe_line() {
    let mut harness = a_drive("Severe Jump");
    at_fatigue(&mut harness, 85.0);
    at_fatigue(&mut harness, 85.0);
    let said = harness.app.event_lines();
    assert_eq!(count(&said, "Dangerously drowsy"), 1, "{said:?}");
    assert_eq!(count(&said, "Getting drowsy."), 0, "{said:?}");
}

fn driver_screen_fatigue(harness: &mut PlaytestHarness) -> String {
    let handle = DriveRef::of(&harness.shared_driving().expect("a drive on the stack"));
    let mut screen = DrivingStatusScreenState::new(handle, "driver");
    let items = screen.build_items(&mut harness.app.ctx);
    items
        .iter()
        .map(|item| item.text(&screen, &harness.app.ctx))
        .find(|text| text.starts_with("Fatigue:"))
        .expect("the driver screen carries a Fatigue line")
}

fn status_fatigue(harness: &mut PlaytestHarness) -> Option<String> {
    harness.with_drive(|d, ctx| {
        d.status_lines(ctx)
            .into_iter()
            .find(|line| line.starts_with("Fatigue:"))
    })
}

#[test]
fn the_readouts_name_the_band_from_worn_up() {
    let mut harness = a_drive("Band Readout");
    at_fatigue(&mut harness, 30.0);
    assert_eq!(status_fatigue(&mut harness), None);
    assert_eq!(driver_screen_fatigue(&mut harness), "Fatigue: 30 percent");

    at_fatigue(&mut harness, 45.0);
    assert_eq!(
        status_fatigue(&mut harness).as_deref(),
        Some("Fatigue: 45 percent, worn")
    );
    assert_eq!(
        driver_screen_fatigue(&mut harness),
        "Fatigue: 45 percent, worn"
    );

    at_fatigue(&mut harness, 64.0);
    assert_eq!(
        status_fatigue(&mut harness).as_deref(),
        Some("Fatigue: 64 percent, drowsy")
    );
    assert_eq!(
        driver_screen_fatigue(&mut harness),
        "Fatigue: 64 percent, drowsy"
    );

    at_fatigue(&mut harness, 90.0);
    assert_eq!(
        driver_screen_fatigue(&mut harness),
        "Fatigue: 90 percent, severe"
    );
}

fn resume(
    harness: &mut PlaytestHarness,
    edit: impl FnOnce(&mut serde_json::Value),
) -> DrivingState {
    let mut snapshot = harness.with_drive(|d, ctx| d.snapshot(ctx));
    edit(&mut snapshot);
    DrivingState::from_snapshot(&mut harness.app.ctx, &snapshot).expect("resumes")
}

#[test]
fn the_band_and_the_nod_survive_a_save_and_resume() {
    let mut harness = a_drive("Nod Snapshot");
    at_fatigue(&mut harness, 88.0);
    let saved = NodState {
        nod: Nod::Awake { since_gm: 4.5 },
        misses: 2,
    };
    harness.with_drive(move |d, _| d.nod = saved);
    let resumed = resume(&mut harness, |_| {});
    assert_eq!(resumed.alertness, Alertness::Severe);
    assert_eq!(resumed.nod, saved);
    assert!(!resumed.nod_staged);

    let quiet = NodState {
        nod: Nod::Recovering { left_gm: 1.25 },
        misses: 1,
    };
    harness.with_drive(move |d, _| d.nod = quiet);
    assert_eq!(resume(&mut harness, |_| {}).nod, quiet);
}

#[test]
fn a_trip_saved_mid_nod_opens_that_nod_again_on_resume() {
    let mut harness = a_drive("Nod Mid Save");
    at_fatigue(&mut harness, 90.0);
    harness.with_drive(|d, _| {
        d.nod = NodState {
            nod: Nod::Nodding { left_s: 0.4 },
            misses: 1,
        }
    });
    let resumed = resume(&mut harness, |_| {});
    assert!(!resumed.nod.demands_driver());
    assert!(resumed.nod_staged);
    assert_eq!(resumed.nod.misses, 1);
}

#[test]
fn an_old_snapshot_without_the_machines_loads_without_a_cue() {
    let mut harness = a_drive("Old Snapshot");
    at_fatigue(&mut harness, 65.0);
    let mut resumed = resume(&mut harness, |snapshot| {
        let data = snapshot.as_object_mut().expect("an object");
        data.remove("alertness");
        data.remove("nod");
    });
    assert_eq!(resumed.alertness, Alertness::Drowsy);
    assert_eq!(resumed.nod, NodState::default());
    harness.clear_speech();
    resumed.update_hours_and_fatigue(&mut harness.app.ctx, 0.1);
    let said = harness.app.event_lines();
    assert_eq!(count(&said, "Getting drowsy."), 0, "{said:?}");
}
