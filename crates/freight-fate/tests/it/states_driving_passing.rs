//! Lane keeping on full passes a slow vehicle the hazard call names, and
//! comes back to the right lane once it is behind (owner ruling,
//! 2026-10-01). The pass itself is pinned beside the call it replaces, in
//! `transcript_driving_cruise_weather_hazards`; this is the rest of it.

use ff_core::sim::traffic_manager::TrafficVehicle;
use ff_core::sim::trip_models::{
    OpenSide, RoadStop, TrafficContext, TripEvent, TripEventData, TripEventKind,
};
use ff_core::speech_text::{in_lane_hazard_call, passing_hazard_call};
use freight_fate::playtest::harness::PlaytestHarness;

use crate::transcript_cruise_support::*;
use crate::transcript_driving_cruise_weather_hazards::{meet_a_slow_car, the_call};

#[test]
fn after_the_pass_lane_keeping_returns_right_once_the_car_is_behind() {
    let mut harness = meet_a_slow_car("Pass Return", "full", 2, 0, &[], false);
    assert!(the_call(&harness).ends_with("Passing on the left."));

    // Over in the left lane, and held there while the car it is passing is
    // still ahead of or beside the cab.
    let mut elapsed = 0.0;
    while harness.read_drive(|d| d.lane.lane) == 0 && elapsed < 5.0 {
        frame(&mut harness, DT);
        elapsed += DT;
    }
    assert_eq!(harness.read_drive(|d| d.lane.lane), 1);
    let behind_by = |h: &PlaytestHarness| {
        h.read_drive(|d| {
            let car = d
                .trip
                .traffic_manager
                .vehicles
                .iter()
                .find(|v| v.key == "bench:lead")
                .map_or(f64::INFINITY, |v| v.position_mi);
            d.trip.position_mi - car
        })
    };
    while harness.read_drive(|d| d.lane.lane == 1 && d.lane_change_target.is_none())
        && elapsed < 180.0
    {
        frame(&mut harness, DT);
        elapsed += DT;
    }
    assert!(
        behind_by(&harness) > 0.0,
        "moved back in ahead of the car it was passing\n{}",
        harness.transcript_text()
    );
    assert_eq!(harness.read_drive(|d| d.lane_change_target), Some(0));

    while harness.read_drive(|d| d.lane.lane) != 0 && elapsed < 190.0 {
        frame(&mut harness, DT);
        elapsed += DT;
    }
    assert_eq!(harness.read_drive(|d| d.lane.lane), 0);
    assert!(harness.read_drive(|d| d.passing.is_none()));
    assert!(
        said_any(&harness, "In the right lane."),
        "{:#?}",
        spoken(&harness)
    );
    assert!(!said_any(&harness, "sideswiped"), "{:#?}", spoken(&harness));
    // The lane it comes back to is not announced as open first: the truck
    // takes it, and the arrival says so.
    assert!(
        !said_any(&harness, "Right lane open."),
        "{:#?}",
        spoken(&harness)
    );
}

/// The slow-car call as the trip raises it with the left lane open.
fn slow_car_call(at_mi: f64) -> TripEvent {
    let lead = TrafficVehicle::new(
        "bench:raised",
        at_mi + 0.02,
        45.0,
        45.0,
        0,
        "following",
        "car",
    )
    .with_lane(0);
    TripEvent {
        kind: TripEventKind::Hazard,
        message: in_lane_hazard_call("Slow car right ahead.", OpenSide::Left),
        data: TripEventData {
            deadline_s: Some(2.5),
            traffic: Some(TrafficContext {
                lead,
                gap_mi: 0.02,
                closing_mph: 20.0,
            }),
            dodgeable: Some(true),
            in_lane: Some(true),
            open_side: Some(OpenSide::Left),
            pass_message: passing_hazard_call("Slow car right ahead.", OpenSide::Left),
            name: Some("the slow car".to_string()),
            ..Default::default()
        },
    }
}

