//! The driver directory: every driver with a public profile, on duty or not,
//! reached from the Online menu next to Drivers on duty.
//!
//! The rows keep the site's reading order (on duty first, then by last on
//! duty), each says on duty and what, or when the driver was last on duty in
//! round figures, and Enter opens the same profile screen the drivers list
//! does. The list never re-checks on its own; Check again does.

use crate::states_online_support::*;
use freight_fate::app::testing::TestApp;
use freight_fate::app::SharedState;
use freight_fate::net::testing::FakeTransport;
use freight_fate::net::NetError;
use freight_fate::states::base::{Key, Menu};
use freight_fate::states::online_hub::OnlineHubState;
use freight_fate::states::online_states::{
    directory_row_text, last_on_duty_text_at, wall_time, DriverDirectoryState, DriverProfileState,
};
use serde_json::{json, Value};

const HOUR_S: f64 = 3600.0;
const DAY_S: f64 = 24.0 * HOUR_S;
const NOW_S: f64 = 1_800_000_000.0;

/// The site's answer, stamped against the real clock: the rows' ages are
/// read against it, so a fixed fixture time would drift into the future.
fn directory() -> Value {
    let now_ms = wall_time() * 1000.0;
    json!({"drivers": [
        {"driverId": "road-star-1234", "displayName": "Road Star", "onDuty": true,
         "activity": "Driving: Reno, Nevada to Boise, Idaho", "detail": "produce, 40% there",
         "changedAt": now_ms},
        {"driverId": "night-owl-5678", "displayName": "Night Owl", "onDuty": false,
         "lastOnDutyAt": now_ms - 3.0 * DAY_S * 1000.0},
        {"driverId": "new-hire-9012", "displayName": "New Hire", "onDuty": false},
    ], "asOf": now_ms})
}

fn open_directory(app: &mut TestApp) -> SharedState {
    let mut state = DriverDirectoryState::new(&mut app.ctx);
    state.threaded = false;
    push(app, state)
}

fn tick(app: &mut TestApp, shared: &SharedState) {
    with_state::<DriverDirectoryState, _>(shared, |s| Menu::update(s, &mut app.ctx, 0.0));
}

#[test]
fn test_last_on_duty_is_coarse_hours_days_weeks_months() {
    let at = |age_s: f64| last_on_duty_text_at((NOW_S - age_s) * 1000.0, NOW_S);
    assert_eq!(at(20.0 * 60.0), "Last on duty less than an hour ago");
    assert_eq!(at(HOUR_S + 1.0), "Last on duty an hour ago");
    assert_eq!(at(30.0 * HOUR_S), "Last on duty 30 hours ago");
    assert_eq!(at(2.0 * DAY_S), "Last on duty 2 days ago");
    assert_eq!(at(13.0 * DAY_S), "Last on duty 13 days ago");
    assert_eq!(at(14.0 * DAY_S), "Last on duty 2 weeks ago");
    assert_eq!(at(8.0 * DAY_S), "Last on duty 8 days ago");
    assert_eq!(at(61.0 * DAY_S), "Last on duty 2 months ago");
    assert_eq!(at(400.0 * DAY_S), "Last on duty 13 months ago");
    // A stamp from the future reads as just now, never as a negative age.
    assert_eq!(at(-HOUR_S), "Last on duty less than an hour ago");
}

#[test]
fn test_a_row_says_on_duty_and_what_or_when_they_were_last_on_duty() {
    let rows = directory()["drivers"].as_array().unwrap().clone();
    assert_eq!(
        directory_row_text(&rows[0]),
        "Road Star. On duty. Driving: Reno, Nevada to Boise, Idaho"
    );
    assert!(
        directory_row_text(&rows[1]).starts_with("Night Owl. Last on duty "),
        "{}",
        directory_row_text(&rows[1])
    );
    assert_eq!(
        directory_row_text(&rows[2]),
        "New Hire. Not seen on duty yet"
    );
    assert_eq!(
        directory_row_text(&json!({"driverId": "x-1234", "onDuty": false})),
        "A driver. Not seen on duty yet"
    );
}

