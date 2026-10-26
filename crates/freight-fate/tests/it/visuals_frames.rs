//! The visual display against a real drive: what it captures from the
//! driving state, and that a frame of it draws.
//!
//! `render_frames_to_disk` is the eye test: it draws a set of real frames
//! (day, night, rain, snow, fog, a menu, a paused drive) through SDL's
//! software renderer into BMP files, so a change to the look can be seen
//! without a window. Run it by name with somewhere to put them:
//!
//! ```text
//! FREIGHT_FATE_VISUALS_SHOTS=/tmp/shots cargo test -p freight-fate --test it \
//!     visuals_frames::render_frames_to_disk -- --ignored
//! ```

use ff_core::message_log::{MessageCategory, MessageLog};
use ff_core::sim::traffic_manager::TrafficVehicle;
use ff_core::sim::weather::WeatherKind;
use freight_fate::playtest::harness::{PlaytestHarness, StartDelivery};
use freight_fate::visuals::scene_drive::{
    Biome, RoadAhead, SignKind, SignView, Terrain, WorkZone, ROAD_SAMPLES,
};
use freight_fate::visuals::{Panel, Scene, Screen, Visuals};

/// A delivery under way at highway speed, with traffic around the truck.
fn drive_with_traffic() -> PlaytestHarness {
    let mut h = PlaytestHarness::new();
    h.start_delivery(StartDelivery::named("Visual Display"));
    h.with_drive(|d, _| {
        d.departure_checked = true;
        d.trip.position_mi = 24.0;
        d.trip.truck.velocity_mps = 26.0;
        let pos = d.trip.position_mi;
        let mut ahead =
            TrafficVehicle::new("car-ahead", pos + 0.08, 64.0, 64.0, 0, "cruising", "car");
        ahead.lane = 0;
        let mut passing =
            TrafficVehicle::new("semi-left", pos + 0.02, 66.0, 66.0, 1, "passing", "semi");
        passing.lane = 1;
        let mut behind = TrafficVehicle::new(
            "box-behind",
            pos - 0.1,
            60.0,
            60.0,
            0,
            "following",
            "box truck",
        );
        behind.lane = 0;
        d.trip.traffic_manager.vehicles = vec![ahead, passing, behind];
    });
    h.drive_frames(3);
    h
}

fn drive_scene(h: &PlaytestHarness) -> Scene {
    let scene = Scene::capture(&h.app.ctx);
    assert!(
        matches!(scene.screen, Screen::Drive { overlay: None, .. }),
        "at the wheel, the road view shows"
    );
    scene
}

#[test]
fn a_real_drive_captures_its_road_traffic_and_readings() {
    let h = drive_with_traffic();
    let scene = drive_scene(&h);
    let Screen::Drive { view, .. } = &scene.screen else {
        unreachable!();
    };
    assert_eq!(view.road.curvature.len(), ROAD_SAMPLES);
    assert_eq!(view.road.grade.len(), ROAD_SAMPLES);
    assert!(view.lanes.count >= 1);
    assert!(view.hud.speed_mph > 40.0, "{}", view.hud.speed_mph);
    assert!(view.position_ft > 0.0);
    let ahead: Vec<_> = view.traffic.iter().filter(|v| v.ahead_ft > 0.0).collect();
    assert!(
        !ahead.is_empty(),
        "the car ahead is in view: {:?}",
        view.traffic
    );
    assert!(
        view.traffic.iter().any(|v| v.ahead_ft < 0.0),
        "the box truck behind is on the radar"
    );
    assert!(!view.hud.clock.is_empty());
    assert!(
        view.hud.remaining_text.ends_with("left"),
        "{}",
        view.hud.remaining_text
    );
    // Every speed-limit sign agrees with the limit enforced where it stands.
    h.read_drive(|d| {
        for sign in &view.signs {
            if let SignKind::SpeedLimit(limit) = sign.kind {
                let mile = d.trip.position_mi + sign.ahead_ft as f64 / 5280.0;
                let corridor = |m: f64| d.trip.corridor_limit_at(m).round() as u32;
                let zone_start = d.trip.zones.iter().any(|z| {
                    (z.start_mi - mile).abs() < 0.01 && z.limit_mph.round() as u32 == limit
                });
                // A drop's sign stands at its boundary, where either side's
                // limit is the one just taking effect.
                assert!(
                    limit == corridor(mile) || limit == corridor(mile + 0.02) || zone_start,
                    "a {limit} sign where the limit is {}",
                    corridor(mile)
                );
            }
        }
    });
    let mut visuals = Visuals::new();
    let log = MessageLog::new();
    assert!(visuals.build(scene, &log, (1800, 1280), false));
    for batch in visuals.batches() {
        assert!(batch
            .indices
            .iter()
            .all(|&i| (i as usize) < batch.vertices.len()));
    }
}

