//! Guest drivers: the orinks.net calls (`online_guest`), the activation
//! request a guest sends when it connects an account, and the menus that make
//! a guest, rename it, and keep cloud backup for accounts.

use std::sync::{Arc, Mutex};

use crate::states_online_support::*;
use freight_fate::app::testing::TestApp;
use freight_fate::app::SharedState;
use freight_fate::net::testing::{ClosureTransport, FakeTransport};
use freight_fate::net::{NetError, SharedTransport};
use freight_fate::online_activation::{start_activation, start_activation_as};
use freight_fate::online_guest::{
    self, create_guest, guest_for, load_guest, rename_guest, save_guest, GuestCreate, GuestRecord,
    GuestRename,
};
use freight_fate::online_presence::OnlineIdentity;
use freight_fate::states::base::{Key, State};
use freight_fate::states::online_hub::OnlineHubState;
use freight_fate::states::online_states::{
    current_guest, load_identity, GuestNameState, GuestSetupState, OnlineSetupState, SetupOutcome,
    SetupPhase,
};
use serde_json::{json, Value};

type Seen = Arc<Mutex<Vec<(String, Option<Value>, Vec<(String, String)>)>>>;

/// A transport that records each call and answers by URL ending.
fn routed(routes: Vec<(&'static str, Result<Value, u16>)>) -> (SharedTransport, Seen) {
    let seen: Seen = Arc::new(Mutex::new(Vec::new()));
    let log = Arc::clone(&seen);
    let transport: SharedTransport = Arc::new(ClosureTransport(
        move |url: &str,
              payload: Option<&Value>,
              headers: &[(String, String)],
              _m: Option<&str>| {
            log.lock()
                .unwrap()
                .push((url.to_string(), payload.cloned(), headers.to_vec()));
            for (ending, answer) in &routes {
                if url.ends_with(ending) {
                    return match answer {
                        Ok(body) => Ok(body.clone()),
                        Err(code) => Err(NetError::http(*code)),
                    };
                }
            }
            Err(NetError::http(404))
        },
    ));
    (transport, seen)
}

fn auth(headers: &[(String, String)]) -> Option<String> {
    headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("authorization"))
        .map(|(_, value)| value.clone())
}

fn guest_reply() -> Value {
    json!({
        "status": "ready",
        "driver_id": "guest-0123456789abcdef",
        "token": format!("ffd_{}", "c".repeat(64)),
        "display_name": "Copper Fox 12",
        "guest": true,
    })
}

fn guest_identity() -> OnlineIdentity {
    OnlineIdentity::new("guest-0123456789abcdef", &format!("ffd_{}", "c".repeat(64)))
}

fn temp_dir() -> tempfile::TempDir {
    tempfile::tempdir().expect("a temp dir")
}

// -- the calls ----------------------------------------------------------------------

#[test]
fn create_guest_reads_the_identity_and_name_and_names_this_computer() {
    let dir = temp_dir();
    let (transport, seen) = routed(vec![("/api/freight-fate/guest", Ok(guest_reply()))]);
    let made = create_guest(transport.as_ref(), dir.path());
    assert_eq!(
        made,
        GuestCreate::Ready {
            identity: guest_identity(),
            display_name: "Copper Fox 12".to_string(),
        }
    );
    let calls = seen.lock().unwrap();
    assert_eq!(calls.len(), 1);
    let key = calls[0].1.as_ref().unwrap()["machine_key"]
        .as_str()
        .unwrap();
    assert_eq!(key.len(), 32);
}

#[test]
fn create_guest_tells_busy_from_broken() {
    let dir = temp_dir();
    let (busy, _) = routed(vec![("/guest", Err(429))]);
    assert_eq!(
        create_guest(busy.as_ref(), dir.path()),
        GuestCreate::RateLimited
    );
    let (down, _) = routed(vec![("/guest", Err(503))]);
    assert_eq!(create_guest(down.as_ref(), dir.path()), GuestCreate::Failed);
    let (odd, _) = routed(vec![("/guest", Ok(json!({"status": "ready"})))]);
    assert_eq!(create_guest(odd.as_ref(), dir.path()), GuestCreate::Failed);
}

