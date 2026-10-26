//! What "dodgeable" means, and what it is not allowed to decide.
//!
//! Owner, 2026-08-24: traffic forces the truck to brake on a road with
//! nowhere to go, and there the response should be firmer than where there is
//! somewhere to go. The words already told the two roads apart -- one lane
//! gets "Brake!", a road with an open lane gets "Change lanes or brake!" --
//! and the physics did not. Worse than undifferentiated: every lead-vehicle
//! hazard was emitted dodgeable whatever the road, so the driver with no lane
//! was handed `LANE_TAP_CHANGE_S` of allowance for a move they could not make
//! and the assist waited it out while the truck kept closing.
//!
//! Two separate questions came out of one flag, and this file is the pin on
//! keeping them apart:
//!
//! * `dodgeable` -- IS THERE SOMEWHERE TO GO. The thing sits in one lane and
//!   the road has an open lane on this side. It buys the lane-change
//!   allowance and nothing else.
//! * `in_lane` -- IS THE THING IN OUR LANE, as against spanning the road.
//!   With `hazard_lead_mph` it decides the speed braking ALONE has to reach.
//!
//! The order mattered. Flipping `dodgeable` without splitting the target off
//! it first would have dragged the truck to `HAZARD_SAFE_MPH` for a moving
//! vehicle, which is Brandon's regression of 2026-08-23 all over again.

use ff_core::sim::traffic_manager::TrafficVehicle;
use ff_core::sim::trip_models::{
    hazard_is_in_lane, TripEvent, TripEventData, TripEventKind, TRAFFIC_WARNING_GAP_S,
};
use ff_core::sim::weather::WeatherKind;
use ff_core::speech_text::SpokenMessage;

use freight_fate::playtest::harness::{PlaytestHarness, StartDelivery};
use freight_fate::states::driving_core::{
    HazardShape, HAZARD_CREEP_MPH, HAZARD_SAFE_MPH, LANE_TAP_CHANGE_S,
};

use crate::states_driving_traffic_rate::bench_lead;

const MPH_PER_MPS: f64 = 2.2369362920544;

// -- rigging -------------------------------------------------------------------------

fn a_drive(name: &str) -> PlaytestHarness {
    let mut harness = PlaytestHarness::new();
    harness.start_delivery(StartDelivery::named(name));
    harness.with_drive(|drive, _| {
        drive.tutorial = None;
        drive.departure_checked = true;
        drive.trip.hazard_check_mi = 1e9;
        drive.trip.inspection_check_mi = 1e9;
        drive.trip.traffic_manager.rolling_bubble = false;
        drive.trip.set_npc_vehicles(Vec::new());
        drive.trip.traffic_pressures.clear();
        drive.trip.zones.retain(|z| z.aadt.is_none());
        drive.trip.weather.current = WeatherKind::Clear;
        drive.truck_mut().start_engine();
        drive.truck_mut().set_air_ready(false);
    });
    harness.clear_speech();
    harness
}

/// A hazard event as the emitter now builds one: the two questions answered
/// separately, plus the lead's speed where the hazard is a vehicle.
fn a_hazard(name: &str, dodgeable: bool, in_lane: bool, lead_mph: Option<f64>) -> TripEvent {
    let traffic = lead_mph.map(|mph| ff_core::sim::trip_models::TrafficContext {
        lead: TrafficVehicle::new("bench:lead", 1.0, mph, mph, 0, "cruising", "car"),
        gap_mi: 0.02,
        closing_mph: 20.0,
    });
    TripEvent {
        kind: TripEventKind::Hazard,
        message: SpokenMessage::new("Brake!"),
        data: TripEventData {
            deadline_s: Some(3.0),
            dodgeable: Some(dodgeable),
            in_lane: Some(in_lane),
            traffic,
            name: Some(name.to_string()),
            ..Default::default()
        },
    }
}

fn arm(harness: &mut PlaytestHarness, event: TripEvent) {
    harness.with_drive(move |drive, ctx| drive.handle_trip_event(ctx, &event));
}

fn at_mph(harness: &mut PlaytestHarness, mph: f64) {
    harness.with_drive(move |drive, _| drive.truck_mut().velocity_mps = mph / MPH_PER_MPS);
}

