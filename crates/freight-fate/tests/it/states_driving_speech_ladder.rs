//! The S4 driving speech ladder where it meets a real drive: which category
//! each trip event is, which call sites hand the gate the right one, and
//! whether a whole drive actually says fewer things as the rung tightens.
//!
//! Port of the `DrivingState` half of `tests/test_driving_speech_ladder.py`.
//! The table itself and the `GameContext` gate are already covered --
//! `ff_core::speech_pacing` and `app_driving_speech_ladder.rs` -- and nothing
//! is duplicated here.
//!
//! # Two things that bite in this file specifically
//!
//! The capture sits BELOW the ladder gate and below the event pacer, where
//! Python's stub replaced `ctx.say_event` and sat above both. So "spoken" here
//! means what a player would actually hear at that rung: a STATUS line at
//! `urgent_only` is absent from both `event_lines()` and message review.
//!
//! And the pacer measures in REAL seconds. A case that calls the same standing
//! condition twice in the same instant is answered by the plain
//! "said this a moment ago" window rather than by the rung or the condition
//! key -- which is the vacuous-test trap the Python file's own note warns
//! about. Those cases run on [`TestApp::fake_pacer_clock`] and advance it past
//! the repeat window between calls, exactly as Python's `_FakeClock` did.

use std::rc::Rc;

use ff_core::models::cargo_condition::CARGO_EXCEPTION_PCT;
use ff_core::models::jobs::{Job, CARGO_CATALOG};
use ff_core::models::profile::Profile;
use ff_core::sim::trip::{Trip, TripOptions};
use ff_core::sim::trip_models::{TrafficPressure, TripEvent, TripEventData, TripEventKind};
use ff_core::sim::weather::{WeatherKind, WeatherSystem};
use ff_core::speech_pacing::{disposition_for, Disposition, SpeechCategory};
use ff_core::speech_text::{stop_callout, SpokenMessage, StopCalloutParts};

use ff_core::data::world_models::{Leg, Route};
use freight_fate::app::testing::TestApp;
use freight_fate::app::SayEvent;
use freight_fate::playtest::breaker::{self, tweak_rigs, Verdict};
use freight_fate::playtest::harness::{PlaytestHarness, StartDelivery};
use freight_fate::states::base::{InputEvent, Key, Mods, State};
use freight_fate::states::driving::DrivingState;
use freight_fate::states::driving_core::*;
use freight_fate::states::driving_events::{event_category_for_kind, FLAVOR_EVENT_KINDS};

const DT: f64 = 1.0 / 60.0;

// -- rigging -------------------------------------------------------------------------

fn mph_to_mps(mph: f64) -> f64 {
    mph / 2.23694
}

/// `_app()`: a headless app with the dedicated event voice on, so a line that
/// survives the rung lands on the event channel where these tests read it.
fn an_app() -> TestApp {
    let mut app = TestApp::new();
    app.ctx.settings.sapi_events = true;
    app
}

/// `_urgent_only_app()`.
fn an_urgent_only_app() -> TestApp {
    let mut app = an_app();
    app.ctx.settings.driving_speech = "urgent_only".to_string();
    app
}

/// `_real_driving(app)`: Denver to Salt Lake City, seeded, on an empty road.
///
/// `tutorial_done = true` for the reason Python gives: the first-run exemption
/// reads that flag and a fresh profile defaults it false, which would hand
/// every one of these tests a rung that silences nothing. The trip is seeded
/// and the sky pinned because an unseeded delivery draws fresh weather, and an
/// ice day moves the advisory speeds some of these lines carry.
fn a_drive(app: &mut TestApp) -> DrivingState {
    let world = app.ctx.world;
    let mut profile = Profile::named_in("Ladder Fix Round", "Denver");
    profile.tutorial_done = true;
    app.ctx.profile = Some(profile);
    let route = world
        .route_from_cities(&["Denver", "Salt Lake City"])
        .expect("Denver to Salt Lake City is a route");
    let job = Job::new(
        CARGO_CATALOG
            .get("general")
            .expect("the general cargo type"),
        12.0,
        "Denver",
        "yard",
        "Salt Lake City",
        200.0,
        900.0,
        12.0,
    );
    let mut drive = DrivingState::new(
        &mut app.ctx,
        job,
        route,
        Some(1),
        DRIVE_PHASE_DELIVERY,
        Some(10.0),
    );
    // `quiet_trip(driving)`.
    drive.trip.set_npc_vehicles(Vec::new());
    drive.trip.traffic_manager.rolling_bubble = false;
    drive.trip.traffic_pressures.clear();
    drive.trip.hazard_check_mi = 1e9;
    drive.trip.inspection_check_mi = 1e9;
    drive.trip.set_patrols(Vec::new());
    drive.weather_mut().current = WeatherKind::Clear;
    drive.departure_checked = true;
    drive.tutorial = None;
    drive
}

/// Every line the drive SUBMITTED since `from`, read off the review log.
///
/// One entry per submission and no requeues -- an interrupting or
/// backlog-flushing line hands the line it cut back to finish behind it, which
/// is real delivery but not a second call site.
fn logged_since(app: &TestApp, from: usize) -> Vec<String> {
    app.ctx.message_log.messages[from..]
        .iter()
        .map(|message| message.text.clone())
        .collect()
}

/// A trip event of one kind, with the data the classifier reads.
fn an_event(kind: TripEventKind, data: TripEventData) -> TripEvent {
    TripEvent {
        kind,
        message: SpokenMessage::new("test"),
        data,
    }
}

// -- classification ------------------------------------------------------------------

#[test]
fn test_the_hazard_call_is_safety() {
    assert_eq!(
        event_category_for_kind(TripEventKind::Hazard),
        Some(SpeechCategory::Safety)
    );
}

#[test]
fn test_a_planned_stop_is_navigation() {
    // The heads-up is an advisory; the arrival is not. "Road Ranger, exit 292,
    // one mile" is worth a tone at urgent only -- a player who has turned the
    // road down that far knows how to pull in (owner, 2026-08-17). Pulling in
    // itself still speaks.
    assert_eq!(
        event_category_for_kind(TripEventKind::StopAhead),
        Some(SpeechCategory::NavigationAdvisory)
    );
    assert_eq!(
        event_category_for_kind(TripEventKind::StopReached),
        Some(SpeechCategory::Navigation)
    );

    // The terse half is what a cut rung would actually be silencing, and it
    // carries no key instruction -- so nothing about pulling in is lost. The
    // mistake this pins came from reading a SpokenMessage's normal half while
    // reasoning about its terse one, which is easy to do again.
    let line = stop_callout(&StopCalloutParts {
        typed_name: "travel center: Road Ranger",
        plain_name: "Road Ranger",
        exit_label: "exit 292",
        distance: "one mile",
        parking_certainty: "confirmed",
        ..Default::default()
    });
    assert!(line.normal.contains("Press X"), "{}", line.normal);
    let terse = line.terse.as_deref().expect("the callout has a terse half");
    assert!(!terse.contains("Press"), "{terse}");
}