#[test]
fn lining_up_for_an_exit_the_call_stays_the_drivers() {
    // Inside the two miles where lane keeping is heading for the right lane
    // for its own exit, it does not pull out to pass: the call is the one a
    // driver answers, and braking is what the assist does about it. A semi
    // alongside keeps the rig's own call from passing first.
    let mut harness = meet_a_slow_car("Pass Near Exit", "full", 2, 0, &[1], false);
    let raise = |harness: &mut PlaytestHarness, exit_ahead: Option<f64>| {
        harness.clear_speech();
        harness.with_drive(move |d, ctx| {
            d.hazard_deadline = None;
            let at = d.trip.position_mi;
            d.exit_stop = exit_ahead
                .map(|ahead| RoadStop::new("Test Receiver", at + ahead, "delivery_destination"));
            d.handle_trip_event(ctx, &slow_car_call(at));
        });
    };

    raise(&mut harness, Some(1.5));
    assert_eq!(
        the_call(&harness),
        "Change lanes or brake! Slow car right ahead. Left lane open."
    );
    assert!(harness.read_drive(|d| d.lane_change_target).is_none());

    // The same call with the exit still five miles off is passed.
    raise(&mut harness, Some(5.0));
    assert_eq!(
        the_call(&harness),
        "Slow car right ahead. Passing on the left."
    );
    assert_eq!(harness.read_drive(|d| d.lane_change_target), Some(1));
}

/// A two-lane road with a passing lane one way, the slow car ahead, lane
/// keeping on full, driven in `forward` (the leg's own direction) or back.
///
/// OSM's `lanes:forward=2` names the way's drawing direction, not the
/// leg's, so the map cannot say whose passing lane it is. Read as ours, the
/// truck was given a second lane on about half of these stretches and full
/// lane keeping pulled out to pass into what was really the oncoming
/// passing lane (player report, 2026-10-13: "the cruise control swerves you
/// right into oncoming cars").
fn meet_a_slow_car_beside_a_passing_lane(forward: bool) -> PlaytestHarness {
    use ff_core::data::world_models::{
        CorridorDetail, GradeSegment, LaneSegment, Leg, Route, SpeedLimitSample,
    };
    use ff_core::sim::lane::MPH_PER_MPS;
    use ff_core::sim::trip_models::TRAFFIC_WARNING_GAP_S;
    use ff_core::sim::weather::WeatherKind;
    use freight_fate::playtest::harness::RouteSetup;

    let mut harness = PlaytestHarness::new();
    harness.app.ctx.settings.speed_keeper = false;
    harness.app.ctx.settings.automatic_emergency_braking = true;
    harness.app.ctx.settings.lane_keeping = "full".to_string();
    harness.app.ctx.settings.time_scale = 1.0;
    harness.start_route(
        "aberdeen_sd_us",
        "pierre_sd_us",
        RouteSetup::seeded(7).named("Passing Lane"),
    );
    harness.with_drive(move |d, ctx| {
        let a = d.trip.route.cities[0].clone();
        let b = d.trip.route.cities.last().cloned().expect("a destination");
        let detail = CorridorDetail {
            speed_limits: vec![SpeedLimitSample {
                at_mi: 0.0,
                mph: Some(65.0),
                source: "test bench".to_string(),
                hgv: false,
            }],
            grade_segments: vec![GradeSegment::new(0.0, 200.0, 0.0, "flat", "test bench")],
            lane_segments: vec![LaneSegment {
                start_mi: 0.0,
                end_mi: 200.0,
                lanes: 3,
                lanes_forward: 2,
                lanes_backward: 1,
                oneway: false,
                source: "test bench".to_string(),
            }],
            ..Default::default()
        };
        let mut leg = Leg::new(&a, &b, 200.0, "US-83", "hills", Vec::new()).with_detail(detail);
        leg.divided = Some(false);
        leg.lanes = 1;
        let cities = if forward { vec![a, b] } else { vec![b, a] };
        d.trip.route = Route::from_legs(cities, vec![leg]);
        d.trip
            .traffic_manager
            .set_route(&d.trip.route, &d.trip.leg_starts);
        d.reset_turn_state_for_trip();
        d.trip.zones.clear();
        d.trip.curves.clear();
        d.destination_exit_taken = true;
        d.trip.time_scale = 1.0;
        d.trip.hazard_check_mi = 1e9;
        d.trip.inspection_check_mi = 1e9;
        d.trip.posts.clear();
        d.weather_mut().current = WeatherKind::Clear;
        d.departure_checked = true;
        d.truck_mut().start_engine();
        d.truck_mut().transmission.automatic = true;
        d.truck_mut().set_air_ready(false);
        d.trip.position_mi = 20.0;
        d.truck_mut().velocity_mps = 65.0 / MPH_PER_MPS;
        d.trip.traffic_manager.rolling_bubble = false;
        d.speed_control_armed = true;
        d.cruise_mph = Some(65.0);
        if let Some(profile) = ctx.profile.as_mut() {
            profile.tutorial_done = true;
        }
        d.tutorial = None;
        let position = d.trip.position_mi;
        let gap_mi = 45.0 * (TRAFFIC_WARNING_GAP_S - 0.4) / 3600.0;
        d.trip.set_npc_vehicles(vec![TrafficVehicle::new(
            "bench:lead",
            position + gap_mi,
            45.0,
            45.0,
            0,
            "following",
            "car",
        )
        .with_lane(0)]);
    });
    harness.clear_speech();
    release_keys(&mut harness);
    for _ in 0..120 {
        frame(&mut harness, DT);
        if said_any(&harness, "Slow car right ahead") {
            return harness;
        }
    }
    panic!("no lead warning was spoken: {:#?}", spoken(&harness));
}

