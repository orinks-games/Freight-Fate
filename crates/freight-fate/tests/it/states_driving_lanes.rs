//! The discrete lane layer: `states/driving_updates/lanes.rs`, the tap change
//! in `states/driving_controls/keys.rs`, and the trip-side closure lookups
//! they all answer to.
//!
//! Ported from `tests/test_lane_discrete.py`. The `LaneKeeping` cases at the
//! top of that file live inline in `ff-core`'s `sim/lane.rs`; everything here
//! is the half that needs a real drive, a real trip, or both.
//!
//! Two Python seams have no Rust equivalent, and each is noted at its use:
//! `monkeypatch.setattr(trip, "traffic_context", lambda: ...)` (a real lead
//! vehicle is placed instead, which produces the same context), and the pair
//! `monkeypatch.setattr(trip, "_hazard_risk", ...)` /
//! `monkeypatch.setattr(road_events, "eligible_hazards", ...)` (the hazard
//! scale makes the draw certain and the seed is searched for the hazard the
//! Python case stubbed in).

use ff_core::data::world::get_world;
use ff_core::data::world_models::{CorridorDetail, LaneSegment, Leg, Route};
use ff_core::models::enforcement::{citation_fine, WORK_ZONE_BARRELS_FINE};
use ff_core::models::jobs::{Job, CARGO_CATALOG};
use ff_core::models::profile::Profile;
use ff_core::sim::lane::lane_label;
use ff_core::sim::traffic_manager::TrafficVehicle;
use ff_core::sim::trip::{Trip, TripOptions};
use ff_core::sim::trip_models::{
    hazard_is_in_lane, OpenSide, TripEvent, TripEventData, TripEventKind, Zone,
};
use ff_core::sim::vehicle::TruckState;
use ff_core::sim::weather::{WeatherKind, WeatherSystem};

use freight_fate::app::testing::TestApp;
use freight_fate::states::base::{InputEvent, Key, Mods};
use freight_fate::states::driving::DrivingState;
use freight_fate::states::driving_core::{DRIVE_PHASE_DELIVERY, HAZARD_CREEP_MPH, HAZARD_SAFE_MPH};

const MPH_PER_MPS: f64 = 2.2369362920544;

// -- rigging -------------------------------------------------------------------------

/// `_driving(app)`: a Buffalo to Rochester delivery on a clean road.
///
/// The Python fixture empties the traffic bubble and lets everything else
/// stand. Two more pins here, neither of which any case measures: the trip
/// seed (Python's is unseeded, so the zones it places are a fresh draw every
/// run) and the weather (an unseeded sky can come up ice, whose safe speed
/// sits under the speeds these cases roll at). The zones are then cleared
/// outright, because every closure in this file is one the case placed.
fn a_drive(app: &mut TestApp) -> DrivingState {
    let world = get_world();
    let mut profile = Profile::named_in("Lanes", "Buffalo");
    profile.tutorial_done = true;
    app.ctx.profile = Some(profile);
    let route = world
        .supported_route("Buffalo", "Rochester", None)
        .expect("the world routes")
        .expect("Buffalo to Rochester has a route");
    let mut job = Job::new(
        &CARGO_CATALOG["general"],
        12.0,
        "Buffalo",
        "company yard",
        "Rochester",
        route.miles(),
        1000.0,
        12.0,
    );
    job.destination_location = "Rochester freight market".to_string();
    let mut drive = DrivingState::new(
        &mut app.ctx,
        job,
        route,
        Some(99),
        DRIVE_PHASE_DELIVERY,
        Some(12.0),
    );
    drive.trip.set_npc_vehicles(Vec::new());
    drive.trip.weather.current = WeatherKind::Clear;
    drive.trip.zones.clear();
    app.clear_speech();
    drive
}

/// `_rolling(driving, mph)`.
fn rolling(d: &mut DrivingState, mph: f64) {
    d.trip.truck.start_engine();
    d.trip.truck.velocity_mps = mph / MPH_PER_MPS;
}

/// `_npc(position_mi, lane, speed_mph)`.
fn npc(position_mi: f64, lane: i64, speed_mph: f64) -> TrafficVehicle {
    TrafficVehicle::new(
        &format!("npc:{lane}:{position_mi}"),
        position_mi,
        speed_mph,
        speed_mph,
        -lane,
        "cruising",
        "car",
    )
    .with_lane(lane)
}

/// `_synthetic_trip(segments, miles, seed)`: a trip over one made-up leg with
/// the given baked lane segments.
fn synthetic_trip(segments: Vec<LaneSegment>, miles: f64, seed: i64) -> Trip {
    synthetic_trip_with(segments, miles, seed, 1.0)
}

fn synthetic_trip_with(
    segments: Vec<LaneSegment>,
    miles: f64,
    seed: i64,
    hazard_scale: f64,
) -> Trip {
    synthetic_trip_between("a", "b", segments, miles, seed, hazard_scale)
}

/// The same made-up leg between two named endpoints.
///
/// The default pair are the Python fixture's `a` and `b`, which are not world
/// cities -- fine for everything that only reads geometry, and fatal for
/// anything that asks the trip what region it is in.
fn synthetic_trip_between(
    from: &str,
    to: &str,
    segments: Vec<LaneSegment>,
    miles: f64,
    seed: i64,
    hazard_scale: f64,
) -> Trip {
    let leg = Leg::new(from, to, miles, "US-30", "flat", Vec::new()).with_detail(CorridorDetail {
        lane_segments: segments,
        ..Default::default()
    });
    let route = Route::new(vec![from.to_string(), to.to_string()], vec![leg.into()]);
    Trip::new(
        route,
        TruckState::default(),
        WeatherSystem::new("heartland", Some(seed), None, None, true),
        TripOptions {
            seed: Some(seed),
            time_scale: 1.0,
            hazard_scale,
            ..Default::default()
        },
    )
}

fn oneway(start_mi: f64, end_mi: f64, lanes: i64) -> LaneSegment {
    LaneSegment {
        start_mi,
        end_mi,
        lanes,
        oneway: true,
        ..Default::default()
    }
}

fn undivided(start_mi: f64, end_mi: f64, lanes: i64) -> LaneSegment {
    LaneSegment {
        start_mi,
        end_mi,
        lanes,
        oneway: false,
        ..Default::default()
    }
}

