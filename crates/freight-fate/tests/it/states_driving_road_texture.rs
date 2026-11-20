//! Seams and bridge joints from the pavement layer: the surface the layer
//! reads decides the seams, its structures decide the joints, and where it
//! reads nothing the road is invented.

use ff_core::data::pavement::StructureKind;
use ff_core::models::business::LEASED_OWNER_OPERATOR;
use ff_core::models::jobs::{Job, CARGO_CATALOG};
use ff_core::models::profile::Profile;
use ff_core::sim::outside::RIVER_SPAN_MI;
use ff_core::sim::road_texture::joints::{JointSpan, SpanOrigin, MIN_SPAN_MI};
use ff_core::sim::road_texture::{
    Axle, Crossing, Hit, RoadSurface, AXLE_GAP_M, OVERLAY_SEAM_FACTOR, POOR_SEAM_FACTOR,
    TRAILER_GAP_M,
};
use ff_core::sim::weather::WeatherKind;

use freight_fate::app::testing::{AudioLog, TestApp};
use freight_fate::audio::BANK_JITTER;
use freight_fate::states::driving::DrivingState;
use freight_fate::states::driving_core::*;
use freight_fate::states::driving_updates::road_texture::{hit_sound, SEAM_LEVEL};

const FRAME: f64 = 1.0 / 60.0;
/// Full seam severity: 30 m/s, about 67 mph.
const HIGHWAY_MPS: f64 = 30.0;
const METRES_PER_MILE: f64 = 1_609.344;

/// `from` to `to` on the supported route, clear weather, at route mile `at`.
fn a_drive(app: &mut TestApp, from: &str, to: &str, at: f64) -> (DrivingState, AudioLog) {
    let world = app.ctx.world;
    let mut profile = Profile::named_in("Listener", from);
    profile.tutorial_done = true;
    profile.business_status = LEASED_OWNER_OPERATOR.to_string();
    app.ctx.profile = Some(profile);
    let route = world
        .supported_route(from, to, None)
        .expect("the world routes")
        .expect("a route");
    let job = Job::new(
        CARGO_CATALOG.get("general").expect("general cargo"),
        12.0,
        from,
        "yard",
        to,
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
        Some(12.0),
    );
    drive.trip.weather.current = WeatherKind::Clear;
    drive.trip.weather.game_hours = None;
    place(&mut drive, at);
    let log = app.record_audio();
    (drive, log)
}

/// Put the truck at route mile `at` without crossing anything on the way.
fn place(drive: &mut DrivingState, at: f64) {
    drive.trip.position_mi = at;
    drive.road_texture.last_mi = at;
}

/// Roll at `speed` for `seconds`, the route passing `scale` times faster
/// than the wheels, the way the trip moves it.
fn cruise(drive: &mut DrivingState, app: &mut TestApp, speed: f64, scale: f64, seconds: f64) {
    for _ in 0..(seconds / FRAME).round() as usize {
        drive.trip.truck.velocity_mps = speed;
        drive.trip.position_mi += speed * scale * FRAME / METRES_PER_MILE;
        drive.update_audio(&mut app.ctx, FRAME);
    }
}

fn plays(log: &AudioLog, key: &str) -> Vec<f64> {
    log.borrow()
        .played
        .iter()
        .filter(|(k, _, _)| k == key)
        .map(|(_, v, _)| *v)
        .collect()
}

fn seams(log: &AudioLog) -> Vec<f64> {
    plays(log, "vehicle/seam_steer")
}

/// A route mile the drive's layer reads as `surface`, away from its ends.
fn mile_reading(drive: &DrivingState, surface: &str) -> f64 {
    let total = drive.trip.total_miles();
    (0..(total * 10.0) as usize)
        .map(|i| i as f64 / 10.0)
        .find(|&m| {
            (1..=6).all(|k| {
                drive
                    .trip
                    .pavement_reading_at(m + 0.1 * k as f64)
                    .is_some_and(|r| r.surface.name() == surface)
            })
        })
        .map(|m| m + 0.2)
        .unwrap_or_else(|| panic!("no {surface} on this route"))
}