#[test]
fn test_the_lead_announcement_yields_before_the_turn_itself() {
    // "In a mile, take exit 42" is a heads-up; "take exit 42" is not. This is
    // the split that makes quiet and urgent_only different settings.
    let lead = an_event(
        TripEventKind::GpsCue,
        TripEventData {
            advance: Some(true),
            ..Default::default()
        },
    );
    let turn = an_event(TripEventKind::GpsCue, TripEventData::default());
    assert_eq!(
        DrivingState::event_category(&lead),
        Some(SpeechCategory::NavigationAdvisory)
    );
    assert_eq!(
        DrivingState::event_category(&turn),
        Some(SpeechCategory::Navigation)
    );

    // And the rungs actually deliver them differently.
    assert_eq!(
        disposition_for("quiet", Some(SpeechCategory::NavigationAdvisory)),
        Disposition::Terse
    );
    assert_eq!(
        disposition_for("urgent_only", Some(SpeechCategory::NavigationAdvisory)),
        Disposition::Silent
    );
    for rung in ["quiet", "urgent_only"] {
        assert_eq!(
            disposition_for(rung, Some(SpeechCategory::Navigation)),
            Disposition::Terse
        );
    }
}

#[test]
fn test_weather_colour_is_status_not_navigation() {
    // This is what makes "act-now cues only" real at urgent_only: the stop you
    // must act on is NAVIGATION and speaks; the weather turning is STATUS and
    // does not.
    assert_eq!(
        event_category_for_kind(TripEventKind::WeatherChange),
        Some(SpeechCategory::Status)
    );
}

#[test]
fn test_billboards_and_landmarks_bypass_the_ladder_entirely() {
    // The owner's directive, at the classification layer: flavor is not a
    // ladder category. Mapping BILLBOARD to STATUS would silence billboards at
    // urgent_only, which is precisely what must not happen. A flavor kind
    // classifies as None, so the gate passes it through and its own chatter
    // switch decides.
    for kind in [TripEventKind::Billboard, TripEventKind::Landmark] {
        assert_eq!(event_category_for_kind(kind), None, "{kind:?}");
        assert!(FLAVOR_EVENT_KINDS.contains(&kind), "{kind:?}");
    }
}

/// Every `TripEventKind`, with a guard that keeps the list honest.
///
/// Python iterated the enum. Rust has no such reflection, so the list is
/// written out -- and [`kind_is_listed`] below is an exhaustive `match`, which
/// stops compiling the moment a variant is added. That is what makes the
/// "nobody classified this kind" case below real rather than a snapshot of
/// what somebody typed once.
const ALL_TRIP_EVENT_KINDS: [TripEventKind; 18] = [
    TripEventKind::ZoneEnter,
    TripEventKind::ZoneExit,
    TripEventKind::StopAhead,
    TripEventKind::StopReached,
    TripEventKind::CityReached,
    TripEventKind::Hazard,
    TripEventKind::WeatherChange,
    TripEventKind::Inspection,
    TripEventKind::GpsCue,
    TripEventKind::StateCrossing,
    TripEventKind::TimezoneCrossing,
    TripEventKind::Checkpoint,
    TripEventKind::TollCharged,
    TripEventKind::Landmark,
    TripEventKind::Billboard,
    TripEventKind::Curve,
    TripEventKind::Lane,
    TripEventKind::Arrived,
];

fn kind_is_listed(kind: TripEventKind) -> bool {
    // The arms do nothing; being EXHAUSTIVE is the whole point. Add a variant
    // to `TripEventKind` and this stops compiling, which is what forces the
    // list above to keep up with the enum Python simply iterated.
    match kind {
        TripEventKind::ZoneEnter
        | TripEventKind::ZoneExit
        | TripEventKind::StopAhead
        | TripEventKind::StopReached
        | TripEventKind::CityReached
        | TripEventKind::Hazard
        | TripEventKind::WeatherChange
        | TripEventKind::Inspection
        | TripEventKind::GpsCue
        | TripEventKind::StateCrossing
        | TripEventKind::TimezoneCrossing
        | TripEventKind::Checkpoint
        | TripEventKind::TollCharged
        | TripEventKind::Landmark
        | TripEventKind::Billboard
        | TripEventKind::Curve
        | TripEventKind::Lane
        | TripEventKind::Arrived => {}
    }
    ALL_TRIP_EVENT_KINDS.contains(&kind)
}

#[test]
fn test_every_trip_event_kind_is_classified() {
    // Every kind is either governed by the ladder or explicitly left to the
    // flavor switches. Neither list may quietly gain a member by omission: a
    // new event kind must make someone decide which it is.
    for kind in ALL_TRIP_EVENT_KINDS {
        assert!(kind_is_listed(kind), "{kind:?}");
    }
    let undecided: Vec<TripEventKind> = ALL_TRIP_EVENT_KINDS
        .into_iter()
        .filter(|kind| {
            event_category_for_kind(*kind).is_none() && !FLAVOR_EVENT_KINDS.contains(kind)
        })
        .collect();
    assert!(
        undecided.is_empty(),
        "trip event kinds nobody classified: {undecided:?}"
    );

    let both: Vec<TripEventKind> = ALL_TRIP_EVENT_KINDS
        .into_iter()
        .filter(|kind| {
            event_category_for_kind(*kind).is_some() && FLAVOR_EVENT_KINDS.contains(kind)
        })
        .collect();
    assert!(
        both.is_empty(),
        "trip event kinds claimed by both lists: {both:?}"
    );
}

// -- the gate, with a literal line ---------------------------------------------------

#[test]
fn test_the_load_damage_coaching_tail_is_silent_at_urgent_only() {
    let mut app = an_urgent_only_app();

    app.ctx.say_event_with(
        "Brake and corner gently from here.",
        SayEvent::queued().category(SpeechCategory::Coaching),
    );

    assert!(app.event_lines().is_empty());
    app.shutdown();
}

#[test]
fn test_the_same_tail_speaks_on_the_coaching_rung() {
    // "coaching" is no longer a rung (2026-08-17) and falls back to standard,
    // where COACHING is FIRST_OCCURRENCE -- so the tail still speaks once,
    // which is what this case has always been about.
    let mut app = an_app();
    app.ctx.settings.driving_speech = "coaching".to_string();

    app.ctx.say_event_with(
        "Brake and corner gently from here.",
        SayEvent::queued().category(SpeechCategory::Coaching),
    );

    assert_eq!(
        app.event_lines(),
        vec!["Brake and corner gently from here.".to_string()]
    );
    app.shutdown();
}

// -- the real announce paths ---------------------------------------------------------

