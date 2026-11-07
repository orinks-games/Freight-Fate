//! Port of `tests/test_online_setup.py`: OnlineSetupState, the
//! activation-code setup menu.
//!
//! The menu is static at five items; only the first item's label carries
//! progress (see the type docs). `start_activation` and `poll_activation` are
//! answered by a fake transport here -- the same style the network tier's own
//! tests use -- so nothing in this file touches the network or a real browser.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::states_online_support::*;
use freight_fate::app::testing::TestApp;
use freight_fate::app::SharedState;
use freight_fate::cloud_saves::AUTH_HELP;
use freight_fate::net::testing::{ClosureTransport, FakeTransport};
use freight_fate::net::{Event, NetError, SharedTransport};
use freight_fate::online_activation::{spell_code, Activation, PollResult};
use freight_fate::online_presence::{IdentityStore, MemoryStore};
use freight_fate::states::base::{InputEvent, Menu, State};
use freight_fate::states::online_states::{
    load_identity, poll_loop, set_identity_store_override, Mailbox, OnlineSetupState, PollSchedule,
    SetupOutcome, SetupPhase, PROFILE_SHARING_CONSENT_VERSION,
};
use serde_json::{json, Value};

fn an_activation() -> Activation {
    Activation {
        device_code: "a".repeat(64),
        user_code: "WKQR-3468".to_string(),
        verification_uri: "https://orinks.net/activate".to_string(),
        verification_uri_complete: "https://orinks.net/activate?code=WKQR-3468".to_string(),
        expires_at: freight_fate::states::online_states::wall_time() + 600.0,
        interval: 3.0,
    }
}

/// The server's `activate/start` reply for [`an_activation`].
fn activation_reply() -> Value {
    json!({
        "device_code": "a".repeat(64),
        "user_code": "WKQR-3468",
        "verification_uri": "https://orinks.net/activate",
        "verification_uri_complete": "https://orinks.net/activate?code=WKQR-3468",
        "expires_in": 600,
        "interval": 3,
    })
}

fn ready() -> PollResult {
    PollResult {
        status: "ready".to_string(),
        driver_id: Some("rig-hauler".to_string()),
        token: Some(format!("ffd_{}", "b".repeat(64))),
        display_name: Some("Rig Hauler".to_string()),
    }
}

/// A transport whose every call waits on `gate` (the tests' NeverRunsThread:
/// the request is in flight for as long as the test wants), counting calls
/// as they arrive.
struct Gate {
    gate: Arc<Event>,
    calls: Arc<AtomicUsize>,
}

impl Gate {
    fn new() -> (Self, SharedTransport) {
        let gate = Arc::new(Event::new());
        let calls = Arc::new(AtomicUsize::new(0));
        let (g, c) = (Arc::clone(&gate), Arc::clone(&calls));
        let transport: SharedTransport = Arc::new(ClosureTransport(
            move |_url: &str, _p: Option<&Value>, _h: &[(String, String)], _m: Option<&str>| {
                c.fetch_add(1, Ordering::SeqCst);
                g.wait(Duration::from_secs(10));
                Err(NetError::other("OSError", "gated"))
            },
        ));
        (Self { gate, calls }, transport)
    }

    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }

    fn release(&self) {
        self.gate.set();
    }
}

/// `_make_ctx` + `OnlineSetupState(ctx)` + `state.enter()`: the app with a
/// base screen under the setup menu, the menu on top, workers inline.
fn setup_app(autostart: bool) -> (TestApp, SharedState) {
    let mut app = TestApp::new();
    app.push_state(base_state());
    let mut state = OnlineSetupState::with_autostart(&mut app.ctx, autostart);
    state.threaded = false;
    app.clear_speech();
    let shared = push(&mut app, state);
    (app, shared)
}

fn update(app: &mut TestApp, shared: &SharedState) {
    with_state::<OnlineSetupState, _>(shared, |s| State::update(s, &mut app.ctx, 0.0));
    app.ctx.run_deferred();
}

fn setup(
    app: &mut TestApp,
    shared: &SharedState,
    f: impl FnOnce(&mut OnlineSetupState, &mut freight_fate::app::GameContext),
) {
    with_state::<OnlineSetupState, _>(shared, |s| f(s, &mut app.ctx));
    app.ctx.run_deferred();
}

fn phase(shared: &SharedState) -> SetupPhase {
    with_state::<OnlineSetupState, _>(shared, |s| s.phase)
}

fn activation_of(shared: &SharedState) -> Option<Activation> {
    with_state::<OnlineSetupState, _>(shared, |s| s.activation.clone())
}

