//! The `say_event(valid=...)` gates, all of them, asked the one question the
//! port kept getting wrong.
//!
//! A queued line carries a "is this still true?" test. The pacer consults it
//! only on the rescue path: a ROUTE or CRITICAL line cut off mid-sentence is
//! offered back once so an interruption cannot destroy it, and the test is
//! what stops the words coming back after the moment they described has
//! passed.
//!
//! Python's tests read the live drive (`self.trip.position_mi < scale_mi`,
//! `self._hazard_deadline is not None`). A Rust validity closure is `'static`
//! and cannot borrow the drive, and in four places the port substituted
//! something that only looks the same: a projection onto the wall clock, or
//! an answer taken once at submission time. Both say "still true" for a
//! driver who has already dealt with the thing -- a hazard dodged, a bend
//! taken, a gore passed, a key pressed -- because neither reading moves when
//! the truck does.
//!
//! Each test here queues the real line through the real state, changes the
//! fact the line depends on, and then cuts the channel with an urgent line.
//! Every one of them fails against a clock projection or a snapshot, and
//! passes against a live reading.

use ff_core::data::curves::RouteCurve;
use ff_core::data::world::get_world;
use ff_core::models::jobs::{Job, CARGO_CATALOG};
use ff_core::models::profile::Profile;
use ff_core::sim::trip_models::{RoadStop, TripEvent, TripEventData, TripEventKind};
use ff_core::sim::weather::WeatherKind;
use ff_core::speech_text::SpokenMessage;

use freight_fate::app::testing::TestApp;
use freight_fate::states::driving::DrivingState;
use freight_fate::states::driving_core::*;

// -- rigging -------------------------------------------------------------------------

/// The urgent line that cuts the channel. Anything CRITICAL will do; this is
/// one the driving layer really speaks.
const CUTTER: &str = "Emergency vehicle approaching from behind.";

/// A Buffalo to Rochester delivery on the event voice, with a fake pacer
/// clock parked at zero so every queued line still counts as mid-sentence.
fn a_drive(app: &mut TestApp) -> DrivingState {
    let world = get_world();
    let mut profile = Profile::named_in("Gates", "Buffalo");
    profile.tutorial_done = true;
    app.ctx.profile = Some(profile);
    app.ctx.settings.sapi_events = true;
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
        Some(0),
        DRIVE_PHASE_DELIVERY,
        Some(12.0),
    );
    drive.trip.set_npc_vehicles(Vec::new());
    drive.trip.weather.current = WeatherKind::Clear;
    drive.trip.truck.start_engine();
    drive
}

fn mph_to_mps(mph: f64) -> f64 {
    mph / 2.2369362920544
}

/// How many times the player actually heard a line carrying `needle`.
fn heard(app: &TestApp, needle: &str) -> usize {
    app.event_lines()
        .into_iter()
        .filter(|line| line.contains(needle))
        .count()
}

// -- the hazard call ------------------------------------------------------------------

#[test]
fn a_hazard_call_is_not_handed_back_after_the_collision() {
    // Python gates the hazard call on `self._hazard_deadline is not None`:
    // the words may only come back while there is still a hazard to answer.
    // The port projected the deadline forward onto the wall clock instead,
    // which stays open for the whole of any run whose simulated time outpaces
    // real time -- and the collision line is itself the interrupt that offers
    // the rescue, so "Debris on the road ahead. Brake!" came back immediately
    // after the truck had already hit it.
    let mut app = TestApp::new();
    let _clock = app.fake_pacer_clock();
    let mut d = a_drive(&mut app);
    app.ctx.settings.automatic_emergency_braking = false; // nobody is going to brake
    d.trip.position_mi = 12.0;
    d.trip.truck.velocity_mps = mph_to_mps(65.0);
    app.clear_speech();

    let hazard = TripEvent {
        kind: TripEventKind::Hazard,
        message: SpokenMessage::new("Debris on the road ahead. Brake!"),
        data: TripEventData::default(),
    };
    d.handle_trip_event(&mut app.ctx, &hazard);
    assert_eq!(heard(&app, "Debris on the road ahead"), 1);
    assert!(d.hazard_deadline.is_some());

    // Ignore it all the way to the deadline: the hazard stops being
    // answerable at the moment of impact.
    d.update_hazard(&mut app.ctx, 60.0);
    assert!(d.hazard_deadline.is_none());
    assert_eq!(heard(&app, "Collision!"), 1);
    assert_eq!(
        heard(&app, "Debris on the road ahead"),
        1,
        "the hazard warning was handed back after the truck had already hit \
         the debris: {:?}",
        app.event_lines()
    );
}

