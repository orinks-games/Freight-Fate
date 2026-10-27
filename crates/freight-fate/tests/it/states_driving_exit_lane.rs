//! The exit lane is the deceleration lane (owner's drive, I-70 East into
//! Denver West, 2026-09-24). On the approach a driver moves to the rightmost
//! travel lane and stays centred there; the exit lane opens beside it at the
//! taper, just ahead of the gore, and that is the one place the cab asks for a
//! steer into it. The old model was a lateral offset held inside the right
//! lane for miles: the owner, already in the right lane, was told to steer
//! right again and again, pushed toward the shoulder, overcorrected into the
//! left lane, and came back onto the semi he had been following.
//!
//! Every case runs on a bench road with one invented truck stop, driven frame
//! by frame through the real lane model with partial lane keeping.

use ff_core::sim::traffic_manager::TrafficVehicle;
use ff_core::sim::trip_models::RoadStop;
use freight_fate::playtest::harness::{PlaytestHarness, StartDelivery};
use freight_fate::states::base::{Key, Mods};
use freight_fate::states::driving_core::{EXIT_COMMIT_WINDOW_MI, EXIT_TAPER_MI};

use crate::transcript_cruise_support::{bench_road, MPS_PER_MPH};

const DT: f64 = 1.0 / 30.0;
const STOP_MI: f64 = 40.0;

/// A truck at 60 on cruise in lane `lane`, `ahead_mi` short of a truck stop
/// on a bench road posted 65, with lane keeping `mode`. Not yet signalled.
fn rig(mode: &str, lane: i64, ahead_mi: f64) -> (PlaytestHarness, RoadStop) {
    rig_scaled(mode, lane, ahead_mi, 1.0)
}

/// [`rig`] on a compressed clock, which is what widens the window X can arm
/// an exit in to eight miles and more.
fn rig_scaled(
    mode: &str,
    lane: i64,
    ahead_mi: f64,
    time_scale: f64,
) -> (PlaytestHarness, RoadStop) {
    let mut harness = PlaytestHarness::new();
    harness.start_delivery(StartDelivery::named("Exit Lane"));
    harness.app.ctx.settings.lane_keeping = mode.to_string();
    harness.app.ctx.settings.automatic_transmission = true;
    harness.app.ctx.settings.time_scale = time_scale;
    harness.with_drive(move |d, _| {
        d.departure_checked = true;
        bench_road(d, 65.0, 0.0, time_scale);
        d.truck_mut().set_air_ready(false);
    });
    harness.press_key(Key::E, None); // engine on
    let mut stop = RoadStop::new("Prairie Travel Center", STOP_MI, "truck_stop");
    stop.actions = vec!["park".to_string(), "fuel".to_string()];
    stop.parking = "confirmed".to_string();
    stop.exit_label = "exit 42".to_string();
    let staged = stop.clone();
    harness.with_drive(move |d, _| {
        d.trip.stops = vec![staged.clone()];
        d.truck_mut().transmission.gear = 10;
        d.truck_mut().velocity_mps = 60.0 * MPS_PER_MPH;
        d.trip.position_mi = staged.at_mi - ahead_mi;
        d.lane.recentre(lane);
    });
    harness.press_key(Key::K, None); // cruise at 60
    frame(&mut harness);
    harness.clear_speech();
    (harness, stop)
}

fn frame(harness: &mut PlaytestHarness) {
    harness.advance_clock(DT);
    harness.with_drive(|d, ctx| d.update_frame(ctx, DT));
}

fn steer(harness: &mut PlaytestHarness, key: Option<Key>) {
    for k in [Key::Left, Key::Right] {
        if Some(k) == key {
            harness.app.ctx.input.press(k, Mods::NONE);
        } else {
            harness.app.ctx.input.release(k, Mods::NONE);
        }
    }
}

fn heard(harness: &PlaytestHarness) -> Vec<String> {
    harness.app.speech().lines()
}

fn count(harness: &PlaytestHarness, needle: &str) -> usize {
    heard(harness)
        .iter()
        .filter(|line| line.contains(needle))
        .count()
}

/// Every lane instruction the cab gave, word for word.
fn lane_instructions(harness: &PlaytestHarness) -> Vec<String> {
    heard(harness)
        .into_iter()
        .filter(|line| {
            line.contains("right lane") || line.contains("exit lane") || line.contains("Steer")
        })
        .collect()
}