/// `"poll_loop" stubbed` + `start_activation` answered: inline mode runs the
/// start request and posts the activation without entering the poll loop.
fn start_answers(reply: Value) -> TransportGuard {
    install_transport(FakeTransport::replying(reply))
}

// -- starting ---------------------------------------------------------------

#[test]
fn test_starting_speaks_the_activation_code() {
    let (mut app, state) = setup_app(false);
    let _browser = install_browser(true);
    let _transport = start_answers(activation_reply());

    setup(&mut app, &state, |s, ctx| s.start_setup(ctx));
    update(&mut app, &state); // drains the "activation" outcome, which is what speaks the code

    assert!(app
        .main_lines()
        .iter()
        .any(|line| line.contains("WKQR-3468")));
    assert_eq!(activation_of(&state), Some(an_activation_at(&state)));
    assert_eq!(phase(&state), SetupPhase::Waiting);
}

/// The activation the state holds, with its own `expires_at` (the server's
/// relative deadline was resolved when the reply arrived).
fn an_activation_at(state: &SharedState) -> Activation {
    let expires_at = activation_of(state).map(|a| a.expires_at).unwrap_or(0.0);
    Activation {
        expires_at,
        ..an_activation()
    }
}

#[test]
fn test_start_failure_is_spoken_and_recoverable() {
    let (mut app, state) = setup_app(false);
    let _transport = install_transport(FakeTransport::failing(NetError::http(503)));

    setup(&mut app, &state, |s, ctx| s.start_setup(ctx));
    update(&mut app, &state); // drains the "start_failed" outcome, which is what speaks the failure

    assert!(app
        .main_lines()
        .iter()
        .any(|line| line.contains("Could not reach orinks.net")));
    assert_eq!(activation_of(&state), None);
    assert_eq!(phase(&state), SetupPhase::Idle);
}

#[test]
fn test_browser_that_would_not_open_speaks_address_and_code_and_keeps_polling() {
    let (mut app, state) = setup_app(false);
    let _browser = install_browser(false);
    let _transport = start_answers(activation_reply());

    setup(&mut app, &state, |s, ctx| s.start_setup(ctx));
    update(&mut app, &state); // drains the "activation" outcome, which is what announces the failure

    let activation = an_activation();
    let last = app.main_lines().last().cloned().unwrap();
    assert!(last.contains(&activation.verification_uri));
    assert!(last.contains(&activation.user_code));
    assert!(last.contains("Say my activation code again"));
    assert!(last.contains("Copy my activation code"));
    // Opening the browser failing must not cancel polling.
    assert_eq!(phase(&state), SetupPhase::Waiting);
}

// -- review affordances: item 2 and item 3 -----------------------------------

#[test]
fn test_repeat_item_spells_the_code_phonetically() {
    let (mut app, state) = setup_app(false);
    setup(&mut app, &state, |s, ctx| {
        s.activation = Some(an_activation());
        s.repeat_code(ctx);
    });
    let spelled = spell_code("WKQR-3468");
    assert!(app.main_lines().iter().any(|line| line.contains(&spelled)));
}

#[test]
fn test_repeat_item_without_a_code_points_back_at_setup() {
    let (mut app, state) = setup_app(false);
    setup(&mut app, &state, |s, ctx| s.repeat_code(ctx));
    assert!(app
        .main_lines()
        .iter()
        .any(|line| line.contains("Set up this computer with orinks.net")));
}

#[test]
fn test_copy_item_reports_success() {
    let (mut app, state) = setup_app(false);
    setup(&mut app, &state, |s, ctx| {
        s.activation = Some(an_activation());
        s.copy_code(ctx);
    });
    let lines = app.main_lines();
    assert!(lines
        .iter()
        .any(|line| line.to_lowercase().contains("copied")));
    assert!(!lines
        .iter()
        .any(|line| line.to_lowercase().contains("could not copy")));
}

#[test]
fn test_copy_item_never_claims_a_failed_copy() {
    let (mut app, state) = setup_app(false);
    app.ctx.clipboard = Box::new(RefusingClipboard);
    setup(&mut app, &state, |s, ctx| {
        s.activation = Some(an_activation());
        s.copy_code(ctx);
    });
    let lines = app.main_lines();
    assert!(!lines.iter().any(|line| {
        let lower = line.to_lowercase();
        lower.contains("copied") && !lower.contains("could not")
    }));
    assert!(lines
        .iter()
        .any(|line| line.to_lowercase().contains("could not copy")));
}

