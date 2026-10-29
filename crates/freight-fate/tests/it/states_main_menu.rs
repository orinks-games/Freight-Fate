//! The main menu and the screens it opens: ports of
//! `tests/test_main_menu_save_scan.py`, the main-menu paths of
//! `tests/test_smoke.py`, the menu cases of `tests/test_legacy_career_gate.py`,
//! the app parts of `tests/test_career_start_options.py` and the notice
//! screens of `tests/test_save_migration.py`.
//!
//! The city hub, the online offer and the mid-trip resume are all real
//! here, so these tests assert the screen the Python flow landed on.

use crate::states_main_menu_support::*;
use ff_core::models::profile::{Profile, DEFAULT_CITY};
use ff_core::models::start_options::DEFAULT_START_KEY;
use freight_fate::app::testing::{set_headless_env, TestApp};
use freight_fate::app::version;
use freight_fate::audio::{Audio, AudioEngine, NullBackend};
use freight_fate::states::base::{Key, State};
use freight_fate::states::city::CityMenuState;
use freight_fate::states::main_menu::{
    self, enter_world, CareerActionsState, CareerStartState, ConfirmCareerActionState,
    ConfirmQuitState, HomeCityState, HomeTerminalState, LoadDriverState, MainMenuState,
    ManageCareersState, NameEntryState,
};
use freight_fate::states::save_notice::{
    LegacyCareerNoticeState, SaveMigrationNoticeState, SaveModifiedNoticeState,
};
use freight_fate::updater;

// -- tests/test_main_menu_save_scan.py ------------------------------------------

#[test]
fn test_main_menu_enter_scans_saves_once() {
    let mut app = TestApp::new();
    Profile::named_in("Road Star", "Denver").save().unwrap();
    let before = main_menu::loadable_saves_scan_count();
    app.push_state(MainMenuState::new());
    assert_eq!(main_menu::loadable_saves_scan_count() - before, 1);
}

#[test]
fn test_main_menu_enter_scan_cache_does_not_leak_between_enters() {
    // Re-entering the menu after a save changed must still see it: the
    // per-enter cache must not survive past its own enter() call.
    let mut app = TestApp::new();
    Profile::named_in("Road Star", "Denver").save().unwrap();
    app.push_state(MainMenuState::new());
    assert_eq!(main_menu::loadable_saves().len(), 1);
    Profile::named_in("Coast Runner", "Chicago").save().unwrap();
    // re-enter, as returning from a submenu would
    with_state_mut::<MainMenuState, _>(&mut app, |s, ctx| s.enter(ctx));
    assert_eq!(main_menu::loadable_saves().len(), 2);
}

#[test]
fn test_reuse_loadable_saves_scan_is_reentrant() {
    // A nested scope must not drop the cache the outer scope still owns.
    let _app = TestApp::new();
    Profile::named_in("Nested Driver", "Denver").save().unwrap();
    let before = main_menu::loadable_saves_scan_count();
    main_menu::reuse_loadable_saves_scan(|| {
        main_menu::loadable_saves();
        main_menu::reuse_loadable_saves_scan(|| {
            main_menu::loadable_saves();
        });
        main_menu::loadable_saves();
    });
    assert_eq!(main_menu::loadable_saves_scan_count() - before, 1);
}

// -- tests/test_smoke.py (main-menu paths) ---------------------------------------

