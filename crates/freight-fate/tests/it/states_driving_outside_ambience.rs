//! `states/driving_updates/outside.rs`: birds by day, insects at night and a
//! river under the bridge, each through the closed window or clear through
//! the open one, and the joints at each end of a river bridge (a player's
//! ask, 2026-11-19). Pure ambience: what is asserted
//! is which loop plays and at what level, never a spoken line.

use ff_core::models::business::LEASED_OWNER_OPERATOR;
use ff_core::models::jobs::{Job, CARGO_CATALOG};
use ff_core::models::profile::Profile;
use ff_core::sim::cab::GLASS_TRAVEL_S;
use ff_core::sim::outside::{OutsideScene, Water, RIVER_HOLD_S, RIVER_SPAN_MI, WATER_FADE_OUT_S};
use ff_core::sim::road_texture::{RoadSurface, AXLE_GAP_M, TRAILER_GAP_M};
use ff_core::sim::weather::WeatherKind;

use freight_fate::app::testing::{AudioLog, RecordedLoop, TestApp};
use freight_fate::audio::{
    Audio, CH_NATURE_OPEN, CH_NATURE_SEALED, CH_OUTSIDE, CH_WATER_OPEN, CH_WATER_SEALED,
};
use freight_fate::states::base::{InputEvent, Key};
use freight_fate::states::driving::DrivingState;
use freight_fate::states::driving_core::*;

use crate::audio_support::recordings_rig;

const FRAME: f64 = 1.0 / 30.0;

/// Buffalo to Rochester on the Thruway, parked at mile 20, clear and warm
/// (seasons off, so no temperature), at `local_hour` Eastern.
fn a_drive(app: &mut TestApp, local_hour: f64) -> (DrivingState, AudioLog) {
    let world = app.ctx.world;
    let mut profile = Profile::named_in("Listener", "Buffalo");
    profile.tutorial_done = true;
    profile.business_status = LEASED_OWNER_OPERATOR.to_string();
    app.ctx.profile = Some(profile);
    let route = world
        .supported_route("Buffalo", "Rochester", None)
        .expect("the world routes")
        .expect("Buffalo to Rochester has a route");
    let job = Job::new(
        CARGO_CATALOG
            .get("general")
            .expect("the general cargo type"),
        12.0,
        "Buffalo",
        "yard",
        "Rochester",
        200.0,
        900.0,
        12.0,
    );
    let mut drive = DrivingState::new(
        &mut app.ctx,
        job,
        route,
        Some(99),
        DRIVE_PHASE_DELIVERY,
        Some(local_hour),
    );
    drive.trip.start_hour = local_hour;
    drive.trip.game_minutes = 0.0;
    drive.trip.position_mi = 20.0;
    drive.trip.truck.velocity_mps = 0.0;
    drive.trip.weather.current = WeatherKind::Clear;
    drive.trip.weather.game_hours = None;
    let log = app.record_audio();
    (drive, log)
}

/// `seconds` of frames of the cab and the outside, as `update_audio` runs them.
fn run(drive: &mut DrivingState, app: &mut TestApp, seconds: f64) {
    let frames = (seconds / FRAME).round() as usize;
    for _ in 0..frames {
        drive.update_cab_audio(&mut app.ctx, FRAME);
        drive.update_outside_audio(&mut app.ctx, FRAME);
    }
}

fn on(log: &AudioLog, channel: u32) -> Option<RecordedLoop> {
    log.borrow().loops.get(&channel).cloned()
}

