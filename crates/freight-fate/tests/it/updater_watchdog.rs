//! The update download screen can always be left, and always says why
//! (issue 266: a Mac tester's update "hung up, refusing to do anything").
//! The transfer and the unpack run on a worker; these tests play that
//! worker by hand so a stalled connection or a wedged unpacker is
//! deterministic, and check the macOS bundle guards and the bounded
//! unpacker underneath.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use freight_fate::app::testing::TestApp;
use freight_fate::states::base::{InputEvent, Key, Mods, State};
use freight_fate::states::update::{
    UpdateDownloadState, MANUAL_DOWNLOAD, STALL_TIMEOUT_S, UNPACK_TIMEOUT_S,
};
use freight_fate::updater::{self, Platform, UpdateInfo, UpdaterEnv};

fn snapshot_info() -> UpdateInfo {
    UpdateInfo {
        tag: "1.9-tester-20261001".to_string(),
        title: "Freight Fate 1.9 tester snapshot 2026-10-01".to_string(),
        notes: vec![],
        asset_name: "FreightFate-1.9-tester-20261001-macos-arm64.zip".to_string(),
        asset_url: "https://example.test/a".to_string(),
        asset_size: 419_438_969,
        asset_sha256: String::new(),
        slim: None,
    }
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
    let new_root = tmp.path().join("FreightFate.app");
    fs::create_dir(&new_root).unwrap();
    let mut state = UpdateDownloadState::finished_with(
        snapshot_info(),
        tmp.path().to_path_buf(),
        new_root.clone(),
        Box::new(|_| true),
        Box::new(|root| root.to_path_buf()),
    )
    .with_apply(Box::new(|_, _| {
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
fn a_smaller_update_saved_for_a_manual_install_says_to_copy_it_over() {
    // The smaller archive has no music or radio packs: set up as a fresh
    // install it would have none.
    let mut app = TestApp::new();
    app.push_state(base_screen());
    app.push_state(base_screen());
    let tmp = tempfile::tempdir().unwrap();
    let new_root = tmp.path().join("FreightFate");
    fs::create_dir(&new_root).unwrap();
    let mut state = UpdateDownloadState::finished_with(
        snapshot_info(),
        tmp.path().to_path_buf(),
        new_root.clone(),
        Box::new(|_| true),
        Box::new(|root| root.to_path_buf()),
    )
    .leaving_out_packs()
    .with_apply(Box::new(|_, _| {
        Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "no shell",
        ))
    }));
    app.ctx.running = true;
    app.clear_speech();

    state.update(&mut app.ctx, 0.0);
    app.ctx.run_deferred();

    let said = app.main_lines().join(" ");
    assert!(said.contains("Copy it over your game folder"), "{said}");
    assert!(!said.contains("Install it yourself"), "{said}");
    app.shutdown();
}

#[test]
fn a_started_apply_script_restarts_the_game() {
    let mut app = TestApp::new();
    app.push_state(base_screen());
    let tmp = tempfile::tempdir().unwrap();
    let new_root = tmp.path().join("FreightFate.app");
    fs::create_dir(&new_root).unwrap();
    let mut state = UpdateDownloadState::finished_with(
        snapshot_info(),
        tmp.path().to_path_buf(),
        new_root,
        Box::new(|_| true),
        Box::new(|root| root.to_path_buf()),
    )
    .with_apply(Box::new(|_, _| Ok(())));
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

fn mac_env(executable: &Path) -> UpdaterEnv {
    UpdaterEnv::fake(Platform::MacOs, executable)
}

fn fake_bundle(parent: &Path) -> PathBuf {
    let macos = parent
        .join("FreightFate.app")
        .join("Contents")
        .join("MacOS");
    fs::create_dir_all(&macos).unwrap();
    let exe = macos.join("FreightFate");
    fs::write(&exe, b"").unwrap();
    exe
}

#[test]
fn a_mac_bundle_in_a_writable_folder_updates_itself() {
    let tmp = tempfile::tempdir().unwrap();
    let exe = fake_bundle(tmp.path());
    let update = tmp.path().join("staging").join("FreightFate.app");
    fs::create_dir_all(&update).unwrap();
    assert!(updater::can_auto_apply(&update, &mac_env(&exe)));
}

#[test]
fn a_translocated_mac_bundle_cannot_update_itself() {
    // An app opened straight out of a quarantined download runs from a
    // read-only copy that disappears when it quits; swapping it would
    // relaunch nothing.
    let tmp = tempfile::tempdir().unwrap();
    let exe = fake_bundle(&tmp.path().join("AppTranslocation").join("ABCD").join("d"));
    let update = tmp.path().join("staging").join("FreightFate.app");
    fs::create_dir_all(&update).unwrap();
    assert!(updater::is_translocated(&exe));
    assert!(!updater::can_auto_apply(&update, &mac_env(&exe)));
    assert!(!updater::is_translocated(Path::new(
        "/Applications/FreightFate.app"
    )));
}

#[cfg(unix)]
#[test]
fn a_mac_bundle_in_a_read_only_folder_cannot_update_itself() {
    use std::os::unix::fs::PermissionsExt;
    let tmp = tempfile::tempdir().unwrap();
    let locked = tmp.path().join("locked");
    let exe = fake_bundle(&locked);
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o555)).unwrap();
    // Root ignores permissions, so the probe proves nothing there.
    let probe = locked.join(".probe");
    let root = fs::write(&probe, b"").is_ok();
    let _ = fs::remove_file(&probe);
    if !root {
        assert!(!updater::macos_bundle_swappable(
            &locked.join("FreightFate.app")
        ));
        let update = tmp.path().join("staging").join("FreightFate.app");
        fs::create_dir_all(&update).unwrap();
        assert!(!updater::can_auto_apply(&update, &mac_env(&exe)));
    }
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();
}

#[test]
fn a_parked_mac_bundle_moves_home_and_never_replaces_one_there() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    fs::create_dir(&home).unwrap();
    let bundle = tmp.path().join("staging").join("FreightFate.app");
    fs::create_dir_all(bundle.join("Contents")).unwrap();

    let dest = updater::stash_for_manual_install(&bundle, Some(&home));
    assert_eq!(dest, home.join("FreightFate.app"));
    assert!(dest.join("Contents").is_dir());
    assert!(!bundle.exists());

    // A second bundle must not delete the one already in the home folder.
    let again = tmp.path().join("staging2").join("FreightFate.app");
    fs::create_dir_all(&again).unwrap();
    assert_eq!(
        updater::stash_for_manual_install(&again, Some(&home)),
        again
    );
    assert!(dest.join("Contents").is_dir());
}

