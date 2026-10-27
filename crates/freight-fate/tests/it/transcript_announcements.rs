//! Announcement priority (safety preempts chatter) and speed-scaled lead
//! time (port of `tests/test_announcements.py`).
//!
//! # Where this differs from the Python
//!
//! Python patched `ctx.say_event` and read the `(text, kwargs)` the call site
//! passed -- so it could assert `priority is EventPriority.ROUTE` and
//! `interrupt is False` straight off the keyword arguments. Rust records at
//! `ctx.speech`, one rung BELOW the ladder and the pacer, so the keywords are
//! not visible there. Each of those cases is ported in two halves that
//! together say more than the original did: the classification is asserted
//! directly through `DrivingState::event_priority` (the same function the
//! call site consults), and the delivery is asserted from the transcript --
//! the line reached the voice, in the right order, without an interrupt.
//!
//! The interrupt flag IS visible at the capture, because `say_event` passes
//! it through to the sink, so `(text, interrupt)` assertions port verbatim.
//!
//! The pacer is also live here, and it measures staleness on the WALL clock
//! while these tests advance the ambient queue in simulated seconds. A test
//! that hands `update_ambient_events` 2.5 s while no real time passes is
//! telling the pacer the voice is still mid-sentence, and the line it drains
//! is dropped as stale -- which is the harness's own known trap, not the
//! game misbehaving. So every test here runs the pacer on a fake clock and
//! advances it by exactly the seconds it advances the drive, which is what a
//! real drive has.

use ff_core::sim::enforcement_posts::{method_by_kind, EnforcementPost};
use ff_core::sim::trip::{Trip, TripOptions};
use ff_core::sim::trip_models::{
    NavigationCue, RoadStop, TripEvent, TripEventData, TripEventKind, Zone,
    ZONE_WARNING_LOOKAHEAD_MI, ZONE_WARNING_MAX_MI, ZONE_WARNING_REAL_S,
};
use ff_core::sim::vehicle::TruckState;
use ff_core::sim::weather::WeatherSystem;
use hearken::speech::EventPriority;

use ff_core::data::world::get_world;
use ff_core::models::jobs::{cargo_type, Job};
use ff_core::models::profile::Profile;
use ff_core::sim::driving_modes::tuning_for_time_scale;
use freight_fate::app::testing::{FakeClock, TestApp};
use freight_fate::states::base::{InputEvent, Key, Mods};
use freight_fate::states::driving::DrivingState;
use freight_fate::states::driving_core::{AMBIENT_EVENT_SPACING_S, DRIVE_PHASE_DELIVERY};
use freight_fate::states::driving_events::AMBIENT_QUEUE_MAX_AGE_S;

/// Advance the ambient queue and the pacer's clock together, so simulated
/// seconds and the staleness projection agree (see the module note).
fn advance_ambient(app: &mut TestApp, d: &mut DrivingState, clock: &FakeClock, seconds: f64) {
    clock.advance(seconds);
    d.update_ambient_events(&mut app.ctx, seconds);
}

/// `_driving(app)`: a loaded Buffalo -> Rochester delivery, not on the stack.
fn a_drive(app: &mut TestApp) -> DrivingState {
    app.ctx.profile = Some(Profile::named_in("Cues", "Buffalo"));
    let route = app
        .ctx
        .world
        .supported_route("Buffalo", "Rochester", None)
        .unwrap()
        .expect("Buffalo to Rochester is supported");
    let miles = route.miles();
    let mut job = Job::new(
        cargo_type("general").unwrap(),
        12.0,
        "Buffalo",
        "company yard",
        "Rochester",
        miles,
        1000.0,
        12.0,
    );
    job.destination_location = "Rochester freight market".to_string();
    DrivingState::new(&mut app.ctx, job, route, None, DRIVE_PHASE_DELIVERY, None)
}

fn event(kind: TripEventKind, text: &str, data: TripEventData) -> TripEvent {
    TripEvent {
        kind,
        message: text.into(),
        data,
    }
}

fn zoned(kind: TripEventKind, text: &str, zone: &Zone) -> TripEvent {
    event(
        kind,
        text,
        TripEventData {
            zone: Some(zone.clone()),
            ..Default::default()
        },
    )
}

