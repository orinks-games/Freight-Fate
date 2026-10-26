//! `states/driving_turns.rs`, `states/driving_pacenotes.rs` and
//! `states/driving_location.rs`.
//!
//! Ported from `tests/test_turn_commitment.py`, `tests/test_pacenotes.py` and
//! `tests/test_driving_place_keys.py`. Cases that need a mixin this task did
//! not port keep their bodies behind `#[ignore]`.

use std::sync::Arc;

use ff_core::data::corners::corner_speed_mph;
use ff_core::data::curves::{curve_severity, leg_curves, min_radius_ft, route_curves, RouteCurve};
use ff_core::data::world::get_world;
use ff_core::data::world_models::{CorridorDetail, Landmark, Leg, Route, RouteCheckpoint};
use ff_core::models::jobs::{Job, CARGO_CATALOG};
use ff_core::models::profile::Profile;
use ff_core::settings::Settings;
use ff_core::sim::trip::{Trip, TripOptions};
use ff_core::sim::trip_models::TripEventKind;
use ff_core::sim::weather::{WeatherKind, WeatherSystem};

use freight_fate::app::testing::TestApp;
use freight_fate::states::base::{InputEvent, Key, Mods};
use freight_fate::states::driving::DrivingState;
use freight_fate::states::driving_core::*;
use freight_fate::states::driving_location::NEAREST_TOWN_MI;
use freight_fate::states::driving_turns::{
    is_judged_turn, RAMP_GUIDE_DEMAND, TURN_COMMIT_TAIL_MI, TURN_CORNER_MAX_MPH,
    TURN_MISS_LOOP_MIN, TURN_WINDOW_MAX_MI, TURN_WINDOW_MIN_MI,
};

// -- rigging -------------------------------------------------------------------------

/// `_driving(app)`: a Buffalo to Rochester delivery.
fn a_drive(app: &mut TestApp) -> DrivingState {
    let world = get_world();
    let mut profile = Profile::named_in("Corners", "Buffalo");
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
        None,
        DRIVE_PHASE_DELIVERY,
        Some(12.0),
    );
    // `quiet_trip(driving)`: an empty road, and a pinned sky. The trip seed
    // is unseeded, so a drive that does not pin the weather draws a real
    // condition -- and an ice day's safe speed sits under the advisories
    // these cases measure.
    drive.trip.set_npc_vehicles(Vec::new());
    drive.trip.weather.current = WeatherKind::Clear;
    drive
}

/// `_street_chain(d, time_scale, short_block_mi)`: swap the drive onto a
/// deterministic three-block facility street chain.
///
/// Leg 1 is a left onto a 25 mph street (advise 20, the trailer cap); leg 2
/// is a right onto a 15 mph service way (advise 15, the street's own limit).
/// `short_block_mi` shortens the middle block so the two corners arrive in
/// quick succession, the way a real city grid delivers them.
fn street_chain(d: &mut DrivingState, time_scale: f64, short_block_mi: f64) {
    let city = d.trip.route.cities[0].clone();
    let legs = vec![
        Leg::local(
            &city,
            0.6,
            "East Navarre Street",
            "Start on East Navarre Street.",
            25.0,
        ),
        // A square city corner, and a sharper one into the service way, so the
        // chain exercises two different baked angles rather than one.
        Leg::local(
            &city,
            short_block_mi,
            "North Michigan Street",
            "Turn left onto North Michigan Street.",
            25.0,
        )
        .with_turn_deg(90.0),
        Leg::local(
            &city,
            0.5,
            "West Sample Street",
            "Turn right onto West Sample Street.",
            15.0,
        )
        .with_turn_deg(105.0),
    ];
    let route = Route::from_legs(vec![city.clone(); 4], legs);
    let truck = d.trip.truck.clone();
    let weather = WeatherSystem::new("", Some(3), None, None, false);
    let mut trip = Trip::new(
        route,
        truck,
        weather,
        TripOptions {
            seed: Some(3),
            time_scale,
            ..Default::default()
        },
    );
    trip.set_npc_vehicles(Vec::new());
    d.replace_trip(trip);
    d.reset_turn_state_for_trip();
}

fn a_street_chain(d: &mut DrivingState) {
    street_chain(d, 1.0, 0.5);
}

fn alt(k: Key) -> InputEvent {
    InputEvent::key_mods(k, Mods::ALT)
}

fn mph(d: &mut DrivingState, mph: f64) {
    d.trip.truck.engine_on = true;
    d.trip.truck.velocity_mps = mph / 2.23694;
}

/// `_at_turn`: roll up to a turn, hear its call, and let the reaction window
/// expire.
fn at_turn(d: &mut DrivingState, app: &mut TestApp, at_mi: f64, speed_mph: f64) {
    d.trip.position_mi = at_mi - 0.2;
    mph(d, speed_mph);
    d.update_turn_commitment(&mut app.ctx, 0.016);
    d.trip.position_mi = at_mi;
    let dt = d.turn_grace_s + 1.0;
    d.update_turn_commitment(&mut app.ctx, dt);
}

/// Every event line so far that carries `needle`, newest last.
///
/// Python read `spoken[-1]` off a stubbed `say_event`. Here the real pacer is
/// in the path: an interrupting line purges the channel and hands the ROUTE
/// line it cut back to be requeued, so the rescued line legitimately lands
/// after the one that cut it. What each assertion is about is the line
/// itself, not its position behind a rescue.
fn lines_with(app: &TestApp, needle: &str) -> Vec<String> {
    app.event_lines()
        .into_iter()
        .filter(|line| line.contains(needle))
        .collect()
}

fn last_with(app: &TestApp, needle: &str) -> String {
    lines_with(app, needle)
        .pop()
        .unwrap_or_else(|| panic!("no event line carried {needle:?}: {:?}", app.event_lines()))
}

// -- the approach call -------------------------------------------------------

#[test]
fn test_approach_call_names_the_side_street_distance_and_speed() {
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    a_street_chain(&mut d);
    app.clear_speech();
    d.trip.position_mi = 0.4;
    mph(&mut d, 30.0);
    d.update_turn_commitment(&mut app.ctx, 0.016);
    let spoken = app.event_lines();
    assert_eq!(spoken.len(), 1);
    assert_eq!(
        spoken[0],
        "Left turn onto North Michigan Street, a quarter mile. Advise 14 miles per hour."
    );
    assert!(d.turn_grace_s > 0.0);
    d.update_turn_commitment(&mut app.ctx, 0.016);
    assert_eq!(app.event_lines().len(), 1); // said once, not every frame
}

/// Owner, Abilene streets, 2026-09-23: "That left turn announced at least
/// twice before the turn. Necessary?" The route's quarter-mile lead and the
/// turn's own approach call named the same corner seconds apart.
fn the_route_lead_for(d: &mut DrivingState) -> ff_core::sim::trip_models::TripEvent {
    let cue = d.turn_cue_in_play().expect("a corner is in play");
    ff_core::sim::trip_models::TripEvent {
        kind: TripEventKind::GpsCue,
        message: ff_core::speech_text::SpokenMessage::new(format!(
            "In a quarter mile, {}.",
            cue.text
        )),
        data: ff_core::sim::trip_models::TripEventData {
            cue: Some(cue),
            advance: Some(true),
            ..Default::default()
        },
    }
}

#[test]
fn test_the_route_lead_stays_quiet_for_a_turn_already_called() {
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    a_street_chain(&mut d);
    app.clear_speech();
    d.trip.position_mi = 0.4;
    mph(&mut d, 30.0);
    d.update_turn_commitment(&mut app.ctx, 0.016);
    assert_eq!(app.event_lines().len(), 1, "the approach call");
    let lead = the_route_lead_for(&mut d);
    d.handle_trip_event(&mut app.ctx, &lead);
    assert_eq!(app.event_lines().len(), 1, "{:?}", app.event_lines());
}

#[test]
fn test_a_turn_call_after_the_route_lead_adds_only_the_advisory() {
    let mut app = TestApp::new();
    let clock = app.fake_pacer_clock();
    let mut d = a_drive(&mut app);
    a_street_chain(&mut d);
    app.clear_speech();
    d.trip.position_mi = 0.4;
    mph(&mut d, 30.0);
    let cue = d.turn_cue_in_play().expect("a corner is in play");
    let lead = the_route_lead_for(&mut d);
    d.trip
        .announced_navigation
        .insert(format!("{}:advance", cue.key));
    d.handle_trip_event(&mut app.ctx, &lead);
    clock.advance(5.0); // the lead has been heard
    d.update_turn_commitment(&mut app.ctx, 0.016);
    assert_eq!(
        app.event_lines().last().map(String::as_str),
        Some("Advise 14 miles per hour."),
        "{:?}",
        app.event_lines()
    );
}

#[test]
fn test_terse_keeps_the_advise_speed() {
    // The advise speed is the one number a driver brakes to. Terse used to
    // drop it with the rest of the tail and left nothing to aim for.
    let mut app = TestApp::new();
    app.ctx.settings.driving_speech = "quiet".to_string();
    let mut d = a_drive(&mut app);
    a_street_chain(&mut d);
    app.clear_speech();
    d.trip.position_mi = 0.4;
    mph(&mut d, 30.0);
    d.keeper_mph = Some(25.0);
    d.update_turn_commitment(&mut app.ctx, 0.016);
    let spoken = app.event_lines();
    assert_eq!(
        spoken[0],
        "Left turn onto North Michigan Street, a quarter mile. Advise 14 miles per hour."
    );
    // Never a bare "Right now": the verb always leads.
    assert!(!spoken[0].contains("Right now"));
}

#[test]
fn test_a_turn_the_truck_is_already_slow_for_is_still_called_with_its_speed() {
    // Player report, 2026-10-13: the advise speed came on some turns and not
    // others, because only a truck over it was told. A driver who had
    // slowed right down heard no number and so never learned how fast the
    // next corner could be taken.
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    a_street_chain(&mut d);
    app.clear_speech();
    d.trip.position_mi = 0.4;
    mph(&mut d, 8.0);
    let cue = d.turn_cue_in_play().expect("a corner is in play");
    assert!(d.trip.truck.speed_mph() < d.turn_speed_mph(&cue));
    d.update_turn_commitment(&mut app.ctx, 0.016);
    assert_eq!(
        app.event_lines(),
        vec![
            "Left turn onto North Michigan Street, a quarter mile. Advise 14 miles per hour."
                .to_string()
        ]
    );
}

#[test]
fn test_the_approach_call_under_the_speed_keeper_is_just_the_corner_and_its_speed() {
    // Owner, 2026-10-25: "Speed keeper easing." on every corner made the
    // street transition too wordy. The advise speed already tells the
    // driver the truck will slow, so the call ends there.
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    a_street_chain(&mut d);
    app.clear_speech();
    d.trip.position_mi = 0.4;
    mph(&mut d, 30.0);
    d.keeper_mph = Some(25.0);
    d.update_turn_commitment(&mut app.ctx, 0.016);
    assert_eq!(
        app.event_lines()[0],
        "Left turn onto North Michigan Street, a quarter mile. Advise 14 miles per hour."
    );
}

#[test]
fn test_the_approach_call_stays_quiet_about_a_keeper_with_nothing_to_shed() {
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    a_street_chain(&mut d);
    app.clear_speech();
    d.trip.position_mi = 0.4;
    // Manual speed control leaves the call exactly as it was.
    mph(&mut d, 30.0);
    d.update_turn_commitment(&mut app.ctx, 0.016);
    assert!(app.event_lines()[0].ends_with("Advise 14 miles per hour."));
    // So does a keeper already under the corner speed: there is nothing
    // for it to shed, so there is nothing to say about it.
    mph(&mut d, 8.0);
    d.keeper_mph = Some(8.0);
    let cue = d.turn_cue_in_play().expect("a corner is in play");
    assert!(d
        .turn_approach_text(&app.ctx, &cue, 0.2)
        .ends_with("Advise 14 miles per hour."));
}

