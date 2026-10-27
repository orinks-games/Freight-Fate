//! The archive players download, and the check that re-opens it.

use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

use chrono::{Datelike, Timelike};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, DateTime, ZipArchive, ZipWriter};

use super::{dist_dir, APP_NAME, RUST_BAKED_FILE_ENTRY, UPDATE_SKIPPED_PACKS};
use crate::util::{self, Result};
use crate::windows_runtime::REQUIRED_CRT;

const LINUX_TARBALL_SUFFIXES: &[&str] = &["-linux-x64.tar.gz", "-linux-arm64.tar.gz"];

/// The `linux-<arch>` label a Linux tarball is named with: `x64` for the PC
/// build and `arm64` for the aarch64 one (the Blazie notetakers, Raspberry
/// Pi), matching the Mac archive's `arm64` rather than the AppImage's
/// `aarch64`, because the in-game updater picks by this suffix.
pub fn linux_archive_suffix(machine: &str) -> Result<&'static str> {
    match machine.to_lowercase().as_str() {
        "x86_64" | "amd64" => Ok("linux-x64"),
        "aarch64" | "arm64" => Ok("linux-arm64"),
        other => Err(format!(
            "No Linux release archive name for machine {other:?}."
        )),
    }
}

/// A Career 1.9 tester snapshot or a stable tag from v1.9.0 on. Their Apple
/// Silicon archive is named `-macos-arm64`; a 1.8 stable keeps `-macos`.
pub fn is_career_19_label(label: &str) -> bool {
    if label.starts_with("1.9-tester-") {
        return true;
    }
    let re = regex::Regex::new(r"^v?(\d+)\.(\d+)\.").expect("valid regex");
    re.captures(label).is_some_and(|c| {
        let major: u64 = c[1].parse().unwrap_or(0);
        let minor: u64 = c[2].parse().unwrap_or(0);
        (major, minor) >= (1, 9)
    })
}

fn zip_time(path: &Path) -> DateTime {
    let local = path
        .metadata()
        .and_then(|m| m.modified())
        .map(chrono::DateTime::<chrono::Local>::from)
        .ok();
    local
        .and_then(|t| {
            DateTime::from_date_and_time(
                t.year().clamp(1980, 2107) as u16,
                t.month() as u8,
                t.day() as u8,
                t.hour() as u8,
                t.minute() as u8,
                t.second() as u8,
            )
            .ok()
        })
        .unwrap_or_default()
}