// -- the untangle: what braking alone has to reach --------------------------------------

#[test]
fn a_vehicle_hazard_clears_at_the_lead_speed_whether_or_not_a_lane_is_open() {
    // THE TRAP THIS FILE'S ORDER OF WORK EXISTS FOR. Before the split, the
    // target read `dodgeable`, so the moment a one-lane lead stopped being
    // called dodgeable it would have fallen through to HAZARD_SAFE_MPH: the
    // truck dragged to 25 for a car doing 55, which is exactly what Brandon
    // reported on 2026-08-23 and what the lead-speed target fixed.
    let mut harness = a_drive("Lead Target");
    harness.with_drive(|d, _| {
        for dodgeable in [true, false] {
            let shape = HazardShape {
                dodgeable,
                in_lane: true,
                lead_mph: Some(55.0),
                brake_to_mph: None,
            };
            assert_eq!(
                d.hazard_target_mph(Some(shape)),
                55.0,
                "a vehicle doing 55 must clear at 55 whatever the road offers \
                 (dodgeable = {dodgeable})"
            );
        }
        // A lead that has itself stopped still asks for a stop.
        let stopped = HazardShape {
            dodgeable: false,
            in_lane: true,
            lead_mph: Some(0.0),
            brake_to_mph: None,
        };
        assert_eq!(d.hazard_target_mph(Some(stopped)), HAZARD_CREEP_MPH);
    });
}

#[test]
fn a_fixed_obstacle_takes_the_near_stop_whether_or_not_a_lane_is_open() {
    // The other half of the untangle. A carcass is still a carcass on a road
    // with two lanes, and it is still a carcass on a road with one: braking
    // alone means nearly stopping either way. Keying this off "is there
    // somewhere to go" would have turned a one-lane carcass into a 25 mph
    // problem the moment dodgeable started meaning what it says.
    let mut harness = a_drive("Obstacle Target");
    harness.with_drive(|d, _| {
        for dodgeable in [true, false] {
            let shape = HazardShape {
                dodgeable,
                in_lane: true,
                lead_mph: None,
                brake_to_mph: None,
            };
            assert_eq!(
                d.hazard_target_mph(Some(shape)),
                HAZARD_CREEP_MPH,
                "a thing lying in the lane needs nearly a stop (dodgeable = {dodgeable})"
            );
        }
    });
}

#[test]
fn a_hazard_that_spans_the_road_takes_the_moving_hazard_safe_speed() {
    // Fog, ice, a crosswind: nothing in the lane to creep past, so the near
    // stop is not the answer and never was.
    let mut harness = a_drive("Spanning Target");
    harness.with_drive(|d, _| {
        let shape = HazardShape {
            dodgeable: false,
            in_lane: false,
            lead_mph: None,
            brake_to_mph: None,
        };
        assert_eq!(d.hazard_target_mph(Some(shape)), HAZARD_SAFE_MPH);
    });
}

#[test]
fn only_an_open_lane_buys_the_lane_change_allowance() {
    // The allowance is for the MOVE, so it is bought by the road having
    // somewhere to move to -- and by nothing else. Same hazard, same target,
    // and the two windows differ by exactly the time a tap change takes.
    let mut harness = a_drive("Allowance");
    harness.with_drive(|d, _| {
        d.truck_mut().velocity_mps = 65.0 / MPH_PER_MPS;
        let nowhere = HazardShape {
            dodgeable: false,
            in_lane: true,
            lead_mph: Some(45.0),
            brake_to_mph: None,
        };
        let somewhere = HazardShape {
            dodgeable: true,
            ..nowhere
        };
        assert_eq!(
            d.hazard_target_mph(Some(nowhere)),
            d.hazard_target_mph(Some(somewhere)),
            "the road must not change what braking has to reach"
        );
        let tight = d.hazard_deadline_for(4.0, Some(nowhere));
        let roomy = d.hazard_deadline_for(4.0, Some(somewhere));
        assert!(
            (roomy - (tight + LANE_TAP_CHANGE_S)).abs() < 1e-9,
            "the open-lane window is {roomy:.3} s and the closed one {tight:.3} s; the \
             difference must be exactly the {LANE_TAP_CHANGE_S} s a tap change takes"
        );
    });
}

