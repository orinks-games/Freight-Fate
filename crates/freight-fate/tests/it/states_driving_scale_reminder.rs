//! The open scale's last reminder at compressed time, who made a late one
//! late, the re-announcement after a check-in or a pause, the closed-scale
//! check-in, and the stop screen under a held key.
//!
//! One tester drive (log, 2026-10-07) is the shape of every case here: an
//! open scale announced twelve miles out, a closed scale six miles short of
//! it that the driver checked in at anyway and was told "Officers wave you
//! straight back onto the highway", a nine-minute pause, then "Weigh station
//! in half a mile" three real seconds before the bypass lights at ten times
//! real speed -- and the stop screen reading its only row twenty times while
//! the brake key was still held.

use ff_core::models::jobs::{Job, CARGO_CATALOG};
use ff_core::models::profile::Profile;
use ff_core::sim::enforcement_posts::{EnforcementPost, KIND_FIXED_SCALE, METHOD_SCALE_SCREEN};
use ff_core::sim::trip_models::{RoadStop, LOW_SPEED_TIME_SCALE, SCALE_BYPASS_MPH};
use ff_core::sim::weather::WeatherKind;

use freight_fate::app::testing::TestApp;
use freight_fate::playtest::breaker::{Rig, RigOptions, DT};
use freight_fate::states::base::{InputEvent, Key, Mods};
use freight_fate::states::driving::DrivingState;
use freight_fate::states::driving_core::DRIVE_PHASE_DELIVERY;
use freight_fate::states::driving_enforcement::{
    SCALE_REMINDER_REAL_LEAD_S, WEIGH_STATION_REMINDER_MAX_MI, WEIGH_STATION_REMINDER_MI,
};
use freight_fate::states::driving_menu_states::DriveRef;
use freight_fate::states::driving_pause_states::PauseMenuState;
use freight_fate::states::driving_rest_states::{EnforcementStopState, RestStopState};
use freight_fate::states::driving_updates::pacing::CLOCK_OVERRIDE_LOG_HOLD_S;

const MPS_PER_MPH: f64 = 1.0 / 2.23694;
const CLOSED_MI: f64 = 4.0;
const OPEN_MI: f64 = 10.0;
const CLOSED_NAME: &str = "I-55 Weigh Station";
const OPEN_NAME: &str = "I-40 Weigh Station";

fn mph_to_mps(mph: f64) -> f64 {
    mph * MPS_PER_MPH
}

fn a_scale(name: &str, at_mi: f64) -> RoadStop {
    let mut stop = RoadStop::new(name, at_mi, "weigh_station");
    stop.actions = vec!["inspect".to_string()];
    stop.parking = "none".to_string();
    stop
}

fn open_post(stop: &RoadStop) -> EnforcementPost {
    let mut post = EnforcementPost::new(stop.at_mi, KIND_FIXED_SCALE);
    post.method = METHOD_SCALE_SCREEN.to_string();
    post.reach_mi = 1.0;
    post.staffed = true;
    post.anchor = stop.key();
    post
}

/// The tester's road: a closed scale, then the open one six miles on.
fn closed_then_open(drive: &mut DrivingState) -> (RoadStop, RoadStop) {
    let closed = a_scale(CLOSED_NAME, CLOSED_MI);
    let open = a_scale(OPEN_NAME, OPEN_MI);
    drive.trip.stops = vec![closed.clone(), open.clone()];
    drive.trip.posts = vec![open_post(&open)];
    (closed, open)
}

/// A rig at ten times real speed. Seed 1 is a trip whose bypass roll is
/// caught at the open scale, so a judged crossing is a heard one.
fn ten_times_rig() -> Rig {
    let mut rig = Rig::new(RigOptions {
        seed: 1,
        ..RigOptions::default()
    });
    rig.app.ctx.settings.time_scale = 10.0;
    rig.drive.trip.time_scale = 10.0;
    rig
}

/// One frame at a pinned 61 mph: the driver ignores every instruction.
fn hold_61_for(rig: &mut Rig, frames: usize, until: impl Fn(&Rig) -> bool) -> usize {
    for frame in 0..frames {
        rig.drive.trip.truck.velocity_mps = mph_to_mps(61.0);
        rig.step(1, DT, None);
        if until(rig) {
            return frame + 1;
        }
    }
    frames
}

fn first_index(rig: &Rig, needle: &str) -> Option<usize> {
    rig.transcript()
        .iter()
        .position(|line| line.contains(needle))
}

/// Open the pause menu over the rig's drive and choose "Resume driving".
fn pause_and_resume(rig: &mut Rig) {
    rig.with_drive_on_stack(|rig, drive: DriveRef| {
        let state = PauseMenuState::with_drive(drive);
        rig.app.ctx.push_state(state);
        rig.app.ctx.run_deferred();
        assert!(
            rig.select_menu_containing("Resume driving"),
            "{:?}",
            rig.menu_labels()
        );
    });
}

