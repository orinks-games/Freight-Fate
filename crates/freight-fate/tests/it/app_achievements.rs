//! The App()-bound half of `tests/test_achievements.py`.
//!
//! `ff_core::achievements` owns the catalog and the pure award machinery, and
//! its own `tests.rs` covers those. Everything here drives the game crate --
//! `GameContext::award_achievement`, the delivery settlement, the badge
//! trackers at the wheel, and the main-menu achievements screens -- which
//! `ff-core` cannot see, so these cases live on this side of the dependency.

use std::path::Path;
use std::sync::Arc;

use ff_core::achievements::{achievement_by_id, achievements_in_category, categories};
use ff_core::data::world::get_world;
use ff_core::models::jobs::{Job, JobBoard, OfferOptions};
use ff_core::models::profile::Profile;
use ff_core::radio::{effective_range_miles, RadioReception, RadioStation};
use ff_core::sim::real_traffic::wall_clock;
use ff_core::sim::trip_models::{NavigationCue, TripEvent, TripEventData, TripEventKind};
use ff_core::speech_text::{achievement_announced, SpokenMessage};
use serde_json::{json, Value};

use freight_fate::app::testing::TestApp;
use freight_fate::net::testing::ClosureTransport;
use freight_fate::net::{NetError, SharedTransport};
use freight_fate::online_journal::JournalOutbox;
use freight_fate::online_presence::OnlineIdentity;

use crate::states_main_menu_support::*;
use freight_fate::app::share;
use freight_fate::states::base::Key;
use freight_fate::states::city_pickup::{PickupFacilityState, PickupOptions};
use freight_fate::states::driving::DrivingState;
use freight_fate::states::driving_core::DRIVE_PHASE_DELIVERY;
use freight_fate::states::driving_menu_states::ArrivalState;
use freight_fate::states::main_menu::{
    AchievementCareerState, AchievementCategoryState, AchievementsState, MainMenuState,
};

// -- rigging -----------------------------------------------------------------------

/// The first unlocked job Chicago offers this career (Python's
/// `next(job for job in JobBoard(world).offers(...) if not job.locked_reason(...))`).
fn an_unlocked_job(app: &TestApp, level: i64) -> Job {
    let profile = app.ctx.profile.as_ref().expect("a career");
    let endorsements: Vec<&str> = profile.career.endorsements().into_iter().collect();
    let career_level = profile.career.level();
    let jobs = JobBoard::new(app.ctx.world, None, None).offers(
        &profile.current_city,
        &endorsements,
        OfferOptions {
            level,
            market: Some(&profile.market),
            ..OfferOptions::default()
        },
    );
    jobs.into_iter()
        .find(|job| {
            job.locked_reason(&endorsements, career_level, None, false)
                .is_empty()
        })
        .expect("Chicago offers an unlocked job")
}

/// A delivery that has just reached the gate on time, ready to settle.
fn a_finished_delivery(app: &mut TestApp, job: Job) -> DrivingState {
    let route = app
        .ctx
        .world
        .supported_route_options(&job.origin, &job.destination, 1)
        .expect("the world routes")
        .into_iter()
        .next()
        .expect("the corridor is supported");
    let deadline = job.deadline_game_h;
    let mut driving = DrivingState::new(
        &mut app.ctx,
        job,
        route,
        Some(4),
        DRIVE_PHASE_DELIVERY,
        None,
    );
    driving.trip.game_minutes = deadline * 30.0; // half the clock used: on time
    driving.speeding_tickets = 0;
    driving
}

/// Settle one delivery out of Chicago end to end.
fn deliver_one(app: &mut TestApp) {
    let job = an_unlocked_job(app, 0);
    let mut driving = a_finished_delivery(app, job);
    ArrivalState::new(&mut app.ctx, &mut driving);
}

/// Whether the career holds `id`.
fn earned(app: &TestApp, id: &str) -> bool {
    app.ctx
        .profile
        .as_ref()
        .expect("a career")
        .achievements
        .iter()
        .any(|a| a == id)
}

/// A journal that can queue safely in an app test without reaching the network.
fn offline_journal(path: &Path) -> JournalOutbox {
    let transport: SharedTransport = Arc::new(ClosureTransport(
        |_url: &str,
         _payload: Option<&Value>,
         _headers: &[(String, String)],
         _method: Option<&str>| { Err(NetError::other("OSError", "offline")) },
    ));
    JournalOutbox::with(
        Some(OnlineIdentity::new(
            "driver-1234",
            &format!("ffd_{}", "a".repeat(64)),
        )),
        true,
        path,
        transport,
        wall_clock(),
    )
}

// -- GameContext::award_achievement -------------------------------------------------

