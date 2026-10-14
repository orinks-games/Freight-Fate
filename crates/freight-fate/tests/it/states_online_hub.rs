//! Port of `tests/test_online_hub.py`: the Online hub, one main-menu home for
//! the board, account, and sharing.

use crate::states_main_menu_support as menus;
use crate::states_online_support::*;
use ff_core::achievements::{achievements_in_category, categories, ACHIEVEMENTS, HIDDEN_NAME};
use freight_fate::app::testing::TestApp;
use freight_fate::controller::ControllerButton;
use freight_fate::duty_watch::{DutyWatch, DutyWatchOptions, POLL_INTERVAL_S};
use freight_fate::net::testing::FakeTransport;
use freight_fate::net::testing::ManualClock;
use freight_fate::states::account_achievements::{
    AccountAchievementCategoryState, AccountAchievementsState,
};
use freight_fate::states::base::{InputEvent, Key};
use freight_fate::states::main_menu::MainMenuState;
use freight_fate::states::online_hub::OnlineHubState;
use freight_fate::states::online_states::DriversOnlineState;
use serde_json::{json, Map, Value};

fn hub(app: &mut TestApp) -> freight_fate::app::SharedState {
    let hub = OnlineHubState::new(&mut app.ctx);
    push(app, hub)
}

#[test]
fn test_main_menu_online_item_opens_the_hub() {
    let mut app = TestApp::new();
    let _board = install_transport(FakeTransport::replying(json!({"drivers": []})));
    app.push_state(MainMenuState::new());
    menus::move_to::<MainMenuState>(&mut app, "Online");
    // Spoken help text exists for F1.
    assert!(!menus::current_help::<MainMenuState>(&app).is_empty());
    menus::key(&mut app, Key::Return);
    assert!(menus::is::<OnlineHubState>(&app));

    let hub = app.state().expect("the hub is on the stack");
    // The board leads because viewing it shares nothing; the
    // online-enhancement master switch sits right under it.
    let rows = labels::<OnlineHubState>(&hub, &app.ctx);
    assert_eq!(rows[0], "Drivers on duty");
    assert_eq!(rows[1], "Driver directory");
    assert_eq!(rows[2], "Say when drivers go on or off duty: off");
    assert_eq!(rows[3], "Account achievements");
    assert_eq!(rows[4], "Your profile");
    assert_eq!(rows[5], "Online services: on");
    assert_eq!(rows[6], "Let remote dispatchers take my calls: off");
    let help = helps::<OnlineHubState>(&hub, &app.ctx);
    for (row, help) in rows.iter().zip(help.iter()).take(rows.len() - 1) {
        assert!(!help.is_empty(), "{row} has no help"); // every row but Back explains itself
    }
}

#[test]
fn test_remote_dispatch_toggle_requires_an_account_and_persists() {
    let mut app = TestApp::new();
    let hub = hub(&mut app);
    move_to::<OnlineHubState>(&mut app, &hub, "Let remote dispatchers take my calls");
    let help = helps::<OnlineHubState>(&hub, &app.ctx);
    assert!(
        help[6].contains("office answers if nobody does within a minute"),
        "{}",
        help[6]
    );

    app.clear_speech();
    press(&mut app, Key::Return);
    assert!(!app.ctx.settings.remote_dispatch_calls);
    assert!(said(&app).contains("need your orinks.net account"));

    let _identity = install_identity(&app, Some(&identity()));
    app.clear_speech();
    press(&mut app, Key::Return);
    assert!(app.ctx.settings.remote_dispatch_calls);
    assert!(ff_core::settings::Settings::load().remote_dispatch_calls);
    assert!(said(&app).contains("Let remote dispatchers take my calls: on"));
}

