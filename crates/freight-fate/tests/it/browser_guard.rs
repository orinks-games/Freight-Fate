//! Nothing in a test process may put a page in a real web browser.
//!
//! On 2026-08-24 the driver setup page opened in the owner's browser while
//! the suite was running: `open_url` reached the live site unless a test had
//! remembered to install an override, and the override lived in a
//! thread-local, so a page opened from any other thread was never covered by
//! it at all. Both halves of that are what this file pins.
//!
//! Read `freight_fate::browser` for the mechanism. In one line: the browser
//! is a capability `main()` grants, a test binary has no `main()` of the
//! game's, and so `open_url` in a test refuses, records the address, and
//! panics naming it.

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::Arc;

use freight_fate::app::testing::TestApp;
use freight_fate::browser;
use freight_fate::online_activation::Activation;
use freight_fate::states::base::State;
use freight_fate::states::online_states::{OnlineSetupState, SetupOutcome};

use crate::states_main_menu_support as menus;
use crate::states_online_support::{install_browser, push, with_state};

/// The refusal record is process-wide and every test in this binary shares
/// it, so each case here asks for an address only it would ever ask for.
fn unique_url(tag: &str) -> String {
    format!("https://orinks.net/activate?code=GUARD-{tag}")
}

fn was_refused(url: &str) -> bool {
    browser::refused_urls().iter().any(|seen| seen == url)
}

/// The capability itself: a test process is never the game.
///
/// If this ever passes with `true`, everything below it is decoration --
/// something has handed the suite a real browser.
#[test]
fn test_a_test_process_is_never_granted_a_real_browser() {
    assert!(!browser::real_browser_allowed());
}

#[test]
fn test_an_unseamed_open_is_refused_and_recorded_instead_of_opened() {
    let url = unique_url("plain");
    let outcome = catch_unwind(|| browser::open_url(&url));
    assert!(outcome.is_err(), "the open should have been refused");
    assert!(was_refused(&url), "{:?}", browser::refused_urls());
}

/// The case test discipline cannot catch.
///
/// The seam is per thread, so a page opened from a worker thread was never
/// covered by the override the test installed on its own thread -- no amount
/// of remembering to call `install_browser` fixes that. The capability is
/// process-wide, so the worker refuses exactly as the game loop would.
#[test]
fn test_a_spawned_thread_cannot_escape_the_seam() {
    let url = unique_url("spawned");
    let seam = install_browser(true);
    let spawned_url = url.clone();
    let worker = std::thread::Builder::new()
        .name("browser-guard-probe".to_string())
        .spawn(move || browser::open_url(&spawned_url))
        .expect("the probe thread starts");
    let outcome = worker.join();

    assert!(
        outcome.is_err(),
        "a spawned thread reached a browser: {outcome:?}"
    );
    assert!(was_refused(&url), "{:?}", browser::refused_urls());
    // And the seam on THIS thread never saw it, which is the whole point:
    // it could not have stood between the worker and the live site.
    assert!(seam.opened().is_empty());
}

/// The real game's behaviour is unchanged: a seam (in the game, the
/// capability) still hands the page to the opener and still answers with
/// whether it opened.
#[test]
fn test_an_installed_seam_still_answers_for_the_browser() {
    let url = unique_url("seamed");
    let seam = install_browser(true);
    assert!(browser::open_url(&url));
    assert_eq!(seam.opened(), vec![url.clone()]);
    assert!(!was_refused(&url));
}

/// The defect as it happened: the setup screen announcing an activation
/// code opens the verification page, and that page is the live driver setup
/// page with the player's code in it.
#[test]
fn test_the_activation_page_is_never_opened_for_real() {
    let mut app = TestApp::new();
    let url = unique_url("activation");
    let activation = Activation {
        device_code: "a".repeat(64),
        user_code: "GUARD-3468".to_string(),
        verification_uri: "https://orinks.net/activate".to_string(),
        verification_uri_complete: url.clone(),
        expires_at: freight_fate::states::online_states::wall_time() + 600.0,
        interval: 3.0,
    };
    let state = OnlineSetupState::new(&mut app.ctx);
    let shared = push(&mut app, state);
    with_state::<OnlineSetupState, _>(&shared, |s| {
        s.outcome.post(SetupOutcome::Activation(activation))
    });

    // No browser seam installed -- the mistake this whole file exists for.
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        with_state::<OnlineSetupState, _>(&shared, |s| State::update(s, &mut app.ctx, 0.0));
    }));

    assert!(
        outcome.is_err(),
        "the verification page was opened for real"
    );
    assert!(was_refused(&url), "{:?}", browser::refused_urls());
}

/// Report a problem used to call `webbrowser::open` directly, with no seam
/// on it at all: nothing could have stood between a test that pressed the
/// row and the live bug-report form.
#[test]
fn test_report_a_problem_goes_through_the_same_door() {
    let mut app = TestApp::new();
    let seam = install_browser(true);
    app.push_state(freight_fate::states::main_menu::MainMenuState::new());
    menus::select::<freight_fate::states::main_menu::MainMenuState>(&mut app, "Report a problem");

    let opened = seam.opened();
    assert_eq!(opened.len(), 1, "{opened:?}");
    assert!(opened[0].contains("/issues/new"), "{opened:?}");
    // The spoken half is unchanged: the player is told where it went and
    // which log to attach.
    let said = app.main_lines().join(" ");
    assert!(said.contains("Opening the bug report page"), "{said}");
}

/// A seam installed and then taken away leaves the thread refusing, not
/// reaching a browser -- otherwise a guard's `Drop` would re-arm the defect
/// for every test that ran on that thread afterwards.
#[test]
fn test_dropping_a_seam_leaves_the_thread_refusing() {
    let url = unique_url("dropped");
    {
        let _seam = install_browser(true);
    }
    let outcome = catch_unwind(|| browser::open_url(&url));
    assert!(outcome.is_err());
    assert!(was_refused(&url), "{:?}", browser::refused_urls());
}

/// The seam type is the one the game's own opener takes, so a test cannot
/// install something the game would not accept.
#[test]
fn test_a_seam_may_answer_that_the_browser_failed() {
    let url = unique_url("failing");
    let calls = Arc::new(std::sync::Mutex::new(Vec::new()));
    let log = Arc::clone(&calls);
    browser::set_open_url_override(Some(Arc::new(move |u: &str| {
        log.lock().unwrap().push(u.to_string());
        false
    })));
    let answered = browser::open_url(&url);
    browser::set_open_url_override(None);

    assert!(!answered);
    assert_eq!(calls.lock().unwrap().clone(), vec![url.clone()]);
    assert!(!was_refused(&url));
}

/// The activation address comes from the site; anything but a web page is
/// dropped before it reaches the system or the test seam.
#[test]
fn test_only_web_addresses_reach_the_browser() {
    let recorded = install_browser(true);
    for url in [
        "file:///C:/Windows/System32/calc.exe",
        "ms-settings:privacy",
        "javascript:alert(1)",
        "https://",
        "not a url",
    ] {
        assert!(!browser::is_web_address(url), "{url}");
        assert!(!browser::open_url(url), "{url}");
    }
    assert!(browser::is_web_address(
        "https://www.orinks.net/activate?code=AB"
    ));
    assert!(browser::is_web_address("http://localhost:3000/activate"));
    assert!(recorded.opened().is_empty(), "{:?}", recorded.opened());
}
