//! `build-release`: build a standalone Freight Fate distribution.
//!
//! Packages the Rust game and archives it for release:
//!
//! * Windows: `dist/FreightFate-<label>-windows-portable.zip`
//! * Linux: `dist/FreightFate-<label>-linux-x64.tar.gz` on x86_64, and
//!   `-linux-arm64.tar.gz` on aarch64 (the Blazie BT Speak and BT Braille,
//!   Raspberry Pi and other ARM64 Linux machines)
//! * macOS Apple Silicon: `dist/FreightFate-<label>-macos-arm64.zip`
//! * macOS Intel: `dist/FreightFate-<label>-macos.zip`
//!
//! `<label>` is the project version ([`project_version`]), or `--tag`
//! (used for developer snapshots).
//!
//! The pipeline is `cargo build --release -p freight-fate
//! --no-default-features --features visuals` (no agent server in a player
//! build, and the visual display named, since it is one of the defaults),
//! then `ff-bake` to turn the JSON data tree into `world.ffdata`, then a
//! `FreightFate/` folder with the executable renamed to `FreightFate`. On
//! macOS it creates `FreightFate.app` with the executable under
//! `Contents/MacOS`, native libraries under `Contents/Frameworks`, and data,
//! packs, build metadata and documents under `Contents/Resources`. With
//! `MACOS_SIGN_IDENTITY` set the app is signed with that Developer ID under
//! the hardened runtime, and with the `MACOS_NOTARY_*` variables also set it
//! is notarized and stapled before it is archived; without them it is
//! ad-hoc signed for local use.

pub mod appimage;
mod archive;
pub mod ios;
pub mod ios_signing;
mod macos;
mod packs;
pub mod plist;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use regex::Regex;

use crate::cli::{Args, Spec};
use crate::pyjson::{self, Object, Value};
use crate::util::{self, rel, Result};
use crate::windows_runtime;

pub use archive::verify_archive;
pub use packs::require_real_pack;

pub const APP_NAME: &str = "FreightFate";
/// The checkout's packs sit under `assets/`; a release puts them in the
/// package folder, `freight_fate/`.
pub const SOURCE_ASSETS: &str = "assets";
/// `[[bin]] name` in crates/freight-fate/Cargo.toml. The staged copy is
/// renamed to [`APP_NAME`]: the updater accepts the stem case-insensitively,
/// but the apply script, `extracted_root` and [`verify_archive`] all spell it
/// `FreightFate`.
const RUST_PACKAGE: &str = "freight-fate";
const RUST_BIN_STEM: &str = "freightfate";
/// The baked binary data container. `ff-bake` builds it out of the JSON
/// tree through the game's own loaders, and the release ships it INSTEAD of
/// that tree: 142 MB of JSON becomes roughly 7 MB, and the game maps it
/// rather than parsing 94 MB of leg shards before the menu.
pub const RUST_BAKED_FILE: &str = "world.ffdata";
/// Where the container sits inside the staged payload, relative to its root.
pub const RUST_BAKED_FILE_ENTRY: &str = "freight_fate/data/world.ffdata";
/// The container's first eight bytes (`ff_core::data::baked::MAGIC`).
const BAKED_MAGIC: &[u8] = b"FFDATA\x00\x00";
/// The runtime JSON files the container replaces. Nothing here is staged;
/// the list is what [`verify_rust_payload`] checks did NOT leak into the
/// payload, so a half-migrated build that ships both is caught.
const RUST_BAKED_SOURCE_FILES: &[&str] = &[
    "border_yards.json",
    "buffs.json",
    "carriers.json",
    "channel3000.json",
    "city_services.json",
    "facility_approaches.json",
    "facility_endpoints.json",
    "local_approaches.json",
    "local_geometry.json",
    "radio_catalog.json",
    "radio_community.json",
    "radio_imported.json",
    "street_limits.json",
    "world_data/index.json",
    "world_data/geo.json",
    "world_data/us/cities.json",
    "world_data/ca/cities.json",
    "world_data/us/gameplay/curves.jsonl",
    "world_data/us/gameplay/curve_artifacts.jsonl",
];
/// Loose runtime data files still shipped as files, because the container
/// does not cover them. Empty today -- everything the game loads is baked --
/// and kept so a new runtime data file has somewhere to be registered.
const RUST_DATA_FILES: &[&str] = &[];
/// The committed loose sound tree travels as editable files: the asset
/// loader reads it beside the pack. The licensed overlay never ships loose.
const LOOSE_SOUND_TREE: &str = "assets/sounds";
const LICENSED_SOUND_TREE: &str = "sounds-licensed";
/// Build-only leftovers in the Cargo profile directory that carry a native
/// suffix on some platforms but are not runtime libraries.
const CARGO_NON_RUNTIME_SUFFIXES: &[&str] = &["pdb", "d", "rlib", "lib", "exp"];
/// The packs an update skips: most of the download, and they rarely change
/// between builds. The in-game updater fetches the rest of a zip release by
/// byte range and keeps the player's own packs when their size and CRC-32
/// match the zip's index; build_info.json records both.
pub const UPDATE_SKIPPED_PACKS: &[&str] =
    &["freight_fate/music.pak", "freight_fate/channel3000.pak"];