// -- what the emitter now says, and hands the physics ------------------------------------

/// Drive a one-lane or multi-lane road up behind a slower vehicle and return
/// the hazard the trip raised.
fn lead_vehicle_hazard(lanes_your_side: i64) -> TripEvent {
    use ff_core::data::world_models::{CorridorDetail, LaneSegment, Leg, Route};
    use ff_core::sim::trip::{Trip, TripOptions};
    use ff_core::sim::vehicle::TruckState;
    use ff_core::sim::weather::WeatherSystem;

    let leg = Leg::new("a", "b", 900.0, "US-30", "flat", Vec::new()).with_detail(CorridorDetail {
        // An undivided leg's `lanes` is the count for BOTH directions.
        lane_segments: vec![LaneSegment {
            start_mi: 0.0,
            end_mi: 900.0,
            lanes: lanes_your_side * 2,
            oneway: false,
            ..Default::default()
        }],
        ..Default::default()
    });
    let route = Route::new(vec!["a".to_string(), "b".to_string()], vec![leg.into()]);
    let mut trip = Trip::new(
        route,
        TruckState::default(),
        WeatherSystem::new("heartland", Some(3), None, None, true),
        TripOptions {
            seed: Some(3),
            time_scale: 1.0,
            ..Default::default()
        },
    );
    trip.position_mi = 50.0;
    // Every closure in this file is one the case placed, and a coned-off lane
    // would quietly make a four-lane road a three-lane one.
    trip.zones.clear();
    trip.curves.clear();
    trip.traffic_warning_mi = 0.0;
    trip.truck.velocity_mps = 50.0 / MPH_PER_MPS;
    assert_eq!(
        trip.lane_count_at(None),
        lanes_your_side,
        "the bench did not build the road the case asked for"
    );
    let mut lead =
        TrafficVehicle::new("npc:lead", 50.01, 30.0, 30.0, 0, "cruising", "car").with_lane(0);
    lead.intent = "braking".to_string();
    trip.set_npc_vehicles(vec![lead]);
    trip.check_hazards(1.0);
    trip.events
        .iter()
        .find(|e| e.kind == TripEventKind::Hazard)
        .cloned()
        .expect("a lead-vehicle hazard")
}

#[test]
fn a_lead_vehicle_with_nowhere_to_go_is_not_dodgeable_and_the_call_agrees() {
    // The owner's road. The words already said "Brake!" here; what is new is
    // that the physics is handed the same answer, so no lane-change allowance
    // is added to a window on a road that offers no lane change.
    let event = lead_vehicle_hazard(1);
    assert_eq!(
        event.text(),
        "Brake! Brake lights right ahead. No lane open."
    );
    assert!(!event.text().contains("Change lanes"));
    assert_eq!(event.data.dodgeable, Some(false));
    assert_eq!(event.data.in_lane, Some(true));
}

#[test]
fn a_lead_vehicle_with_a_lane_open_keeps_both_its_offer_and_its_allowance() {
    // The guard against making every hazard urgent: the multi-lane case is
    // untouched, offer and allowance both.
    let event = lead_vehicle_hazard(3);
    assert_eq!(
        event.text(),
        "Change lanes or brake! Brake lights right ahead. Middle lane open."
    );
    assert_eq!(event.data.dodgeable, Some(true));
    assert_eq!(event.data.in_lane, Some(true));
}

