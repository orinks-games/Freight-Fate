//! Report a problem: the logs go ahead to orinks.net and the report form
//! opens in the browser; the GitHub page stays the fallback; and after a run
//! that never quit, the main menu asks once whether to report it.
//!
//! The relay is a fake installed on the test's thread; the screen reads it
//! on the game thread and hands it to its worker, so a real worker thread
//! still posts to the fake. Nothing here can reach orinks.net: the network
//! guard refuses a test process (see `network_guard`).

use std::sync::{Arc, Mutex};

use freight_fate::app::testing::TestApp;
use freight_fate::online_presence::base_url;
use freight_fate::problem_reports::{set_sender_override, Answer, Sender};
use freight_fate::states::base::{Key, State};
use freight_fate::states::main_menu::MainMenuState;
use freight_fate::states::problem_report::{CrashReportOfferState, ProblemReportLinkState};

use crate::states_main_menu_support as menus;
use crate::states_online_support::{install_browser, push, with_state};

/// A relay that records what it was sent and answers from a table.
pub struct FakeRelay {
    pub posts: Mutex<Vec<(String, serde_json::Value)>>,
    answer: Result<Answer, String>,
}

impl Sender for FakeRelay {
    fn post(&self, endpoint: &str, json: &[u8]) -> Result<Answer, String> {
        let report = serde_json::from_slice(json).expect("the report is JSON");
        self.posts
            .lock()
            .unwrap()
            .push((endpoint.to_string(), report));
        self.answer.clone()
    }
}

/// Clears the relay override on drop.
pub struct RelayGuard;

impl Drop for RelayGuard {
    fn drop(&mut self) {
        set_sender_override(None);
    }
}

/// Route the screen's send through a relay giving `answer`.
pub fn install_relay(answer: Result<Answer, String>) -> (Arc<FakeRelay>, RelayGuard) {
    let relay = Arc::new(FakeRelay {
        posts: Mutex::new(Vec::new()),
        answer,
    });
    set_sender_override(Some(relay.clone()));
    (relay, RelayGuard)
}

/// The page the relay hands back for a report, on the site's own origin.
pub fn page_url() -> String {
    format!("{}/report/0123456789abcdef", base_url())
}

/// A relay that takes the logs and answers with the page.
pub fn opening_relay() -> (Arc<FakeRelay>, RelayGuard) {
    install_relay(Ok(Answer {
        status: 201,
        body: serde_json::json!({ "code": "0123456789abcdef", "url": page_url() }).to_string(),
    }))
}

/// Wait for the screen's send, then run the frame that reads its answer.
pub fn finish_send(app: &mut TestApp) {
    let shared = app.state().expect("a state on the stack");
    let worker = with_state::<ProblemReportLinkState, _>(&shared, |s| s.worker.take());
    if let Some(worker) = worker {
        worker.join().expect("the send finished");
    }
    with_state::<ProblemReportLinkState, _>(&shared, |s| State::update(s, &mut app.ctx, 0.0));
}

fn said(app: &TestApp) -> String {
    app.main_lines().join(" ")
}

#[test]
fn test_report_a_problem_sends_the_logs_ahead_and_opens_the_site_form() {
    let mut app = TestApp::new();
    let browser = install_browser(true);
    let (relay, _relay) = opening_relay();
    push(&mut app, MainMenuState::new());

    menus::select::<MainMenuState>(&mut app, "Report a problem");
    assert!(menus::is::<ProblemReportLinkState>(&app));
    assert!(
        said(&app).contains("Preparing the report."),
        "{}",
        said(&app)
    );
    finish_send(&mut app);

    let posts = relay.posts.lock().unwrap().clone();
    assert_eq!(posts.len(), 1, "{posts:?}");
    let (endpoint, report) = &posts[0];
    assert_eq!(
        *endpoint,
        format!("{}/api/problem-reports/open", base_url())
    );
    assert_eq!(report["app"], "Freight Fate");
    assert_eq!(report["version"], freight_fate::app::version());
    assert_eq!(report["description"], "");
    assert_eq!(report["crashed"], false);

    assert_eq!(browser.opened(), vec![page_url()]);
    assert!(
        said(&app).contains("The report opened in your browser."),
        "{}",
        said(&app)
    );
    assert!(menus::is::<MainMenuState>(&app), "back on the main menu");
}

#[test]
fn test_the_github_page_is_the_fallback_when_the_site_cannot_be_reached() {
    let mut app = TestApp::new();
    let browser = install_browser(true);
    let (_relay, _guard) = install_relay(Err("The connection timed out.".to_string()));
    push(&mut app, MainMenuState::new());

    menus::select::<MainMenuState>(&mut app, "Report a problem");
    finish_send(&mut app);

    let opened = browser.opened();
    assert_eq!(opened.len(), 1, "{opened:?}");
    assert!(opened[0].contains("/issues/new"), "{opened:?}");
    let said = said(&app);
    assert!(
        said.contains(
            "The report form could not be opened: The connection timed out. Opening the \
             bug report page on GitHub instead. Attach your game log: game.log in"
        ),
        "{said}"
    );
    assert!(menus::is::<MainMenuState>(&app));
}