#[test]
fn rename_guest_signs_in_and_reads_the_new_name() {
    let (transport, seen) = routed(vec![(
        "/guest/rename",
        Ok(json!({"ok": true, "display_name": "Quiet Heron 7"})),
    )]);
    let renamed = rename_guest(&guest_identity(), transport.as_ref());
    assert_eq!(renamed, GuestRename::Renamed("Quiet Heron 7".to_string()));
    let calls = seen.lock().unwrap();
    assert_eq!(
        calls[0].1.as_ref().unwrap()["driverId"],
        "guest-0123456789abcdef"
    );
    assert_eq!(
        auth(&calls[0].2),
        Some(format!("Bearer ffd_{}", "c".repeat(64)))
    );
}

#[test]
fn rename_guest_answers_for_each_refusal() {
    for (code, expected) in [
        (422, GuestRename::NotAGuest),
        (401, GuestRename::NotAGuest),
        (429, GuestRename::RateLimited),
        (503, GuestRename::Failed),
    ] {
        let (transport, _) = routed(vec![("/guest/rename", Err(code))]);
        assert_eq!(
            rename_guest(&guest_identity(), transport.as_ref()),
            expected,
            "{code}"
        );
    }
}

fn start_reply() -> Value {
    json!({
        "device_code": "a".repeat(64),
        "user_code": "WKQR-3468",
        "verification_uri": "https://orinks.net/activate",
        "verification_uri_complete": "https://orinks.net/activate?code=WKQR-3468",
        "expires_in": 600,
        "interval": 3,
    })
}

#[test]
fn a_guest_connecting_an_account_proves_itself() {
    let dir = temp_dir();
    let (transport, seen) = routed(vec![("/activate/start", Ok(start_reply()))]);
    assert!(start_activation_as(transport.as_ref(), dir.path(), Some(&guest_identity())).is_some());
    assert!(start_activation(transport.as_ref(), dir.path()).is_some());
    let calls = seen.lock().unwrap();
    let (_, guest_body, guest_headers) = &calls[0];
    assert_eq!(
        guest_body.as_ref().unwrap()["guest_driver_id"],
        "guest-0123456789abcdef"
    );
    assert!(auth(guest_headers).is_some());
    // Anyone else's request carries neither.
    let (_, plain_body, plain_headers) = &calls[1];
    assert!(plain_body
        .as_ref()
        .unwrap()
        .get("guest_driver_id")
        .is_none());
    assert!(auth(plain_headers).is_none());
}

#[test]
fn the_guest_record_counts_only_for_its_own_identity() {
    let dir = temp_dir();
    assert_eq!(load_guest(dir.path()), None);
    let record = GuestRecord {
        driver_id: "guest-0123456789abcdef".to_string(),
        display_name: "Copper Fox 12".to_string(),
    };
    save_guest(dir.path(), &record).unwrap();
    assert_eq!(guest_for(Some(&guest_identity()), dir.path()), Some(record));
    let account = OnlineIdentity::new("road-star-1a2b3c4d", "ffd_x");
    assert_eq!(guest_for(Some(&account), dir.path()), None);
    assert_eq!(guest_for(None, dir.path()), None);
    online_guest::clear_guest(dir.path());
    assert_eq!(load_guest(dir.path()), None);
}

// -- the menus ------------------------------------------------------------------------

fn update<T: State + 'static>(app: &mut TestApp, shared: &SharedState) {
    with_state::<T, _>(shared, |s| State::update(s, &mut app.ctx, 0.0));
    app.ctx.run_deferred();
}

fn make_guest(app: &mut TestApp) -> (TransportGuard, Seen) {
    let (transport, seen) = routed(vec![
        ("/api/freight-fate/guest", Ok(guest_reply())),
        ("/profile-sharing", Ok(json!({"ok": true, "enabled": true}))),
    ]);
    let guard = install_transport(transport);
    app.push_state(base_state());
    let mut state = GuestSetupState::new(&mut app.ctx);
    state.threaded = false;
    let shared = push(app, state);
    with_state::<GuestSetupState, _>(&shared, |s| s.start(&mut app.ctx));
    update::<GuestSetupState>(app, &shared); // the guest: saved, sharing asked
    update::<GuestSetupState>(app, &shared); // sharing confirmed
    (guard, seen)
}

