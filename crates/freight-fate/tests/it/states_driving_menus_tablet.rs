//! `states/driving_radio_app.rs` and `states/driving_school.rs`: the Radio
//! app on the driver tablet, and the sandboxed practice road.
//!
//! Ported from `tests/test_radio_app.py` (its app half; the pure
//! `RadioState.search` cases live with the radio port), the drive cases of
//! `tests/test_radio_favorites.py`, and `tests/test_driving_school.py`.

use ff_core::models::profile::Profile;
use ff_core::sim::vehicle::TruckState;

use freight_fate::app::testing::TestApp;
use freight_fate::app::SharedState;
use freight_fate::states::base::{Key, Menu};
use freight_fate::states::driving_core::Instructor;
use freight_fate::states::driving_menu_states::{DriveRef, DrivingStatusScreenState};
use freight_fate::states::driving_radio_app::{
    RadioAppState, RadioCategoryState, RadioSearchEntryState, RadioStationListState,
};
use freight_fate::states::driving_school::{
    DrivingSchoolState, LessonKind, RollingBasicsLesson, SchoolDrivingState,
};

use crate::states_driving_menus_support::*;

/// `radio_app`: a Denver drive with the whole dial on, and the Radio app open.
///
/// The recording audio stands in for the fixture's `play_radio_stream` patch:
/// the null backend refuses every stream, which would send the dial straight
/// past a web station onto the next one that will play.
fn a_radio_drive(app: &mut TestApp) -> SharedState {
    app.record_audio();
    app.ctx.settings.radio_streamer_safe = false;
    a_drive_between(app, "Denver", "Salt Lake City", "Radio App")
}

// -- the Radio app front page ---------------------------------------------------------------

#[test]
fn test_the_front_page_reports_what_is_playing_and_the_power_state() {
    let mut app = TestApp::new();
    let drive = a_radio_drive(&mut app);
    with_drive(&drive, |d| {
        d.trip.truck.start_engine();
    });
    let mut state = RadioAppState::new(DriveRef::of(&drive));
    let rows = build_labels(&mut state, &mut app.ctx);
    let now_playing = drive_and_ctx(&drive, &mut app, |d, ctx| d.radio_now_playing_text(ctx));
    assert_eq!(rows[0], now_playing);
    assert!(
        rows[1].starts_with("Radio: on, tuned to ") || rows[1].starts_with("Radio: off, tuned to "),
        "{rows:?}"
    );
    assert!(rows.iter().any(|r| r == "Search stations"), "{rows:?}");
    assert!(
        rows.iter().any(|r| r.starts_with("Stations in range:")),
        "{rows:?}"
    );
    assert_eq!(rows.last().map(String::as_str), Some("Back to Driver apps"));
}

#[test]
fn test_the_power_row_switches_the_radio_and_says_so_on_the_row() {
    let mut app = TestApp::new();
    let drive = a_radio_drive(&mut app);
    with_drive(&drive, |d| {
        d.trip.truck.start_engine();
        d.radio.enabled = true;
    });
    let mut state = RadioAppState::new(DriveRef::of(&drive));
    Menu::enter(&mut state, &mut app.ctx);
    activate(&mut state, &mut app.ctx, "Radio: on");
    assert!(!with_drive(&drive, |d| d.radio.enabled));
    assert!(
        labels(&state, &app.ctx)[1].starts_with("Radio: off"),
        "{:?}",
        labels(&state, &app.ctx)
    );
}

// -- searching the dial ------------------------------------------------------------------------

/// Type a query into the search field on the stack.
fn type_query(app: &mut TestApp, text: &str) {
    for ch in text.chars() {
        app.handle_event(&typed(ch));
    }
}

