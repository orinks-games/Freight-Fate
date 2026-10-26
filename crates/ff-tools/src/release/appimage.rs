//! `build-appimage`: package the staged Linux build as a cross-distro
//! AppImage.
//!
//! Runs after `build-release` and expects its staged folder,
//! `build/FreightFate`. The Rust build needs almost no bundled libraries
//! (SDL2, Prism and the audio engine are compiled into the executable), so
//! the AppImage is mostly the launcher, the desktop entry and the
//! self-update path the tarball cannot offer. linuxdeploy still runs over
//! it, with the host-integration stacks below excluded so the game keeps
//! using the target system's GLib, D-Bus, AT-SPI/speech stack and OpenSSL.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use crate::cli::{Args, Spec};
use crate::http;
use crate::util::{self, Result};

/// Both tools are published per architecture under the same release, named
/// by `uname -m`: x86_64 for the PC build, aarch64 for the ARM64 one (the
/// Blazie BT Speak and BT Braille, Raspberry Pi). The AppImage is named the
/// same way, which is the AppImage convention; the tarball's `arm64` label
/// is `build-release`'s.
const LINUXDEPLOY_URL: &str = "https://github.com/linuxdeploy/linuxdeploy/releases/download/\
1-alpha-20251107-1/linuxdeploy-{arch}.AppImage";
const APPIMAGE_RUNTIME_URL: &str =
    "https://github.com/AppImage/type2-runtime/releases/download/20251108/runtime-{arch}";

/// Libraries that must come from the target system, not the AppImage.
const EXCLUDED_LIBRARY_GLOBS: &[&str] = &[
    // GLib/GIO: host gio modules (GVFS, etc.) load into whichever GLib is in
    // the process; shipping our own triggers duplicate-GType registration
    // crashes ("cannot register existing type 'GTask'").
    "libglib-2.0*",
    "libgio-2.0*",
    "libgobject-2.0*",
    "libgmodule-2.0*",
    "libgthread-2.0*",
    // GTK/desktop + accessibility stack: must be the host's so AT-SPI
    // (screen readers), themes, and input methods work.
    "libgtk-3*",
    "libgdk-3*",
    "libgdk_pixbuf-2.0*",
    "libpango*",
    "libcairo*",
    "libpixman*",
    "libatk*",
    "libatspi*",
    "libnotify*",
    "libthai*",
    "libdatrie*",
    "libfribidi*",
    "libgraphite2*",
    "libharfbuzz*",
    // Host system plumbing tied to the running OS.
    "libdbus-1*",
    "libsystemd*",
    "libselinux*",
    "libcap*",
    "libmount*",
    "libblkid*",
    "liblz4*",
    "libgcrypt*",
    "libgpg-error*",
    // TLS/HTTP: use the distro's security-patched OpenSSL/curl stack.
    "libssl*",
    "libcrypto*",
    "libcurl*",
    "libgnutls*",
    "libnettle*",
    "libhogweed*",
    "libtasn1*",
    "libp11-kit*",
    "libidn2*",
    "libunistring*",
    "libpsl*",
    "libnghttp2*",
    "libssh*",
    "librtmp*",
    "liblber*",
    "libldap*",
    "libsasl2*",
    "libgssapi_krb5*",
    "libkrb5*",
    "libk5crypto*",
    "libkeyutils*",
];

/// Sonames that must never appear in usr/lib (the subset of the globs above
/// that is a hard failure, as the archive's OpenSSL check is).
const FORBIDDEN_BUNDLED_PREFIXES: &[&str] = &[
    "libssl",
    "libcrypto",
    "libglib-2.0",
    "libgio-2.0",
    "libgobject-2.0",
    "libgtk-3",
    "libgdk-3",
    "libatk",
    "libatspi",
];

fn work_dir() -> PathBuf {
    util::repo_root().join("build").join("appimage")
}

fn appdir() -> PathBuf {
    work_dir().join("AppDir")
}

fn assets_dir() -> PathBuf {
    util::repo_root().join("tools").join("appimage")
}

