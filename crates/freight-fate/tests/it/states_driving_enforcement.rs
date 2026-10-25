//! `states/driving_enforcement.rs`: the scale houses, the patrol watch, the
//! cues, and the seeded draws that decide whether an officer acts.
//!
//! Ported from the app-shell halves of `tests/test_enforcement_presence.py`
//! and `test_troopers.py`, plus `test_scale_check_in_guidance.py`,
//! `test_weigh_station_transponder.py` and `test_speeding_consequences.py`.
//! The pure-model cases in those files (placement, `observe`, the safety
//! record's own arithmetic) already live with `ff_core::sim::enforcement_*`
//! and `ff_core::models::safety_record`; what is here is everything a real
//! `DrivingState` answers.
//!
//! Python's `monkeypatch.setattr(app.ctx, "say_event", ...)` bypassed the
//! speech ladder and the pacer; the Rust captures read what the player would
//! really have heard, which is stricter and occasionally why an assertion is
//! phrased as "contains" rather than an exact list.

use ff_core::models::enforcement::{
    CHAIN_LAW_FINE, FOLLOWING_TOO_CLOSE_FINE, LANE_MISUSE_FINE, LIGHTS_FINE, UNSAFE_DAMAGE_FINE,
};
use ff_core::models::jobs::{Job, CARGO_CATALOG};
use ff_core::models::profile::Profile;
use ff_core::pyrandom::PyRandom;
use ff_core::sim::enforcement_observe::{
    Observation, OBSERVE_HOLD_MI, TAILGATE_GAP_S, WHAT_CHAINS, WHAT_DAMAGE, WHAT_FOLLOWING,
    WHAT_LANE, WHAT_LIGHTS,
};
use ff_core::sim::enforcement_posts::{
    method_by_kind, post_seed, EnforcementPost, KIND_FIXED_SCALE, KIND_MEDIAN, KIND_SCALE_APRON,
    PACING_WINDOW_MI, TABLEAU_SIREN_LEAD_MI,
};
use ff_core::sim::traffic_manager::TrafficVehicle;
use ff_core::sim::trip_models::RoadStop;
use freight_fate::states::base::State;

use freight_fate::app::testing::{AudioLog, TestApp};
use freight_fate::states::driving::DrivingState;
use freight_fate::states::driving_core::*;
use freight_fate::states::driving_enforcement::{
    DEFERRED_STOP_MAX_MI, PASS_TRIGGER_MI, POST_MARKER_LEAD_MI, SCALE_BED_CLOSED_MAX_VOLUME,
    SCALE_BED_OPEN_MAX_VOLUME, SCALE_BED_START_MI, SCALE_NOTICE_SAMPLE, WEIGH_STATION_REMINDER_MI,
};
use freight_fate::states::driving_events::stop_plan::StopPlanPurpose;
use freight_fate::states::driving_menu_states::StopPurposeState;

// -- rigging -------------------------------------------------------------------------

fn mph_to_mps(mph: f64) -> f64 {
    mph / 2.23694
}

/// `_driving(app)` of `test_enforcement_presence.py`: Buffalo to Rochester.
fn a_drive(app: &mut TestApp, name: &str) -> DrivingState {
    let world = app.ctx.world;
    let mut profile = Profile::named_in(name, "Buffalo");
    profile.tutorial_done = true;
    app.ctx.profile = Some(profile);
    let route = world
        .supported_route("Buffalo", "Rochester", None)
        .expect("the world routes")
        .expect("Buffalo to Rochester is supported");
    let mut job = Job::new(
        CARGO_CATALOG
            .get("general")
            .expect("the general cargo type"),
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
    drive
}

/// `enforcement_helpers.always_observing_post`: a staffed, already-heard post
/// watching `reach_mi` up to `at_mi`.
fn always_observing_post(at_mi: f64, kind: &str, reach_mi: f64, notice: f64) -> EnforcementPost {
    EnforcementPost {
        method: method_by_kind(kind).to_string(),
        reach_mi,
        facing: "both".to_string(),
        staffed: true,
        notice,
        announced: true,
        ..EnforcementPost::new(at_mi, kind)
    }
}

fn a_watching_post(at_mi: f64) -> EnforcementPost {
    always_observing_post(at_mi, KIND_MEDIAN, 1.0, 1.0)
}

/// `enforcement_helpers.open_scale_post`: an open weigh station standing
/// behind `stop`. Whether a scale is open is settled at trip build from a
/// seeded draw, so a test that invents a RoadStop has to say which side of
/// that draw it wants.
fn open_scale_post(stop: &RoadStop) -> EnforcementPost {
    EnforcementPost {
        method: method_by_kind(KIND_FIXED_SCALE).to_string(),
        reach_mi: 0.5,
        facing: "with_traffic".to_string(),
        staffed: true,
        anchor: stop.key(),
        announced: true,
        ..EnforcementPost::new(stop.at_mi, KIND_FIXED_SCALE)
    }
}

fn a_scale(name: &str, at_mi: f64) -> RoadStop {
    let mut stop = RoadStop::new(name, at_mi, "weigh_station");
    stop.actions = vec!["inspect".to_string()];
    stop.parking = "none".to_string();
    stop
}

fn a_plaza(at_mi: f64) -> RoadStop {
    let mut stop = RoadStop::new("Blue Beacon Travel Plaza", at_mi, "travel_center");
    stop.actions = ["park", "save", "fuel", "food", "break", "sleep"]
        .map(str::to_string)
        .to_vec();
    stop.parking = "confirmed".to_string();
    stop.exit_label = "exit 55".to_string();
    stop
}

/// `_with_scale(d)`: an injected scale and a sleep-capable travel center just
/// past it.
fn with_scale(
    drive: &mut DrivingState,
    scale_mi: f64,
    plaza_mi: f64,
    scale_open: bool,
) -> (RoadStop, RoadStop) {
    let scale = a_scale("Ontario Scale", scale_mi);
    let plaza = a_plaza(plaza_mi);
    drive.trip.stops = vec![scale.clone(), plaza.clone()];
    drive.trip.posts = if scale_open {
        vec![open_scale_post(&scale)]
    } else {
        Vec::new()
    };
    (scale, plaza)
}

/// Every sound key the audio backend was asked for.
fn played(log: &AudioLog) -> Vec<String> {
    log.borrow()
        .played
        .iter()
        .map(|(key, _, _)| key.clone())
        .collect()
}

/// `enforcement_helpers.watch_speed`: put the truck `over` the limit under a
/// watching post and let it look. Returns the posted limit. Seeds the
/// over-limit distance past the observation hold directly: the hold is a
/// stretch of road, and driving it out frame by frame is not what any of
/// these tests is about.
fn watch_speed(drive: &mut DrivingState, app: &mut TestApp, over: f64) -> f64 {
    drive.trip.position_mi = drive.trip.total_miles() / 2.0;
    drive.enforcement_prev_mi = drive.trip.position_mi;
    let (limit, _) = drive.trip.speed_limit_at(drive.trip.position_mi);
    drive.trip.truck.velocity_mps = mph_to_mps(limit + over);
    drive.over_limit_mi = OBSERVE_HOLD_MI * 2.0;
    drive.update_enforcement_watch(&mut app.ctx, 0.1);
    limit
}

/// `_driving(app, patrol_intensity=...)` of `test_troopers.py`: one post that
/// watches the whole route and has already been heard, so a test can put the
/// truck anywhere and ask what it sees.
fn one_post_watching_everything(drive: &mut DrivingState, notice: f64) {
    let total = drive.trip.total_miles();
    drive.trip.posts = vec![always_observing_post(
        total,
        KIND_MEDIAN,
        total + 1.0,
        notice,
    )];
}

// -- the cue ladder, on a live drive --------------------------------------------------

#[test]
fn test_the_marked_unit_pass_actually_plays() {
    // The shipped, credited, unit-tested pass-by never once played: troopers
    // spawned with intent "cruising" and next_situation returned None for
    // cruising, so the asset was unreachable from inside the game.
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, "Presence");
    let log = app.record_audio();
    drive.trip.posts = vec![a_watching_post(5.0)];
    drive.enforcement_prev_mi = 4.0;
    drive.trip.position_mi = 6.0;

    drive.update_enforcement_watch(&mut app.ctx, 0.1);

    // The marker leads; the vehicle whoosh is scheduled behind it.
    assert!(played(&log).iter().any(|key| key == SIGNATURE_KEY));
    drive.service_pending_sounds(&mut app.ctx, 1.0);
    assert!(played(&log).iter().any(|key| key == "traffic/trooper_pass"));
}

#[test]
fn test_the_marker_leads_the_whoosh_rather_than_being_buried_in_it() {
    // traffic/trooper_pass and traffic/car_pass are both tyre-whoosh clips
    // differing only by a subtle chirp inside the whoosh. A two-element
    // signature -- marker first, at its own level, then the vehicle --
    // survives where a garnished one does not.
    const { assert!(PASS_MARKER_LEAD_S >= 0.15) };
}