#[test]
fn test_search_tunes_a_station_by_name() {
    let mut app = TestApp::new();
    let drive = a_radio_drive(&mut app);
    with_drive(&drive, |d| {
        d.trip.truck.start_engine();
    });
    let mut state = RadioAppState::new(DriveRef::of(&drive));
    activate(&mut state, &mut app.ctx, "Search stations");
    assert!(top_is::<RadioSearchEntryState>(&app));

    type_query(&mut app, "phoenix fire");
    app.handle_event(&key(Key::Return));
    assert!(top_is::<RadioStationListState>(&app));

    let rows = with_top_ctx::<RadioStationListState, _>(&mut app, build_labels);
    assert!(
        rows[0].starts_with("Phoenix Fire FM, Web radio, always available"),
        "{rows:?}"
    );

    app.clear_speech();
    app.handle_event(&key(Key::Return));
    assert_eq!(
        with_drive(&drive, |d| d.radio.station_id.clone()),
        "phoenix-fire-fm"
    );
    assert!(
        app.main_lines()
            .iter()
            .any(|line| line.contains("Phoenix Fire FM")),
        "{:?}",
        app.main_lines()
    );
    // The row now says so.
    let rows = with_top_ctx::<RadioStationListState, _>(&mut app, build_labels);
    assert!(rows[0].contains("tuned now"), "{rows:?}");
}

#[test]
fn test_a_search_with_no_match_stays_in_the_field() {
    let mut app = TestApp::new();
    let drive = a_radio_drive(&mut app);
    let mut state = RadioAppState::new(DriveRef::of(&drive));
    activate(&mut state, &mut app.ctx, "Search stations");
    type_query(&mut app, "zzqxv");
    app.clear_speech();
    app.handle_event(&key(Key::Return));
    assert!(top_is::<RadioSearchEntryState>(&app));
    assert!(
        last(&app).starts_with("No stations match zzqxv"),
        "{}",
        last(&app)
    );
}

#[test]
fn test_an_empty_search_asks_for_something_to_look_for() {
    let mut app = TestApp::new();
    let drive = a_radio_drive(&mut app);
    let mut state = RadioAppState::new(DriveRef::of(&drive));
    activate(&mut state, &mut app.ctx, "Search stations");
    app.clear_speech();
    app.handle_event(&key(Key::Return));
    assert!(top_is::<RadioSearchEntryState>(&app));
    assert_eq!(last(&app), "Type something to search for first.");
}

#[test]
fn test_an_out_of_range_favorite_says_so_instead_of_tuning() {
    let mut app = TestApp::new();
    let drive = a_radio_drive(&mut app);
    let station = with_drive(&drive, |d| {
        // Any terrestrial station on the dial that does not come in here.
        d.radio
            .catalog
            .iter()
            .find(|s| !s.real_stream && !s.fallback)
            .cloned()
            .expect("a terrestrial station")
    });
    let tuned_before = with_drive(&drive, |d| d.radio.station_id.clone());
    let mut list = RadioStationListState::search(
        DriveRef::of(&drive),
        "far",
        vec![(station.clone(), None)],
        1,
    );
    let rows = build_labels(&mut list, &mut app.ctx);
    assert!(rows[0].contains("out of range here"), "{rows:?}");
    app.clear_speech();
    activate(&mut list, &mut app.ctx, &station.display_name());
    assert_eq!(
        last(&app),
        format!("{} is out of range here.", station.display_name())
    );
    assert_eq!(
        with_drive(&drive, |d| d.radio.station_id.clone()),
        tuned_before
    );
}

#[test]
fn test_favorites_are_saved_listed_and_tuned_from_the_app() {
    let mut app = TestApp::new();
    let drive = a_radio_drive(&mut app);
    with_drive(&drive, |d| {
        d.trip.truck.start_engine();
    });
    let mut state = RadioAppState::new(DriveRef::of(&drive));
    Menu::enter(&mut state, &mut app.ctx);

    let tuned = with_drive(&drive, |d| d.radio.current_station().display_name());
    app.clear_speech();
    activate(
        &mut state,
        &mut app.ctx,
        &format!("Save {tuned} to favorites"),
    );
    assert_eq!(last(&app), format!("Saved {tuned} to favorites."));
    let rows = labels(&state, &app.ctx);
    assert!(
        rows.contains(&format!("Remove {tuned} from favorites")),
        "{rows:?}"
    );
    assert!(rows.contains(&"Favorites: 1 saved".to_string()), "{rows:?}");

    activate(&mut state, &mut app.ctx, "Favorites:");
    assert!(top_is::<RadioStationListState>(&app));
    let rows = with_top_ctx::<RadioStationListState, _>(&mut app, build_labels);
    assert!(rows[0].starts_with(&tuned), "{rows:?}");
    assert!(rows[0].ends_with("favorite"), "{rows:?}");
}

