//! The updater's Freight Fate side: the build identity, channels, the
//! mirror, where updates are published, and the update screens around the
//! engine's checks (`hearken-update`, which has its own tests for the
//! download, the unpack and the swap).

use std::fs;
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::Mutex;

use serde_json::json;

use freight_fate::app::testing::TestApp;
use freight_fate::states::base::Menu;
use freight_fate::states::main_menu::SettingsCategoryState;
use freight_fate::states::update::UpdatePromptState;
use freight_fate::updater::{
    self, build_info_from_dict, mirror_url, normalize_mirror_prefix, resolve_channel, BuildInfo,
    Platform, UpdateInfo, UpdaterEnv,
};
use hearken_update::{
    Fetch, FetchError, Found, Host, Install, Manifest, Os, Query, Running, SecretKey, SourceError,
    Update, UpdateSource,
};

fn env() -> UpdaterEnv {
    UpdaterEnv::fake(Platform::current(), Path::new("/tmp/not-frozen/python"))
}

/// A manifest for `version` on `channel`, as a release would sign it.
fn manifest(version: &str, channel: &str) -> Manifest {
    Manifest {
        format: 1,
        game: updater::GAME.into(),
        version: version.into(),
        channel: channel.into(),
        platform: "windows-x64".into(),
        archive: format!("FreightFate-{version}-windows-portable.zip"),
        url: None,
        size: 1_000_000,
        sha256: "ab".repeat(32),
        notes: "## Changes\n- **Cruise control.** K sets cruise.".into(),
        published: None,
        index: None,
    }
}

/// An update to `version`, as the check would hand it to the screens.
fn info(version: &str) -> UpdateInfo {
    UpdateInfo::new(Update {
        manifest: manifest(version, "stable"),
        archive_url: "https://example.test/FreightFate.zip".into(),
        index_url: None,
        plan: None,
    })
}

#[test]
fn test_spoken_version_translates_dev_suffix() {
    assert_eq!(
        updater::spoken_version("1.8.6.dev0"),
        "1.8.6 development build"
    );
    assert_eq!(updater::spoken_version("1.8.5"), "1.8.5");
}

#[test]
fn test_resolve_channel_prefers_explicit_setting() {
    let nightly = BuildInfo::new("nightly-20260610", "dev", "2026-06-10");
    assert_eq!(resolve_channel("stable", Some(&nightly)), "stable");
    assert_eq!(resolve_channel("dev", None), "dev");
}

#[test]
fn test_resolve_channel_follows_build_when_unset() {
    let nightly = BuildInfo::new("nightly-20260610", "dev", "2026-06-10");
    assert_eq!(resolve_channel("", Some(&nightly)), "dev");
    assert_eq!(resolve_channel("", None), "stable");
}

#[test]
fn test_update_mirror_prefix_is_https_and_normalized() {
    assert_eq!(
        normalize_mirror_prefix(" https://gh-proxy.org/// "),
        Ok("https://gh-proxy.org/".to_string())
    );
    assert!(normalize_mirror_prefix("http://gh-proxy.org/").is_err());
    assert!(normalize_mirror_prefix("https://gh-proxy.org/?token=x").is_err());
    assert!(normalize_mirror_prefix("").is_err());
}

#[test]
fn test_update_mirror_prepends_original_github_url() {
    let github = "https://github.com/orinks-games/Freight-Fate/releases/download/v1/file.zip";
    assert_eq!(
        mirror_url(github, Some("https://gh-proxy.org")),
        Ok(format!("https://gh-proxy.org/{github}"))
    );
    assert_eq!(mirror_url(github, None), Ok(github.to_string()));
}

#[test]
fn test_update_mirror_setting_reveals_labeled_prefix_field() {
    let mut app = TestApp::new();
    let mut state = SettingsCategoryState::new("updates");

    let direct = state.build_items(&mut app.ctx);
    let direct_labels: Vec<String> = direct
        .iter()
        .map(|item| item.text(&state, &app.ctx))
        .collect();
    assert!(!direct_labels
        .iter()
        .any(|label| label.starts_with("GitHub mirror prefix:")));

    app.ctx.settings.update_mirror_enabled = true;
    app.ctx.settings.update_mirror_prefix = "https://gh-proxy.org/".to_string();
    let mirrored = state.build_items(&mut app.ctx);
    let prefix = mirrored
        .iter()
        .find(|item| {
            item.text(&state, &app.ctx)
                .starts_with("GitHub mirror prefix:")
        })
        .expect("enabled mirror has a prefix field");
    assert_eq!(
        prefix.text(&state, &app.ctx),
        "GitHub mirror prefix: https://gh-proxy.org/"
    );
    assert_eq!(
        prefix.help_text(&state, &app.ctx),
        "The mirror's address, put in front of every GitHub link. Use a mirror you trust: updates download through it."
    );
    app.shutdown();
}