fn zip_tree(build: &Path, out: &Path) -> Result<()> {
    let file = std::fs::File::create(out).map_err(|e| format!("{}: {e}", out.display()))?;
    let mut zip = ZipWriter::new(file);
    for path in util::walk_all(build) {
        let name = format!("{APP_NAME}/{}", util::rel(&path, build));
        let options = SimpleFileOptions::default()
            .compression_method(CompressionMethod::Deflated)
            .last_modified_time(zip_time(&path))
            .large_file(path.metadata().is_ok_and(|m| m.len() >= 0xFFFF_FFFF));
        if path.is_dir() {
            zip.add_directory(format!("{name}/"), options)
                .map_err(|e| format!("zip: {e}"))?;
        } else {
            zip.start_file(name, options)
                .map_err(|e| format!("zip: {e}"))?;
            let mut source =
                std::fs::File::open(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            std::io::copy(&mut source, &mut zip).map_err(|e| format!("zip: {e}"))?;
        }
    }
    zip.finish()
        .map_err(|e| format!("zip: {e}"))?
        .flush()
        .map_err(|e| e.to_string())
}

fn tar_tree(build: &Path, out: &Path) -> Result<()> {
    let file = std::fs::File::create(out).map_err(|e| format!("{}: {e}", out.display()))?;
    let encoder = flate2::write::GzEncoder::new(file, flate2::Compression::default());
    let mut tar = tar::Builder::new(encoder);
    tar.follow_symlinks(false);
    tar.append_dir_all(APP_NAME, build)
        .map_err(|e| format!("tar: {e}"))?;
    tar.into_inner()
        .and_then(|gz| gz.finish())
        .map_err(|e| format!("tar: {e}"))?;
    Ok(())
}

/// Archive the staged build under this platform's release name.
pub fn archive(build: &Path, label: &str, platform: &str) -> Result<PathBuf> {
    let dist = dist_dir();
    let out = match platform {
        "win32" => {
            let out = dist.join(format!("{APP_NAME}-{label}-windows-portable.zip"));
            zip_tree(build, &out)?;
            out
        }
        "darwin" => {
            let apple_silicon = matches!(util::machine().as_str(), "arm64" | "aarch64");
            let suffix = if is_career_19_label(label) && apple_silicon {
                "macos-arm64"
            } else {
                "macos"
            };
            let out = dist.join(format!("{APP_NAME}-{label}-{suffix}.zip"));
            util::run(
                Command::new("ditto")
                    .args(["-c", "-k", "--keepParent"])
                    .arg(build)
                    .arg(&out),
            )?;
            out
        }
        _ => {
            let out = dist.join(format!(
                "{APP_NAME}-{label}-{}.tar.gz",
                linux_archive_suffix(&util::machine())?
            ));
            tar_tree(build, &out)?;
            out
        }
    };
    Ok(out)
}

/// Map an archive's file entries to (size, permission mode).
fn archive_entries(out: &Path) -> Result<HashMap<String, (u64, u32)>> {
    let mut entries = HashMap::new();
    let file = std::fs::File::open(out).map_err(|e| format!("{}: {e}", out.display()))?;
    if out.to_string_lossy().ends_with(".tar.gz") {
        let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(file));
        for entry in tar.entries().map_err(|e| format!("tar: {e}"))? {
            let entry = entry.map_err(|e| format!("tar: {e}"))?;
            let header = entry.header();
            if header.entry_type().is_file() {
                let name = entry
                    .path()
                    .map_err(|e| format!("tar: {e}"))?
                    .to_string_lossy()
                    .replace('\\', "/");
                entries.insert(
                    name,
                    (
                        header.size().unwrap_or(0),
                        header.mode().unwrap_or(0) & 0o7777,
                    ),
                );
            }
        }
    } else {
        let mut zip = ZipArchive::new(file).map_err(|e| format!("zip: {e}"))?;
        for i in 0..zip.len() {
            let info = zip.by_index_raw(i).map_err(|e| format!("zip: {e}"))?;
            if info.is_dir() {
                continue;
            }
            let name = info.name().map_err(|e| format!("zip: {e}"))?.into_owned();
            entries.insert(name, (info.size(), info.unix_mode().unwrap_or(0) & 0o7777));
        }
    }
    Ok(entries)
}