#[test]
fn test_account_new_achievement_queues_once_across_two_careers() {
    // The journals are deliberately separate so their own event-id dedupe
    // cannot hide a missing account-ledger check.
    let mut app = TestApp::new();
    let first_journal = offline_journal(&app.data_dir.path().join("first-journal.json"));
    app.ctx.services.journal = first_journal.clone();
    app.ctx.profile = Some(Profile::named("First Career"));
    let first_path = app.ctx.profile.as_ref().expect("a career").path();

    assert!(app.ctx.award_achievement("first_delivery").is_some());

    let second_journal = offline_journal(&app.data_dir.path().join("second-journal.json"));
    app.ctx.services.journal = second_journal.clone();
    app.ctx.profile = Some(Profile::named("Second Career"));
    let second_path = app.ctx.profile.as_ref().expect("a career").path();

    assert!(app.ctx.award_achievement("first_delivery").is_some());
    assert_eq!(
        first_journal.items().len() + second_journal.items().len(),
        1
    );
    assert_eq!(
        Profile::load(&first_path).unwrap().achievements,
        ["first_delivery"]
    );
    assert_eq!(
        Profile::load(&second_path).unwrap().achievements,
        ["first_delivery"]
    );
}

#[test]
fn test_ledger_write_failure_keeps_career_award_but_suppresses_public_event() {
    let mut app = TestApp::new();
    app.ctx.services.journal = offline_journal(&app.data_dir.path().join("journal.json"));
    let blocked_data_dir = app.data_dir.path().join("blocked-data-dir");
    std::fs::write(&blocked_data_dir, b"a file, not a directory").unwrap();
    app.ctx.account_achievements =
        freight_fate::account_achievements::AccountAchievements::empty(&blocked_data_dir);
    app.ctx.profile = Some(Profile::named("Ledger Failure"));
    let audio = app.record_audio();
    app.clear_speech();

    assert!(app.ctx.award_achievement("first_delivery").is_some());
    assert!(earned(&app, "first_delivery"));
    assert_eq!(
        app.main_lines(),
        vec![achievement_announced("Signed, Sealed, Hauled").normal]
    );
    assert_eq!(audio.borrow().played.len(), 1);
    assert!(app.ctx.services.journal.items().is_empty());
}

#[test]
fn test_ledger_write_failure_still_announces_the_arrival_award() {
    let mut app = TestApp::new();
    let blocked_data_dir = app.data_dir.path().join("blocked-data-dir");
    std::fs::write(&blocked_data_dir, b"a file, not a directory").unwrap();
    app.ctx.account_achievements =
        freight_fate::account_achievements::AccountAchievements::empty(&blocked_data_dir);
    app.ctx.profile = Some(Profile::named_in("Ledger Failure", "Chicago"));
    let job = an_unlocked_job(&app, 0);
    let mut driving = a_finished_delivery(&mut app, job);

    let arrival = ArrivalState::new(&mut app.ctx, &mut driving);

    assert!(earned(&app, "first_delivery"));
    assert!(
        arrival
            .summary_parts
            .iter()
            .any(|line| line.contains("New achievement")),
        "{:?}",
        arrival.summary_parts
    );
}

#[test]
fn test_award_speaks_the_short_line_and_logs_the_flavor() {
    // Live, the announce is the earcon and the name only (R9); the full
    // flavor record still rides the award and reaches the review log.
    let mut app = TestApp::new();
    app.ctx.profile = Some(Profile::named("Badge Driver"));
    let log = app.record_audio();
    app.clear_speech();

    let award = app
        .ctx
        .award_achievement("first_delivery")
        .expect("a fresh career has not earned it");

    assert_eq!(
        app.main_lines(),
        vec![achievement_announced(award.achievement.name).normal]
    );
    assert!(award.message.normal.starts_with("New achievement!"));
    assert_eq!(
        app.ctx
            .message_log
            .messages
            .last()
            .expect("a log entry")
            .text,
        award.message.normal
    );
    let played: Vec<String> = log
        .borrow()
        .played
        .iter()
        .map(|(key, _, _)| key.clone())
        .collect();
    assert_eq!(played, vec!["ui/level_up".to_string()]);
}

#[test]
fn test_award_achievement_persists_and_deduplicates_notification() {
    let mut app = TestApp::new();
    app.ctx.profile = Some(Profile::named("Badge Driver"));
    let log = app.record_audio();
    app.clear_speech();

    let first = app.ctx.award_achievement("first_delivery");
    let second = app.ctx.award_achievement("first_delivery");

    assert!(first.is_some());
    assert!(second.is_none());
    let played: Vec<String> = log
        .borrow()
        .played
        .iter()
        .map(|(key, _, _)| key.clone())
        .collect();
    assert_eq!(played, vec!["ui/level_up".to_string()]);
    let path = app.ctx.profile.as_ref().expect("a career").path();
    let reloaded = Profile::load(&path).expect("the career saved");
    assert_eq!(reloaded.achievements, vec!["first_delivery".to_string()]);
}