#[test]
fn the_main_menu_welcomes_and_walks_a_new_career_to_the_home_city() {
    // The head of `test_full_game_flow_headless`: the welcome, New career,
    // the name, the start, the region, the city. The city hub and the drive
    // beyond it belong to other port tasks.
    let mut app = TestApp::new();
    app.push_state(MainMenuState::new());
    assert!(is::<MainMenuState>(&app));
    let lines = app.visible_lines();
    assert_eq!(lines[0], "Freight Fate");
    let welcome = format!("Freight Fate {}.", updater::spoken_version(version()));
    assert!(app.main_lines().iter().any(|line| line.contains(&welcome)));

    select::<MainMenuState>(&mut app, "New career");
    assert!(is::<NameEntryState>(&app));
    for ch in "Smoke".chars() {
        typed(&mut app, ch);
    }
    key(&mut app, Key::Return);
    assert!(is::<CareerStartState>(&app));
    key(&mut app, Key::Return); // default start: Northstar
    assert!(is::<HomeTerminalState>(&app));
    assert!(current_label::<HomeTerminalState>(&app).starts_with("Great Lakes"));
    key(&mut app, Key::Return); // default region: Great Lakes
    assert!(is::<HomeCityState>(&app));
    assert!(current_label::<HomeCityState>(&app).starts_with("Chicago"));
    app.clear_speech();
    key(&mut app, Key::Return);
    // The career exists, the welcome was spoken, and the whole new-career
    // chain is gone from the stack: main menu, then the city placeholder.
    let profile = app.ctx.profile.clone().expect("a career was created");
    assert_eq!(profile.name, "Smoke");
    assert_eq!(profile.current_city, DEFAULT_CITY);
    assert!(app
        .main_lines()
        .iter()
        .any(|line| line.starts_with("First-day briefing")));
    // The briefing just said where the truck is parked: the terminal behind
    // it gives the cash a company briefing leaves out, not the location again.
    let lines = app.main_lines();
    assert!(
        lines.iter().any(|line| line.ends_with(" dollars.")),
        "{lines:?}"
    );
    assert!(
        !lines.iter().any(|line| line.contains(", Chicago. ")),
        "{lines:?}"
    );
    assert_eq!(app.ctx.stack_len(), 2);
    assert!(is::<CityMenuState>(&app));
}

#[test]
fn escape_at_the_main_menu_asks_before_quitting() {
    let mut app = TestApp::new();
    app.push_state(MainMenuState::new());
    key(&mut app, Key::Escape);
    assert!(is::<ConfirmQuitState>(&app));
    assert_eq!(
        labels::<ConfirmQuitState>(&app),
        vec!["No, stay in Freight Fate", "Yes, quit Freight Fate"]
    );
    key(&mut app, Key::Escape);
    assert!(is::<MainMenuState>(&app));
    key(&mut app, Key::Escape);
    key(&mut app, Key::Down);
    app.set_running(true);
    key(&mut app, Key::Return);
    assert!(!app.running());
}

/// Darren, 2026-08-22: "I have ruined two of my routes doing this."
///
/// Alt+F4 and the window's close button both arrive as a quit event, and that
/// ended the process on the spot. Mid-leg it is silently destructive: saving
/// happens only at stops, so the drive is gone and the save still points at
/// the last stop. It now raises the same gate Escape does, and says what is
/// at stake when there is a drive to lose. The mid-drive half lives in
/// `states_city.rs`, where a real `DrivingState` is at hand.
#[test]
fn test_closing_the_window_asks_before_it_takes_the_drive() {
    let mut app = TestApp::new();
    app.push_state(MainMenuState::new());
    app.clear_speech();
    app.set_running(true);

    app.handle_close_request();
    assert!(is::<ConfirmQuitState>(&app));
    assert!(app.running(), "the window close must ask, not go");
    let said = app.main_lines().last().cloned().expect("the gate speaks");
    assert!(said.contains("Quit Freight Fate?"));
    // Nothing to lose from the title, so nothing is claimed.
    assert!(!said.contains("part way through a drive"));

    // It lands on No, and No puts the player back where they were.
    assert!(current_label::<ConfirmQuitState>(&app).starts_with("No"));
    key(&mut app, Key::Return);
    assert!(is::<MainMenuState>(&app));
    assert!(app.running());
}

/// A confirmation the player cannot escape would be the worse bug.
///
/// If speech has dropped or the dialog is somehow unreachable, pressing
/// Alt+F4 again has to close the game -- so the gate asks exactly once.
#[test]
fn test_a_second_close_request_is_obeyed_without_argument() {
    let mut app = TestApp::new();
    app.push_state(MainMenuState::new());
    app.set_running(true);

    app.handle_close_request();
    assert!(is::<ConfirmQuitState>(&app));
    app.handle_close_request();
    assert!(!app.running());
}

