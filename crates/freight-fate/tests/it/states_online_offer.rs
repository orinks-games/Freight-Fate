//! Port of `tests/test_online_offer.py`: the one-time first-run orinks.net
//! offer.
//!
//! Nothing here touches the network: the offer itself makes no calls, and the
//! accept path is asserted by the state it pushes, not by running setup.
//!
//! Most tests here drive the state directly, which says nothing about how the
//! offer composes with the lines spoken around it. The two spoken-order tests
//! at the bottom drive a real career creation through the app instead, and
//! wait on the main menu port.

use crate::states_main_menu_support as menus;
use crate::states_online_support::*;
use freight_fate::app::testing::TestApp;
use freight_fate::app::SharedState;
use freight_fate::net::testing::FakeTransport;
use freight_fate::net::NetError;
use freight_fate::states::base::Key;
use freight_fate::states::city::CityMenuState;
use freight_fate::states::main_menu::{
    first_state_after_career_creation, HomeCityState, MainMenuState,
};
use freight_fate::states::online_offer::{should_offer_online, OnlineOfferState};
use freight_fate::states::online_states::OnlineSetupState;

/// A base screen under the offer, so the offer's exits have something to
/// replace and the stack never empties.
fn app_with_offer() -> (TestApp, SharedState) {
    let mut app = TestApp::new();
    app.ctx.settings.online_offer_seen = false;
    // The offer replaces itself with the city menu on the way out, and that
    // screen needs a career parked somewhere to describe.
    app.ctx.profile = Some(ff_core::models::profile::Profile::named_in(
        "Rookie", "Chicago",
    ));
    app.push_state(base_state());
    let offer = OnlineOfferState::new(&mut app.ctx);
    let shared = push(&mut app, offer);
    (app, shared)
}

#[test]
fn test_offered_when_the_gate_is_open_and_nothing_is_connected() {
    let mut app = TestApp::new();
    let _identity = install_identity(&app, None);
    app.ctx.settings.online_offer_seen = false;
    assert!(should_offer_online(&app.ctx));
}

#[test]
fn test_not_offered_once_seen() {
    let mut app = TestApp::new();
    let _identity = install_identity(&app, None);
    app.ctx.settings.online_offer_seen = true;
    assert!(!should_offer_online(&app.ctx));
}

/// A second career on a connected computer must not ask again -- the
/// connection is per computer, not per career.
#[test]
fn test_not_offered_when_already_connected() {
    let mut app = TestApp::new();
    let _identity = install_identity(&app, Some(&identity()));
    app.ctx.settings.online_offer_seen = false;
    assert!(!should_offer_online(&app.ctx));
}

#[test]
fn test_declining_sets_the_gate_and_enters_the_world() {
    let (mut app, offer) = app_with_offer();
    with_state::<OnlineOfferState, _>(&offer, |s| s.decline(&mut app.ctx));
    app.ctx.run_deferred();
    assert!(app.ctx.settings.online_offer_seen);
    // `("replace", "CityMenuState")`: the offer is gone and the world screen
    // stands in its place over the same base.
    assert!(is_state::<CityMenuState>(&app.state().unwrap()));
    assert_eq!(app.ctx.stack_len(), 2);
}

/// The player must never be stuck here, and backing out must still spend
/// the one offer -- otherwise it reappears on the next career.
#[test]
fn test_escape_behaves_exactly_like_not_now() {
    let (mut app, offer) = app_with_offer();
    press(&mut app, Key::Escape);
    assert!(app.ctx.settings.online_offer_seen);
    assert!(is_state::<CityMenuState>(&app.state().unwrap()));
    assert!(!std::rc::Rc::ptr_eq(&app.state().unwrap(), &offer));
    assert_eq!(app.ctx.stack_len(), 2);
}

#[test]
fn test_the_offer_names_where_to_find_it_later() {
    let (app, _offer) = app_with_offer();
    assert!(said(&app).contains("Online"));
}

