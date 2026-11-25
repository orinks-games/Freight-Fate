//! A chain law follows the road (player report, 2026-11-21): the sign call,
//! "Chain law lifted." when the snow ends, the sign again when it returns,
//! the bare-road line, and putting chains on with the sky held.

use ff_core::sim::chain_control::{ChainControl, ChainSet};
use ff_core::sim::weather::WeatherKind;

use freight_fate::playtest::harness::{PlaytestHarness, StartDelivery};
use freight_fate::states::driving::DrivingState;

const DT: f64 = 1.0 / 60.0;
const SIGN: &str = "Chain law, level 1";
const LIFTED: &str = "Chain law lifted.";
const BARE: &str = "Bare road. Stop and take the chains off.";

fn a_drive() -> PlaytestHarness {
    let mut harness = PlaytestHarness::new();
    harness.start_delivery(StartDelivery::named("Chain Road"));
    harness.with_drive(|drive, ctx| {
        ctx.settings.destination_approach_assist = false;
        drive.tutorial = None;
        drive.departure_checked = true;
        drive.trip.hazard_check_mi = 1e9;
        drive.trip.inspection_check_mi = 1e9;
        drive.trip.traffic_manager.rolling_bubble = false;
        drive.trip.set_npc_vehicles(Vec::new());
        drive.trip.traffic_pressures.clear();
        drive.trip.zones.clear();
        drive.trip.set_patrols(Vec::new());
        drive.trip.posts.clear();
        drive.trip.chain_law_areas = vec![(2.0, 30.0)];
        drive.trip.position_mi = 5.0;
        drive.trip.truck.velocity_mps = 11.0;
        // Chained, so the entry warning stays out of these cases.
        drive.trip.truck.chains_on = true;
        drive.chain_set = ChainSet::from_truck(true, true);
        sky(drive, WeatherKind::Snow);
    });
    harness.clear_speech();
    harness
}

fn sky(drive: &mut DrivingState, kind: WeatherKind) {
    drive.trip.weather.forced = Some(kind);
}

fn frames(harness: &mut PlaytestHarness, n: usize) {
    for _ in 0..n {
        harness.advance_clock(DT);
        harness.with_drive(|drive, ctx| {
            // Hold a steady chain speed: these cases are about the road.
            drive.trip.truck.velocity_mps = 11.0;
            drive.trip.truck.throttle = 0.0;
            drive.trip.truck.brake = 0.0;
            drive.update_frame(ctx, DT);
        });
    }
}

fn count(harness: &PlaytestHarness, needle: &str) -> usize {
    harness
        .app
        .event_lines()
        .iter()
        .filter(|line| line.contains(needle))
        .count()
}

#[test]
fn test_the_law_lifts_once_when_the_snow_ends_and_posts_again_when_it_returns() {
    let mut harness = a_drive();
    frames(&mut harness, 600);
    assert_eq!(count(&harness, SIGN), 1, "{:?}", harness.app.event_lines());
    harness.with_drive(|d, _| sky(d, WeatherKind::Cloudy));
    frames(&mut harness, 600);
    assert_eq!(
        count(&harness, LIFTED),
        1,
        "{:?}",
        harness.app.event_lines()
    );
    assert_eq!(
        harness.with_drive(|d, _| d.trip.chain_control),
        ChainControl::Clear
    );
    harness.with_drive(|d, _| sky(d, WeatherKind::Snow));
    frames(&mut harness, 600);
    assert_eq!(count(&harness, SIGN), 2, "{:?}", harness.app.event_lines());
    assert_eq!(count(&harness, LIFTED), 1);
}

#[test]
fn test_the_law_lifts_aloud_for_a_stopped_truck_too() {
    let mut harness = a_drive();
    frames(&mut harness, 600);
    harness.with_drive(|d, _| sky(d, WeatherKind::Cloudy));
    for _ in 0..600 {
        harness.advance_clock(DT);
        harness.with_drive(|drive, ctx| {
            drive.trip.truck.velocity_mps = 0.0;
            drive.trip.truck.set_parking_brake();
            drive.update_frame(ctx, DT);
        });
    }
    assert_eq!(
        count(&harness, LIFTED),
        1,
        "{:?}",
        harness.app.event_lines()
    );
}

#[test]
fn test_no_chain_call_under_cloud_whatever_the_alerts() {
    let mut harness = a_drive();
    harness.with_drive(|d, _| {
        sky(d, WeatherKind::Cloudy);
        d.trip.weather.current = WeatherKind::Cloudy;
        d.trip.live_alerts = vec![ff_core::sim::real_weather_alerts::WeatherAlert {
            id: "watch".to_string(),
            event: "Winter Storm Watch".to_string(),
            severity: "Moderate".to_string(),
            headline: String::new(),
            area: String::new(),
            description: String::new(),
        }];
    });
    frames(&mut harness, 600);
    let lines = harness.app.event_lines();
    assert!(
        !lines.iter().any(|l| l.to_lowercase().contains("chain")),
        "{lines:?}"
    );
    assert_eq!(harness.with_drive(|d, _| d.trip.chain_law_level()), 0);
}

#[test]
fn test_chains_on_bare_road_speak_once_after_half_a_mile() {
    let mut harness = a_drive();
    harness.with_drive(|d, _| {
        sky(d, WeatherKind::Cloudy);
        d.trip.weather.current = WeatherKind::Cloudy;
        d.chain_set = ChainSet::from_truck(true, false);
    });
    let start = harness.with_drive(|d, _| d.trip.position_mi);
    let mut heard_at = None;
    for _ in 0..4000 {
        frames(&mut harness, 1);
        if heard_at.is_none() && count(&harness, BARE) > 0 {
            heard_at = Some(harness.with_drive(|d, _| d.trip.position_mi));
        }
        if harness.with_drive(|d, _| d.trip.position_mi) - start > 1.5 {
            break;
        }
    }
    let at = heard_at.expect("the bare-road line") - start;
    assert!((0.5..0.6).contains(&at), "heard after {at} miles");
    assert_eq!(count(&harness, BARE), 1, "{:?}", harness.app.event_lines());
}

#[test]
fn test_putting_chains_on_in_simulated_snow_leaves_it_snowing() {
    // Before the fix the chain-up billed its minutes to the sky's change
    // timer, and the snow often ended during the work (18 to 38 percent of
    // chain-ups in a 500-seed sweep).
    for seed in 0..40 {
        let mut harness = a_drive();
        let still_snow = harness.with_drive(|d, ctx| {
            d.trip.weather.forced = None;
            d.trip.weather.current = WeatherKind::Snow;
            d.trip.weather.minutes_until_change = 5.0 + seed as f64;
            d.trip.truck.velocity_mps = 0.0;
            ctx.profile
                .as_mut()
                .expect("a career")
                .set_chains_owned(true);
            d.do_install_chains(ctx).expect("stopped, so it works");
            d.trip.weather.current
        });
        assert_eq!(still_snow, WeatherKind::Snow, "seed {seed}");
    }
}
