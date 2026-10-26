//! A driver's public profile read in the game: Enter on the drivers list, or
//! Your profile on the Online menu.
//!
//! The rows are the profile page on orinks.net in the online-profile
//! design's spoken order -- identity, then the career, then what was earned
//! -- one fact per row so a screen reader user can re-read the one they
//! wanted. The name is on screen before the site answers, and a hidden or
//! unreachable profile says so in the player's terms.

use crate::states_online_support::*;
use freight_fate::app::testing::TestApp;
use freight_fate::app::SharedState;
use freight_fate::net::testing::FakeTransport;
use freight_fate::net::NetError;
use freight_fate::states::base::{Key, Menu, SimpleMenuState};
use freight_fate::states::online_hub::OnlineHubState;
use freight_fate::states::online_states::{profile_rows, DriverProfileState, DriversOnlineState};
use serde_json::{json, Value};

const NOW_MS: f64 = 1_800_000_000_000.0;

fn board_row(name: &str, activity: &str) -> Value {
    json!({
        "driverId": format!("{}-1234", name.to_lowercase().replace(' ', "-")),
        "displayName": name,
        "activity": activity,
        "detail": "",
        "updatedAt": NOW_MS,
        "changedAt": NOW_MS,
    })
}

/// The site's answer for a driver with a full 1.9 career behind them.
fn full_profile() -> Value {
    json!({
        "driver": {"driverId": "road-star-1234", "displayName": "Road Star", "visibility": "public"},
        "snapshot": {
            "saveName": "Littlebear",
            "businessIdentity": "Company driver",
            "carrierName": "Swift Transportation",
            "level": 12,
            "careerTitle": "Road Veteran",
            "truckName": "Peterbilt 579",
            "truckIsCarrierAssigned": true,
            "fleetTier": "Regional",
            "deliveries": 1234,
            "milesDriven": 45678.9,
            "onTimeRate": 97.4,
            "damageFreeRate": 99.0,
            "safetyRecord": {
                "citations": 2, "seriousViolations": 0, "majorOffenses": 0,
                "outOfServiceOrders": 1, "cargoClaims": 1, "carrierTerminations": 0, "repossessions": 0,
            },
            "statesVisited": 12,
            "citiesVisited": 40,
            "longestHaulMiles": 1200.4,
            "lifetimeEarnings": 123456,
            "netWorth": 9999,
            "netWorthComplete": true,
            "reputation": 78.2,
            "endorsements": ["Hazmat", "Tanker"],
        },
        "presence": {"activity": "Driving", "detail": "I-80 near Reno", "updatedAt": NOW_MS},
        "achievementCount": 14,
        "recentAchievements": [
            {"achievementKey": "a", "label": "First Delivery", "earnedAt": 1,
             "description": "The first load signed for and delivered."},
            {"achievementKey": "b", "label": "Night Owl", "earnedAt": 2},
        ],
        "events": [{"eventId": "e1", "summary": "Delivered produce to Reno on time."}],
    })
}

fn open_profile(app: &mut TestApp, driver_id: &str, seed: Option<Value>, own: bool) -> SharedState {
    let mut state = DriverProfileState::new(&mut app.ctx, driver_id, seed, own);
    state.threaded = false;
    push(app, state)
}

fn tick(app: &mut TestApp, shared: &SharedState) {
    with_state::<DriverProfileState, _>(shared, |s| Menu::update(s, &mut app.ctx, 0.0));
}

fn rows(app: &TestApp, shared: &SharedState) -> Vec<String> {
    labels::<DriverProfileState>(shared, &app.ctx)
}

#[test]
fn test_the_rows_follow_the_designs_spoken_order_one_fact_each() {
    assert_eq!(
        profile_rows(&full_profile(), ff_core::settings::ScoreDisplay::Both)
            .into_iter()
            .map(|row| row.replace(&updated_phrase(), "updated"))
            .collect::<Vec<_>>(),
        vec![
            "Road Star",
            "On duty. Driving. I-80 near Reno. updated",
            "Current career: Littlebear",
            "Employment: Company driver",
            "Carrier: Swift Transportation",
            "Level 12, Road Veteran",
            "Assigned truck: Peterbilt 579",
            "Carrier fleet tier: Regional",
            "Current career resume",
            "Lifetime deliveries: 1,234",
            "Lifetime miles: 45,679",
            "On time: 97 percent",
            "Damage free: 99 percent",
            "Safety record: 2 citations, 0 serious violations, 0 major offenses, \
             1 out-of-service order, 1 cargo claim, 0 carrier terminations, 0 repossessions",
            "States visited: 12",
            "Cities visited: 40",
            "Longest haul: 1,200 miles",
            "Lifetime career earnings: 123,456 dollars",
            "Net worth: 9,999 dollars",
            "Reputation: Well regarded. 78 out of 100.",
            "Endorsements: Hazmat, Tanker",
            "Achievements across every career: 14",
            "Recent achievement: First Delivery. The first load signed for and delivered.",
            "Recent achievement: Night Owl",
            "Road journal: Delivered produce to Reno on time.",
        ]
    );
}

