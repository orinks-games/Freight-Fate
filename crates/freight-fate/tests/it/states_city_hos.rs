//! Hours of service at the terminal: the bunk room, and the dispatch board's
//! warning about a load your current shift cannot legally cover.
//!
//! These are the `tests/test_hos.py` cases that drive `CityMenuState` and
//! `JobBoardState`. They spent the port as `#[ignore]`d stubs in
//! `crates/ff-core/src/sim/hos/tests.rs`, where they could never run:
//! `ff-core` cannot depend on the game crate, so neither screen is visible
//! from there.

use crate::states_city_support::*;
use ff_core::models::career::LEVEL_XP;
use ff_core::models::jobs::{
    plan_hos, remaining_route_hos_plan, route_drive_hours, Job, JobBoard, OfferOptions,
};
use ff_core::models::profile::Profile;
use ff_core::pyfmt::fmt_f;
use ff_core::sim::hos::limits;
use freight_fate::app::testing::TestApp;
use freight_fate::states::base::{Key, Menu};
use freight_fate::states::city::{open_freight_market, CityMenuState, JobBoardState};
use freight_fate::states::city_pickup::{
    start_loaded_drive, LoadedDriveOptions, PickupFacilityState, PICKUP_CHECK_IN_MIN,
    PICKUP_LOADING_MIN,
};
use freight_fate::states::driving_menu_states::DriveRef;
use freight_fate::states::driving_rest_states::RestStopState;

use super::states_driving_menus_support::{activate, with_drive};

fn approx(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-6 * b.abs().max(1.0)
}

/// `job_with_supported_route(world, city, level, jobs)`.
///
/// An offered `city` job at `level` whose route the world really supports.
/// Endorsement cargo is skipped: an unendorsed test career would be refused
/// for the endorsement, not for the hours these cases are about.
fn job_with_supported_route(app: &TestApp, city: &str, level: i64, jobs: &[Job]) -> Job {
    let acceptable = |job: &Job| {
        job.cargo.credentials.is_empty()
            && matches!(
                app.ctx
                    .world
                    .supported_route(&job.origin, &job.destination, None),
                Ok(Some(_))
            )
    };
    if let Some(job) = jobs.iter().find(|job| acceptable(job)) {
        return job.clone();
    }
    // A shifted job draw (which happens as the map grows) must not make this
    // StopIteration: the case needs a genuine acceptable job, not one seed's.
    for seed in 0..200 {
        let mut board = JobBoard::seeded(app.ctx.world, seed);
        let offers = board.offers(city, &[] as &[&str], OfferOptions::level(level));
        if let Some(job) = offers.iter().find(|job| acceptable(job)) {
            return job.clone();
        }
    }
    panic!("no offered {city} job with a supported route under any seed");
}

/// `JobBoard(ctx.world, seed=2).offers("Austin", set(), level=2)`.
fn austin_offers(app: &TestApp) -> Vec<Job> {
    let mut board = JobBoard::seeded(app.ctx.world, 2);
    board.offers("Austin", &[] as &[&str], OfferOptions::level(2))
}