/// Where the checkout keeps its build output.
pub fn dist_dir() -> PathBuf {
    util::repo_root().join("dist")
}

pub fn build_dir() -> PathBuf {
    util::repo_root().join("build")
}

/// The folder `build-release` stages the game into, `build/FreightFate`.
/// On Linux and Windows the executable sits inside it under the same name.
pub fn staged_dir() -> PathBuf {
    build_dir().join(APP_NAME)
}

/// The version a developer snapshot reports: `[workspace.metadata.release]
/// package-version` in the root Cargo.toml (`1.9.5.dev0`). It carries the
/// `.devN` the crate version cannot, so a snapshot never calls itself the
/// stable release.
pub fn project_version() -> Result<String> {
    let manifest = util::read_text(&util::repo_root().join("Cargo.toml"))?;
    let mut in_table = false;
    for line in manifest.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_table = line == "[workspace.metadata.release]";
            continue;
        }
        if in_table {
            if let Some(value) = line.strip_prefix("package-version") {
                let value = value.trim_start().trim_start_matches('=').trim();
                return Ok(value.trim_matches('"').to_string());
            }
        }
    }
    Err("Cargo.toml has no [workspace.metadata.release] package-version".into())
}

/// `version`: print the project version, or with `--release-line` the
/// version without its `.devN` suffix.
pub fn version_main(argv: &[String]) -> Result<i32> {
    let args = Args::parse(
        argv,
        &Spec {
            values: &[],
            flags: &["release-line"],
        },
    )
    .map_err(|e| e.to_string())?;
    let version = project_version()?;
    if args.flag("release-line") {
        println!("{}", version.split(".dev").next().unwrap_or(&version));
    } else {
        println!("{version}");
    }
    Ok(0)
}

fn platform_native_exts(platform: &str) -> &'static [&'static str] {
    match platform {
        "win32" => &["dll"],
        "darwin" => &["dylib"],
        "ios" => &["framework"],
        _ => &["so"],
    }
}

fn runtime_root(build_dir: &Path) -> PathBuf {
    if is_app(build_dir) {
        build_dir.join("Contents").join("MacOS")
    } else {
        build_dir.to_path_buf()
    }
}

fn is_app(path: &Path) -> bool {
    path.extension().is_some_and(|e| e == "app")
}

fn ext_lower(path: &Path) -> String {
    path.extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default()
}

/// The executable file name the staged build ships.
pub fn rust_exe_name(platform: &str) -> String {
    format!(
        "{APP_NAME}{}",
        if platform == "win32" { ".exe" } else { "" }
    )
}

/// The executable file name `cargo build` writes.
pub fn cargo_exe_name(platform: &str) -> String {
    format!(
        "{RUST_BIN_STEM}{}",
        if platform == "win32" { ".exe" } else { "" }
    )
}

/// True for public 1.8 nightlies and Career 1.9 tester prereleases.
pub fn is_snapshot_label(label: &str) -> bool {
    if label.starts_with("nightly-") {
        return true;
    }
    label
        .strip_prefix("1.9-tester-")
        .is_some_and(|d| d.len() == 8 && d.bytes().all(|b| b.is_ascii_digit()))
}

/// The version a build labelled `label` is: the label's own for a stable
/// (`v1.9.1` or `1.9.1`), else the project version.
pub fn label_package_version(label: &str) -> Result<String> {
    if is_snapshot_label(label) {
        project_version()
    } else {
        Ok(label.strip_prefix('v').unwrap_or(label).to_string())
    }
}

/// The checked-out commit, so the updater can tell a same-day rebuild under
/// one snapshot tag from the copy it replaced; "" outside git.
fn build_commit() -> String {
    util::git(&util::repo_root(), &["rev-parse", "HEAD"]).unwrap_or_default()
}

