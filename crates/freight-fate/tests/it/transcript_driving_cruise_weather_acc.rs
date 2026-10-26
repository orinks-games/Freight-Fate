//! Hazard cancellation and adaptive cruise (port of
//! `tests/test_driving_cruise_weather.py`, lines 928-1817).
//!
//! Second of the split; see `transcript_driving_cruise_weather.rs` for why the
//! Python file is split and `transcript_cruise_support` for what replaced each
//! monkeypatch.

use ff_core::sim::trip_models::{NPCVehicle, TripEvent, TripEventData, TripEventKind, Zone};
use ff_core::sim::trip_route_helpers::zone_key;
use ff_core::sim::weather::WeatherKind;
use freight_fate::playtest::harness::PlaytestHarness;
use freight_fate::states::base::Key;
use freight_fate::states::driving_core::{ACC_LIMIT_OFFSET_MPH, LANE_TAP_CHANGE_S, MPH_PER_MPS};

use crate::transcript_cruise_support::*;

/// `driving.trip.speed_limit_at = lambda mile: (limit, reason)` where the
/// patch returned a REASON: only a zone does that.
fn post_zone(harness: &mut PlaytestHarness, limit: f64, reason: &str) {
    let reason = reason.to_string();
    harness.with_drive(move |d, _| {
        d.trip.zones = vec![Zone::new(0.0, 1e6, limit, &reason)];
    });
}

fn hazard_event(message: &str, data: TripEventData) -> TripEvent {
    TripEvent {
        kind: TripEventKind::Hazard,
        message: message.into(),
        data,
    }
}

#[test]
fn test_cruise_control_requires_road_speed_and_cancels_on_hazard() {
    let mut harness = bench_drive("Hazard Cancel", 200.0, 0.0);
    // parked: refuses to engage
    press(&mut harness, Key::K, None);
    assert!(harness.read_drive(|d| d.cruise_mph).is_none());
    // engaged at speed, a hazard hands control back to the driver
    press(&mut harness, Key::E, None);
    harness.with_drive(|d, _| {
        d.truck_mut().transmission.gear = 10;
        d.truck_mut().velocity_mps = 26.8;
    });
    press(&mut harness, Key::K, None);
    assert!(harness.read_drive(|d| d.cruise_mph).is_some());
    harness.with_drive(|d, ctx| {
        d.handle_trip_event(
            ctx,
            &hazard_event(
                "Brake now!",
                TripEventData {
                    deadline_s: Some(4.0),
                    ..Default::default()
                },
            ),
        )
    });
    assert!(harness.read_drive(|d| d.cruise_mph).is_none());
}

#[test]
fn test_hazard_announces_speed_control_pause_once() {
    // Paused, not disarmed (owner ruling, 2026-09-01: a highway hazard must
    // not disable cruise): the controllers drop so the driver brakes, the
    // session stays armed, and the line says so once.
    let mut harness = bench_drive("Hazard Once", 200.0, 0.0);
    press(&mut harness, Key::E, None);
    harness.with_drive(|d, _| {
        d.truck_mut().transmission.gear = 10;
        d.truck_mut().velocity_mps = 26.8;
    });
    press(&mut harness, Key::K, None);
    harness.clear_speech();

    harness.with_drive(|d, ctx| {
        d.handle_trip_event(
            ctx,
            &hazard_event(
                "Brake now!",
                TripEventData {
                    deadline_s: Some(4.0),
                    ..Default::default()
                },
            ),
        )
    });

    assert!(harness.read_drive(|d| d.speed_control_armed));
    assert!(harness.read_drive(|d| d.cruise_mph).is_none());
    let said = last(&harness);
    assert!(said.starts_with("Brake now!"), "{said}");
    assert_eq!(said.matches("Automatic speed control paused.").count(), 1);
    assert!(!said.contains("canceled"), "{said}");
}

#[test]
fn test_dodgeable_hazard_leaves_cruise_armed_through_the_lane_change_dodge() {
    // Shane's report, 2026-08-14: with adaptive cruise on, dodging a dodgeable
    // hazard by changing lanes killed the whole session outright -- not just
    // easing off for the lane being left, which is the narrower bug 3cbdcffb
    // fixed. Only braking, the driver's own or the automatic brake taking
    // over, may cancel cruise; a lane change that answers the hazard must not.
    let mut harness = bench_drive("Dodge Cruise", 200.0, 0.0);
    press(&mut harness, Key::E, None);
    harness.with_drive(|d, _| {
        d.truck_mut().transmission.gear = 10;
        d.truck_mut().velocity_mps = 26.8; // ~60 mph
    });
    press(&mut harness, Key::K, None);
    assert!(harness.read_drive(|d| d.cruise_mph).is_some());
    assert_eq!(harness.read_drive(|d| d.lane.lane), 0);

    harness.with_drive(|d, ctx| {
        d.handle_trip_event(
            ctx,
            &hazard_event(
                "A deer is in the road.",
                TripEventData {
                    name: Some("deer".to_string()),
                    dodgeable: Some(true),
                    ..Default::default()
                },
            ),
        )
    });
    // A dodgeable hazard alone never hands the pedal back -- that is what the
    // lane change below is for.
    assert!(harness.read_drive(|d| d.cruise_mph).is_some());
    assert!(harness.read_drive(|d| d.hazard_deadline).is_some());

    harness.with_drive(|d, ctx| d.tap_lane_change(ctx, 1)); // dodge into the open lane
    for _ in 0..((LANE_TAP_CHANGE_S * 60.0) as usize + 5) {
        frame(&mut harness, DT);
        assert!(harness.read_drive(|d| d.cruise_mph).is_some()); // never drops mid-maneuver
    }

    assert_eq!(harness.read_drive(|d| d.lane.lane), 1);
    assert!(harness.read_drive(|d| d.lane_change_target).is_none());
    assert!(harness.read_drive(|d| d.cruise_mph).is_some());
}

