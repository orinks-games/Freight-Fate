//! The delivery exit end to end: which interchange it picks and how that
//! answer is cached (`states/driving_events/destination_exit.rs`), what the
//! callout says, the countdown an armed exit runs
//! (`states/driving_events/update_exit.rs`), what cruise does about it, and
//! what happens when the driver blows past it
//! (`states/driving_events/arrival.rs`).
//!
//! Ported from the destination-exit block of `tests/test_driving_features.py`
//! (`test_armed_exit_counts_down` through
//! `test_destination_exit_completion_clears_remaining_route_miles`, plus
//! `test_the_destination_exit_call_outranks_chatter`).
//!
//! Three Python seams have no Rust equivalent and are arranged for real:
//! `monkeypatch.setattr(driving, "_upcoming_exit_stop", ...)` becomes a real
//! stop on the trip with the destination exit marked taken, so the shipped
//! lookup reaches it; `monkeypatch.setattr(driving,
//! "_destination_exit_details", ...)` becomes a primed
//! `destination_exit_cache`, which is the field that answer really lives in;
//! and `_scan_destination_exit_details` cannot be counted, so the caching
//! case watches the cache itself -- a sentinel written into it survives every
//! call that must not rescan and is replaced by the ones that must.

use ff_core::data::world::get_world;
use ff_core::data::world_models::Interchange;
use ff_core::models::jobs::{Job, CARGO_CATALOG};
use ff_core::models::profile::Profile;
use ff_core::sim::trip_models::{
    NavigationCue, RoadStop, TripEvent, TripEventData, TripEventKind, Zone,
};
use ff_core::sim::weather::{WeatherKind, WeatherSystem};
use ff_core::speech_text::SpokenMessage;

use freight_fate::app::testing::TestApp;
use freight_fate::playtest::harness::{PlaytestHarness, StartDelivery};
use freight_fate::states::driving::DrivingState;
use freight_fate::states::driving_core::{DRIVE_PHASE_DELIVERY, EXIT_MAINLINE_EASE_MPH};
use freight_fate::states::driving_menu_states::FacilityArrivalState;

const DT: f64 = 1.0 / 60.0;
const MPH_PER_MPS: f64 = 2.23694;

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
        drive.trip.set_patrols(Vec::new());
        drive.trip.posts.clear();
        // `driving_feature_helpers.release_air_brakes`: without it the truck
        // is still on the spring brakes and cruise refuses to hold.
        drive.truck_mut().set_air_ready(false);
    });
    harness.clear_speech();
    harness
}

fn frame(harness: &mut PlaytestHarness) {
    harness.advance_clock(DT);
    harness.with_drive(|drive, ctx| drive.update_frame(ctx, DT));
}

fn events(harness: &PlaytestHarness) -> Vec<String> {
    harness.app.event_lines()
}

fn last_event(harness: &PlaytestHarness) -> String {
    events(harness).last().cloned().unwrap_or_default()
}

fn said(harness: &PlaytestHarness) -> Vec<String> {
    harness.app.speech().lines()
}

fn last_said(harness: &PlaytestHarness) -> String {
    said(harness).last().cloned().unwrap_or_default()
}

/// `SimpleNamespace(at_mi=..., type="delivery_destination", ...)`: the bare
/// armed exit the countdown cases walk toward.
fn a_destination_stop(at_mi: f64) -> RoadStop {
    let mut stop = RoadStop::new("Test Receiver", at_mi, "delivery_destination");
    stop.actions = vec!["deliver".to_string()];
    stop
}

/// The anchors an armed exit spoke, in order.
fn countdown_calls(harness: &PlaytestHarness) -> Vec<String> {
    events(harness)
        .into_iter()
        .filter(|line| line.starts_with("Destination exit in"))
        .map(|line| line.split('.').next().unwrap_or_default().to_string())
        .collect()
}

/// Walk an armed exit down the anchors the Python cases walk.
fn walk_the_countdown(harness: &mut PlaytestHarness, at_mi: f64, aheads: &[f64]) {
    for ahead in aheads {
        let ahead = *ahead;
        harness.advance_clock(20.0); // a mile of road between anchors, not an instant
        harness.with_drive(move |drive, ctx| {
            drive.trip.position_mi = at_mi - ahead;
            drive.update_exit(ctx, 0.0, DT);
        });
    }
}

// -- the countdown ---------------------------------------------------------------------

#[test]
fn test_armed_exit_counts_down() {
    // An armed exit re-anchors itself at two miles, one mile, half a mile.
    // Backport of the 1.9-line countdown: a signal-on announcement miles out
    // was the last word before the miss -- 1.8 players kept losing exits armed
    // under scenery chatter.
    let mut harness = a_drive("Countdown");
    let at_mi = harness.read_drive(|d| d.trip.position_mi) + 3.0;
    harness.with_drive(move |drive, _| {
        drive.exit_stop = Some(a_destination_stop(at_mi));
        drive.exit_countdown_said.clear();
    });
    harness.clear_speech();

    walk_the_countdown(&mut harness, at_mi, &[2.5, 1.9, 1.9, 0.9, 0.4, 0.3]);

    // Each anchor speaks once, in order, in every lane-keeping mode: the
    // separate two-mile "exit lane" prompt is gone, since the exit lane is
    // asked for where it opens.
    assert_eq!(
        countdown_calls(&harness),
        vec![
            "Destination exit in 2 miles".to_string(),
            "Destination exit in 1 mile".to_string(),
            "Destination exit in half a mile".to_string(),
        ]
    );
}