// -- tests/test_legacy_career_gate.py (the career menus) ----------------------------

#[test]
fn test_legacy_career_stays_listed_and_opens_the_notice() {
    let mut app = TestApp::new();
    let path = write_1_8_save("Old Timer");
    let before = std::fs::read(&path).unwrap();

    app.push_state(MainMenuState::new());
    let rows = labels::<MainMenuState>(&app);
    // Nothing is loadable, so there is no Continue item -- but the career
    // has not vanished: Choose career is offered and the welcome says why.
    assert!(!rows.iter().any(|label| label.starts_with("Continue")));
    assert!(rows.iter().any(|label| label == "Choose career"));
    assert!(app
        .main_lines()
        .iter()
        .any(|text| text.contains("earlier version")));

    app.push_state(LoadDriverState::new());
    assert_eq!(
        labels::<LoadDriverState>(&app)[0],
        "Old Timer: career from an earlier version of Freight Fate"
    );

    app.clear_speech();
    key(&mut app, Key::Return);
    assert!(is::<LegacyCareerNoticeState>(&app));
    let spoken = app.main_lines();
    assert!(spoken.iter().any(|text| text.contains("Nothing was lost")));
    assert!(spoken
        .iter()
        .any(|text| text.contains("still works in Freight Fate 1.8")));

    // The first choice starts a fresh career; Escape instead returns to
    // the career list without changing anything.
    assert_eq!(
        labels::<LegacyCareerNoticeState>(&app)[0],
        "Start a new career"
    );
    key(&mut app, Key::Escape);
    assert!(is::<LoadDriverState>(&app));

    // Through all of it the old save was never touched.
    assert_eq!(std::fs::read(&path).unwrap(), before);
    assert!(app.ctx.profile.is_none());
}

#[test]
fn test_notice_start_new_career_opens_name_entry() {
    let mut app = TestApp::new();
    write_1_8_save("Old Timer");
    app.push_state(LoadDriverState::new());
    key(&mut app, Key::Return); // open the notice
    key(&mut app, Key::Return); // Start a new career
    assert!(is::<NameEntryState>(&app));
}

#[test]
fn test_new_career_will_not_overwrite_a_same_named_legacy_save() {
    let mut app = TestApp::new();
    let path = write_1_8_save("Old Timer");
    let before = std::fs::read(&path).unwrap();
    let region = app.ctx.world.cities[DEFAULT_CITY].region.clone();
    let picker = HomeCityState::new(
        &app.ctx,
        "Old Timer",
        DEFAULT_START_KEY,
        &region,
        &[DEFAULT_CITY.to_string()],
    );
    app.push_state(picker);
    app.clear_speech();

    key(&mut app, Key::Return);

    assert!(app
        .main_lines()
        .iter()
        .any(|text| text.contains("different driver name")));
    assert!(app.ctx.profile.is_none());
    assert_eq!(std::fs::read(&path).unwrap(), before);
    // The picker is still up; the career chain was not torn down.
    assert!(is::<HomeCityState>(&app));
}

// -- tests/test_career_start_options.py (app parts) -----------------------------------

#[test]
fn test_new_career_start_menu_lists_company_and_owner_operator() {
    let mut app = TestApp::new();
    app.push_state(CareerStartState::new("Choice Driver"));
    let rows = labels::<CareerStartState>(&app);
    assert!(rows
        .iter()
        .any(|label| label.contains("Northstar Freight Lines")));
    assert!(rows
        .iter()
        .any(|label| label.contains("Great Lakes Training Transport")));
    assert!(rows
        .iter()
        .any(|label| label.contains("Owner-operator start")));
    let intro = with_state::<CareerStartState, _>(&app, |s, _| s.intro_help().to_string());
    assert!(intro.contains("assigned carrier equipment"));
    assert!(intro.contains("higher risk"));
}