// -- (a) the reminder's real seconds ------------------------------------------------

#[test]
fn an_open_scale_behind_a_closed_one_at_ten_times_gets_real_seconds_after_a_pause() {
    let mut rig = ten_times_rig();
    let (closed, open) = closed_then_open(&mut rig.drive);
    rig.prepare(61.0, None);

    // Twelve miles is inside the capped lookahead, so the notice for the
    // open scale lands first; the closed one between is crossed in silence
    // and never judged.
    hold_61_for(&mut rig, 20_000, |rig| {
        rig.drive.trip.position_mi > CLOSED_MI + 3.5
    });
    assert_eq!(
        rig.said("Open weigh station ahead"),
        1,
        "{:?}",
        rig.transcript()
    );
    let closed_key = rig.drive.weigh_station_key(&closed);
    assert!(!rig.drive.enforcement_events.contains(&closed_key));
    assert!(rig.drive.pull_over.is_none());
    assert!(
        rig.drive.trip.effective_time_scale() > 5.0,
        "the approach is meant to run compressed: {}",
        rig.drive.trip.effective_time_scale()
    );

    // The tester's pause, about two and a half miles short of the scale.
    pause_and_resume(&mut rig);
    hold_61_for(&mut rig, 3, |_| false);
    let reannounce = format!("{OPEN_NAME} is still ahead, open, ");
    assert_eq!(rig.said(&reannounce), 1, "{:?}", rig.transcript());

    // Drive on with no signal, counting real frames from the reminder.
    let mut reminder_frame: Option<usize> = None;
    let mut judged_frame: Option<usize> = None;
    let mut crossed_frame: Option<usize> = None;
    for frame in 0..40_000 {
        rig.drive.trip.truck.velocity_mps = mph_to_mps(61.0);
        let before = rig.drive.trip.position_mi;
        rig.step(1, DT, None);
        if reminder_frame.is_none() && rig.said("Signal for the scale exit.") > 0 {
            reminder_frame = Some(frame);
            assert!(
                rig.drive.trip.effective_time_scale() <= 1.0,
                "the reminder must put the clock on real time"
            );
        }
        if crossed_frame.is_none()
            && before < open.at_mi
            && rig.drive.trip.position_mi >= open.at_mi
        {
            crossed_frame = Some(frame);
        }
        if judged_frame.is_none() && rig.said("Scale bypass enforcement") > 0 {
            judged_frame = Some(frame);
            break;
        }
        if rig.drive.trip.position_mi > open.at_mi + 0.5 {
            break;
        }
    }
    let reminder = reminder_frame.unwrap_or_else(|| panic!("no reminder: {:?}", rig.transcript()));
    let crossed = crossed_frame.expect("the truck reached the scale");
    let judged = judged_frame.unwrap_or_else(|| {
        panic!(
            "a reminded, ignored open scale must still be judged: {:?}",
            rig.transcript()
        )
    });
    let lead_s = (crossed - reminder) as f64 * DT;
    assert!(
        lead_s >= SCALE_REMINDER_REAL_LEAD_S,
        "only {lead_s:.1} real seconds from the reminder to the gore"
    );
    assert!((judged - reminder) as f64 * DT >= SCALE_REMINDER_REAL_LEAD_S);
    assert!(first_index(&rig, &reannounce) < first_index(&rig, "Signal for the scale exit."));
}

#[test]
fn a_crossing_is_judged_only_after_the_driver_was_told_in_time() {
    // A scale never announced, or a reminder the game held back until late:
    // the driver never had the seconds, so the scale does not charge. One
    // announced whose reminder the driver's own pace skipped or delayed
    // still does.
    for (noticed, reminded, held_by_game, age_s, charged) in [
        (false, false, false, 0.0, false),
        (true, false, false, 0.0, true),
        (true, true, true, SCALE_REMINDER_REAL_LEAD_S - 1.0, false),
        (true, true, false, SCALE_REMINDER_REAL_LEAD_S - 1.0, true),
        (true, true, true, SCALE_REMINDER_REAL_LEAD_S, true),
    ] {
        let mut rig = ten_times_rig();
        let (_closed, open) = closed_then_open(&mut rig.drive);
        let key = rig.drive.weigh_station_key(&open);
        if noticed {
            rig.drive.weigh_station_noticed.insert(key.clone());
        }
        if reminded {
            rig.drive.weigh_station_reminder_key = key.clone();
        }
        if held_by_game {
            rig.drive.scale_reminder_held_by_game.insert(key.clone());
        }
        rig.drive.weigh_station_reminder_age_s = age_s;
        rig.drive.trip.position_mi = open.at_mi + 0.05;
        rig.drive.trip.truck.velocity_mps = mph_to_mps(61.0);

        rig.drive
            .check_weigh_station_enforcement(&mut rig.app.ctx, open.at_mi - 0.05);

        assert_eq!(
            rig.drive.pull_over.is_some(),
            charged,
            "noticed {noticed}, reminded {reminded}, held by the game {held_by_game}, \
             {age_s} s ago"
        );
        assert!(rig.drive.enforcement_events.contains(&key));
    }
}