#[test]
fn test_weather_change_is_silent_at_urgent_only_through_the_real_path() {
    // Classification alone cannot catch a call site that never threads the
    // category through: WEATHER_CHANGE and LANE only ever reach the voice by
    // way of the ambient speaker, which used to drop the category on the floor
    // no matter what the classifier said. This drives a real WEATHER_CHANGE
    // event through the actual speaking path.
    let mut app = an_urgent_only_app();
    let mut drive = a_drive(&mut app);
    app.clear_speech();

    let event = TripEvent {
        kind: TripEventKind::WeatherChange,
        message: SpokenMessage::new("Weather turning: heavy rain."),
        data: TripEventData::default(),
    };
    drive.handle_trip_event(&mut app.ctx, &event);
    drive.update_ambient_events(&mut app.ctx, DT);

    assert!(app.event_lines().is_empty(), "{:?}", app.event_lines());
    app.shutdown();
}

#[test]
fn test_the_out_of_service_wall_speaks_at_urgent_only() {
    // The wall governs the truck to a creep and orders a stop on the shoulder
    // right now -- SAFETY, not the STATUS every other damage band uses.
    let mut app = an_urgent_only_app();
    let mut drive = a_drive(&mut app);
    drive.trip.truck.engine_on = true;
    drive.trip.truck.velocity_mps = mph_to_mps(55.0);
    drive.trip.truck.damage_pct = ff_core::sim::vehicle::DAMAGE_OUT_OF_SERVICE_PCT + 1.0;
    app.clear_speech();

    drive.update_damage_bands(&mut app.ctx, DT);

    let lines = app.event_lines();
    let last = lines
        .last()
        .unwrap_or_else(|| panic!("the wall stayed silent"));
    assert!(last.to_lowercase().contains("out of service"), "{last}");
    app.shutdown();
}

#[test]
fn test_a_reduced_power_band_still_stays_quiet_at_urgent_only() {
    // Specificity check for the case above: the SAFETY override is a local one
    // on the wall's own branch, not a blanket over every damage band.
    let mut app = an_urgent_only_app();
    let mut drive = a_drive(&mut app);
    drive.trip.truck.engine_on = true;
    drive.trip.truck.damage_pct = ff_core::sim::vehicle::DAMAGE_DERATE_PCT + 1.0;
    app.clear_speech();

    drive.update_damage_bands(&mut app.ctx, DT);

    assert!(app.event_lines().is_empty(), "{:?}", app.event_lines());
    app.shutdown();
}

#[test]
fn test_drifting_off_the_pavement_speaks_at_urgent_only() {
    // The off-pavement announcer only ever fires on entry or worsening, so
    // every line it emits is the warning, never the standing position --
    // SAFETY throughout.
    let mut app = an_urgent_only_app();
    let mut drive = a_drive(&mut app);
    drive.lane.lane = 0;
    drive.trip.truck.velocity_mps = 13.0;
    app.clear_speech();

    drive.lane.offset = 1.35;
    drive.announce_off_pavement(&mut app.ctx);

    assert!(!app.event_lines().is_empty());
    app.shutdown();
}

#[test]
fn test_back_on_the_pavement_still_stays_quiet_at_urgent_only() {
    // Specificity check: the standing-condition recovery line is correctly
    // STATUS and must stay silenced at the quietest rung; only the warning
    // transition was miscategorised. Drives the lane update directly (not the
    // whole frame) so a fresh drive's other first-frame NAVIGATION chatter --
    // always audible -- cannot mask the assertion.
    let mut app = an_urgent_only_app();
    let mut drive = a_drive(&mut app);
    drive.lane.lane = 0;
    drive.lane.offset = 0.0;
    drive.road_position_band = Some(1); // was off, band tracked from before
    app.clear_speech();

    drive.update_lane(&mut app.ctx, DT);

    assert!(app.event_lines().is_empty(), "{:?}", app.event_lines());
    app.shutdown();
}

#[test]
fn test_spring_brakes_setting_speaks_at_urgent_only() {
    // This is the low-air EMERGENCY the taxonomy splits from the low-air band
    // -- already an interrupt with the buzzer, which is the code's own verdict
    // on its urgency. SAFETY, not STATUS.
    let mut app = an_urgent_only_app();
    let mut drive = a_drive(&mut app);
    drive.trip.truck.set_air_pressure_psi(35.0); // below the spring-brake set threshold
    assert!(
        drive.trip.truck.spring_brakes_active(),
        "35 psi no longer sets the spring brakes"
    );
    app.clear_speech();

    drive.update_air_brake_announcements(&mut app.ctx, true, true, false, false);

    let lines = app.event_lines();
    let last = lines
        .last()
        .unwrap_or_else(|| panic!("the spring brakes set in silence"));
    assert!(last.to_lowercase().contains("spring brakes"), "{last}");
    app.shutdown();
}

#[test]
fn test_the_rolling_low_air_warning_speaks_at_urgent_only() {
    // The last warning before the spring brakes set on their own. Same
    // urgency-decides-the-category shape as the HOS check -- SAFETY rolling.
    let mut app = an_urgent_only_app();
    let mut drive = a_drive(&mut app);
    let t = &mut drive.trip.truck;
    t.engine_on = true;
    t.velocity_mps = 10.0; // rolling
    t.set_air_pressure_psi(55.0); // low-air band, above the spring threshold
    assert!(
        t.air_low_warning() && !t.spring_brakes_active(),
        "55 psi is no longer the band"
    );
    // A cold-started truck constructs with the low-air line already said (it
    // starts low), so a fresh degradation must re-arm it -- exactly the
    // hysteresis the real update loop re-arms on recovery.
    drive.low_air_said = false;
    app.clear_speech();

    drive.update_air_brake_announcements(&mut app.ctx, true, true, false, true);

    let lines = app.event_lines();
    let last = lines
        .last()
        .unwrap_or_else(|| panic!("the rolling low-air warning stayed silent"));
    assert!(last.to_lowercase().contains("low air"), "{last}");
    app.shutdown();
}

#[test]
fn test_the_parked_low_air_warning_stays_quiet_at_urgent_only() {
    // Legitimately STATUS: "leave the parking brake alone" is a band readout,
    // not an act-now cue.
    let mut app = an_urgent_only_app();
    let mut drive = a_drive(&mut app);
    let t = &mut drive.trip.truck;
    t.engine_on = true;
    t.velocity_mps = 0.0; // parked
    t.set_air_pressure_psi(55.0);
    drive.low_air_said = false;
    app.clear_speech();

    drive.update_air_brake_announcements(&mut app.ctx, true, true, false, true);

    assert!(app.event_lines().is_empty(), "{:?}", app.event_lines());
    app.shutdown();
}

// -- the air-brake lockout's standing reason -----------------------------------------
//
// A tester's transcript showed "Parking brake set. Press P to release it."
// spoken twice back to back with nothing about the truck changed in between:
// the player kept holding the accelerator against a parked, air-ready truck,
// and the lockout's 4-second retrigger re-announced the same fact every time
// it fired. These drive the real method so a key removed from the actual call
// site fails a test.
//
// All three advance a fake clock past the pacer's repeat window between calls.
// Without that, an identical-text repeat would already be caught by the plain
// "said this recently" window regardless of the condition key, and the test
// would pass whether or not the key survived a revert.