#[test]
fn test_every_staffed_post_is_audible_before_it_can_observe_you() {
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, "Presence");
    let log = app.record_audio();
    let mut post = a_watching_post(6.0);
    post.announced = false;
    let post_id = post.id();
    drive.trip.posts = vec![post];
    // Still outside the post's reach: it watches from 5.0 up to 6.0.
    drive.enforcement_prev_mi = 3.0;
    drive.trip.position_mi = 4.9;

    drive.update_enforcement_watch(&mut app.ctx, 0.1);

    assert!(played(&log).iter().any(|key| key == SIGNATURE_KEY));
    let post = drive
        .trip
        .post_mut(&post_id)
        .expect("the post is on the trip");
    assert!(post.announced);
    assert!(drive.trip.position_mi < drive.trip.posts[0].watch_start_mi());
}

#[test]
fn test_the_marker_lead_puts_the_cue_before_the_look() {
    // The cue and the observation can never land in the same instant.
    const { assert!(POST_MARKER_LEAD_MI > 0.0) };
}

#[test]
fn test_an_empty_crossover_is_never_audible() {
    // The cue that read as a trooper ignoring a speeder, because it was. An
    // unstaffed post cannot observe anyone, so a pass-by earcon for one is a
    // police presence the player can hear and can never be caught by.
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, "Presence");
    let log = app.record_audio();
    let mut post = a_watching_post(5.0);
    post.staffed = false;
    drive.trip.posts = vec![post];
    drive.enforcement_prev_mi = 4.0;
    drive.trip.position_mi = 6.0;

    drive.update_enforcement_watch(&mut app.ctx, 0.1);
    drive.service_pending_sounds(&mut app.ctx, 1.0);

    assert!(played(&log).is_empty(), "{:?}", played(&log));
}

#[test]
fn test_a_scale_makes_no_marked_unit_pass_of_its_own() {
    // The scale bed already covers the approach.
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, "Presence");
    let stop = a_scale("Ontario Scale", 5.0);
    drive.trip.stops = vec![stop.clone()];
    drive.trip.posts = vec![open_scale_post(&stop)];
    let log = app.record_audio();
    drive.enforcement_prev_mi = 4.0;
    drive.trip.position_mi = 6.0;

    drive.update_enforcement_watch(&mut app.ctx, 0.1);
    drive.service_pending_sounds(&mut app.ctx, 1.0);

    assert!(!played(&log).iter().any(|key| key == "traffic/trooper_pass"));
}

#[test]
fn test_how_loud_the_road_sounds_comes_from_the_road() {
    // The slider's replacement: the same number that places the posts, not a
    // parallel formula that could drift away from it.
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, "Presence");
    drive.trip.position_mi = drive.trip.total_miles() / 2.0;
    let at = drive.trip.post_density_at(drive.trip.position_mi);
    assert!(at > 0.0);
    assert_eq!(drive.ambience_scale(), at);
}

// -- the radio -----------------------------------------------------------------------

#[test]
fn test_a_stop_cuts_the_radio_rather_than_ducking_it() {
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, "Presence");

    drive.cut_radio_for_stop(&mut app.ctx);

    assert!(drive.radio_cut_for_stop);
    // Cut, not merely lowered: the duck field is untouched.
    assert_eq!(drive.radio_cue_duck, 1.0);
}

#[test]
fn test_a_cue_leaves_the_radio_alone_when_ducking_is_off() {
    // Owner, 2026-08-17: "the cop marker is ducking the radio when I have
    // auto-ducking off." The marker still plays; it just no longer digs
    // itself a hole first.
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, "Presence");
    let log = app.record_audio();
    app.ctx.settings.duck_audio_for_speech = false;

    drive.play_enforcement_marker(&mut app.ctx, 0.8, 0.0);

    assert_eq!(drive.radio_cue_duck, 1.0, "ducked with the setting off");
    assert!(
        !played(&log).is_empty(),
        "the marker itself must still sound"
    );
}

#[test]
fn test_a_cue_ducks_the_radio_on_its_own_field() {
    // Never the picket duck: that one self-heals and would drag this away.
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, "Presence");
    app.ctx.settings.duck_audio_for_speech = true;

    drive.duck_radio_for_cue(&mut app.ctx);

    assert!(drive.radio_cue_duck < 1.0);
    assert_eq!(drive.radio_picket_duck, 1.0);
    drive.service_radio_cue_duck(&mut app.ctx, 10.0);
    assert_eq!(drive.radio_cue_duck, 1.0);
}

// -- the tableau ---------------------------------------------------------------------

fn a_tableau_post(at_mi: f64) -> EnforcementPost {
    EnforcementPost {
        tableau: true,
        ..always_observing_post(at_mi, KIND_MEDIAN, 1.0, 1.0)
    }
}

#[test]
fn test_the_tableau_cues_play_once_each() {
    // The siren leads the spot; the stopped pair plays as you pass it.
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, "Presence");
    let log = app.record_audio();
    drive.trip.posts = vec![a_tableau_post(6.0)];

    drive.enforcement_prev_mi = 6.0 - TABLEAU_SIREN_LEAD_MI - 0.1;
    drive.trip.position_mi = 6.0 - TABLEAU_SIREN_LEAD_MI + 0.1;
    drive.update_enforcement_watch(&mut app.ctx, 0.1);
    drive.service_pending_sounds(&mut app.ctx, 1.0);
    assert!(played(&log).iter().any(|key| key == "events/police_siren"));
    assert!(!played(&log).iter().any(|key| key == "traffic/trooper_pass"));

    drive.enforcement_prev_mi = 5.9;
    drive.trip.position_mi = 6.1;
    drive.update_enforcement_watch(&mut app.ctx, 0.1);
    assert!(played(&log).iter().any(|key| key == "traffic/trooper_pass"));
    assert!(played(&log).iter().any(|key| key == "traffic/car_pass"));
}

#[test]
fn test_tableau_audio_defers_while_the_players_own_stop_is_active() {
    // A tableau siren and the player's own pull-over siren share the same
    // `events/police_siren` asset, so playing the tableau's while the player
    // is mid-stop would sound like a second trooper on top of their own. The
    // missed cue is dropped for good rather than replayed once the stop
    // clears -- the shoulder pass still fires normally afterward.
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, "Presence");
    let log = app.record_audio();
    drive.trip.posts = vec![a_tableau_post(6.0)];

    drive.trip.position_mi = 6.0 - TABLEAU_SIREN_LEAD_MI - 0.1;
    drive.enforcement_prev_mi = drive.trip.position_mi;
    drive.pull_over = Some(PULL_OVER_LIGHTS.to_string()); // the player's own stop
    drive.trip.position_mi = 6.0 - TABLEAU_SIREN_LEAD_MI + 0.1;
    drive.update_enforcement_watch(&mut app.ctx, 0.1);
    drive.service_pending_sounds(&mut app.ctx, 1.0);
    assert!(!played(&log).iter().any(|key| key == "events/police_siren"));

    drive.pull_over = None;
    drive.trip.position_mi = 6.1;
    drive.update_enforcement_watch(&mut app.ctx, 0.1);
    drive.service_pending_sounds(&mut app.ctx, 1.0);
    // Dropped, not replayed late.
    assert!(!played(&log).iter().any(|key| key == "events/police_siren"));
    assert!(played(&log).iter().any(|key| key == "traffic/trooper_pass"));
    assert!(played(&log).iter().any(|key| key == "traffic/car_pass"));
}

#[test]
fn test_a_post_that_already_caught_the_player_never_also_runs_its_tableau() {
    // Once this post has had its own look at the player, its tableau story --
    // "a trooper has somebody stopped" -- no longer makes sense.
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, "Presence");
    let log = app.record_audio();
    let mut post = a_tableau_post(6.0);
    post.declined = true;
    drive.trip.posts = vec![post];

    drive.trip.position_mi = 4.0;
    drive.enforcement_prev_mi = 4.0;
    drive.trip.position_mi = 6.1; // cross both trigger miles in one frame
    drive.update_enforcement_watch(&mut app.ctx, 0.1);
    drive.service_pending_sounds(&mut app.ctx, 1.0);

    assert!(!played(&log).iter().any(|key| key == "events/police_siren"));
    assert!(!played(&log).iter().any(|key| key == "traffic/trooper_pass"));
    assert!(!played(&log).iter().any(|key| key == "traffic/car_pass"));
}

#[test]
fn test_the_tableau_intro_line_speaks_once_and_says_it_is_not_the_player() {
    // The siren cue alone reads like a driver's own stop starting, so it now
    // reliably introduces itself -- every tableau, not a chance draw like the
    // CB flavor line -- and only once, on the siren-lead trigger.
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, "Presence");
    drive.trip_seed = 1;
    drive.trip.posts = vec![a_tableau_post(6.0)];
    app.clear_speech();

    drive.enforcement_prev_mi = 6.0 - TABLEAU_SIREN_LEAD_MI - 0.1;
    drive.trip.position_mi = 6.0 - TABLEAU_SIREN_LEAD_MI + 0.1;
    drive.update_enforcement_watch(&mut app.ctx, 0.1);

    let lines = app.event_lines();
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert!(lines[0].contains("not you"), "{:?}", lines[0]);
    assert!(
        lines[0].to_lowercase().contains("trooper"),
        "{:?}",
        lines[0]
    );

    // The shoulder pass a little later must not repeat the introduction.
    drive.trip.position_mi = 6.1;
    drive.update_enforcement_watch(&mut app.ctx, 0.1);
    assert_eq!(app.event_lines().len(), 1);
}

