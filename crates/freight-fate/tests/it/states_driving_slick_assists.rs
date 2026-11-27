//! Cruise, lane keeping and the traction lines on ice and snow (2.0 alpha
//! freezing-rain report, 2026-11-27): the truck reaches the weather's safe
//! speed, stays on the road with the wheel in the assist's hands, and the
//! ABS and wheelspin lines speak only for the driver's own foot.

use ff_core::sim::lane_keep_status::LaneKeepAvailability;
use ff_core::sim::vehicle::DriveTractionState;
use ff_core::sim::weather::WeatherKind;
use freight_fate::playtest::harness::{PlaytestHarness, RouteSetup};
use freight_fate::states::base::Key;

use crate::transcript_cruise_support::*;

const ABS_LINE: &str = "ABS pulsing. Hold steady on the brake.";
const SPIN_LINE: &str = "Drive wheels spinning. Ease off the throttle.";

fn count(harness: &PlaytestHarness, line: &str) -> usize {
    spoken(harness)
        .iter()
        .filter(|said| said.contains(line))
        .count()
}

fn speed(harness: &PlaytestHarness) -> f64 {
    harness.read_drive(|d| d.truck().speed_mph())
}

fn force(harness: &mut PlaytestHarness, kind: WeatherKind) {
    harness.with_drive(move |d, _| {
        d.trip.weather.forced = Some(kind);
        d.weather_mut().current = kind;
    });
}

/// A loaded truck on the level bench at 60 with cruise set there.
fn cruising_bench(name: &str, grade_pct: f64) -> PlaytestHarness {
    let mut harness = bench_drive(name, 65.0, grade_pct);
    release_keys(&mut harness);
    press(&mut harness, Key::E, None);
    harness.with_drive(|d, _| {
        d.truck_mut().transmission.gear = 10;
        d.truck_mut().velocity_mps = 60.0 * MPS_PER_MPH;
        d.truck_mut().throttle = 0.35;
    });
    press(&mut harness, Key::K, None);
    assert!(harness.read_drive(|d| d.cruise_mph).is_some(), "cruise set");
    frames(&mut harness, 60 * 3, DT);
    harness
}

#[test]
fn cruise_reaches_the_ice_speed_inside_forty_seconds_without_pedal_lines() {
    let mut harness = cruising_bench("Slick Cruise Ice", 0.0);
    force(&mut harness, WeatherKind::Ice);
    let mut reached = None;
    for second in 1..=60 {
        frames(&mut harness, 60, DT);
        if speed(&harness) <= 25.0 {
            reached = Some(second);
            break;
        }
    }
    let reached = reached.unwrap_or_else(|| {
        panic!(
            "still {:.0} mph after a minute\n{}",
            speed(&harness),
            spoken(&harness).join("\n")
        )
    });
    assert!(reached < 40, "25 mph after {reached} seconds");
    frames(&mut harness, 60 * 20, DT);
    assert_eq!(count(&harness, ABS_LINE), 0, "{:?}", spoken(&harness));
    assert_eq!(count(&harness, SPIN_LINE), 0, "{:?}", spoken(&harness));
}

#[test]
fn cruise_holds_twenty_on_an_icy_downgrade() {
    // Already down near the ice's number when the grade starts to tell: from
    // 60, a 4 percent downgrade on ice takes the truck's whole service brake
    // a minute and a half to shed (physics, not the assist).
    let mut harness = cruising_bench("Slick Cruise Downgrade", -4.0);
    force(&mut harness, WeatherKind::Ice);
    harness.with_drive(|d, _| d.truck_mut().velocity_mps = 24.0 * MPS_PER_MPH);
    let mut trace = Vec::new();
    for _ in 0..40 {
        frames(&mut harness, 60, DT);
        trace.push(format!("{:.1}", speed(&harness)));
    }
    let mut worst: f64 = 0.0;
    for _ in 0..30 {
        frames(&mut harness, 60, DT);
        worst = worst.max((speed(&harness) - 20.0).abs());
        trace.push(format!("{:.1}", speed(&harness)));
    }
    assert!(
        worst <= 3.0,
        "{worst:.1} mph off 20 at worst; speeds by second {}\n{}",
        trace.join(" "),
        spoken(&harness).join("\n")
    );
    assert_eq!(count(&harness, ABS_LINE), 0, "{:?}", spoken(&harness));
}