#[test]
fn test_the_air_brake_lockout_speaks_once_while_the_reason_is_unchanged() {
    let mut app = an_app();
    let clock = app.fake_pacer_clock();
    let mut drive = a_drive(&mut app);
    drive.trip.truck.engine_on = true;
    drive.trip.truck.set_air_ready(true); // air ready, brake still set
    app.clear_speech();

    for _ in 0..4 {
        // the player holding the accelerator against the lockout
        drive.brake_lockout_cue_timer = 0.0;
        clock.advance(10.0); // well past the plain repeat window
        drive.maybe_say_air_brake_lockout(&mut app.ctx);
    }

    assert_eq!(app.event_lines().len(), 1, "{:?}", app.event_lines());
    app.shutdown();
}

#[test]
fn test_the_air_brake_lockout_speaks_again_when_the_reason_changes() {
    let mut app = an_app();
    let clock = app.fake_pacer_clock();
    let mut drive = a_drive(&mut app);
    drive.trip.truck.engine_on = false; // fresh trip: cold air start, engine off
    app.clear_speech();

    drive.maybe_say_air_brake_lockout(&mut app.ctx);
    drive.brake_lockout_cue_timer = 0.0;
    clock.advance(10.0);
    drive.trip.truck.engine_on = true; // engine started, air still not built

    drive.maybe_say_air_brake_lockout(&mut app.ctx);

    let lines = app.event_lines();
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert_ne!(lines[0], lines[1]);
    app.shutdown();
}

#[test]
fn test_the_air_brake_lockout_recurs_once_it_clears_and_comes_back() {
    // The key used to be set and never released, so the pacer's single
    // app-session condition table kept the first "Parking brake set..." on
    // file forever. A later, unrelated recurrence of the identical text --
    // the lockout clears, then the player parks somewhere else and hits the
    // accelerator before releasing the brake -- would go silent under the
    // stale key. The clear is seen by a real per-frame pass, which is where
    // the reset lives.
    let mut app = an_app();
    let clock = app.fake_pacer_clock();
    let mut drive = a_drive(&mut app);
    drive.trip.truck.engine_on = true;
    drive.trip.truck.set_air_ready(true); // locked out, air ready
    app.clear_speech();

    drive.maybe_say_air_brake_lockout(&mut app.ctx); // first instance: speaks

    drive.trip.truck.parking_brake = false; // the lockout genuinely clears
    clock.advance(10.0);
    drive.update(&mut app.ctx, DT); // a real per-frame pass sees the clear

    drive.trip.truck.set_air_ready(true); // locked out again, later
    drive.brake_lockout_cue_timer = 0.0;
    clock.advance(10.0);
    drive.maybe_say_air_brake_lockout(&mut app.ctx); // a fresh instance: must speak too

    let parking: Vec<String> = app
        .event_lines()
        .into_iter()
        .filter(|line| line.contains("Parking brake set"))
        .collect();
    assert_eq!(parking.len(), 2, "{parking:?}");
    assert_eq!(parking[0], parking[1]); // identical text, both spoken
    app.shutdown();
}

// -- money and safety on the real lines ----------------------------------------------

#[test]
fn test_routine_cargo_condition_is_silent_at_urgent_only() {
    // The coaching tail only rides the first report; every message this sends
    // -- including that first one -- carries the pay consequence (an
    // exception, a claim, a refused load). MONEY, not COACHING, governs the
    // whole line.
    let mut app = an_urgent_only_app();
    let mut drive = a_drive(&mut app);
    drive.trip.truck.cargo_damage_pct = CARGO_EXCEPTION_PCT + 1.0;
    app.clear_speech();

    drive.announce_cargo_condition(&mut app.ctx);

    assert!(app.event_lines().is_empty());
    app.shutdown();
}

#[test]
fn test_the_carrier_grounding_speaks_at_urgent_only_as_safety() {
    // A replacement tractor and its restart instruction are essential recovery
    // information, even though the message also names financial consequences.
    let mut app = an_urgent_only_app();
    let mut drive = a_drive(&mut app);
    app.clear_speech();

    drive.carrier_grounds_the_tractor(&mut app.ctx);

    let lines = app.event_lines();
    let last = lines
        .last()
        .unwrap_or_else(|| panic!("the grounding was answered with a chime"));
    assert!(last.to_lowercase().contains("grounded"), "{last}");
    assert!(last.to_lowercase().contains("carrier"), "{last}");
    app.shutdown();
}

#[test]
fn test_an_engine_stall_speaks_at_urgent_only_as_safety() {
    // A stall is an unrequested failure that stops the truck and names the key
    // to get it moving again -- the same "will not move, here is what to
    // press" shape as the out-of-service wall and the spring-brake emergency,
    // both already SAFETY. It was tagged CONFIRMATION, so a quiet or
    // urgent_only driver got a chime and no recovery instruction with a dead
    // engine.
    //
    // Python monkeypatched `TruckState.update` to force the stall. Here the
    // real condition is arranged instead: a manual box launched in sixth with
    // the clutch out lugs the engine to a stop, which is the stall the game
    // models. It takes more than one frame, so the drive runs frames until the
    // engine dies -- the announcement fires on whichever frame that is.
    let mut app = an_urgent_only_app();
    let mut drive = a_drive(&mut app);
    let t = &mut drive.trip.truck;
    t.start_engine();
    t.set_air_ready(false);
    t.transmission.automatic = false;
    t.transmission.clutch = 1.0;
    assert!(t.transmission.request_gear(6).ok);
    t.transmission.clutch = 0.0;
    t.throttle = 0.2;
    app.ctx.settings.automatic_transmission = false;
    app.clear_speech();

    let mut stalled = false;
    for _ in 0..(60 * 5) {
        drive.update(&mut app.ctx, DT);
        if drive.trip.truck.stalled && !drive.trip.truck.engine_on {
            stalled = true;
            break;
        }
    }
    assert!(stalled, "sixth gear from a standstill no longer stalls");

    let lines = app.event_lines();
    assert!(
        lines
            .iter()
            .any(|line| line.to_lowercase().contains("engine stalled")),
        "{lines:?}"
    );
    app.shutdown();
}