#[test]
fn test_the_planner_sees_past_the_corner_it_is_already_easing_for() {
    // A corner holds the keeper's target through its own tail, and a city
    // block is shorter than that tail. Holding the FIRST corner's number
    // through it meant the 15 mph service way one short block on never
    // reached the planner at all -- turns "coming up really quickly", as the
    // tester put it. The corner already being eased for is a floor on what to
    // shed for now, never a reason to stop looking for something slower.
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    let load = d.trip.truck.roll_load_fraction();
    street_chain(&mut d, 1.0, 0.08);
    let cues = d.turn_cues_in_play();
    let (first, second) = (cues[0].clone(), cues[1].clone());
    assert_eq!(d.turn_speed_mph(&first), side_speed(&first, 90.0, load));
    assert_eq!(d.turn_speed_mph(&second), side_speed(&second, 105.0, load));
    assert!(second.at_mi - first.at_mi < 0.15); // inside the first corner's tail

    // Easing for the first corner, well before the second one is close. The
    // first corner's number is lowered only if the second is too close to
    // brake for after it.
    d.trip.position_mi = first.at_mi - 0.05;
    mph(&mut d, 25.0);
    let first_target = d.corner_targets_ahead()[0].1;
    assert!(first_target <= side_speed(&first, 90.0, load));
    assert_eq!(
        d.keeper_speed_ahead(&mut app.ctx),
        Some((first_target, "turn".to_string()))
    );

    // One block on, with the second corner's own window open, the planner
    // takes the slower number rather than riding the first corner's out.
    d.trip.position_mi = second.at_mi - 0.03;
    mph(&mut d, 19.0);
    assert_eq!(
        d.keeper_speed_ahead(&mut app.ctx),
        Some((side_speed(&second, 105.0, load), "turn".to_string()))
    );
}

fn turn_chimes(audio: &freight_fate::app::testing::AudioLog) -> Vec<(String, f64)> {
    audio
        .borrow()
        .played
        .iter()
        .filter(|(key, _, _)| key.starts_with("events/turn_"))
        .map(|(key, _, pan)| (key.clone(), *pan))
        .collect()
}

#[test]
fn test_the_turn_chime_goes_out_with_the_call_that_names_it() {
    // The chime used to sound as the truck took the corner, and the call for
    // the next corner followed it: a right chime landed beside "Left turn"
    // (player report, 2026-10-13). It goes out once, with the words for its
    // own turn, panned to that turn's side.
    let mut app = TestApp::new();
    let audio = app.record_audio();
    let mut d = a_drive(&mut app);
    a_street_chain(&mut d);
    let cue = d.turn_cue_in_play().expect("a corner is in play");
    d.trip.position_mi = 0.4;
    mph(&mut d, 30.0);
    d.update_turn_commitment(&mut app.ctx, 0.016);
    assert_eq!(
        turn_chimes(&audio),
        vec![("events/turn_left".to_string(), -TURN_CUE_PAN)]
    );

    // The route's own cues for the same corner add no second chime, and
    // neither does taking it. Three chimes on one turn out of Houston is
    // what that cost before (owner drive, 2026-09-20).
    assert!(is_judged_turn(&cue));
    let near = ff_core::sim::trip_models::TripEvent {
        kind: TripEventKind::GpsCue,
        message: ff_core::speech_text::SpokenMessage::new(cue.near_text.clone()),
        data: ff_core::sim::trip_models::TripEventData {
            cue: Some(cue.clone()),
            ..Default::default()
        },
    };
    d.handle_trip_event(&mut app.ctx, &near);
    d.resolve_turn(&cue);
    assert_eq!(turn_chimes(&audio).len(), 1);
}

#[test]
fn test_a_turn_first_named_by_the_route_chimes_with_that_line() {
    let mut app = TestApp::new();
    let audio = app.record_audio();
    let mut d = a_drive(&mut app);
    a_street_chain(&mut d);
    let cue = d.turn_cue_in_play().expect("a corner is in play");
    let near = ff_core::sim::trip_models::TripEvent {
        kind: TripEventKind::GpsCue,
        message: ff_core::speech_text::SpokenMessage::new(cue.near_text.clone()),
        data: ff_core::sim::trip_models::TripEventData {
            cue: Some(cue.clone()),
            ..Default::default()
        },
    };
    d.handle_trip_event(&mut app.ctx, &near);
    assert!(d.turn_announced.contains(&cue.key));
    assert_eq!(
        turn_chimes(&audio),
        vec![("events/turn_left".to_string(), -TURN_CUE_PAN)]
    );
}

#[test]
fn test_a_turn_inside_the_last_ones_tail_is_called_before_it() {
    // One street turn speaks at a time, and a turn just taken stayed the
    // "nearest" for the tenth of a mile past it: a second turn inside that
    // tenth was called only once the truck was round it (agent drive,
    // Aberdeen yard, 2026-09-23).
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    street_chain(&mut d, 1.0, 0.06);
    let corners: Vec<_> = d
        .trip
        .navigation_cues
        .iter()
        .filter(|cue| is_judged_turn(cue))
        .cloned()
        .collect();
    assert!(corners.len() >= 2, "the chain has two turns");
    let (first, second) = (&corners[0], &corners[1]);
    assert!(
        second.at_mi - first.at_mi < 0.1,
        "the second is inside the tail"
    );
    // Called but not yet taken, the first keeps the floor: nothing about the
    // second is said over it.
    d.trip.position_mi = first.at_mi - 0.05;
    d.trip.check_navigation_cues();
    assert!(d
        .trip
        .announced_navigation
        .contains(&format!("{}:near", first.key)));
    for half in ["advance", "near"] {
        assert!(
            !d.trip
                .announced_navigation
                .contains(&format!("{}:{half}", second.key)),
            "the second turn's {half} was said over the first"
        );
    }
    d.trip.position_mi = first.at_mi + 0.001;
    d.trip.check_navigation_cues();
    d.trip.position_mi = second.at_mi - 0.02;
    d.trip.check_navigation_cues();
    assert!(
        d.trip
            .announced_navigation
            .contains(&format!("{}:near", second.key)),
        "the second turn was not called before it"
    );
}

/// Run the route's own street calls for this spot and hand them to the drive.
fn route_calls(d: &mut DrivingState, app: &mut TestApp) {
    d.trip.check_navigation_cues();
    for event in std::mem::take(&mut d.trip.events) {
        d.handle_trip_event(&mut app.ctx, &event);
    }
}

#[test]
fn test_the_next_turn_waits_until_the_truck_is_round_the_last() {
    // The call for the next corner went out while the truck was still round
    // the last one, so "Turn right onto North Freeway Road" was heard over
    // the left turn, four turns out of four (agent drive, Tucson streets,
    // 2026-09-24).
    let mut app = TestApp::new();
    let audio = app.record_audio();
    let mut d = a_drive(&mut app);
    street_chain(&mut d, 1.0, 0.06);
    assert!(d.trip.truck.trailer_attached);
    let corners: Vec<_> = d
        .trip
        .navigation_cues
        .iter()
        .filter(|cue| is_judged_turn(cue))
        .cloned()
        .collect();
    let (first, second) = (corners[0].clone(), corners[1].clone());
    // The first corner, called by the route and approached at a crawl.
    mph(&mut d, 8.0);
    d.trip.position_mi = first.at_mi - 0.05;
    route_calls(&mut d, &mut app);
    d.update_turn_commitment(&mut app.ctx, 0.016);
    app.clear_speech();

    // At the corner: no chime (it went out with the call), and not a word
    // about the next one, from the route or from the corner's approach call.
    let before = turn_chimes(&audio).len();
    d.trip.position_mi = first.at_mi + 0.001;
    route_calls(&mut d, &mut app);
    d.update_turn_commitment(&mut app.ctx, 0.016);
    assert_eq!(turn_chimes(&audio).len(), before);
    route_calls(&mut d, &mut app);
    d.update_turn_commitment(&mut app.ctx, 0.016);
    assert!(
        lines_with(&app, "West Sample Street").is_empty(),
        "the next turn was called over this one: {:?}",
        app.event_lines()
    );

    // With the trailer round (a WB-67 is 73.5 feet), the next turn is called.
    d.trip.position_mi = first.at_mi + 0.015;
    route_calls(&mut d, &mut app);
    d.update_turn_commitment(&mut app.ctx, 0.016);
    assert!(
        !lines_with(&app, "West Sample Street").is_empty(),
        "{:?}",
        app.event_lines()
    );
    assert_eq!(
        turn_chimes(&audio).last(),
        Some(&("events/turn_right".to_string(), TURN_CUE_PAN)),
        "the right turn's chime goes with the right turn's call"
    );
    assert!(second.at_mi > d.trip.position_mi);
}

#[test]
fn test_a_cold_arrival_at_the_turn_still_gets_its_window() {
    // A resumed save can reach the turn without ever hearing the approach.
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    a_street_chain(&mut d);
    app.clear_speech();
    d.trip.position_mi = 0.6;
    mph(&mut d, 45.0);
    d.update_turn_commitment(&mut app.ctx, 0.016);
    let spoken = app.event_lines();
    assert!(spoken
        .last()
        .expect("a corner call")
        .starts_with("Turn left now onto North Michigan Street."));
    assert_eq!(d.turn_miss_count, 0);
    assert!(d.turn_grace_s > 0.0);
}

#[test]
fn test_the_window_is_real_seconds_not_a_fixed_distance() {
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    street_chain(&mut d, 40.0, 0.5);
    mph(&mut d, 55.0);
    assert_eq!(d.turn_window_mi(), TURN_WINDOW_MAX_MI);
    street_chain(&mut d, 1.0, 0.5);
    mph(&mut d, 20.0);
    assert_eq!(d.turn_window_mi(), TURN_WINDOW_MIN_MI);
}

#[test]
fn test_the_clock_waits_for_the_brake_point() {
    // The call is sized in real seconds on the compressed clock, so it opens
    // far out; dropping the clock there crawled a 30 mph approach for four
    // real minutes (flight, and the agent drive into Abilene, 2026-09-23).
    // The call still comes early. The clock stays paced past it, slides down
    // ahead of the brake point, and is real time from the brake point on.
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    street_chain(&mut d, 40.0, 0.5);
    mph(&mut d, 30.0);
    let paced = d.trip.effective_time_scale();
    assert!(paced > 1.0);
    let cue = d.turn_cue_in_play().expect("a corner is in play");
    let brake_mi = d.turn_brake_point_mi(&cue);

    d.trip.position_mi = 0.0;
    d.update_turn_commitment(&mut app.ctx, 0.016);
    assert!(
        !last_with(&app, "North Michigan Street").is_empty(),
        "the call comes at the window"
    );
    assert!(!d.trip.controlled_turn);
    assert_eq!(
        d.trip.effective_time_scale(),
        paced,
        "still paced past the call"
    );

    // Easing: between the trip's pacing and real time.
    d.trip.position_mi = cue.at_mi - brake_mi - 0.05;
    d.update_turn_commitment(&mut app.ctx, 0.016);
    let easing = d.trip.effective_time_scale();
    assert!(1.0 < easing && easing < paced, "easing at {easing:.1}x");

    d.trip.position_mi = cue.at_mi - brake_mi + 0.001;
    d.update_turn_commitment(&mut app.ctx, 0.016);
    assert!(d.trip.controlled_turn);
    assert_eq!(d.trip.effective_time_scale(), 1.0);

    d.trip.position_mi = 0.6;
    mph(&mut d, 18.0);
    let dt = d.turn_grace_s + 1.0;
    d.update_turn_commitment(&mut app.ctx, dt);
    assert!(!d.trip.controlled_turn);
}

// -- the turn speed ----------------------------------------------------------

#[test]
fn test_turn_speed_comes_from_the_corners_own_angle() {
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    let load = d.trip.truck.roll_load_fraction();
    a_street_chain(&mut d);
    let cues: Vec<_> = d
        .trip
        .navigation_cues
        .iter()
        .filter(|cue| cue.key.starts_with("local:turn:"))
        .cloned()
        .collect();
    assert_eq!(cues.len(), 2);
    // The square corner and the sharper one get DIFFERENT speeds -- the whole
    // point of the change. Both come from the angle, not the 25 mph street.
    let square = d.turn_speed_mph(&cues[0]);
    let sharp = d.turn_speed_mph(&cues[1]);
    assert_eq!(square, side_speed(&cues[0], 90.0, load));
    assert_eq!(sharp, side_speed(&cues[1], 105.0, load));
    assert!(sharp < square, "{sharp} should be under {square}");
    // And neither is the old flat clamp any more.
    assert!(square < TURN_CORNER_MAX_MPH);
}