#[test]
fn test_online_menu_row_opens_the_directory_and_asks_the_site_once() {
    let mut app = TestApp::new();
    let transport = FakeTransport::replying(directory());
    let _guard = install_transport(transport.clone());
    let hub_state = OnlineHubState::new(&mut app.ctx);
    let hub = push(&mut app, hub_state);
    move_to::<OnlineHubState>(&mut app, &hub, "Driver directory");
    app.clear_speech();
    press(&mut app, Key::Return);

    let shared = app.state().expect("a state is on the stack");
    assert!(is_state::<DriverDirectoryState>(&shared));
    // The real screen fetches on its own thread; wait for that one to land
    // rather than starting a second fetch.
    for _ in 0..300 {
        if with_state::<DriverDirectoryState, _>(&shared, |s| s.fetched()) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(with_state::<DriverDirectoryState, _>(&shared, |s| s.fetched()));
    tick(&mut app, &shared);

    let asked: Vec<String> = transport.requests().iter().map(|r| r.url.clone()).collect();
    assert_eq!(asked.len(), 1, "{asked:?}");
    assert!(asked[0].ends_with("/freight-fate/directory"), "{asked:?}");
    let spoken = said(&app);
    assert!(
        spoken.contains("3 drivers have a public profile, 1 on duty."),
        "{spoken}"
    );
    assert!(spoken.contains("Road Star. On duty."), "{spoken}");
}

#[test]
fn test_rows_keep_the_site_order_and_end_with_check_again_and_back() {
    let mut app = TestApp::new();
    let _guard = install_transport(FakeTransport::replying(directory()));
    let shared = open_directory(&mut app);
    tick(&mut app, &shared);

    let rows = labels::<DriverDirectoryState>(&shared, &app.ctx);
    assert_eq!(rows.len(), 5, "{rows:?}");
    assert_eq!(
        rows[0],
        "Road Star. On duty. Driving: Reno, Nevada to Boise, Idaho"
    );
    assert_eq!(rows[1], "Night Owl. Last on duty 3 days ago");
    assert_eq!(rows[2], "New Hire. Not seen on duty yet");
    assert_eq!(rows[3], "Check again");
    assert_eq!(rows[4], "Back");
    let help = helps::<DriverDirectoryState>(&shared, &app.ctx);
    for (row, help) in rows.iter().zip(help.iter()).take(rows.len() - 1) {
        assert!(!help.is_empty(), "{row} has no help");
    }
}

#[test]
fn test_enter_on_a_driver_opens_their_profile_with_the_name_already_known() {
    let mut app = TestApp::new();
    let transport = FakeTransport::replying(directory());
    let _guard = install_transport(transport.clone());
    let shared = open_directory(&mut app);
    tick(&mut app, &shared);
    move_to::<DriverDirectoryState>(&mut app, &shared, "Night Owl");
    app.clear_speech();

    // The site never answers the profile question, so what is on screen is
    // what the directory row already knew.
    transport.set_error(Some(NetError::other("OSError", "")));
    press(&mut app, Key::Return);

    let top = app.state().expect("a state is on the stack");
    assert!(
        is_state::<DriverProfileState>(&top),
        "Enter opened the profile"
    );
    with_state::<DriverProfileState, _>(&top, |s| {
        assert_eq!(s.driver_id(), "night-owl-5678");
    });
    let asked = transport
        .requests()
        .last()
        .expect("the profile was asked for")
        .url
        .clone();
    assert!(
        asked.ends_with("/api/freight-fate/drivers/night-owl-5678"),
        "{asked}"
    );
    let rows = labels::<DriverProfileState>(&top, &app.ctx);
    assert_eq!(rows[0], "Night Owl", "{rows:?}");

    // Back lands on the same driver in the directory, and asks the site for
    // nothing: the list is what it was, cursor included.
    let before = transport.requests().len();
    press(&mut app, Key::Escape);
    app.ctx.run_deferred();
    let top = app.state().expect("the directory is back");
    assert!(is_state::<DriverDirectoryState>(&top));
    assert!(current_label::<DriverDirectoryState>(&top, &app.ctx).starts_with("Night Owl"));
    assert_eq!(transport.requests().len(), before);
}

#[test]
fn test_an_empty_directory_and_an_unreachable_site_say_so_in_the_players_terms() {
    let mut app = TestApp::new();
    let transport = FakeTransport::replying(json!({"drivers": []}));
    let _guard = install_transport(transport.clone());
    let shared = open_directory(&mut app);
    app.clear_speech();
    tick(&mut app, &shared);
    assert!(
        said(&app).contains("No drivers have a public profile yet."),
        "{}",
        said(&app)
    );
    let rows = labels::<DriverDirectoryState>(&shared, &app.ctx);
    assert_eq!(rows[0], "No drivers have a public profile yet");

    // Check again asks once more, and an unreachable answer is worded as
    // the site, not the player.
    transport.set_error(Some(NetError::other("OSError", "")));
    move_to::<DriverDirectoryState>(&mut app, &shared, "Check again");
    app.clear_speech();
    press(&mut app, Key::Return);
    assert!(
        said(&app).contains("Checking the driver directory."),
        "{}",
        said(&app)
    );
    app.clear_speech();
    tick(&mut app, &shared);
    assert!(
        said(&app).contains("The driver directory could not be reached."),
        "{}",
        said(&app)
    );
    let rows = labels::<DriverDirectoryState>(&shared, &app.ctx);
    assert_eq!(rows[0], "The driver directory could not be reached");
    assert_eq!(rows.last().map(String::as_str), Some("Back"));
    assert_eq!(
        transport
            .requests()
            .iter()
            .filter(|r| r.url.ends_with("/freight-fate/directory"))
            .count(),
        2
    );
}