#[test]
fn test_hub_drivers_board_item_opens_the_board() {
    let mut app = TestApp::new();
    let _board = install_transport(FakeTransport::replying(json!({"drivers": []})));
    let hub = hub(&mut app);
    assert_eq!(
        current_label::<OnlineHubState>(&hub, &app.ctx),
        "Drivers on duty"
    );
    // The board leads because viewing it shares nothing; the
    // online-enhancement master switch sits right under it.
    let rows = labels::<OnlineHubState>(&hub, &app.ctx);
    assert_eq!(rows[0], "Drivers on duty");
    assert_eq!(rows[1], "Driver directory");
    assert_eq!(rows[2], "Say when drivers go on or off duty: off");
    assert_eq!(rows[3], "Account achievements");
    assert_eq!(rows[4], "Your profile");
    assert_eq!(rows[5], "Online services: on");
    assert_eq!(rows[6], "Let remote dispatchers take my calls: off");
    let help = helps::<OnlineHubState>(&hub, &app.ctx);
    for (row, help) in rows.iter().zip(help.iter()).take(rows.len() - 1) {
        assert!(!help.is_empty(), "{row} has no help"); // every row but Back explains itself
    }
    press(&mut app, Key::Return);
    assert!(is_state::<DriversOnlineState>(&app.state().unwrap()));
}

#[test]
fn test_account_achievement_browser_speaks_scope_controls_and_badge_states_without_writing() {
    assert!(OnlineHubState::INTRO_HELP.contains("work without connecting"));
    let mut app = TestApp::new();
    app.ctx.settings.announce_menu_position = true;
    app.ctx.account_achievements =
        freight_fate::account_achievements::AccountAchievements::empty(app.data_dir.path());

    let hidden = ACHIEVEMENTS
        .iter()
        .find(|achievement| achievement.hidden)
        .expect("the catalog has a hidden achievement");
    let earned = ACHIEVEMENTS
        .iter()
        .find(|achievement| !achievement.hidden)
        .expect("the catalog has a visible achievement");
    app.ctx
        .account_achievements
        .record(earned.id, None)
        .unwrap();
    let ledger_path = app.data_dir.path().join("account-achievements.json");
    let before = std::fs::read(&ledger_path).unwrap();
    let hub = hub(&mut app);

    move_to::<OnlineHubState>(&mut app, &hub, "Account achievements");
    app.clear_speech();
    press(&mut app, Key::Return);
    assert!(is_state::<AccountAchievementsState>(&app.state().unwrap()));
    let opening = said(&app);
    assert!(opening.contains("Account achievements"), "{opening}");
    assert!(
        opening.contains("earned across every career on this installation"),
        "{opening}"
    );
    assert!(opening.contains("Controls and account scope"), "{opening}");

    let account = app.state().unwrap();
    let rows = labels::<AccountAchievementsState>(&account, &app.ctx);
    assert_eq!(rows[0], "Controls and account scope");
    assert!(rows[1].starts_with("Summary:"));
    assert_eq!(
        &rows[2..2 + categories().len()],
        categories()
            .iter()
            .map(|category| {
                let count = achievements_in_category(category.id).len();
                let done = usize::from(category.id == earned.category);
                format!("{}. {done} of {count}", category.title)
            })
            .collect::<Vec<_>>()
    );

    app.clear_speech();
    press(&mut app, Key::Return);
    let controls = said(&app);
    assert!(controls.contains("Enter opens a category"), "{controls}");
    assert!(controls.contains("Escape returns to Online"), "{controls}");
    assert!(
        controls.contains("Inside one, Enter repeats the achievement"),
        "{controls}"
    );
    assert!(
        controls.contains("Escape returns to the categories"),
        "{controls}"
    );

    move_to::<AccountAchievementsState>(
        &mut app,
        &account,
        categories()
            .iter()
            .find(|category| category.id == earned.category)
            .unwrap()
            .title,
    );
    press(&mut app, Key::Return);
    assert!(is_state::<AccountAchievementCategoryState>(
        &app.state().unwrap()
    ));

    let category_state = app.state().unwrap();
    let category_rows = labels::<AccountAchievementCategoryState>(&category_state, &app.ctx);
    let catalog = achievements_in_category(earned.category);
    assert_eq!(category_rows.len(), catalog.len() + 1);
    for (row, achievement) in category_rows.iter().zip(catalog.iter()) {
        if achievement.id == earned.id {
            assert!(
                row.starts_with(&format!("Earned: {}", earned.name)),
                "{row}"
            );
        } else {
            assert!(
                row.starts_with(&format!("Locked: {}", achievement.name)),
                "{row}"
            );
        }
    }

    move_to::<AccountAchievementCategoryState>(
        &mut app,
        &category_state,
        &format!("Earned: {}", earned.name),
    );
    let position_line = app.main_lines().last().cloned().unwrap_or_default();
    assert!(position_line.contains(" of "), "{position_line}");
    app.clear_speech();
    press(&mut app, Key::Return);
    assert!(said(&app).starts_with(&format!("Earned: {}", earned.name)));

    let visible_locked = catalog
        .iter()
        .find(|achievement| achievement.id != earned.id)
        .expect("category has another visible locked achievement");
    move_to::<AccountAchievementCategoryState>(
        &mut app,
        &category_state,
        &format!("Locked: {}", visible_locked.name),
    );
    app.clear_speech();
    press(&mut app, Key::Return);
    assert!(said(&app).starts_with(&format!("Locked: {}", visible_locked.name)));

    press(&mut app, Key::Escape);
    assert!(is_state::<AccountAchievementsState>(&app.state().unwrap()));
    let account = app.state().unwrap();
    move_to::<AccountAchievementsState>(
        &mut app,
        &account,
        categories()
            .iter()
            .find(|category| category.id == hidden.category)
            .unwrap()
            .title,
    );
    press(&mut app, Key::Return);
    let hidden_state = app.state().unwrap();
    assert!(is_state::<AccountAchievementCategoryState>(&hidden_state));
    let hidden_rows = labels::<AccountAchievementCategoryState>(&hidden_state, &app.ctx);
    assert_eq!(hidden_rows[0], format!("Locked: {HIDDEN_NAME}"));
    app.clear_speech();
    press(&mut app, Key::Return);
    assert!(said(&app).starts_with(&format!("Locked: {HIDDEN_NAME}")));
    assert_eq!(std::fs::read(&ledger_path).unwrap(), before);

    app.clear_speech();
    press(&mut app, Key::Escape);
    assert!(is_state::<AccountAchievementsState>(&app.state().unwrap()));
    assert!(said(&app).contains("Account achievements"));
    assert_eq!(std::fs::read(&ledger_path).unwrap(), before);
}

