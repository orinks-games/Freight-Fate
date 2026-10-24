//! The co-driver and the road's hazards: what a driver still has to handle
//! on the All assists preset, handled with the co-driver at the wheel.

use ff_core::sim::traffic_manager::TrafficVehicle;
use ff_core::sim::trip_models::Zone;
use ff_core::sim::weather::WeatherKind;
use freight_fate::playtest::harness::{PlaytestHarness, RouteSetup, StartDelivery};
use freight_fate::states::driving::DrivingState;
use freight_fate::states::driving_team::TeamMode;

const DT: f64 = 1.0 / 60.0;

/// A quiet road: no traffic, hazards, inspections, patrols, zones or
/// weather, so the case reads the co-driver and nothing else.
fn quiet_drive(name: &str) -> PlaytestHarness {
    let mut harness = PlaytestHarness::new();
    harness.start_delivery(StartDelivery::named(name));
    harness.with_drive(|d, _| {
        d.tutorial = None;
        d.departure_checked = true;
        d.trip.hazard_check_mi = 1e9;
        d.trip.inspection_check_mi = 1e9;
        d.trip.traffic_manager.rolling_bubble = false;
        d.trip.set_npc_vehicles(Vec::new());
        d.trip.traffic_pressures.clear();
        d.trip.zones.clear();
        d.trip.weather.current = WeatherKind::Clear;
        d.trip.set_patrols(Vec::new());
        d.trip.posts.clear();
    });
    harness
}

fn run_until(
    harness: &mut PlaytestHarness,
    max_frames: usize,
    mut done: impl FnMut(&PlaytestHarness) -> bool,
) -> bool {
    for _ in 0..max_frames {
        harness.advance_frame_clock();
        harness.app.frame(DT);
        if done(harness) {
            return true;
        }
    }
    false
}

/// The co-driver at the wheel, cruising on a road with two lanes or more.
fn cruising(name: &str) -> PlaytestHarness {
    let mut harness = quiet_drive(name);
    harness.with_drive(|d, ctx| d.hand_wheel_to_co_driver(ctx, TeamMode::RideAlong));
    let ok = run_until(&mut harness, 60 * 60 * 30, |h| {
        h.read_drive(|d| {
            d.truck().speed_mph() > 45.0
                && d.ramp_mi.is_none()
                && d.lane.lane_count >= 2
                && (d.cruise_mph.is_some() || d.keeper_mph.is_some())
        })
    });
    assert!(ok, "the co-driver never got cruising on a multi-lane road");
    harness
}

/// A work zone `ahead_mi` on, taper first, closing the truck's own lane.
/// Returns (taper start, zone end).
fn closure_on_the_trucks_lane(d: &mut DrivingState, ahead_mi: f64) -> (f64, f64) {
    let side = if d.lane.lane == 0 { "right" } else { "left" };
    let taper = d.trip.position_mi + ahead_mi;
    let start = taper + 1.0;
    let end = start + 2.0;
    d.trip
        .zones
        .push(Zone::new(taper, start, 55.0, "construction merge").with_closed_side(Some(side)));
    d.trip
        .zones
        .push(Zone::new(start, end, 45.0, "construction").with_closed_side(Some(side)));
    (taper, end)
}

fn assert_clean_through(harness: &PlaytestHarness) {
    let (damage, fines, at_wheel) = harness.read_drive(|d| {
        (
            d.truck().damage_pct,
            d.ticket_fines_paid,
            d.team.as_ref().is_some_and(|team| team.wheel.is_some()),
        )
    });
    let lines = harness.transcript();
    let tail = lines[lines.len().saturating_sub(20)..].join("\n");
    assert_eq!(damage, 0.0, "the truck took damage:\n{tail}");
    assert_eq!(fines, 0.0, "a ticket was written:\n{tail}");
    assert!(at_wheel, "the co-driver handed back:\n{tail}");
}

#[test]
fn the_co_driver_moves_out_of_a_closing_lane_before_the_taper() {
    let mut harness = cruising("Team Early Merge");
    let (taper, end) = harness.with_drive(|d, _| closure_on_the_trucks_lane(d, 1.5));
    let closing = harness.read_drive(|d| d.lane.lane);
    assert!(run_until(&mut harness, 60 * 60 * 5, |h| {
        h.read_drive(|d| d.trip.position_mi >= taper)
    }));
    assert_ne!(
        harness.read_drive(|d| d.lane.lane),
        closing,
        "still in the closing lane at the taper"
    );
    assert!(run_until(&mut harness, 60 * 60 * 10, |h| {
        h.read_drive(|d| d.trip.position_mi > end + 0.2)
    }));
    assert_clean_through(&harness);
}

#[test]
fn boxed_in_beside_traffic_the_co_driver_drops_back_and_merges_behind_it() {
    let mut harness = cruising("Team Boxed Merge");
    let (_, end) = harness.with_drive(|d, _| {
        // A truck keeping pace in the open lane, just behind the cab, holds
        // the gap shut all the way to the barrels unless the co-driver drops
        // back. They used to ride the closed lane into them.
        let lane = d.lane.lane;
        let open = if lane == 0 { 1 } else { lane - 1 };
        let mph = d.truck().speed_mph();
        let beside = TrafficVehicle::new(
            "team:beside",
            d.trip.position_mi - 0.005,
            mph,
            mph,
            open - lane,
            "cruising",
            "truck",
        )
        .with_lane(open);
        d.trip.traffic_manager.vehicles = vec![beside];
        closure_on_the_trucks_lane(d, 0.5)
    });
    assert!(run_until(&mut harness, 60 * 60 * 10, |h| {
        h.read_drive(|d| d.trip.position_mi > end + 0.2)
    }));
    assert_clean_through(&harness);
}

