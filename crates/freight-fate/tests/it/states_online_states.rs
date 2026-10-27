//! Port of `tests/test_profile_sharing_sync.py`, plus the Online menu's
//! left/right row alignment.

use crate::states_online_support::*;
use freight_fate::app::testing::TestApp;
use freight_fate::app::SharedState;
use freight_fate::net::testing::FakeTransport;
use freight_fate::net::NetError;
use freight_fate::states::base::{Key, State};
use freight_fate::states::online_hub::OnlineHubState;
use freight_fate::states::online_states::{ProfileSharingSyncState, DISCLOSURE};

#[test]
fn test_disclosure_is_single_profile_sharing_consent() {
    let lowered = DISCLOSURE.to_lowercase();
    assert!(lowered.contains("profile sharing"));
    assert!(lowered.contains("road-journal"));
    assert!(lowered.contains("official achievements"));
    assert!(lowered.contains("updates feed"));
    assert!(!lowered.contains("separately"));
    assert!(!lowered.contains("unlisted"));
    assert!(!lowered.contains("factual"));
}

/// Since 1.9 connecting an account turns Profile sharing and cloud backup
/// on, so the one screen a player reaches for to hear what they are agreeing
/// to has to say that -- and still say how to undo it.
#[test]
fn test_disclosure_says_connecting_turns_both_on() {
    let lowered = DISCLOSURE.to_lowercase();
    assert!(lowered.contains("turns profile sharing on"));
    assert!(lowered.contains("backing your careers up"));
    assert!(lowered.contains("online menu"));
    assert!(!lowered.contains("off until you turn it on"));
}

/// A base screen under the sync menu, so a pop has somewhere to land.
fn sharing_app(enabled: bool) -> (TestApp, SharedState) {
    let mut app = TestApp::new();
    app.push_state(base_state());
    let mut state = ProfileSharingSyncState::new(&mut app.ctx, enabled);
    state.threaded = false;
    let shared = push(&mut app, state);
    (app, shared)
}

fn post_and_update(app: &mut TestApp, state: &SharedState, outcome: &'static str) {
    with_state::<ProfileSharingSyncState, _>(state, |s| {
        s.outcome.post(outcome);
        s.update(&mut app.ctx, 0.0);
    });
    app.ctx.run_deferred();
}

#[test]
fn test_disable_success_confirms_off_and_clears_pending() {
    let (mut app, state) = sharing_app(false);
    app.ctx.settings.online_presence = true;
    app.ctx.settings.profile_sharing_pending_off = true;
    post_and_update(&mut app, &state, "ok");
    assert!(!app.ctx.settings.online_presence);
    assert!(!app.ctx.settings.profile_sharing_pending_off);
    assert!(app
        .main_lines()
        .iter()
        .any(|t| t.contains("Profile sharing is off")));
    assert!(!std::rc::Rc::ptr_eq(&app.state().unwrap(), &state));
}

#[test]
fn test_disable_failure_remains_pending_and_never_claims_off() {
    let (mut app, state) = sharing_app(false);
    app.ctx.settings.online_presence = true;
    app.ctx.settings.profile_sharing_pending_off = true;
    post_and_update(&mut app, &state, "error");
    assert!(app.ctx.settings.online_presence);
    assert!(app.ctx.settings.profile_sharing_pending_off);
    let said = app.main_lines();
    assert!(said.iter().any(|t| t.contains("may still be public")));
    assert!(!said.iter().any(|t| t.contains("Profile sharing is off.")));
    assert!(std::rc::Rc::ptr_eq(&app.state().unwrap(), &state));
}

#[test]
fn test_disable_start_stops_local_services_before_server_confirmation() {
    let (mut app, state) = sharing_app(false);
    let _identity = install_identity(&app, Some(&identity()));
    let _transport = install_transport(FakeTransport::failing(NetError::http(500)));
    app.ctx.settings.online_presence = true;
    app.ctx.adopt_online_identity(Some(identity()));
    app.ctx.apply_online_presence();
    assert!(app.ctx.services.online.enabled());
    with_state::<ProfileSharingSyncState, _>(&state, |s| s.start(&mut app.ctx));
    assert!(app.ctx.settings.profile_sharing_pending_off);
    assert!(!app.ctx.services.online.enabled());
    assert!(!app.ctx.services.journal.enabled());
    with_state::<ProfileSharingSyncState, _>(&state, |s| s.update(&mut app.ctx, 0.0));
    assert_eq!(
        labels::<ProfileSharingSyncState>(&state, &app.ctx)[0],
        "Turn Profile sharing off"
    );
}

#[test]
fn test_enable_success_and_failure_are_server_authoritative() {
    let (mut app, failed) = sharing_app(true);
    post_and_update(&mut app, &failed, "error");
    assert!(!app.ctx.settings.online_presence);
    assert!(!app
        .main_lines()
        .iter()
        .any(|t| t.contains("Profile sharing is on.")));

    app.pop_state();
    app.clear_speech();
    let mut succeeded = ProfileSharingSyncState::new(&mut app.ctx, true);
    succeeded.threaded = false;
    let succeeded = push(&mut app, succeeded);
    post_and_update(&mut app, &succeeded, "ok");
    assert!(app.ctx.settings.online_presence);
    assert_eq!(app.ctx.settings.profile_sharing_consent_version, 3);
    assert!(app
        .main_lines()
        .iter()
        .any(|t| t.contains("Profile sharing is on.")));
    assert!(!std::rc::Rc::ptr_eq(&app.state().unwrap(), &succeeded));
}

#[test]
fn test_cancel_returns_without_changing_profile_sharing() {
    let (mut app, state) = sharing_app(true);
    app.ctx.settings.online_presence = false;
    press(&mut app, Key::Escape);
    assert!(!app.ctx.settings.online_presence);
    assert!(!std::rc::Rc::ptr_eq(&app.state().unwrap(), &state));
}

// `test_only_current_profile_sharing_consent_can_remain_on` is live in
// `crates/ff-core/src/settings/tests.rs`: the gate is Settings::load's.

// -- the Online menu ------------------------------------------------------------------

/// `_open_online_settings`: the Settings picker's Online pointer opens the
/// hub; with the main menu unported the hub is pushed directly.
fn open_online_settings(app: &mut TestApp) -> SharedState {
    let hub = OnlineHubState::new(&mut app.ctx);
    push(app, hub)
}

// The online category wires labels and left/right handlers as two
// parallel lists; a new row must land in both at the same index or
// left/right silently retargets a neighboring setting.
#[test]
fn test_online_adjust_rows_still_line_up() {
    let mut app = TestApp::new();
    let _identity = install_identity(&app, Some(&identity()));
    let cat = open_online_settings(&mut app);
    let before = app.ctx.settings.discord_presence;
    move_to::<OnlineHubState>(&mut app, &cat, "Discord presence");
    press(&mut app, Key::Right);
    assert_eq!(app.ctx.settings.discord_presence, !before);
}