#[test]
fn a_ramp_missed_after_a_late_reminder_is_not_judged_but_one_armed_early_is() {
    // Signalled for the scale, then missed its ramp. Armed in answer to a
    // reminder three real seconds old that the game held back, the driver
    // never had the time; armed before any reminder was needed, or after one
    // made late by their own crawl, the miss is theirs.
    for (reminded, held_by_game, charged) in [
        (true, true, false),
        (true, false, true),
        (false, false, true),
    ] {
        let mut rig = ten_times_rig();
        let (_closed, open) = closed_then_open(&mut rig.drive);
        let key = rig.drive.weigh_station_key(&open);
        rig.drive.weigh_station_noticed.insert(key.clone());
        if reminded {
            rig.drive.weigh_station_reminder_key = key.clone();
            rig.drive.weigh_station_reminder_age_s = 3.0;
        }
        if held_by_game {
            rig.drive.scale_reminder_held_by_game.insert(key.clone());
        }
        rig.drive.exit_stop = Some(open.clone());
        rig.drive.exit_signal_on = true;
        rig.drive.trip.position_mi = open.at_mi + 0.05;
        rig.drive.trip.truck.velocity_mps = mph_to_mps(61.0);

        rig.drive
            .check_weigh_station_enforcement(&mut rig.app.ctx, open.at_mi - 0.05);
        assert!(rig.drive.weigh_station_pending.is_some());
        rig.drive.exit_stop = None; // the exit watch called it a miss
        rig.drive.resolve_weigh_station_bypass(&mut rig.app.ctx);

        assert_eq!(
            rig.drive.pull_over.is_some(),
            charged,
            "reminded {reminded}, held by the game {held_by_game}"
        );
        assert!(rig.drive.enforcement_events.contains(&key));
    }
}

/// Drive at a pinned speed until `until` holds or the truck is past `stop_mi`.
fn hold_for(rig: &mut Rig, mph: f64, stop_mi: f64, until: impl Fn(&Rig) -> bool) {
    for _ in 0..200_000 {
        rig.drive.trip.truck.velocity_mps = mph_to_mps(mph);
        rig.step(1, DT, None);
        if until(rig) || rig.drive.trip.position_mi > stop_mi || rig.drive.pull_over.is_some() {
            return;
        }
    }
}

#[test]
fn crawling_up_to_the_scale_then_crossing_at_speed_is_still_judged() {
    // QA at ten times (2026-10-07): fourteen miles an hour, under the bypass
    // speed so the reminder stayed quiet, to 0.06 of a mile out; then across
    // at forty-five. The reminder spoke about seven real seconds before the
    // gore and the crossing was excused for good. The notice had said pull
    // in, and the half-mile window was thirty real seconds at any legal
    // speed: the shortfall was the driver's own crawl.
    let mut rig = ten_times_rig();
    let (_closed, open) = closed_then_open(&mut rig.drive);
    let key = rig.drive.weigh_station_key(&open);
    rig.prepare(61.0, None);
    rig.drive.trip.position_mi = open.at_mi - 3.0;
    rig.drive.enforcement_prev_mi = open.at_mi - 3.0;

    hold_for(&mut rig, 61.0, open.at_mi, |rig| {
        rig.drive.trip.position_mi >= open.at_mi - 0.9
    });
    assert_eq!(
        rig.said("Open weigh station ahead"),
        1,
        "{:?}",
        rig.transcript()
    );
    let reminder = "Signal for the scale exit.";
    assert_eq!(rig.said(reminder), 0, "{:?}", rig.transcript());

    hold_for(&mut rig, 14.0, open.at_mi, |rig| {
        rig.drive.trip.position_mi >= open.at_mi - 0.06
    });
    assert_eq!(
        rig.said(reminder),
        0,
        "under the bypass speed the reminder stays quiet: {:?}",
        rig.transcript()
    );
    assert!(rig.drive.trip.position_mi < open.at_mi);

    let mut reminded_at: Option<usize> = None;
    let mut crossed_at: Option<usize> = None;
    for frame in 0..20_000 {
        rig.drive.trip.truck.velocity_mps = mph_to_mps(45.0);
        let before = rig.drive.trip.position_mi;
        rig.step(1, DT, None);
        if reminded_at.is_none() && rig.said(reminder) > 0 {
            reminded_at = Some(frame);
        }
        if crossed_at.is_none() && before < open.at_mi && rig.drive.trip.position_mi >= open.at_mi {
            crossed_at = Some(frame);
        }
        if rig.drive.pull_over.is_some() || rig.drive.trip.position_mi > open.at_mi + 0.3 {
            break;
        }
    }
    let reminded =
        reminded_at.unwrap_or_else(|| panic!("no late reminder: {:?}", rig.transcript()));
    let crossed = crossed_at.expect("the truck crossed the scale");
    assert!(
        ((crossed - reminded) as f64 * DT) < SCALE_REMINDER_REAL_LEAD_S,
        "the case under test is a reminder too late to act on"
    );
    assert!(!rig.drive.scale_reminder_held_by_game.contains(&key));
    assert!(
        rig.drive.pull_over.is_some(),
        "a crawl-then-speed crossing must be judged: {:?}",
        rig.transcript()
    );
    assert_eq!(
        rig.said("Scale bypass enforcement"),
        1,
        "{:?}",
        rig.transcript()
    );
}

