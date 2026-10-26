//! `build-ios`: build Freight Fate for iPhone and iPad.
//!
//! The iOS game is the desktop game: the same Rust states and menus, spoken
//! through Prism (VoiceOver, or AVSpeech when VoiceOver is off), with SDL2
//! running the UIKit window and game controllers and a small Objective-C
//! touch layer (`crates/freight-fate/ios/ff_touch.m`) turning gestures into
//! key presses. This turns a `cargo build` for an iOS target into an
//! installable `FreightFate.app`:
//!
//! * the executable, with the baked world data, the committed loose sounds
//!   and the sound and music packs beside it (an iOS bundle is flat, so the
//!   game's "resources beside the executable" layout already fits);
//! * nothing under `Frameworks/`: SDL2, Prism and the audio engine are all
//!   linked into the executable;
//! * an `Info.plist` declaring controller support, and the app icon compiled
//!   from `crates/freight-fate/ios/Assets.xcassets`;
//! * a code signature: ad-hoc for the Simulator, or a real identity plus
//!   provisioning profile for a device.
//!
//! ```text
//! cargo tools build-ios                          # Simulator build
//! cargo tools build-ios --install --launch
//! cargo tools build-ios --device --sign-identity "Apple Development: ..." \
//!     --provisioning-profile path/to/profile.mobileprovision
//! cargo tools build-ios --device --ipa --build-number 7 \
//!     --sign-identity "Apple Distribution: ..." \
//!     --provisioning-profile path/to/appstore.mobileprovision   # TestFlight
//! ```
//!
//! The Simulator build targets Apple Silicon Macs (`aarch64-apple-ios-sim`).

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use super::plist::{self, Plist};
use crate::cli::{Args, Spec};
use crate::util::{self, Result};

pub const BUNDLE_ID: &str = "net.orinks.freightfate";
const APP_NAME: &str = "FreightFate";
const DISPLAY_NAME: &str = "Freight Fate";
/// SDL2's UIKit backend and Prism's VoiceOver backend both run on iOS 12,
/// but GameController's extended-gamepad profile names and CoreHaptics need
/// 14; nothing older is worth carrying.
const MINIMUM_IOS: &str = "14.0";

#[derive(Clone, Copy)]
struct Target {
    triple: &'static str,
    sdk: &'static str,
    platform: &'static str,
    simulator: bool,
}

const SIMULATOR: Target = Target {
    triple: "aarch64-apple-ios-sim",
    sdk: "iphonesimulator",
    platform: "iPhoneSimulator",
    simulator: true,
};
const DEVICE: Target = Target {
    triple: "aarch64-apple-ios",
    sdk: "iphoneos",
    platform: "iPhoneOS",
    simulator: false,
};

fn ios_build() -> PathBuf {
    util::repo_root().join("build").join("ios")
}

/// `1.9.0.dev0` -> `1.9.0`: App Store Connect takes at most three integers.
pub fn store_version(version: &str) -> String {
    let mut parts = Vec::new();
    for part in version.split('.').take(3) {
        let digits: String = part.chars().take_while(char::is_ascii_digit).collect();
        if digits.is_empty() {
            break;
        }
        parts.push(digits);
    }
    if parts.is_empty() {
        "0".into()
    } else {
        parts.join(".")
    }
}