#[test]
fn test_dodgeable_hazard_leaves_the_keeper_armed_through_the_lane_change_dodge() {
    // The speed keeper shares `disarm_speed_control` with cruise, so the same
    // dodge that must not kill adaptive cruise must not kill the keeper
    // either.
    let mut harness = bench_drive("Dodge Keeper", 200.0, 0.0);
    // A real construction zone the whole way, so the keeper holds itself
    // rather than handing straight back to cruise the moment the road under
    // the wheels reads as open (that switch is `update_keeper`'s own job, not
    // this test's).
    post_zone(&mut harness, 25.0, "construction");
    press(&mut harness, Key::E, None);
    harness.with_drive(|d, ctx| {
        d.truck_mut().transmission.gear = 6;
        d.truck_mut().velocity_mps = 11.2; // ~25 mph
        d.engage_keeper(ctx, 30.0, "construction", Some(25.0), false);
    });
    assert!(harness.read_drive(|d| d.keeper_mph).is_some());
    assert_eq!(harness.read_drive(|d| d.lane.lane), 0);

    harness.with_drive(|d, ctx| {
        d.handle_trip_event(
            ctx,
            &hazard_event(
                "A deer is in the road.",
                TripEventData {
                    name: Some("deer".to_string()),
                    dodgeable: Some(true),
                    ..Default::default()
                },
            ),
        )
    });
    assert!(harness.read_drive(|d| d.keeper_mph).is_some()); // announcement alone spares it
    assert!(harness.read_drive(|d| d.hazard_deadline).is_some());

    harness.with_drive(|d, ctx| d.tap_lane_change(ctx, 1)); // dodge into the open lane
    for _ in 0..((LANE_TAP_CHANGE_S * 60.0) as usize + 5) {
        frame(&mut harness, DT);
        assert!(harness.read_drive(|d| d.keeper_mph).is_some()); // never drops mid-maneuver
    }

    assert_eq!(harness.read_drive(|d| d.lane.lane), 1);
    assert!(harness.read_drive(|d| d.lane_change_target).is_none());
    assert!(harness.read_drive(|d| d.keeper_mph).is_some());
}

#[test]
fn test_driver_braking_still_cancels_cruise_during_a_dodge() {
    // The other half of Shane's contract: a lane change never cancels cruise,
    // but the driver's own brake still does, mid-dodge or not.
    let mut harness = bench_drive("Dodge Brake", 200.0, 0.0);
    press(&mut harness, Key::E, None);
    harness.with_drive(|d, _| {
        d.truck_mut().transmission.gear = 10;
        d.truck_mut().velocity_mps = 26.8;
    });
    press(&mut harness, Key::K, None);
    assert!(harness.read_drive(|d| d.cruise_mph).is_some());

    harness.with_drive(|d, ctx| {
        d.handle_trip_event(
            ctx,
            &hazard_event(
                "A deer is in the road.",
                TripEventData {
                    name: Some("deer".to_string()),
                    dodgeable: Some(true),
                    ..Default::default()
                },
            ),
        );
        d.tap_lane_change(ctx, 1);
    });
    assert!(harness.read_drive(|d| d.cruise_mph).is_some()); // still armed, mid-dodge

    hold(&mut harness, &[Key::Down]);
    frame(&mut harness, DT);
    assert!(harness.read_drive(|d| d.cruise_mph).is_none());
}

#[test]
fn test_an_ignored_dodgeable_hazard_still_ends_cruise_at_the_deadline() {
    // Reviewer-caught regression on the announce-time fix above: with the
    // automatic brake turned OFF, a dodgeable hazard the driver never answers
    // -- no dodge, no brake -- used to ride cruise straight into the collision
    // with the session still showing armed. Only braking may cancel cruise,
    // but the deadline lapsing un-dodged IS the collision, which is the third
    // way the promise ends -- whatever the AEB setting.
    let mut harness = bench_drive("Ignored Hazard", 200.0, 0.0);
    release_keys(&mut harness);
    harness.app.ctx.settings.automatic_emergency_braking = false;
    press(&mut harness, Key::E, None);
    harness.with_drive(|d, _| {
        d.truck_mut().transmission.gear = 10;
        d.truck_mut().velocity_mps = 26.8;
    });
    press(&mut harness, Key::K, None);
    assert!(harness.read_drive(|d| d.cruise_mph).is_some());

    harness.with_drive(|d, ctx| {
        d.handle_trip_event(
            ctx,
            &hazard_event(
                "A deer is in the road.",
                TripEventData {
                    name: Some("deer".to_string()),
                    dodgeable: Some(true),
                    ..Default::default()
                },
            ),
        )
    });
    assert!(harness.read_drive(|d| d.cruise_mph).is_some()); // the hazard alone still spares it
    assert!(harness.read_drive(|d| d.hazard_deadline).is_some());
    let damage_before = harness.read_drive(|d| d.truck().damage_pct);

    let mut lapsed = false;
    for _ in 0..2000 {
        frame(&mut harness, DT);
        if harness.read_drive(|d| d.hazard_deadline).is_none() {
            lapsed = true;
            break;
        }
    }
    assert!(lapsed, "the hazard deadline never lapsed");

    assert!(harness.read_drive(|d| d.truck().damage_pct) > damage_before); // the collision applied
    assert!(harness.read_drive(|d| d.cruise_mph).is_none());
    assert!(!harness.read_drive(|d| d.speed_control_armed));
}

#[test]
fn test_metric_cruise_minimum_refusal_uses_metric_units() {
    let mut harness = PlaytestHarness::new();
    harness.app.ctx.settings.imperial_units = false;
    harness.start_delivery(freight_fate::playtest::harness::StartDelivery::named(
        "Metric",
    ));
    harness.with_drive(|d, _| {
        d.departure_checked = true;
        bench_road(d, 200.0, 0.0, 1.0);
    });
    harness.clear_speech();
    press(&mut harness, Key::K, None);
    assert!(harness.read_drive(|d| d.cruise_mph).is_none());
    let said = last(&harness);
    assert!(said.contains("kilometers per hour"), "{said}");
    assert!(!said.contains("miles per hour"), "{said}");
}

// -- adaptive cruise ------------------------------------------------------------