#[test]
fn a_hazard_after_the_reminder_excuses_the_crossing_unless_the_driver_crawled_first() {
    // QA (2026-10-07): the reminder spoke half a mile out, a hazard held the
    // cab from 0.48 to 0.15 of a mile, and the crossing was charged -- the
    // hazard ate most of the real seconds the reminder promised. A reminder
    // the driver's own crawl made late stays theirs, hazard or not.
    for (crawl_first, charged) in [(false, false), (true, true)] {
        let mut rig = ten_times_rig();
        let (_closed, open) = closed_then_open(&mut rig.drive);
        let key = rig.drive.weigh_station_key(&open);
        rig.prepare(61.0, None);
        rig.drive.trip.position_mi = open.at_mi - 3.0;
        rig.drive.enforcement_prev_mi = open.at_mi - 3.0;
        let reminder = "Signal for the scale exit.";

        hold_for(&mut rig, 61.0, open.at_mi, |rig| {
            rig.drive.trip.position_mi >= open.at_mi - 0.9
        });
        assert_eq!(rig.said(reminder), 0, "{:?}", rig.transcript());
        let hazard_from = if crawl_first {
            hold_for(&mut rig, 14.0, open.at_mi, |rig| {
                rig.drive.trip.position_mi >= open.at_mi - 0.3
            });
            assert_eq!(rig.said(reminder), 0, "{:?}", rig.transcript());
            0.28
        } else {
            0.48
        };
        hold_for(&mut rig, 61.0, open.at_mi, |rig| {
            rig.drive.trip.position_mi >= open.at_mi - hazard_from
        });
        assert_eq!(
            rig.said(reminder),
            1,
            "crawl first {crawl_first}: {:?}",
            rig.transcript()
        );
        assert!(!rig.drive.scale_reminder_held_by_game.contains(&key));

        // The hazard holds the cab to 0.15 of a mile, then the driver, who
        // never touches the exit, crosses at road speed.
        let mut crossed = false;
        for _ in 0..20_000 {
            let left = open.at_mi - rig.drive.trip.position_mi;
            rig.drive.hazard_deadline = (left > 0.15).then_some(60.0);
            rig.drive.trip.truck.velocity_mps = mph_to_mps(61.0);
            let before = rig.drive.trip.position_mi;
            rig.step(1, DT, None);
            crossed |= before < open.at_mi && rig.drive.trip.position_mi >= open.at_mi;
            if rig.drive.pull_over.is_some() || rig.drive.trip.position_mi > open.at_mi + 0.3 {
                break;
            }
        }
        assert!(crossed, "crawl first {crawl_first}: the truck crossed");
        assert!(rig.drive.enforcement_events.contains(&key));
        assert!(
            rig.drive.weigh_station_reminder_age_s < SCALE_REMINDER_REAL_LEAD_S,
            "the hazard's seconds are not the driver's: {:.1} s",
            rig.drive.weigh_station_reminder_age_s
        );
        assert_eq!(
            rig.drive.scale_reminder_held_by_game.contains(&key),
            !crawl_first,
            "crawl first {crawl_first}"
        );
        assert_eq!(
            rig.drive.pull_over.is_some(),
            charged,
            "crawl first {crawl_first}: {:?}",
            rig.transcript()
        );
    }
}

#[test]
fn a_hazard_during_the_drivers_own_crawl_does_not_excuse_the_crossing() {
    // The crawl keeps the reminder quiet, so a hazard that takes the cab
    // before it speaks used to mark the scale held by the game, and the
    // crossing at speed after it was excused: the crawl-then-speed dodge
    // with a hazard in the middle.
    let mut rig = ten_times_rig();
    let (_closed, open) = closed_then_open(&mut rig.drive);
    let key = rig.drive.weigh_station_key(&open);
    rig.prepare(61.0, None);
    rig.drive.trip.position_mi = open.at_mi - 3.0;
    rig.drive.enforcement_prev_mi = open.at_mi - 3.0;
    let reminder = "Signal for the scale exit.";

    hold_for(&mut rig, 61.0, open.at_mi, |rig| {
        rig.drive.trip.position_mi >= open.at_mi - 0.9
    });
    hold_for(&mut rig, 14.0, open.at_mi, |rig| {
        rig.drive.trip.position_mi >= open.at_mi - 0.3
    });
    assert_eq!(rig.said(reminder), 0, "{:?}", rig.transcript());
    assert!(rig.drive.scale_reminder_late_by_driver.contains(&key));

    let mut crossed = false;
    for _ in 0..20_000 {
        let left = open.at_mi - rig.drive.trip.position_mi;
        rig.drive.hazard_deadline = (left > 0.15).then_some(60.0);
        rig.drive.trip.truck.velocity_mps = mph_to_mps(61.0);
        let before = rig.drive.trip.position_mi;
        rig.step(1, DT, None);
        crossed |= before < open.at_mi && rig.drive.trip.position_mi >= open.at_mi;
        if rig.drive.pull_over.is_some() || rig.drive.trip.position_mi > open.at_mi + 0.3 {
            break;
        }
    }
    assert!(crossed, "the truck crossed");
    assert!(!rig.drive.scale_reminder_held_by_game.contains(&key));
    assert!(
        rig.drive.pull_over.is_some(),
        "the crossing is judged: {:?}",
        rig.transcript()
    );
}