#[test]
fn test_new_company_career_choice_creates_company_profile() {
    let mut app = TestApp::new();
    app.push_state(CareerStartState::new("Prairie Driver"));
    select::<CareerStartState>(&mut app, "Prairie Link Regional");
    key(&mut app, Key::Return);
    assert!(is::<HomeCityState>(&app));
    assert!(current_label::<HomeCityState>(&app).contains("Kansas City"));
    app.clear_speech();
    key(&mut app, Key::Return);

    // The city hub is another task's screen; the placeholder stands where
    // `CityMenuState` will.
    assert!(is::<CityMenuState>(&app));
    let profile = app.ctx.profile.clone().unwrap();
    assert_eq!(profile.carrier_name, "Prairie Link Regional");
    assert_eq!(profile.business_status, "company_driver");
    assert!(profile.visible_owned_trucks().is_empty());
    // The briefing is spoken first now; the city menu's own announcement
    // queues behind it instead of being cut off, so it is not the last line.
    let spoken = app.main_lines();
    let briefing = spoken
        .iter()
        .find(|line| line.contains("First-day briefing"))
        .expect("the briefing was spoken");
    assert!(briefing.contains("Prairie Link Regional"));
    assert!(briefing.contains("same-region lanes"));
}

// -- tests/test_save_migration.py (notice screens) --------------------------------------

fn profile_with_pending_migration_notice(name: &str) -> Profile {
    let mut p = Profile::named(name);
    p.migration_notice_pending = true;
    Profile::load(&p.save().unwrap()).unwrap()
}

#[test]
fn test_migration_notice_shows_once_then_enters_world() {
    let mut app = TestApp::new();
    app.ctx.profile = Some(profile_with_pending_migration_notice("Notice"));

    enter_world(&mut app.ctx, false);
    app.ctx.run_deferred();
    assert!(is::<SaveMigrationNoticeState>(&app));
    assert!(app
        .main_lines()
        .iter()
        .any(|text| text.contains("older versions")));
    assert_eq!(labels::<SaveMigrationNoticeState>(&app)[0], "OK");

    key(&mut app, Key::Return);
    assert!(is::<CityMenuState>(&app));
    assert!(!app.ctx.profile.as_ref().unwrap().migration_notice_pending);

    // The dismissal is saved: a fresh load goes straight into the world.
    let path = app.ctx.profile.as_ref().unwrap().path();
    app.ctx.profile = Some(Profile::load(&path).unwrap());
    assert!(!app.ctx.profile.as_ref().unwrap().migration_notice_pending);
    enter_world(&mut app.ctx, false);
    app.ctx.run_deferred();
    assert!(is::<CityMenuState>(&app));
}

#[test]
fn test_migration_notice_escape_also_acknowledges() {
    let mut app = TestApp::new();
    app.ctx.profile = Some(profile_with_pending_migration_notice("Escape Notice"));
    enter_world(&mut app.ctx, false);
    app.ctx.run_deferred();
    assert!(is::<SaveMigrationNoticeState>(&app));
    key(&mut app, Key::Escape);
    assert!(is::<CityMenuState>(&app));
    assert!(!app.ctx.profile.as_ref().unwrap().migration_notice_pending);
}

#[test]
fn test_modified_notice_shows_once_then_enters_world() {
    let mut app = TestApp::new();
    let p = Profile::named("Edited");
    let path = p.save().unwrap();
    let mut data = read_save(&path);
    data.insert("money".into(), serde_json::json!(999_999.0));
    write_packed(&path, &data);

    app.ctx.profile = Some(Profile::load(&path).unwrap());
    assert!(app.ctx.profile.as_ref().unwrap().integrity_modified);

    enter_world(&mut app.ctx, false);
    app.ctx.run_deferred();
    assert!(is::<SaveModifiedNoticeState>(&app));
    assert!(app
        .main_lines()
        .iter()
        .any(|text| text.contains("marked as modified")));

    key(&mut app, Key::Return);
    assert!(is::<CityMenuState>(&app));
    assert!(!app.ctx.profile.as_ref().unwrap().integrity_notice_pending);
    // The mark itself never clears from a dismissal.
    assert!(app.ctx.profile.as_ref().unwrap().integrity_modified);

    // The dismissal is saved: a fresh load goes straight into the world.
    app.ctx.profile = Some(Profile::load(&path).unwrap());
    assert!(!app.ctx.profile.as_ref().unwrap().integrity_notice_pending);
    enter_world(&mut app.ctx, false);
    app.ctx.run_deferred();
    assert!(is::<CityMenuState>(&app));
}