#[test]
fn loaded_departure_with_mandatory_sleep_has_an_achievable_deadline() {
    let mut app = TestApp::new();
    app.ctx.profile = Some(Profile::named_in("Near HOS", "Austin"));
    app.ctx.settings.hos_mode = "realistic".to_string();
    let offers = austin_offers(&app);
    let job = offers
        .into_iter()
        .filter(|job| job.deadline_game_h < 10.0 && job.cargo.credentials.is_empty())
        .find(|job| {
            matches!(
                app.ctx
                    .world
                    .supported_route(&job.origin, &job.destination, None),
                Ok(Some(_))
            )
        })
        .expect("short cached offer with under ten hours to deliver");
    let route = app
        .ctx
        .world
        .supported_route(&job.origin, &job.destination, None)
        .unwrap()
        .unwrap();
    let stop_mi = route
        .accessible_stop_details(false)
        .iter()
        .find(|stop| stop.name == "Walburg Travel Center & Food Court")
        .expect("a real sleep-capable route stop")
        .at_mi;
    let to_stop_h = route_drive_hours(Some(&route), 0.0, Some(app.ctx.world))
        - route_drive_hours(Some(&route), stop_mi, Some(app.ctx.world));
    assert!(to_stop_h > 0.0 && to_stop_h < 1.0);
    // The board may have cached the offer before loading consumed most of
    // the duty window. The driver can legally reach Walburg, then must sleep.
    let original_deadline = job.deadline_game_h;
    app.ctx.profile.as_mut().unwrap().hos.duty_min = 13.0 * 60.0;
    let clock = &app.ctx.profile.as_ref().unwrap().hos;
    let legal = plan_hos(
        route.miles(),
        Some(&route),
        Some(app.ctx.world),
        Some(clock),
    );
    assert_eq!(legal.sleeps, 1);
    assert!(legal.total_h() > 10.0);
    assert!(original_deadline < 10.0);
    assert!(job.payout_default(legal.total_h(), 0.0) < job.pay);

    start_loaded_drive(&mut app.ctx, job, route, LoadedDriveOptions::default());
    let saved = app
        .ctx
        .profile
        .as_ref()
        .unwrap()
        .active_trip
        .as_ref()
        .unwrap();
    let offered = saved["job"]["deadline_game_h"].as_f64().unwrap();
    println!(
        "cached deadline {original_deadline:.1}h; legal trip {:.1}h; adjusted deadline {offered:.1}h",
        legal.total_h()
    );
    assert!(
        offered >= legal.total_h(),
        "cached deadline {original_deadline:.1}h; legal trip {:.1}h; loaded deadline {offered:.1}h",
        legal.total_h()
    );
    assert!(saved["job"]["deadline_covers_rest"].as_bool().unwrap());
    assert!(
        app.main_lines().join(" ").contains("Deadline extended to ")
            && app
                .main_lines()
                .join(" ")
                .contains("This run needs a 10-hour sleep."),
        "{:?}",
        app.main_lines()
    );
    let drive = app.ctx.state().expect("loaded driving state");
    let stop = with_drive(&drive, |d| {
        d.trip.position_mi = stop_mi;
        d.trip.game_minutes = to_stop_h * 60.0;
        d.trip
            .stops
            .iter()
            .find(|stop| stop.name == "Walburg Travel Center & Food Court")
            .expect("the stop is available to the truck")
            .clone()
    });
    app.ctx
        .profile
        .as_mut()
        .unwrap()
        .hos
        .drive(to_stop_h * 60.0);
    assert!(app.ctx.profile.as_ref().unwrap().hos.duty_min < 14.0 * 60.0);
    let mut rest = RestStopState::with_drive(DriveRef::of(&drive), stop, false);
    Menu::enter(&mut rest, &mut app.ctx);
    app.clear_speech();
    activate(&mut rest, &mut app.ctx, "Sleep 10 hours");
    let preview = app.main_lines().join(" ");
    assert!(
        preview.contains(&format!(
            "The delivery deadline will be in {} hours",
            fmt_f(offered - to_stop_h - 10.0, 1)
        )),
        "{preview}"
    );
    activate(&mut rest, &mut app.ctx, "Sleep 10 hours");
    assert_eq!(app.ctx.profile.as_ref().unwrap().hos.duty_min, 0.0);
    assert_eq!(
        app.ctx
            .profile
            .as_ref()
            .unwrap()
            .active_trip
            .as_ref()
            .unwrap()["job"]["deadline_game_h"]
            .as_f64(),
        Some(offered)
    );
    with_drive(&drive, |d| {
        assert!(approx(d.trip.game_minutes, (to_stop_h + 10.0) * 60.0));
        assert_eq!(d.job.deadline_game_h, offered);
        let remaining = remaining_route_hos_plan(
            &d.route,
            stop_mi,
            Some(app.ctx.world),
            &app.ctx.profile.as_ref().unwrap().hos,
        );
        let lawful_arrival = to_stop_h + 10.0 + remaining.total_h();
        assert!(lawful_arrival < offered);
        assert!(d.job.payout_default(lawful_arrival, 0.0) > d.job.pay);
    });
}

