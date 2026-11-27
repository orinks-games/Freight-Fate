//! `build-android`: build Freight Fate for Android phones and tablets.
//!
//! The Android game is the desktop game, as on iOS: the same Rust states and
//! menus, spoken through Prism (Android's text to speech), with SDL2 running
//! the window, sound focus and game controllers. Android loads the game as
//! a library, so the crate built is `freight-fate-android` (`libmain.so`),
//! and SDL's Java activity, subclassed as `FreightFateActivity`, starts it.
//! This turns that library into an installable, signed `FreightFate.apk`:
//!
//! * `lib/<abi>/`: `libmain.so`, SDL's `libSDL2.so` (HIDAPI included), and
//!   the NDK's `libc++_shared.so` (Prism is C++), stored
//!   uncompressed and page-aligned so Android maps them in place;
//! * `classes.dex`: SDL's Java activity, Prism's Android voices (with the
//!   djinni support classes they use; the text to speech voice is the
//!   game's corrected copy, see [`JAVA_OVERRIDES`]) and
//!   `FreightFateActivity`;
//! * `assets/game/`: the desktop release layout (baked world, data files,
//!   loose sounds, the sound and music packs, `build_info.json`), which the
//!   activity unpacks into the app's private storage on first launch and
//!   again whenever `build_info.json` changes;
//! * the manifest and the launcher icon (the iOS app icon).
//!
//! The APK is signed with the Android debug key (`~/.android/debug.keystore`,
//! created on first use), which is what installing over USB needs.
//!
//! ```text
//! cargo tools build-android                    # build/android/FreightFate.apk
//! cargo tools build-android --install --launch # and run it on the phone
//! cargo tools build-android --unsealed         # a test build with no sealing key
//! ```
//!
//! Needs the Android SDK (`ANDROID_HOME`) with a build-tools release, the
//! `android-36` platform and an NDK (`ANDROID_NDK_HOME`, or the newest under
//! the SDK), a JDK 17 (`JAVA_HOME` or `javac` on the path), `cargo-ndk`
//! (`cargo install cargo-ndk`), Ninja, and `rustup target add
//! aarch64-linux-android`. `adb` comes with the SDK's platform tools.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::cli::{Args, Spec};
use crate::util::{self, Result};

mod elf;

/// The application id, kept the same as the iOS bundle id.
const APP_ID: &str = "net.orinks.freightfate";
const ACTIVITY: &str = "net.orinks.freightfate/.FreightFateActivity";
/// Android 14: the oldest release the port is tested on.
const MIN_SDK: u32 = 34;
/// The platform the Java sources compile against, and the target SDK.
const COMPILE_SDK: u32 = 36;
/// The one ABI built until other phones are tested: 64-bit ARM.
const DEFAULT_ABI: &str = "arm64-v8a";
/// Android 15 phones may use 16 KB pages; aligning to that suits 4 KB too.
const PAGE_ALIGN_KB: &str = "16";
const APK_NAME: &str = "FreightFate.apk";
const CRATE_DIR: &str = "crates/freight-fate-android";

fn android_build() -> PathBuf {
    super::build_dir().join("android")
}

/// The Rust target triple for an Android ABI.
fn triple(abi: &str) -> Result<&'static str> {
    match abi {
        "arm64-v8a" => Ok("aarch64-linux-android"),
        "x86_64" => Ok("x86_64-linux-android"),
        other => Err(format!("unsupported ABI {other}: use arm64-v8a or x86_64")),
    }
}

/// The SDK, NDK and JDK tools this build calls.
struct Toolchain {
    sdk: PathBuf,
    ndk: PathBuf,
    build_tools: PathBuf,
    android_jar: PathBuf,
    java_bin: Option<PathBuf>,
}