#[test]
fn from_the_left_lane_the_co_driver_leaves_a_left_closure() {
    let mut harness = cruising("Team Left Closure");
    let (_, end) = harness.with_drive(|d, _| {
        d.lane.lane = d.lane.lane_count - 1;
        closure_on_the_trucks_lane(d, 1.0)
    });
    assert!(run_until(&mut harness, 60 * 60 * 10, |h| {
        h.read_drive(|d| d.trip.position_mi > end + 0.2)
    }));
    assert_clean_through(&harness);
}

/// Chicago to Cleveland, seed 11: the stop for the red light at the end of
/// the receiver's exit ramp left the truck a fifth of a mile short of the
/// line, and the co-driver sat there through every green after it without
/// handing back, because the assist brakes for a light but never drives up
/// to one.
#[test]
#[cfg_attr(ci_quick, ignore = "sweep: drives a 345-mile load")]
fn the_co_driver_drives_up_to_a_ramp_light_the_assist_stopped_short_of() {
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
        d.tutorial = None;
        d.departure_checked = true;
        d.trip.hazard_check_mi = 2.0;
        d.trip.traffic_manager.rolling_bubble = true;
        d.hand_wheel_to_co_driver(ctx, TeamMode::RideAlong);
    });
    let mut stopped_on_ramp = 0;
    let arrived = run_until(&mut harness, 60 * 60 * 60, |h| {
        if !h.state_is::<DrivingState>() {
            return true;
        }
        let (on_ramp, mph, finished) =
            h.read_drive(|d| (d.ramp_mi.is_some(), d.truck().speed_mph(), d.trip.finished));
        if on_ramp && mph < 0.5 {
            stopped_on_ramp += 1;
        } else {
            stopped_on_ramp = 0;
        }
        // A minute of frames: ten minutes of the game's clock at this
        // drive's time scale, longer than any light's red.
        assert!(
            stopped_on_ramp < 60 * 60,
            "parked on the exit ramp with the co-driver at the wheel"
        );
        finished
    });
    assert!(arrived, "the load never reached the receiver");
}

/// A heavy truck on a long downgrade at the hands of the co-driver, cruise
/// set at 60 on a 65 road for `seconds`. Returns the fastest the truck went,
/// whether descent control ever said it could not hold the grade, and
/// whether the co-driver braked for it.
fn co_driver_descent(grade_pct: f64, brake_temp_c: f64, seconds: f64) -> (f64, bool, bool) {
    use crate::transcript_cruise_support::{
        bench_road_segments, release_keys, start_drive_scaled, MPS_PER_MPH, START_MI,
    };
    let mut harness = start_drive_scaled("Team Descent", Some(10.0));
    release_keys(&mut harness);
    harness.with_drive(move |d, ctx| {
        let grades = [
            (0.0, START_MI + 0.2, 0.0),
            (START_MI + 0.2, 400.0, grade_pct),
        ];
        bench_road_segments(d, &[(0.0, 65.0)], &grades, 10.0);
        d.trip.position_mi = START_MI;
        d.tutorial = None;
        d.trip.hazard_check_mi = 1e9;
        d.trip.inspection_check_mi = 1e9;
        d.trip.traffic_manager.rolling_bubble = false;
        d.trip.set_npc_vehicles(Vec::new());
        d.trip.set_patrols(Vec::new());
        d.weather_mut().forced = Some(WeatherKind::Clear);
        d.weather_mut().current = WeatherKind::Clear;
        d.truck_mut().cargo_kg = 25_000.0;
        d.truck_mut().start_engine();
        d.truck_mut().set_air_ready(false);
        d.truck_mut().brake_temp_c = brake_temp_c;
        d.truck_mut().brake_wear_pct = 60.0;
        d.truck_mut().velocity_mps = 60.0 * MPS_PER_MPH;
        d.hand_wheel_to_co_driver(ctx, TeamMode::RideAlong);
        d.engage_cruise(ctx, 60.0, false);
    });
    let (mut fastest, mut beaten, mut braked) = (0.0f64, false, false);
    for _ in 0..(seconds / DT) as usize {
        harness.advance_frame_clock();
        harness.app.frame(DT);
        harness.read_drive(|d| {
            fastest = fastest.max(d.truck().speed_mph());
            beaten |= d.descent_limit_state == "grade";
            braked |= d
                .team
                .as_ref()
                .and_then(|team| team.wheel.as_ref())
                .is_some_and(|wheel| wheel.pilot.descent_snub_to.is_some());
        });
    }
    (fastest, beaten, braked)
}

#[test]
fn the_co_driver_brakes_for_a_grade_descent_control_cannot_hold() {
    // Hot drums on 8 percent: descent control alone let the truck run on to
    // 82 and said it could not hold the grade, and the co-driver drove on.
    let (fastest, beaten, braked) = co_driver_descent(-8.0, 500.0, 60.0);
    assert!(braked, "the co-driver never braked for the grade");
    assert!(
        !beaten,
        "descent control had to say it could not hold the grade"
    );
    assert!(fastest < 75.0, "the truck ran away to {fastest:.0}");
}

#[test]
fn on_a_grade_descent_control_holds_the_co_driver_leaves_the_brakes_alone() {
    let (fastest, beaten, braked) = co_driver_descent(-7.0, 200.0, 60.0);
    assert!(!beaten && !braked, "braked on a grade the assist held");
    assert!(
        fastest <= 61.0,
        "the truck gained on a held grade: {fastest:.0}"
    );
}
