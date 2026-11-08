//! Port of `tests/test_cloud_public_career.py`: choosing which career fronts
//! the public profile, from the Cloud backup menu.
//!
//! One career is the player's public face; the rest stay private cloud
//! backups. The list says which career is public, each backed-up career
//! offers to become it behind a spoken confirmation, and the choice goes to
//! orinks.net.

use crate::states_online_support::*;
use freight_fate::app::testing::TestApp;
use freight_fate::app::{share, SharedState};
use freight_fate::net::testing::FakeTransport;
use freight_fate::net::NetError;
use freight_fate::states::base::{Key, State};
use freight_fate::states::cloud_save_states::{
    CloudBackupState, CloudSlotState, ConfirmDeleteCloudState, ConfirmKeepMineState,
    ConfirmPublicCareerState, ConfirmRestoreState, SlotHandle,
};
use freight_fate::states::online_states::wall_time;
use serde_json::{json, Map, Value};

fn cloud_reply(public: Option<&str>) -> Value {
    json!({
        "saves": [
            {"saveName": "Road Star", "revision": 2, "createdAt": wall_time() * 1000.0},
            {"saveName": "Night Runs", "revision": 1, "createdAt": wall_time() * 1000.0},
        ],
        "publicSaveName": public,
    })
}

/// The backup menu over a cloud service answering `cloud_reply(public)`,
/// fetched and announced.
fn open_backup_menu(app: &mut TestApp, public: Option<&str>) -> (SharedState, IdentityGuard) {
    let guard = install_identity(app, Some(&identity()));
    install_cloud(app, FakeTransport::replying(cloud_reply(public)), true);
    let mut state = CloudBackupState::new(&mut app.ctx);
    state.threaded = false;
    let shared = push(app, state);
    assert!(with_state::<CloudBackupState, _>(&shared, |s| s.fetched()));
    with_state::<CloudBackupState, _>(&shared, |s| s.update(&mut app.ctx, 0.0));
    (shared, guard)
}

/// Pump update until the slot's worker hands back its outcome (inline, so
/// one update is enough).
fn settle(app: &mut TestApp, slot: &SharedState) {
    with_state::<CloudSlotState, _>(slot, |s| s.update(&mut app.ctx, 0.0));
    app.ctx.run_deferred();
}

#[test]
fn test_the_list_says_which_career_is_public() {
    let mut app = TestApp::new();
    let (state, _guard) = open_backup_menu(&mut app, Some("Night Runs"));
    let texts = labels::<CloudBackupState>(&state, &app.ctx);
    assert!(texts
        .iter()
        .any(|t| t.contains("Night Runs") && t.contains("your public career")));
    assert!(!texts
        .iter()
        .any(|t| t.contains("Road Star") && t.contains("your public career")));
}

#[test]
fn test_a_backed_up_career_can_become_the_public_one() {
    let mut app = TestApp::new();
    let (state, _guard) = open_backup_menu(&mut app, None);
    // The public-career choice goes to the cloud service's transport: swap
    // in one that says yes and records what was chosen.
    let chooser = FakeTransport::replying(json!({"ok": true}));
    install_cloud(&mut app, chooser.clone(), true);

    move_to::<CloudBackupState>(&mut app, &state, "Night Runs");
    press(&mut app, Key::Return);
    let slot = app.state().unwrap();
    assert!(is_state::<CloudSlotState>(&slot));
    with_state::<CloudSlotState, _>(&slot, |s| s.threaded = false);

    for _ in 0..16 {
        if current_label::<CloudSlotState>(&slot, &app.ctx).contains("public career") {
            break;
        }
        press(&mut app, Key::Down);
    }
    assert_eq!(
        current_label::<CloudSlotState>(&slot, &app.ctx),
        "Make this your public career"
    );
    let index = with_state::<CloudSlotState, _>(&slot, |s| s.menu.index);
    assert!(!helps::<CloudSlotState>(&slot, &app.ctx)[index].is_empty());
    press(&mut app, Key::Return);
    let confirm = app.state().unwrap();
    assert!(is_state::<ConfirmPublicCareerState>(&confirm));

    move_to::<ConfirmPublicCareerState>(&mut app, &confirm, "Yes");
    press(&mut app, Key::Return);
    settle(&mut app, &slot);

    let chosen: Vec<Value> = chooser
        .posts()
        .iter()
        .map(|p| p["saveName"].clone())
        .collect();
    assert_eq!(chosen, vec![Value::from("Night Runs")]);
    // The slot now says it is the public career instead of offering it.
    let texts = labels::<CloudSlotState>(&slot, &app.ctx);
    assert!(texts.contains(&"This is your public career".to_string()));
    assert!(!texts.contains(&"Make this your public career".to_string()));
    // And the backup list behind it agrees without another fetch.
    assert_eq!(
        with_state::<CloudBackupState, _>(&state, |s| s.public_save.clone()),
        Some("Night Runs".to_string())
    );
}