// -- polling dispatch, driven through the real poll_activation --------------
//
// These inject a fake *transport* and let the real poll_activation turn it
// into a PollResult, then feed that through the real poll_loop -- covering
// the status-to-outcome mapping end to end, which the mailbox-driven tests
// below (by design) do not.

fn run_real_poll_loop(
    activation: &Activation,
    transport: &dyn freight_fate::net::Transport,
) -> Option<SetupOutcome> {
    let outcome = Mailbox::new();
    poll_loop(
        activation,
        &Event::new(),
        &outcome,
        transport,
        &PollSchedule::default(),
    );
    outcome.take()
}

fn ready_reply() -> Value {
    json!({
        "status": "ready",
        "driver_id": "rig-hauler",
        "token": format!("ffd_{}", "b".repeat(64)),
        "display_name": "Rig Hauler",
    })
}

#[test]
fn test_poll_loop_reaches_ready_with_the_result_intact() {
    let outcome = run_real_poll_loop(
        &an_activation(),
        FakeTransport::replying(ready_reply()).as_ref(),
    );
    let Some(SetupOutcome::Ready(result)) = outcome else {
        panic!("expected ready, got {outcome:?}");
    };
    assert_eq!(result.driver_id.as_deref(), Some("rig-hauler"));
    assert_eq!(result.display_name.as_deref(), Some("Rig Hauler"));
}

#[test]
fn test_poll_loop_stops_on_expired_from_a_410() {
    let outcome = run_real_poll_loop(
        &an_activation(),
        FakeTransport::failing(NetError::http(410)).as_ref(),
    );
    assert_eq!(outcome, Some(SetupOutcome::Expired));
}

/// The code's own expires_at passing is checked before the network call --
/// an activation that arrived already past its deadline never has to ask
/// the server at all.
#[test]
fn test_poll_loop_stops_on_expired_from_the_deadline_without_polling() {
    let activation = Activation {
        expires_at: freight_fate::states::online_states::wall_time() - 1.0,
        ..an_activation()
    };
    let transport = FakeTransport::replying(json!({"status": "pending"}));
    let outcome = run_real_poll_loop(&activation, transport.as_ref());
    assert_eq!(outcome, Some(SetupOutcome::Expired));
    assert_eq!(transport.request_count(), 0);
}

#[test]
fn test_poll_loop_stops_on_a_400_corrupt_code() {
    let outcome = run_real_poll_loop(
        &an_activation(),
        FakeTransport::failing(NetError::http(400)).as_ref(),
    );
    assert_eq!(outcome, Some(SetupOutcome::Error));
}

/// The regression finding 1 fixes: a 503 and a dropped connection must
/// not be terminal the way a 400 is -- the loop keeps polling the same code
/// and still reaches "ready" once the network recovers.
#[test]
fn test_poll_loop_survives_transient_failures_and_still_reaches_ready() {
    let calls = Arc::new(AtomicUsize::new(0));
    let seen = Arc::clone(&calls);
    let transport = ClosureTransport(
        move |_url: &str, _p: Option<&Value>, _h: &[(String, String)], _m: Option<&str>| match seen
            .fetch_add(1, Ordering::SeqCst)
            + 1
        {
            1 => Err(NetError::http(503)),
            2 => Err(NetError::other("OSError", "connection reset")),
            _ => Ok(ready_reply()),
        },
    );
    let outcome = Mailbox::new();
    let schedule = PollSchedule {
        interval_first_s: 0.001,
        interval_later_s: 0.001,
        ..PollSchedule::default()
    };
    poll_loop(
        &an_activation(),
        &Event::new(),
        &outcome,
        &transport,
        &schedule,
    );
    assert_eq!(calls.load(Ordering::SeqCst), 3);
    let Some(SetupOutcome::Ready(result)) = outcome.take() else {
        panic!("expected ready");
    };
    assert_eq!(result.driver_id.as_deref(), Some("rig-hauler"));
}

// -- polling outcomes, driven directly through the update() mailbox ---------