/// Drive a road of `lanes_your_side` lanes until the hazard draw lands on the
/// named catalog hazard, and return the event it raised.
pub(crate) fn drawn_hazard(lanes_your_side: i64, wanted: &str) -> TripEvent {
    use ff_core::data::world::get_world;
    use ff_core::data::world_models::{CorridorDetail, LaneSegment, Leg, Route};
    use ff_core::sim::trip::{Trip, TripOptions};
    use ff_core::sim::vehicle::TruckState;
    use ff_core::sim::weather::WeatherSystem;

    // The hazard pool is drawn per region, so the endpoints have to be real
    // world cities or the region lookup cannot resolve.
    let corridor = get_world()
        .supported_route("Chicago", "St. Louis", None)
        .expect("the world routes")
        .expect("Chicago to St. Louis has a route");
    let (from, to) = (
        corridor.cities[0].clone(),
        corridor.cities.last().unwrap().clone(),
    );
    for seed in 0..400 {
        let leg =
            Leg::new(&from, &to, 900.0, "US-30", "flat", Vec::new()).with_detail(CorridorDetail {
                lane_segments: vec![LaneSegment {
                    start_mi: 0.0,
                    end_mi: 900.0,
                    lanes: lanes_your_side * 2,
                    oneway: false,
                    ..Default::default()
                }],
                ..Default::default()
            });
        let route = Route::new(vec![from.clone(), to.clone()], vec![leg.into()]);
        let mut trip = Trip::new(
            route,
            TruckState::default(),
            WeatherSystem::new("heartland", Some(seed), None, None, true),
            TripOptions {
                seed: Some(seed),
                time_scale: 1.0,
                hazard_scale: 4.0,
                ..Default::default()
            },
        );
        trip.position_mi = 50.0;
        trip.zones.clear();
        trip.curves.clear();
        trip.hazard_check_mi = 0.0;
        trip.weather.current = WeatherKind::Clear;
        assert_eq!(trip.lane_count_at(None), lanes_your_side);
        trip.check_hazards(0.0);
        let Some(event) = trip.events.iter().find(|e| e.kind == TripEventKind::Hazard) else {
            continue;
        };
        if event.text().contains(wanted) {
            return event.clone();
        }
    }
    panic!("no seed in 0..400 drew {wanted} on a {lanes_your_side}-lane road");
}

#[test]
fn a_fixed_obstacle_is_dodgeable_where_a_lane_is_open_and_the_call_says_so() {
    // Owner, 2026-08-24: "we should be able to swerve around some fixed
    // obstacles." Debris beside an open lane is exactly what a driver goes
    // around rather than stopping dead for, and the rule that decides it is
    // the same one the vehicle case uses -- is there somewhere to go.
    let open = drawn_hazard(3, "Debris on the road");
    assert_eq!(
        open.text(),
        "Change lanes or brake! Debris on the road. Middle lane open."
    );
    assert_eq!(open.data.dodgeable, Some(true));
    assert_eq!(open.data.in_lane, Some(true));

    // And on a road with one lane it is the near stop, said plainly, with no
    // lane change offered.
    let shut = drawn_hazard(1, "Debris on the road");
    assert_eq!(shut.text(), "Brake! Debris on the road. No lane open.");
    assert!(!shut.text().contains("Change lanes"));
    assert_eq!(shut.data.dodgeable, Some(false));
    // Still in the lane, which is what keeps the near stop.
    assert_eq!(shut.data.in_lane, Some(true));
}

#[test]
fn a_hazard_that_spans_the_road_is_never_dodgeable_however_many_lanes() {
    // The kinds a lane change could never answer, named rather than lumped
    // in: ice, fog, a crosswind, a dust storm, and an animal that may bolt
    // either way. "Brake now!" is theirs alone, and it keeps its meaning
    // precisely because a merely narrow road no longer borrows it.
    for text in [
        "a deer crossing the road",
        "ice on the bridge deck",
        "a dust storm dropping visibility",
        "stopped traffic around a fender bender",
    ] {
        assert!(
            !hazard_is_in_lane(text),
            "{text:?} spans the road; it must not be marked as sitting in one lane"
        );
    }
    // Drawn on a three-lane road, where a lane change is available and still
    // is not the answer: the road ahead is blocked, not one lane of it.
    let event = drawn_hazard(3, "Stopped traffic around a fender bender");
    assert_eq!(
        event.text(),
        "Brake now! Stopped traffic around a fender bender."
    );
    assert_eq!(event.data.dodgeable, Some(false));
    assert_eq!(event.data.in_lane, Some(false));
}

// -- what the driver hears when it is over -----------------------------------------------