// -- the terminal bunk room ------------------------------------------------------------

#[test]
fn test_city_sleep_resets_hours_and_advances_the_clock() {
    // A spent duty window used to follow you into the city with no way to
    // sleep it off short of driving (illegally) to a rest stop.
    let mut app = TestApp::new();
    career(&mut app, "Bunk Room", "Austin");
    let city = CityMenuState::new(&app.ctx, false);
    app.push_state(city);

    let before = {
        let p = profile_mut(&mut app);
        p.hos.drive(660.0); // a fully spent shift
        p.fatigue = 75.0;
        p.game_hours
    };

    select::<CityMenuState>(&mut app, "Sleep 10 hours");

    let p = profile(&app);
    assert!(approx(p.game_hours, before + 10.0), "{}", p.game_hours);
    assert_eq!(p.hos.driving_min, 0.0);
    assert_eq!(p.hos.duty_min, 0.0);
    assert_eq!(p.fatigue, 0.0);
}

#[test]
fn test_city_sleep_when_already_rested_needs_a_second_enter() {
    // Pressing Enter on Sleep right after a reset used to quietly burn another
    // 10 hours; a rested driver now gets a warning first.
    let mut app = TestApp::new();
    app.ctx.profile = Some(Profile::named_in("Rested", "Austin"));
    let city = CityMenuState::new(&app.ctx, false);
    app.push_state(city);
    let before = profile(&app).game_hours;

    select::<CityMenuState>(&mut app, "Sleep 10 hours"); // first Enter: warning only
    assert_eq!(profile(&app).game_hours, before);
    key(&mut app, Key::Return); // confirm
    assert!(approx(profile(&app).game_hours, before + 10.0));

    // Rested again after sleeping: the next Enter warns again, and moving off
    // the item cancels the pending confirmation.
    select::<CityMenuState>(&mut app, "Sleep 10 hours");
    assert!(approx(profile(&app).game_hours, before + 10.0));
    key(&mut app, Key::Down);
    key(&mut app, Key::Up);
    key(&mut app, Key::Return);
    assert!(
        approx(profile(&app).game_hours, before + 10.0),
        "warned, not slept"
    );

    // A tired driver sleeps on the first press, as before.
    {
        let p = profile_mut(&mut app);
        p.hos.drive(300.0);
        p.fatigue = 40.0;
    }
    key(&mut app, Key::Return);
    assert!(approx(profile(&app).game_hours, before + 20.0));
    assert_eq!(profile(&app).fatigue, 0.0);
}

// -- the dispatch board's hours warning --------------------------------------------------

#[test]
fn test_dispatch_warns_before_accepting_job_that_exceeds_current_hos() {
    let mut app = TestApp::new();
    app.record_audio();
    app.ctx.profile = Some(Profile::named_in("HOS Dispatch", "Austin"));
    app.ctx.settings.hos_mode = "realistic".to_string();
    let drive_limit = limits("realistic").expect("realistic has limits").0;
    profile_mut(&mut app).hos.drive(drive_limit - 30.0);
    let jobs = austin_offers(&app);
    let job = job_with_supported_route(&app, "Austin", 2, &jobs);
    let mut board = JobBoardState::new(&app.ctx, vec![job]);

    app.clear_speech();
    board.accept(&mut app.ctx, 0);
    app.ctx.run_deferred();

    assert!(
        app.main_lines()
            .last()
            .is_some_and(|line| line.contains("Needs an extra rest on this shift's hours")),
        "{:?}",
        app.main_lines()
    );
    assert!(profile(&app).active_trip.is_none());

    board.accept(&mut app.ctx, 0);
    app.ctx.run_deferred();

    assert!(profile(&app).active_trip.is_some());
}