/// `enforcement_helpers.always_observing_post`.
fn always_observing_post(at_mi: f64, reach_mi: f64) -> EnforcementPost {
    EnforcementPost {
        method: method_by_kind("median_post").to_string(),
        reach_mi,
        facing: "both".to_string(),
        staffed: true,
        notice: 1.0,
        announced: true,
        ..EnforcementPost::new(at_mi, "median_post")
    }
}

fn cb_chatter(text: &str, at_mi: f64, reach_mi: f64) -> TripEvent {
    event(
        TripEventKind::GpsCue,
        text,
        TripEventData {
            cb_patrol: Some(always_observing_post(at_mi, reach_mi)),
            ..Default::default()
        },
    )
}

// -- classification ------------------------------------------------------------------

#[test]
fn test_only_the_hazard_call_stays_critical() {
    // R1 narrows CRITICAL to act-NOW: every interrupt purges the channel, so
    // class membership is itself a safety property. Zone entries,
    // checkpoints, and zone-ahead/traffic warnings are act-soon and ride
    // ROUTE's never-dropped queue instead; money (a charged toll) joins them,
    // because a consequence must always be heard. Chatter stays ambient.
    let mut app = TestApp::new();
    let d = a_drive(&mut app);
    let zone = Zone::new(5.0, 8.0, 45.0, "construction");

    let hazard = event(
        TripEventKind::Hazard,
        "Brake now!",
        TripEventData::default(),
    );
    assert!(DrivingState::is_critical_event(&hazard));
    assert_eq!(d.event_priority(&hazard), EventPriority::Critical);

    let traffic_cue = NavigationCue::new("traffic:1", "traffic", 12.0, "traffic ahead", "");
    let act_soon = [
        zoned(TripEventKind::ZoneEnter, "zone", &zone),
        zoned(TripEventKind::GpsCue, "construction ahead", &zone),
        event(
            TripEventKind::GpsCue,
            "traffic ahead",
            TripEventData {
                cue: Some(traffic_cue),
                ..Default::default()
            },
        ),
        event(
            TripEventKind::Checkpoint,
            "Passing Grand Island on I-190",
            TripEventData::default(),
        ),
        event(
            TripEventKind::TollCharged,
            "E-ZPass toll charged: 15 dollars.",
            TripEventData::default(),
        ),
    ];
    for e in &act_soon {
        assert!(!DrivingState::is_critical_event(e), "{:?}", e.kind);
        assert_eq!(
            d.event_priority(e),
            EventPriority::Important,
            "{:?}",
            e.kind
        );
    }

    let ambient = [
        cb_chatter("CB radio: patrol ahead", 14.0, 4.0),
        event(
            TripEventKind::WeatherChange,
            "rain",
            TripEventData::default(),
        ),
        event(
            TripEventKind::GpsCue,
            "exit ahead",
            TripEventData::default(),
        ),
    ];
    for e in &ambient {
        assert!(!DrivingState::is_critical_event(e), "{:?}", e.kind);
        assert_eq!(d.event_priority(e), EventPriority::Ambient, "{:?}", e.kind);
    }
}

#[test]
fn test_zone_entry_is_delivered_queued_at_route_priority() {
    // The demoted zone entry goes to the voice queued with ROUTE priority --
    // never an interrupt that could cut a real warning mid-word, never lost
    // behind the one-deep ambient slot either.
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    let zone = Zone::new(5.0, 8.0, 45.0, "construction");
    let entry = zoned(
        TripEventKind::ZoneEnter,
        "Reduced speed zone: construction, 45 miles per hour.",
        &zone,
    );
    // The classification the call site reads (Python asserted this off the
    // `priority=` keyword its `say_event` stub captured).
    assert_eq!(d.event_priority(&entry), EventPriority::Important);

    app.clear_speech();
    d.handle_trip_event(&mut app.ctx, &entry);
    let calls = app.event_calls();
    let (text, interrupt) = calls
        .iter()
        .find(|(text, _)| text.contains("Reduced speed zone"))
        .expect("the zone entry reached the voice");
    assert!(!interrupt);
    assert!(text.contains("Reduced speed zone"));
}