#[test]
fn test_event_achievement_speaks_through_screen_reader() {
    // An achievement is never road information: it goes to the screen reader
    // channel whatever the caller asks for, never to the event voice.
    let mut app = TestApp::new();
    app.ctx.profile = Some(Profile::named("Screen Reader Badges"));
    app.clear_speech();

    let award = app
        .ctx
        .award_achievement_with("first_delivery", true, true)
        .expect("a fresh career has not earned it");

    assert_eq!(
        app.main_lines(),
        vec![achievement_announced(award.award.achievement.name).normal]
    );
    assert!(app.event_lines().is_empty(), "{:?}", app.event_lines());
}

#[test]
fn test_suppressed_award_collects_without_chime_or_speech() {
    let mut app = TestApp::new();
    app.ctx.profile = Some(Profile::named("Quiet Badges"));
    let log = app.record_audio();
    app.clear_speech();

    let award = app
        .ctx
        .award_achievement_with("first_delivery", false, false);

    assert!(award.is_some());
    assert!(app.main_lines().is_empty(), "{:?}", app.main_lines());
    assert!(log.borrow().played.is_empty(), "{:?}", log.borrow().played);
    assert!(
        app.ctx.achievement_notice.starts_with("New achievement!"),
        "{}",
        app.ctx.achievement_notice
    );
}

// -- the delivery settlement --------------------------------------------------------

#[test]
fn test_return_trip_badge_needs_the_reverse_of_the_last_route() {
    let mut app = TestApp::new();
    app.ctx.profile = Some(Profile::named_in("Shuttle Run", "Chicago"));
    let job = an_unlocked_job(&app, 0);
    // The previous delivery ran this exact lane the other way around.
    app.ctx
        .profile
        .as_mut()
        .expect("a career")
        .achievement_stats
        .insert(
            "last_route".to_string(),
            json!([job.destination.clone(), job.origin.clone()]),
        );
    let (origin, destination) = (job.origin.clone(), job.destination.clone());
    let mut driving = a_finished_delivery(&mut app, job);

    ArrivalState::new(&mut app.ctx, &mut driving);

    assert!(earned(&app, "return_trip"));
    let stats = &app
        .ctx
        .profile
        .as_ref()
        .expect("a career")
        .achievement_stats;
    // The lane just driven becomes the new benchmark for the next run.
    assert_eq!(
        stats["last_route"],
        json!([origin.clone(), destination.clone()])
    );
    // A career's home city is pinned by the first delivery's origin.
    assert_eq!(stats["home_city"], json!(origin));
}

#[test]
fn test_delivery_settlement_awards_only_first_delivery_on_a_first_run() {
    // A fresh run one earns first_delivery alone.
    //
    // first_on_time, clean_delivery, and speed_limit_saint would all clear on
    // a typical first run too (on time, no damage, no speeding) -- the rookie
    // chain's delivery-count floors are what keep them from piling on top of
    // first_delivery before the player has done more than one run.
    let mut app = TestApp::new();
    app.ctx.profile = Some(Profile::named_in("Settlement Badges", "Chicago"));
    let job = an_unlocked_job(&app, 0);
    let mut driving = a_finished_delivery(&mut app, job);

    let arrival = ArrivalState::new(&mut app.ctx, &mut driving);

    assert!(earned(&app, "first_delivery"));
    for id in ["first_on_time", "clean_delivery", "speed_limit_saint"] {
        assert!(!earned(&app, id), "{id} needs more than one delivery");
    }
    // R9: the settlement names the run's badges but no longer reads their
    // flavor paragraphs -- the story stays in the log. The announce keeps the
    // exclamation style (owner preference), singular or plural.
    let summary = arrival.summary_parts.join(" ");
    assert!(summary.contains("Signed, Sealed, Hauled"), "{summary}");
    assert!(summary.contains("New achievement"), "{summary}");
    assert!(summary.contains('!'), "{summary}");
    assert!(!summary.contains("No fanfare needed"), "{summary}");
    let path = app.ctx.profile.as_ref().expect("a career").path();
    let reloaded = Profile::load(&path).expect("the career saved");
    assert_eq!(
        reloaded.achievements,
        app.ctx.profile.as_ref().expect("a career").achievements
    );
}

#[test]
fn test_rookie_chain_achievements_clear_their_delivery_floors() {
    // first_on_time/clean_delivery/speed_limit_saint spread across runs 2-4.
    // five_deliveries, unaffected by the rookie chain, still lands on run 5.
    let mut app = TestApp::new();
    app.ctx.profile = Some(Profile::named_in("Rookie Chain", "Chicago"));

    deliver_one(&mut app);
    assert_eq!(app.ctx.profile.as_ref().unwrap().career.deliveries, 1);
    assert!(!earned(&app, "first_on_time"));
    assert!(!earned(&app, "clean_delivery"));
    assert!(!earned(&app, "speed_limit_saint"));

    deliver_one(&mut app);
    assert_eq!(app.ctx.profile.as_ref().unwrap().career.deliveries, 2);
    assert!(earned(&app, "first_on_time"));
    assert!(!earned(&app, "clean_delivery"));
    assert!(!earned(&app, "speed_limit_saint"));

    deliver_one(&mut app);
    assert_eq!(app.ctx.profile.as_ref().unwrap().career.deliveries, 3);
    assert!(earned(&app, "clean_delivery"));
    assert!(!earned(&app, "speed_limit_saint"));

    deliver_one(&mut app);
    assert_eq!(app.ctx.profile.as_ref().unwrap().career.deliveries, 4);
    assert!(earned(&app, "speed_limit_saint"));
    assert!(!earned(&app, "five_deliveries"));

    deliver_one(&mut app);
    assert_eq!(app.ctx.profile.as_ref().unwrap().career.deliveries, 5);
    assert!(earned(&app, "five_deliveries"));
}