#[test]
fn test_an_unmeasured_corner_is_priced_as_a_square_one() {
    // Most of the map's approaches are estimated and carry no angle. They must
    // still get a corner speed, and it must be the square-corner one rather
    // than the street's posted limit.
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    let load = d.trip.truck.roll_load_fraction();
    a_street_chain(&mut d);
    for leg in d.trip.route.legs.iter_mut() {
        std::sync::Arc::make_mut(leg).local_turn_deg = 0.0;
    }
    let cue = d
        .trip
        .navigation_cues
        .iter()
        .find(|cue| cue.key.starts_with("local:turn:"))
        .cloned()
        .expect("a turn cue");
    assert_eq!(d.turn_speed_mph(&cue), corner_speed_mph(None, load));
}

#[test]
fn test_under_the_turn_speed_passes_cleanly() {
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    a_street_chain(&mut d);
    let minutes = d.trip.game_minutes;
    // Under the square corner's own advised speed, which is a little over 9
    // now that the corner is priced from its angle rather than clamped at 20.
    at_turn(&mut d, &mut app, 0.6, 8.0);
    assert_eq!(d.turn_miss_count, 0);
    assert_eq!(d.trip.game_minutes, minutes);
    assert_eq!(d.trip.position_mi, 0.6);
}

#[test]
fn test_no_miss_while_the_turns_own_cue_is_still_speaking() {
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    a_street_chain(&mut d);
    d.trip.position_mi = 0.4;
    mph(&mut d, 40.0);
    d.update_turn_commitment(&mut app.ctx, 0.016);
    let grace = d.turn_grace_s;
    assert!(grace > 0.0);
    d.trip.position_mi = 0.6;
    d.update_turn_commitment(&mut app.ctx, 0.016);
    assert_eq!(d.turn_miss_count, 0); // still inside the spoken window
    d.update_turn_commitment(&mut app.ctx, grace + 1.0);
    assert_eq!(d.turn_miss_count, 1);
}

#[test]
fn test_a_missed_turn_keeps_the_speed_control_session_armed() {
    // A miss drops the keeper for the corner but keeps the session,
    // so it resumes on its own afterwards. Disarming it left the truck
    // idling off the corner with nothing said (agent drives, 2026-09-01;
    // owner ruling: the keeper manages facility-approach corners).
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    a_street_chain(&mut d);
    d.speed_control_armed = true;
    d.keeper_mph = Some(30.0);
    app.clear_speech();
    at_turn(&mut d, &mut app, 0.6, 40.0);
    assert_eq!(d.turn_miss_count, 1);
    assert!(d.keeper_mph.is_none());
    assert!(d.speed_control_armed, "the session survives the miss");
    assert!(!lines_with(&app, "Missed the turn").is_empty());
}

#[test]
fn test_a_hazard_never_reads_as_a_missed_turn() {
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    a_street_chain(&mut d);
    d.hazard_deadline = Some(5.0); // a swerve is not a blown turn
    at_turn(&mut d, &mut app, 0.6, 45.0);
    assert_eq!(d.turn_miss_count, 0);
}

#[test]
fn test_a_microsleep_never_reads_as_a_missed_turn() {
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    a_street_chain(&mut d);
    d.microsleep_deadline = Some(3.0);
    at_turn(&mut d, &mut app, 0.6, 45.0);
    assert_eq!(d.turn_miss_count, 0);
}

// -- the miss ----------------------------------------------------------------

#[test]
fn test_over_the_turn_speed_goes_around_the_block_and_charges_time() {
    // Player report, 2026-10-13: a miss dropped the truck back onto the
    // approach, "as if you had a teleport". It drives on, circles the block,
    // and comes out on the street it missed.
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    a_street_chain(&mut d);
    app.clear_speech();
    let minutes = d.trip.game_minutes;
    at_turn(&mut d, &mut app, 0.6, 45.0);
    assert_eq!(d.turn_miss_count, 1);
    assert_eq!(d.trip.game_minutes, minutes + TURN_MISS_LOOP_MIN);
    assert!(d.trip.position_mi >= 0.6 + TURN_COMMIT_TAIL_MI);
    assert!(!d.trip.controlled_turn);
    let text = last_with(&app, "Missed the turn");
    assert!(app
        .event_calls()
        .iter()
        .any(|(line, interrupt)| *line == text && *interrupt));
    assert_eq!(
        text,
        "Missed the turn onto North Michigan Street. Going around the block."
    );
    // The corner is settled; the next one is in play.
    assert_eq!(
        d.turn_cue_in_play().expect("the next corner").key,
        "local:turn:2"
    );
}

#[test]
fn test_terse_miss_keeps_the_essentials() {
    let mut app = TestApp::new();
    app.ctx.settings.driving_speech = "quiet".to_string();
    let mut d = a_drive(&mut app);
    a_street_chain(&mut d);
    app.clear_speech();
    at_turn(&mut d, &mut app, 0.6, 45.0);
    assert_eq!(
        last_with(&app, "Missed the turn."),
        "Missed the turn. Going around the block."
    );
}

#[test]
fn test_every_miss_says_the_same_short_line() {
    let mut app = TestApp::new();
    let clock = app.fake_pacer_clock();
    let mut d = a_drive(&mut app);
    street_chain(&mut d, 1.0, 0.5);
    app.clear_speech();
    at_turn(&mut d, &mut app, 0.6, 45.0);
    clock.advance(30.0); // the first miss has been heard
    at_turn(&mut d, &mut app, 1.1, 45.0);
    assert_eq!(d.turn_miss_count, 2);
    assert_eq!(
        last_with(&app, "Missed the turn"),
        "Missed the turn onto West Sample Street. Going around the block."
    );
    assert!(d.trip.position_mi > 1.1);
}

#[test]
fn test_snapshot_round_trips_the_turn_miss_count() {
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    d.turn_miss_count = 2;
    let snapshot = d.snapshot(&app.ctx);
    let resumed = DrivingState::from_snapshot(&mut app.ctx, &snapshot).expect("the drive resumes");
    assert_eq!(resumed.turn_miss_count, 2);
}

#[test]
fn test_highway_junctions_are_never_judged() {
    // Highway maneuver cues carry no direction, radius, or lane ordinal, so
    // they stay warn-only; nothing on a corridor route is a judged turn.
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    assert!(d.turn_cue_in_play().is_none());
    d.update_turn_commitment(&mut app.ctx, 0.016);
    assert_eq!(d.turn_miss_count, 0);
}

// -- the guide runs through turns and ramps ---------------------------------

#[test]
fn test_the_road_bed_leans_through_a_street_turn() {
    // Python read this through `_curve_steer_demand`, which delegates to the
    // maneuver demand whenever the active curve is a connector or absent;
    // `driving_updates` has not landed, so the delegate is called directly.
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    a_street_chain(&mut d);
    mph(&mut d, 20.0);
    d.trip.position_mi = 0.2; // nothing to lean into yet
    assert_eq!(d.maneuver_steer_demand(None), 0.0);
    d.trip.position_mi = 0.5; // leaning in
    let leaning = d.maneuver_steer_demand(None);
    assert!(leaning < 0.0); // a left turn leans left
    d.trip.position_mi = 0.6; // at the turn, full lean
    assert!(d.maneuver_steer_demand(None) < leaning);
    // And the right turn leans the other way.
    d.trip.position_mi = 0.65;
    d.update_turn_commitment(&mut app.ctx, 0.016);
    d.trip.position_mi = 1.1;
    assert!(d.maneuver_steer_demand(None) > 0.0);
}

#[test]
fn test_the_road_bed_leans_through_a_ramp_connector() {
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    let connector = RouteCurve {
        start_mi: d.trip.position_mi - 0.05,
        apex_mi: d.trip.position_mi,
        end_mi: d.trip.position_mi + 0.05,
        direction: 'R',
        advisory_mph: 30,
        min_radius_ft: 300,
        deflection_deg: 80.0,
        connector: true,
    };
    mph(&mut d, 40.0);
    // This returned 0.0 before the fix: the guide went silent on ramps.
    assert!(d.maneuver_steer_demand(Some(&connector)) > 0.0);
}

#[test]
fn test_the_speed_limit_readout_leaves_a_connector_arc_unnamed() {
    // Owner drive, I-30 to I-35 at Fort Worth: S said "The bend here advises
    // 40" inside a highway-to-highway connector that the curve call, the
    // servo and the cargo model all leave out by design -- a bend no assist
    // acts on. The readout names mainline bends only now.
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    let mut arc = RouteCurve {
        start_mi: d.trip.position_mi - 0.05,
        apex_mi: d.trip.position_mi,
        end_mi: d.trip.position_mi + 0.05,
        direction: 'R',
        advisory_mph: 30,
        min_radius_ft: 300,
        deflection_deg: 80.0,
        connector: true,
    };
    d.trip.curves = vec![arc];
    mph(&mut d, 55.0);

    app.clear_speech();
    d.speak_speed_limit(&mut app.ctx);
    let said = app.main_lines().join(" ");
    assert!(said.starts_with("Speed limit"), "{said}");
    assert!(!said.contains("bend here advises"), "{said}");

    // The same arc as a mainline bend is still named: the fix is the
    // connector flag, not the readout.
    arc.connector = false;
    d.trip.curves = vec![arc];
    app.clear_speech();
    d.speak_speed_limit(&mut app.ctx);
    let said = app.main_lines().join(" ");
    assert!(said.contains("The bend here advises 30"), "{said}");
}

#[test]
fn test_the_road_bed_leans_on_an_exit_ramp() {
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    d.ramp_mi = Some(0.3);
    assert_eq!(d.maneuver_steer_demand(None), RAMP_GUIDE_DEMAND);
}

#[test]
fn test_a_corner_you_are_already_slow_enough_for_still_buys_real_seconds() {
    // Owner, arriving in Spokane 2026-08-21: "I missed the turn."
    //
    // Four corners spoke in fifteen real seconds -- 23:26:22, :25, :27, :35 --
    // because the speed keeper was holding 14 through the facility zones, under
    // every corner's own advised speed. The commitment loop takes an early
    // return there ("already slow enough to make it"), and that return was also
    // the only path that never set `controlled_turn`. So the clock stayed
    // compressed and a downtown street chain arrived as a burst nobody could
    // act on.
    //
    // Being slow enough to MAKE the corner is not the same as being given time
    // to HEAR about it. The advisory may stay quiet; the clock may not stay
    // compressed. It goes real at the brake point, which for a truck with no
    // speed to shed is the reaction seconds alone.
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    street_chain(&mut d, 40.0, 0.5);
    // Under the corner's own advised speed, the way the keeper holds a
    // truck through a facility zone.
    mph(&mut d, 8.0);
    let cue = d.turn_cue_in_play().expect("a corner is in play");
    assert!(d.trip.truck.speed_mph() <= d.turn_speed_mph(&cue));
    assert!(d.trip.effective_time_scale() > 1.0);
    let brake_mi = d.turn_brake_point_mi(&cue);
    let real_s = brake_mi / 8.0 * 3600.0;
    assert!(
        real_s >= 8.0,
        "only {real_s:.1} real seconds to act on the corner"
    );

    d.trip.position_mi = cue.at_mi - brake_mi + 0.001;
    d.update_turn_commitment(&mut app.ctx, 0.016);

    assert!(d.trip.controlled_turn);
    assert_eq!(d.trip.effective_time_scale(), 1.0);
}

#[test]
fn test_a_now_corner_call_is_never_a_droppable_lead() {
    // A cold arrival at a corner gets the "now" form, and that is the
    // only instruction the driver will get for it. Dropped as stale behind the
    // handoff line it left the owner with nothing, twice on one arrival
    // (Spokane, 2026-08-22). A lead may go stale; a "now" call may not.
    //
    // Rust: the capture sink records what was spoken, not the priority the
    // call carried, so the priority is read off the pacer's own contract --
    // a ROUTE line is never dropped, an ambient lead is.
    let mut app = TestApp::new();
    // The two halves happen in the same microsecond of real time, and the
    // pacer drops a stale AMBIENT lead behind a line still in the air --
    // which is the very thing under test. Give it a clock the test moves.
    let clock = app.fake_pacer_clock();
    let mut d = a_drive(&mut app);
    a_street_chain(&mut d);
    app.clear_speech();

    // Arrive cold, 150 feet from the corner, over its speed.
    d.trip.position_mi = 0.6 - 150.0 / 5280.0; // the corner is at 0.6
    mph(&mut d, 30.0);
    let cue = d.turn_cue_in_play().expect("a corner is in play");
    let ahead = cue.at_mi - d.trip.position_mi;
    let now_call = d.turn_approach_text(&app.ctx, &cue, ahead);
    assert!(
        now_call.starts_with("Turn left now onto") || now_call.starts_with("Turn right now onto")
    );
    d.update_turn_commitment(&mut app.ctx, 0.016);
    assert_eq!(
        app.event_lines().last().expect("a corner call").as_str(),
        now_call
    );

    // A quarter mile out it is a lead, and a lead stays droppable.
    clock.advance(60.0);
    let mut d2 = a_drive(&mut app);
    a_street_chain(&mut d2);
    app.clear_speech();
    d2.trip.position_mi = 0.6 - 0.25;
    mph(&mut d2, 30.0);
    d2.update_turn_commitment(&mut app.ctx, 0.016);
    let message = app.event_lines().last().expect("a corner call").clone();
    assert!(!message.split(" onto ").next().unwrap_or("").contains("now"));
}