/// The two edits the signature alone never caught: a balance rewritten while
/// the game ran (the game signs it), and a career written out as plain
/// unsigned JSON (the game used to sign that for you).
#[test]
fn test_a_typed_in_fortune_is_heard_about_however_it_got_into_the_save() {
    let mut app = TestApp::new();

    let mut memory_edit = Profile::named("Memory Edit");
    memory_edit.career.total_earnings = 338.36;
    memory_edit.set_money(999_999_999.0);
    let signed_by_the_game = memory_edit.save().unwrap();

    let by_hand = Profile::named("By Hand");
    let mut data = by_hand.to_dict();
    data.remove("_signature");
    data.insert("money".into(), serde_json::json!(250_000.0));
    let plain_json = by_hand.path().with_extension("json");
    std::fs::write(
        &plain_json,
        serde_json::to_string(&serde_json::Value::Object(data)).unwrap(),
    )
    .unwrap();

    for path in [signed_by_the_game, plain_json] {
        app.ctx.profile = Some(Profile::load(&path).unwrap());
        assert!(app.ctx.profile.as_ref().unwrap().integrity_modified);
        enter_world(&mut app.ctx, false);
        app.ctx.run_deferred();
        assert!(is::<SaveModifiedNoticeState>(&app), "{}", path.display());
        assert!(app
            .main_lines()
            .iter()
            .any(|text| text.contains("marked as modified")));
        // The career is marked, never locked: Enter goes on to the city.
        key(&mut app, Key::Return);
        assert!(is::<CityMenuState>(&app));
    }
}

// -- manage careers -----------------------------------------------------------------

#[test]
fn manage_careers_deletes_a_save_after_confirmation() {
    let mut app = TestApp::new();
    let path = Profile::named_in("Doomed", "Denver").save().unwrap();
    app.push_state(MainMenuState::new());
    select::<MainMenuState>(&mut app, "Manage careers");
    assert!(is::<ManageCareersState>(&app));
    assert!(labels::<ManageCareersState>(&app)[0].starts_with("Doomed: level 1"));
    key(&mut app, Key::Return);
    assert!(is::<CareerActionsState>(&app));
    select::<CareerActionsState>(&mut app, "Delete this career");
    assert!(is::<ConfirmCareerActionState>(&app));
    assert_eq!(
        labels::<ConfirmCareerActionState>(&app)[0],
        "Yes, delete Doomed"
    );
    app.clear_speech();
    key(&mut app, Key::Return);
    assert!(!path.exists());
    assert!(is::<MainMenuState>(&app));
    assert_eq!(app.ctx.stack_len(), 1);
    assert!(app
        .main_lines()
        .iter()
        .any(|line| line == "Doomed deleted."));
    assert!(!labels::<MainMenuState>(&app)
        .iter()
        .any(|label| label.starts_with("Continue")));
}

#[test]
fn continue_latest_career_welcomes_the_driver_back() {
    let mut app = TestApp::new();
    Profile::named_in("Road Star", "Denver").save().unwrap();
    app.push_state(MainMenuState::new());
    assert!(labels::<MainMenuState>(&app)[0].starts_with("Continue latest career: Road Star"));
    app.clear_speech();
    key(&mut app, Key::Return);
    assert_eq!(app.ctx.profile.as_ref().unwrap().name, "Road Star");
    assert!(app
        .main_lines()
        .iter()
        .any(|line| line == "Welcome back, Road Star."));
    assert!(is::<CityMenuState>(&app));
}

// -- the run that has no sound ----------------------------------------------