#[test]
fn test_pickup_completion_awards_the_merged_first_day_badge() {
    // first_dispatch/air_ready/first_pickup are retired in favor of first_day.
    //
    // A fresh career's pickup no longer dings first_dispatch, air_ready, and
    // first_pickup in a row -- it dings first_day once instead.
    let mut app = TestApp::new();
    app.ctx.profile = Some(Profile::named_in("First Day Run", "Chicago"));
    let job = an_unlocked_job(&app, 0);
    let pickup = PickupFacilityState::new(
        &app.ctx,
        job.clone(),
        PickupOptions {
            checked_in: true,
            ..PickupOptions::default()
        },
    );
    app.push_shared(share(pickup));
    app.ctx.run_deferred();
    // Getting the freight on is the first row now that check-in is done: the
    // dock, or the drop-and-hook yard, whichever this shipper runs.
    let primary = labels::<PickupFacilityState>(&app)[0].clone();
    select::<PickupFacilityState>(&mut app, &primary);
    finish_timed_state(&mut app);

    assert!(earned(&app, "first_day"));
    for id in ["first_dispatch", "first_pickup", "air_ready"] {
        assert!(!earned(&app, id), "{id} is retired as an award");
    }

    let mut driving = a_finished_delivery(&mut app, job);
    ArrivalState::new(&mut app.ctx, &mut driving);

    assert!(earned(&app, "first_day"));
    assert!(earned(&app, "first_delivery"));
    for id in ["first_dispatch", "first_pickup", "air_ready"] {
        assert!(!earned(&app, id), "{id} is retired as an award");
    }
    // Delivery-count floors keep the rest of the rookie chain off run one.
    for id in ["first_on_time", "clean_delivery", "speed_limit_saint"] {
        assert!(!earned(&app, id), "{id} needs more than one delivery");
    }
}

/// `finish_timed_state(app)`: run the timed message screen out.
fn finish_timed_state(app: &mut TestApp) {
    use freight_fate::states::base::TimedMessageState;
    let remaining = with_state::<TimedMessageState, _>(app, |s, _| s.remaining);
    let state = app.state().expect("a timed state");
    {
        let mut borrowed = state.borrow_mut();
        borrowed.update(&mut app.ctx, remaining + 0.01);
    }
    app.ctx.run_deferred();
}

#[test]
fn test_eastbound_badge_fires_only_on_an_eastbound_delivery() {
    // The first-delivery badge is direction-neutral now; "eastbound" is its own.
    assert!(!achievement_by_id("first_delivery")
        .expect("the catalog has it")
        .name
        .to_lowercase()
        .contains("eastbound"));

    let mut app = TestApp::new();
    app.ctx.profile = Some(Profile::named_in("Eastbound Run", "chicago_il_us"));
    let world = get_world();
    let origin_lon = world.city("Chicago").expect("Chicago is on the map").lon;
    // Find any unlocked, net-eastbound job Chicago offers. Which seed yields
    // one shifts as the map grows, so search seeds instead of pinning one --
    // the case only needs a genuine eastbound delivery, not a specific one.
    let (endorsements, career_level, market) = {
        let profile = app.ctx.profile.as_ref().expect("a career");
        let endorsements: Vec<&str> = profile.career.endorsements().into_iter().collect();
        (endorsements, profile.career.level(), profile.market.clone())
    };
    let job = (0..200)
        .flat_map(|seed| {
            JobBoard::seeded(world, seed).offers(
                "Chicago",
                &endorsements,
                OfferOptions {
                    level: 5,
                    market: Some(&market),
                    ..OfferOptions::default()
                },
            )
        })
        .find(|candidate| {
            candidate
                .locked_reason(&endorsements, career_level, None, false)
                .is_empty()
                && world
                    .cities
                    .get(&candidate.destination)
                    .is_some_and(|city| city.lon > origin_lon + 1.0)
        })
        .expect("expected Chicago to offer a net-eastbound job under some seed");
    let mut driving = a_finished_delivery(&mut app, job);

    ArrivalState::new(&mut app.ctx, &mut driving);

    assert!(earned(&app, "eastbound_delivery"));
    assert!(!earned(&app, "westbound_delivery"));
}