fn file_crc32(path: &Path) -> Result<u32> {
    use std::io::Read;
    let mut file = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut hasher = crc32fast::Hasher::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = file
            .read(&mut buf)
            .map_err(|e| format!("{}: {e}", path.display()))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher.finalize())
}

/// Record what this build is, for the in-game updater.
///
/// `package_version` is what the game reports as its own version, and what
/// the updater compares releases against. A snapshot carries the project
/// version, since its label is a date-stamped tag. A stable build carries
/// the label's version: the `v1.9.0` tag push once built a game that called
/// itself a development build and was offered 1.9.0 again on every start.
pub fn stamp_build_info(root: &Path, label: &str) -> Result<()> {
    let snapshot = is_snapshot_label(label);
    let mut info = Object::new();
    info.insert(
        "tag",
        Value::from(if snapshot {
            label.to_string()
        } else {
            format!("v{}", label.strip_prefix('v').unwrap_or(label))
        }),
    );
    info.insert(
        "channel",
        Value::from(if snapshot { "dev" } else { "stable" }),
    );
    info.insert(
        "built_at",
        Value::from(chrono::Utc::now().format("%Y-%m-%d").to_string()),
    );
    info.insert(
        "package_version",
        Value::from(label_package_version(label)?),
    );
    info.insert("commit", Value::from(build_commit()));
    let mut packs = Vec::new();
    for relative in UPDATE_SKIPPED_PACKS {
        let path = root.join(relative);
        if path.is_file() {
            let size = path.metadata().map_err(|e| e.to_string())?.len();
            let mut record = Object::new();
            record.insert("path", Value::from(*relative));
            record.insert("size", Value::from(size));
            record.insert("crc32", Value::from(file_crc32(&path)? as u64));
            packs.push(Value::Object(record));
        }
    }
    if !packs.is_empty() {
        info.insert("packs", Value::Array(packs));
    }
    util::write_text(
        &root.join("build_info.json"),
        &pyjson::dumps(&Value::Object(info), &pyjson::Options::indent(2)),
    )
}

/// Copy player-facing release documents into the packaged runtime.
fn stage_release_docs(root: &Path) -> Result<()> {
    let repo = util::repo_root();
    let changelog = repo.join("CHANGELOG.md");
    if !changelog.exists() {
        return Err(format!("Changelog was not found: {}", changelog.display()));
    }
    util::copy_file(&changelog, &root.join("CHANGELOG.md"))?;
    let license = repo.join("LICENSE");
    if !license.exists() {
        return Err(format!("License was not found: {}", license.display()));
    }
    // PolyForm's Notices term expects the terms to travel with every copy.
    util::copy_file(&license, &root.join("LICENSE.txt"))?;
    let manual = repo.join("docs").join("user-manual.md");
    if !manual.exists() {
        return Err(format!("User manual was not found: {}", manual.display()));
    }
    util::copy_file(&manual, &root.join("USER_MANUAL.md"))?;
    // Also ship a browser-friendly, accessible HTML rendering of the manual.
    let html = crate::manual_html::markdown_to_html(
        &util::read_text(&manual)?,
        "Freight Fate Player Manual",
    );
    util::write_text(&root.join("USER_MANUAL.html"), &html)
    // The translator's kit is its own release asset (`translation-kit`),
    // not part of the game: a player never reads it.
}

/// Stage the approved packs and keep the credits readable.
fn stage_sound_pack(root: &Path) -> Result<()> {
    let assets = util::repo_root().join(SOURCE_ASSETS);
    let package = root.join("freight_fate");
    for pack in ["sounds.pak", "music.pak", "channel3000.pak"] {
        util::copy_file(&assets.join(pack), &package.join(pack))?;
    }
    let credits = assets.join("sounds").join("CREDITS.md");
    if !credits.exists() {
        return Err(format!(
            "Sound credits were not found: {}",
            credits.display()
        ));
    }
    util::copy_file(&credits, &root.join("SOUND_CREDITS.md"))
}

fn cargo_build_command(target_dir: Option<&Path>) -> Command {
    let mut cmd = Command::new("cargo");
    // No default features: the agent server is dev tooling, and a player has
    // no use for an agent at the wheel. The visual display is a default too,
    // so it is named back in.
    cmd.args([
        "build",
        "--release",
        "-p",
        RUST_PACKAGE,
        "--no-default-features",
        "--features",
        "visuals",
    ]);
    if let Some(dir) = target_dir {
        cmd.arg("--target-dir").arg(dir);
    }
    cmd.current_dir(util::repo_root());
    cmd
}