#[test]
fn test_a_daily_limit_is_said_in_plain_words() {
    let mut app = TestApp::new();
    let _browser = install_browser(true);
    let (_relay, _guard) = install_relay(Ok(Answer {
        status: 429,
        body: String::new(),
    }));
    push(&mut app, MainMenuState::new());

    menus::select::<MainMenuState>(&mut app, "Report a problem");
    finish_send(&mut app);

    assert!(
        said(&app).contains("Too many reports have been sent today."),
        "{}",
        said(&app)
    );
}

#[test]
fn test_no_browser_at_all_names_the_issues_page() {
    let mut app = TestApp::new();
    let browser = install_browser(false);
    let (_relay, _guard) = opening_relay();
    push(&mut app, MainMenuState::new());

    menus::select::<MainMenuState>(&mut app, "Report a problem");
    finish_send(&mut app);

    // One try: a browser that would not open the page gets no second page.
    assert_eq!(browser.opened(), vec![page_url()]);
    let said = said(&app);
    assert!(
        said.contains(
            "Could not open a web browser. Report problems at \
             github.com/orinks-games/Freight-Fate/issues."
        ),
        "{said}"
    );
    assert!(menus::is::<MainMenuState>(&app));
}

#[test]
fn test_escape_leaves_while_the_logs_are_still_going() {
    let mut app = TestApp::new();
    let _browser = install_browser(true);
    let (_relay, _guard) = opening_relay();
    push(&mut app, MainMenuState::new());

    menus::select::<MainMenuState>(&mut app, "Report a problem");
    let shared = app.state().expect("the report screen");
    menus::key(&mut app, Key::Escape);
    assert!(menus::is::<MainMenuState>(&app));

    // The send finishes on its own and nothing more is said or opened.
    let worker = with_state::<ProblemReportLinkState, _>(&shared, |s| s.worker.take());
    if let Some(worker) = worker {
        worker.join().unwrap();
    }
}

#[test]
fn test_the_main_menu_offers_to_report_a_crash_once() {
    let mut app = TestApp::new();
    let _browser = install_browser(true);
    let (relay, _guard) = opening_relay();
    app.ctx.crash_offer_pending = true;
    let menu = push(&mut app, MainMenuState::new());

    with_state::<MainMenuState, _>(&menu, |s| State::update(s, &mut app.ctx, 0.0));
    assert!(menus::is::<CrashReportOfferState>(&app));
    assert!(!app.ctx.crash_offer_pending, "asked once");
    let said = said(&app);
    assert!(
        said.contains("Freight Fate closed unexpectedly the last time it ran. Report the problem?"),
        "{said}"
    );

    menus::select::<CrashReportOfferState>(&mut app, "Report it");
    assert!(menus::is::<ProblemReportLinkState>(&app));
    finish_send(&mut app);
    let posts = relay.posts.lock().unwrap().clone();
    assert_eq!(posts.len(), 1);
    assert_eq!(posts[0].1["crashed"], true, "{:?}", posts[0].1);
    assert!(menus::is::<MainMenuState>(&app));
}

#[test]
fn test_not_now_leaves_the_crash_question_for_the_menu_row() {
    let mut app = TestApp::new();
    app.ctx.crash_offer_pending = true;
    let menu = push(&mut app, MainMenuState::new());
    with_state::<MainMenuState, _>(&menu, |s| State::update(s, &mut app.ctx, 0.0));
    assert!(menus::is::<CrashReportOfferState>(&app));

    menus::select::<CrashReportOfferState>(&mut app, "Not now");
    assert!(menus::is::<MainMenuState>(&app));
    assert!(
        app.ctx.settings.crash_report_offer,
        "the setting is untouched"
    );
    // A second frame does not ask again.
    with_state::<MainMenuState, _>(&menu, |s| State::update(s, &mut app.ctx, 0.0));
    assert!(menus::is::<MainMenuState>(&app));
}

#[test]
fn test_stop_asking_turns_the_setting_off() {
    let mut app = TestApp::new();
    app.ctx.crash_offer_pending = true;
    let menu = push(&mut app, MainMenuState::new());
    with_state::<MainMenuState, _>(&menu, |s| State::update(s, &mut app.ctx, 0.0));

    menus::select::<CrashReportOfferState>(&mut app, "Stop asking after crashes");
    assert!(!app.ctx.settings.crash_report_offer);
    assert!(menus::is::<MainMenuState>(&app));
    assert!(
        said(&app).contains("will not ask again after a crash"),
        "{}",
        said(&app)
    );
}

#[test]
fn test_the_crash_question_is_skipped_when_the_setting_is_off() {
    let mut app = TestApp::new();
    app.ctx.settings.crash_report_offer = false;
    app.ctx.crash_offer_pending = true;
    let menu = push(&mut app, MainMenuState::new());
    with_state::<MainMenuState, _>(&menu, |s| State::update(s, &mut app.ctx, 0.0));
    assert!(menus::is::<MainMenuState>(&app));
    assert!(!app.ctx.crash_offer_pending);
    assert!(
        !said(&app).contains("closed unexpectedly"),
        "{}",
        said(&app)
    );
}