// -- the road ------------------------------------------------------------------------

#[test]
fn test_state_crossing_keeps_gameplay_prompt_before_achievement() {
    // Where the truck is comes first; the badge is decoration on top of it.
    let mut app = TestApp::new();
    app.ctx.profile = Some(Profile::named_in("State Line", "Chicago"));
    let job = an_unlocked_job(&app, 0);
    let mut driving = a_finished_delivery(&mut app, job);
    driving.trip.game_minutes = 0.0;
    app.clear_speech();

    let cue = NavigationCue::new(
        "state:test",
        "state_crossing",
        10.0,
        "crossing from Illinois into Missouri",
        "Crossing into Missouri near St. Louis.",
    );
    let data = TripEventData {
        cue: Some(cue),
        ..TripEventData::default()
    };
    let event = TripEvent {
        kind: TripEventKind::StateCrossing,
        message: SpokenMessage::new("Crossing into Missouri near St. Louis."),
        data,
    };
    driving.handle_trip_event(&mut app.ctx, &event);

    assert_eq!(
        app.event_lines(),
        vec!["Crossing into Missouri near St. Louis.".to_string()]
    );
    // Live, only the name (R9); the flavor waits in the log and menu.
    assert_eq!(
        app.main_lines().first().cloned().unwrap_or_default(),
        "New achievement! Kept It Between the Lines."
    );
}

/// `_driving_for_badges()`: a drive with the physics quiet, for exercising
/// badge triggers only.
fn a_drive_for_badges(app: &mut TestApp) -> DrivingState {
    a_quiet_drive(app, "Buffalo", "Rochester")
}

fn a_quiet_drive(app: &mut TestApp, origin: &str, destination: &str) -> DrivingState {
    let world = get_world();
    app.ctx.profile = Some(Profile::named_in("Badge Tracker", origin));
    let route = world
        .supported_route(origin, destination, None)
        .expect("the world routes")
        .expect("the corridor is supported");
    let mut job = Job::new(
        &ff_core::models::jobs::CARGO_CATALOG["general"],
        12.0,
        origin,
        "company yard",
        destination,
        route.miles(),
        1000.0,
        12.0,
    );
    job.destination_location = format!("{destination} freight market");
    let mut drive = DrivingState::new(&mut app.ctx, job, route, None, DRIVE_PHASE_DELIVERY, None);
    // `quiet_trip` plus `open_limits`: an empty road, no random hazards, a
    // pinned sky. The badge trackers read the truck, not the corridor, so
    // nothing here needs the limits lifted the way Python's rig did.
    drive.trip.hazard_check_mi = 1e9;
    drive.trip.inspection_check_mi = 1e9;
    drive.trip.traffic_manager.rolling_bubble = false;
    drive.trip.set_npc_vehicles(Vec::new());
    drive.trip.weather.current = ff_core::sim::weather::WeatherKind::Clear;
    let truck = &mut drive.trip.truck;
    truck.start_engine();
    truck.set_air_ready(false);
    truck.transmission.gear = truck.transmission.num_gears();
    truck.grade = 0.0;
    drive
}

/// The same quiet rig on a corridor that crosses three state lines.
fn a_drive_across_states(app: &mut TestApp) -> DrivingState {
    a_quiet_drive(app, "Chicago", "Cleveland")
}

#[test]
fn test_the_number_that_means_nothing_takes_a_whole_mile() {
    // Sixty-nine has to be held, not merely passed through.
    let mut app = TestApp::new();
    let mut d = a_drive_for_badges(&mut app);

    // Passing through the number on the way somewhere else is not holding it.
    for speed in [60.0, 69.0, 75.0, 69.0] {
        d.trip.truck.velocity_mps = speed / 2.23694;
        d.track_driving_badges(&mut app.ctx, 1.0 / 60.0);
    }
    assert!(!earned(&app, "sixty_nine_mph"));

    for _ in 0..(70 * 60) {
        // a mile at sixty-nine takes about fifty seconds
        d.trip.truck.velocity_mps = 69.0 / 2.23694;
        d.track_driving_badges(&mut app.ctx, 1.0 / 60.0);
    }
    assert!(earned(&app, "sixty_nine_mph"));
}

#[test]
fn test_the_old_national_limit_takes_a_whole_mile_too() {
    // The double nickel, held rather than passed through, same as its
    // sibling above -- and it must not fall out of ordinary highway driving,
    // which is the whole risk with a number this close to a posted limit.
    let mut app = TestApp::new();
    let mut d = a_drive_for_badges(&mut app);

    for speed in [45.0, 55.0, 65.0, 55.0] {
        d.trip.truck.velocity_mps = speed / 2.23694;
        d.track_driving_badges(&mut app.ctx, 1.0 / 60.0);
    }
    assert!(!earned(&app, "fifty_five_mph"));

    // Most of a mile is not a mile.
    for _ in 0..(40 * 60) {
        d.trip.truck.velocity_mps = 55.0 / 2.23694;
        d.track_driving_badges(&mut app.ctx, 1.0 / 60.0);
    }
    assert!(!earned(&app, "fifty_five_mph"));

    for _ in 0..(40 * 60) {
        d.trip.truck.velocity_mps = 55.0 / 2.23694;
        d.track_driving_badges(&mut app.ctx, 1.0 / 60.0);
    }
    assert!(earned(&app, "fifty_five_mph"));
    // And the other held number stayed where it was.
    assert!(!earned(&app, "sixty_nine_mph"));
}