fn cargo_profile_dir(target_dir: Option<&Path>) -> PathBuf {
    target_dir
        .map(Path::to_path_buf)
        .unwrap_or_else(|| util::repo_root().join("target"))
        .join("release")
}

/// Build the baked data container and return where it landed. Runs before
/// staging, so a data tree that fails to load fails the build here rather
/// than in front of a player.
pub fn bake_world_data(target_dir: Option<&Path>, out: Option<&Path>) -> Result<PathBuf> {
    let repo = util::repo_root();
    let source = repo.join("data");
    if !source.join("world_data").is_dir() {
        return Err(format!(
            "Runtime data tree is missing from the checkout: {}",
            source.display()
        ));
    }
    let destination = out
        .map(Path::to_path_buf)
        .unwrap_or_else(|| build_dir().join(RUST_BAKED_FILE));
    if let Some(parent) = destination.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let mut cmd = Command::new("cargo");
    cmd.args(["run", "--release", "-p", "ff-core", "--bin", "ff-bake"]);
    if let Some(dir) = target_dir {
        cmd.arg("--target-dir").arg(dir);
    }
    cmd.arg("--")
        .arg("--data-dir")
        .arg(&source)
        .arg("--out")
        .arg(&destination)
        .current_dir(&repo);
    util::run(&mut cmd)?;
    if !destination.is_file() {
        return Err(format!(
            "ff-bake wrote no container at {}",
            destination.display()
        ));
    }
    Ok(destination)
}

/// Shared libraries the crates' build scripts staged beside the binary. Top
/// level only: `deps/` holds proc-macro DLLs that are build-time artifacts.
pub fn rust_native_libraries(profile_dir: &Path, exts: &[&str]) -> Vec<PathBuf> {
    let mut libs: Vec<PathBuf> = std::fs::read_dir(profile_dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file())
        .filter(|p| {
            let ext = ext_lower(p);
            !CARGO_NON_RUNTIME_SUFFIXES.contains(&ext.as_str()) && exts.contains(&ext.as_str())
        })
        .collect();
    util::sort_paths(&mut libs);
    libs
}

/// The COMMITTED loose sound tree, relative to the checkout. Committed means
/// asked of git, not read off the disk: globbing the working tree once
/// shipped 254 MB of loose source audio a developer had sitting there.
fn rust_loose_sound_files(package_dir: &Path) -> Result<Vec<PathBuf>> {
    let tree = package_dir.join(LOOSE_SOUND_TREE);
    if !tree.is_dir() {
        return Err(format!(
            "Committed sound tree was not found: {}",
            tree.display()
        ));
    }
    let listed = util::output(
        Command::new("git")
            .args(["ls-files", "-z", "--", "."])
            .current_dir(&tree),
    )?;
    let mut tracked: Vec<PathBuf> = listed
        .split('\0')
        .filter(|n| !n.is_empty())
        .map(|n| tree.join(n))
        .collect();
    if tracked.is_empty() {
        return Err(format!(
            "git lists no committed files under {}",
            tree.display()
        ));
    }
    util::sort_paths(&mut tracked);
    Ok(tracked
        .into_iter()
        .filter(|p| p.is_file() && !p.components().any(|c| c.as_os_str() == "__pycache__"))
        .filter_map(|p| p.strip_prefix(package_dir).ok().map(Path::to_path_buf))
        .collect())
}

/// Every (source, destination relative to the staged folder) pair.
pub fn plan_rust_layout(
    profile_dir: &Path,
    package_dir: &Path,
    platform: &str,
    baked_data: &Path,
) -> Result<Vec<(PathBuf, PathBuf)>> {
    let exe = profile_dir.join(cargo_exe_name(platform));
    if !exe.is_file() {
        return Err(format!(
            "cargo build left no executable at {}",
            exe.display()
        ));
    }
    if !baked_data.is_file() {
        return Err("the baked data container is missing; bake the world before staging".into());
    }
    let mut plan = vec![(exe, PathBuf::from(rust_exe_name(platform)))];
    for lib in rust_native_libraries(profile_dir, platform_native_exts(platform)) {
        let name = PathBuf::from(lib.file_name().unwrap_or_default());
        plan.push((lib, name));
    }
    let package = PathBuf::from("freight_fate");
    plan.push((
        baked_data.to_path_buf(),
        package.join("data").join(RUST_BAKED_FILE),
    ));
    for relative in RUST_DATA_FILES {
        let source = package_dir.join("data").join(relative);
        if !source.is_file() {
            return Err(format!(
                "Runtime data file is missing from the checkout: {}",
                source.display()
            ));
        }
        plan.push((source, package.join("data").join(relative)));
    }
    for relative in rust_loose_sound_files(package_dir)? {
        plan.push((package_dir.join(&relative), package.join(&relative)));
    }
    Ok(plan)
}