/// Speak a fresh main menu's entry announcement.
///
/// `Menu` is imported here rather than at the top of the file because
/// `MainMenuState` implements `enter` on both `Menu` and `State`, and having
/// both traits in scope makes every unqualified `enter` call ambiguous.
fn announce_main_menu(app: &mut TestApp) {
    use freight_fate::states::base::Menu;
    MainMenuState::new().announce_entry(&mut app.ctx);
}

#[test]
fn a_run_with_no_sound_says_so_instead_of_leaving_the_player_in_silence() {
    // Starting anyway when the sound device will not open is the design.
    // Starting anyway without a word is not: reported on Linux, where the
    // device failed to open, the whole drive ran silent, and the only trace
    // was a line in a log a blind player has no reason to read.
    let mut app = TestApp::new();
    let mut engine = AudioEngine::with_backend(Box::new(NullBackend::new()));
    engine.set_silence_notice(true);
    app.ctx.audio = Box::new(engine);
    announce_main_menu(&mut app);
    let spoken = app.main_lines();
    assert!(spoken
        .iter()
        .any(|line| line.contains("Game sounds could not start on this computer")));
    assert!(spoken
        .iter()
        .any(|line| line.contains("no engine, traffic, or alert sounds")));
    // Said once, not on every trip back to the main menu.
    app.clear_speech();
    announce_main_menu(&mut app);
    assert!(!app
        .main_lines()
        .iter()
        .any(|line| line.contains("Game sounds could not start")));
}

#[test]
fn silence_that_was_asked_for_is_announced_to_nobody() {
    // A headless run, a test double, the playtest harness: silent on purpose,
    // and saying so on every start would be noise.
    let mut app = TestApp::new();
    announce_main_menu(&mut app);
    assert!(!app
        .main_lines()
        .iter()
        .any(|line| line.contains("Game sounds could not start")));
}

#[test]
fn a_headless_run_on_the_no_sound_device_arms_no_notice() {
    // The real backend pick, under the environment CI and the harness use:
    // Hearken plays through its silent backend because this run asked it
    // to, which must not read as a device that failed to open.
    set_headless_env();
    let mut engine = AudioEngine::from_preference("");
    assert!(!engine.take_silence_notice());
}

/// Quitting writes nothing: every way out of the terminal already saved.
///
/// Escape and "Quit to main menu" both save and say so, and every terminal
/// action that changes the career saves on the spot, so the quit-time save
/// only rewrote a file that already matched -- and each rewrite queued a
/// fresh cloud backup that quit then waited on. Jessie's log, 2026-09-01:
/// 0.7 seconds on the save and 2.3 on the upload it caused, for nothing.
#[test]
fn test_quitting_from_the_title_leaves_the_save_alone() {
    let mut app = TestApp::new();
    app.ctx.profile = Some(Profile::named_in("Quit Writes Nothing", DEFAULT_CITY));
    let path = app.ctx.profile.as_ref().expect("a career").path();
    assert!(!path.exists(), "the career starts unsaved");
    app.push_state(MainMenuState::new());
    app.set_running(true);

    app.handle_close_request();
    select::<ConfirmQuitState>(&mut app, "Yes");
    assert!(!app.running());
    assert!(
        !path.exists(),
        "confirming quit from the title wrote a save"
    );

    app.shutdown();
    assert!(!path.exists(), "shutdown wrote a save");
}

/// The window's close button at the terminal is the one exit that skipped
/// the terminal's own save, so its confirmation saves the way Escape does.
#[test]
fn test_closing_the_window_at_the_terminal_saves_before_quitting() {
    let mut app = TestApp::new();
    app.ctx.profile = Some(Profile::named_in("Close At Terminal", DEFAULT_CITY));
    let path = app.ctx.profile.as_ref().expect("a career").path();
    assert!(!path.exists(), "the career starts unsaved");
    let city = CityMenuState::new(&app.ctx, false);
    app.push_state(city);
    app.set_running(true);

    app.handle_close_request();
    assert!(is::<ConfirmQuitState>(&app));
    select::<ConfirmQuitState>(&mut app, "Yes");
    assert!(!app.running());
    assert!(
        path.exists(),
        "closing the window at the terminal must save before it quits"
    );
}