#[test]
fn test_adaptive_cruise_follows_npc_traffic() {
    let mut harness = bench_drive("ACC Follow", 200.0, 0.0);
    harness.with_drive(|d, _| {
        let at = d.trip.position_mi + 0.08;
        d.trip.set_npc_vehicles(vec![NPCVehicle::new(
            "npc:acc",
            at,
            44.0,
            44.0,
            0,
            "braking_traffic",
        )
        .into()]);
    });
    press(&mut harness, Key::E, None);
    harness.with_drive(|d, _| {
        d.truck_mut().transmission.gear = 10;
        d.truck_mut().velocity_mps = 29.0;
        d.truck_mut().throttle = 0.9;
    });
    harness.clear_speech();
    press(&mut harness, Key::K, None);
    frame(&mut harness, DT);

    assert!(harness.read_drive(|d| d.cruise_mph).is_some());
    assert!(harness.read_drive(|d| d.acc_following));
    assert!(harness.read_drive(|d| d.truck().throttle) < 0.9);
    assert!(harness.read_drive(|d| d.truck().brake) > 0.0);
    assert!(said_any(&harness, "Traffic ahead, slowing."));
}

#[test]
fn test_adaptive_cruise_ignores_the_lane_being_left_mid_change() {
    // Tester report: with an automated lane change underway, cruise kept
    // following the slow lead in the lane being LEFT for the whole maneuver --
    // "I'm changing lanes, fucking drive." Mid-change, lead selection follows
    // the destination lane, so a lead still sitting in the origin lane no
    // longer caps the target.
    let mut harness = bench_drive("ACC Origin Lane", 200.0, 0.0);
    harness.with_drive(|d, _| {
        let at = d.trip.position_mi + 0.08;
        d.trip.set_npc_vehicles(vec![NPCVehicle::new(
            "npc:origin",
            at,
            44.0,
            44.0,
            0,
            "braking_traffic",
        )
        .into()]);
    });
    press(&mut harness, Key::E, None);
    harness.with_drive(|d, _| {
        d.truck_mut().transmission.gear = 10;
        d.truck_mut().velocity_mps = 29.0; // ~65 mph
    });
    press(&mut harness, Key::K, None);
    assert_eq!(harness.read_drive(|d| d.lane.lane), 0);
    harness.with_drive(|d, ctx| d.tap_lane_change(ctx, 1)); // start the pass
    assert_eq!(harness.read_drive(|d| d.lane_change_target), Some(1));

    frames(&mut harness, 10, DT);

    assert_eq!(harness.read_drive(|d| d.lane_change_target), Some(1)); // still underway
    assert!(!harness.read_drive(|d| d.acc_following));
    assert!(approx(harness.read_drive(|d| d.truck().brake), 0.0));
}

#[test]
fn test_adaptive_cruise_switches_lanes_on_a_held_steering_crossing() {
    // Held steering has no tap-change destination intent. Cruise must keep
    // following the slow car in the current lane until the tires actually
    // cross the line, then stop following it on that crossing frame.
    let mut harness = bench_drive("ACC Held Crossing", 200.0, 0.0);
    harness.with_drive(|d, _| {
        let at = d.trip.position_mi + 0.08;
        d.trip.set_npc_vehicles(vec![NPCVehicle::new(
            "npc:origin",
            at,
            44.0,
            44.0,
            0,
            "braking_traffic",
        )
        .into()]);
        d.truck_mut().transmission.gear = 10;
        d.truck_mut().velocity_mps = 29.0; // ~65 mph
    });
    harness.app.ctx.settings.lane_keeping = "off".to_string();
    press(&mut harness, Key::E, None);
    press(&mut harness, Key::K, None);
    assert_eq!(harness.read_drive(|d| d.lane.lane), 0);

    hold(&mut harness, &[Key::Left]);
    frame(&mut harness, DT);
    assert_eq!(harness.read_drive(|d| d.lane.lane), 0);
    assert!(harness.read_drive(|d| d.acc_following));

    for _ in 0..180 {
        frame(&mut harness, DT);
        if harness.read_drive(|d| d.lane.lane) == 1 {
            break;
        }
    }
    release_keys(&mut harness);

    assert_eq!(harness.read_drive(|d| d.lane.lane), 1);
    assert!(harness.read_drive(|d| d.lane_change_target).is_none());
    assert_eq!(
        harness.read_drive(|d| d.trip.traffic_manager.player_lane),
        1
    );
    assert!(!harness.read_drive(|d| d.acc_following));
    assert!(said_any(&harness, "In the left lane."));
}

#[test]
fn test_adaptive_cruise_follows_destination_traffic_on_a_held_steering_crossing() {
    // A held crossing must not discard occupied-lane safety. The destination
    // lead is ignored before the line, then followed on the exact frame the
    // truck enters that lane.
    let mut harness = bench_drive("ACC Held Destination", 200.0, 0.0);
    harness.with_drive(|d, _| {
        let at = d.trip.position_mi + 0.08;
        let mut destination =
            NPCVehicle::new("npc:destination", at, 38.0, 38.0, -1, "braking_traffic");
        destination.lane = 1;
        d.trip.set_npc_vehicles(vec![destination.into()]);
        d.truck_mut().transmission.gear = 10;
        d.truck_mut().velocity_mps = 29.0; // ~65 mph
    });
    harness.app.ctx.settings.lane_keeping = "off".to_string();
    press(&mut harness, Key::E, None);
    press(&mut harness, Key::K, None);
    hold(&mut harness, &[Key::Left]);
    frame(&mut harness, DT);
    assert!(!harness.read_drive(|d| d.acc_following));

    let mut following_on_cross = false;
    for _ in 0..180 {
        frame(&mut harness, DT);
        if harness.read_drive(|d| d.lane.lane) == 1 {
            following_on_cross = harness.read_drive(|d| d.acc_following);
            break;
        }
    }
    release_keys(&mut harness);

    assert_eq!(harness.read_drive(|d| d.lane.lane), 1);
    assert!(harness.read_drive(|d| d.lane_change_target).is_none());
    assert!(following_on_cross);
}