#[test]
fn going_online_as_a_guest_saves_it_and_turns_sharing_on() {
    let mut app = TestApp::new();
    let _identity = install_identity(&app, None);
    install_cloud(&mut app, FakeTransport::revisions(), false);
    app.ctx.settings.cloud_saves = true;
    let (_transport, seen) = make_guest(&mut app);

    assert_eq!(load_identity(), Some(guest_identity()));
    assert_eq!(
        current_guest().map(|g| g.display_name),
        Some("Copper Fox 12".to_string())
    );
    assert!(app.ctx.settings.online_presence);
    assert!(!app.ctx.settings.cloud_saves, "guests get no cloud backup");
    // No profile, so the journal and achievements are never queued.
    assert!(!app.ctx.services.journal.enabled());
    let spoken = said(&app);
    assert!(
        spoken.contains("You're Copper Fox 12 on the drivers board."),
        "{spoken}"
    );
    let urls: Vec<String> = seen.lock().unwrap().iter().map(|c| c.0.clone()).collect();
    assert!(urls[0].ends_with("/api/freight-fate/guest"));
    assert!(urls[1].ends_with("/api/freight-fate/profile-sharing"));
}

#[test]
fn a_busy_site_leaves_nothing_saved() {
    let mut app = TestApp::new();
    let _identity = install_identity(&app, None);
    let (transport, _) = routed(vec![("/api/freight-fate/guest", Err(429))]);
    let _guard = install_transport(transport);
    app.push_state(base_state());
    let mut state = GuestSetupState::new(&mut app.ctx);
    state.threaded = false;
    let shared = push(&mut app, state);
    with_state::<GuestSetupState, _>(&shared, |s| s.start(&mut app.ctx));
    update::<GuestSetupState>(&mut app, &shared);
    assert_eq!(load_identity(), None);
    assert_eq!(current_guest(), None);
    assert!(said(&app).contains("orinks.net is busy. Try again in a minute."));
}

#[test]
fn the_hub_names_the_guest_and_keeps_backup_for_accounts() {
    let mut app = TestApp::new();
    let _identity = install_identity(&app, None);
    install_cloud(&mut app, FakeTransport::revisions(), false);
    let hub_state = OnlineHubState::new(&mut app.ctx);
    let hub = push(&mut app, hub_state);
    let rows = labels::<OnlineHubState>(&hub, &app.ctx);
    assert!(rows.contains(&"Set up orinks.net account".to_string()));
    assert!(rows.contains(&"Drive online as a guest".to_string()));

    let (_transport, _) = make_guest(&mut app);
    let hub_state = OnlineHubState::new(&mut app.ctx);
    let hub = push(&mut app, hub_state);
    let rows = labels::<OnlineHubState>(&hub, &app.ctx);
    assert!(
        rows.contains(&"Connect an orinks.net account".to_string()),
        "{rows:?}"
    );
    assert!(
        rows.contains(&"Guest name: Copper Fox 12".to_string()),
        "{rows:?}"
    );

    app.clear_speech();
    move_to::<OnlineHubState>(&mut app, &hub, "Back up saves");
    press(&mut app, Key::Return);
    assert!(!app.ctx.settings.cloud_saves);
    assert!(said(&app).contains("Cloud backup needs an orinks.net account."));
}

#[test]
fn an_account_driver_has_no_guest() {
    let mut app = TestApp::new();
    let _identity = install_identity(&app, Some(&identity()));
    let hub_state = OnlineHubState::new(&mut app.ctx);
    let hub = push(&mut app, hub_state);
    let rows = labels::<OnlineHubState>(&hub, &app.ctx);
    assert!(rows.contains(&"orinks.net account: connected".to_string()));
    assert!(rows.contains(&"Guest driver: off, account connected".to_string()));
}

#[test]
fn asking_for_a_new_name_saves_it_here() {
    let mut app = TestApp::new();
    let _identity = install_identity(&app, None);
    install_cloud(&mut app, FakeTransport::revisions(), false);
    let (_first, _) = make_guest(&mut app);
    let (transport, _) = routed(vec![(
        "/guest/rename",
        Ok(json!({"ok": true, "display_name": "Quiet Heron 7"})),
    )]);
    let _guard = install_transport(transport);
    let mut names = GuestNameState::new(&mut app.ctx);
    names.threaded = false;
    let shared = push(&mut app, names);
    assert!(said(&app).contains("Guest name. You're Copper Fox 12."));
    with_state::<GuestNameState, _>(&shared, |s| s.ask(&mut app.ctx));
    update::<GuestNameState>(&mut app, &shared);
    assert_eq!(
        current_guest().map(|g| g.display_name),
        Some("Quiet Heron 7".to_string())
    );
    assert!(said(&app).contains("You're Quiet Heron 7 now."));
}