impl Toolchain {
    fn find() -> Result<Self> {
        let sdk = std::env::var_os("ANDROID_HOME")
            .or_else(|| std::env::var_os("ANDROID_SDK_ROOT"))
            .map(PathBuf::from)
            .ok_or("Set ANDROID_HOME to the Android SDK folder.")?;
        let ndk = match std::env::var_os("ANDROID_NDK_HOME") {
            Some(ndk) => PathBuf::from(ndk),
            None => newest_child(&sdk.join("ndk"))
                .ok_or("No NDK found: install one with sdkmanager, or set ANDROID_NDK_HOME.")?,
        };
        check_ndk_release(&ndk)?;
        let build_tools = newest_child(&sdk.join("build-tools"))
            .ok_or("No build-tools found in the Android SDK: install them with sdkmanager.")?;
        let android_jar = sdk
            .join("platforms")
            .join(format!("android-{COMPILE_SDK}"))
            .join("android.jar");
        if !android_jar.is_file() {
            return Err(format!(
                "The android-{COMPILE_SDK} platform is missing: install it with sdkmanager."
            ));
        }
        let java_bin = std::env::var_os("JAVA_HOME").map(|home| PathBuf::from(home).join("bin"));
        Ok(Self {
            sdk,
            ndk,
            build_tools,
            android_jar,
            java_bin,
        })
    }

    /// A build-tools program: `.exe` or `.bat` on Windows.
    fn tool(&self, name: &str) -> PathBuf {
        let plain = self.build_tools.join(name);
        if cfg!(windows) {
            for ext in ["exe", "bat"] {
                let candidate = plain.with_extension(ext);
                if candidate.is_file() {
                    return candidate;
                }
            }
        }
        plain
    }

    fn java_tool(&self, name: &str) -> PathBuf {
        match &self.java_bin {
            Some(bin) => bin.join(name),
            None => PathBuf::from(name),
        }
    }

    fn adb(&self) -> PathBuf {
        self.sdk.join("platform-tools").join("adb")
    }

    /// The NDK's C++ runtime for `triple`.
    fn libcxx_shared(&self, triple: &str) -> Result<PathBuf> {
        let prebuilt = self.ndk.join("toolchains").join("llvm").join("prebuilt");
        let host = newest_child(&prebuilt).ok_or("The NDK has no prebuilt toolchain.")?;
        let lib = host
            .join("sysroot")
            .join("usr")
            .join("lib")
            .join(triple)
            .join("libc++_shared.so");
        if lib.is_file() {
            Ok(lib)
        } else {
            Err(format!("The NDK has no {}", lib.display()))
        }
    }
}

/// The oldest NDK whose linker aligns libraries to 16 KB pages by default.
const OLDEST_NDK: u64 = 28;

/// Refuse an NDK older than r28, read from its `source.properties`
/// (`Pkg.Revision = 29.0.14206865`). The libraries are checked after the
/// build as well; this names the cause before a long build.
fn check_ndk_release(ndk: &Path) -> Result<()> {
    let properties = std::fs::read_to_string(ndk.join("source.properties"))
        .map_err(|e| format!("{} is not an NDK: {e}", ndk.display()))?;
    let major =
        ndk_major(&properties).ok_or_else(|| format!("{} gives no NDK release", ndk.display()))?;
    if major < OLDEST_NDK {
        return Err(format!(
            "NDK r{major} at {} links libraries that phones with 16 KB pages cannot load; \
             install NDK r{OLDEST_NDK} or newer",
            ndk.display()
        ));
    }
    Ok(())
}

fn ndk_major(properties: &str) -> Option<u64> {
    properties.lines().find_map(|line| {
        let (key, value) = line.split_once('=')?;
        (key.trim() == "Pkg.Revision")
            .then(|| value.trim().split('.').next()?.parse().ok())
            .flatten()
    })
}

/// The last entry of `dir` in version order (`36.1.0` after `36.0.0`).
fn newest_child(dir: &Path) -> Option<PathBuf> {
    let mut children: Vec<PathBuf> = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect();
    children.sort_by_key(|path| version_key(path));
    children.pop()
}

fn version_key(path: &Path) -> Vec<u64> {
    path.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .split(|c: char| !c.is_ascii_digit())
        .filter(|part| !part.is_empty())
        .map(|part| part.parse().unwrap_or(0))
        .collect()
}