/// A pending poll posts no outcome; only the elapsed-time check in
/// update() ever speaks "Still waiting." -- separately from this test.
#[test]
fn test_pending_poll_keeps_waiting_and_speaks_nothing_new() {
    // The Python StopAfterTwoWaits: the second poll is the last one.
    let stop = Arc::new(Event::new());
    let calls = Arc::new(AtomicUsize::new(0));
    let (s, c) = (Arc::clone(&stop), Arc::clone(&calls));
    let transport = ClosureTransport(
        move |_url: &str, _p: Option<&Value>, _h: &[(String, String)], _m: Option<&str>| {
            if c.fetch_add(1, Ordering::SeqCst) + 1 >= 2 {
                s.set();
            }
            Ok(json!({"status": "pending"}))
        },
    );
    let outcome = Mailbox::new();
    let schedule = PollSchedule {
        interval_first_s: 0.001,
        interval_later_s: 0.001,
        ..PollSchedule::default()
    };
    poll_loop(&an_activation(), &stop, &outcome, &transport, &schedule);
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert!(outcome.take().is_none());
}

/// The whole point of this screen's focus handler is the browser round
/// trip, so the event has to arrive the way the app loop delivers it --
/// through `App::handle_event`, not poked at the state. The port's `match`
/// on the event kind used to swallow focus before any screen saw it, and
/// coming back from the browser was met with silence.
#[test]
fn test_coming_back_from_the_browser_re_orients_the_player() {
    let (mut app, state) = setup_app(false);
    setup(&mut app, &state, |s, _| {
        s.activation = Some(an_activation());
        s.phase = SetupPhase::Waiting;
    });
    app.clear_speech();

    app.handle_event(&InputEvent::WindowFocusGained);

    let lines = app.main_lines();
    assert!(
        lines
            .iter()
            .any(|line| line.starts_with("Back in Freight Fate.")),
        "{lines:#?}"
    );
}

#[test]
fn test_still_waiting_is_spoken_once_after_five_seconds() {
    let (mut app, state) = setup_app(false);
    setup(&mut app, &state, |s, _| {
        s.activation = Some(an_activation());
        s.phase = SetupPhase::Waiting;
        s.poll_started = Some(Instant::now() - Duration::from_secs(6));
    });
    update(&mut app, &state);
    update(&mut app, &state); // a second frame must not repeat the line
    let waits = app
        .main_lines()
        .into_iter()
        .filter(|line| line == "Still waiting.")
        .count();
    assert_eq!(waits, 1);
}

/// The line fires five seconds after the announcement *starts*, and the
/// browser-failed announcement (code, address, and both fallback menu items)
/// takes far longer than that to speak. Interrupting would cut the player off
/// mid-address on the one path -- a remote session where no browser opens --
/// where hearing the address is the only way to finish setup.
#[test]
fn test_still_waiting_never_interrupts_the_code_announcement() {
    let (mut app, state) = setup_app(false);
    setup(&mut app, &state, |s, _| {
        s.activation = Some(an_activation());
        s.phase = SetupPhase::Waiting;
        s.poll_started = Some(Instant::now() - Duration::from_secs(6));
    });
    update(&mut app, &state);
    assert!(app
        .main_calls()
        .contains(&("Still waiting.".to_string(), false)));
}

/// The interval has to measure five seconds of actual waiting, not five
/// seconds of the announcement still being spoken -- so the clock starts
/// once the code has been announced, not before. (The Python test read the
/// clock from inside the speech stub; here the clock is checked to be
/// unstarted before the frame that announces, and running after it.)
#[test]
fn test_still_waiting_clock_starts_after_the_announcement() {
    let (mut app, state) = setup_app(false);
    let _browser = install_browser(true);
    let _transport = start_answers(activation_reply());
    setup(&mut app, &state, |s, ctx| s.start_setup(ctx));
    assert!(with_state::<OnlineSetupState, _>(&state, |s| s
        .poll_started
        .is_none()));
    app.clear_speech();
    update(&mut app, &state); // drains the "activation" outcome: announce, then start the clock
    assert!(app
        .main_lines()
        .iter()
        .any(|line| line.contains("WKQR-3468")));
    assert!(with_state::<OnlineSetupState, _>(&state, |s| s
        .poll_started
        .is_some()));
}

/// Run the setup-time Profile sharing call inline against a server that
/// answers `ok` (or refuses), recording what it was asked for.
fn sharing_answers(ok: bool) -> (TransportGuard, Arc<FakeTransport>) {
    let transport = if ok {
        FakeTransport::replying(json!({"ok": true, "enabled": true}))
    } else {
        FakeTransport::failing(NetError::http(500))
    };
    (install_transport(transport.clone()), transport)
}