// -- pacenotes (tests/test_pacenotes.py) ---------------------------------------------

fn a_curve(start: f64, direction: char, advisory: i64, radius: i64, deflection: f64) -> RouteCurve {
    RouteCurve {
        start_mi: start,
        apex_mi: start + 0.05,
        end_mi: start + 0.1,
        direction,
        advisory_mph: advisory,
        min_radius_ft: radius,
        deflection_deg: deflection,
        connector: false,
    }
}

fn a_default_curve(start: f64) -> RouteCurve {
    a_curve(start, 'L', 35, 307, 60.0)
}

/// `_spoken_pacenotes`: install `curves`, set the speed, and run one frame's
/// curve events through the drive.
fn spoken_pacenotes(
    app: &mut TestApp,
    d: &mut DrivingState,
    curves: Vec<RouteCurve>,
    speed_mph: f64,
) -> Vec<String> {
    app.clear_speech();
    d.trip.curves = curves;
    d.trip.announced_curves.clear();
    d.trip.truck.velocity_mps = speed_mph * 0.44704;
    let events = d.trip.update(0.0);
    for event in events {
        if event.kind == TripEventKind::Curve {
            d.handle_trip_event(&mut app.ctx, &event);
        }
    }
    app.event_lines()
}

#[test]
fn test_shard_loads_mainline_curves_without_connectors() {
    let records = leg_curves("aberdeen_sd_us:pierre_sd_us", true);
    assert!(
        !records.is_empty(),
        "the baked shard should cover this swept leg"
    );
    assert!(records
        .iter()
        .all(|r| r.direction == 'L' || r.direction == 'R'));
    // Connector rows (interchange arcs) never reach the pacenote layer.
    assert!(records.iter().all(|r| r.advisory_mph > 0));
}

#[test]
fn test_route_curves_mirror_reverse_legs() {
    let world = get_world();
    let route = world
        .supported_route("aberdeen_sd_us", "pierre_sd_us", None)
        .expect("the world routes")
        .expect("a route");
    let reverse = world
        .supported_route("pierre_sd_us", "aberdeen_sd_us", None)
        .expect("the world routes")
        .expect("a route");
    let forward_curves = route_curves(&route, &route.cities, true);
    let reverse_curves = route_curves(&reverse, &reverse.cities, true);
    assert!(!forward_curves.is_empty() && !reverse_curves.is_empty());
    assert_eq!(forward_curves.len(), reverse_curves.len());
    // The same physical bend, met from the other end, turns the other way.
    let first = &forward_curves[0];
    let mirrored = &reverse_curves[reverse_curves.len() - 1];
    assert_ne!(first.direction, mirrored.direction);
    assert_eq!(first.advisory_mph, mirrored.advisory_mph);
    let total = route.miles();
    assert!((mirrored.start_mi - (total - first.end_mi)).abs() < 0.01);
}

#[test]
fn test_severity_ladder() {
    // A hairpin is a shape, not a speed: it comes back on itself (135 degrees,
    // MUTCD's Hairpin Curve sign) AND has to be crawled (a Turn sign, 30 or
    // less). Either alone is some other kind of bend -- see
    // test_a_hairpin_is_a_shape_not_a_speed in ff-core's curves module.
    assert_eq!(curve_severity(20, 170.0), "hairpin");
    assert_eq!(curve_severity(20, 60.0), "sharp");
    assert_eq!(curve_severity(45, 170.0), "moderate");
    assert_eq!(curve_severity(30, 60.0), "sharp");
    assert_eq!(curve_severity(45, 60.0), "moderate");
    assert_eq!(curve_severity(65, 60.0), "gentle");
}

#[test]
fn test_short_distance_text_units() {
    let s = Settings::default();
    assert_eq!(s.short_distance_text(0.25), "a quarter mile");
    assert_eq!(s.short_distance_text(0.5), "half a mile");
    assert_eq!(s.short_distance_text(0.74), "three quarters of a mile");
    assert_eq!(s.short_distance_text(1.0), "one mile");
    let m = Settings {
        imperial_units: false,
        ..Default::default()
    };
    assert_eq!(m.short_distance_text(0.25), "400 meters");
    assert_eq!(m.short_distance_text(1.0), "1.6 kilometers");
}

#[test]
fn test_pacenote_called_before_a_demanding_bend() {
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    let pos = d.trip.position_mi;
    let spoken = spoken_pacenotes(
        &mut app,
        &mut d,
        vec![a_curve(pos + 0.3, 'R', 30, 307, 60.0)],
        60.0,
    );
    assert!(!spoken.is_empty(), "a 30 mph bend at 60 mph demands a call");
    assert!(spoken[0].contains("Sharp right"));
    assert!(spoken[0].contains("Advise"));
    // The same frame never calls twice, and the next frame stays quiet.
    let events = d.trip.update(0.0);
    for event in events {
        if event.kind == TripEventKind::Curve {
            d.handle_trip_event(&mut app.ctx, &event);
        }
    }
    assert_eq!(app.event_lines().len(), 1);
}

#[test]
fn test_pacenote_stays_silent_when_already_slow() {
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    let pos = d.trip.position_mi;
    // A radius a 55 sign is really built on: 307 ft asked 0.54 g at 50,
    // past where any load goes over, and the call now prices the bend for
    // the load (bend sweep, 2026-09-24).
    let radius = min_radius_ft(55.0) as i64;
    let spoken = spoken_pacenotes(
        &mut app,
        &mut d,
        vec![a_curve(pos + 0.3, 'L', 55, radius, 60.0)],
        50.0,
    );
    assert!(
        spoken.is_empty(),
        "a bend you are already slow enough for stays silent"
    );
}

#[test]
fn test_gentle_bends_only_call_when_truly_hot() {
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    let pos = d.trip.position_mi;
    // Five over a gentle 60 sweep is chatter, not help: silent.
    let spoken = spoken_pacenotes(
        &mut app,
        &mut d,
        vec![a_curve(pos + 0.3, 'L', 60, 2400, 60.0)],
        65.0,
    );
    assert!(spoken.is_empty());
    // Twelve over the same sweep is genuinely hot: called.
    let spoken = spoken_pacenotes(
        &mut app,
        &mut d,
        vec![a_curve(pos + 0.3, 'L', 60, 2400, 60.0)],
        72.0,
    );
    assert!(!spoken.is_empty() && spoken[0].contains("Gentle bend"));
}

#[test]
fn test_pacenote_respects_the_setting() {
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    app.ctx.settings.curve_callouts = false;
    let pos = d.trip.position_mi;
    let spoken = spoken_pacenotes(
        &mut app,
        &mut d,
        vec![a_curve(pos + 0.3, 'L', 25, 307, 60.0)],
        60.0,
    );
    assert!(spoken.is_empty());
}

#[test]
fn test_curve_event_uses_the_documented_short_pacenote_wording() {
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    // The bare call: with curve speed assistance on (the default) the
    // approach servo appends its own clause, pinned by its own cases below.
    app.ctx.settings.curve_speed_assist = false;
    let pos = d.trip.position_mi;
    let spoken = spoken_pacenotes(
        &mut app,
        &mut d,
        vec![a_curve(pos + 0.3, 'R', 30, 307, 60.0)],
        60.0,
    );
    assert_eq!(
        spoken,
        ["Sharp right, a quarter mile. Advise 30 miles per hour."]
    );
}

#[test]
fn test_upcoming_curve_remains_eligible_after_resume() {
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    // The bare call, as above.
    app.ctx.settings.curve_speed_assist = false;
    app.clear_speech();
    let pos = d.trip.position_mi;
    let curve = a_curve(pos + 0.3, 'L', 30, 307, 60.0);
    d.trip.curves = vec![curve];
    d.trip.truck.velocity_mps = 60.0 * 0.44704;

    let minutes = d.trip.game_minutes;
    d.trip.restore(pos, minutes);
    let key = format!("curve:{:.3}:{}", curve.start_mi, curve.direction);
    assert!(!d.trip.announced_curves.contains(&key));

    let events = d.trip.update(0.0);
    for event in events {
        if event.kind == TripEventKind::Curve {
            d.handle_trip_event(&mut app.ctx, &event);
        }
    }
    assert_eq!(
        app.event_lines(),
        ["Sharp left, a quarter mile. Advise 30 miles per hour."]
    );
}

#[test]
fn test_safe_speed_folds_in_the_bend() {
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    // Out on the corridor: at mile zero the truck is still inside the
    // origin's own access zone, whose posted number is under the bend's.
    d.trip.position_mi = 5.0;
    app.clear_speech();
    let pos = d.trip.position_mi;
    d.trip.curves = vec![a_curve(pos + 0.2, 'L', 25, 307, 60.0)];
    d.speak_safe_speed(&mut app.ctx);
    let said = app.main_lines().last().expect("a safe-speed line").clone();
    assert!(said.contains("for the bend"), "{said}");
    assert!(said.contains("25"), "{said}");
}

#[test]
fn test_upcoming_lists_the_next_bends() {
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    app.clear_speech();
    let pos = d.trip.position_mi;
    d.trip.curves = vec![
        a_curve(pos + 2.0, 'L', 30, 307, 60.0),
        a_curve(pos + 4.0, 'R', 65, 307, 60.0), // gentle: stays out
    ];
    d.speak_upcoming(&mut app.ctx, 15.0);
    let text = app
        .main_lines()
        .last()
        .expect("an upcoming line")
        .to_lowercase();
    assert!(text.contains("sharp left"));
    assert!(text.contains("advise"));
    assert!(!text.contains("gentle"));
}

#[test]
fn test_close_curve_says_just_ahead() {
    // Sub-quarter-mile distances must never round UP to "a quarter mile":
    // the words promised time the driver did not have (AZ-260, 2026-07-19).
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    let pos = d.trip.position_mi;
    let spoken = spoken_pacenotes(&mut app, &mut d, vec![a_default_curve(pos + 0.05)], 50.0);
    assert!(!spoken.is_empty() && spoken[0].contains("just ahead"));
    assert!(!spoken[0].contains("quarter"));
}

#[test]
fn test_silenced_curve_call_respeaks_once_refreshed() {
    // Ctrl must silence instantly (screen-reader reflex), but a safety call
    // cut mid-sentence re-speaks once with a fresh distance -- and only
    // while the bend is still ahead and the truck still hot.
    let mut app = TestApp::new();
    // The refreshed call repeats the same words from the same milepost, and
    // the pacer suppresses a line the player heard a moment ago -- a moment
    // that is microseconds long here. The clock is the test's to move.
    let clock = app.fake_pacer_clock();
    let mut d = a_drive(&mut app);
    let pos = d.trip.position_mi;
    let spoken = spoken_pacenotes(
        &mut app,
        &mut d,
        vec![a_curve(pos + 0.3, 'R', 30, 307, 60.0)],
        60.0,
    );
    assert_eq!(spoken.len(), 1);

    d.handle_key_event(&mut app.ctx, &InputEvent::key(Key::LCtrl)); // the reflex
    clock.advance(60.0);
    d.update_critical_respeak(&mut app.ctx, 2.5); // past the re-speak delay
    let after = app.event_lines();
    assert_eq!(after.len(), 2);
    assert!(after[1].contains("Sharp right"));

    // One shot only: another Ctrl after the re-arm is spent stays quiet.
    d.handle_key_event(&mut app.ctx, &InputEvent::key(Key::LCtrl));
    clock.advance(60.0);
    d.update_critical_respeak(&mut app.ctx, 2.5);
    assert_eq!(app.event_lines().len(), 2);

    // And a call silenced AFTER braking below the advisory stays quiet.
    d.trip.announced_curves.clear();
    let spoken2 = spoken_pacenotes(
        &mut app,
        &mut d,
        vec![a_curve(pos + 0.3, 'R', 30, 307, 60.0)],
        60.0,
    );
    assert_eq!(spoken2.len(), 1);
    d.trip.truck.velocity_mps = 25.0 * 0.44704;
    d.handle_key_event(&mut app.ctx, &InputEvent::key(Key::LCtrl));
    clock.advance(60.0);
    d.update_critical_respeak(&mut app.ctx, 2.5);
    assert_eq!(app.event_lines().len(), 1);
}