/// `versionCode` from a version label: `1.9.7` is 1090700, and a snapshot's
/// `.devN` adds N, so each snapshot installs over the one before it.
fn version_code(label: &str) -> u32 {
    let mut numbers = label
        .trim_start_matches('v')
        .split(|c: char| !c.is_ascii_digit())
        .filter(|part| !part.is_empty())
        .map(|part| part.parse::<u32>().unwrap_or(0));
    let major = numbers.next().unwrap_or(0);
    let minor = numbers.next().unwrap_or(0);
    let patch = numbers.next().unwrap_or(0);
    let extra = numbers.next().unwrap_or(0).min(99);
    major * 1_000_000 + minor.min(99) * 10_000 + patch.min(99) * 100 + extra
}

/// The workspace's target folder, as cargo-ndk will use it.
fn target_dir() -> PathBuf {
    std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| util::repo_root().join("target"))
}

fn cargo_build(
    tools: &Toolchain,
    abi: &str,
    release: bool,
    sealed: bool,
    cargo_config: Option<&str>,
) -> Result<PathBuf> {
    let root = util::repo_root();
    let mut cmd = Command::new("cargo");
    cmd.args(["ndk", "-t", abi, "-P", &MIN_SDK.to_string(), "build"]);
    if let Some(config) = cargo_config {
        cmd.args(["--config", config]);
    }
    cmd.args(["-p", "freight-fate-android"]);
    if release {
        cmd.arg("--release");
    }
    if sealed {
        super::seal::configure(&mut cmd)?;
    } else {
        cmd.env_remove("HEARKEN_SEAL_KEY");
    }
    let cmake_dir = root.join(CRATE_DIR).join("cmake");
    let toolchain_file = cmake_dir.join("sdl2-android-toolchain.cmake");
    // CMake's own Android support (Prism) reads ANDROID_NDK; the NDK's
    // toolchain file (SDL2, through SDL2_TOOLCHAIN) reads the rest.
    cmd.current_dir(&root)
        .env("ANDROID_NDK_HOME", &tools.ndk)
        .env("ANDROID_NDK_ROOT", &tools.ndk)
        .env("ANDROID_NDK", &tools.ndk)
        .env(
            "SDL2_TOOLCHAIN",
            toolchain_file.to_string_lossy().replace('\\', "/"),
        )
        .env("FF_ANDROID_ABI", abi)
        .env("FF_ANDROID_PLATFORM", MIN_SDK.to_string());
    // Visual Studio's NMake cannot build for Android; Ninja can.
    if std::env::var_os("CMAKE_GENERATOR").is_none() {
        cmd.env("CMAKE_GENERATOR", "Ninja");
    }
    let triple = triple(abi)?;
    let profile_dir = target_dir()
        .join(triple)
        .join(if release { "release" } else { "debug" });
    let sdl_settings = sdl_settings_hash(&cmake_dir)?;
    let stamp = profile_dir.join(SDL_SETTINGS_STAMP);
    if std::fs::read_to_string(&stamp).ok().as_deref() != Some(sdl_settings.as_str()) {
        clean_sdl(triple, release, cargo_config)?;
    }
    util::run(&mut cmd)?;
    std::fs::write(&stamp, &sdl_settings).map_err(|e| format!("{}: {e}", stamp.display()))?;
    Ok(profile_dir)
}

/// Beside each Android build: the hash of the `cmake/` files its SDL2 was
/// built with.
const SDL_SETTINGS_STAMP: &str = "sdl2-settings.sha256";

/// One hash over the files in `cmake/`, which shape the SDL2 that sdl2-sys
/// builds. Its build script names nothing to rerun on, so Cargo would keep
/// an SDL built with older ones; [`cargo_build`] cleans sdl2-sys when this
/// changes.
fn sdl_settings_hash(cmake_dir: &Path) -> Result<String> {
    use sha2::{Digest, Sha256};
    let mut files: Vec<PathBuf> = std::fs::read_dir(cmake_dir)
        .map_err(|e| format!("{}: {e}", cmake_dir.display()))?
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|path| path.is_file())
        .collect();
    files.sort();
    let mut hasher = Sha256::new();
    for file in &files {
        hasher.update(
            file.file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .as_bytes(),
        );
        hasher.update([0]);
        hasher.update(util::file_sha256(file)?.as_bytes());
        hasher.update([0]);
    }
    Ok(hex::encode(hasher.finalize()))
}