#[test]
fn test_the_tab_radio_screen_favorites_count_opens_the_list() {
    let mut app = TestApp::new();
    let drive = a_radio_drive(&mut app);
    drive_and_ctx(&drive, &mut app, |d, ctx| {
        d.trip.truck.start_engine();
        d.toggle_radio_favorite(ctx);
    });
    let tuned = with_drive(&drive, |d| d.radio.current_station().display_name());
    let mut state = DrivingStatusScreenState::new(DriveRef::of(&drive), "radio");
    Menu::enter(&mut state, &mut app.ctx);
    let rows = labels(&state, &app.ctx);
    assert!(
        rows.contains(&"Favorites saved: 1.".to_string()),
        "{rows:?}"
    );

    activate(&mut state, &mut app.ctx, "Favorites saved:");
    assert!(top_is::<RadioStationListState>(&app));
    let rows = with_top_ctx::<RadioStationListState, _>(&mut app, build_labels);
    assert!(rows[0].starts_with(&tuned), "{rows:?}");
}

#[test]
fn test_an_empty_favourites_list_says_how_to_fill_it() {
    let mut app = TestApp::new();
    let drive = a_radio_drive(&mut app);
    let mut list = RadioStationListState::new(DriveRef::of(&drive), "favorites");
    let rows = build_labels(&mut list, &mut app.ctx);
    assert_eq!(
        rows[0],
        "No favorites saved yet. The Radio app saves the tuned station."
    );
    assert_eq!(rows[1], "Back");
}

// -- the driving school ---------------------------------------------------------------------

fn a_student(app: &mut TestApp) {
    let mut profile = Profile::named_in("Student", "denver_co_us");
    profile.set_money(12_345.0);
    app.ctx.profile = Some(profile);
}

/// Hand the practice drive's instructor one hook, then put it back.
fn lesson_step(app: &mut TestApp, f: impl FnOnce(&mut dyn Instructor, &mut TestApp)) {
    let state = app.ctx.state().expect("the practice drive");
    let mut tutorial = {
        let mut borrowed = state.borrow_mut();
        borrowed
            .as_any_mut()
            .downcast_mut::<SchoolDrivingState>()
            .expect("a practice drive")
            .drive_mut()
            .tutorial
            .take()
            .expect("an instructor")
    };
    f(tutorial.as_mut(), app);
    let mut borrowed = state.borrow_mut();
    if let Some(school) = borrowed.as_any_mut().downcast_mut::<SchoolDrivingState>() {
        school.drive_mut().tutorial = Some(tutorial);
    }
}

/// A stand-in truck carrying just the speed the lesson is watching, the way
/// Python handed the lesson `drive.truck` directly.
fn truck_at(app: &TestApp, mph: f64, parking_brake: bool) -> TruckState {
    let mut truck = TruckState::new(app.ctx.profile.as_ref().expect("a career").truck_specs());
    truck.velocity_mps = mph / 2.2369362920544;
    truck.parking_brake = parking_brake;
    truck
}