#[test]
fn test_build_info_malformed_falls_back_to_stable_version() {
    assert_eq!(
        build_info_from_dict(&json!([]), "1.6.0"),
        BuildInfo::new("v1.6.0", "stable", "")
    );
}

#[test]
fn test_build_info_stamp_marks_stable_and_nightly_channels() {
    // `stamp_build_info` (cargo tools build-release) writes tag, channel and
    // built_at; the reader derives the channel from the tag when the stamp
    // leaves it out or garbles it.
    let stable = build_info_from_dict(
        &json!({"tag": "v1.6.0", "channel": "stable", "built_at": "2026-06-15"}),
        "1.6.0",
    );
    assert_eq!(stable.tag, "v1.6.0");
    assert_eq!(stable.channel, "stable");
    assert!(!stable.built_at.is_empty());

    let nightly = build_info_from_dict(
        &json!({"tag": "nightly-20260615", "channel": "weird", "built_at": "2026-06-15"}),
        "1.6.0",
    );
    assert_eq!(nightly.tag, "nightly-20260615");
    assert_eq!(nightly.channel, "dev");
    assert!(!nightly.built_at.is_empty());

    let tester = build_info_from_dict(
        &json!({"tag": "1.9-tester-20260828", "built_at": "2026-08-28"}),
        "1.9.0",
    );
    assert_eq!(tester.tag, "1.9-tester-20260828");
    assert_eq!(tester.channel, "dev");
}

#[test]
fn test_nuitka_standalone_folder_counts_as_packaged_build() {
    let dir = tempfile::tempdir().unwrap();
    let exe = dir.path().join("FreightFate.exe");
    fs::write(&exe, "").unwrap();
    fs::create_dir(dir.path().join("freight_fate")).unwrap();
    let env = UpdaterEnv::fake(Platform::current(), &exe);

    assert!(updater::is_frozen_in(&env));
    assert_eq!(
        updater::load_build_info_in(&env, "1.6.0"),
        Some(BuildInfo::new("v1.6.0", "stable", ""))
    );
    fs::write(
        dir.path().join("build_info.json"),
        json!({"tag": "nightly-20260615", "channel": "dev", "built_at": "2026-06-15"}).to_string(),
    )
    .unwrap();
    assert_eq!(
        updater::load_build_info_in(&env, "1.6.0"),
        Some(BuildInfo::new("nightly-20260615", "dev", "2026-06-15"))
    );
}

#[test]
fn test_macos_build_info_is_loaded_from_app_resources() {
    let tmp = tempfile::tempdir().unwrap();
    let exe = tmp
        .path()
        .join("FreightFate.app")
        .join("Contents")
        .join("MacOS")
        .join("FreightFate");
    fs::create_dir_all(exe.parent().unwrap()).unwrap();
    fs::write(&exe, b"Mach-O").unwrap();
    let resources = exe.parent().unwrap().parent().unwrap().join("Resources");
    fs::create_dir_all(&resources).unwrap();
    fs::write(
        resources.join("build_info.json"),
        r#"{"tag":"1.9-tester-20260830","channel":"dev","built_at":"2026-08-30"}"#,
    )
    .unwrap();
    let env = UpdaterEnv::fake(Platform::MacOs, &exe);

    let info = updater::load_build_info_in(&env, "1.9.0").unwrap();

    assert_eq!(info.tag, "1.9-tester-20260830");
    assert_eq!(info.channel, "dev");
}

#[test]
fn test_build_info_none_when_not_frozen() {
    assert!(!updater::is_frozen_in(&env()));
    assert!(updater::load_build_info_in(&env(), "1.6.0").is_none());
    // The test binary is not a packaged build either.
    assert!(!updater::is_frozen());
    assert!(updater::load_build_info("1.6.0").is_none());
}

#[test]
fn test_manual_update_check_explains_source_builds() {
    use freight_fate::app::testing::TestApp;
    use freight_fate::states::update::UpdateCheckState;

    // The test binary is never a packaged build, so `is_frozen()` is false
    // the way the Python test's monkeypatch made it.
    assert!(!updater::is_frozen());
    let mut app = TestApp::new();
    app.push_state(UpdateCheckState::new());
    let message = {
        let state = app.state().unwrap();
        let state = state.borrow();
        let check = state.as_any().downcast_ref::<UpdateCheckState>().unwrap();
        assert!(check.checker.is_none());
        check.message.clone()
    };
    assert!(message.contains("This copy runs from source; update it with git."));
    assert_eq!(app.main_lines(), vec![message]);
    app.shutdown();
}

