//! The update download screen can always be left, and always says why
//! (issue 266: a Mac tester's update "hung up, refusing to do anything").
//! The transfer and the unpack run on a worker; these tests play that
//! worker by hand so a stalled connection or a wedged unpacker is
//! deterministic. The engine's own tests cover the transfer, the unpack
//! and the swap underneath.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use freight_fate::app::testing::TestApp;
use freight_fate::states::base::{InputEvent, Key, Mods, State};
use freight_fate::states::update::{
    UpdateDownloadState, MANUAL_DOWNLOAD, STALL_TIMEOUT_S, UNPACK_TIMEOUT_S,
};
use freight_fate::updater::{self, Install, Staged, UpdateInfo};
use hearken_update::{Manifest, Update};

fn snapshot_info() -> UpdateInfo {
    UpdateInfo::new(Update {
        manifest: Manifest {
            format: 1,
            game: updater::GAME.into(),
            version: "1.9.6-dev.202610010324".into(),
            channel: "dev".into(),
            platform: "macos-arm64".into(),
            archive: "FreightFate-1.9-tester-20261001-macos-arm64.zip".into(),
            url: None,
            size: 419_438_969,
            sha256: "ab".repeat(32),
            notes: String::new(),
            published: None,
            index: None,
        },
        archive_url: "https://example.test/a".into(),
        index_url: None,
        plan: None,
    })
}

/// A Mac app staged in `parent`, and the install it replaces.
fn staged_app(parent: &Path) -> (Staged, Install) {
    let app = parent.join("FreightFate.app");
    fs::create_dir_all(&app).unwrap();
    let staged = Staged::Bundle(app);
    let install = Install::Bundle {
        app: PathBuf::from("/Applications/FreightFate.app"),
        exe: PathBuf::from("Contents/MacOS/FreightFate"),
    };
    (staged, install)
}

/// A plain screen for a pop to land on, so the stack never empties.
fn base_screen() -> freight_fate::states::base::SimpleMenuState {
    freight_fate::states::base::SimpleMenuState::new(
        "Base",
        vec![freight_fate::states::base::MenuItem::new(
            "Back",
            |_: &mut freight_fate::states::base::SimpleMenuState,
             ctx: &mut freight_fate::app::GameContext| ctx.pop_state(),
        )],
    )
}

fn key(key: Key) -> InputEvent {
    InputEvent::KeyDown {
        key,
        mods: Mods::NONE,
        text: None,
        repeat: false,
    }
}

/// Advance the screen `seconds` in one-second frames.
fn run_for(state: &mut UpdateDownloadState, app: &mut TestApp, seconds: f64) {
    let mut left = seconds;
    while left > 0.0 && !state.is_finished() {
        let dt = left.min(1.0);
        state.update(&mut app.ctx, dt);
        app.ctx.run_deferred();
        left -= dt;
    }
}

#[test]
fn a_download_that_goes_quiet_is_abandoned_with_a_spoken_reason() {
    let mut app = TestApp::new();
    // Two screens, so the pop leaves one: an empty stack is a quit.
    app.push_state(base_screen());
    app.push_state(base_screen());
    let mut state = UpdateDownloadState::in_progress(snapshot_info());
    let depth = app.ctx.stack_len();
    app.ctx.running = true;
    let running_before = app.ctx.running;
    app.clear_speech();

    state.report_progress(10_000_000, 419_438_969);
    run_for(&mut state, &mut app, STALL_TIMEOUT_S - 5.0);
    // Bytes keep arriving: the clock starts over each time.
    state.report_progress(20_000_000, 419_438_969);
    run_for(&mut state, &mut app, STALL_TIMEOUT_S - 5.0);
    assert!(!state.is_finished(), "a slow line is not a stalled one");
    assert_eq!(app.ctx.stack_len(), depth);

    run_for(&mut state, &mut app, 10.0);

    assert!(state.is_finished());
    assert!(state.is_cancelled(), "the worker is told to stop");
    assert_eq!(app.ctx.stack_len(), depth - 1, "handed back to the menu");
    let said = app.main_lines().join(" ");
    assert!(said.contains("The download stopped."), "{said}");
    assert!(said.contains("Your game is unchanged."), "{said}");
    assert!(said.contains(MANUAL_DOWNLOAD), "{said}");
    assert_eq!(app.ctx.running, running_before, "the game keeps running");
    app.shutdown();
}

#[test]
fn escape_leaves_at_once_even_while_the_worker_is_stuck() {
    let mut app = TestApp::new();
    app.push_state(base_screen());
    let mut state = UpdateDownloadState::in_progress(snapshot_info());
    let depth = app.ctx.stack_len();
    app.clear_speech();

    state.handle_event(&mut app.ctx, &key(Key::Escape));
    app.ctx.run_deferred();

    assert!(state.is_cancelled());
    assert!(state.is_finished());
    assert_eq!(app.ctx.stack_len(), depth - 1, "no waiting on the worker");
    let said = app.main_lines().join(" ");
    assert!(said.contains("Update canceled."), "{said}");
    app.shutdown();
}