/// Connecting switches both on, and the copy has to say so.
///
/// A substring check can never close this gap, in either direction: a
/// rewrite that keeps the two required phrases verbatim, then adds a clause
/// that walks one of them back -- or, as this copy read before 1.9, one that
/// says connecting only "lets you turn on" backup when it now turns it on --
/// still passes both a word-ban and a positive substring check while leaving
/// the player with a wrong idea of what is backed up or public. So this pins
/// the entire spoken line, word for word. Any future edit to this copy has to
/// update this literal string, which forces the person making the edit to
/// read what the offer now claims before it can ship.
#[test]
fn test_the_offer_says_backup_and_the_public_profile_both_come_on() {
    let mut app = TestApp::new();
    app.ctx.settings.online_offer_seen = false;
    app.clear_speech();
    let offer = OnlineOfferState::new(&mut app.ctx);
    push(&mut app, offer);
    assert_eq!(
        app.main_lines(),
        vec!["Before you set off. You can connect this computer to an \
              orinks.net account. That backs your career up so you can bring \
              it to another computer, and puts your driver profile and on-duty \
              activity on the public site. You can turn either of those off \
              afterwards from Online on the main menu. It takes a code and \
              your browser, and you can do it any time instead. Not now. 1 of 2."
            .to_string()]
    );
}

/// The low-effort answer on a one-shot consent prompt should be the one
/// that changes nothing.
#[test]
fn test_not_now_is_the_starting_item() {
    let (app, offer) = app_with_offer();
    assert!(current_label::<OnlineOfferState>(&offer, &app.ctx).contains("Not now"));
}

#[test]
fn test_accepting_pushes_setup_with_activation_already_started() {
    // OnlineSetupState must go on top via push_state (not replace_state),
    // so the CityMenuState underneath survives -- that is what makes
    // backing out of setup land in the world instead of back on this offer.
    let (mut app, offer) = app_with_offer();
    let _browser = install_browser(true);
    // The setup state's autostart contacts orinks.net; answer from a fake
    // so no request leaves the test.
    let _transport = install_transport(FakeTransport::failing(NetError::other(
        "OSError",
        "no network in tests",
    )));
    with_state::<OnlineOfferState, _>(&offer, |s| s.accept(&mut app.ctx));
    app.ctx.run_deferred();

    assert!(app.ctx.settings.online_offer_seen);
    let top = app.state().unwrap();
    assert!(is_state::<OnlineSetupState>(&top));
    // The flag, not just the state: pushing setup without autostart would
    // leave the player confirming a decision they already made.
    assert!(with_state::<OnlineSetupState, _>(&top, |s| s.autostart));
    // And the city menu is still reachable underneath, via replace_state on
    // the original offer state.
    let states = app.states();
    assert_eq!(states.len(), 3);
    assert!(is_state::<CityMenuState>(&states[1]));
    // Let the autostart worker settle before the app shuts down.
    if let Some(worker) = with_state::<OnlineSetupState, _>(&top, |s| s.worker.take()) {
        let _ = worker.join();
    }
}

#[test]
fn test_creating_a_first_career_reaches_the_offer() {
    // The offer replaces the city menu at career creation; its own exits put
    // the city menu back, so a player lands in the same place either way.
    let mut app = TestApp::new();
    let _identity = install_identity(&app, None);
    app.ctx.settings.online_offer_seen = false;
    app.ctx.profile = Some(ff_core::models::profile::Profile::named_in(
        "Rookie", "Chicago",
    ));
    let next = first_state_after_career_creation(&app.ctx);
    assert!(is_state::<OnlineOfferState>(&next));
}

#[test]
fn test_creating_a_later_career_goes_straight_to_the_city_menu() {
    let mut app = TestApp::new();
    let _identity = install_identity(&app, None);
    app.ctx.settings.online_offer_seen = true;
    app.ctx.profile = Some(ff_core::models::profile::Profile::named_in(
        "Rookie", "Chicago",
    ));
    let next = first_state_after_career_creation(&app.ctx);
    assert!(is_state::<CityMenuState>(&next));
}

// -- what a first-run player actually hears ------------------------------------
//
// Everything above tests one state in isolation. These drive the real career
// creation, because the defect they exist to catch is not in either line: it
// is one line interrupting the other, which only exists when the two are
// composed. They assert on what reaches the speech layer, in order, including
// whether each line cancelled the one before it.