#[test]
fn test_the_tableau_intro_line_stays_silent_while_deferred() {
    // Same deferral as the tableau audio: never layered on the player's own
    // stop, and not replayed late once the stop clears.
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, "Presence");
    drive.trip_seed = 1;
    drive.trip.posts = vec![a_tableau_post(6.0)];
    app.clear_speech();

    drive.trip.position_mi = 6.0 - TABLEAU_SIREN_LEAD_MI - 0.1;
    drive.enforcement_prev_mi = drive.trip.position_mi;
    drive.pull_over = Some(PULL_OVER_LIGHTS.to_string());
    drive.trip.position_mi = 6.0 - TABLEAU_SIREN_LEAD_MI + 0.1;
    drive.update_enforcement_watch(&mut app.ctx, 0.1);
    assert!(app.event_lines().is_empty());

    drive.pull_over = None;
    drive.trip.position_mi = 6.1;
    drive.update_enforcement_watch(&mut app.ctx, 0.1);
    assert!(app.event_lines().is_empty()); // dropped with the cue it rides
}

#[test]
fn test_the_tableau_intro_terse_form_keeps_the_bare_fact() {
    // A seeded pinch of why lands on some tableaus and not others, but the
    // terse rendering never carries it -- only the bare "not you" fact.
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, "Presence");
    drive.trip_seed = 7;
    let mut bare_seen = false;
    let mut flavored_seen = false;
    for i in 0..30 {
        let post = always_observing_post(i as f64, KIND_MEDIAN, 1.0, 1.0);
        let message = drive.tableau_intro_message(&post);
        assert_eq!(
            message.render(true),
            "A trooper has somebody stopped on the shoulder, not you."
        );
        if message.normal == message.render(true) {
            bare_seen = true;
        } else {
            flavored_seen = true;
            assert!(message
                .normal
                .starts_with("A trooper has somebody stopped on the shoulder "));
            assert!(message.normal.ends_with(", not you."));
        }
    }
    assert!(bare_seen && flavored_seen);
}

// -- deferral: one demand on the driver at a time -------------------------------------

#[test]
fn test_enforcement_defers_while_the_cab_already_has_a_demand() {
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, "Presence");
    let _log = app.record_audio();
    let post = always_observing_post(6.0, KIND_MEDIAN, 2.0, 1.0);
    let post_id = post.id();
    drive.trip.posts = vec![post];
    drive.trip.position_mi = 5.5;
    drive.enforcement_prev_mi = 5.4;
    drive.trip.truck.velocity_mps = mph_to_mps(120.0); // flagrant
    drive.over_limit_mi = OBSERVE_HOLD_MI * 3.0;
    drive.hazard_deadline = Some(2.0);

    drive.update_enforcement_watch(&mut app.ctx, 0.1);

    assert!(drive.pull_over.is_none());
    assert!(drive.deferred_post_ids.contains(&post_id));
    // It still has its look owing.
    assert!(!drive.trip.posts[0].declined);

    drive.hazard_deadline = None;
    drive.enforcement_prev_mi = 5.5;
    drive.over_limit_mi = OBSERVE_HOLD_MI * 3.0;
    drive.update_enforcement_watch(&mut app.ctx, 0.1);
    assert_eq!(drive.pull_over.as_deref(), Some(PULL_OVER_LIGHTS));
}

#[test]
fn test_a_deferred_look_survives_the_truck_leaving_the_post_behind() {
    // `deferred_post_ids` was written and never read anywhere, so "defer,
    // never drop" only held while the post still covered the truck's mile. A
    // hazard window outlasts a one-mile radar reach several times over at any
    // pacing, so in practice every look that landed during one was thrown
    // away (Jerry, 2026-08-11: whole routes over the limit with nothing
    // happening).
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, "Presence");
    let _log = app.record_audio();
    drive.trip.posts = vec![a_watching_post(6.0)];
    drive.trip.position_mi = 5.5;
    drive.enforcement_prev_mi = 5.4;
    drive.trip.truck.velocity_mps = mph_to_mps(120.0);
    drive.over_limit_mi = OBSERVE_HOLD_MI * 3.0;
    drive.hazard_deadline = Some(6.0);
    drive.update_enforcement_watch(&mut app.ctx, 0.1);
    assert!(drive.pull_over.is_none()); // one demand on the driver at a time

    // The hazard runs long enough that the truck is well past the post by the
    // time the cab is quiet. The stop still happens.
    drive.trip.position_mi = 9.0;
    drive.enforcement_prev_mi = 9.0;
    drive.hazard_deadline = None;
    assert!(drive.trip.posts_watching(drive.trip.position_mi).is_empty());
    drive.update_enforcement_watch(&mut app.ctx, 0.1);
    assert_eq!(drive.pull_over.as_deref(), Some(PULL_OVER_LIGHTS));
}

#[test]
fn test_a_trooper_who_never_caught_up_loses_you() {
    // A held look is not a debt collected fifty miles later.
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, "Presence");
    let _log = app.record_audio();
    drive.trip.posts = vec![a_watching_post(6.0)];
    drive.trip.position_mi = 5.5;
    drive.enforcement_prev_mi = 5.4;
    drive.trip.truck.velocity_mps = mph_to_mps(120.0);
    drive.over_limit_mi = OBSERVE_HOLD_MI * 3.0;
    drive.hazard_deadline = Some(6.0);
    drive.update_enforcement_watch(&mut app.ctx, 0.1);
    assert!(drive.pull_over.is_none());

    drive.trip.position_mi = 5.5 + DEFERRED_STOP_MAX_MI + 1.0;
    drive.enforcement_prev_mi = drive.trip.position_mi;
    drive.hazard_deadline = None;
    drive.update_enforcement_watch(&mut app.ctx, 0.1);
    assert!(drive.pull_over.is_none());
}

// -- being caught (test_troopers.py, the driving side) --------------------------------

#[test]
fn test_speeding_past_a_staffed_post_starts_a_pull_over() {
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, "Leadfoot");
    one_post_watching_everything(&mut drive, 1.0);
    let _log = app.record_audio();

    watch_speed(&mut drive, &mut app, 20.0);

    assert_eq!(drive.pull_over.as_deref(), Some(PULL_OVER_LIGHTS));
    // The ticket is written at the stop, not here.
    assert_eq!(drive.speeding_tickets, 0);
}

#[test]
fn test_speeding_with_no_post_watching_costs_nothing() {
    // Getting away with it is the intended outcome, not a gap. This used to
    // assert a silent strike was banked for the dock; there is no such thing
    // any more.
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, "Leadfoot");
    drive.trip.posts = Vec::new();
    let _log = app.record_audio();
    let money_before = profile_of(&app.ctx).money();

    watch_speed(&mut drive, &mut app, 20.0);

    assert!(drive.pull_over.is_none());
    assert_eq!(drive.speeding_tickets, 0);
    assert_eq!(profile_of(&app.ctx).money(), money_before);
}

#[test]
fn test_debug_off_mode_never_pulls_you_over() {
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, "Leadfoot");
    one_post_watching_everything(&mut drive, 1.0);
    app.ctx.settings.hos_mode = "debug_off".to_string();
    let _log = app.record_audio();
    let money_before = profile_of(&app.ctx).money();

    watch_speed(&mut drive, &mut app, 20.0);

    assert!(drive.pull_over.is_none());
    assert_eq!(drive.speeding_tickets, 0);
    assert_eq!(profile_of(&app.ctx).money(), money_before);
}

#[test]
fn test_an_observed_stop_that_is_not_speed_names_what_the_trooper_saw() {
    // The non-speeding branch of `begin_observed_stop`: the summary, the
    // fine, and the line the driver is sent back onto the road with.
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, "Leadfoot");
    let post = always_observing_post(6.0, "work_zone_post", 1.0, 1.0);
    drive.trip.posts = vec![post];
    drive.trip.position_mi = 5.9;
    drive.enforcement_prev_mi = 5.8;
    drive.trip.truck.damage_pct = 95.0; // visible damage a visual post cannot miss
    let _log = app.record_audio();

    // Walk the seeded roll until this post's look survives it; the draw is
    // named and position-quantised, so a mile that catches stays caught.
    let mut caught = false;
    for step in 0..40 {
        drive.trip.position_mi = 5.9 + step as f64 * 0.005;
        drive.enforcement_prev_mi = drive.trip.position_mi;
        if let Some(post) = drive.trip.post_mut("post:0:6.0:work_zone_post") {
            post.declined = false;
        }
        drive.update_enforcement_watch(&mut app.ctx, 0.1);
        if drive.pull_over.is_some() {
            caught = true;
            break;
        }
    }
    assert!(caught, "a visual post eventually sees 95 percent damage");
    assert_eq!(drive.pull_over_kind, "observed");
    assert_eq!(drive.pull_over_title, "Roadside pull-over");
    assert!(
        drive
            .pull_over_summary
            .contains("ordered a roadside safety inspection"),
        "{}",
        drive.pull_over_summary
    );
    assert_eq!(
        drive.pull_over_return,
        "Back on the highway. Repair the truck at the next safe stop."
    );
}

