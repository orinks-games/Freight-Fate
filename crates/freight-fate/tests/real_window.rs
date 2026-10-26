//! The visual display in a real window: the game's own window and renderer
//! (Metal on a Mac), its own event loop and frame, played in real time by
//! the sighted driver (`it/sighted_driver.rs`), which reads only what the
//! display captures and presses only the player's keys. Every assist that
//! would drive for it is off.
//!
//! Not part of `cargo test`: it needs a desktop, and takes about two
//! minutes. Run it by name with a scratch data directory, and optionally a
//! directory for captures of the window:
//!
//! ```text
//! FREIGHT_FATE_DATA_DIR=/tmp/ff-scratch FREIGHT_FATE_WINDOW_SHOTS=/tmp/shots \
//!     cargo test -p freight-fate --test real_window
//! ```
//!
//! The game runs as a background app, so the window never takes focus and
//! typing elsewhere cannot reach it, and it keeps drawing while covered
//! (`FREIGHT_FATE_ALWAYS_DRAW`), so a capture by window id shows the live
//! frame. Sound and speech are muted.

#[path = "it/sighted_driver.rs"]
mod sighted_driver;

use std::time::{Duration, Instant};

use ff_core::data::world::get_world;
use ff_core::sim::traffic_manager::TrafficVehicle;
use freight_fate::app::App;
use freight_fate::playtest::road::{build_driving, find_feature_seeded, RoadOptions};
use freight_fate::states::driving::DrivingState;
use freight_fate::visuals::{Scene, Screen};

use sighted_driver::{SightedDriver, DT};

fn with_drive<R>(app: &App, f: impl FnOnce(&DrivingState) -> R) -> R {
    let state = app.state().expect("a screen");
    let state = state.borrow();
    f(state
        .as_any()
        .downcast_ref::<DrivingState>()
        .expect("the drive is on top"))
}

