//! A stop inside a work zone can still be taken. Every exit is taken from the
//! right lane, so a right lane closure over a stop made it unreachable: a
//! player signaled for the only truck stop left before their break and drove
//! past it (problem report, 2026-11-16). The trip now puts such a closure on
//! the left side (`keep_exits_open`), and these drives take the exit through
//! it with the driver's own lane work and with lane keeping on full.

use ff_core::sim::trip::keep_exits_open;
use ff_core::sim::trip_models::{RoadStop, Zone};
use ff_core::speech_text::SpokenMessage;
use freight_fate::playtest::harness::{PlaytestHarness, StartDelivery};
use freight_fate::states::base::{Key, Mods};
use freight_fate::states::driving_events::ambient::Ambient;

use crate::transcript_cruise_support::{bench_road, MPS_PER_MPH};

const DT: f64 = 1.0 / 30.0;
const STOP_MI: f64 = 40.0;

/// A truck at 60 on cruise in the right lane, 3 miles short of a truck stop
/// that sits in the middle of a work zone drawn with its right lane closed,
/// on lane keeping `mode`. `placed` runs the trip's own exit check on the
/// zone, the way `place_zones` does.
fn rig(mode: &str, placed: bool) -> PlaytestHarness {
    let mut harness = PlaytestHarness::new();
    harness.start_delivery(StartDelivery::named("Exit In Work Zone"));
    harness.app.ctx.settings.lane_keeping = mode.to_string();
    harness.app.ctx.settings.automatic_transmission = true;
    harness.with_drive(|d, _| {
        d.departure_checked = true;
        bench_road(d, 65.0, 0.0, 1.0);
        d.truck_mut().set_air_ready(false);
    });
    harness.press_key(Key::E, None); // engine on
    harness.with_drive(move |d, _| {
        let mut stop = RoadStop::new("Prairie Travel Center", STOP_MI, "truck_stop");
        stop.actions = vec!["park".to_string(), "fuel".to_string()];
        stop.parking = "confirmed".to_string();
        stop.exit_label = "exit 42".to_string();
        d.trip.stops = vec![stop];
        d.trip.zones = vec![
            Zone::new(STOP_MI - 2.0, STOP_MI - 1.0, 55.0, "construction merge")
                .with_closed_side(Some("right")),
            Zone::new(STOP_MI - 1.0, STOP_MI + 3.0, 45.0, "construction")
                .with_closed_side(Some("right")),
        ];
        if placed {
            let windows = d.trip.exit_windows();
            keep_exits_open(&mut d.trip.zones, &windows);
        }
        d.truck_mut().transmission.gear = 10;
        d.truck_mut().velocity_mps = 60.0 * MPS_PER_MPH;
        d.trip.position_mi = STOP_MI - 3.0;
        d.lane.recentre(0);
    });
    harness.press_key(Key::K, None); // cruise at 60
    frame(&mut harness);
    harness.clear_speech();
    harness
}

fn frame(harness: &mut PlaytestHarness) {
    harness.advance_clock(DT);
    harness.with_drive(|d, ctx| d.update_frame(ctx, DT));
}

fn hold_right(harness: &mut PlaytestHarness, held: bool) {
    if held {
        harness.app.ctx.input.press(Key::Right, Mods::NONE);
    } else {
        harness.app.ctx.input.release(Key::Right, Mods::NONE);
    }
}

fn heard(harness: &PlaytestHarness) -> Vec<String> {
    harness.app.speech().lines()
}

fn heard_any(harness: &PlaytestHarness, needle: &str) -> bool {
    heard(harness).iter().any(|line| line.contains(needle))
}

/// Frames until `until` holds or the truck is past the stop's exit; whether
/// `until` held.
fn drive_until(harness: &mut PlaytestHarness, until: impl Fn(&PlaytestHarness) -> bool) -> bool {
    for _ in 0..(30 * 60 * 5) {
        if until(harness) {
            return true;
        }
        if harness.read_drive(|d| d.trip.position_mi > STOP_MI + 1.0) {
            return false;
        }
        frame(harness);
    }
    false
}