#[test]
fn test_adaptive_cruise_follows_the_lane_being_entered_mid_change() {
    // The other half of the fix: a slow lead already sitting in the
    // DESTINATION lane must still cap the target mid-change. Lead selection
    // follows the lane being entered -- it does not simply stop following.
    let mut harness = bench_drive("ACC Dest Lane", 200.0, 0.0);
    harness.with_drive(|d, _| {
        let at = d.trip.position_mi + 0.08;
        let mut npc = NPCVehicle::new("npc:dest", at, 44.0, 44.0, -1, "braking_traffic");
        npc.lane = 1;
        d.trip.set_npc_vehicles(vec![npc.into()]);
    });
    press(&mut harness, Key::E, None);
    harness.with_drive(|d, _| {
        d.truck_mut().transmission.gear = 10;
        d.truck_mut().velocity_mps = 29.0;
    });
    harness.clear_speech();
    press(&mut harness, Key::K, None);
    assert_eq!(harness.read_drive(|d| d.lane.lane), 0);
    // A tap waits while the lane is taken (issue #356), so this case starts
    // the change itself to read the lead mid-change.
    harness.with_drive(|d, ctx| d.tap_lane_change(ctx, 1));
    assert_eq!(harness.read_drive(|d| d.lane_change_target), None);
    assert!(said_any(&harness, "Left lane blocked by a"));
    harness.with_drive(|d, _| {
        d.lane_change_target = Some(1);
        d.lane_change_timer = 3.0;
    });

    frames(&mut harness, 10, DT);

    assert_eq!(harness.read_drive(|d| d.lane_change_target), Some(1)); // still underway
    assert!(harness.read_drive(|d| d.acc_following));
    assert!(said_any(&harness, "Traffic ahead, slowing."));
}

#[test]
fn test_adaptive_cruise_reverts_to_origin_lane_when_a_change_is_aborted() {
    // No latching: drifting back out of a change must hand lead selection back
    // to the origin lane the instant the lane layer stops reporting a change,
    // restoring the origin-lane lead's cap.
    let mut harness = bench_drive("ACC Abort", 200.0, 0.0);
    harness.with_drive(|d, _| {
        let at = d.trip.position_mi + 0.08;
        d.trip.set_npc_vehicles(vec![NPCVehicle::new(
            "npc:origin",
            at,
            44.0,
            44.0,
            0,
            "braking_traffic",
        )
        .into()]);
    });
    press(&mut harness, Key::E, None);
    harness.with_drive(|d, _| {
        d.truck_mut().transmission.gear = 10;
        d.truck_mut().velocity_mps = 29.0;
    });
    harness.clear_speech();
    press(&mut harness, Key::K, None);
    harness.with_drive(|d, ctx| d.tap_lane_change(ctx, 1));

    frames(&mut harness, 10, DT);
    assert!(!harness.read_drive(|d| d.acc_following)); // destination lane clear

    harness.with_drive(|d, _| d.lane_change_target = None); // drifted back -- aborted
    frames(&mut harness, 10, DT);

    assert_eq!(harness.read_drive(|d| d.lane.lane), 0); // still in the origin lane
    assert!(harness.read_drive(|d| d.acc_following)); // following the origin lead again
    assert!(said_any(&harness, "Traffic ahead, slowing."));
}

#[test]
fn test_adaptive_cruise_ignores_distant_slower_traffic() {
    // A slower vehicle far out in the traffic bubble must not drag cruise
    // down: matching a distant lead's speed parked the truck at the bubble
    // edge, where the lead popped in and out of range and re-announced itself
    // every lap.
    let mut harness = bench_drive("ACC Distant", 200.0, 0.0);
    harness.with_drive(|d, _| {
        let at = d.trip.position_mi + 2.3;
        d.trip.set_npc_vehicles(vec![NPCVehicle::new(
            "npc:far", at, 30.0, 30.0, 0, "slow_car",
        )
        .into()]);
    });
    press(&mut harness, Key::E, None);
    harness.with_drive(|d, _| {
        d.truck_mut().transmission.gear = 10;
        d.truck_mut().velocity_mps = 26.8; // ~60 mph
    });
    harness.clear_speech();
    press(&mut harness, Key::K, None);
    frame(&mut harness, DT);

    assert!(!harness.read_drive(|d| d.acc_following));
    assert!(approx(harness.read_drive(|d| d.truck().brake), 0.0));
    assert!(!said_any(&harness, "Traffic ahead, slowing."));
}

#[test]
fn test_adaptive_cruise_follow_cue_does_not_repeat_within_the_cooldown() {
    // If following flaps (the lead leaves the bubble and comes back), the
    // spoken cue must not fire again inside the quiet window.
    //
    // Flat ground: this test pins the follow-cue cooldown, not descent
    // physics. The helper route opens on a real 8.6 percent downhill, where
    // descent control engages the jake, the automatic starts a downshift, and
    // cruise rightly skips traffic decisions mid-shift -- on exactly the frame
    // this test asserts.
    let mut harness = bench_drive("ACC Cooldown", 200.0, 0.0);
    // The lead must also sit clearly INSIDE the follow gap: at the bubble edge
    // the approach-control formula is deliberately indifferent (a distant lead
    // must not drag the target down), and "following" there flips on
    // hundredths of a mile per hour of truck state.
    let slow_lead = |harness: &mut PlaytestHarness| {
        harness.with_drive(|d, _| {
            let at = d.trip.position_mi + 0.04;
            d.trip.set_npc_vehicles(vec![NPCVehicle::new(
                "npc:acc", at, 44.0, 44.0, 0, "slow_car",
            )
            .into()]);
        });
    };
    slow_lead(&mut harness);
    press(&mut harness, Key::E, None);
    harness.with_drive(|d, _| {
        d.truck_mut().transmission.gear = 10;
        d.truck_mut().velocity_mps = 29.0; // ~65 mph
    });
    harness.clear_speech();
    press(&mut harness, Key::K, None);

    let cue_count = |harness: &PlaytestHarness| {
        spoken(harness)
            .iter()
            .filter(|e| *e == "Traffic ahead, slowing.")
            .count()
    };

    frame(&mut harness, DT);
    assert!(harness.read_drive(|d| d.acc_following));
    // Raised once. The route-start line lands in the same frame and queues
    // behind it rather than purging it and handing it back, so the voice is
    // sent it once.
    let raised_once = cue_count(&harness);
    assert_eq!(raised_once, 1, "{:#?}", spoken(&harness));

    harness.with_drive(|d, _| d.trip.set_npc_vehicles(Vec::new())); // drifts out
    frame(&mut harness, DT);
    assert!(!harness.read_drive(|d| d.acc_following));

    slow_lead(&mut harness); // and back in
    frame(&mut harness, DT);
    assert!(harness.read_drive(|d| d.acc_following)); // follows again, but quietly
    assert_eq!(cue_count(&harness), raised_once);
}