/// Connect an account from a guest computer; orinks.net answers with
/// `driver_id`.
fn connect_as(app: &mut TestApp, driver_id: &str, display_name: &str) {
    let _sharing = install_transport(FakeTransport::replying(
        json!({"ok": true, "enabled": true}),
    ));
    let mut setup = OnlineSetupState::new(&mut app.ctx);
    setup.threaded = false;
    let shared = push(app, setup);
    let result = freight_fate::online_activation::PollResult {
        status: "ready".to_string(),
        driver_id: Some(driver_id.to_string()),
        token: Some(format!("ffd_{}", "d".repeat(64))),
        display_name: Some(display_name.to_string()),
    };
    with_state::<OnlineSetupState, _>(&shared, |s| {
        s.phase = SetupPhase::Waiting;
        s.outcome.post(SetupOutcome::Ready(result));
    });
    update::<OnlineSetupState>(app, &shared);
}

#[test]
fn connecting_an_account_with_a_driver_ends_the_guest() {
    let mut app = TestApp::new();
    let _identity = install_identity(&app, None);
    install_cloud(&mut app, FakeTransport::revisions(), false);
    let (_first, _) = make_guest(&mut app);
    connect_as(&mut app, "road-star-1a2b3c4d", "Road Star");
    assert_eq!(current_guest(), None);
    assert_eq!(load_guest(&ff_core::models::profile::data_dir()), None);
    let spoken = said(&app);
    assert!(
        spoken.contains("Connected to orinks.net as Road Star. Your careers now back up"),
        "{spoken}"
    );
    assert!(app.ctx.settings.cloud_saves);
}

#[test]
fn an_account_with_no_driver_takes_the_guest_over() {
    let mut app = TestApp::new();
    let _identity = install_identity(&app, None);
    install_cloud(&mut app, FakeTransport::revisions(), false);
    let (_first, _) = make_guest(&mut app);
    connect_as(&mut app, "guest-0123456789abcdef", "Copper Fox 12");
    assert_eq!(current_guest(), None);
    assert!(said(&app).contains(
        "Connected to orinks.net as Copper Fox 12. Your guest driver is this account now."
    ));
}

#[test]
fn a_guest_has_no_profile_of_its_own() {
    let mut app = TestApp::new();
    let _identity = install_identity(&app, None);
    install_cloud(&mut app, FakeTransport::revisions(), false);
    let (_transport, _) = make_guest(&mut app);
    let hub_state = OnlineHubState::new(&mut app.ctx);
    let hub = push(&mut app, hub_state);
    app.clear_speech();
    move_to::<OnlineHubState>(&mut app, &hub, "Your profile");
    press(&mut app, Key::Return);
    assert!(said(&app).contains(
        "Your profile needs an orinks.net account. Choose Connect an orinks.net account first."
    ));
}

#[test]
fn the_board_names_a_guest_and_opens_no_profile_for_it() {
    let mut app = TestApp::new();
    let _guard = install_transport(FakeTransport::replying(json!({"drivers": [
        {"driverId": "guest-0123456789abcdef", "displayName": "Copper Fox 12",
         "activity": "Driving", "detail": "I-80", "updatedAt": 0, "guest": true},
    ]})));
    let mut board = freight_fate::states::online_states::DriversOnlineState::new(&mut app.ctx);
    board.threaded = false;
    let shared = push(&mut app, board);
    update::<freight_fate::states::online_states::DriversOnlineState>(&mut app, &shared);
    let rows = labels::<freight_fate::states::online_states::DriversOnlineState>(&shared, &app.ctx);
    assert!(
        rows.iter()
            .any(|row| row.starts_with("Copper Fox 12, guest. Driving. I-80.")),
        "{rows:?}"
    );
    let depth = app.ctx.stack_len();
    move_to::<freight_fate::states::online_states::DriversOnlineState>(
        &mut app,
        &shared,
        "Copper Fox 12, guest",
    );
    press(&mut app, Key::Return);
    assert_eq!(app.ctx.stack_len(), depth, "no profile opens for a guest");
}