#[test]
fn test_dispatch_warns_when_the_load_fits_the_window_only_on_paper() {
    // Chippewa Falls to Duluth, owner, 2026-09-12: a 5-hour job accepted on
    // 4 h 17 m of duty window passed the board's check by a few minutes,
    // and the drive ended with a forced 10-hour sleep 5.2 hours past the
    // deadline. A fit with less than the planning cushion to spare must warn.
    let (_, duty_limit, _) = limits("realistic").expect("realistic has limits");
    let pickup_min = PICKUP_CHECK_IN_MIN + PICKUP_LOADING_MIN;

    let board_with_margin = |margin_min: f64| {
        let mut app = TestApp::new();
        app.record_audio();
        app.ctx.profile = Some(Profile::named_in("Paper Fit", "Austin"));
        app.ctx.settings.hos_mode = "realistic".to_string();
        let jobs = austin_offers(&app);
        let job = job_with_supported_route(&app, "Austin", 2, &jobs);
        let route = app
            .ctx
            .world
            .supported_route(&job.origin, &job.destination, None)
            .expect("route lookup")
            .expect("supported route");
        let loaded_min = route_drive_hours(Some(&route), 0.0, Some(app.ctx.world)) * 60.0;
        profile_mut(&mut app).hos.duty_min = duty_limit - pickup_min - loaded_min - margin_min;
        let board = JobBoardState::new(&app.ctx, vec![job]);
        (app, board)
    };

    // Ten minutes to spare on the loaded route alone: on paper it fits, and
    // that is exactly the case that lost the delivery.
    let (mut app, mut board) = board_with_margin(10.0);
    app.clear_speech();
    board.accept(&mut app.ctx, 0);
    app.ctx.run_deferred();
    assert!(
        app.main_lines()
            .last()
            .is_some_and(|line| line.contains("Needs an extra rest on this shift's hours")),
        "{:?}",
        app.main_lines()
    );
    assert!(profile(&app).active_trip.is_none());
    // One live TestApp per thread: release the first before the second.
    drop(board);
    drop(app);

    // Two hours to spare covers the deadhead and the cushion: no warning,
    // the load is accepted on the first press.
    let (mut app, mut board) = board_with_margin(120.0);
    app.clear_speech();
    board.accept(&mut app.ctx, 0);
    app.ctx.run_deferred();
    assert!(
        !app.main_lines()
            .iter()
            .any(|line| line.contains("Needs an extra rest on this shift's hours")),
        "{:?}",
        app.main_lines()
    );
    assert!(profile(&app).active_trip.is_some());
}

#[test]
fn test_dispatch_board_warns_when_all_generated_jobs_exceed_current_hos() {
    let mut app = TestApp::new();
    app.record_audio();
    let mut p = Profile::named_in("All Risky", "Austin");
    p.career.xp = LEVEL_XP[7]; // senior: browsable board
    app.ctx.profile = Some(p);
    app.ctx.settings.hos_mode = "realistic".to_string();
    let drive_limit = limits("realistic").expect("realistic has limits").0;
    profile_mut(&mut app).hos.drive(drive_limit - 10.0);
    let jobs: Vec<Job> = austin_offers(&app).into_iter().take(5).collect();
    assert_eq!(jobs.len(), 5);
    let mut board = JobBoardState::new(&app.ctx, jobs.clone());

    app.clear_speech();
    Menu::announce_entry(&mut board, &mut app.ctx);
    app.ctx.run_deferred();

    assert!(
        app.main_lines()
            .iter()
            .any(|line| line.contains("Every load needs an extra rest on your current hours")),
        "{:?}",
        app.main_lines()
    );
    for index in 0..jobs.len() {
        app.clear_speech();
        board.accept(&mut app.ctx, index);
        app.ctx.run_deferred();
        assert!(
            app.main_lines()
                .last()
                .is_some_and(|line| line.contains("Needs an extra rest on this shift's hours")),
            "job {index}: {:?}",
            app.main_lines()
        );
        assert!(profile(&app).active_trip.is_none());
    }

    board.accept(&mut app.ctx, jobs.len() - 1);
    app.ctx.run_deferred();

    assert!(profile(&app).active_trip.is_some());
}

