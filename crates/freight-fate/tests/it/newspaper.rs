//! The driver newspaper: the fetch that keeps the issue the site published,
//! and the Newspaper app that reads it, from Driver apps and the Online menu.
//! Nothing mentions the paper until the game holds an issue.

use std::sync::Arc;

use ff_core::newspaper::{parse_issue, PaperIssue};
use freight_fate::app::testing::TestApp;
use freight_fate::net::testing::FakeTransport;
use freight_fate::net::{self, NetError};
use freight_fate::newspaper::{news_url, Newspaper, NewspaperOptions};
use freight_fate::states::base::Key;
use freight_fate::states::driving_menu_states::DriverAppsState;
use freight_fate::states::newspaper::{NewspaperSectionState, NewspaperState};
use freight_fate::states::online_hub::OnlineHubState;
use serde_json::Value;

use crate::states_driving_menus_support as drive;
use crate::states_online_support::{self as online, labels, push};

const FIXTURE: &str = include_str!("../../../ff-core/src/newspaper/fixture_issue.json");

fn fixture() -> PaperIssue {
    parse_issue(FIXTURE).expect("the fixture parses")
}

fn fixture_body() -> Value {
    serde_json::from_str(FIXTURE).expect("the fixture is JSON")
}

fn service(transport: Arc<FakeTransport>, cache: &std::path::Path) -> Newspaper {
    Newspaper::new(NewspaperOptions {
        enabled: true,
        cache_path: Some(cache.to_path_buf()),
        transport,
        threaded: false,
        network_allowed: || true,
    })
}

// -- the fetch -----------------------------------------------------------------------------

#[test]
fn test_a_fetched_issue_is_held_and_kept_for_the_next_launch() {
    let dir = tempfile::tempdir().expect("temp dir");
    let cache = dir.path().join("newspaper.json");
    let transport = FakeTransport::replying(fixture_body());
    let paper = service(transport.clone(), &cache);
    assert!(!paper.holding());
    paper.start();
    assert_eq!(paper.status(), "holding issue 25");
    assert_eq!(transport.requests()[0].url, news_url());
    assert!(cache.exists());

    // The next launch opens with it, offline.
    let offline = FakeTransport::failing(NetError::Connection("offline".into()));
    let again = service(offline, &cache);
    assert_eq!(again.issue(), Some(fixture()));
    again.start();
    assert_eq!(
        again.issue(),
        Some(fixture()),
        "a failed fetch keeps the issue"
    );
}

#[test]
fn test_the_site_answering_no_issue_drops_the_held_one() {
    let dir = tempfile::tempdir().expect("temp dir");
    let cache = dir.path().join("newspaper.json");
    let paper = service(FakeTransport::replying(fixture_body()), &cache);
    paper.start();
    assert!(paper.holding());

    let gone = FakeTransport::failing(NetError::Http {
        code: 404,
        body: Vec::new(),
    });
    let paper = service(gone, &cache);
    assert!(paper.holding());
    paper.start();
    assert!(!paper.holding());
    assert!(!cache.exists());
}

#[test]
fn test_a_body_it_cannot_read_keeps_what_is_held() {
    let dir = tempfile::tempdir().expect("temp dir");
    let cache = dir.path().join("newspaper.json");
    let paper = service(FakeTransport::replying(fixture_body()), &cache);
    paper.start();

    let mut newer = fixture_body();
    newer["version"] = Value::from(2);
    let paper = service(FakeTransport::replying(newer), &cache);
    paper.start();
    assert_eq!(paper.status(), "holding issue 25");
}

#[test]
fn test_no_fetch_with_online_services_off() {
    let dir = tempfile::tempdir().expect("temp dir");
    let transport = FakeTransport::replying(fixture_body());
    let paper = Newspaper::new(NewspaperOptions {
        enabled: false,
        cache_path: Some(dir.path().join("newspaper.json")),
        transport: transport.clone(),
        threaded: false,
        network_allowed: || true,
    });
    paper.start();
    paper.refresh_if_stale();
    assert_eq!(transport.request_count(), 0);
    assert!(!paper.holding());
}

/// A test process is never the game, so the real fetch never starts: the
/// service the app builds, on the live transport and threaded, sends
/// nothing, and the network guard records no attempt at the paper.
#[test]
fn test_the_real_fetch_never_runs_in_a_test_process() {
    assert!(!net::real_network_allowed());
    let dir = tempfile::tempdir().expect("temp dir");
    let paper = Newspaper::new(NewspaperOptions {
        enabled: true,
        cache_path: Some(dir.path().join("newspaper.json")),
        ..Default::default()
    });
    paper.start();
    paper.refresh_if_stale();
    paper.shutdown();
    assert!(!paper.holding());
    assert!(!net::refused_requests()
        .iter()
        .any(|seen| seen.contains("/api/freight-fate/news")));
}

