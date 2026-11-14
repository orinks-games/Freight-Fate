//! Screens that used to be one long spoken sentence and are lists of lines
//! now: Time and weather, the first-day briefing and the career plan at the
//! terminal, Trip status on the pause menu, and the tightened logbook.

use crate::states_city_support::*;
use crate::states_driving_menus_support::{a_drive, with_drive};
use ff_core::models::profile::Profile;
use ff_core::sim::hos::DutySegment;
use ff_core::sim::real_weather::{Observation, RealWeatherProvider};
use freight_fate::app::testing::TestApp;
use freight_fate::states::base::SimpleMenuState;
use freight_fate::states::city::CityMenuState;
use freight_fate::states::driving_menu_states::DriveRef;
use freight_fate::states::driving_pause_states::{trip_status_lines, PauseMenuState};
use freight_fate::states::logbook::{logbook_lines, LogbookState};
use std::sync::{mpsc, Arc, Mutex};

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
        rows[1].starts_with("Orientation and truck assignment are at Northstar Freight Lines "),
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

// -- live weather on Time and weather -----------------------------------------------
//
// Ported from `test_season.py` and `test_weather.py`: each provider runs
// inline over a stub fetch, so no case reaches the network.

/// A wall clock that never moves, so an observation's age is exact.
const WALL_NOW: f64 = 2_000_000_000.0;

/// A provider whose every station reports `text` at `temperature_c`,
/// observed `age_s` before [`WALL_NOW`].
fn reporting(text: &'static str, temperature_c: f64, age_s: f64) -> RealWeatherProvider {
    RealWeatherProvider::new(Arc::new(move |_lat, _lon| {
        Ok(Observation::new(text, 5.0, Some(temperature_c), Some(10.0))
            .observed_at(WALL_NOW - age_s))
    }))
    .with_wall_clock(Arc::new(|| WALL_NOW))
    .with_threaded(false)
}

/// The Time and weather rows, read off a fresh terminal.
fn weather_rows(app: &mut TestApp) -> Vec<String> {
    let city = CityMenuState::new(&app.ctx, false);
    app.push_state(city);
    select::<CityMenuState>(app, "Time and weather");
    labels::<SimpleMenuState>(app)
}

#[test]
fn test_terminal_sleep_uses_independent_calendar_with_live_weather() {
    // The calendar toggle off: live skies, but the date is the career's own
    // and moves with career time.
    let mut app = TestApp::new();
    career(&mut app, "Calendar Driver", "Chicago");
    app.ctx.settings.live_weather_controls_calendar = false;
    app.ctx.settings.real_weather = false;
    let simulated = weather_rows(&mut app);

    app.ctx.settings.real_weather = true;
    app.ctx
        .set_real_weather_provider(Arc::new(reporting("Clear", 25.0, 0.0)));
    let live = weather_rows(&mut app);
    assert_eq!(
        live[1], simulated[1],
        "the date follows the career: {live:?}"
    );
    assert!(live[3].starts_with("Live weather in Chicago: "), "{live:?}");
    assert!(live[3].contains("77 degrees"), "{live:?}");

    profile_mut(&mut app).game_hours += 24.0;
    app.ctx.settings.real_weather = false;
    let next_day_simulated = weather_rows(&mut app);
    app.ctx.settings.real_weather = true;
    let next_day = weather_rows(&mut app);
    assert_ne!(next_day[1], live[1], "a day of career time moves the date");
    assert_eq!(next_day[1], next_day_simulated[1], "{next_day:?}");
    assert!(next_day[3].contains("77 degrees"), "{next_day:?}");
}