#[test]
fn test_every_anchor_asks_for_the_signal_until_it_is_set() {
    // Doing everything the approach asks except the signal is still a miss,
    // and until 2026-09-19 no line on the approach asked. An agent drove
    // AZ-260 into Payson steering right and slowing for the ramp exactly as
    // told, heard three calls that named the lane and the ramp only, and the
    // first mention of a signal in the whole run was the miss itself.
    //
    // So every anchor asks while the signal is off, and the moment it is set
    // it stops asking -- a countdown still nagging for a signal already on is
    // the same failure the other way round.
    let mut harness = a_drive("Anchor Signal");
    // The signal gate is the manual driver's; full lane keeping takes the exit.
    harness.app.ctx.settings.lane_keeping = "partial".to_string();
    let at_mi = harness.read_drive(|d| d.trip.position_mi) + 3.0;
    harness.with_drive(move |drive, _| {
        drive.exit_stop = Some(a_destination_stop(at_mi));
        drive.exit_countdown_said.clear();
    });
    harness.clear_speech();

    walk_the_countdown(&mut harness, at_mi, &[0.9]);

    let asked: Vec<String> = events(&harness)
        .into_iter()
        .filter(|line| line.starts_with("Destination exit in"))
        .collect();
    assert_eq!(asked.len(), 1, "{asked:#?}");
    assert!(asked[0].to_lowercase().contains("signal"), "{}", asked[0]);

    // Signal set: the anchor at half a mile has nothing left to say about it.
    harness.with_drive(|drive, _| drive.exit_signal_on = true);
    harness.clear_speech();

    walk_the_countdown(&mut harness, at_mi, &[0.4]);

    let after: Vec<String> = events(&harness)
        .into_iter()
        .filter(|line| line.starts_with("Destination exit in"))
        .collect();
    assert_eq!(after.len(), 1, "{after:#?}");
    assert!(!after[0].to_lowercase().contains("signal"), "{}", after[0]);
}

#[test]
fn test_armed_exit_counts_down_from_two_miles_when_the_truck_holds_the_lane() {
    let mut harness = PlaytestHarness::new();
    harness.app.ctx.settings.lane_keeping = "full".to_string();
    harness.start_delivery(StartDelivery::named("Countdown Full"));
    harness.with_drive(|drive, _| {
        drive.tutorial = None;
        drive.departure_checked = true;
        drive.trip.hazard_check_mi = 1e9;
        drive.trip.inspection_check_mi = 1e9;
        drive.trip.traffic_manager.rolling_bubble = false;
        drive.trip.set_npc_vehicles(Vec::new());
        drive.trip.zones.retain(|z| z.aadt.is_none());
        drive.trip.weather.current = WeatherKind::Clear;
        drive.trip.set_patrols(Vec::new());
        drive.trip.posts.clear();
    });
    let at_mi = harness.read_drive(|d| d.trip.position_mi) + 3.0;
    harness.with_drive(move |drive, _| {
        drive.exit_stop = Some(a_destination_stop(at_mi));
        drive.exit_countdown_said.clear();
    });
    harness.clear_speech();

    walk_the_countdown(&mut harness, at_mi, &[2.5, 1.9, 1.9, 0.9, 0.4, 0.3]);

    assert_eq!(
        countdown_calls(&harness),
        vec![
            "Destination exit in 2 miles".to_string(),
            "Destination exit in 1 mile".to_string(),
            "Destination exit in half a mile".to_string(),
        ]
    );
}

#[test]
fn test_armed_exit_countdown_silent_on_terse() {
    // Terse speech opts out of the countdown; the signal-on line stays last.
    let mut harness = a_drive("Countdown Terse");
    harness.app.ctx.settings.driving_speech = "quiet".to_string();
    let at_mi = harness.read_drive(|d| d.trip.position_mi) + 3.0;
    harness.with_drive(move |drive, _| {
        drive.exit_stop = Some(a_destination_stop(at_mi));
        drive.exit_countdown_said.clear();
    });
    harness.clear_speech();

    walk_the_countdown(&mut harness, at_mi, &[2.5, 1.9, 0.9, 0.3]);

    assert!(
        countdown_calls(&harness).is_empty(),
        "{:#?}",
        events(&harness)
    );
}

// -- which interchange ---------------------------------------------------------------

/// A Buffalo to Rochester delivery built straight from the world, with the
/// last leg's interchanges replaced when `interchanges` is given.
fn a_rochester_run(
    app: &mut TestApp,
    name: &str,
    interchanges: Option<Vec<Interchange>>,
) -> DrivingState {
    let world = get_world();
    app.ctx.profile = Some(Profile::named_in(name, "Buffalo"));
    let mut route = world
        .supported_route("Buffalo", "Rochester", None)
        .expect("the world routes")
        .expect("Buffalo to Rochester has a route");
    if let Some(interchanges) = interchanges {
        let last = route.legs.len() - 1;
        let leg = &route.legs[last];
        let mut detail = leg.corridor().clone();
        detail.interchanges = interchanges;
        let replaced = ff_core::data::world_models::Leg::new(
            &leg.a,
            &leg.b,
            leg.miles,
            &leg.highway,
            &leg.terrain,
            leg.stops.clone(),
        )
        .with_detail(detail);
        route.legs[last] = std::sync::Arc::new(replaced);
    }
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
    DrivingState::new(
        &mut app.ctx,
        job,
        route,
        Some(11),
        DRIVE_PHASE_DELIVERY,
        None,
    )
}

fn an_interchange(at_mi: f64, exit_ref: &str, destination: &str, highway: &str) -> Interchange {
    Interchange {
        at_mi,
        exit_ref: exit_ref.to_string(),
        destinations: vec![destination.to_string()],
        name: String::new(),
        via: String::new(),
        highway: highway.to_string(),
        source: "test".to_string(),
        ramp_control: String::new(),
        ramp_far_end: String::new(),
        ..Default::default()
    }
}