#[cfg(unix)]
#[test]
fn the_unpacker_is_killed_at_its_deadline() {
    let started = Instant::now();
    let mut slow = std::process::Command::new("sleep");
    slow.arg("30");
    let err = updater::run_bounded(&mut slow, Duration::from_millis(200), None).unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::TimedOut);
    assert!(started.elapsed() < Duration::from_secs(10));
}

#[cfg(unix)]
#[test]
fn the_unpacker_is_killed_on_cancel() {
    let cancelled = AtomicBool::new(true);
    let started = Instant::now();
    let mut slow = std::process::Command::new("sleep");
    slow.arg("30");
    let err =
        updater::run_bounded(&mut slow, Duration::from_secs(60), Some(&cancelled)).unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::Interrupted);
    assert!(started.elapsed() < Duration::from_secs(10));
}

#[cfg(unix)]
#[test]
fn a_failing_unpacker_is_an_error_not_a_hang() {
    let mut failing = std::process::Command::new("false");
    assert!(updater::run_bounded(&mut failing, Duration::from_secs(10), None).is_err());
    let mut fine = std::process::Command::new("true");
    assert!(updater::run_bounded(&mut fine, Duration::from_secs(10), None).is_ok());
}

/// A local server that sends `sent` bytes, then holds the connection open
/// and silent until the test ends -- a transfer stalled mid-body.
fn stalled_server(sent: usize) -> (std::net::TcpStream, std::thread::JoinHandle<()>) {
    use std::io::Write;
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let (mut conn, _) = listener.accept().unwrap();
        conn.write_all(&vec![7u8; sent]).unwrap();
        conn.flush().unwrap();
        // Hold the socket open, sending nothing, until the client hangs up
        // or the test process ends.
        let mut sink = [0u8; 1];
        let _ = std::io::Read::read(&mut conn, &mut sink);
    });
    let client = std::net::TcpStream::connect(addr).unwrap();
    (client, server)
}