#[test]
fn a_menu_over_the_drive_is_an_overlay_and_freezes_the_road() {
    let mut h = drive_with_traffic();
    h.press_key(freight_fate::states::base::Key::Escape, None);
    let scene = Scene::capture(&h.app.ctx);
    let Screen::Drive {
        overlay: Some(panel),
        ..
    } = &scene.screen
    else {
        panic!("the pause menu sits over the road: {:?}", scene.screen);
    };
    assert!(panel.rows.is_some(), "the pause menu reads as a menu");
    assert!(
        !scene.animates(),
        "a paused drive is not redrawn every frame"
    );
}

/// Draw `scene` at `size` through the software renderer and save it.
pub(crate) fn shoot(
    visuals: &mut Visuals,
    scene: Scene,
    log: &MessageLog,
    size: (u32, u32),
    path: &std::path::Path,
) {
    use sdl2::pixels::PixelFormatEnum;
    use sdl2::surface::Surface;
    assert!(visuals.build(scene, log, size, true));
    let surface = Surface::new(size.0, size.1, PixelFormatEnum::ARGB8888).expect("surface");
    let mut canvas = surface.into_canvas().expect("software canvas");
    let creator = canvas.texture_creator();
    let mut atlas = freight_fate::visuals::create_atlas(&creator).expect("atlas");
    // A fresh texture: upload the whole atlas, not just this frame's new glyphs.
    visuals.invalidate_atlas();
    freight_fate::visuals::draw_frame(&mut canvas, &mut atlas, visuals).expect("draw");
    canvas.present();
    canvas.into_surface().save_bmp(path).expect("save");
}