/// Make the next build compile SDL2 again.
fn clean_sdl(triple: &str, release: bool, cargo_config: Option<&str>) -> Result<()> {
    println!("SDL2's build settings changed; rebuilding SDL2");
    let mut cmd = Command::new("cargo");
    cmd.arg("clean");
    if let Some(config) = cargo_config {
        cmd.args(["--config", config]);
    }
    cmd.args(["-p", "sdl2-sys", "--target", triple]);
    if release {
        cmd.arg("--release");
    }
    cmd.current_dir(util::repo_root());
    util::run(&mut cmd)
}

/// The native libraries the APK carries, by file name.
fn native_libraries(tools: &Toolchain, profile_dir: &Path, abi: &str) -> Result<Vec<PathBuf>> {
    let mut libs = Vec::new();
    for name in ["libmain.so", "libSDL2.so"] {
        let lib = profile_dir.join(name);
        if !lib.is_file() {
            return Err(format!("cargo build left no {}", lib.display()));
        }
        libs.push(lib);
    }
    libs.push(tools.libcxx_shared(triple(abi)?)?);
    for lib in &libs {
        let bytes = std::fs::read(lib).map_err(|e| format!("{}: {e}", lib.display()))?;
        let name = lib.file_name().unwrap_or_default().to_string_lossy();
        elf::check_page_alignment(&name, &bytes)?;
    }
    Ok(libs)
}

/// `assets/game`: the desktop release layout, minus the executable.
fn stage_game_files(assets: &Path, label: &str, music: bool) -> Result<()> {
    let game = assets.join("game");
    if game.exists() {
        std::fs::remove_dir_all(&game).map_err(|e| e.to_string())?;
    }
    let baked = super::bake_world_data(None, Some(&android_build().join(super::RUST_BAKED_FILE)))?;
    // The same sources `plan_rust_layout` reads: data files and loose sounds
    // relative to the repository, the packs from `assets/`.
    let repo = util::repo_root();
    let assets_dir = repo.join(super::SOURCE_ASSETS);
    let package = game.join("freight_fate");
    util::copy_file(&baked, &package.join("data").join(super::RUST_BAKED_FILE))?;
    for relative in super::RUST_DATA_FILES {
        util::copy_file(
            &repo.join("data").join(relative),
            &package.join("data").join(relative),
        )?;
    }
    for relative in super::rust_loose_sound_files(&repo)? {
        util::copy_file(&repo.join(&relative), &package.join(&relative))?;
    }
    super::packs::require_real_pack(&assets_dir.join("sounds.pak"))?;
    util::copy_file(&assets_dir.join("sounds.pak"), &package.join("sounds.pak"))?;
    if music {
        super::packs::ensure_music_pack(None)?;
        util::copy_file(&assets_dir.join("music.pak"), &package.join("music.pak"))?;
    }
    let credits = assets_dir.join("sounds").join("CREDITS.md");
    if credits.is_file() {
        util::copy_file(&credits, &game.join("SOUND_CREDITS.md"))?;
    }
    // Last: the activity compares this file to decide whether to unpack.
    super::stamp_build_info(&game, label)
}

/// The source folder of a package in the build, from `cargo metadata`,
/// read with the same cargo config as the build so a patched dependency's
/// Java sources are the ones packaged.
fn package_source(name: &str, cargo_config: Option<&str>) -> Result<PathBuf> {
    let mut cmd = Command::new("cargo");
    cmd.args(["metadata", "--format-version", "1"]);
    if let Some(config) = cargo_config {
        cmd.args(["--config", config]);
    }
    let json = util::output(cmd.current_dir(util::repo_root()))?;
    let metadata: serde_json::Value = serde_json::from_str(&json).map_err(|e| e.to_string())?;
    metadata["packages"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|package| package["name"] == name)
        .and_then(|package| package["manifest_path"].as_str())
        .and_then(|manifest| Path::new(manifest).parent().map(Path::to_path_buf))
        .ok_or_else(|| format!("cargo metadata does not list {name}"))
}