#[test]
fn a_notice_first_heard_inside_the_reminder_window_still_excuses_a_quick_crossing() {
    // The game's late, not the driver's: the scale only came into range
    // inside the reminder window (a trip starting there, a notice held back
    // by the cab), so the reminder rides behind it with too few seconds.
    let mut rig = ten_times_rig();
    let (_closed, open) = closed_then_open(&mut rig.drive);
    let key = rig.drive.weigh_station_key(&open);
    rig.prepare(61.0, None);
    rig.drive.trip.position_mi = open.at_mi - 0.2;
    rig.drive.enforcement_prev_mi = open.at_mi - 0.2;

    hold_for(&mut rig, 61.0, open.at_mi + 0.3, |_| false);

    assert_eq!(
        rig.said("Open weigh station ahead"),
        1,
        "{:?}",
        rig.transcript()
    );
    assert!(rig.drive.scale_reminder_held_by_game.contains(&key));
    assert!(rig.drive.enforcement_events.contains(&key));
    assert!(rig.drive.pull_over.is_none(), "{:?}", rig.transcript());
    assert_eq!(rig.said("Scale bypass enforcement"), 0);
}

#[test]
fn a_cab_taken_inside_the_reminder_window_marks_the_reminder_the_games_late() {
    for (hazard, mph, held) in [(true, 61.0, true), (false, 14.0, false)] {
        let mut rig = ten_times_rig();
        let (_closed, open) = closed_then_open(&mut rig.drive);
        let key = rig.drive.weigh_station_key(&open);
        rig.drive.weigh_station_noticed.insert(key.clone());
        rig.drive.trip.position_mi = open.at_mi - 0.3;
        rig.drive.trip.truck.velocity_mps = mph_to_mps(mph);
        if hazard {
            rig.drive.hazard_deadline = Some(3.0);
        }

        rig.drive
            .check_weigh_station_enforcement(&mut rig.app.ctx, open.at_mi - 0.31);

        assert_eq!(
            rig.drive.scale_reminder_held_by_game.contains(&key),
            held,
            "hazard {hazard}, {mph} mph"
        );
    }
}

#[test]
fn the_reminder_window_is_sized_from_the_real_clock() {
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app);
    drive.trip.time_scale = 10.0;
    // At any legal truck speed the half mile already holds the lead on the
    // real clock the reminder starts.
    drive.trip.truck.velocity_mps = mph_to_mps(61.0);
    assert_eq!(drive.scale_reminder_mi(), WEIGH_STATION_REMINDER_MI);
    assert!(drive.scale_reminder_mi() / 61.0 * 3600.0 >= SCALE_REMINDER_REAL_LEAD_S);
    // Faster than half a mile can hold, the window grows -- to a ceiling.
    drive.trip.truck.velocity_mps = mph_to_mps(160.0);
    let window = drive.scale_reminder_mi();
    assert!(window > WEIGH_STATION_REMINDER_MI && window <= WEIGH_STATION_REMINDER_MAX_MI);
    assert!(window / 160.0 * 3600.0 >= SCALE_REMINDER_REAL_LEAD_S - 1e-9);
}

// -- (a) the hold in the clock's trace -------------------------------------------------

/// The open scale announced, a mile and a half out at 61 and ten times real
/// speed, driven on until its last reminder has spoken and the hold is on.
fn reminded_of_the_open_scale() -> (Rig, RoadStop) {
    let mut rig = ten_times_rig();
    let (_closed, open) = closed_then_open(&mut rig.drive);
    let key = rig.drive.weigh_station_key(&open);
    rig.drive.weigh_station_noticed.insert(key);
    rig.drive.trip.position_mi = open.at_mi - 1.5;
    rig.prepare(61.0, None);
    hold_61_for(&mut rig, 20_000, |rig| {
        rig.drive.trip.scale_reminder_hold_mi.is_some()
    });
    assert!(
        rig.said("Signal for the scale exit.") > 0,
        "{:?}",
        rig.transcript()
    );
    assert!(rig.drive.trip.position_mi < open.at_mi);
    (rig, open)
}