#[test]
fn test_school_lesson_is_a_sandbox_and_restores_the_real_profile() {
    let mut app = TestApp::new();
    a_student(&mut app);
    app.ctx.push_state(DrivingSchoolState::new());
    let rows = with_top_ctx::<DrivingSchoolState, _>(&mut app, build_labels);
    assert_eq!(rows[0], "Lesson 1: Rolling basics");
    assert_eq!(rows[1], "Back to terminal");

    with_top_ctx::<DrivingSchoolState, _>(&mut app, |school, ctx| {
        activate(school, ctx, "Lesson 1")
    });
    assert!(top_is::<SchoolDrivingState>(&app));
    // The drive runs on a throwaway copy, not the career.
    assert!(app.ctx.school_sandbox);
    assert!(app.ctx.school_real_profile.is_some());
    assert_eq!(app.ctx.profile.as_ref().expect("a career").name, "Student");

    // Sandbox saves never reach disk.
    let save_path = app.ctx.profile.as_ref().expect("a career").path();
    app.ctx.save_profile();
    assert!(!save_path.exists(), "the sandbox wrote {save_path:?}");

    // Run the lesson: engine, parking brake, roll to 30, stop.
    lesson_step(&mut app, |lesson, app| lesson.begin(&mut app.ctx));
    lesson_step(&mut app, |lesson, app| {
        lesson.on_engine_started(&mut app.ctx)
    });
    lesson_step(&mut app, |lesson, app| {
        lesson.on_parking_brake_released(&mut app.ctx)
    });
    // Sandbox spending stays on the copy.
    app.ctx.profile.as_mut().expect("a career").spend(500.0);
    let rolling = truck_at(&app, 31.0, false);
    lesson_step(&mut app, |lesson, app| {
        lesson.update(&mut app.ctx, 1.0 / 60.0, &rolling)
    });
    let stopped = truck_at(&app, 0.0, false);
    lesson_step(&mut app, |lesson, app| {
        lesson.update(&mut app.ctx, 1.0 / 60.0, &stopped)
    });
    app.ctx.run_deferred();

    // Lesson completion pops back to the school with the career restored.
    assert!(top_is::<DrivingSchoolState>(&app));
    assert!(!app.ctx.school_sandbox);
    assert!(app.ctx.school_real_profile.is_none());
    assert_eq!(
        app.ctx.profile.as_ref().expect("a career").money(),
        12_345.0
    );
}

#[test]
fn test_school_air_reminder_matches_the_gauge() {
    // The lesson's timed nudge used to say "wait for air pressure" even
    // after the air was up (the first-run tutorial had the same fault,
    // agent drive 2026-09-01). With air ready the reminder is just P.
    let mut app = TestApp::new();
    a_student(&mut app);
    app.ctx.push_state(DrivingSchoolState::new());
    with_top_ctx::<DrivingSchoolState, _>(&mut app, |school, ctx| {
        activate(school, ctx, "Lesson 1")
    });
    lesson_step(&mut app, |lesson, app| lesson.begin(&mut app.ctx));
    lesson_step(&mut app, |lesson, app| {
        lesson.on_engine_started(&mut app.ctx)
    });
    app.clear_speech();
    let mut parked = truck_at(&app, 0.0, true);
    parked.set_air_ready(true);
    // Past the lesson's hint delay in one step.
    lesson_step(&mut app, |lesson, app| {
        lesson.update(&mut app.ctx, 31.0, &parked)
    });
    let brake = app.ctx.control_hint("parking_brake");
    assert_eq!(
        app.main_lines(),
        vec![format!(
            "Reminder: air is ready. Press {brake} to release the parking brake."
        )]
    );
}

#[test]
fn test_escaping_a_lesson_restores_the_profile_too() {
    let mut app = TestApp::new();
    a_student(&mut app);
    app.ctx.push_state(DrivingSchoolState::new());
    with_top_ctx::<DrivingSchoolState, _>(&mut app, |school, ctx| {
        activate(school, ctx, "Lesson 1")
    });
    assert!(app.ctx.school_sandbox);
    app.ctx.profile.as_mut().expect("a career").set_money(1.0);

    // Any pop of the practice drive restores, no matter how it happens.
    app.ctx.pop_state();
    app.ctx.run_deferred();
    assert!(top_is::<DrivingSchoolState>(&app));
    assert!(!app.ctx.school_sandbox);
    assert_eq!(
        app.ctx.profile.as_ref().expect("a career").money(),
        12_345.0
    );
}

#[test]
fn test_real_saves_still_write_after_school() {
    let mut app = TestApp::new();
    a_student(&mut app);
    app.ctx.push_state(DrivingSchoolState::new());
    with_top_ctx::<DrivingSchoolState, _>(&mut app, |school, ctx| {
        activate(school, ctx, "Lesson 1")
    });
    app.ctx.pop_state(); // leave the lesson
    app.ctx.run_deferred();
    let save_path = app.ctx.profile.as_ref().expect("a career").path();
    app.ctx.save_profile();
    assert!(
        save_path.exists(),
        "the real career did not reach {save_path:?}"
    );
}