#[test]
fn test_the_public_career_shows_status_not_an_action() {
    let mut app = TestApp::new();
    let (state, _guard) = open_backup_menu(&mut app, Some("Road Star"));
    move_to::<CloudBackupState>(&mut app, &state, "Road Star");
    press(&mut app, Key::Return);
    let slot = app.state().unwrap();
    assert!(is_state::<CloudSlotState>(&slot));
    let texts = labels::<CloudSlotState>(&slot, &app.ctx);
    assert!(texts.contains(&"This is your public career".to_string()));
    assert!(!texts.contains(&"Make this your public career".to_string()));
}

fn a_revision() -> Value {
    json!({"revision": 4, "createdAt": wall_time() * 1000.0})
}

/// A slot screen for `name` over `transport`, with a real save for the
/// career on this computer.
fn slot_with_local_save(
    app: &mut TestApp,
    name: &str,
    transport: std::sync::Arc<FakeTransport>,
) -> (SharedState, IdentityGuard) {
    let guard = install_identity(app, Some(&identity()));
    install_cloud(app, transport, true);
    let mut profile = ff_core::models::profile::Profile::named(name);
    profile.set_money(3294.0);
    profile.save().unwrap();
    let state = CloudSlotState::new(&mut app.ctx, name, vec![a_revision()], None, None);
    (push(app, state), guard)
}

/// Brandon (armstrong445), 2026-08-15. A career could only travel upward
/// through the queue after a save, or through the conflict screen's "Keep
/// this computer's save" -- which vanished with the conflict that summoned
/// it, so once the queue stuck there was no way to send his newer career up
/// and he lost a level to the cloud's older copy. The row is always there
/// now, first under the status line, and it uses the queue's own upload.
#[test]
fn test_a_career_can_be_backed_up_by_hand_without_a_conflict() {
    let mut app = TestApp::new();
    let transport = FakeTransport::revisions();
    let (slot, _guard) = slot_with_local_save(&mut app, "armstrong45", transport.clone());
    let texts = labels::<CloudSlotState>(&slot, &app.ctx);
    assert!(texts[0].starts_with("Status:"));
    assert_eq!(texts[1], "Back up this career now");

    move_to::<CloudSlotState>(&mut app, &slot, "Back up this career now");
    press(&mut app, Key::Return);
    assert_eq!(transport.posts().len(), 1, "the upload went out at once");
    settle(&mut app, &slot);

    let spoken = said(&app);
    assert!(spoken.contains("Backing up this career."), "{spoken}");
    assert!(
        spoken.contains("Backed up. The cloud copy now matches this computer's save."),
        "{spoken}"
    );
    assert!(!with_state::<CloudSlotState, _>(&slot, |s| s.busy));
    let texts = labels::<CloudSlotState>(&slot, &app.ctx);
    assert!(texts[0].contains("Backed up"), "{}", texts[0]);
}

/// Pressing the row can never cost the career: a failed attempt says so,
/// says the cloud copy was not changed, and hands the screen back.
#[test]
fn test_a_backup_by_hand_that_does_not_go_through_says_so_and_changes_nothing() {
    let mut app = TestApp::new();
    let transport = FakeTransport::failing(NetError::Connection("refused".to_string()));
    let (slot, _guard) = slot_with_local_save(&mut app, "armstrong45", transport);
    move_to::<CloudSlotState>(&mut app, &slot, "Back up this career now");
    press(&mut app, Key::Return);
    settle(&mut app, &slot);
    let spoken = said(&app);
    assert!(spoken.contains("has not gone through yet"), "{spoken}");
    assert!(
        spoken.contains("the cloud copy was not changed"),
        "{spoken}"
    );
    assert!(!with_state::<CloudSlotState, _>(&slot, |s| s.busy));
}