#[test]
fn test_delivery_exit_uses_real_destination_interchange() {
    let mut app = TestApp::new();
    let mut drive = a_rochester_run(&mut app, "Rochester Exit", None);

    let destination = drive
        .destination_exit_stop(&mut app.ctx)
        .expect("the delivery has a destination exit");

    assert!(!destination.exit_label.is_empty(), "{destination:?}");
    assert!(
        (destination.at_mi - 72.8).abs() <= 0.2,
        "{}",
        destination.at_mi
    );
}

#[test]
fn test_delivery_exit_prefers_nearest_interchange_over_early_city_sign() {
    let mut app = TestApp::new();
    let world = get_world();
    let route = world
        .supported_route("Buffalo", "Rochester", None)
        .expect("the world routes")
        .expect("Buffalo to Rochester has a route");
    let last = &route.legs[route.legs.len() - 1];
    let (miles, highway) = (last.miles, last.highway.clone());
    let mut drive = a_rochester_run(
        &mut app,
        "Nearest Exit",
        Some(vec![
            an_interchange(10.0, "10", "Rochester", &highway),
            an_interchange(miles - 1.0, "near", "Freight district", &highway),
        ]),
    );

    let details = drive
        .destination_exit_details(&app.ctx, false)
        .expect("a destination exit was found");

    assert_eq!(details.1, "exit near");
}

#[test]
fn test_destination_exit_scan_is_cached_until_the_exit_passes() {
    // The scan walks every interchange on the route building spoken phrases,
    // and check_destination_exit runs every frame -- the cache must absorb
    // that (issue 70's crash landed in this per-frame churn) while still
    // rescanning when the winning exit is passed or the truck moves backward.
    //
    // Python counted calls into the scan. The cache is what the count was
    // standing in for, so it is watched directly: a sentinel written over the
    // cached answer survives exactly the calls that must not rescan.
    let mut app = TestApp::new();
    let world = get_world();
    let route = world
        .supported_route("Buffalo", "Rochester", None)
        .expect("the world routes")
        .expect("Buffalo to Rochester has a route");
    let last = &route.legs[route.legs.len() - 1];
    let (miles, highway) = (last.miles, last.highway.clone());
    let mut drive = a_rochester_run(
        &mut app,
        "Cached Exit",
        Some(vec![
            an_interchange(10.0, "10", "Rochester", &highway),
            an_interchange(miles - 1.0, "near", "Freight district", &highway),
        ]),
    );

    let first = drive
        .destination_exit_details(&app.ctx, false)
        .expect("a destination exit was found");
    assert_eq!(first.1, "exit near");

    // A sentinel in place of the cached answer: a second call at the same
    // milepost must hand the sentinel back, which is only possible if it
    // never rescanned.
    let sentinel = (first.0, "exit sentinel".to_string(), String::new());
    let pos = drive.trip.position_mi;
    drive.destination_exit_cache = Some((pos, Some(sentinel.clone())));
    assert_eq!(
        drive.destination_exit_details(&app.ctx, false),
        Some(sentinel)
    );

    // Passing the winning exit forces one rescan; nothing is left ahead, and
    // that empty answer is itself cached.
    drive.trip.position_mi = first.0 + 0.1;
    assert_eq!(drive.destination_exit_details(&app.ctx, false), None);
    drive.destination_exit_cache = Some((drive.trip.position_mi, None));
    assert_eq!(drive.destination_exit_details(&app.ctx, false), None);

    // A backward move (the missed-exit rewind) brings the exit back.
    drive.trip.position_mi = first.0 - 1.0;
    assert_eq!(
        drive.destination_exit_details(&app.ctx, false),
        Some(first.clone())
    );

    // include_past bypasses the cache entirely for the one-shot callers: the
    // sentinel is ignored and the real answer comes back.
    drive.destination_exit_cache = Some((
        drive.trip.position_mi,
        Some((first.0, "exit sentinel".to_string(), String::new())),
    ));
    assert_eq!(drive.destination_exit_details(&app.ctx, true), Some(first));
}

// -- what the callout says ---------------------------------------------------------------

#[test]
fn test_terse_destination_exit_omits_press_x_instruction() {
    let mut harness = a_drive("Terse Callout");
    harness.app.ctx.settings.driving_speech = "quiet".to_string();
    harness.with_drive(|drive, ctx| {
        let destination = drive
            .destination_exit_stop(ctx)
            .expect("a delivery run has a destination exit");
        drive.trip.position_mi = destination.at_mi - 4.0;
    });
    harness.clear_speech();

    harness.with_drive(|drive, ctx| drive.check_destination_exit(ctx));

    let (message, interrupt) = harness
        .app
        .event_calls()
        .last()
        .cloned()
        .expect("the callout speaks");
    assert!(!interrupt, "the callout queues rather than cutting in");
    assert!(message.contains("destination exit"), "{message}");
    assert!(!message.contains("Press X"), "{message}");
    assert!(!message.contains("take it"), "{message}");
}

#[test]
fn test_destination_exit_announcement_names_lane_move_when_drift_is_on() {
    let mut harness = PlaytestHarness::new();
    harness.app.ctx.settings.lane_keeping = "partial".to_string();
    harness.start_delivery(StartDelivery::named("Drift Callout"));
    harness.with_drive(|drive, ctx| {
        drive.tutorial = None;
        drive.departure_checked = true;
        drive.trip.hazard_check_mi = 1e9;
        drive.trip.inspection_check_mi = 1e9;
        drive.trip.traffic_manager.rolling_bubble = false;
        drive.trip.set_npc_vehicles(Vec::new());
        drive.trip.zones.retain(|z| z.aadt.is_none());
        drive.trip.weather.current = WeatherKind::Clear;
        drive.trip.set_patrols(Vec::new());
        drive.trip.posts.clear();
        let destination = drive
            .destination_exit_stop(ctx)
            .expect("a delivery run has a destination exit");
        drive.trip.position_mi = destination.at_mi - 4.0;
        // Out of the right lane: the move is owed.
        drive.lane.lane_count = 2;
        drive.lane.lane = 1;
    });
    harness.clear_speech();

    harness.with_drive(|drive, ctx| drive.check_destination_exit(ctx));

    let said = last_event(&harness);
    assert!(
        said.to_lowercase().contains("move to the right lane"),
        "{said}"
    );
    // The lane move is the second half of the instruction; the signal is the
    // first, and it is the one that decides whether the exit is taken at all
    // (agent drive into Payson, 2026-09-19).
    assert!(said.to_lowercase().contains("signal"), "{said}");
    assert!(!said.contains("X takes"), "{said}");
}