#[test]
fn test_account_achievements_back_item_returns_to_online_hub_with_focus() {
    let mut app = TestApp::new();
    let hub = hub(&mut app);
    move_to::<OnlineHubState>(&mut app, &hub, "Account achievements");
    press(&mut app, Key::Return);
    assert!(is_state::<AccountAchievementsState>(&app.state().unwrap()));

    let account = app.state().unwrap();
    move_to::<AccountAchievementsState>(&mut app, &account, "Back to Online");
    app.clear_speech();
    press(&mut app, Key::Return);
    assert!(std::rc::Rc::ptr_eq(&app.state().unwrap(), &hub));
    assert_eq!(
        current_label::<OnlineHubState>(&hub, &app.ctx),
        "Account achievements"
    );
    assert!(said(&app).contains("Account achievements"));
}

#[test]
fn test_account_achievements_escape_returns_to_online_hub_with_focus_and_speech() {
    let mut app = TestApp::new();
    let hub = hub(&mut app);
    move_to::<OnlineHubState>(&mut app, &hub, "Account achievements");
    press(&mut app, Key::Return);
    assert!(is_state::<AccountAchievementsState>(&app.state().unwrap()));

    app.clear_speech();
    press(&mut app, Key::Escape);

    assert!(std::rc::Rc::ptr_eq(&app.state().unwrap(), &hub));
    assert_eq!(
        current_label::<OnlineHubState>(&hub, &app.ctx),
        "Account achievements"
    );
    assert!(said(&app).contains("Account achievements"));
}