/// Assemble the release folder from the plan plus the packs and docs.
fn stage_rust_build(
    profile_dir: &Path,
    build: &Path,
    baked_data: &Path,
    platform: &str,
    label: &str,
) -> Result<PathBuf> {
    let repo = util::repo_root();
    let assets = repo.join(SOURCE_ASSETS);
    for pack in ["sounds.pak", "music.pak", "channel3000.pak"] {
        require_real_pack(&assets.join(pack))?;
    }
    let plan = plan_rust_layout(profile_dir, &repo, platform, baked_data)?;
    let build = if platform == "darwin" {
        build.with_extension("app")
    } else {
        build.to_path_buf()
    };
    if build.exists() {
        std::fs::remove_dir_all(&build).map_err(|e| format!("{}: {e}", build.display()))?;
    }
    let executable_root = runtime_root(&build);
    let resource_root = if platform == "darwin" {
        build.join("Contents").join("Resources")
    } else {
        executable_root.clone()
    };
    std::fs::create_dir_all(&executable_root).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&resource_root).map_err(|e| e.to_string())?;
    let frameworks = build.join("Contents").join("Frameworks");
    if platform == "darwin" {
        std::fs::create_dir_all(&frameworks).map_err(|e| e.to_string())?;
    }
    let exe_name = PathBuf::from(rust_exe_name(platform));
    for (source, relative) in &plan {
        let destination = if platform == "darwin" && ext_lower(source) == "dylib" {
            frameworks.join(source.file_name().unwrap_or_default())
        } else if *relative == exe_name {
            executable_root.join(relative)
        } else {
            resource_root.join(relative)
        };
        util::copy_file(source, &destination)?;
    }
    let built_exe = profile_dir.join(cargo_exe_name(platform));
    if platform == "darwin" {
        // Fail fast: a dynamically linked SDL2 can only be Homebrew's
        // sdl2-compat, which needs an SDL3 the player does not have.
        if let Some(sdl) = macos::dynamic_sdl_dependency(&built_exe)? {
            return Err(format!(
                "macOS executable links SDL2 dynamically ({sdl}); Homebrew's sdl2 is now \
                 sdl2-compat, which loads SDL3 at runtime and fails on player Macs. Build \
                 with the crate's `bundled` + `static-link` SDL2 features instead."
            ));
        }
        macos::write_info_plist(&build, label)?;
        macos::relocate_libraries(&build)?;
    } else if platform == "linux" {
        // A system SDL2 is a different file on every distribution, and the
        // tarball has to start on all of them.
        if let Some(sdl) = linux_dynamic_sdl_dependency(&built_exe)? {
            return Err(format!(
                "Linux executable links SDL2 dynamically ({sdl}); the tarball has to run on \
                 every distribution, so build with the crate's `bundled` + `static-link` \
                 SDL2 features instead."
            ));
        }
    }
    if platform == "win32" {
        windows_runtime::stage_windows_runtime(&executable_root)?;
    } else {
        make_executable(&executable_root.join(rust_exe_name(platform)))?;
    }
    stage_sound_pack(&resource_root)?;
    Ok(build)
}

#[cfg(unix)]
pub fn make_executable(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mode = path
        .metadata()
        .map_err(|e| format!("{}: {e}", path.display()))?
        .permissions()
        .mode();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode | 0o755))
        .map_err(|e| format!("{}: {e}", path.display()))
}