#[test]
fn test_adaptive_cruise_caps_at_posted_limit() {
    // A posted limit well below the held set speed: predictive ACC must ease
    // off rather than carry the driver over the limit into a speeding strike.
    let mut harness = bench_drive("ACC Cap", 45.0, 0.0);
    press(&mut harness, Key::E, None);
    harness.with_drive(|d, _| {
        d.truck_mut().transmission.gear = 10;
        d.truck_mut().velocity_mps = 29.0; // ~65 mph
        d.truck_mut().throttle = 0.8;
    });
    harness.clear_speech();
    press(&mut harness, Key::K, None); // set cruise at ~65
    assert!(harness.read_drive(|d| d.cruise_mph).expect("cruise") > 60.0);

    frame(&mut harness, DT);

    assert!(harness.read_drive(|d| d.acc_limit_capped));
    assert!(harness.read_drive(|d| d.truck().throttle) < 0.8); // backed off the throttle
    assert!(harness.read_drive(|d| d.truck().brake) > 0.0); // braking down toward the limit
    assert!(said_any(&harness, "ruise easing to"));
}

#[test]
fn test_adaptive_cruise_slows_before_large_limit_drop() {
    let (mut harness, _drop_at) = a_limit_drop("ACC Drop", 0.4, 65.0, 40.0);
    press(&mut harness, Key::E, None);
    harness.with_drive(|d, _| {
        d.truck_mut().transmission.gear = 10;
        d.truck_mut().velocity_mps = 30.4; // ~68 mph
        d.truck_mut().throttle = 0.8;
    });
    harness.clear_speech();
    press(&mut harness, Key::K, None);

    frame(&mut harness, DT);

    assert!(harness.read_drive(|d| d.acc_limit_capped));
    assert!(harness.read_drive(|d| d.truck().throttle) < 0.8);
    assert!(harness.read_drive(|d| d.truck().brake) > 0.0);
    assert!(said_any(&harness, "ruise easing to"));
}

#[test]
fn test_adaptive_cruise_easing_preannounces_the_capped_target() {
    // Cruise's own "easing to X" line for a plain posted-limit drop already
    // named a number; wiring it into the trip's pre-announce set is the other
    // half of the fix that lets a plain arrival confirmation for that same
    // number stay quiet (owner's live playtest, 2026-08-12). What cruise
    // actually said is the ACC-offset target (posted + ACC_LIMIT_OFFSET_MPH
    // here, since this is a plain drop, not a restricted zone), not the raw
    // posted number -- pre-announcing that raw number instead would silence an
    // arrival confirmation cruise never actually spoke.
    let (mut harness, _drop_at) = a_limit_drop("ACC Preannounce", 0.4, 65.0, 40.0);
    press(&mut harness, Key::E, None);
    harness.with_drive(|d, _| {
        d.truck_mut().transmission.gear = 10;
        d.truck_mut().velocity_mps = 30.4; // ~68 mph
        d.truck_mut().throttle = 0.8;
    });
    harness.clear_speech();
    press(&mut harness, Key::K, None);

    frame(&mut harness, DT);

    assert!(said_any(&harness, "ruise easing to"));
    // 40 + ACC_LIMIT_OFFSET_MPH
    assert!(harness.read_drive(|d| d
        .trip
        .limit_drop_preannounced
        .contains(&(40.0 + ACC_LIMIT_OFFSET_MPH))));
}

/// A bench road that posts `before` until `ahead_mi` up the road and `after`
/// from there on -- Python's conditional `speed_limit_at` lambda.
fn a_limit_drop(name: &str, ahead_mi: f64, before: f64, after: f64) -> (PlaytestHarness, f64) {
    let mut harness = start_drive(name);
    // The drive starts at mile 0 of the bench leg, so the drop mile and the
    // baked sample's offset are the same number.
    harness.with_drive(move |d, _| {
        bench_road_with(d, &[(0.0, before), (ahead_mi, after)], 0.0, 1.0);
        d.truck_mut().set_air_ready(false);
    });
    harness.app.ctx.settings.time_scale = 1.0;
    let drop_at = harness.read_drive(|d| d.trip.position_mi) + ahead_mi;
    assert!(harness.read_drive(|d| d.trip.position_mi) < drop_at);
    (harness, drop_at)
}

#[test]
fn test_adaptive_cruise_limit_drop_is_never_read_as_speeding() {
    // Cruise braking the truck down to a new limit is not disregard.
    //
    // It used to be measured against a real-time strike clock; it is measured
    // against the over-limit distance an officer reads now, and the answer has
    // to be the same: the accrual resets, and nothing is charged.
    //
    // Python's `@pytest.mark.parametrize` over five rows.
    for (speed_mph, over_before_mi, dt) in [
        (45.0, 0.07, 0.1),
        (46.0, 0.07, 0.1),
        (55.0, 0.06, 0.5),
        (65.0, 0.05, 1.0),
        (70.0, 0.04, 1.5),
    ] {
        let mut harness = bench_drive("ACC No Strike", 35.0, 0.0);
        press(&mut harness, Key::E, None);
        harness.with_drive(move |d, _| {
            d.truck_mut().transmission.gear = 10;
            d.truck_mut().velocity_mps = speed_mph * MPS_PER_MPH;
            d.truck_mut().throttle = 0.0;
            d.cruise_mph = Some(65.0);
            d.over_limit_mi = over_before_mi;
        });
        harness.clear_speech();

        frame(&mut harness, dt);

        assert!(harness.read_drive(|d| d.acc_limit_capped), "{speed_mph}");
        assert!(harness.read_drive(|d| d.truck().brake) > 0.0, "{speed_mph}");
        assert!(approx(harness.read_drive(|d| d.over_limit_mi), 0.0));
        assert!(harness.read_drive(|d| d.pull_over.is_none()));
        assert!(!said_any(&harness, "Lights and siren"));
    }
}

#[test]
fn test_adaptive_cruise_ignores_far_small_limit_drop() {
    let (mut harness, _drop_at) = a_limit_drop("ACC Far Drop", 1.4, 65.0, 60.0);
    press(&mut harness, Key::E, None);
    harness.with_drive(|d, _| {
        d.truck_mut().transmission.gear = 10;
        d.truck_mut().velocity_mps = 30.4; // ~68 mph
    });
    press(&mut harness, Key::K, None);

    frame(&mut harness, DT);

    assert!(!harness.read_drive(|d| d.acc_limit_capped));
    assert!(approx(harness.read_drive(|d| d.truck().brake), 0.0));
}