#[test]
fn test_destination_exit_suppresses_matching_interchange_gps_cue() {
    let mut harness = a_drive("Suppress Cue");
    let at_mi = harness.with_drive(|drive, ctx| {
        let destination = drive
            .destination_exit_stop(ctx)
            .expect("a delivery run has a destination exit");
        drive.trip.position_mi = destination.at_mi - 1.0;
        destination.at_mi
    });
    harness.clear_speech();

    harness.with_drive(|drive, ctx| drive.check_destination_exit(ctx));
    harness.with_drive(move |drive, ctx| {
        let cue = NavigationCue::new(
            "interchange:test",
            "interchange",
            at_mi,
            "generic exit cue",
            "generic exit cue",
        );
        drive.handle_trip_event(
            ctx,
            &TripEvent {
                kind: TripEventKind::GpsCue,
                message: SpokenMessage::new("Exit ahead from generic navigation cue."),
                data: TripEventData {
                    cue: Some(cue),
                    ..Default::default()
                },
            },
        );
    });

    let lines = events(&harness);
    assert_eq!(lines.len(), 1, "{lines:#?}");
    assert!(lines[0].contains("destination exit"), "{lines:#?}");
    assert!(!lines[0].contains("generic navigation cue"), "{lines:#?}");
}

#[test]
fn test_the_destination_exit_call_outranks_chatter() {
    // The line that says lane keeping is taking the exit must not be dropped.
    //
    // On full lane keeping the truck leaves the highway without the driver
    // touching anything, and this announcement is the only warning that it is
    // about to. At the AMBIENT default it was dropped whenever another line
    // landed in the same moment, so the exit read as taking itself -- reported
    // twice (Sarah A, 2026-08-15, "seems to be random").
    //
    // Python read the priority off its stubbed `say_event`. There is no stub
    // here, so the property is measured where a player meets it: the callout
    // still arrives with an ambient line filling the channel, which is
    // exactly what an AMBIENT priority would lose.
    let mut harness = PlaytestHarness::new();
    harness.app.ctx.settings.lane_keeping = "full".to_string();
    harness.start_delivery(StartDelivery::named("Outranks Chatter"));
    harness.with_drive(|drive, ctx| {
        drive.tutorial = None;
        drive.departure_checked = true;
        drive.trip.hazard_check_mi = 1e9;
        drive.trip.inspection_check_mi = 1e9;
        drive.trip.traffic_manager.rolling_bubble = false;
        drive.trip.set_npc_vehicles(Vec::new());
        drive.trip.zones.retain(|z| z.aadt.is_none());
        drive.trip.weather.current = WeatherKind::Clear;
        drive.trip.set_patrols(Vec::new());
        drive.trip.posts.clear();
        let destination = drive
            .destination_exit_stop(ctx)
            .expect("a delivery run has a destination exit");
        drive.trip.position_mi = destination.at_mi - 1.0;
        drive.destination_exit_announced_key = String::new();
    });
    harness.clear_speech();
    // Ambient chatter filling the channel in the same moment.
    harness
        .app
        .ctx
        .say_event("A weathered barn stands off the shoulder.");

    harness.with_drive(|drive, ctx| drive.check_destination_exit(ctx));

    let lines = events(&harness);
    assert!(
        lines.iter().any(|line| line.contains("destination exit")),
        "the destination exit was never announced: {lines:#?}"
    );
}

// -- what cruise does about it -------------------------------------------------------------