#[test]
fn test_dispatch_does_not_warn_after_hours_reset() {
    // A full 10-hour reset must clear the dispatch hours warning, even for
    // multi-day runs: the route's own sleeps are budgeted into the deadline,
    // so only hours already spent this shift are worth warning about.
    let mut app = TestApp::new();
    app.record_audio();
    app.ctx.profile = Some(Profile::named_in("Rested", "Austin"));
    app.ctx.settings.hos_mode = "realistic".to_string();
    {
        let p = profile_mut(&mut app);
        p.hos.drive(600.0); // a nearly spent shift...
        p.hos.sleep(); // ...wiped by the 10-hour reset
    }
    let jobs: Vec<Job> = austin_offers(&app).into_iter().take(5).collect();
    let mut board = JobBoardState::new(&app.ctx, jobs.clone());

    app.clear_speech();
    Menu::announce_entry(&mut board, &mut app.ctx);
    app.ctx.run_deferred();

    assert!(
        !app.main_lines()
            .last()
            .is_some_and(|line| line.contains("extra rest")),
        "{:?}",
        app.main_lines()
    );

    let job = job_with_supported_route(&app, "Austin", 2, &jobs);
    let index = jobs
        .iter()
        .position(|listed| listed.destination == job.destination)
        .unwrap_or(0);
    app.clear_speech();
    board.accept(&mut app.ctx, index);
    app.ctx.run_deferred();

    assert!(
        !app.main_lines()
            .last()
            .is_some_and(|line| line.contains("Needs an extra rest on this shift's hours")),
        "{:?}",
        app.main_lines()
    );
    assert!(profile(&app).active_trip.is_some());
}

/// Warn on a board load at 8 pm with the shift nearly spent, sleep 10
/// hours in the bunk room, reopen the board. Returns the warned load and
/// the market days before and after the sleep.
fn warn_sleep_and_reopen(app: &mut TestApp, xp: f64) -> (Job, i64, i64) {
    app.record_audio();
    let mut p = Profile::named_in("Slept On It", "Austin");
    p.career.xp = xp;
    p.game_hours = 20.0;
    app.ctx.profile = Some(p);
    app.ctx.settings.hos_mode = "realistic".to_string();
    let drive_limit = limits("realistic").expect("realistic has limits").0;
    profile_mut(app).hos.drive(drive_limit - 10.0);
    let day_before = profile(app).market_day();

    open_freight_market(&mut app.ctx);
    app.ctx.run_deferred();
    let count = with_state::<JobBoardState, _>(app, |b, _| b.jobs.len());
    let mut warned = None;
    for index in 0..count {
        app.clear_speech();
        with_state_mut::<JobBoardState, _>(app, |b, ctx| b.accept(ctx, index));
        app.ctx.run_deferred();
        if app
            .main_lines()
            .iter()
            .any(|l| l.contains("Needs an extra rest on this shift's hours"))
        {
            warned = Some(with_state::<JobBoardState, _>(app, |b, _| {
                b.jobs[index].clone()
            }));
            break;
        }
    }
    let warned = warned.expect("a load on the board draws the hours warning");
    assert!(profile(app).active_trip.is_none());

    // Back to the terminal and into the bunk room, as the warning says.
    app.ctx.pop_state();
    app.ctx.run_deferred();
    {
        let p = profile_mut(app);
        p.game_hours += 10.0;
        p.hos.sleep();
    }
    let day_after = profile(app).market_day();
    open_freight_market(&mut app.ctx);
    app.ctx.run_deferred();
    (warned, day_before, day_after)
}