fn main() {
    let Some(data_dir) = std::env::var_os("FREIGHT_FATE_DATA_DIR") else {
        eprintln!("real_window: set FREIGHT_FATE_DATA_DIR to a scratch directory; not running");
        return;
    };
    // Muted, and nothing live: a test process may not use the network (the
    // game's guard refuses it), so live fuel prices and the rest stay off.
    let settings = std::path::Path::new(&data_dir).join("settings.json");
    if !settings.exists() {
        std::fs::create_dir_all(&data_dir).expect("scratch data dir");
        std::fs::write(
            &settings,
            r#"{"master_volume": 0.0, "speech_volume": 0.0, "online_services": false,
                "discord_presence": false, "real_fuel_prices": false, "real_weather": false,
                "real_traffic": false, "real_parking": false, "assist_preset_chosen": true}"#,
        )
        .expect("scratch settings");
    }
    std::env::set_var("SDL_MAC_BACKGROUND_APP", "1");
    std::env::set_var("FREIGHT_FATE_ALWAYS_DRAW", "1");
    let shots = std::env::var_os("FREIGHT_FATE_WINDOW_SHOTS").map(std::path::PathBuf::from);

    let mut app = App::new().expect("the game window");
    {
        let s = &mut app.ctx.settings;
        s.master_volume = 0.0;
        s.speech_volume = 0.0;
        s.speed_keeper = false;
        s.pedal_latch = "off".to_string();
        s.visual_display = true;
    }
    app.ctx.apply_volumes();
    app.ctx.apply_speech();
    let opts = RoadOptions {
        feature: "curve".to_string(),
        seed: Some(7),
        trip_seed: Some(7),
        assists: Some("realistic".to_string()),
        lane_keeping: Some("off".to_string()),
        curve_assist: Some(false),
        predictive_cruise: Some(false),
        descent: Some("off".to_string()),
        ..Default::default()
    };
    let pairs = vec![("Denver".to_string(), "Salt Lake City".to_string())];
    let hit = find_feature_seeded(get_world(), &pairs, "curve", &opts)
        .into_iter()
        .next()
        .expect("a bend on the way to Salt Lake City");
    let (mut drive, start_mi) = build_driving(&mut app.ctx, &hit, &opts);
    app.ctx.settings.automatic_emergency_braking = false;
    app.ctx.settings.stop_and_go_assist = false;
    // A slow car ahead, to be seen and followed.
    let pos = drive.trip.position_mi;
    let mut slow = TrafficVehicle::new("slow-car", pos + 0.18, 44.0, 44.0, 0, "cruising", "car");
    slow.lane = drive.lane.lane;
    drive.trip.traffic_manager.vehicles.push(slow);
    let bend_end = {
        let bend = drive.trip.curve_at(hit.at_mi + 0.01).expect("the bend");
        bend.start_mi.max(bend.end_mi) + 0.5
    };
    let advisory = drive
        .trip
        .curve_at(hit.at_mi + 0.01)
        .map_or(0, |b| b.advisory_mph) as f64;
    app.push_state(drive);
    let window = window_number();
    println!("real_window: drive staged at mile {start_mi:.1}; window {window:?}");

    let mut driver = SightedDriver::default();
    let frame = Duration::from_secs_f64(DT);
    let started = Instant::now();
    let mut next_frame = Instant::now();
    let mut next_shot = Duration::from_secs(3);
    let (mut frames, mut shot) = (0u32, 0u32);
    let (mut over_s, mut off_s, mut slowest) = (0.0f64, 0.0f64, f64::INFINITY);
    let (start_damage, start_cargo) =
        with_drive(&app, |d| (d.truck().damage_pct, d.truck().cargo_damage_pct));
    loop {
        let scene = Scene::capture(&app.ctx);
        let Screen::Drive {
            view,
            overlay: None,
        } = &scene.screen
        else {
            panic!("the road view went away mid-drive: {:?}", scene.screen);
        };
        for event in driver.decide(view) {
            app.handle_event(&event);
        }
        // The real frame: the window's events, the drive, then drawing it.
        app.frame(DT);
        frames += 1;
        let (pos, speed, limit, off, in_bend) = with_drive(&app, |d| {
            (
                d.trip.position_mi,
                d.truck().speed_mph(),
                d.enforced_limit_prev.unwrap_or(65.0),
                d.off_pavement(),
                d.trip
                    .curve_at(d.trip.position_mi)
                    .is_some_and(|c| !c.connector && c.advisory_mph < 55),
            )
        });
        if speed > limit + 5.0 {
            over_s += DT;
        }
        if off {
            off_s += DT;
        }
        if in_bend {
            slowest = slowest.min(speed);
        }
        let elapsed = started.elapsed();
        if let (Some(dir), Some(id)) = (&shots, window) {
            if elapsed >= next_shot && shot < 24 {
                std::fs::create_dir_all(dir).expect("shots dir");
                let path = dir.join(format!("window_{shot:02}.png"));
                let ok = std::process::Command::new("screencapture")
                    .args(["-x", "-o", "-t", "png", &format!("-l{id}")])
                    .arg(&path)
                    .status()
                    .is_ok_and(|s| s.success());
                println!(
                    "real_window: {:5.1} s, mile {pos:.2}, {speed:.0} mph, capture {}",
                    elapsed.as_secs_f64(),
                    if ok {
                        path.display().to_string()
                    } else {
                        "failed".to_string()
                    }
                );
                shot += 1;
                next_shot += Duration::from_secs(if in_bend { 2 } else { 6 });
            }
        }
        if pos > bend_end || elapsed > Duration::from_secs(300) {
            break;
        }
        // Real time: one frame every sixtieth of a second, as the game runs.
        next_frame += frame;
        let now = Instant::now();
        if next_frame > now {
            std::thread::sleep(next_frame - now);
        } else {
            next_frame = now;
        }
    }
    let (end_mi, damage, cargo, widest) = with_drive(&app, |d| {
        (
            d.trip.position_mi,
            d.truck().damage_pct - start_damage,
            d.truck().cargo_damage_pct - start_cargo,
            d.lane.offset.abs() * 6.0,
        )
    });
    println!(
        "real_window: drove {:.2} mi in {:.0} s ({frames} frames): slowest in the {advisory:.0} mph \
         bend {slowest:.0} mph, over the limit {over_s:.1} s, off the pavement {off_s:.1} s, \
         {widest:.1} ft off centre at the end, damage +{damage:.1}% truck +{cargo:.1}% cargo",
        end_mi - start_mi,
        started.elapsed().as_secs_f64(),
    );
    app.shutdown();
    assert!(end_mi > bend_end, "got through the bend");
    assert!(slowest <= advisory + 2.0, "slowed for the bend");
    assert!(
        over_s < 3.0 && off_s < 0.5,
        "kept to the limit and the pavement"
    );
    assert!(damage < 1.0 && cargo < 1.0, "no crash");
    println!("real_window: ok");
}

/// The game window's macOS window number, for `screencapture -l`.
#[cfg(target_os = "macos")]
fn window_number() -> Option<isize> {
    use objc2_app_kit::NSWindow;
    use sdl2::sys::{
        SDL_GetVersion, SDL_GetWindowFromID, SDL_GetWindowWMInfo, SDL_SysWMinfo, SDL_bool,
        SDL_SYSWM_TYPE,
    };

    // SAFETY: SDL owns the window for the life of the app above; the info
    // struct is plain C data, zeroed then filled by SDL; on Cocoa its union
    // begins with the NSWindow pointer, which SDL holds while the window
    // lives. This runs on the main thread that created the window.
    unsafe {
        for id in 1..8 {
            let window = SDL_GetWindowFromID(id);
            if window.is_null() {
                continue;
            }
            let mut info: SDL_SysWMinfo = std::mem::zeroed();
            SDL_GetVersion(&mut info.version);
            if SDL_GetWindowWMInfo(window, &mut info) != SDL_bool::SDL_TRUE
                || info.subsystem != SDL_SYSWM_TYPE::SDL_SYSWM_COCOA
            {
                continue;
            }
            let ns_window =
                std::ptr::read_unaligned(info.info.dummy.as_ptr().cast::<*const NSWindow>());
            if let Some(ns_window) = ns_window.as_ref() {
                return Some(ns_window.windowNumber());
            }
        }
    }
    None
}

#[cfg(not(target_os = "macos"))]
fn window_number() -> Option<isize> {
    None
}
