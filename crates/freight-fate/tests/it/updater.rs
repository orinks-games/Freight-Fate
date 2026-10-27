//! Port of `tests/test_updater.py`: update discovery, channel resolution,
//! notes flattening, apply scripts.

use std::cell::RefCell;
use std::fs;
use std::path::Path;

use serde_json::{json, Value};

use freight_fate::app::testing::TestApp;
use freight_fate::net::NetError;
use freight_fate::states::base::{Menu, State};
use freight_fate::states::main_menu::SettingsCategoryState;
use freight_fate::states::update::{UpdateDownloadState, UpdatePromptState};
use freight_fate::updater::{
    self, build_info_from_dict, check_for_update_with, dev_update_from, flatten_markdown,
    mirror_url, normalize_mirror_prefix, parse_version, pick_asset, quote_for_script,
    resolve_channel, snapshot_update_from, stable_update_from, write_apply_script, Architecture,
    BuildInfo, Platform, UpdateInfo, UpdaterEnv, APPIMAGE_SUFFIX, APPIMAGE_SUFFIX_AARCH64,
    TARBALL_SUFFIX, TARBALL_SUFFIX_AARCH64,
};

fn release_with(
    tag: &str,
    prerelease: bool,
    body: &str,
    published: &str,
    assets: &[&str],
) -> Value {
    json!({
        "tag_name": tag,
        "prerelease": prerelease,
        "body": body,
        "published_at": published,
        "assets": assets.iter().map(|suffix| json!({
            "name": format!("FreightFate-{tag}{suffix}"),
            "browser_download_url": format!("https://example.test/{tag}/{suffix}"),
            "size": 50_000_000,
        })).collect::<Vec<_>>(),
    })
}

/// Every archive a release carries, one per platform and CPU, so a test
/// that runs with the host's own architecture finds its download on any
/// runner. The AppImages are added by the tests that need them.
const ALL_ASSETS: [&str; 5] = [
    "-windows-portable.zip",
    "-macos.zip",
    "-macos-arm64.zip",
    "-linux-x64.tar.gz",
    "-linux-arm64.tar.gz",
];

fn release(tag: &str) -> Value {
    release_with(tag, false, "", "", &ALL_ASSETS)
}

fn nightly(tag: &str) -> Value {
    release_with(tag, true, "", "", &ALL_ASSETS)
}

fn tester(tag: &str) -> Value {
    release_with(tag, true, "", "", &ALL_ASSETS)
}

fn stable_at(tag: &str, published: &str) -> Value {
    release_with(tag, false, "", published, &ALL_ASSETS)
}

/// The environment the Python tests ran under: this machine's platform, an
/// executable that is not a packaged build, no APPIMAGE.
fn env() -> UpdaterEnv {
    UpdaterEnv::fake(Platform::current(), Path::new("/tmp/not-frozen/python"))
}

fn env_on(platform: Platform) -> UpdaterEnv {
    UpdaterEnv::fake(platform, Path::new("/tmp/not-frozen/python"))
}

fn mac_env(architecture: Architecture) -> UpdaterEnv {
    UpdaterEnv::fake_with_architecture(
        Platform::MacOs,
        architecture,
        Path::new("/tmp/not-frozen/FreightFate"),
    )
}

fn fake_appimage(dir: &Path) -> std::path::PathBuf {
    let appimage = dir.join("FreightFate-1.8.5-linux-x86_64.AppImage");
    fs::write(&appimage, b"old").unwrap();
    appimage
}

fn linux_appimage_env(appimage: Option<&Path>) -> UpdaterEnv {
    linux_appimage_env_on(Architecture::X86_64, appimage)
}

fn linux_appimage_env_on(architecture: Architecture, appimage: Option<&Path>) -> UpdaterEnv {
    let mut env = UpdaterEnv::fake_with_architecture(
        Platform::Linux,
        architecture,
        Path::new("/tmp/not-frozen/python"),
    );
    env.appimage = appimage.map(|p| p.display().to_string());
    env
}

// -- version parsing and channels --------------------------------------------

#[test]
fn test_parse_version_orders_semver() {
    assert!(parse_version("v1.6.0") > parse_version("1.5.0"));
    assert!(parse_version("1.10.0") > parse_version("1.9.3"));
    assert_eq!(parse_version("garbage"), vec![0]);
}