#[test]
fn unpacking_is_spoken_and_does_not_count_as_a_stall() {
    let mut app = TestApp::new();
    app.push_state(base_screen());
    let mut state = UpdateDownloadState::in_progress(snapshot_info());
    let depth = app.ctx.stack_len();
    app.clear_speech();

    state.report_progress(419_438_969, 419_438_969);
    state.report_unpacking();
    run_for(&mut state, &mut app, STALL_TIMEOUT_S * 3.0);

    assert!(!state.is_finished(), "no bytes arrive while unpacking");
    assert_eq!(app.ctx.stack_len(), depth);
    let said = app.main_lines().join(" ");
    assert_eq!(
        said.matches("Download complete. Unpacking the update.")
            .count(),
        1,
        "{said}"
    );

    app.clear_speech();
    state.handle_event(&mut app.ctx, &key(Key::Tab));
    assert!(app.main_lines().join(" ").contains("Unpacking the update."));
    app.shutdown();
}

#[test]
fn a_wedged_unpack_is_abandoned_with_a_spoken_reason() {
    let mut app = TestApp::new();
    app.push_state(base_screen());
    let mut state = UpdateDownloadState::in_progress(snapshot_info());
    let depth = app.ctx.stack_len();
    state.report_unpacking();
    app.clear_speech();

    // Past the bound in big frames: a paused or throttled loop still
    // reaches it.
    let mut left = UNPACK_TIMEOUT_S + 1.0;
    while left > 0.0 && !state.is_finished() {
        state.update(&mut app.ctx, 60.0);
        app.ctx.run_deferred();
        left -= 60.0;
    }

    assert!(state.is_finished());
    assert!(state.is_cancelled());
    assert_eq!(app.ctx.stack_len(), depth - 1);
    let said = app.main_lines().join(" ");
    assert!(said.contains("took too long"), "{said}");
    assert!(said.contains(MANUAL_DOWNLOAD), "{said}");
    app.shutdown();
}

#[test]
fn an_apply_script_that_cannot_start_never_quits_the_game() {
    // Quitting with no script behind it closed the game with nothing to
    // reopen it: the update "hung", then the game was simply gone.
    let mut app = TestApp::new();
    // Two screens, so the pop leaves one: an empty stack is a quit.
    app.push_state(base_screen());
    app.push_state(base_screen());
    let tmp = tempfile::tempdir().unwrap();
    let (staged, install) = staged_app(tmp.path());
    let new_root = staged.path().to_path_buf();
    let mut state = UpdateDownloadState::finished_with(
        snapshot_info(),
        tmp.path().to_path_buf(),
        staged,
        install,
        Box::new(|_, _| true),
        Box::new(|staged, _| staged.path().to_path_buf()),
    )
    .with_apply(Box::new(|_, _, _| {
        Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "no shell",
        ))
    }));
    let depth = app.ctx.stack_len();
    app.ctx.running = true;
    let running_before = app.ctx.running;
    app.clear_speech();

    state.update(&mut app.ctx, 0.0);
    app.ctx.run_deferred();

    assert_eq!(app.ctx.running, running_before, "the game must not quit");
    assert_eq!(app.ctx.stack_len(), depth - 1);
    let said = app.main_lines().join(" ");
    assert!(said.contains("could not install itself"), "{said}");
    assert!(said.contains(&new_root.display().to_string()), "{said}");
    assert!(!said.contains("restarts by itself"), "{said}");
    app.shutdown();
}

#[test]
fn a_started_apply_script_restarts_the_game() {
    let mut app = TestApp::new();
    app.push_state(base_screen());
    let tmp = tempfile::tempdir().unwrap();
    let (staged, install) = staged_app(tmp.path());
    let mut state = UpdateDownloadState::finished_with(
        snapshot_info(),
        tmp.path().to_path_buf(),
        staged,
        install,
        Box::new(|_, _| true),
        Box::new(|staged, _| staged.path().to_path_buf()),
    )
    .with_apply(Box::new(|_, _, _| Ok(())));
    app.ctx.running = true;
    app.clear_speech();

    state.update(&mut app.ctx, 0.0);

    assert!(!app.ctx.running, "quits so the script can swap the app");
    assert!(app
        .main_lines()
        .join(" ")
        .contains("The game closes and restarts by itself."));
    app.shutdown();
}

