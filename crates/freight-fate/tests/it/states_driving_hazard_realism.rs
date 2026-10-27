//! Owner, 2026-10-19: the assists go around things by changing lanes as
//! much as possible. An animal is braked for firmly in the lane, never
//! swerved for and never stopped for. The call names the animal and asks
//! for nothing.

use ff_core::sim::lane::MPH_PER_MPS;
use ff_core::sim::trip_models::{
    OpenSide, TripEvent, TripEventData, TripEventKind, ANIMAL_BRAKE_TO_MPH,
};
use ff_core::sim::weather::WeatherKind;
use ff_core::speech_text::{in_lane_hazard_call, passing_hazard_call, SpokenMessage};
use freight_fate::playtest::harness::{PlaytestHarness, RouteSetup};

use crate::states_driving_hazard_dodge::drawn_hazard;
use crate::states_driving_traffic_rate::lane_bench;
use crate::transcript_cruise_support::{frame, said_any, spoken, DT};

/// Two lanes each way, the truck in the right one at 65 on cruise, lane
/// keeping on full and automatic braking on, nothing else on the road.
fn open_interstate(name: &str) -> PlaytestHarness {
    let mut harness = PlaytestHarness::new();
    harness.app.ctx.settings.speed_keeper = false;
    harness.app.ctx.settings.automatic_emergency_braking = true;
    harness.app.ctx.settings.lane_keeping = "full".to_string();
    harness.app.ctx.settings.time_scale = 1.0;
    harness.start_route(
        "aberdeen_sd_us",
        "pierre_sd_us",
        RouteSetup::seeded(7).named(name),
    );
    harness.with_drive(|d, ctx| {
        lane_bench(d, 65.0, 2);
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
        d.trip.set_npc_vehicles(Vec::new());
        d.trip.traffic_manager.vehicles.clear();
        d.lane.set_lane_count(2);
        d.lane.lane = 0;
        d.trip.traffic_manager.player_lane = 0;
        d.speed_control_armed = true;
        d.cruise_mph = Some(65.0);
        if let Some(profile) = ctx.profile.as_mut() {
            profile.tutorial_done = true;
        }
        d.tutorial = None;
    });
    harness.clear_speech();
    harness
}

/// Debris in the truck's lane with the left lane open, as the trip calls it.
fn debris_call() -> TripEvent {
    TripEvent {
        kind: TripEventKind::Hazard,
        message: in_lane_hazard_call("Debris on the road.", OpenSide::Left),
        data: TripEventData {
            deadline_s: Some(3.5),
            dodgeable: Some(true),
            in_lane: Some(true),
            open_side: Some(OpenSide::Left),
            pass_message: passing_hazard_call("Debris on the road.", OpenSide::Left),
            animal: Some(false),
            name: Some("the debris".to_string()),
            ..Default::default()
        },
    }
}

fn deer_call() -> TripEvent {
    TripEvent {
        kind: TripEventKind::Hazard,
        message: SpokenMessage::new("A deer crossing the road."),
        data: TripEventData {
            deadline_s: Some(3.5),
            dodgeable: Some(false),
            in_lane: Some(false),
            animal: Some(true),
            brake_to_mph: Some(ANIMAL_BRAKE_TO_MPH),
            name: Some("the deer".to_string()),
            ..Default::default()
        },
    }
}

fn arm(harness: &mut PlaytestHarness, event: TripEvent) {
    harness.with_drive(move |d, ctx| d.handle_trip_event(ctx, &event));
}

#[test]
fn debris_with_a_lane_open_is_gone_around_not_braked_for() {
    let mut harness = open_interstate("Debris Pass");
    arm(&mut harness, debris_call());
    assert!(
        said_any(&harness, "Passing on the left."),
        "{:#?}",
        spoken(&harness)
    );
    assert_eq!(harness.read_drive(|d| d.lane_change_target), Some(1));
    let mut slowest = f64::INFINITY;
    let mut braked = false;
    let mut elapsed = 0.0;
    while harness.read_drive(|d| d.hazard_deadline.is_some()) && elapsed < 15.0 {
        frame(&mut harness, DT);
        elapsed += DT;
        let (mph, aeb) = harness.read_drive(|d| (d.truck().speed_mph(), d.aeb_brake));
        slowest = slowest.min(mph);
        braked |= aeb > 0.0;
    }
    assert!(
        !braked,
        "the automatic brake acted for debris it went around"
    );
    assert!(
        slowest > 55.0,
        "slowed to {slowest:.0} for debris it passed"
    );
    assert_eq!(harness.read_drive(|d| d.lane.lane), 1);
}