/// Whatever `updated_text` says for a stamp of NOW_MS on this clock.
fn updated_phrase() -> String {
    freight_fate::states::online_states::updated_text(NOW_MS)
}

#[test]
fn test_a_driver_with_nothing_shared_yet_reads_as_empty_not_broken() {
    let profile = json!({
        "driver": {"driverId": "new-driver-1234", "displayName": "New Driver"},
        "snapshot": null,
        "presence": null,
        "achievementCount": 0,
        "recentAchievements": [],
        "events": [],
    });
    assert_eq!(
        profile_rows(&profile, ff_core::settings::ScoreDisplay::Both),
        vec![
            "New Driver",
            "Off duty",
            "No career shared yet",
            "No achievements yet",
            "No road journal entries yet",
        ]
    );
}

#[test]
fn test_a_site_that_does_not_send_orders_reads_the_record_without_them() {
    let mut profile = full_profile();
    profile["snapshot"]["safetyRecord"]
        .as_object_mut()
        .expect("a record")
        .remove("outOfServiceOrders");
    let listed = profile_rows(&profile, ff_core::settings::ScoreDisplay::Both);
    let record = listed
        .iter()
        .find(|r| r.starts_with("Safety record"))
        .expect("a safety record row");
    assert!(!record.contains("out-of-service"), "{record}");
}

#[test]
fn test_net_worth_is_left_out_until_every_part_of_it_is_known() {
    let mut profile = full_profile();
    profile["snapshot"]["netWorthComplete"] = json!(false);
    let listed = profile_rows(&profile, ff_core::settings::ScoreDisplay::Both);
    assert!(
        !listed.iter().any(|r| r.starts_with("Net worth")),
        "{listed:?}"
    );
    assert!(listed
        .iter()
        .any(|r| r.starts_with("Owned truck") || r.starts_with("Assigned truck")));
}

#[test]
fn test_enter_on_a_driver_in_the_list_opens_their_profile() {
    let mut app = TestApp::new();
    let transport =
        FakeTransport::replying(json!({"drivers": [board_row("Road Star", "Driving")]}));
    let _guard = install_transport(transport.clone());
    let mut board = DriversOnlineState::new(&mut app.ctx);
    board.threaded = false;
    let board = push(&mut app, board);
    with_state::<DriversOnlineState, _>(&board, |s| Menu::update(s, &mut app.ctx, 0.0));
    move_to::<DriversOnlineState>(&mut app, &board, "Road Star");
    app.clear_speech();

    // The site answers the profile question next.
    transport.set_reply(Some(full_profile()));
    press(&mut app, Key::Return);

    let shared = app.state().expect("a state is on the stack");
    assert!(
        is_state::<DriverProfileState>(&shared),
        "Enter opened the profile"
    );
    let asked = transport
        .requests()
        .last()
        .expect("the profile was asked for")
        .url
        .clone();
    assert!(
        asked.ends_with("/api/freight-fate/drivers/road-star-1234"),
        "{asked}"
    );
    tick(&mut app, &shared);

    // The name leads, then the career in a breath; the rows carry the rest.
    let spoken = said(&app);
    assert!(spoken.contains("Driver profile"), "{spoken}");
    assert!(
        spoken.contains("Road Star. Level 12, Road Veteran. Company driver."),
        "{spoken}"
    );
    let listed = rows(&app, &shared);
    assert_eq!(listed[0], "Road Star");
    assert!(
        listed.contains(&"Lifetime deliveries: 1,234".to_string()),
        "{listed:?}"
    );
    assert_eq!(listed.last().map(String::as_str), Some("Back"));

    // Back lands on the same driver in the list.
    transport.set_reply(Some(
        json!({"drivers": [board_row("Road Star", "Driving")]}),
    ));
    press(&mut app, Key::Escape);
    app.ctx.run_deferred();
    let top = app.state().expect("the list is back");
    assert!(is_state::<DriversOnlineState>(&top));
    assert!(current_label::<DriversOnlineState>(&top, &app.ctx).starts_with("Road Star"));
}

#[test]
fn test_the_name_is_on_screen_before_the_site_answers() {
    let mut app = TestApp::new();
    // Rows built before any fetch has started: the moment between Enter and
    // the site's reply, frozen.
    let mut state = DriverProfileState::new(
        &mut app.ctx,
        "road-star-1234",
        Some(board_row("Road Star", "Driving")),
        false,
    );
    let listed: Vec<String> = built_rows(&mut state, &mut app.ctx)
        .into_iter()
        .map(|(label, _help)| label)
        .collect();
    assert_eq!(listed[0], "Road Star", "{listed:?}");
    assert!(
        listed.iter().any(|r| r == "Checking the profile"),
        "{listed:?}"
    );
}