#[test]
fn test_linked_follower_rides_the_tail_not_its_own_call() {
    // Owner's Payson run (2026-07-19): "Then right" was a preview, not a
    // replacement -- every linked bend also fired its own full call and
    // chained S-bends flooded the driver. One chain, one call.
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    let pos = d.trip.position_mi;
    let first = a_curve(pos + 0.3, 'L', 35, 307, 60.0);
    let follower = a_curve(pos + 0.45, 'R', 25, 307, 170.0);
    let spoken = spoken_pacenotes(&mut app, &mut d, vec![first, follower], 60.0);
    assert_eq!(spoken.len(), 1);
    // The tail carries the follower's severity and tighter advisory.
    assert!(spoken[0].contains("Then hairpin right, advise 25 miles per hour."));

    // The follower stays suppressed on later frames too.
    let events = d.trip.update(0.0);
    for event in events {
        if event.kind == TripEventKind::Curve {
            d.handle_trip_event(&mut app.ctx, &event);
        }
    }
    assert_eq!(app.event_lines().len(), 1);
}

#[test]
fn test_a_curve_call_carries_its_own_expiry() {
    // A cut curve call is offered back once; it must still be true by then.
    //
    // Shane P heard the curve call and its cruise clause come back repeatedly
    // through one bend (2026-08-21). Capping the rescue at one stopped the
    // pile-up, but the survivor was still ungated: replayed a second later it
    // can name a corner the truck is already through, or tell a driver who has
    // just braked to brake. Every curve call now hands the rescue a test.
    //
    // Rust: the predicate cannot ride a 'static closure, so the gate carries
    // the bend's own numbers and reads the truck through `live`. It is built
    // ONCE here, where the call is made, and asked at all three moments --
    // which is the point of the shape: an answer taken at the call is true by
    // construction, and a gate that cannot move never refuses anything.
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    app.ctx.settings.curve_speed_assist = true;
    let pos = d.trip.position_mi;
    let curve = a_curve(pos + 0.3, 'R', 40, 307, 60.0);

    for cruise_mph in [None, Some(65.0)] {
        d.trip.position_mi = pos;
        d.trip.truck.velocity_mps = 65.0 * 0.44704;
        d.cruise_mph = cruise_mph;
        let gate = d
            .curve_call_still_true(Some(&curve))
            .expect("a curve to gate");
        d.refresh_live_facts();
        assert!(gate.holds());

        // Braked for the bend: the rescue has nothing left to say.
        d.trip.truck.velocity_mps = 35.0 * 0.44704;
        d.refresh_live_facts();
        assert!(!gate.holds());

        // Or through it at speed, which is the miserable one to hear.
        d.trip.truck.velocity_mps = 65.0 * 0.44704;
        d.trip.position_mi = curve.end_mi + 0.05;
        d.refresh_live_facts();
        assert!(!gate.holds());
    }
    // No curve at all leaves the rescue ungated, exactly as before.
    assert!(d.curve_call_still_true(None).is_none());
}

#[test]
fn test_curve_assist_holds_the_tightest_speed_in_a_linked_chain() {
    // Darren, 2026-08-23, load damaged 12 percent on NY-12.
    //
    // His log has the words and the machine disagreeing:
    //
    //     Curve left, a quarter mile. Advise 40 miles per hour. Then sharp
    //     left, advise 30 miles per hour. Adaptive cruise easing to 40 miles
    //     per hour for the bend.
    //     ...
    //     Sharp left: too fast, drifting to the outside.
    //     The load has shifted hard and is damaged, 12 percent.
    //
    // The tail is the follower's ONLY call -- the trip suppresses its own so a
    // chain speaks once -- so easing to the first bend's 40 and releasing the
    // cap at the first bend's end carried the truck into a 30 mph bend at 40
    // with nothing left to warn it. The spoken line named 30 the whole way.
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    app.ctx.settings.curve_speed_assist = true;
    let pos = d.trip.position_mi;
    let lead = a_curve(pos + 0.3, 'L', 40, 307, 60.0);
    // Close enough behind to ride the tail rather than earn its own call.
    let follower = a_curve(lead.end_mi + 0.1, 'L', 30, 307, 60.0);

    d.cruise_mph = Some(60.0);
    let spoken = spoken_pacenotes(&mut app, &mut d, vec![lead, follower], 60.0);

    assert!(!spoken.is_empty(), "a 40 mph bend at 60 demands a call");
    assert!(
        spoken[0].contains("Then"),
        "the follower rides the tail, so it has no call of its own"
    );

    // The cap is the chain's tightest number, not the first bend's...
    assert_eq!(d.cruise_curve_mph, Some(30.0));
    // ...and it holds until the FOLLOWER is behind, not the lead.
    assert!(d.cruise_curve_end_mi.expect("a cap was set") >= follower.end_mi);
}

#[test]
fn test_curve_speed_assistance_still_acts_with_curve_callouts_off() {
    // The owner drives with curve callouts off and curve speed assistance on.
    // The handler used to return at the callout switch, before the cruise
    // easing, so the assist never touched the truck: cruise carried it into a
    // 35 mph bend at 90 km/h and the load shifted 31 percent over two bends
    // (agent drive, 2026-09-01). Words off; assist on.
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    app.ctx.settings.curve_speed_assist = true;
    app.ctx.settings.curve_callouts = false;
    let pos = d.trip.position_mi;
    let bend = a_curve(pos + 0.3, 'L', 40, 307, 60.0);
    d.cruise_mph = Some(60.0);
    let spoken = spoken_pacenotes(&mut app, &mut d, vec![bend], 60.0);
    assert!(spoken.is_empty(), "callouts off means no words: {spoken:?}");
    assert_eq!(d.cruise_curve_mph, Some(40.0), "but the assist still eases");
}

#[test]
fn test_a_too_tight_bend_still_pauses_cruise_with_curve_callouts_off() {
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    app.ctx.settings.curve_speed_assist = true;
    app.ctx.settings.curve_callouts = false;
    d.speed_control_armed = true;
    let pos = d.trip.position_mi;
    let bend = a_curve(pos + 0.3, 'L', 15, 197, 60.0);
    d.cruise_mph = Some(60.0);
    let spoken = spoken_pacenotes(&mut app, &mut d, vec![bend], 60.0);
    assert!(spoken.is_empty(), "callouts off means no words: {spoken:?}");
    assert!(d.cruise_mph.is_none(), "cruise let go for the bend");
    assert!(d.speed_control_armed, "but the session survives");
    assert!(
        d.cruise_resume_after_mi.is_some(),
        "and knows where to come back"
    );
}

#[test]
fn test_a_lone_curve_still_holds_only_its_own_speed() {
    // The chain rule must not make every bend the slowest bend nearby.
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    app.ctx.settings.curve_speed_assist = true;
    let pos = d.trip.position_mi;
    let alone = a_curve(pos + 0.3, 'R', 40, 307, 60.0);
    d.cruise_mph = Some(60.0);

    spoken_pacenotes(&mut app, &mut d, vec![alone], 60.0);

    assert_eq!(d.cruise_curve_mph, Some(40.0));
    assert_eq!(
        d.cruise_curve_end_mi,
        Some(alone.start_mi.max(alone.end_mi))
    );
}

// -- a bend under cruise's floor pauses the session ------------------------------------

/// The first open-road mile of the corridor: no zone, a real limit, so the
/// resume path reaches for adaptive cruise rather than the speed keeper.
fn open_road_mile(d: &mut DrivingState) -> f64 {
    let total = d.trip.total_miles();
    let mut mile = 5.0;
    while mile < total - 5.0 {
        let (limit, reason) = d.trip.speed_limit_at(mile);
        if reason.is_none() && limit >= 45.0 {
            return mile;
        }
        mile += 0.5;
    }
    panic!("this corridor has no open-road mile for the cruise cases to sit on");
}

/// Adaptive cruise armed at 60 on open road, meeting a bend advised at 15 --
/// under the 20 cruise can hold at all, so the easing branch cannot take it.
/// Returns the bend and the lines spoken for it.
fn cruise_meets_a_bend_too_tight_to_ease(
    app: &mut TestApp,
    d: &mut DrivingState,
) -> (RouteCurve, Vec<String>) {
    app.ctx.settings.curve_speed_assist = true;
    d.trip.position_mi = open_road_mile(d);
    d.trip.truck.set_air_ready(false);
    d.trip.truck.start_engine();
    d.trip.truck.brake = 0.0;
    d.trip.truck.velocity_mps = 60.0 * 0.44704;
    d.engage_cruise(&mut app.ctx, 60.0, false);
    assert!(d.speed_control_armed);
    assert_eq!(d.cruise_mph, Some(60.0));
    let tight = a_curve(d.trip.position_mi + 0.3, 'R', 15, 120, 120.0);
    let spoken = spoken_pacenotes(app, d, vec![tight], 60.0);
    (tight, spoken)
}

#[test]
fn test_a_bend_under_cruises_floor_pauses_the_session_instead_of_dropping_it() {
    // Owner ruling, 2026-09-01: on the highway a hazard or a curve must not
    // switch adaptive cruise off; it pauses and comes back. The hazard cancels
    // were converted that day; this was the last disarm left, and the truck
    // coasted out of the bend with the session gone.
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    let (tight, spoken) = cruise_meets_a_bend_too_tight_to_ease(&mut app, &mut d);

    assert!(!spoken.is_empty(), "a 15 mph bend at 60 demands a call");
    assert!(
        spoken[0].contains("Adaptive cruise paused for the bend"),
        "{spoken:#?}"
    );
    assert!(
        !spoken
            .iter()
            .any(|line| line.contains("Adaptive cruise off")),
        "{spoken:#?}"
    );
    // Cruise is off the pedal for the bend, the session is not over, and the
    // set speed is remembered for the resume.
    assert!(d.cruise_mph.is_none());
    assert!(d.speed_control_armed);
    assert_eq!(d.speed_control_target_mph, Some(60.0));
    // The pause lifts past the bend's end plus the commit tail, so a resume
    // can never land mid-corner.
    let end = tight.start_mi.max(tight.end_mi);
    assert_eq!(d.cruise_resume_after_mi, Some(end + TURN_COMMIT_TAIL_MI));
}

#[test]
fn test_the_curve_pause_does_not_resume_inside_the_bend() {
    // Rolling, off the brakes, at road speed -- every other resume condition
    // met -- but still short of the resume mile: nothing re-engages.
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    let (tight, _) = cruise_meets_a_bend_too_tight_to_ease(&mut app, &mut d);
    app.clear_speech();

    d.trip.position_mi = tight.apex_mi;
    d.trip.truck.brake = 0.0;
    d.trip.truck.velocity_mps = 60.0 * 0.44704;
    d.resume_speed_control_if_ready(&mut app.ctx, false);

    assert!(d.cruise_mph.is_none(), "cruise re-engaged mid-bend");
    assert!(d.speed_control_armed);
    assert!(d.cruise_resume_after_mi.is_some());
    assert!(
        app.speech().lines().is_empty(),
        "{:#?}",
        app.speech().lines()
    );
}

#[test]
fn test_the_curve_pause_resumes_at_the_remembered_target_past_the_bend() {
    // Past the resume mile the existing resume re-engages on its own, at the
    // target the driver set, and announces it the way it does after a hazard.
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    let (tight, _) = cruise_meets_a_bend_too_tight_to_ease(&mut app, &mut d);
    app.clear_speech();

    let resume_mi = d.cruise_resume_after_mi.expect("a resume mile");
    assert!(resume_mi > tight.end_mi);
    d.trip.position_mi = resume_mi + 0.01;
    d.trip.truck.brake = 0.0;
    d.trip.truck.velocity_mps = 45.0 * 0.44704;
    d.resume_speed_control_if_ready(&mut app.ctx, false);

    assert_eq!(d.cruise_mph, Some(60.0));
    assert!(d.speed_control_armed);
    assert!(d.cruise_resume_after_mi.is_none());
    let lines = app.speech().lines();
    assert!(
        lines
            .iter()
            .any(|line| line.contains("Cruise resuming, 60")),
        "{lines:#?}"
    );
}