#[test]
fn test_zone_warning_rides_route_while_weather_chatter_stays_ambient() {
    // The zone-ahead warning is act-soon: it queues at ROUTE (short
    // patience, never dropped, requeued if cut) instead of interrupting, and
    // it bypasses the ambient spacing slot chatter waits in.
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    let zone = Zone::new(5.0, 8.0, 45.0, "construction");
    let warning = "Brake now! In 2 miles, construction ahead. Merge left for the \
                   flagger taper; speed limit 55, then 45 through the work zone.";
    let e = zoned(TripEventKind::GpsCue, warning, &zone);
    assert_eq!(d.event_priority(&e), EventPriority::Important);

    app.clear_speech();
    d.handle_trip_event(&mut app.ctx, &e);
    // Spoken at once, never parked in the slot.
    assert_eq!(app.event_calls(), vec![(warning.to_string(), false)]);
}

// -- entry, air, and the horn ---------------------------------------------------------

#[test]
fn test_terse_drive_entry_skips_startup_handholding() {
    let mut app = TestApp::new();
    app.ctx.settings.driving_speech = "quiet".to_string();
    let mut d = a_drive(&mut app);
    app.clear_speech();

    d.enter_drive(&mut app.ctx);

    let spoken = app.main_lines();
    assert!(!spoken.is_empty());
    let entry = &spoken[0];
    assert!(!entry.contains("Press"), "{entry}");
    assert!(!entry.contains("F1"), "{entry}");
    assert!(entry.contains("air"), "{entry}");
    assert!(entry.contains("parking brake"), "{entry}");
}

#[test]
fn test_cold_start_low_air_does_not_stack_on_entry() {
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    assert!(d.truck().air_low_warning());
    assert!(d.low_air_said);
    app.clear_speech();

    d.truck_mut().start_engine();
    // Python's three-argument shim; Rust requires the engine reading too,
    // and a cold start had the engine on already by the time this ran.
    d.update_air_brake_announcements(&mut app.ctx, true, false, true, true);

    assert_eq!(app.event_lines(), Vec::<String>::new());
}

#[test]
fn test_horn_loops_while_key_is_held() {
    let mut app = TestApp::new();
    let log = app.record_audio();
    let mut d = a_drive(&mut app);

    d.handle_key_event(&mut app.ctx, &InputEvent::key_text(Key::H, 'h'));
    d.handle_key_event(&mut app.ctx, &InputEvent::key_up(Key::H));
    assert_eq!(log.borrow().horn, vec!["start", "stop"]);

    d.handle_key_event(&mut app.ctx, &InputEvent::key_text(Key::H, 'h'));
    d.handle_key_event(&mut app.ctx, &InputEvent::key(Key::Escape));
    let horn = log.borrow().horn.clone();
    assert_eq!(&horn[horn.len() - 2..], ["start", "stop"]);
}

// -- the curve callout ----------------------------------------------------------------

#[test]
fn test_curve_callout_setting_controls_the_single_automatic_announcement() {
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    let e = event(
        TripEventKind::Curve,
        "Sharp curve left, half a mile, advisory 35.",
        TripEventData {
            advisory_mph: Some(35.0),
            ..Default::default()
        },
    );
    app.clear_speech();

    d.handle_trip_event(&mut app.ctx, &e);
    // Curve calls interrupt: queued behind chatter they arrived with the
    // bend seconds away (owner's AZ-260 log, 2026-07-19).
    assert_eq!(app.event_calls(), vec![(e.message.normal.clone(), true)]);

    app.ctx.settings.curve_callouts = false;
    d.handle_trip_event(&mut app.ctx, &e);
    assert_eq!(app.event_calls(), vec![(e.message.normal.clone(), true)]);
}

// -- ambient chatter and the spacing slot ---------------------------------------------

#[test]
fn test_cb_radio_chatter_queues_and_uses_cb_audio() {
    let mut app = TestApp::new();
    let log = app.record_audio();
    let mut d = a_drive(&mut app);
    app.clear_speech();

    d.handle_trip_event(
        &mut app.ctx,
        &cb_chatter(
            "CB chatter in 5 miles: drivers report a trooper ahead. \
             Ease back and check your speed.",
            14.0,
            4.0,
        ),
    );

    let played = log.borrow().played.clone();
    assert_eq!(
        played.last().map(|(key, _, _)| key.as_str()),
        Some("events/cb_radio_chatter")
    );
    assert!(!app.event_calls().last().expect("chatter spoke").1);
}