#[test]
fn test_ready_poll_adopts_identity_and_speaks_the_display_name() {
    let (mut app, state) = setup_app(false);
    // The identity store records what was saved rather than discarding it:
    // driver_id reaching adopt_online_identity is not enough to prove the
    // right token was saved -- a bug that adopted the correct driver with a
    // wrong, truncated, or empty token would still pass every other
    // assertion here, and would only surface later, silently, at the next
    // presence heartbeat.
    let _identity = install_identity(&app, None);
    install_cloud(&mut app, FakeTransport::revisions(), false);
    let (_sharing, _) = sharing_answers(true);
    setup(&mut app, &state, |s, _| {
        s.activation = Some(an_activation());
        s.phase = SetupPhase::Waiting;
        s.outcome.post(SetupOutcome::Ready(ready()));
    });
    update(&mut app, &state); // drains "ready": saves the identity, asks for sharing
    update(&mut app, &state); // drains the "sharing" answer

    assert!(app
        .main_lines()
        .iter()
        .any(|line| line.contains("Connected to orinks.net as Rig Hauler.")));
    assert_eq!(activation_of(&state), None);
    let saved = load_identity().expect("the identity was saved");
    assert_eq!(saved.driver_id, "rig-hauler");
    assert_eq!(saved.driver_token, ready().token.unwrap());
    assert_eq!(app.ctx.services.cloud.identity(), Some(saved));
    assert!(!std::rc::Rc::ptr_eq(&app.state().unwrap(), &state)); // ("pop",)
}

/// The whole point of connecting: a new account must not land on a public
/// profile that reads "no career statistics yet". Those statistics are
/// derived from the cloud backup, so both have to come on together.
#[test]
fn test_connecting_turns_on_sharing_and_cloud_backup() {
    let (mut app, state) = setup_app(false);
    let _identity = install_identity(&app, None);
    install_cloud(&mut app, FakeTransport::revisions(), false);
    let (_sharing, transport) = sharing_answers(true);
    setup(&mut app, &state, |s, _| {
        s.phase = SetupPhase::Waiting;
        s.outcome.post(SetupOutcome::Ready(ready()));
    });
    update(&mut app, &state);
    // Cloud backup needs no handshake, so it is on the moment the account is;
    // sharing waits for the server, which is why it is still off right here.
    assert!(app.ctx.settings.cloud_saves);
    assert!(!app.ctx.settings.online_presence);
    assert_eq!(phase(&state), SetupPhase::Sharing);

    update(&mut app, &state);

    let asked: Vec<Value> = transport
        .posts()
        .iter()
        .map(|p| p["enabled"].clone())
        .collect();
    assert_eq!(asked, vec![Value::Bool(true)]);
    assert!(app.ctx.settings.online_presence);
    assert_eq!(
        app.ctx.settings.profile_sharing_consent_version,
        PROFILE_SHARING_CONSENT_VERSION
    );
    assert!(!app.ctx.settings.profile_sharing_pending_off);
    assert!(!std::rc::Rc::ptr_eq(&app.state().unwrap(), &state)); // ("pop",)
}

/// orinks.net refusing the sharing switch is not a failed setup: the
/// account is connected and backing up either way. Sending the player back
/// for a fresh activation code would throw away work the code already did.
#[test]
fn test_a_refused_sharing_call_keeps_the_connection_and_names_the_retry() {
    let (mut app, state) = setup_app(false);
    let _identity = install_identity(&app, None);
    install_cloud(&mut app, FakeTransport::revisions(), false);
    let (_sharing, _) = sharing_answers(false);
    setup(&mut app, &state, |s, _| {
        s.phase = SetupPhase::Waiting;
        s.outcome.post(SetupOutcome::Ready(ready()));
    });
    update(&mut app, &state);
    // The refusal is the last thing this state says, and popping reveals the
    // screen underneath, which announces itself straight after -- so clear
    // first and read the refusal off the front instead.
    app.clear_speech();
    update(&mut app, &state);

    assert!(!app.ctx.settings.online_presence);
    assert_eq!(app.ctx.settings.profile_sharing_consent_version, 0);
    assert!(app.ctx.settings.cloud_saves);
    let said = app.main_lines();
    let refusal = said.first().cloned().unwrap();
    assert!(refusal.contains("Profile sharing"));
    assert!(refusal.contains("Online"));
    assert!(!refusal.contains("activation code"));
    assert!(!std::rc::Rc::ptr_eq(&app.state().unwrap(), &state)); // ("pop",)
}