#[test]
fn test_seams_follow_the_read_surface_both_ways() {
    for (from, to) in [("Kenosha", "Chicago"), ("Chicago", "Kenosha")] {
        let mut app = TestApp::new();
        let (mut drive, log) = a_drive(&mut app, from, to, 0.0);
        let jointed = mile_reading(&drive, "jointed_plain_concrete");
        let continuous = mile_reading(&drive, "continuous_concrete");
        let overlay = mile_reading(&drive, "asphalt_over_jointed_concrete");

        place(&mut drive, jointed);
        cruise(&mut drive, &mut app, HIGHWAY_MPS, 1.0, 3.0);
        let on_slabs = seams(&log);
        assert!(on_slabs.len() >= 4, "{from}: {on_slabs:?}");
        assert_eq!(
            drive.road_texture.surface.origin(),
            "read jointed_plain_concrete"
        );
        // Good concrete: the approved level.
        assert!(
            on_slabs.iter().all(|v| (v - SEAM_LEVEL).abs() < 1e-9),
            "{on_slabs:?}"
        );

        log.borrow_mut().played.clear();
        place(&mut drive, continuous);
        cruise(&mut drive, &mut app, HIGHWAY_MPS, 1.0, 3.0);
        // The drives of a seam crossed just before still land; no new seam.
        assert!(seams(&log).is_empty(), "{from}: {:?}", seams(&log));
        assert_eq!(drive.road_texture.surface.name(), "smooth");

        log.borrow_mut().played.clear();
        place(&mut drive, overlay);
        cruise(&mut drive, &mut app, HIGHWAY_MPS, 1.0, 3.0);
        let soft = seams(&log);
        assert!(soft.len() >= 4, "{from}: {soft:?}");
        assert!(
            soft.iter()
                .all(|v| (v - SEAM_LEVEL * OVERLAY_SEAM_FACTOR).abs() < 1e-9),
            "{soft:?}"
        );
    }
}

#[test]
fn test_a_stretch_the_layer_does_not_read_is_invented_road() {
    let mut app = TestApp::new();
    let (mut drive, _log) = a_drive(&mut app, "Kenosha", "Chicago", 0.0);
    // Kenosha to Chicago has no reading from 64.805 to 64.97 (leg miles).
    place(&mut drive, 64.9);
    assert_eq!(drive.trip.pavement_reading_at(64.9), None);
    cruise(&mut drive, &mut app, HIGHWAY_MPS, 1.0, 0.1);
    assert_eq!(drive.road_texture.surface.origin(), "invented");
    // And back onto read road.
    place(&mut drive, 30.0);
    cruise(&mut drive, &mut app, HIGHWAY_MPS, 1.0, 0.1);
    assert_eq!(
        drive.road_texture.surface.origin(),
        "read jointed_plain_concrete"
    );
}

#[test]
fn test_a_held_surface_wins_over_the_layer() {
    let mut app = TestApp::new();
    let (mut drive, log) = a_drive(&mut app, "Kenosha", "Chicago", 17.0);
    drive.road_texture.surface = RoadSurface::held(true);
    cruise(&mut drive, &mut app, HIGHWAY_MPS, 1.0, 3.0);
    assert_eq!(drive.road_texture.surface.origin(), "held");
    assert!(!seams(&log).is_empty());
}

/// Bridge joint hits on a drive with only `spans`, crossing them at highway speed.
fn joints_over(spans: Vec<JointSpan>, scale: f64, seconds: f64) -> usize {
    let mut app = TestApp::new();
    let (mut drive, log) = a_drive(&mut app, "Kenosha", "Chicago", 17.0);
    drive.road_texture.surface = RoadSurface::held(false);
    drive.road_texture.spans = spans;
    cruise(&mut drive, &mut app, HIGHWAY_MPS, scale, seconds);
    plays(&log, "vehicle/bridge_joint").len()
}

fn bridge(from_mi: f64, to_mi: f64) -> JointSpan {
    JointSpan::from_structure(from_mi, to_mi, StructureKind::Bridge).expect("a jointed bridge")
}