#[test]
fn test_eighty_eight_miles_an_hour_is_noticed() {
    let mut app = TestApp::new();
    let mut d = a_drive_for_badges(&mut app);

    d.trip.truck.velocity_mps = 80.0 / 2.23694;
    d.track_driving_badges(&mut app.ctx, 1.0 / 60.0);
    assert!(!earned(&app, "eighty_eight_mph"));

    d.trip.truck.velocity_mps = 89.0 / 2.23694;
    d.track_driving_badges(&mut app.ctx, 1.0 / 60.0);
    assert!(earned(&app, "eighty_eight_mph"));
}

#[test]
fn test_a_jake_only_descent_is_ruined_by_one_touch_of_the_brake() {
    // The badge is for the gear discipline, so the service brake resets it.
    let mut app = TestApp::new();
    let mut d = a_drive_for_badges(&mut app);
    d.trip.truck.grade = -0.05;
    d.trip.truck.engine_brake_stage = 2;

    fn roll(d: &mut DrivingState, app: &mut TestApp, seconds: f64, brake: f64) {
        for _ in 0..(seconds * 60.0) as i64 {
            d.trip.truck.velocity_mps = 55.0 / 2.23694;
            d.trip.truck.brake = brake;
            d.track_driving_badges(&mut app.ctx, 1.0 / 60.0);
        }
    }

    roll(&mut d, &mut app, 100.0, 0.0); // most of the way there
    roll(&mut d, &mut app, 1.0, 0.4); // one touch, and it starts over
    assert!(!earned(&app, "jake_only_descent"));
    roll(&mut d, &mut app, 200.0, 0.0);
    assert!(earned(&app, "jake_only_descent"));
}

#[test]
fn test_cooking_the_drums_is_its_own_badge() {
    let mut app = TestApp::new();
    let mut d = a_drive_for_badges(&mut app);

    d.trip.truck.brake_temp_c = d.trip.truck.brake_fade_onset_c() - 50.0;
    d.track_driving_badges(&mut app.ctx, 1.0 / 60.0);
    assert!(!earned(&app, "brake_smoke"));

    d.trip.truck.brake_temp_c = d.trip.truck.brake_fade_onset_c() + 10.0;
    d.track_driving_badges(&mut app.ctx, 1.0 / 60.0);
    assert!(earned(&app, "brake_smoke"));
}

/// A station the reception tests can hold, with the reach the fringe rule
/// measures against.
fn a_test_station(id: &str, range_miles: f64) -> RadioStation {
    let mut station = RadioStation::new(id, "Test Radio", "", "country", "test");
    station.range_miles = range_miles;
    station
}

/// Mileposts on this route where the truck stands in three different states.
///
/// Python replaced `trip.state_at` with a lambda per state. There is no seam
/// for it here, so the tally is walked across a corridor that really does
/// cross three state lines -- which is a stronger rig than the patch was.
fn three_state_miles(d: &mut DrivingState) -> Vec<f64> {
    let total = d.trip.total_miles();
    let mut seen: Vec<String> = Vec::new();
    let mut miles = Vec::new();
    let mut mile = 0.0;
    while mile < total {
        let state = d.trip.state_at(Some(mile));
        if !state.is_empty() && !seen.contains(&state) {
            seen.push(state);
            miles.push(mile);
            if miles.len() == 3 {
                break;
            }
        }
        mile += 1.0;
    }
    miles
}