#[test]
fn test_destination_exit_keeps_cruise_and_eases_for_ramp() {
    // Pin the exit signage: the random job assignment picks the route, and not
    // every destination exit carries a "toward" phrase in its sign data, so
    // scanning the real interchanges on whatever route dispatch drew is a coin
    // flip. Python replaced the lookup with a fixed tuple; here the route
    // carries a real interchange whose sign reads that way, so the phrase
    // comes out of the same scan the game runs.
    let mut app = TestApp::new();
    let world = get_world();
    let route = world
        .supported_route("Buffalo", "Rochester", None)
        .expect("the world routes")
        .expect("Buffalo to Rochester has a route");
    let last = &route.legs[route.legs.len() - 1];
    let (miles, highway) = (last.miles, last.highway.clone());
    let mut signed = an_interchange(miles - 1.0, "20", "Memphis", &highway);
    signed.via = "US 64 East".to_string();
    // The line asks for the signal, which only a driver holding the lane owes.
    app.ctx.settings.lane_keeping = "partial".to_string();
    let mut drive = a_rochester_run(&mut app, "Cruise Eases", Some(vec![signed]));
    drive.trip.hazard_check_mi = 1e9;
    drive.trip.inspection_check_mi = 1e9;
    drive.trip.traffic_manager.rolling_bubble = false;
    drive.trip.set_npc_vehicles(Vec::new());
    drive.trip.set_patrols(Vec::new());
    drive.trip.posts.clear();
    drive.trip.weather.current = WeatherKind::Clear;
    drive.departure_checked = true;
    drive.tutorial = None;
    let destination = drive
        .destination_exit_stop(&mut app.ctx)
        .expect("the delivery has a destination exit");
    drive.trip.position_mi = destination.at_mi - 4.0;
    drive.trip.truck.start_engine();
    drive.trip.truck.velocity_mps = 60.0 / MPH_PER_MPS;
    drive.cruise_mph = Some(60.0);
    drive.speed_control_target_mph = Some(60.0);
    app.clear_speech();

    drive.check_destination_exit(&mut app.ctx);

    assert_eq!(drive.cruise_mph, Some(60.0));
    // The exit's own floor, not a flat 40 for every exit in the country, and
    // not the ramp's number either: at most ten under road speed at the gore,
    // because the ramp's speed is braked for past it (realistic exit,
    // 2026-09-24).
    let ramp_mph = 60.0f64.min(drive.exit_approach_floor_mph(Some(&destination)));
    let (road, _) = drive.trip.speed_limit_at(destination.at_mi);
    assert!(
        ramp_mph >= road - EXIT_MAINLINE_EASE_MPH,
        "{ramp_mph} on a {road}"
    );
    assert!(
        ramp_mph < 59.0,
        "this corridor's exit floor sits under the set speed"
    );
    assert_eq!(drive.cruise_exit_mph, Some(ramp_mph));
    let (message, interrupt) = app
        .event_calls()
        .last()
        .cloned()
        .expect("the callout speaks");
    assert!(!interrupt);
    assert!(message.contains("exit "), "{message}");
    assert!(message.contains("toward"), "{message}");
    assert!(message.contains("destination exit"), "{message}");
    // Already in the right lane: no lane instruction at all, the exit lane
    // is called where it opens (owner's drive into Denver West, 2026-09-24).
    assert!(!message.contains("lane"), "{message}");
    // And the gate the lane work is in aid of. Doing everything this line
    // asks except the signal is still a miss, so the line has to ask for it
    // (agent drive into Payson, 2026-09-19).
    assert!(message.to_lowercase().contains("signal"), "{message}");
    assert!(!message.contains("X takes"), "{message}");
    // How cruise takes the ramp is not narrated (owner, 2026-10-14).
    assert!(!message.contains("Adaptive cruise"), "{message}");

    // The ramp approach target is this exit's own, so read it rather than
    // spelling a number that now depends on the road (owner, 2026-08-21).
    app.clear_speech();
    drive.adjust_cruise(&mut app.ctx, -1, false);
    assert_eq!(
        app.main_lines().last().cloned().unwrap_or_default(),
        format!(
            "Open-road cruise target 55 miles per hour. Ramp approach target {ramp_mph:.0} miles per hour."
        )
    );
    for _ in 0..3 {
        drive.adjust_cruise(&mut app.ctx, -1, false);
    }
    // Wound below the ramp's own number, the driver's setting wins: the spoken
    // approach target is never ABOVE what they asked the truck to do, even
    // though the ramp's own figure is what got stored.
    let spoken_target = drive
        .cruise_mph
        .unwrap_or(0.0)
        .min(drive.cruise_exit_mph.unwrap_or(f64::INFINITY));
    let said = app.main_lines().last().cloned().unwrap_or_default();
    assert!(
        said.ends_with(&format!(
            "Ramp approach target {spoken_target:.0} miles per hour."
        )),
        "{said}"
    );
}

#[test]
fn test_taking_the_announced_exit_does_not_repeat_the_ramp_cap() {
    let mut harness = a_drive("No Repeat Cap");
    harness.with_drive(|drive, ctx| {
        let stop = drive
            .destination_exit_stop(ctx)
            .expect("a delivery run has a destination exit");
        drive.trip.position_mi = stop.at_mi - 3.0;
        drive.truck_mut().start_engine();
        // At road speed, so the exit floor sits under it.
        let (road, _) = drive.trip.speed_limit_at(stop.at_mi);
        drive.truck_mut().velocity_mps = road / MPH_PER_MPS;
        drive.engage_cruise(ctx, road, false);
    });
    harness.clear_speech();

    // Announces the exit and caps cruise.
    harness.with_drive(|drive, ctx| drive.check_destination_exit(ctx));
    let ramp_mph = harness
        .read_drive(|d| d.cruise_exit_mph)
        .expect("the callout capped cruise");
    let set = harness.read_drive(|d| d.cruise_mph).expect("cruise");
    assert!(ramp_mph < set - 1.0, "{ramp_mph} under {set}");
    assert!(
        !last_said(&harness).contains("Adaptive cruise"),
        "{}",
        last_said(&harness)
    );

    harness.advance_clock(10.0);
    harness.clear_speech();
    harness.with_drive(|drive, ctx| drive.take_exit(ctx));

    // The exit key is a turn signal now: "Signal set for ..." (the blinker
    // itself waits for half a mile) replaced the older "Signaling for ..."
    // callout when the cancel/confirm model landed.
    let confirmation = last_said(&harness);
    assert!(confirmation.contains("Signal set for"), "{confirmation}");
    // Already said, and already capped.
    assert!(!confirmation.contains("Adaptive cruise"), "{confirmation}");
    assert_eq!(harness.read_drive(|d| d.cruise_exit_mph), Some(ramp_mph));
}