#[test]
fn test_a_warm_night_with_the_window_up_plays_the_muffled_insects_only() {
    let mut app = TestApp::new();
    let (mut drive, log) = a_drive(&mut app, 23.0);
    run(&mut drive, &mut app, 4.0);
    assert_eq!(drive.outside.scene, OutsideScene::Insects);
    let sealed = on(&log, CH_NATURE_SEALED).expect("the muffled bed plays");
    let open = on(&log, CH_NATURE_OPEN).expect("the clear copy runs beside it");
    assert!(sealed.key.starts_with("ambient/insects_") && sealed.key.ends_with("_sealed"));
    assert_eq!(open.key, sealed.key.trim_end_matches("_sealed"));
    assert!(sealed.volume > 0.5, "{sealed:?}");
    assert_eq!(open.volume, 0.0, "nothing clear through a closed window");
    assert_eq!(sealed.pan, 0.0);

    // Rolling at highway speed, the tires cover some of it.
    drive.trip.truck.velocity_mps = 29.0;
    run(&mut drive, &mut app, 0.1);
    let rolling = on(&log, CH_NATURE_SEALED).unwrap();
    assert!(rolling.volume < sealed.volume, "{rolling:?} vs {sealed:?}");
    assert!(
        rolling.volume > sealed.volume * 0.75,
        "the closed window keeps the insects over the road"
    );
}

#[test]
fn test_z_moves_the_insects_to_the_clear_copy_on_the_drivers_side() {
    let mut app = TestApp::new();
    let (mut drive, log) = a_drive(&mut app, 23.0);
    run(&mut drive, &mut app, 4.0);
    let before = on(&log, CH_NATURE_SEALED).unwrap().volume;

    drive.handle_key_event(&mut app.ctx, &InputEvent::key(Key::Z));
    run(&mut drive, &mut app, GLASS_TRAVEL_S / 2.0);
    let (sealed, open) = (
        on(&log, CH_NATURE_SEALED).unwrap(),
        on(&log, CH_NATURE_OPEN).unwrap(),
    );
    assert!(
        sealed.volume > 0.0 && open.volume > 0.0,
        "a crossfade midway"
    );
    run(&mut drive, &mut app, GLASS_TRAVEL_S);
    let (sealed, open) = (
        on(&log, CH_NATURE_SEALED).unwrap(),
        on(&log, CH_NATURE_OPEN).unwrap(),
    );
    assert_eq!(sealed.volume, 0.0, "the muffled copy keeps running at zero");
    assert!((open.volume - before).abs() < 1e-9, "{open:?}");
    assert!(open.pan < 0.0, "the driver's window is on the left");

    drive.handle_key_event(&mut app.ctx, &InputEvent::key(Key::Z));
    run(&mut drive, &mut app, GLASS_TRAVEL_S + 0.1);
    assert_eq!(on(&log, CH_NATURE_OPEN).unwrap().volume, 0.0);
    assert!((on(&log, CH_NATURE_SEALED).unwrap().volume - before).abs() < 1e-9);
}

#[test]
fn test_day_plays_birds_and_dusk_swaps_them_for_insects() {
    let mut app = TestApp::new();
    let (mut drive, log) = a_drive(&mut app, 12.0);
    run(&mut drive, &mut app, 4.0);
    assert_eq!(drive.outside.scene, OutsideScene::Birds);
    assert_eq!(
        on(&log, CH_NATURE_SEALED).unwrap().key,
        "ambient/birds_day_sealed"
    );
    // Dusk: the birds fade out before the insects come in.
    drive.trip.start_hour = 19.5;
    run(&mut drive, &mut app, 1.0);
    assert_eq!(drive.outside.scene, OutsideScene::Insects);
    assert_eq!(
        on(&log, CH_NATURE_SEALED).unwrap().key,
        "ambient/birds_day_sealed",
        "still fading"
    );
    run(&mut drive, &mut app, 3.0);
    assert!(on(&log, CH_NATURE_SEALED)
        .unwrap()
        .key
        .starts_with("ambient/insects_"));
}

#[test]
fn test_heavy_rain_and_a_cold_night_play_nothing_outside() {
    let mut app = TestApp::new();
    let (mut drive, log) = a_drive(&mut app, 23.0);
    drive.trip.weather.current = WeatherKind::HeavyRain;
    run(&mut drive, &mut app, 4.0);
    assert_eq!(drive.outside.scene, OutsideScene::Quiet);
    assert!(on(&log, CH_NATURE_SEALED).is_none() && on(&log, CH_NATURE_OPEN).is_none());
    drop(drive);
    drop(app);

    let mut app = TestApp::new();
    let (mut drive, log) = a_drive(&mut app, 23.0);
    // A January night by the lakes, seasons on.
    drive.trip.weather.game_hours = Some(14.0 * 24.0 + 23.0);
    let t = drive.trip.weather.temperature_c().expect("seasons on");
    assert!(t < 8.0, "{t}");
    run(&mut drive, &mut app, 4.0);
    assert_eq!(drive.outside.scene, OutsideScene::Quiet);
    assert!(on(&log, CH_NATURE_SEALED).is_none());
}

