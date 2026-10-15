//! Driving by the picture alone.
//!
//! A sighted driver that never hears a word: every decision it makes is
//! read off the visual display's capture of the drive -- the speedometer,
//! the speed-limit sign, the curve warning, the road bending on screen, the
//! truck's place between the lane lines, the traffic ahead and on the
//! radar, the blind-spot lights -- and every action goes in as the same key
//! presses a player
//! makes, through the real game frame. Every assist that would drive for it
//! is off (the realistic preset, lane keeping off, curve assist off), so
//! steering, speed and passing are all its own.
//!
//! If the picture leaves out something a driver needs, this driver misses
//! it: it runs wide in the bend, rear-ends the slow car, or gets pulled over.
//! That is how the blind-spot lights came to exist: the first version of
//! this driver passed the slow car, saw it a few hundred feet back on the
//! radar, moved over, and was billed a sideswipe, because the game counts a
//! vehicle as holding its lane well behind where it is drawn. The lights
//! read the game's own clearance check.
//!
//! Set `FREIGHT_FATE_VISUALS_SHOTS` to also save a flipbook of the drive.

use ff_core::data::world::get_world;
use ff_core::message_log::MessageLog;
use ff_core::sim::traffic_manager::TrafficVehicle;
use freight_fate::playtest::harness::{PlaytestHarness, StartDelivery};
use freight_fate::playtest::road::{find_feature_seeded, RoadOptions};
use freight_fate::visuals::{Scene, Screen, Visuals};

use crate::sighted_driver::{bend_under_truck, SightedDriver, DT};

/// What happened on a drive by sight, as the game (not the picture) saw it.
#[derive(Debug, Default)]
struct Report {
    miles: f64,
    seconds: f64,
    passes: u32,
    lanes_used: std::collections::BTreeSet<i32>,
    ended_in_lane: i64,
    over_limit_s: f64,
    off_road_s: f64,
    widest_ft: f32,
    slowest_in_bends: f64,
    closest_ft: f64,
    passed_slow_car: bool,
    pulled_over: bool,
    damage: f64,
    cargo_damage: f64,
}

/// Put a slow car in the truck's lane a little way ahead.
fn slow_car_ahead(h: &mut PlaytestHarness, ahead_mi: f64, mph: f64) {
    h.with_drive(|d, _| {
        let pos = d.trip.position_mi;
        let mut slow =
            TrafficVehicle::new("slow-car", pos + ahead_mi, mph, mph, 0, "cruising", "car");
        slow.lane = d.lane.lane;
        d.trip.traffic_manager.vehicles.push(slow);
    });
}