/// `_closure_footprints(trip)`: (taper start, work zone end) for every
/// construction zone that cones off a lane.
fn closure_footprints(trip: &Trip) -> Vec<(f64, f64)> {
    let mut out = Vec::new();
    for zone in &trip.zones {
        if zone.reason != "construction" || zone.closed_lane.is_none() {
            continue;
        }
        let taper = trip
            .zones
            .iter()
            .find(|z| z.reason == "construction merge" && (z.end_mi - zone.start_mi).abs() < 0.01);
        let start = taper.map_or(zone.start_mi, |t| t.start_mi);
        out.push((start, zone.end_mi));
    }
    out
}

/// `_run_into_the_barrels(d, zone)`: ride the coned-off lane until the
/// barrels take the truck.
fn run_into_the_barrels(d: &mut DrivingState, app: &mut TestApp, zone: &Zone) {
    d.trip.position_mi = (zone.start_mi + zone.end_mi) / 2.0;
    d.trip.zones.push(zone.clone());
    let closed = zone.closed_lane.expect("the zone cones off a lane");
    d.lane.lane = closed;
    for _ in 0..200 {
        d.update_merge(&mut app.ctx, 0.1);
        if d.merge_deadline.is_none() && d.lane.lane != closed {
            return;
        }
    }
    panic!("the barrels never fired");
}

/// Every line the drive submitted, read off the review log rather than the
/// channel: the Python suite monkeypatched `say_event` and so saw exactly
/// what each call site handed over, and an interrupting line's purge can
/// otherwise hand a cut line back to finish behind it.
fn logged(app: &TestApp) -> Vec<String> {
    app.ctx
        .message_log
        .messages
        .iter()
        .map(|m| m.text.clone())
        .collect()
}

// -- Hazard dodgeability --------------------------------------------------------------

#[test]
fn test_fixed_lane_hazards_are_in_lane_and_sweeping_ones_are_not() {
    // A property of the THING, not of the road: whether a lane change
    // actually answers it also needs a lane to change into, which the
    // emitter folds in separately (see the emitter cases below).
    assert!(hazard_is_in_lane("debris on the road"));
    assert!(hazard_is_in_lane("a vehicle stopped on the shoulder"));
    assert!(hazard_is_in_lane("a mattress lying in the lane"));
    assert!(!hazard_is_in_lane("a deer crossing the road"));
    assert!(!hazard_is_in_lane("ice on the bridge deck"));
    assert!(!hazard_is_in_lane("a dust storm dropping visibility"));
}

// -- Construction closures ------------------------------------------------------------

#[test]
fn test_construction_zones_sometimes_close_a_lane() {
    let world = get_world();
    let route = world
        .supported_route("Chicago", "St. Louis", None)
        .expect("the world routes")
        .expect("Chicago to St. Louis has a route");
    let (mut found_closed, mut found_open) = (false, false);
    for seed in 0..60 {
        let trip = Trip::new(
            route.clone(),
            TruckState::default(),
            WeatherSystem::new("great_lakes", Some(seed), None, None, true),
            TripOptions {
                time_scale: 20.0,
                seed: Some(seed),
                start_hour: 10.0,
                imperial: true,
                hazard_scale: 1.0,
                ..Default::default()
            },
        );
        for zone in &trip.zones {
            if zone.reason != "construction" {
                continue;
            }
            match zone.closed_lane {
                Some(0) | Some(1) => {
                    found_closed = true;
                    // The taper ahead carries the same closure for its callout.
                    let tapers: Vec<&Zone> = trip
                        .zones
                        .iter()
                        .filter(|z| {
                            z.reason == "construction merge"
                                && (z.end_mi - zone.start_mi).abs() < 0.01
                        })
                        .collect();
                    assert!(!tapers.is_empty());
                    assert_eq!(tapers[0].closed_lane, zone.closed_lane);
                }
                None => found_open = true,
                _ => {}
            }
        }
        if found_closed && found_open {
            break;
        }
    }
    assert!(found_closed && found_open);
}

#[test]
fn test_closure_messages_name_the_closed_side() {
    let mut app = TestApp::new();
    let d = a_drive(&mut app);
    let trip = &d.trip;
    let closed_right = Zone::new(5.0, 8.0, 45.0, "construction").with_closed_lane(Some(0));
    let closed_left = Zone::new(5.0, 8.0, 45.0, "construction").with_closed_lane(Some(1));
    let open_zone = Zone::new(5.0, 8.0, 45.0, "construction");
    assert!(trip
        .zone_warning_message(&closed_right, 2.0)
        .contains("Merge left."));
    assert!(trip
        .zone_warning_message(&closed_left, 2.0)
        .contains("Merge right."));
    assert!(!trip.zone_warning_message(&open_zone, 2.0).contains("Merge"));
    assert!(trip
        .zone_entry_message(&closed_left)
        .contains("Keep right."));
    assert!(trip
        .zone_entry_message(&closed_right)
        .contains("Keep left."));
}

/// Shane's report: told the right lane was closed, then found the closure on
/// the other side. The zone stores a side, so the lane the callouts name and
/// the lane the game shuts are one fact -- on a three-wide road too, where a
/// bare index of 1 used to be the middle lane while every line called it the
/// left one.
#[test]
fn test_the_side_announced_is_the_side_that_is_shut() {
    let mut trip = synthetic_trip(vec![oneway(0.0, 900.0, 3)], 900.0, 7);
    assert_eq!(trip.lane_count_at(Some(100.0)), 3);
    for (side, expected) in [("right", 0i64), ("left", 2i64)] {
        let zone = Zone::new(90.0, 120.0, 45.0, "construction").with_closed_side(Some(side));
        trip.zones = vec![zone.clone()];
        let (shut, keep) = Trip::closure_phrases(&zone);
        assert_eq!(shut, side);
        assert_ne!(keep, side);
        let index = trip
            .closed_lane_at(Some(100.0), None)
            .expect("a closed lane");
        assert_eq!(index, expected);
        // What the player is told, and which lane the game shuts, agree.
        assert_eq!(lane_label(index, 3), shut);
        assert!(trip
            .zone_warning_message(&zone, 2.0)
            .contains(&format!("Merge {keep}.")));
        assert!(trip
            .zone_entry_message(&zone)
            .contains(&format!("Keep {keep}.")));
    }
}

/// A stored lane index means a different lane at either end of one work zone;
/// the side does not.
#[test]
fn test_a_closure_keeps_its_side_where_the_road_widens() {
    let mut trip = synthetic_trip(
        vec![oneway(0.0, 100.0, 2), oneway(100.0, 900.0, 3)],
        900.0,
        11,
    );
    let zone = Zone::new(90.0, 120.0, 45.0, "construction").with_closed_side(Some("left"));
    trip.zones = vec![zone];
    assert_eq!(trip.closed_lane_at(Some(95.0), None), Some(1)); // left of two
    assert_eq!(trip.closed_lane_at(Some(110.0), None), Some(2)); // still left, of three
    assert_eq!(
        lane_label(trip.closed_lane_at(Some(95.0), None).unwrap(), 2),
        "left"
    );
    assert_eq!(
        lane_label(trip.closed_lane_at(Some(110.0), None).unwrap(), 3),
        "left"
    );
}