#[test]
fn a_stalled_transfer_fails_within_the_idle_window() {
    let (client, _server) = stalled_server(5000);
    let tmp = tempfile::tempdir().unwrap();
    let file = fs::File::create(tmp.path().join("part.zip")).unwrap();
    let mut seen = 0u64;
    let mut progress = |done: u64, _total: u64| seen = done;
    let started = Instant::now();

    let err = updater::stream_to_file(
        client,
        file,
        419_438_969,
        Some(&mut progress),
        None,
        Duration::from_millis(400),
    )
    .unwrap_err();

    assert!(matches!(err, updater::DownloadError::Stalled(_)), "{err:?}");
    assert_eq!(seen, 5000, "the bytes before the stall still count");
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "{:?}",
        started.elapsed()
    );
}

#[test]
fn a_cancel_returns_promptly_from_a_blocked_transfer() {
    let (client, _server) = stalled_server(10);
    let tmp = tempfile::tempdir().unwrap();
    let file = fs::File::create(tmp.path().join("part.zip")).unwrap();
    let cancelled = std::sync::Arc::new(AtomicBool::new(false));
    let flag = std::sync::Arc::clone(&cancelled);
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(200));
        flag.store(true, std::sync::atomic::Ordering::SeqCst);
    });
    let started = Instant::now();

    let err = updater::stream_to_file(
        client,
        file,
        100,
        None,
        Some(&cancelled),
        Duration::from_secs(600),
    )
    .unwrap_err();

    assert!(matches!(err, updater::DownloadError::Cancelled), "{err:?}");
    assert!(
        started.elapsed() < Duration::from_secs(3),
        "{:?}",
        started.elapsed()
    );
}

#[test]
fn a_whole_transfer_is_written_and_hashed() {
    let body = b"freight fate".repeat(20_000);
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("whole.zip");
    let file = fs::File::create(&path).unwrap();
    let digest = updater::stream_to_file(
        std::io::Cursor::new(body.clone()),
        file,
        body.len() as u64,
        None,
        None,
        Duration::from_secs(5),
    )
    .unwrap();
    assert_eq!(fs::read(&path).unwrap(), body);
    use sha2::Digest;
    let expected: String = sha2::Sha256::digest(&body)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    assert_eq!(digest, expected);
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
    let (mut app, mut state) = run_worker(Box::new(|info, dir, progress, _cancelled| {
        fs::write(dir.join(&info.asset_name), vec![0u8; 4096]).unwrap();
        progress(4096, 419_438_969);
        Err(updater::DownloadError::Stalled(Duration::from_secs(60)))
    }));
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
    let (app, state) = run_worker(Box::new(|info, dir, _progress, cancelled| {
        fs::write(dir.join(&info.asset_name), vec![0u8; 4096]).unwrap();
        cancelled.store(true, std::sync::atomic::Ordering::SeqCst);
        Err(updater::DownloadError::Cancelled)
    }));
    let staging = state.staging().expect("a staging folder was made");
    assert!(!staging.exists(), "partial download left in {staging:?}");
    assert!(state.error().is_none(), "nothing to say: the player left");
    let mut app = app;
    app.shutdown();
}

#[test]
fn a_damaged_transfer_is_refused() {
    let (mut app, mut state) = run_worker(Box::new(|_info, _dir, _progress, _cancelled| {
        Err(updater::DownloadError::Corrupt)
    }));
    app.clear_speech();
    state.update(&mut app.ctx, 0.1);
    let said = app.main_lines().join(" ");
    assert!(said.contains("arrived damaged"), "{said}");
    app.shutdown();
}

#[test]
fn the_release_digest_travels_with_the_update() {
    let release = serde_json::json!({
        "tag_name": "1.9-tester-20261001",
        "prerelease": true,
        "body": "",
        "published_at": "2026-10-01T03:24:08Z",
        "assets": [{
            "name": "FreightFate-1.9-tester-20261001-macos-arm64.zip",
            "browser_download_url": "https://example.test/mac.zip",
            "size": 419_438_969,
            "digest": "sha256:20DADBF941750030688722278586CD4488F192D4398FE736822742A38CB1FDE9",
        }],
    });
    let env = UpdaterEnv::fake_with_architecture(
        Platform::MacOs,
        updater::Architecture::Aarch64,
        Path::new("/Applications/FreightFate.app/Contents/MacOS/FreightFate"),
    );
    let build = updater::BuildInfo::new("1.9-tester-20260930", "dev", "2026-09-30");
    let info =
        updater::snapshot_update_from(&[release], Some(&build), "1.9.0", None, &env).unwrap();
    assert_eq!(
        info.asset_sha256,
        "20dadbf941750030688722278586cd4488f192d4398fe736822742a38cb1fde9"
    );
}