fn hold_scale_text() -> String {
    format!("scale reminder: {OPEN_NAME}")
}

#[test]
fn the_scale_reminder_hold_is_a_named_clock_override_and_its_trace_starts_and_ends() {
    let (mut rig, open) = reminded_of_the_open_scale();
    assert_eq!(rig.drive.trip.real_time_override(), Some("scale reminder"));
    assert_eq!(
        rig.drive.clock_override_reason(),
        Some(hold_scale_text()),
        "the session log names the scale holding the clock"
    );
    assert_eq!(rig.drive.trip.effective_time_scale(), 1.0);

    // Held past the trace's own hold, the start is written once.
    let log_hold_frames = (CLOCK_OVERRIDE_LOG_HOLD_S / DT) as usize + 2;
    hold_61_for(&mut rig, log_hold_frames, |_| false);
    assert_eq!(
        rig.drive.clock_pacing.override_reason,
        Some(hold_scale_text())
    );
    assert!(rig.drive.clock_pacing.override_logged, "start logged");
    assert!(rig.drive.trip.position_mi < open.at_mi);

    // Already judged, so the crossing below is the end of the hold and
    // nothing else: no police stop takes the clock over at the gore.
    let key = rig.drive.weigh_station_key(&open);
    rig.drive.enforcement_events.insert(key);
    hold_61_for(&mut rig, 20_000, |rig| {
        rig.drive.trip.position_mi > open.at_mi + 0.05
    });
    assert!(rig.drive.pull_over.is_none());
    assert_eq!(rig.drive.trip.scale_reminder_hold_mi, None);
    assert_eq!(rig.drive.trip.real_time_override(), None);
    assert_eq!(
        rig.drive.clock_pacing.override_reason, None,
        "the trace saw the hold end at the scale"
    );
    assert!(!rig.drive.clock_pacing.override_logged);
    assert!(rig.drive.trip.effective_time_scale() > 5.0);
}

#[test]
fn a_truck_stopped_inside_the_reminder_half_mile_runs_the_low_speed_clock() {
    // QA, 2026-10-07: a truck stopped short of the scale after its reminder
    // sat on the wall clock until it reached the gore, where any other
    // stopped truck runs at the low-speed pacing.
    let (mut rig, open) = reminded_of_the_open_scale();
    let stopped_frames = (5.0 / DT) as usize;
    for _ in 0..stopped_frames {
        rig.drive.trip.truck.velocity_mps = 0.0;
        rig.step(1, DT, None);
    }
    rig.drive.trip.truck.velocity_mps = 0.0;
    assert!(rig.drive.trip.position_mi < open.at_mi);
    assert!(
        rig.drive.trip.scale_reminder_hold(),
        "still armed to the gore"
    );
    assert_eq!(rig.drive.trip.real_time_override(), None);
    assert_eq!(rig.drive.clock_override_reason(), None);
    assert_eq!(rig.drive.trip.effective_time_scale(), LOW_SPEED_TIME_SCALE);
    assert_eq!(
        rig.drive.clock_pacing.override_reason, None,
        "the trace saw the hold let go"
    );

    // The line is the bypass speed: under it a crossing is not charged, so
    // the clock is paced; over it the hold pins real time again before a
    // crossing can count.
    rig.drive.trip.truck.velocity_mps = mph_to_mps(SCALE_BYPASS_MPH - 0.5);
    assert_eq!(rig.drive.trip.real_time_override(), None);
    assert!(rig.drive.trip.effective_time_scale() > LOW_SPEED_TIME_SCALE);
    rig.drive.trip.truck.velocity_mps = mph_to_mps(SCALE_BYPASS_MPH + 0.5);
    assert_eq!(rig.drive.trip.real_time_override(), Some("scale reminder"));
    assert_eq!(rig.drive.trip.effective_time_scale(), 1.0);

    // Pulled away at road speed, the hold is back by name.
    hold_61_for(&mut rig, 3, |_| false);
    assert_eq!(rig.drive.clock_override_reason(), Some(hold_scale_text()));

    // Crawled through the gore under the bypass speed: paced the whole way,
    // and not a bypass.
    let crawl = 10.0;
    for _ in 0..40_000 {
        rig.drive.trip.truck.velocity_mps = mph_to_mps(crawl);
        rig.step(1, DT, None);
        if rig.drive.trip.position_mi < open.at_mi {
            rig.drive.trip.truck.velocity_mps = mph_to_mps(crawl);
            assert_eq!(rig.drive.trip.real_time_override(), None);
            assert!(rig.drive.trip.effective_time_scale() > LOW_SPEED_TIME_SCALE);
        }
        if rig.drive.trip.position_mi > open.at_mi + 0.05 {
            break;
        }
    }
    assert!(rig.drive.trip.position_mi > open.at_mi);
    assert!(rig.drive.pull_over.is_none(), "{:?}", rig.transcript());
    assert_eq!(rig.said("Scale bypass enforcement"), 0);
    assert_eq!(rig.drive.trip.scale_reminder_hold_mi, None);
}