// -- the following-distance hold ------------------------------------------------------

#[test]
fn test_a_momentary_gap_dip_does_not_accrue_a_following_offence() {
    // The mirror of the over-limit accumulator: a post should read a
    // following distance, not a frame.
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, "Presence");
    drive.trip.position_mi = 10.0;
    drive.trip.truck.velocity_mps = mph_to_mps(60.0);
    // A lead vehicle close enough that the gap is inside the tailgate window.
    let gap_mi = 60.0 * (TAILGATE_GAP_S * 0.5) / 3600.0;
    drive.trip.traffic_manager.vehicles = vec![TrafficVehicle::new(
        "lead",
        drive.trip.position_mi + gap_mi,
        60.0,
        60.0,
        0,
        "cruising",
        "car",
    )];

    drive.accrue_following_gap(0.01);
    assert!(drive.closed_up_mi > 0.0);

    // An assist that is already recovering the gap is not disregard.
    drive.closed_up_mi = 0.0;
    drive.speed_control_armed = true;
    drive.trip.truck.brake = 0.5;
    drive.trip.truck.throttle = 0.0;
    drive.accrue_following_gap(0.01);
    assert_eq!(drive.closed_up_mi, 0.0);

    // And an open road clears it outright.
    drive.trip.set_npc_vehicles(Vec::new());
    drive.closed_up_mi = 1.0;
    drive.accrue_following_gap(0.01);
    assert_eq!(drive.closed_up_mi, 0.0);
}

#[test]
fn test_the_over_limit_accumulator_forgives_an_assist_that_is_braking() {
    // The destination-exit ease used to accrue over-limit distance while the
    // assist was doing exactly what it was asked to.
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, "Presence");
    drive.trip.posts = Vec::new();
    drive.trip.position_mi = 10.0;
    drive.enforcement_prev_mi = 9.9;
    let (limit, _) = drive.trip.speed_limit_at(drive.trip.position_mi);
    drive.trip.truck.velocity_mps = mph_to_mps(limit + 25.0);
    drive.over_limit_mi = 0.5;
    drive.speed_control_armed = true;
    drive.trip.truck.brake = 0.5;
    drive.trip.truck.throttle = 0.0;

    drive.update_enforcement_watch(&mut app.ctx, 0.1);

    assert_eq!(drive.over_limit_mi, 0.0);
}

#[test]
fn test_a_limit_that_just_dropped_earns_braking_room() {
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, "Presence");
    drive.trip.posts = Vec::new();
    drive.trip.position_mi = 10.0;
    drive.enforcement_prev_mi = 9.9;
    let (limit, _) = drive.trip.speed_limit_at(drive.trip.position_mi);
    drive.trip.truck.velocity_mps = mph_to_mps(limit + 25.0);
    drive.over_limit_mi = 0.5;
    drive.limit_drop_grace_s = 3.0;

    drive.update_enforcement_watch(&mut app.ctx, 0.1);

    assert_eq!(drive.over_limit_mi, 0.0);
}

// -- pacing --------------------------------------------------------------------------

#[test]
fn test_a_pacing_unit_banks_road_behind_the_truck_and_is_dropped_past_its_window() {
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, "Presence");
    let post = always_observing_post(6.0, "roving_patrol", 0.6, 1.0);
    let post_id = post.id();
    drive.trip.posts = vec![post];

    drive.trip.position_mi = 6.2; // inside the window behind the unit
    drive.track_pacing(0.2);
    assert_eq!(drive.pacing_mi.get(&post_id).copied(), Some(0.2));

    drive.trip.position_mi = 6.0 + PACING_WINDOW_MI + 0.1;
    drive.track_pacing(0.1);
    assert!(!drive.pacing_mi.contains_key(&post_id));
}

// -- determinism ---------------------------------------------------------------------

#[test]
fn test_the_observation_seed_is_position_quantised_never_time_quantised() {
    // Identical driving through identical road has to produce an identical
    // outcome whatever the frame rate, and a reload must not re-roll whether
    // a trooper was looking at you.
    let key = |seed: i64, position: f64| {
        post_seed(
            Some(seed),
            "post:0:6.0:median_post",
            &format!("observe:speeding:{position}"),
        )
    };
    assert_eq!(key(0, 10.5), key(0, 10.5));
    assert_ne!(key(0, 10.5), key(0, 10.6));
    assert_ne!(key(0, 10.5), key(1, 10.5));
}

#[test]
fn test_the_same_road_twice_produces_the_same_outcome() {
    let mut app = TestApp::new();
    let outcome = |app: &mut TestApp| {
        let mut drive = a_drive(app, "Presence");
        one_post_watching_everything(&mut drive, 0.4);
        drive.trip.position_mi = drive.trip.total_miles() / 2.0;
        drive.enforcement_prev_mi = drive.trip.position_mi;
        let (limit, _) = drive.trip.speed_limit_at(drive.trip.position_mi);
        drive.trip.truck.velocity_mps = mph_to_mps(limit + 14.0);
        drive.over_limit_mi = OBSERVE_HOLD_MI * 2.0;
        drive.update_enforcement_watch(&mut app.ctx, 0.1);
        drive.pull_over.clone()
    };
    let first = outcome(&mut app);
    let second = outcome(&mut app);
    assert_eq!(first, second);
}

// -- the weigh station ---------------------------------------------------------------

#[test]
fn test_scale_notice_lookahead_sample_covers_the_real_sentence() {
    // The spoken lead is sized from a sample; it must not undershoot the real
    // wording with a long stop name.
    let real = concat!(
        "Open weigh station ahead in two miles, Northbound Platte River ",
        "Port of Entry. All trucks must pull in."
    );
    assert!(
        ramp_arrival_grace_seconds(SCALE_NOTICE_SAMPLE, 0.0)
            >= ramp_arrival_grace_seconds(real, 0.0)
    );
}

#[test]
fn test_only_an_open_scale_answers_scale_is_open() {
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, "Jerry");
    let (scale, _plaza) = with_scale(&mut drive, 10.0, 11.0, true);
    assert!(drive.scale_is_open(&scale));

    // A closed scale still carries an apron post -- state police may be
    // sitting on it -- but the silence-means-closed rule holds.
    let apron = EnforcementPost {
        anchor: scale.key(),
        ..always_observing_post(scale.at_mi, KIND_SCALE_APRON, 0.5, 1.0)
    };
    drive.trip.posts = vec![apron];
    assert!(!drive.scale_is_open(&scale));
    assert!(drive.open_scale_ahead(5.0).is_none());
}

#[test]
fn test_the_open_scale_lookahead_is_at_least_the_flat_notice_distance() {
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, "Jerry");
    drive.trip.truck.velocity_mps = mph_to_mps(45.0);
    assert!(drive.scale_notice_lookahead_mi(&app.ctx) >= WEIGH_STATION_NOTICE_MI);
}

#[test]
fn test_the_scale_notice_expires_once_the_distance_it_names_is_wrong() {
    // The notice names a distance, and a distance is a claim about now.
    //
    // A cut ROUTE line can be handed back to be spoken behind the line that
    // cut it, so this one has to say when it has stopped being true. It goes
    // wrong while the scale is still AHEAD, which is sooner than the reminder
    // that follows it: handed back after "Weigh station in half a mile" it
    // told the driver the scale was two miles off, the two lines
    // contradicting each other one after the other (Python adversarial
    // battery, scale_bypass_to_the_end).
    //
    // Python asserts on the `valid` callable the notice carries. A Rust gate
    // is a boxed closure the capture cannot hand back, so this drives the
    // rescue itself: queue the real notice, cut the channel with an urgent
    // line, and count what the player heard.
    //
    // The urgent line that cuts the channel, as in `states_driving_valid_gates`.
    const CUTTER: &str = "Emergency vehicle approaching from behind.";
    let notices = |app: &TestApp| {
        app.event_lines()
            .into_iter()
            .filter(|line| line.starts_with("Open weigh station ahead in "))
            .collect::<Vec<_>>()
    };

    // Where it was spoken, its own words are the road that is left, so a cut
    // that early still gets it back -- the gate must not be nailed shut.
    {
        let mut app = TestApp::new();
        let _clock = app.fake_pacer_clock();
        let mut drive = a_drive(&mut app, "Jerry");
        with_scale(&mut drive, 10.0, 11.0, true);
        drive.trip.position_mi = 8.0;
        drive.trip.truck.velocity_mps = mph_to_mps(45.0);
        app.clear_speech();

        drive.check_weigh_station_enforcement(&mut app.ctx, 7.8);
        assert_eq!(notices(&app).len(), 1, "{:?}", app.event_lines());
        assert!(notices(&app)[0].starts_with("Open weigh station ahead in 2.0 miles"));
        app.ctx.say_event(CUTTER);
        assert_eq!(
            notices(&app).len(),
            2,
            "a notice cut while its own words are still true must come back: {:?}",
            app.event_lines()
        );
    }

    // Closed to half a mile: the sentence now names a distance the truck
    // drove through several minutes ago, so the rescue must let it die.
    // And past the scale it is dead too, for the same reason the reminder is.
    for moved_to in [9.5, 10.5] {
        let mut app = TestApp::new();
        let _clock = app.fake_pacer_clock();
        let mut drive = a_drive(&mut app, "Jerry");
        with_scale(&mut drive, 10.0, 11.0, true);
        drive.trip.position_mi = 8.0;
        drive.trip.truck.velocity_mps = mph_to_mps(45.0);
        app.clear_speech();

        drive.check_weigh_station_enforcement(&mut app.ctx, 7.8);
        assert_eq!(notices(&app).len(), 1);
        drive.trip.position_mi = moved_to;
        drive.refresh_live_facts();
        app.ctx.say_event(CUTTER);
        assert_eq!(
            notices(&app).len(),
            1,
            "the notice was replayed at mile {moved_to} naming the distance it \
             spoke from mile 8: {:?}",
            app.event_lines()
        );
    }
}