#[test]
fn test_the_manual_stall_line_leads_to_first_gear_not_a_second_stall() {
    // A manual only stalls in fourth or taller, where shifting up goes taller
    // still: "restart, then select W" put a truck stalled in fifth into sixth,
    // and letting the clutch out stalled it again. The keys the line names,
    // pressed in its order, must leave the truck running in first.
    let mut app = an_app();
    let mut drive = a_drive(&mut app);
    app.ctx.settings.automatic_transmission = false;
    let t = &mut drive.trip.truck;
    t.start_engine();
    t.set_air_ready(false);
    t.transmission.automatic = false;
    t.transmission.clutch = 1.0;
    assert!(t.transmission.request_gear(5).ok);
    t.transmission.clutch = 0.0;
    app.clear_speech();
    for _ in 0..(60 * 5) {
        drive.update(&mut app.ctx, DT);
        if drive.trip.truck.stalled {
            break;
        }
    }
    assert!(
        drive.trip.truck.stalled,
        "fifth at a standstill no longer stalls"
    );
    let lines = app.event_lines();
    let line = lines
        .iter()
        .find(|line| line.contains("Engine stalled"))
        .unwrap_or_else(|| panic!("no stall line in {lines:?}"));
    assert!(
        line.contains("Press N for neutral and E to restart")
            && line.contains("hold Left Shift and press W for first gear"),
        "{line}"
    );

    let press = |drive: &mut DrivingState, app: &mut TestApp, key: Key, mods: Mods| {
        let event = InputEvent::KeyDown {
            key,
            mods,
            text: None,
            repeat: false,
        };
        drive.handle_key_event(&mut app.ctx, &event);
    };
    press(&mut drive, &mut app, Key::N, Mods::NONE);
    press(&mut drive, &mut app, Key::E, Mods::NONE);
    app.ctx.input.press(Key::LShift, Mods::NONE);
    drive.update(&mut app.ctx, DT);
    press(&mut drive, &mut app, Key::W, Mods::SHIFT);
    for _ in 0..30 {
        drive.update(&mut app.ctx, DT);
    }
    app.ctx.input.release(Key::LShift, Mods::NONE);
    for _ in 0..(60 * 2) {
        drive.update(&mut app.ctx, DT);
    }
    let t = &drive.trip.truck;
    assert_eq!(t.transmission.gear, 1);
    assert!(t.engine_on && !t.stalled, "stalled again after the restart");
    app.shutdown();
}

#[test]
fn test_a_tire_chain_release_speaks_at_urgent_only_as_safety() {
    // Losing chains changes traction immediately, so this remains a safety call.
    let mut app = an_urgent_only_app();
    let mut drive = a_drive(&mut app);
    drive.trip.truck.chains_just_snapped = true;
    app.clear_speech();

    drive.update_traction_cues(&mut app.ctx);

    let lines = app.event_lines();
    let last = lines
        .last()
        .unwrap_or_else(|| panic!("the chain let go in silence"));
    assert_eq!(
        last,
        "A tire chain broke and is flailing. Slow down and pull over to check it."
    );
    app.shutdown();
}

// -- the mandatory-stop misses -------------------------------------------------------

#[test]
fn test_missed_destination_exit_speaks_at_urgent_only() {
    // The route just changed and this names the manoeuvre that still gets the
    // load delivered -- NAVIGATION, not CONFIRMATION, so it survives
    // urgent_only as words rather than an earcon blip.
    let mut harness = PlaytestHarness::new();
    harness.start_delivery(StartDelivery::named("Missed Exit Rung"));
    harness.app.ctx.settings.driving_speech = "urgent_only".to_string();
    harness.with_drive(|drive, ctx| {
        drive.tutorial = None;
        drive.departure_checked = true;
        drive.trip.hazard_check_mi = 1e9;
        drive.trip.inspection_check_mi = 1e9;
        drive.trip.traffic_manager.rolling_bubble = false;
        drive.trip.set_npc_vehicles(Vec::new());
        drive.trip.traffic_pressures.clear();
        drive.trip.weather.current = WeatherKind::Clear;
        drive.trip.zones.retain(|z| z.aadt.is_none());
        drive.trip.set_patrols(Vec::new());
        drive.trip.posts.clear();
        // Without this the truck is still on the spring brakes.
        drive.truck_mut().set_air_ready(false);
        // A freshly created career defaults `tutorial_done` false, and the
        // first-run exemption then hands this test a rung that silences
        // nothing.
        if let Some(profile) = ctx.profile.as_mut() {
            profile.tutorial_done = true;
        }
        drive.trip.position_mi = drive.trip.total_miles();
        drive.trip.finished = true;
        drive.truck_mut().velocity_mps = mph_to_mps(45.0);
    });
    harness.clear_speech();

    harness.advance_clock(DT);
    harness.with_drive(|drive, ctx| drive.update_frame(ctx, DT));

    // Membership, not the last line: this frame crosses the whole route at
    // once, so anything else the road owed the driver speaks in the same tick
    // and the last slot is nobody's contract.
    let lines = harness.app.event_lines();
    assert!(
        lines
            .iter()
            .any(|line| line.to_lowercase().contains("missed the destination exit")),
        "{lines:?}"
    );
}

#[test]
fn test_missed_facility_gate_speaks_at_urgent_only() {
    // The same mandatory-stop-miss family as the destination exit.
    let mut app = an_urgent_only_app();
    let mut drive = a_drive(&mut app);
    app.clear_speech();

    drive.handle_missed_facility_gate(&mut app.ctx);

    let lines = app.event_lines();
    let last = lines
        .last()
        .unwrap_or_else(|| panic!("the gate miss was answered with a chime"));
    assert!(last.to_lowercase().contains("gate"), "{last}");
    app.shutdown();
}

#[test]
fn test_drove_past_the_destination_terminal_speaks_at_urgent_only() {
    // The ramp-terminal loop-back names the same manoeuvre as the
    // facility-gate and destination-exit misses.
    let mut app = an_urgent_only_app();
    let mut drive = a_drive(&mut app);
    let stop = ff_core::sim::trip_models::RoadStop::new(
        "Salt Lake City Warehouse",
        drive.trip.total_miles(),
        "facility",
    );
    app.clear_speech();

    drive.loop_back_to_destination_terminal(&mut app.ctx, &stop);

    let lines = app.event_lines();
    let last = lines
        .last()
        .unwrap_or_else(|| panic!("the terminal miss was answered with a chime"));
    assert!(last.to_lowercase().contains("drove past"), "{last}");
    app.shutdown();
}

#[test]
fn test_missed_turn_speaks_at_urgent_only() {
    // A blown street turn is the same mandatory-stop-miss family as the
    // highway misses above.
    let mut app = an_urgent_only_app();
    let mut drive = a_drive(&mut app);
    let city = drive.trip.route.cities[0].clone();
    let legs = vec![
        Leg::local(
            &city,
            0.6,
            "East Navarre Street",
            "Start on East Navarre Street.",
            25.0,
        ),
        Leg::local(
            &city,
            0.5,
            "North Michigan Street",
            "Turn left onto North Michigan Street.",
            25.0,
        ),
    ];
    let route = Route::from_legs(vec![city.clone(); 3], legs);
    let truck = drive.trip.truck.clone();
    let weather = WeatherSystem::new("", Some(3), None, None, false);
    let mut trip = Trip::new(
        route,
        truck,
        weather,
        TripOptions {
            seed: Some(3),
            ..Default::default()
        },
    );
    trip.set_npc_vehicles(Vec::new());
    drive.replace_trip(trip);
    drive.reset_turn_state_for_trip();
    app.clear_speech();

    // Roll up to the turn far too fast and let the reaction window lapse.
    drive.trip.position_mi = 0.4;
    drive.trip.truck.engine_on = true;
    drive.trip.truck.velocity_mps = mph_to_mps(45.0);
    drive.update_turn_commitment(&mut app.ctx, 0.016);
    drive.trip.position_mi = 0.6;
    let grace = drive.turn_grace_s + 1.0;
    drive.update_turn_commitment(&mut app.ctx, grace);

    let lines = app.event_lines();
    let last = lines
        .last()
        .unwrap_or_else(|| panic!("the missed turn was answered with a chime"));
    assert!(last.to_lowercase().contains("missed the turn"), "{last}");
    app.shutdown();
}