#[test]
fn a_hazard_call_is_still_handed_back_while_the_hazard_is_live() {
    // The mirror image, and the reason the gate cannot simply be `false`: a
    // warning cut off mid-sentence by another urgent line, with the hazard
    // still armed, is exactly what the rescue slot exists for.
    let mut app = TestApp::new();
    let _clock = app.fake_pacer_clock();
    let mut d = a_drive(&mut app);
    d.trip.position_mi = 12.0;
    d.trip.truck.velocity_mps = mph_to_mps(65.0);
    app.clear_speech();

    let hazard = TripEvent {
        kind: TripEventKind::Hazard,
        message: SpokenMessage::new("Debris on the road ahead. Brake!"),
        data: TripEventData::default(),
    };
    d.handle_trip_event(&mut app.ctx, &hazard);
    app.ctx.say_event(CUTTER);

    assert!(d.hazard_deadline.is_some());
    assert_eq!(
        heard(&app, "Debris on the road ahead"),
        2,
        "a live hazard's warning must survive being cut: {:?}",
        app.event_lines()
    );
}

// -- the curve call -------------------------------------------------------------------

fn a_bend(start_mi: f64) -> RouteCurve {
    RouteCurve {
        start_mi,
        apex_mi: start_mi + 0.05,
        end_mi: start_mi + 0.1,
        direction: 'L',
        advisory_mph: 35,
        min_radius_ft: 700,
        deflection_deg: 60.0,
        connector: false,
    }
}

fn a_curve_event(curve: &RouteCurve, ahead_mi: f64) -> TripEvent {
    TripEvent {
        kind: TripEventKind::Curve,
        message: SpokenMessage::new("Sharp left, a quarter mile. Advise 35."),
        data: TripEventData {
            curve: Some(*curve),
            advisory_mph: Some(curve.advisory_mph as f64),
            ahead_mi: Some(ahead_mi),
            ..TripEventData::default()
        },
    }
}

#[test]
fn a_curve_call_is_not_handed_back_once_the_bend_is_behind_the_truck() {
    // Python handed `say_event` the predicate itself -- the bend still
    // ahead, and the truck still above its advisory -- and re-ran it when the
    // rescue fired. The port took the answer once, at submission time, where
    // it is true by construction: a curve call is only made for a bend that
    // is ahead and a truck that is fast. So the gate never refused anything,
    // and "Sharp left" came back after the bend was taken.
    let mut app = TestApp::new();
    let _clock = app.fake_pacer_clock();
    let mut d = a_drive(&mut app);
    app.ctx.settings.curve_callouts = true;
    d.trip.position_mi = 10.0;
    d.trip.truck.velocity_mps = mph_to_mps(62.0);
    let curve = a_bend(10.25);
    app.clear_speech();

    d.handle_trip_event(&mut app.ctx, &a_curve_event(&curve, 0.25));
    assert_eq!(heard(&app, "Sharp left"), 1);

    // Through the bend and slowed for it: neither half of the test holds now.
    d.trip.position_mi = 10.6;
    d.trip.truck.velocity_mps = mph_to_mps(30.0);
    d.refresh_live_facts();
    app.ctx.say_event(CUTTER);

    assert_eq!(
        heard(&app, "Sharp left"),
        1,
        "the curve call was handed back after the bend: {:?}",
        app.event_lines()
    );
}

#[test]
fn a_curve_call_is_still_handed_back_while_the_bend_is_ahead() {
    let mut app = TestApp::new();
    let _clock = app.fake_pacer_clock();
    let mut d = a_drive(&mut app);
    app.ctx.settings.curve_callouts = true;
    d.trip.position_mi = 10.0;
    d.trip.truck.velocity_mps = mph_to_mps(62.0);
    let curve = a_bend(10.25);
    app.clear_speech();

    d.handle_trip_event(&mut app.ctx, &a_curve_event(&curve, 0.25));
    app.ctx.say_event(CUTTER);

    assert_eq!(
        heard(&app, "Sharp left"),
        2,
        "a bend still ahead must survive being cut: {:?}",
        app.event_lines()
    );
}

// -- the destination exit confirmation ------------------------------------------------

/// A destination exit ahead, already called out, waiting on the driver's
/// press: the state `_toggle_exit_signal` reads to answer a callout rather
/// than to arm an exit cold.
fn a_destination_exit(d: &mut DrivingState, at_mi: f64) -> RoadStop {
    let mut stop = RoadStop::new("Rochester freight market", at_mi, "delivery_destination");
    stop.exit_label = "Exit 12".to_string();
    d.trip.stops.push(stop.clone());
    d.exit_stop = Some(stop.clone());
    d.destination_exit_response_s = 6.0;
    d.destination_exit_announced_key = DrivingState::destination_exit_key(&stop);
    stop
}