#[test]
fn test_parse_version_orders_dev_prereleases() {
    // A dev checkout sits between the previous stable and the release it
    // works toward, so promoting dev to stable is offered as an update.
    assert!(parse_version("1.8.6.dev0") < parse_version("v1.8.6"));
    assert!(parse_version("1.8.6.dev0") > parse_version("v1.8.5"));
    assert!(parse_version("1.8.6.dev1") > parse_version("1.8.6.dev0"));
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

// -- stable channel -----------------------------------------------------------

#[test]
fn test_stable_update_found_when_newer() {
    let info = stable_update_from(
        &release_with("v9.9.9", false, "- Big stuff", "", &ALL_ASSETS),
        "1.5.0",
        &env(),
    )
    .expect("an update");
    assert_eq!(info.tag, "v9.9.9");
    assert!(info.title.contains("9.9.9"));
    assert_eq!(info.notes, vec!["Big stuff"]);
    assert!(info.asset_url.starts_with("https://example.test/"));
}

#[test]
fn test_stable_no_update_when_current_or_older() {
    assert!(stable_update_from(&release("v1.5.0"), "1.5.0", &env()).is_none());
    assert!(stable_update_from(&release("v1.4.0"), "1.5.0", &env()).is_none());
}

#[test]
fn test_stable_no_update_without_platform_asset() {
    assert!(
        stable_update_from(&release_with("v9.9.9", false, "", "", &[]), "1.5.0", &env()).is_none()
    );
}

#[test]
fn test_apple_silicon_macos_selects_only_arm64_archive() {
    let release = release_with(
        "1.9-tester-20260830",
        true,
        "",
        "",
        &["-macos.zip", "-macos-arm64.zip"],
    );
    let asset = pick_asset(&release, None, &mac_env(Architecture::Aarch64)).unwrap();
    assert!(asset.0.ends_with("-macos-arm64.zip"));
}

#[test]
fn test_intel_macos_does_not_offer_arm64_archive() {
    let release = release_with(
        "1.9-tester-20260830",
        true,
        "",
        "",
        &["-macos.zip", "-macos-arm64.zip"],
    );
    assert!(pick_asset(&release, None, &mac_env(Architecture::X86_64)).is_none());
}

#[test]
fn test_stable_macos_keeps_legacy_archive_contract_on_both_architectures() {
    let release = release_with("v1.8.8", false, "", "", &["-macos.zip"]);
    for architecture in [Architecture::Aarch64, Architecture::X86_64] {
        let asset = pick_asset(&release, None, &mac_env(architecture)).unwrap();
        assert!(asset.0.ends_with("-macos.zip"));
    }
}

#[test]
fn test_shared_stable_fixture_offers_legacy_archive_on_apple_silicon_macos() {
    let info = stable_update_from(&release("v1.8.8"), "1.8.7", &mac_env(Architecture::Aarch64))
        .expect("the shared stable fixture must include its legacy Mac archive");
    assert!(info.asset_name.ends_with("-macos.zip"));
}

#[test]
fn test_stable_channel_ignores_newer_19_tester_prerelease() {
    // The stable channel reads GitHub's latest stable endpoint, not the
    // prerelease list shared by both snapshot families.
    let api = |path: &str| -> Result<Value, NetError> {
        match path {
            "/releases/latest" => Ok(stable_at("v1.8.8.1", "2026-08-08T15:00:00Z")),
            "/releases?per_page=100&page=1" => Ok(json!([
                tester("1.9-tester-20260829"),
                stable_at("v1.8.8.1", "2026-08-08T15:00:00Z"),
            ])),
            other => panic!("unexpected path {other}"),
        }
    };

    let info = check_for_update_with("stable", "1.8.8", None, &env_on(Platform::Windows), &api)
        .unwrap()
        .expect("a stable update");
    assert_eq!(info.tag, "v1.8.8.1");
}

#[test]
fn test_stable_build_is_not_offered_its_own_release() {
    // The 1.9.0 stable shipped stamped `1.9.0.dev0` and tagged `vv1.9.0`, so
    // the version text said it was older than v1.9.0 and the updater offered
    // the same release again on every start.
    let api = |path: &str| -> Result<Value, NetError> {
        assert_eq!(path, "/releases/latest");
        Ok(release("v1.9.0"))
    };
    let env = env_on(Platform::Windows);
    for tag in ["v1.9.0", "vv1.9.0"] {
        let build = BuildInfo::new(tag, "stable", "2026-10-04");
        let found = check_for_update_with("stable", "1.9.0.dev0", Some(&build), &env, &api);
        assert_eq!(found.unwrap(), None, "{tag}");
    }

    // An older stable copy is still offered the release.
    let build = BuildInfo::new("v1.8.8.1", "stable", "2026-08-08");
    let info = check_for_update_with("stable", "1.8.8.1", Some(&build), &env, &api)
        .unwrap()
        .expect("a stable update");
    assert_eq!(info.tag, "v1.9.0");
}

// -- dev channel --------------------------------------------------------------

#[test]
fn test_dev_update_skips_non_nightlies_and_finds_newer() {
    let releases = vec![
        release("v1.5.0"), // stable, ignored
        nightly("nightly-20260611"),
        nightly("nightly-20260610"),
    ];
    let build = BuildInfo::new("nightly-20260610", "dev", "2026-06-10");
    let info = dev_update_from(&releases, Some(&build), None, &env()).expect("an update");
    assert_eq!(info.tag, "nightly-20260611");
    assert!(info.title.contains("2026-06-11"));
}

#[test]
fn test_shared_18_nightly_fixture_offers_legacy_archive_on_apple_silicon_macos() {
    let build = BuildInfo::new("nightly-20260610", "dev", "2026-06-10");
    let info = dev_update_from(
        &[nightly("nightly-20260611")],
        Some(&build),
        None,
        &mac_env(Architecture::Aarch64),
    )
    .expect("the shared 1.8 nightly fixture must include its legacy Mac archive");
    assert!(info.asset_name.ends_with("-macos.zip"));
}

#[test]
fn test_dev_update_sorts_nightlies_before_comparing() {
    let releases = vec![
        nightly("nightly-20260610"),
        nightly("nightly-20260612"),
        nightly("nightly-20260611"),
    ];
    let build = BuildInfo::new("nightly-20260611", "dev", "2026-06-11");
    let info = dev_update_from(&releases, Some(&build), None, &env()).expect("an update");
    assert_eq!(info.tag, "nightly-20260612");
}

#[test]
fn test_dev_no_update_when_on_latest_nightly() {
    let releases = vec![nightly("nightly-20260611")];
    let build = BuildInfo::new("nightly-20260611", "dev", "2026-06-11");
    assert!(dev_update_from(&releases, Some(&build), None, &env()).is_none());
}

#[test]
fn test_dev_update_uses_partial_nightly_build_info() {
    let build = build_info_from_dict(&json!({"tag": "nightly-20260611"}), "1.6.0");
    assert_eq!(build.channel, "dev");
    assert_eq!(build.tag, "nightly-20260611");

    let releases = vec![nightly("nightly-20260611"), nightly("nightly-20260610")];
    assert!(dev_update_from(&releases, Some(&build), None, &env()).is_none());
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
fn test_dev_stable_build_compares_by_build_date() {
    let releases = vec![nightly("nightly-20260611")];
    let older = BuildInfo::new("v1.5.0", "stable", "2026-06-01");
    let newer = BuildInfo::new("v1.6.0", "stable", "2026-06-11");
    assert!(dev_update_from(&releases, Some(&older), None, &env()).is_some());
    assert!(dev_update_from(&releases, Some(&newer), None, &env()).is_none());
}

#[test]
fn test_dev_steered_to_stable_when_it_postdates_newest_nightly() {
    // Dev work was promoted to stable this afternoon; the nightly the user is
    // on predates it. They should be offered stable, not left on nightlies.
    let stable = stable_at("v1.7.0", "2026-06-26T15:00:00Z");
    let releases = vec![stable.clone(), nightly("nightly-20260625")];
    let build = BuildInfo::new("nightly-20260625", "dev", "2026-06-25");
    let info = dev_update_from(&releases, Some(&build), Some(&stable), &env()).expect("an update");
    assert_eq!(info.tag, "v1.7.0");
    assert!(info.title.contains("1.7.0"));
}

#[test]
fn test_dev_ties_favor_stable_over_equivalent_nightly() {
    // Tonight's nightly is content-identical to the stable released earlier the
    // same day. A dev user on an older nightly should land on stable, not the
    // equivalent same-day nightly.
    let stable = stable_at("v1.7.0", "2026-06-26T15:00:00Z");
    let releases = vec![
        stable.clone(),
        nightly("nightly-20260626"),
        nightly("nightly-20260625"),
    ];
    let build = BuildInfo::new("nightly-20260625", "dev", "2026-06-25");
    let info = dev_update_from(&releases, Some(&build), Some(&stable), &env()).expect("an update");
    assert_eq!(info.tag, "v1.7.0");
}

#[test]
fn test_dev_on_promoted_stable_is_not_pulled_onto_equivalent_nightly() {
    // A dev user who already took the stable update must not be churned onto
    // the content-identical nightly that builds the same evening.
    let stable = stable_at("v1.7.0", "2026-06-26T15:00:00Z");
    let releases = vec![stable.clone(), nightly("nightly-20260626")];
    let build = BuildInfo::new("v1.7.0", "stable", "2026-06-26");
    assert!(dev_update_from(&releases, Some(&build), Some(&stable), &env()).is_none());
}

#[test]
fn test_dev_offers_same_day_nightly_that_postdates_the_stable() {
    // The v1.8.5.1 morning, 2026-07-23: stable published 01:07 UTC, the
    // 03:58 UTC cron nightly carried two fixes merged in between. The old
    // date-granularity tie favored stable and hid the nightly from every
    // dev-channel player (owner report, same day).
    let stable = stable_at("v1.8.5.1", "2026-07-23T01:07:34Z");
    let nightly = release_with(
        "nightly-20260723",
        true,
        "",
        "2026-07-23T03:58:51Z",
        &ALL_ASSETS,
    );
    let build = BuildInfo::new("v1.8.5.1", "stable", "2026-07-23");
    let info = dev_update_from(
        &[stable.clone(), nightly],
        Some(&build),
        Some(&stable),
        &env(),
    )
    .expect("an update");
    assert_eq!(info.tag, "nightly-20260723");
}

#[test]
fn test_dev_morning_nightly_still_steered_to_same_day_afternoon_stable() {
    // Promotion day with real timestamps: the 04:00 UTC cron nightly
    // predates the afternoon stable, so a player on that morning nightly
    // converges onto the promoted stable -- the date tie used to block
    // this direction too.
    let stable = stable_at("v1.9.0", "2026-08-01T15:00:00Z");
    let nightly = release_with(
        "nightly-20260801",
        true,
        "",
        "2026-08-01T04:00:00Z",
        &ALL_ASSETS,
    );
    let build = BuildInfo::new("nightly-20260801", "dev", "2026-08-01");
    let info = dev_update_from(
        &[stable.clone(), nightly],
        Some(&build),
        Some(&stable),
        &env(),
    )
    .expect("an update");
    assert_eq!(info.tag, "v1.9.0");
}

#[test]
fn test_dev_resumes_nightlies_once_they_outpace_stable() {
    // Days later dev advances past stable again; nightlies resume.
    let stable = stable_at("v1.7.0", "2026-06-26T15:00:00Z");
    let releases = vec![stable.clone(), nightly("nightly-20260630")];
    let build = BuildInfo::new("v1.7.0", "stable", "2026-06-26");
    let info = dev_update_from(&releases, Some(&build), Some(&stable), &env()).expect("an update");
    assert_eq!(info.tag, "nightly-20260630");
}

#[test]
fn test_18_snapshot_channel_ignores_19_tester_tags() {
    // Public 1.8 nightlies and Career 1.9 testers share the GitHub releases
    // list. A 1.8 snapshot on the dev channel must keep following nightly-*
    // and never pick a 1.9-tester-* prerelease, even when the tester is newer.
    let stable = stable_at("v1.8.5", "2026-08-01T15:00:00Z");
    let releases = vec![
        stable.clone(),
        nightly("nightly-20260820"),
        tester("1.9-tester-20260828"),
        nightly("nightly-20260810"),
    ];
    let build = BuildInfo::new("nightly-20260810", "dev", "2026-08-10");
    let info = snapshot_update_from(&releases, Some(&build), "1.8.6", Some(&stable), &env())
        .expect("an update");
    assert_eq!(info.tag, "nightly-20260820");
}

#[test]
fn test_19_snapshot_channel_picks_newest_19_tester() {
    // A Career 1.9 packaged build on the same snapshot/dev channel looks
    // only at 1.9-tester-* prereleases, newest YYYYMMDD first, and ignores
    // public 1.8 nightlies and 1.8 stables even when those are newer.
    let stable = stable_at("v1.8.6", "2026-08-28T18:00:00Z");
    let releases = vec![
        stable.clone(),
        nightly("nightly-20260828"),
        tester("1.9-tester-20260820"),
        tester("1.9-tester-20260825"),
        tester("1.9-tester-20260822"),
    ];
    let build = BuildInfo::new("1.9-tester-20260820", "dev", "2026-08-20");
    let info = snapshot_update_from(&releases, Some(&build), "1.9.0", Some(&stable), &env())
        .expect("an update");
    assert_eq!(info.tag, "1.9-tester-20260825");
    assert!(info.title.contains("1.9 tester snapshot 2026-08-25"));
}

#[test]
fn test_19_snapshot_offers_the_arm64_linux_tarball_to_an_arm64_process() {
    // The snapshot ships a native ARM64 Linux tarball (the Blazie BT Speak
    // and BT Braille, Raspberry Pi). A tester build running on aarch64 Linux
    // must be offered that archive, never the x86_64 one and never "no
    // update": the shared fixture has to carry it, or every snapshot test
    // that runs with the host's architecture fails on an ARM64 runner.
    let stable = stable_at("v1.8.6", "2026-08-28T18:00:00Z");
    let releases = vec![tester("1.9-tester-20260820"), tester("1.9-tester-20260825")];
    let build = BuildInfo::new("1.9-tester-20260820", "dev", "2026-08-20");
    let env = UpdaterEnv::fake_with_architecture(
        Platform::Linux,
        Architecture::Aarch64,
        Path::new("/tmp/not-frozen/python"),
    );
    let info = snapshot_update_from(&releases, Some(&build), "1.9.0", Some(&stable), &env)
        .expect("an update");
    assert_eq!(info.tag, "1.9-tester-20260825");
    assert!(info.asset_name.ends_with("-linux-arm64.tar.gz"));
}

#[test]
fn test_19_snapshot_rebuilt_under_the_same_tag_is_offered_by_commit() {
    // 2026-10-03: the release candidate re-cut 1.9-tester-20261003 in the
    // afternoon; the copy from that morning's run carried the same tag and
    // was told it was up to date. The stamped commit tells them apart.
    const MORNING: &str = "be5c41eae0a4d45b9193c705e48185479756ea44";
    const AFTERNOON: &str = "47df2cd3e0f2d61e8e06188e8267910cbb7b7db1";
    let mut rebuilt = tester("1.9-tester-20261003");
    rebuilt["target_commitish"] = json!(AFTERNOON);
    let releases = vec![tester("1.9-tester-20261002"), rebuilt];
    let mut build = BuildInfo::new("1.9-tester-20261003", "dev", "2026-10-03");

    build.commit = MORNING.to_string();
    let info = snapshot_update_from(&releases, Some(&build), "1.9.0", None, &env())
        .expect("the rebuild is offered");
    assert_eq!(info.tag, "1.9-tester-20261003");

    build.commit = AFTERNOON.to_string();
    assert!(snapshot_update_from(&releases, Some(&build), "1.9.0", None, &env()).is_none());

    // A copy stamped before commits were recorded stays where it was.
    build.commit = String::new();
    assert!(snapshot_update_from(&releases, Some(&build), "1.9.0", None, &env()).is_none());

    let stamped = build_info_from_dict(
        &json!({"tag": "1.9-tester-20261003", "commit": AFTERNOON}),
        "1.9.0",
    );
    assert_eq!(stamped.commit, AFTERNOON);
}

#[test]
fn test_19_snapshot_with_no_user_facing_changes_is_not_offered_over_its_stable() {
    // 2026-10-07: the nightly cut 1.9-tester-20261007 four hours after v1.9.3
    // with only a build-timeout change on top, and a 1.9.3 copy on the
    // snapshot channel was offered it as an update to the same game.
    const QUIET: &str = "Preview snapshot for players.\n\n\
        ## Changes since the previous snapshot\n\n- No user-facing changes\n";
    let quiet = release_with(
        "1.9-tester-20261007",
        true,
        QUIET,
        "2026-10-07T04:15:32Z",
        &ALL_ASSETS,
    );
    let stable = release_with(
        "v1.9.3",
        false,
        "## Fixes\n- **Snow chains stay on.**",
        "2026-10-07T00:04:35Z",
        &ALL_ASSETS,
    );
    let older = release_with(
        "1.9-tester-20261006",
        true,
        "## Added\n- **LWorks Radio.**",
        "2026-10-06T04:16:00Z",
        &ALL_ASSETS,
    );
    let oldest = release_with(
        "1.9-tester-20261005",
        true,
        "## Fixed\n- **Scales.**",
        "2026-10-05T04:16:00Z",
        &ALL_ASSETS,
    );
    let releases = vec![quiet.clone(), stable.clone(), older, oldest];

    let on_stable = BuildInfo::new("v1.9.3", "dev", "2026-10-07");
    assert!(snapshot_update_from(&releases, Some(&on_stable), "1.9.3", None, &env()).is_none());

    // A copy from before the stable release still lacks its fixes, which the
    // quiet snapshot's notes leave out because 1.9.3 listed them.
    let before = BuildInfo::new("1.9-tester-20261006", "dev", "2026-10-06");
    let info = snapshot_update_from(&releases, Some(&before), "1.9.0", None, &env())
        .expect("the stable's fixes arrive through the snapshot");
    assert_eq!(info.tag, "1.9-tester-20261007");
    // What's new reads what this copy is getting, not "No user-facing changes".
    assert_eq!(
        info.notes,
        [
            "Freight Fate version 1.9.3",
            "Fixes",
            "Snow chains stay on."
        ]
    );

    let further_back = BuildInfo::new("1.9-tester-20261005", "dev", "2026-10-05");
    let info = snapshot_update_from(&releases, Some(&further_back), "1.9.0", None, &env())
        .expect("the snapshot still carries the stable");
    assert_eq!(
        info.notes,
        [
            "Freight Fate version 1.9.3",
            "Fixes",
            "Snow chains stay on.",
            "Freight Fate 1.9 tester snapshot 2026-10-06",
            "Added",
            "LWorks Radio.",
        ]
    );

    // A later snapshot with real changes is offered over the stable as before.
    let real = release_with(
        "1.9-tester-20261008",
        true,
        "## Fixed\n- **A real fix.**",
        "2026-10-08T04:15:00Z",
        &ALL_ASSETS,
    );
    let releases = vec![real, quiet, stable];
    let info = snapshot_update_from(&releases, Some(&on_stable), "1.9.3", None, &env())
        .expect("a snapshot with changes");
    assert_eq!(info.tag, "1.9-tester-20261008");
}

#[test]
fn test_19_snapshot_channel_skips_newest_tester_without_windows_archive() {
    let releases = vec![
        release_with("1.9-tester-20260829", true, "", "", &["-macos.zip"]),
        tester("1.9-tester-20260828"),
    ];
    let build = BuildInfo::new("1.9-tester-20260827", "dev", "2026-08-27");

    let info = snapshot_update_from(
        &releases,
        Some(&build),
        "1.9.0",
        None,
        &env_on(Platform::Windows),
    )
    .expect("the newest compatible tester");
    assert_eq!(info.tag, "1.9-tester-20260828");
}

#[test]
fn test_check_for_update_uses_latest_and_the_release_list() {
    let env = env();
    let api = |path: &str| -> Result<Value, NetError> {
        match path {
            "/releases/latest" => Ok(release("v9.9.9")),
            "/releases?per_page=100&page=1" => {
                Ok(json!([release("v9.9.9"), nightly("nightly-20260611")]))
            }
            other => panic!("unexpected path {other}"),
        }
    };
    let stable = check_for_update_with("stable", "1.5.0", None, &env, &api).unwrap();
    assert_eq!(stable.unwrap().tag, "v9.9.9");
    // Neither release carries a published_at, so the date tie cannot favour
    // the stable and the newest nightly is offered, as in Python.
    let dev = check_for_update_with("dev", "1.5.0", None, &env, &api).unwrap();
    assert_eq!(dev.unwrap().tag, "nightly-20260611");
    // No stable release published yet: 404 means "nothing to offer", not an error.
    let no_stable = |_path: &str| -> Result<Value, NetError> { Err(NetError::http(404)) };
    assert!(
        check_for_update_with("stable", "1.5.0", None, &env, &no_stable)
            .unwrap()
            .is_none()
    );
    let down = |_path: &str| -> Result<Value, NetError> { Err(NetError::http(503)) };
    assert!(check_for_update_with("stable", "1.5.0", None, &env, &down).is_err());
}

#[test]
fn test_career_19_update_finds_a_tester_after_a_full_release_page() {
    let first_page = vec![release("v1.8.8"); 100];
    let requested = RefCell::new(Vec::new());
    let api = |path: &str| -> Result<Value, NetError> {
        requested.borrow_mut().push(path.to_string());
        match path {
            "/releases?per_page=100&page=1" => Ok(Value::Array(first_page.clone())),
            "/releases?per_page=100&page=2" => Ok(json!([tester("1.9-tester-20260829")])),
            other => panic!("unexpected path {other}"),
        }
    };
    let build = BuildInfo::new("1.9-tester-20260820", "dev", "2026-08-20");

    let info = check_for_update_with(
        "dev",
        "1.9.0",
        Some(&build),
        &env_on(Platform::Windows),
        &api,
    )
    .unwrap()
    .expect("a tester from the second release page");

    assert_eq!(info.tag, "1.9-tester-20260829");
    assert_eq!(
        requested.into_inner(),
        vec![
            "/releases?per_page=100&page=1",
            "/releases?per_page=100&page=2",
        ]
    );
}

#[test]
fn test_career_19_release_pagination_stops_at_exhaustion_without_crossing_channels() {
    let first_page = vec![nightly("nightly-20260829"); 100];
    let requested = RefCell::new(Vec::new());
    let api = |path: &str| -> Result<Value, NetError> {
        requested.borrow_mut().push(path.to_string());
        match path {
            "/releases?per_page=100&page=1" => Ok(Value::Array(first_page.clone())),
            "/releases?per_page=100&page=2" => Ok(json!([])),
            other => panic!("unexpected path {other}"),
        }
    };
    let build = BuildInfo::new("1.9-tester-20260820", "dev", "2026-08-20");

    let info = check_for_update_with(
        "dev",
        "1.9.0",
        Some(&build),
        &env_on(Platform::Windows),
        &api,
    )
    .unwrap();

    assert!(info.is_none());
    assert_eq!(
        requested.into_inner(),
        vec![
            "/releases?per_page=100&page=1",
            "/releases?per_page=100&page=2",
        ]
    );
}

#[test]
fn test_career_19_release_pagination_is_bounded_to_ten_full_pages() {
    let full_page = vec![nightly("nightly-20260829"); 100];
    let requested = RefCell::new(Vec::new());
    let api = |path: &str| -> Result<Value, NetError> {
        requested.borrow_mut().push(path.to_string());
        let page = path
            .strip_prefix("/releases?per_page=100&page=")
            .and_then(|value| value.parse::<usize>().ok())
            .expect("a bounded release-page request");
        assert!(page <= 10, "requested an eleventh release page");
        Ok(Value::Array(full_page.clone()))
    };
    let build = BuildInfo::new("1.9-tester-20260820", "dev", "2026-08-20");

    let info = check_for_update_with(
        "dev",
        "1.9.0",
        Some(&build),
        &env_on(Platform::Windows),
        &api,
    )
    .unwrap();

    assert!(info.is_none());
    assert_eq!(requested.into_inner().len(), 10);
}

#[test]
fn test_18_update_finds_a_nightly_after_a_full_tester_page() {
    let first_page = vec![tester("1.9-tester-20260829"); 100];
    let requested = RefCell::new(Vec::new());
    let api = |path: &str| -> Result<Value, NetError> {
        requested.borrow_mut().push(path.to_string());
        match path {
            "/releases?per_page=100&page=1" => Ok(Value::Array(first_page.clone())),
            "/releases?per_page=100&page=2" => Ok(json!([nightly("nightly-20260829")])),
            "/releases/latest" => Ok(stable_at("v1.8.8", "2026-08-20T15:00:00Z")),
            other => panic!("unexpected path {other}"),
        }
    };
    let build = BuildInfo::new("nightly-20260820", "dev", "2026-08-20");

    let info = check_for_update_with(
        "dev",
        "1.8.9.dev0",
        Some(&build),
        &env_on(Platform::Windows),
        &api,
    )
    .unwrap()
    .expect("a nightly from the second release page");

    assert_eq!(info.tag, "nightly-20260829");
    assert_eq!(requested.into_inner().last().unwrap(), "/releases/latest");
}

// -- assets and notes ---------------------------------------------------------

#[test]
fn test_pick_asset_matches_platform_suffix() {
    let rel = release("v1.6.0");
    let (name, _url, size) = pick_asset(&rel, Some("-windows-portable.zip"), &env()).unwrap();
    assert!(name.ends_with("-windows-portable.zip"));
    assert_eq!(size, 50_000_000);
    let (name, _, _) = pick_asset(&rel, Some("-linux-x64.tar.gz"), &env()).unwrap();
    assert!(name.ends_with("-linux-x64.tar.gz"));
    assert!(pick_asset(&rel, Some("-bsd.tar.xz"), &env()).is_none());
}

#[test]
fn test_pick_asset_chooses_the_macos_app_archive_for_mac_players() {
    let rel = tester("1.9-tester-20260830");
    let (name, url, size) = pick_asset(&rel, None, &mac_env(Architecture::Aarch64)).unwrap();
    assert_eq!(name, "FreightFate-1.9-tester-20260830-macos-arm64.zip");
    assert_eq!(
        url,
        "https://example.test/1.9-tester-20260830/-macos-arm64.zip"
    );
    assert_eq!(size, 50_000_000);
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
fn test_flatten_markdown_strips_formatting() {
    let body = "## Changes\n\n- **Cruise control.** K sets cruise.\n\
* See [the manual](https://example.test) for `details`.\n\
---\n";
    assert_eq!(
        flatten_markdown(Some(body)),
        vec![
            "Changes",
            "Cruise control. K sets cruise.",
            "See the manual for details.",
        ]
    );
}

#[test]
fn test_flatten_markdown_handles_empty_body() {
    assert!(flatten_markdown(Some("")).is_empty());
    assert!(flatten_markdown(None).is_empty());
}

// -- apply script -------------------------------------------------------------

/// A path as the POSIX apply scripts write it, escaped for sh. On Windows a
/// temp path's backslashes come out doubled.
fn sh(path: &std::path::Path) -> String {
    quote_for_script(&path.display().to_string(), false)
}

#[test]
fn test_write_apply_script_waits_for_pid_and_relaunches() {
    let tmp = tempfile::tempdir().unwrap();
    let staging = tmp.path().join("staging");
    fs::create_dir(&staging).unwrap();
    let new_root = staging.join("FreightFate");
    let install = tmp.path().join("install");
    let env = env();
    let script = write_apply_script(&new_root, &install, &staging, 4242, &env).unwrap();
    let text = fs::read_to_string(&script).unwrap();
    assert!(text.contains("4242"));
    assert!(text.contains(&install.display().to_string()));
    assert!(text.contains(&new_root.display().to_string()));
    assert!(text.contains("FreightFate"));
    assert_eq!(script.parent().unwrap(), tmp.path()); // outside the staging dir it deletes
                                                      // portable saves live inside the install folder; the swap must not
                                                      // touch them (Windows excludes the dir, POSIX never purges the root)
    if env.platform == Platform::Windows {
        assert!(text.contains("/XD _internal saves"));
    } else {
        assert!(text.contains(&format!("rm -rf \"{}/saves\"", new_root.display())));
    }
    assert!(!text.contains("/PURGE"));
    assert!(!text.contains(&format!("rm -rf \"{}\"", install.display())));
}

#[test]
fn test_write_apply_script_windows_template_is_the_bat() {
    let tmp = tempfile::tempdir().unwrap();
    let staging = tmp.path().join("staging");
    fs::create_dir(&staging).unwrap();
    let new_root = staging.join("FreightFate");
    let install = tmp.path().join("install");
    let script = write_apply_script(
        &new_root,
        &install,
        &staging,
        4242,
        &env_on(Platform::Windows),
    )
    .unwrap();
    let text = fs::read_to_string(&script).unwrap();
    assert!(script.extension().is_some_and(|e| e == "bat"));
    assert!(text.starts_with("@echo off\n:wait\n"));
    assert!(text.contains(&format!(
        "robocopy \"{}\\_internal\" \"{}\\_internal\" /MIR /R:10 /W:1 >NUL",
        new_root.display(),
        install.display()
    )));
    assert!(text.contains(&format!(
        "start \"\" \"{}\\FreightFate.exe\"",
        install.display()
    )));
    assert!(text.ends_with("del \"%~f0\"\n"));
}

/// An install folder with a percent sign or a dollar sign must reach the
/// apply script as that folder, not as whatever cmd or sh expands it to.
#[test]
fn test_apply_script_paths_survive_shell_expansion() {
    assert_eq!(
        quote_for_script(r"C:\Games\100% Trucks\FreightFate", true),
        r"C:\Games\100%% Trucks\FreightFate"
    );
    assert_eq!(
        quote_for_script("/home/sam/$HOME `id` \\x \"q\"", false),
        "/home/sam/\\$HOME \\`id\\` \\\\x \\\"q\\\""
    );
    assert_eq!(
        quote_for_script("/home/sam/Games/FreightFate", false),
        "/home/sam/Games/FreightFate"
    );

    let tmp = tempfile::tempdir().unwrap();
    let staging = tmp.path().join("staging");
    fs::create_dir(&staging).unwrap();
    let new_root = staging.join("FreightFate");
    let install = tmp.path().join("100% $HOME");
    let bat =
        write_apply_script(&new_root, &install, &staging, 7, &env_on(Platform::Windows)).unwrap();
    let text = fs::read_to_string(&bat).unwrap();
    assert!(text.contains("100%% $HOME"));
    assert!(!text.contains("100% $HOME"));
    let sh =
        write_apply_script(&new_root, &install, &staging, 7, &env_on(Platform::Linux)).unwrap();
    let text = fs::read_to_string(&sh).unwrap();
    assert!(text.contains("100% \\$HOME"));
}

#[test]
fn test_extracted_root_finds_macos_app_bundle() {
    // The macOS archive holds FreightFate.app (ditto --keepParent), not a
    // plain FreightFate folder; extraction must find the bundle (issue #25).
    let tmp = tempfile::tempdir().unwrap();
    let staging = tmp.path().join("unpacked");
    fs::create_dir_all(staging.join("FreightFate.app/Contents/MacOS")).unwrap();
    assert_eq!(
        updater::extracted_root(&staging, "the archive", &env_on(Platform::MacOs)).unwrap(),
        staging.join("FreightFate.app")
    );
}

#[test]
fn test_extracted_root_missing_app_raises() {
    let tmp = tempfile::tempdir().unwrap();
    let err =
        updater::extracted_root(tmp.path(), "FreightFate-1.7.0-macos.zip", &env()).unwrap_err();
    assert!(err.to_string().contains("FreightFate-1.7.0-macos.zip"));
}

#[test]
fn test_install_target_is_bundle_root_on_macos() {
    let tmp = tempfile::tempdir().unwrap();
    let exe = tmp
        .path()
        .join("FreightFate.app/Contents/MacOS/FreightFate");
    fs::create_dir_all(exe.parent().unwrap()).unwrap();
    fs::write(&exe, "").unwrap();
    let env = UpdaterEnv::fake(Platform::MacOs, &exe);
    assert_eq!(
        updater::install_target_in(&env),
        plain_canonical(&tmp.path().join("FreightFate.app"))
    );
}

#[test]
fn test_install_target_is_install_root_off_macos() {
    // Only macOS wraps the install in a bundle; elsewhere the exe dir is it.
    let env = env_on(Platform::Linux);
    assert_eq!(
        updater::install_target_in(&env),
        updater::install_root_in(&env)
    );
}

#[test]
fn test_write_apply_script_macos_swaps_whole_bundle() {
    let tmp = tempfile::tempdir().unwrap();
    let staging = tmp.path().join("staging");
    fs::create_dir(&staging).unwrap();
    let new_root = staging.join("FreightFate.app");
    let install = tmp.path().join("Applications").join("FreightFate.app");
    let script = write_apply_script(
        &new_root,
        &install,
        &staging,
        4242,
        &env_on(Platform::MacOs),
    )
    .unwrap();
    let text = fs::read_to_string(&script).unwrap();
    assert!(text.contains("4242"));
    assert!(text.contains(&sh(&new_root)));
    // the old bundle is parked, not deleted, until the new one is in place
    assert!(text.contains(&format!("mv \"{0}\" \"{0}.old\"", sh(&install))));
    assert!(text.contains(&format!("mv \"{0}.old\" \"{0}\"", sh(&install))));
    assert!(text.contains(&format!("open \"{}\"", sh(&install))));
    assert_eq!(script.parent().unwrap(), tmp.path()); // outside the staging dir it deletes
}

// -- settings -----------------------------------------------------------------

// `test_settings_default_and_validation` is live in `crates/ff-core/src/settings/tests.rs`.

#[test]
fn test_build_info_none_when_not_frozen() {
    assert!(!updater::is_frozen_in(&env()));
    assert!(updater::load_build_info_in(&env(), "1.6.0").is_none());
    // The test binary is not a packaged build either.
    assert!(!updater::is_frozen());
    assert!(updater::load_build_info("1.6.0").is_none());
}

#[test]
#[ignore = "Nuitka's __compiled__ marker has no Rust equivalent; is_frozen reads the install layout instead (see test_nuitka_standalone_folder_counts_as_packaged_build)"]
fn test_is_frozen_detects_nuitka() {}

/// `fs::canonicalize` output with the Windows verbatim prefix undone, the
/// way the updater must hand paths to the apply script: robocopy refuses
/// `\\?\` outright, and every Windows update silently copied nothing
/// until the prefix was stripped (the tester "restart loop", 2026-08-31).
fn plain_canonical(path: &std::path::Path) -> std::path::PathBuf {
    let canonical = fs::canonicalize(path).unwrap();
    let text = canonical.to_string_lossy();
    if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
        return std::path::PathBuf::from(format!(r"\\{rest}"));
    }
    match text.strip_prefix(r"\\?\") {
        Some(rest) => std::path::PathBuf::from(rest.to_string()),
        None => canonical,
    }
}

#[test]
fn test_install_root_is_executable_dir() {
    let exe = std::env::current_exe().unwrap();
    let root = updater::install_root();
    // Never the verbatim form: the apply script's robocopy refuses it.
    assert!(!root.to_string_lossy().starts_with(r"\\?\"));
    assert_eq!(root, plain_canonical(&exe).parent().unwrap());
}

// -- update states (app shell) ------------------------------------------------

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
static UPDATE_CHECK_TESTS: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[test]
fn test_startup_update_prompt_respects_skipped_version() {
    let _serial = UPDATE_CHECK_TESTS.lock().unwrap_or_else(|e| e.into_inner());
    use freight_fate::app::testing::TestApp;
    use freight_fate::states::main_menu::MainMenuState;
    use freight_fate::states::update::UpdateChecker;

    let info = updater::UpdateInfo {
        tag: "v1.6.1".to_string(),
        title: "Freight Fate version 1.6.1".to_string(),
        notes: Vec::new(),
        asset_name: "FreightFate-1.6.1-windows-portable.zip".to_string(),
        asset_url: "https://example.test/FreightFate.zip".to_string(),
        asset_size: 1,
        asset_sha256: String::new(),
        slim: None,
    };
    let mut app = TestApp::new();
    app.ctx.settings.skipped_update = "v1.6.1".to_string();
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
#[ignore = "needs a seam for updater::is_frozen(): arm_update_check reads the real install layout, and a test binary is never a packaged build"]
fn test_terminal_exit_arms_fresh_packaged_update_check() {}

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
#[ignore = "needs a seam for updater::is_frozen(): arm_update_check reads the real install layout, and a test binary is never a packaged build"]
fn test_pickup_facility_exit_arms_fresh_packaged_update_check() {}

#[test]
#[ignore = "needs a seam for updater::is_frozen(): arm_update_check reads the real install layout, and a test binary is never a packaged build"]
fn test_drive_exit_does_not_arm_fresh_update_check() {}

#[test]
fn test_remind_later_help_describes_terminal_exit_check() {
    let mut app = TestApp::new();
    let info = UpdateInfo {
        tag: "v1.6.1".to_string(),
        title: "Freight Fate version 1.6.1".to_string(),
        notes: vec![],
        asset_name: "FreightFate-1.6.1-windows-portable.zip".to_string(),
        asset_url: "https://example.test/FreightFate.zip".to_string(),
        asset_size: 1,
        asset_sha256: String::new(),
        slim: None,
    };
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

// -- AppImage -------------------------------------------------------------------

#[test]
fn test_running_appimage_path_requires_existing_file() {
    let tmp = tempfile::tempdir().unwrap();
    assert!(updater::running_appimage_path(None).is_none());
    let missing = tmp.path().join("missing.AppImage");
    assert!(updater::running_appimage_path(Some(&missing.display().to_string())).is_none());
    let appimage = fake_appimage(tmp.path());
    assert_eq!(
        updater::running_appimage_path(Some(&appimage.display().to_string())),
        Some(appimage)
    );
}

#[test]
fn test_platform_suffix_prefers_appimage_when_running_as_one() {
    let tmp = tempfile::tempdir().unwrap();
    assert_eq!(
        updater::platform_suffix(&linux_appimage_env(None)),
        TARBALL_SUFFIX
    );
    let appimage = fake_appimage(tmp.path());
    assert_eq!(
        updater::platform_suffix(&linux_appimage_env(Some(&appimage))),
        APPIMAGE_SUFFIX
    );
}

#[test]
fn test_linux_suffixes_follow_the_process_architecture() {
    // The Blazie BT Speak and BT Braille are ARM64 Linux. Offered the x86_64
    // tarball, the updater would install a game that cannot start there,
    // and an x86_64 desktop offered the ARM64 one likewise.
    let tmp = tempfile::tempdir().unwrap();
    assert_eq!(
        updater::platform_suffix(&linux_appimage_env_on(Architecture::Aarch64, None)),
        TARBALL_SUFFIX_AARCH64
    );
    assert_eq!(TARBALL_SUFFIX_AARCH64, "-linux-arm64.tar.gz");
    let appimage = tmp
        .path()
        .join("FreightFate-1.9-tester-20260902-linux-aarch64.AppImage");
    fs::write(&appimage, b"old").unwrap();
    assert_eq!(
        updater::platform_suffix(&linux_appimage_env_on(
            Architecture::Aarch64,
            Some(&appimage)
        )),
        APPIMAGE_SUFFIX_AARCH64
    );
    assert_eq!(APPIMAGE_SUFFIX_AARCH64, "-linux-aarch64.AppImage");
    assert_eq!(
        updater::tarball_suffix(Architecture::X86_64),
        TARBALL_SUFFIX
    );
    assert_eq!(
        updater::appimage_suffix(Architecture::X86_64),
        APPIMAGE_SUFFIX
    );
}

#[test]
fn test_arm64_linux_picks_the_arm64_tarball_and_never_the_x64_one() {
    let both = release_with(
        "1.9-tester-20260902",
        true,
        "",
        "",
        &[
            TARBALL_SUFFIX,
            APPIMAGE_SUFFIX,
            TARBALL_SUFFIX_AARCH64,
            APPIMAGE_SUFFIX_AARCH64,
        ],
    );
    let arm = linux_appimage_env_on(Architecture::Aarch64, None);
    let (name, _, _) = pick_asset(&both, None, &arm).unwrap();
    assert_eq!(name, "FreightFate-1.9-tester-20260902-linux-arm64.tar.gz");
    let pc = linux_appimage_env_on(Architecture::X86_64, None);
    let (name, _, _) = pick_asset(&both, None, &pc).unwrap();
    assert_eq!(name, "FreightFate-1.9-tester-20260902-linux-x64.tar.gz");

    // A release from before the ARM64 build existed has nothing for an
    // ARM64 process: no update rather than the wrong one.
    let x64_only = release_with(
        "1.9-tester-20260901",
        true,
        "",
        "",
        &[TARBALL_SUFFIX, APPIMAGE_SUFFIX],
    );
    assert!(pick_asset(&x64_only, None, &arm).is_none());
    let (name, _, _) = pick_asset(&x64_only, None, &pc).unwrap();
    assert!(name.ends_with(TARBALL_SUFFIX));
}

#[test]
fn test_arm64_appimage_run_picks_the_arm64_appimage_asset() {
    let tmp = tempfile::tempdir().unwrap();
    let appimage = tmp
        .path()
        .join("FreightFate-1.9-tester-20260901-linux-aarch64.AppImage");
    fs::write(&appimage, b"old").unwrap();
    let env = linux_appimage_env_on(Architecture::Aarch64, Some(&appimage));
    let both = release_with(
        "1.9-tester-20260902",
        true,
        "",
        "",
        &[
            TARBALL_SUFFIX,
            APPIMAGE_SUFFIX,
            TARBALL_SUFFIX_AARCH64,
            APPIMAGE_SUFFIX_AARCH64,
        ],
    );
    let (name, _, _) = pick_asset(&both, None, &env).unwrap();
    assert_eq!(
        name,
        "FreightFate-1.9-tester-20260902-linux-aarch64.AppImage"
    );
}

#[test]
fn test_appimage_run_picks_appimage_asset() {
    let tmp = tempfile::tempdir().unwrap();
    let appimage = fake_appimage(tmp.path());
    let rel = release_with(
        "v9.9.9",
        false,
        "",
        "",
        &[
            "-windows-portable.zip",
            "-linux-x64.tar.gz",
            "-linux-x86_64.AppImage",
        ],
    );
    let info =
        stable_update_from(&rel, "1.5.0", &linux_appimage_env(Some(&appimage))).expect("an update");
    assert!(info.asset_name.ends_with("-linux-x86_64.AppImage"));
}

#[test]
fn test_appimage_run_falls_back_to_tarball_for_old_releases() {
    // Releases published before the AppImage existed only ship the tarball;
    // the update must still be offered rather than reported as up to date.
    let tmp = tempfile::tempdir().unwrap();
    let appimage = fake_appimage(tmp.path());
    let info = stable_update_from(
        &release("v9.9.9"),
        "1.5.0",
        &linux_appimage_env(Some(&appimage)),
    )
    .expect("an update");
    assert!(info.asset_name.ends_with("-linux-x64.tar.gz"));
}

#[test]
fn test_stage_update_appimage_is_the_update() {
    let tmp = tempfile::tempdir().unwrap();
    let staging = tmp.path().join("staging");
    fs::create_dir(&staging).unwrap();
    let archive = staging.join("FreightFate-9.9.9-linux-x86_64.AppImage");
    fs::write(&archive, b"new").unwrap();

    assert_eq!(
        updater::stage_update(&archive, &staging, &env_on(Platform::Linux)).unwrap(),
        archive
    );
    assert!(archive.exists()); // nothing unpacked, nothing deleted
}

#[test]
fn test_stage_update_unpacks_a_zip_and_drops_the_archive() {
    let tmp = tempfile::tempdir().unwrap();
    let staging = tmp.path().join("staging");
    fs::create_dir(&staging).unwrap();
    let archive = staging.join("FreightFate-9.9.9-windows-portable.zip");
    {
        let file = fs::File::create(&archive).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default();
        zip.add_directory("FreightFate/", options).unwrap();
        zip.start_file("FreightFate/build_info.json", options)
            .unwrap();
        use std::io::Write;
        zip.write_all(b"{}").unwrap();
        zip.finish().unwrap();
    }
    let new_root = updater::stage_update(&archive, &staging, &env_on(Platform::Windows)).unwrap();
    assert_eq!(new_root, staging.join("unpacked").join("FreightFate"));
    assert!(new_root.join("build_info.json").is_file());
    assert!(!archive.exists());
}

/// An archive entry that climbs out of the staging folder must never land
/// outside it, whatever else the unpack does with it.
#[test]
fn test_stage_update_keeps_a_climbing_zip_entry_inside_staging() {
    let tmp = tempfile::tempdir().unwrap();
    let staging = tmp.path().join("staging");
    fs::create_dir(&staging).unwrap();
    let archive = staging.join("FreightFate-9.9.9-windows-portable.zip");
    {
        use std::io::Write;
        let file = fs::File::create(&archive).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default();
        zip.add_directory("FreightFate/", options).unwrap();
        zip.start_file("FreightFate/build_info.json", options)
            .unwrap();
        zip.write_all(b"{}").unwrap();
        zip.start_file("../../escaped.txt", options).unwrap();
        zip.write_all(b"outside").unwrap();
        zip.finish().unwrap();
    }
    let _ = updater::stage_update(&archive, &staging, &env_on(Platform::Windows));
    assert!(!tmp.path().join("escaped.txt").exists());
    assert!(!staging.join("escaped.txt").exists());
}

#[test]
fn test_stage_update_unpacks_a_tarball() {
    let tmp = tempfile::tempdir().unwrap();
    let staging = tmp.path().join("staging");
    fs::create_dir(&staging).unwrap();
    let archive = staging.join("FreightFate-9.9.9-linux-x64.tar.gz");
    {
        let file = fs::File::create(&archive).unwrap();
        let encoder = flate2::write::GzEncoder::new(file, flate2::Compression::default());
        let mut tar = tar::Builder::new(encoder);
        let mut header = tar::Header::new_gnu();
        header.set_size(2);
        header.set_mode(0o644);
        header.set_cksum();
        tar.append_data(&mut header, "FreightFate/build_info.json", &b"{}"[..])
            .unwrap();
        tar.into_inner().unwrap().finish().unwrap();
    }
    let new_root = updater::stage_update(&archive, &staging, &env_on(Platform::Linux)).unwrap();
    assert_eq!(new_root, staging.join("unpacked").join("FreightFate"));
    assert!(new_root.join("build_info.json").is_file());
}

#[test]
fn test_can_auto_apply_matrix() {
    let tmp = tempfile::tempdir().unwrap();
    let appimage_update = tmp.path().join("FreightFate-9.9.9-linux-x86_64.AppImage");
    fs::write(&appimage_update, b"new").unwrap();
    let folder_update = tmp.path().join("FreightFate");
    fs::create_dir(&folder_update).unwrap();

    // Not an AppImage run: folder swaps work, an AppImage file does not.
    let plain = linux_appimage_env(None);
    assert!(updater::can_auto_apply(&folder_update, &plain));
    assert!(!updater::can_auto_apply(&appimage_update, &plain));

    // AppImage run with a writable folder: swap the file, never the folder
    // (the mounted payload is read-only and disposable).
    let appimage = fake_appimage(tmp.path());
    let running = linux_appimage_env(Some(&appimage));
    assert!(updater::can_auto_apply(&appimage_update, &running));
    assert!(!updater::can_auto_apply(&folder_update, &running));
}

#[test]
fn test_write_apply_script_appimage_swaps_the_file() {
    let tmp = tempfile::tempdir().unwrap();
    let staging = tmp.path().join("staging");
    fs::create_dir(&staging).unwrap();
    let new_root = staging.join("FreightFate-9.9.9-linux-x86_64.AppImage");
    fs::write(&new_root, b"new").unwrap();
    let install = tmp
        .path()
        .join("Apps")
        .join("FreightFate-1.8.5-linux-x86_64.AppImage");

    let script = write_apply_script(
        &new_root,
        &install,
        &staging,
        4242,
        &env_on(Platform::Linux),
    )
    .unwrap();
    let text = fs::read_to_string(&script).unwrap();

    assert!(text.contains("4242"));
    // staged next to the target so the final rename is atomic
    assert!(text.contains(&format!(
        "cp \"{}\" \"{}.update-new\"",
        sh(&new_root),
        sh(&install)
    )));
    assert!(text.contains(&format!("chmod +x \"{}.update-new\"", sh(&install))));
    assert!(text.contains(&format!("mv -f \"{0}.update-new\" \"{0}\"", sh(&install))));
    // relaunches the new AppImage file, never the dead mount path
    assert!(text.contains(&format!("\"{}\" &", sh(&install))));
    assert_eq!(script.parent().unwrap(), tmp.path()); // outside the staging dir it deletes
                                                      // the mounted payload is never touched: no folder copy, no purge
    assert!(!text.contains("cp -a"));
    assert!(!text.contains(&format!("rm -rf \"{}\"", sh(&install))));
}

#[test]
fn test_apply_and_restart_appimage_targets_running_appimage() {
    let tmp = tempfile::tempdir().unwrap();
    let appimage = fake_appimage(tmp.path());
    let staging = tmp.path().join("staging");
    fs::create_dir(&staging).unwrap();
    let new_root = staging.join("FreightFate-9.9.9-linux-x86_64.AppImage");
    fs::write(&new_root, b"new").unwrap();

    let mut spawned: Vec<Vec<String>> = Vec::new();
    let env = linux_appimage_env(Some(&appimage));
    let script = updater::apply_and_restart_with(&new_root, &staging, &env, &mut |cmd| {
        spawned.push(cmd);
        Ok(())
    })
    .unwrap()
    .expect("a script");

    assert_eq!(spawned[0][0], "/bin/sh");
    assert_eq!(spawned[0][1], script.display().to_string());
    let text = fs::read_to_string(&script).unwrap();
    assert!(text.contains(&sh(&appimage)));
}

#[test]
fn test_apply_and_restart_appimage_without_env_refuses() {
    // A .AppImage download while not running from an AppImage must never
    // fall through to the folder-swap script.
    let tmp = tempfile::tempdir().unwrap();
    let staging = tmp.path().join("staging");
    fs::create_dir(&staging).unwrap();
    let new_root = staging.join("FreightFate-9.9.9-linux-x86_64.AppImage");
    fs::write(&new_root, b"new").unwrap();

    let mut spawned: Vec<Vec<String>> = Vec::new();
    let result = updater::apply_and_restart_with(
        &new_root,
        &staging,
        &linux_appimage_env(None),
        &mut |cmd| {
            spawned.push(cmd);
            Ok(())
        },
    )
    .unwrap();

    assert!(result.is_none());
    assert!(spawned.is_empty());
}

#[test]
fn test_stash_for_manual_install_moves_file_to_home() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    fs::create_dir(&home).unwrap();
    let update = tmp.path().join("FreightFate-9.9.9-linux-x86_64.AppImage");
    fs::write(&update, b"new").unwrap();

    let dest = updater::stash_for_manual_install(&update, Some(&home));

    assert_eq!(dest, home.join(update.file_name().unwrap()));
    assert_eq!(fs::read(&dest).unwrap(), b"new");
    assert!(!update.exists());
}

#[test]
fn test_stash_for_manual_install_leaves_folders_in_place() {
    let tmp = tempfile::tempdir().unwrap();
    let folder = tmp.path().join("FreightFate");
    fs::create_dir(&folder).unwrap();
    assert_eq!(updater::stash_for_manual_install(&folder, None), folder);
}

#[test]
fn test_download_state_parks_update_when_not_auto_appliable() {
    // An install that cannot swap itself must say where the download went
    // and hand the player back, never restart into a dead end.
    let mut app = TestApp::new();
    app.push_state(base_screen());
    let tmp = tempfile::tempdir().unwrap();
    let info = UpdateInfo {
        tag: "v9.9.9".to_string(),
        title: "Freight Fate version 9.9.9".to_string(),
        notes: vec![],
        asset_name: "FreightFate-9.9.9-linux-x86_64.AppImage".to_string(),
        asset_url: "https://example.test/a".to_string(),
        asset_size: 1,
        asset_sha256: String::new(),
        slim: None,
    };
    let new_root = tmp.path().join(&info.asset_name);
    fs::write(&new_root, b"new").unwrap();
    let mut state = UpdateDownloadState::finished_with(
        info,
        tmp.path().to_path_buf(),
        new_root.clone(),
        Box::new(|_root| false),
        Box::new(|root| root.to_path_buf()),
    );
    let depth = app.ctx.stack_len();
    app.clear_speech();

    state.update(&mut app.ctx, 0.0);
    app.ctx.run_deferred();

    assert_eq!(app.ctx.stack_len(), depth - 1); // handed back, not restarted
    let said = app.main_lines().join(" ");
    assert!(said.contains("cannot update itself"), "{said}");
    assert!(said.contains(&new_root.display().to_string()), "{said}");
    app.shutdown();
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

#[test]
fn test_update_info_is_plain_data() {
    let info = UpdateInfo {
        tag: "v9.9.9".to_string(),
        title: "Freight Fate version 9.9.9".to_string(),
        notes: vec![],
        asset_name: "FreightFate-9.9.9-linux-x86_64.AppImage".to_string(),
        asset_url: "https://example.test/a".to_string(),
        asset_size: 1,
        asset_sha256: String::new(),
        slim: None,
    };
    assert_eq!(info.clone(), info);
}