#[test]
fn test_terminal_does_not_present_modeled_temperature_while_live_weather_loads() {
    let mut app = TestApp::new();
    career(&mut app, "Loading Weather Driver", "Chicago");
    app.ctx.settings.real_weather = true;
    app.ctx.settings.live_weather_controls_calendar = false;
    // The fetch waits on a channel nobody sends on, so the first request
    // stays in flight until the test drops the sender.
    let (hold, wait) = mpsc::channel::<()>();
    let wait = Mutex::new(wait);
    let provider = Arc::new(RealWeatherProvider::new(Arc::new(move |_lat, _lon| {
        let _ = wait.lock().map(|rx| rx.recv());
        Err("released".to_string())
    })));
    app.ctx.set_real_weather_provider(Arc::clone(&provider));

    let rows = weather_rows(&mut app);
    drop(hold);
    provider.join_background();
    assert_eq!(
        rows[3], "Live weather loading in Chicago: still loading.",
        "{rows:?}"
    );
    assert!(!rows.iter().any(|row| row.contains("degrees")), "{rows:?}");
}

#[test]
fn test_terminal_weather_source_ignores_the_online_services_master() {
    // Owner ruling 2026-08-08: live weather is its own switch, and turning
    // online services off does not take it away.
    let mut app = TestApp::new();
    career(&mut app, "Offline Weather Driver", "Chicago");
    app.ctx.settings.real_weather = true;
    app.ctx.settings.online_services = false;
    app.ctx
        .set_real_weather_provider(Arc::new(reporting("Light Rain", 18.0, 0.0)));
    let rows = weather_rows(&mut app);
    assert!(rows[3].starts_with("Live weather in Chicago: "), "{rows:?}");
}

#[test]
fn test_terminal_reports_old_fresh_observation_as_live_without_updating() {
    let mut app = TestApp::new();
    career(&mut app, "Fresh Old Weather Driver", "Chicago");
    app.ctx.settings.real_weather = true;
    app.ctx
        .set_real_weather_provider(Arc::new(reporting("Light Rain", 18.0, 12.0 * 60.0)));
    let rows = weather_rows(&mut app);
    assert!(rows[3].starts_with("Live weather in Chicago: "), "{rows:?}");
    assert!(
        !rows.iter().any(|row| row.contains("Last-known")),
        "{rows:?}"
    );
    assert!(
        rows.iter()
            .any(|row| row == "The report is 12 minutes old."),
        "{rows:?}"
    );
    assert!(
        !rows
            .iter()
            .any(|row| row.to_lowercase().contains("updating")),
        "{rows:?}"
    );
}

#[test]
fn test_city_menu_warms_the_weather_provider() {
    // Parked at the terminal, the truck's city is fetched before any drive
    // asks, so the drive starts on real conditions instead of "loading".
    let mut app = TestApp::new();
    career(&mut app, "Warm Weather Driver", "Chicago");
    app.ctx.settings.real_weather = true;
    let asked = Arc::new(Mutex::new(Vec::new()));
    let log = Arc::clone(&asked);
    app.ctx.set_real_weather_provider(Arc::new(
        RealWeatherProvider::new(Arc::new(move |lat, lon| {
            log.lock().unwrap().push((lat, lon));
            Err("offline bench".to_string())
        }))
        .with_threaded(false),
    ));
    let city = CityMenuState::new(&app.ctx, false);
    app.push_state(city);
    let chicago = app.ctx.world.city("Chicago").expect("Chicago is a city");
    let station = RealWeatherProvider::station_identity(chicago.lat, chicago.lon);
    let asked = asked.lock().unwrap();
    assert!(
        asked
            .iter()
            .any(|&(lat, lon)| RealWeatherProvider::station_identity(lat, lon) == station),
        "{asked:?}"
    );
}

#[test]
fn test_live_data_providers_ignore_the_online_services_master_switch() {
    // Owner ruling 2026-08-08: live weather and live traffic each have their
    // own switch; the online services master does not reach them.
    let mut app = TestApp::new();
    app.ctx.settings.online_services = false;
    app.ctx.settings.real_weather = true;
    app.ctx.settings.real_traffic = true;
    app.ctx
        .set_real_weather_provider(Arc::new(reporting("Clear", 20.0, 0.0)));
    assert!(app.ctx.real_weather_provider().is_some());
    assert!(app.ctx.real_traffic_provider().is_some());
    app.ctx.settings.real_weather = false;
    app.ctx.settings.real_traffic = false;
    assert!(app.ctx.real_weather_provider().is_none());
    assert!(app.ctx.real_traffic_provider().is_none());
}