#[test]
fn test_the_lesson_walks_its_stages_in_order() {
    let mut app = TestApp::new();
    a_student(&mut app);
    app.ctx.settings.automatic_transmission = true;
    let mut lesson = RollingBasicsLesson::new();
    assert_eq!(lesson.stage, 0);
    lesson.begin(&mut app.ctx);
    lesson.on_engine_started(&mut app.ctx);
    assert_eq!(lesson.stage, 1);
    lesson.on_parking_brake_released(&mut app.ctx);
    assert_eq!(lesson.stage, 2);
    let rolling = truck_at(&app, 31.0, false);
    lesson.update(&mut app.ctx, 1.0 / 60.0, &rolling);
    assert_eq!(lesson.stage, 3);
    assert!(!lesson.done);
}

#[test]
fn test_a_manual_lesson_waits_for_first_gear() {
    let mut app = TestApp::new();
    a_student(&mut app);
    app.ctx.settings.automatic_transmission = false;
    let mut lesson = RollingBasicsLesson::new();
    lesson.begin(&mut app.ctx);
    lesson.on_engine_started(&mut app.ctx);
    lesson.on_parking_brake_released(&mut app.ctx);
    // Still waiting for a gear on a manual box.
    assert_eq!(lesson.stage, 1);
    lesson.on_gear_engaged(&mut app.ctx);
    assert_eq!(lesson.stage, 2);
}

#[test]
fn test_only_one_lesson_is_on_the_roster_so_far() {
    assert_eq!(
        freight_fate::states::driving_school::LESSONS
            .iter()
            .map(|(name, kind, _)| (*name, *kind))
            .collect::<Vec<_>>(),
        vec![("Lesson 1: Rolling basics", LessonKind::RollingBasics)]
    );
}

// -- paged search results ----------------------------------------------------------------------

/// A search list holding `count` distinct stations, opened and ready.
fn paged_search(app: &mut TestApp, drive: &SharedState, count: usize) -> RadioStationListState {
    let station = with_drive(drive, |d| {
        d.radio
            .receivable_stations()
            .first()
            .map(|r| r.station.clone())
            .expect("a station")
    });
    let hits: Vec<_> = (0..count)
        .map(|i| {
            let mut s = station.clone();
            s.id = format!("test-station-{i:03}");
            s.name = format!("Network {i:03}");
            (s, None)
        })
        .collect();
    let mut list = RadioStationListState::search(DriveRef::of(drive), "network", hits, count);
    Menu::enter(&mut list, &mut app.ctx);
    list
}

fn station_rows(list: &RadioStationListState, app: &TestApp) -> usize {
    labels(list, &app.ctx)
        .iter()
        .filter(|l| l.contains(", Network "))
        .count()
}

fn press(list: &mut RadioStationListState, app: &mut TestApp, k: Key) {
    Menu::handle_event(list, &mut app.ctx, &key(k));
}

#[test]
fn test_search_with_exactly_one_page_has_no_paging_rows() {
    for count in [25, 1] {
        let mut app = TestApp::new();
        let drive = a_radio_drive(&mut app);
        let list = paged_search(&mut app, &drive, count);
        let rows = labels(&list, &app.ctx);
        assert_eq!(station_rows(&list, &app), count, "{rows:?}");
        assert!(!rows.iter().any(|r| r.contains("page")), "{rows:?}");
    }
}

#[test]
fn test_search_with_26_matches_pages_at_the_boundary() {
    let mut app = TestApp::new();
    let drive = a_radio_drive(&mut app);
    let mut list = paged_search(&mut app, &drive, 26);
    assert_eq!(station_rows(&list, &app), 25);
    app.clear_speech();
    press(&mut list, &mut app, Key::PageDown);
    assert!(
        last(&app).starts_with("Results 26 to 26 of 26."),
        "{}",
        last(&app)
    );
    assert_eq!(station_rows(&list, &app), 1);
    let rows = labels(&list, &app.ctx);
    assert!(rows.iter().any(|r| r == "Previous page"), "{rows:?}");
    assert!(!rows.iter().any(|r| r.starts_with("Next page")), "{rows:?}");
}