/// Frames until `until` holds, or panic with the transcript.
fn drive_until(
    harness: &mut PlaytestHarness,
    what: &str,
    until: impl Fn(&PlaytestHarness) -> bool,
) {
    for _ in 0..(30 * 60 * 5) {
        if until(harness) {
            return;
        }
        frame(harness);
    }
    panic!("never {what}\n{}", harness.transcript_text());
}

/// The L key's neighbour clause: everything after "In the right lane, ...".
fn neighbour_readout(harness: &mut PlaytestHarness) -> String {
    let text = harness.with_drive(|d, _| d.lane_status_text());
    text.split_once(". ")
        .map(|(_, rest)| rest.to_string())
        .unwrap_or_default()
}

#[test]
fn already_in_the_right_lane_hears_one_steer_at_the_taper_and_takes_the_exit() {
    let (mut harness, stop) = rig("partial", 0, 2.3);
    harness.press_key(Key::X, None);
    assert!(heard(&harness).iter().any(|l| l.contains("Signal set")));

    // The whole approach, the two-mile, one-mile and half-mile anchors
    // included: no lane instruction, because the truck is where it belongs.
    drive_until(&mut harness, "reached the taper", |h| {
        h.read_drive(|d| d.lane.exit_lane_open)
    });
    let position = harness.read_drive(|d| d.trip.position_mi);
    assert!(
        position >= stop.at_mi - EXIT_TAPER_MI - 0.01,
        "the exit lane opened {:.2} mi early",
        stop.at_mi - position
    );
    assert_eq!(
        count(&harness, "in half a mile"),
        1,
        "{:?}",
        heard(&harness)
    );
    assert_eq!(
        lane_instructions(&harness),
        vec!["Exit lane opening. Steer right into it.".to_string()],
        "{:?}",
        heard(&harness)
    );

    // Right into it, and the truck is on the ramp: clean, with no shoulder.
    steer(&mut harness, Some(Key::Right));
    drive_until(&mut harness, "took the exit", |h| {
        h.read_drive(|d| d.ramp_mi.is_some())
    });
    steer(&mut harness, None);
    let position = harness.read_drive(|d| d.trip.position_mi);
    assert!(position <= stop.at_mi + EXIT_COMMIT_WINDOW_MI);
    assert!(heard(&harness).iter().any(|l| l.contains("Exit 42 for")));
    assert_eq!(count(&harness, "Exit lane opening"), 1);
    assert_eq!(count(&harness, "sideswiped"), 0, "{:?}", heard(&harness));
    assert_eq!(count(&harness, "Off the road"), 0, "{:?}", heard(&harness));
    assert_eq!(count(&harness, "missed"), 0, "{:?}", heard(&harness));
}

#[test]
fn out_of_the_right_lane_is_asked_to_move_right_until_it_is_there() {
    let (mut harness, _stop) = rig("partial", 1, 2.3);
    harness.press_key(Key::X, None);
    let arming = heard(&harness)
        .into_iter()
        .find(|l| l.contains("Signal set"))
        .expect("the signal line");
    assert!(arming.contains("Move to the right lane."), "{arming}");

    // The two-mile anchor still owes the move.
    drive_until(&mut harness, "heard the two-mile anchor", |h| {
        count(h, "in 2 miles") > 0
    });
    let two_mile = heard(&harness)
        .into_iter()
        .find(|l| l.contains("in 2 miles"))
        .unwrap_or_default();
    assert!(two_mile.contains("Move to the right lane."), "{two_mile}");

    // Across into the right lane, then centred there.
    steer(&mut harness, Some(Key::Right));
    drive_until(&mut harness, "reached the right lane", |h| {
        h.read_drive(|d| d.lane.lane == 0)
    });
    steer(&mut harness, None);
    harness.clear_speech();

    // From here the anchors name the exit and nothing else, until the taper.
    drive_until(&mut harness, "reached the taper", |h| {
        h.read_drive(|d| d.lane.exit_lane_open)
    });
    assert!(count(&harness, "in 1 mile") > 0, "{:?}", heard(&harness));
    assert_eq!(
        lane_instructions(&harness),
        vec!["Exit lane opening. Steer right into it.".to_string()],
        "{:?}",
        heard(&harness)
    );
}

