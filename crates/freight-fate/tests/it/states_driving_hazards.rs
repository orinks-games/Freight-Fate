//! Hazards from the moment they arm to the line that says they are behind
//! you: `states/driving_events/trip_events.rs` (the arming and folding
//! branches), `states/driving_updates/hazards.rs` (resolution and the
//! assist's own application) and the earcon map in `states/driving_core.rs`.
//!
//! Ported from the hazard block at the top of
//! `tests/test_driving_features.py` plus
//! `test_terse_hazard_drops_brake_now_instruction` and
//! `test_a_folded_hazard_does_not_follow_the_truck_into_its_new_lane`.
//!
//! Python's `monkeypatch.setattr(app.ctx, "say_event", ...)` replaced the
//! whole delivery layer, so its assertions read raw calls. Here the real
//! delivery layer runs and the lines are read back off the capture, which
//! means the pacer is in the loop; where a case fires two hazards in one
//! instant the clock is moved with the truck so the pacer is not deciding
//! what the hazard machinery said.

use ff_core::sim::traffic_manager::TrafficVehicle;
use ff_core::sim::trip_models::{NavigationCue, TripEvent, TripEventData, TripEventKind, Zone};
use ff_core::sim::weather::WeatherKind;
use ff_core::speech_text::{hazard_call, SpokenMessage};

use freight_fate::playtest::harness::{PlaytestHarness, StartDelivery};
use freight_fate::states::driving_core::{
    route_event_sound, DRIVE_PHASE_PICKUP, HAZARD_CREEP_MPH, HAZARD_SAFE_MPH,
};

const MPH_PER_MPS: f64 = 2.2369362920544;
const DT: f64 = 1.0 / 60.0;

// -- rigging -------------------------------------------------------------------------

/// `start_drive(app)` + `quiet_trip(driving)`.
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