fn took_the_exit(harness: &PlaytestHarness) -> bool {
    harness.read_drive(|d| d.ramp_mi.is_some())
}

#[test]
fn the_zone_over_the_stop_closes_the_left_lane() {
    let harness = rig("partial", true);
    let sides: Vec<Option<String>> =
        harness.read_drive(|d| d.trip.zones.iter().map(|z| z.closed_side.clone()).collect());
    assert_eq!(sides, vec![Some("left".to_string()); 2]);
}

#[test]
fn own_lane_work_takes_a_stop_inside_a_work_zone() {
    let mut harness = rig("partial", true);
    harness.press_key(Key::X, None);
    assert!(heard_any(&harness, "Signal set"), "{:?}", heard(&harness));
    assert!(
        drive_until(&mut harness, |h| h.read_drive(|d| d.lane.exit_lane_open)),
        "the exit lane never opened\n{}",
        harness.transcript_text()
    );
    // The truck stayed in the right lane through the taper and the zone.
    assert_eq!(harness.read_drive(|d| d.lane.lane), 0);
    hold_right(&mut harness, true);
    let took = drive_until(&mut harness, took_the_exit);
    hold_right(&mut harness, false);
    assert!(took, "missed the exit\n{}", harness.transcript_text());
    assert!(heard_any(&harness, "Exit 42 for"), "{:?}", heard(&harness));
    assert!(
        !heard_any(&harness, "right lane is closed"),
        "{:?}",
        heard(&harness)
    );
    assert!(!heard_any(&harness, "issed"), "{:?}", heard(&harness));
}

#[test]
fn full_lane_keeping_takes_a_stop_inside_a_work_zone() {
    let mut harness = rig("full", true);
    harness.press_key(Key::X, None);
    assert!(heard_any(&harness, "Signal set"), "{:?}", heard(&harness));
    assert!(
        drive_until(&mut harness, took_the_exit),
        "missed the exit\n{}",
        harness.transcript_text()
    );
    assert!(!heard_any(&harness, "issed"), "{:?}", heard(&harness));
}

#[test]
fn a_right_closure_over_the_stop_was_the_miss() {
    // The report as it happened: the same zone left on the right, and the
    // exit cannot be taken. This is what `keep_exits_open` prevents.
    let mut harness = rig("full", false);
    harness.press_key(Key::X, None);
    assert!(
        !drive_until(&mut harness, took_the_exit),
        "took the exit through a closed right lane\n{}",
        harness.transcript_text()
    );
}

#[test]
fn a_stop_notice_behind_chatter_is_still_heard() {
    // Agent drive, I-71 at Seville (2026-11-16): the 5-mile notice for the
    // truck stop inside the zone came due while a billboard was being read.
    // It waited out the spacing between roadside lines, then went to the
    // voice as chatter and was dropped as stale behind the billboard. It is
    // the only call that names a stop the driver may need.
    let mut harness = rig("full", true);
    harness.with_drive(|d, _| {
        d.trip.position_mi = STOP_MI - 6.0;
        d.trip.announced_stops.clear();
    });
    assert!(drive_until(&mut harness, |h| h
        .read_drive(|d| d.trip.position_mi > STOP_MI - 5.1)));
    harness.clear_speech();
    harness.with_drive(|d, ctx| {
        d.speak_ambient_event(
            ctx,
            SpokenMessage::new(
                "Billboard: a long one, read out word by word while the truck rolls \
                 on toward the work zone and the truck stop past the taper, long \
                 enough that anything queued behind it waits well past three seconds.",
            ),
            Ambient::new(),
        );
    });
    assert!(drive_until(&mut harness, |h| h
        .read_drive(|d| d.trip.position_mi > STOP_MI - 4.0)));
    let lines = heard(&harness);
    assert!(lines[0].starts_with("Billboard"), "{lines:?}");
    let notice = "Prairie Travel Center at exit 42 in 5 miles";
    let said = lines.iter().filter(|line| line.contains(notice)).count();
    // Once, though the pacer may hand the line back after a cut.
    assert_eq!(
        said.saturating_sub(harness.app.ctx.handed_back_count(notice)),
        1,
        "{lines:?}"
    );
}