#[test]
fn the_resolution_line_never_claims_a_lane_change_the_road_had_no_room_for() {
    // A hazard resolved by braking must not tell the driver they eased around
    // it on a road with nowhere to ease into. The vehicle half of this was
    // fixed on 2026-08-24; the obstacle half is fixed here.
    let mut harness = a_drive("Nowhere To Ease");
    harness.with_drive(|d, _| d.trip.truck.velocity_mps = 60.0 / MPH_PER_MPS);
    arm(
        &mut harness,
        a_hazard("the debris", false, true, None), // one lane, a thing in it
    );
    harness.clear_speech();
    at_mph(&mut harness, HAZARD_CREEP_MPH - 1.0);
    harness.with_drive(|d, ctx| d.update_hazard(ctx, 1.0 / 60.0));
    let heard = harness.app.event_lines();
    assert!(
        heard.iter().any(|line| line == "Clear of the debris."),
        "{heard:#?}"
    );
    assert!(!heard.iter().any(|line| line.contains("ease around")));
}

#[test]
fn the_resolution_line_still_offers_easing_around_where_there_was_a_lane() {
    let mut harness = a_drive("Room To Ease");
    harness.with_drive(|d, _| d.trip.truck.velocity_mps = 60.0 / MPH_PER_MPS);
    arm(&mut harness, a_hazard("the debris", true, true, None));
    harness.clear_speech();
    at_mph(&mut harness, HAZARD_CREEP_MPH - 1.0);
    harness.with_drive(|d, ctx| d.update_hazard(ctx, 1.0 / 60.0));
    let heard = harness.app.event_lines();
    assert!(
        heard.iter().any(|line| line == "Clear of the debris."),
        "{heard:#?}"
    );
}

#[test]
fn a_vehicle_is_never_told_to_nearly_stop_for() {
    // The lingering "It is still in your lane. Nearly stop." belongs to a
    // THING in the lane. A vehicle is cleared by matching it, so that line
    // would be an instruction the assist is not following.
    let mut harness = a_drive("Slow Car Hint");
    harness.with_drive(|d, _| d.trip.truck.velocity_mps = 60.0 / MPH_PER_MPS);
    arm(
        &mut harness,
        a_hazard("the slow car", false, true, Some(5.0)),
    );
    harness.clear_speech();
    at_mph(&mut harness, HAZARD_SAFE_MPH - 1.0);
    harness.with_drive(|d, ctx| d.update_hazard(ctx, 1.0 / 60.0));
    let heard = harness.app.event_lines();
    assert!(
        !heard.iter().any(|line| line.contains("Nearly stop")),
        "{heard:#?}"
    );
}

// -- the response, driven ----------------------------------------------------------------

#[test]
fn with_nowhere_to_go_the_assist_acts_sooner_and_holds_the_truck_nearer() {
    // The measurement the whole change is for, as an assertion. The same
    // meeting on two roads: truck at 65, a car doing 45 already inside the
    // warning window, hands off from the call. Before this change the two
    // roads were identical in every number -- same window, same moment the
    // assist took the pedal, same distance covered before it did.
    let nowhere = bench_lead(1, 45.0);
    let somewhere = bench_lead(2, 45.0);

    assert!(
        nowhere.call.contains("Brake! Slow car"),
        "{:?}",
        nowhere.call
    );
    assert!(
        somewhere.call.contains("Change lanes or brake! Slow car"),
        "{:?}",
        somewhere.call
    );

    // Sooner: the driver with nowhere to go is given exactly the tap change
    // less, and the assist takes the truck that much earlier.
    assert!(
        (somewhere.granted_s - (nowhere.granted_s + LANE_TAP_CHANGE_S)).abs() < 1e-6,
        "windows were {:.2} s with a lane and {:.2} s without",
        somewhere.granted_s,
        nowhere.granted_s
    );
    assert!(
        nowhere.assist_after_s < somewhere.assist_after_s,
        "the assist acted {:.2} s after the call with nowhere to go and {:.2} s with a lane \
         open: the road with no way past must be the one acted on first",
        nowhere.assist_after_s,
        somewhere.assist_after_s
    );

    // Firmer: it takes the truck while the car is still ahead of it, rather
    // than a couple of hundred feet after the truck has already gone by.
    assert!(
        nowhere.assist_gap_ft > somewhere.assist_gap_ft,
        "the assist acted {:.0} ft from the car with nowhere to go and {:.0} ft with a lane \
         open",
        nowhere.assist_gap_ft,
        somewhere.assist_gap_ft
    );

    // And both still resolve at the CAR's speed, not at a near stop.
    for run in [&nowhere, &somewhere] {
        assert!(
            run.bottom_mph > HAZARD_SAFE_MPH,
            "the truck was dragged to {:.1} mph for a car doing 45 on a {}-lane road",
            run.bottom_mph,
            run.lanes
        );
        assert!(
            run.bottom_mph < 50.0,
            "the truck never came down to the car at all: {:.1} mph",
            run.bottom_mph
        );
        assert!(
            run.resolution.contains("Clear of the slow car."),
            "{:?}",
            run.resolution
        );
    }
}