#[test]
fn test_a_gap_the_assist_owns_is_never_the_drivers_disregard() {
    // Darren, fined twice for a gap no control of his governed.
    //
    // 1,200 dollars on I-75 (2026-08-18) is what the carve-out was written
    // for, and it only forgave a gap while the assist was actively BRAKING --
    // which is adaptive cruise recovering, and nothing else.
    //
    // The speed KEEPER does not follow traffic at all: driving_speed_control
    // has no notion of a lead vehicle, it holds the posted number. So in a
    // work zone it sits at the sign's 55 while the line ahead bunches up,
    // closing the gap with the throttle open and never touching the brake.
    // That read as the driver's disregard and cost him 2,400 dollars on I-94
    // (2026-08-24), doubled for the construction zone, with the cab
    // announcing "Speed keeper holding 55 miles per hour" as it happened.
    //
    // The question is who has the pedal, not what the pedal is doing.
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, "Darren");
    drive.closed_up_mi = 0.0;
    drive.trip.truck.brake = 0.0;

    // The keeper owns the throttle and is holding the zone's number.
    drive.speed_control_armed = true;
    drive.cruise_mph = None;
    drive.cruise_applied = 0.45;
    drive.trip.truck.throttle = 0.45;
    assert!(drive.assist_owns_the_pedal());

    // Adaptive cruise braking to recover a gap: the original case, still
    // forgiven.
    drive.cruise_mph = Some(60.0);
    drive.cruise_applied = 0.0;
    drive.trip.truck.throttle = 0.0;
    drive.trip.truck.brake = 0.3;
    assert!(drive.assist_owns_the_pedal());

    // But a driver pressing PAST what the assist asked for is closing the gap
    // themselves, and owns it.
    drive.cruise_applied = 0.20;
    drive.trip.truck.throttle = 0.85;
    drive.trip.truck.brake = 0.0;
    assert!(!drive.assist_owns_the_pedal());

    // And with no assist at all it is entirely theirs.
    drive.speed_control_armed = false;
    drive.cruise_mph = None;
    drive.trip.truck.throttle = 0.0;
    assert!(!drive.assist_owns_the_pedal());
}

#[test]
fn test_reminder_fires_once_when_still_fast_with_no_scale_exit_armed() {
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, "Jerry");
    let (scale, _) = with_scale(&mut drive, 10.0, 11.0, true);
    let key = format!("weigh:{}:{:.1}", scale.name, scale.at_mi);
    drive.weigh_station_noticed.insert(key.clone());
    drive.trip.truck.velocity_mps = mph_to_mps(45.0);
    app.clear_speech();

    drive.trip.position_mi = 9.6;
    drive.check_scale_reminder(&mut app.ctx, &scale, 0.4, &key);
    drive.trip.position_mi = 9.7;
    drive.check_scale_reminder(&mut app.ctx, &scale, 0.3, &key);

    let reminders: Vec<String> = app
        .event_lines()
        .into_iter()
        .filter(|line| line.starts_with("Ontario Scale in "))
        .collect();
    assert_eq!(
        reminders,
        vec!["Ontario Scale in half a mile. Signal for the scale exit.".to_string()]
    );
}

#[test]
fn test_reminder_speaks_the_road_actually_left() {
    // The distance was hard-coded to the threshold, so a reminder that landed
    // at a couple of hundred yards overstated the road left -- and beside the
    // approach line, which speaks a real short distance, the scale appeared
    // to move further away as the truck closed on it (2026-08-15).
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, "Jerry");
    let (scale, _) = with_scale(&mut drive, 10.0, 11.0, true);
    let key = format!("weigh:{}:{:.1}", scale.name, scale.at_mi);
    drive.weigh_station_noticed.insert(key.clone());
    drive.trip.truck.velocity_mps = mph_to_mps(45.0);
    drive.trip.position_mi = scale.at_mi - 0.15;
    app.clear_speech();

    drive.check_scale_reminder(&mut app.ctx, &scale, 0.15, &key);

    let reminders: Vec<String> = app
        .event_lines()
        .into_iter()
        .filter(|line| line.starts_with("Ontario Scale in "))
        .collect();
    assert_eq!(
        reminders,
        vec!["Ontario Scale in a quarter mile. Signal for the scale exit.".to_string()]
    );
}

#[test]
fn test_reminder_stays_quiet_once_the_scale_exit_is_armed() {
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, "Jerry");
    let (scale, _) = with_scale(&mut drive, 10.0, 11.0, true);
    let key = format!("weigh:{}:{:.1}", scale.name, scale.at_mi);
    drive.weigh_station_noticed.insert(key.clone());
    drive.trip.truck.velocity_mps = mph_to_mps(45.0);
    drive.exit_stop = Some(scale.clone());
    drive.exit_signal_on = true;
    app.clear_speech();

    drive.check_scale_reminder(&mut app.ctx, &scale, 0.4, &key);

    assert!(app.event_lines().is_empty());
}

#[test]
fn test_reminder_stays_quiet_below_the_bypass_speed() {
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, "Jerry");
    let (scale, _) = with_scale(&mut drive, 10.0, 11.0, true);
    let key = format!("weigh:{}:{:.1}", scale.name, scale.at_mi);
    drive.weigh_station_noticed.insert(key.clone());
    drive.trip.truck.velocity_mps = mph_to_mps(10.0);
    app.clear_speech();

    drive.check_scale_reminder(&mut app.ctx, &scale, 0.4, &key);

    assert!(app.event_lines().is_empty());
}

#[test]
fn test_a_green_transponder_verdict_retires_the_reminder() {
    // "Signal for the scale exit" would contradict a weigh-in-motion green.
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, "Jerry");
    let (scale, _) = with_scale(&mut drive, 10.0, 11.0, true);
    let key = format!("weigh:{}:{:.1}", scale.name, scale.at_mi);
    drive.weigh_station_noticed.insert(key.clone());
    drive.trip.truck.velocity_mps = mph_to_mps(45.0);
    drive
        .weigh_station_transponder_verdict
        .insert(key.clone(), "green".to_string());
    app.clear_speech();

    drive.check_scale_reminder(&mut app.ctx, &scale, 0.4, &key);

    assert!(app.event_lines().is_empty());
}

#[test]
fn test_a_green_transponder_lets_the_co_driver_and_the_exit_key_pass_the_scale() {
    // Owner, 2026-10-19: with a transponder showing green, the truck rolls
    // on by the scale as the rules allow; only a scale that wants the truck
    // in is one to signal for.
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, "Jerry");
    let (scale, _) = with_scale(&mut drive, 10.0, 11.0, true);
    drive.trip.position_mi = 8.0;
    assert!(
        drive.check_in_scale_ahead(&app.ctx, 5.0).is_some(),
        "with no transponder the open scale wants the truck"
    );
    app.ctx.profile.as_mut().expect("a career").career =
        ff_core::models::career::Career::with_xp(1.0e7);
    let key = drive.weigh_station_key(&scale);
    drive
        .weigh_station_transponder_verdict
        .insert(key.clone(), "green".to_string());
    assert!(!drive.scale_wants_truck(&app.ctx, &scale));
    assert!(drive.check_in_scale_ahead(&app.ctx, 5.0).is_none());
    drive
        .weigh_station_transponder_verdict
        .insert(key, "red".to_string());
    assert!(drive.check_in_scale_ahead(&app.ctx, 5.0).is_some());
}

#[test]
fn test_the_reminder_window_is_the_last_half_mile() {
    const { assert!(WEIGH_STATION_REMINDER_MI > 0.0 && WEIGH_STATION_REMINDER_MI < 1.0) };
}