/// Re-open the finished archive and prove the payload survived archiving.
/// A bad walk, an archiver quirk or a dropped permission bit here would ship
/// a download with no runnable game inside, and nothing else would notice.
pub fn verify_archive(out: &Path) -> Result<()> {
    let name = out
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    let macos = name.ends_with("-macos-arm64.zip") || name.ends_with("-macos.zip");
    let linux = LINUX_TARBALL_SUFFIXES.iter().any(|s| name.ends_with(s));
    let (root, exe_entry, needs_exec) = if macos {
        let root = format!("{APP_NAME}.app/Contents/MacOS");
        let exe = format!("{root}/{APP_NAME}");
        (root, exe, true)
    } else if linux {
        (APP_NAME.to_string(), format!("{APP_NAME}/{APP_NAME}"), true)
    } else if name.ends_with("-windows-portable.zip") {
        (
            APP_NAME.to_string(),
            format!("{APP_NAME}/{APP_NAME}.exe"),
            false,
        )
    } else {
        return Err(format!("Unrecognized release archive name: {name}"));
    };
    let entries = archive_entries(out)?;
    let Some(&(size, mode)) = entries.get(&exe_entry) else {
        return Err(format!(
            "Release archive is missing the executable {exe_entry}: {name}"
        ));
    };
    if size == 0 {
        return Err(format!(
            "Release archive executable is empty: {exe_entry} in {name}"
        ));
    }
    if needs_exec && mode & 0o111 == 0 {
        return Err(format!(
            "Release archive executable lost its executable permission: {exe_entry} in {name}"
        ));
    }
    // A Mac bundle keeps its payload under Resources; the others beside the exe.
    let payload_root = if macos {
        format!("{APP_NAME}.app/Contents/Resources")
    } else {
        root.clone()
    };
    let mut required = vec![
        "build_info.json",
        "LICENSE.txt",
        "USER_MANUAL.md",
        "freight_fate/sounds.pak",
    ];
    required.extend(UPDATE_SKIPPED_PACKS);
    required.push(RUST_BAKED_FILE_ENTRY);
    let mut missing: Vec<String> = required
        .iter()
        .map(|e| format!("{payload_root}/{e}"))
        .filter(|e| !entries.contains_key(e))
        .collect();
    if macos {
        let plist = format!("{APP_NAME}.app/Contents/Info.plist");
        if !entries.contains_key(&plist) {
            missing.push(plist);
        }
    } else if !linux {
        // The delivered ZIP, not only the build runner, must carry the CRT.
        let folded: HashMap<String, u64> = entries
            .iter()
            .map(|(k, (size, _))| (k.to_lowercase(), *size))
            .collect();
        for crt in REQUIRED_CRT {
            let entry = format!("{root}/{crt}");
            if folded.get(&entry.to_lowercase()).copied().unwrap_or(0) == 0 {
                missing.push(entry);
            }
        }
    }
    if !missing.is_empty() {
        return Err(format!(
            "Release archive is missing payload files: {} in {name}",
            missing.join(", ")
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn staged(dir: &Path) -> PathBuf {
        let build = dir.join("FreightFate");
        let files = [
            "FreightFate",
            "build_info.json",
            "LICENSE.txt",
            "USER_MANUAL.md",
            "freight_fate/sounds.pak",
            "freight_fate/music.pak",
            "freight_fate/channel3000.pak",
            "freight_fate/data/world.ffdata",
        ];
        for f in files {
            let p = build.join(f);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(&p, "x").unwrap();
        }
        build
    }

    #[cfg(unix)]
    #[test]
    fn a_tarball_keeps_its_executable_bit() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let build = staged(dir.path());
        let exe = build.join("FreightFate");
        let out = dir.path().join("FreightFate-t-linux-x64.tar.gz");
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o644)).unwrap();
        tar_tree(&build, &out).unwrap();
        assert!(verify_archive(&out)
            .unwrap_err()
            .contains("executable permission"));
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
        tar_tree(&build, &out).unwrap();
        verify_archive(&out).unwrap();
    }

    #[test]
    fn a_windows_zip_needs_the_crt() {
        let dir = tempfile::tempdir().unwrap();
        let build = staged(dir.path());
        std::fs::rename(build.join("FreightFate"), build.join("FreightFate.exe")).unwrap();
        let out = dir.path().join("FreightFate-t-windows-portable.zip");
        zip_tree(&build, &out).unwrap();
        let err = verify_archive(&out).unwrap_err();
        assert!(err.contains("FreightFate/vcruntime140.dll"), "{err}");
        for crt in REQUIRED_CRT {
            std::fs::write(build.join(crt.to_uppercase()), "dll").unwrap();
        }
        zip_tree(&build, &out).unwrap();
        verify_archive(&out).unwrap();
    }

    #[test]
    fn labels_and_suffixes() {
        assert!(is_career_19_label("1.9-tester-20261001"));
        assert!(is_career_19_label("v1.9.4"));
        assert!(is_career_19_label("2.0.0"));
        assert!(!is_career_19_label("v1.8.8"));
        assert_eq!(linux_archive_suffix("AMD64").unwrap(), "linux-x64");
        assert_eq!(linux_archive_suffix("aarch64").unwrap(), "linux-arm64");
        assert!(linux_archive_suffix("riscv64").is_err());
    }
}