fn info_plist(version: &str, target: Target, build_number: Option<&str>) -> Plist {
    let gamepad = |name: &str| Plist::dict(vec![("ProfileName", Plist::string(name))]);
    Plist::dict(vec![
        ("CFBundleDevelopmentRegion", Plist::string("en")),
        ("CFBundleDisplayName", Plist::string(DISPLAY_NAME)),
        ("CFBundleExecutable", Plist::string(APP_NAME)),
        ("CFBundleIdentifier", Plist::string(BUNDLE_ID)),
        ("CFBundleInfoDictionaryVersion", Plist::string("6.0")),
        ("CFBundleName", Plist::string(APP_NAME)),
        ("CFBundlePackageType", Plist::string("APPL")),
        (
            "CFBundleShortVersionString",
            Plist::string(store_version(version)),
        ),
        (
            "CFBundleVersion",
            Plist::string(build_number.unwrap_or(version)),
        ),
        (
            "CFBundleSupportedPlatforms",
            Plist::strings(&[target.platform]),
        ),
        ("DTPlatformName", Plist::string(target.sdk)),
        ("LSRequiresIPhoneOS", Plist::Bool(true)),
        ("UIRequiredDeviceCapabilities", Plist::strings(&["arm64"])),
        ("MinimumOSVersion", Plist::string(MINIMUM_IOS)),
        (
            "UIDeviceFamily",
            Plist::Array(vec![Plist::Int(1), Plist::Int(2)]),
        ),
        ("UILaunchScreen", Plist::Dict(Vec::new())),
        // iOS 27 will not launch an app built against its SDK without a
        // scene lifecycle; `FFSceneDelegate` (in `ff_touch.m`) puts SDL2's
        // windows on the scene.
        (
            "UIApplicationSceneManifest",
            Plist::dict(vec![
                ("UIApplicationSupportsMultipleScenes", Plist::Bool(false)),
                (
                    "UISceneConfigurations",
                    Plist::dict(vec![(
                        "UIWindowSceneSessionRoleApplication",
                        Plist::Array(vec![Plist::dict(vec![
                            ("UISceneConfigurationName", Plist::string("Default")),
                            ("UISceneDelegateClassName", Plist::string("FFSceneDelegate")),
                        ])]),
                    )]),
                ),
            ]),
        ),
        ("UIRequiresFullScreen", Plist::Bool(true)),
        ("UIStatusBarHidden", Plist::Bool(true)),
        (
            "UIApplicationSupportsIndirectInputEvents",
            Plist::Bool(true),
        ),
        (
            "UISupportedInterfaceOrientations",
            Plist::strings(&[
                "UIInterfaceOrientationPortrait",
                "UIInterfaceOrientationLandscapeLeft",
                "UIInterfaceOrientationLandscapeRight",
            ]),
        ),
        (
            "UISupportedInterfaceOrientations~ipad",
            Plist::strings(&[
                "UIInterfaceOrientationPortrait",
                "UIInterfaceOrientationPortraitUpsideDown",
                "UIInterfaceOrientationLandscapeLeft",
                "UIInterfaceOrientationLandscapeRight",
            ]),
        ),
        // Game controllers: SDL2 reads them through GameController.
        ("GCSupportsControllerUserInteraction", Plist::Bool(true)),
        (
            "GCSupportedGameControllers",
            Plist::Array(vec![gamepad("ExtendedGamepad"), gamepad("MicroGamepad")]),
        ),
        (
            "NSBluetoothAlwaysUsageDescription",
            Plist::string("Freight Fate uses Bluetooth to talk to game controllers."),
        ),
        ("ITSAppUsesNonExemptEncryption", Plist::Bool(false)),
    ])
}

fn capture(program: &str, args: &[&str]) -> Result<String> {
    Ok(util::output(Command::new(program).args(args))?
        .trim()
        .to_string())
}

/// The toolchain keys Xcode stamps into Info.plist; App Store Connect checks
/// them.
fn sdk_info(target: Target) -> Result<Plist> {
    let sdk_version = capture("xcrun", &["--sdk", target.sdk, "--show-sdk-version"])?;
    let sdk_build = capture("xcrun", &["--sdk", target.sdk, "--show-sdk-build-version"])?;
    let xcode = capture("xcodebuild", &["-version"])?;
    let lines: Vec<&str> = xcode.lines().collect();
    let last_word = |line: Option<&&str>| {
        line.and_then(|l| l.split_whitespace().last())
            .unwrap_or_default()
            .to_string()
    };
    let version = last_word(lines.first());
    let xcode_build = last_word(lines.last());
    let mut numbers = version.split('.');
    let major: u32 = numbers
        .next()
        .and_then(|m| m.parse().ok())
        .ok_or_else(|| format!("cannot read the Xcode version from {xcode:?}"))?;
    let minor = numbers.next().unwrap_or("0");
    let patch = numbers.next().unwrap_or("0");
    Ok(Plist::dict(vec![
        (
            "DTCompiler",
            Plist::string("com.apple.compilers.llvm.clang.1_0"),
        ),
        ("DTPlatformBuild", Plist::string(sdk_build.clone())),
        ("DTPlatformVersion", Plist::string(sdk_version.clone())),
        ("DTSDKBuild", Plist::string(sdk_build)),
        (
            "DTSDKName",
            Plist::string(format!("{}{sdk_version}", target.sdk)),
        ),
        (
            "DTXcode",
            Plist::string(format!("{major:02}{minor}{patch}")),
        ),
        ("DTXcodeBuild", Plist::string(xcode_build)),
        (
            "BuildMachineOSBuild",
            Plist::string(capture("sw_vers", &["-buildVersion"])?),
        ),
    ]))
}