#[test]
fn test_the_radio_badges_follow_the_signal() {
    // Holding a station across three states, and catching one at the fringe.
    let mut app = TestApp::new();
    let mut d = a_drive_across_states(&mut app);
    let station = a_test_station("ff:test", 100.0);
    let reception = RadioReception::new(station.clone(), None, 1.0, "test");
    d.radio_signal_factor = 1.0;
    let miles = three_state_miles(&mut d);
    assert_eq!(miles.len(), 3, "the corridor must cross three states");

    for mile in &miles[..2] {
        d.trip.position_mi = *mile;
        d.track_radio_badges(&mut app.ctx, &reception);
    }
    assert!(!earned(&app, "radio_three_states"));
    d.trip.position_mi = miles[2];
    d.track_radio_badges(&mut app.ctx, &reception);
    assert!(earned(&app, "radio_three_states"));

    // A catch well past the flat contour -- only height does that -- is a
    // skip. Riding a station into its own static near the edge is not. Taken
    // from `effective_range_miles` rather than arithmetic on `range_miles`:
    // the flat contour is the doubled reach held under RADIO_MAX_REACH_MI,
    // and hardcoding either factor left this assertion stale when the cap
    // landed (2026-08-18).
    let contour = effective_range_miles(&station, None);
    let far = RadioReception::new(station.clone(), Some(contour * 1.3), 1.0, "test");
    d.track_radio_badges(&mut app.ctx, &far);
    assert!(earned(&app, "radio_fringe_catch"));

    // And the near edge does not qualify, on a career that has not earned it.
    drop(d);
    drop(app);
    let mut app = TestApp::new();
    let mut d = a_drive_for_badges(&mut app);
    d.radio_signal_factor = 1.0;
    let near_edge = RadioReception::new(station, Some(contour * 1.05), 1.0, "test");
    d.track_radio_badges(&mut app.ctx, &near_edge);
    assert!(!earned(&app, "radio_fringe_catch"));
}

#[test]
fn test_a_new_station_restarts_the_three_state_tally() {
    let mut app = TestApp::new();
    let mut d = a_drive_across_states(&mut app);
    d.radio_signal_factor = 1.0;
    let miles = three_state_miles(&mut d);
    assert_eq!(miles.len(), 3, "the corridor must cross three states");

    for (index, mile) in miles.iter().enumerate() {
        d.trip.position_mi = *mile;
        // A different station every state: no single signal held three.
        let reception = RadioReception::new(
            a_test_station(&format!("ff:{index}"), 0.0),
            None,
            1.0,
            "test",
        );
        d.track_radio_badges(&mut app.ctx, &reception);
    }

    assert!(!earned(&app, "radio_three_states"));
}

// -- the main-menu achievements screens ----------------------------------------------

#[test]
fn test_main_menu_achievement_path_is_keyboard_accessible() {
    let mut app = TestApp::new();
    let mut profile = Profile::named("Menu Badges");
    profile.achievements.push("first_delivery".to_string());
    profile.save().expect("the career saves");
    app.push_state(MainMenuState::new());

    select::<MainMenuState>(&mut app, "Achievements");
    assert!(is::<AchievementCareerState>(&app));
    assert!(
        current_label::<AchievementCareerState>(&app).starts_with("Menu Badges: 1 of"),
        "{}",
        current_label::<AchievementCareerState>(&app)
    );
    key(&mut app, Key::Return);

    // The career picker opens the category menu, not a flat badge list.
    assert!(is::<AchievementsState>(&app));
    assert!(
        current_label::<AchievementsState>(&app).starts_with("Summary: 1 of"),
        "{}",
        current_label::<AchievementsState>(&app)
    );

    // first_delivery ("Signed, Sealed, Hauled") lives in "Out on the Road";
    // the still-locked first_dispatch ("Breaker, Breaker") lives in "Career
    // and Rank" -- reaching both proves categories actually route to
    // different badges, not just a relabelled flat list.
    select::<AchievementsState>(&mut app, "Out on the Road");
    assert!(is::<AchievementCategoryState>(&app));
    assert_eq!(
        with_state::<AchievementCategoryState, _>(&app, |s, _| s.category.id),
        "road"
    );
    let rows = labels::<AchievementCategoryState>(&app);
    assert!(
        rows.iter().any(|r| r.starts_with("Earned: Signed")),
        "{rows:?}"
    );
    app.clear_speech();
    select::<AchievementCategoryState>(&mut app, "Earned: Signed");
    assert!(
        app.main_lines()
            .last()
            .is_some_and(|l| l.starts_with("Earned: Signed")),
        "{:?}",
        app.main_lines()
    );

    key(&mut app, Key::Escape);
    assert!(is::<AchievementsState>(&app));

    select::<AchievementsState>(&mut app, "Career and Rank");
    assert!(is::<AchievementCategoryState>(&app));
    assert_eq!(
        with_state::<AchievementCategoryState, _>(&app, |s, _| s.category.id),
        "career"
    );
    let rows = labels::<AchievementCategoryState>(&app);
    assert!(
        rows.iter()
            .any(|r| r.starts_with("Locked: Breaker, Breaker")),
        "{rows:?}"
    );
}

#[test]
fn test_category_with_nothing_earned_still_reads_naturally() {
    let mut app = TestApp::new();
    let profile = Profile::named("Fresh Driver");
    let category = categories()
        .iter()
        .find(|c| c.id == "hidden")
        .expect("the Deep Cuts category");
    let achs = achievements_in_category(category.id);
    let count = achs.len();
    app.clear_speech();

    app.push_state(AchievementCategoryState::new(profile, category, achs));

    assert!(
        app.main_lines()
            .last()
            .is_some_and(|l| l.starts_with(&format!("Deep Cuts. 0 of {count} earned."))),
        "{:?}",
        app.main_lines()
    );
    let rows = labels::<AchievementCategoryState>(&app);
    for row in &rows[..rows.len() - 1] {
        assert!(
            row.starts_with("Locked: A Secret the Manifest Is Keeping"),
            "{rows:?}"
        );
    }
}