#[test]
fn test_truly_ambient_chatter_is_spaced_without_blocking_safety() {
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    app.clear_speech();

    d.handle_trip_event(
        &mut app.ctx,
        &event(
            TripEventKind::WeatherChange,
            "Weather: rain.",
            TripEventData::default(),
        ),
    );
    d.handle_trip_event(
        &mut app.ctx,
        &cb_chatter(
            "CB chatter in 5 miles: drivers report a trooper ahead.",
            14.0,
            4.0,
        ),
    );
    d.handle_trip_event(
        &mut app.ctx,
        &event(
            TripEventKind::GpsCue,
            "Exit 12 ahead.",
            TripEventData::default(),
        ),
    );

    assert_eq!(
        app.event_calls(),
        vec![
            ("Weather: rain.".to_string(), false),
            ("Exit 12 ahead.".to_string(), false),
        ]
    );

    d.handle_trip_event(
        &mut app.ctx,
        &event(
            TripEventKind::Hazard,
            "Brake now! Debris.",
            TripEventData::default(),
        ),
    );
    assert_eq!(
        app.event_calls().last().cloned(),
        Some(("Brake now! Debris.".to_string(), true))
    );

    d.update_ambient_events(&mut app.ctx, AMBIENT_EVENT_SPACING_S);
    assert_eq!(
        app.event_calls().last().cloned(),
        Some(("Brake now! Debris.".to_string(), true))
    );

    d.hazard_deadline = None;
    d.handle_trip_event(
        &mut app.ctx,
        &cb_chatter(
            "CB chatter in 4 miles: drivers report a trooper ahead.",
            14.0,
            3.0,
        ),
    );
    d.update_ambient_events(&mut app.ctx, AMBIENT_EVENT_SPACING_S);
    assert_eq!(
        app.event_calls().last().cloned(),
        Some((
            "CB chatter in 4 miles: drivers report a trooper ahead.".to_string(),
            false
        ))
    );
}

#[test]
fn test_the_cb_call_a_hazard_swallowed_still_comes_back_on_alt_c() {
    // Sarah A., issue 156. The CB names what is sitting up the road and
    // says it once. Here a hazard owns the road when the call arrives, the
    // queue ages it out before things go quiet, and the driver hears the
    // squelch and not one word of the call. Alt C is the whole way back to
    // it -- which is why the call is recorded when it is HANDLED rather
    // than when the voice finally gets to it.
    let mut app = TestApp::new();
    let clock = app.fake_pacer_clock();
    let mut d = a_drive(&mut app);
    d.trip.position_mi = 0.0;
    d.hazard_deadline = Some(6.0);
    app.clear_speech();

    let post = always_observing_post(6.0, 2.0); // watched from mile 4
    d.handle_trip_event(
        &mut app.ctx,
        &cb_chatter(
            "CB chatter, in 4 miles: a driver reports a trooper.",
            6.0,
            2.0,
        ),
    );
    advance_ambient(&mut app, &mut d, &clock, AMBIENT_QUEUE_MAX_AGE_S + 1.0);
    d.hazard_deadline = None;
    advance_ambient(&mut app, &mut d, &clock, AMBIENT_EVENT_SPACING_S);
    assert!(
        app.event_lines().is_empty(),
        "the call was never spoken: {:?}",
        app.event_lines()
    );

    // Two miles on, the driver asks what the CB said.
    d.trip.position_mi = 2.0;
    app.clear_speech();
    d.handle_key_event(&mut app.ctx, &InputEvent::key_mods(Key::C, Mods::ALT));
    let said = app.main_lines();
    assert_eq!(said, vec![d.trip.cb_patrol_message(&post, 2.0)]);
}