/// Compile the asset catalog into `Assets.car`; return its Info.plist keys.
fn compile_icon(app: &Path, target: Target) -> Result<Plist> {
    let partial = ios_build().join("assets-info.plist");
    let catalog = util::repo_root()
        .join("crates")
        .join("freight-fate")
        .join("ios")
        .join("Assets.xcassets");
    util::run(
        Command::new("xcrun")
            .arg("actool")
            .arg("--compile")
            .arg(app)
            .args(["--platform", target.sdk])
            .args(["--minimum-deployment-target", MINIMUM_IOS])
            .args(["--app-icon", "AppIcon"])
            .arg("--output-partial-info-plist")
            .arg(&partial)
            .args(["--target-device", "iphone", "--target-device", "ipad"])
            .args(["--output-format", "human-readable-text"])
            .arg(&catalog)
            .stdout(Stdio::null()),
    )?;
    plist::from_xml(&util::read_text(&partial)?)
}

fn cargo_build(target: Target, release: bool) -> Result<PathBuf> {
    let root = util::repo_root();
    let mut cmd = Command::new("cargo");
    // No agent server; the visual display is what draws the screen.
    cmd.args(["build", "-p", "freight-fate", "--no-default-features"])
        .args(["--features", "visuals", "--target", target.triple]);
    if release {
        cmd.arg("--release");
    }
    cmd.current_dir(&root)
        .env("IPHONEOS_DEPLOYMENT_TARGET", MINIMUM_IOS);
    util::run(&mut cmd)?;
    Ok(root
        .join("target")
        .join(target.triple)
        .join(if release { "release" } else { "debug" }))
}

fn provisioning_entitlements(profile: &Path) -> Result<Plist> {
    let decoded = util::output(
        Command::new("security")
            .args(["cms", "-D", "-i"])
            .arg(profile),
    )?;
    Ok(plist::from_xml(&decoded)?
        .get("Entitlements")
        .cloned()
        .unwrap_or(Plist::Dict(Vec::new())))
}

fn codesign(app: &Path, identity: &str, entitlements: Option<&Path>) -> Result<()> {
    let mut cmd = Command::new("codesign");
    cmd.args(["--force", "--sign", identity, "--timestamp=none"]);
    if let Some(path) = entitlements {
        cmd.arg("--entitlements").arg(path);
    }
    util::run(cmd.arg(app))?;
    util::run(
        Command::new("codesign")
            .args(["--verify", "--strict", "--deep"])
            .arg(app),
    )
}

fn stage_app(
    profile_dir: &Path,
    target: Target,
    label: &str,
    music: bool,
    build_number: Option<&str>,
) -> Result<PathBuf> {
    let app = ios_build().join(target.sdk).join(format!("{APP_NAME}.app"));
    if app.exists() {
        std::fs::remove_dir_all(&app).map_err(|e| e.to_string())?;
    }
    std::fs::create_dir_all(&app).map_err(|e| e.to_string())?;
    let baked = super::bake_world_data(None, Some(&ios_build().join(super::RUST_BAKED_FILE)))?;
    // The desktop layout plan, minus native libraries (there are none to
    // embed): executable, baked world, data files, loose sounds.
    let repo = util::repo_root();
    for (source, relative) in super::plan_rust_layout(profile_dir, &repo, "ios", &baked)? {
        util::copy_file(&source, &app.join(relative))?;
    }
    super::make_executable(&app.join(APP_NAME))?;

    let assets = repo.join(super::SOURCE_ASSETS);
    let package = app.join("freight_fate");
    super::packs::require_real_pack(&assets.join("sounds.pak"))?;
    util::copy_file(&assets.join("sounds.pak"), &package.join("sounds.pak"))?;
    if music {
        super::packs::ensure_music_pack(None)?;
        util::copy_file(&assets.join("music.pak"), &package.join("music.pak"))?;
    }
    let credits = assets.join("sounds").join("CREDITS.md");
    if credits.is_file() {
        util::copy_file(&credits, &app.join("SOUND_CREDITS.md"))?;
    }
    super::stamp_build_info(&app, label)?;

    let mut info = info_plist(&super::project_version()?, target, build_number);
    info.update(compile_icon(&app, target)?);
    if !target.simulator {
        info.update(sdk_info(target)?);
    }
    util::write_text(&app.join("Info.plist"), &plist::to_xml(&info))?;
    util::write_text(&app.join("PkgInfo"), "APPL????")?;
    Ok(app)
}