// -- (b) and (c): the check-in, and what comes after it ------------------------------

#[test]
fn checking_in_at_a_closed_scale_says_closed_and_names_the_open_one_still_ahead() {
    let mut rig = ten_times_rig();
    let (closed, open) = closed_then_open(&mut rig.drive);
    // The open scale was announced miles back.
    let open_key = rig.drive.weigh_station_key(&open);
    rig.drive.weigh_station_noticed.insert(open_key);
    rig.drive.trip.position_mi = closed.at_mi;
    rig.drive.trip.truck.velocity_mps = 0.0;
    rig.drive.trip.truck.set_parking_brake();
    rig.app.clear_speech();

    let minutes_before = rig.drive.trip.game_minutes;
    if let Some(profile) = rig.app.ctx.profile.as_mut() {
        profile.active_trip = None;
    }
    let log = rig.app.record_audio();
    rig.with_drive_on_stack(|rig, drive: DriveRef| {
        let mut state = RestStopState::with_drive(drive, closed.clone(), false);
        let shared = rig.app.ctx.state().expect("the drive is on the stack");
        {
            let mut borrowed = shared.borrow_mut();
            let driving = borrowed
                .as_any_mut()
                .downcast_mut::<DrivingState>()
                .expect("the rig's drive");
            state.enter_over_drive(&mut rig.app.ctx, driving);
        }
        rig.app.ctx.push_state_with(state, false);
        rig.app.ctx.run_deferred();
        // Nobody to check in with: no check-in row, a row that says closed.
        assert!(
            !rig.menu_labels()
                .iter()
                .any(|row| row.contains("Check in at inspection station")),
            "{:?}",
            rig.menu_labels()
        );
        let closed_row = format!("{CLOSED_NAME} is closed");
        // The arrival says the scale's name once: the closed row already
        // carries it (QA, 2026-10-07: "I-40 Weigh Station. Inspection
        // station. ... I-40 Weigh Station is closed.").
        let arrival: Vec<String> = rig
            .transcript()
            .into_iter()
            .filter(|line| line.contains("Inspection station."))
            .collect();
        assert_eq!(arrival.len(), 1, "{:?}", rig.transcript());
        assert_eq!(
            arrival[0].matches(CLOSED_NAME).count(),
            1,
            "{:?}",
            arrival[0]
        );
        assert!(arrival[0].contains(&closed_row), "{:?}", arrival[0]);
        log.borrow_mut().played.clear();
        assert!(rig.select_menu_containing(&closed_row));
        // Nothing to record: no notify tone, no inspected visit, no save.
        assert!(
            !log.borrow()
                .played
                .iter()
                .any(|(key, ..)| key == "ui/notify"),
            "{:?}",
            log.borrow().played
        );
        assert!(
            rig.menu_labels()
                .iter()
                .any(|row| row.contains(&closed_row)),
            "a closed scale settles nothing, so its row stays: {:?}",
            rig.menu_labels()
        );
        assert!(
            rig.app
                .ctx
                .profile
                .as_ref()
                .is_some_and(|profile| profile.active_trip.is_none()),
            "a closed scale check-in is not a save"
        );
        assert!(rig.select_menu_containing("Back to the road"));
    });

    let closed_line = format!("{CLOSED_NAME} is closed. Pull back onto the highway.");
    assert_eq!(rig.said(&closed_line), 1, "{:?}", rig.transcript());
    assert!(!rig.drive.stop_visit(&closed).inspected);
    assert_eq!(rig.said("wave you"), 0, "{:?}", rig.transcript());
    assert_eq!(rig.said("Inspection check-in complete"), 0);
    assert_eq!(
        rig.drive.trip.game_minutes, minutes_before,
        "a closed scale costs no time"
    );

    // Back on the road, the open one is named once, after the stop.
    rig.step(2, DT, None);
    let expected =
        format!("[event] That was {CLOSED_NAME}. {OPEN_NAME} is still ahead, open, 6.0 miles.");
    let heard: Vec<String> = rig
        .transcript()
        .into_iter()
        .filter(|line| line.contains("still ahead, open"))
        .collect();
    assert_eq!(heard, vec![expected]);
}