/// By this point the token is stored and the account is connected. Leaving
/// would strand the game believing sharing is off while orinks.net may have
/// already turned it on, and "Nothing was saved" would be a plain lie.
#[test]
fn test_backing_out_mid_sharing_is_refused_and_never_says_nothing_was_saved() {
    let (mut app, state) = setup_app(false);
    let _identity = install_identity(&app, None);
    install_cloud(&mut app, FakeTransport::revisions(), false);
    // The server has not answered yet: the sharing request is in flight.
    let (gate, transport) = Gate::new();
    let _sharing = install_transport(transport);
    setup(&mut app, &state, |s, _| {
        s.threaded = true;
        s.phase = SetupPhase::Waiting;
        s.outcome.post(SetupOutcome::Ready(ready()));
    });
    update(&mut app, &state);
    assert_eq!(phase(&state), SetupPhase::Sharing);
    app.clear_speech();

    setup(&mut app, &state, Menu::go_back);

    assert!(std::rc::Rc::ptr_eq(&app.state().unwrap(), &state)); // popped == []
    let said = app.main_lines();
    assert!(!said.iter().any(|line| line.contains("Nothing was saved")));
    assert!(said.iter().any(|line| line.contains("Stay here")));
    gate.release();
    if let Some(worker) = with_state::<OnlineSetupState, _>(&state, |s| s.worker.take()) {
        let _ = worker.join();
    }
}

/// A second press must not throw away a finished setup for a fresh code.
#[test]
fn test_choosing_setup_again_mid_sharing_does_not_start_a_new_activation() {
    let (mut app, state) = setup_app(false);
    let transport = FakeTransport::replying(activation_reply());
    let _transport = install_transport(transport.clone());
    setup(&mut app, &state, |s, ctx| {
        s.phase = SetupPhase::Sharing;
        s.start_setup(ctx);
    });
    assert_eq!(transport.request_count(), 0); // started == []
    assert_eq!(phase(&state), SetupPhase::Sharing);
    assert!(app
        .main_lines()
        .iter()
        .any(|line| line.contains("Profile sharing")));
}

#[test]
fn test_token_save_failure_reuses_the_keyring_failure_wording() {
    let (mut app, state) = setup_app(false);
    // A store whose "directory" is a plain file: the identity file can never
    // be written, which is the OSError `identity.save()` raised.
    let blocker = app.data_dir.path().join("not-a-dir");
    std::fs::write(&blocker, b"x").unwrap();
    set_identity_store_override(Some(Arc::new(IdentityStore::new(
        &blocker,
        Some(MemoryStore::new()),
    ))));
    let _guard = IdentityGuardLocal;
    setup(&mut app, &state, |s, _| {
        s.activation = Some(an_activation());
        s.phase = SetupPhase::Waiting;
        s.outcome.post(SetupOutcome::Ready(ready()));
    });
    update(&mut app, &state);

    assert!(app
        .main_lines()
        .iter()
        .any(|line| line.contains("could not save the driver token securely")));
    assert!(std::rc::Rc::ptr_eq(&app.state().unwrap(), &state)); // no ("pop",)
}

struct IdentityGuardLocal;

impl Drop for IdentityGuardLocal {
    fn drop(&mut self) {
        set_identity_store_override(None);
    }
}

#[test]
fn test_expiry_speaks_the_recovery() {
    let (mut app, state) = setup_app(false);
    setup(&mut app, &state, |s, _| {
        s.activation = Some(an_activation());
        s.phase = SetupPhase::Waiting;
        s.outcome.post(SetupOutcome::Expired);
    });
    update(&mut app, &state);
    let last = app.main_lines().last().cloned().unwrap();
    assert!(last.to_lowercase().contains("expired"));
    assert!(last.contains("Set up this computer with orinks.net"));
    assert_eq!(activation_of(&state), None);
    assert_eq!(phase(&state), SetupPhase::Expired);
}

#[test]
fn test_corrupt_code_error_does_not_suggest_waiting() {
    let (mut app, state) = setup_app(false);
    setup(&mut app, &state, |s, _| {
        s.activation = Some(an_activation());
        s.phase = SetupPhase::Waiting;
        s.outcome.post(SetupOutcome::Error);
    });
    update(&mut app, &state);
    let last = app.main_lines().last().cloned().unwrap();
    assert!(!last.to_lowercase().contains("wait"));
    // ...and the two halves have to agree: telling the player retrying will
    // not help, then naming a menu item to choose again, is a contradiction
    // heard aloud with no way to scroll back and re-read it.
    assert!(!last.to_lowercase().contains("not fix"));
    assert!(last.contains("Set up this computer with orinks.net"));
    assert_eq!(activation_of(&state), None);
    assert_eq!(phase(&state), SetupPhase::Error);
}

// -- leaving the menu ---------------------------------------------------------