#[test]
fn test_a_manual_k_during_the_curve_pause_disarms_and_forgets_the_resume_mile() {
    // The driver's own K still means "off", exactly as today -- and it takes
    // the pending resume with it, so nothing re-engages a session they just
    // switched off.
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    let (tight, _) = cruise_meets_a_bend_too_tight_to_ease(&mut app, &mut d);
    app.clear_speech();

    d.trip.position_mi = tight.apex_mi;
    d.toggle_cruise(&mut app.ctx);

    assert!(!d.speed_control_armed);
    assert!(d.cruise_mph.is_none());
    assert!(d.cruise_resume_after_mi.is_none());
    assert!(app
        .speech()
        .lines()
        .iter()
        .any(|line| line.contains("Automatic speed control off.")));

    // And with the session gone, the road past the bend brings nothing back.
    d.trip.position_mi = tight.end_mi + TURN_COMMIT_TAIL_MI + 0.01;
    d.trip.truck.velocity_mps = 45.0 * 0.44704;
    d.resume_speed_control_if_ready(&mut app.ctx, false);
    assert!(d.cruise_mph.is_none());
    assert!(!d.speed_control_armed);
}

// -- the place keys (tests/test_driving_place_keys.py) --------------------------------

fn a_village(name: &str, at_mi: f64, off_mi: f64) -> Landmark {
    Landmark {
        name: name.to_string(),
        at_mi,
        category: "village".to_string(),
        kind: "point".to_string(),
        spoken: format!("Passing {name}"),
        off_mi,
        directions: vec!["both".to_string()],
    }
}

fn a_route_town(name: &str, at_mi: f64) -> RouteCheckpoint {
    RouteCheckpoint {
        name: name.to_string(),
        at_mi,
        checkpoint_type: "place".to_string(),
        state: "New York".to_string(),
        highway: "I-90".to_string(),
        ..Default::default()
    }
}

/// Replace the curated route towns on the leg the truck is currently
/// driving (the villages are cleared with them).
fn set_leg_checkpoints(d: &mut DrivingState, checkpoints: Vec<RouteCheckpoint>) {
    let (index, _) = d.trip.leg_at_mile(d.trip.position_mi);
    let old = d.trip.route.legs[index].clone();
    let detail = CorridorDetail {
        checkpoints,
        ..Default::default()
    };
    let leg = Leg::new(
        &old.a,
        &old.b,
        old.miles,
        &old.highway,
        &old.terrain,
        Vec::new(),
    )
    .with_detail(detail);
    d.trip.route.legs[index] = Arc::new(leg);
}

/// `_set_leg_landmarks`: replace the landmarks on the leg the truck is
/// currently driving.
fn set_leg_landmarks(d: &mut DrivingState, landmarks: Vec<Landmark>) {
    let (index, _) = d.trip.leg_at_mile(d.trip.position_mi);
    let old = d.trip.route.legs[index].clone();
    let detail = CorridorDetail {
        landmarks,
        ..Default::default()
    };
    let leg = Leg::new(
        &old.a,
        &old.b,
        old.miles,
        &old.highway,
        &old.terrain,
        Vec::new(),
    )
    .with_detail(detail);
    d.trip.route.legs[index] = Arc::new(leg);
}

/// The truck's offset in the current leg's own native frame.
fn native_offset(d: &DrivingState) -> (f64, bool) {
    let (index, start) = d.trip.leg_at_mile(d.trip.position_mi);
    let leg = &d.trip.route.legs[index];
    let forward = d.trip.route.cities[index] == leg.a;
    let offset = d.trip.position_mi - start;
    (if forward { offset } else { leg.miles - offset }, forward)
}

#[test]
fn test_each_alt_number_speaks_one_place_fact() {
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    app.clear_speech();

    d.handle_key_event(&mut app.ctx, &alt(Key::Num1));
    let state = app.main_lines().last().expect("a state line").clone();
    assert!(state.starts_with("In "));

    d.handle_key_event(&mut app.ctx, &alt(Key::Num2));
    let road = app.main_lines().last().expect("a road line").clone();
    assert!(road.starts_with("On "));
    assert_ne!(road, state);

    d.handle_key_event(&mut app.ctx, &alt(Key::Num3));
    assert!(!app.main_lines().last().expect("a town line").is_empty());

    d.handle_key_event(&mut app.ctx, &alt(Key::Num4));
    let direction = app.main_lines().last().expect("a direction line").clone();
    assert!(direction.ends_with("bound.") || direction.contains("No signed direction"));

    // Four presses, four answers, and each one is a single sentence --
    // the whole point of the keys is that they are shorter than R.
    let spoken = app.main_lines();
    assert_eq!(spoken.len(), 4);
    for line in &spoken {
        assert!(line.matches('.').count() <= 2, "{line}");
    }
}

#[test]
fn test_alt_with_a_number_does_not_touch_the_engine_brake() {
    // The collision that made these keys unsafe before they existed.
    //
    // Alt+1 used to fall through to the jake-stage branch, so a driver asking
    // what state they were in changed the engine brake instead.
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    d.trip.truck.start_engine();
    d.trip.truck.engine_brake_stage = 3;
    d.jake_selected_stage = 3;

    for k in [Key::Num1, Key::Num2, Key::Num3] {
        d.handle_key_event(&mut app.ctx, &alt(k));
        assert_eq!(d.trip.truck.engine_brake_stage, 3);
    }

    // Without Alt the stages still work exactly as they did.
    d.handle_key_event(&mut app.ctx, &InputEvent::key(Key::Num2));
    assert_eq!(d.trip.truck.engine_brake_stage, 2);
}

#[test]
fn test_town_key_names_the_town_the_truck_is_in() {
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    // Mid-leg, clear of both cities: the leg's own cities are towns too.
    let (index, start) = d.trip.leg_at_mile(d.trip.position_mi);
    d.trip.position_mi = start + d.trip.route.legs[index].miles / 2.0;
    app.clear_speech();
    let (native, _forward) = native_offset(&d);
    // A village on the road, right where the truck is: that is the town
    // the driver is in, not one they can see.
    set_leg_landmarks(&mut d, vec![a_village("Pine", native, 0.1)]);
    d.speak_current_town(&mut app.ctx);
    assert_eq!(app.main_lines().last().expect("a town line"), "In Pine.");
}

#[test]
fn test_town_key_places_a_town_off_the_road() {
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    // Mid-leg, clear of both cities: the leg's own cities are towns too.
    let (index, start) = d.trip.leg_at_mile(d.trip.position_mi);
    d.trip.position_mi = start + d.trip.route.legs[index].miles / 2.0;
    app.clear_speech();
    let (native, forward) = native_offset(&d);
    let ahead = if forward { native + 4.0 } else { native - 4.0 };
    set_leg_landmarks(&mut d, vec![a_village("Fairfield", ahead, 6.3)]);
    d.speak_current_town(&mut app.ctx);
    // Only the name: the distance off the road was filler (owner, 2026-10-12).
    assert_eq!(
        app.main_lines().last().expect("a town line"),
        "Nearest town: Fairfield."
    );
}

#[test]
fn test_town_key_says_so_when_there_is_no_town() {
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    app.clear_speech();
    set_leg_landmarks(&mut d, Vec::new());
    // Well clear of both ends of the leg: the leg's own cities are towns too.
    let (index, start) = d.trip.leg_at_mile(d.trip.position_mi);
    let miles = d.trip.route.legs[index].miles;
    d.trip.position_mi = start + miles / 2.0;
    assert!(
        miles / 2.0 > NEAREST_TOWN_MI,
        "the Buffalo to Rochester leg is {miles} miles"
    );
    d.speak_current_town(&mut app.ctx);
    assert_eq!(
        app.main_lines().last().expect("a town line"),
        "No town near here."
    );
}

#[test]
fn test_town_key_names_the_route_town_just_passed() {
    // "Passing Batavia, New York on I-90" is a curated route town, not a
    // baked village, and Alt+3 right after it used to say there was no
    // town (owner, 2026-09-16).
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    // Mid-leg, clear of both cities: the leg's own cities are towns too.
    let (index, start) = d.trip.leg_at_mile(d.trip.position_mi);
    d.trip.position_mi = start + d.trip.route.legs[index].miles / 2.0;
    app.clear_speech();
    let (native, forward) = native_offset(&d);
    let behind = if forward { native - 0.5 } else { native + 0.5 };
    set_leg_checkpoints(&mut d, vec![a_route_town("Batavia", behind)]);
    d.speak_current_town(&mut app.ctx);
    assert_eq!(app.main_lines().last().expect("a town line"), "In Batavia.");

    // Once it is a few miles back it is still the nearest town, not forgotten.
    app.clear_speech();
    let back = if forward { native - 4.0 } else { native + 4.0 };
    set_leg_checkpoints(&mut d, vec![a_route_town("Batavia", back)]);
    d.speak_current_town(&mut app.ctx);
    assert_eq!(
        app.main_lines().last().expect("a town line"),
        "Nearest town: Batavia."
    );
}

#[test]
fn test_town_key_names_the_city_the_leg_starts_in() {
    // At the start of the leg the truck is in the leg's own city, whether
    // or not the corridor bakes a village there.
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    app.clear_speech();
    set_leg_landmarks(&mut d, Vec::new());
    let (index, start) = d.trip.leg_at_mile(d.trip.position_mi);
    d.trip.position_mi = start + 0.2;
    let city = app
        .ctx
        .world
        .spoken_city(&d.trip.route.cities[index], Some(false));
    d.speak_current_town(&mut app.ctx);
    assert_eq!(
        app.main_lines().last().expect("a town line"),
        &format!("In {city}.")
    );
}

#[test]
fn test_place_keys_stay_honest_on_city_streets() {
    // A street chain has a street and a city but no shield and no heading.
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    a_street_chain(&mut d);
    d.surface_chain = true;
    app.clear_speech();

    d.speak_current_direction(&mut app.ctx);
    assert!(app
        .main_lines()
        .last()
        .expect("a direction line")
        .contains("No signed direction here."));

    d.speak_current_state(&mut app.ctx);
    let state = app.main_lines().last().expect("a state line").clone();
    assert!(state.starts_with("In "));
    assert!(!state.contains("None"));

    d.speak_current_town(&mut app.ctx);
    assert!(app
        .main_lines()
        .last()
        .expect("a town line")
        .starts_with("In "));
}

#[test]
fn test_keypad_numbers_answer_the_same_way() {
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    app.clear_speech();

    d.handle_key_event(&mut app.ctx, &alt(Key::Num1));
    d.handle_key_event(&mut app.ctx, &alt(Key::Kp1));
    let said = app.main_lines();
    assert_eq!(said[said.len() - 1], said[said.len() - 2]);
}

// -- the route report (states/driving_location.rs) ------------------------------------

#[test]
fn test_route_status_leads_with_progress_then_places_the_truck() {
    // `_speak_route_status`: progress leads so a one-line braille display gets
    // it without panning, then the road, the state, and where it is heading.
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    app.clear_speech();
    d.speak_route_status(&mut app.ctx);
    let said = app.main_lines().last().expect("a route status").clone();
    assert!(said.contains(" percent there, "));
    assert!(said.contains(" On "));
    assert!(said.contains(", toward "));
}

#[test]
fn test_route_status_on_a_street_chain_answers_with_the_gate() {
    // On the facility approach the highway framing is a lie: the driver heard
    // "on I-90 West, 3 miles remaining" with a frozen countdown while rolling
    // city streets toward the gate (playtest 2026-07-22).
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    a_street_chain(&mut d);
    d.surface_chain = true;
    app.clear_speech();
    d.speak_route_status(&mut app.ctx);
    let said = app.main_lines().last().expect("a route status").clone();
    assert!(said.starts_with("On city streets, "));
    assert!(said.contains(" to the gate at "));
}

