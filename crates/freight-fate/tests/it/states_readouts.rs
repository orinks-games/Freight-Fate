//! Screens that used to be one long spoken sentence and are lists of lines
//! now: Time and weather, the first-day briefing and the career plan at the
//! terminal, Trip status on the pause menu, and the tightened logbook.

use crate::states_city_support::*;
use crate::states_driving_menus_support::{a_drive, with_drive};
use ff_core::models::profile::Profile;
use ff_core::sim::hos::DutySegment;
use freight_fate::app::testing::TestApp;
use freight_fate::states::base::SimpleMenuState;
use freight_fate::states::city::CityMenuState;
use freight_fate::states::driving_menu_states::DriveRef;
use freight_fate::states::driving_pause_states::{trip_status_lines, PauseMenuState};
use freight_fate::states::logbook::{logbook_lines, LogbookState};

fn at_terminal(app: &mut TestApp, name: &str) {
    career(app, name, "Chicago");
    let city = CityMenuState::new(&app.ctx, false);
    app.push_state(city);
}

#[test]
fn test_time_and_weather_is_a_screen_of_lines() {
    let mut app = TestApp::new();
    at_terminal(&mut app, "Weather Rows");
    select::<CityMenuState>(&mut app, "Time and weather");
    assert!(is::<SimpleMenuState>(&app));
    let rows = labels::<SimpleMenuState>(&app);
    assert!(rows[0].starts_with("It is "), "{rows:?}");
    assert!(rows[0].ends_with('.'), "{rows:?}");
    // The calendar line: a date, then the season.
    assert!(
        ["spring", "summer", "fall", "autumn", "winter"]
            .iter()
            .any(|season| rows[1].ends_with(&format!("{season}."))),
        "{rows:?}"
    );
    assert_eq!(rows[2], "Day 1 of your career.");
    assert!(rows[3].contains("weather in Chicago: "), "{rows:?}");
    assert_eq!(rows.last().map(String::as_str), Some("Back"));
    // Nothing of the old one-sentence readout survives in a single row.
    assert!(
        !rows.iter().any(|r| r.contains("day 1 of your career")),
        "{rows:?}"
    );
}

#[test]
fn test_first_day_briefing_is_a_screen_of_lines() {
    let mut app = TestApp::new();
    at_terminal(&mut app, "Briefing Rows");
    select::<CityMenuState>(&mut app, "First-day briefing");
    assert!(is::<SimpleMenuState>(&app));
    let rows = labels::<SimpleMenuState>(&app);
    assert!(
        rows[0].starts_with("First-day briefing: welcome aboard "),
        "{rows:?}"
    );
    assert!(
        rows[1].starts_with("Your assigned truck is parked at "),
        "{rows:?}"
    );
    assert!(
        rows.iter().any(|r| r == "Next: the dispatch board."),
        "{rows:?}"
    );
    assert_eq!(rows.len(), 7, "{rows:?}");
}

#[test]
fn test_trip_status_is_a_screen_of_lines() {
    let mut app = TestApp::new();
    let drive = a_drive(&mut app);
    let lines = with_drive(&drive, |d| trip_status_lines(d, &app.ctx));
    assert!(lines[0].starts_with("Hauling "), "{lines:?}");
    assert!(lines[0].contains(" to "), "{lines:?}");
    assert!(lines.iter().any(|l| l.contains(" used of ")), "{lines:?}");
    assert_eq!(lines.len(), 4, "{lines:?}");
    for line in &lines {
        assert!(line.ends_with('.'), "{line}");
    }

    // The pause menu row opens it as a screen.
    app.push_state(PauseMenuState::with_drive(DriveRef::of(&drive)));
    select::<PauseMenuState>(&mut app, "Trip status");
    assert!(is::<SimpleMenuState>(&app));
    let rows = labels::<SimpleMenuState>(&app);
    assert_eq!(rows[0], lines[0]);
    assert_eq!(rows.last().map(String::as_str), Some("Back"));
}

// -- the logbook ------------------------------------------------------------------------

fn logged_profile(name: &str) -> Profile {
    let mut p = Profile::named_in(name, "Chicago");
    p.game_hours = 34.0; // 10 AM on day two
    let log = &mut p.duty_log;
    log.record("off_duty", 24.0, 30.0, "Chicago", "");
    log.record("on_duty_not_driving", 30.0, 31.0, "Chicago", "pre-trip");
    log.record("driving", 31.0, 34.0, "I-90", "");
    p
}

#[test]
fn test_logbook_opens_with_what_you_are_doing_and_since_when() {
    let mut app = TestApp::new();
    app.ctx.profile = Some(logged_profile("Logbook Rows"));
    let lines = logbook_lines(&app.ctx, None);
    assert_eq!(lines[0], "Driving since 7 AM at I-90.");
    // The status is not said a second time inside the hours lines.
    assert!(!lines[1].contains("ELD status"), "{lines:?}");
    assert!(lines[1].starts_with("Driving left: "), "{lines:?}");
    assert!(
        lines
            .iter()
            .any(|l| l.starts_with("30-minute break due in ") || l.starts_with("Duty window ")),
        "{lines:?}"
    );
}

#[test]
fn test_logbook_totals_are_one_line_and_entries_read_newest_first() {
    let mut app = TestApp::new();
    app.ctx.profile = Some(logged_profile("Logbook Order"));
    let lines = logbook_lines(&app.ctx, None);
    let totals = lines
        .iter()
        .find(|l| l.starts_with("Today: "))
        .expect("a totals line");
    assert!(totals.contains("driving 3 hours"), "{totals}");
    assert!(totals.contains("sleeper berth"), "{totals}");
    // No heading row; the entries follow the totals, newest first, each led
    // by what the driver was doing.
    assert!(
        !lines.iter().any(|l| l.contains("Recent logbook")),
        "{lines:?}"
    );
    let at = lines.iter().position(|l| l.starts_with("Today: ")).unwrap();
    assert_eq!(lines[at + 1], "Driving, 7 AM to 10 AM, 3 hours, I-90.");
    assert_eq!(
        lines[at + 2],
        "On duty, not driving, 6 AM to 7 AM, 1 hour, Chicago, pre-trip."
    );
    assert!(lines[at + 3].starts_with("Off duty, "), "{lines:?}");
    assert_eq!(lines.len(), at + 4, "{lines:?}");
}

#[test]
fn test_logbook_screen_reads_the_first_line_on_entry() {
    let mut app = TestApp::new();
    app.ctx.profile = Some(logged_profile("Logbook Entry"));
    app.clear_speech();
    app.push_state(LogbookState::new(None));
    let said = app.main_lines().join(" ");
    assert!(
        said.contains("Logbook. Driving since 7 AM at I-90."),
        "{said}"
    );
    let rows = labels::<LogbookState>(&app);
    assert_eq!(rows.last().map(String::as_str), Some("Back"));
}

#[test]
fn test_empty_logbook_says_so_once() {
    let mut app = TestApp::new();
    career(&mut app, "Fresh Log", "Chicago");
    let lines = logbook_lines(&app.ctx, None);
    assert_eq!(lines[0], "Off duty. No logbook entries yet.");
    assert!(lines.iter().any(|l| l.starts_with("Today: ")), "{lines:?}");
    let _ = DutySegment::new("driving", 0.0, 1.0, "", "");
}