/// `active_zone` answers with the slowest zone at the mile, so a jam laid over
/// the roadwork used to leave the closure unenforced and unspoken while the
/// warning had already named it.
#[test]
fn test_a_jam_over_the_work_zone_cannot_hide_the_closure() {
    let mut trip = synthetic_trip(vec![oneway(0.0, 900.0, 2)], 900.0, 5);
    let work = Zone::new(90.0, 120.0, 45.0, "construction").with_closed_side(Some("right"));
    let jam = Zone::new(80.0, 130.0, 25.0, "heavy traffic");
    trip.zones = vec![work.clone(), jam.clone()];
    trip.position_mi = 100.0;
    assert_eq!(trip.active_zone(), Some(jam)); // the slower of the two
    assert_eq!(trip.active_closure(None), Some(work));
    assert_eq!(trip.closed_lane_at(None, None), Some(0));
}

/// The never-stuck invariant: wherever a lane is shut, another lane on our
/// side is open at that same mile.
#[test]
fn test_a_closure_always_leaves_somewhere_to_go() {
    let segments = || {
        vec![
            oneway(0.0, 300.0, 2),
            oneway(300.0, 600.0, 3),
            oneway(600.0, 900.0, 2),
        ]
    };
    let mut checked = 0;
    for seed in 0..25 {
        let trip = synthetic_trip(segments(), 900.0, seed);
        for (start, end) in closure_footprints(&trip) {
            let mut mile = start;
            while mile <= end {
                let count = trip.lane_count_at(Some(mile));
                if let Some(closed) = trip.closed_lane_at(Some(mile), None) {
                    checked += 1;
                    assert!(count >= 2);
                    assert!((0..count).contains(&closed));
                    assert!(closed == 0 || closed == count - 1); // never the middle lane
                    assert!((0..count).any(|lane| lane != closed));
                }
                mile += 0.25;
            }
        }
    }
    assert!(checked > 0); // the sweep actually saw closures
}

/// The one authority a hazard warning's wording answers to: the same lane
/// count a lane change is refused against, and the same work-zone closure
/// `closed_lane_at` already reads.
#[test]
fn test_has_open_adjacent_lane_at_reads_lane_count_and_closures() {
    let one_lane = synthetic_trip(vec![undivided(0.0, 900.0, 2)], 900.0, 1);
    assert!(!one_lane.has_open_adjacent_lane_at(Some(100.0))); // nowhere on this side

    let mut two_lane = synthetic_trip(vec![oneway(0.0, 900.0, 2)], 900.0, 2);
    assert!(two_lane.has_open_adjacent_lane_at(Some(100.0)));

    // The only other lane on a two-lane road, coned off: no escape either.
    two_lane.zones =
        vec![Zone::new(90.0, 120.0, 45.0, "construction").with_closed_side(Some("right"))];
    assert!(!two_lane.has_open_adjacent_lane_at(Some(100.0)));

    // A third lane still leaves somewhere to go once one is closed.
    let mut three_lane = synthetic_trip(vec![oneway(0.0, 900.0, 3)], 900.0, 3);
    three_lane.zones =
        vec![Zone::new(90.0, 120.0, 45.0, "construction").with_closed_side(Some("right"))];
    assert!(three_lane.has_open_adjacent_lane_at(Some(100.0)));
}

/// The side the hazard call names (owner, 2026-09-01) answers to the truck's
/// own lane, the same closure, and the same traffic clearance the L key and
/// the dodge's arrival read -- so it never names a lane a tap would refuse or
/// a sideswipe would punish.
#[test]
fn test_open_side_at_reads_the_trucks_lane_closures_and_traffic() {
    // An empty road at mile 100: every closure and every vehicle below is
    // one the case placed.
    fn empty_at_100(mut trip: Trip) -> Trip {
        trip.position_mi = 100.0;
        trip.zones.clear();
        trip.traffic_manager.rolling_bubble = false;
        trip.set_npc_vehicles(Vec::new());
        trip
    }
    let one_lane = empty_at_100(synthetic_trip(vec![undivided(0.0, 900.0, 2)], 900.0, 1));
    assert_eq!(one_lane.open_side_at(None), OpenSide::Neither);

    // Lane 0 is the right lane, so from there the only neighbour is left.
    let mut two_lane = empty_at_100(synthetic_trip(vec![oneway(0.0, 900.0, 2)], 900.0, 2));
    two_lane.traffic_manager.player_lane = 0;
    assert_eq!(two_lane.open_side_at(None), OpenSide::Left);
    two_lane.traffic_manager.player_lane = 1;
    assert_eq!(two_lane.open_side_at(None), OpenSide::Right);

    // A vehicle riding alongside in that lane holds it.
    two_lane.traffic_manager.player_lane = 0;
    two_lane.set_npc_vehicles(vec![npc(100.02, 1, 60.0)]);
    assert_eq!(two_lane.open_side_at(None), OpenSide::Neither);
    two_lane.set_npc_vehicles(Vec::new());

    // The other lane coned off: nowhere to go, whichever lane the truck is in.
    two_lane.zones =
        vec![Zone::new(90.0, 120.0, 45.0, "construction").with_closed_side(Some("left"))];
    assert_eq!(two_lane.open_side_at(None), OpenSide::Neither);

    // The middle of three: both sides, until one is held or closed.
    let mut three_lane = empty_at_100(synthetic_trip(vec![oneway(0.0, 900.0, 3)], 900.0, 3));
    three_lane.traffic_manager.player_lane = 1;
    assert_eq!(three_lane.open_side_at(None), OpenSide::Either);
    three_lane.set_npc_vehicles(vec![npc(100.02, 2, 60.0)]);
    assert_eq!(three_lane.open_side_at(None), OpenSide::Right);
    three_lane.set_npc_vehicles(Vec::new());
    three_lane.zones =
        vec![Zone::new(90.0, 120.0, 45.0, "construction").with_closed_side(Some("right"))];
    assert_eq!(three_lane.open_side_at(None), OpenSide::Left);
}