#[test]
fn test_a_closing_distance_is_never_spoken_as_zero() {
    use freight_fate::states::driving_location::spoken_closing_distance;
    // `Trip::distance_text` rounds to whole units, so anything under half a
    // mile spoke as "0 miles" (owner report, 2026-08-15).
    assert_eq!(spoken_closing_distance(0.0, true), "50 feet");
    assert!(spoken_closing_distance(0.02, true).contains("feet"));
    assert_eq!(spoken_closing_distance(0.5, true), "half a mile");
    assert!(spoken_closing_distance(0.02, false).contains("meters"));
}

// -- the approach servo: curve speed assistance in every driving mode ------------------
//
// Owner ruling, 2026-09-01: "assists should handle curves better to avoid
// load/cargo shifting damage." Live that night, every assist on, adaptive
// cruise at 90 km/h carried the truck into a 35 mph left bend on US-83 near
// Junction, Texas -- "Sharp left: too fast, drifting to the outside" -- and
// the load shifted 12, then 31 percent over two bends. These drive REAL
// frames through a staged bend and read the speed the truck actually carries
// across its start, the drift line, and the cargo.

const DT: f64 = 1.0 / 60.0;

/// What a drive through one bend measured.
struct BendRun {
    /// The speed the truck crossed the bend's start at, mph.
    speed_at_start_mph: f64,
    /// The worst the load got, percent.
    cargo_damage_pct: f64,
    /// Every event line spoken from the call to the far side of the bend.
    lines: Vec<String>,
}

#[test]
fn test_curve_fixture_stays_empty_after_traffic_update() {
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);

    a_hot_bend_ahead(&mut app, &mut d, 60.0, 35, 307, 0.5);
    assert!(
        d.trip.traffic_manager.vehicles.is_empty(),
        "the bend fixture should start with no traffic"
    );

    d.trip.update(DT);

    assert!(
        d.trip.traffic_manager.vehicles.is_empty(),
        "updating the empty-road bend fixture replenished traffic"
    );
}

/// The engine running, air up, rolling at `speed_mph` on the corridor's first
/// open-road mile, curve speed assistance on, with a bend of `advisory` and
/// `radius` set `ahead_mi` up the road.
fn a_hot_bend_ahead(
    app: &mut TestApp,
    d: &mut DrivingState,
    speed_mph: f64,
    advisory: i64,
    radius: i64,
    ahead_mi: f64,
) -> RouteCurve {
    app.ctx.settings.curve_speed_assist = true;
    d.trip.traffic_manager.rolling_bubble = false;
    d.trip.position_mi = open_road_mile(d);
    d.trip.truck.set_air_ready(false);
    d.trip.truck.start_engine();
    d.trip.truck.brake = 0.0;
    d.trip.truck.velocity_mps = speed_mph * 0.44704;
    let bend = a_curve(d.trip.position_mi + ahead_mi, 'L', advisory, radius, 60.0);
    d.trip.curves = vec![bend];
    d.trip.announced_curves.clear();
    app.clear_speech();
    bend
}

/// Roll real frames from here to past the bend's commit tail, `each_frame`
/// getting a turn before every frame for the driver's own pedals.
fn drive_through_the_bend(
    app: &mut TestApp,
    d: &mut DrivingState,
    bend: &RouteCurve,
    clock: &freight_fate::app::testing::FakeClock,
    mut each_frame: impl FnMut(&mut TestApp, &mut DrivingState),
) -> BendRun {
    let mut speed_at_start_mph = None;
    let mut cargo_damage_pct: f64 = 0.0;
    let until_mi = bend.end_mi + TURN_COMMIT_TAIL_MI + 0.05;
    // Two real minutes is far longer than half a mile at road speed takes;
    // a run that needs more of the clock has stalled and the loop says so.
    for _ in 0..(120.0 / DT) as usize {
        if d.trip.position_mi >= until_mi {
            break;
        }
        each_frame(app, d);
        app.ctx.input.begin_frame(DT);
        d.update_frame(&mut app.ctx, DT);
        clock.advance(DT);
        if speed_at_start_mph.is_none() && d.trip.position_mi >= bend.start_mi {
            speed_at_start_mph = Some(d.trip.truck.speed_mph());
        }
        cargo_damage_pct = cargo_damage_pct.max(d.trip.truck.cargo_damage_pct);
    }
    BendRun {
        speed_at_start_mph: speed_at_start_mph.expect("the truck never reached the bend"),
        cargo_damage_pct,
        lines: app.event_lines(),
    }
}

/// Whether the bend's too-fast warning was spoken: the line that comes before
/// the bend costs the load or the lane anything (`driving_rollover`).
fn drifted(run: &BendRun) -> bool {
    run.lines
        .iter()
        .any(|line| line.contains(", too fast. Slow to"))
}

#[test]
fn test_cruise_into_a_hot_bend_arrives_at_the_advisory() {
    // (a) Adaptive cruise at 60, a 35 mph bend half a mile out -- the owner's
    // US-83 case in the harness. The truck is at the advisory by the bend's
    // start, never drifts, and the load never moves.
    let mut app = TestApp::new();
    let clock = app.fake_pacer_clock();
    let mut d = a_drive(&mut app);
    let bend = a_hot_bend_ahead(&mut app, &mut d, 60.0, 35, 307, 0.5);
    d.engage_cruise(&mut app.ctx, 60.0, false);
    assert_eq!(d.cruise_mph, Some(60.0));
    app.clear_speech();

    // Cruise's own closing brake does nearly all of this shed (measured
    // 2026-09-01: 60 to 38 in the first tenth of a mile on its cap alone);
    // the servo behind it is a backstop, and reads as one.
    let mut servo_max: f64 = 0.0;
    let run = drive_through_the_bend(&mut app, &mut d, &bend, &clock, |_, d| {
        servo_max = servo_max.max(d.curve_servo.as_ref().map_or(0.0, |s| s.brake));
    });
    // A light trim, not a stop. It read under 0.12 while the gearbox's torque
    // interruption ran on the real clock, and only because the box hunted
    // the truck down to 25 for a 35 bend; arriving on the number now, the
    // servo trims the last of it at about 0.15 (2026-09-23).
    assert!(
        servo_max < 0.2,
        "cruise makes the bend on its own; the servo should barely touch the pedal: {servo_max:.2}"
    );

    assert!(
        run.speed_at_start_mph >= 35.0 - 5.0,
        "shed far past the advisory, to {:.1} mph",
        run.speed_at_start_mph
    );
    assert!(
        run.speed_at_start_mph <= 35.0 + 2.0,
        "crossed the bend's start at {:.1} mph: {:#?}",
        run.speed_at_start_mph,
        run.lines
    );
    assert!(!drifted(&run), "{:#?}", run.lines);
    assert_eq!(
        run.cargo_damage_pct, 0.0,
        "the load moved: {:#?}",
        run.lines
    );
    // Cruise's easing line is the one call; the assist rides it silently and
    // says nothing on its own.
    assert!(
        run.lines
            .iter()
            .any(|line| line.contains("Adaptive cruise easing to 35 miles per hour.")),
        "{:#?}",
        run.lines
    );
    assert!(
        !run.lines
            .iter()
            .any(|line| line.contains("Curve assistance")),
        "cruise's line covers the bend; the assist must not speak twice: {:#?}",
        run.lines
    );
    // And the servo let go past the tail.
    assert!(d.curve_servo.is_none());
}

#[test]
fn test_a_manual_driver_off_the_pedals_is_braked_to_the_advisory() {
    // (b) The same bend with cruise OFF and no pedals: a manual driver who
    // heard the call and did nothing. Before tonight nothing proactive
    // happened at all; now the assist takes the brakes on the approach, and
    // the call says so in the same breath.
    let mut app = TestApp::new();
    let clock = app.fake_pacer_clock();
    let mut d = a_drive(&mut app);
    let bend = a_hot_bend_ahead(&mut app, &mut d, 60.0, 35, 307, 0.5);
    assert!(d.cruise_mph.is_none());

    let run = drive_through_the_bend(&mut app, &mut d, &bend, &clock, |_, _| {});

    assert!(
        run.speed_at_start_mph <= 35.0 + 2.0,
        "crossed the bend's start at {:.1} mph: {:#?}",
        run.speed_at_start_mph,
        run.lines
    );
    assert!(!drifted(&run), "{:#?}", run.lines);
    assert_eq!(run.cargo_damage_pct, 0.0);
    let call = run
        .lines
        .iter()
        .find(|line| line.contains("left, half a mile"))
        .unwrap_or_else(|| panic!("a curve call: {:#?}", run.lines));
    assert!(
        call.ends_with("Advise 35 miles per hour. Curve assistance slowing."),
        "one utterance, the pacenote plus the assist clause: {call:?}"
    );
    // The reactive line inside the bend is the bare sentence; the servo owns
    // this bend, so it never fires on top of the approach clause.
    assert!(
        !run.lines.iter().any(|line| line == "Slowing for the bend."),
        "the reactive line must not double the approach line: {:#?}",
        run.lines
    );
    assert!(d.curve_servo.is_none());
}

#[test]
fn test_a_bend_under_cruises_floor_is_braked_down_and_cruise_comes_back() {
    // (c) Advisory 15, under the 20 cruise can hold: cruise pauses as it did
    // tonight, but the servo now brings the truck down to 15 instead of
    // leaving the bend to the driver, and the existing resume brings cruise
    // back past it.
    let mut app = TestApp::new();
    let clock = app.fake_pacer_clock();
    let mut d = a_drive(&mut app);
    let bend = a_hot_bend_ahead(&mut app, &mut d, 60.0, 15, 120, 0.9);
    d.engage_cruise(&mut app.ctx, 60.0, false);
    app.clear_speech();

    let mut paused_in_the_bend = false;
    let run = drive_through_the_bend(&mut app, &mut d, &bend, &clock, |_, d| {
        if d.trip.position_mi >= d.trip.curves[0].start_mi
            && d.trip.position_mi <= d.trip.curves[0].end_mi
            && d.cruise_mph.is_none()
        {
            paused_in_the_bend = true;
        }
    });

    assert!(
        run.speed_at_start_mph <= 15.0 + 2.0,
        "crossed the bend's start at {:.1} mph: {:#?}",
        run.speed_at_start_mph,
        run.lines
    );
    assert!(!drifted(&run), "{:#?}", run.lines);
    assert_eq!(run.cargo_damage_pct, 0.0);
    assert!(paused_in_the_bend, "cruise stayed on through the bend");
    let call = run
        .lines
        .iter()
        .find(|line| line.contains("Adaptive cruise paused for the bend"))
        .expect("the pause line");
    assert!(
        call.contains("curve assistance slowing. Cruise resumes past the bend"),
        "{call:?}"
    );
    // Past the tail the pause is spent, and once the driver has the truck
    // back up to road speed the session comes back on its own, at the set
    // speed -- the line's promise.
    assert!(d.cruise_resume_after_mi.is_none(), "{:#?}", run.lines);
    assert!(d.speed_control_armed, "{:#?}", run.lines);
    app.ctx.input.press(Key::Up, Mods::NONE);
    for _ in 0..(60.0 / DT) as usize {
        if d.cruise_mph.is_some() {
            break;
        }
        app.ctx.input.begin_frame(DT);
        d.update_frame(&mut app.ctx, DT);
        clock.advance(DT);
    }
    let lines = app.event_lines();
    assert_eq!(d.cruise_mph, Some(60.0), "{lines:#?}");
    assert!(
        lines
            .iter()
            .any(|line| line.contains("Cruise resuming, 60")),
        "{lines:#?}"
    );
}

#[test]
fn test_the_drivers_own_brake_takes_the_bend_back_from_the_servo() {
    // (d) A driver holding the brake harder than the servo is not fought:
    // their key cancels the servo for the bend, the assist says it let go,
    // and the pedal the truck feels is theirs.
    let mut app = TestApp::new();
    let clock = app.fake_pacer_clock();
    let mut d = a_drive(&mut app);
    let bend = a_hot_bend_ahead(&mut app, &mut d, 60.0, 35, 307, 0.5);

    // One frame arms the servo off the call.
    app.ctx.input.begin_frame(DT);
    d.update_frame(&mut app.ctx, DT);
    clock.advance(DT);
    assert!(d.curve_servo.is_some(), "{:#?}", app.event_lines());

    // Then the driver stands on the brake for two seconds and lets go.
    app.ctx.input.press(Key::Down, Mods::NONE);
    let mut brake_seen: f64 = 0.0;
    for _ in 0..(2.0 / DT) as usize {
        app.ctx.input.begin_frame(DT);
        d.update_frame(&mut app.ctx, DT);
        clock.advance(DT);
        brake_seen = brake_seen.max(d.trip.truck.brake);
    }
    app.ctx.input.release(Key::Down, Mods::NONE);
    assert!(
        brake_seen >= 0.9,
        "the driver's pedal never reached full: {brake_seen}"
    );
    assert!(
        d.curve_servo.is_none(),
        "the servo survived the driver's own brake"
    );
    // The bend is theirs for the rest of it: nothing re-arms on the way in.
    let run = drive_through_the_bend(&mut app, &mut d, &bend, &clock, |_, d| {
        assert!(
            d.curve_servo.is_none(),
            "the servo re-armed after the cancel"
        );
    });
    assert!(
        run.lines
            .iter()
            .any(|line| line == "Curve assistance released."),
        "{:#?}",
        run.lines
    );
}