#[test]
fn full_lane_keeping_moves_right_for_the_exit_itself() {
    // Owner's drive, I-94 into Chicago, 2026-10-01: lane keeping on full,
    // pulled out to the middle lane around truck traffic three miles out,
    // and lane keeping held the middle lane through the gore -- twice, each
    // loop-back promising "lane keeping will take it". The exit lane opens
    // only beside the right lane, so taking the exit means getting there.
    let (mut harness, _stop) = rig("full", 1, 2.3);
    harness.press_key(Key::X, None);
    let arming = heard(&harness)
        .into_iter()
        .find(|l| l.contains("Signal set"))
        .expect("the signal line");
    // Nothing to tap: lane keeping makes the move.
    assert!(!arming.contains("right lane"), "{arming}");

    drive_until(&mut harness, "took the exit", |h| {
        h.read_drive(|d| d.ramp_mi.is_some())
    });
    // Once, though the pacer may hand the line back after a cut.
    let phrase = "Changing to the right lane for the exit.";
    assert_eq!(
        count(&harness, phrase).saturating_sub(harness.app.ctx.handed_back_count(phrase)),
        1,
        "{:?}",
        heard(&harness)
    );
    assert!(heard(&harness).iter().any(|l| l.contains("Exit 42 for")));
    assert_eq!(count(&harness, "missed"), 0, "{:?}", heard(&harness));
    assert_eq!(count(&harness, "sideswiped"), 0, "{:?}", heard(&harness));
}

#[test]
fn full_lane_keeping_waits_for_the_right_lane_to_open() {
    let (mut harness, stop) = rig("full", 1, 2.3);
    harness.press_key(Key::X, None);
    // A semi riding beside the cab in the right lane, at the truck's speed.
    let pace = |d: &mut freight_fate::states::driving::DrivingState| {
        let at = d.trip.position_mi;
        let mph = d.truck().speed_mph();
        d.trip.set_npc_vehicles(vec![TrafficVehicle::new(
            "npc:beside",
            at,
            mph,
            mph,
            0,
            "cruising",
            "semi",
        )
        .with_lane(0)]);
    };
    // Past the two-mile mark with the lane held: lane keeping stays put.
    while harness.read_drive(|d| d.trip.position_mi < stop.at_mi - 1.5) {
        harness.with_drive(|d, _| pace(d));
        frame(&mut harness);
        assert!(
            harness.read_drive(|d| d.lane.lane == 1 && d.lane_change_target.is_none()),
            "moved into a held lane\n{}",
            harness.transcript_text()
        );
    }

    // The semi drops back out of sight: in it goes, and the exit is taken.
    harness.with_drive(|d, _| d.trip.set_npc_vehicles(Vec::new()));
    drive_until(&mut harness, "took the exit", |h| {
        h.read_drive(|d| d.ramp_mi.is_some())
    });
    assert_eq!(count(&harness, "sideswiped"), 0, "{:?}", heard(&harness));
}

#[test]
fn right_lane_traffic_is_never_in_the_exit_lane() {
    // The mirror check calls any vehicle within a third of a mile of a lane
    // change a sideswipe. The exit lane is an auxiliary lane with no through
    // traffic, so a car close behind in the right lane is not in it.
    let (mut harness, _stop) = rig("partial", 0, 0.3);
    harness.press_key(Key::X, None);
    harness.with_drive(|d, _| {
        let at = d.trip.position_mi - 0.04;
        let mph = d.truck().speed_mph();
        d.trip.set_npc_vehicles(vec![TrafficVehicle::new(
            "npc:behind",
            at,
            mph,
            mph,
            0,
            "cruising",
            "semi",
        )
        .with_lane(0)]);
    });
    drive_until(&mut harness, "reached the taper", |h| {
        h.read_drive(|d| d.lane.exit_lane_open)
    });
    steer(&mut harness, Some(Key::Right));
    drive_until(&mut harness, "took the exit", |h| {
        let (lanes, count) = h.read_drive(|d| {
            (
                d.trip
                    .traffic_manager
                    .vehicles
                    .iter()
                    .map(|v| v.lane)
                    .collect::<Vec<_>>(),
                d.lane.lane_count,
            )
        });
        assert!(
            lanes.iter().all(|lane| (0..count).contains(lane)),
            "traffic outside the travel lanes: {lanes:?} of {count}"
        );
        h.read_drive(|d| d.ramp_mi.is_some())
    });
    steer(&mut harness, None);
    assert_eq!(count(&harness, "sideswiped"), 0, "{:?}", heard(&harness));
}