#[test]
fn test_stop_notice_yields_to_recent_route_speech() {
    // A travel-plaza notice right after a spoken navigation line queues
    // behind the spacing window instead of stacking on the instruction.
    let mut app = TestApp::new();
    let clock = app.fake_pacer_clock();
    let mut d = a_drive(&mut app);
    app.clear_speech();

    let merge = "Merge onto I-90 East toward South Bend, then 66 miles on it.";
    let plaza = "travel center: Petro Stopping Centers in 1 mile. Press X to signal for the exit.";
    d.handle_trip_event(
        &mut app.ctx,
        &event(TripEventKind::GpsCue, merge, TripEventData::default()),
    );
    d.handle_trip_event(
        &mut app.ctx,
        &event(TripEventKind::StopAhead, plaza, TripEventData::default()),
    );
    // The plaza notice waits.
    assert_eq!(
        app.event_lines(),
        vec![merge.to_string()],
        "{:?}",
        app.event_lines()
    );

    let spacing = tuning_for_time_scale(d.trip.time_scale).ambient_spacing_s;
    advance_ambient(&mut app, &mut d, &clock, spacing);
    // And speaks once the window clears.
    assert_eq!(
        app.event_calls().last().cloned(),
        Some((plaza.to_string(), false))
    );
}

#[test]
fn test_ambient_chatter_waits_while_hazard_is_active() {
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    app.clear_speech();

    d.hazard_deadline = Some(5.0);
    d.handle_trip_event(
        &mut app.ctx,
        &event(
            TripEventKind::WeatherChange,
            "Weather: rain.",
            TripEventData::default(),
        ),
    );
    assert_eq!(app.event_calls(), Vec::new());

    d.update_ambient_events(&mut app.ctx, AMBIENT_EVENT_SPACING_S);
    assert_eq!(app.event_calls(), Vec::new());

    d.hazard_deadline = None;
    d.update_ambient_events(&mut app.ctx, 0.0);
    assert_eq!(
        app.event_calls(),
        vec![("Weather: rain.".to_string(), false)]
    );
}

#[test]
fn test_zone_entry_no_longer_destroys_pending_ambient_chatter() {
    // A zone entry used to interrupt, and the interrupt threw away whatever
    // chatter was waiting in the ambient slot. Queued at ROUTE it only pushes
    // the chatter back: the zone speaks first, the crossing still speaks.
    let mut app = TestApp::new();
    let clock = app.fake_pacer_clock();
    let mut d = a_drive(&mut app);
    app.clear_speech();

    d.handle_trip_event(
        &mut app.ctx,
        &event(
            TripEventKind::WeatherChange,
            "Weather: rain.",
            TripEventData::default(),
        ),
    );
    d.handle_trip_event(
        &mut app.ctx,
        &event(
            TripEventKind::StateCrossing,
            "Crossing Ohio.",
            TripEventData::default(),
        ),
    );
    assert_eq!(
        d.pending_ambient_events.back().unwrap().priority,
        Some(EventPriority::Important)
    );
    assert_eq!(
        app.event_calls(),
        vec![("Weather: rain.".to_string(), false)]
    );

    let zone = Zone::new(5.0, 8.0, 45.0, "construction");
    d.handle_trip_event(
        &mut app.ctx,
        &zoned(
            TripEventKind::ZoneEnter,
            "Construction ahead. Speed limit 45.",
            &zone,
        ),
    );
    // Python asserted `interrupt is False` here, reading the keyword its
    // `say_event` stub captured. The call site still queues (the ROUTE
    // classification above is what it passes), but the line reaches the VOICE
    // interrupting: the weather line spoken a fraction of a second earlier is
    // still projected to be talking, and a ROUTE line will only wait 0.8 s
    // behind a backlog before flushing it and speaking fresh. That flush is
    // the pacer working, and it is invisible from where Python recorded.
    // What the case is actually about survives it -- the crossing waiting in
    // the ambient queue is untouched, and still speaks.
    assert_eq!(
        app.event_calls().last().cloned(),
        Some(("Construction ahead. Speed limit 45.".to_string(), true))
    );

    let spacing = tuning_for_time_scale(d.trip.time_scale).ambient_spacing_s;
    advance_ambient(&mut app, &mut d, &clock, spacing);
    assert_eq!(
        app.event_calls().last().cloned(),
        Some(("Crossing Ohio.".to_string(), false))
    );
}

// -- the trip's own ordering and lead time --------------------------------------------