// -- the queued ambient line ---------------------------------------------------------

#[test]
fn test_a_queued_stop_notice_speaks_the_distance_it_delivers_at() {
    // The ambient queue's age cap is real seconds; a stop notice's distance
    // decays in game miles. Queued at "in 5 miles" behind a hazard, the line
    // used to be performed with two miles left and the building in sight
    // (Brandon, 2026-08-20). A queued line with a render speaks the distance
    // as of delivery, and drops silently once the stop is behind the truck.
    let mut app = an_app();
    let mut drive = a_drive(&mut app);
    app.clear_speech();

    let mut pending = PendingAmbient::new("Pilot Travel Center in 5 miles.");
    pending.category = Some(SpeechCategory::NavigationAdvisory);
    pending.render = Some(Rc::new(|_drive: &DrivingState, _ctx: &_| {
        Some("Pilot Travel Center in 2 miles.".to_string())
    }));
    drive.pending_ambient_events.push_back(pending);
    drive.ambient_event_cooldown_s = 0.0;
    drive.hazard_deadline = None;

    drive.update_ambient_events(&mut app.ctx, 0.0);

    assert_eq!(
        app.event_lines(),
        vec!["Pilot Travel Center in 2 miles.".to_string()]
    );

    app.clear_speech();
    drive.ambient_event_cooldown_s = 0.0;
    let mut lapsed = PendingAmbient::new("Pilot Travel Center in 5 miles.");
    lapsed.category = Some(SpeechCategory::NavigationAdvisory);
    // the stop fell behind while it waited
    lapsed.render = Some(Rc::new(|_drive: &DrivingState, _ctx: &_| None));
    drive.pending_ambient_events.push_back(lapsed);

    drive.update_ambient_events(&mut app.ctx, 0.0);

    assert!(app.event_lines().is_empty(), "{:?}", app.event_lines());
    app.shutdown();
}

// -- the terse halves the rung renders -----------------------------------------------

#[test]
fn test_traffic_advisories_are_short_at_every_rung() {
    // They shipped as long sentences: "Exit traffic building in 2 miles.
    // Signal early, hold the right exit lane, and be ready to slow near 45"
    // was the longest line on the drive. Now one short form serves every
    // rung, so there is no separate terse half to fall out of step.
    let mut app = an_app();
    let drive = a_drive(&mut app);
    for kind in ["exit", "construction_merge", "route_merge", "pack"] {
        let pressure = TrafficPressure {
            start_mi: 0.2,
            end_mi: 0.6,
            kind: kind.to_string(),
            direction: "right".to_string(),
            intensity: 0.5,
            target_speed_mph: 45.0,
            reason: "on-ramp".to_string(),
        };
        let message = drive.trip.traffic_pressure_message(&pressure, 2.0);
        assert!(message.terse.is_none(), "{kind}");
        assert!(
            message.normal.split(". ").count() <= 2,
            "{kind}: {}",
            message.normal
        );
        assert!(message.normal.len() < 50, "{kind}: {}", message.normal);
    }
    app.shutdown();
}

#[test]
fn test_the_stop_bar_countdown_shrinks_on_the_quieter_rungs() {
    // The bar has an instrument as well as a voice, so the voice says less.
    // Inside the tick's range a centre tick speeds up as the bar closes and
    // fuses to a solid tone at the end -- rate carries distance, silence means
    // stopped. Every spoken milestone inside that range was speech restating
    // what the driver was already listening to. Standard keeps the calls the
    // tick cannot make; quiet keeps one (owner, 2026-08-21).
    let mut app = an_app();
    let drive = a_drive(&mut app);
    app.ctx.settings.imperial_units = true;

    app.ctx.settings.driving_speech = "standard".to_string();
    let standard = drive.ramp_bar_milestones(&app.ctx);
    app.ctx.settings.driving_speech = "quiet".to_string();
    let quiet = drive.ramp_bar_milestones(&app.ctx);

    assert_eq!(standard.len(), 2, "two calls at standard, down from four");
    assert!(standard.len() < RAMP_GAP_MILESTONES_FT.len());
    assert_eq!(standard, vec![1000, 500]);
    // Nothing spoken inside the tick's own reach on STANDARD: out there the
    // tick cannot help, which is the whole reason those calls survive.
    for threshold in &standard {
        assert!(
            *threshold as f64 / 5280.0 > RAMP_BAR_TICK_RANGE_MI,
            "{threshold}"
        );
    }

    // Quiet is the far call plus the HANDOFF -- the distance where the tick
    // starts, so the words pass the driver to the sound rather than simply
    // stopping (owner, after driving it, 2026-08-21).
    assert_eq!(quiet, vec![1000, 300]);
    assert_eq!(
        quiet[0], standard[0],
        "the far call is the same on both rungs"
    );
    let handoff = *quiet.last().expect("a handoff call") as f64 / 5280.0;
    assert!(
        (handoff - RAMP_BAR_TICK_RANGE_MI).abs() < 1e-9,
        "the quiet handoff call sits exactly where the tick begins: {handoff}"
    );

    // Metric behaves the same way against the same physical distances.
    app.ctx.settings.imperial_units = false;
    app.ctx.settings.driving_speech = "standard".to_string();
    assert_eq!(drive.ramp_bar_milestones(&app.ctx), vec![300, 150]);
    app.ctx.settings.driving_speech = "quiet".to_string();
    // 100 m is the milestone nearest the tick's 91 m reach.
    assert_eq!(drive.ramp_bar_milestones(&app.ctx), vec![300, 100]);
    app.shutdown();
}

#[test]
fn test_the_quiet_stop_bar_call_is_the_distance_and_nothing_else() {
    // Quiet gets one call for the whole approach, so it carries one fact. It
    // used to render as "1000 feet to stop bar, speed limit 25" -- but by the
    // time it lands the driver has already been told this is a bar and what
    // the limit is, so both halves are a re-read (owner, 2026-08-21).
    let mut app = an_app();
    let mut drive = a_drive(&mut app);
    app.ctx.settings.imperial_units = true;
    app.ctx.settings.driving_speech = "quiet".to_string();

    drive.ramp_light_announced = true;
    drive.ramp_waiting_at_light = false;
    drive.ramp_mi = Some(RAMP_ACCESS_MI + (900.0 / 5280.0));
    drive.trip.truck.engine_on = true;
    drive.trip.truck.velocity_mps = mph_to_mps(30.0);
    let from = app.ctx.message_log.messages.len();
    app.clear_speech();

    drive.update_ramp_gap_countdown(&mut app.ctx);

    // Read as SUBMITTED lines, not raw channel traffic. The second call is a
    // ROUTE line that flushes a backlog, and the pacer hands the cut line back
    // to finish behind it -- so "1000 feet." legitimately reaches the voice
    // twice. That is delivery, not a second call site, and the review log
    // records one entry per submission.
    let lines = logged_since(&app, from);
    assert_eq!(
        lines.first().map(String::as_str),
        Some("1000 feet."),
        "{lines:?}"
    );

    // And the handoff call, when the bar is close enough that the tick is
    // about to take over, is just as bare.
    drive.ramp_mi = Some(RAMP_ACCESS_MI + (290.0 / 5280.0));
    drive.update_ramp_gap_countdown(&mut app.ctx);

    let lines = logged_since(&app, from);
    assert_eq!(
        lines.last().map(String::as_str),
        Some("300 feet."),
        "{lines:?}"
    );
    assert_eq!(lines.len(), 2, "one far call, one handoff call: {lines:?}");
    app.shutdown();
}