#[test]
fn the_emergency_brake_is_not_spent_on_an_ordinary_lead_vehicle() {
    // The hardest stop the rig has is reserved for a stop MEASURED to be
    // losing ground -- drums cooking, a grade steepening, grip that is not
    // there. Catching a slower car on level ground is none of those, on
    // either road, and acting sooner must not turn into acting harder than
    // the measurement warrants.
    for lanes in [1, 2] {
        for lead_mph in [45.0, 10.0] {
            let run = bench_lead(lanes, lead_mph);
            assert!(
                !run.emergency,
                "the emergency application was spent catching a car doing {lead_mph:.0} on a \
                 {lanes}-lane road"
            );
        }
    }
}

/// A warning window still has to cover the stop it is asking for: the point
/// of the change is that the driver with no lane loses the ALLOWANCE for a
/// move they cannot make, not that they lose their own reaction time.
#[test]
fn losing_the_allowance_never_takes_the_drivers_own_reaction_time() {
    let mut harness = a_drive("Reaction Floor");
    harness.with_drive(|d, _| {
        d.truck_mut().velocity_mps = 65.0 / MPH_PER_MPS;
        let nowhere = HazardShape {
            dodgeable: false,
            in_lane: true,
            lead_mph: Some(45.0),
            brake_to_mph: None,
        };
        let window = d.hazard_deadline_for(4.0, Some(nowhere));
        let engage = d.aeb_engage_s(d.hazard_target_mph(Some(nowhere)));
        assert!(
            window - engage >= 4.0 - 1e-9,
            "the driver was left {:.2} s of their own, not the 4.0 s the hazard asked for",
            window - engage
        );
    });
}

/// The warning window and the vehicle it was raised about must agree: a lead
/// hazard's own budget is computed from ITS lead speed, not from whatever the
/// last hazard left behind.
#[test]
fn a_fresh_hazards_budget_is_built_from_its_own_lead_not_the_last_ones() {
    let mut harness = a_drive("Own Budget");
    harness.with_drive(|d, _| d.trip.truck.velocity_mps = 65.0 / MPH_PER_MPS);
    arm(
        &mut harness,
        a_hazard("the slow car", false, true, Some(45.0)),
    );
    let (deadline, expected) = harness.read_drive(|d| {
        let shape = HazardShape {
            dodgeable: false,
            in_lane: true,
            lead_mph: Some(45.0),
            brake_to_mph: None,
        };
        (
            d.hazard_deadline.expect("a hazard deadline"),
            d.aeb_engage_s(d.hazard_target_mph(Some(shape))),
        )
    });
    // The window is the engage point plus the driver's own time; if the
    // budget had been built against a near stop instead of the car's 45 the
    // engage point alone would already be most of the deadline.
    assert!(
        deadline > expected,
        "the deadline {deadline:.2} s does not even cover its own engage point {expected:.2} s"
    );
    let stopping = harness.read_drive(|d| {
        d.aeb_engage_s(d.hazard_target_mph(Some(HazardShape {
            dodgeable: false,
            in_lane: true,
            lead_mph: None,
            brake_to_mph: None,
        })))
    });
    assert!(
        expected < stopping,
        "budgeting a car doing 45 must cost less than budgeting a near stop: {expected:.2} s \
         against {stopping:.2} s"
    );
}

// -- the emitter and the assist can never disagree ----------------------------------------