#[test]
fn test_account_categories_and_late_badges_are_controller_reachable() {
    let mut app = TestApp::new();
    let hub = hub(&mut app);
    move_to::<OnlineHubState>(&mut app, &hub, "Account achievements");
    press(&mut app, Key::Return);

    let account = app.state().unwrap();
    for button in [
        ControllerButton::DPadDown,
        ControllerButton::DPadDown,
        ControllerButton::A,
    ] {
        with_state::<AccountAchievementsState, _>(&account, |state| {
            freight_fate::states::base::State::handle_controller(
                state,
                &mut app.ctx,
                &InputEvent::button(button),
            )
        });
        app.ctx.run_deferred();
    }
    assert!(is_state::<AccountAchievementCategoryState>(
        &app.state().unwrap()
    ));

    let category = app.state().unwrap();
    let rows = labels::<AccountAchievementCategoryState>(&category, &app.ctx);
    let final_achievement = rows[rows.len() - 2].clone();
    for _ in 0..rows.len() - 2 {
        with_state::<AccountAchievementCategoryState, _>(&category, |state| {
            freight_fate::states::base::State::handle_controller(
                state,
                &mut app.ctx,
                &InputEvent::button(ControllerButton::DPadDown),
            )
        });
    }
    assert_eq!(
        current_label::<AccountAchievementCategoryState>(&category, &app.ctx),
        final_achievement
    );

    app.clear_speech();
    with_state::<AccountAchievementCategoryState, _>(&category, |state| {
        freight_fate::states::base::State::handle_controller(
            state,
            &mut app.ctx,
            &InputEvent::button(ControllerButton::A),
        )
    });
    assert!(said(&app).starts_with(&final_achievement));

    app.clear_speech();
    with_state::<AccountAchievementCategoryState, _>(&category, |state| {
        freight_fate::states::base::State::handle_controller(
            state,
            &mut app.ctx,
            &InputEvent::button(ControllerButton::B),
        )
    });
    app.ctx.run_deferred();
    assert!(is_state::<AccountAchievementsState>(&app.state().unwrap()));
    let account = app.state().unwrap();
    assert!(
        current_label::<AccountAchievementsState>(&account, &app.ctx)
            .starts_with(categories()[0].title)
    );
    assert!(said(&app).contains(categories()[0].title));

    app.clear_speech();
    with_state::<AccountAchievementsState, _>(&account, |state| {
        freight_fate::states::base::State::handle_controller(
            state,
            &mut app.ctx,
            &InputEvent::button(ControllerButton::B),
        )
    });
    app.ctx.run_deferred();
    assert!(std::rc::Rc::ptr_eq(&app.state().unwrap(), &hub));
    assert_eq!(
        current_label::<OnlineHubState>(&hub, &app.ctx),
        "Account achievements"
    );
    assert!(said(&app).contains("Account achievements"));
}

/// Right arrow on an action row does nothing; on a toggle row it flips that
/// row's own setting. Pins the adjust list against drifting out of step with
/// build_items when rows are added or reordered.
#[test]
fn test_hub_left_right_adjust_rows_align_with_items() {
    let mut app = TestApp::new();
    let hub = hub(&mut app);
    for label in [
        "Drivers on duty",
        "Driver directory",
        "Your profile",
        "Open my driver setup page",
        "Restore a cloud backup",
        "Link a Mastodon account",
    ] {
        move_to::<OnlineHubState>(&mut app, &hub, label);
        let before = app.ctx.stack_len();
        press(&mut app, Key::Right);
        assert_eq!(app.ctx.stack_len(), before);
        assert!(std::rc::Rc::ptr_eq(&app.state().unwrap(), &hub));
    }
    move_to::<OnlineHubState>(&mut app, &hub, "Discord presence");
    let before = app.ctx.settings.discord_presence;
    press(&mut app, Key::Right);
    assert_ne!(app.ctx.settings.discord_presence, before);
    press(&mut app, Key::Left);
    assert_eq!(app.ctx.settings.discord_presence, before);
}