/// Run the real worker over `fetch` and wait for it to report.
fn run_worker(fetch: freight_fate::states::update::FetchHook) -> (TestApp, UpdateDownloadState) {
    let mut app = TestApp::new();
    app.push_state(base_screen());
    let mut state = UpdateDownloadState::new(snapshot_info()).with_fetch(fetch);
    state.enter(&mut app.ctx);
    let deadline = Instant::now() + Duration::from_secs(20);
    while !state.is_done() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(state.is_done(), "the worker finished");
    (app, state)
}

#[test]
fn a_failed_transfer_removes_its_partial_download_and_says_why() {
    let (mut app, mut state) = run_worker(Box::new(
        |info, dir, _install, progress, _phase, _cancelled| {
            fs::write(dir.join(&info.update.manifest.archive), vec![0u8; 4096]).unwrap();
            progress(4096, 419_438_969);
            Err(updater::DownloadError::Stalled(Duration::from_secs(60)))
        },
    ));
    let staging = state.staging().expect("a staging folder was made");
    assert!(!staging.exists(), "partial download left in {staging:?}");
    let depth = app.ctx.stack_len();
    app.clear_speech();

    state.update(&mut app.ctx, 0.1);
    app.ctx.run_deferred();

    assert_eq!(app.ctx.stack_len(), depth - 1);
    let said = app.main_lines().join(" ");
    assert!(
        said.contains("The download stopped. Nothing arrived for 60 seconds."),
        "{said}"
    );
    assert!(said.contains("Your game is unchanged."), "{said}");
    assert!(said.contains(MANUAL_DOWNLOAD), "{said}");
    app.shutdown();
}

#[test]
fn a_cancelled_transfer_removes_its_partial_download() {
    let (app, state) = run_worker(Box::new(
        |info, dir, _install, _progress, _phase, cancelled| {
            fs::write(dir.join(&info.update.manifest.archive), vec![0u8; 4096]).unwrap();
            cancelled.store(true, std::sync::atomic::Ordering::SeqCst);
            Err(updater::DownloadError::Cancelled)
        },
    ));
    let staging = state.staging().expect("a staging folder was made");
    assert!(!staging.exists(), "partial download left in {staging:?}");
    assert!(state.error().is_none(), "nothing to say: the player left");
    let mut app = app;
    app.shutdown();
}

#[test]
fn a_damaged_transfer_is_refused() {
    let (mut app, mut state) = run_worker(Box::new(
        |_info, _dir, _install, _progress, _phase, _cancelled| Err(updater::DownloadError::Corrupt),
    ));
    app.clear_speech();
    state.update(&mut app.ctx, 0.1);
    let said = app.main_lines().join(" ");
    assert!(said.contains("arrived damaged"), "{said}");
    app.shutdown();
}

#[test]
fn a_staged_update_this_copy_cannot_apply_is_saved_and_said_where() {
    let mut app = TestApp::new();
    app.push_state(base_screen());
    app.push_state(base_screen());
    let tmp = tempfile::tempdir().unwrap();
    let (staged, install) = staged_app(tmp.path());
    let parked = tmp.path().join("parked").join("FreightFate.app");
    let dest = parked.clone();
    let mut state = UpdateDownloadState::finished_with(
        snapshot_info(),
        tmp.path().to_path_buf(),
        staged,
        install,
        Box::new(|_, _| false),
        Box::new(move |_, _| dest.clone()),
    )
    .with_apply(Box::new(|_, _, _| {
        panic!("a copy that cannot update never applies")
    }));
    app.ctx.running = true;
    app.clear_speech();

    state.update(&mut app.ctx, 0.0);
    app.ctx.run_deferred();

    assert!(app.ctx.running, "the game keeps running");
    let said = app.main_lines().join(" ");
    assert!(said.contains("this install cannot update itself"), "{said}");
    assert!(said.contains(&parked.display().to_string()), "{said}");
    app.shutdown();
}

#[test]
fn the_worker_hands_the_staged_update_to_the_screen() {
    let (mut app, mut state) = run_worker(Box::new(|_info, dir, _install, progress, phase, _c| {
        progress(10, 10);
        phase(updater::Phase::Unpacking);
        let app = dir.join("FreightFate.app");
        fs::create_dir_all(&app).unwrap();
        Ok(Staged::Bundle(app))
    }));
    let staged = state.staged().expect("staged");
    assert!(staged.path().ends_with("FreightFate.app"));
    let applied = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let seen = applied.clone();
    state = state
        .with_auto_apply(Box::new(|_, _| true))
        .with_apply(Box::new(move |_, _, _| {
            seen.store(true, std::sync::atomic::Ordering::SeqCst);
            Ok(())
        }));
    app.ctx.running = true;
    state.update(&mut app.ctx, 0.1);
    assert!(applied.load(std::sync::atomic::Ordering::SeqCst));
    assert!(!app.ctx.running, "quits for the swap");
    if let Some(staging) = state.staging() {
        let _ = fs::remove_dir_all(staging);
    }
    app.shutdown();
}