#[test]
fn test_adaptive_cruise_allows_a_small_offset_over_the_limit() {
    // A few mph over the posted limit is a natural with-traffic pace and well
    // under the speeding-strike threshold, so cruise should not pull it back.
    let mut harness = bench_drive("ACC Offset", 60.0, 0.0);
    press(&mut harness, Key::E, None);
    harness.with_drive(|d, _| {
        d.truck_mut().transmission.gear = 10;
        d.truck_mut().velocity_mps = 28.2; // ~63 mph, 3 over a 60 limit
    });
    press(&mut harness, Key::K, None);

    frame(&mut harness, DT);

    assert!(!harness.read_drive(|d| d.acc_limit_capped));
    assert!(approx(harness.read_drive(|d| d.truck().brake), 0.0));
}

#[test]
fn test_adaptive_cruise_increases_gap_for_bad_weather() {
    let mut harness = bench_drive("ACC Weather Gap", 200.0, 0.0);
    press(&mut harness, Key::E, None);
    harness.with_drive(|d, _| {
        d.truck_mut().transmission.gear = 10;
        d.truck_mut().velocity_mps = 29.0;
        d.truck_mut().throttle = 0.5;
    });
    harness.clear_speech();
    press(&mut harness, Key::K, None);

    harness.with_drive(|d, _| {
        let at = d.trip.position_mi + 0.08;
        d.trip.set_npc_vehicles(vec![NPCVehicle::new(
            "npc:weather-gap",
            at,
            65.0,
            65.0,
            0,
            "steady_truck",
        )
        .into()]);
        d.weather_mut().current = WeatherKind::Clear;
    });
    let clear_gap = harness.with_drive(|d, ctx| d.acc_gap_seconds(ctx));
    frame(&mut harness, DT);
    assert!(!harness.read_drive(|d| d.acc_following));

    harness.with_drive(|d, _| d.weather_mut().current = WeatherKind::HeavyRain);
    let wet_gap = harness.with_drive(|d, ctx| d.acc_gap_seconds(ctx));
    frame(&mut harness, DT);

    assert!(wet_gap > clear_gap, "{wet_gap} {clear_gap}");
    assert!(harness.read_drive(|d| d.acc_following));
    assert!(said_any(&harness, "Wet roads, longer gap."));
}

#[test]
fn test_adaptive_cruise_stays_armed_before_restricted_zone() {
    let mut harness = bench_drive("ACC Zone Warn", 200.0, 0.0);
    press(&mut harness, Key::E, None);
    harness.with_drive(|d, _| {
        d.truck_mut().transmission.gear = 10;
        d.truck_mut().velocity_mps = 26.8;
    });
    press(&mut harness, Key::K, None);
    assert!(harness.read_drive(|d| d.cruise_mph).is_some());
    harness.clear_speech();

    harness.with_drive(|d, ctx| {
        d.handle_trip_event(
            ctx,
            &TripEvent {
                kind: TripEventKind::GpsCue,
                message: "In 2 miles, construction ahead. Speed limit 45.".into(),
                data: TripEventData {
                    zone: Some(Zone::new(10.0, 15.0, 45.0, "construction")),
                    ..Default::default()
                },
            },
        )
    });

    assert!(harness.read_drive(|d| d.cruise_mph).is_some());
    assert_eq!(
        last(&harness),
        "In 2 miles, construction ahead. Speed limit 45."
    );
}

#[test]
fn test_adaptive_cruise_switches_to_keeper_for_heavy_traffic() {
    let mut harness = bench_drive("ACC To Keeper", 200.0, 0.0);
    press(&mut harness, Key::E, None);
    harness.with_drive(|d, _| {
        d.truck_mut().transmission.gear = 10;
        d.truck_mut().velocity_mps = 26.8;
    });
    press(&mut harness, Key::K, None);
    assert!(harness.read_drive(|d| d.cruise_mph).is_some());
    harness.clear_speech();

    harness.with_drive(|d, ctx| {
        d.handle_trip_event(
            ctx,
            &TripEvent {
                kind: TripEventKind::ZoneEnter,
                message: "Entering heavy traffic zone. Speed limit 50 now.".into(),
                data: TripEventData {
                    zone: Some(Zone::new(10.0, 15.0, 50.0, "heavy traffic")),
                    ..Default::default()
                },
            },
        )
    });

    assert!(harness.read_drive(|d| d.cruise_mph).is_none());
    assert!(approx(
        harness.read_drive(|d| d.keeper_mph).expect("keeper"),
        50.0
    ));
    assert!(approx_abs(
        harness
            .read_drive(|d| d.speed_control_target_mph)
            .expect("a target"),
        60.0,
        1.0
    ));
    assert!(harness.read_drive(|d| d.speed_control_armed));
    let lines = spoken(&harness);
    assert_eq!(
        lines[lines.len() - 2],
        "Entering heavy traffic zone. Speed limit 50 now. \
         Speed keeper holding 50 miles per hour."
    );
    // Live achievement announces are name-only now (R9: the flavor moved to
    // the log). The announce reads exactly "New achievement! <name>." with no
    // trailing flavor, unlike the full-record log line.
    assert_eq!(
        lines[lines.len() - 1],
        "New achievement! Bumper-to-Bumper Blues."
    );
}