/// Drive by sight until `end_mi`, through the real game frame.
fn drive_by_sight(h: &mut PlaytestHarness, end_mi: f64, shots_prefix: &str) -> Report {
    let shots = std::env::var_os("FREIGHT_FATE_VISUALS_SHOTS").map(std::path::PathBuf::from);
    let mut visuals = Visuals::new();
    let log = MessageLog::new();
    let mut driver = SightedDriver::default();
    let mut report = Report {
        slowest_in_bends: f64::INFINITY,
        closest_ft: f64::INFINITY,
        ..Default::default()
    };
    let (start_mi, start_damage, start_cargo) = h.read_drive(|d| {
        (
            d.trip.position_mi,
            d.truck().damage_pct,
            d.truck().cargo_damage_pct,
        )
    });
    let mut frames = 0u32;
    let mut settled_frames = 0u32;
    let mut shot = 0;
    while frames < 60 * 60 * 10 {
        let scene = Scene::capture(&h.app.ctx);
        let Screen::Drive {
            view,
            overlay: None,
        } = &scene.screen
        else {
            panic!("the road view went away mid-drive: {:?}", scene.screen);
        };
        let events = driver.decide(view);
        for event in &events {
            h.app.handle_event(event);
        }
        h.app.frame(DT);
        frames += 1;
        report.lanes_used.insert(view.lanes.truck_lane);
        // Settled: in the lane it means to be in, for two seconds.
        if driver.target_lane == Some(view.lanes.truck_lane) {
            settled_frames += 1;
        } else {
            settled_frames = 0;
        }

        // The referee reads the game, not the picture.
        let (pos, speed, limit, off, offset, in_bend, pulled, slow_gap_ft) = h.read_drive(|d| {
            let pos = d.trip.position_mi;
            let slow = d
                .trip
                .npc_vehicles()
                .iter()
                .find(|v| v.key == "slow-car")
                .map(|v| ((v.position_mi - pos) * 5280.0, v.lane == d.lane.lane));
            (
                pos,
                d.truck().speed_mph(),
                d.enforced_limit_prev.unwrap_or(65.0),
                d.off_pavement(),
                (d.lane.offset * 6.0).abs() as f32,
                d.trip
                    .curve_at(pos)
                    .is_some_and(|c| !c.connector && c.advisory_mph < 55),
                d.pull_over.is_some(),
                slow,
            )
        });
        if speed > limit + 5.0 {
            report.over_limit_s += DT;
        }
        if off {
            report.off_road_s += DT;
        }
        if settled_frames > 120 {
            report.widest_ft = report.widest_ft.max(offset);
        }
        if in_bend {
            report.slowest_in_bends = report.slowest_in_bends.min(speed);
        }
        if let Some((gap, same_lane)) = slow_gap_ft {
            if same_lane && gap > 0.0 {
                report.closest_ft = report.closest_ft.min(gap);
            }
            report.passed_slow_car |= gap < -150.0;
        }
        report.pulled_over |= pulled;
        if let Some(dir) = &shots {
            // A frame every fifteen seconds, and every three while passing
            // or in a bend.
            let busy = view.lanes.truck_lane != 0 || bend_under_truck(view) > 1.0 / 5000.0;
            let every = if busy { 180 } else { 900 };
            if frames.is_multiple_of(every) && shot < 16 {
                std::fs::create_dir_all(dir).expect("shots dir");
                crate::visuals_frames::shoot(
                    &mut visuals,
                    scene.clone(),
                    &log,
                    (1800, 1280),
                    &dir.join(format!("{shots_prefix}_{shot:02}.bmp")),
                );
                shot += 1;
            }
        }
        if pos > end_mi {
            break;
        }
    }
    let (end_mi_now, damage, cargo, lane) = h.read_drive(|d| {
        (
            d.trip.position_mi,
            d.truck().damage_pct,
            d.truck().cargo_damage_pct,
            d.lane.lane,
        )
    });
    report.miles = end_mi_now - start_mi;
    report.seconds = frames as f64 * DT;
    report.passes = driver.passes;
    report.ended_in_lane = lane;
    report.damage = damage - start_damage;
    report.cargo_damage = cargo - start_cargo;
    println!("{shots_prefix}: {report:?}");
    report
}

/// Every assist that would drive for the player, off.
fn by_hand() -> RoadOptions {
    RoadOptions {
        assists: Some("realistic".to_string()),
        lane_keeping: Some("off".to_string()),
        curve_assist: Some(false),
        predictive_cruise: Some(false),
        descent: Some("off".to_string()),
        ..Default::default()
    }
}

fn the_driver_kept_it_clean(r: &Report) {
    assert!(!r.pulled_over, "nobody pulled the truck over");
    assert!(
        r.over_limit_s < 3.0,
        "kept to the posted limit: {:.1} s over",
        r.over_limit_s
    );
    assert!(
        r.off_road_s < 0.5,
        "stayed on the pavement: {:.1} s off",
        r.off_road_s
    );
    assert!(
        r.widest_ft < 6.0,
        "held its lane: {:.1} ft from centre at worst",
        r.widest_ft
    );
    assert!(r.damage < 1.0, "no crash: +{:.1}% truck damage", r.damage);
    assert!(
        r.cargo_damage < 1.0,
        "the load rode well: +{:.1}% cargo damage",
        r.cargo_damage
    );
}

