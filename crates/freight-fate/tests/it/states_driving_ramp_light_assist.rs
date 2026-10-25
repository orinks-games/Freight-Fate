//! The player on the All assists preset at a ramp that ends at a light: the
//! assist that stopped the truck for the red takes it on when it turns
//! green, as facility stopping assistance promises.

use freight_fate::playtest::harness::{PlaytestHarness, RouteSetup};
use freight_fate::states::driving::DrivingState;

const DT: f64 = 1.0 / 60.0;

/// Agent drive (2026-10-25): on Cleveland's exit 170A the truck stopped 400
/// feet short of the ramp light and sat there through every green after it,
/// with every assist on and nobody at the pedals.
#[test]
#[cfg_attr(ci_quick, ignore = "sweep: drives the last miles of a 345-mile load")]
fn all_assists_take_the_truck_on_from_a_ramp_light_it_stopped_short_of() {
    let mut harness = PlaytestHarness::new();
    harness.start_route(
        "chicago_il_us",
        "cleveland_oh_us",
        RouteSetup {
            trip_seed: Some(11),
            ..RouteSetup::default()
        },
    );
    harness.with_drive(|d, ctx| {
        ctx.settings.apply_driving_assistance_preset("all");
        ctx.settings.facility_streets = true;
        ctx.settings.time_scale = 10.0;
        d.tutorial = None;
        d.departure_checked = true;
        d.trip.hazard_check_mi = 1e9;
        d.trip.inspection_check_mi = 1e9;
        d.trip.traffic_manager.rolling_bubble = true;
        let total = d.trip.total_miles();
        d.trip.position_mi = total - 9.0;
        d.truck_mut().start_engine();
        d.truck_mut().set_air_ready(false);
        d.trip.truck.velocity_mps = 26.0;
        d.engage_cruise(ctx, 60.0, false);
    });
    let mut stopped_on_ramp = 0;
    let mut longest = 0;
    let mut finished = false;
    for _ in 0..(60 * 60 * 40) {
        harness.advance_frame_clock();
        harness.app.frame(DT);
        if !harness.state_is::<DrivingState>() {
            finished = true;
            break;
        }
        let (on_ramp, mph, done) =
            harness.read_drive(|d| (d.ramp_mi.is_some(), d.truck().speed_mph(), d.trip.finished));
        if done {
            finished = true;
            break;
        }
        if on_ramp && mph < 0.5 {
            stopped_on_ramp += 1;
            longest = longest.max(stopped_on_ramp);
        } else {
            stopped_on_ramp = 0;
        }
        if stopped_on_ramp >= 60 * 60 {
            break;
        }
    }
    assert!(
        longest < 60 * 60,
        "parked on the exit ramp for a minute of frames with every assist on\n{:#?}",
        harness.app.event_lines()
    );
    assert!(
        finished || harness.read_drive(|d| d.ramp_mi.is_none()),
        "never left the ramp"
    );
    // The assist drives the truck up to the light, so it never says it let go
    // partway down the ramp (owner, 2026-10-25).
    let released: Vec<String> = harness
        .transcript()
        .into_iter()
        .filter(|line| line.contains("Route-transition assistance released"))
        .collect();
    assert!(released.is_empty(), "{released:?}");
}