#[test]
fn test_leaving_the_menu_stops_the_worker() {
    let (mut app, state) = setup_app(false);
    let _browser = install_browser(true);
    // A real background thread and real time: the activation arrives, then
    // every poll answers pending on a shrunk schedule.
    let transport = ClosureTransport(
        |url: &str, _p: Option<&Value>, _h: &[(String, String)], _m: Option<&str>| {
            if url.ends_with("/activate/start") {
                Ok(activation_reply())
            } else {
                Ok(json!({"status": "pending"}))
            }
        },
    );
    let _transport = install_transport(Arc::new(transport));
    setup(&mut app, &state, |s, ctx| {
        s.threaded = true;
        s.schedule = PollSchedule {
            interval_first_s: 0.01,
            interval_later_s: 0.01,
            first_phase_s: 0.01,
            ..PollSchedule::default()
        };
        s.start_setup(ctx);
    });
    std::thread::sleep(Duration::from_millis(50)); // let the worker actually reach the poll loop
    let worker =
        with_state::<OnlineSetupState, _>(&state, |s| s.worker.take()).expect("one worker");
    assert!(!worker.is_finished());

    setup(&mut app, &state, State::exit); // what pop_state() calls when the player backs out

    let deadline = Instant::now() + Duration::from_secs(2);
    while !worker.is_finished() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(worker.is_finished());
    let _ = worker.join();
}

#[test]
fn test_cancel_while_waiting_says_canceled_and_still_goes_back() {
    let (mut app, state) = setup_app(false);
    setup(&mut app, &state, |s, ctx| {
        s.activation = Some(an_activation());
        s.phase = SetupPhase::Waiting;
        Menu::go_back(s, ctx);
    });
    assert!(app
        .main_lines()
        .iter()
        .any(|line| line.to_lowercase().contains("canceled")));
    assert!(!std::rc::Rc::ptr_eq(&app.state().unwrap(), &state)); // popped == ["pop"]
}

/// A player who backs out before the activation code even arrives (still
/// contacting orinks.net) gets the same confirmation as one who backs out
/// mid-poll -- not just the generic menu-back sound and no word on it.
#[test]
fn test_cancel_while_starting_also_says_canceled() {
    let (mut app, state) = setup_app(false);
    setup(&mut app, &state, |s, ctx| {
        s.phase = SetupPhase::Starting;
        Menu::go_back(s, ctx);
    });
    assert!(app
        .main_lines()
        .iter()
        .any(|line| line.to_lowercase().contains("canceled")));
    assert!(!std::rc::Rc::ptr_eq(&app.state().unwrap(), &state));
}

/// No request is in flight yet, so there is nothing to confirm canceling
/// -- only "starting" and "waiting" get the extra line.
#[test]
fn test_cancel_while_idle_stays_silent_about_canceling() {
    let (mut app, state) = setup_app(false);
    assert_eq!(phase(&state), SetupPhase::Idle);
    setup(&mut app, &state, Menu::go_back);
    assert!(!app
        .main_lines()
        .iter()
        .any(|line| line.to_lowercase().contains("canceled")));
}

// -- only one worker at a time -------------------------------------------------

/// The guard in start_setup, not real threading, is what is under test
/// here: a second press while a request is already under way must not spin
/// up a second background worker.
#[test]
fn test_choosing_setup_twice_starts_only_one_worker_thread() {
    let (mut app, state) = setup_app(false);
    let (gate, transport) = Gate::new();
    let _transport = install_transport(transport);
    setup(&mut app, &state, |s, ctx| {
        s.threaded = true;
        s.start_setup(ctx); // phase -> "starting"; worker #1 in flight
        s.start_setup(ctx); // still "starting": guard must skip a new worker
    });
    let deadline = Instant::now() + Duration::from_secs(2);
    while gate.calls() < 1 && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(gate.calls(), 1);

    // Simulate the activation having already arrived (phase "waiting").
    setup(&mut app, &state, |s, ctx| {
        s.phase = SetupPhase::Waiting;
        s.activation = Some(an_activation());
        s.start_setup(ctx); // a status repeat, not a fresh request
    });
    assert_eq!(gate.calls(), 1);
    setup(&mut app, &state, State::exit);
    gate.release();
    if let Some(worker) = with_state::<OnlineSetupState, _>(&state, |s| s.worker.take()) {
        let _ = worker.join();
    }
}