fn java_sources(cargo_config: Option<&str>) -> Result<Vec<PathBuf>> {
    let sdl_crate = package_source("sdl2-sys", cargo_config)?;
    let prism_crate = package_source("prism-sys", cargo_config)?;
    let (overridden, overrides) = java_overrides(&sdl_crate, &prism_crate)?;
    let prism = prism_crate.join("prism");
    let roots = [
        sdl_crate.join(SDL_JAVA),
        prism.join("source").join("backends").join("android"),
        prism
            .join("third_party")
            .join("djinni")
            .join("support")
            .join("java"),
        util::repo_root().join(CRATE_DIR).join("java"),
    ];
    let mut sources = Vec::new();
    for root in &roots {
        if !root.is_dir() {
            return Err(format!("Java sources are missing: {}", root.display()));
        }
        sources.extend(
            util::walk_files(root)
                .into_iter()
                .filter(|path| path.extension().is_some_and(|ext| ext == "java"))
                .filter(|path| !overridden.contains(path)),
        );
    }
    sources.extend(overrides);
    Ok(sources)
}

/// SDL's Android Java, under the sdl2-sys crate.
const SDL_JAVA: &str = "SDL/android-project/app/src/main/java";

/// Where a library's file comes from.
#[derive(Clone, Copy)]
enum Library {
    Sdl,
    Prism,
}

/// One library file the game compiles its own copy of, from
/// `crates/freight-fate-android/<folder>/`.
struct JavaOverride {
    library: Library,
    /// The library's file, under its crate's folder.
    path: &'static str,
    /// The SHA-256 of the library file the copy was made from.
    made_from: &'static str,
    /// The folder of the copy, under the Android crate.
    folder: &'static str,
}

/// The library files the game compiles its own copies of.
///
/// - Prism's text to speech voice. Prism 0.1.6 centres the rate and pitch
///   scales on 3.05 and 2.125 rather than Android's normal 1.0, so the
///   game's 50% pitch spoke like a chipmunk; and it reaches only the phone's
///   preferred engine. The copy fixes the centre, lists every installed
///   engine's voices named "engine: language: voice", and moves to another
///   engine when one of its voices is chosen. It stays until Prism itself
///   does both.
/// - SDL 2.26's HIDAPI device manager. Android 14 refuses its broadcast
///   receivers (registered without the exported flag) and its USB
///   permission request (a mutable PendingIntent with an implicit Intent),
///   which left HIDAPI off, and with it PlayStation controller rumble. The
///   copy makes the two changes later SDL releases made. It stays until
///   sdl2-sys bundles an SDL with them.
/// - SDL 2.26's controller manager. It opens any device that reports a
///   D-pad as a joystick, and Android lists a Bluetooth keyboard as one for
///   its arrow keys, so the game said "Controller connected" with the
///   keyboard's name and its arrows arrived as D-pad buttons. A keyboard
///   with a volume wheel reports a joystick axis too. The copy leaves out a
///   device with a full letter keyboard and no stick, hat or trigger axis.
const JAVA_OVERRIDES: &[JavaOverride] = &[
    JavaOverride {
        library: Library::Prism,
        path: "prism/source/backends/android/AndroidTextToSpeechBackend.java",
        made_from: "71a123b679702801030e47ba26e2fe8cf71179700824aec825b8df6107be6dc0",
        folder: "prism",
    },
    JavaOverride {
        library: Library::Sdl,
        path: "SDL/android-project/app/src/main/java/org/libsdl/app/HIDDeviceManager.java",
        made_from: "0b571eb0a49fb3a01505b5066b2e8d7aafa257cf1fa5393fca42a22bc5960cda",
        folder: "sdl",
    },
    JavaOverride {
        library: Library::Sdl,
        path: "SDL/android-project/app/src/main/java/org/libsdl/app/SDLControllerManager.java",
        made_from: "6487643ae8ab90510f924331a24f8177b485ec997babf8fbd90596917a2bb1dc",
        folder: "sdl",
    },
];