// -- the rest key defers to the open scale --------------------------------------------

#[test]
fn test_rest_key_at_speed_defers_to_the_open_scale() {
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, "Jerry");
    let (_scale, plaza) = with_scale(&mut drive, 10.0, 11.0, true);
    drive.trip.position_mi = 8.2;
    drive.trip.truck.velocity_mps = mph_to_mps(54.0);
    drive.trip.planned_stop_key = Some(plaza.key());
    drive.selected_stop_key = Some(plaza.key());
    drive.selected_stop_assist_armed = true;
    app.clear_speech();

    drive.try_rest_stop(&mut app.ctx);

    let spoken = app.event_lines().join(" ");
    assert!(spoken.contains("Weigh station first"), "{spoken}");
    assert!(spoken.contains("All trucks must stop"), "{spoken}");
    assert_eq!(
        drive.trip.planned_stop_key.as_deref(),
        Some(plaza.key().as_str())
    );
    assert_eq!(
        drive.selected_stop_key.as_deref(),
        Some(plaza.key().as_str())
    );
}

#[test]
fn test_rest_key_on_a_scale_ramp_keeps_an_existing_sleep_plan() {
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, "Jerry");
    let (scale, plaza) = with_scale(&mut drive, 10.0, 11.0, true);
    drive.trip.position_mi = scale.at_mi;
    drive.trip.truck.velocity_mps = mph_to_mps(15.0);
    drive.ramp_stop = Some(scale);
    drive.ramp_mi = Some(0.3);
    drive.trip.planned_stop_key = Some(plaza.key());
    drive.selected_stop_key = Some(plaza.key());
    drive.selected_stop_assist_armed = true;
    app.clear_speech();

    drive.try_rest_stop(&mut app.ctx);

    let spoken = app.main_lines().join(" ");
    assert!(
        spoken.contains("On the ramp for weigh station: Ontario Scale"),
        "{spoken}"
    );
    assert_eq!(
        drive.trip.planned_stop_key.as_deref(),
        Some(plaza.key().as_str())
    );
    assert_eq!(
        drive.selected_stop_key.as_deref(),
        Some(plaza.key().as_str())
    );
}

#[test]
fn test_rest_key_plans_normally_when_the_scale_is_closed() {
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, "Jerry");
    let (_scale, plaza) = with_scale(&mut drive, 10.0, 11.0, false);
    drive.trip.position_mi = 8.2;
    drive.trip.truck.velocity_mps = mph_to_mps(54.0);
    app.clear_speech();

    drive.try_rest_stop(&mut app.ctx);

    let spoken = app.main_lines().join(" ") + &app.event_lines().join(" ");
    assert!(!spoken.contains("Weigh station first"), "{spoken}");
    // The scale let the key through to the purpose list; the sleep row
    // plans the plaza.
    assert!(app
        .ctx
        .state()
        .is_some_and(|state| state.borrow().as_any().is::<StopPurposeState>()));
    drive.plan_stop_for(&mut app.ctx, StopPlanPurpose::Sleep);
    assert_eq!(
        drive.selected_stop_key.as_deref(),
        Some(plaza.key().as_str())
    );
}

#[test]
fn test_rest_key_plans_normally_when_the_scale_is_behind() {
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, "Jerry");
    let (_scale, plaza) = with_scale(&mut drive, 10.0, 12.5, true);
    drive.trip.position_mi = 11.6; // well past the scale, plaza ahead
    drive.trip.truck.velocity_mps = mph_to_mps(54.0);
    app.clear_speech();

    drive.try_rest_stop(&mut app.ctx);

    let spoken = app.main_lines().join(" ") + &app.event_lines().join(" ");
    assert!(!spoken.contains("Weigh station first"), "{spoken}");
    // The scale let the key through to the purpose list; the sleep row
    // plans the plaza.
    assert!(app
        .ctx
        .state()
        .is_some_and(|state| state.borrow().as_any().is::<StopPurposeState>()));
    drive.plan_stop_for(&mut app.ctx, StopPlanPurpose::Sleep);
    assert_eq!(
        drive.selected_stop_key.as_deref(),
        Some(plaza.key().as_str())
    );
}

#[test]
fn test_rest_key_police_stop_guard_still_outranks_the_scale() {
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, "Jerry");
    with_scale(&mut drive, 10.0, 11.0, true);
    drive.trip.position_mi = 8.2;
    drive.trip.truck.velocity_mps = mph_to_mps(54.0);
    drive.pull_over = Some(PULL_OVER_LIGHTS.to_string());
    app.clear_speech();

    drive.try_rest_stop(&mut app.ctx);

    let spoken = app.main_lines().join(" ") + &app.event_lines().join(" ");
    assert!(spoken.contains("Resolve the police stop"), "{spoken}");
    assert!(!spoken.contains("Weigh station first"), "{spoken}");
}

#[test]
fn test_rest_key_ignores_the_scale_in_a_casual_hos_mode() {
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, "Jerry");
    let (_scale, plaza) = with_scale(&mut drive, 10.0, 11.0, true);
    app.ctx.settings.hos_mode = "debug_off".to_string();
    drive.trip.position_mi = 8.2;
    drive.trip.truck.velocity_mps = mph_to_mps(54.0);
    app.clear_speech();

    drive.try_rest_stop(&mut app.ctx);

    let spoken = app.main_lines().join(" ") + &app.event_lines().join(" ");
    assert!(!spoken.contains("Weigh station first"), "{spoken}");
    // The scale let the key through to the purpose list; the sleep row
    // plans the plaza.
    assert!(app
        .ctx
        .state()
        .is_some_and(|state| state.borrow().as_any().is::<StopPurposeState>()));
    drive.plan_stop_for(&mut app.ctx, StopPlanPurpose::Sleep);
    assert_eq!(
        drive.selected_stop_key.as_deref(),
        Some(plaza.key().as_str())
    );
}

// -- the exit key: the nearer open scale claims it --------------------------------------

#[test]
fn test_the_nearer_open_scale_outranks_a_farther_planned_stop() {
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, "Jerry");
    let (scale, plaza) = with_scale(&mut drive, 10.0, 11.0, true);
    drive.trip.position_mi = 8.2;
    drive.trip.truck.velocity_mps = mph_to_mps(54.0);

    let claimed = drive.scale_claiming_exit(&mut app.ctx, Some(&plaza));

    assert_eq!(claimed.map(|found| found.key()), Some(scale.key()));
}

#[test]
fn test_a_nearer_selected_stop_is_left_alone() {
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, "Jerry");
    let (_scale, plaza) = with_scale(&mut drive, 11.0, 10.0, true);
    drive.trip.position_mi = 8.2;
    drive.trip.truck.velocity_mps = mph_to_mps(54.0);

    assert!(drive
        .scale_claiming_exit(&mut app.ctx, Some(&plaza))
        .is_none());
}

#[test]
fn test_the_scale_never_claims_its_own_exit_twice() {
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, "Jerry");
    let (scale, _plaza) = with_scale(&mut drive, 10.0, 11.0, true);
    drive.trip.position_mi = 8.2;
    drive.trip.truck.velocity_mps = mph_to_mps(54.0);

    // Nothing outranked; the normal arming handles it.
    assert!(drive
        .scale_claiming_exit(&mut app.ctx, Some(&scale))
        .is_none());
    assert!(drive.scale_claiming_exit(&mut app.ctx, None).is_none());
}

#[test]
fn test_a_signaled_speed_valid_open_scale_enters_its_ramp() {
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, "Scale Ramp");
    let (scale, _) = with_scale(&mut drive, 10.0, 11.0, true);
    drive.trip.position_mi = scale.at_mi + 0.01;
    drive.trip.truck.velocity_mps = mph_to_mps(33.0);
    drive.exit_stop = Some(scale.clone());
    drive.exit_signal_on = true;
    drive.exit_lane_entered = true;

    drive.update_exit(&mut app.ctx, 0.02, 0.1);

    assert_eq!(
        drive.ramp_stop.as_ref().map(RoadStop::key),
        Some(scale.key())
    );
    assert!(drive.ramp_mi.is_some());
    assert!(!drive.exit_signal_on);
}

#[test]
fn test_a_scale_taken_at_the_taper_is_not_crossed_again_on_the_way_out() {
    // Steered into the exit lane where it opened, 300 feet short of the gore.
    // The ramp holds the highway odometer, so the truck rejoined short of the
    // scale, crossed it pulling away, and was charged with bypassing the
    // scale it had just checked in at.
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, "Taper Scale");
    let (scale, _) = with_scale(&mut drive, 10.0, 11.0, true);
    drive.trip.position_mi = scale.at_mi - EXIT_TAPER_MI;
    drive.trip.truck.velocity_mps = mph_to_mps(40.0);
    drive.exit_stop = Some(scale.clone());
    drive.exit_signal_on = true;
    drive.exit_lane_entered = true;
    drive.exit_taper_said = true;

    drive.update_exit(&mut app.ctx, 0.02, 0.1);

    assert_eq!(
        drive.ramp_stop.as_ref().map(RoadStop::key),
        Some(scale.key())
    );
    assert!(drive.trip.position_mi >= scale.at_mi);

    // Checked in, back on the road, and past 15 pulling away.
    drive.ramp_mi = None;
    drive.ramp_stop = None;
    let previous = drive.trip.position_mi;
    drive.trip.position_mi += 0.01;
    drive.trip.truck.velocity_mps = mph_to_mps(25.0);
    drive.check_weigh_station_enforcement(&mut app.ctx, previous);

    assert!(!drive
        .enforcement_events
        .contains(&drive.weigh_station_key(&scale)));
    assert!(drive.pull_over.is_none());
}