#[test]
fn test_packaged_logging_writes_info_to_game_log() {
    // Python forced `updater.is_frozen()` true and pointed `game_root` at a
    // temp dir. Rust has no seam for either, and `configure_logging` can only
    // configure once per process anyway. `FREIGHT_FATE_LOG_FILE` reaches the
    // same code by the door the playtest sessions use: file output at INFO
    // from a source checkout, which is exactly what the packaged branch asks
    // for. What this pins is that an INFO line reaches the file rather than
    // being filtered away as it would be from a plain source run.
    let _guard = freight_fate::app::testing::env_lock();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("logs").join("game.log");
    std::env::remove_var("FREIGHT_FATE_LOG");
    std::env::set_var("FREIGHT_FATE_LOG_FILE", &path);

    freight_fate::app::logging::configure_logging();
    log::info!(target: "freight_fate::speech", "Speech backend: Speech Dispatcher");
    log::logger().flush();

    std::env::remove_var("FREIGHT_FATE_LOG_FILE");
    let text = fs::read_to_string(&path).expect("the log file was created");
    assert!(text.contains("Speech backend: Speech Dispatcher"), "{text}");
}

/// The main menu's session update checker is one process-wide static, so the
/// tests that install into it run one at a time.
static UPDATE_CHECK_TESTS: Mutex<()> = Mutex::new(());

#[test]
fn test_startup_update_prompt_respects_skipped_version() {
    let _serial = UPDATE_CHECK_TESTS.lock().unwrap_or_else(|e| e.into_inner());
    use freight_fate::app::testing::TestApp;
    use freight_fate::states::main_menu::MainMenuState;
    use freight_fate::states::update::UpdateChecker;

    let info = info("1.6.1");
    let mut app = TestApp::new();
    app.ctx.settings.skipped_update = "1.6.1".to_string();
    app.push_state(MainMenuState::new());
    let depth = app.ctx.stack_len();
    MainMenuState::install_update_check(Some(UpdateChecker::finished(Some(info), None)), false);
    app.tick(0.0);
    MainMenuState::install_update_check(None, false);
    // The skipped version raises no prompt.
    assert_eq!(app.ctx.stack_len(), depth);
    app.shutdown();
}

#[test]
fn test_terminal_exit_does_not_check_for_updates_from_source() {
    let _serial = UPDATE_CHECK_TESTS.lock().unwrap_or_else(|e| e.into_inner());
    use freight_fate::app::testing::TestApp;
    use freight_fate::states::main_menu::MainMenuState;

    // From source there is nothing to update to, so arming is a no-op and
    // the session keeps whatever checker it already had (here: none).
    let app = TestApp::new();
    assert!(!updater::is_frozen());
    MainMenuState::install_update_check(None, true);
    MainMenuState::arm_update_check(&app.ctx.settings);
    assert_eq!(MainMenuState::update_check_status(), (false, true));
    MainMenuState::install_update_check(None, false);
    drop(app);
}

#[test]
fn test_remind_later_help_describes_terminal_exit_check() {
    let mut app = TestApp::new();
    let info = info("1.6.1");
    let mut state = UpdatePromptState::new(info);
    let items = state.build_items(&mut app.ctx);
    let (label, help) = items
        .iter()
        .map(|item| {
            (
                item.text(&state, &app.ctx),
                item.help_text(&state, &app.ctx),
            )
        })
        .find(|(text, _)| text == "Remind me later")
        .expect("the prompt offers Remind me later");
    assert_eq!(label, "Remind me later");
    assert!(
        help.contains("from a terminal or pickup facility"),
        "{help}"
    );
    assert!(help.contains("next time the game starts"), "{help}");
    app.shutdown();
}

// -- the engine side ------------------------------------------------------------

#[test]
fn an_update_is_named_and_its_notes_are_speakable() {
    let stable = info("1.9.6");
    assert_eq!(stable.title, "Freight Fate version 1.9.6");
    assert_eq!(stable.tag, "1.9.6");
    assert_eq!(stable.notes, ["Changes", "Cruise control. K sets cruise."]);
    assert_eq!(stable.download_size(), 1_000_000);
    assert_eq!(
        updater::update_title("1.9.6-dev.202610300537"),
        "Freight Fate 1.9 tester snapshot 2026-10-30"
    );
}

#[test]
fn an_appimage_copy_follows_its_own_platform() {
    let tmp = tempfile::tempdir().unwrap();
    let appimage = tmp.path().join("FreightFate-linux-x86_64.AppImage");
    fs::write(&appimage, b"\x7fELF").unwrap();
    let running = Running {
        key: SecretKey::generate().unwrap().public(),
        game: updater::GAME.into(),
        version: "1.9.6".into(),
        channel: "stable".into(),
        platform: "linux-x64".into(),
    };
    let mut host = Host::fake(Os::Linux, &tmp.path().join("FreightFate"));
    let folder = updater::running_for(running.clone(), &host).unwrap();
    assert_eq!(folder.platform, "linux-x64");
    host.appimage = Some(appimage);
    let image = updater::running_for(running, &host).unwrap();
    assert_eq!(image.platform, "linux-x64-appimage");
    // Asked twice, still one suffix.
    assert_eq!(
        updater::running_for(image, &host).unwrap().platform,
        "linux-x64-appimage"
    );
}