/// The library sources to leave out, and the game's copies to compile in
/// their place. Refuses when a library's file is not the one its copy was
/// made from: a new SDL or Prism needs its changes carried into the copy
/// (or the copy dropped) before it ships.
fn java_overrides(sdl: &Path, prism: &Path) -> Result<(Vec<PathBuf>, Vec<PathBuf>)> {
    let crate_dir = util::repo_root().join(CRATE_DIR);
    let mut overridden = Vec::new();
    let mut copies = Vec::new();
    for item in JAVA_OVERRIDES {
        let root = match item.library {
            Library::Sdl => sdl,
            Library::Prism => prism,
        };
        let original = root.join(item.path);
        let found = util::file_sha256(&original)?;
        if found != item.made_from {
            return Err(format!(
                "{} is not the file the copy in {}/{} was made from (SHA-256 {found}); \
                 carry its changes into the copy, or drop the copy if the library \
                 now has the fixes, then update JAVA_OVERRIDES",
                original.display(),
                CRATE_DIR,
                item.folder
            ));
        }
        let name = Path::new(item.path)
            .file_name()
            .ok_or_else(|| format!("no file name in {}", item.path))?;
        overridden.push(original);
        copies.push(crate_dir.join(item.folder).join(name));
    }
    Ok((overridden, copies))
}