#[test]
fn test_riding_a_closed_lane_warns_then_hits_the_barrels() {
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    rolling(&mut d, 55.0);
    d.trip.position_mi = 6.0;
    d.trip
        .zones
        .push(Zone::new(5.0, 9.0, 45.0, "construction").with_closed_lane(Some(1)));
    d.lane.lane = 1;
    let before = d.trip.truck.damage_pct;

    d.update_merge(&mut app.ctx, 0.1);
    assert!(d.merge_deadline.is_some()); // warned, clock running
    assert_eq!(d.trip.truck.damage_pct, before);

    for _ in 0..200 {
        d.update_merge(&mut app.ctx, 0.1);
        if d.merge_deadline.is_none() {
            break;
        }
    }
    assert_eq!(d.lane.lane, 0); // shoved into the open lane
    assert!(d.trip.truck.damage_pct > before);
}

#[test]
fn test_moving_over_in_time_avoids_the_barrels() {
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    rolling(&mut d, 55.0);
    d.trip.position_mi = 6.0;
    d.trip
        .zones
        .push(Zone::new(5.0, 9.0, 45.0, "construction").with_closed_lane(Some(1)));
    d.lane.lane = 1;
    let before = d.trip.truck.damage_pct;

    d.update_merge(&mut app.ctx, 0.1); // warning
    d.lane.lane = 0; // player merges out
    for _ in 0..120 {
        d.update_merge(&mut app.ctx, 0.1);
    }
    assert_eq!(d.trip.truck.damage_pct, before);
}

/// Shane's Detroit-Mansfield trap: the work zone closed the only lane the road
/// had, so there was nowhere legal to be.
#[test]
fn test_a_one_lane_road_never_gets_a_coned_off_lane() {
    for seed in 0..40 {
        // Undivided two-way, so one lane our side for the whole leg.
        let trip = synthetic_trip(vec![undivided(0.0, 900.0, 2)], 900.0, seed);
        assert!(
            closure_footprints(&trip).is_empty(),
            "seed {seed} coned off the only lane"
        );
    }
}

/// A zone that starts on two lanes and ends on one is the same trap.
#[test]
fn test_a_closure_never_straddles_a_stretch_that_narrows() {
    for seed in 0..40 {
        let trip = synthetic_trip(
            vec![
                oneway(0.0, 450.0, 2),      // two lanes our side
                undivided(450.0, 900.0, 2), // one lane our side
            ],
            900.0,
            seed,
        );
        for (start, end) in closure_footprints(&trip) {
            let mut mile = start;
            while mile <= end {
                assert!(
                    trip.lane_count_at(Some(mile)) >= 2,
                    "seed {seed} closes a lane at mile {mile}"
                );
                mile += 0.25;
            }
        }
    }
}

/// The trap in Shane's log: pinned in lane zero on a one-lane stretch, the
/// merge warning fired forever and the barrels took a bite every few seconds.
#[test]
fn test_riding_a_closed_lane_with_nowhere_to_go_is_never_punished() {
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    rolling(&mut d, 55.0);
    d.trip.position_mi = 6.0;
    d.trip
        .zones
        .push(Zone::new(5.0, 9.0, 45.0, "construction").with_closed_lane(Some(0)));
    d.lane.set_lane_count(1); // the road narrowed under the zone
    let before = d.trip.truck.damage_pct;

    for _ in 0..400 {
        d.update_merge(&mut app.ctx, 0.1);
    }

    assert!(d.merge_deadline.is_none());
    assert_eq!(d.trip.truck.damage_pct, before);
    assert!(!logged(&app).iter().any(|text| text.contains("closed")));
}

#[test]
fn test_plowing_the_barrels_costs_a_fine_and_a_serious_violation() {
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    rolling(&mut d, 55.0);
    let (before_money, before_serious) = {
        let p = app.ctx.profile.as_ref().expect("a profile");
        (p.money(), p.driving_record.serious_violations.len())
    };

    let zone = Zone::new(5.0, 9.0, 45.0, "construction").with_closed_lane(Some(1));
    run_into_the_barrels(&mut d, &mut app, &zone);

    // NOT doubled for the zone: the base is already the roadwork penalty
    // (RSMo 304.585), so doubling would charge twice for the same fact.
    let expected = citation_fine(WORK_ZONE_BARRELS_FINE, 0, false, None);
    assert_eq!(expected, WORK_ZONE_BARRELS_FINE);
    let p = app.ctx.profile.as_ref().expect("a profile");
    assert_eq!(p.money(), before_money - expected);
    assert_eq!(d.ticket_fines_paid, expected);
    assert_eq!(
        p.driving_record.serious_violations.len(),
        before_serious + 1
    );
}

#[test]
fn test_the_barrel_citation_says_the_charged_figure_and_why() {
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    rolling(&mut d, 55.0);
    let zone = Zone::new(5.0, 9.0, 45.0, "construction").with_closed_lane(Some(1));
    run_into_the_barrels(&mut d, &mut app, &zone);

    let expected = citation_fine(WORK_ZONE_BARRELS_FINE, 0, false, None);
    let cited: Vec<String> = logged(&app)
        .into_iter()
        .filter(|s| s.contains("through the barrels is a citation"))
        .collect();
    assert!(!cited.is_empty());
    assert!(cited[0].contains(&format!(
        "{} dollars",
        ff_core::pyfmt::fmt_grouped(expected, 0)
    )));
    // It must NOT claim a doubling it did not apply.
    assert!(!cited[0].contains("doubled"));
}

#[test]
fn test_the_barrel_citation_escalates_for_a_repeat_offender() {
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    rolling(&mut d, 55.0);
    let before_money = {
        let p = app.ctx.profile.as_mut().expect("a profile");
        p.driving_record.citations = 2;
        p.money()
    };

    let zone = Zone::new(5.0, 9.0, 45.0, "construction").with_closed_lane(Some(1));
    run_into_the_barrels(&mut d, &mut app, &zone);

    // Priors still escalate it, up to the repeat cap; the zone does not.
    let expected = citation_fine(WORK_ZONE_BARRELS_FINE, 2, false, None);
    assert_eq!(expected, WORK_ZONE_BARRELS_FINE * 2.0);
    assert_eq!(
        app.ctx.profile.as_ref().unwrap().money(),
        before_money - expected
    );
}

/// Two strikes in one closure is one refusal to merge, so one citation -- the
/// tester's log caught the barrels twice in eight seconds.
#[test]
fn test_the_barrel_fine_is_charged_once_per_work_zone() {
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    rolling(&mut d, 55.0);
    let before_money = app.ctx.profile.as_ref().expect("a profile").money();
    let zone = Zone::new(5.0, 9.0, 45.0, "construction").with_closed_lane(Some(1));

    run_into_the_barrels(&mut d, &mut app, &zone);
    let damage_after_one = d.trip.truck.damage_pct;
    let position = d
        .trip
        .zones
        .iter()
        .position(|z| *z == zone)
        .expect("the zone");
    d.trip.zones.remove(position);
    run_into_the_barrels(&mut d, &mut app, &zone);

    let expected = citation_fine(WORK_ZONE_BARRELS_FINE, 0, false, None);
    assert_eq!(
        app.ctx.profile.as_ref().unwrap().money(),
        before_money - expected
    );
    assert!(d.trip.truck.damage_pct > damage_after_one); // the truck still pays
}