/// Phase "starting" with no activation yet: there is no code to repeat,
/// but returning in silence reads as "did that keypress register" in a game
/// with no visual fallback to check against.
#[test]
fn test_pressing_setup_again_before_the_code_arrives_is_never_silent() {
    let (mut app, state) = setup_app(false);
    let (gate, transport) = Gate::new();
    let _transport = install_transport(transport);
    setup(&mut app, &state, |s, ctx| {
        s.threaded = true;
        s.start_setup(ctx); // phase -> "starting", no activation yet
    });
    app.clear_speech();
    setup(&mut app, &state, |s, ctx| s.start_setup(ctx)); // the second press

    assert_eq!(activation_of(&state), None);
    assert_eq!(
        app.main_lines(),
        vec!["Still contacting orinks.net for an activation code.".to_string()]
    );
    setup(&mut app, &state, State::exit);
    gate.release();
    if let Some(worker) = with_state::<OnlineSetupState, _>(&state, |s| s.worker.take()) {
        let _ = worker.join();
    }
}

// -- autostart from the offer's accept path ------------------------------------

/// A player who just said yes must not be asked to confirm again.
#[test]
fn test_autostart_begins_setup_on_entry() {
    let mut app = TestApp::new();
    let (gate, transport) = Gate::new();
    let _transport = install_transport(transport);
    let state = OnlineSetupState::with_autostart(&mut app.ctx, true);
    let shared = push(&mut app, state);
    assert_eq!(phase(&shared), SetupPhase::Starting); // started == [True]
    setup(&mut app, &shared, State::exit);
    gate.release();
    if let Some(worker) = with_state::<OnlineSetupState, _>(&shared, |s| s.worker.take()) {
        let _ = worker.join();
    }
}

/// Reaching setup from the Online menu must still wait for the player.
#[test]
fn test_without_autostart_entry_starts_nothing() {
    let mut app = TestApp::new();
    let (gate, transport) = Gate::new();
    let _transport = install_transport(transport);
    let state = OnlineSetupState::new(&mut app.ctx);
    let shared = push(&mut app, state);
    assert_eq!(phase(&shared), SetupPhase::Idle); // started == []
    assert_eq!(gate.calls(), 0);
}

/// A player who just said "Set up now" already knows what this state is
/// for. Announcing the five-item menu and then talking over it a moment
/// later with "Contacting orinks.net..." (from the real start_setup) would
/// read as the game losing its place mid-sentence -- so autostart must go
/// straight to that line instead of announcing the menu first.
#[test]
fn test_autostart_skips_the_menu_intro_and_speaks_setup_starting() {
    let mut app = TestApp::new();
    let (gate, transport) = Gate::new(); // the network request itself is not what this test checks
    let _transport = install_transport(transport);
    app.clear_speech();
    let state = OnlineSetupState::with_autostart(&mut app.ctx, true);
    let shared = push(&mut app, state);

    let said = app.main_lines();
    assert_eq!(
        said,
        vec!["Contacting orinks.net for an activation code.".to_string()]
    );
    assert!(!said
        .iter()
        .any(|line| line.contains("orinks.net account setup")));
    setup(&mut app, &shared, State::exit);
    gate.release();
    if let Some(worker) = with_state::<OnlineSetupState, _>(&shared, |s| s.worker.take()) {
        let _ = worker.join();
    }
}

/// The Online-menu path is unchanged: a player choosing setup from a
/// menu has not already committed, so the five-item menu is announced as
/// before.
#[test]
fn test_without_autostart_entry_still_speaks_the_menu_intro() {
    let (app, _state) = setup_app(false);
    let said = app.main_lines();
    assert!(said
        .iter()
        .any(|line| line.contains("orinks.net account setup")));
    assert!(!said
        .iter()
        .any(|line| line == "Contacting orinks.net for an activation code."));
}

/// The reconnect advice has to survive a rename of the thing it names.
///
/// `cloud_saves::AUTH_HELP` is spoken when orinks.net stops accepting this
/// computer's sign-in, and its whole job is to walk the player to the
/// control that fixes it. It used to send them to an Add computer button
/// and a paste field, both of which went away with the clipboard setup
/// (armstrong445, 2026-08-15: a capped account was told to use a flow that
/// no longer exists). Reading the label off the live menu means the next
/// rename fails here instead of in a player's ears.
#[test]
fn test_auth_help_names_the_item_this_menu_really_offers() {
    let mut app = TestApp::new();
    let state = OnlineSetupState::new(&mut app.ctx);
    let idle_label = state.setup_label();
    assert!(AUTH_HELP.contains(&idle_label));
    assert!(!AUTH_HELP.contains("Add computer"));
    assert!(!AUTH_HELP.to_lowercase().contains("paste"));
}