/// Zip the signed app as `Payload/FreightFate.app` for App Store Connect.
fn package_ipa(app: &Path) -> Result<PathBuf> {
    let ipa = ios_build().join(format!("{APP_NAME}.ipa"));
    let _ = std::fs::remove_file(&ipa);
    let staging = tempfile::tempdir().map_err(|e| e.to_string())?;
    let payload = staging.path().join("Payload");
    std::fs::create_dir(&payload).map_err(|e| e.to_string())?;
    util::run(
        Command::new("ditto")
            .arg(app)
            .arg(payload.join(app.file_name().unwrap_or_default())),
    )?;
    util::run(
        Command::new("ditto")
            .args(["-c", "-k", "--sequesterRsrc", "--keepParent"])
            .arg(&payload)
            .arg(&ipa),
    )?;
    Ok(ipa)
}

fn simctl(args: &[&str]) -> Result<()> {
    util::run(Command::new("xcrun").arg("simctl").args(args))
}

const USAGE: &str = "usage: build-ios [--device] [--debug] [--tag LABEL] [--no-music] \
[--build-number N] [--ipa] [--sign-identity ID] [--provisioning-profile PATH] \
[--simulator UDID] [--install] [--launch] [--bundle-id]";

pub fn main(argv: &[String]) -> Result<i32> {
    let args = Args::parse(
        argv,
        &Spec {
            values: &[
                "tag",
                "build-number",
                "sign-identity",
                "provisioning-profile",
                "simulator",
            ],
            flags: &[
                "device",
                "debug",
                "no-music",
                "ipa",
                "install",
                "launch",
                "bundle-id",
            ],
        },
    )
    .map_err(|e| format!("{e}\n{USAGE}"))?;
    if args.flag("bundle-id") {
        println!("{BUNDLE_ID}");
        return Ok(0);
    }
    let device = args.flag("device");
    let target = if device { DEVICE } else { SIMULATOR };
    let identity = args.value("sign-identity");
    let profile = args.path("provisioning-profile");
    if device && (identity.is_none() || profile.is_none()) {
        return Err("--device needs --sign-identity and --provisioning-profile".into());
    }
    if device && (args.flag("install") || args.flag("launch")) {
        return Err("--install and --launch are for the Simulator".into());
    }
    if args.flag("ipa") && !device {
        return Err("--ipa is for a device build".into());
    }

    let label = match args.value("tag") {
        Some(tag) => tag.to_string(),
        None => super::project_version()?,
    };
    let profile_dir = cargo_build(target, !args.flag("debug"))?;
    let app = stage_app(
        &profile_dir,
        target,
        &label,
        !args.flag("no-music"),
        args.value("build-number"),
    )?;

    match (identity, profile) {
        (Some(identity), Some(profile)) if device => {
            util::copy_file(&profile, &app.join("embedded.mobileprovision"))?;
            let entitlements = ios_build().join("entitlements.plist");
            util::write_text(
                &entitlements,
                &plist::to_xml(&provisioning_entitlements(&profile)?),
            )?;
            codesign(&app, identity, Some(&entitlements))?;
        }
        _ => codesign(&app, "-", None)?,
    }
    println!("Built {}", app.display());
    if args.flag("ipa") {
        println!("Packaged {}", package_ipa(&app)?.display());
    }
    let simulator = args.string("simulator", "booted");
    let app_path = app.to_string_lossy();
    if args.flag("install") || args.flag("launch") {
        simctl(&["install", &simulator, &app_path])?;
    }
    if args.flag("launch") {
        simctl(&["launch", &simulator, BUNDLE_ID])?;
    }
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn store_versions_keep_three_integers() {
        assert_eq!(store_version("1.9.0.dev0"), "1.9.0");
        assert_eq!(store_version("1.9"), "1.9");
        assert_eq!(store_version("1.10rc1.2"), "1.10.2");
        assert_eq!(store_version("dev"), "0");
    }

    #[test]
    fn info_plist_declares_controllers_and_the_build_number() {
        let info = info_plist("1.9.0.dev0", DEVICE, Some("7"));
        assert_eq!(info.get("CFBundleVersion"), Some(&Plist::string("7")));
        assert_eq!(
            info.get("CFBundleShortVersionString"),
            Some(&Plist::string("1.9.0"))
        );
        let xml = plist::to_xml(&info);
        assert!(xml.contains("<string>ExtendedGamepad</string>"));
        assert!(xml.contains("<string>iPhoneOS</string>"));
        assert!(xml.contains("<string>FFSceneDelegate</string>"));
    }
}