#[test]
fn test_cruise_pre_brakes_for_heavy_traffic_like_a_work_zone() {
    // Heavy traffic is a restricted zone, so cruise aims at its posted limit
    // exactly instead of carrying the with-traffic offset into the jam, and it
    // keeps aiming there once the warning window has retracted behind it.
    //
    // The end-to-end playtest case for this cannot run on this line: without
    // baked traffic volume no congestion zone lands on any route, so it is
    // marked xfail in the harness tests and the gap is a 2.0 item. This covers
    // the lookahead itself with a zone put on the route directly.
    let mut harness = bench_drive("Heavy Pre-brake", 70.0, 0.0);
    press(&mut harness, Key::E, None);
    harness.with_drive(|d, _| {
        d.truck_mut().transmission.gear = 10;
        d.truck_mut().velocity_mps = 31.3; // ~70 mph
    });
    press(&mut harness, Key::K, None);
    assert!(harness.read_drive(|d| d.cruise_mph).is_some());

    let start = harness.read_drive(|d| d.trip.position_mi) + 0.5;
    let zone = Zone::new(start, start + 3.0, 50.0, "heavy traffic");
    let staged = zone.clone();
    harness.with_drive(move |d, _| {
        d.trip.zones.push(staged.clone());
        d.trip.announced_zone_warnings.insert(zone_key(&staged));
    });

    assert_eq!(
        harness.with_drive(|d, ctx| d.restricted_zone_limit_ahead(ctx)),
        Some((50.0, "heavy traffic".to_string()))
    );
    // Exactly the zone's limit, with no with-traffic offset added.
    assert_eq!(
        harness.with_drive(|d, ctx| d.acc_posted_limit_ahead(ctx)),
        (50.0, Some("heavy traffic".to_string()))
    );
    // The latch carries the reason too, so a zone that slips back out of the
    // speed-scaled warning window is still braked for by name.
    assert_eq!(
        harness.read_drive(|d| d.construction_slowdown.clone()),
        Some((zone.end_mi, 50.0, "heavy traffic".to_string()))
    );
    // Python patched `trip._zone_warning_lookahead_mi` to 0. That lookahead is
    // speed-scaled with a floor, so there is no seam for zero here; retracting
    // the window by stopping the truck and moving the zone out past the floor
    // asks the same question -- is the LATCH still braking for it by name once
    // the window no longer reaches?
    harness.with_drive(|d, _| {
        d.truck_mut().velocity_mps = 0.0;
        let far = d.trip.position_mi + 50.0;
        for zone in &mut d.trip.zones {
            if zone.reason == "heavy traffic" {
                zone.start_mi = far;
                zone.end_mi = far + 3.0;
            }
        }
    });
    assert_eq!(
        harness.with_drive(|d, ctx| d.restricted_zone_limit_ahead(ctx)),
        Some((50.0, "heavy traffic".to_string()))
    );
}

#[test]
fn test_cruise_eases_to_the_taper_speed_first_then_the_work_zone_at_the_barrels() {
    // The warning promises two numbers: the taper's, then the work zone's.
    // Cruise used to aim at the work zone's from the moment the warning
    // landed, miles out, so a driver told "55 at the taper, then 45" was
    // already at 45 long before the taper (owner, 2026-09-14).
    let mut harness = bench_drive("Taper Stage", 70.0, 0.0);
    press(&mut harness, Key::E, None);
    harness.with_drive(|d, _| {
        d.truck_mut().transmission.gear = 10;
        d.truck_mut().velocity_mps = 31.3; // ~70 mph
    });
    press(&mut harness, Key::K, None);

    let barrels = harness.read_drive(|d| d.trip.position_mi) + 1.5;
    let taper = Zone::new(barrels - 1.0, barrels, 55.0, "construction merge");
    let work = Zone::new(barrels, barrels + 3.0, 45.0, "construction");
    let announced = work.clone();
    harness.with_drive(move |d, _| {
        d.trip.zones.push(taper.clone());
        d.trip.zones.push(work.clone());
        d.trip.announced_zone_warnings.insert(zone_key(&announced));
    });

    // A mile and a half out at 70: the taper's number, as the warning said.
    assert_eq!(
        harness.with_drive(|d, ctx| d.restricted_zone_limit_ahead(ctx)),
        Some((55.0, "construction".to_string()))
    );
    assert_eq!(
        harness.with_drive(|d, ctx| d.acc_posted_limit_ahead(ctx)),
        (55.0, Some("construction".to_string()))
    );
    // Down to 55 and inside braking distance of the barrels: the zone's.
    harness.with_drive(move |d, _| {
        d.truck_mut().velocity_mps = 24.6; // ~55 mph
        d.trip.position_mi = barrels - 0.2;
    });
    assert_eq!(
        harness.with_drive(|d, ctx| d.restricted_zone_limit_ahead(ctx)),
        Some((45.0, "construction".to_string()))
    );
    // Once the zone stage has begun it holds: slowing shrinks the braking
    // window, and the target must not climb back to 55 over the last yards.
    harness.with_drive(move |d, _| {
        d.truck_mut().velocity_mps = 20.6; // ~46 mph
        d.trip.position_mi = barrels - 0.6;
    });
    assert_eq!(
        harness.with_drive(|d, ctx| d.restricted_zone_limit_ahead(ctx)),
        Some((45.0, "construction".to_string()))
    );
}

#[test]
fn test_speed_control_restores_cruise_target_after_zone() {
    let mut harness = bench_drive("Restore Target", 65.0, 0.0);
    release_keys(&mut harness);
    press(&mut harness, Key::E, None);
    harness.with_drive(|d, _| {
        d.truck_mut().transmission.gear = 10;
        d.truck_mut().velocity_mps = 26.8; // ~60 mph
    });
    harness.clear_speech();
    press(&mut harness, Key::K, None);
    let original_target = harness.read_drive(|d| d.cruise_mph).expect("cruise");

    post_zone(&mut harness, 25.0, "construction");
    frame(&mut harness, DT);
    assert!(harness.read_drive(|d| d.cruise_mph).is_none());
    assert!(approx(
        harness.read_drive(|d| d.keeper_mph).expect("keeper"),
        25.0
    ));

    harness.with_drive(|d, _| d.trip.zones.clear());
    frame(&mut harness, DT);
    assert!(harness.read_drive(|d| d.keeper_mph).is_none());
    assert!(approx(
        harness.read_drive(|d| d.cruise_mph).expect("cruise"),
        original_target
    ));
    // Once for the zone. The work-zone warning lands in the same frame and
    // queues behind the keeper line instead of purging it and handing it back.
    assert_eq!(
        spoken(&harness)
            .iter()
            .filter(|e| e.contains("Speed keeper holding"))
            .count(),
        1,
        "{:#?}",
        spoken(&harness)
    );
    assert_eq!(
        spoken(&harness)
            .iter()
            .filter(|e| e.contains("Cruise resuming"))
            .count(),
        1
    );
}