#[test]
fn test_a_scale_ramp_uses_real_time_so_the_driver_can_stop_at_the_bar() {
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, "Scale Clock");
    let (scale, _) = with_scale(&mut drive, 10.0, 11.0, true);
    drive.ramp_stop = Some(scale);
    drive.ramp_mi = Some(0.5);
    drive.ramp_control.clear();

    drive.update_exit(&mut app.ctx, 0.0, 0.1);

    assert!(drive.trip.controlled_ramp);
}

#[test]
fn test_facility_stopping_assistance_brakes_for_a_scale_entrance() {
    let mut app = TestApp::new();
    app.ctx.settings.destination_approach_assist = true;
    let mut drive = a_drive(&mut app, "Scale Assist");
    let (scale, _) = with_scale(&mut drive, 10.0, 11.0, true);
    drive.ramp_stop = Some(scale);
    drive.ramp_mi = Some(0.02);
    drive.ramp_terminal_done = true;
    drive.trip.truck.velocity_mps = mph_to_mps(20.0);

    drive.update_destination_approach_assist(&mut app.ctx);

    assert!(drive.destination_arrival_active);
    assert!(drive.trip.truck.brake > 0.0);
    assert!(app
        .event_lines()
        .iter()
        .any(|line| line.contains("Slowing for the entrance.")));
}

#[test]
fn test_facility_stopping_assistance_does_not_coast_to_a_stop_short_of_the_scale() {
    let mut app = TestApp::new();
    app.ctx.settings.destination_approach_assist = true;
    let mut drive = a_drive(&mut app, "Scale Creep");
    let (scale, _) = with_scale(&mut drive, 10.0, 11.0, true);
    drive.ramp_stop = Some(scale);
    drive.ramp_mi = Some(0.08);
    drive.ramp_terminal_done = true;
    drive.trip.truck.velocity_mps = mph_to_mps(1.0);
    drive.trip.truck.brake = 0.0;
    drive.trip.truck.parking_brake = false;

    drive.update_destination_approach_assist(&mut app.ctx);

    assert!(drive.destination_arrival_active);
    assert!(drive.trip.truck.throttle > 0.0);
    assert!(app
        .event_lines()
        .iter()
        .any(|line| line.contains("Slowing for the entrance.")));
    const { assert!(FACILITY_LANE_ROLL_MPH == 12.0) };
    const { assert!(ARRIVAL_FINAL_CREEP_MI == 200.0 / 5280.0) };
}

#[test]
fn test_facility_stopping_assistance_runs_a_scale_ramp_at_the_lane_speed_not_a_walk() {
    // Owner, 2026-09-03: on a ramp with no terminal control -- a scale's
    // always is -- the assist latched at the top and walked the whole half
    // mile at 12 mph. A lane is road until the final lengths: at 20 mph with
    // 0.4 miles of ramp left there is nothing to shed yet, so the assist's
    // pedal is the throttle, not the brake.
    let mut app = TestApp::new();
    app.ctx.settings.destination_approach_assist = true;
    let mut drive = a_drive(&mut app, "Scale Lane");
    let (scale, _) = with_scale(&mut drive, 10.0, 11.0, true);
    drive.ramp_stop = Some(scale);
    drive.ramp_mi = Some(0.4);
    drive.ramp_terminal_done = true;
    // The lane's own number here: the posted limit, never above the ramp's
    // advisory speed. The hold is that number this far from the bar.
    let (posted_mph, _) = drive.trip.speed_limit_at(drive.trip.position_mi);
    let lane_mph = posted_mph.min(drive.armed_ramp_mph(None));
    assert!(
        lane_mph > FACILITY_LANE_ROLL_MPH + 3.0,
        "this ramp's lane number is {lane_mph:.1} mph; the case needs one above the old walk"
    );
    drive.trip.truck.velocity_mps = mph_to_mps(lane_mph - 3.0);
    drive.trip.truck.brake = 0.0;
    drive.trip.truck.parking_brake = false;

    drive.update_destination_approach_assist(&mut app.ctx);

    assert!(drive.destination_arrival_active);
    assert_eq!(
        drive.trip.truck.brake, 0.0,
        "braked under the lane number {lane_mph:.1}"
    );
    assert_eq!(drive.destination_assist_brake, 0.0);
    assert!(
        drive.trip.truck.throttle > 0.0,
        "no throttle toward the lane number {lane_mph:.1}"
    );

    // Well over the lane's number the stop profile to the bar asks for real
    // brake, and the throttle stays off. (Just over it, the truck is lifted
    // and left to drag: a service floor held down a ramp costs the air the
    // gate needs.)
    drive.trip.truck.throttle = 0.0;
    drive.trip.truck.velocity_mps = mph_to_mps(lane_mph + 30.0);
    drive.update_destination_approach_assist(&mut app.ctx);
    assert!(drive.trip.truck.brake > 0.0);
    assert_eq!(drive.trip.truck.throttle, 0.0);
}

#[test]
fn test_a_casual_hos_mode_never_lets_a_scale_claim_the_exit() {
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, "Jerry");
    let (_scale, plaza) = with_scale(&mut drive, 10.0, 11.0, true);
    app.ctx.settings.hos_mode = "debug_off".to_string();
    drive.trip.position_mi = 8.2;
    drive.trip.truck.velocity_mps = mph_to_mps(54.0);

    assert!(drive
        .scale_claiming_exit(&mut app.ctx, Some(&plaza))
        .is_none());
}

// -- one demand at a time: the armed exit stands down -----------------------------------

#[test]
fn test_a_beginning_stop_stands_down_the_armed_exit() {
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, "Jerry");
    let (_scale, plaza) = with_scale(&mut drive, 10.0, 11.0, true);
    drive.exit_stop = Some(plaza);
    drive.exit_signal_on = true;
    drive.cruise_exit_mph = Some(40.0);

    assert!(drive.stand_down_exit_for_stop(&mut app.ctx));

    assert!(drive.exit_stop.is_none());
    assert!(!drive.exit_signal_on);
    assert!(drive.cruise_exit_mph.is_none());
}

#[test]
fn test_standing_down_with_no_armed_exit_reports_nothing_to_say() {
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, "Jerry");
    assert!(!drive.stand_down_exit_for_stop(&mut app.ctx));
}

// -- the scale bed ---------------------------------------------------------------------

#[test]
fn test_an_open_scale_swells_louder_than_a_closed_one() {
    // Open and closed are not two different ambiences: the swell says
    // "scale", and only its ceiling differs.
    const { assert!(SCALE_BED_OPEN_MAX_VOLUME > SCALE_BED_CLOSED_MAX_VOLUME) };

    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, "Presence");
    let stop = a_scale("Ontario Scale", 10.0);
    drive.trip.stops = vec![stop.clone()];
    drive.trip.posts = vec![open_scale_post(&stop)];
    drive.trip.position_mi = 10.0 - SCALE_BED_START_MI / 2.0;

    drive.update_scale_bed(&mut app.ctx);
    let open = drive.scale_bed_volume;
    assert!(!drive.scale_bed_key.is_empty());

    drive.trip.posts = vec![EnforcementPost {
        anchor: stop.key(),
        ..always_observing_post(stop.at_mi, KIND_SCALE_APRON, 0.5, 1.0)
    }];
    drive.update_scale_bed(&mut app.ctx);
    assert!(drive.scale_bed_volume < open);

    // Past the scale, the bed lets go.
    drive.trip.position_mi = 12.0;
    drive.update_scale_bed(&mut app.ctx);
    assert!(drive.scale_bed_key.is_empty());
    assert_eq!(drive.scale_bed_volume, 0.0);
}

// -- the safety record -----------------------------------------------------------------

#[test]
fn test_the_safety_record_line_says_a_band_and_never_a_trade_acronym() {
    let mut app = TestApp::new();
    let drive = a_drive(&mut app, "Presence");

    let clean = drive.safety_record_line(&mut app.ctx);
    assert!(clean.contains("Safety record: clean"), "{clean}");

    {
        let profile = app.ctx.profile.as_mut().expect("a profile");
        profile.driving_record.citations = 5;
        profile.driving_record.citation_times = vec![0.0; 5];
        profile.driving_record.serious_violations = vec![0.0; 5];
        profile.career.reputation = 10.0;
    }
    let dirty = drive.safety_record_line(&mut app.ctx);
    assert!(dirty.contains("Safety record: targeted"), "{dirty}");
    for jargon in ["ISS", "CSA"] {
        assert!(!clean.contains(jargon));
        assert!(!dirty.contains(jargon));
    }
}