#[test]
fn lane_keeping_never_passes_into_a_passing_lane_that_may_be_oncoming() {
    for forward in [true, false] {
        let mut harness = meet_a_slow_car_beside_a_passing_lane(forward);
        assert!(
            !the_call(&harness).contains("Passing"),
            "driving {}: {}",
            if forward { "forward" } else { "back" },
            the_call(&harness)
        );
        assert_eq!(harness.read_drive(|d| d.lane.lane_count), 1);
        for _ in 0..300 {
            frame(&mut harness, DT);
            assert_eq!(
                harness.read_drive(|d| (d.lane.lane, d.lane_change_target)),
                (0, None),
                "{:#?}",
                spoken(&harness)
            );
        }
    }
}

/// Frames until lane keeping has started moving the truck to `target`.
fn until_moving_to(harness: &mut PlaytestHarness, target: i64, limit_s: f64) {
    let mut elapsed = 0.0;
    while harness.read_drive(|d| d.lane_change_target) != Some(target) {
        assert!(
            elapsed < limit_s,
            "never started for lane {target}: {:#?}",
            spoken(harness)
        );
        frame(harness, DT);
        elapsed += DT;
    }
}

#[test]
fn a_vehicle_merging_in_while_the_truck_moves_back_calls_the_move_off() {
    // The right lane was clear when the move back began, and a box truck off
    // the ramp took it in the seconds the move takes. Lane keeping used to
    // finish the move anyway, into the side of it (player report,
    // 2026-10-13: "it crashed my truck into a box truck that entered the
    // right lane").
    let mut harness = meet_a_slow_car("Merge On Return", "full", 2, 0, &[], false);
    until_moving_to(&mut harness, 1, 5.0);
    while harness.read_drive(|d| d.lane.lane) != 1 {
        frame(&mut harness, DT);
    }
    until_moving_to(&mut harness, 0, 180.0);
    harness.with_drive(|d, _| {
        // Beside the trailer, on the ramp.
        let at = d.trip.position_mi - 0.01;
        let mut vehicles = d.trip.traffic_manager.vehicles.clone();
        vehicles.push(
            TrafficVehicle::new("bench:merger", at, 66.0, 66.0, 2, "merging", "box truck")
                .with_lane(-1),
        );
        d.trip.set_npc_vehicles(vehicles);
    });
    for _ in 0..(6.0 / DT) as usize {
        frame(&mut harness, DT);
    }
    assert!(!said_any(&harness, "sideswiped"), "{:#?}", spoken(&harness));
    assert!(
        said_any(&harness, "Staying in the left lane."),
        "{:#?}",
        spoken(&harness)
    );
    // Still passing: it moves back once the box truck is clear.
    assert_eq!(
        harness.read_drive(|d| (d.lane.lane, d.passing)),
        (1, Some((0, 1)))
    );
}

#[test]
fn a_passing_lane_that_ends_mid_move_leaves_the_truck_where_it_was() {
    // Two lanes became one while lane keeping was pulling out to pass. The
    // move landed in the only lane left, the one the slow car is in, and was
    // billed as a sideswipe of the car being passed (player report,
    // 2026-10-13: "it happens when two lanes become a single lane").
    let mut harness = meet_a_slow_car("Lane Ends Mid Pass", "full", 2, 0, &[], false);
    until_moving_to(&mut harness, 1, 5.0);
    harness.with_drive(|d, _| {
        let vehicles = d.trip.traffic_manager.vehicles.clone();
        crate::states_driving_traffic_rate::lane_bench(d, 65.0, 1);
        d.trip.set_npc_vehicles(vehicles);
    });
    for _ in 0..(4.0 / DT) as usize {
        frame(&mut harness, DT);
    }
    assert!(!said_any(&harness, "sideswiped"), "{:#?}", spoken(&harness));
    assert!(
        said_any(&harness, "The road narrows to one lane."),
        "{:#?}",
        spoken(&harness)
    );
    assert_eq!(
        harness.read_drive(|d| (d.lane.lane, d.lane_change_target, d.passing)),
        (0, None, None)
    );
}