#[test]
fn test_signaling_for_an_exit_eases_cruise_at_most_ten_under_road_speed() {
    // Pressing X is the commitment to leave the highway, so adaptive cruise
    // takes an exit target with it -- for a truck stop exit just as much as
    // for the destination, and it lets go again on a cancel.
    //
    // The target is where the truck has to BE at the gore, not where it goes
    // the moment the signal is on. Signalling used to start the shed
    // immediately, so a driver who signalled early watched automatic control
    // slow with the exit nowhere in sight (Shane, 2026-08-15). And it is ten
    // under road speed at most, never the ramp's own number: that is braked
    // for past the gore (realistic exit, 2026-09-24).
    let mut harness = a_drive("Signal Eases");
    let road = harness.with_drive(|drive, ctx| {
        // The invented stop is reached through the real lookup: the
        // destination exit is out of the way and this is the only route stop.
        drive.destination_exit_taken = true;
        let mut stop = RoadStop::new("Petro Knoxville", 40.0, "truck_stop");
        stop.actions = ["fuel", "sleep"].iter().map(|a| a.to_string()).collect();
        drive.trip.stops = vec![stop];
        drive.trip.position_mi = 37.0;
        drive.truck_mut().start_engine();
        drive.truck_mut().grade = 0.0;
        // At road speed, the way a drive arrives at an exit.
        let (limit, _) = drive.trip.speed_limit_at(40.0);
        drive.truck_mut().velocity_mps = limit / MPH_PER_MPS;
        // Holding the road, the way a drive arrives here.
        drive.truck_mut().throttle = 0.4;
        drive.engage_cruise(ctx, limit, false);
        limit
    });
    let set_mph = harness
        .read_drive(|d| d.cruise_mph)
        .expect("cruise engaged");
    harness.clear_speech();

    harness.with_drive(|drive, ctx| drive.take_exit(ctx));

    assert_eq!(
        harness.read_drive(|d| d.exit_stop.as_ref().map(|s| s.name.clone())),
        Some("Petro Knoxville".to_string())
    );
    let ramp_mph = harness.with_drive(|d, _| d.exit_approach_floor_mph(None));
    let ramp_cruise = harness.read_drive(|d| d.armed_ramp_cruise_mph(None));
    assert_eq!(ramp_mph, ramp_cruise.max(road - EXIT_MAINLINE_EASE_MPH));
    assert!(
        ramp_mph >= road - EXIT_MAINLINE_EASE_MPH,
        "{ramp_mph} on a {road}"
    );
    assert_eq!(harness.read_drive(|d| d.cruise_exit_mph), Some(ramp_mph));
    assert!(
        !last_said(&harness).contains("Adaptive cruise"),
        "{}",
        last_said(&harness)
    );

    // Three miles out the ramp target is a plan, not a brake: the cap is
    // nowhere near the set speed, and cruise is still holding the road.
    let cap = harness
        .read_drive(|d| d.ramp_approach_cap_mph())
        .expect("an armed exit has an approach cap");
    assert!(
        cap > harness.read_drive(|d| d.cruise_mph.unwrap_or(0.0)),
        "{cap}"
    );
    harness.with_drive(|drive, _| drive.truck_mut().brake = 0.0);
    for _ in 0..4 {
        harness.with_drive(|drive, ctx| {
            drive.update_cruise(ctx, 0.5, false, false, false);
        });
        // Nothing sheds this far out.
        assert_eq!(harness.read_drive(|d| d.truck().brake), 0.0);
    }
    // And the set speed is untouched.
    assert_eq!(harness.read_drive(|d| d.cruise_mph), Some(set_mph));

    // Inside the reaction distance it has reached the ramp target, so this is
    // independent of which real interchange happens to be nearest the
    // invented stop: cruise acts on it with throttle off and brakes on.
    harness.with_drive(|drive, _| drive.trip.position_mi = 40.0 - 0.05);
    let cap = harness
        .read_drive(|d| d.ramp_approach_cap_mph())
        .expect("an armed exit has an approach cap");
    assert!(
        cap < harness.read_drive(|d| d.cruise_mph.unwrap_or(0.0)),
        "{cap}"
    );
    harness.with_drive(|drive, ctx| drive.update_cruise(ctx, 0.5, false, false, false));
    assert_eq!(harness.read_drive(|d| d.truck().throttle), 0.0);
    assert!(harness.read_drive(|d| d.truck().brake) > 0.0);

    // Back up the road to cancel: inside the commit window X no longer means
    // "never mind", and the cancel is what is under test here.
    harness.with_drive(|drive, ctx| {
        drive.trip.position_mi = 37.0;
        drive.take_exit(ctx); // X again cancels
    });
    assert_eq!(harness.read_drive(|d| d.cruise_exit_mph), None);
}

// -- zones the truck will never reach --------------------------------------------------

#[test]
fn test_a_zone_past_the_destination_exit_is_never_announced() {
    // The facility gate covers the last half mile, but a delivery leaves the
    // highway at least a mile before that, so its 15 mph limit was announced
    // and then never took effect. Warn only for zones the truck will drive
    // into.
    let mut harness = a_drive("Unreachable Zone");
    let (total, exit_at) = harness.with_drive(|drive, ctx| {
        let total = drive.trip.total_miles();
        let stop = drive
            .destination_exit_stop(ctx)
            .expect("a delivery run has a destination exit");
        (total, stop.at_mi)
    });
    let gate = Zone::new(total - 0.5, total, 15.0, "facility gate");
    assert!(gate.start_mi >= exit_at, "the delivery is gone by then");
    harness.clear_speech();

    let gate_cue = |zone: Zone, text: &str| TripEvent {
        kind: TripEventKind::GpsCue,
        message: SpokenMessage::new(text),
        data: TripEventData {
            zone: Some(zone),
            ..Default::default()
        },
    };
    let cue = gate_cue(
        gate.clone(),
        "In 2 miles, facility gate ahead. Speed limit 15.",
    );
    harness.with_drive(move |drive, ctx| drive.handle_trip_event(ctx, &cue));
    assert!(events(&harness).is_empty(), "{:#?}", events(&harness));

    // A zone the truck really does reach still gets its heads-up. (The
    // facility-family reasons are suppressed until the exit is taken and
    // replayed on the street chain, so reachability is tested with a
    // highway-side reason.)
    let reachable = Zone::new(exit_at - 2.0, exit_at - 1.0, 35.0, "construction");
    let cue = gate_cue(reachable, "In 2 miles, construction ahead. Speed limit 35.");
    harness.with_drive(move |drive, ctx| drive.handle_trip_event(ctx, &cue));
    assert!(
        last_event(&harness).starts_with("In 2 miles, construction"),
        "{}",
        last_event(&harness)
    );

    // A pickup leg drives all the way to the gate, so it keeps the warning.
    // The clock moves first: in one instant the previous line is still
    // notionally mid-sentence, so this one cuts it and the pacer hands it
    // back to be requeued -- which would leave the requeued line last.
    harness.advance_clock(10.0);
    harness.with_drive(|drive, _| drive.phase = "pickup");
    harness.clear_speech();
    let cue = gate_cue(gate, "In 2 miles, facility gate ahead. Speed limit 15.");
    harness.with_drive(move |drive, ctx| drive.handle_trip_event(ctx, &cue));
    assert!(
        last_event(&harness).starts_with("In 2 miles, facility gate"),
        "{}",
        last_event(&harness)
    );
}

