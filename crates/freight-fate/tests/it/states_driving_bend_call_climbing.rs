//! A sharp bend's approach call is judged on the speed the truck will bring
//! to it, not the speed it has now (missing-roads pass, 2026-11-23): out of
//! the Greensboro yard cruise was taking a truck at 32 up to 45, a 30 mph
//! bend 0.15 mile ahead was never over the call speed while it was ahead,
//! and the first word of it was "too fast" with the truck already in it.

use ff_core::data::curves::RouteCurve;
use ff_core::sim::weather::WeatherKind;

use freight_fate::playtest::harness::{PlaytestHarness, RouteSetup};

use crate::states_driving_speed_keeper_sweep::corridors;
use crate::transcript_cruise_support::quiet;

const DT: f64 = 1.0 / 60.0;
const MPS_PER_MPH: f64 = 0.44704;

/// Pull away under cruise set to `set_mph` from 25 mph, a 30 mph sharp left
/// half a mile ahead, and return the truck's speed when the call was spoken.
fn call_speed_climbing_to(set_mph: f64) -> Option<f64> {
    let corridor = corridors(1).pop().expect("the world has a corridor");
    let mut harness = PlaytestHarness::new();
    harness.app.ctx.settings.time_scale = 1.0;
    harness.app.ctx.settings.automatic_transmission = true;
    harness.app.ctx.settings.curve_speed_assist = true;
    harness.start_route(
        &corridor.origin,
        &corridor.destination,
        RouteSetup::seeded(4242).named("Climbing Bend"),
    );
    let bend_mi = harness.with_drive(|d, _| {
        quiet(&mut d.trip);
        d.weather_mut().current = WeatherKind::Clear;
        d.departure_checked = true;
        d.destination_exit_taken = true;
        d.tutorial = None;
        d.trip.set_patrols(Vec::new());
        d.truck_mut().start_engine();
        d.truck_mut().transmission.automatic = true;
        d.truck_mut().set_air_ready(false);
        let half = d.trip.total_miles() * 0.5;
        d.trip.position_mi = half;
        let bend_mi = half + 0.5;
        d.trip.curves = vec![RouteCurve {
            start_mi: bend_mi,
            apex_mi: bend_mi + 0.03,
            end_mi: bend_mi + 0.06,
            direction: 'L',
            advisory_mph: 30,
            min_radius_ft: 200,
            deflection_deg: 60.0,
            connector: false,
        }];
        d.trip.announced_curves.clear();
        d.truck_mut().velocity_mps = 25.0 * MPS_PER_MPH;
        bend_mi
    });
    assert!(
        !harness.read_drive(|d| d.trip.is_facility_approach_route()),
        "a corridor that reads as a facility approach can never call a bend"
    );
    harness.with_drive(move |d, ctx| d.engage_cruise(ctx, set_mph, false));
    harness.clear_speech();
    for _ in 0..60 * 60 {
        harness.advance_frame_clock();
        let (position, speed) = harness.with_drive(|d, ctx| {
            d.update_frame(ctx, DT);
            (d.trip.position_mi, d.truck().speed_mph())
        });
        if harness
            .transcript()
            .iter()
            .any(|line| line.contains("Sharp left"))
        {
            return Some(speed);
        }
        if position >= bend_mi {
            return None;
        }
    }
    None
}

#[test]
fn a_truck_climbing_past_a_sharp_bends_number_hears_it_while_still_under() {
    let speed = call_speed_climbing_to(45.0)
        .expect("cruise taking the truck to 45 into a 30 mph bend: the bend is called");
    assert!(
        speed <= 33.0,
        "the call waited for the truck to pass the call speed: spoken at {speed:.1} mph"
    );
}

#[test]
fn a_truck_whose_cruise_stays_under_the_call_speed_hears_nothing() {
    assert_eq!(
        call_speed_climbing_to(31.0),
        None,
        "cruise set to 31 never brings the truck over a 30 mph bend's call speed"
    );
}