/// An app whose next career creation is a genuine first run.
///
/// `TestApp::new` seeds the one-time gate as already spent for every test (as
/// `tests/conftest.py` does), so a test about the offer has to open it back
/// up -- otherwise this file would pin copy the player never reaches.
fn first_run_app() -> TestApp {
    let mut app = TestApp::new();
    app.ctx.settings.online_offer_seen = false;
    app
}

/// Drive New career as far as the home city picker, ready to confirm.
fn open_city_picker(app: &mut TestApp, name: &str) {
    app.push_state(MainMenuState::new());
    menus::select::<MainMenuState>(app, "New career");
    for ch in name.chars() {
        menus::typed(app, ch);
    }
    menus::key(app, Key::Return);
    menus::key(app, Key::Return); // default company start
    menus::key(app, Key::Return); // default region
    assert!(menus::is::<HomeCityState>(app));
}

#[test]
fn test_the_welcome_is_heard_in_full_and_then_the_offer() {
    // The welcome comes first and nothing cuts it off, then the offer.
    //
    // Speaking the welcome after pushing the offer state cancelled the offer
    // mid-sentence: the player heard "Welcome aboard", then silence, sitting
    // on an unexplained two-item consent menu whose disclosure -- that
    // connecting turns cloud backup and the public profile on -- had been cut
    // away.
    let mut app = first_run_app();
    let _identity = install_identity(&app, None);
    open_city_picker(&mut app, "Rookie");
    app.clear_speech();

    menus::key(&mut app, Key::Return); // confirm the home city

    assert!(menus::is::<OnlineOfferState>(&app));
    let calls = app.main_calls();
    let lines: Vec<String> = calls.iter().map(|(text, _)| text.clone()).collect();
    // The 1.9 line greets with the first-day briefing rather than dev's
    // shorter welcome; what matters here is that it opens the sequence.
    assert!(lines[0].starts_with("First-day briefing: "), "{lines:?}");
    assert!(lines[0].ends_with("Next: dispatch board."), "{lines:?}");
    // The parked-at location speaks the city's name, never the map key:
    // "in the chicago_il_us service area" reached a driver's ears before
    // the first agent-driven playtest caught it (2026-08-30).
    assert!(
        !lines[0].contains("_us") && lines[0].contains(" service area"),
        "{lines:?}"
    );
    assert!(lines[1].starts_with("Before you set off."), "{lines:?}");
    assert!(lines[1].ends_with("Not now. 1 of 2."), "{lines:?}");
    // The welcome opens the sequence, and every line after it queues, so both
    // are heard end to end rather than the last one winning.
    let interrupts: Vec<bool> = calls.iter().map(|(_, interrupt)| *interrupt).collect();
    let mut expected = vec![false; interrupts.len()];
    expected[0] = true;
    assert_eq!(interrupts, expected, "{lines:?}");
    assert_eq!(lines.len(), 2, "{lines:?}");
}

#[test]
fn test_saying_no_is_heard_before_the_city_menu_announcement() {
    // The one line naming where to find Online later must survive the move
    // into the world -- it is the whole of saying no.
    let mut app = first_run_app();
    let _identity = install_identity(&app, None);
    open_city_picker(&mut app, "Rookie");
    menus::key(&mut app, Key::Return); // confirm the home city
    assert!(menus::is::<OnlineOfferState>(&app));
    // the cursor starts here
    assert!(menus::current_label::<OnlineOfferState>(&app).contains("Not now"));
    app.clear_speech();

    menus::key(&mut app, Key::Return);

    assert!(menus::is::<CityMenuState>(&app));
    let calls = app.main_calls();
    let lines: Vec<String> = calls.iter().map(|(text, _)| text.clone()).collect();
    assert_eq!(lines[0], "Online on the main menu connects any time.");
    assert!(lines[1].ends_with(" dollars."), "{lines:?}");
    assert!(lines[2].starts_with("Dispatch board."), "{lines:?}");
    let interrupts: Vec<bool> = calls.iter().map(|(_, interrupt)| *interrupt).collect();
    let mut expected = vec![false; interrupts.len()];
    expected[0] = true;
    assert_eq!(interrupts, expected, "{lines:?}");
    assert_eq!(lines.len(), 3, "{lines:?}");
}