#[test]
fn test_the_load_you_sleep_for_is_still_on_the_board_after_the_rest() {
    // Owner, 2026-10-06: finish a drive, pick the next load, hear the hours
    // warning, sleep -- and the load was gone. The sleep crossed midnight,
    // the new day rebuilt the board, and nothing remembered the pick.
    let mut app = TestApp::new();
    let (warned, day_before, day_after) = warn_sleep_and_reopen(&mut app, LEVEL_XP[7]);
    assert_ne!(day_before, day_after, "the sleep crosses into a new day");

    let (index, focused) = with_state::<JobBoardState, _>(&app, |b, _| {
        let index = b.jobs.iter().position(|job| {
            job.destination == warned.destination
                && job.origin_location == warned.origin_location
                && job.pay == warned.pay
        });
        (index, Menu::menu(b).index)
    });
    let index = index.expect("the warned load is on the new day's board");
    assert_eq!(focused, index, "the board opens on the load you slept for");
    assert!(warned.deadline_covers_rest, "posted on the tired clock");
    // Rested, the offer no longer claims your hours will force a rest.
    let carried = with_state::<JobBoardState, _>(&app, |b, _| b.jobs[index].clone());
    assert!(!carried.deadline_covers_rest);

    // Rested now: Enter takes it with no second warning.
    app.clear_speech();
    with_state_mut::<JobBoardState, _>(&mut app, |b, ctx| b.accept(ctx, index));
    app.ctx.run_deferred();
    assert!(
        !app.main_lines()
            .iter()
            .any(|l| l.contains("Needs an extra rest on this shift's hours")),
        "{:?}",
        app.main_lines()
    );
    assert!(profile(&app).active_trip.is_some());
}

#[test]
fn test_an_assigned_load_you_sleep_for_is_still_the_assignment() {
    let mut app = TestApp::new();
    let (warned, day_before, day_after) = warn_sleep_and_reopen(&mut app, 0.0);
    assert_ne!(day_before, day_after);
    let assigned = with_state::<JobBoardState, _>(&app, |b, _| {
        assert!(b.assigned_mode());
        b.assigned_job().clone()
    });
    assert_eq!(assigned.destination, warned.destination);
    assert_eq!(assigned.origin_location, warned.origin_location);
    assert_eq!(assigned.pay, warned.pay);
}

// -- a load staged at the home yard ----------------------------------------------------

#[test]
fn test_a_load_staged_at_the_home_yard_opens_the_pickup_without_a_deadhead() {
    // The home terminal is a facility in the city's own list, so dispatch
    // can hand out a load that ships from it -- and the first agent
    // playtest (2026-09-02) then drove a two-mile "deadhead from the
    // terminal to the terminal" on every assigned load. The shipping office
    // is a walk across the yard: the pickup opens here.
    let mut app = TestApp::new();
    app.record_audio();
    app.ctx.profile = Some(Profile::named_in("Yard Load", "Austin"));
    let terminal = app
        .ctx
        .world
        .home_terminal("Austin")
        .expect("Austin has a yard");
    let jobs = austin_offers(&app);
    let mut job = job_with_supported_route(&app, "Austin", 2, &jobs);
    job.origin_location = terminal.name.clone();
    let mut board = JobBoardState::new(&app.ctx, vec![job]);

    app.clear_speech();
    board.accept(&mut app.ctx, 0);
    app.ctx.run_deferred();

    let top = app.ctx.state().expect("a state");
    assert!(
        top.borrow().as_any().is::<PickupFacilityState>(),
        "the shipping office, not a drive to it"
    );
    assert!(
        profile(&app)
            .active_trip
            .as_ref()
            .is_some_and(|trip| trip["kind"] == "pickup"),
        "a save resumes at the pickup"
    );
    let said = app.main_lines().join(" ");
    assert!(said.contains("The load is here in the yard."), "{said}");
    assert!(!said.contains("Deadhead"), "{said}");
}