/// The bug charging for itself would be worse than the bug: a driver with
/// nowhere to merge must not lose a dollar.
#[test]
fn test_no_open_lane_means_no_fine() {
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    rolling(&mut d, 55.0);
    let (before_money, before_serious) = {
        let p = app.ctx.profile.as_ref().expect("a profile");
        (p.money(), p.driving_record.serious_violations.len())
    };
    d.trip.position_mi = 6.0;
    d.trip
        .zones
        .push(Zone::new(5.0, 9.0, 45.0, "construction").with_closed_lane(Some(0)));
    d.lane.set_lane_count(1);

    for _ in 0..400 {
        d.update_merge(&mut app.ctx, 0.1);
    }
    // Even reached directly, the citation refuses to write itself.
    let zone = d.trip.zones.last().cloned().expect("the zone");
    d.cite_barrel_strike(&mut app.ctx, &zone);

    let p = app.ctx.profile.as_ref().expect("a profile");
    assert_eq!(p.money(), before_money);
    assert_eq!(d.ticket_fines_paid, 0.0);
    assert_eq!(p.driving_record.serious_violations.len(), before_serious);
}

/// The other half of Shane's trap. He was told the right lane was closed and
/// moved left; where the road drops a lane the count clamp renumbers the lanes
/// under the truck, and the lane he had moved into became the closed one. The
/// road moved him, so the road moves him back out -- no barrels, no citation,
/// and he is told what happened.
#[test]
fn test_a_narrowing_road_never_leaves_the_truck_in_the_closed_lane() {
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    rolling(&mut d, 55.0);
    d.trip.position_mi = 6.0;
    d.trip
        .zones
        .push(Zone::new(5.0, 9.0, 45.0, "construction").with_closed_side(Some("left")));
    d.lane.set_lane_count(3);
    d.lane.lane = 1; // the middle lane, open: the left one is coned off
    d.leave_a_lane_the_road_closed(&mut app.ctx); // seed the lane count it has seen
    let before = d.trip.truck.damage_pct;
    app.clear_speech();
    let log_from = app.ctx.message_log.messages.len();

    d.lane.set_lane_count(2); // the road drops a lane under the truck
    assert_eq!(d.trip.closed_lane_at(None, Some(2)), Some(1)); // which is where it now is
    d.leave_a_lane_the_road_closed(&mut app.ctx);

    assert_eq!(d.lane.lane, 0);
    assert!(d.merge_deadline.is_none());
    assert_eq!(d.trip.truck.damage_pct, before);
    let said = &logged(&app)[log_from..];
    assert!(!said.is_empty());
    let last = said.last().unwrap();
    assert_eq!(last, "Left lane closed. In the right lane.");
}

/// Darren's report, 2026-08-14: a highway narrowing to one lane with no work
/// zone at all moved the truck without a word -- `set_lane_count` clamps the
/// lane index silently. This is the same forced-move trap as the closure case
/// above, minus the cones, so it gets the same never-dropped treatment
/// (interrupt=true, the closure call's own mechanism).
#[test]
fn test_a_narrowing_road_with_no_closure_says_the_forced_move() {
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    rolling(&mut d, 55.0);
    d.lane.set_lane_count(2);
    d.lane.lane = 1; // the left lane, which is about to stop existing
    d.leave_a_lane_the_road_closed(&mut app.ctx); // seed the lane count it has seen
    app.clear_speech();

    d.lane_before_narrow = Some(d.lane.lane); // what update_lane captures pre-clamp
    d.lane.set_lane_count(1); // the road itself narrows to one lane
    d.leave_a_lane_the_road_closed(&mut app.ctx);

    assert_eq!(d.lane.lane, 0);
    let calls = app.event_calls();
    assert_eq!(calls.len(), 1);
    let (text, interrupt) = &calls[0];
    // Narrowing to ONE lane already tells the driver which lane they are in,
    // so the line names no side that does not exist any more (Cary,
    // 2026-08-15).
    assert_eq!(text, "The road narrows to one lane.");
    assert!(!text.contains("right lane"));
    assert!(*interrupt); // never-dropped, same family as the closure call
    assert!(logged(&app).contains(text));
}

/// The clamp only moves a truck sitting in a lane that stops existing. Already
/// in the lane that survives the narrowing, nothing was forced and nothing
/// needs saying.
#[test]
fn test_a_narrowing_road_stays_silent_when_the_truck_already_survives() {
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    rolling(&mut d, 55.0);
    d.lane.set_lane_count(2);
    d.lane.lane = 0; // already in the right lane, which survives
    d.leave_a_lane_the_road_closed(&mut app.ctx); // seed the lane count it has seen
    app.clear_speech();
    let before_log = app.ctx.message_log.messages.len();

    d.lane_before_narrow = Some(d.lane.lane);
    d.lane.set_lane_count(1); // the road narrows, but the truck never moves
    d.leave_a_lane_the_road_closed(&mut app.ctx);

    assert_eq!(d.lane.lane, 0);
    assert!(app.event_calls().is_empty());
    assert_eq!(app.ctx.message_log.messages.len(), before_log);
}

/// The rescue above must not become a free pass: with the road unchanged,
/// riding the cones still earns the warning and the barrels.
#[test]
fn test_a_lane_the_driver_steered_into_is_still_theirs_to_answer_for() {
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    rolling(&mut d, 55.0);
    d.trip.position_mi = 6.0;
    d.trip
        .zones
        .push(Zone::new(5.0, 9.0, 45.0, "construction").with_closed_side(Some("left")));
    d.leave_a_lane_the_road_closed(&mut app.ctx);
    d.lane.lane = 1; // steered into the closed lane

    d.leave_a_lane_the_road_closed(&mut app.ctx);
    assert_eq!(d.lane.lane, 1); // nothing moved under the truck

    d.update_merge(&mut app.ctx, 0.1);
    assert!(d.merge_deadline.is_some());
}