fn a_hazard(text: &str, deadline_s: f64, dodgeable: bool, name: &str) -> TripEvent {
    TripEvent {
        kind: TripEventKind::Hazard,
        message: SpokenMessage::new(text),
        data: TripEventData {
            deadline_s: Some(deadline_s),
            dodgeable: Some(dodgeable),
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

fn event_calls(harness: &PlaytestHarness) -> Vec<(String, bool)> {
    harness.app.event_calls()
}

fn event_lines(harness: &PlaytestHarness) -> Vec<String> {
    harness.app.event_lines()
}

// -- the earcon map -------------------------------------------------------------------

#[test]
fn test_trip_event_sounds_use_contextual_cues() {
    let plain = |kind: TripEventKind, text: &str| TripEvent {
        kind,
        message: SpokenMessage::new(text),
        data: TripEventData::default(),
    };
    assert_eq!(
        route_event_sound(&plain(TripEventKind::Hazard, "Brake now!")),
        Some("events/hazard_warning")
    );
    assert_eq!(
        route_event_sound(&plain(TripEventKind::TollCharged, "Toll")),
        Some("events/toll_charged")
    );
    assert_eq!(
        route_event_sound(&plain(TripEventKind::StateCrossing, "Crossing")),
        Some("events/state_crossing")
    );

    let zone_event = TripEvent {
        kind: TripEventKind::ZoneEnter,
        message: SpokenMessage::new("construction ahead"),
        data: TripEventData {
            zone: Some(Zone::new(1.0, 2.0, 45.0, "construction")),
            ..Default::default()
        },
    };
    assert_eq!(
        route_event_sound(&zone_event),
        Some("events/construction_zone")
    );
    // Only a jam is traffic slowing. The yard's access road, a facility
    // gate or an approach zone is a speed limit, and gets the plain notice
    // instead: every drive used to open on the traffic cue with the engine
    // off (agent drive, 2026-09-01).
    let zone_of = |reason: &str| TripEvent {
        kind: TripEventKind::ZoneEnter,
        message: SpokenMessage::new("zone"),
        data: TripEventData {
            zone: Some(Zone::new(0.0, 1.0, 15.0, reason)),
            ..Default::default()
        },
    };
    assert_eq!(
        route_event_sound(&zone_of("heavy traffic")),
        Some("events/traffic_slowing")
    );
    for reason in [
        "facility access road",
        "facility gate",
        "destination approach",
    ] {
        assert_eq!(route_event_sound(&zone_of(reason)), None, "{reason}");
    }

    let cb_event = TripEvent {
        kind: TripEventKind::GpsCue,
        message: SpokenMessage::new("CB patrol ahead"),
        data: TripEventData {
            cb_patrol: Some(ff_core::sim::enforcement_posts::EnforcementPost::new(
                4.0,
                ff_core::sim::enforcement_posts::KIND_MEDIAN,
            )),
            ..Default::default()
        },
    };
    assert_eq!(
        route_event_sound(&cb_event),
        Some("events/cb_radio_chatter")
    );

    let cue_event = |cue: NavigationCue| TripEvent {
        kind: TripEventKind::GpsCue,
        message: SpokenMessage::new(&cue.near_text),
        data: TripEventData {
            cue: Some(cue),
            ..Default::default()
        },
    };
    let toll_ahead = NavigationCue::new(
        "toll:0:12.0:Ohio Turnpike",
        "toll",
        1.0,
        "toll road ahead: Ohio Turnpike",
        "E-ZPass toll point ahead, Ohio Turnpike westbound settlement.",
    );
    assert_eq!(route_event_sound(&cue_event(toll_ahead)), None);
    let left_turn = NavigationCue::new(
        "local:left",
        "local_turn",
        1.0,
        "turn left onto Depot Street",
        "Turn left onto Depot Street.",
    )
    .with_direction("left");
    let right_turn = NavigationCue::new(
        "local:right",
        "local_turn",
        1.0,
        "turn right onto Yard Road",
        "Turn right onto Yard Road.",
    )
    .with_direction("right");
    let ahead_turn = NavigationCue::new(
        "local:ahead",
        "local_turn",
        1.0,
        "start on Market Street",
        "Start on Market Street.",
    )
    .with_direction("ahead");
    let ambiguous_turn = NavigationCue::new(
        "local:ambiguous",
        "local_turn",
        1.0,
        "turn onto Market Street",
        "Turn onto Market Street.",
    );
    let highway_maneuver = NavigationCue::new(
        "maneuver:right",
        "maneuver",
        1.0,
        "keep right for I-80",
        "Keep right for I-80.",
    )
    .with_direction("right");
    assert_eq!(
        route_event_sound(&cue_event(left_turn)),
        Some("events/turn_left")
    );
    assert_eq!(
        route_event_sound(&cue_event(right_turn)),
        Some("events/turn_right")
    );
    assert_eq!(
        route_event_sound(&cue_event(ahead_turn)),
        Some("events/turn_ahead")
    );
    assert_eq!(route_event_sound(&cue_event(ambiguous_turn)), None);
    assert_eq!(route_event_sound(&cue_event(highway_maneuver)), None);

    let traffic_cue = NavigationCue::new("traffic:test", "traffic", 1.0, "traffic ahead", "");
    for (vehicle_class, sound) in [
        ("car", "events/traffic_slowing"),
        ("box truck", "events/traffic_slowing"),
        ("semi", "events/traffic_slowing"),
        ("state trooper", "traffic/trooper_pass"),
    ] {
        let event = TripEvent {
            kind: TripEventKind::GpsCue,
            message: SpokenMessage::new(&traffic_cue.near_text),
            data: TripEventData {
                cue: Some(traffic_cue.clone()),
                npc_vehicle: Some(TrafficVehicle::new(
                    "npc",
                    12.0,
                    55.0,
                    55.0,
                    1,
                    "cruising",
                    vehicle_class,
                )),
                ..Default::default()
            },
        };
        assert_eq!(route_event_sound(&event), Some(sound));
    }
    assert_eq!(
        route_event_sound(&cue_event(traffic_cue)),
        Some("events/traffic_slowing")
    );
}

#[test]
fn test_entering_the_yard_access_road_is_a_notice_not_slowing_traffic() {
    // A career's first drive is the pickup leg, and its first event is the
    // access-road zone under the parked truck (on a delivery leg the same
    // zone waits for the destination exit). It is handled like any zone
    // entry: a soft notice, never the traffic-slowing earcon the sound
    // catalog promises means a jam.
    let mut harness = a_drive("Yard Zone");
    harness.with_drive(|drive, _| drive.phase = DRIVE_PHASE_PICKUP);
    let log = harness.app.record_audio();
    arm(
        &mut harness,
        TripEvent {
            kind: TripEventKind::ZoneEnter,
            message: SpokenMessage::new("Facility access road. Speed limit 15."),
            data: TripEventData {
                zone: Some(Zone::new(0.0, 0.4, 15.0, "facility access road")),
                ..Default::default()
            },
        },
    );
    let played: Vec<String> = log
        .borrow()
        .played
        .iter()
        .map(|(key, _, _)| key.clone())
        .collect();
    assert!(
        !played.iter().any(|key| key == "events/traffic_slowing"),
        "{played:?}"
    );
    assert!(played.iter().any(|key| key == "ui/notify"), "{played:?}");
}

// -- resolution -----------------------------------------------------------------------

#[test]
fn test_passing_hazard_plays_clear_sound() {
    let mut harness = a_drive("Hazard Clear");
    let log = harness.app.record_audio();
    harness.with_drive(|drive, _| {
        drive.hazard_deadline = Some(3.0);
        drive.truck_mut().velocity_mps = (HAZARD_SAFE_MPH - 1.0) / MPH_PER_MPS;
    });
    harness.clear_speech();

    harness.with_drive(|drive, ctx| drive.update_hazard(ctx, DT));

    assert!(harness.read_drive(|d| d.hazard_deadline).is_none());
    assert!(
        log.borrow()
            .played
            .iter()
            .any(|(key, volume, _)| key == "events/hazard_clear" && *volume == 0.75),
        "{:#?}",
        log.borrow().played
    );
    assert_eq!(event_calls(&harness), vec![("Clear.".to_string(), false)]);
}

#[test]
fn test_single_hazard_is_named_in_its_resolution_line() {
    // Fix B, the simple case: naming the hazard is the same plumbing as the
    // stacked case, so a lone hazard gets it too instead of a generic "it".
    let mut harness = a_drive("Named Hazard");
    at_mph(&mut harness, HAZARD_SAFE_MPH + 10.0);
    arm(
        &mut harness,
        a_hazard(
            "Brake now! A deer crossing the road.",
            4.0,
            false,
            "the deer",
        ),
    );
    harness.clear_speech();
    // Ten seconds, long enough for the warning(s) ahead of this line to have
    // finished speaking. Python read the interrupt flag off a stub of
    // `say_event`, so it saw what the hazard machinery ASKED for; the capture
    // here sees what was DELIVERED, and a queued line landing on a backed-up
    // channel is promoted to an interrupt by the anti-backlog flush. Without
    // moving the clock every line in the case lands in one instant, and the
    // assertion would be measuring that artefact instead of the resolution's
    // own queued delivery.
    harness.advance_clock(10.0);

    at_mph(&mut harness, HAZARD_SAFE_MPH - 1.0);
    harness.with_drive(|drive, ctx| drive.update_hazard(ctx, DT));

    assert!(harness.read_drive(|d| d.hazard_deadline).is_none());
    assert_eq!(
        event_calls(&harness),
        vec![("Clear of the deer.".to_string(), false)]
    );
}

#[test]
fn test_two_stacked_hazards_are_each_named_once() {
    // Shane's deer: a second hazard arming while one is still pending used to
    // silently overwrite it, so the deer's outcome never got spoken. Both
    // must now clear together, named, in one resolution line.
    let mut harness = a_drive("Stacked");
    at_mph(&mut harness, HAZARD_SAFE_MPH + 10.0);
    arm(
        &mut harness,
        a_hazard(
            "Brake now! A deer crossing the road.",
            4.0,
            false,
            "the deer",
        ),
    );
    // Still moving too fast to have cleared the deer -- the second hazard
    // must fold in beside it, not clobber it. Each call gets the seconds it
    // takes to speak; two urgent lines stacked in one instant leave a
    // backlog the delivery layer would flush, which is a pacer artefact of a
    // zero-time test rather than anything the hazard machinery decided.
    harness.advance_clock(3.0);
    at_mph(&mut harness, HAZARD_SAFE_MPH + 10.0);
    arm(
        &mut harness,
        a_hazard(
            "Brake! Slowed traffic ahead.",
            2.5,
            false,
            "the slowed traffic",
        ),
    );
    assert_eq!(
        harness.read_drive(|d| d.hazard_names.clone()),
        vec!["the deer".to_string(), "the slowed traffic".to_string()]
    );

    harness.clear_speech();
    // Ten seconds, long enough for the warning(s) ahead of this line to have
    // finished speaking. Python read the interrupt flag off a stub of
    // `say_event`, so it saw what the hazard machinery ASKED for; the capture
    // here sees what was DELIVERED, and a queued line landing on a backed-up
    // channel is promoted to an interrupt by the anti-backlog flush. Without
    // moving the clock every line in the case lands in one instant, and the
    // assertion would be measuring that artefact instead of the resolution's
    // own queued delivery.
    harness.advance_clock(10.0);
    at_mph(&mut harness, HAZARD_SAFE_MPH - 1.0);
    harness.with_drive(|drive, ctx| drive.update_hazard(ctx, DT));

    assert!(harness.read_drive(|d| d.hazard_deadline).is_none());
    assert_eq!(
        event_calls(&harness),
        vec![(
            "Clear of the deer and the slowed traffic.".to_string(),
            false
        )]
    );
}

#[test]
fn test_stacked_hazard_wording_follows_the_strictest_dodgeability() {
    // A non-dodgeable hazard folded in with a dodgeable one means "ease
    // around" is the wrong promise for the group: the non-dodgeable one wins
    // the wording. An all-dodgeable stack keeps the ease-around family and
    // still names what was cleared.
    let mut harness = a_drive("Dodgeable");
    at_mph(&mut harness, HAZARD_SAFE_MPH + 10.0);
    arm(
        &mut harness,
        a_hazard(
            "Change lanes or brake! Debris on the road.",
            4.0,
            true,
            "the debris",
        ),
    );
    harness.advance_clock(3.0);
    at_mph(&mut harness, HAZARD_SAFE_MPH + 10.0);
    arm(
        &mut harness,
        a_hazard(
            "Change lanes or brake! Retread debris from a blown tire.",
            4.0,
            true,
            "the tire debris",
        ),
    );
    assert!(harness.read_drive(|d| d.hazard_dodgeable));

    harness.clear_speech();
    // Ten seconds, long enough for the warning(s) ahead of this line to have
    // finished speaking. Python read the interrupt flag off a stub of
    // `say_event`, so it saw what the hazard machinery ASKED for; the capture
    // here sees what was DELIVERED, and a queued line landing on a backed-up
    // channel is promoted to an interrupt by the anti-backlog flush. Without
    // moving the clock every line in the case lands in one instant, and the
    // assertion would be measuring that artefact instead of the resolution's
    // own queued delivery.
    harness.advance_clock(10.0);
    at_mph(&mut harness, HAZARD_CREEP_MPH - 1.0);
    harness.with_drive(|drive, ctx| drive.update_hazard(ctx, DT));

    assert_eq!(
        event_calls(&harness),
        vec![(
            "Clear of the debris and the tire debris.".to_string(),
            false
        )]
    );
}

#[test]
fn test_a_hazard_already_outrun_gets_its_own_line_before_the_next_arms() {
    // The first hazard's condition is already met when the second arms: it
    // earns its own clean resolution line instead of being folded in or
    // silently dropped.
    let mut harness = a_drive("Outrun");
    at_mph(&mut harness, HAZARD_SAFE_MPH + 10.0);
    arm(
        &mut harness,
        a_hazard(
            "Brake now! A deer crossing the road.",
            4.0,
            false,
            "the deer",
        ),
    );
    // Slowed below the deer's own safe speed before the next hazard hits.
    at_mph(&mut harness, HAZARD_SAFE_MPH - 1.0);
    harness.clear_speech();
    arm(
        &mut harness,
        a_hazard(
            "Brake! Slowed traffic ahead.",
            2.5,
            false,
            "the slowed traffic",
        ),
    );

    assert!(
        event_lines(&harness).contains(&"Clear of the deer.".to_string()),
        "{:#?}",
        event_lines(&harness)
    );
    assert_eq!(
        harness.read_drive(|d| d.hazard_names.clone()),
        vec!["the slowed traffic".to_string()]
    );
    assert!(harness.read_drive(|d| d.hazard_deadline).is_some());
}

#[test]
fn test_terse_hazard_resolution_stays_silent_words() {
    // R4/R14: the hazard-clear earcon IS the terse confirmation. Stacking
    // hazards must not grow the resolution into terse words -- the message
    // handed to the delivery layer must still render empty in terse, however
    // many hazards it names in normal speech.
    //
    // Python read the raw `SpokenMessage` off its stubbed `say_event`. There
    // is no stub here, so the pair is checked the way a player would hear it:
    // the same drive resolved once on standard speech and once on quiet.
    let normal = {
        let mut harness = a_drive("Terse Pair Normal");
        stack_two(&mut harness);
        harness.clear_speech();
        at_mph(&mut harness, HAZARD_SAFE_MPH - 1.0);
        harness.with_drive(|drive, ctx| drive.update_hazard(ctx, DT));
        event_lines(&harness)
    };
    assert_eq!(
        normal,
        vec!["Clear of the deer and the slowed traffic.".to_string()]
    );

    let mut harness = a_drive("Terse Pair Quiet");
    harness.app.ctx.settings.driving_speech = "quiet".to_string();
    stack_two(&mut harness);
    harness.clear_speech();
    at_mph(&mut harness, HAZARD_SAFE_MPH - 1.0);
    harness.with_drive(|drive, ctx| drive.update_hazard(ctx, DT));

    assert!(
        event_lines(&harness).is_empty(),
        "the terse rendering is empty, so nothing is spoken: {:#?}",
        event_lines(&harness)
    );
    assert!(harness.read_drive(|d| d.hazard_deadline).is_none());
}

/// The deer and the slowed traffic, armed together and still pending.
fn stack_two(harness: &mut PlaytestHarness) {
    at_mph(harness, HAZARD_SAFE_MPH + 10.0);
    arm(
        harness,
        a_hazard(
            "Brake now! A deer crossing the road.",
            4.0,
            false,
            "the deer",
        ),
    );
    at_mph(harness, HAZARD_SAFE_MPH + 10.0);
    arm(
        harness,
        a_hazard(
            "Brake! Slowed traffic ahead.",
            2.5,
            false,
            "the slowed traffic",
        ),
    );
}

#[test]
fn test_terse_hazard_drops_brake_now_instruction() {
    let mut harness = a_drive("Terse Call");
    harness.app.ctx.settings.driving_speech = "quiet".to_string();
    harness.clear_speech();

    let call = hazard_call("Brake now!", "Debris on the shoulder.");
    harness.with_drive(move |drive, ctx| {
        drive.handle_trip_event(
            ctx,
            &TripEvent {
                kind: TripEventKind::Hazard,
                message: call,
                data: TripEventData {
                    deadline_s: Some(4.0),
                    ..Default::default()
                },
            },
        )
    });

    assert_eq!(
        event_calls(&harness).last().cloned(),
        Some(("Debris on the shoulder.".to_string(), true))
    );
}

// -- the assist's own application ------------------------------------------------------

#[test]
fn test_assist_releases_its_own_emergency_application() {
    // The AEB escalation sets truck.emergency_brake; the assist must release
    // it when the hazard resolves. In real play the input pass stomps the
    // flag from the B key every frame, but nothing guarantees an input frame
    // between engage and clear -- a harness driving the sim directly (the
    // smoke drive, the playtest tool) was left standing on everything
    // forever.
    let mut harness = a_drive("Assist Release");

    // Clear path: AEB engaged and escalated, then the truck gets slow enough.
    harness.with_drive(|drive, ctx| {
        drive.hazard_deadline = Some(3.0);
        drive.automatic_braking_announced = true;
        drive.aeb_brake = 1.0;
        drive.aeb_emergency = true;
        drive.truck_mut().emergency_brake = true;
        drive.truck_mut().velocity_mps = 0.0;
        drive.update_hazard(ctx, DT);
    });
    assert!(harness.read_drive(|d| d.hazard_deadline).is_none());
    assert!(!harness.read_drive(|d| d.truck().emergency_brake));
    assert_eq!(harness.read_drive(|d| d.aeb_brake), 0.0);

    // Collision path: the deadline runs out with the application still on.
    harness.with_drive(|drive, ctx| {
        drive.hazard_deadline = Some(1.0 / 120.0);
        drive.automatic_braking_announced = true;
        drive.aeb_brake = 1.0;
        drive.aeb_emergency = true;
        drive.truck_mut().emergency_brake = true;
        drive.truck_mut().velocity_mps = 30.0;
        drive.update_hazard(ctx, DT);
    });
    assert!(harness.read_drive(|d| d.hazard_deadline).is_none());
    assert!(!harness.read_drive(|d| d.truck().emergency_brake));
    assert_eq!(harness.read_drive(|d| d.aeb_brake), 0.0);

    // A driver-held application is not the assist's to release.
    harness.with_drive(|drive, ctx| {
        drive.hazard_deadline = Some(3.0);
        drive.automatic_braking_announced = false;
        drive.aeb_brake = 0.0;
        drive.aeb_emergency = false;
        drive.truck_mut().emergency_brake = true;
        drive.truck_mut().velocity_mps = 0.0;
        drive.update_hazard(ctx, DT);
    });
    assert!(harness.read_drive(|d| d.truck().emergency_brake));
}

// -- which lane the hazard is in --------------------------------------------------------

#[test]
fn test_a_folded_hazard_does_not_follow_the_truck_into_its_new_lane() {
    // The lane belongs to the hazard, not to the truck.
    //
    // Shane, 2026-08-21: "the repeating happened everytime I was changing
    // lanes until the two-three repeats are done." Dodging is answered by
    // being in a different lane from the hazard -- but the hazard's lane was
    // re-stamped to the truck's CURRENT lane on every hazard event, folds
    // included. So a hazard folding in while the driver was answering the
    // last one moved with them: dodge, get re-armed in the lane just reached,
    // dodge again. Obeying the instruction is what made the instruction come
    // back.
    let mut harness = a_drive("Folded Lane");
    at_mph(&mut harness, HAZARD_SAFE_MPH + 10.0);
    harness.with_drive(|drive, _| drive.lane.lane = 0);

    arm(
        &mut harness,
        a_hazard(
            "Change lanes or brake! Retread debris from a blown tire.",
            4.0,
            true,
            "the tire debris",
        ),
    );
    assert_eq!(harness.read_drive(|d| d.hazard_lane), 0);

    // The driver does exactly what they were told and moves over.
    harness.with_drive(|drive, _| drive.lane.lane = 1);

    // A second hazard folds in while the first is still live. It must not
    // drag the hazard into lane 1 with the truck.
    at_mph(&mut harness, HAZARD_SAFE_MPH + 10.0);
    arm(
        &mut harness,
        a_hazard(
            "Change lanes or brake! A shredded tire carcass.",
            4.0,
            true,
            "the carcass",
        ),
    );
    assert_eq!(
        harness.read_drive(|d| d.hazard_lane),
        0,
        "the hazard stayed where it was"
    );
    // And the dodge the driver already made still counts as an answer.
    assert_ne!(
        harness.read_drive(|d| d.lane.lane),
        harness.read_drive(|d| d.hazard_lane)
    );
}