#[test]
fn after_going_around_debris_the_truck_holds_the_passing_lane_until_it_is_by() {
    let mut harness = open_interstate("Debris Clear");
    arm(&mut harness, debris_call());
    let clear_mi = harness
        .read_drive(|d| d.pass_clear_mi)
        .expect("a pass around a thing in the lane knows where it ends");
    let mut elapsed = 0.0;
    let mut back_early = false;
    while harness.read_drive(|d| d.passing.is_some() || d.lane_change_target.is_some())
        && elapsed < 60.0
    {
        frame(&mut harness, DT);
        elapsed += DT;
        let (at, target) = harness.read_drive(|d| (d.trip.position_mi, d.lane_change_target));
        back_early |= target == Some(0) && at < clear_mi;
    }
    assert!(!back_early, "started back into the lane beside the debris");
    assert_eq!(
        harness.read_drive(|d| d.lane.lane),
        0,
        "back in the right lane"
    );
}

#[test]
fn an_animal_is_braked_for_firmly_in_the_lane_not_stopped_for() {
    let mut harness = open_interstate("Deer");
    arm(&mut harness, deer_call());
    let calls = spoken(&harness);
    assert!(
        calls.iter().any(|line| line == "A deer crossing the road."),
        "{calls:#?}"
    );
    assert!(
        !calls
            .iter()
            .any(|line| line.contains("speed control paused")),
        "{calls:#?}"
    );
    let mut braked = false;
    let mut emergency = false;
    let mut slowest = f64::INFINITY;
    let mut elapsed = 0.0;
    while harness.read_drive(|d| d.hazard_deadline.is_some()) && elapsed < 20.0 {
        frame(&mut harness, DT);
        elapsed += DT;
        let (mph, aeb, hard) =
            harness.read_drive(|d| (d.truck().speed_mph(), d.aeb_brake, d.aeb_emergency));
        slowest = slowest.min(mph);
        braked |= aeb > 0.0;
        emergency |= hard;
    }
    assert!(braked, "the automatic brake did not act for a deer");
    assert!(!emergency, "the emergency brake was used for a deer");
    assert!(slowest > 30.0, "slowed to {slowest:.0} for a deer");
    assert_eq!(harness.read_drive(|d| d.lane.lane), 0, "swerved for a deer");
    assert!(
        said_any(&harness, "Clear of the deer.") || said_any(&harness, "Collision with the deer."),
        "{:#?}",
        spoken(&harness)
    );
}

#[test]
fn with_automatic_braking_off_a_deer_is_left_to_the_driver() {
    let mut harness = open_interstate("Deer No Assist");
    harness.with_drive(|_, ctx| ctx.settings.automatic_emergency_braking = false);
    arm(&mut harness, deer_call());
    let mut braked = false;
    let mut elapsed = 0.0;
    while harness.read_drive(|d| d.hazard_deadline.is_some()) && elapsed < 20.0 {
        frame(&mut harness, DT);
        elapsed += DT;
        braked |= harness.read_drive(|d| d.aeb_brake > 0.0);
    }
    assert!(!braked);
    assert!(
        said_any(&harness, "Clear of the deer.") || said_any(&harness, "Collision with the deer."),
        "{:#?}",
        spoken(&harness)
    );
}

#[test]
fn a_deer_arriving_during_a_lane_change_does_not_call_the_change_off() {
    let mut harness = open_interstate("Deer Mid Pass");
    arm(&mut harness, debris_call());
    frame(&mut harness, DT);
    arm(&mut harness, deer_call());
    assert!(harness.read_drive(|d| d.hazard_dodgeable && !d.hazard_animal));
    let mut braked = false;
    let mut elapsed = 0.0;
    while harness.read_drive(|d| d.lane.lane == 0) && elapsed < 10.0 {
        frame(&mut harness, DT);
        elapsed += DT;
        braked |= harness.read_drive(|d| d.aeb_brake > 0.0);
    }
    assert!(!braked, "the deer turned the pass into a brake");
    assert_eq!(harness.read_drive(|d| d.lane.lane), 1);
}

#[test]
fn the_drawn_deer_call_asks_for_nothing() {
    let deer = drawn_hazard(2, "deer");
    assert_eq!(deer.text(), "A deer crossing the road.");
    assert_eq!(deer.data.animal, Some(true));
}

#[test]
fn a_mattress_with_something_across_the_road_is_not_a_near_stop() {
    let mut harness = open_interstate("Mattress And Gust");
    let mattress = TripEvent {
        kind: TripEventKind::Hazard,
        message: SpokenMessage::new("A mattress in the lane."),
        data: TripEventData {
            deadline_s: Some(6.0),
            dodgeable: Some(false),
            in_lane: Some(true),
            brake_to_mph: Some(35.0),
            animal: Some(false),
            name: Some("the mattress".to_string()),
            ..Default::default()
        },
    };
    let gust = TripEvent {
        kind: TripEventKind::Hazard,
        message: SpokenMessage::new("A strong crosswind gust."),
        data: TripEventData {
            deadline_s: Some(6.0),
            dodgeable: Some(false),
            in_lane: Some(false),
            animal: Some(false),
            name: Some("the gust".to_string()),
            ..Default::default()
        },
    };
    arm(&mut harness, mattress);
    arm(&mut harness, gust);
    let target = harness.read_drive(|d| d.hazard_target_mph(None));
    assert!(
        target >= 25.0,
        "braking to {target:.0} for a mattress and a gust"
    );
}