/// Compile the Java sources and dex them into `classes.dex`.
fn build_dex(tools: &Toolchain, work: &Path, cargo_config: Option<&str>) -> Result<PathBuf> {
    let classes = work.join("classes");
    let dex = work.join("dex");
    for dir in [&classes, &dex] {
        if dir.exists() {
            std::fs::remove_dir_all(dir).map_err(|e| e.to_string())?;
        }
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let sources = java_sources(cargo_config)?;
    let list = work.join("java-sources.txt");
    let quoted: Vec<String> = sources
        .iter()
        .map(|path| format!("\"{}\"", path.to_string_lossy().replace('\\', "/")))
        .collect();
    util::write_text(&list, &quoted.join("\n"))?;
    util::run(
        Command::new(tools.java_tool("javac"))
            .args([
                "-source",
                "17",
                "-target",
                "17",
                "-nowarn",
                "-encoding",
                "UTF-8",
            ])
            .arg("-classpath")
            .arg(&tools.android_jar)
            .arg("-d")
            .arg(&classes)
            .arg(format!("@{}", list.display())),
    )?;
    let class_files: Vec<PathBuf> = util::walk_files(&classes)
        .into_iter()
        .filter(|path| path.extension().is_some_and(|ext| ext == "class"))
        .collect();
    let class_list = work.join("class-files.txt");
    let quoted: Vec<String> = class_files
        .iter()
        .map(|path| path.to_string_lossy().replace('\\', "/"))
        .collect();
    util::write_text(&class_list, &quoted.join("\n"))?;
    util::run(
        Command::new(tools.tool("d8"))
            .args(["--release", "--min-api", &MIN_SDK.to_string(), "--lib"])
            .arg(&tools.android_jar)
            .arg("--output")
            .arg(&dex)
            .arg(format!("@{}", class_list.display())),
    )?;
    Ok(dex.join("classes.dex"))
}

/// Link the manifest and icon into an unsigned, unaligned APK. A test
/// build is debuggable, so `adb shell run-as` can read its log and saves.
fn link_resources(
    tools: &Toolchain,
    work: &Path,
    label: &str,
    debuggable: bool,
) -> Result<PathBuf> {
    let res = work.join("res");
    let icon = util::repo_root()
        .join("crates")
        .join("freight-fate")
        .join("ios")
        .join("Assets.xcassets")
        .join("AppIcon.appiconset")
        .join("AppIcon-1024.png");
    util::copy_file(&icon, &res.join("mipmap-xxxhdpi").join("ic_launcher.png"))?;
    // The crate's own resources: the data extraction rules.
    let own = util::repo_root().join(CRATE_DIR).join("res");
    for file in util::walk_files(&own) {
        util::copy_file(&file, &res.join(util::rel(&file, &own)))?;
    }
    let compiled = work.join("res.zip");
    util::run(
        Command::new(tools.tool("aapt2"))
            .arg("compile")
            .arg("--dir")
            .arg(&res)
            .arg("-o")
            .arg(&compiled),
    )?;
    let apk = work.join("linked.apk");
    let mut link = Command::new(tools.tool("aapt2"));
    link.arg("link");
    if debuggable {
        link.arg("--debug-mode");
    }
    util::run(
        link.arg("-o")
            .arg(&apk)
            .arg("-I")
            .arg(&tools.android_jar)
            .arg("--manifest")
            .arg(
                util::repo_root()
                    .join(CRATE_DIR)
                    .join("AndroidManifest.xml"),
            )
            .args(["--min-sdk-version", &MIN_SDK.to_string()])
            .args(["--target-sdk-version", &COMPILE_SDK.to_string()])
            .args(["--version-code", &version_code(label).to_string()])
            .args(["--version-name", label.trim_start_matches('v')])
            .arg(&compiled),
    )?;
    Ok(apk)
}

/// Add `classes.dex`, the native libraries and the game files to the
/// linked APK. The game files are added here rather than by aapt2, whose
/// Windows build names nested assets with backslashes, which Android reads
/// as part of a file name, not as folders.
fn add_code(apk: &Path, dex: &Path, abi: &str, libs: &[PathBuf], assets: &Path) -> Result<()> {
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(apk)
        .map_err(|e| e.to_string())?;
    let mut zip = zip::ZipWriter::new_append(file).map_err(|e| e.to_string())?;
    let deflated = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    // Native libraries stay uncompressed: Android maps them straight out of
    // the APK (zipalign page-aligns them below).
    let stored =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    let mut entries = vec![(dex.to_path_buf(), "classes.dex".to_string(), deflated)];
    for lib in libs {
        let name = lib.file_name().unwrap_or_default().to_string_lossy();
        entries.push((lib.clone(), format!("lib/{abi}/{name}"), stored));
    }
    for file in util::walk_files(assets) {
        // `rel` joins with forward slashes, the zip separator, on every host.
        let relative = util::rel(&file, assets);
        // The packs and the world container are compressed already; storing
        // them makes the first-launch unpack a plain copy.
        let compressed = file
            .extension()
            .is_some_and(|ext| ext == "pak" || ext == "ffdata");
        entries.push((
            file.clone(),
            format!("assets/{relative}"),
            if compressed { stored } else { deflated },
        ));
    }
    for (source, name, options) in entries {
        let mut bytes = Vec::new();
        std::fs::File::open(&source)
            .and_then(|mut f| f.read_to_end(&mut bytes))
            .map_err(|e| format!("{}: {e}", source.display()))?;
        zip.start_file(name, options).map_err(|e| e.to_string())?;
        zip.write_all(&bytes).map_err(|e| e.to_string())?;
    }
    zip.finish().map_err(|e| e.to_string())?;
    Ok(())
}

/// The Android debug key, created the way Android Studio creates it.
fn debug_keystore(tools: &Toolchain) -> Result<PathBuf> {
    #[allow(deprecated)]
    let home = std::env::home_dir().ok_or("No home folder for the debug keystore.")?;
    let keystore = home.join(".android").join("debug.keystore");
    if !keystore.is_file() {
        std::fs::create_dir_all(keystore.parent().unwrap_or(&home)).map_err(|e| e.to_string())?;
        util::run(
            Command::new(tools.java_tool("keytool"))
                .args(["-genkeypair", "-keystore"])
                .arg(&keystore)
                .args([
                    "-storepass",
                    "android",
                    "-keypass",
                    "android",
                    "-alias",
                    "androiddebugkey",
                    "-keyalg",
                    "RSA",
                    "-keysize",
                    "2048",
                    "-validity",
                    "10000",
                    "-dname",
                    "CN=Android Debug,O=Android,C=US",
                ]),
        )?;
    }
    Ok(keystore)
}

fn align_and_sign(tools: &Toolchain, linked: &Path, out: &Path) -> Result<()> {
    let aligned = linked.with_file_name("aligned.apk");
    util::run(
        Command::new(tools.tool("zipalign"))
            .args(["-f", "-P", PAGE_ALIGN_KB, "4"])
            .arg(linked)
            .arg(&aligned),
    )?;
    let keystore = debug_keystore(tools)?;
    util::run(
        Command::new(tools.tool("apksigner"))
            .arg("sign")
            .arg("--ks")
            .arg(&keystore)
            .args(["--ks-pass", "pass:android", "--key-pass", "pass:android"])
            .arg("--out")
            .arg(out)
            .arg(&aligned),
    )
}

const USAGE: &str = "usage: build-android [--debug] [--unsealed] [--tag LABEL] [--no-music] \
[--abi arm64-v8a|x86_64] [--cargo-config PATH] [--install] [--launch] [--app-id]";

const SPEC: Spec = Spec {
    values: &["tag", "abi", "cargo-config"],
    flags: &[
        "debug", "unsealed", "no-music", "install", "launch", "app-id",
    ],
};

pub fn main(argv: &[String]) -> Result<i32> {
    let args = Args::parse(argv, &SPEC).map_err(|e| format!("{e}\n{USAGE}"))?;
    if args.flag("app-id") {
        println!("{APP_ID}");
        return Ok(0);
    }
    let abi = args.string("abi", DEFAULT_ABI);
    triple(&abi)?;
    let tools = Toolchain::find()?;
    let label = match args.value("tag") {
        Some(tag) => tag.to_string(),
        None => super::project_version()?,
    };
    let release = !args.flag("debug");
    // A player build carries the career sealing key, as on every platform.
    // A test build for one phone may leave it out; its careers then open
    // only in other unsealed builds.
    let sealed = release && !args.flag("unsealed");
    let profile_dir = cargo_build(&tools, &abi, release, sealed, args.value("cargo-config"))?;
    let libs = native_libraries(&tools, &profile_dir, &abi)?;

    let work = android_build().join("work");
    std::fs::create_dir_all(&work).map_err(|e| e.to_string())?;
    let assets = work.join("assets");
    stage_game_files(&assets, &label, !args.flag("no-music"))?;
    let dex = build_dex(&tools, &work, args.value("cargo-config"))?;
    let linked = link_resources(&tools, &work, &label, !sealed)?;
    add_code(&linked, &dex, &abi, &libs, &assets)?;
    let apk = android_build().join(APK_NAME);
    align_and_sign(&tools, &linked, &apk)?;
    println!("Built {}", apk.display());
    if !sealed {
        println!(
            "This build has no career sealing key: careers it saves open only in other unsealed builds."
        );
    }

    if args.flag("install") || args.flag("launch") {
        util::run(Command::new(tools.adb()).args(["install", "-r"]).arg(&apk))?;
    }
    if args.flag("launch") {
        util::run(Command::new(tools.adb()).args(["shell", "am", "start", "-n", ACTIVITY]))?;
    }
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_codes_order_releases_and_snapshots() {
        assert_eq!(version_code("1.9.7"), 1_090_700);
        assert_eq!(version_code("v1.9.7"), 1_090_700);
        assert_eq!(version_code("1.9.7.dev3"), 1_090_703);
        assert!(version_code("1.10.0") > version_code("1.9.99.dev9"));
        assert_eq!(version_code("dev"), 0);
    }

    #[test]
    fn newer_tool_versions_sort_last() {
        let mut paths = [
            PathBuf::from("36.1.0"),
            PathBuf::from("9.0.0"),
            PathBuf::from("36.0.0"),
        ];
        paths.sort_by_key(|path| version_key(path));
        assert_eq!(paths.last(), Some(&PathBuf::from("36.1.0")));
    }

    #[test]
    fn only_tested_abis_are_accepted() {
        assert_eq!(triple("arm64-v8a"), Ok("aarch64-linux-android"));
        assert!(triple("armeabi-v7a").is_err());
    }

    #[test]
    fn the_ndk_release_comes_from_its_revision() {
        let r29 = "Pkg.Desc = Android NDK\nPkg.Revision = 29.0.14206865\nPkg.ReleaseName = r29\n";
        assert_eq!(ndk_major(r29), Some(29));
        assert_eq!(ndk_major("Pkg.Revision=27.1.12297006"), Some(27));
        assert_eq!(ndk_major("Pkg.Desc = Android NDK"), None);
    }
}