#[cfg(not(unix))]
pub fn make_executable(_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    path.metadata()
        .is_ok_and(|m| m.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn is_executable(_path: &Path) -> bool {
    true
}

/// The sonames an ELF executable declares it needs (`readelf -d`).
fn linux_needed_libraries(executable: &Path) -> Result<Vec<String>> {
    let out = util::output(Command::new("readelf").arg("-d").arg(executable))?;
    let mut needed = Vec::new();
    for line in out.lines().filter(|l| l.contains("(NEEDED)")) {
        // `0x0000000000000001 (NEEDED)  Shared library: [libc.so.6]`
        let soname = line
            .rsplit('[')
            .next()
            .unwrap_or("")
            .trim_end_matches(']')
            .trim()
            .to_string();
        if !soname.is_empty() && !needed.contains(&soname) {
            needed.push(soname);
        }
    }
    Ok(needed)
}

/// The SDL2 soname recorded in `executable`; `None` is the only shippable
/// answer, since the `bundled` + `static-link` build compiles SDL2 in.
fn linux_dynamic_sdl_dependency(executable: &Path) -> Result<Option<String>> {
    Ok(linux_needed_libraries(executable)?
        .into_iter()
        .find(|s| s.starts_with("libSDL2")))
}

/// A player's machine is hostile territory: anything in the payload can and
/// will be read. These shapes cover every kind of credential this project
/// touches plus the common cloud formats, matched against text files.
fn secret_patterns() -> Vec<(&'static str, Regex)> {
    [
        ("private key block", r"-----BEGIN [A-Z ]*PRIVATE KEY-----"),
        ("GitHub token", r"\bgh[pousr]_[A-Za-z0-9]{20,}"),
        (
            "GitHub fine-grained token",
            r"\bgithub_pat_[A-Za-z0-9_]{20,}",
        ),
        (
            "Convex deploy key",
            r"\b(?:prod|preview|dev):[A-Za-z0-9-]+\|[A-Za-z0-9+/=_-]{20,}",
        ),
        ("Vercel blob token", r"\bvercel_blob_rw_[A-Za-z0-9_]{10,}"),
        ("AWS access key", r"\bAKIA[0-9A-Z]{16}\b"),
        ("Slack token", r"\bxox[baprs]-[A-Za-z0-9-]{10,}"),
        ("OpenAI-style secret key", r"\bsk-[A-Za-z0-9_-]{24,}"),
        ("JWT", r"\beyJ[A-Za-z0-9_-]{14,}\.eyJ[A-Za-z0-9_-]{14,}\."),
    ]
    .into_iter()
    .map(|(label, p)| (label, Regex::new(p).expect("valid regex")))
    .collect()
}

const SECRET_SCAN_SUFFIXES: &[&str] = &[
    "cfg", "css", "html", "ini", "js", "json", "md", "plist", "toml", "txt", "xml", "yaml", "yml",
];

/// Fail the build if the staged payload carries anything secret-shaped:
/// credential file names, or text matching a known token format.
pub fn verify_no_shipped_secrets(build: &Path) -> Result<()> {
    let patterns = secret_patterns();
    let mut findings = Vec::new();
    for path in util::walk_files(build) {
        let relative = rel(&path, build);
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        if name.starts_with(".env") || name.ends_with(".pem") || name.contains("deploy_key") {
            findings.push(format!("{relative}: secret-shaped file name"));
            continue;
        }
        if !SECRET_SCAN_SUFFIXES.contains(&ext_lower(&path).as_str()) {
            continue;
        }
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        let text = String::from_utf8_lossy(&bytes);
        for (label, pattern) in &patterns {
            if pattern.is_match(&text) {
                findings.push(format!("{relative}: looks like a {label}"));
            }
        }
    }
    if !findings.is_empty() {
        return Err(format!(
            "Release payload contains secret-shaped content and must not ship: {}",
            findings.join("; ")
        ));
    }
    Ok(())
}

/// Prove the staged folder holds what the binary loads.
fn verify_rust_payload(build: &Path, platform: &str) -> Result<()> {
    let executable_root = runtime_root(build);
    let root = if is_app(build) {
        build.join("Contents").join("Resources")
    } else {
        executable_root.clone()
    };
    let exe = executable_root.join(rust_exe_name(platform));
    let data_dir = root.join("freight_fate").join("data");
    let container = data_dir.join(RUST_BAKED_FILE);
    let mut required = vec![
        exe.clone(),
        root.join("build_info.json"),
        root.join("LICENSE.txt"),
        root.join("CHANGELOG.md"),
        root.join("USER_MANUAL.md"),
        root.join("USER_MANUAL.html"),
        root.join("SOUND_CREDITS.md"),
        root.join("freight_fate").join("sounds.pak"),
        root.join("freight_fate").join("music.pak"),
        root.join("freight_fate").join("channel3000.pak"),
        root.join("freight_fate")
            .join(LOOSE_SOUND_TREE)
            .join("CREDITS.md"),
        container.clone(),
    ];
    required.extend(RUST_DATA_FILES.iter().map(|r| data_dir.join(r)));
    let missing: Vec<String> = required
        .iter()
        .filter(|p| !p.exists())
        .map(|p| rel(p, build))
        .collect();
    if !missing.is_empty() {
        return Err(format!(
            "Rust payload is incomplete: {}",
            missing.join(", ")
        ));
    }
    // Not just present: the real container.
    let head = std::fs::read(&container).map_err(|e| e.to_string())?;
    if !head.starts_with(BAKED_MAGIC) {
        return Err(format!(
            "{RUST_BAKED_FILE} is not a Freight Fate baked data container"
        ));
    }
    if head.len() < 1_000_000 {
        return Err(format!(
            "{RUST_BAKED_FILE} is only {} bytes; the real container holds the whole map",
            head.len()
        ));
    }
    // And the JSON it replaced stays home: shipping both doubles the download
    // and leaves two answers to every question.
    let mut doubled: Vec<String> = RUST_BAKED_SOURCE_FILES
        .iter()
        .filter(|r| data_dir.join(r).exists() && !RUST_DATA_FILES.contains(r))
        .map(|r| r.to_string())
        .collect();
    if data_dir.join("world_data").exists() {
        doubled.push("world_data/".into());
    }
    if !doubled.is_empty() {
        doubled.sort();
        doubled.truncate(10);
        return Err(format!(
            "Rust payload ships the JSON the baked container replaced: {}",
            doubled.join(", ")
        ));
    }
    let package = root.join("freight_fate");
    let mut leaked: Vec<String> = util::walk_all(&package)
        .into_iter()
        .filter(|p| {
            let name = p
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            ["world_source", "__pycache__", LICENSED_SOUND_TREE].contains(&name.as_str())
                || ext_lower(p) == "py"
        })
        .map(|p| rel(&p, build))
        .collect();
    if !leaked.is_empty() {
        leaked.sort();
        leaked.truncate(10);
        return Err(format!(
            "Rust payload ships source-only files: {}",
            leaked.join(", ")
        ));
    }
    if platform == "win32" {
        windows_runtime::verify_windows_runtime(&executable_root)?;
        if !root.join("SDL2.dll").exists() {
            return Err("Rust payload is missing the native library SDL2.dll.".into());
        }
    }
    // macOS and Linux: SDL2, Prism and the audio engine are compiled into the
    // executable; staging refuses a dynamic SDL2 link.
    packs::verify_sound_packs(build)?;
    verify_no_shipped_secrets(build)?;
    if platform != "win32" && !is_executable(&exe) {
        return Err(format!(
            "Packaged executable is not runnable: {}",
            rel(&exe, build)
        ));
    }
    Ok(())
}

/// Boot the packaged game for a few frames with dummy drivers.
fn smoke_check(build: &Path, platform: &str) -> Result<()> {
    let exe = if is_app(build) {
        build.join("Contents").join("MacOS").join(APP_NAME)
    } else {
        build.join(rust_exe_name(platform))
    };
    let scratch = tempfile::Builder::new()
        .prefix("freight-fate-smoke-")
        .tempdir()
        .map_err(|e| e.to_string())?;
    let log = scratch.path().join("game.log");
    let mut cmd = Command::new(&exe);
    cmd.arg("--smoke");
    if is_app(build) {
        // A Mac app smokes headless; the hosted macOS job selects the
        // explicit non-launch mode instead.
        cmd.arg("--headless");
    }
    cmd.current_dir(exe.parent().unwrap_or(build))
        .env("SDL_VIDEODRIVER", "dummy")
        .env("SDL_AUDIODRIVER", "dummy")
        .env("FREIGHT_FATE_NO_SPEECH", "1")
        .env("FREIGHT_FATE_DATA_DIR", scratch.path().join("player-data"))
        .env("FREIGHT_FATE_LOG_FILE", &log);
    if let Err(e) = util::run_with_timeout(&mut cmd, Duration::from_secs(120)) {
        if let Ok(text) = std::fs::read(&log) {
            eprintln!("Packaged smoke log before failure:");
            eprintln!("{}", String::from_utf8_lossy(&text));
        }
        return Err(e);
    }
    println!("Smoke check passed: the packaged runtime boots and loads its resources.");
    Ok(())
}

/// Remove any saves or logs left in the build before archiving. The smoke
/// check boots the build, and a `saves` folder must NEVER ship: it would
/// leak the builder's profile and signing key.
fn strip_user_data(build: &Path) {
    let mut roots = vec![build.to_path_buf()];
    if is_app(build) {
        roots.push(build.join("Contents").join("MacOS"));
    }
    for root in roots {
        for name in ["saves", "logs"] {
            let leftover = root.join(name);
            if leftover.exists() {
                let _ = std::fs::remove_dir_all(&leftover);
                println!("Stripped bundled '{name}/' from the build (never ship user data).");
            }
        }
    }
}

/// The whole pipeline, ending with the verified archive.
fn build_rust(
    label: &str,
    target_dir: Option<&Path>,
    run_smoke: bool,
    macos_non_launch_verify: bool,
) -> Result<PathBuf> {
    let platform = util::platform_name();
    if run_smoke && macos_non_launch_verify {
        return Err(
            "process smoke and macOS non-launch verification are mutually exclusive".into(),
        );
    }
    if macos_non_launch_verify && platform != "darwin" {
        return Err("macOS non-launch verification requires macOS".into());
    }
    // Restore the verified music and Channel 3000 packs before Cargo runs.
    packs::ensure_music_pack(None)?;
    packs::ensure_channel3000_pack(None)?;
    util::run(&mut cargo_build_command(target_dir))?;
    let profile_dir = cargo_profile_dir(target_dir);
    let baked = bake_world_data(target_dir, None)?;
    let build = stage_rust_build(&profile_dir, &staged_dir(), &baked, platform, label)?;
    let resource_root = if is_app(&build) {
        build.join("Contents").join("Resources")
    } else {
        build.clone()
    };
    stamp_build_info(&resource_root, label)?;
    stage_release_docs(&resource_root)?;
    verify_rust_payload(&build, platform)?;
    strip_user_data(&build);
    if run_smoke {
        smoke_check(&build, platform)?;
        // Keep the last-line privacy defense even though the smoke redirects
        // its data and log paths.
        strip_user_data(&build);
    } else if macos_non_launch_verify {
        // The hosted macOS runner has repeatedly left a correctly relocated,
        // signed arm64 process before logging for the whole bound, so the
        // proof there stays non-launching: payload structure above, native
        // dependency audit and strict signature below, archive after.
        println!("Using non-launch macOS packaged verification on the hosted runner.");
    } else {
        println!("Skipped the packaged process smoke check.");
    }
    if platform == "darwin" {
        macos::sign_distribution(&build)?;
        macos::notarize_distribution(&build)?;
    }
    std::fs::create_dir_all(dist_dir()).map_err(|e| e.to_string())?;
    let out = archive::archive(&build, label, platform)?;
    verify_archive(&out)?;
    Ok(out)
}

const USAGE: &str = "usage: build-release [--tag LABEL] [--smoke] [--skip-smoke] \
[--macos-non-launch-verify] [--cargo-target-dir DIR] [--check-dependencies]";

pub fn main(argv: &[String]) -> Result<i32> {
    let args = Args::parse(
        argv,
        &Spec {
            values: &["tag", "cargo-target-dir"],
            flags: &[
                "skip-smoke",
                "check-dependencies",
                "rust",
                "smoke",
                "macos-non-launch-verify",
            ],
        },
    )
    .map_err(|e| format!("{e}\n{USAGE}"))?;
    let non_launch = args.flag("macos-non-launch-verify");
    if non_launch && args.flag("smoke") {
        return Err("--macos-non-launch-verify cannot be combined with --smoke".into());
    }
    if non_launch && args.flag("skip-smoke") {
        return Err("--macos-non-launch-verify cannot be combined with --skip-smoke".into());
    }
    if non_launch && util::platform_name() != "darwin" {
        return Err("--macos-non-launch-verify requires macOS".into());
    }
    if args.flag("check-dependencies") {
        println!("Release dependency check passed: the Rust build fetches its own.");
        return Ok(0);
    }
    let label = match args.value("tag").filter(|t| !t.is_empty()) {
        Some(tag) => tag.to_string(),
        None => project_version()?,
    };
    let target_dir = args
        .path("cargo-target-dir")
        .filter(|p| !p.as_os_str().is_empty())
        .map(|p| std::path::absolute(&p).unwrap_or(p));
    let out = build_rust(
        &label,
        target_dir.as_deref(),
        args.flag("smoke") && !args.flag("skip-smoke"),
        non_launch,
    )?;
    let size = out.metadata().map(|m| m.len()).unwrap_or(0) as f64 / 1e6;
    println!("Built {} ({size:.1} MB)", out.display());
    Ok(0)
}

/// `windows-imports [PROFILE_DIR]`: audit the Windows executable a Cargo
/// build left, as the release does.
///
#[cfg(test)]
mod tests;