#[test]
fn test_shutdown_is_bounded_and_idempotent() {
    let paper = Newspaper::default();
    let started = std::time::Instant::now();
    paper.shutdown();
    paper.shutdown();
    assert!(started.elapsed() < freight_fate::newspaper::SHUTDOWN_WAIT);
}

// -- the app -------------------------------------------------------------------------------

#[test]
fn test_no_issue_means_no_newspaper_row_anywhere() {
    let mut app = TestApp::new();
    app.ctx.services.newspaper.stage(None);
    let drive_state = drive::a_drive(&mut app);
    let mut apps = DriverAppsState::new(drive::drive_ref(&drive_state));
    let rows = drive::build_labels(&mut apps, &mut app.ctx);
    assert!(
        !rows.iter().any(|row| row.contains("Newspaper")),
        "{rows:?}"
    );

    let hub = OnlineHubState::new(&mut app.ctx);
    let hub = push(&mut app, hub);
    let rows = labels::<OnlineHubState>(&hub, &app.ctx);
    assert!(
        !rows.iter().any(|row| row.contains("Newspaper")),
        "{rows:?}"
    );
    assert_eq!(rows[3], "Account achievements");
}

#[test]
fn test_driver_apps_lists_the_newspaper_after_eld_and_reads_it() {
    let mut app = TestApp::new();
    app.ctx.services.newspaper.stage(Some(fixture()));
    let drive_state = drive::a_drive(&mut app);
    let mut apps = DriverAppsState::new(drive::drive_ref(&drive_state));
    let rows = drive::build_labels(&mut apps, &mut app.ctx);
    let eld = rows.iter().position(|row| row == "ELD").expect("ELD row");
    assert_eq!(rows[eld + 1], "Newspaper");

    drive::activate(&mut apps, &mut app.ctx, "Newspaper");
    assert!(drive::top_is::<NewspaperState>(&app));
    let said = online::said(&app);
    assert!(
        said.contains("The Mile Marker, issue 25. November 16 to 22, 2026."),
        "{said}"
    );
    let sections = drive::with_top_ctx::<NewspaperState, _>(&mut app, drive::build_labels);
    assert_eq!(sections[0], "Since Monday, November 23");
    assert_eq!(sections[1], "Front page");
    assert_eq!(
        sections.last().map(String::as_str),
        Some("Back to Driver apps")
    );

    // Into Front page, line by line, then back to the sections.
    app.handle_event(&drive::key(Key::Down));
    app.handle_event(&drive::key(Key::Return));
    assert!(drive::top_is::<NewspaperSectionState>(&app));
    let lines = drive::with_top_ctx::<NewspaperSectionState, _>(&mut app, drive::build_labels);
    assert_eq!(lines[0], "Dana Rig passes 1,000,000 miles.");
    assert_eq!(lines.last().map(String::as_str), Some("Back to sections"));
    app.handle_event(&drive::key(Key::Down));
    assert!(drive::last(&app).starts_with("Mo K is first into Aberdeen, South Dakota."));
    app.handle_event(&drive::key(Key::Escape));
    assert!(drive::top_is::<NewspaperState>(&app));
}

#[test]
fn test_the_online_menu_row_sits_with_the_reading_rows_and_the_toggles_keep_their_rows() {
    let mut app = TestApp::new();
    app.ctx.services.newspaper.stage(Some(fixture()));
    let hub = OnlineHubState::new(&mut app.ctx);
    let hub = push(&mut app, hub);
    let rows = labels::<OnlineHubState>(&hub, &app.ctx);
    assert_eq!(rows[2], "Say when drivers go on or off duty: off");
    assert_eq!(rows[3], "Newspaper");
    assert_eq!(rows[4], "Account achievements");
    assert_eq!(rows[6], "Online services: on");

    // Right on the Newspaper row changes nothing; on Online services it
    // still flips that switch, one row further down.
    online::move_to::<OnlineHubState>(&mut app, &hub, "Newspaper");
    let before = app.ctx.settings.clone();
    online::press(&mut app, Key::Right);
    assert_eq!(app.ctx.settings.online_services, before.online_services);
    assert_eq!(
        app.ctx.settings.duty_notifications,
        before.duty_notifications
    );
    online::move_to::<OnlineHubState>(&mut app, &hub, "Online services");
    online::press(&mut app, Key::Right);
    assert!(!app.ctx.settings.online_services);

    online::move_to::<OnlineHubState>(&mut app, &hub, "Newspaper");
    online::press(&mut app, Key::Return);
    let top = app.state().expect("a screen");
    assert!(online::is_state::<NewspaperState>(&top));
    let sections = labels::<NewspaperState>(&top, &app.ctx);
    assert_eq!(sections.last().map(String::as_str), Some("Back"));
}

// -- the follow-up: translation scope, the title on return, row help ---------------------