// -- every driving call site carries a category --------------------------------------

#[test]
fn test_no_driving_say_event_call_site_is_left_untagged() {
    // The gate defaults untagged lines to speaking, which is the right failure
    // mode but the wrong finished state: an untagged line is one the ladder
    // cannot quiet. This pins the sweep as done.
    //
    // Python walked the AST for a `category=` keyword at the call site. Rust
    // has no keyword arguments: the category rides a `SayEvent` builder, which
    // is sometimes written inline at the call and sometimes bound a few lines
    // above it (`let mut opts = SayEvent::queued(); opts.category = ...`). So
    // the check resolves that binding inside the enclosing function rather
    // than only reading the call's own text. A `force`d line is exempt for the
    // same reason it is in Python: the player asked for it and must hear it
    // whatever the rung says.
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/states");
    let mut sources: Vec<std::path::PathBuf> = Vec::new();
    collect_driving_sources(&root, true, &mut sources);
    assert!(
        sources.len() > 60,
        "the driving sources moved: only found {}",
        sources.len()
    );

    let mut untagged: Vec<String> = Vec::new();
    let mut examined = 0usize;
    for path in &sources {
        let source = std::fs::read_to_string(path).expect("a readable source file");
        let name = path
            .strip_prefix(&root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");
        for call in say_event_calls(&source) {
            examined += 1;
            if call_is_tagged(&call) {
                continue;
            }
            untagged.push(format!("{name}:{}", call.line));
        }
    }
    // Non-vacuous: a scanner that stopped matching would otherwise report a
    // clean sweep it never made.
    assert!(
        examined > 100,
        "the scanner only found {examined} say_event call sites in the driving states"
    );
    assert!(
        untagged.is_empty(),
        "untagged say_event call sites: {untagged:?}"
    );
}

/// One `say_event` call site, with what it takes to judge it.
struct SayEventCall {
    line: usize,
    /// The call's own argument list, comments and string literals blanked.
    args: String,
    /// The enclosing function from its `fn` line down to this call, same
    /// masking -- where a `SayEvent` bound above the call is built.
    prefix: String,
}

/// Whether this call hands the gate a category (or is a `force`d line).
fn call_is_tagged(call: &SayEventCall) -> bool {
    if call.args.contains(".category(") || call.args.contains(".force(") {
        return true;
    }
    // The options are the last argument. When it is a plain binding --
    // `opts`, or `opts()` / `opts(reason)` for the closure form -- follow it
    // back to where it was built or assigned inside this function.
    // A multi-line call ends with a trailing comma, so the last split piece is
    // whitespace: take the last one that is not.
    let Some(last) = call
        .args
        .rsplit(',')
        .map(str::trim)
        .find(|piece| !piece.is_empty())
    else {
        return false;
    };
    let name = last.split('(').next().unwrap_or(last).trim();
    if name.is_empty() || !name.chars().all(|c| c.is_alphanumeric() || c == '_') {
        return false;
    }
    if call.prefix.contains(&format!("{name}.category")) {
        return true;
    }
    // `let [mut] NAME = <initializer>;`, read to the semicolon that ends it.
    // The NEAREST such binding above the call, so a function that rebinds the
    // name is judged on the one this call actually passes.
    for opener in [format!("let mut {name} ="), format!("let {name} =")] {
        let Some(at) = call.prefix.rfind(&opener) else {
            continue;
        };
        let tail = &call.prefix[at..];
        let end = tail.find(";\n").map(|e| e + 1).unwrap_or(tail.len());
        if tail[..end].contains(".category(") || tail[..end].contains(".force(") {
            return true;
        }
    }
    false
}

/// Every `states/driving*` source file, the tree the Python sweep walked.
///
/// Python globbed `driving*.py` in one flat directory. Here the same code is
/// split across `driving*.rs` AND the `driving*/` directories beside them, and
/// the files inside those (`driving_updates/air.rs`, `driving_events/
/// ambient.rs`) carry most of the call sites -- so below the top level every
/// `.rs` file counts, not only the ones whose own name starts with "driving".
fn collect_driving_sources(root: &std::path::Path, top: bool, out: &mut Vec<std::path::PathBuf>) {
    for entry in std::fs::read_dir(root).expect("the states directory") {
        let entry = entry.expect("a readable directory entry");
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if path.is_dir() {
            if !top || name.starts_with("driving") {
                collect_driving_sources(&path, false, out);
            }
        } else if name.ends_with(".rs") && (!top || name.starts_with("driving")) {
            out.push(path);
        }
    }
}

/// `source` with every string literal, char literal and comment blanked to
/// spaces, so a parenthesis inside a message or a comment cannot unbalance the
/// scan below. Newlines survive, so line numbers still line up.
///
/// Handles raw strings (`r"..."`, `r#"..."#`) because the driving sources have
/// one -- `FREEWAY_VIA_PATTERN` -- and lifetimes (`&'a str`), which look like
/// an unterminated char literal and would otherwise swallow the rest of a file.
fn mask_literals(source: &str) -> String {
    let chars: Vec<char> = source.chars().collect();
    let mut out = String::with_capacity(source.len());
    let mut index = 0usize;
    let blank = |out: &mut String, ch: char| out.push(if ch == '\n' { '\n' } else { ' ' });
    while index < chars.len() {
        let ch = chars[index];
        // Line comment.
        if ch == '/' && chars.get(index + 1) == Some(&'/') {
            while index < chars.len() && chars[index] != '\n' {
                blank(&mut out, chars[index]);
                index += 1;
            }
            continue;
        }
        // Block comment, nesting as Rust's do.
        if ch == '/' && chars.get(index + 1) == Some(&'*') {
            let mut depth = 0usize;
            while index < chars.len() {
                if chars[index] == '/' && chars.get(index + 1) == Some(&'*') {
                    depth += 1;
                    blank(&mut out, chars[index]);
                    blank(&mut out, chars[index + 1]);
                    index += 2;
                    continue;
                }
                if chars[index] == '*' && chars.get(index + 1) == Some(&'/') {
                    depth -= 1;
                    blank(&mut out, chars[index]);
                    blank(&mut out, chars[index + 1]);
                    index += 2;
                    if depth == 0 {
                        break;
                    }
                    continue;
                }
                blank(&mut out, chars[index]);
                index += 1;
            }
            continue;
        }
        // Raw string: `r`, some hashes, a quote; closed by a quote and the
        // same number of hashes.
        if ch == 'r'
            && (index == 0 || !(chars[index - 1].is_alphanumeric() || chars[index - 1] == '_'))
        {
            let mut hashes = 0usize;
            while chars.get(index + 1 + hashes) == Some(&'#') {
                hashes += 1;
            }
            if chars.get(index + 1 + hashes) == Some(&'"') {
                for _ in 0..(hashes + 2) {
                    blank(&mut out, chars[index]);
                    index += 1;
                }
                while let Some(here) = chars.get(index) {
                    if *here == '"' && (0..hashes).all(|n| chars.get(index + 1 + n) == Some(&'#')) {
                        for _ in 0..(hashes + 1) {
                            blank(&mut out, chars[index]);
                            index += 1;
                        }
                        break;
                    }
                    blank(&mut out, *here);
                    index += 1;
                }
                continue;
            }
        }
        // Ordinary string.
        if ch == '"' {
            blank(&mut out, ch);
            index += 1;
            while index < chars.len() {
                if chars[index] == '\\' {
                    blank(&mut out, chars[index]);
                    if index + 1 < chars.len() {
                        blank(&mut out, chars[index + 1]);
                    }
                    index += 2;
                    continue;
                }
                let done = chars[index] == '"';
                blank(&mut out, chars[index]);
                index += 1;
                if done {
                    break;
                }
            }
            continue;
        }
        // Char literal, but NOT a lifetime: `'a` has no closing quote.
        if ch == '\'' {
            let closes = if chars.get(index + 1) == Some(&'\\') {
                chars
                    .iter()
                    .skip(index + 2)
                    .position(|c| *c == '\'')
                    .map(|offset| index + 2 + offset)
                    .filter(|close| *close <= index + 5)
            } else if chars.get(index + 2) == Some(&'\'') {
                Some(index + 2)
            } else {
                None
            };
            if let Some(close) = closes {
                while index <= close {
                    blank(&mut out, chars[index]);
                    index += 1;
                }
                continue;
            }
        }
        out.push(ch);
        index += 1;
    }
    out
}

/// Every `say_event`/`say_event_with` call in `source`.
///
/// Runs over [`mask_literals`], so the parenthesis balancing sees code only.
fn say_event_calls(source: &str) -> Vec<SayEventCall> {
    let masked: Vec<char> = mask_literals(source).chars().collect();
    let mut calls = Vec::new();
    let mut index = 0usize;
    let mut line = 1usize;
    while index < masked.len() {
        if masked[index] == '\n' {
            line += 1;
            index += 1;
            continue;
        }
        let rest: String = masked[index..].iter().take(15).collect();
        if rest.starts_with("say_event(") || rest.starts_with("say_event_with(") {
            let open = index + rest.find('(').expect("the call's open paren");
            let (args, consumed, newlines) = read_call(&masked, open);
            calls.push(SayEventCall {
                line,
                args,
                prefix: enclosing_fn_prefix(&masked, index),
            });
            line += newlines;
            index = consumed;
            continue;
        }
        index += 1;
    }
    calls
}

/// The enclosing function's text from its `fn` line down to `at`.
fn enclosing_fn_prefix(masked: &[char], at: usize) -> String {
    let mut line_start = 0usize;
    let mut best = 0usize;
    for (index, ch) in masked.iter().enumerate().take(at) {
        if *ch == '\n' {
            let text: String = masked[line_start..index].iter().collect();
            let trimmed = text.trim_start();
            let head = trimmed
                .strip_prefix("pub(crate) ")
                .or_else(|| trimmed.strip_prefix("pub(super) "))
                .or_else(|| trimmed.strip_prefix("pub "))
                .unwrap_or(trimmed);
            if head.starts_with("fn ") || head.starts_with("async fn ") {
                best = line_start;
            }
            line_start = index + 1;
        }
    }
    masked[best..at].iter().collect()
}

/// The masked text between `open`'s parentheses, the index just past the
/// close, and how many newlines were crossed.
fn read_call(masked: &[char], open: usize) -> (String, usize, usize) {
    let mut depth = 0i32;
    let mut index = open;
    let mut newlines = 0usize;
    let mut out = String::new();
    while index < masked.len() {
        let ch = masked[index];
        if ch == '\n' {
            newlines += 1;
        }
        if ch == '(' {
            depth += 1;
        } else if ch == ')' {
            depth -= 1;
            if depth == 0 {
                return (out, index + 1, newlines);
            }
        }
        out.push(ch);
        index += 1;
    }
    panic!("unbalanced say_event call");
}

// -- the whole-drive proof -----------------------------------------------------------

/// `reverse_down_the_route` (backing down the interstate) was picked by
/// measuring, not by assumption: most of the battery's scenarios never reach a
/// STATUS or CONFIRMATION call site that survives the per-condition repeat
/// suppression, so their rung-to-rung counts are flat and would make this a
/// vacuous test. This one reliably says a fresh "engine is screaming at
/// redline" STATUS readout on each further mile of engine wear -- short
/// words at quiet, silence at urgent_only, full words at standard.
const SCENARIO: &str = "reverse_down_the_route";

/// One scenario's transcript at one rung.
///
/// Python patched `Rig.__init__`; [`tweak_rigs`] is the same seam made
/// explicit. Both overrides were found by walking the battery rather than
/// assuming: the rig never sets `driving_speech`, so every scenario runs at
/// the stock default; and it builds a fresh `Profile`, whose `tutorial_done`
/// defaults false -- which makes the ENTIRE ladder gate a no-op and every rung
/// sound identical.
fn transcript_at(rung: &str) -> Vec<String> {
    let rung = rung.to_string();
    let guard = tweak_rigs(move |rig| {
        rig.app.ctx.settings.driving_speech = rung.clone();
        if let Some(profile) = rig.app.ctx.profile.as_mut() {
            profile.tutorial_done = true;
        }
    });
    let outcome = breaker::run_scenario(SCENARIO).expect("a registered scenario");
    drop(guard);
    assert_ne!(outcome.verdict, Verdict::Error, "{}", outcome.note);
    outcome.transcript
}

#[test]
fn test_a_drive_gets_quieter_as_the_rung_tightens() {
    let standard = transcript_at("standard");
    let quiet = transcript_at("quiet");
    let urgent_only = transcript_at("urgent_only");
    assert!(standard.join("\n").contains("Engine at redline"));
    assert!(quiet.join("\n").contains("Redline. Engine wear"));
    assert!(!urgent_only.join("\n").contains("Redline. Engine wear"));
    assert!(quiet.len() > urgent_only.len());
    assert!(quiet.join(" ").len() <= standard.join(" ").len());
}