/// A lane change takes seconds; the cones can arrive inside them. The
/// completion used to commit the move whatever had happened meanwhile.
#[test]
fn test_a_tap_change_refuses_a_lane_that_closed_on_the_way_over() {
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    rolling(&mut d, 55.0);
    d.trip.position_mi = 6.0;
    d.tap_lane_change(&mut app.ctx, 1);
    assert_eq!(d.lane_change_target, Some(1));

    // The work zone starts under the truck while the change is running.
    d.trip
        .zones
        .push(Zone::new(5.0, 9.0, 45.0, "construction").with_closed_side(Some("left")));
    app.clear_speech();
    for _ in 0..40 {
        d.update_tap_lane_change(&mut app.ctx, 0.1);
    }

    assert_eq!(d.lane.lane, 0);
    assert_eq!(d.lane_change_target, None);
    let said = logged(&app);
    assert!(said
        .last()
        .is_some_and(|s| s.contains("left lane is closed")));
}

/// The merge taper is where the closure starts. Riding it used to be silent
/// all the way to the first barrel.
#[test]
fn test_the_taper_warns_a_truck_in_the_lane_that_is_closing() {
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    rolling(&mut d, 55.0);
    d.trip.position_mi = 4.5;
    d.trip
        .zones
        .push(Zone::new(4.0, 5.0, 55.0, "construction merge").with_closed_side(Some("left")));
    d.lane.lane = 1; // riding the lane that closes at the barrels
    let before = d.trip.truck.damage_pct;
    let log_from = app.ctx.message_log.messages.len();

    for _ in 0..200 {
        d.update_merge(&mut app.ctx, 0.1);
    }

    let said = logged(&app)[log_from..].to_vec();
    assert!(said
        .first()
        .is_some_and(|s| s.contains("closes at the work zone ahead")));
    assert_eq!(
        said.iter()
            .filter(|s| s.contains("closes at the work zone"))
            .count(),
        1
    );
    assert!(d.merge_deadline.is_none()); // the barrel clock belongs to the work zone
    assert_eq!(d.trip.truck.damage_pct, before);
}

#[test]
fn test_the_taper_refuses_a_move_into_the_lane_that_is_closing() {
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    rolling(&mut d, 55.0);
    d.trip.position_mi = 4.5;
    d.trip
        .zones
        .push(Zone::new(4.0, 5.0, 55.0, "construction merge").with_closed_side(Some("left")));
    app.clear_speech();

    d.tap_lane_change(&mut app.ctx, 1);
    assert_eq!(d.lane_change_target, None);
    assert_eq!(d.lane.lane, 0);
    assert_eq!(
        app.main_lines(),
        vec!["The left lane closes at the work zone ahead."]
    );
}

#[test]
fn test_tap_change_refuses_the_closed_lane() {
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    rolling(&mut d, 55.0);
    d.trip.position_mi = 6.0;
    d.trip
        .zones
        .push(Zone::new(5.0, 9.0, 45.0, "construction").with_closed_lane(Some(1)));
    d.tap_lane_change(&mut app.ctx, 1);
    assert_eq!(d.lane_change_target, None);
    assert_eq!(d.lane.lane, 0);
}

// -- Tap lane changes (steering assist off) --------------------------------------------

#[test]
fn test_tap_lane_change_completes_after_the_timed_drift() {
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    rolling(&mut d, 60.0);
    assert_eq!(d.lane.lane, 0);
    d.tap_lane_change(&mut app.ctx, 1);
    assert_eq!(d.lane_change_target, Some(1));
    assert_eq!(d.lane.lane, 0); // not there yet
    for _ in 0..40 {
        d.update_tap_lane_change(&mut app.ctx, 0.1);
    }
    assert_eq!(d.lane.lane, 1);
    assert_eq!(d.lane_change_target, None);
}

#[test]
fn test_tap_lane_change_needs_speed_and_a_real_lane() {
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    d.tap_lane_change(&mut app.ctx, 1); // engine off, parked
    assert_eq!(d.lane_change_target, None);
    rolling(&mut d, 60.0);
    d.tap_lane_change(&mut app.ctx, -1); // already in the right lane
    assert_eq!(d.lane_change_target, None);
}

/// Answering "you are already in the right lane" to someone asking to go left
/// tells them nothing about why they cannot.
#[test]
fn test_asking_for_a_lane_the_road_does_not_have_names_that_side() {
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    rolling(&mut d, 60.0);
    d.lane.set_lane_count(1);
    app.clear_speech();

    d.tap_lane_change(&mut app.ctx, 1);
    assert_eq!(d.lane_change_target, None);
    assert_eq!(app.main_lines(), vec!["No lane to your left here."]);

    app.clear_speech();
    d.tap_lane_change(&mut app.ctx, -1);
    assert_eq!(app.main_lines(), vec!["No lane to your right here."]);
}

// -- Hazard dodges and sideswipes ------------------------------------------------------

#[test]
fn test_changing_lanes_dodges_a_dodgeable_hazard() {
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    rolling(&mut d, 60.0);
    d.hazard_deadline = Some(5.0);
    d.hazard_dodgeable = true;
    d.hazard_lane = 0;
    d.lane.lane = 1; // the change just landed
    d.finish_lane_change(&mut app.ctx, false);
    assert!(d.hazard_deadline.is_none());
}

#[test]
fn test_a_brake_only_hazard_cannot_be_dodged() {
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    rolling(&mut d, 60.0);
    d.hazard_deadline = Some(5.0);
    d.hazard_dodgeable = false;
    d.hazard_lane = 0;
    d.lane.lane = 1;
    d.finish_lane_change(&mut app.ctx, false);
    assert!(d.hazard_deadline.is_some());
}

#[test]
fn test_swerving_into_occupied_space_is_a_sideswipe() {
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    rolling(&mut d, 60.0);
    d.hazard_deadline = Some(5.0);
    d.hazard_dodgeable = true;
    d.hazard_lane = 0;
    let at = d.trip.position_mi + 0.1;
    d.trip.set_npc_vehicles(vec![npc(at, 1, 50.0)]);
    let before = d.trip.truck.damage_pct;
    d.lane.lane = 1;
    d.finish_lane_change(&mut app.ctx, false);
    assert!(d.trip.truck.damage_pct > before);
    assert!(d.hazard_deadline.is_some()); // the hazard is still coming
}