#[test]
fn test_search_with_52_matches_reaches_every_page() {
    let mut app = TestApp::new();
    let drive = a_radio_drive(&mut app);
    let mut list = paged_search(&mut app, &drive, 52);
    assert!(
        app.main_lines()
            .iter()
            .any(|l| l.contains("52 matches") && l.contains("Results 1 to 25 of 52.")),
        "{:?}",
        app.main_lines()
    );
    let rows = labels(&list, &app.ctx);
    assert_eq!(station_rows(&list, &app), 25);
    assert!(
        rows.iter()
            .any(|r| r.starts_with("Next page. Results 26 to 50 of 52")),
        "{rows:?}"
    );
    press(&mut list, &mut app, Key::PageDown);
    assert!(
        last(&app).starts_with("Results 26 to 50 of 52."),
        "{}",
        last(&app)
    );
    assert_eq!(station_rows(&list, &app), 25);
    press(&mut list, &mut app, Key::PageDown);
    assert!(
        last(&app).starts_with("Results 51 to 52 of 52."),
        "{}",
        last(&app)
    );
    assert_eq!(station_rows(&list, &app), 2);
    assert!(labels(&list, &app.ctx)[0].contains("Network 050"));
    // Past the end says so and stays put.
    press(&mut list, &mut app, Key::PageDown);
    assert!(
        last(&app).starts_with("Last page of results."),
        "{}",
        last(&app)
    );
    press(&mut list, &mut app, Key::PageUp);
    assert!(
        last(&app).starts_with("Results 26 to 50 of 52."),
        "{}",
        last(&app)
    );
    // The range is logged on its own, so it can be reviewed and copied.
    assert_eq!(
        app.ctx.message_log.messages.last().map(|m| m.text.as_str()),
        Some("Results 26 to 50 of 52.")
    );
    press(&mut list, &mut app, Key::PageUp);
    press(&mut list, &mut app, Key::PageUp);
    assert!(
        last(&app).starts_with("First page of results."),
        "{}",
        last(&app)
    );
}

#[test]
fn test_search_paging_rows_work_from_the_menu_alone() {
    let mut app = TestApp::new();
    let drive = a_radio_drive(&mut app);
    let mut list = paged_search(&mut app, &drive, 41);
    activate(&mut list, &mut app.ctx, "Next page");
    assert!(
        last(&app).starts_with("Results 26 to 41 of 41."),
        "{}",
        last(&app)
    );
    activate(&mut list, &mut app.ctx, "Previous page");
    assert!(
        last(&app).starts_with("Results 1 to 25 of 41."),
        "{}",
        last(&app)
    );
}

#[test]
fn test_search_with_40_and_50_and_51_matches_count_pages_right() {
    for (count, pages) in [(40usize, 2usize), (50, 2), (51, 3)] {
        let mut app = TestApp::new();
        let drive = a_radio_drive(&mut app);
        let mut list = paged_search(&mut app, &drive, count);
        let mut seen = station_rows(&list, &app);
        let mut visited = 1;
        while labels(&list, &app.ctx)
            .iter()
            .any(|r| r.starts_with("Next page"))
        {
            press(&mut list, &mut app, Key::PageDown);
            seen += station_rows(&list, &app);
            visited += 1;
        }
        assert_eq!(seen, count);
        assert_eq!(visited, pages, "{count} matches");
    }
}

// -- category browser --------------------------------------------------------------------------

#[test]
fn test_browse_by_category_lists_all_stations_and_each_category_paged() {
    let mut app = TestApp::new();
    let drive = a_radio_drive(&mut app);
    let mut menu = RadioCategoryState::new(DriveRef::of(&drive));
    Menu::enter(&mut menu, &mut app.ctx);
    let rows = labels(&menu, &app.ctx);
    assert!(rows[0].starts_with("All stations: "), "{rows:?}");
    assert!(
        rows.iter().any(|r| r.starts_with("Terrestrial: ")),
        "{rows:?}"
    );
    assert_eq!(rows.last().map(String::as_str), Some("Back"));

    let hits = with_drive(&drive, |d| d.radio.browse(None));
    assert!(hits.len() > 25);
    let mut list = RadioStationListState::browse(DriveRef::of(&drive), "All stations", hits);
    Menu::enter(&mut list, &mut app.ctx);
    let rows = labels(&list, &app.ctx);
    assert!(rows.iter().any(|r| r.starts_with("Next page.")), "{rows:?}");
    app.clear_speech();
    press(&mut list, &mut app, Key::PageDown);
    assert!(last(&app).starts_with("Results 26 to "), "{}", last(&app));
}