#[test]
#[cfg_attr(ci_quick, ignore = "sweep: drives five miles of mountain road")]
fn by_sight_alone_a_driver_steers_a_mountain_bend_and_follows_a_slow_car() {
    // Denver toward Salt Lake City: a 40 mph bend four miles from the
    // staged start, on road that narrows to one lane each way.
    let opts = RoadOptions {
        feature: "curve".to_string(),
        seed: Some(7),
        trip_seed: Some(7),
        ..by_hand()
    };
    let pairs = vec![("Denver".to_string(), "Salt Lake City".to_string())];
    let hit = find_feature_seeded(get_world(), &pairs, "curve", &opts)
        .into_iter()
        .next()
        .expect("a bend on the way to Salt Lake City");
    let mut h = PlaytestHarness::new();
    h.app.ctx.settings.speed_keeper = false;
    h.app.ctx.settings.pedal_latch = "off".to_string();
    h.start_road_feature(&hit, &opts);
    h.app.ctx.settings.automatic_emergency_braking = false;
    h.app.ctx.settings.stop_and_go_assist = false;
    assert_eq!(h.app.ctx.settings.lane_keeping, "off");
    slow_car_ahead(&mut h, 0.18, 44.0);
    let (advisory, bend_end) = h.read_drive(|d| {
        let bend = d
            .trip
            .curve_at(hit.at_mi + 0.01)
            .expect("the bend the search found");
        (
            bend.advisory_mph as f64,
            bend.start_mi.max(bend.end_mi) + 0.5,
        )
    });
    let r = drive_by_sight(&mut h, bend_end, "bend");
    assert!(
        r.miles > 3.0,
        "the drive got through the bend and beyond: {r:?}"
    );
    the_driver_kept_it_clean(&r);
    assert!(
        r.slowest_in_bends <= advisory + 2.0,
        "slowed for the {advisory:.0} mph bend: {:.0} mph",
        r.slowest_in_bends
    );
    assert!(
        r.closest_ft > 80.0,
        "never closed on the slow car: {:.0} ft",
        r.closest_ft
    );
}

#[test]
#[cfg_attr(ci_quick, ignore = "sweep: drives three miles of interstate")]
fn by_sight_alone_a_driver_passes_a_slow_car_and_keeps_right() {
    let mut h = PlaytestHarness::new();
    {
        let s = &mut h.app.ctx.settings;
        let opts = by_hand();
        s.apply_driving_assistance_preset(opts.assists.as_deref().unwrap_or_default());
        s.lane_keeping = "off".to_string();
        s.curve_speed_assist = false;
        s.predictive_cruise = false;
        s.descent_speed_control = "off".to_string();
        s.speed_keeper = false;
        s.pedal_latch = "off".to_string();
        s.automatic_emergency_braking = false;
        s.stop_and_go_assist = false;
    }
    h.start_delivery(StartDelivery::named("Visual Display"));
    h.with_drive(|d, _| {
        d.departure_checked = true;
        d.trip.position_mi = 24.0;
        d.trip.traffic_manager.vehicles.clear();
    });
    h.prepare_for_driving(58.0);
    let lanes = h.read_drive(|d| d.lane.lane_count);
    assert!(
        lanes >= 2,
        "an interstate with room to pass ({lanes} lanes)"
    );
    slow_car_ahead(&mut h, 0.15, 35.0);
    let r = drive_by_sight(&mut h, 27.0, "pass");
    the_driver_kept_it_clean(&r);
    assert!(r.passes >= 1, "the slow car on screen was passed: {r:?}");
    assert!(
        r.lanes_used.contains(&1),
        "the pass used the left lane: {r:?}"
    );
    assert!(r.passed_slow_car, "the slow car fell behind: {r:?}");
    assert_eq!(
        r.ended_in_lane, 0,
        "back in the right lane after the pass: {r:?}"
    );
}