/// Pinballing across the same line is one sideswipe, not three.
///
/// Tester transcript, 2026-08-11: "You sideswiped a box truck in the right
/// lane! The truck took damage, now 13 percent." three times inside six
/// tenths of a second, for one brush against one vehicle -- and the damage
/// charged three times with it.
#[test]
fn test_one_contact_is_billed_and_announced_once() {
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    rolling(&mut d, 60.0);
    let at = d.trip.position_mi + 0.1;
    d.trip.set_npc_vehicles(vec![npc(at, 1, 50.0)]);
    app.clear_speech();
    let log_from = app.ctx.message_log.messages.len();
    d.lane.lane = 1;
    d.finish_lane_change(&mut app.ctx, false);
    let after_first = d.trip.truck.damage_pct;
    // The tires roll the markers again as the truck settles back over.
    d.finish_lane_change(&mut app.ctx, false);
    d.finish_lane_change(&mut app.ctx, false);
    assert_eq!(logged(&app)[log_from..].len(), 1);
    assert_eq!(d.trip.truck.damage_pct, after_first);
}

/// The guard is a cooldown, not a once-per-trip latch.
#[test]
fn test_a_later_sideswipe_is_its_own_contact() {
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    rolling(&mut d, 60.0);
    let at = d.trip.position_mi + 0.1;
    d.trip.set_npc_vehicles(vec![npc(at, 1, 50.0)]);
    app.clear_speech();
    let log_from = app.ctx.message_log.messages.len();
    d.lane.lane = 1;
    d.finish_lane_change(&mut app.ctx, false);
    d.sideswipe_cooldown_s = 0.0; // seconds later, moving over again
    d.finish_lane_change(&mut app.ctx, false);
    assert_eq!(logged(&app)[log_from..].len(), 2);
}

#[test]
fn test_hazard_event_records_dodge_context() {
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    rolling(&mut d, 60.0);
    let event = TripEvent {
        kind: TripEventKind::Hazard,
        message: "Change lanes or brake! Debris on the road.".into(),
        data: TripEventData {
            deadline_s: Some(4.0),
            dodgeable: Some(true),
            ..Default::default()
        },
    };
    d.handle_trip_event(&mut app.ctx, &event);
    assert!(d.hazard_deadline.is_some());
    assert!(d.hazard_dodgeable);
    assert_eq!(d.hazard_lane, d.lane.lane);
}

// -- Hazard wording must not offer a lane change nobody can make -----------------------

/// Manual playtest, US-285 toward Denver, 2026-08-12: one lane your side, and
/// the lead-vehicle warning still said "Change lanes or brake!" -- an escape
/// the road never offered. The lead-vehicle branch must ask the same lane
/// authority a real lane change answers to.
///
/// Python stubbed `trip.traffic_context`; there is no such seam here, so the
/// lead is a real vehicle placed to produce the same context (0.01 mi ahead,
/// 20 mph slower, brake lights on).
#[test]
fn test_traffic_pressure_hazard_says_brake_only_with_no_lane_to_swerve_into() {
    let mut trip = synthetic_trip(vec![undivided(0.0, 900.0, 2)], 900.0, 3);
    trip.position_mi = 50.0;
    trip.traffic_warning_mi = 0.0;
    trip.truck.velocity_mps = 50.0 / MPH_PER_MPS;
    let mut lead = npc(50.01, 0, 30.0);
    lead.intent = "braking".to_string();
    trip.set_npc_vehicles(vec![lead]);

    trip.check_hazards(1.0);

    let events: Vec<&TripEvent> = trip
        .events
        .iter()
        .filter(|e| e.kind == TripEventKind::Hazard)
        .collect();
    assert!(!events.is_empty());
    assert_eq!(
        events[0].text(),
        "Brake! Brake lights right ahead. No lane open."
    );
    assert!(!events[0].text().contains("change lanes"));
    // And the physics is told the same thing the words were: there is
    // nowhere to go, so no lane-change allowance is added to the driver's
    // window. It is still a thing in our lane, which is what decides the
    // speed brake alone has to reach.
    assert_eq!(events[0].data.dodgeable, Some(false));
    assert_eq!(events[0].data.in_lane, Some(true));
}

/// Same lead-vehicle warning, but on a road with somewhere to go: the wording
/// is unchanged from before this fix.
#[test]
fn test_traffic_pressure_hazard_keeps_the_lane_offer_when_one_exists() {
    let mut trip = synthetic_trip(vec![oneway(0.0, 900.0, 2)], 900.0, 4);
    trip.position_mi = 50.0;
    trip.traffic_warning_mi = 0.0;
    trip.truck.velocity_mps = 50.0 / MPH_PER_MPS;
    let mut lead = npc(50.01, 0, 30.0);
    lead.intent = "braking".to_string();
    trip.set_npc_vehicles(vec![lead]);

    trip.check_hazards(1.0);

    let events: Vec<&TripEvent> = trip
        .events
        .iter()
        .filter(|e| e.kind == TripEventKind::Hazard)
        .collect();
    assert!(!events.is_empty());
    assert_eq!(
        events[0].text(),
        "Change lanes or brake! Brake lights right ahead. Left lane open."
    );
}

/// The fixed-object hazard family (debris, a stopped vehicle) gets the same
/// treatment as the lead-vehicle warning: no lane, no offer.
///
/// Python stubbed `eligible_hazards` to hand back debris and `_hazard_risk` to
/// make the draw certain. Neither is a seam here: the hazard scale makes the
/// draw certain, and the seed is searched until the weighted pick lands on the
/// same hazard the Python case named.
#[test]
fn test_random_dodgeable_hazard_says_brake_only_with_no_lane_to_swerve_into() {
    // The synthetic route's own cities are not in the world, so the region
    // lookup `check_hazards` makes on the way to the hazard pool cannot
    // resolve; real endpoints stand in for Python's pinned region name.
    let corridor = get_world()
        .supported_route("Chicago", "St. Louis", None)
        .expect("the world routes")
        .expect("Chicago to St. Louis has a route");
    let (from, to) = (
        corridor.cities[0].clone(),
        corridor.cities.last().unwrap().clone(),
    );
    for seed in 0..400 {
        let mut trip =
            synthetic_trip_between(&from, &to, vec![undivided(0.0, 900.0, 2)], 900.0, seed, 4.0);
        trip.position_mi = 50.0;
        trip.hazard_check_mi = 0.0;
        trip.weather.current = WeatherKind::Clear;

        trip.check_hazards(0.0);

        let Some(event) = trip.events.iter().find(|e| e.kind == TripEventKind::Hazard) else {
            continue;
        };
        if !event.text().contains("Debris on the road") {
            continue;
        }
        assert_eq!(event.text(), "Brake! Debris on the road. No lane open.");
        // Same rule for an object as for a vehicle: no lane, no dodge. The
        // debris is still in our lane, so brake alone still owes the near
        // stop -- that is `in_lane`, not `dodgeable`.
        assert_eq!(event.data.dodgeable, Some(false));
        assert_eq!(event.data.in_lane, Some(true));
        return;
    }
    panic!("no seed in 0..400 drew debris on the road");
}