#[test]
fn a_pause_with_no_open_scale_announced_adds_nothing() {
    let mut rig = ten_times_rig();
    let (_closed, open) = closed_then_open(&mut rig.drive);
    rig.drive.truck_mut().velocity_mps = mph_to_mps(61.0);
    // Just inside the notice's lead, wherever the spoken notice sizes it.
    let lead_mi = rig.drive.scale_notice_lookahead_mi(&rig.app.ctx);
    rig.drive.trip.position_mi = open.at_mi - lead_mi + 0.05;
    pause_and_resume(&mut rig);
    // Not announced yet: the notice, not a re-announcement, is what speaks
    // it -- on this very frame, and only once.
    assert!(rig.drive.scale_reannounce.is_none());
    rig.step(2, DT, None);
    assert_eq!(
        rig.said("Open weigh station ahead"),
        1,
        "{:?}",
        rig.transcript()
    );
    assert_eq!(rig.said("still ahead, open"), 0, "{:?}", rig.transcript());

    // Past the scale, nothing to re-announce either.
    let open_key = rig.drive.weigh_station_key(&open);
    rig.drive.weigh_station_noticed.insert(open_key);
    rig.drive.trip.position_mi = open.at_mi + 0.2;
    pause_and_resume(&mut rig);
    rig.step(2, DT, None);
    assert_eq!(rig.said("still ahead, open"), 0, "{:?}", rig.transcript());
}

#[test]
fn a_pause_after_the_reminder_brings_the_instruction_back() {
    let mut rig = ten_times_rig();
    let (_closed, open) = closed_then_open(&mut rig.drive);
    let key = rig.drive.weigh_station_key(&open);
    rig.drive.weigh_station_noticed.insert(key.clone());
    rig.drive.weigh_station_reminder_key = key;
    rig.drive.trip.position_mi = open.at_mi - 0.5;
    rig.drive.trip.truck.velocity_mps = mph_to_mps(61.0);
    rig.app.clear_speech();

    pause_and_resume(&mut rig);
    rig.step(1, DT, None);
    let heard: Vec<String> = rig
        .transcript()
        .into_iter()
        .filter(|line| line.contains("still ahead, open"))
        .collect();
    assert_eq!(
        heard,
        vec![format!(
            "[event] {OPEN_NAME} is still ahead, open, half a mile. Signal for the scale exit."
        )]
    );
}

// -- (d) the stop screen under a held key ----------------------------------------------

fn a_drive(app: &mut TestApp) -> DrivingState {
    let world = app.ctx.world;
    let mut profile = Profile::named_in("Held Key", "Buffalo");
    profile.tutorial_done = true;
    app.ctx.profile = Some(profile);
    let route = world
        .supported_route("Buffalo", "Rochester", None)
        .expect("the world routes")
        .expect("Buffalo to Rochester is supported");
    let mut job = Job::new(
        CARGO_CATALOG
            .get("general")
            .expect("the general cargo type"),
        12.0,
        "Buffalo",
        "company yard",
        "Rochester",
        route.miles(),
        1000.0,
        12.0,
    );
    job.destination_location = "Rochester freight market".to_string();
    let mut drive = DrivingState::new(
        &mut app.ctx,
        job,
        route,
        Some(0),
        DRIVE_PHASE_DELIVERY,
        Some(12.0),
    );
    drive.trip.set_npc_vehicles(Vec::new());
    drive.trip.weather.current = WeatherKind::Clear;
    drive.trip.posts = Vec::new();
    drive
}

fn held(key: Key) -> InputEvent {
    InputEvent::KeyDown {
        key,
        mods: Mods::NONE,
        text: None,
        repeat: true,
    }
}

#[test]
fn a_held_brake_key_does_not_read_the_stop_screen_row_over_and_over() {
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app);
    drive.trip_seed = 1; // caught: the bypass stop is deterministic
    let open = a_scale(OPEN_NAME, 10.0);
    drive.trip.stops = vec![open.clone()];
    drive.trip.posts = vec![open_post(&open)];
    let key = drive.weigh_station_key(&open);
    drive.weigh_station_noticed.insert(key.clone());
    drive.weigh_station_reminder_key = key;
    drive.weigh_station_reminder_age_s = SCALE_REMINDER_REAL_LEAD_S;
    drive.trip.position_mi = 10.1;
    drive.trip.truck.velocity_mps = mph_to_mps(55.0);
    drive.check_weigh_station_enforcement(&mut app.ctx, 9.9);
    assert!(drive.pull_over.is_some());

    app.clear_speech();
    drive.trip.truck.velocity_mps = 0.0;
    drive.update_pull_over(&mut app.ctx, 1.0, true);
    app.ctx.run_deferred();
    let top = app.ctx.state().expect("the stop screen");
    assert!(top.borrow().as_any().is::<EnforcementStopState>());

    // The brake key is still down: the keyboard's own repeat, every 34 ms.
    for _ in 0..20 {
        app.dispatch_to_state(&held(Key::Down));
    }
    let count = |app: &TestApp, needle: &str| {
        app.speech()
            .transcript_lines()
            .iter()
            .filter(|line| line.contains(needle))
            .count()
    };
    assert_eq!(
        count(&app, "Pull back onto the highway."),
        1,
        "{:?}",
        app.speech().transcript_lines()
    );
    assert_eq!(
        count(&app, "Fine: "),
        1,
        "{:?}",
        app.speech().transcript_lines()
    );

    // A fresh press is the player asking: it still reads the row.
    app.dispatch_to_state(&InputEvent::key(Key::Down));
    assert_eq!(count(&app, "Pull back onto the highway."), 2);
}