#[test]
fn test_missed_destination_exit_suppresses_facility_zone_cues() {
    let mut harness = a_drive("Missed Zone Cues");
    harness.with_drive(|drive, ctx| {
        drive.destination_exit_taken = false;
        drive.handle_trip_event(
            ctx,
            &TripEvent {
                kind: TripEventKind::GpsCue,
                message: SpokenMessage::new(
                    "In 1 miles, destination approach ahead. Speed limit 35.",
                ),
                data: TripEventData {
                    zone: Some(Zone::new(99.0, 100.0, 35.0, "destination approach")),
                    ..Default::default()
                },
            },
        );
        drive.handle_trip_event(
            ctx,
            &TripEvent {
                kind: TripEventKind::ZoneEnter,
                message: SpokenMessage::new("facility gate ahead. Speed limit 15."),
                data: TripEventData {
                    zone: Some(Zone::new(99.8, 100.0, 15.0, "facility gate")),
                    ..Default::default()
                },
            },
        );
    });
    assert!(events(&harness).is_empty(), "{:#?}", events(&harness));

    harness.with_drive(|drive, _| {
        drive.trip.position_mi = drive.trip.total_miles();
        drive.trip.finished = true;
    });
    frame(&mut harness);

    assert!(
        last_event(&harness)
            .to_lowercase()
            .contains("missed the destination exit"),
        "{}",
        last_event(&harness)
    );
}

// -- missing it ---------------------------------------------------------------------------

#[test]
fn test_delivery_does_not_complete_without_taking_destination_exit() {
    let mut harness = a_drive("No Exit No Delivery");
    harness.with_drive(|drive, _| {
        drive.trip.position_mi = drive.trip.total_miles();
        drive.trip.finished = true;
        drive.truck_mut().velocity_mps = 0.0;
    });
    harness.clear_speech();

    frame(&mut harness);

    assert!(harness.state_is::<DrivingState>());
    assert!(!harness.read_drive(|d| d.trip.finished));
    assert!(
        harness.read_drive(|d| d.trip.position_mi) < harness.read_drive(|d| d.trip.total_miles())
    );
    assert!(harness
        .with_drive(|d, ctx| d.destination_exit_stop(ctx))
        .is_some());
    let said = last_event(&harness).to_lowercase();
    assert!(said.contains("missed the destination exit"), "{said}");
    assert!(said.contains("you turned around"), "{said}");
    assert!(!said.contains("back up"), "{said}");
}

/// Blow past the destination exit and let the loop-back fire.
fn miss_the_destination_exit(harness: &mut PlaytestHarness) -> String {
    harness.with_drive(|drive, _| {
        drive.trip.position_mi = drive.trip.total_miles();
        drive.trip.finished = true;
        drive.truck_mut().velocity_mps = 0.0;
    });
    harness.clear_speech();
    frame(harness);
    events(harness)
        .into_iter()
        .find(|line| line.to_lowercase().contains("missed the destination exit"))
        .unwrap_or_else(|| panic!("nothing said the exit was missed: {:#?}", events(harness)))
}

#[test]
fn test_the_loop_back_never_tells_an_automated_driver_to_signal() {
    // Issue #155. With lane keeping on full the truck takes the destination
    // exit itself, so the turnaround must not send the driver reaching for
    // the take-exit control for an exit that is no longer theirs to take.
    let mut harness = a_drive("Loop Back Automated");
    harness.app.ctx.settings.lane_keeping = "full".to_string();

    let missed = miss_the_destination_exit(&mut harness);

    assert!(
        !missed.contains("press X") && !missed.contains("Press X"),
        "{missed}"
    );
    assert!(missed.contains("ahead again"), "{missed}");
}

#[test]
fn test_the_loop_back_still_asks_a_manual_driver_for_the_signal() {
    for mode in ["partial", "off"] {
        let mut harness = a_drive("Loop Back Manual");
        harness.app.ctx.settings.lane_keeping = mode.to_string();

        let missed = miss_the_destination_exit(&mut harness);

        assert!(missed.contains("Press X"), "lane keeping {mode}: {missed}");
        drop(harness);
    }
}

#[test]
fn test_the_full_lane_keeping_callout_names_only_the_exit() {
    // The driver chose lane keeping; the callout names the exit and stops
    // there, on the first approach and the loop-back alike (owner,
    // 2026-10-14).
    let mut harness = a_drive("Loop Back Warning");
    harness.app.ctx.settings.lane_keeping = "full".to_string();

    let missed = miss_the_destination_exit(&mut harness);

    assert!(!missed.contains("Lane keeping"), "{missed}");
    let announcement = harness.with_drive(|drive, ctx| {
        let stop = a_destination_stop(drive.trip.position_mi + 3.0);
        drive.destination_exit_announcement(ctx, &stop, 3.0)
    });
    assert!(
        announcement == "In 3 miles, the destination exit for Test Receiver.",
        "{announcement}"
    );
}