#[test]
fn never_steering_in_misses_the_exit_at_the_end_of_the_gore_window() {
    let (mut harness, stop) = rig("partial", 0, 0.3);
    harness.press_key(Key::X, None);
    drive_until(&mut harness, "settled the exit", |h| {
        h.read_drive(|d| d.exit_stop.is_none())
    });
    let position = harness.read_drive(|d| d.trip.position_mi);
    assert!(position >= stop.at_mi + EXIT_COMMIT_WINDOW_MI, "{position}");
    assert!(harness.read_drive(|d| d.ramp_mi.is_none()));
    assert_eq!(count(&harness, "not in the exit lane"), 1);
}

#[test]
fn the_lane_readout_holds_steady_where_the_lane_count_does() {
    // The owner heard "Middle lane open" and "Left lane open" by turns while
    // the truck sat in the right lane. The log settled it: a real lane drop,
    // said once, and older lines replayed from the message history. Pinned
    // here so the exit lane opening beside the truck never renumbers the
    // travel lanes either.
    let (mut harness, _stop) = rig("partial", 0, 1.2);
    harness.press_key(Key::X, None);
    let first = neighbour_readout(&mut harness);
    assert!(first.ends_with("lane open."), "{first}");
    let mut exit_lane_seen = false;
    for step in 0..(30 * 90) {
        if harness.read_drive(|d| d.ramp_mi.is_some()) {
            break;
        }
        let open = harness.read_drive(|d| d.lane.exit_lane_open);
        exit_lane_seen |= open;
        steer(&mut harness, open.then_some(Key::Right));
        if step % 15 == 0 {
            assert_eq!(neighbour_readout(&mut harness), first);
        }
        frame(&mut harness);
    }
    assert!(exit_lane_seen);
}

// -- the blinker (owner ruling, 2026-09-24) ---------------------------------------------

fn blinker_clicks(log: &freight_fate::app::testing::AudioLog) -> usize {
    log.borrow()
        .played
        .iter()
        .filter(|(key, ..)| key == "vehicle/turn_signal")
        .count()
}

/// Armed eight and a half miles out, the blinker stays quiet until half a
/// mile, clicks from there, and the exit is still taken: X is the commitment,
/// the blinker only its sound. Agent drives blinked 7.3 miles to the gore.
fn armed_far_out_blinks_from_half_a_mile(mode: &str) {
    let (mut harness, stop) = rig_scaled(mode, 0, 8.5, 24.0);
    assert!(harness.read_drive(|d| d.exit_window_mi()) > 8.5);
    let log = harness.app.record_audio();
    harness.press_key(Key::X, None);
    let arming = heard(&harness)
        .into_iter()
        .find(|l| l.contains("Signal"))
        .expect("the signal line");
    assert!(arming.starts_with("Signal set for exit 42"), "{arming}");
    assert!(harness.read_drive(|d| d.exit_signal_on));

    for _ in 0..(30 * 5) {
        frame(&mut harness);
    }
    assert_eq!(blinker_clicks(&log), 0, "{mode}: a blinker miles out");

    // The miles between, then the last of them driven.
    let at = stop.at_mi;
    harness.with_drive(move |d, _| d.trip.position_mi = at - 0.6);
    // Every click lands at half a mile or nearer (one frame of road slack).
    for _ in 0..(30 * 60) {
        let before = blinker_clicks(&log);
        frame(&mut harness);
        let ahead = at - harness.read_drive(|d| d.trip.position_mi);
        if blinker_clicks(&log) > before {
            assert!(ahead <= 0.501, "{mode}: clicked {ahead:.3} mi out");
        }
        if ahead <= 0.45 {
            break;
        }
    }
    for _ in 0..(30 * 3) {
        frame(&mut harness);
    }
    assert!(
        blinker_clicks(&log) > 0,
        "{mode}: no blinker at half a mile"
    );

    // Right into the exit lane where it opens (lane keeping on full takes
    // it itself), and the exit is taken.
    for _ in 0..(30 * 60) {
        if harness.read_drive(|d| d.ramp_mi.is_some()) {
            break;
        }
        let open = harness.read_drive(|d| d.lane.exit_lane_open);
        steer(&mut harness, open.then_some(Key::Right));
        frame(&mut harness);
    }
    steer(&mut harness, None);
    assert!(
        harness.read_drive(|d| d.ramp_mi.is_some()),
        "{mode}: never took the exit\n{}",
        harness.transcript_text()
    );
}

#[test]
fn the_blinker_starts_at_half_a_mile_on_partial_lane_keeping() {
    armed_far_out_blinks_from_half_a_mile("partial");
}

#[test]
fn the_blinker_starts_at_half_a_mile_on_full_lane_keeping() {
    armed_far_out_blinks_from_half_a_mile("full");
}