/// One lane your side: the lingering hint must not offer a lane change, and a
/// lane-change refusal must never be required to clear the hazard -- slowing
/// alone, with no lane change ever attempted, still resolves it and earns the
/// achievement.
#[test]
fn test_hazard_hint_and_clearing_need_no_lane_when_there_is_none() {
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    // Python stubbed `trip.has_open_adjacent_lane_at`; here the drive is put
    // on a road that really has one lane our side, which is what that stub
    // stood for.
    d.replace_trip(synthetic_trip(vec![undivided(0.0, 900.0, 2)], 900.0, 6));
    assert!(!d.trip.has_open_adjacent_lane_at(None));
    rolling(&mut d, 65.0);
    d.hazard_deadline = Some(5.0);
    // One lane our side: an object in it is not dodgeable, and the emitter
    // says so now. It is still in the lane, which is what the hint and the
    // near stop both key on.
    d.hazard_dodgeable = false;
    d.hazard_in_lane = true;
    d.hazard_lane = d.lane.lane;
    d.hazard_slow_hint_said = false;
    d.automatic_braking_announced = false;
    app.clear_speech();
    let log_from = app.ctx.message_log.messages.len();

    // Slow past the old moving-hazard speed: the hint fires once, and never
    // names a lane change.
    d.trip.truck.velocity_mps = (HAZARD_SAFE_MPH - 1.0) / MPH_PER_MPS;
    d.update_hazard(&mut app.ctx, 1.0 / 60.0);
    let said = logged(&app)[log_from..].to_vec();
    assert_eq!(
        said.iter()
            .filter(|s| s.as_str() == "It is still in your lane. Nearly stop.")
            .count(),
        1
    );
    assert!(!said.iter().any(|s| s.contains("change lanes")));

    // Never changed lanes: nearly stopping alone still clears it.
    d.trip.truck.velocity_mps = (HAZARD_CREEP_MPH - 1.0) / MPH_PER_MPS;
    d.update_hazard(&mut app.ctx, 1.0 / 60.0);
    assert!(d.hazard_deadline.is_none());
    assert!(app
        .ctx
        .profile
        .as_ref()
        .expect("a profile")
        .achievements
        .contains(&"hazard_avoided".to_string()));
}

// -- Keep right except to pass ---------------------------------------------------------

#[test]
fn test_camping_the_left_lane_draws_a_cb_nag() {
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    rolling(&mut d, 60.0);
    d.lane.lane = 1;
    for _ in 0..50 {
        d.update_keep_right(&mut app.ctx, 1.0);
    }
    assert_eq!(d.keep_right_nags, 1);
    // The grumble is CB chatter, so Alt C brings it back word for word --
    // there is no distance in it to go stale (issue 156).
    let recalled = d
        .last_cb_chatter
        .as_ref()
        .expect("the nag is repeatable")
        .clone();
    assert!(
        recalled
            .text
            .starts_with("You have been riding the left lane"),
        "{}",
        recalled.text
    );
    assert!(recalled.text.contains("Keep right except to pass"));
    assert!(recalled.post.is_none());
    app.clear_speech();
    d.handle_key_event(&mut app.ctx, &InputEvent::key_mods(Key::C, Mods::ALT));
    assert_eq!(
        app.main_lines().last().cloned().unwrap_or_default(),
        recalled.text
    );
    // Dropping back right resets the pressure.
    d.lane.lane = 0;
    d.update_keep_right(&mut app.ctx, 1.0);
    assert_eq!(d.keep_right_nags, 0);
    assert_eq!(d.left_lane_s, 0.0);
}

#[test]
fn test_left_lane_is_fine_while_passing() {
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    rolling(&mut d, 60.0);
    d.lane.lane = 1;
    // Slower traffic in the right lane just ahead: a legitimate pass.
    let at = d.trip.position_mi + 0.3;
    d.trip.set_npc_vehicles(vec![npc(at, 0, 45.0)]);
    for _ in 0..50 {
        d.update_keep_right(&mut app.ctx, 1.0);
    }
    assert_eq!(d.keep_right_nags, 0);
}

#[test]
fn test_left_lane_is_fine_when_the_right_lane_is_coned_off() {
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    rolling(&mut d, 50.0);
    d.trip.position_mi = 6.0;
    d.trip
        .zones
        .push(Zone::new(5.0, 9.0, 45.0, "construction").with_closed_lane(Some(0)));
    d.lane.lane = 1;
    for _ in 0..50 {
        d.update_keep_right(&mut app.ctx, 1.0);
    }
    assert_eq!(d.keep_right_nags, 0);
}

// -- Exits leave from the right lane ---------------------------------------------------

#[test]
fn test_exit_readiness_requires_the_right_lane() {
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    d.exit_lane_entered = true;
    assert!(d.exit_lane_ready());
    d.lane.lane = 1;
    assert!(!d.exit_lane_ready());
    d.lane_change_target = Some(0); // already moving back right at the gore
    assert!(d.exit_lane_ready());
}

#[test]
fn test_snapshot_round_trips_the_lane_index() {
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    d.lane.lane = 1;
    let snap = d.snapshot(&app.ctx);
    assert_eq!(snap["lane_index"], serde_json::json!(1));
}

/// Lane keeping on full holds the lane, so the left stick cannot steer; a
/// push past halfway asks for one lane change, and only one until the stick
/// comes back near the center. A controller driver had no way to change
/// lanes on full at all.
#[test]
fn test_a_stick_push_on_full_lane_keeping_changes_one_lane() {
    let mut app = TestApp::new();
    app.ctx.settings.lane_keeping = "full".to_string();
    let mut d = a_drive(&mut app);
    rolling(&mut d, 55.0);
    d.trip.position_mi = 6.0;
    d.lane.lane = 0;

    d.update_stick_lane_change(&mut app.ctx, -0.3); // not far enough yet
    assert_eq!(d.lane_change_target, None);
    d.update_stick_lane_change(&mut app.ctx, -0.8);
    assert_eq!(d.lane_change_target, Some(1)); // left, as the Left arrow asks
    for _ in 0..40 {
        d.update_tap_lane_change(&mut app.ctx, 0.1);
    }
    assert_eq!(d.lane.lane, 1);

    // Still held over: no second change.
    d.update_stick_lane_change(&mut app.ctx, -0.9);
    assert_eq!(d.lane_change_target, None);
    // Back to the center, then right.
    d.update_stick_lane_change(&mut app.ctx, 0.0);
    d.update_stick_lane_change(&mut app.ctx, 0.7);
    assert_eq!(d.lane_change_target, Some(0));
}