#[test]
#[ignore = "writes images; run by name with FREIGHT_FATE_VISUALS_SHOTS set"]
fn render_frames_to_disk() {
    let Some(dir) = std::env::var_os("FREIGHT_FATE_VISUALS_SHOTS") else {
        return;
    };
    let dir = std::path::PathBuf::from(dir);
    std::fs::create_dir_all(&dir).expect("shots dir");
    let size = (1800, 1280);
    let h = drive_with_traffic();
    let base = drive_scene(&h);
    let mut log = MessageLog::new();
    log.add(
        "Speed limit drops to 55 in 2 miles.",
        MessageCategory::Event,
    );
    log.add("Car ahead, 400 feet, holding 64.", MessageCategory::Event);
    let mut visuals = Visuals::new();
    let variants: [(&str, WeatherKind, f32); 8] = [
        ("day_clear", WeatherKind::Clear, 11.0),
        ("dawn_cloudy", WeatherKind::Cloudy, 6.4),
        ("sunset_clear", WeatherKind::Clear, 19.3),
        ("night_clear", WeatherKind::Clear, 23.0),
        ("day_rain", WeatherKind::HeavyRain, 14.0),
        ("night_storm", WeatherKind::Thunderstorm, 22.0),
        ("day_snow", WeatherKind::Snow, 10.0),
        ("day_fog", WeatherKind::Fog, 8.5),
    ];
    for (name, kind, hour) in variants {
        let mut scene = base.clone();
        if let Screen::Drive { view, .. } = &mut scene.screen {
            view.weather.kind = kind;
            view.weather.hour = hour;
            view.weather.wet = matches!(kind, WeatherKind::HeavyRain | WeatherKind::Thunderstorm);
            view.weather.snow_cover = kind == WeatherKind::Snow;
            view.weather.visibility_ft = if kind == WeatherKind::Fog {
                900.0
            } else {
                52_800.0
            };
        }
        shoot(
            &mut visuals,
            scene,
            &log,
            size,
            &dir.join(format!("{name}.bmp")),
        );
    }
    // Landscapes, a bend with its signs, a work zone, a city at night.
    let with_view = |f: &dyn Fn(&mut freight_fate::visuals::DriveView)| {
        let mut scene = base.clone();
        if let Screen::Drive { view, .. } = &mut scene.screen {
            f(view);
        }
        scene
    };
    let bend = |view: &mut freight_fate::visuals::DriveView, radius: f32| {
        for (i, k) in view.road.curvature.iter_mut().enumerate() {
            let s = RoadAhead::sample_s(i);
            *k = if s > 350.0 { 1.0 / radius } else { 0.0 };
        }
    };
    let scenes = [
        (
            "desert_mountains",
            with_view(&|v| {
                v.weather.biome = Biome::Desert;
                v.weather.terrain = Terrain::Mountains;
                v.weather.hour = 16.5;
                v.weather.heading = Some(4.5);
            }),
        ),
        (
            "rockies_bend",
            with_view(&|v| {
                v.weather.biome = Biome::Conifer;
                v.weather.terrain = Terrain::Mountains;
                v.weather.hour = 10.0;
                bend(v, 1100.0);
                for (i, g) in v.road.grade.iter_mut().enumerate() {
                    *g = if RoadAhead::sample_s(i) > 600.0 {
                        0.05
                    } else {
                        0.0
                    };
                }
                v.signs = vec![
                    SignView {
                        ahead_ft: 260.0,
                        side: 1,
                        kind: SignKind::Curve {
                            left: false,
                            mph: 45,
                        },
                    },
                    SignView {
                        ahead_ft: 900.0,
                        side: -1,
                        kind: SignKind::Chevron { left: false },
                    },
                    SignView {
                        ahead_ft: 1060.0,
                        side: -1,
                        kind: SignKind::Chevron { left: false },
                    },
                    SignView {
                        ahead_ft: 1220.0,
                        side: -1,
                        kind: SignKind::Chevron { left: false },
                    },
                ];
            }),
        ),
        (
            "work_zone",
            with_view(&|v| {
                v.weather.hour = 13.0;
                v.zones = vec![WorkZone {
                    start_ft: 200.0,
                    end_ft: 2400.0,
                    closed_lane: Some(1),
                }];
                v.signs = vec![
                    SignView {
                        ahead_ft: 150.0,
                        side: 1,
                        kind: SignKind::RoadWork,
                    },
                    SignView {
                        ahead_ft: 420.0,
                        side: 1,
                        kind: SignKind::SpeedLimit(45),
                    },
                    SignView {
                        ahead_ft: 700.0,
                        side: 1,
                        kind: SignKind::Guide {
                            lines: vec!["Keep right for I-94 toward Milwaukee".to_string()],
                            distance: Some("1/2 MILE".to_string()),
                        },
                    },
                    SignView {
                        ahead_ft: 1100.0,
                        side: 1,
                        kind: SignKind::Billboard {
                            text: "Joe's Diner. Pie all day, exit 31".to_string(),
                            seed: 7,
                        },
                    },
                ];
            }),
        ),
        (
            "city_night",
            with_view(&|v| {
                v.weather.hour = 21.7;
                v.lanes.urban = true;
                v.lanes.count = 3;
                v.signs = vec![SignView {
                    ahead_ft: 500.0,
                    side: 1,
                    kind: SignKind::Service {
                        lines: vec!["TRAVEL CENTER".to_string(), "1 MILE".to_string()],
                    },
                }];
            }),
        ),
    ];
    for (name, scene) in scenes {
        shoot(
            &mut visuals,
            scene,
            &log,
            size,
            &dir.join(format!("{name}.bmp")),
        );
    }
    // A reader screen.
    let reader = Panel::from_lines(&[
        "How to play - Driving (2/9)".to_string(),
        String::new(),
        "  Up arrow is the throttle and Down arrow is the brake.".to_string(),
        "> Left and Right steer. With lane keeping on, the truck holds its lane for you."
            .to_string(),
        "  Space speaks your speed. Tab opens the status pages.".to_string(),
    ])
    .expect("reader");
    shoot(
        &mut visuals,
        Scene {
            screen: Screen::Panel(reader),
            achievement: None,
        },
        &log,
        size,
        &dir.join("reader.bmp"),
    );
    // The title menu over its backdrop.
    let menu = Panel::from_lines(&[
        "Freight Fate 1.9.0".to_string(),
        String::new(),
        "> New career".to_string(),
        "  Continue career".to_string(),
        "  How to play".to_string(),
        "  Settings".to_string(),
        "  Achievements".to_string(),
        "  Quit".to_string(),
    ])
    .expect("panel");
    shoot(
        &mut visuals,
        Scene {
            screen: Screen::Panel(menu.clone()),
            achievement: None,
        },
        &log,
        size,
        &dir.join("menu_title.bmp"),
    );
    // Paused over the drive.
    let mut paused = base.clone();
    if let Screen::Drive { overlay, .. } = &mut paused.screen {
        *overlay = Some(menu);
    }
    shoot(
        &mut visuals,
        paused,
        &log,
        size,
        &dir.join("menu_paused.bmp"),
    );
    // The window size the game opens at, without Retina.
    shoot(
        &mut visuals,
        base,
        &log,
        (900, 640),
        &dir.join("day_small.bmp"),
    );
}

/// What a frame of the road view costs the CPU: reading the drive, and
/// building the triangles. Run by name; the test profile optimizes less
/// than a release build, so the numbers are an upper bound.
#[test]
#[ignore = "timing; run by name"]
fn frame_cost() {
    let h = drive_with_traffic();
    let scene = drive_scene(&h);
    let log = MessageLog::new();
    let mut visuals = Visuals::new();
    assert!(visuals.build(scene.clone(), &log, (1800, 1280), true));
    let frames: u32 = std::env::var("FREIGHT_FATE_FRAME_COST_FRAMES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(300);
    let start = std::time::Instant::now();
    for _ in 0..frames {
        std::hint::black_box(Scene::capture(&h.app.ctx));
    }
    let capture = start.elapsed() / frames;
    let start = std::time::Instant::now();
    for _ in 0..frames {
        visuals.build(scene.clone(), &log, (1800, 1280), true);
    }
    let build = start.elapsed() / frames;
    let vertices: usize = visuals.batches().iter().map(|b| b.vertices.len()).sum();
    println!("capture {capture:?}, build {build:?}, {vertices} vertices a frame");
}