#[test]
fn test_a_river_plays_while_over_it_and_ends_after_the_hold() {
    let mut app = TestApp::new();
    let (mut drive, log) = a_drive(&mut app, 12.0);
    run(&mut drive, &mut app, 0.1);
    let river = *drive
        .outside
        .river_miles()
        .first()
        .expect("the Genesee is on this route");
    assert!(on(&log, CH_WATER_SEALED).is_none());

    drive.trip.position_mi = river;
    run(&mut drive, &mut app, 1.0);
    assert!(matches!(drive.outside.water, Water::Over { .. }));
    let sealed = on(&log, CH_WATER_SEALED).expect("the river plays");
    let open = on(&log, CH_WATER_OPEN).expect("its clear copy runs beside it");
    assert_eq!(sealed.key, "ambient/river_sealed");
    assert_eq!(open.key, "ambient/river");
    assert!(sealed.volume > 0.5 && open.volume == 0.0);

    // Past it: the hold, then the fade, then nothing.
    drive.trip.position_mi = river + 1.0;
    run(&mut drive, &mut app, RIVER_HOLD_S - 0.5);
    assert!(on(&log, CH_WATER_SEALED).is_some(), "still held");
    run(&mut drive, &mut app, 0.5 + WATER_FADE_OUT_S + 0.2);
    assert_eq!(drive.outside.water, Water::Away);
    assert!(on(&log, CH_WATER_SEALED).is_none() && on(&log, CH_WATER_OPEN).is_none());
}

#[test]
fn test_a_rest_stops_bed_ends_when_the_drive_resumes() {
    let mut app = TestApp::new();
    let (mut drive, log) = a_drive(&mut app, 23.0);
    app.ctx.set_ambient(Some("ambient/truck_stop"));
    assert_eq!(log.borrow().ambient.as_deref(), Some("ambient/truck_stop"));
    drive.update_audio(&mut app.ctx, FRAME);
    assert_eq!(log.borrow().ambient, None);
}

#[test]
fn test_leaving_the_drive_stops_all_four_outside_loops() {
    let Some(mut r) = recordings_rig() else {
        return;
    };
    for (ch, key) in CH_OUTSIDE.iter().zip([
        "ambient/insects_1_sealed",
        "ambient/insects_1",
        "ambient/river_sealed",
        "ambient/river",
    ]) {
        r.engine.start_loop_with(*ch, key, 0.5, 0);
        assert!(r.engine.backend().loop_entry(*ch).is_some(), "{key}");
    }
    r.engine.stop_world();
    for ch in CH_OUTSIDE {
        assert!(r.engine.backend().loop_entry(ch).is_none());
    }
}

#[test]
fn test_a_staged_window_down_starts_the_drive_with_the_glass_down() {
    let mut app = TestApp::new();
    let scenario = freight_fate::playtest::scenario::Scenario::from_json(
        serde_json::json!({"window": "down"}).as_object().unwrap(),
    )
    .expect("window down parses");
    assert_eq!(scenario.window_down, Some(true));
    assert!(freight_fate::playtest::scenario::Scenario::from_json(
        serde_json::json!({"window": "ajar"}).as_object().unwrap()
    )
    .is_err());
    app.ctx.playtest_window_down = scenario.window_down;
    let (mut drive, log) = a_drive(&mut app, 23.0);
    assert_eq!(drive.cab.window(), ff_core::sim::cab::Window::Down);
    run(&mut drive, &mut app, 4.0);
    assert!(on(&log, CH_NATURE_OPEN).unwrap().volume > 0.5);
}