#[test]
fn test_a_long_bridge_is_two_joints_and_a_short_one_one() {
    // Each joint is three hits: steer, drives, trailer.
    assert_eq!(joints_over(vec![bridge(17.5, 18.5)], 20.0, 6.0), 6);
    assert_eq!(joints_over(vec![bridge(17.5, 17.56)], 20.0, 6.0), 3);
    // Relaxed pacing hears a shorter bridge at both ends.
    assert_eq!(joints_over(vec![bridge(17.5, 17.85)], 10.0, 6.0), 6);
    assert_eq!(joints_over(vec![bridge(17.5, 17.85)], 20.0, 6.0), 3);
    // An assumed river span keeps its two joints.
    assert_eq!(joints_over(vec![JointSpan::river(17.8)], 20.0, 6.0), 6);
}

#[test]
fn test_each_joint_lands_under_all_three_axles_in_wheel_travel() {
    let mut app = TestApp::new();
    let (mut drive, log) = a_drive(&mut app, "Kenosha", "Chicago", 17.0);
    drive.road_texture.surface = RoadSurface::held(false);
    drive.road_texture.spans = vec![bridge(17.01, 18.0)];
    // At 20 m/s and real-time pacing the near end is half a second away.
    let speed = 20.0;
    let until_end = 0.01 * METRES_PER_MILE / speed;
    cruise(&mut drive, &mut app, speed, 1.0, until_end + FRAME);
    assert_eq!(
        plays(&log, "vehicle/bridge_joint").len(),
        1,
        "the steer axle"
    );
    cruise(
        &mut drive,
        &mut app,
        speed,
        1.0,
        (AXLE_GAP_M + TRAILER_GAP_M) / speed,
    );
    assert_eq!(
        plays(&log, "vehicle/bridge_joint").len(),
        3,
        "drives, trailer"
    );
}

#[test]
fn test_a_bridge_crossed_crawling_makes_no_joint() {
    let mut app = TestApp::new();
    let (mut drive, log) = a_drive(&mut app, "Kenosha", "Chicago", 17.0);
    drive.road_texture.spans = vec![bridge(17.001, 17.002 + MIN_SPAN_MI)];
    cruise(&mut drive, &mut app, 2.0, 20.0, 6.0);
    assert!(drive.trip.position_mi > 17.06);
    assert!(plays(&log, "vehicle/bridge_joint").is_empty());
}

#[test]
fn test_the_route_joints_only_its_real_bridges_and_each_river_once() {
    let mut app = TestApp::new();
    let (drive, _log) = a_drive(&mut app, "Houston", "San Antonio", 0.0);
    let spans = drive.road_texture.spans.clone();
    assert!(!spans.is_empty());
    for s in &spans {
        match s.origin {
            SpanOrigin::Structure(kind) => {
                assert_ne!(kind, StructureKind::Tunnel);
                assert!(s.to_mi - s.from_mi >= MIN_SPAN_MI - 1e-9, "{s:?}");
            }
            SpanOrigin::RiverAssumed => {}
        }
    }
    // No river is jointed twice: an assumed span never sits on a structure.
    for river in drive.outside.river_miles() {
        let around: Vec<_> = spans
            .iter()
            .filter(|s| s.from_mi - RIVER_SPAN_MI <= *river && *river <= s.to_mi + RIVER_SPAN_MI)
            .collect();
        let assumed = around
            .iter()
            .filter(|s| s.origin == SpanOrigin::RiverAssumed)
            .count();
        assert!(
            assumed <= 1 && (assumed == 0 || around.len() == 1),
            "{river}: {around:?}"
        );
    }
    // Culvert-sized structures are on the layer but not jointed.
    let leg = &drive.trip.route.legs[0];
    let short = leg
        .pavement()
        .expect("the layer")
        .structures
        .iter()
        .filter(|s| s.to_mi - s.from_mi < MIN_SPAN_MI)
        .count();
    assert!(short > 0);
}

#[test]
fn test_no_hit_ever_asks_for_more_than_one() {
    for crossing in [Crossing::Seam, Crossing::BridgeJoint] {
        for axle in [Axle::Steer, Axle::Drive, Axle::Trailer] {
            for factor in [OVERLAY_SEAM_FACTOR, 1.0, POOR_SEAM_FACTOR] {
                let (_, level) = hit_sound(
                    Hit {
                        axle,
                        crossing,
                        factor,
                    },
                    1.0,
                );
                assert!(
                    level * BANK_JITTER.1 <= 1.0,
                    "{crossing:?} {axle:?} {factor}: {level}"
                );
            }
        }
    }
}