#[test]
fn test_a_clean_record_is_waved_through_and_a_dirty_one_is_not() {
    // The decision is a real seeded draw -- a clean driver is waved through
    // nearly always, not always -- so this pins the odds over a spread of
    // trips rather than one lucky roll.
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, "Presence");
    let stop = a_scale("Ontario Scale", 10.0);

    {
        let profile = app.ctx.profile.as_mut().expect("a profile");
        profile.driving_record.citations = 0;
        profile.driving_record.serious_violations.clear();
        profile.career.reputation = 85.0;
    }
    let clean_pulls = (0..20)
        .filter(|seed| {
            drive.trip_seed = *seed;
            drive.scale_selects_driver(&mut app.ctx, &stop)
        })
        .count();
    assert!(
        clean_pulls <= 4,
        "a clean record is waved through nearly every time ({clean_pulls})"
    );

    {
        let profile = app.ctx.profile.as_mut().expect("a profile");
        profile.driving_record.citations = 5;
        profile.driving_record.citation_times = vec![0.0; 5];
        profile.driving_record.serious_violations = vec![0.0; 5];
        profile.career.reputation = 10.0;
    }
    for seed in 0..6 {
        drive.trip_seed = seed;
        assert!(drive.scale_selects_driver(&mut app.ctx, &stop));
    }
}

// -- the RNG parity canaries -------------------------------------------------------------

#[test]
fn test_seed_one_lands_under_the_eighty_five_percent_catch_chance() {
    // The comment every scale-bypass test in the Python suite leans on:
    // "seed 1 lands under the 85 percent catch chance for this exact scale
    // key, so the stop is deterministic." That claim is a statement about
    // CPython's string seeding, and it is the single most load-bearing piece
    // of RNG parity in the enforcement layer.
    let key = "weigh:Ontario Scale:10.0";
    let roll = PyRandom::new_from_str(&format!("1:scale-bypass:{key}")).random();
    assert!(
        roll < WEIGH_STATION_BYPASS_CATCH_CHANCE,
        "seed 1 must be caught: {roll}"
    );
}

#[test]
fn test_seed_eleven_rolls_over_the_catch_chance_and_gets_away() {
    // The same crossing seed 1 catches gets away clean -- silently, by
    // design.
    let key = "weigh:Ontario Scale:10.0";
    let roll = PyRandom::new_from_str(&format!("11:scale-bypass:{key}")).random();
    assert!(
        roll >= WEIGH_STATION_BYPASS_CATCH_CHANCE,
        "seed 11 must get away: {roll}"
    );
}

#[test]
fn test_the_tableau_draw_is_named_and_seeded_per_post() {
    // The same shape, on this module's own draw: deterministic per tableau,
    // so a reload never changes whether a given post's line carries a reason.
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, "Presence");
    drive.trip_seed = 7;
    let post = always_observing_post(6.0, KIND_MEDIAN, 1.0, 1.0);
    let first = drive.tableau_intro_message(&post);
    let second = drive.tableau_intro_message(&post);
    assert_eq!(first.normal, second.normal);
    assert_eq!(
        post_seed(Some(7), &post.id(), "tableau_intro"),
        "7:police:post:0:6.0:median_post:tableau_intro"
    );
}

// -- the pass trigger --------------------------------------------------------------------

#[test]
fn test_the_pass_earcon_fires_just_past_the_post() {
    const { assert!(PASS_TRIGGER_MI > 0.0 && PASS_TRIGGER_MI < 0.25) };
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, "Presence");
    let log = app.record_audio();
    drive.trip.posts = vec![a_watching_post(5.0)];
    // Short of the trigger: nothing yet.
    drive.enforcement_prev_mi = 4.9;
    drive.trip.position_mi = 5.0;
    drive.update_marked_unit_passes(&mut app.ctx, 4.9);
    assert!(played(&log).is_empty());

    drive.trip.position_mi = 5.0 + PASS_TRIGGER_MI + 0.01;
    drive.update_marked_unit_passes(&mut app.ctx, 5.0);
    assert!(played(&log).iter().any(|key| key == SIGNATURE_KEY));
}

// -- what the officer says they saw ---------------------------------------------------

/// One observation of `what` from a visual post, for the terms below.
fn an_observation(post: &EnforcementPost, what: &str) -> Observation {
    Observation {
        post: post.clone(),
        confidence: 1.0,
        method: post.method.clone(),
        what: what.to_string(),
        detail: String::new(),
    }
}

#[test]
fn test_every_observed_offence_names_itself_and_prices_itself() {
    // Chain law, following too close, lights and lane misuse are the things
    // an officer SEES rather than clocks, and they are priced in
    // models/enforcement -- asked of the constants rather than hardcoded, so
    // a rebalance moves one number.
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, "Presence");
    drive.trip.truck.damage_pct = 95.0;
    let post = always_observing_post(6.0, "work_zone_post", 1.0, 1.0);

    let (summary, fine, back) = drive.observed_stop_terms(&an_observation(&post, WHAT_DAMAGE));
    assert!(
        summary.contains("saw visible truck damage at 95 percent"),
        "{summary}"
    );
    assert_eq!(fine, UNSAFE_DAMAGE_FINE);
    assert_eq!(
        back,
        "Back on the highway. Repair the truck at the next safe stop."
    );

    let (summary, fine, back) = drive.observed_stop_terms(&an_observation(&post, WHAT_CHAINS));
    assert!(
        summary.contains("running the chain control without chains on the drives"),
        "{summary}"
    );
    assert_eq!(fine, CHAIN_LAW_FINE);
    assert_eq!(
        back,
        "Back on the highway. Chain up before the next control."
    );

    let (summary, fine, back) = drive.observed_stop_terms(&an_observation(&post, WHAT_FOLLOWING));
    assert!(
        summary.contains("watched you close right up on the vehicle ahead"),
        "{summary}"
    );
    assert_eq!(fine, FOLLOWING_TOO_CLOSE_FINE);
    assert_eq!(back, "Back on the highway. Leave yourself a gap.");

    let (summary, fine, back) = drive.observed_stop_terms(&an_observation(&post, WHAT_LIGHTS));
    assert!(summary.contains("saw you running dark"), "{summary}");
    assert_eq!(fine, LIGHTS_FINE);
    assert_eq!(back, "Back on the highway. Keep your lights on after dark.");

    // Anything else falls through to the lane-misuse wording and price.
    let (summary, fine, back) = drive.observed_stop_terms(&an_observation(&post, WHAT_LANE));
    assert!(
        summary.contains("pulled you over for lane misuse"),
        "{summary}"
    );
    assert_eq!(fine, LANE_MISUSE_FINE);
    assert_eq!(back, "Back on the highway. Keep right except to pass.");
}

#[test]
fn test_the_spoken_reason_is_the_posts_own_context_label() {
    // "a trooper on this {reason}" is interpolated from the post kind, so a
    // work-zone unit never introduces itself as a weigh station.
    let mut app = TestApp::new();
    let drive = a_drive(&mut app, "Presence");
    let work_zone = always_observing_post(6.0, "work_zone_post", 1.0, 1.0);
    let (summary, _, _) = drive.observed_stop_terms(&an_observation(&work_zone, WHAT_LIGHTS));
    assert!(summary.contains("work zone enforcement"), "{summary}");

    let crossover = a_watching_post(6.0);
    let (summary, _, _) = drive.observed_stop_terms(&an_observation(&crossover, WHAT_LIGHTS));
    assert!(summary.contains("highway enforcement"), "{summary}");
}

#[test]
fn test_overweight_cargo_is_a_real_check_and_red_lights_the_scale() {
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, "Heavy");
    let scale = a_scale("I-90 West Scale", 10.0);
    // 12-ton general load on a stock rig is legal.
    assert!(!drive.cargo_is_overweight());
    let legal = drive.roll_transponder_verdict(&scale, "weigh:legal");
    assert!(legal == "green" || legal == "red", "{legal}");
    // 30 metric tons of cargo on the stock tare is over 80,000 lb GVW.
    drive.trip.truck.cargo_kg = 30.0 * KG_PER_TON;
    assert!(drive.cargo_is_overweight());
    let verdict = drive.roll_transponder_verdict(&scale, "weigh:heavy");
    assert_eq!(verdict, "red");
}

#[test]
fn test_leaving_the_drive_mid_stop_releases_the_siren() {
    // The siren's own dead-man's switch only ticks from inside update(), so
    // it can never time itself out once the state stops running -- leaving
    // to the menu (or a save load, which exits the same way) mid pull-over
    // used to leave it playing until the game closed.
    let mut app = TestApp::new();
    let mut drive = a_drive(&mut app, "Presence");
    drive.pull_over = Some(PULL_OVER_LIGHTS.to_string());
    drive.hold_stop_siren(&mut app.ctx);
    assert!(drive.siren.active);

    State::exit(&mut drive, &mut app.ctx);

    assert!(!drive.siren.active);
}