#[test]
fn test_cruise_target_can_be_adjusted_while_keeper_is_active() {
    let mut harness = bench_drive("Adjust In Keeper", 200.0, 0.0);
    post_zone(&mut harness, 15.0, "facility access road");
    press(&mut harness, Key::E, None);
    harness.with_drive(|d, _| {
        d.truck_mut().transmission.gear = 3;
        d.truck_mut().velocity_mps = 4.5;
    });
    press(&mut harness, Key::K, None);
    harness.clear_speech();

    press(&mut harness, Key::Equals, None);

    assert!(approx_abs(
        harness.read_drive(|d| d.keeper_mph).expect("keeper"),
        10.0,
        0.5
    ));
    assert!(approx(
        harness
            .read_drive(|d| d.speed_control_target_mph)
            .expect("a target"),
        25.0
    ));
    assert_eq!(last(&harness), "Open-road cruise target 25 miles per hour.");
}

/// The posted-limit scan sees a drop further out under time compression:
/// the working setpoint walks down in REAL seconds, so at twenty times the
/// truck covers twenty times the road while cruise eases.
#[test]
fn test_cruise_sees_a_limit_drop_further_out_under_time_compression() {
    // 70 posted, dropping to 30 three miles out.
    let mut harness = bench_drive_with("Compressed Drop", &[(0.0, 70.0), (3.0, 30.0)], 0.0);
    harness.with_drive(|d, _| {
        d.truck_mut().transmission.gear = 10;
        d.truck_mut().velocity_mps = 31.3; // ~70 mph
    });
    // Real time: three miles is well past the physics lookahead.
    assert_eq!(
        harness.with_drive(|d, ctx| d.acc_posted_limit_ahead(ctx)),
        (70.0, None)
    );
    // Twenty times: the same drop is inside the window.
    harness.app.ctx.settings.time_scale = 20.0;
    harness.with_drive(|d, _| d.trip.time_scale = 20.0);
    assert_eq!(
        harness.with_drive(|d, ctx| d.acc_posted_limit_ahead(ctx)),
        (30.0, None)
    );
}

/// Once cruise has eased for a drop it keeps aiming at it. The scan's window
/// is a braking distance sized from the truck's speed, so as cruise slowed
/// the drop fell back out of sight, the cap lifted, the truck wound back up,
/// and the drop was found again: four brake-and-surge cycles and four
/// "Posted limit lower" lines on one approach (agent drive, RI-146,
/// 2026-09-02).
#[test]
fn test_cruise_keeps_aiming_at_a_limit_drop_once_it_has_eased_for_it() {
    let mut harness = bench_drive_with("Held Drop", &[(0.0, 70.0), (3.0, 30.0)], 0.0);
    harness.app.ctx.settings.time_scale = 20.0;
    harness.with_drive(|d, _| {
        d.trip.time_scale = 20.0;
        d.truck_mut().transmission.gear = 10;
        d.truck_mut().velocity_mps = 31.3; // ~70 mph
    });
    assert_eq!(
        harness.with_drive(|d, ctx| d.acc_posted_limit_ahead(ctx)),
        (30.0, None)
    );
    // Eased to 35: the braking window no longer reaches three miles out.
    harness.with_drive(|d, _| d.truck_mut().velocity_mps = 15.6);
    assert_eq!(
        harness.with_drive(|d, ctx| d.acc_posted_limit_ahead(ctx)),
        (30.0, None),
        "the drop is held, not rediscovered"
    );
    // Past the drop the hold is spent; the road under the wheels answers.
    harness.with_drive(|d, _| d.trip.position_mi = 3.2);
    assert_eq!(
        harness.with_drive(|d, ctx| d.acc_posted_limit_ahead(ctx)),
        (30.0, None)
    );
    assert!(harness.read_drive(|d| d.acc_limit_hold.is_none()));
}

#[test]
fn test_a_hazard_that_leaves_the_truck_below_cruise_speed_still_resumes() {
    // Owner, Willmar to Owatonna, 2026-09-16: "Brake! Slow car right ahead"
    // matched a 17 mph car, "Well done", and then nothing. Adaptive cruise
    // refuses below 20, so the session sat paused with the driver holding 17
    // and nobody saying why. The keeper is the automation for the low-speed
    // stretch (the acceleration-lane bridge, Brandon, 2026-08-21) and it
    // takes this one too: builds behind whatever held the truck down, then
    // hands to cruise at road speed.
    let mut harness = bench_drive("Slow Car Resume", 55.0, 0.0);
    press(&mut harness, Key::E, None);
    harness.with_drive(|d, _| {
        d.truck_mut().transmission.gear = 10;
        d.truck_mut().velocity_mps = 26.8;
    });
    press(&mut harness, Key::K, None);
    assert!(harness.read_drive(|d| d.cruise_mph).is_some());
    harness.with_drive(|d, ctx| {
        d.handle_trip_event(
            ctx,
            &hazard_event(
                "Brake! Slow car right ahead.",
                TripEventData {
                    deadline_s: Some(4.0),
                    ..Default::default()
                },
            ),
        )
    });
    assert!(harness.read_drive(|d| d.speed_control_armed));
    assert!(harness.read_drive(|d| d.cruise_mph).is_none());

    // Braked down to the car's 17 and the hazard clears there.
    harness.with_drive(|d, _| d.truck_mut().velocity_mps = 17.0 / MPH_PER_MPS);
    harness.clear_speech();
    harness.with_drive(|d, ctx| d.clear_hazard(ctx));
    harness.with_drive(|d, ctx| d.resume_speed_control_if_ready(ctx, false));

    assert!(harness.read_drive(|d| d.speed_control_armed));
    assert!(
        harness.read_drive(|d| d.keeper_mph).is_some(),
        "the keeper bridges the crawl back to road speed: {:?}",
        spoken(&harness)
    );
    assert!(
        said_any(&harness, "Speed control resuming."),
        "{:?}",
        spoken(&harness)
    );

    // Back at road speed, the keeper hands to adaptive cruise on its own.
    harness.with_drive(|d, _| d.truck_mut().velocity_mps = 24.0 / MPH_PER_MPS);
    harness.with_drive(|d, ctx| d.update_keeper(ctx, 0.1, false, false, false));
    assert!(harness.read_drive(|d| d.cruise_mph).is_some());
    assert!(harness.read_drive(|d| d.keeper_mph).is_none());
}