#[test]
fn test_the_servo_holds_a_bend_on_a_downgrade_on_one_application() {
    // Inside a 40 mph bend on a 6.1 percent downgrade, the truck a hair over
    // the hold band. The servo used to snub for a frame, let go the moment
    // the truck was back under the band, and get it handed straight back by
    // the hill: a fresh application ten times a second, 125 psi to the spring
    // brakes before the bend was over (owner's AZ-260 drive, 2026-09-18). The
    // air system charges every RISE of the pedal, so the rises are what is
    // counted here.
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    a_hot_bend_ahead(&mut app, &mut d, 41.5, 40, 400, 0.5);
    let here = d.trip.position_mi;
    d.arm_curve_servo(40.0, here - 0.1, here + 5.0, false);

    let mut applied_total = 0.0;
    let mut last = 0.0;
    for _ in 0..(45.0 / DT) as usize {
        d.trip.truck.grade = -0.061; // the trip normally stamps this each frame
        d.trip.truck.brake = 0.0; // no pedal: the servo is the only thing braking
        d.update_curve_speed_servo(&app.ctx);
        applied_total += (d.trip.truck.brake - last).max(0.0);
        last = d.trip.truck.brake;
        d.trip.truck.update(DT);
    }

    assert!(
        applied_total < 3.0,
        "the pedal was re-made over and over: {applied_total:.1} applications in one bend"
    );
    assert!(
        !d.trip.truck.air_low_warning(),
        "the bend drained the tanks to {:.0} psi",
        d.trip.truck.air_pressure_psi()
    );
    assert!(
        d.trip.truck.speed_mph() < 40.0 + 2.0,
        "and the hill got away from it: {:.1} mph",
        d.trip.truck.speed_mph()
    );
}

#[test]
fn test_the_servo_never_fans_the_pedal_on_any_grade_at_any_advisory() {
    // The AZ-260 bend was one grade, one advisory and one entry speed. The
    // same hold has to be steady on every hill the map has, from a two
    // percent roll to an eight percent pitch, at a town corner's number and
    // a highway sweeper's, entered a hair over, just past the band, and hot.
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    for grade in [-0.02, -0.03, -0.04, -0.05, -0.061, -0.07, -0.08] {
        for target in [15.0, 25.0, 30.0, 40.0, 55.0] {
            // 400 ft, or what a sign this fast is really built on: a 55 on
            // 400 ft asks half a g, which curve assistance now slows for
            // below the sign on its own (bend sweep, 2026-09-24).
            let radius = min_radius_ft(target).max(400.0) as i64;
            for (over, push) in [(0.5, 0.0), (1.5, 0.0), (8.0, 0.0), (0.9, 0.04)] {
                a_hot_bend_ahead(&mut app, &mut d, target + over, target as i64, radius, 0.5);
                d.curve_servo = None;
                let here = d.trip.position_mi;
                d.arm_curve_servo(target, here - 0.1, here + 5.0, false);

                let mut applied_total = 0.0;
                let mut last = 0.0;
                for _ in 0..(45.0 / DT) as usize {
                    d.trip.truck.grade = grade;
                    d.trip.truck.brake = 0.0;
                    // The last row is adaptive cruise holding the same bend:
                    // a light push on the truck the whole way through, so the
                    // hold never quite balances and the band edge is revisited.
                    d.trip.truck.throttle = push;
                    d.update_curve_speed_servo(&app.ctx);
                    applied_total += (d.trip.truck.brake - last).max(0.0);
                    last = d.trip.truck.brake;
                    d.trip.truck.update(DT);
                }

                let case = format!(
                    "{:.1} percent, advise {target}, {over} over, push {push}",
                    grade * 100.0
                );
                assert!(
                    applied_total < 1.5,
                    "{case}: {applied_total:.1} applications in one bend"
                );
                assert!(
                    !d.trip.truck.air_low_warning(),
                    "{case}: tanks down to {:.0} psi",
                    d.trip.truck.air_pressure_psi()
                );
                assert!(
                    d.trip.truck.speed_mph() < target + 2.0,
                    "{case}: the hill got away, {:.1} mph",
                    d.trip.truck.speed_mph()
                );
            }
        }
    }
}

#[test]
fn test_the_last_few_miles_an_hour_are_shed_on_the_real_clock() {
    // A bend called late with the truck barely over the pacenote margin: the
    // call decompresses the clock, the first touch of the brakes takes the
    // truck under the margin, and the clock used to snap back to the
    // compressed pacing with the servo still shedding -- so the road left
    // ran out seventeen times faster than the truck slowed and the pedal
    // went to the floor for 3 mph (AZ-260 bench trace, 2026-09-18).
    let mut app = TestApp::new();
    let clock = app.fake_pacer_clock();
    let mut d = a_drive(&mut app);
    d.trip.time_scale = 20.0;
    let bend = a_hot_bend_ahead(&mut app, &mut d, 44.0, 40, 400, 0.08);

    let mut servo_max: f64 = 0.0;
    let mut armed = false;
    drive_through_the_bend(&mut app, &mut d, &bend, &clock, |_, d| {
        if let Some(servo) = d.curve_servo.as_ref() {
            armed = true;
            servo_max = servo_max.max(servo.brake);
            if d.trip.truck.speed_mph() > servo.target_mph {
                assert_eq!(
                    d.trip.effective_time_scale(),
                    1.0,
                    "compressed clock with {:.1} mph still to shed",
                    d.trip.truck.speed_mph() - servo.target_mph
                );
            }
        }
    });

    assert!(armed, "the bend never armed the servo");
    assert!(
        servo_max < 0.6,
        "four miles an hour should not take a hard application: {servo_max:.2}"
    );
}

/// The worst the truck sat in its lane taking the 35 mph bend at 35 with
/// curve assistance off and `lane_keeping`, nobody steering.
fn worst_offset_at_the_advisory(lane_keeping: &str) -> (f64, Vec<String>) {
    let mut app = TestApp::new();
    let clock = app.fake_pacer_clock();
    let mut d = a_drive(&mut app);
    let bend = a_hot_bend_ahead(&mut app, &mut d, 35.0, 35, 307, 0.3);
    app.ctx.settings.curve_speed_assist = false;
    app.ctx.settings.lane_keeping = lane_keeping.to_string();
    let mut worst: f64 = 0.0;
    let run = drive_through_the_bend(&mut app, &mut d, &bend, &clock, |_, d| {
        d.trip.truck.velocity_mps = 35.0 * 0.44704;
        if d.trip.position_mi >= bend.start_mi && d.trip.position_mi <= bend.end_mi {
            worst = worst.max(d.lane.offset.abs());
        }
    });
    (worst, run.lines)
}

#[test]
fn test_the_cab_never_speaks_an_advisory_the_load_cannot_hold() {
    // A 141 ft bend on a banked road, posted 30 because the bake rounded 27.6
    // up: at 30 it asks a full trailer 0.37 g of its 0.35. The cab speaks the
    // load's own number there, in the 5 mph steps a plaque comes in; an empty
    // trailer is told the sign.
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    let bend = a_curve(d.trip.position_mi + 1.0, 'L', 30, 141, 60.0);
    d.trip.truck.trailer_attached = true;
    d.trip.truck.cargo_kg = ff_core::sim::vehicle::REFERENCE_CARGO_KG;
    d.trip.truck.liquid = None;
    assert_eq!(d.spoken_advisory_mph(&bend), 25);
    d.trip.truck.cargo_kg = 0.0;
    assert_eq!(d.spoken_advisory_mph(&bend), 30);
    // A sign the load holds is spoken as it is.
    let gentle = a_curve(d.trip.position_mi + 1.0, 'L', 45, 600, 40.0);
    d.trip.truck.cargo_kg = ff_core::sim::vehicle::REFERENCE_CARGO_KG;
    assert_eq!(d.spoken_advisory_mph(&gentle), 45);
}

#[test]
fn test_partial_lane_keeping_holds_a_bend_at_its_advisory_without_curve_assistance() {
    // Owner ruling, 2026-09-24: partial lane keeping steers through the
    // road's curve the way curve assistance does; the driver keeps lane
    // changes and speed. Its correction used to be capped with the driver's
    // key at the steering limit, so this bend ran wide at its own advisory.
    let (partial, lines) = worst_offset_at_the_advisory("partial");
    assert!(
        partial < 0.5,
        "partial lane keeping left the truck {partial:.2} off centre at the advisory"
    );
    assert!(
        !lines.iter().any(|l| l.contains(", too fast. Slow to")),
        "{lines:#?}"
    );
    // Lane keeping off stays manual: nobody steering, the bend is not held.
    let (off, _) = worst_offset_at_the_advisory("off");
    assert!(
        off > partial + 0.3,
        "with lane keeping off the bend steered itself: {off:.2}"
    );
}

#[test]
fn test_with_the_assist_off_a_hot_bend_still_drifts() {
    // (e) The setting means something: with curve speed assistance off, a
    // driver holding the throttle into the same bend is warned while there
    // is still road to slow in, nothing brakes for them, and at 60 into a
    // 35 the truck goes over.
    let mut app = TestApp::new();
    let clock = app.fake_pacer_clock();
    let mut d = a_drive(&mut app);
    let bend = a_hot_bend_ahead(&mut app, &mut d, 60.0, 35, 307, 0.5);
    app.ctx.settings.curve_speed_assist = false;
    app.ctx.input.press(Key::Up, Mods::NONE);

    let mut warned_before_the_bend = false;
    let run = drive_through_the_bend(&mut app, &mut d, &bend, &clock, |app, d| {
        assert!(
            d.curve_servo.is_none(),
            "the servo armed with the assist off"
        );
        if d.trip.position_mi < bend.start_mi
            && app
                .event_lines()
                .iter()
                .any(|l| l.contains(", too fast. Slow to"))
        {
            warned_before_the_bend = true;
        }
    });

    assert!(warned_before_the_bend, "{:#?}", run.lines);
    assert!(drifted(&run), "{:#?}", run.lines);
    assert!(
        run.lines
            .iter()
            .any(|line| line.contains("rolled over in the bend")),
        "{:#?}",
        run.lines
    );
    assert!(
        !run.lines
            .iter()
            .any(|line| line.contains("Curve assistance")),
        "{:#?}",
        run.lines
    );
}

#[test]
fn test_the_approach_servo_says_nothing_with_curve_callouts_off() {
    // Callouts off silences the words, never the assist (tonight's earlier
    // fix): the truck is still braked to the advisory, and nothing is said.
    let mut app = TestApp::new();
    let clock = app.fake_pacer_clock();
    let mut d = a_drive(&mut app);
    let bend = a_hot_bend_ahead(&mut app, &mut d, 60.0, 35, 307, 0.5);
    app.ctx.settings.curve_callouts = false;

    let run = drive_through_the_bend(&mut app, &mut d, &bend, &clock, |_, _| {});

    assert!(
        run.speed_at_start_mph <= 35.0 + 2.0,
        "crossed the bend's start at {:.1} mph",
        run.speed_at_start_mph
    );
    assert!(!drifted(&run), "{:#?}", run.lines);
    assert!(
        !run.lines
            .iter()
            .any(|line| line.contains("Curve") || line.contains("curve")),
        "callouts off means no words: {:#?}",
        run.lines
    );
}

/// The corner model's speed for `cue`'s own side at `deg`.
fn side_speed(cue: &ff_core::sim::trip_models::NavigationCue, deg: f64, load: f64) -> f64 {
    ff_core::data::corners::corner_speed_side_mph(
        Some(deg),
        ff_core::data::corners::CornerSide::from_direction(&cue.direction),
        load,
    )
}