/// The `uname -m` name the AppImage tools and the output file use.
pub fn appimage_architecture(machine: &str) -> Result<&'static str> {
    match machine.to_lowercase().as_str() {
        "x86_64" | "amd64" => Ok("x86_64"),
        "aarch64" | "arm64" => Ok("aarch64"),
        other => Err(format!("No AppImage tooling pinned for machine {other:?}.")),
    }
}

/// Download `url` to `target` with simple retries; keep an existing file.
fn download(url: &str, target: &Path) -> Result<PathBuf> {
    if target.metadata().is_ok_and(|m| m.len() > 0) {
        return Ok(target.to_path_buf());
    }
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let agent = http::agent(Some(Duration::from_secs(600)));
    let attempts = 4;
    for attempt in 1..=attempts {
        println!("Downloading {url} (attempt {attempt})");
        let outcome = std::fs::File::create(target)
            .map_err(|e| e.to_string())
            .and_then(|mut file| http::download(&agent, url, &[], &mut file))
            .and_then(|status| match status {
                200..=299 if target.metadata().is_ok_and(|m| m.len() > 0) => Ok(()),
                200..=299 => Err("downloaded file is empty".into()),
                other => Err(format!("HTTP status {other}")),
            });
        match outcome {
            Ok(()) => return Ok(target.to_path_buf()),
            Err(e) if attempt == attempts => {
                let _ = std::fs::remove_file(target);
                return Err(format!("Failed to download {url}: {e}"));
            }
            Err(_) => std::thread::sleep(Duration::from_secs(5 * attempt)),
        }
    }
    unreachable!("the last attempt returns")
}

/// Copy a folder tree, keeping file permissions (the executable bit).
fn copy_tree(from: &Path, to: &Path) -> Result<()> {
    for path in util::walk_all(from) {
        let dest = to.join(path.strip_prefix(from).unwrap_or(&path));
        if path.is_dir() {
            std::fs::create_dir_all(&dest).map_err(|e| e.to_string())?;
        } else {
            util::copy_file(&path, &dest)?;
        }
    }
    Ok(())
}

fn assemble_appdir(staged: &Path) -> Result<()> {
    let appdir = appdir();
    if appdir.exists() {
        std::fs::remove_dir_all(&appdir).map_err(|e| e.to_string())?;
    }
    println!("Copying {} into AppDir", staged.display());
    copy_tree(staged, &appdir.join("opt").join("freightfate"))
}

fn run_linuxdeploy(linuxdeploy: &Path, runtime: &Path, label: &str) -> Result<PathBuf> {
    let work = work_dir();
    let appdir = appdir();
    let assets = assets_dir();
    util::copy_file(
        &assets.join("freightfate.png"),
        &work.join("freightfate.png"),
    )?;
    let mut cmd = Command::new(linuxdeploy);
    cmd.arg("--appimage-extract-and-run") // runner images ship without FUSE
        .arg(format!("--appdir={}", appdir.display()))
        .arg(format!(
            "--deploy-deps-only={}",
            appdir.join("opt").join("freightfate").display()
        ))
        .arg(format!(
            "--desktop-file={}",
            assets.join("freightfate.desktop").display()
        ))
        .arg(format!(
            "--icon-file={}",
            work.join("freightfate.png").display()
        ))
        .arg(format!(
            "--custom-apprun={}",
            assets.join("AppRun").display()
        ))
        .arg("--output=appimage");
    for glob in EXCLUDED_LIBRARY_GLOBS {
        cmd.arg(format!("--exclude-library={glob}"));
    }
    cmd.current_dir(&work)
        .env("LINUXDEPLOY_OUTPUT_VERSION", label)
        .env("LDAI_RUNTIME_FILE", runtime);
    println!("Running: {}", util::show_command(&cmd));
    util::run(&mut cmd)?;

    let tool_name = linuxdeploy.file_name().unwrap_or_default();
    let mut produced: Vec<(std::time::SystemTime, PathBuf)> = util::glob_suffix(&work, ".AppImage")
        .into_iter()
        .filter(|p| p.file_name() != Some(tool_name))
        .filter_map(|p| Some((p.metadata().ok()?.modified().ok()?, p)))
        .collect();
    produced.sort();
    produced
        .pop()
        .map(|(_, p)| p)
        .ok_or_else(|| "linuxdeploy did not produce an AppImage".into())
}

