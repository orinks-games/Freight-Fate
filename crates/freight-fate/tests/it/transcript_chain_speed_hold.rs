//! Cruise held to chain speed keeps the truck under chain speed's overspeed
//! number down a mountain grade, on the air the truck has; and "Too fast
//! for chains." speaks at each new onset (brief 4, 2026-11-23: the Donner
//! descent ran 33 to 35 mph on cruise "held to 30 for chains", and every
//! onset after the first was dropped as already said).

use ff_core::sim::chain_control::ChainSet;
use ff_core::sim::vehicle::CHAIN_OVERSPEED_MPH;
use ff_core::sim::weather::WeatherKind;
use freight_fate::playtest::harness::PlaytestHarness;

use crate::transcript_cruise_support::*;

const TOO_FAST: &str = "Too fast for chains. Keep under 30 miles per hour.";

fn too_fast_count(harness: &PlaytestHarness) -> usize {
    harness
        .app
        .event_lines()
        .iter()
        .filter(|line| *line == TOO_FAST)
        .count()
}

/// A mountain descent the way Donner's runs: `grade` percent downhill a
/// mile at a time, broken by a short 2.5 percent pull, so a climb is always
/// inside cruise's preview. The pull is what made cruise bank speed past
/// chain speed.
fn rolling_descent(grade: f64) -> Vec<(f64, f64, f64)> {
    let mut segments = Vec::new();
    let mut mi = 0.0;
    while mi < BENCH_MILES {
        segments.push((mi, mi + 1.0, grade));
        segments.push((mi + 1.0, mi + 1.3, 2.5));
        mi += 1.3;
    }
    segments
}

/// Chained on cruise (asked for 55, held to 30) down [`rolling_descent`]
/// under `weather`, with `cargo_kg` aboard.
fn chained_descent(name: &str, grade: f64, weather: WeatherKind, cargo_kg: f64) -> PlaytestHarness {
    let road = rolling_descent(grade);
    let mut harness = cruising(name, 30.0, 55.0, &road);
    // The relaxed pace a career drives at (the agent server's default).
    harness.with_drive(move |d, _| {
        let at = d.trip.position_mi;
        bench_road_segments(d, &[(0.0, 55.0)], &road, 10.0);
        d.trip.position_mi = at;
    });
    harness.app.ctx.settings.time_scale = 10.0;
    harness.app.ctx.settings.imperial_units = true;
    harness.app.ctx.settings.destination_approach_assist = false;
    harness.with_drive(move |d, ctx| {
        d.weather_mut().current = weather;
        d.weather_mut().forced = Some(weather);
        let t = d.truck_mut();
        t.chains_on = true;
        t.chain_wear_pct = 0.0;
        t.cargo_kg = cargo_kg;
        d.chain_set = ChainSet::from_truck(true, weather == WeatherKind::Snow);
        d.engage_cruise(ctx, 55.0, false);
    });
    harness.clear_speech();
    harness
}

#[test]
fn cruise_holds_a_chained_truck_under_chain_speed_down_a_mountain_grade() {
    // Empty, as on the Donner drive, and loaded.
    for grade in [-3.0, -4.5, -6.7] {
        for (weather, cargo_kg) in [
            (WeatherKind::Snow, 0.0),
            (WeatherKind::Cloudy, 0.0),
            (WeatherKind::Snow, 18_000.0),
            (WeatherKind::Cloudy, 18_000.0),
        ] {
            let mut harness = chained_descent("Chain descent", grade, weather, cargo_kg);
            let start = harness.read_drive(|d| d.trip.position_mi);
            let (mut top, mut low_air, mut snubs, mut was_snubbing) = (0.0f64, f64::MAX, 0, false);
            while harness.read_drive(|d| d.trip.position_mi) - start < 4.0 {
                frame(&mut harness, DT);
                let (mph, air, snubbing, holding) = harness.read_drive(|d| {
                    (
                        d.truck().speed_mph(),
                        d.truck().air_pressure_psi(),
                        d.cruise_snubbing,
                        d.truck().air_brakes_holding(),
                    )
                });
                assert!(
                    !holding,
                    "{weather:?}: the spring brakes came on at {air:.0} psi"
                );
                top = top.max(mph);
                low_air = low_air.min(air);
                if snubbing && !was_snubbing {
                    snubs += 1;
                }
                was_snubbing = snubbing;
            }
            let (cruise, low_warning) =
                harness.read_drive(|d| (d.cruise_mph, d.truck().specs.air_low_warning_psi));
            assert_eq!(
                cruise,
                Some(30.0),
                "{weather:?}: still on cruise at chain speed"
            );
            assert!(
                top <= CHAIN_OVERSPEED_MPH,
                "{grade} {weather:?} {cargo_kg}: top {top:.2} mph over {CHAIN_OVERSPEED_MPH}"
            );
            assert!(
                low_air > low_warning + 20.0,
                "{weather:?}: air fell to {low_air:.1} psi ({snubs} snubs)"
            );
            assert_eq!(
                too_fast_count(&harness),
                0,
                "{weather:?}: {:?}",
                spoken(&harness)
            );
            // The base rate: 0.2 percent a mile on snow; bare road stays inside
            // its 2 mile grace for half the run, then wears at the bare rate.
            if weather == WeatherKind::Snow {
                let wear = harness.read_drive(|d| d.truck().chain_wear_pct);
                assert!(wear < 0.9, "snow: {wear:.2} percent over 4 miles");
            }
            eprintln!("{grade} {weather:?} {cargo_kg} kg: top {top:.2} mph, {snubs} snubs, air low {low_air:.1} psi");
        }
    }
}

#[test]
fn too_fast_for_chains_speaks_at_each_new_onset() {
    let mut harness = bench_drive("Chain onsets", 55.0, 0.0);
    harness.app.ctx.settings.imperial_units = true;
    harness.with_drive(|d, _| {
        d.weather_mut().current = WeatherKind::Snow;
        d.weather_mut().forced = Some(WeatherKind::Snow);
        d.truck_mut().chains_on = true;
        d.chain_set = ChainSet::from_truck(true, true);
    });
    harness.clear_speech();
    // By foot: 36, back to 28, 36 again; and a dip to 31 between, which is
    // not back at chain speed and so not a new onset. Five seconds a step,
    // past the voice's own few-second repeat window.
    for (mph, want) in [(36.0, 1), (31.0, 1), (36.0, 1), (28.0, 1), (36.0, 2)] {
        for _ in 0..300 {
            harness.with_drive(move |d, _| d.truck_mut().velocity_mps = mph * MPS_PER_MPH);
            frame(&mut harness, DT);
        }
        harness.app.ctx.run_deferred();
        assert_eq!(
            too_fast_count(&harness),
            want,
            "at {mph}: {:?}",
            harness.app.event_lines()
        );
    }
}