/// The box truck from the merge-on-return report, off the ramp at 66 mph
/// `behind_mi` back from the cab while lane keeping is moving the truck back.
fn merge_a_box_truck_on_the_return(behind_mi: f64) -> PlaytestHarness {
    let mut harness = meet_a_slow_car("Pacer On Return", "full", 2, 0, &[], false);
    until_moving_to(&mut harness, 1, 5.0);
    while harness.read_drive(|d| d.lane.lane) != 1 {
        frame(&mut harness, DT);
    }
    until_moving_to(&mut harness, 0, 180.0);
    harness.with_drive(move |d, _| {
        let at = d.trip.position_mi - behind_mi;
        let mut vehicles = d.trip.traffic_manager.vehicles.clone();
        vehicles.push(
            TrafficVehicle::new("bench:merger", at, 66.0, 66.0, 2, "merging", "box truck")
                .with_lane(-1),
        );
        d.trip.set_npc_vehicles(vehicles);
    });
    harness
}

const KEEP_RIGHT_LINES: [&str; 2] = [
    "Keep right except to pass.",
    "Traffic is stacking up and passing you on the right.",
];

/// Frames until the truck is back in the right lane, at most `limit_s`,
/// failing on any keep-right CB line on the way. Returns the seconds taken
/// and whether the truck was ever held under 58 mph.
fn back_to_the_right_lane(harness: &mut PlaytestHarness, limit_s: f64) -> (f64, bool) {
    let mut eased = false;
    let mut elapsed = 0.0;
    while harness.read_drive(|d| d.lane.lane) != 0 {
        assert!(
            elapsed < limit_s,
            "not back in the right lane after {limit_s} s\n{}",
            harness.transcript_text()
        );
        frame(harness, DT);
        elapsed += DT;
        eased |= harness.read_drive(|d| d.trip.truck.speed_mph() < 58.0);
        for line in KEEP_RIGHT_LINES {
            assert!(!said_any(harness, line), "{:#?}", spoken(harness));
        }
    }
    (elapsed, eased)
}

fn box_truck_ahead_by(harness: &PlaytestHarness) -> f64 {
    harness.read_drive(|d| {
        d.trip
            .traffic_manager
            .vehicles
            .iter()
            .find(|v| v.key == "bench:merger")
            .map_or(f64::INFINITY, |v| v.position_mi - d.trip.position_mi)
    })
}

#[test]
fn a_box_truck_merging_far_back_does_not_hold_the_move_back() {
    // The QA repro: the box truck merges at 66 mph a third of a mile back
    // while the truck moves back at 65. Read at the quarter-mile footprint
    // every traffic vehicle carries it was alongside, and the truck waited
    // 260 s in the left lane. At its own length it is a third of a mile
    // back and closing at 1 mph, and the move back carries on.
    let mut harness = merge_a_box_truck_on_the_return(0.34);
    let (took, _) = back_to_the_right_lane(&mut harness, 60.0);
    assert!(took < 60.0);
    for _ in 0..(20.0 / DT) as usize {
        frame(&mut harness, DT);
    }
    let said = spoken(&harness);
    assert!(said_any(&harness, "In the right lane."), "{said:#?}");
    assert!(!said_any(&harness, "sideswiped"), "{said:#?}");
    for line in KEEP_RIGHT_LINES {
        assert!(!said_any(&harness, line), "{said:#?}");
    }
}