/// Under a recorded conflict the upload is a choice between two copies, so
/// the conflict rows own it and the plain row steps aside; with no save on
/// this computer there is nothing to send and the row is not offered.
#[test]
fn test_the_backup_row_steps_aside_for_a_conflict_or_no_local_save() {
    let mut app = TestApp::new();
    let (slot, _guard) = slot_with_local_save(&mut app, "armstrong45", FakeTransport::revisions());
    record_conflict(
        &app,
        "armstrong45",
        Some("armstrong45, level 7, 9,100 dollars"),
    );
    // Rebuilt after the conflict landed: the rows on the stack were built
    // when the screen was pushed.
    let texts: Vec<String> =
        with_state::<CloudSlotState, _>(&slot, |s| built_rows(s, &mut app.ctx))
            .into_iter()
            .map(|(t, _)| t)
            .collect();
    assert!(
        !texts.iter().any(|t| t == "Back up this career now"),
        "{texts:?}"
    );
    assert!(texts
        .iter()
        .any(|t| t.starts_with("Keep this computer's save")));
    drop(slot);
    drop(_guard);
    drop(app);

    let mut app = TestApp::new();
    let _guard = install_identity(&app, Some(&identity()));
    install_cloud(&mut app, FakeTransport::revisions(), true);
    let mut state =
        CloudSlotState::new(&mut app.ctx, "nobody-here", vec![a_revision()], None, None);
    let texts: Vec<String> = built_rows(&mut state, &mut app.ctx)
        .into_iter()
        .map(|(t, _)| t)
        .collect();
    assert!(
        !texts.iter().any(|t| t == "Back up this career now"),
        "{texts:?}"
    );
}

fn record_conflict(app: &TestApp, name: &str, summary: Option<&str>) {
    let mut latest = Map::new();
    latest.insert("latestRevision".to_string(), Value::from(4));
    if let Some(summary) = summary {
        latest.insert("latestSummary".to_string(), Value::from(summary));
    }
    app.ctx
        .cloud_saves_service()
        .sync_state()
        .record_conflict(name, &latest);
}

/// Brandon (armstrong445), 2026-08-15. The screen named the cloud copy's
/// level and money and said nothing whatever about the save already on his
/// machine, so he was asked to choose between something described and
/// something anonymous. The safe-feeling answer to that question is to
/// choose neither, and that is what he did for a day while his career sat
/// unbacked. Both copies are now described the same way, from the same
/// `backup_summary` the server line is built with, so they can be compared
/// word for word.
#[test]
fn test_a_conflict_names_both_copies_so_the_choice_can_be_answered() {
    let mut app = TestApp::new();
    // This computer's copy: a real save, described by backup_summary.
    let mut profile = ff_core::models::profile::Profile::named("armstrong45");
    profile.set_money(3294.0);
    profile.save().unwrap();
    let expected_mine =
        freight_fate::cloud_saves::backup_summary(&Value::Object(profile.to_dict()));
    record_conflict(
        &app,
        "armstrong45",
        Some("armstrong45, level 7, 9,100 dollars"),
    );
    let mut state =
        CloudSlotState::new(&mut app.ctx, "armstrong45", vec![a_revision()], None, None);
    let labels: Vec<String> = built_rows(&mut state, &mut app.ctx)
        .into_iter()
        .map(|(t, _)| t)
        .collect();
    let headline = labels
        .iter()
        .find(|t| t.contains("needs attention"))
        .unwrap();

    // What he keeps and what he moves to, both audible before either
    // choice is read out.
    assert!(headline.contains(&format!("This computer's copy is {expected_mine}")));
    assert!(headline.contains("The cloud copy is armstrong45, level 7, 9,100 dollars"));

    let keep_mine = labels
        .iter()
        .find(|t| t.starts_with("Keep this computer's save"))
        .unwrap();
    let use_cloud = labels
        .iter()
        .find(|t| t.starts_with("Use the cloud copy"))
        .unwrap();
    // Each row names what it KEEPS: two rows differing only in "this
    // computer" and "the cloud" is not a choice a player can answer.
    assert!(keep_mine.contains("3,294 dollars"));
    assert!(use_cloud.contains("level 7, 9,100 dollars"));
}

/// The whole point of this screen is to unstick a career. A local save
/// that will not load must not be the thing that stops it being unstuck.
#[test]
fn test_an_unreadable_local_save_costs_a_sentence_not_the_resolution() {
    let mut app = TestApp::new();
    record_conflict(
        &app,
        "armstrong45",
        Some("armstrong45, level 7, 9,100 dollars"),
    );
    // No local save at all reads as "", the same as one that will not load.
    let mut state =
        CloudSlotState::new(&mut app.ctx, "armstrong45", vec![a_revision()], None, None);
    let labels: Vec<String> = built_rows(&mut state, &mut app.ctx)
        .into_iter()
        .map(|(t, _)| t)
        .collect();
    assert!(labels
        .iter()
        .any(|t| t.starts_with("Keep this computer's save")));
    assert!(labels.iter().any(|t| t.contains("The cloud copy is")));
}