#[test]
fn the_words_and_the_physics_read_the_same_lane_authority() {
    // One predicate, asked once. If these ever drift apart the driver is told
    // one thing and the truck does another, which is the whole class of bug
    // this file is about.
    for lanes in [1, 2, 3] {
        let event = lead_vehicle_hazard(lanes);
        let offers_lane = event.text().contains("Change lanes");
        assert_eq!(
            offers_lane,
            event.data.dodgeable.unwrap_or(false),
            "on a {lanes}-lane road the call was {:?} and the physics was told dodgeable = {:?}",
            event.text(),
            event.data.dodgeable
        );
    }
}

#[test]
fn a_hazard_event_that_says_nothing_about_its_lane_still_behaves() {
    // Older callers -- tools, the break scenarios, anything constructing a
    // hazard by hand -- set only `dodgeable`. That meant "a thing in the
    // lane you could steer around", so it stands in for both.
    let mut harness = a_drive("Legacy Event");
    harness.with_drive(|d, _| d.trip.truck.velocity_mps = 60.0 / MPH_PER_MPS);
    let event = TripEvent {
        kind: TripEventKind::Hazard,
        message: SpokenMessage::new("Brake!"),
        data: TripEventData {
            deadline_s: Some(3.0),
            dodgeable: Some(true),
            name: Some("the ladder".to_string()),
            ..Default::default()
        },
    };
    arm(&mut harness, event);
    harness.read_drive(|d| {
        assert!(d.hazard_in_lane);
        assert_eq!(d.hazard_target_mph(None), HAZARD_CREEP_MPH);
    });
}

#[test]
fn folding_a_second_hazard_in_takes_the_slower_of_the_two_demands() {
    // A thing lying in the lane does not stop lying there because fog rolled
    // in on top of it, so the group keeps the near stop rather than falling
    // back to the road-spanning safe speed.
    let mut harness = a_drive("Folded");
    harness.with_drive(|d, _| d.trip.truck.velocity_mps = 60.0 / MPH_PER_MPS);
    arm(&mut harness, a_hazard("the ladder", false, true, None));
    arm(&mut harness, a_hazard("the fog", false, false, None));
    harness.read_drive(|d| {
        assert!(d.hazard_in_lane, "the ladder is still lying there");
        assert!(!d.hazard_dodgeable);
        assert_eq!(d.hazard_target_mph(None), HAZARD_CREEP_MPH);
    });
}

#[test]
fn the_warning_window_is_the_only_thing_an_open_lane_changes() {
    // Stated as a property over every shape, so a future change that reaches
    // for `dodgeable` to decide a speed fails here rather than in a drive.
    let mut harness = a_drive("Property");
    harness.with_drive(|d, _| {
        d.truck_mut().velocity_mps = 65.0 / MPH_PER_MPS;
        for in_lane in [true, false] {
            for lead_mph in [None, Some(0.0), Some(30.0), Some(60.0)] {
                let shut = HazardShape {
                    dodgeable: false,
                    in_lane,
                    lead_mph,
                    brake_to_mph: None,
                };
                let open = HazardShape {
                    dodgeable: true,
                    ..shut
                };
                assert_eq!(
                    d.hazard_target_mph(Some(shut)),
                    d.hazard_target_mph(Some(open)),
                    "an open lane changed what braking has to reach for {shut:?}"
                );
                assert!(
                    (d.hazard_deadline_for(4.0, Some(open))
                        - d.hazard_deadline_for(4.0, Some(shut))
                        - LANE_TAP_CHANGE_S)
                        .abs()
                        < 1e-9,
                    "an open lane bought something other than the tap change for {shut:?}"
                );
            }
        }
    });
}

// -- the trigger is untouched -------------------------------------------------------------

#[test]
fn the_warning_still_fires_on_the_same_gap_it_always_did() {
    // The fix is about the RESPONSE, not about how often the truck is held
    // up. The trigger is the traffic warning gap and nothing here moves it.
    let event = lead_vehicle_hazard(1);
    assert!(event.data.traffic.is_some());
    let context = event.data.traffic.as_ref().expect("filtered on it");
    assert!(
        context.gap_seconds() <= TRAFFIC_WARNING_GAP_S,
        "the warning fired at {:.2} s of gap, past its own {TRAFFIC_WARNING_GAP_S} s threshold",
        context.gap_seconds()
    );
}