#[test]
fn the_exit_confirmation_is_not_handed_back_past_the_gore() {
    // Python: `valid=lambda: self.trip.position_mi < exit_mi`. The port wrote
    // the deviation note for this one and then left the gate off the options
    // entirely, so "Move right for the exit lane" could be handed back with
    // the ramp already behind the truck -- an instruction for a maneuver that
    // no longer exists.
    let mut app = TestApp::new();
    let _clock = app.fake_pacer_clock();
    let mut d = a_drive(&mut app);
    d.trip.position_mi = 20.0;
    d.trip.truck.velocity_mps = mph_to_mps(60.0);
    let stop = a_destination_exit(&mut d, 21.0);
    app.clear_speech();

    d.toggle_exit_signal(&mut app.ctx);
    assert_eq!(heard(&app, "Signal set for"), 1);

    d.trip.position_mi = stop.at_mi + 0.5; // the gore is behind the truck
    d.refresh_live_facts();
    app.ctx.say_event(CUTTER);

    assert_eq!(
        heard(&app, "Signal set for"),
        1,
        "the exit confirmation was handed back past the gore: {:?}",
        app.event_lines()
    );
}

#[test]
fn the_exit_confirmation_is_still_handed_back_before_the_gore() {
    let mut app = TestApp::new();
    let _clock = app.fake_pacer_clock();
    let mut d = a_drive(&mut app);
    d.trip.position_mi = 20.0;
    d.trip.truck.velocity_mps = mph_to_mps(60.0);
    a_destination_exit(&mut d, 21.0);
    app.clear_speech();

    d.toggle_exit_signal(&mut app.ctx);
    app.ctx.say_event(CUTTER);

    assert_eq!(
        heard(&app, "Signal set for"),
        2,
        "an exit still ahead must survive being cut: {:?}",
        app.event_lines()
    );
}

// -- the destination hold prompt ------------------------------------------------------

fn at_the_gate(d: &mut DrivingState, mph: f64) {
    d.destination_exit_taken = true;
    d.trip.position_mi = d.trip.total_miles();
    d.trip.finished = true;
    d.trip.truck.engine_on = true;
    d.trip.truck.velocity_mps = mph_to_mps(mph);
    d.gate_speed_warned = false;
    d.gate_grace_s = 0.0;
}

#[test]
fn the_hold_prompt_is_not_handed_back_once_the_dock_menu_is_open() {
    // Python: `valid=lambda: not self._arrival_menu_open`. A line asking for
    // a keypress, handed back after the press it asked for, asks for
    // something that has already happened -- Shane heard three of these at
    // one dock. The port left this gate off too.
    let mut app = TestApp::new();
    let _clock = app.fake_pacer_clock();
    let mut d = a_drive(&mut app);
    app.ctx.settings.destination_approach_assist = true;
    at_the_gate(&mut d, 0.2);
    app.clear_speech();

    d.handle_arrival_gate(&mut app.ctx);
    assert_eq!(heard(&app, "Holding at the entrance"), 1);

    // The driver pressed it. The frame loop stops here, which is why the
    // reading has to be stamped where the flag moves.
    d.open_facility_arrival(&mut app.ctx);
    assert!(d.arrival_menu_open);
    app.ctx.say_event(CUTTER);

    assert_eq!(
        heard(&app, "Holding at the entrance"),
        1,
        "the hold prompt asked again for a press that had already happened: {:?}",
        app.event_lines()
    );
}

// -- the brake lockout ----------------------------------------------------------------

#[test]
fn the_parking_brake_lockout_comes_back_only_while_the_brake_is_set() {
    // Agent drive out of Aberdeen Company Yard, 2026-09-30: throttle against
    // a set parking brake, release it, roll -- and "Parking brake set. Press
    // P to release it." came back interrupting, twice, with the truck moving.
    // Pressing P there sets the spring brakes at speed.
    for released in [true, false] {
        let mut app = TestApp::new();
        let _clock = app.fake_pacer_clock();
        let mut d = a_drive(&mut app);
        d.trip.truck.set_air_ready(true);
        d.trip.truck.parking_brake = true;
        app.clear_speech();
        d.maybe_say_air_brake_lockout(&mut app.ctx);
        assert_eq!(
            heard(&app, "Parking brake set"),
            1,
            "{:?}",
            app.event_lines()
        );

        if released {
            d.trip.truck.parking_brake = false;
        }
        d.refresh_live_facts();
        app.ctx.say_event(CUTTER);

        let expected = if released { 1 } else { 2 };
        assert_eq!(
            heard(&app, "Parking brake set"),
            expected,
            "released {released}: {:?}",
            app.event_lines()
        );
    }
}