#[test]
fn test_missed_destination_recovery_does_not_keep_issuing_gate_speed_strikes() {
    let mut harness = a_drive("Recovery Strikes");
    harness.with_drive(|drive, _| {
        drive.trip.position_mi = drive.trip.total_miles();
        drive.trip.finished = true;
        drive.truck_mut().velocity_mps = 85.0 / MPH_PER_MPS;
    });
    harness.clear_speech();

    frame(&mut harness);
    assert!(harness.state_is::<DrivingState>());
    // Twenty over the limit also earns a speeding warning in the same frame,
    // so the missed-exit line is looked up rather than assumed last.
    let missed = events(&harness)
        .into_iter()
        .find(|line| line.to_lowercase().contains("missed the destination exit"))
        .unwrap_or_else(|| panic!("nothing said the exit was missed: {:#?}", events(&harness)));
    assert!(!missed.to_lowercase().contains("back up"), "{missed}");

    harness.advance_clock(7.0);
    harness.with_drive(|drive, ctx| drive.update_frame(ctx, 7.0));

    assert_eq!(harness.read_drive(|d| d.speeding_tickets), 0);
    assert!(
        !events(&harness)
            .iter()
            .any(|line| line.contains("End of facility gate zone")),
        "{:#?}",
        events(&harness)
    );
}

// -- taking it --------------------------------------------------------------------------

/// `driving_feature_helpers.take_destination_exit`: move onto the delivery
/// ramp and stop at the destination gate.
fn take_destination_exit(harness: &mut PlaytestHarness) {
    harness.app.ctx.settings.destination_approach_assist = false;
    harness.with_drive(|drive, ctx| {
        let destination = drive
            .destination_exit_stop(ctx)
            .expect("a delivery run has a destination exit");
        drive.exit_stop = Some(destination.clone());
        drive.exit_lane_entered = true;
        drive.trip.position_mi = destination.at_mi;
        drive.truck_mut().velocity_mps = 0.0;
        drive.update_exit(ctx, 0.0, DT);
        let ramp = drive.ramp_mi.unwrap_or(0.0);
        drive.update_exit(ctx, ramp, DT);
        if drive.surface_chain {
            // Chain-capable destinations flow off the ramp onto city streets;
            // fast-forward the street chain to the facility gate.
            drive.trip.position_mi = drive.trip.total_miles();
            drive.trip.finished = true;
            drive.truck_mut().velocity_mps = 0.0;
            drive.truck_mut().set_parking_brake();
            drive.handle_arrival_gate(ctx);
        }
    });
    harness.finish_timed_state();
    if harness.state_is::<DrivingState>() {
        harness.with_drive(|drive, ctx| {
            drive.trip.position_mi = drive.trip.total_miles();
            drive.trip.finished = true;
            drive.truck_mut().set_parking_brake();
            drive.handle_arrival_gate(ctx);
        });
        harness.finish_timed_state();
    }
}

#[test]
fn test_destination_exit_opens_delivery_gate() {
    let mut harness = a_drive("Gate Opens");

    take_destination_exit(&mut harness);

    assert!(harness.state_is::<FacilityArrivalState>());
    // Either delivery is a valid arrival: a dock if this receiver unloads
    // live, dropping the loaded box if they have a yard for it.
    let focused = harness.focused_label().expect("a focused row");
    assert!(
        focused == "Dock and deliver" || focused == "Drop the loaded trailer and hook an empty",
        "{focused}"
    );
}

#[test]
fn test_destination_exit_completion_clears_remaining_route_miles() {
    let mut harness = a_drive("Miles Cleared");

    take_destination_exit(&mut harness);

    assert!(harness.state_is::<FacilityArrivalState>());
    assert!(harness.read_drive(|d| d.trip.finished));
    assert!(
        (harness.read_drive(|d| d.trip.position_mi) - harness.read_drive(|d| d.trip.total_miles()))
            .abs()
            < 1e-6
    );
    assert!(harness.read_drive(|d| d.trip.remaining_miles()).abs() < 1e-6);
    let lines = harness.with_drive(|d, ctx| d.status_lines(ctx));
    assert!(
        lines.iter().any(|line| line.contains("0 miles remaining")),
        "{lines:#?}"
    );
}

#[test]
fn test_the_gore_of_a_labeled_exit_never_invents_a_second_destination_exit() {
    // Agent drive into Abilene, 2026-09-22: inside the last 0.05 mile before
    // exit 286A the scan found nothing ahead, the drive fell back to an
    // estimated exit near the route's end, and the cab said "In 3 miles, the
    // destination exit" seconds before the truck took 286A.
    let mut harness = a_drive("Gore");
    let labeled = harness.with_drive(|drive, ctx| drive.scan_destination_exit_details(ctx, false));
    let (exit_at, _, _) = labeled.expect("the default delivery route has a labeled exit");

    harness.with_drive(|drive, ctx| {
        drive.trip.position_mi = exit_at - 0.03;
        drive.destination_exit_cache = None;
        assert!(
            drive.destination_exit_stop(ctx).is_none(),
            "no second destination exit at the gore of the real one"
        );
        drive.trip.position_mi = exit_at + 0.2;
        drive.destination_exit_cache = None;
        assert!(drive.destination_exit_stop(ctx).is_none());
    });
    harness.with_drive(|drive, ctx| drive.check_destination_exit(ctx));
    assert!(
        !events(&harness)
            .iter()
            .any(|line| line.contains("destination exit")),
        "{:?}",
        events(&harness)
    );
}

/// Keeps the weather import honest for the rigging above.
#[allow(dead_code)]
fn _weather_kind_is_used() -> WeatherSystem {
    WeatherSystem::new("heartland", Some(3), None, None, true)
}