#[test]
fn a_box_truck_pacing_the_held_move_back_is_let_by_and_followed_in() {
    // A box truck alongside at 66 held the move back of a truck cruising
    // at 65, and lane keeping waited in the left lane while the CB told the
    // driver off (QA drive, 2026-10-13). Lane keeping now eases off
    // without a word, lets the box truck by and drops in three seconds behind it,
    // inside a minute, and the left lane is excused while it waits.
    let mut harness = merge_a_box_truck_on_the_return(0.01);
    let (took, eased) = back_to_the_right_lane(&mut harness, 60.0);
    assert!(took < 60.0, "{took}");
    assert!(eased, "held the left lane at speed beside the box truck");
    assert!(
        box_truck_ahead_by(&harness) > 0.0,
        "moved back in ahead of it"
    );
    assert_eq!(harness.read_drive(|d| d.passing), None);
    for _ in 0..(20.0 / DT) as usize {
        frame(&mut harness, DT);
    }
    let said = spoken(&harness);
    assert_eq!(
        said.iter()
            .filter(|l| *l == "Staying in the left lane.")
            .count(),
        1,
        "{said:#?}"
    );
    assert!(!said_any(&harness, "Easing off"), "{said:#?}");
    assert!(said_any(&harness, "In the right lane."), "{said:#?}");
    assert!(!said_any(&harness, "sideswiped"), "{said:#?}");
    for line in KEEP_RIGHT_LINES {
        assert!(!said_any(&harness, line), "{said:#?}");
    }
    // Back at the set speed once it is in behind.
    assert!(harness.read_drive(|d| d.pass_return_ease_mph.is_none()));
}

#[test]
fn the_left_lane_is_excused_only_for_a_while_after_a_called_off_move() {
    // The excuse covers lane keeping's own hold, not the driver who takes
    // over and stays out there: with lane keeping off after the call, the
    // CB is back once the window and the usual nag time have run.
    let mut harness = merge_a_box_truck_on_the_return(0.01);
    while !said_any(&harness, "Staying in the left lane.") {
        frame(&mut harness, DT);
    }
    assert!(harness.read_drive(|d| d.keep_right_excused_s > 0.0));
    harness.app.ctx.settings.lane_keeping = "off".to_string();
    harness.with_drive(|d, _| {
        d.passing = None;
        let vehicles: Vec<TrafficVehicle> = Vec::new();
        d.trip.set_npc_vehicles(vehicles);
    });
    for _ in 0..(150.0 / DT) as usize {
        frame(&mut harness, DT);
    }
    assert!(
        said_any(&harness, KEEP_RIGHT_LINES[0]),
        "{:#?}",
        spoken(&harness)
    );
}
#[test]
fn a_passing_lane_ending_mid_move_is_announced_once() {
    // Three lanes became two while lane keeping was pulling out to pass.
    // The narrowing was said, cut by the automatic braking that answered the
    // slow car, and handed back to be said again (QA drive, 2026-10-13:
    // "The road narrows to two lanes." twice, about eight seconds apart).
    let mut harness = meet_a_slow_car("Three To Two", "full", 3, 1, &[], false);
    until_moving_to(&mut harness, 2, 5.0);
    harness.with_drive(|d, _| {
        let vehicles = d.trip.traffic_manager.vehicles.clone();
        crate::states_driving_traffic_rate::lane_bench(d, 65.0, 2);
        d.trip.set_npc_vehicles(vehicles);
    });
    for _ in 0..(30.0 / DT) as usize {
        frame(&mut harness, DT);
    }
    let said = spoken(&harness);
    assert_eq!(
        said.iter()
            .filter(|l| *l == "The road narrows to two lanes.")
            .count(),
        1,
        "{said:#?}"
    );
    assert!(!said_any(&harness, "sideswiped"), "{said:#?}");
    assert_eq!(
        harness.read_drive(|d| (d.lane.lane, d.lane_change_target)),
        (1, None)
    );
}

/// Issue #356: lane keeping was still passing when the driver tapped back
/// right, and the truck moved into the side of what it was passing. On
/// full, a tap waits for the lane: it says what is in it and stays put.
#[test]
fn a_tap_back_into_the_vehicle_being_passed_waits_for_the_lane() {
    let mut harness = meet_a_slow_car("Tap Back", "full", 2, 0, &[], false);
    let mut elapsed = 0.0;
    while harness.read_drive(|d| d.lane.lane) == 0 && elapsed < 5.0 {
        frame(&mut harness, DT);
        elapsed += DT;
    }
    assert_eq!(harness.read_drive(|d| d.lane.lane), 1);
    harness.with_drive(|d, ctx| d.tap_lane_change(ctx, -1));
    assert!(
        said_any(&harness, "Right lane blocked by a"),
        "{:#?}",
        spoken(&harness)
    );
    assert_eq!(harness.read_drive(|d| d.lane_change_target), None);
    for _ in 0..120 {
        frame(&mut harness, DT);
    }
    assert!(!said_any(&harness, "sideswiped"), "{:#?}", spoken(&harness));
}