#[test]
fn test_no_outside_loop_is_asked_for_more_than_full_volume() {
    // Hearken clamps a sound at 1, so a bed asked above it would flatten and
    // the glass would top out early. Parked (no road mask), over a river,
    // through a whole window travel each way.
    let mut app = TestApp::new();
    let (mut drive, log) = a_drive(&mut app, 23.0);
    run(&mut drive, &mut app, 0.1);
    let river = *drive.outside.river_miles().first().expect("a river");
    drive.trip.position_mi = river;
    let mut loudest: f64 = 0.0;
    let mut step = |drive: &mut DrivingState, app: &mut TestApp, seconds: f64| {
        for _ in 0..(seconds / FRAME).round() as usize {
            drive.update_cab_audio(&mut app.ctx, FRAME);
            drive.update_outside_audio(&mut app.ctx, FRAME);
            for ch in CH_OUTSIDE {
                if let Some(l) = on(&log, ch) {
                    assert!(l.volume <= 1.0, "{ch}: {l:?}");
                    loudest = loudest.max(l.volume);
                }
            }
        }
    };
    step(&mut drive, &mut app, 4.0);
    drive.handle_key_event(&mut app.ctx, &InputEvent::key(Key::Z));
    step(&mut drive, &mut app, GLASS_TRAVEL_S + 0.5);
    drive.handle_key_event(&mut app.ctx, &InputEvent::key(Key::Z));
    step(&mut drive, &mut app, GLASS_TRAVEL_S + 0.5);
    assert!(
        loudest > 0.9,
        "the beds reach full volume parked: {loudest}"
    );
}

/// The bridge joint hits played, in order.
fn bridge_hits(log: &AudioLog) -> usize {
    log.borrow()
        .played
        .iter()
        .filter(|(key, _, _)| key == "vehicle/bridge_joint")
        .count()
}

/// One frame of the whole audio update, rolling at `speed`.
fn roll(drive: &mut DrivingState, app: &mut TestApp, speed: f64, seconds: f64) {
    for _ in 0..(seconds / FRAME).round() as usize {
        drive.trip.truck.velocity_mps = speed;
        drive.update_audio(&mut app.ctx, FRAME);
    }
}

#[test]
fn test_each_end_of_a_river_bridge_is_a_joint_under_all_three_axles() {
    let mut app = TestApp::new();
    let (mut drive, log) = a_drive(&mut app, 12.0);
    drive.road_texture.surface = RoadSurface::held(false);
    roll(&mut drive, &mut app, 0.0, 0.1);
    let river = *drive.outside.river_miles().first().expect("a river");

    // On to the bridge: the steer axle at once, then the drives and the
    // trailer as the wheels roll over the joint.
    drive.trip.position_mi = river - RIVER_SPAN_MI + 0.01;
    roll(&mut drive, &mut app, 20.0, FRAME);
    assert_eq!(bridge_hits(&log), 1, "the steer axle");
    roll(
        &mut drive,
        &mut app,
        20.0,
        (AXLE_GAP_M + TRAILER_GAP_M) / 20.0 + 0.1,
    );
    assert_eq!(bridge_hits(&log), 3, "steer, drives, trailer");

    // Off the far end: three more.
    drive.trip.position_mi = river + RIVER_SPAN_MI + 0.01;
    roll(&mut drive, &mut app, 20.0, 1.5);
    assert_eq!(bridge_hits(&log), 6);
}

#[test]
fn test_a_bridge_crossed_parked_or_crawling_makes_no_joint() {
    let mut app = TestApp::new();
    let (mut drive, log) = a_drive(&mut app, 12.0);
    roll(&mut drive, &mut app, 0.0, 0.1);
    let river = *drive.outside.river_miles().first().expect("a river");
    drive.trip.position_mi = river;
    roll(&mut drive, &mut app, 2.0, 1.0);
    drive.trip.position_mi = river + 1.0;
    roll(&mut drive, &mut app, 2.0, 1.0);
    assert_eq!(bridge_hits(&log), 0);
}