/// The owner pressed "No, keep this computer's save" on his own career
/// expecting it to upload, and it backed out doing nothing (2026-08-15) --
/// because it was the restore confirmation's CANCEL, word for word the same
/// promise as the conflict screen's real action, "Keep this computer's save
/// and back it up". On the one screen where a career is already stuck, a
/// retreat dressed as the remedy costs the player the fix. Cancels say they
/// cancel.
#[test]
fn test_no_cancel_row_is_named_after_a_real_action() {
    let mut app = TestApp::new();
    let slot = share(CloudSlotState::new(
        &mut app.ctx,
        "armstrong45",
        vec![a_revision()],
        None,
        None,
    ));
    let handle = SlotHandle::new(slot, "armstrong45");
    let mut restore =
        ConfirmRestoreState::new(&mut app.ctx, handle.clone(), json!({"revision": 4}));
    let mut keep = ConfirmKeepMineState::new(&mut app.ctx, handle.clone());
    let mut delete = ConfirmDeleteCloudState::new(&mut app.ctx, handle);
    let mut rows: Vec<(&str, Vec<String>)> = Vec::new();
    rows.push((
        "ConfirmRestoreState",
        built_rows(&mut restore, &mut app.ctx)
            .into_iter()
            .map(|(t, _)| t)
            .collect(),
    ));
    rows.push((
        "ConfirmKeepMineState",
        built_rows(&mut keep, &mut app.ctx)
            .into_iter()
            .map(|(t, _)| t)
            .collect(),
    ));
    rows.push((
        "ConfirmDeleteCloudState",
        built_rows(&mut delete, &mut app.ctx)
            .into_iter()
            .map(|(t, _)| t)
            .collect(),
    ));
    for (name, labels) in rows {
        let no_row = labels.iter().find(|t| t.starts_with("No")).unwrap();
        assert_eq!(
            no_row, "No, cancel and change nothing",
            "{name} offers {no_row:?}, which describes an outcome rather than a cancellation"
        );
        // And the yes still says what it does, so the pair is not two
        // indistinguishable rows.
        assert!(labels.iter().any(|t| t.starts_with("Yes,")));
    }
}

/// A player who lands on the restore confirmation while meaning to push
/// their own save up needs the way out named, not just the retreat.
#[test]
fn test_the_restore_cancel_points_back_at_the_upload_choice() {
    let mut app = TestApp::new();
    let slot = share(CloudSlotState::new(
        &mut app.ctx,
        "armstrong45",
        vec![a_revision()],
        None,
        None,
    ));
    let mut state = ConfirmRestoreState::new(
        &mut app.ctx,
        SlotHandle::new(slot, "armstrong45"),
        json!({"revision": 4}),
    );
    let (_, help) = built_rows(&mut state, &mut app.ctx)
        .into_iter()
        .find(|(t, _)| t.starts_with("No"))
        .unwrap();
    assert!(help.contains("Keep this computer's save and back it up"));
}

/// Restoring a career deleted from this computer replaces nothing, so the
/// result must not promise a fallback copy of a save that never existed.
#[test]
fn test_restoring_a_career_this_computer_no_longer_has_claims_no_fallback() {
    let mut app = TestApp::new();
    let _guard = install_identity(&app, Some(&identity()));
    install_cloud(&mut app, FakeTransport::revisions(), true);
    let mut state = CloudSlotState::new(&mut app.ctx, "Gone Local", vec![a_revision()], None, None);
    state.threaded = false;
    let slot = push(&mut app, state);
    app.clear_speech();
    with_state::<CloudSlotState, _>(&slot, |s| {
        s.busy = true;
        s.outcome.post("restored".to_string());
        s.update(&mut app.ctx, 0.0);
    });
    let said = app.main_lines().join(" ");
    assert!(
        said.contains("Gone Local is back on this computer"),
        "{said}"
    );
    assert!(!said.contains("fallback"), "{said}");
}

/// Both optional-summary paths must state the replacement as a full sentence.
#[test]
fn test_restore_confirmation_is_complete_with_or_without_summary() {
    for summary in [None, Some("level 4, 12 deliveries")] {
        let mut app = TestApp::new();
        let mut entry = a_revision();
        if let Some(summary) = summary {
            entry["summary"] = json!(summary);
        }
        let slot = share(CloudSlotState::new(
            &mut app.ctx,
            "Road Star",
            vec![entry.clone()],
            None,
            None,
        ));
        let state =
            ConfirmRestoreState::new(&mut app.ctx, SlotHandle::new(slot, "Road Star"), entry);
        app.clear_speech();
        push(&mut app, state);
        let said = app.main_lines().join(" ");
        assert!(
            said.contains("This will replace this computer's save for that career."),
            "{said}"
        );
        assert!(
            said.contains("The replaced save is kept as a fallback file."),
            "{said}"
        );
        assert!(!said.contains(", replacing"), "{said}");
        if summary.is_some() {
            assert!(
                said.contains("It is level 4, 12 deliveries. This will replace"),
                "{said}"
            );
        } else {
            assert!(!said.contains("It is"), "{said}");
        }
    }
}