/// The bundled sonames a host-integration rule forbids.
pub fn forbidden_libraries(names: &[String]) -> Vec<String> {
    let mut bad: Vec<String> = names
        .iter()
        .filter(|n| FORBIDDEN_BUNDLED_PREFIXES.iter().any(|p| n.starts_with(p)))
        .cloned()
        .collect();
    bad.sort();
    bad
}

fn verify_bundled_libraries() -> Result<()> {
    let lib_dir = appdir().join("usr").join("lib");
    let names: Vec<String> = std::fs::read_dir(&lib_dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    let forbidden = forbidden_libraries(&names);
    if !forbidden.is_empty() {
        return Err(format!(
            "AppImage must not bundle host-integration libraries (GLib/GTK/OpenSSL stay on \
             the target system): {forbidden:?}"
        ));
    }
    println!("Verified {} bundled libraries in usr/lib", names.len());
    Ok(())
}

pub fn main(argv: &[String]) -> Result<i32> {
    let args = Args::parse(
        argv,
        &Spec {
            values: &["tag"],
            flags: &["keep-workdir", "rust"],
        },
    )
    .map_err(|e| format!("{e}\nusage: build-appimage [--tag LABEL] [--keep-workdir]"))?;
    if util::platform_name() != "linux" {
        println!("AppImage packaging only runs on Linux.");
        return Ok(1);
    }
    let staged = super::staged_dir();
    if !staged.join(super::APP_NAME).exists() {
        println!(
            "Staged build not found at {}; run `cargo tools build-release` first.",
            staged.display()
        );
        return Ok(1);
    }
    let label = match args.value("tag").filter(|t| !t.is_empty()) {
        Some(tag) => tag.to_string(),
        None => super::project_version()?,
    };
    println!("Freight Fate AppImage build, label {label}");

    let work = work_dir();
    if work.exists() && !args.flag("keep-workdir") {
        std::fs::remove_dir_all(&work).map_err(|e| e.to_string())?;
    }
    std::fs::create_dir_all(&work).map_err(|e| e.to_string())?;

    let arch = appimage_architecture(&util::machine())?;
    let tools = work.join("tools");
    let linuxdeploy = download(
        &LINUXDEPLOY_URL.replace("{arch}", arch),
        &tools.join(format!("linuxdeploy-{arch}.AppImage")),
    )?;
    super::make_executable(&linuxdeploy)?;
    let runtime = download(
        &APPIMAGE_RUNTIME_URL.replace("{arch}", arch),
        &tools.join(format!("runtime-{arch}")),
    )?;

    assemble_appdir(&staged)?;
    let produced = run_linuxdeploy(&linuxdeploy, &runtime, &label)?;
    verify_bundled_libraries()?;

    let dist = super::dist_dir();
    std::fs::create_dir_all(&dist).map_err(|e| e.to_string())?;
    let target = dist.join(format!("{}-{label}-linux-{arch}.AppImage", super::APP_NAME));
    if target.exists() {
        std::fs::remove_file(&target).map_err(|e| e.to_string())?;
    }
    if std::fs::rename(&produced, &target).is_err() {
        util::copy_file(&produced, &target)?;
        std::fs::remove_file(&produced).map_err(|e| e.to_string())?;
    }
    super::make_executable(&target)?;
    println!("Created {}", target.display());
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn architectures_use_uname_names() {
        assert_eq!(appimage_architecture("AMD64").unwrap(), "x86_64");
        assert_eq!(appimage_architecture("arm64").unwrap(), "aarch64");
        assert!(appimage_architecture("riscv64").is_err());
    }

    #[test]
    fn host_stacks_are_refused() {
        let names: Vec<String> = ["libssl.so.3", "libz.so.1", "libatk-1.0.so.0"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(
            forbidden_libraries(&names),
            ["libatk-1.0.so.0", "libssl.so.3"]
        );
    }

    #[test]
    fn appimage_assets_are_present() {
        for name in ["AppRun", "freightfate.desktop", "freightfate.png"] {
            assert!(assets_dir().join(name).is_file(), "{name}");
        }
    }
}