/// The on/off duty notice row: off by default, flips on Enter, and the
/// background watch follows it -- and follows the master switch too.
#[test]
fn test_hub_duty_notice_row_turns_the_watch_on_and_off() {
    let mut app = TestApp::new();
    // A watch that reads a fake list, so turning it on never touches the
    // network the test sandbox refuses.
    app.ctx.services.duty = DutyWatch::new(DutyWatchOptions {
        transport: FakeTransport::replying(json!({"drivers": []})),
        threaded: false,
        ..DutyWatchOptions::default()
    });
    let hub = hub(&mut app);
    assert!(!app.ctx.settings.duty_notifications);
    assert!(!app.ctx.services.duty.enabled());

    move_to::<OnlineHubState>(&mut app, &hub, "Say when drivers go on or off duty");
    app.clear_speech();
    press(&mut app, Key::Return);
    assert!(app.ctx.settings.duty_notifications);
    assert!(app.ctx.services.duty.enabled());
    assert!(
        said(&app).contains("Say when drivers go on or off duty: on"),
        "{}",
        said(&app)
    );
    // The choice is on disk, not just in memory.
    assert!(ff_core::settings::Settings::load().duty_notifications);

    // The master switch stands it down without losing the choice.
    move_to::<OnlineHubState>(&mut app, &hub, "Online services");
    press(&mut app, Key::Return);
    assert!(!app.ctx.services.duty.enabled());
    assert!(app.ctx.settings.duty_notifications);
    press(&mut app, Key::Return);
    assert!(app.ctx.services.duty.enabled());

    move_to::<OnlineHubState>(&mut app, &hub, "Say when drivers go on or off duty");
    press(&mut app, Key::Left);
    assert!(!app.ctx.settings.duty_notifications);
    assert!(!app.ctx.services.duty.enabled());
}

/// A watched arrival reaches the player through the game loop, queued on the
/// normal channel so it never cuts urgent speech.
#[test]
fn test_a_duty_notice_is_spoken_by_the_game_loop() {
    let mut app = TestApp::new();
    let transport = FakeTransport::replying(json!({"drivers": []}));
    let clock = ManualClock::new();
    app.ctx.services.duty = DutyWatch::new(DutyWatchOptions {
        enabled: true,
        transport: transport.clone(),
        clock: clock.clock(),
        threaded: false,
        ..DutyWatchOptions::default()
    });
    app.ctx.services.duty.start();
    transport.set_reply(Some(json!({"drivers": [{
        "driverId": "road-star-1234", "displayName": "Road Star",
        "activity": "Driving", "detail": "", "updatedAt": 0,
    }]})));
    clock.advance(POLL_INTERVAL_S);
    app.ctx.services.duty.pump();
    app.clear_speech();
    app.tick(0.016);
    assert_eq!(
        app.main_calls(),
        vec![("Road Star is on duty.".to_string(), false)]
    );
}

/// The path is not something anyone should have to remember.
///
/// Josh's ask, 2026-08-15: a player who needs to rename their driver or
/// sign a computer out has to get to /freight-fate/online/setup, and until
/// now the only way there was typing it. The row opens the address the
/// build actually talks to, staged host included.
#[test]
fn test_hub_opens_the_driver_setup_page_in_a_browser() {
    // FREIGHT_FATE_ONLINE_URL is process-global, and these three tests write
    // it at once. Without the lock they raced, and the race was invisible for
    // as long as DEFAULT_BASE_URL happened to be the staged host they set --
    // losing it still read back the expected address. The 1.9 cutover moved
    // the default to production and the coincidence ended.
    let _env = freight_fate::app::testing::env_lock();
    let mut app = TestApp::new();
    std::env::set_var("FREIGHT_FATE_ONLINE_URL", "https://dev.orinks.net");
    let browser = install_browser(true);
    let hub = hub(&mut app);
    move_to::<OnlineHubState>(&mut app, &hub, "Open my driver setup page");
    press(&mut app, Key::Return);

    assert_eq!(
        browser.opened(),
        vec!["https://dev.orinks.net/freight-fate/online/setup".to_string()]
    );
    // It stays on the hub: nothing to come back to in the game.
    assert!(std::rc::Rc::ptr_eq(&app.state().unwrap(), &hub));
    let said = said(&app);
    assert!(said.contains("Opening your driver setup page in your browser"));
    std::env::remove_var("FREIGHT_FATE_ONLINE_URL");
}

/// A remote or streamed session is the normal case where the browser
/// never opens, and there is no review cursor to read an address out of.
#[test]
fn test_hub_setup_page_falls_back_to_the_clipboard() {
    let _env = freight_fate::app::testing::env_lock();
    let mut app = TestApp::new();
    std::env::set_var("FREIGHT_FATE_ONLINE_URL", "https://dev.orinks.net");
    let _browser = install_browser(false);
    let hub = hub(&mut app);
    move_to::<OnlineHubState>(&mut app, &hub, "Open my driver setup page");
    press(&mut app, Key::Return);
    assert!(said(&app).contains("clipboard"));
    std::env::remove_var("FREIGHT_FATE_ONLINE_URL");
}

