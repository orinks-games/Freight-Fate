//! Chains used the way the game asks cost what the base rate says (owner
//! report, 2026-11-23: one clean use at Donner cost 19 percent of a set).
//! Driven through the real speed control: cruise held to chain speed on
//! snow, then the bare-road line, then the stop or the miles after it.

use ff_core::sim::chain_control::ChainSet;
use ff_core::sim::weather::WeatherKind;
use freight_fate::playtest::harness::PlaytestHarness;
use freight_fate::states::base::Key;

use crate::transcript_cruise_support::*;

/// A truck chained on snow at 30 mph, cruise asked for 55 (held to 30 for
/// chains), the player at the wheel with Facility stopping off.
fn chained_on_cruise(name: &str) -> PlaytestHarness {
    let mut harness = cruising(name, 30.0, 55.0, &[(0.0, BENCH_MILES, 0.0)]);
    harness.app.ctx.settings.destination_approach_assist = false;
    harness.with_drive(|d, ctx| {
        d.weather_mut().current = WeatherKind::Snow;
        let t = d.truck_mut();
        t.chains_on = true;
        t.chain_wear_pct = 0.0;
        d.chain_set = ChainSet::from_truck(true, true);
        // The driver asks for 55; chains hold it to 30.
        d.engage_cruise(ctx, 55.0, false);
    });
    harness
}

fn mile(harness: &PlaytestHarness) -> f64 {
    harness.read_drive(|d| d.trip.position_mi)
}

fn wear(harness: &PlaytestHarness) -> f64 {
    harness.read_drive(|d| d.truck().chain_wear_pct)
}

/// Frames until `done`, at most `limit`.
fn roll_until(
    harness: &mut PlaytestHarness,
    limit: usize,
    done: impl Fn(&PlaytestHarness) -> bool,
) {
    for _ in 0..limit {
        if done(harness) {
            return;
        }
        frame(harness, DT);
    }
    let state = harness.read_drive(|d| {
        format!(
            "mile {:.2}, {:.1} mph, cruise {:?}, engine {}, chains {}, surface {}",
            d.trip.position_mi,
            d.truck().speed_mph(),
            d.cruise_mph,
            d.truck().engine_on,
            d.truck().chains_on,
            d.truck().surface
        )
    });
    panic!("never got there: {state}");
}

/// Three miles of snow at the cruise cap, then bare road to the line.
fn to_the_bare_road_line(harness: &mut PlaytestHarness) -> f64 {
    let snow_end = START_MI + 3.0;
    roll_until(harness, 60 * 60 * 10, |h| mile(h) >= snow_end);
    let snow_wear = wear(harness);
    assert_eq!(
        harness.read_drive(|d| d.cruise_mph),
        Some(30.0),
        "cruise held to chain speed"
    );
    harness.with_drive(|d, _| d.weather_mut().current = WeatherKind::Cloudy);
    harness.clear_speech();
    roll_until(harness, 60 * 60 * 10, |h| {
        said_any(h, "Bare road. Stop and take the chains off.")
    });
    snow_wear
}

#[test]
fn a_clean_use_of_chains_costs_under_two_percent() {
    let mut harness = chained_on_cruise("Chain budget");
    let snow_wear = to_the_bare_road_line(&mut harness);
    // Three miles of snow at the cap: the base rate, 0.2 percent a mile.
    assert!(snow_wear < 0.7, "snow: {snow_wear}");
    // The driver brakes to a stop where the line says.
    harness.with_drive(|d, ctx| d.cancel_cruise(ctx, false));
    hold(&mut harness, &[Key::Down]);
    roll_until(&mut harness, 60 * 60 * 5, |h| {
        h.read_drive(|d| d.truck().speed_mph()) < 0.5
    });
    release_keys(&mut harness);
    let total = wear(&harness);
    assert!(total < 2.0, "set on to set off: {total}");
    assert!(harness.read_drive(|d| d.truck().chains_on));
}

#[test]
fn chains_left_on_bare_road_still_slap_and_snap_later() {
    let mut harness = chained_on_cruise("Chain ignored");
    to_the_bare_road_line(&mut harness);
    let bare_from = harness.read_drive(|d| d.trip.position_mi - d.truck().chain_bare_mi);
    roll_until(&mut harness, 60 * 60 * 60, |h| {
        said_any(
            h,
            "A chain is slapping the fender. Stop soon and tighten it.",
        )
    });
    let slap_at = mile(&harness) - bare_from;
    roll_until(&mut harness, 60 * 60 * 60, |h| {
        said_any(h, "A tire chain broke and is flailing.")
    });
    let snap_at = mile(&harness) - bare_from;
    assert!(
        (slap_at - 12.6).abs() < 0.4,
        "slap {slap_at} miles into bare road"
    );
    assert!(
        (snap_at - 14.45).abs() < 0.4,
        "snap {snap_at} miles into bare road"
    );
    // The snap line ends with the truck's damage (issue 533). On the 2.0
    // map lane keeping drops out on snow, so the unsteered bench truck can
    // pick up shoulder damage first: read the figure off the truck.
    let damage = harness.read_drive(|d| d.truck().damage_pct);
    assert!(said_any(&harness, &format!("Damage {damage:.0} percent.")));
}