#[test]
fn test_departure_merge_cue_is_emitted_before_the_stop_notice() {
    // Regression: at departure the travel-plaza notice used to be emitted
    // ahead of the onramp merge cue, so the one actionable instruction was
    // the last thing queued on the event voice.
    let world = get_world();
    let route = world
        .route_options("Chicago", "Indianapolis", 3, false)
        .unwrap()
        .into_iter()
        .next()
        .expect("a Chicago to Indianapolis route");
    let mut truck = TruckState::default();
    truck.transmission.automatic = true;
    truck.start_engine();
    let mut trip = Trip::new(
        route,
        truck,
        WeatherSystem::new("great_lakes", Some(1), None, None, true),
        TripOptions {
            world: Some(world),
            ..TripOptions::seeded(2)
        },
    );
    trip.traffic_manager.rolling_bubble = false;
    trip.stops = vec![RoadStop::new("Test Travel Plaza", 1.0, "travel_plaza")];
    trip.announced_stops.clear();

    let events = trip.update(1.0 / 60.0);
    let kinds: Vec<&str> = events
        .iter()
        .filter(|e| matches!(e.kind, TripEventKind::GpsCue | TripEventKind::StopAhead))
        .map(|e| {
            let onramp = e
                .data
                .cue
                .as_ref()
                .map(|cue| cue.kind == "onramp")
                .unwrap_or(false);
            if onramp {
                "merge"
            } else {
                e.kind.as_str()
            }
        })
        .collect();
    assert!(kinds.contains(&"merge"), "{kinds:?}");
    assert!(kinds.contains(&"stop_ahead"), "{kinds:?}");
    assert!(
        kinds.iter().position(|k| *k == "merge") < kinds.iter().position(|k| *k == "stop_ahead"),
        "{kinds:?}"
    );
}

#[test]
fn test_zone_warning_lead_scales_with_speed_and_pacing() {
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    d.trip.time_scale = 20.0;
    // Teleporting to highway speed at mile zero lands on the city's baked
    // curves, and a sharp bend now pins the clock to real time (its own
    // feature, its own test). The subject here is zone-lead scaling.
    d.trip.curves = Vec::new();

    d.truck_mut().velocity_mps = 0.0; // crawling -> the minimum base lead
    let crawl = d.trip.zone_warning_lookahead_mi();
    assert!((crawl - ZONE_WARNING_LOOKAHEAD_MI).abs() < 1e-9);

    d.truck_mut().velocity_mps = 70.0 / 2.23694; // highway speed -> more warning
    let fast = d.trip.zone_warning_lookahead_mi();
    let expected = ZONE_WARNING_REAL_S * 70.0 * d.trip.time_scale / 3600.0;
    assert!((fast - expected).abs() < 0.05, "{fast} vs {expected}");
    assert!(fast <= ZONE_WARNING_MAX_MI);

    d.trip.time_scale = 40.0; // faster pacing compresses time -> even more lead
    let faster = d.trip.zone_warning_lookahead_mi();
    assert!(faster >= fast);
    assert!((faster - ZONE_WARNING_MAX_MI).abs() < 1e-9);
}

#[test]
fn test_the_construction_warning_does_not_shout_brake_now_from_eight_miles() {
    // Shane, 2026-08-24: "should really we be slowing down as early as we are
    // for construction routes?"
    //
    // The assists do not, in fact, slow early -- his own log has the ease
    // landing within seconds of the zone. He was slowing because the advance
    // warning told him to: "Brake now! In 8 miles, construction ahead."
    //
    // "Brake now" is the emergency hazard opening, the words used for debris
    // in your lane. Saying it about something eight miles off is untrue, gets
    // acted on, and spends a phrase that has to mean something when a real
    // emergency borrows it. The heavy-traffic and generic zone warnings have
    // always opened with the distance; construction was the odd one out.
    //
    // Python builds a duck-typed stand-in for the three helpers the builder
    // reads. Rust has a real `Trip` to hand, so the wording comes off the
    // real one.
    let mut app = TestApp::new();
    let d = a_drive(&mut app);
    let zone = Zone::new(120.0, 124.0, 45.0, "construction");
    let message = d.trip.zone_warning_message(&zone, 8.0);

    assert!(!message.contains("Brake now"), "{message}");
    // The information all survives: what, how far, and both limits.
    assert!(message.starts_with("Construction, 8 miles."), "{message}");
    assert!(
        message.contains("45") && message.contains("55"),
        "{message}"
    );
}