/// Neither browser nor clipboard: the address itself has to be spoken,
/// because it is the only way the player can reach the page at all.
#[test]
fn test_hub_setup_page_reads_the_address_out_when_nothing_else_works() {
    let _env = freight_fate::app::testing::env_lock();
    let mut app = TestApp::new();
    std::env::set_var("FREIGHT_FATE_ONLINE_URL", "https://dev.orinks.net");
    let _browser = install_browser(false);
    app.ctx.clipboard = Box::new(RefusingClipboard);
    let hub = hub(&mut app);
    move_to::<OnlineHubState>(&mut app, &hub, "Open my driver setup page");
    press(&mut app, Key::Return);
    assert!(said(&app).contains("https://dev.orinks.net/freight-fate/online/setup"));
    std::env::remove_var("FREIGHT_FATE_ONLINE_URL");
}

/// `build_items()` rather than the pushed rows: the rows are only populated
/// on enter(), and these cases are about the label, not the screen.
fn hub_row(app: &mut TestApp, needle: &str) -> (String, String) {
    let mut hub = OnlineHubState::new(&mut app.ctx);
    built_rows(&mut hub, &mut app.ctx)
        .into_iter()
        .find(|(text, _)| text.contains(needle))
        .expect("the row")
}

fn record_conflict(app: &TestApp, name: &str) {
    let mut latest = Map::new();
    latest.insert("latestRevision".to_string(), Value::from(4));
    app.ctx
        .cloud_saves_service()
        .sync_state()
        .record_conflict(name, &latest);
}

/// Brandon (armstrong445), 2026-08-15: a conflict parked his backups at
/// revision 2 against the cloud's 4, and the recovery line told him to open
/// "Restore a cloud backup". He landed on that row five times across twenty
/// minutes and never opened it -- then signed out of every computer and
/// re-activated instead, which cannot clear a conflict. Under the bare name
/// the row promises to REPLACE the career he has just played, which is the
/// opposite of what he wanted, so the waiting decision has to say itself
/// here rather than wait to be discovered.
#[test]
fn test_the_restore_row_says_when_a_career_is_waiting_on_a_choice() {
    let mut app = TestApp::new();
    record_conflict(&app, "armstrong45");
    let (text, help) = hub_row(&mut app, "Restore a cloud backup");

    assert!(text.contains("armstrong45"));
    assert!(text.contains("which copy to keep"));
    // The reason to open a row named "Restore" when you mean to keep your
    // own save has to reach the player, or the name wins the argument.
    assert!(help.contains("not backing up at all until you pick"));
    assert!(help.contains("keeps what you have played"));
    assert!(help.contains("nothing is overwritten until you choose"));
}

/// The warning is only worth anything if it is absent the rest of the
/// time; a row that always claims something needs attention is noise.
#[test]
fn test_the_restore_row_is_its_plain_self_when_nothing_is_waiting() {
    let mut app = TestApp::new();
    let (text, help) = hub_row(&mut app, "Restore a cloud backup");
    assert_eq!(text, "Restore a cloud backup");
    assert!(!help.contains("waiting"));
}

#[test]
fn test_several_waiting_careers_are_counted_not_listed() {
    let mut app = TestApp::new();
    for name in ["a", "b", "c"] {
        record_conflict(&app, name);
    }
    let (text, _) = hub_row(&mut app, "Restore a cloud backup");
    assert!(text.contains("3 careers are waiting"));
}

/// This label is built on every pass through the Online menu. A cloud
/// service that is off, missing or still starting must not take the whole
/// screen down with it. (The Python test made the service accessor raise;
/// `GameContext::cloud_saves_service` cannot fail here, so this pins the
/// nearest real case: a service that is off, with no sync state on disk.)
#[test]
fn test_a_broken_cloud_service_costs_the_row_its_warning_not_the_menu() {
    let mut app = TestApp::new();
    install_cloud(&mut app, FakeTransport::new(), false);
    let (text, _) = hub_row(&mut app, "Restore a cloud backup");
    assert_eq!(text, "Restore a cloud backup");
}