#[test]
fn test_a_driver_with_no_public_profile_says_so() {
    let mut app = TestApp::new();
    let _guard = install_transport(FakeTransport::failing(NetError::http(404)));
    let shared = open_profile(
        &mut app,
        "hidden-driver-1234",
        Some(board_row("Hidden Driver", "Driving")),
        false,
    );
    tick(&mut app, &shared);
    assert!(
        said(&app).contains("This driver has no public profile."),
        "{}",
        said(&app)
    );
    let listed = rows(&app, &shared);
    assert_eq!(listed[0], "Hidden Driver");
    assert!(
        listed
            .iter()
            .any(|r| r == "This driver has no public profile"),
        "{listed:?}"
    );
}

#[test]
fn test_your_own_hidden_profile_says_what_to_change() {
    let mut app = TestApp::new();
    let _guard = install_transport(FakeTransport::failing(NetError::http(404)));
    let shared = open_profile(&mut app, "driver-me-1234", None, true);
    tick(&mut app, &shared);
    let spoken = said(&app);
    assert!(spoken.contains("Your profile"), "{spoken}");
    assert!(
        spoken.contains("Your profile is not public. Turn Profile sharing on"),
        "{spoken}"
    );
    assert!(rows(&app, &shared)
        .iter()
        .any(|r| r.starts_with("Your profile is not public")));
}

#[test]
fn test_an_unreachable_site_is_not_read_as_a_hidden_profile() {
    let mut app = TestApp::new();
    let _guard = install_transport(FakeTransport::failing(NetError::other("OSError", "")));
    let shared = open_profile(
        &mut app,
        "road-star-1234",
        Some(board_row("Road Star", "Driving")),
        false,
    );
    tick(&mut app, &shared);
    let spoken = said(&app);
    assert!(
        spoken.contains("The profile could not be reached."),
        "{spoken}"
    );
    assert!(!spoken.contains("no public profile"), "{spoken}");
}

#[test]
fn test_your_profile_on_the_online_menu_needs_an_account() {
    let mut app = TestApp::new();
    let _board = install_transport(FakeTransport::replying(json!({"drivers": []})));
    let _identity = install_identity(&app, None);
    let hub_state = OnlineHubState::new(&mut app.ctx);
    let hub = push(&mut app, hub_state);
    move_to::<OnlineHubState>(&mut app, &hub, "Your profile");
    app.clear_speech();
    press(&mut app, Key::Return);
    assert!(
        said(&app).contains("needs your orinks.net account"),
        "{}",
        said(&app)
    );
    let top = app.state().expect("still on the hub");
    assert!(is_state::<OnlineHubState>(&top));
}

#[test]
fn test_your_profile_on_the_online_menu_opens_your_own() {
    let mut app = TestApp::new();
    let _board = install_transport(FakeTransport::replying(json!({"drivers": []})));
    let me = identity();
    let _identity = install_identity(&app, Some(&me));
    let hub_state = OnlineHubState::new(&mut app.ctx);
    let hub = push(&mut app, hub_state);
    move_to::<OnlineHubState>(&mut app, &hub, "Your profile");
    press(&mut app, Key::Return);
    let top = app.state().expect("the profile is on the stack");
    assert!(is_state::<DriverProfileState>(&top));
    with_state::<DriverProfileState, _>(&top, |s| {
        assert_eq!(s.driver_id(), me.driver_id);
    });
}

#[test]
fn test_places_visited_opens_from_the_row_after_the_city_count() {
    let mut app = TestApp::new();
    let mut profile = full_profile();
    profile["snapshot"]["visitedPlaces"] = json!([
        {"state": "Illinois", "cities": ["Chicago", "Rockford"]},
        {"state": "Nevada", "cities": ["Reno"]},
    ]);
    let transport = FakeTransport::replying(profile);
    let _guard = install_transport(transport.clone());
    let shared = open_profile(&mut app, "road-star-1234", None, false);
    tick(&mut app, &shared);

    let listed = rows(&app, &shared);
    let count = listed
        .iter()
        .position(|r| r == "Cities visited: 40")
        .expect("the city count is listed");
    assert_eq!(listed[count + 1], "Places visited", "{listed:?}");

    move_to::<DriverProfileState>(&mut app, &shared, "Places visited");
    press(&mut app, Key::Return);
    app.ctx.run_deferred();
    let top = app.state().expect("the list is open");
    assert_eq!(
        labels::<SimpleMenuState>(&top, &app.ctx),
        ["Illinois: Chicago, Rockford", "Nevada: Reno", "Back"]
    );
}

#[test]
fn test_a_profile_without_the_places_list_offers_no_row_for_it() {
    // A snapshot from before the site sent the list: the counts stand alone.
    let mut app = TestApp::new();
    let transport = FakeTransport::replying(full_profile());
    let _guard = install_transport(transport.clone());
    let shared = open_profile(&mut app, "road-star-1234", None, false);
    tick(&mut app, &shared);
    let listed = rows(&app, &shared);
    assert!(
        listed.iter().any(|r| r == "Cities visited: 40"),
        "{listed:?}"
    );
    assert!(!listed.iter().any(|r| r == "Places visited"), "{listed:?}");
}