#[test]
fn the_drivers_own_full_press_on_ice_speaks_abs_once_per_press() {
    let mut harness = bench_drive("Slick Press Ice", 65.0, 0.0);
    release_keys(&mut harness);
    press(&mut harness, Key::E, None);
    harness.with_drive(|d, _| {
        d.truck_mut().transmission.gear = 10;
        d.truck_mut().velocity_mps = 45.0 * MPS_PER_MPH;
    });
    force(&mut harness, WeatherKind::Ice);
    frames(&mut harness, 30, DT);
    for presses in 1..=2 {
        hold(&mut harness, &[Key::Down]);
        frames(&mut harness, 60 * 3, DT);
        release_keys(&mut harness);
        frames(&mut harness, 60, DT);
        assert_eq!(
            count(&harness, ABS_LINE),
            presses,
            "{:.0} mph\n{}",
            speed(&harness),
            spoken(&harness).join("\n")
        );
    }
}

fn hands_off_all_assists(name: &str, kind: WeatherKind) {
    let mut harness = PlaytestHarness::new();
    harness.start_route(
        "chicago_il_us",
        "madison_wi_us",
        RouteSetup::default().named(name),
    );
    harness.prepare_for_driving(60.0);
    harness
        .app
        .ctx
        .settings
        .apply_driving_assistance_preset("all");
    harness.with_drive(|d, _| {
        d.trip.truck.velocity_mps = 60.0 * MPS_PER_MPH;
        d.trip.position_mi = 20.0;
        d.lane.lane = 0;
        d.lane.lane_count = 2;
        d.trip.traffic_manager.vehicles.clear();
    });
    press(&mut harness, Key::K, None);
    frames(&mut harness, 60 * 2, DT);
    force(&mut harness, kind);
    for _ in 0..60 {
        frames(&mut harness, 60, DT);
        let (off, damage, lane_keep) = harness.read_drive(|d| {
            (
                d.off_pavement(),
                d.trip.truck.damage_pct,
                d.lane_keep_availability,
            )
        });
        assert!(!off, "off the pavement\n{}", spoken(&harness).join("\n"));
        assert_eq!(damage, 0.0, "{}", spoken(&harness).join("\n"));
        assert_eq!(lane_keep, LaneKeepAvailability::Active);
    }
    assert_eq!(count(&harness, ABS_LINE), 0, "{:?}", spoken(&harness));
    assert_eq!(count(&harness, SPIN_LINE), 0, "{:?}", spoken(&harness));
}

#[test]
fn all_assists_hands_off_stays_on_the_road_in_freezing_rain() {
    hands_off_all_assists("Slick Hands Off Ice", WeatherKind::Ice);
}

#[test]
fn all_assists_hands_off_stays_on_the_road_in_snow() {
    hands_off_all_assists("Slick Hands Off Snow", WeatherKind::Snow);
}

#[test]
fn backing_on_ice_by_the_drivers_own_key_speaks_the_wheelspin_line_once() {
    // Backing in the automatic, the held brake key is the throttle, so wheels
    // spinning under it are the driver's to ease off.
    let mut harness = bench_drive("Slick Backing Ice", 65.0, 0.0);
    release_keys(&mut harness);
    press(&mut harness, Key::E, None);
    harness.with_drive(|d, _| {
        d.truck_mut().transmission.automatic = true;
        d.truck_mut().velocity_mps = 0.0;
    });
    force(&mut harness, WeatherKind::Ice);
    harness.app.ctx.playtest_drive_traction = Some(DriveTractionState::Spinning);
    hold(&mut harness, &[Key::Down]);
    frames(&mut harness, 60 * 5, DT);
    let backing = harness.read_drive(|d| d.truck().transmission.in_reverse());
    assert!(backing, "the held brake key put the automatic in reverse");
    assert_eq!(
        count(&harness, SPIN_LINE),
        1,
        "{}",
        spoken(&harness).join("\n")
    );
}

#[test]
fn cruise_with_the_drive_wheels_spinning_says_nothing() {
    let mut harness = cruising_bench("Slick Cruise Spin", 0.0);
    force(&mut harness, WeatherKind::Ice);
    harness.app.ctx.playtest_drive_traction = Some(DriveTractionState::Spinning);
    frames(&mut harness, 60 * 5, DT);
    assert!(
        harness.read_drive(|d| d.cruise_mph).is_some(),
        "cruise still set"
    );
    assert_eq!(count(&harness, SPIN_LINE), 0, "{:?}", spoken(&harness));
}