/// Every badge in the catalog is either awarded somewhere in the shipping
/// code or listed below as deliberately retired.
///
/// Found by audit on 2026-09-20: `thrifty_run` and `coffee_regular` had never
/// been awardable in either runtime -- no award site, no test, no retirement
/// note, from the day they were written. Nothing caught it, because nothing
/// was looking. This looks.
///
/// It reads the source rather than playing the game, so it proves reachability
/// and not correctness: a badge can be wired here and still fire at the wrong
/// moment. What it does catch is the failure that has actually happened --
/// copy written for a badge nobody wired up.
#[test]
fn test_every_badge_is_awarded_somewhere_or_named_as_retired() {
    use std::collections::BTreeSet;
    use std::fs;
    use std::path::Path;

    // Merged into "first_day" at the pickup, by design; their catalog rows
    // stay so the cloud validator's allow-list never sees a removed id.
    const RETIRED: [&str; 3] = ["first_dispatch", "first_pickup", "air_ready"];

    fn sources(dir: &Path, out: &mut String) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if path.is_dir() {
                if name != "tests" && name != "playtest" {
                    sources(&path, out);
                }
            } else if name.ends_with(".rs") && name != "catalog.rs" && name != "tests.rs" {
                out.push_str(&fs::read_to_string(&path).unwrap_or_default());
            }
        }
    }

    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/");
    let mut code = String::new();
    sources(&root.join("ff-core/src"), &mut code);
    sources(&root.join("freight-fate/src"), &mut code);
    // Comments name retired ids in prose; strip them so a note is not a wire.
    let code: String = code
        .lines()
        .filter(|line| !line.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n");

    let retired: BTreeSet<&str> = RETIRED.into_iter().collect();
    let unwired: Vec<&str> = ff_core::achievements::ACHIEVEMENTS
        .iter()
        .map(|a| a.id)
        .filter(|id| !retired.contains(id) && !code.contains(&format!("\"{id}\"")))
        .collect();
    assert!(
        unwired.is_empty(),
        "these badges have copy but no way to earn them: {unwired:?}"
    );

    // And a retired badge must stay retired. Here the question is narrower --
    // is it AWARDED -- because a bare mention is not a wire: the training
    // stage enum spells "first_dispatch" as its own stage id, which is a name
    // collision and not a badge.
    let revived: Vec<&str> = RETIRED
        .into_iter()
        .filter(|id| {
            code.contains(&format!("award_achievement(\"{id}\")"))
                || code.contains(&format!("push(&mut ids, \"{id}\")"))
        })
        .collect();
    assert!(
        revived.is_empty(),
        "retired badges are wired again: {revived:?}"
    );
}

/// Eight miles to the gallon over the whole run earns the badge, and a
/// thirstier run does not. Unearnable until 2026-09-20: the copy had been in
/// the catalog from the start with no award site anywhere.
#[test]
fn test_a_light_footed_run_earns_its_mileage_badge() {
    let mut app = TestApp::new();
    app.ctx.profile = Some(Profile::named("Sipper"));
    let job = an_unlocked_job(&app, 0);
    let mut driving = a_finished_delivery(&mut app, job);

    // 400 miles on 40 gallons is ten to the gallon.
    driving.trip.position_mi = 400.0;
    driving.trip.fuel_used_gal = 40.0;
    ArrivalState::new(&mut app.ctx, &mut driving);
    assert!(earned(&app, "thrifty_run"));
    drop(app);

    let mut app = TestApp::new();
    app.ctx.profile = Some(Profile::named("Heavy Foot"));
    let job = an_unlocked_job(&app, 0);
    let mut driving = a_finished_delivery(&mut app, job);
    // The same 400 miles on 80 gallons is five, which the model calls normal.
    driving.trip.position_mi = 400.0;
    driving.trip.fuel_used_gal = 80.0;
    ArrivalState::new(&mut app.ctx, &mut driving);
    assert!(!earned(&app, "thrifty_run"));
}

/// A run that burned nothing cannot claim the mileage badge: a resumed trip
/// starts its fuel tally at zero, and zero gallons is not infinite mileage.
#[test]
fn test_a_run_with_no_fuel_burned_claims_no_mileage() {
    let mut app = TestApp::new();
    app.ctx.profile = Some(Profile::named("No Burn"));
    let job = an_unlocked_job(&app, 0);
    let mut driving = a_finished_delivery(&mut app, job);
    driving.trip.position_mi = 400.0;
    driving.trip.fuel_used_gal = 0.0;
    ArrivalState::new(&mut app.ctx, &mut driving);
    assert!(!earned(&app, "thrifty_run"));
}