#[test]
fn a_test_build_never_updates_itself() {
    // Tests are built without the update key, as any `cargo run` is.
    assert_eq!(updater::running(), None);
    let host = Host::current();
    let why = updater::blocked(None, &host).expect("blocked");
    assert!(
        updater::blocked_line(why).contains("packaged game") || updater::managed_by_agnow(),
        "{why:?}"
    );
    let never = AtomicBool::new(false);
    let err = updater::check_for_update("stable", None, &never).unwrap_err();
    assert!(err.contains("This copy runs from source") || updater::managed_by_agnow());
}

/// Hands out the manifest it holds and counts what was asked of it.
struct OneManifest {
    bytes: Vec<u8>,
    asked: Mutex<Vec<Query>>,
}

impl UpdateSource for OneManifest {
    fn find(&self, query: &Query, _: &dyn Fetch) -> Result<Option<Found>, SourceError> {
        self.asked.lock().unwrap().push(query.clone());
        Ok(Some(Found {
            manifest: self.bytes.clone(),
            location: "https://example.test/releases/update-stable-windows-x64.json".into(),
            files: Vec::new(),
        }))
    }

    fn describe(&self) -> String {
        "a test source".into()
    }
}

/// Fails every request: the checks here read no file but the manifest.
struct NoNetwork;

impl Fetch for NoNetwork {
    fn get(&self, url: &str, _: &[(&str, &str)], _: u64) -> Result<Vec<u8>, FetchError> {
        panic!("fetched {url}");
    }

    fn open(&self, url: &str) -> Result<hearken_update::Body, FetchError> {
        panic!("opened {url}");
    }
}

fn running_on(key: &SecretKey, version: &str) -> Running {
    Running {
        key: key.public(),
        game: updater::GAME.into(),
        version: version.into(),
        channel: "stable".into(),
        platform: "windows-x64".into(),
    }
}

#[test]
fn a_signed_newer_release_is_offered_and_nothing_else() {
    let key = SecretKey::generate().unwrap();
    let signed = hearken_update::manifest::sign(&manifest("1.9.6", "stable"), &key).unwrap();
    let source = OneManifest {
        bytes: signed.into_bytes(),
        asked: Mutex::new(Vec::new()),
    };
    let install = Install::Folder {
        dir: std::env::temp_dir(),
        exe: "FreightFate.exe".into(),
    };
    let never = AtomicBool::new(false);

    let found = updater::check_with(
        &source,
        &NoNetwork,
        &running_on(&key, "1.9.5"),
        &install,
        "stable",
        &never,
    )
    .unwrap()
    .expect("1.9.6 is newer");
    assert_eq!(found.title, "Freight Fate version 1.9.6");
    assert_eq!(
        found.update.archive_url,
        "https://example.test/releases/FreightFate-1.9.6-windows-portable.zip"
    );
    assert_eq!(source.asked.lock().unwrap()[0].game, "freight-fate");

    // The same version, or an older one, is never offered.
    for current in ["1.9.6", "1.9.7"] {
        let none = updater::check_with(
            &source,
            &NoNetwork,
            &running_on(&key, current),
            &install,
            "stable",
            &never,
        );
        assert_eq!(none, Ok(None), "{current}");
    }

    // Signed with any other key, it is refused and said so.
    let stranger = SecretKey::generate().unwrap();
    let refused = updater::check_with(
        &source,
        &NoNetwork,
        &running_on(&stranger, "1.9.5"),
        &install,
        "stable",
        &never,
    )
    .unwrap_err();
    assert!(refused.contains("not signed by Freight Fate"), "{refused}");
}

#[test]
fn snapshots_sort_above_the_stable_they_follow_and_below_the_next() {
    use hearken_update::Version;
    let v = |s: &str| Version::parse(s).unwrap();
    assert!(v("1.9.6-dev.202610300537").is_newer_than(&v("1.9.5")));
    assert!(v("1.9.6-dev.202610300837").is_newer_than(&v("1.9.6-dev.202610300537")));
    assert!(v("1.9.6").is_newer_than(&v("1.9.6-dev.202610300837")));
}

#[test]
fn stable_and_snapshot_checks_go_to_their_own_places() {
    let source = updater::ReleaseSource::new();
    assert_eq!(source.describe(), "github.com/orinks-games/Freight-Fate");
    assert!(updater::STABLE_MANIFESTS
        .starts_with("https://github.com/orinks-games/Freight-Fate/releases/latest/download/"));
}