const SPANISH: &str = r#"
msgid ""
msgstr ""
"Language: es\n"

msgid "Achievements"
msgstr "Logros"
"#;

fn spanish() -> Arc<ff_core::l10n::Translator> {
    Arc::new(ff_core::l10n::Translator::new(
        ff_core::l10n::Catalog::from_po("es", SPANISH).expect("test catalog parses"),
    ))
}

/// The issue's headings are the site's English while the app is open, and
/// the game's own rows of the same name translate again once it is left.
#[test]
fn test_the_issue_stays_english_only_while_the_app_is_open() {
    let mut app = TestApp::new();
    app.ctx.translator = spanish();
    app.ctx.services.newspaper.stage(Some(fixture()));
    assert_eq!(app.ctx.tr("Achievements"), "Logros");

    let hub = OnlineHubState::new(&mut app.ctx);
    let hub = push(&mut app, hub);
    online::move_to::<OnlineHubState>(&mut app, &hub, "Newspaper");
    online::press(&mut app, Key::Return);
    let paper = app.state().expect("a screen");
    assert!(online::is_state::<NewspaperState>(&paper));
    online::move_to::<NewspaperState>(&mut app, &paper, "Achievements");
    assert!(
        drive::last(&app).starts_with("Achievements."),
        "{}",
        drive::last(&app)
    );
    assert_eq!(app.ctx.tr("Achievements"), "Achievements");

    // Into a section and back: still the site's text.
    online::press(&mut app, Key::Return);
    assert!(drive::top_is::<NewspaperSectionState>(&app));
    assert_eq!(app.ctx.tr("Achievements"), "Achievements");
    online::press(&mut app, Key::Escape);
    assert!(drive::top_is::<NewspaperState>(&app));
    assert_eq!(app.ctx.tr("Achievements"), "Achievements");

    // Out of the app: the game's row translates again.
    online::press(&mut app, Key::Escape);
    assert!(drive::top_is::<OnlineHubState>(&app));
    assert_eq!(app.ctx.tr("Achievements"), "Logros");
    assert_eq!(app.ctx.tr("Achievements. 4 of 14."), "Logros. 4 of 14.");
}

#[test]
fn test_leaving_by_the_back_row_also_ends_the_english_text() {
    let mut app = TestApp::new();
    app.ctx.translator = spanish();
    app.ctx.services.newspaper.stage(Some(fixture()));
    let hub = OnlineHubState::new(&mut app.ctx);
    let hub = push(&mut app, hub);
    online::move_to::<OnlineHubState>(&mut app, &hub, "Newspaper");
    online::press(&mut app, Key::Return);
    assert_eq!(app.ctx.tr("Achievements"), "Achievements");
    online::press(&mut app, Key::End);
    online::press(&mut app, Key::Return);
    assert!(drive::top_is::<OnlineHubState>(&app));
    assert_eq!(app.ctx.tr("Achievements"), "Logros");
}

#[test]
fn test_opening_says_the_whole_title_and_coming_back_the_masthead() {
    let mut app = TestApp::new();
    app.ctx.services.newspaper.stage(Some(fixture()));
    let hub = OnlineHubState::new(&mut app.ctx);
    let hub = push(&mut app, hub);
    online::move_to::<OnlineHubState>(&mut app, &hub, "Newspaper");
    online::press(&mut app, Key::Return);
    let opened = app.main_lines();
    assert!(
        opened
            .iter()
            .any(|line| line == "The Mile Marker, issue 25. November 16 to 22, 2026."),
        "{opened:?}"
    );

    online::press(&mut app, Key::Down);
    online::press(&mut app, Key::Return);
    assert!(drive::top_is::<NewspaperSectionState>(&app));
    let before = app.main_lines().len();
    online::press(&mut app, Key::Escape);
    let back: Vec<String> = app.main_lines()[before..].to_vec();
    assert_eq!(
        back.first().map(String::as_str),
        Some("The Mile Marker."),
        "{back:?}"
    );
    assert!(
        back.iter().any(|line| line.starts_with("Front page.")),
        "{back:?}"
    );
    assert!(
        !back.iter().any(|line| line.contains("issue 25")),
        "{back:?}"
    );
}

#[test]
fn test_f1_on_a_section_row_says_how_to_open_it() {
    let mut app = TestApp::new();
    app.ctx.services.newspaper.stage(Some(fixture()));
    let hub = OnlineHubState::new(&mut app.ctx);
    let hub = push(&mut app, hub);
    online::move_to::<OnlineHubState>(&mut app, &hub, "Newspaper");
    online::press(&mut app, Key::Return);
    online::press(&mut app, Key::Down);
    online::press(&mut app, Key::F1);
    assert_eq!(
        drive::last(&app),
        "Enter opens a section. Escape goes back."
    );
}
